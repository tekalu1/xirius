//! How a press on the window's body reaches the parts under it, in the order they are drawn
//! over one another. The screen hands over what it has read of the event; this order is what
//! tests drive too.

use crate::studio::{Geometry, Hit, Studio};
use crate::tools::Mods;

/// What a press asks of the screen once it has been dealt with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PressOut {
    /// The window repaints.
    pub notify: bool,
    /// Files may have been asked for (a file dialog, a drop target): the screen looks.
    pub files: bool,
}

impl PressOut {
    const NOTIFY: Self = Self {
        notify: true,
        files: false,
    };
    const BOTH: Self = Self {
        notify: true,
        files: true,
    };
}

impl Studio {
    /// A press at (`x`, `y`) in the body that no dialog, setting or palette took.
    pub(crate) fn window_press(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        clicks: u32,
        mods: Mods,
    ) -> PressOut {
        // The presses that go on after a click on a start-screen row (a double-click, or one
        // press after another: counts 2, 3, 4 …) land on the song that came up under it: they
        // are the rest of that click, and the song does not take them. The place is let go by
        // a press of its own (count 1), not by the one it swallows.
        if clicks >= 2
            && !self.home_shown()
            && self.preparing.is_none()
            && let Some((px, py)) = self.row_press
            && (x - px).abs() <= 8.0
            && (y - py).abs() <= 8.0
        {
            return PressOut::NOTIFY;
        }
        if clicks <= 1 {
            self.row_press = None;
        }
        // A press past the dialogs, the settings and the palette takes the keys from what had
        // them.
        self.press_keys(g, x, y);
        // The windows that float over the screen take a press before what is under them.
        if self.popup.is_none()
            && (self.value_press(g, x, y, clicks, mods)
                || self.instrument_press(g, x, y, clicks, mods))
        {
            return PressOut::BOTH;
        }
        // The start screen covers the body, and what is under it is not drawn: it takes no
        // press. The bars, the popups and the windows over it do.
        let home = self.home_shown();
        if !home && self.cpu_down(g, x, y, clicks) {
            return PressOut::NOTIFY;
        }
        if self.name_press(g, x, y, clicks) {
            return PressOut::NOTIFY;
        }
        if self.bar_down(g, x, y, clicks) {
            return PressOut::NOTIFY;
        }
        if home {
            let row = matches!(
                self.hit(g, x, y),
                crate::studio::Hit::Home(crate::home::Spot::Row(..))
            );
            let out = self.press_at_hit(g, x, y, clicks, mods);
            self.row_press = row.then_some((x, y));
            return out;
        }
        if self.mixer_bar_down(g, x, y) {
            return PressOut::NOTIFY;
        }
        if self.zoom_box_down(g, x, y, mods) {
            return PressOut::NOTIFY;
        }
        if self.edge_down(g, x, y, clicks) || self.side_down(g, x, y, clicks) {
            return PressOut::NOTIFY;
        }
        self.name_drag_down(g, x, y);
        self.insert_drag_down(g, x, y);
        if self.chain_down(g, x, y) {
            return PressOut::BOTH;
        }
        if clicks == 2 && self.ruler_double_click(g, x, y) {
            return PressOut::NOTIFY;
        }
        self.ruler_drag_down(g, x, y, mods);
        if self.roll_press(g, x, y, mods, clicks) {
            return PressOut::NOTIFY;
        }
        if clicks == 2 && self.arrange_open(g, x, y, mods) {
            return PressOut::NOTIFY;
        }
        self.arrange.clicks = clicks;
        if self.arrange_down(g, x, y, mods) {
            if clicks == 2 {
                self.arrange_open(g, x, y, mods);
            }
            return PressOut::NOTIFY;
        }
        self.press_at_hit(g, x, y, clicks, mods)
    }

    /// The two rows of either ruler have distinct double-click actions.
    fn ruler_double_click(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let (ruler, scale) = if let Some(ctx) = self.roll_ctx(g) {
            let ruler = self.roll.frame(ctx.body).ruler;
            if ruler.contains(x, y) {
                (ruler, self.roll.ruler_scale(&ctx))
            } else {
                (g.ruler, self.ruler_scale(g))
            }
        } else {
            (g.ruler, self.ruler_scale(g))
        };
        if !ruler.contains(x, y) {
            return false;
        }
        let mapped = {
            let settings = self.song.settings();
            settings
                // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only settings snapshot.
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .mousemap
                .changed
                .contains_key("ruler.any.empty.click")
        };
        if mapped {
            return false;
        }
        let tick = scale.tick(x).max(0);
        if !crate::ruler::on_band(ruler, y) {
            // design-lint: allow(ui-blocking-io) PRJ-031: transport seek submits the existing command without external I/O.
            self.seek(tick);
            if !self.transport.playing {
                self.toggle_play();
            }
            return true;
        }
        let project = self.project();
        let span = project
            .loop_range
            .map(|l| (scale.x(l.start), scale.x(l.end)));
        if crate::ruler::grip_at(span, x) != crate::ruler::LoopGrip::New {
            return false;
        }
        let map = project.time_map();
        let bar = map.to_bbt(tick).0;
        let start = map.bar_start(bar);
        let end = map.bar_start(bar + 1);
        // design-lint: allow(ui-blocking-io) PRJ-031: this is the existing synchronous in-memory edit command.
        self.arrange_run("loop.set", serde_json::json!({"start": start, "end": end}));
        if !self.transport.looping {
            self.toggle_loop();
        }
        true
    }

