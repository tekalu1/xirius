//! Input maps resolve names; execution fills the selection exactly as the palette does.
use serde_json::{Map, Value, json};
use xirius_core::id::{IdKind, TrackId};
use xirius_prefs::{Chord, Os};

use crate::palette::Fill;
use crate::studio::{DrawerTab, FileRequest, Geometry, Studio};
use crate::tools::{Mods, Tool};
use crate::tree::RowKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    pub(crate) time: f32,
    pub(crate) data: f32,
    pub(crate) scroll: (f32, f32),
    pub(crate) roll: bool,
}

#[derive(Default)]
pub struct InputState {
    pub context_delete: Option<(&'static str, Value)>,
    pub context_clipboard: Option<Value>,
    pub middle_origin: Option<(f32, f32)>,
    pub middle_moved: bool,
    pub tempo_selected: Option<xirius_core::id::TempoId>,
    pub marker_selected: Option<xirius_core::id::MarkerId>,
    pub name: Option<crate::track_name::Name>,
    pub prefs: Option<xirius_prefs::Prefs>,
    pub snap_area: Option<&'static str>,
    pub zoom_rect_armed: bool,
    undo: Vec<Zoom>,
    redo: Vec<Zoom>,
    stored: Option<Zoom>,
    selection: Option<Zoom>,
    pub parameter: usize,
    pub zoom_box: Option<crate::zoom_input::BoxZoom>,
    pub pointer: (f32, f32),
    pub mouse_drag_disabled: bool,
    pub mouse_click_action: Option<String>,
    pub browser_focus: bool,
    pub edge: crate::drag_scroll::EdgeScroll,
}

pub fn bits(m: Mods) -> u8 {
    (u8::from(m.ctrl) * xirius_prefs::chord::MOD)
        | (u8::from(m.alt) * xirius_prefs::chord::ALT)
        | (u8::from(m.shift) * xirius_prefs::chord::SHIFT)
}

impl Studio {
    pub fn input_context(&self, g: &Geometry) -> &'static str {
        if self
            .popup
            .as_ref()
            .is_some_and(|p| p.kind == crate::popup::PopupKind::Export)
        {
            "export"
        } else if self.input_map.name.is_some()
            || self.browser.typing
            || self.popup.as_ref().is_some_and(|p| p.naming.is_some())
        {
            "text"
        } else if self.browser.open && self.input_map.browser_focus {
            "browser"
        } else if self.arrange.focus && !self.arrange.points.is_empty() {
            "automation"
        } else if !self.arrange.focus && self.roll_ctx(g).is_some() {
            if self.roll.points.is_empty() {
                "roll"
            } else {
                "lane"
            }
        } else if self.drawer_open && self.tab == DrawerTab::Mixer && !self.arrange.focus {
            "mixer"
        } else {
            "arrange"
        }
    }

    /// KEY-025: the menu's owner can use this without owning the input map.
    pub fn key_display(&self, context: &str, command: &str) -> String {
        let mut s = self
            .song
            .settings()
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        s.reload_if_changed();
        s.keymap.display_for(context, command, Os::current())
    }

    /// The key a menu row shows for `name` (KEY-025): the one the keymap gives the command
    /// itself (not its variants with arguments, such as paste at the original position), empty
    /// when it has none, and it follows a remap in the current editing context.
    pub fn menu_key(&self, name: &str) -> String {
        let context = if self.arrange.focus {
            if self.arrange.points.is_empty() {
                "arrange"
            } else {
                "automation"
            }
        } else if self.drawer_open && self.tab == DrawerTab::Mixer {
            "mixer"
        } else if self.roll.points.is_empty() {
            "roll"
        } else {
            "lane"
        };
        self.menu_key_in(context, name)
    }

