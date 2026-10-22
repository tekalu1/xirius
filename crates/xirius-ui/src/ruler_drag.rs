//! KEY-016: a drag up and down on the ruler zooms. `ruler.any.empty.drag` says what the drag does:
//! `view.zoom.time` (the default; the tick under the press stays where it is), `view.zoom.data`
//! (the rows' height) or `view.scroll` (the time axis follows the pointer sideways). The press
//! itself still does the ruler's click (it moves the play position), so this never takes it.
//! Down zooms in, up zooms out.

use crate::ruler as shared;
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// Pixels of a drag that are one notch of the wheel.
pub const STEP_PX: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Arrange,
    Roll,
}

/// A press on the ruler that a drag can turn into a zoom.
#[derive(Clone, Debug, PartialEq)]
pub struct Grab {
    place: Place,
    action: String,
    from: (f32, f32),
    last: (f32, f32),
    started: bool,
}

impl Grab {
    pub fn started(&self) -> bool {
        self.started
    }
}

impl Studio {
    /// The ruler under a point, below the loop band (the band's own drag is the loop's).
    fn ruler_place(&self, g: &Geometry, x: f32, y: f32) -> Option<Place> {
        if let Some(ctx) = self.roll_ctx(g) {
            let ruler = self.roll.frame(ctx.body).ruler;
            if ruler.contains(x, y) {
                // The clip's end handle is dragged for the clip's length, not the zoom.
                let handle = self.roll.end_grab(&ctx, &self.roll.frame(ctx.body), x, y);
                return (!shared::on_band(ruler, y) && !handle).then_some(Place::Roll);
            }
        }
        (g.ruler.contains(x, y) && !shared::on_band(g.ruler, y)).then_some(Place::Arrange)
    }

    /// A press at (x, y) with `mods` held. Never takes the press: the click goes on.
    pub fn ruler_drag_down(&mut self, g: &Geometry, x: f32, y: f32, mods: Mods) {
        self.ruler_grab = None;
        if self.popup.is_some() || self.palette.is_some() {
            return;
        }
        let Some(place) = self.ruler_place(g, x, y) else {
            return;
        };
        let action = {
            let s = self
                .song
                .settings()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            crate::mouse_dispatch::action(&s.mousemap, "ruler.any.empty.drag", mods)
        };
        let Some(action) = action.filter(|a| !a.is_empty()) else {
            return;
        };
        self.ruler_grab = Some(Grab {
            place,
            action,
            from: (x, y),
            last: (x, y),
            started: false,
        });
    }

    /// The pointer moved with the button down. True when the view changed.
    pub fn ruler_drag_move(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(grab) = &mut self.ruler_grab else {
            return false;
        };
        if !grab.started
            && (x - grab.from.0).abs().max((y - grab.from.1).abs()) <= crate::roll::SLOP
        {
            return false;
        }
        let (dx, dy) = (x - grab.last.0, y - grab.last.1);
        grab.started = true;
        grab.last = (x, y);
        let (place, at) = (grab.place, grab.from);
        let action = grab.action.clone();
        let steps = dy / STEP_PX;
        match (place, action.as_str()) {
            (Place::Arrange, "view.zoom.time") => self.zoom(g, at.0, steps),
            (Place::Arrange, "view.zoom.data") => {
                self.arrange_wheel(g, g.lanes.x + 1.0, g.lanes.y + 1.0, steps, Mods::ctrl());
            }
            (Place::Arrange, "view.scroll") => {
                self.scroll.0 -= dx;
                self.clamp_scroll(g);
            }
            (Place::Roll, _) => {
                let Some(ctx) = self.roll_ctx(g) else {
                    return false;
                };
                // The roll's wheel counts in the same notch the arrangement's does.
                let notch = steps * crate::screen::ZOOM_NOTCH_PX;
                match action.as_str() {
                    "view.zoom.time" => {
                        self.roll
                            .wheel(&ctx, at.0, at.1, 0.0, notch, Mods::ctrl_shift());
                    }
                    "view.zoom.data" => {
                        let top = self.roll.frame(ctx.body).grid.y;
                        self.roll.wheel(&ctx, at.0, top, 0.0, notch, Mods::ctrl());
                    }
                    "view.scroll" => {
                        self.roll.scroll.0 -= dx;
                        self.roll.clamp_scroll(&ctx);
                    }
                    _ => return false,
                }
            }
            _ => return false,
        }
        true
    }

