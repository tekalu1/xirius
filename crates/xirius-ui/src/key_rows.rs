//! The key rows that need the screen's playhead, snap grid or selected range (KEY-001, OPS-014):
//! jumping to a marker or the loop, setting the loop at the playhead, nudging and cutting on the
//! snap grid, and moving clips to the cursor.
use serde_json::{Map, Value, json};

use crate::studio::{Geometry, Studio};
use xirius_core::nav;

impl Studio {
    /// Runs the rows that act on the playhead directly. `None` means the row is not one of these.
    pub(crate) fn key_row(
        &mut self,
        g: &Geometry,
        name: &str,
        args: &Map<String, Value>,
    ) -> Option<bool> {
        match name {
            "select.step" => Some(self.select_step(g, args)),
            // ←→ in the roll (UI-088): the selected notes move; with none, the selection steps.
            "edit.nudge.left" | "edit.nudge.right"
                if args.contains_key("grid")
                    && !self.arrange.focus
                    && self.roll.points.is_empty() =>
            {
                Some(self.arrow_nudge(g, name, args))
            }
            // ↑↓ in the roll (UI-284): the selected notes move a semitone; with none, the
            // selection steps.
            "note.transpose" if !self.arrange.focus && matches!(semitone(args), Some(1 | -1)) => {
                Some(self.arrow_transpose(g, args))
            }
            "transport.goto" => {
                let to = args.get("to").and_then(Value::as_str).unwrap_or_default();
                match xirius_core::nav::destination(&self.project(), self.playhead, to) {
                    Ok(at) => {
                        self.error = None;
                        // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
                        self.seek(at);
                        Some(true)
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        Some(false)
                    }
                }
            }
            "loop.start.set" | "loop.end.set" if !args.contains_key("at") => Some(
                self.arrange_run(name, json!({"at": self.playhead}))
                    .is_some(),
            ),
            _ => None,
        }
    }

    /// An arrow key, Home or End: the selection moves to the next clip (or note, in the roll).
    fn arrow_nudge(&mut self, g: &Geometry, name: &str, args: &Map<String, Value>) -> bool {
        if let Some(ctx) = self.roll_input(g) {
            let out = self.roll.command(&ctx, name, args);
            if out.used {
                return self.apply_roll(g, out);
            }
        }
        let dir = if name == "edit.nudge.right" {
            "right"
        } else {
            "left"
        };
        let mut step = Map::new();
        step.insert("dir".into(), json!(dir));
        self.select_step(g, &step)
    }

    /// ↑ or ↓: the selected notes a semitone up or down (one step of the history); with none,
    /// the selection moves to the note above or below.
    fn arrow_transpose(&mut self, g: &Geometry, args: &Map<String, Value>) -> bool {
        if let Some(ctx) = self.roll_input(g) {
            let out = self.roll.command(&ctx, "note.transpose", args);
            if out.used {
                let shown = self.apply_roll(g, out);
                self.sound_picked_by_key(g);
                return shown;
            }
        }
        let dir = if semitone(args) == Some(1) {
            "up"
        } else {
            "down"
        };
        let mut step = Map::new();
        step.insert("dir".into(), json!(dir));
        self.select_step(g, &step)
    }

    /// A key moved or picked notes in the roll: with `edit.audition`, the top one sounds until
    /// the key comes up.
    pub(crate) fn sound_picked_by_key(&mut self, g: &Geometry) {
        if !self.roll.audition_edits {
            return;
        }
        let Some(ctx) = self.roll_ctx(g) else {
            return;
        };
        let Some(pitch) = ctx
            .notes()
            .iter()
            .filter(|n| self.roll.selected.contains(&n.id))
            .map(|n| n.pitch)
            .max()
        else {
            return;
        };
        self.roll.audition = Some(pitch);
        self.roll.audition_by_edit = true;
        self.roll.audition_by_key = true;
        self.sound_roll_audition();
    }

