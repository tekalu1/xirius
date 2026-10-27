//! The rest of the wheel and the middle button (EDT-068). The wheel already
//! scrolls (Shift: across), Ctrl + wheel already sets the rows' height (track
//! rows in the arrangement, key rows in the roll) and Ctrl + Shift + wheel the
//! time scale. What this adds: a plain wheel over the ruler zooms the time axis
//! around the pointer, and the middle button held down drags the view along
//! with the pointer, in the arrangement, the roll and the mixer.

use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The view the middle button holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanArea {
    Arrange,
    Roll,
    Mixer,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pan {
    pub area: PanArea,
    /// Where the pointer was at the last move.
    last: (f32, f32),
}

impl Studio {
    /// The middle button goes down at (x, y): the view under it is held. False
    /// over anything that does not scroll, or while something floats on top.
    pub fn pan_begin(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.popup.is_some() || self.palette.is_some() {
            return false;
        }
        let area = if self.roll_ctx(g).is_some_and(|c| c.body.contains(x, y)) {
            PanArea::Roll
        } else if self.mixer_shown() && g.drawer_body.contains(x, y) {
            PanArea::Mixer
        } else if [g.lanes, g.column, g.ruler, g.section_band, g.tempo]
            .iter()
            .any(|r| r.contains(x, y))
        {
            PanArea::Arrange
        } else {
            return false;
        };
        self.pan = Some(Pan { area, last: (x, y) });
        true
    }

    /// The pointer moves with the middle button down: the content follows it.
    /// False when nothing is held.
    pub fn pan_move(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(pan) = &mut self.pan else {
            return false;
        };
        let (dx, dy) = (x - pan.last.0, y - pan.last.1);
        if self
            .input_map
            .middle_origin
            .is_some_and(|(a, b)| (x - a).abs() + (y - b).abs() > 4.0)
        {
            self.input_map.middle_moved = true;
        }
        pan.last = (x, y);
        match pan.area {
            PanArea::Arrange => {
                self.scroll.0 -= dx;
                self.scroll.1 -= dy;
                self.clamp_scroll(g);
            }
            PanArea::Roll => {
                if let Some(ctx) = self.roll_ctx(g) {
                    self.roll.scroll.0 -= dx;
                    self.roll.scroll.1 -= dy;
                    self.roll.clamp_scroll(&ctx);
                }
            }
            PanArea::Mixer => {
                let max = self.mixer_max_scroll(g);
                self.mixer.scroll = (self.mixer.scroll - dx).clamp(0.0, max);
            }
        }
        true
    }

    /// The middle button comes up. True when it held a view.
    pub fn pan_end(&mut self) -> bool {
        self.pan.take().is_some()
    }

    /// A plain wheel over the ruler (or the section row under it) zooms the
    /// time axis around the pointer, by `steps` notches. A sideways turn still
    /// scrolls. True when it zoomed.
    pub fn ruler_wheel(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        mods: Mods,
    ) -> bool {
        let over = g.ruler.contains(x, y) || g.section_band.contains(x, y);
        // Windows sends the turn with Shift held sideways (EDT-136).
        let (dx, dy) = if mods.shift { (0.0, dy + dx) } else { (dx, dy) };
        if !over || dy == 0.0 || dy.abs() < dx.abs() {
            return false;
        }
        // `ruler.any.empty.wheel`: a map the user changed says what the wheel does; otherwise the
        // pref (`edit.wheel.ruler`) does, and only with no modifier held.
        let act = {
            let s = self
                .song
                .settings()
                // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if s.mousemap.changed.contains_key("ruler.any.empty.wheel") {
                crate::mouse_dispatch::action(&s.mousemap, "ruler.any.empty.wheel", mods)
            } else if s.prefs.text("edit.wheel.ruler") == "scroll"
                || mods.ctrl
                || mods.shift
                || mods.alt
            {
                return false;
            } else {
                Some("view.zoom.time".to_string())
            }
        };
        match act.as_deref() {
            Some("view.zoom.time") => {
                self.zoom(g, x, dy / crate::screen::ZOOM_NOTCH_PX);
                true
            }
            // Nothing is bound: the wheel is spent here. The other wheel actions are the lanes'.
            Some("") => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::DrawerTab;

    const W: f32 = 1440.0;
    const H: f32 = 820.0;

    fn studio() -> Studio {
        let mut s = Studio::sample();
        s.beat_w = 16.0;
        s
    }

    #[test]
    fn a_wheel_over_the_ruler_zooms_around_the_pointer() {
        let mut s = studio();
        let g = s.geometry(W, H);
        let x = g.ruler.x + 300.0;
        let y = g.ruler.y + g.ruler.h / 2.0;
        let tick = s.px_to_ticks(x - g.lanes.x + s.scroll.0);
        assert!(s.ruler_wheel(&g, x, y, 0.0, 120.0, Mods::NONE));
        assert!(s.beat_w > 16.0);
        let after = s.px_to_ticks(x - g.lanes.x + s.scroll.0);
        assert!((after - tick).abs() <= 1, "{tick} {after}");
        // Back out.
        assert!(s.ruler_wheel(&g, x, y, 0.0, -120.0, Mods::NONE));
        assert!((s.beat_w - 16.0).abs() < 0.01);
        // The section row zooms too.
        let band = g.section_band;
        assert!(s.ruler_wheel(&g, x, band.y + 2.0, 0.0, 60.0, Mods::NONE));
    }

    #[test]
    fn the_ruler_leaves_the_rows_shift_and_ctrl_to_the_wheel_as_before() {
        let mut s = studio();
        let g = s.geometry(W, H);
        let y = g.ruler.y + 4.0;
        let x = g.lanes.x + 100.0;
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert!(!s.ruler_wheel(&g, x, y, 0.0, 60.0, shift));
        assert!(!s.ruler_wheel(&g, x, y, 0.0, 60.0, ctrl));
        assert!(!s.ruler_wheel(&g, x, y, -60.0, 10.0, Mods::NONE));
        assert!(!s.ruler_wheel(&g, x, g.lanes.y + 20.0, 0.0, 60.0, Mods::NONE));
        assert_eq!(s.beat_w, 16.0);
    }

    #[test]
    fn the_middle_button_drags_the_arrangement() {
        let mut s = studio();
        let g = s.geometry(W, H);
        let (mx, my) = s.max_scroll(&g);
        assert!(mx > 200.0, "the sample is wider than the lanes: {mx}");
        let (x, y) = (g.lanes.x + 600.0, g.lanes.y + 40.0);
        assert!(s.pan_begin(&g, x, y));
        assert!(s.pan_move(&g, x - 200.0, y));
        assert_eq!(s.scroll.0, 200.0);
        // Dragging right past the start stops there.
        assert!(s.pan_move(&g, x + 400.0, y + 400.0));
        assert_eq!(s.scroll, (0.0, 0.0));
        assert!(s.pan_move(&g, x - 99_999.0, y - 99_999.0));
        assert_eq!(s.scroll, (mx, my));
        assert!(s.pan_end());
        assert!(!s.pan_move(&g, x, y));
        assert!(!s.pan_end());
    }

    #[test]
    fn the_middle_button_drags_the_roll_and_the_mixer() {
        let mut s = studio();
        let g = s.geometry(W, H);
        let ctx = s.roll_ctx(&g).expect("the sample opens a clip");
        let before = s.roll.scroll;
        let grid = s.roll.frame(ctx.body).grid;
        let (x, y) = (grid.x + 200.0, grid.y + 40.0);
        assert!(s.pan_begin(&g, x, y));
        assert_eq!(s.pan.unwrap().area, PanArea::Roll);
        assert!(s.pan_move(&g, x, y - 30.0));
        assert_eq!(s.roll.scroll.1, before.1 + 30.0);
        assert!(s.pan_end());
        // The arrangement did not move.
        assert_eq!(s.scroll, (0.0, 0.0));

        s.tab = DrawerTab::Mixer;
        let g = s.geometry(800.0, H);
        let max = s.mixer_max_scroll(&g);
        let (x, y) = (g.drawer_body.x + 300.0, g.drawer_body.y + 40.0);
        assert!(s.pan_begin(&g, x, y));
        assert_eq!(s.pan.unwrap().area, PanArea::Mixer);
        assert!(s.pan_move(&g, x - 50.0, y));
        assert_eq!(s.mixer.scroll, 50.0_f32.min(max));
        assert!(s.pan_end());
    }

    #[test]
    fn the_middle_button_holds_nothing_under_a_pop_over_or_on_the_top_corner() {
        let mut s = studio();
        let g = s.geometry(W, H);
        assert!(!s.pan_begin(&g, g.head.x + 2.0, g.head.y + 2.0));
        s.open_popup(
            crate::popup::PopupKind::Grid,
            crate::studio::Rect::new(600.0, 10.0, 10.0, 10.0),
        );
        assert!(!s.pan_begin(&g, g.lanes.x + 10.0, g.lanes.y + 10.0));
        assert_eq!(s.pan, None);
    }

    #[test]
    fn ctrl_and_the_wheel_set_the_track_rows_and_the_key_rows() {
        let mut s = studio();
        let g = s.geometry(W, H);
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        let track_h = s.arrange.track_h;
        assert!(s.arrange_wheel(&g, g.lanes.x + 40.0, g.lanes.y + 20.0, 1.0, ctrl));
        assert!(s.arrange.track_h > track_h);
        assert!(s.arrange_wheel(&g, g.lanes.x + 40.0, g.lanes.y + 20.0, -2.0, ctrl));
        assert!(s.arrange.track_h < track_h);

        let ctx = s.roll_ctx(&g).unwrap();
        let row_h = s.roll.row_h;
        let grid = s.roll.frame(ctx.body).grid;
        let out = s
            .roll
            .wheel(&ctx, grid.x + 50.0, grid.y + 50.0, 0.0, 60.0, ctrl);
        s.apply_roll(&g, out);
        assert!(s.roll.row_h > row_h);
        let out = s
            .roll
            .wheel(&ctx, grid.x + 50.0, grid.y + 50.0, 0.0, -120.0, ctrl);
        s.apply_roll(&g, out);
        assert!(s.roll.row_h < row_h);
    }

    #[test]
    fn the_ruler_wheel_setting_scroll_leaves_the_wheel_to_the_lanes() {
        let mut s = studio();
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "edit.wheel.ruler", "value": "scroll"}),
        );
        assert!(s.error.is_none(), "{:?}", s.error);
        let g = s.geometry(W, H);
        let x = g.ruler.x + 300.0;
        let y = g.ruler.y + g.ruler.h / 2.0;
        assert!(!s.ruler_wheel(&g, x, y, 0.0, 120.0, Mods::NONE));
        assert!((s.beat_w - 16.0).abs() < 0.01);
    }
}