    /// The button came up. True when the drag had zoomed (or moved) the view.
    pub fn ruler_drag_up(&mut self) -> bool {
        self.ruler_grab.take().is_some_and(|g| g.started)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 860.0);
        s.resized(&g);
        s.drawer_open = false;
        (s, g)
    }

    /// A point on the ruler under the loop band.
    fn below_band(g: &Geometry) -> (f32, f32) {
        (g.ruler.x + 300.0, g.ruler.y + g.ruler.h - 4.0)
    }

    fn map(s: &mut Studio, action: &str) {
        s.song
            .run(
                "mousemap.set",
                json!({"context":"ruler.any.empty.drag","modifier":"none","action":action}),
            )
            .unwrap();
    }

    #[test]
    fn key_016_a_drag_down_on_the_ruler_zooms_in_around_the_press() {
        let (mut s, g) = setup();
        let (x, y) = below_band(&g);
        let before = s.beat_w;
        let tick = s.px_to_ticks(x - g.lanes.x + s.scroll.0);
        let steps = s.song.steps();
        s.ruler_drag_down(&g, x, y, Mods::NONE);
        assert!(s.ruler_grab.is_some());
        assert!(
            !s.ruler_drag_move(&g, x, y + 2.0),
            "inside the slop nothing moves"
        );
        assert!(s.ruler_drag_move(&g, x, y + 60.0));
        assert!(s.beat_w > before);
        // The tick under the press stays under it, unless the scroll ran out.
        let now = s.px_to_ticks(x - g.lanes.x + s.scroll.0);
        let drift = (s.tick_x(&g, tick) - x).abs();
        assert!(drift < 2.0 || s.scroll.0 == 0.0, "{tick} {now} {drift}");
        assert!(s.ruler_drag_up());
        assert_eq!(s.song.steps(), steps, "a zoom never edits the song");
        // Up zooms out.
        let zoomed = s.beat_w;
        s.ruler_drag_down(&g, x, y, Mods::NONE);
        s.ruler_drag_move(&g, x, y - 80.0);
        assert!(s.beat_w < zoomed);
        s.ruler_drag_up();
    }

    #[test]
    fn key_016_the_loop_band_and_a_press_that_does_not_move_are_not_a_zoom() {
        let (mut s, g) = setup();
        let (x, _) = below_band(&g);
        let before = s.beat_w;
        s.ruler_drag_down(&g, x, g.ruler.y + 2.0, Mods::NONE);
        assert!(s.ruler_grab.is_none(), "the band's drag is the loop's");
        s.ruler_drag_down(&g, x, g.ruler.y + g.ruler.h - 4.0, Mods::NONE);
        assert!(!s.ruler_drag_up(), "a click is not a drag");
        assert_eq!(s.beat_w, before);
        // Outside the ruler, nothing is taken.
        s.ruler_drag_down(&g, g.lanes.x + 50.0, g.lanes.y + 50.0, Mods::NONE);
        assert!(s.ruler_grab.is_none());
    }

    #[test]
    fn key_016_the_map_says_what_the_ruler_drag_does() {
        let (mut s, g) = setup();
        let (x, y) = below_band(&g);
        map(&mut s, "view.scroll");
        let beat = s.beat_w;
        s.scroll.0 = 200.0;
        s.ruler_drag_down(&g, x, y, Mods::NONE);
        s.ruler_drag_move(&g, x - 60.0, y);
        assert_eq!(s.beat_w, beat);
        assert!(s.scroll.0 > 200.0, "the time axis follows the pointer");
        s.ruler_drag_up();
        map(&mut s, "view.zoom.data");
        s.ruler_drag_down(&g, x, y, Mods::NONE);
        s.ruler_drag_move(&g, x, y + 60.0);
        assert_eq!(s.beat_w, beat, "the rows' height, not the time axis");
        s.ruler_drag_up();
        map(&mut s, "");
        s.ruler_drag_down(&g, x, y, Mods::NONE);
        assert!(s.ruler_grab.is_none(), "taken away on purpose");
    }

    #[test]
    fn key_016_the_press_on_the_ruler_still_moves_the_play_position() {
        let (mut s, g) = setup();
        let (x, y) = below_band(&g);
        let hit = s.hit(&g, x, y);
        assert!(matches!(hit, crate::studio::Hit::Ruler(_)), "{hit:?}");
        s.ruler_drag_down(&g, x, y, Mods::NONE);
        s.press(&g, hit);
        assert_eq!(s.playhead, s.px_to_ticks(x - g.lanes.x + s.scroll.0).max(0));
    }

    #[test]
    fn key_016_the_roll_ruler_zooms_too() {
        let (mut s, g) = setup();
        s.drawer_open = true;
        s.tab = crate::studio::DrawerTab::Roll;
        s.resized(&g);
        let Some(ctx) = s.roll_ctx(&g) else {
            panic!("the sample opens a roll");
        };
        let ruler = s.roll.frame(ctx.body).ruler;
        let (x, y) = (ruler.x + 200.0, ruler.y + ruler.h - 3.0);
        let before = s.roll.view(&ctx).beat_px;
        s.ruler_drag_down(&g, x, y, Mods::NONE);
        assert!(s.ruler_grab.is_some());
        assert!(s.ruler_drag_move(&g, x, y + 60.0));
        assert!(s.roll.view(&ctx).beat_px > before);
        s.ruler_drag_up();
    }
}
