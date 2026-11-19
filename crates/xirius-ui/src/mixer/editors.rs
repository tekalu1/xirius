//! A VST3 plugin's own window (UI-040, PLG-027).
//!
//! The window shows the instance that plays, so what is turned in it is heard at once and,
//! once let go, becomes one step of the history ([`Studio::poll_editors`]):
//!
//! A plugin host process opens the window of its instance. Main-window activation queues
//! a raise without assigning a cross-process HWND owner or taking focus.
//!
//! The player starts when a window needs it. Under tests, and while
//! [`xirius_vst3::OFFSCREEN_ENV`] is set, windows open off every screen.
//!

use xirius_core::id::PluginId;
use xirius_core::model::Plugin;
use xirius_engine::server::EditorOpenEvent;
use xirius_vst3::{Caption, Placement};

use crate::studio::Studio;

/// Where an open window's instance is.
pub(crate) enum Where {
    /// The plugin host process's instance; the host has the window. `opened` says the host has
    /// said the window is open once, so it is no longer taken as open while it does not answer.
    Host { opened: bool },
}

/// A plugin's own window.
pub(crate) struct OpenEditor {
    pub id: PluginId,
    pub title: String,
    pub at: Where,
}

impl OpenEditor {
    pub fn param_titles(&self) -> Vec<(u32, String)> {
        Vec::new()
    }

    /// The instance plays: what is turned in the window is heard.
    pub fn heard(&self) -> bool {
        true
    }
}

/// Why a plugin's window does not open, in the words the screen shows (ARC-027).
fn cannot(name: &str, why: &str) -> String {
    format!("{name} {why}ため、窓を開けません")
}

