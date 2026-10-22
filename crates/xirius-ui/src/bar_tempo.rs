//! The tempo readout of the top bar as a control (UI-119). It is dragged up and
//! down (Shift for fine steps), scrolled over, stepped with the arrows while it
//! has the keys, and typed into (double press or Enter; Enter sets, Esc leaves).
//! Every way ends in one `tempo.set` at the tempo point in effect at the
//! playhead, so a drag is one step of the history and the CLI and MCP do the
//! same under the same name.

use serde_json::json;

use crate::draw::Pen;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tools::Mods;
use crate::topbar::{BarButton, TempoMode};

pub const MIN_BPM: f64 = 20.0;
pub const MAX_BPM: f64 = 999.0;
/// BPM per pixel of a drag, and of a drag with Shift.
const DRAG_PER_PX: f64 = 0.5;
const DRAG_FINE_PER_PX: f64 = 0.05;
/// A step of the arrows and of a wheel notch, and with Shift.
const STEP: f64 = 1.0;
const STEP_FINE: f64 = 0.1;
/// The longest text the readout takes (`999.9`).
const TYPED_MAX: usize = 6;

#[derive(Clone, Copy, Debug)]
struct Drag {
    y0: f32,
    start: f64,
    value: f64,
}

#[derive(Clone, Debug)]
struct Typing {
    text: String,
    /// The old value is still selected: the first key replaces it.
    fresh: bool,
}

/// What the tempo readout is doing.
#[derive(Clone, Debug, Default)]
pub struct BarEdit {
    /// The readout has the arrow keys.
    focus: bool,
    drag: Option<Drag>,
    typing: Option<Typing>,
}

impl BarEdit {
    /// Gives the readout the arrow keys.
    pub fn focus_tempo(&mut self) {
        self.focus = true;
    }
}

/// A frame of `t` px inside `r`.
pub fn frame(pen: &mut Pen, r: Rect, t: f32, color: gpui::Hsla) {
    pen.fill(Rect::new(r.x, r.y, r.w, t), color);
    pen.fill(Rect::new(r.x, r.bottom() - t, r.w, t), color);
    pen.fill(Rect::new(r.x, r.y, t, r.h), color);
    pen.fill(Rect::new(r.right() - t, r.y, t, r.h), color);
}

/// A tempo as the song keeps it: clamped, to a tenth.
pub fn snap(bpm: f64) -> f64 {
    (bpm.clamp(MIN_BPM, MAX_BPM) * 10.0).round() / 10.0
}

/// A tempo as the readout writes it.
pub fn text(bpm: f64) -> String {
    format!("{bpm:.1}")
}

impl Studio {
    /// The tempo point in effect at the playhead: its tick and its tempo.
    pub(crate) fn tempo_target(&self) -> (i64, f64) {
        let p = self.project();
        p.tempo
            .iter()
            .rev()
            .find(|t| t.tick <= self.playhead)
            .or(p.tempo.first())
            .map_or((0, 120.0), |t| (t.tick, t.bpm))
    }

    /// Sets the tempo of the point in effect at the playhead; nothing happens
    /// when it already is that.
    pub(crate) fn set_tempo(&mut self, bpm: f64) -> bool {
        let (at, now) = self.tempo_target();
        let bpm = snap(bpm);
        if (bpm - now).abs() < 1e-9 {
            return false;
        }
        self.run_command("tempo.set", json!({ "bpm": bpm, "at": at }))
    }

    /// What the readout shows and what it is in the middle of.
    pub(crate) fn tempo_readout(&self) -> (String, TempoMode) {
        let e = &self.bar_edit;
        if let Some(t) = &e.typing {
            return (t.text.clone(), TempoMode::Edit { fresh: t.fresh });
        }
        if let Some(d) = &e.drag {
            return (text(d.value), TempoMode::Drag);
        }
        let mode = if e.focus {
            TempoMode::Focus
        } else {
            TempoMode::Idle
        };
        (text(self.tempo_target().1), mode)
    }