    /// A right press at (`x`, `y`) in the body that no dialog, setting or palette took: the
    /// menu of what is there. The start screen covers the body, and what is under it is not
    /// drawn: no menu of the song opens through it; only a draft's row has a menu (UI-351).
    pub(crate) fn window_right_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.home_shown() {
            return self.home_right_press(g, x, y);
        }
        if self.popup.is_none() && self.instrument_right(g, x, y) {
            return true;
        }
        if self.cpu_context(g, x, y) || self.lib_context(g, x, y) || self.scene_context(g, x, y) {
            return true;
        }
        self.roll_input(g).is_none() && self.context_press(g, x, y)
    }

    /// The press goes to what is drawn at the point.
    fn press_at_hit(&mut self, g: &Geometry, x: f32, y: f32, clicks: u32, mods: Mods) -> PressOut {
        let hit = self.hit(g, x, y);
        if self.popup.is_none() && self.head_fader_down(g, hit, x, clicks, mods) {
            return PressOut::NOTIFY;
        }
        let menu = self.popup.is_some();
        // A file row is held before its press: the press closes the band under the row
        // selected before, and the rows below it move up.
        let file_row = matches!(
            hit,
            Hit::Browser(
                crate::browser::BrowserHit::File(_)
                    | crate::browser::BrowserHit::Pool(_)
                    | crate::browser::BrowserHit::PoolCount(_)
            )
        );
        let held = !menu && file_row && self.lib_grab(g, hit, x, y, clicks as usize);
        let pressed = self.press(g, hit);
        let grabbed = held || (!menu && !file_row && self.lib_grab(g, hit, x, y, clicks as usize));
        let level = self.level_down(g, hit, x);
        PressOut {
            notify: level || self.drag_begin(g, hit, y) || pressed || grabbed,
            files: true,
        }
    }

    /// The pointer moves over the body (a button may be held). True when the window repaints.
    pub(crate) fn window_moved(&mut self, g: &Geometry, x: f32, y: f32, mods: Mods) -> bool {
        let mut notify = false;
        let pointer_tip = crate::hover_tip::pointer_tip(self, g);
        self.input_map.pointer = (x, y);
        if self.context_open() {
            self.context_hover(g, x, y, std::time::Instant::now());
            self.hover = self.hit(g, x, y);
            return true;
        }
        if pointer_tip != crate::hover_tip::pointer_tip(self, g) {
            notify = true;
        }
        if self.bar_move(y, mods.shift) {
            return true;
        }
        // The start screen covers the body: what is under it is not drawn and takes no move.
        if !self.home_shown() {
            if self.zoom_box_move(x, y)
                || self.mixer_bar_move(g, x)
                || self.edge_move(g, y)
                || self.side_move(g, x)
                || self.pan_move(g, x, y)
                || self.ruler_drag_move(g, x, y)
            {
                return true;
            }
            if self.edge_hover(g, x, y) | self.side_hover(g, x, y) {
                notify = true;
            }
            if self.mixer.fx_drag.is_none() {
                self.seam_moved(g, x, y, std::time::Instant::now());
            }
            if self.mixer.level_drag.is_some() || self.mixer.key_drag.is_some() {
                return self.level_move(x, mods.shift) || notify;
            }
            if self.mixer.drag.is_some() {
                return self.drag_move_at(x, y, mods.shift) || notify;
            }
            if self.insert_drag_move(x, y, mods.shift)
                || self.name_drag_move(x, y)
                || self.fx_move(x, y)
                || self.lib_move(x, y)
            {
                // A browser row may be over an independent window: its drop's plan
                // lights there (UI-353).
                self.browser_row_over(mods.shift);
                let now = std::time::Instant::now();
                if self.mixer.fx_drag.is_some() || self.browser.drag.is_some() {
                    self.seam_moved(g, x, y, now);
                }
                // An audio row shows its ghost clip and seam; a device its line (UI-381).
                self.file_ghost_moved(g, x, y, mods.shift, now);
                self.lib_seam_moved(g, now);
                return true;
            }
        }
        if self.instrument_moved(g, x, y, mods.shift) {
            return true;
        }
        if self.instrument_hover(g, x, y) {
            notify = true;
        }
        if !self.home_shown() {
            if let Some(ctx) = self.roll_input(g) {
                let out = self.roll.moved(&ctx, x, y, mods);
                if self.apply_roll(g, out) {
                    notify = true;
                }
            }
            if self.arrange_move(g, x, y, mods) {
                notify = true;
            }
        }
        let hit = self.hit(g, x, y);
        if hit != self.hover {
            self.hover = hit;
            notify = true;
        }
        notify
    }

    /// The button is let go over the body, past the bars and the dialogs. True when the
    /// window repaints.
    pub(crate) fn window_released(&mut self, g: &Geometry, x: f32, y: f32, mods: Mods) -> bool {
        // The start screen covers the body: what is under it was not pressed, so there is
        // nothing to let go there.
        if self.home_shown() {
            self.let_go_drags();
            return false;
        }
        let mut notify = false;
        self.mixer.seam.pressed = false;
        if self.mixer_bar_up() || self.zoom_box_up(g, x, y) {
            return true;
        }
        self.ruler_drag_up();
        // Alt as the files are let go of puts several end to end on one track (UI-381).
        self.arrange.clip_drop.alt = mods.alt;
        if mods.alt && self.over == crate::independent::Over::Main && self.replace_file_up(g, x, y)
        {
            self.clip_drop_end();
            return true;
        }
        if self.level_up()
            || self.insert_drag_up(g, x, y, mods.shift)
            || self.name_drag_up(g, x, y)
            || self.fx_up(g, x, y, mods.alt)
            || self.lib_up(g, x, y, mods.shift)
            || self.instrument_up()
            || self.drag_end()
        {
            return true;
        }
        if let Some(ctx) = self.roll_ctx(g) {
            let out = self.roll.up(&ctx, x, y, mods);
            if self.apply_roll(g, out) {
                notify = true;
            }
        }
        if self.arrange_up(g, x, y, mods) {
            notify = true;
        }
        notify
    }
}