    /// A key came up: the note a key sounded lets go. True when one did.
    pub(crate) fn key_sound_up(&mut self) -> bool {
        if !std::mem::take(&mut self.roll.audition_by_key) {
            return false;
        }
        self.roll.audition = None;
        self.sound_roll_audition();
        true
    }

    fn select_step(&mut self, g: &Geometry, args: &Map<String, Value>) -> bool {
        let Some(to) = args
            .get("dir")
            .and_then(Value::as_str)
            .and_then(nav::Step::parse)
        else {
            self.error = Some("--dir を読み取れません".into());
            return false;
        };
        let extend = args.get("extend").and_then(Value::as_bool).unwrap_or(false);
        self.error = None;
        if !self.arrange.focus
            && let Some(ctx) = self.roll_input(g)
        {
            let spots: Vec<_> = ctx
                .notes()
                .iter()
                .map(|n| nav::Spot {
                    id: n.id,
                    x: n.start,
                    row: -i64::from(n.pitch),
                })
                .collect();
            let picked: Vec<_> = self.roll.selected.iter().copied().collect();
            if let Some(next) = nav::step(&spots, &picked, to, extend) {
                self.roll.selected = next.into_iter().collect();
                self.sound_picked_by_key(g);
            }
            return true;
        }
        if !self.arrange.points.is_empty() {
            return true;
        }
        let project = self.project();
        let mut spots = Vec::new();
        for (row, track) in crate::draw::ordered_tracks(&project)
            .into_iter()
            .enumerate()
        {
            spots.extend(track.clips.iter().map(|c| nav::Spot {
                id: c.id,
                x: c.start,
                row: row as i64,
            }));
        }
        let picked = self.picked_clips();
        if let Some(next) = nav::step(&spots, &picked, to, extend) {
            self.give_selection(&[], &next);
            self.take_clips();
        }
        true
    }

    /// The values a key row takes from the screen when the binding did not name them.
    pub(crate) fn key_row_defaults(
        &self,
        g: &Geometry,
        name: &str,
        filled: &mut Map<String, Value>,
    ) {
        match name {
            "clip.nudge" | "clip.split.grid" if !filled.contains_key("grid") => {
                let step = self.tools().snap.step(&self.meter());
                filled.insert("grid".into(), json!(step));
            }
            "clip.move.cursor" if !filled.contains_key("at") => {
                filled.insert("at".into(), json!(self.playhead));
            }
            "clip.split.range"
                if !filled.contains_key("start")
                    && !filled.contains_key("end")
                    && !filled.contains_key("range")
                    && !filled.contains_key("loop") =>
            {
                if let Some((start, end)) = self.selected_time_range(g) {
                    filled.insert("start".into(), json!(start));
                    filled.insert("end".into(), json!(end));
                }
            }
            _ => {}
        }
    }
}

