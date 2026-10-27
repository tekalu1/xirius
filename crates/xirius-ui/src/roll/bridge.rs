//! The roll's command boundary: all song changes go through the session.
use super::{Ctx, Out, Then};
use crate::studio::{DrawerTab, Geometry, Studio};
use crate::tools::{Holds, Mods, Snap, Tools};

impl Studio {
    /// The tool and the grid as the roll sees them now: the picked tool and
    /// mode from here, the grid value and snap on/off from the session, so
    /// the top bar and the roll read one state.
    pub fn tools(&self) -> Tools {
        let ppq = self.project().ppq;
        Tools {
            tool: self.tool,
            snap: Snap {
                on: self.song.snap_for("arrange"),
                mode: self.snap_mode,
                grid: xirius_core::time::parse_grid(self.song.grid_for("arrange"), ppq)
                    .unwrap_or(i64::from(ppq) / 4),
                triplet: triplet(self.song.grid_for("arrange")),
            },
            holds: self.holds(),
        }
    }

    pub fn roll_tools(&self) -> Tools {
        let mut tools = self.tools();
        tools.snap.on = self.song.snap_for("roll");
        tools.snap.mode = self.roll_grid_mode();
        tools.snap.grid =
            xirius_core::time::parse_grid(self.song.grid_for("roll"), self.project().ppq)
                .unwrap_or(240);
        tools.snap.triplet = triplet(self.song.grid_for("roll"));
        tools
    }

    /// The holds changed in the settings (KEY-026).
    pub fn holds(&self) -> Holds {
        let shared = self.song.settings().clone();
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let s = shared.lock().unwrap_or_else(|e| e.into_inner());
        Holds::of(&s.mousemap)
    }

    pub fn roll_ctx(&self, g: &Geometry) -> Option<Ctx> {
        if g.drawer_body.h <= 0.0 || self.tab != DrawerTab::Roll {
            return None;
        }
        let (mousemap, prefs) = {
            let settings = self
                .song
                .settings()
                // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            (settings.mousemap.clone(), settings.prefs.clone())
        };
        Some(Ctx {
            mousemap,
            prefs,
            maps: self.meters.of(&self.project(), self.roll.beat_px),
            project: self.project(),
            clip: self.roll_target()?.clip,
            tools: self.roll_tools(),
            shape: self.song.session().state.view.draw_shape,
            body: g.drawer_body,
            drum: self.roll_drum(),
            drum_rows: self.roll_drum_rows(),
            others: self.roll_parts(),
        })
    }

    /// What was changed in the drum rows of the roll's track.
    pub fn roll_drum_rows(&self) -> xirius_core::drum::RowEdits {
        let Some(target) = self.roll_target() else {
            return Default::default();
        };
        let track = target.track.id.to_string();
        let mut edits =
            xirius_core::drum::edits(&self.song.session().state.view.roll, &track).into_owned();
        edits.plugin = self.instrument_note_names(&target.track);
        edits
    }