#[cfg(test)]
mod ruler_tests {
    use super::*;

    #[test]
    fn roll_double_click_uses_each_row_and_keeps_playing() {
        let mut studio = Studio::sample();
        studio.transport.mode = xirius_session::output::OutputMode::Silent { paced: false };
        let g = studio.geometry(1440.0, 860.0);
        studio.resized(&g);
        let ctx = studio.roll_ctx(&g).expect("sample roll is open");
        let ruler = studio.roll.frame(ctx.body).ruler;
        let scale = studio.roll.ruler_scale(&ctx);
        let x = ruler.x + 300.0;
        let before = studio.playhead;
        assert!(studio.ruler_double_click(&g, x, ruler.y + 2.0));
        assert_eq!(studio.playhead, before);
        assert!(studio.transport.looping);
        let project = studio.project();
        let span = project.loop_range.unwrap();
        let map = project.time_map();
        assert_eq!(span.end, map.bar_start(map.to_bbt(span.start).0 + 1));
        assert!(studio.ruler_double_click(&g, x, ruler.bottom() - 3.0));
        assert_eq!(studio.playhead, scale.tick(x).max(0));
        assert!(studio.transport.playing);
        assert!(studio.ruler_double_click(&g, x + 20.0, ruler.bottom() - 3.0));
        assert!(studio.transport.playing);
    }

    #[test]
    fn double_click_on_the_upper_empty_row_makes_one_bar_loop() {
        let mut studio = Studio::sample();
        let g = studio.geometry(1440.0, 860.0);
        studio.resized(&g);
        let x = g.ruler.x + 300.0;
        let y = g.ruler.y + 2.0;
        let playhead = studio.playhead;
        assert!(studio.ruler_double_click(&g, x, y));
        assert_eq!(studio.playhead, playhead);
        assert!(studio.transport.looping);
        let project = studio.project();
        let loop_range = project.loop_range.expect("a loop was made");
        let map = project.time_map();
        let bar = map.to_bbt(loop_range.start).0;
        assert_eq!(loop_range.start, map.bar_start(bar));
        assert_eq!(loop_range.end, map.bar_start(bar + 1));
    }

    #[test]
    fn double_click_on_the_lower_row_seeks_and_starts_playing() {
        let mut studio = Studio::sample();
        studio.transport.mode = xirius_session::output::OutputMode::Silent { paced: false };
        let g = studio.geometry(1440.0, 860.0);
        studio.resized(&g);
        let x = g.ruler.x + 300.0;
        let y = g.ruler.bottom() - 3.0;
        let tick = studio.ruler_scale(&g).tick(x).max(0);
        assert!(studio.ruler_double_click(&g, x, y));
        assert_eq!(studio.playhead, tick);
        assert!(studio.transport.playing);
    }
}