    /// [`Studio::menu_key`] in a given editing context: for a part of the screen that is always
    /// in one (the arrangement's head), whatever has the focus.
    pub fn menu_key_in(&self, context: &str, name: &str) -> String {
        let mut s = self
            .song
            .settings()
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        s.reload_if_changed();
        let mut keys = Vec::new();
        for own in xirius_prefs::catalog::chain(context) {
            for key in s.keymap.keys_of(own, name) {
                if s.keymap
                    .lookup(context, &key)
                    .is_some_and(|(_, command)| command == name)
                {
                    let text = key.display(Os::current());
                    if !keys.contains(&text) {
                        keys.push(text);
                    }
                }
            }
        }
        // " / " between two keys would read as one of them when a key is the slash itself
        // ("NumPad/ / /"): then the first key stands alone.
        if keys.len() > 1 && keys.iter().any(|k| k.contains('/')) {
            keys.truncate(1);
        }
        keys.join(" / ")
    }

    /// Logical characters take consumed Shift out of punctuation (JIS `+`), while physical
    /// bindings use the window's base key. The optional character comes from the window/IME.
    pub fn mapped_key(
        &mut self,
        g: &Geometry,
        key: &str,
        character: Option<&str>,
        mods: Mods,
    ) -> bool {
        self.mapped_key_code(g, key, character, mods, None)
    }

    pub fn mapped_key_code(
        &mut self,
        g: &Geometry,
        key: &str,
        character: Option<&str>,
        mods: Mods,
        code: Option<&str>,
    ) -> bool {
        self.cancel_browser_menu();
        if self.unsaved_key(key)
            || self.prepare_key(key)
            || self.opening_key(key)
            || self.saving_key(key)
        {
            return true;
        }
        if self.name_key(key, mods) {
            return true;
        }
        if self.rec_key(key) {
            return true;
        }
        if self.comp_key(key, mods) {
            return true;
        }
        if self.settings.is_some() {
            return self.settings_key(key, mods);
        }
        if self.palette.is_some() {
            return self.palette_key(g, key, mods);
        }
        if self.bar_key(key, character, mods) {
            return true;
        }
        if self.drag_space(key, true) {
            return true;
        }
        if self.pool_key(g, key, mods) {
            return true;
        }
        if key == "escape" {
            if self.popup.take().is_some() {
                self.close_wheel_group();
                return true;
            }
            // Waiting for a controller's knob ends first (OPS-007).
            if self.live_midi.assign.learning.take().is_some() {
                return true;
            }
            self.roll.cancel();
            self.input_map.zoom_rect_armed = false;
            self.input_map.zoom_box = None;
            if self.arrange.drag.take().is_some() {
                self.arrange.touched();
            }
            return true;
        }
        self.apply_edit_prefs();
        let context = self.input_context(g);
        let Some(chord) = Chord::from_event(key, bits(mods)) else {
            return false;
        };
        let binding = {
            let mut s = self
                .song
                .settings()
                // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            s.reload_if_changed();
            let mut candidates = Vec::new();
            if let Some(c) = character.filter(|c| {
                c.chars().count() == 1
                    && !c
                        .chars()
                        .any(|ch| ch.is_alphanumeric() || ch.is_whitespace() || ch.is_control())
            }) {
                candidates.extend(Chord::from_event(
                    c,
                    bits(mods) & !xirius_prefs::chord::SHIFT,
                ));
            }
            candidates.push(chord.clone());
            if let Some(code) = code.map(String::from).or_else(|| physical(key)) {
                candidates.insert(0, Chord::new(bits(mods), &code));
            }
            let no_notes = self.roll.selected.is_empty();
            candidates.into_iter().find_map(|c| {
                let hit = s.keymap.lookup(context, &c)?;
                // Shift+↑↓ move the roll's selected notes an octave (UI-295); with none, the
                // key goes on to the wider row and the selection grows.
                if no_notes
                    && hit.0 == "roll"
                    && c.mods == xirius_prefs::chord::SHIFT
                    && hit.1.starts_with("note.transpose")
                {
                    let wider = *xirius_prefs::catalog::chain("roll").get(1)?;
                    return s.keymap.lookup(wider, &c);
                }
                Some(hit)
            })
        };
        let Some((_, command)) = binding else {
            return false;
        };
        self.run_binding(g, &command)
    }

