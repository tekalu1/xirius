//! Plugins whose host process went down (PLG-053, PLG-054), and the process each plugin runs in
//! (PLG-022).
//!
//! The studio asks the host how its plugins are a few times a second. A plugin that stopped
//! answering shows a bolt where its icon was: on its insert row in the mixer and at the head of
//! its track. The bolt at the head starts the process again; the row opens the plugin's menu
//! with 再起動 and 外す only. A plugin that took the shared process down is offered a process
//! of its own, and the menu sets where any plugin runs. Only offered: the song decides.

use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::PluginIsolation;

use crate::mixer::MixerHit;
use crate::popup::PopupKind;
use crate::studio::{Geometry, Rect, Studio};

/// How often the host is asked.
pub const POLL: Duration = Duration::from_millis(250);

/// How a plugin in a host process is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub id: PluginId,
    /// Its process went down; it plays dry until a restart.
    pub crashed: bool,
    /// It took the shared process down: a process of its own is offered.
    pub suggest: bool,
}

#[derive(Default)]
pub struct Health {
    pub reports: Vec<Report>,
    /// Reports in place of the host's, for tests and screenshots. A restart brings their
    /// plugins back.
    pub fake: Option<Vec<Report>>,
    /// Restarts asked for so far.
    pub restarts: usize,
    /// A plugin went down since [`Studio::take_went_down`] last asked.
    pub(crate) went_down: bool,
    polled: Option<Instant>,
}

impl Health {
    fn report(&self, id: PluginId) -> Option<&Report> {
        self.reports.iter().find(|r| r.id == id)
    }

    pub fn crashed(&self, id: PluginId) -> bool {
        self.report(id).is_some_and(|r| r.crashed)
    }

    pub fn suggests(&self, id: PluginId) -> bool {
        self.report(id).is_some_and(|r| r.suggest)
    }
}

/// What the plugin menu shows.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginView {
    pub name: String,
    pub crashed: bool,
    pub suggest: bool,
    /// Where the song runs it.
    pub mode: PluginIsolation,
    /// A built-in plugin runs in the engine only.
    pub builtin: bool,
    /// An ARA plugin runs in this process only (PLG-025).
    pub ara: bool,
}

/// A press in the plugin menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlugAct {
    Open,
    Restart,
    Remove,
    Mode(PluginIsolation),
}

impl Studio {
    /// Asks the host how its plugins are, at most every [`POLL`]. True when that changed.
    pub fn poll_plugins(&mut self) -> bool {
        if self.health.polled.is_some_and(|t| t.elapsed() < POLL) {
            return false;
        }
        self.health.polled = Some(Instant::now());
        self.read_plugins()
    }

    /// Asks the host now.
    pub fn poll_plugins_now(&mut self) -> bool {
        self.health.polled = None;
        self.poll_plugins()
    }

