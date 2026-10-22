//! A drag that reaches the edge of the arrangement or the piano roll scrolls the view (EDT-046):
//! the further past the edge, the faster, and Space held during the drag makes it faster still
//! instead of starting the song. What the drag holds keeps to the song: the press point moves
//! with the content, and the drag is played again at the pointer after each step.

use std::time::Instant;

use crate::arrange::Spot;
use crate::studio::{Geometry, Studio};

/// How far inside the edge the scroll begins, in px.
pub const ZONE: f32 = 24.0;
/// The speed at the edge itself, in px per second.
pub const SPEED: f32 = 480.0;
/// How many times faster the scroll goes past the edge, at most.
const DEEPEST: f32 = 4.0;
/// How many times faster Space makes it.
pub const FAST: f32 = 4.0;
/// How many screens past the song's end an arrangement drag may scroll.
const PAST: f32 = 4.0;
/// The longest step one frame takes, so a stalled frame does not jump.
const LONGEST: f32 = 0.05;

/// The edge scroll's own state, kept with the input.
#[derive(Default)]
pub struct EdgeScroll {
    /// Space is held during the drag.
    pub fast: bool,
    last: Option<Instant>,
}

/// The speed along one axis for a pointer at `p` in a view from `lo` to `hi`, in px per second
/// (negative toward `lo`); zero away from the edges.
pub fn edge_speed(p: f32, lo: f32, hi: f32) -> f32 {
    if hi - lo <= ZONE * 2.0 {
        return 0.0;
    }
    if p > hi - ZONE {
        SPEED * ((p - (hi - ZONE)) / ZONE).min(DEEPEST)
    } else if p < lo + ZONE {
        -SPEED * ((lo + ZONE - p) / ZONE).min(DEEPEST)
    } else {
        0.0
    }
}

impl Studio {
    /// A drag that has started, in the arrangement or the roll.
    pub fn edge_dragging(&self) -> bool {
        self.arrange.drag.as_ref().is_some_and(|d| d.started)
            || self.roll.drag.as_ref().is_some_and(|d| d.moved)
    }

    /// Space during a drag: down makes the edge scroll fast, up lets it go. True when the key was
    /// the drag's (so it does not start or stop the song).
    pub fn drag_space(&mut self, key: &str, down: bool) -> bool {
        if key != "space" {
            return false;
        }
        if down && self.edge_dragging() {
            self.input_map.edge.fast = true;
            return true;
        }
        if !down && self.input_map.edge.fast {
            self.input_map.edge.fast = false;
            return true;
        }
        false
    }

