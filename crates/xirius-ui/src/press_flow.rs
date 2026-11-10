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
            return self.press_at_hit(g, x, y, clicks, mods);
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

    /// A right press at (`x`, `y`) in the body that no dialog, setting or palette took: the
    /// menu of what is there. The start screen covers the body, and what is under it is not
    /// drawn: no menu of the song opens through it.
    pub(crate) fn window_right_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.home_shown() {
            return false;
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
        if self.mixer_bar_up() || self.zoom_box_up(g, x, y) {
            return true;
        }
        self.ruler_drag_up();
        if mods.alt && self.replace_file_up(g, x, y) {
            return true;
        }
        if self.level_up()
            || self.insert_drag_up(g, x, y, mods.shift)
            || self.name_drag_up(g, x, y)
            || self.fx_up(g, x, y, mods.alt)
            || self.lib_up(g, x, y)
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