    fn read_plugins(&mut self) -> bool {
        let next = match &self.health.fake {
            Some(fake) => fake.clone(),
            None => self
                .transport
                .host_mut()
                .map(|h| {
                    h.plugins()
                        .reports()
                        .into_iter()
                        .map(|r| Report {
                            id: r.id,
                            crashed: r.status.crashed,
                            suggest: r.suggest().is_some(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };
        if next == self.health.reports {
            return false;
        }
        let down = |r: &[Report]| r.iter().filter(|r| r.crashed).count();
        self.health.went_down |= down(&next) > down(&self.health.reports);
        self.health.reports = next;
        if matches!(
            self.popup.as_ref().map(|p| p.kind),
            Some(PopupKind::Plugin(..) | PopupKind::CrashedPlugin(..))
        ) {
            self.refresh_popup();
        }
        true
    }

    /// The track's first plugin that went down.
    pub fn crashed_on(&self, t: TrackId) -> Option<PluginId> {
        let p = self.project();
        let track = p.tracks.get(&t)?;
        track
            .inserts
            .iter()
            .find(|x| self.health.crashed(x.id))
            .map(|x| x.id)
    }

    /// Starts the plugins' processes again, as `plugin.restart` does; each plugin comes back
    /// with the values it had (PLG-054).
    pub fn restart_plugins(&mut self) -> bool {
        self.restart(None)
    }

    /// Starts the process one plugin runs in again, and no other.
    pub fn restart_plugin(&mut self, id: PluginId) -> bool {
        self.restart(Some(id))
    }

    fn restart(&mut self, only: Option<PluginId>) -> bool {
        self.health.restarts += 1;
        // The ARA plugins that stopped start again with the rest (PLG-055).
        if only.is_none() {
            self.ara_restarted();
        }
        if let Some(fake) = &mut self.health.fake {
            for r in fake.iter_mut().filter(|r| only.is_none_or(|id| r.id == id)) {
                r.crashed = false;
            }
        } else if let Some(host) = self.transport.host_mut()
            && let Err(e) = match only {
                Some(id) => host.restart_plugin_later(id),
                None => host.restart_plugins_later(),
            }
        {
            self.error = Some(e);
        }
        self.read_plugins();
        true
    }

    /// A right press: on a plugin's insert row, its menu.
    pub fn context_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(MixerHit::Open(t, id) | MixerHit::Bypass(t, id)) = self.mixer_hit(g, x, y) else {
            return false;
        };
        let Some(anchor) = self.mixer_rect(g, MixerHit::Open(t, id)) else {
            return false;
        };
        // The instrument row has a menu of its own (CTX-040).
        if crate::instrument::is_instrument_of(&self.project(), t, id) {
            self.open_inst_menu(crate::instrument::menu::Menu::Row(t, id), anchor);
            return true;
        }
        self.open_plugin_menu(t, id, anchor);
        true
    }

    /// Opens the plugin menu under `anchor`.
    pub(crate) fn open_plugin_menu(&mut self, t: TrackId, id: PluginId, anchor: Rect) {
        use crate::context::Item;
        use crate::popup::Action;
        let Some(view) = self.plugin_view(t, id) else {
            return;
        };
        if !view.crashed {
            let mut items = self.plugin_items(t, id);
            // This entry point previously included process choices even for built-ins.
            // Preserve the original action (including its non-window undo behavior).
            let modes = [
                PluginIsolation::InProcess,
                PluginIsolation::Shared,
                PluginIsolation::Dedicated,
            ];
            let mut choices = self.plugin_process_items(t, id);
            for (item, mode) in choices.iter_mut().zip(modes) {
                item.run = crate::context::Run::Legacy(
                    PopupKind::Plugin(t, id),
                    anchor,
                    Box::new(Action::Plugin(PlugAct::Mode(mode))),
                );
            }
            if let Some(item) = items.iter_mut().find(|item| item.label == "動かす場所") {
                item.run = crate::context::Run::Sub(choices);
            } else {
                items.push(Item::sub("動かす場所", choices));
            }
            self.open_classified("insert", items, anchor.x, anchor.bottom());
            return;
        }
        let kind = PopupKind::Plugin(t, id);
        let mut items = vec![
            Item::legacy("再起動", kind, anchor, Action::Plugin(PlugAct::Restart)),
            Item::legacy("外す", kind, anchor, Action::Plugin(PlugAct::Remove)),
        ];
        if view.suggest {
            items.push(Item::legacy(
                "専用で動かす",
                kind,
                anchor,
                Action::Plugin(PlugAct::Mode(PluginIsolation::Dedicated)),
            ));
        }
        items.push(Item::sub("動かす場所", self.plugin_process_items(t, id)));
        self.open_classified("other-60", items, anchor.x, anchor.bottom());
    }

    pub(crate) fn plugin_view(&mut self, t: TrackId, id: PluginId) -> Option<PluginView> {
        let p = self.project();
        let x = p.tracks.get(&t)?.inserts.iter().find(|x| x.id == id)?;
        let builtin = x.format.eq_ignore_ascii_case("builtin");
        // The player keeps an ARA plugin here whatever the song says; the menu shows where it runs.
        let catalog = self.plugin_catalog();
        let ara =
            !builtin && xirius_session::plugin_policy::find(&catalog, x).is_some_and(|e| e.ara);
        let mode = self
            .song
            .settings()
            // design-lint: allow(ui-blocking-io) PRJ-031: memory-only settings snapshot; no file access on the window thread.
            .lock()
            .ok()
            .map(|s| {
                xirius_prefs::isolation::resolve(
                    &s.prefs,
                    &x.format,
                    &x.uid,
                    xirius_session::plugin_policy::legacy(&s.prefs, Some(&catalog), x),
                )
            })
            .unwrap_or(x.isolation);
        Some(PluginView {
            name: x.name.clone(),
            crashed: self.health.crashed(id),
            suggest: self.health.suggests(id)
                && x.isolation != PluginIsolation::Dedicated
                && !builtin
                && !ara,
            mode: if ara || builtin {
                PluginIsolation::InProcess
            } else {
                mode
            },
            builtin,
            ara,
        })
    }

    /// The normal context menu shares the same three effective process choices.
    pub(crate) fn plugin_process_items(
        &mut self,
        t: TrackId,
        id: PluginId,
    ) -> Vec<crate::context::Item> {
        let Some(view) = self.plugin_view(t, id) else {
            return vec![];
        };
        let p = self.project();
        let Some(plugin) = p
            .tracks
            .get(&t)
            .and_then(|t| t.inserts.iter().find(|x| x.id == id))
        else {
            return vec![];
        };
        let key = xirius_prefs::isolation::key(&plugin.format, &plugin.uid);
        [
            ("アプリ内", PluginIsolation::InProcess),
            ("共有", PluginIsolation::Shared),
            ("専用", PluginIsolation::Dedicated),
        ]
        .into_iter()
        .map(|(label, mode)| {
            let value = if mode == PluginIsolation::InProcess {
                "same"
            } else {
                mode.name()
            };
            crate::context::Item {
                run: crate::context::Run::Cmd("prefs.set", json!({"key":key,"value":value})),
                ..crate::context::Item::title(label)
            }
            .enabled(!(view.builtin || view.ara) || mode == PluginIsolation::InProcess)
            .on(view.mode == mode)
        })
        .collect()
    }

    /// A press in the plugin menu.
    pub(crate) fn plugin_action(&mut self, act: PlugAct) -> bool {
        let Some(PopupKind::Plugin(t, id) | PopupKind::CrashedPlugin(t, id)) =
            self.popup.as_ref().map(|p| p.kind)
        else {
            return false;
        };
        match act {
            PlugAct::Open => {
                self.popup = None;
                self.open_plugin(t, id)
            }
            PlugAct::Restart => {
                self.popup = None;
                self.restart_plugin(id)
            }
            PlugAct::Remove => {
                self.popup = None;
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command("plugin.remove", json!({ "ids": [id.to_string()] }))
            }
            PlugAct::Mode(mode) => {
                let key = self
                    .project()
                    .tracks
                    .get(&t)
                    .and_then(|t| t.inserts.iter().find(|x| x.id == id))
                    .map(|x| xirius_prefs::isolation::key(&x.format, &x.uid));
                let Some(key) = key else {
                    return false;
                };
                let value = if mode == PluginIsolation::InProcess {
                    "same"
                } else {
                    mode.name()
                };
                if self.transport.is_windowed() {
                    // design-lint: allow(ui-blocking-io) PRJ-031: settings persistence uses the worker; completion rebuilds with the new policy.
                    self.run_command("prefs.set", json!({"key":key,"value":value}));
                } else {
                    // design-lint: allow(ui-blocking-io) PRJ-031: non-window callers and tests retain the model edit's undo step.
                    self.run_command(
                        "plugin.isolation.set",
                        json!({"ids":[id.to_string()],"mode":mode.name()}),
                    );
                    // design-lint: allow(ui-blocking-io) PRJ-031: this branch has no interactive window; tests use memory-only settings.
                    if let Err(error) = self.song.set("prefs.set", json!({"key":key,"value":value}))
                    {
                        self.fail(error);
                    }
                }
                self.refresh_popup();
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::model::TrackKind;

    use crate::popup::{Action, Look};
    use crate::studio::{DrawerTab, Hit};

    /// A song with an effect on its first track, shown in the mixer.
    fn with_effect() -> (Studio, Geometry, TrackId, PluginId) {
        let mut s = Studio::sample();
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        let t = s
            .project()
            .tracks
            .values()
            .find(|x| x.kind == TrackKind::Audio)
            .expect("an audio track")
            .id;
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": "Echo", "kind": "effect",
                    "format": "vst3", "uid": "00000000000000000000000000000001" }),
        ));
        let id = s.project().tracks[&t].inserts.last().unwrap().id;
        let g = s.geometry(1600.0, 1000.0);
        assert!(
            s.mixer_center(&g, MixerHit::Open(t, id)).is_some(),
            "its row on screen"
        );
        (s, g, t, id)
    }

    fn crash(s: &mut Studio, id: PluginId, suggest: bool) {
        s.health.fake = Some(vec![Report {
            id,
            crashed: true,
            suggest,
        }]);
        assert!(s.poll_plugins_now());
    }

    fn press_cell(s: &mut Studio, g: &Geometry, act: PlugAct) {
        if s.ctx.is_some()
            && s.popup
                .as_ref()
                .is_some_and(|p| p.kind == PopupKind::Context)
        {
            let label = match act {
                PlugAct::Open => "画面を開く",
                PlugAct::Restart => "再起動",
                PlugAct::Remove => "外す",
                PlugAct::Mode(PluginIsolation::InProcess) => "アプリ内",
                PlugAct::Mode(PluginIsolation::Shared) => "共有",
                PlugAct::Mode(PluginIsolation::Dedicated) => "専用で動かす",
            };
            let label = if s.context_path(label).is_some() {
                label
            } else {
                "専用"
            };
            assert!(crate::context::tests::choose(s, g, label));
            return;
        }

        let layout = s.popup.as_ref().expect("the plugin menu").layout(g.size);
        let i = layout
            .cells
            .iter()
            .position(|c| c.action == Some(Action::Plugin(act)))
            .unwrap_or_else(|| panic!("{act:?} in the menu"));
        assert!(s.press(g, Hit::Item(i)));
    }

    #[test]
    fn a_crashed_plugin_shows_on_its_row_and_head_and_comes_back_on_restart() {
        let (mut s, g, t, id) = with_effect();
        assert_eq!(s.crashed_on(t), None);
        crash(&mut s, id, false);
        assert_eq!(s.crashed_on(t), Some(id));

        // The head's bolt restarts.
        assert!(s.press(&g, Hit::TrackIcon(t)));
        assert!(s.popup.is_none(), "no colour choice while a plugin is down");
        assert_eq!(s.health.restarts, 1);
        assert_eq!(s.crashed_on(t), None);

        // The row opens the menu: its name and 再起動 / 外す, nothing more said.
        crash(&mut s, id, false);
        assert!(s.press(&g, Hit::Mixer(MixerHit::Open(t, id))));
        let popup = s.popup.as_ref().expect("the plugin menu");
        assert_eq!(popup.kind, PopupKind::CrashedPlugin(t, id));
        let layout = popup.layout(g.size);
        let down = layout
            .cells
            .iter()
            .find(|c| c.look == Look::Down)
            .expect("the crashed plugin");
        assert_eq!(down.text, "Echo");
        assert!(down.hint.is_empty());
        assert!(
            !layout
                .cells
                .iter()
                .any(|c| c.action == Some(Action::Plugin(PlugAct::Open)))
        );
        press_cell(&mut s, &g, PlugAct::Restart);
        assert_eq!(s.health.restarts, 2);
        assert_eq!(s.crashed_on(t), None);
        assert!(s.popup.is_none());

        // 外す empties the slot.
        crash(&mut s, id, false);
        s.press(&g, Hit::Mixer(MixerHit::Open(t, id)));
        press_cell(&mut s, &g, PlugAct::Remove);
        assert!(s.project().tracks[&t].inserts.iter().all(|x| x.id != id));
    }

    #[test]
    fn a_restart_brings_back_the_plugin_it_was_asked_for_and_not_the_others() {
        let (mut s, g, t, id) = with_effect();
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": "Verb", "kind": "effect",
                    "format": "vst3", "uid": "00000000000000000000000000000002" }),
        ));
        let other = s.project().tracks[&t].inserts.last().unwrap().id;
        let report = |id| Report {
            id,
            crashed: true,
            suggest: false,
        };
        s.health.fake = Some(vec![report(id), report(other)]);
        assert!(s.poll_plugins_now());