    /// Once a frame: scrolls by the time since the last frame. True while it moves, so the
    /// window asks for the next frame.
    pub fn drag_edge_frame(&mut self, g: &Geometry) -> bool {
        if !self.edge_dragging() {
            self.input_map.edge = Default::default();
            return false;
        }
        let now = Instant::now();
        let secs = self
            .input_map
            .edge
            .last
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(LONGEST));
        self.input_map.edge.last = Some(now);
        // The first frame of a drag only starts the clock; it still asks for the next one.
        secs == 0.0 && self.edge_wanted(g) || self.drag_edge_step(g, secs)
    }

    /// Whether the pointer is in an edge's zone of the drag's view.
    fn edge_wanted(&self, g: &Geometry) -> bool {
        self.edge_velocity(g) != (0.0, 0.0)
    }

    /// The scroll speed the drag asks for now, in px per second, Space included.
    fn edge_velocity(&self, g: &Geometry) -> (f32, f32) {
        let (x, y) = self.input_map.pointer;
        let fast = if self.input_map.edge.fast { FAST } else { 1.0 };
        if let Some(d) = self.arrange.drag.as_ref().filter(|d| d.started) {
            let l = g.lanes;
            // Along the time only for a drag that holds something in time (not a track's row or
            // a lane's head in the column), and up and down only for one among the rows (not
            // the ruler's loop, sections or tempo).
            let along = !matches!(
                d.spot,
                Spot::Row(_) | Spot::AutoToggle(_) | Spot::LaneHead { .. }
            );
            let across = !matches!(
                d.spot,
                Spot::Loop(_) | Spot::Section { .. } | Spot::Band | Spot::Tempo(_) | Spot::TempoGap
            );
            let vx = if along {
                edge_speed(x, l.x, l.right())
            } else {
                0.0
            };
            let vy = if across {
                edge_speed(y, l.y, l.bottom())
            } else {
                0.0
            };
            return (vx * fast, vy * fast);
        }
        if let Some(d) = self.roll.drag.as_ref().filter(|d| d.moved)
            && let Some(ctx) = self.roll_input(g)
        {
            let grid = self.roll.frame(ctx.body).grid;
            let vx = edge_speed(x, grid.x, grid.right());
            let vy = if d.lane.is_none() && grid.contains(d.press.0, d.press.1) {
                edge_speed(y, grid.y, grid.bottom())
            } else {
                0.0
            };
            return (vx * fast, vy * fast);
        }
        (0.0, 0.0)
    }

    /// One step of `secs` seconds: scrolls the drag's view and plays the drag again at the
    /// pointer. True when the view moved.
    pub fn drag_edge_step(&mut self, g: &Geometry, secs: f32) -> bool {
        let (vx, vy) = self.edge_velocity(g);
        if (vx, vy) == (0.0, 0.0) || secs <= 0.0 {
            return false;
        }
        let (x, y) = self.input_map.pointer;
        if self.arrange.drag.as_ref().is_some_and(|d| d.started) {
            let before = self.scroll;
            // Past the song's end the drag goes on, a few screens at most, so what it holds can
            // be put where the song does not reach yet.
            let (mx, my) = self.max_scroll(g);
            let past = (mx + g.lanes.w * PAST).max(before.0);
            self.scroll.0 = (self.scroll.0 + vx * secs).clamp(0.0, past);
            self.scroll.1 = (self.scroll.1 + vy * secs).clamp(0.0, my);
            let (dx, dy) = (self.scroll.0 - before.0, self.scroll.1 - before.1);
            if (dx, dy) == (0.0, 0.0) {
                return false;
            }
            if let Some(d) = &mut self.arrange.drag {
                d.from = (d.from.0 - dx, d.from.1 - dy);
            }
            for p in &mut self.arrange.stroke {
                p.0 -= dx;
                p.1 -= dy;
            }
            let mods = self.arrange.mods;
            self.arrange_move(g, x, y, mods);
            return true;
        }
        let Some(ctx) = self.roll_input(g) else {
            return false;
        };
        let before = self.roll.scroll;
        self.roll.scroll.0 += vx * secs;
        self.roll.scroll.1 += vy * secs;
        self.roll.clamp_scroll(&ctx);
        let (dx, dy) = (self.roll.scroll.0 - before.0, self.roll.scroll.1 - before.1);
        if (dx, dy) == (0.0, 0.0) {
            return false;
        }
        if let Some(d) = &mut self.roll.drag {
            d.press = (d.press.0 - dx, d.press.1 - dy);
            d.last = (d.last.0 - dx, d.last.1 - dy);
        }
        let mods = self.roll.mods;
        let out = self.roll.moved(&ctx, x, y, mods);
        self.apply_roll(g, out);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Hit;
    use crate::tools::Mods;
    use xirius_core::id::{ClipId, TrackId};

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = false;
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    /// The first clip whose middle is on the screen, left of the lanes' middle.
    fn a_clip(s: &Studio, g: &Geometry) -> (TrackId, ClipId, (f32, f32)) {
        let p = s.project();
        p.tracks
            .values()
            .flat_map(|t| t.clips.iter().map(move |c| (t.id, c.id)))
            .find_map(|(t, c)| {
                let at = s.center_of(g, Hit::Clip(t, c))?;
                (at.0 < g.lanes.x + g.lanes.w / 2.0).then_some((t, c, at))
            })
            .expect("a clip on the screen")
    }

    fn start(s: &Studio, t: TrackId, c: ClipId) -> xirius_core::time::Tick {
        s.project().tracks[&t]
            .clips
            .iter()
            .find(|k| k.id == c)
            .unwrap()
            .start
    }

    /// The pointer moved as the screen moves it: the place first, then the drag.
    fn move_to(s: &mut Studio, g: &Geometry, x: f32, y: f32) {
        s.input_map.pointer = (x, y);
        s.arrange_move(g, x, y, Mods::NONE);
    }

    #[test]
    fn the_speed_grows_past_the_edge_and_is_nothing_away_from_it() {
        assert_eq!(edge_speed(500.0, 0.0, 1000.0), 0.0);
        assert!(edge_speed(990.0, 0.0, 1000.0) > 0.0);
        assert!(edge_speed(10.0, 0.0, 1000.0) < 0.0);
        assert!(edge_speed(1040.0, 0.0, 1000.0) > edge_speed(1000.0, 0.0, 1000.0));
        assert_eq!(edge_speed(5000.0, 0.0, 1000.0), SPEED * DEEPEST);
        // A view too small for two zones does not scroll.
        assert_eq!(edge_speed(10.0, 0.0, 40.0), 0.0);
    }

    #[test]
    fn edt_046_space_at_the_edge_scrolls_faster_and_esc_puts_the_clip_back_unrecorded() {
        let (mut s, g) = studio();
        let (t, c, (x, y)) = a_clip(&s, &g);
        let was = start(&s, t, c);
        let steps = s.song.steps();
        assert!(s.max_scroll(&g).0 > 0.0, "room to scroll");

        // Pressed on the clip and dragged to the right edge of the lanes.
        s.input_map.pointer = (x, y);
        s.arrange_down(&g, x, y, Mods::NONE);
        let edge = g.lanes.right() - 2.0;
        move_to(&mut s, &g, (x + edge) / 2.0, y);
        move_to(&mut s, &g, edge, y);
        assert!(s.edge_dragging());
        let planned = |s: &Studio| s.plan(&g, s.arrange.drag.as_ref().unwrap());
        let before = format!("{:?}", planned(&s).and_then(|p| p.command));

        // At its own pace, the view moves and the clip held moves with it.
        let x0 = s.scroll.0;
        assert!(s.drag_edge_step(&g, 0.1));
        let slow = s.scroll.0 - x0;
        assert!(slow > 0.0);
        let after = format!("{:?}", planned(&s).and_then(|p| p.command));
        assert_ne!(before, after, "the drag follows the view");

        // Space during the drag is the drag's: it does not start the song, and the next step
        // goes FAST times as far.
        assert!(s.edit_key(&g, "space", Mods::NONE));
        assert!(!s.transport.playing);
        let x1 = s.scroll.0;
        assert!(s.drag_edge_step(&g, 0.1));
        let fast = s.scroll.0 - x1;
        assert!((fast / slow - FAST).abs() < 0.01, "{slow} then {fast}");
        // Let go of Space: back to its pace.
        assert!(s.drag_space("space", false));
        let x2 = s.scroll.0;
        assert!(s.drag_edge_step(&g, 0.1));
        assert!(((s.scroll.0 - x2) - slow).abs() < 0.01);

        // Away from the edge it stops.
        move_to(&mut s, &g, g.lanes.x + g.lanes.w / 2.0, y);
        assert!(!s.drag_edge_step(&g, 0.1));

        // Esc gives the drag up: the clip stays where it was and nothing is in the history.
        assert!(s.edit_key(&g, "escape", Mods::NONE));
        assert!(!s.edge_dragging());
        assert_eq!(start(&s, t, c), was);
        assert_eq!(s.song.steps(), steps);
        // With no drag, Space is the song's again.
        assert!(!s.drag_space("space", true));
    }

    #[test]
    fn edt_046_a_drag_let_go_after_the_edge_scroll_is_one_step_further_than_the_screen() {
        let (mut s, g) = studio();
        let (t, c, (x, y)) = a_clip(&s, &g);
        let was = start(&s, t, c);
        let steps = s.song.steps();
        s.input_map.pointer = (x, y);
        s.arrange_down(&g, x, y, Mods::NONE);
        let edge = g.lanes.right() - 2.0;
        move_to(&mut s, &g, (x + edge) / 2.0, y);
        move_to(&mut s, &g, edge, y);
        assert!(s.edit_key(&g, "space", Mods::NONE));
        for _ in 0..30 {
            assert!(s.drag_edge_step(&g, 0.05));
            // Each frame keeps the scroll inside what there is; the drag's view stays.
            s.resized(&g);
        }
        let scrolled = s.scroll.0;
        assert!(scrolled > s.max_scroll(&g).0, "past the song's end");
        s.arrange_up(&g, edge, y, Mods::NONE);
        assert!(s.drag_space("space", false));
        assert_eq!(s.song.steps(), steps + 1, "one drag, one step");
        // It went as far as the pointer went on the screen and the view under it.
        let moved = s.ticks_to_px(start(&s, t, c) - was);
        assert!(moved > edge - x, "{moved} px");
        assert!(
            (moved - (edge - x + scrolled)).abs() < s.ticks_to_px(3840),
            "{moved} px"
        );
    }

    #[test]
    fn edt_046_a_note_dragged_to_the_rolls_edge_scrolls_it_and_esc_puts_it_back() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 820.0);
        s.resized(&g);
        s.tool = crate::tools::Tool::Select;
        let ctx = s.roll_input(&g).expect("the roll");
        let notes = ctx.notes().to_vec();
        let first = (0..notes.len())
            .min_by_key(|&i| notes[i].start)
            .expect("a note");
        let r = s.roll.note_box(&ctx, first).expect("on the screen");
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let steps = s.song.steps();
        s.input_map.pointer = (x, y);
        assert!(s.roll_press(&g, x, y, Mods::NONE, 1));
        let grid = s.roll.frame(ctx.body).grid;
        let edge = grid.right() - 2.0;
        for px in [(x + edge) / 2.0, edge] {
            s.input_map.pointer = (px, y);
            let ctx = s.roll_input(&g).unwrap();
            let out = s.roll.moved(&ctx, px, y, Mods::NONE);
            s.apply_roll(&g, out);
        }
        assert!(s.edge_dragging());
        let x0 = s.roll.scroll.0;
        assert!(s.drag_edge_step(&g, 0.02));
        let slow = s.roll.scroll.0 - x0;
        assert!(slow > 0.0);
        assert!(s.edit_key(&g, "space", Mods::NONE));
        assert!(!s.transport.playing);
        let x1 = s.roll.scroll.0;
        assert!(s.drag_edge_step(&g, 0.02));
        assert!(s.roll.scroll.0 - x1 > slow * 2.0, "Space goes faster");
        assert!(s.edit_key(&g, "escape", Mods::NONE));
        assert!(!s.edge_dragging());
        assert_eq!(s.roll_input(&g).unwrap().notes(), &notes[..]);
        assert_eq!(s.song.steps(), steps);
    }
}
