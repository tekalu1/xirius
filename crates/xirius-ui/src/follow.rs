//! Autoscroll (EDT-070): while the song plays, the arrangement and the piano
//! roll keep the playhead in view. F turns it on and off. The view moves in
//! one of three ways ([`Mode`]); the settings screen picks the mode, and until
//! it does the mode stays at its default.

use crate::studio::{Geometry, Studio};

/// How the view moves along with the playhead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// The view stays until the playhead leaves it, then jumps so that the
    /// playhead sits at its left edge.
    #[default]
    TurnOver,
    /// The playhead stays in the middle and the view moves all the time.
    ContinuousCenter,
    /// The playhead stays at the left edge and the view moves all the time.
    ContinuousLeft,
}

/// Whether the view follows the playhead, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Follow {
    pub on: bool,
    pub mode: Mode,
}

impl Default for Follow {
    fn default() -> Follow {
        Follow {
            on: true,
            mode: Mode::TurnOver,
        }
    }
}

/// The gap kept between the playhead and the view's edge, in px.
const EDGE: f32 = 24.0;

/// How far the scroll moves so that the playhead, at window x, follows in the
/// view from `left` to `right`. Zero when the view stays where it is.
pub fn scroll_delta(mode: Mode, x: f32, left: f32, right: f32) -> f32 {
    match mode {
        Mode::TurnOver if x < left || x > right - EDGE => x - (left + EDGE),
        Mode::TurnOver => 0.0,
        Mode::ContinuousCenter => x - (left + right) / 2.0,
        Mode::ContinuousLeft => x - (left + EDGE),
    }
}

impl Studio {
    /// Once a frame: moves the arrangement and the roll so that the playhead
    /// stays in view while the song plays.
    pub fn follow_playhead(&mut self, g: &Geometry) {
        if !self.follow.on || !self.transport.playing {
            return;
        }
        let mode = self.follow.mode;
        if g.lanes.w > 0.0 {
            let x = self.ruler_scale(g).x(self.playhead);
            let d = scroll_delta(mode, x, g.lanes.x, g.lanes.right());
            if d != 0.0 {
                self.scroll.0 += d;
                self.clamp_scroll(g);
            }
        }
        if let Some(ctx) = self.roll_ctx(g) {
            let grid = self.roll.frame(ctx.body).grid;
            if grid.w > 0.0 {
                let x = self.roll.ruler_scale(&ctx).x(self.playhead);
                let d = scroll_delta(mode, x, grid.x, grid.right());
                if d != 0.0 {
                    self.roll.scroll.0 += d;
                    self.roll.clamp_scroll(&ctx);
                }
            }
        }
    }

    /// F: turns following on or off, and says which in the notice line.
    pub fn toggle_follow(&mut self) {
        self.follow.on = !self.follow.on;
        let text = if self.follow.on {
            "自動スクロール: 入"
        } else {
            "自動スクロール: 切"
        };
        self.tell(text.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Mods;

    fn geometry(s: &Studio) -> Geometry {
        s.geometry(1440.0, 820.0)
    }

    #[test]
    fn turn_over_waits_at_the_edge_then_jumps_so_the_playhead_is_at_the_left() {
        // Inside the view nothing moves.
        assert_eq!(scroll_delta(Mode::TurnOver, 300.0, 100.0, 900.0), 0.0);
        // Past the right edge the playhead lands EDGE px in from the left.
        assert_eq!(
            scroll_delta(Mode::TurnOver, 890.0, 100.0, 900.0),
            890.0 - (100.0 + EDGE)
        );
        // A seek back to before the left edge jumps the same way.
        assert_eq!(
            scroll_delta(Mode::TurnOver, 50.0, 100.0, 900.0),
            50.0 - (100.0 + EDGE)
        );
    }

    #[test]
    fn the_continuous_modes_hold_the_playhead_at_their_spot() {
        let x = 600.0;
        let center = x - scroll_delta(Mode::ContinuousCenter, x, 100.0, 900.0);
        assert_eq!(center, 500.0);
        let left = x - scroll_delta(Mode::ContinuousLeft, x, 100.0, 900.0);
        assert_eq!(left, 100.0 + EDGE);
    }

    #[test]
    fn the_default_follows_turn_over() {
        assert_eq!(
            Follow::default(),
            Follow {
                on: true,
                mode: Mode::TurnOver
            }
        );
    }

    #[test]
    fn f_toggles_following_and_a_stopped_song_or_an_off_switch_keeps_the_view() {
        let mut s = Studio::sample();
        let g = geometry(&s);
        // Stopped: the view does not move, even with following on.
        s.playhead = s.ruler_scale(&g).tick(g.lanes.x + 3.0 * g.lanes.w);
        s.follow_playhead(&g);
        assert_eq!(s.scroll.0, 0.0);
        // Playing and on: the arrangement scrolls to the playhead.
        s.transport.playing = true;
        s.follow_playhead(&g);
        assert!(s.scroll.0 > 0.0, "the view follows the playhead");
        // Off: the view stays where it is.
        let at = s.scroll.0;
        s.toggle_follow();
        assert!(!s.follow.on);
        s.playhead = s.ruler_scale(&g).tick(g.lanes.x + 5.0 * g.lanes.w);
        s.follow_playhead(&g);
        assert_eq!(s.scroll.0, at);
        // F again turns it back on.
        s.toggle_follow();
        assert!(s.follow.on);
    }

    #[test]
    fn the_f_key_toggles_following_through_the_key_path() {
        let mut s = Studio::sample();
        let g = geometry(&s);
        assert!(s.edit_key(&g, "f", Mods::NONE));
        assert!(!s.follow.on);
        assert!(s.edit_key(&g, "f", Mods::NONE));
        assert!(s.follow.on);
    }

    #[test]
    fn the_roll_turns_over_and_keeps_the_playhead_in_its_grid() {
        let mut s = Studio::sample();
        let g = geometry(&s);
        let ctx = s.roll_ctx(&g).unwrap();
        // A clip wider than the grid, so the roll can scroll.
        s.roll.beat_px = 200.0;
        s.roll.scroll.0 = 0.0;
        let grid = s.roll.frame(ctx.body).grid;
        s.playhead = ctx.clip.start + 12 * ctx.ppq();
        s.transport.playing = true;
        s.follow_playhead(&g);
        let x = s.roll.ruler_scale(&ctx).x(s.playhead);
        assert!(
            x >= grid.x && x <= grid.right(),
            "playhead at {x}, grid {}..{}",
            grid.x,
            grid.right()
        );
        assert!(s.roll.scroll.0 > 0.0);
    }
}