        // The menu of the second one asks for the second one.
        let (x, y) = s.mixer_center(&g, MixerHit::Open(t, other)).unwrap();
        assert!(s.context_press(&g, x, y));
        press_cell(&mut s, &g, PlugAct::Restart);
        assert!(!s.health.crashed(other));
        assert!(s.health.crashed(id), "the first plugin was not asked for");

        // The command takes the plugin in its `id`.
        let args = json!({ "id": id.to_string() });
        assert_eq!(
            s.screen_command(&g, "plugin.restart", args.as_object().unwrap()),
            Some(true)
        );
        assert!(!s.health.crashed(id));
        assert_eq!(s.health.restarts, 2);
    }

    #[test]
    fn a_plugin_that_took_the_shared_process_down_is_offered_one_of_its_own() {
        let (mut s, g, t, id) = with_effect();
        crash(&mut s, id, true);
        let (x, y) = s.mixer_center(&g, MixerHit::Open(t, id)).unwrap();
        assert!(s.context_press(&g, x, y));
        assert!(s.health.crashed(id) && s.health.suggests(id));
        let rows = crate::context::tests::all_rows(&s);
        assert!(rows.iter().any(|r| r.label == "専用で動かす" && r.enabled));
        assert!(rows.iter().any(|r| r.label == "専用" && r.enabled));
        press_cell(&mut s, &g, PlugAct::Mode(PluginIsolation::Dedicated));
        assert_eq!(
            s.project().tracks[&t].inserts.last().unwrap().isolation,
            PluginIsolation::Dedicated
        );
        let v = s.popup.as_ref().unwrap().plugin.clone().unwrap();
        assert!(!v.suggest, "taken");
        assert_eq!(v.mode, PluginIsolation::Dedicated);
        // One step to undo.
        assert!(s.song.undo());
        assert_eq!(
            s.project().tracks[&t].inserts.last().unwrap().isolation,
            PluginIsolation::InProcess
        );
    }

    #[test]
    fn the_menu_sets_where_a_plugin_runs_from_a_right_click_on_its_row() {
        let (mut s, g, t, id) = with_effect();
        let (x, y) = s.mixer_center(&g, MixerHit::Open(t, id)).unwrap();
        assert!(s.context_press(&g, x, y));
        assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Context);
        assert!(s.context_path("画面を開く").is_some());
        assert!(s.context_choose(&g, "動かす場所"));
        let rows = s.context_rows();
        let modes: Vec<_> = rows
            .iter()
            .filter(|r| !r.gap && r.label != "戻る")
            .collect();
        assert_eq!(
            modes.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            ["アプリ内", "共有", "専用"]
        );
        assert_eq!(
            modes
                .iter()
                .filter(|r| r.on)
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            ["アプリ内"]
        );
        press_cell(&mut s, &g, PlugAct::Mode(PluginIsolation::Shared));
        assert_eq!(
            s.project().tracks[&t].inserts.last().unwrap().isolation,
            PluginIsolation::Shared
        );
        assert!(s.popup.is_some(), "the menu stays to show the choice");
    }

    #[test]
    fn a_built_in_plugin_stays_in_the_engine() {
        let (mut s, g, t, _) = with_effect();
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": "EQ", "kind": "effect",
                    "format": "builtin", "uid": "eq" }),
        ));
        let id = s.project().tracks[&t].inserts.last().unwrap().id;
        let (x, y) = s.mixer_center(&g, MixerHit::Open(t, id)).unwrap();
        assert!(s.context_press(&g, x, y));
        assert!(s.context_choose(&g, "動かす場所"));
        let live: Vec<_> = s
            .context_rows()
            .into_iter()
            .filter(|r| r.enabled && r.label != "戻る" && !r.gap)
            .map(|r| r.label)
            .collect();
        assert_eq!(live, ["アプリ内"]);
    }

    #[test]
    fn an_ara_plugin_runs_here_only() {
        let (mut s, g, t, id) = with_effect();
        let (x, y) = s.mixer_center(&g, MixerHit::Open(t, id)).unwrap();
        assert!(s.context_press(&g, x, y));
        press_cell(&mut s, &g, PlugAct::Mode(PluginIsolation::Shared));
        s.popup = None;
        s.use_plugins(xirius_plugins::Catalog {
            entries: vec![xirius_plugins::Entry {
                uid: "00000000000000000000000000000001".into(),
                name: "Echo".into(),
                vendor: "Test".into(),
                kind: xirius_core::model::PluginKind::Effect,
                format: "vst3".into(),
                category: "Fx|Pitch Shift".into(),
                version: "1.0".into(),
                path: std::path::PathBuf::from("C:/nowhere/Echo.vst3"),
                ara: true,
                requires_process_main: false,
            }],
            failures: Vec::new(),
        });
        crash(&mut s, id, true);
        let (x, y) = s.mixer_center(&g, MixerHit::Open(t, id)).unwrap();
        assert!(s.context_press(&g, x, y));
        let all = crate::context::tests::all_rows(&s);
        assert!(!all.iter().any(|r| r.label == "専用で動かす"));
        assert!(s.context_choose(&g, "動かす場所"));
        let modes = s.context_rows();
        assert_eq!(
            modes
                .iter()
                .filter(|r| r.enabled && r.label != "戻る" && !r.gap)
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            ["アプリ内"]
        );
        assert_eq!(
            modes
                .iter()
                .filter(|r| r.on)
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            ["アプリ内"]
        );
    }

    #[test]
    fn with_no_host_running_nothing_is_down() {
        let (mut s, _, t, _) = with_effect();
        assert!(!s.poll_plugins());
        assert_eq!(s.crashed_on(t), None);
        assert!(s.restart_plugins());
        assert!(s.error.is_none(), "{:?}", s.error);
    }
    #[test]
    fn normal_context_process_choices_show_the_effective_policy_and_restore_it() {
        let (mut s, g, t, id) = with_effect();
        let p = s.project();
        let x = p.tracks[&t].inserts.iter().find(|x| x.id == id).unwrap();
        let key = xirius_prefs::isolation::key(&x.format, &x.uid);
        s.song
            .set("prefs.set", json!({"key":key,"value":"dedicated"}))
            .unwrap();
        let items = s.plugin_items(t, id);
        s.open_context(items, 400.0, 300.0);
        assert!(s.context_choose(&g, "動かす場所"));
        let choices = s.context_rows();
        assert!(choices.iter().any(|r| r.label == "専用" && r.on));
        let marks = crate::a11y::marks(&s, &g);
        let dedicated = marks
            .iter()
            .find(|m| m.node.label() == Some("専用"))
            .unwrap();
        assert_eq!(
            dedicated.node.toggled(),
            Some(gpui::accesskit::Toggled::True)
        );
        assert!(s.context_choose(&g, "アプリ内"));
        assert_eq!(s.song.settings().lock().unwrap().prefs.text(&key), "same");
        let items = s.plugin_items(t, id);
        s.open_context(items, 400.0, 300.0);
        assert!(s.context_choose(&g, "動かす場所"));
        assert!(
            s.context_rows()
                .iter()
                .any(|r| r.label == "アプリ内" && r.on)
        );
    }
}
