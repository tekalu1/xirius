//! A wheel turn on the window, given to the part under the pointer (`screen.rs` hands it here).
//!
//! Windows sends a wheel turned with Shift held as a sideways turn: `dx` carries it and `dy` is
//! 0 (`gpui-pre-windows`'s `handle_mouse_wheel_msg`). So a part that reads Shift reads the turn
//! from `dx` as well (EDT-136).

use crate::screen::ZOOM_NOTCH_PX;
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

impl Studio {
    /// A turn of `(dx, dy)` pixels at `(x, y)`. `control` is the Ctrl key itself (`mods.ctrl` is
    /// Ctrl or ⌘). True when the screen changed.
    pub(crate) fn window_wheel(
        &mut self,
        g: &Geometry,
        (x, y): (f32, f32),
        dx: f32,
        dy: f32,
        mods: Mods,
        control: bool,
    ) -> bool {
        if self.preparing.is_some() {
            return self.prepare_scroll(dy);
        }
        if self.recovery.is_some() {
            return false;
        }
        if self.settings.is_some() {
            return self.settings_wheel(g, x, y, dy);
        }
        if self.palette.is_some() {
            return self.palette_wheel(g, x, y, dy);
        }
        // The turn of a wheel that only goes up and down, from either axis.
        let notch = if dy == 0.0 { dx } else { dy };
        // Windows sends Shift + wheel sideways, and that event is one notch: its dx is in
        // characters, not the three lines of a normal notch (P2-4 of review 15).
        let sideways = mods.shift && dy == 0.0 && notch != 0.0;
        let field_px = if sideways {
            crate::screen::ZOOM_NOTCH_PX.copysign(notch)
        } else {
            notch
        };
        if self.bar_wheel(g, x, y, notch, mods.shift, control)
            || self.bar_shape_wheel(g, x, y, notch)
        {
            return true;
        }
        if self.popup.is_some() {
            let Some(p) = self.popup.as_ref() else {
                return false;
            };
            if !p.layout(g.size).rect.contains(x, y) || notch == 0.0 {
                return false;
            }
            // Over a value's field the wheel turns the value, unless the list is already being
            // scrolled (UI-313). The label and the other rows scroll the list.
            let scrolling = self
                .ctx
                .as_ref()
                .and_then(|s| s.scrolled_at)
                .is_some_and(|at| {
                    at.elapsed()
                        < std::time::Duration::from_millis(crate::context::fields::WHEEL_GROUP_MS)
                });
            if !scrolling && let Some(i) = self.context_field_at(g, x, y) {
                return self.context_field_wheel(g, i, field_px, mods.shift, control);
            }
            let Some(p) = self.popup.as_mut() else {
                return false;
            };
            // A list that opened on its current row scrolls on from there.
            if let Some(pick) = p.pick.as_mut() {
                pick.settle(g.size);
            }
            let max = p.max_offset(g.size);
            let offset = match p.pick.as_mut() {
                Some(pick) => &mut pick.offset,
                None => &mut p.offset,
            };
            *offset = if notch < 0.0 {
                (*offset + 1).min(max)
            } else {
                offset.saturating_sub(1)
            };
            // The list was sent even when it was already at its end: the next turn is still
            // the list's (P3-8 of review 15).
            if let Some(st) = self.ctx.as_mut() {
                st.scrolled_at = Some(std::time::Instant::now());
            }
            return true;
        }
        if let Some(ctx) = self.roll_ctx(g)
            && ctx.body.contains(x, y)
        {
            let out = self.roll.wheel(&ctx, x, y, dx, dy, mods);
            self.apply_roll(g, out);
            return true;
        }
        if self.chain_wheel(g, x, y, dy)
            || self.mixer_wheel(g, x, y, dx, dy)
            || self.ruler_wheel(g, x, y, dx, dy, mods)
        {
            return true;
        }
        // The turn with Shift held, which Windows sends sideways.
        let turn = if mods.shift { dy + dx } else { dy };
        if mods.alt
            && !control
            && turn != 0.0
            && self.automation_point_wheel(g, x, y, turn / ZOOM_NOTCH_PX)
        {
            return true;
        }
        // Alt + wheel over the lanes: the waveforms taller or lower (UI-310).
        if mods.alt
            && !control
            && turn != 0.0
            && g.lanes.contains(x, y)
            && self.wave_zoom_wheel(turn / ZOOM_NOTCH_PX)
        {
            return true;
        }
        if self.browser.open && g.panel.contains(x, y) {
            if self.browser.tab == crate::browser::RailTab::Inspector {
                return self.inspector_wheel(g, (x, y), field_px, mods.shift, control);
            }
            self.browser.scroll -= dy;
            let l = self.browser.layout(g.panel);
            self.browser.clamp_scroll(&l);
        } else if control {
            // Ctrl + wheel over the rows: taller tracks; with Shift: the time axis.
            let steps = turn / ZOOM_NOTCH_PX;
            if !self.arrange_wheel(g, x, y, steps, mods) {
                self.zoom(g, x, steps);
            }
        } else {
            self.scroll_by(g, dx, dy, mods.shift);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::PopupKind;
    use crate::studio::{DrawerTab, Hit};
    use crate::topbar::BarButton;
    use crate::tree::RowKind;
    use serde_json::json;
    use xirius_core::auto::AutoTarget;

    /// One notch turned away from you as Windows sends it with Shift held: sideways, three
    /// characters of 20 px.
    const SIDEWAYS: f32 = 60.0;

    fn studio(drawer: bool) -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = drawer;
        s.tab = DrawerTab::Roll;
        let g = s.geometry(1440.0, 820.0);
        s.resized(&g);
        (s, g)
    }

    fn roll_middle(s: &Studio, g: &Geometry) -> (f32, f32) {
        let body = s.roll_ctx(g).expect("the roll").body;
        (body.x + body.w / 2.0, body.y + body.h / 2.0)
    }

    #[test]
    fn ctrl_shift_and_the_wheel_widen_the_roll_as_windows_sends_it() {
        let (mut s, g) = studio(true);
        let at = roll_middle(&s, &g);
        let before = s.roll.beat_px;
        assert!(s.window_wheel(&g, at, SIDEWAYS, 0.0, Mods::ctrl_shift(), true));
        let wider = s.roll.beat_px;
        assert!(wider > before, "{before} -> {wider}");
        // A wheel that sends the turn upright (a pad, macOS) does the same.
        s.roll.beat_px = before;
        assert!(s.window_wheel(&g, at, 0.0, SIDEWAYS, Mods::ctrl_shift(), true));
        assert!((s.roll.beat_px - wider).abs() < 0.001);
        // Turned back: narrower again.
        assert!(s.window_wheel(&g, at, -SIDEWAYS, 0.0, Mods::ctrl_shift(), true));
        assert!((s.roll.beat_px - before).abs() < 0.001);
    }

    #[test]
    fn shift_and_the_wheel_move_the_roll_along_time() {
        let (mut s, g) = studio(true);
        let at = roll_middle(&s, &g);
        let before = (s.roll.scroll, s.roll.beat_px);
        assert!(s.window_wheel(&g, at, -SIDEWAYS, 0.0, Mods::shift(), false));
        assert!(
            s.roll.scroll.0 > before.0.0,
            "{:?} {:?}",
            before.0,
            s.roll.scroll
        );
        assert_eq!(s.roll.scroll.1, before.0.1);
        assert_eq!(s.roll.beat_px, before.1);
    }

    #[test]
    fn ctrl_shift_and_the_wheel_widen_the_arrangement_over_its_lanes_and_its_ruler() {
        let (mut s, g) = studio(false);
        for at in [
            (g.lanes.x + 300.0, g.lanes.y + 20.0),
            (g.ruler.x + 300.0, g.ruler.y + g.ruler.h / 2.0),
        ] {
            let before = s.beat_w;
            assert!(s.window_wheel(&g, at, SIDEWAYS, 0.0, Mods::ctrl_shift(), true));
            assert!(s.beat_w > before, "{at:?}: {before} -> {}", s.beat_w);
        }
    }

    #[test]
    fn shift_and_the_wheel_over_the_ruler_follow_the_map() {
        let (mut s, g) = studio(false);
        s.song
            .run(
                "mousemap.set",
                json!({"context":"ruler.any.empty.wheel","modifier":"Shift","action":"view.zoom.time"}),
            )
            .unwrap();
        let at = (g.ruler.x + 300.0, g.ruler.y + g.ruler.h / 2.0);
        let before = s.beat_w;
        assert!(s.window_wheel(&g, at, SIDEWAYS, 0.0, Mods::shift(), false));
        assert!(s.beat_w > before, "{before} -> {}", s.beat_w);
    }

    #[test]
    fn shift_and_the_wheel_move_a_long_list_down_and_up() {
        let (mut s, g) = studio(false);
        let id = s
            .project()
            .tracks
            .values()
            .find(|t| t.kind != xirius_core::model::TrackKind::Master)
            .unwrap()
            .id
            .to_string();
        for db in 1..=12 {
            assert!(s.run_command(
                "track.volume.set",
                json!({ "ids": [id], "db": -f64::from(db) })
            ));
        }
        assert!(s.press(&g, Hit::Bar(BarButton::History)));
        let p = s.popup.as_ref().expect("the history");
        assert_eq!(p.kind, PopupKind::History);
        assert!(p.max_offset(g.size) > 1);
        let r = p.layout(g.size).rect;
        let at = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        // Toward you: down the list.
        assert!(s.window_wheel(&g, at, -SIDEWAYS, 0.0, Mods::shift(), false));
        assert!(s.window_wheel(&g, at, -SIDEWAYS, 0.0, Mods::shift(), false));
        assert_eq!(s.popup.as_ref().unwrap().offset, 2);
        assert!(s.window_wheel(&g, at, SIDEWAYS, 0.0, Mods::shift(), false));
        assert_eq!(s.popup.as_ref().unwrap().offset, 1);
        // No turn at all leaves it.
        assert!(!s.window_wheel(&g, at, 0.0, 0.0, Mods::NONE, false));
        assert_eq!(s.popup.as_ref().unwrap().offset, 1);
    }

    #[test]
    fn alt_shift_and_the_wheel_move_an_automation_point() {
        let (mut s, g) = studio(false);
        let t = crate::arrange::probe::track(&s, "Keys");
        assert!(s.run_command("auto.lane.add", json!({"track":t,"target":"volume"})));
        for (at, value) in [(15360, -24.0), (17280, -12.0), (19200, -6.0)] {
            assert!(s.run_command(
                "auto.point.add",
                json!({"track":t,"target":"volume","at":at,"value":value}),
            ));
        }
        s.scroll.1 = s.tree().row_of(RowKind::Track(t)).unwrap().y;
        s.toggle_lanes(&g, t);
        let value = |s: &Studio| {
            s.project().tracks[&t]
                .automation
                .iter()
                .find(|l| l.target == AutoTarget::Volume)
                .unwrap()
                .points[1]
                .value
        };
        let at = {
            let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
            (
                g.lanes.x + s.ticks_to_px(17280) - s.scroll.0,
                s.lane_top(&g, &row, 0)
                    + crate::arrange::lane_value_y(&AutoTarget::Volume, value(&s)),
            )
        };
        assert!(s.window_wheel(&g, at, SIDEWAYS, 0.0, Mods::alt_shift(), false));
        assert!(value(&s) > -12.0, "{}", value(&s));
    }
}