    fn on_tempo(&self, g: &Geometry, x: f32, y: f32) -> bool {
        self.hit(g, x, y) == Hit::Bar(BarButton::Tempo)
    }

    fn tempo_begin_typing(&mut self) {
        let now = self.tempo_target().1;
        self.bar_edit.focus = true;
        self.bar_edit.typing = Some(Typing {
            text: text(now),
            fresh: true,
        });
    }

    /// What was typed is the tempo; text with no number in it changes nothing.
    fn tempo_commit_typing(&mut self) {
        if let Some(t) = self.bar_edit.typing.take()
            && let Ok(v) = t.text.trim().parse::<f64>()
            && v.is_finite()
        {
            self.set_tempo(v);
        }
    }

    /// A press anywhere on the window. The readout takes it when it is on the
    /// readout; a press elsewhere sets down what was typed and takes the keys away.
    pub(crate) fn bar_down(&mut self, g: &Geometry, x: f32, y: f32, clicks: u32) -> bool {
        let on = self.on_tempo(g, x, y);
        if self.bar_edit.typing.is_some() {
            if on {
                // The caret stays at the end of the text.
                return true;
            }
            self.tempo_commit_typing();
        }
        if !on {
            self.bar_edit.focus = false;
            return false;
        }
        // A press the bar takes leaves the inspector's field (UI-312).
        self.field_blur();
        self.popup = None;
        self.error = None;
        self.bar_edit.focus = true;
        if clicks >= 2 {
            self.bar_edit.drag = None;
            self.tempo_begin_typing();
            return true;
        }
        let now = self.tempo_target().1;
        self.bar_edit.drag = Some(Drag {
            y0: y,
            start: now,
            value: now,
        });
        true
    }

    /// The pointer moved with the readout held: up raises the tempo.
    pub(crate) fn bar_move(&mut self, y: f32, shift: bool) -> bool {
        let Some(d) = &mut self.bar_edit.drag else {
            return false;
        };
        let per = if shift { DRAG_FINE_PER_PX } else { DRAG_PER_PX };
        d.value = snap(d.start + f64::from(d.y0 - y) * per);
        true
    }

    /// The readout let go: the tempo it shows is set, in one step.
    pub(crate) fn bar_up(&mut self) -> bool {
        let Some(d) = self.bar_edit.drag.take() else {
            return false;
        };
        if (d.value - d.start).abs() > 1e-9 {
            self.set_tempo(d.value);
        }
        true
    }

    /// The wheel over the readout: a notch up raises the tempo by one, by ten with Shift and by
    /// a tenth with Ctrl (UI-313).
    pub(crate) fn bar_wheel(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        delta: f32,
        shift: bool,
        ctrl: bool,
    ) -> bool {
        if !self.on_tempo(g, x, y) {
            return false;
        }
        if self.bar_edit.typing.is_none() && delta != 0.0 && self.bar_edit.drag.is_none() {
            let step = if ctrl && !shift {
                STEP_FINE
            } else if shift {
                STEP * 10.0
            } else {
                STEP
            };
            let now = self.tempo_target().1;
            self.bar_edit.focus = true;
            // The tempo's wheel is one undo step per turn, as a field's is (P3-9 of review 15).
            self.wheel_group_start(crate::context::fields::Kind::Tempo, &[]);
            self.wheel_running = true;
            let _ = self.set_tempo(now + if delta > 0.0 { step } else { -step });
            self.wheel_running = false;
        }
        true
    }

