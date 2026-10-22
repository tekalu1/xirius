//! Apply preferences after a local command or an external edit, outside song history.
use crate::studio::Studio;
use crate::tools::SnapMode;

impl Studio {
    pub fn apply_edit_prefs(&mut self) -> bool {
        let prefs = self
            .song
            .settings()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .prefs
            .clone();
        let area = if self.arrange.focus {
            "arrange"
        } else {
            "roll"
        };
        let area_changed = self.input_map.snap_area != Some(area);
        if self.input_map.prefs.as_ref() == Some(&prefs) && !area_changed {
            return false;
        }
        self.auto_quantize = prefs.bool("edit.quantize.auto");
        self.crosshair = prefs.bool("edit.cursor.crosshair");
        self.value_tip = prefs.bool("edit.drag.value-tip");
        self.roll.audition_edits = prefs.bool("edit.audition");
        self.arrange.click_locates = prefs.bool("edit.click-empty.locate");
        self.arrange.follows_clips = prefs.bool("edit.automation.follows-clips");
        self.arrange.locks_clips = prefs.bool("edit.automation.locks-clips");
        self.follow.mode = match prefs.text("edit.autoscroll").as_str() {
            "center" => crate::follow::Mode::ContinuousCenter,
            "left" => crate::follow::Mode::ContinuousLeft,
            _ => crate::follow::Mode::TurnOver,
        };
        if prefs.text("edit.draw.velocity") != "last" {
            self.roll.next_vel = prefs.number("edit.draw.velocity-value") as u8;
        }

        let old = self.input_map.prefs.as_ref();
        if old.is_none_or(|old| old.get("edit.arrow.mode") != prefs.get("edit.arrow.mode")) {
            // Basic unless the setting says extended (the default became basic on 2026-10-21;
            // a file that saved "extended" keeps it).
            self.roll.extended = prefs.text("edit.arrow.mode") == "extended";
        }
        // Link (the upper half of a clip selects a range) follows the setting when it changes,
        // and the setting turned off keeps it off when the window opens; the toolbar button
        // still turns it on and off for the song.
        let key = "edit.arrange.link";
        let changed = old.is_some_and(|old| old.get(key) != prefs.get(key));
        if changed || (old.is_none() && !prefs.bool(key)) {
            self.song.session_mut().state.view.link = prefs.bool(key);
        }
        for editor in ["arrange", "roll"] {
            let key = format!("edit.snap.{editor}.mode");
            if old.is_none_or(|old| old.get(&key) != prefs.get(&key)) {
                let mode = match prefs.text(&key).as_str() {
                    "bar" => SnapMode::Bar,
                    "quantize" => SnapMode::Quantize,
                    "frames" => SnapMode::Frames,
                    _ => SnapMode::Auto,
                };
                if editor == "roll" {
                    if old.is_some() || self.song.session().state.view.roll.mode.is_none() {
                        self.song.session_mut().state.view.roll.mode = Some(
                            match mode {
                                SnapMode::Auto => "adaptive",
                                SnapMode::Bar => "bar",
                                SnapMode::Quantize => "quantize",
                                SnapMode::Frames => "frames",
                            }
                            .into(),
                        );
                    }
                } else {
                    self.snap_mode = mode;
                }
            }
            let key = format!("edit.snap.{editor}.grid");
            let changed = old.is_some_and(|old| old.get(&key) != prefs.get(&key));
            let configured = old.is_none() && !prefs.bool(&key);
            if changed || configured {
                let view = &mut self.song.session_mut().state.view;
                if editor == "roll" {
                    view.roll.snap = prefs.bool(&key);
                } else {
                    view.snap = prefs.bool(&key);
                }
            }
        }
        self.choose_editor();
        self.input_map.snap_area = Some(area);
        self.input_map.prefs = Some(prefs);
        true
    }

    /// The value a drag shows next to the pointer: nothing with `edit.drag.value-tip` off.
    pub(crate) fn drag_tip<'a>(&self, plan: &'a crate::arrange::Plan) -> &'a str {
        if self.value_tip { &plan.tip } else { "" }
    }

    /// The roll's value balloon, as `drag_tip`.
    pub(crate) fn roll_balloon(&self) -> Option<&crate::roll::Balloon> {
        self.roll
            .preview
            .balloon
            .as_ref()
            .filter(|_| self.value_tip)
    }
}