impl Studio {
    fn vst_bar(&self, plugin: &Plugin) -> xirius_vst3::EditorBar {
        let p = self.project();
        let track = p
            .tracks
            .values()
            .find(|t| t.inserts.iter().any(|p| p.id == plugin.id));
        xirius_vst3::EditorBar {
            track: track.map_or_else(String::new, |t| t.shown_name().into()),
            device: plugin.name.clone(),
            preset: plugin.preset.clone().unwrap_or_else(|| "—".into()),
            presets: self
                .instruments
                .presets
                .external
                .get(&plugin.uid.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default(),
            instrument: plugin.kind == xirius_core::model::PluginKind::Instrument,
            powered: plugin.runs(),
            pinned: self.mixer.editor_pins.contains(&plugin.id),
            sidechain: plugin.sidechain.is_some(),
        }
    }
    pub(crate) fn replace_unpinned_editors(&mut self, keep: PluginId) {
        let close: Vec<_> = self
            .mixer
            .editors
            .iter()
            .filter(|e| e.id != keep && !self.mixer.editor_pins.contains(&e.id))
            .map(|e| e.id)
            .collect();
        for id in close {
            self.close_editor(id);
        }
    }
    /// Activation queues a raise on each plugin's GUI owner. It neither opens a view
    /// nor establishes an HWND owner across threads/processes.
    pub(crate) fn main_window_editor_event(&self, active: bool, restored: bool) {
        for e in main_window_requests(active, restored, &self.mixer.editors) {
            if let Some(host) = self.transport.host() {
                host.raise_plugin_editor(e.id);
            }
        }
    }

    /// Where a window of this process opens: over the studio's window and above it, or off
    /// every screen under tests.
    pub(crate) fn editor_placement(&self) -> Placement {
        let owner = crate::park::own_handle(crate::screen::WINDOW_TITLE);
        let place = if cfg!(test) {
            Placement {
                owner,
                ..Placement::offscreen()
            }
        } else {
            Placement::shown(owner)
        };
        Placement {
            caption: self.caption,
            ..place
        }
    }

    /// The windows' captions take the colours of the app's top bar in `theme`: at once for
    /// the windows that are open, and from the first frame for the ones that open later
    /// (UI-183).
    pub fn set_caption_theme(&mut self, theme: xirius_design::Theme) {
        use xirius_design::{Surface, Theme};
        let bar = Surface::S0.strong_text().resolve(theme);
        let caption = Caption {
            dark: theme == Theme::Dark,
            fill: bar.surface.to_rgb_u32(),
            text: bar.ink.to_rgb_u32(),
        };
        if self.caption == Some(caption) {
            return;
        }
        self.caption = Some(caption);
        for e in &self.mixer.editors {
            if let Some(host) = self.transport.host() {
                host.plugin_caption(e.id, caption);
            }
        }
        if let Some(host) = self.transport.host() {
            host.set_ara_editor_caption(caption);
        }
    }

    /// Opens `plugin`'s own window, on the instance that plays it. `at` keeps the corner of a
    /// window that opens again.
    pub(crate) fn open_vst3_window(
        &mut self,
        plugin: &Plugin,
        title: String,
        at: Option<(i32, i32)>,
    ) -> Result<OpenEditor, String> {
        self.mixer.editor_bars.remove(&plugin.id);
        let _filter = xirius_engine::crash::guard_exception_filter();
        let name = plugin.name.as_str();
        if plugin.format != "vst3" {
            return Err(cannot(name, "は VST3 ではない"));
        }
        let catalog = self.plugin_catalog();
        if catalog.find(&plugin.uid).is_none() {
            return Err(cannot(name, "がこの PC に見つからない"));
        }
        let mut base = self.editor_placement();
        if let Some(area) = &mut base.area {
            area.restore = self.mixer.editor_positions.get(&plugin.id).copied();
        }
        let place = Placement {
            at: at.or(base.at),
            ..base
        };
        let project = self.auto_heard(&self.project());
        let bar = self.vst_bar(plugin);
        let dir = self.song.dir();
        let host = self.transport.start_host(&project, dir)?;
        let status = host
            .plugin_statuses()
            .into_iter()
            .find(|(id, ..)| *id == plugin.id)
            .map(|(.., s)| s);
        let Some(status) = status else {
            // The player has no instance of it: it could not be loaded.
            return Err(cannot(name, "を読み込めなかった"));
        };
        if status.crashed {
            return Err(cannot(name, "が止まっている"));
        }
        if status.error.is_some() {
            return Err(cannot(name, "を読み込めなかった"));
        }
        if let Some(area) = place.area {
            host.plugin_editor_area(plugin.id, area);
        }
        host.plugin_editor_bar(plugin.id, bar);
        host.plugin_editor(plugin.id, true, self.caption)
            .map_err(|e| format!("{name} の窓を開けません: {e}"))?;
        let title = host
            .plugins()
            .editor_window_title(plugin.id)
            .unwrap_or(title);
        Ok(OpenEditor {
            id: plugin.id,
            title,
            at: Where::Host { opened: false },
        })
    }

    /// Shuts a plugin's window.
    pub(crate) fn close_editor(&mut self, id: PluginId) {
        self.mixer.editor_bars.remove(&id);
        self.mixer
            .editor_pending
            .retain(|(other, _, _)| *other != id);
        let gone: Vec<OpenEditor> = self.mixer.take_editors(|e| e.id == id);
        let shut = !gone.is_empty();
        for _ in gone {
            if let Some(host) = self.transport.host_mut() {
                let _ = host.plugin_editor(id, false, None);
            }
        }
        if shut {
            // What was done in the window is in the song when it is gone (PLG-057).
            self.pull_plugins();
        }
    }

    /// Asks every plugin for its state and takes what differs from the song's into it, as one
    /// step of the history (PLG-057). A plugin that changes its patch in its own window does
    /// not always say so, and the song would keep the state it was opened with. For the
    /// moments the song is written or left, and when a window closes.
    pub(crate) fn pull_plugins(&mut self) {
        let Some(host) = self.transport.host_mut() else {
            return;
        };
        // A frame will collect the result. Save/open/quit use their explicit
        // StatePull preflight; this helper must never spin in a window callback.
        host.begin_pull_states();
    }

    /// Brings the open window of `id` above the other windows.
    pub(crate) fn raise_editor(&mut self, id: PluginId) {
        if self.mixer.editors.iter().any(|e| e.id == id)
            && let Some(host) = self.transport.host_mut()
        {
            host.raise_plugin_editor(id);
        }
    }

    /// The window of `id` is open and shown.
    pub(crate) fn editor_shown(&self, id: PluginId) -> bool {
        self.mixer
            .editors
            .iter()
            .filter(|e| e.id == id)
            .any(|e| self.shown(e))
    }

    fn shown(&self, e: &OpenEditor) -> bool {
        match &e.at {
            Where::Host { opened } => {
                let status = self.transport.host().and_then(|host| {
                    host.plugin_statuses()
                        .into_iter()
                        .find(|(id, ..)| *id == e.id)
                        .map(|(.., status)| status)
                });
                status
                    .is_some_and(|s| !s.crashed && s.error.is_none() && (!*opened || s.editor_open))
            }
        }
    }

    /// The host process has said the window of `id` is open: from then on, only its word
    /// decides whether the window is shown.
    pub(crate) fn host_said_editor_open(&self, id: PluginId) -> bool {
        self.mixer
            .editors
            .iter()
            .any(|e| e.id == id && matches!(e.at, Where::Host { opened: true, .. }))
    }

    /// Between frames too: forgets the windows the person closed, opens a window again on
    /// the instance of a new mix, and takes what was turned in the plugins' windows into the
    /// song. True when the song changed.
    pub fn poll_editors(&mut self) -> bool {
        let bar = self.poll_editor_bar();
        let keys = bar | self.poll_editor_keys() | self.mixer.preset_menu.is_some();
        let mut failed = Vec::new();
        let mut opened_sizes = Vec::new();
        if let Some(host) = self.transport.host() {
            for editor in &mut self.mixer.editors {
                for event in host.take_editor_open_events(editor.id) {
                    match event {
                        EditorOpenEvent::Opening => {}
                        EditorOpenEvent::Opened { size, .. } => {
                            let Where::Host { opened } = &mut editor.at;
                            *opened = true;
                            opened_sizes.push((editor.id, size));
                        }
                        EditorOpenEvent::Failed { reason } => failed.push((editor.id, reason)),
                    }
                }
            }
        }
        if !opened_sizes.is_empty() {
            let project = self.project();
            let sizes: Vec<_> = opened_sizes
                .into_iter()
                .filter_map(|(id, size)| {
                    project.tracks.values().find_map(|t| {
                        t.inserts
                            .iter()
                            .find(|p| p.id == id)
                            .map(|p| (p.uid.clone(), size))
                    })
                })
                .collect();
            self.mixer.editor_sizes.extend(sizes);
        }
        for (id, reason) in failed {
            self.close_editor(id);
            self.error = Some(reason);
        }
        let gone: Vec<PluginId> = self
            .mixer
            .editors
            .iter()
            .filter(|e| !self.shown(e))
            .map(|e| e.id)
            .collect();
        for id in gone {
            self.close_editor(id);
        }
        // While the song is being left, answers to the close's own pulls go in at the cursor:
        // they are not the person's edits (UI-354).
        let leaving = self.leaving_song();
        let Some(host) = self.transport.host_mut() else {
            return keys;
        };
        let edits = match if leaving {
            self.song.collect_plugin_edits_in_place(host)
        } else {
            self.song.collect_plugin_edits(host)
        } {
            Ok(false) => false,
            Ok(true) => {
                self.after_edit();
                true
            }
            Err(e) => {
                self.error = Some(e);
                true
            }
        };
        keys | edits
    }

    pub(crate) fn poll_editor_bar(&mut self) -> bool {
        use xirius_vst3::EditorAction;
        let mut actions = std::mem::take(&mut self.mixer.editor_pending);
        let p = self.project();
        for e in &self.mixer.editors {
            let origin = self
                .transport
                .host()
                .and_then(|h| h.plugins().editor_origin(e.id));
            if let Some(origin) = origin {
                self.mixer.editor_positions.insert(e.id, origin);
            }
            let Some((track, plugin)) = p
                .tracks
                .values()
                .find_map(|t| t.inserts.iter().find(|p| p.id == e.id).map(|p| (t, p)))
            else {
                continue;
            };
            let bar = xirius_vst3::EditorBar {
                track: track.shown_name().into(),
                device: plugin.name.clone(),
                preset: plugin.preset.clone().unwrap_or_else(|| "—".into()),
                presets: self
                    .instruments
                    .presets
                    .external
                    .get(&plugin.uid.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_default(),
                instrument: plugin.kind == xirius_core::model::PluginKind::Instrument,
                powered: plugin.runs(),
                pinned: self.mixer.editor_pins.contains(&e.id),
                sidechain: plugin.sidechain.is_some(),
            };
            if self.mixer.editor_bars.get(&e.id) != Some(&bar) {
                if let Some(host) = self.transport.host() {
                    host.plugin_editor_bar(e.id, bar.clone());
                }
                self.mixer.editor_bars.insert(e.id, bar);
            }
            let events = self
                .transport
                .host()
                .map_or_else(Vec::new, |h| h.take_editor_actions(e.id));
            actions.extend(events.into_iter().map(|a| (e.id, track.id, a)));
        }
        let mut changed = false;
        for (id, track, action) in actions {
            if std::env::var_os("XIRIUS_HEADER_PROBE").is_some() {
                eprintln!(
                    "header-action id={id} action={action:?} blocked={}",
                    self.input_blocked()
                );
            }
            if !self.mixer.is_open(id) {
                continue;
            }
            let needs_song = matches!(
                action,
                EditorAction::Power
                    | EditorAction::PresetSelect(_)
                    | EditorAction::PresetPrevious
                    | EditorAction::PresetNext
                    | EditorAction::Sidechain
            );
            if needs_song
                && (self.input_blocked() || (self.macro_run.is_some() && !self.macro_stepping))
            {
                self.mixer.editor_pending.push((id, track, action));
                continue;
            }
            changed = true;
            match action {
                EditorAction::Power => {
                    // design-lint: allow(ui-blocking-io) PRJ-031: guarded dispatcher enqueues the model edit; the plugin GUI never waits for it.
                    let accepted = self.run_command(
                        "plugin.bypass",
                        serde_json::json!({"ids": [id.to_string()]}),
                    );
                    if std::env::var_os("XIRIUS_HEADER_PROBE").is_some() {
                        eprintln!("header-action id={id} power_accepted={accepted}");
                    }
                }
                EditorAction::Pin => {
                    if !self.mixer.editor_pins.remove(&id) {
                        self.mixer.editor_pins.insert(id);
                    }
                }
                EditorAction::Channel => self.open_channel(track),
                EditorAction::PresetMenu { x, y, dpi } => {
                    self.mixer.preset_menu = Some((track, id, x, y, dpi));
                }
                EditorAction::PresetOpen => {
                    self.file_request = Some(crate::studio::FileRequest::PresetOpen(id))
                }
                EditorAction::PresetSave => {
                    self.file_request = Some(crate::studio::FileRequest::PresetSave(id))
                }
                EditorAction::PresetSelect(preset) => {
                    // design-lint: allow(ui-blocking-io) UI-337: the guarded dispatcher loads on the song worker; the plugin GUI only reports the selected name.
                    self.run_command(
                        "preset.load",
                        serde_json::json!({"track":track, "of":id, "preset":preset}),
                    );
                }
                EditorAction::PresetPrevious | EditorAction::PresetNext => {
                    self.step_device_preset(id, action == EditorAction::PresetNext);
                }
                EditorAction::Sidechain => {
                    if let Some(sc) = p
                        .tracks
                        .get(&track)
                        .and_then(|t| t.inserts.iter().find(|p| p.id == id))
                        .and_then(|p| p.sidechain.as_ref())
                    {
                        // design-lint: allow(ui-blocking-io) PRJ-031: guarded dispatcher queues the existing sidechain command.
                        self.run_command("plugin.sidechain.enable", serde_json::json!({"id": id.to_string(), "state": if sc.on { "off" } else { "on" }}));
                    }
                }
            }
        }
        changed
    }
}

/// Activation and restoration request only views already tracked as open.
fn main_window_requests(
    active: bool,
    restored: bool,
    editors: &[OpenEditor],
) -> impl Iterator<Item = &OpenEditor> {
    editors.iter().filter(move |_| active || restored)
}

#[cfg(test)]
mod activation_tests {
    use super::*;

    #[test]
    fn activation_and_restore_request_open_views_without_opening_closed_devices() {
        let mut open = vec![OpenEditor {
            id: PluginId(7),
            title: "Effect".into(),
            at: Where::Host { opened: true },
        }];
        let requests = |active, restored, views: &[OpenEditor]| {
            main_window_requests(active, restored, views)
                .map(|e| e.id)
                .collect::<Vec<_>>()
        };
        assert!(requests(false, false, &open).is_empty());
        assert_eq!(requests(true, false, &open), [PluginId(7)]);
        assert_eq!(requests(false, true, &open), [PluginId(7)]);
        open.clear();
        assert!(requests(true, true, &open).is_empty());
    }
}