/// The semitones a `note.transpose` row moves by.
fn semitone(args: &Map<String, Value>) -> Option<i64> {
    args.get("by")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_prefs::{Chord, Os};

    fn setup() -> (Studio, Geometry) {
        let mut studio = Studio::sample();
        let geometry = studio.geometry(1440.0, 860.0);
        studio.resized(&geometry);
        studio.arrange.focus = true;
        assert!(studio.song.settings().lock().unwrap().dir.is_none());
        (studio, geometry)
    }

    fn only_clip(s: &Studio) -> (i64, i64) {
        let p = s.project();
        let (t, c) = s.selected_clip.unwrap();
        let clip = &p.tracks[&t].clips.iter().find(|k| k.id == c).unwrap();
        (clip.start, clip.length)
    }

    #[test]
    fn key_001_goto_runs_from_the_playhead_and_a_miss_is_said_not_moved() {
        let (mut s, g) = setup();
        s.song.run("marker.add", json!({"at": 960})).unwrap();
        s.song.run("marker.add", json!({"at": 7680})).unwrap();
        s.seek(0);
        assert!(s.run_binding(&g, "transport.goto --to next-marker"));
        assert_eq!(s.playhead, 960);
        assert!(s.run_binding(&g, "transport.goto --to marker-2"));
        assert_eq!(s.playhead, 7680);
        assert!(s.run_binding(&g, "transport.goto --to prev-bar"));
        assert_eq!(s.playhead, 7680 - 3840);
        s.seek(7680);
        assert!(!s.run_binding(&g, "transport.goto --to next-marker"));
        assert_eq!(s.playhead, 7680);
        assert!(s.error.is_some());
    }

    #[test]
    fn key_001_loop_ends_are_set_at_the_playhead_and_one_undo_restores() {
        let (mut s, g) = setup();
        s.song
            .run("loop.set", json!({"start": 0, "end": 7680}))
            .unwrap();
        s.seek(1920);
        assert!(s.run_binding(&g, "loop.start.set"));
        assert_eq!(s.project().loop_range.unwrap().start, 1920);
        s.seek(5760);
        assert!(s.run_binding(&g, "loop.end.set"));
        assert_eq!(s.project().loop_range.unwrap().end, 5760);
        assert!(s.song.undo());
        assert_eq!(s.project().loop_range.unwrap().end, 7680);
    }

    #[test]
    fn key_001_nudge_and_split_use_the_snap_grid_and_each_is_one_step() {
        let (mut s, g) = setup();
        let (start, length) = only_clip(&s);
        let step = s.tools().snap.step(&s.meter());
        let steps = s.song.history().len();
        assert!(s.run_binding(&g, "clip.nudge --dir right"));
        assert_eq!(only_clip(&s).0, start + step);
        assert_eq!(s.song.history().len(), steps + 1);
        assert!(s.song.undo());
        assert_eq!(only_clip(&s), (start, length));
        // Split on the grid: more than one piece appears, in one step.
        let pieces = |s: &Studio| {
            s.project()
                .tracks
                .values()
                .map(|t| t.clips.len())
                .sum::<usize>()
        };
        let before = pieces(&s);
        let grid = (length / 2).max(1);
        assert!(s.run_binding(&g, &format!("clip.split.grid --grid {grid}")));
        assert!(pieces(&s) > before);
        assert!(s.song.undo());
        assert_eq!(pieces(&s), before);
    }

    #[test]
    fn key_001_move_to_cursor_puts_the_clip_at_the_playhead() {
        let (mut s, g) = setup();
        s.seek(1920);
        assert!(s.run_binding(&g, "clip.move.cursor"));
        assert_eq!(only_clip(&s).0, 1920);
        assert!(s.song.undo());
        assert_ne!(only_clip(&s).0, 1920);
    }

    #[test]
    fn key_001_split_range_takes_the_selected_range_when_it_is_not_named() {
        let (mut s, g) = setup();
        let (start, length) = only_clip(&s);
        let (from, to) = s.selected_time_range(&g).unwrap();
        assert_eq!((from, to), (start, start + length));
        // A loop inside the clip: Split Loop cuts there.
        s.song
            .run(
                "loop.set",
                json!({"start": start + length / 4, "end": start + length / 2}),
            )
            .unwrap();
        let pieces = |s: &Studio| {
            s.project()
                .tracks
                .values()
                .map(|t| t.clips.len())
                .sum::<usize>()
        };
        let before = pieces(&s);
        assert!(s.run_binding(&g, "clip.split.range --loop"));
        assert_eq!(pieces(&s), before + 2);
    }

    #[test]
    fn key_001_the_preset_keys_resolve_to_these_rows() {
        let (s, _) = setup();
        let settings = s.song.settings();
        let settings = settings.lock().unwrap();
        for (context, key, command) in [
            ("global", "Shift+N", "transport.goto --to next-marker"),
            ("global", "NumPad3", "transport.goto --to marker-1"),
            ("global", "NumPad1", "transport.goto --to loop-start"),
            ("global", "NumPad+", "transport.goto --to next-beat"),
            ("global", "Alt+NumPad2", "loop.end.set"),
            ("arrange", "Mod+G", "clip.split.grid"),
            ("arrange", "Mod+Alt+X", "clip.split.range"),
            ("arrange", "Alt+Right", "clip.nudge --dir right"),
            ("arrange", "Mod+L", "clip.move.cursor"),
            ("arrange", "Y", "marker.add"),
            ("arrange", "U", "track.monitor"),
            ("arrange", "Alt+NumPad+", "clip.gain.set --by 1"),
            ("arrange", "Alt+Shift+NumPad-", "clip.gain.set --by -0.1"),
        ] {
            let chord = Chord::parse(key, Os::Win).unwrap();
            assert_eq!(
                settings.keymap.lookup(context, &chord).map(|(_, c)| c),
                Some(command.to_string()),
                "{context} {key}"
            );
        }
    }

    fn all_clips(s: &Studio) -> Vec<(i64, i64, xirius_core::id::ClipId)> {
        let p = s.project();
        let mut v = Vec::new();
        for (row, track) in crate::draw::ordered_tracks(&p).into_iter().enumerate() {
            v.extend(track.clips.iter().map(|c| (c.start, row as i64, c.id)));
        }
        v.sort_unstable_by_key(|(x, row, _)| (*x, *row));
        v
    }

    #[test]
    fn key_001_arrow_keys_move_the_clip_selection_and_shift_extends_it() {
        let (mut s, g) = setup();
        let clips = all_clips(&s);
        assert!(clips.len() >= 2, "the sample has several clips");
        let first = clips.first().unwrap().2;
        let last = clips.last().unwrap().2;
        assert!(s.run_binding(&g, "select.step --dir start"));
        assert_eq!(s.picked_clips(), vec![first]);
        assert!(s.run_binding(&g, "select.step --dir end"));
        assert_eq!(s.picked_clips(), vec![last]);
        // Extending from the end back to the start keeps both.
        assert!(s.run_binding(&g, "select.step --dir start --extend"));
        let mut both = s.picked_clips();
        both.sort_unstable();
        let mut want = vec![first, last];
        want.sort_unstable();
        assert_eq!(both, want);
        // Selecting only changes the selection: nothing was written to the song.
        let steps = s.song.history().len();
        assert!(s.run_binding(&g, "select.step --dir down"));
        assert_eq!(s.song.history().len(), steps);
        assert!(!s.picked_clips().is_empty());
    }

    #[test]
    fn key_001_a_bad_direction_is_said_and_the_selection_stays() {
        let (mut s, g) = setup();
        assert!(s.run_binding(&g, "select.step --dir start"));
        let picked = s.picked_clips();
        assert!(!s.run_binding(&g, "select.step --dir sideways"));
        assert!(s.error.is_some());
        assert_eq!(s.picked_clips(), picked);
    }

    #[test]
    fn key_001_the_arrow_keys_are_bound_in_the_preset() {
        let (s, _) = setup();
        let settings = s.song.settings();
        let settings = settings.lock().unwrap();
        for (key, command) in [
            ("Left", "select.step --dir left"),
            ("Right", "select.step --dir right"),
            ("Up", "select.step --dir up"),
            ("Down", "select.step --dir down"),
            ("Home", "select.step --dir start"),
            ("End", "select.step --dir end"),
            ("Shift+Right", "select.step --dir right --extend"),
            ("Mod+Shift+Left", "select.step --dir left --extend"),
            ("Shift+Home", "select.step --dir start --extend"),
        ] {
            let chord = Chord::parse(key, Os::Win).unwrap();
            assert_eq!(
                settings.keymap.lookup("global", &chord).map(|(_, c)| c),
                Some(command.to_string()),
                "{key}"
            );
        }
    }
}