    pub fn run_binding(&mut self, g: &Geometry, binding: &str) -> bool {
        let (name, args) = match xirius_prefs::cmdline::parse(binding, Some(self.song.registry())) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e);
                return true;
            }
        };
        // An older name or spelling of the command runs as the current one.
        let (name, args) = {
            let r = self.song.registry();
            let name = r.canonical(&name).to_string();
            let args = r
                .get(&name)
                .and_then(|d| xirius_core::Registry::spelled(d, &args).ok())
                .unwrap_or(args);
            (name, args)
        };
        self.run_parsed(g, name, args)
    }

    /// Runs a command by its (current) name and arguments: what a typed binding, a key, the
    /// palette and a row of a menu all come to.
    pub(crate) fn run_parsed(
        &mut self,
        g: &Geometry,
        name: String,
        args: Map<String, Value>,
    ) -> bool {
        if self.sound_preparing() {
            return true;
        }
        if self.opening.is_some() {
            if name == "app.quit" {
                self.request_close();
            } else if matches!(
                name.as_str(),
                "project.open" | "project.new" | "version.open"
            ) {
                self.warn(crate::opening::BUSY.into(), None);
            }
            return true;
        }
        if name == "macro.run"
            && let Some(m) = args.get("name").and_then(Value::as_str)
        {
            let m = m.to_string();
            return self.run_macro(g, &m);
        }
        if let Some(used) = self.quant_command(g, &name, &args) {
            return used;
        }
        if let Some(used) = self.key_more(g, &name, &args) {
            return used;
        }
        if let Some(used) = self.heads_key(g, &name, &args) {
            return used;
        }
        if let Some(used) = self.key_row(g, &name, &args) {
            return used;
        }
        if let Some(used) = self.screen_command(g, &name, &args) {
            return used;
        }
        if let Some(tool) = match name.as_str() {
            "tool.select" => Some(Tool::Select),
            "tool.range" => Some(Tool::Range),
            "tool.cut" => Some(Tool::Cut),
            "tool.erase" => Some(Tool::Eraser),
            "tool.draw" => Some(Tool::Draw),
            "tool.mute" => Some(Tool::Mute),
            "tool.listen" => Some(Tool::Audition),
            _ => None,
        } {
            self.arrow_chosen(g, tool);
            self.tool = tool;
            return true;
        }
        match name.as_str() {
            "edit.nudge.left" | "edit.nudge.right"
                if self.arrange.focus && !self.arrange.points.is_empty() =>
            {
                return self.automation_point_nudge(name == "edit.nudge.right");
            }
            "view.context-menu" => {
                return self.key_context(g);
            }
            "view.palette" => {
                self.open_palette(g);
                return true;
            }
            "view.settings" => {
                self.open_settings(crate::settings::Section::General);
                return true;
            }
            // The key opens the song's settings as 「曲の設定」 does (KEY-012).
            "project.settings" if args.is_empty() => {
                self.popup = None;
                return self.open_palette_form("project.settings", json!({}));
            }
            "history.undo" | "history.redo" => {
                self.roll.cancel();
                self.sound_roll_audition();
                let moved = if name == "history.undo" {
                    self.automation_history(false)
                } else {
                    self.automation_history(true)
                };
                if moved {
                    self.after_edit();
                    self.rebuild(g);
                }
                return moved;
            }
            "project.new" => {
                self.start_leaving(crate::saving::Leave::New);
                return true;
            }
            "project.open" if args.contains_key("path") => {
                return self.open_command(&name, Value::Object(args.clone()));
            }
            "project.open" => {
                self.file_request = Some(FileRequest::Open);
                return true;
            }
            "project.save" | "project.save-as" => {
                if name == "project.save-as" || self.song.dir().is_none() {
                    self.file_request = Some(FileRequest::SaveAs);
                    return true;
                }
                return self.start_save();
            }
            "transport.start" => {
                if !self.transport.playing {
                    self.toggle_play();
                }
                return true;
            }
            "transport.play" => return self.toggle_play(),
            "transport.stop" => return self.stop_transport(),
            "transport.seek" => {
                // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
                self.seek(self.tick_arg(args.get("at")).unwrap_or(0));
                return true;
            }
            "record.start" => return self.toggle_record(),
            "record.stop" => {
                if self.transport.take.is_some() {
                    self.toggle_record();
                }
                return true;
            }
            "transport.loop" => return self.toggle_loop(),
            "transport.metronome" => return self.toggle_metronome(),
            "view.snap" => {
                self.command(|s| s.set_snap(!s.snap()));
                return true;
            }
            "view.autoscroll" => {
                self.toggle_follow();
                return true;
            }
            "view.mixer" => {
                self.drawer_open = !self.drawer_open || self.tab != DrawerTab::Mixer;
                self.tab = DrawerTab::Mixer;
                self.resized(g);
                return true;
            }
            "view.record-panel" => {
                self.toggle_rec_panel(g);
                return true;
            }
            // The key picks the files as 「ファイルから…」 does.
            "clip.import" if !args.contains_key("file") => {
                self.popup = None;
                self.file_request = Some(FileRequest::Audio);
                return true;
            }
            // The key asks for the name as the saved versions' pop-over does.
            "version.save" if !args.contains_key("name") => {
                if self.song.dir().is_none() {
                    return false;
                }
                self.press(g, crate::studio::Hit::Bar(crate::topbar::BarButton::Saved));
                return self.popup.is_some()
                    && self.popup_action(g, crate::popup::Action::NameVersion);
            }
            "view.browser" => {
                // With a tab, that tab shows and the browser stays open (BRW-004).
                match args
                    .get("tab")
                    .and_then(Value::as_str)
                    .map(crate::browser::RailTab::from_name)
                {
                    Some(Some(tab)) => {
                        if !(self.browser.open && self.browser.tab == tab) {
                            self.browser.press_rail(tab);
                        }
                    }
                    Some(None) => return false,
                    None => self.browser.open = !self.browser.open,
                }
                self.resized(g);
                return true;
            }
            "view.add-track" => return self.press(g, crate::studio::Hit::AddTrack),
            "view.tracklist.collapse" => {
                self.toggle_tracklist(g);
                return true;
            }
            "view.instrument" => {
                let plugin = self.selected_row.and_then(|row| match row {
                    RowKind::Track(t) => {
                        crate::instrument::current(&self.project().tracks[&t]).map(|p| (t, p.id))
                    }
                    _ => None,
                });
                return plugin.is_some_and(|(t, id)| self.open_plugin(t, id));
            }
            "view.export-run" => {
                return self
                    .popup
                    .as_ref()
                    .is_some_and(|p| p.kind == crate::popup::PopupKind::Export)
                    && self.export_action(crate::export::Act::Go);
            }
            "export.selection" => return self.open_selection_export(g),
            "export.open" | "view.export-stems" => {
                self.export.set_stems(name == "view.export-stems");
                self.open_export(self.export_anchor(g));
                return true;
            }
            "transport.locate-mouse" => {
                // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
                self.seek(self.tick_at(g, self.input_map.pointer.0).max(0));
                return true;
            }
            "loop.selection" | "transport.locate-selection" => {
                if let Some((start, end)) = self.selected_time_range(g) {
                    if name == "loop.selection" {
                        // A section already lies on the grid it was made on.
                        let no_snap = args.get("no-snap") == Some(&json!(true))
                            || self.arrange.focus && self.selected_section().is_some();
                        let (start, end) = if no_snap {
                            (start, end)
                        } else {
                            let (snap, m) = (self.tools().snap, self.meter());
                            if snap.on {
                                (snap.floor(&m, start), snap.ceil(&m, end))
                            } else {
                                (start, end)
                            }
                        };
                        return self.run_command("loop.set", json!({"start":start,"end":end}));
                    }
                    // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
                    self.seek(start);
                    return true;
                }
                return false;
            }
            "view.zoom.rect" => {
                self.input_map.zoom_rect_armed = true;
                return true;
            }
            "view.lanes" => {
                if let Some(RowKind::Track(t)) = self.selected_row {
                    self.toggle_lanes(g, t);
                }
                return true;
            }
            "auto.mode" => {
                if let Some(RowKind::Track(t)) = self.selected_row {
                    let mut mode = args.get("mode").and_then(Value::as_str).unwrap_or("read");
                    if mode == "read"
                        && self.project().tracks[&t].auto_mode == xirius_core::auto::AutoMode::Read
                    {
                        mode = "off";
                    }
                    return self
                        .run_command("auto.mode", json!({"ids":[t.to_string()],"mode":mode}));
                }
                return false;
            }
            "auto.lane.next" => {
                if let Some(RowKind::Track(t)) = self.selected_row {
                    let p = self.project();
                    let lanes = self.arrange.lanes_of(&p.tracks[&t]);
                    let targets = crate::arrange::targets_of(&p.tracks[&t]);
                    if !targets.is_empty() {
                        let n = targets.len();
                        self.input_map.parameter = (self.input_map.parameter
                            + if args.get("back") == Some(&json!(true)) {
                                n - 1
                            } else {
                                1
                            })
                            % n;
                        let target = targets[self.input_map.parameter].to_string();
                        let first = lanes.first().map(|lane| lane.target.to_string());
                        let existing = lanes
                            .iter()
                            .position(|lane| lane.target == targets[self.input_map.parameter]);
                        drop(p);
                        if let Some(from) = first.filter(|_| existing.is_none()) {
                            self.run_command(
                                "auto.lane.target",
                                json!({"track":t.to_string(),"target":from,"to":target}),
                            );
                        } else if existing.is_none() {
                            self.run_command(
                                "auto.lane.add",
                                json!({"track":t.to_string(),"target":target}),
                            );
                        }
                        self.arrange.open.insert(t);
                        self.arrange.touched();
                        self.rebuild(g);
                        if let Some(row) = self.tree().row_of(RowKind::Track(t)) {
                            self.scroll.1 =
                                row.y + row.h + existing.unwrap_or(0) as f32 * crate::tree::LANE_H;
                        }
                    }
                }
                return true;
            }
            _ => {}
        }
        if name.starts_with("view.zoom.") {
            return self.zoom_command(g, &name, &args);
        }
        let original = args.get("at").and_then(Value::as_str) == Some("original");
        if matches!(name.as_str(), "edit.delete" | "clip.delete")
            && let Some((command, args)) = self.input_map.context_delete.take()
        {
            self.arrange_run(command, args);
            self.rebuild(g);
            return true;
        }
        if matches!(name.as_str(), "edit.copy" | "edit.cut") && self.input_context(g) == "mixer" {
            // A plugin picked with the right button goes as a plugin; else a channel selected
            // copies its settings (MIX-007).
            let picked = self.input_map.context_clipboard.clone();
            let channel = if self.mixer.master_picked {
                Some(TrackId::MASTER)
            } else {
                self.selected_row.and_then(|r| match r {
                    RowKind::Track(t) => Some(t),
                    _ => None,
                })
            };
            let used = match (picked, channel) {
                (Some(args), _) => self.arrange_run(&name, args).is_some(),
                (None, Some(t)) if name == "edit.copy" => self
                    .arrange_run("channel.settings.copy", json!({"id": t.to_string()}))
                    .is_some(),
                (None, Some(t)) => self
                    .arrange_run(&name, json!({"tracks":[t.to_string()]}))
                    .is_some(),
                (None, None) => false,
            };
            if used || channel.is_some() {
                self.rebuild(g);
                return true;
            }
        }
        if name == "marker.add" && !args.contains_key("at") {
            return self
                .arrange_run("marker.add", json!({"at": self.playhead}))
                .is_some();
        }
        if name == "clip.delete" && self.arrange.focus {
            if let Some(id) = self.input_map.tempo_selected {
                let used = self
                    .arrange_run("tempo.delete", json!({"id": id.to_string()}))
                    .is_some();
                if used {
                    self.input_map.tempo_selected = None;
                }
                self.rebuild(g);
                return true;
            }
            if let Some(id) = self.input_map.marker_selected.take() {
                self.arrange_run("marker.delete", json!({"ids": [id.to_string()]}));
                self.rebuild(g);
                return true;
            }
        }
        if let Some(used) = self.section_command(g, &name) {
            return used;
        }
        if let Some(used) = self.clip_command(g, &name, original) {
            return used;
        }
        if name == "clip.bounce"
            && let Some((command, bounce)) =
                self.bounce_instead(args.get("new-track") == Some(&Value::Bool(true)))
        {
            self.arrange_run(command, bounce);
            self.rebuild(g);
            return true;
        }
        if !self.arrange.focus
            && let Some(ctx) = self.roll_input(g)
        {
            let out = self.roll.command(&ctx, &name, &args);
            if out.used {
                return self.apply_roll(g, out);
            }
        }
        let Some(def) = self.song.registry().get(&name).cloned() else {
            self.error = Some(format!("「{name}」というコマンドはありません"));
            return true;
        };
        let mut filled = match self.fill(g, &def) {
            Fill::Now(v) | Fill::Later(Some(v)) => v.as_object().cloned().unwrap_or_default(),
            _ => Map::new(),
        };
        // Fill targets even when explicit arguments supply other required values.
        for arg in &def.args {
            let kind = match arg.ty {
                xirius_core::command::ArgType::Ids(k) | xirius_core::command::ArgType::Id(k) => {
                    Some(k)
                }
                _ => None,
            };
            if let Some(kind) =
                kind.filter(|_| arg.required && matches!(arg.name, "ids" | "id" | "track" | "clip"))
            {
                let ids = self.targets(g, kind);
                if !ids.is_empty() {
                    filled.insert(
                        arg.name.into(),
                        if matches!(arg.ty, xirius_core::command::ArgType::Ids(_)) {
                            json!(ids)
                        } else {
                            json!(ids[0])
                        },
                    );
                }
            }
        }
        if name == "clip.split" {
            filled.insert("at".into(), json!(self.playhead));
        }
        self.key_row_defaults(g, &name, &mut filled);
        filled.extend(args);
        if name == "version.open"
            || (name == "ai.decide" && self.song.session().is_open_decision(&filled))
        {
            return self.open_command(&name, Value::Object(filled));
        }
        let ran = if let Some(data) = self.arrange_run(&name, Value::Object(filled)) {
            if name == "clip.duplicate" {
                self.select_made(&data);
            }
            if name == "clip.delete" {
                self.arrange.clips.clear();
            }
            true
        } else {
            false
        };
        self.rebuild(g);
        ran
    }

    pub(crate) fn zoom_snapshot(&self) -> Zoom {
        let roll = !self.arrange.focus && self.drawer_open && self.tab == DrawerTab::Roll;
        if roll {
            Zoom {
                time: self.roll.beat_px,
                data: self.roll.row_h,
                scroll: self.roll.scroll,
                roll,
            }
        } else {
            Zoom {
                time: self.beat_w,
                data: self.arrange.track_h,
                scroll: self.scroll,
                roll,
            }
        }
    }

    pub(crate) fn restore_zoom(&mut self, g: &Geometry, z: Zoom) {
        if z.roll {
            self.roll.beat_px = z.time;
            self.roll.row_h = z.data;
            self.roll.scroll = z.scroll;
        } else {
            self.beat_w = z.time;
            self.arrange.track_h = z.data;
            self.scroll = z.scroll;
            self.arrange.touched();
        }
        self.rebuild(g);
    }

    pub fn zoom_command(&mut self, g: &Geometry, name: &str, args: &Map<String, Value>) -> bool {
        let before = self.zoom_snapshot();
        if name == "view.zoom.selection" && args.get("horizontal") != Some(&json!(true)) {
            if let Some(previous) = self
                .input_map
                .selection
                .take()
                .filter(|z| z.roll == before.roll)
            {
                self.remember_zoom(before);
                self.restore_zoom(g, previous);
                return true;
            }
            self.input_map.selection = Some(before);
        }
        match name {
            "view.zoom.undo" => {
                if let Some(z) = self.input_map.undo.pop() {
                    self.input_map.redo.push(before);
                    self.restore_zoom(g, z);
                }
                return true;
            }
            "view.zoom.redo" => {
                if let Some(z) = self.input_map.redo.pop() {
                    self.input_map.undo.push(before);
                    self.restore_zoom(g, z);
                }
                return true;
            }
            "view.zoom.store" => {
                self.input_map.stored = Some(before);
                return true;
            }
            "view.zoom.toggle" => {
                if let Some(z) = self.input_map.stored.replace(before) {
                    self.restore_zoom(g, z);
                }
                return true;
            }
            _ => {}
        }
        let number = |key: &str| {
            args.get(key)
                .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
                .map(|v| v as f32)
        };
        let by = number("by").unwrap_or(1.0);
        if name == "view.zoom.wave" {
            match (number("to"), number("by")) {
                (Some(to), _) => self.set_wave_zoom(to),
                (None, Some(by)) => {
                    self.step_wave_zoom(by.round() as i32);
                }
                (None, None) => self.cycle_wave_zoom(),
            }
            return true;
        }
        if name == "view.zoom.time" {
            if before.roll {
                let ctx = self.roll_ctx(g);
                match (ctx, number("to")) {
                    (Some(ctx), Some(to)) => self.roll.zoom_to(&ctx, to),
                    (Some(ctx), None) => self.roll.zoom_by(&ctx, by),
                    (None, to) => {
                        self.roll.beat_px = to
                            .unwrap_or(before.time * crate::roll::ZOOM_STEP.powf(by))
                            .clamp(crate::roll::BEAT_PX_MIN, crate::roll::BEAT_PX_MAX);
                    }
                }
            } else if let Some(to) = number("to") {
                self.beat_w = to.clamp(Self::BEAT_W_MIN, self.beat_w_max());
                self.clamp_scroll(g);
            } else {
                self.zoom(g, g.lanes.x, by);
            }
        } else if name == "view.zoom.data" {
            if before.roll {
                self.roll.row_h =
                    (before.data + by * 2.0).clamp(crate::roll::ROW_H_MIN, crate::roll::ROW_H_MAX);
            } else {
                self.arrange.track_h = (before.data + by * crate::arrange::Arrange::TRACK_H_STEP)
                    .clamp(
                        crate::arrange::Arrange::TRACK_H_MIN,
                        crate::arrange::Arrange::TRACK_H_MAX,
                    );
                self.arrange.touched();
            }
        } else {
            let p = self.project();
            let range = if name == "view.zoom.loop" {
                p.loop_range.as_ref().map(|r| (r.start, r.end))
            } else if name == "view.zoom.selection" {
                if before.roll {
                    self.roll_ctx(g).and_then(|ctx| {
                        let ns = self.roll.selected_notes(&ctx);
                        Some((
                            ns.iter().map(|n| n.start).min()?,
                            ns.iter().map(|n| n.start + n.length).max()?,
                        ))
                    })
                } else {
                    let ids = self.targets(g, IdKind::Clip);
                    let cs: Vec<_> = p
                        .tracks
                        .values()
                        .flat_map(|t| &t.clips)
                        .filter(|c| ids.contains(&c.id.to_string()))
                        .collect();
                    Some((
                        cs.iter().map(|c| c.start).min().unwrap_or(0),
                        cs.iter()
                            .map(|c| c.start + c.length)
                            .max()
                            .unwrap_or(i64::from(p.ppq) * 4),
                    ))
                }
            } else if before.roll {
                self.roll_ctx(g).map(|ctx| (0, ctx.clip.length))
            } else {
                Some((
                    0,
                    p.tracks
                        .values()
                        .flat_map(|t| &t.clips)
                        .map(|c| c.start + c.length)
                        .max()
                        .unwrap_or(i64::from(p.ppq) * 4),
                ))
            };
            if let Some((start, end)) = range {
                let w = if before.roll {
                    self.roll.frame(g.drawer_body).grid.w
                } else {
                    g.lanes.w
                };
                let time = w * p.ppq as f32 / (end - start).max(1) as f32;
                let time = if before.roll {
                    time.clamp(crate::roll::BEAT_PX_MIN, crate::roll::BEAT_PX_MAX)
                } else {
                    time.clamp(Self::BEAT_W_MIN, self.beat_w_max())
                };
                if before.roll {
                    self.roll.beat_px = time;
                    self.roll.scroll.0 = start as f32 / p.ppq as f32 * time;
                } else {
                    self.beat_w = time;
                    self.scroll.0 = start as f32 / p.ppq as f32 * time;
                }
            }
        }
        if name == "view.zoom.selection" && args.get("horizontal") != Some(&json!(true)) {
            self.fit_selected_data(g, before.roll);
        }
        if before != self.zoom_snapshot() {
            self.remember_zoom(before);
        }
        self.rebuild(g);
        true
    }

    pub(crate) fn remember_zoom(&mut self, z: Zoom) {
        self.input_map.undo.push(z);
        self.input_map.redo.clear();
    }

    pub fn selected_time_range(&self, g: &Geometry) -> Option<(i64, i64)> {
        if !self.arrange.focus
            && let Some(ctx) = self.roll_ctx(g)
        {
            let ns = self.roll.selected_notes(&ctx);
            return Some((
                ctx.clip.start + ns.iter().map(|n| n.start).min()?,
                ctx.clip.start + ns.iter().map(|n| n.start + n.length).max()?,
            ));
        }
        if let Some(s) = self.selected_section() {
            return Some((s.start, s.start + s.length));
        }
        let p = self.project();
        let ids = self.targets(g, IdKind::Clip);
        let cs: Vec<_> = p
            .tracks
            .values()
            .flat_map(|t| &t.clips)
            .filter(|c| ids.contains(&c.id.to_string()))
            .collect();
        Some((
            cs.iter().map(|c| c.start).min()?,
            cs.iter().map(|c| c.start + c.length).max()?,
        ))
    }

    fn fit_selected_data(&mut self, g: &Geometry, roll: bool) {
        if roll {
            if let Some(ctx) = self.roll_ctx(g) {
                let notes = self.roll.selected_notes(&ctx);
                if let (Some(low), Some(high)) = (
                    notes.iter().map(|n| n.pitch).min(),
                    notes.iter().map(|n| n.pitch).max(),
                ) {
                    let rows = f32::from(high - low + 3);
                    self.roll.row_h = (self.roll.frame(ctx.body).grid.h / rows)
                        .clamp(crate::roll::ROW_H_MIN, crate::roll::ROW_H_MAX);
                    self.roll.scroll.1 =
                        f32::from(127 - high.saturating_add(1).min(127)) * self.roll.row_h;
                }
            }
        } else {
            let rows:Vec<_>=self.tree().rows.iter().filter(|r|matches!(r.kind,RowKind::Track(t) if self.arrange.clips.iter().any(|(id,_)|*id==t))).copied().collect();
            if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
                let factor = g.lanes.h / (last.y + last.h - first.y).max(1.0);
                self.arrange.track_h = (self.arrange.track_h * factor).clamp(
                    crate::arrange::Arrange::TRACK_H_MIN,
                    crate::arrange::Arrange::TRACK_H_MAX,
                );
                self.scroll.1 = first.y * factor;
                self.arrange.touched();
            }
        }
    }
}

fn physical(key: &str) -> Option<String> {
    let named = match key {
        "/" => Some("Slash"),
        "=" | "+" => Some("Equal"),
        "-" => Some("Minus"),
        "," => Some("Comma"),
        "." => Some("Period"),
        "[" => Some("BracketLeft"),
        "]" => Some("BracketRight"),
        ";" => Some("Semicolon"),
        "'" => Some("Quote"),
        "\\" => Some("Backslash"),
        "`" => Some("Backquote"),
        _ => None,
    };
    if let Some(name) = named {
        return Some(format!("code:{name}"));
    }
    if key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() {
        return Some(format!("code:Key{}", key.to_ascii_uppercase()));
    }
    if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() {
        return Some(format!("code:Digit{key}"));
    }
    None
}

#[cfg(test)]
mod tests;