    /// The names the track's instrument gives its keys (EDT-134): those of the first playing
    /// instrument plugin that names any.
    fn instrument_note_names(
        &self,
        track: &xirius_core::model::Track,
    ) -> std::collections::BTreeMap<u8, String> {
        let Some(host) = self.transport.host() else {
            return Default::default();
        };
        track
            .inserts
            .iter()
            .filter(|x| x.kind == xirius_core::model::PluginKind::Instrument && !x.deactivated)
            .map(|x| host.plugins().note_names(x.id))
            .find(|names| !names.is_empty())
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    /// Once a frame (EDT-134, UI-285): while the roll shows a part of an instrument track,
    /// starts the player (it plays nothing) once for that track, so the device is open and the
    /// instrument loaded before the first key or note sounds, and the drum rows can read its
    /// names. True when the names changed.
    pub(crate) fn poll_roll_host(&mut self) -> bool {
        let shown = self.drawer_open && self.tab == crate::studio::DrawerTab::Roll;
        let (track, drums) = match self.selected_clip {
            Some((t, c)) if shown => (
                self.project().tracks.get(&t).cloned(),
                self.roll_drum_of(c).is_some(),
            ),
            _ => (None, false),
        };
        if let Some(track) = &track
            && track.kind == xirius_core::model::TrackKind::Instrument
            && self.transport.host().is_none()
            && !self.transport.is_windowed()
            && !self.sound_preparing()
            && self.roll.host_for != Some(track.id)
        {
            self.roll.host_for = Some(track.id);
            let project = self.auto_heard(&self.project());
            let dir = self.song.dir();
            let _ = self.transport.start_host(&project, dir);
        }
        let names = match track {
            Some(track) if drums => self.instrument_note_names(&track),
            _ => Default::default(),
        };
        if names == self.roll.drum_names {
            return false;
        }
        self.roll.drum_names = names;
        true
    }

    /// The drum map the roll's clip is shown in (EDT-006), or none for the keyboard.
    /// Whether the drawer's bar shows the roll's view switch: the roll is open on a part.
    pub fn roll_switch_shown(&self) -> bool {
        self.drawer_open
            && self.tab == crate::studio::DrawerTab::Roll
            && self.roll_target().is_some()
    }

    /// The switch was pressed: the open part is shown as drums or as keys from now on.
    pub(crate) fn roll_view_choose(&mut self, g: &Geometry, drums: bool) -> bool {
        let Some(target) = self.roll_target() else {
            return false;
        };
        let mode = if drums { "drums" } else { "keys" };
        let args = serde_json::json!({ "clip": target.clip.id.to_string(), "mode": mode });
        self.roll_view_run(g, args)
    }

    /// The map button was pressed: a menu of the maps, the one in use marked.
    pub(crate) fn roll_map_menu(&mut self, g: &Geometry) -> bool {
        let Some(target) = self.roll_target() else {
            return false;
        };
        let now = self.roll_drum();
        let clip = target.clip.id.to_string();
        let mut items: Vec<_> = xirius_core::drum::MAPS
            .iter()
            .map(|(map, label)| {
                let args = serde_json::json!({ "clip": clip, "map": map });
                self.ctx_setting(label, "roll.view", args)
                    .on(now.as_deref() == Some(*map))
            })
            .collect();
        if let Some(ctx) = self.roll_ctx(g) {
            items.extend(self.drum_row_items(&ctx));
        }
        let r = g.roll_map();
        self.open_context(items, r.x, r.bottom() - xirius_design::Layout::TOP_BAR_H);
        true
    }

    fn roll_view_run(&mut self, g: &Geometry, args: serde_json::Value) -> bool {
        if self.arrange_run("roll.view", args).is_none() {
            return false;
        }
        self.rebuild(g);
        if let Some(ctx) = self.roll_ctx(g) {
            self.roll.sync(&ctx);
        }
        true
    }

    /// Whether the pencil draws with the length of the note before (the setting 直前のノート)
    /// rather than the grid's value (EDT-131).
    pub fn draw_length_last(&self) -> bool {
        let settings = self
            .song
            .settings()
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        settings.prefs.text("edit.draw.length") == "last"
    }

    /// The length the next note is drawn with, as the bar shows it, while the roll draws with the
    /// note before. None when the roll is not in sight, in the drum view (a hit has no length) or
    /// with the grid's value.
    pub fn draw_length_text(&self) -> Option<String> {
        if self.tool_area() != crate::tools::Area::Roll
            || !self.roll_switch_shown()
            || self.roll_drum().is_some()
            || !self.draw_length_last()
        {
            return None;
        }
        let ppq = self.project().ppq;
        let step = self
            .roll_tools()
            .snap
            .step(&self.meters.of(&self.project(), self.roll.beat_px));
        Some(super::length_text(
            self.roll.draw_length(true, step),
            i64::from(ppq),
        ))
    }

    /// The switch "入力の長さ" (or the bar's chip, which is its way back) was pressed.
    pub(crate) fn draw_length_choose(&mut self, last: bool) -> bool {
        let value = if last { "last" } else { "inspector" };
        self.arrange_run(
            "prefs.set",
            serde_json::json!({ "key": "edit.draw.length", "value": value }),
        )
        .is_some()
    }

    pub fn roll_drum(&self) -> Option<String> {
        self.roll_drum_of(self.roll_target()?.clip.id)
    }

    fn roll_drum_of(&self, clip: xirius_core::id::ClipId) -> Option<String> {
        xirius_core::drum::shown(&self.project(), &self.song.session().state.view.roll, clip)
    }

    /// The roll's context for a pointer or key event: none while a pop-over
    /// is open, so a press there goes to the pop-over (or closes it).
    pub fn roll_input(&self, g: &Geometry) -> Option<Ctx> {
        if self.popup.is_some() {
            return None;
        }
        self.roll_ctx(g)
    }

    pub fn apply_roll(&mut self, g: &Geometry, out: Out) -> bool {
        if let Some(tool) = out.tool {
            self.tool = tool;
        }
        if let Some(at) = out.seek {
            // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
            self.seek(at);
        }
        if out.follow {
            self.toggle_follow();
        }
        for edit in out.edits {
            let args = if edit.command == "note.quantize" {
                self.quant_args(edit.args)
            } else {
                edit.args
            };
            let changed_velocity = args["notes"]
                .as_array()
                .and_then(|notes| notes.iter().find_map(|n| n["velocity"].as_u64()))
                .map(|v| v as u8);
            match self.song.run(edit.command, args) {
                Ok(data) => {
                    self.error = None;
                    match edit.then {
                        Then::Clear => self.roll.selected.clear(),
                        Then::Select => {
                            self.roll.selected = data["ids"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|v| v.as_str()?.parse().ok())
                                .collect();
                        }
                        Then::Keep => {}
                        Then::Part => {
                            if let Some(id) = data["id"].as_str().and_then(|id| id.parse().ok())
                                && let Some((track, _)) = self.project().find_clip(id)
                            {
                                self.select_clip_here(track, id);
                            }
                        }
                    }
                    self.after_edit();
                    if let Some(velocity) = out.next_velocity.or(changed_velocity) {
                        self.roll.next_vel = velocity;
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(ctx) = self.roll_ctx(g) {
            self.roll.sync(&ctx);
        }
        self.sound_roll_audition();
        out.used || out.redraw
    }

    pub fn edit_key(&mut self, g: &Geometry, key: &str, mods: Mods) -> bool {
        self.mapped_key(g, key, None, mods)
    }
}

/// Whether a grid value of the list is a triplet (`1/8t`).
fn triplet(grid: &str) -> bool {
    grid.trim().ends_with('t')
}