    /// A key for the readout: the arrows and Enter while it has the keys, every
    /// key while its text is being typed.
    pub(crate) fn bar_key(&mut self, key: &str, ch: Option<&str>, m: Mods) -> bool {
        if m.ctrl || m.alt {
            return false;
        }
        if self.bar_edit.typing.is_some() {
            return self.bar_typing(key, ch);
        }
        if !self.bar_edit.focus || self.popup.is_some() {
            return false;
        }
        match key {
            "up" | "down" => {
                // Shift is the fields' "further", not the drag's "finer" (P3-2 of review 15).
                let step = if m.shift { STEP * 10.0 } else { STEP };
                let now = self.tempo_target().1;
                self.set_tempo(now + if key == "up" { step } else { -step });
                true
            }
            "enter" => {
                self.tempo_begin_typing();
                true
            }
            "escape" => {
                self.bar_edit.focus = false;
                true
            }
            _ => false,
        }
    }

    fn bar_typing(&mut self, key: &str, ch: Option<&str>) -> bool {
        match key {
            "enter" => self.tempo_commit_typing(),
            "escape" => self.bar_edit.typing = None,
            "backspace" => {
                if let Some(t) = &mut self.bar_edit.typing {
                    if t.fresh {
                        t.text.clear();
                    } else {
                        t.text.pop();
                    }
                    t.fresh = false;
                }
            }
            _ => {
                let typed = ch
                    .filter(|c| c.chars().count() == 1)
                    .and_then(|c| c.chars().next())
                    .map(crate::context::fields::halfwidth)
                    .filter(|c| c.is_ascii_digit() || *c == '.');
                if let (Some(c), Some(t)) = (typed, &mut self.bar_edit.typing) {
                    if t.fresh {
                        t.text.clear();
                        t.fresh = false;
                    }
                    if t.text.chars().count() < TYPED_MAX && (c != '.' || !t.text.contains('.')) {
                        t.text.push(c);
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Geometry;

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = false;
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    fn at(s: &Studio, g: &Geometry, b: BarButton) -> (f32, f32) {
        s.center_of(g, Hit::Bar(b))
            .expect("the button is on the bar")
    }

    fn bpm(s: &Studio) -> f64 {
        s.tempo_target().1
    }

    fn key(s: &mut Studio, k: &str, shift: bool) -> bool {
        s.bar_key(
            k,
            None,
            Mods {
                shift,
                ..Mods::default()
            },
        )
    }

    fn type_in(s: &mut Studio, typed: &str) {
        for c in typed.chars() {
            let c = c.to_string();
            assert!(s.bar_key(&c, Some(&c), Mods::default()));
        }
    }

    #[test]
    fn the_readout_is_a_button_of_the_bar_and_the_meter_beside_it() {
        let (s, g) = studio();
        for b in [BarButton::Tempo, BarButton::Meter] {
            let (x, y) = at(&s, &g, b);
            // In the bottom bar, under the body (UI-300).
            assert!(y >= g.size.1);
            assert_eq!(s.hit(&g, x, y), Hit::Bar(b));
        }
    }

    #[test]
    fn a_drag_moves_the_tempo_by_the_pixel_and_sets_it_once_on_release() {
        let (mut s, g) = studio();
        let before = s.song.history().len();
        let was = bpm(&s);
        let (x, y) = at(&s, &g, BarButton::Tempo);
        assert!(s.bar_down(&g, x, y, 1));
        assert_eq!(s.tempo_readout().1, TempoMode::Drag);
        // 20 px up: +10 bpm, shown at once and not yet set.
        assert!(s.bar_move(y - 20.0, false));
        assert_eq!(s.tempo_readout().0, text(was + 10.0));
        assert_eq!(bpm(&s), was);
        assert!(s.bar_move(y - 40.0, false));
        assert!(s.bar_up());
        assert_eq!(bpm(&s), was + 20.0);
        assert_eq!(s.song.history().len(), before + 1);
        assert_eq!(s.tempo_readout().1, TempoMode::Focus);
    }

    #[test]
    fn shift_makes_the_drag_fine_and_down_lowers() {
        let (mut s, g) = studio();
        let was = bpm(&s);
        let (x, y) = at(&s, &g, BarButton::Tempo);
        s.bar_down(&g, x, y, 1);
        s.bar_move(y - 20.0, true);
        s.bar_up();
        assert!((bpm(&s) - (was + 1.0)).abs() < 1e-9);
        s.bar_down(&g, x, y, 1);
        s.bar_move(y + 20.0, false);
        s.bar_up();
        assert!((bpm(&s) - (was - 9.0)).abs() < 1e-9);
    }

    #[test]
    fn a_press_without_a_move_changes_nothing() {
        let (mut s, g) = studio();
        let before = s.song.history().len();
        let (x, y) = at(&s, &g, BarButton::Tempo);
        s.bar_down(&g, x, y, 1);
        s.bar_up();
        assert_eq!(s.song.history().len(), before);
    }

    #[test]
    fn a_drag_stops_at_the_ends_of_the_range() {
        let (mut s, g) = studio();
        let (x, y) = at(&s, &g, BarButton::Tempo);
        s.bar_down(&g, x, y, 1);
        s.bar_move(y - 5000.0, false);
        s.bar_up();
        assert_eq!(bpm(&s), MAX_BPM);
        s.bar_down(&g, x, y, 1);
        s.bar_move(y + 5000.0, false);
        s.bar_up();
        assert_eq!(bpm(&s), MIN_BPM);
    }

    #[test]
    fn the_wheel_steps_by_one_by_ten_with_shift_and_by_a_tenth_with_ctrl() {
        let (mut s, g) = studio();
        let was = bpm(&s);
        let (x, y) = at(&s, &g, BarButton::Tempo);
        let before = s.song.history().len();
        assert!(s.bar_wheel(&g, x, y, 1.0, false, false));
        assert_eq!(bpm(&s), was + 1.0);
        assert!(s.bar_wheel(&g, x, y, 1.0, true, false));
        assert_eq!(bpm(&s), was + 11.0, "Shift: ten");
        assert!(s.bar_wheel(&g, x, y, -1.0, false, true));
        assert!((bpm(&s) - (was + 10.9)).abs() < 1e-9, "Ctrl: a tenth");
        assert_eq!(s.song.history().len(), before + 3);
        // Over something else the wheel is not the readout's.
        let (mx, my) = at(&s, &g, BarButton::Meter);
        assert!(!s.bar_wheel(&g, mx, my, 1.0, false, false));
    }

    #[test]
    fn the_wheels_turn_is_one_undo_step() {
        let (mut s, g) = studio();
        let was = bpm(&s);
        let (x, y) = at(&s, &g, BarButton::Tempo);
        assert!(s.bar_wheel(&g, x, y, 1.0, false, false));
        assert!(s.bar_wheel(&g, x, y, 1.0, false, false));
        assert_eq!(bpm(&s), was + 2.0);
        s.undo(&g);
        assert_eq!(bpm(&s), was);
    }

    #[test]
    fn the_arrows_step_the_tempo_once_the_readout_has_the_keys() {
        let (mut s, g) = studio();
        let was = bpm(&s);
        assert!(!key(&mut s, "up", false), "no keys before it is pressed");
        let (x, y) = at(&s, &g, BarButton::Tempo);
        s.bar_down(&g, x, y, 1);
        s.bar_up();
        assert!(key(&mut s, "up", false));
        assert_eq!(bpm(&s), was + 1.0);
        assert!(key(&mut s, "down", true));
        assert_eq!(bpm(&s), was - 9.0, "Shift: ten, as the fields do");
        assert!(key(&mut s, "up", true));
        assert_eq!(bpm(&s), was + 1.0);
        // Esc gives the keys back.
        assert!(key(&mut s, "escape", false));
        assert!(!key(&mut s, "up", false));
        assert_eq!(s.tempo_readout().1, TempoMode::Idle);
    }

    #[test]
    fn a_double_press_or_enter_types_the_tempo_and_enter_sets_it() {
        let (mut s, g) = studio();
        let (x, y) = at(&s, &g, BarButton::Tempo);
        let was = bpm(&s);
        s.bar_down(&g, x, y, 1);
        s.bar_up();
        s.bar_down(&g, x, y, 2);
        s.bar_up();
        // The old value is selected; the text shows it until a key replaces it.
        assert_eq!(
            s.tempo_readout(),
            (text(was), TempoMode::Edit { fresh: true })
        );
        type_in(&mut s, "93.5");
        assert_eq!(
            s.tempo_readout(),
            ("93.5".into(), TempoMode::Edit { fresh: false })
        );
        let before = s.song.history().len();
        assert!(key(&mut s, "enter", false));
        assert_eq!(bpm(&s), 93.5);
        assert_eq!(s.song.history().len(), before + 1);
        assert_eq!(s.tempo_readout().1, TempoMode::Focus);

        // Enter on the focused readout opens the same box; Esc leaves the tempo.
        assert!(key(&mut s, "enter", false));
        type_in(&mut s, "200");
        assert!(key(&mut s, "escape", false));
        assert_eq!(bpm(&s), 93.5);
        assert_eq!(s.song.history().len(), before + 1);
    }

    #[test]
    fn typing_is_clamped_and_text_with_no_number_changes_nothing() {
        let (mut s, g) = studio();
        let (x, y) = at(&s, &g, BarButton::Tempo);
        s.bar_down(&g, x, y, 2);
        type_in(&mut s, "5000");
        key(&mut s, "enter", false);
        assert_eq!(bpm(&s), MAX_BPM);
        s.bar_down(&g, x, y, 2);
        type_in(&mut s, "5");
        key(&mut s, "enter", false);
        assert_eq!(bpm(&s), MIN_BPM);
        // Backspace takes the selected value away, and Enter on nothing sets nothing.
        let before = s.song.history().len();
        s.bar_down(&g, x, y, 2);
        key(&mut s, "backspace", false);
        key(&mut s, "enter", false);
        assert_eq!(s.song.history().len(), before);
        // One point at most, and no letters.
        s.bar_down(&g, x, y, 2);
        type_in(&mut s, "1.2.x3");
        assert_eq!(s.tempo_readout().0, "1.23");
    }

    #[test]
    fn the_ime_full_width_digits_are_taken() {
        let (mut s, g) = studio();
        let (x, y) = at(&s, &g, BarButton::Tempo);
        s.bar_down(&g, x, y, 2);
        type_in(&mut s, "８５");
        assert_eq!(s.tempo_readout().0, "85");
        assert!(key(&mut s, "enter", false));
        assert_eq!(bpm(&s), 85.0);
    }

    #[test]
    fn a_press_elsewhere_sets_down_what_was_typed() {
        let (mut s, g) = studio();
        let (x, y) = at(&s, &g, BarButton::Tempo);
        s.bar_down(&g, x, y, 2);
        type_in(&mut s, "77");
        // A press on the readout itself is the caret's, not the end of the box.
        assert!(s.bar_down(&g, x, y, 1));
        assert!(matches!(s.tempo_readout().1, TempoMode::Edit { .. }));
        let (px, py) = at(&s, &g, BarButton::Play);
        assert!(!s.bar_down(&g, px, py, 1));
        assert_eq!(bpm(&s), 77.0);
        assert_eq!(s.tempo_readout().1, TempoMode::Idle);
    }

    #[test]
    fn the_tempo_set_is_the_point_in_effect_at_the_playhead() {
        let (mut s, _) = studio();
        // A second point at the 5th beat; the playhead past it.
        assert!(s.run_command("tempo.set", json!({ "bpm": 100.0, "at": 4 * 960 })));
        s.playhead = 5 * 960;
        let n = s.project().tempo.len();
        assert_eq!(bpm(&s), 100.0);
        assert!(s.set_tempo(110.0));
        let p = s.project();
        assert_eq!(p.tempo.len(), n, "it changes the point, it adds none");
        assert_eq!(p.tempo.last().map(|t| t.bpm), Some(110.0));
        assert_ne!(p.tempo.first().map(|t| t.bpm), Some(110.0));
    }
}
