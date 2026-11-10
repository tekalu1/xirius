//! A branch's level and pan, typed and stepped as a device's knob is (UI-324), on the strip's
//! level row and on the channel editor's branch heads (MIX-045, approved 2026-11-06).
//!
//! A press on the bar, the value or the pan takes the keys: ←→ (and ↑↓) move the level by
//! 1 dB (Shift 0.1), PageUp/PageDown by 6 dB, the pan by 5 % (Shift 1 %, PageUp/PageDown 20 %:
//! a tenth of its range, as a device's knob takes); Delete puts it back to its default, M mutes,
//! Enter or a double press types it, Tab goes to the next value and Esc lets the keys go. F6 and
//! a value going out of sight let them go too, in the field as well (what is typed goes in). In
//! the field Enter puts the value in, Esc keeps the old one and Tab puts it in and goes on. A
//! level takes "-3", "-3 dB" and "-inf"; a pan "C", "L20" and "R35".

use serde_json::json;
use xirius_core::id::{NodeId, TrackId};
use xirius_core::model::{MAX_DB, MIN_DB};

use crate::mixer::flow::Target;
use crate::mixer::{LevelDrag, MixerHit};
use crate::settings::FieldId;
use crate::settings::field::Field;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tools::Mods;

/// Below this a level goes to silence with the next step down (the bar's floor).
pub const FLOOR: f64 = -60.0;
/// A step of the arrows, with Shift, and of PageUp/PageDown, in dB.
const STEP: f64 = 1.0;
const FINE: f64 = 0.1;
const PAGE: f64 = 6.0;
/// A pan step of the arrows, and with Shift.
const PAN_STEP: f64 = 0.05;
const PAN_FINE: f64 = 0.01;
/// A pan step of PageUp/PageDown: a tenth of L100〜R100, as a knob's large step is.
const PAN_PAGE: f64 = 0.2;

/// Which value of a branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Which {
    Gain,
    Pan,
}

/// Where the value is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// The level row under the picked row of a strip.
    Strip,
    /// A branch's head in the channel editor.
    Editor,
}

/// The value that has the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Focus {
    pub track: TrackId,
    pub node: NodeId,
    pub which: Which,
    pub place: Place,
}

/// A value being typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub focus: Focus,
    pub field: Field,
    /// Nothing is typed yet: the first key replaces the value shown.
    pub fresh: bool,
}

impl Entry {
    pub fn type_text(&mut self, range: Option<std::ops::Range<usize>>, text: &str, marked: bool) {
        if std::mem::take(&mut self.fresh) {
            self.field.set(String::new());
        }
        if marked {
            self.field.mark_text(range, text);
        } else {
            self.field.replace_text(range, text);
        }
    }
}

/// A level as the field starts with it: one decimal, or "-inf" at silence.
pub fn gain_text(db: f64) -> String {
    if db <= MIN_DB {
        "-inf".into()
    } else {
        format!("{db:.1}")
    }
}

/// A pan as it is shown and typed: C, L20, R35.
pub fn pan_text(pan: f64) -> String {
    let n = (pan * 100.0).round() as i64;
    match n {
        0 => "C".into(),
        n if n < 0 => format!("L{}", -n),
        n => format!("R{n}"),
    }
}

/// F1 to F24, which a field typing a value leaves to the screen.
fn function_key(key: &str) -> bool {
    key.strip_prefix('f')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

fn minus(s: &str) -> String {
    s.trim().replace(['−', '–'], "-")
}

fn number(s: &str) -> Option<f64> {
    let ok = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        && s.chars().any(|c| c.is_ascii_digit());
    ok.then(|| s.parse::<f64>().ok()).flatten()
}

/// A typed level: "-3", "-3 dB", "-inf"; held to the levels a branch takes.
pub fn parse_gain(s: &str) -> Option<f64> {
    let s = minus(s);
    let lower = s.to_lowercase();
    let s = lower.strip_suffix("db").unwrap_or(&lower).trim();
    if matches!(s, "-inf" | "inf" | "-∞" | "∞") {
        return Some(MIN_DB);
    }
    number(s).map(|v| v.clamp(MIN_DB, MAX_DB))
}

/// A typed pan: "C", "L20", "R35", or a number from −100 to 100.
pub fn parse_pan(s: &str) -> Option<f64> {
    let s = minus(s);
    let lower = s.to_lowercase();
    let s = lower.strip_suffix('%').unwrap_or(&lower).trim();
    if matches!(s, "c" | "center" | "0") {
        return Some(0.0);
    }
    let side =
        |rest: &str, sign: f64| number(rest.trim()).map(|v| (sign * v / 100.0).clamp(-1.0, 1.0));
    if let Some(rest) = s.strip_prefix('l') {
        return side(rest, -1.0);
    }
    if let Some(rest) = s.strip_prefix('r') {
        return side(rest, 1.0);
    }
    number(s).map(|v| (v / 100.0).clamp(-1.0, 1.0))
}

/// A level moved by `by` dB: up from the floor at least, and down past the floor to silence.
pub fn step_gain(db: f64, by: f64) -> f64 {
    let v = if by > 0.0 {
        db.max(FLOOR) + by
    } else {
        db + by
    };
    if v < FLOOR {
        MIN_DB
    } else {
        ((v * 10.0).round() / 10.0).min(MAX_DB)
    }
}

fn step_pan(pan: f64, by: f64) -> f64 {
    ((pan + by) * 100.0).round().clamp(-100.0, 100.0) / 100.0
}

impl Studio {
    /// Branch `node` of `t` with its pan.
    fn branch_now(&self, t: TrackId, node: NodeId) -> Option<(Target, f64)> {
        let target = self.row_target(t, node)?;
        let p = self.project();
        let (_, pan, _) = p.tracks.get(&t)?.rack.as_ref()?.levels(node)?;
        Some((target, pan))
    }

    /// The values the keys go round with Tab, from `f`: on a strip the level row's (its pan
    /// only on a wide strip); in the editor each branch head's, left to right.
    fn value_stops(&self, f: Focus) -> Vec<Focus> {
        let at = |node, which| Focus { node, which, ..f };
        match f.place {
            Place::Strip => {
                let mut out = vec![at(f.node, Which::Gain)];
                if self.strip_pan_shown(f.track, f.node) {
                    out.push(at(f.node, Which::Pan));
                }
                out
            }
            Place::Editor => crate::channel::branch_heads(&self.project(), f.track)
                .into_iter()
                .flat_map(|(n, pan)| {
                    std::iter::once(at(n, Which::Gain)).chain(pan.then(|| at(n, Which::Pan)))
                })
                .collect(),
        }
    }

    /// Whether the strip of `t` shows `node`'s pan on its level row (a wide strip).
    pub(crate) fn strip_pan_shown(&self, t: TrackId, node: NodeId) -> bool {
        self.mixer.wide.contains(&t) && self.row_target(t, node).is_some_and(|target| !target.layer)
    }

    /// A value takes the keys.
    pub(crate) fn value_focus(&mut self, f: Focus) {
        if self
            .mixer
            .value_entry
            .as_ref()
            .is_some_and(|e| e.focus != f)
        {
            self.value_commit();
        }
        self.mixer.value_focus = Some(f);
    }

    /// The keys go to another place (F6, the palette, a menu): what is typed goes in and the
    /// value lets the keys go.
    pub(crate) fn value_let_go(&mut self) {
        self.value_commit();
        self.mixer.value_focus = None;
    }

    /// Whether `f` is on screen now: the strip's level row under its picked row (its pan only
    /// on a wide strip) in the mixer shown, or a head of the channel editor open on the track.
    pub(crate) fn value_shown(&self, f: Focus) -> bool {
        match f.place {
            Place::Strip => {
                let p = self.project();
                let Some(t) = p.tracks.get(&f.track) else {
                    return false;
                };
                self.mixer_shown()
                    && self.mixer.look().lines(t).iter().any(|l| {
                        l.kind == crate::mixer::flow::Kind::Level
                            && l.target.is_some_and(|x| x.id == f.node)
                    })
                    && (f.which == Which::Gain || self.strip_pan_shown(f.track, f.node))
            }
            Place::Editor => {
                self.channel.as_ref().is_some_and(|w| w.track == f.track)
                    && crate::channel::branch_heads(&self.project(), f.track)
                        .into_iter()
                        .any(|(n, pan)| n == f.node && (f.which == Which::Gain || pan))
            }
        }
    }

    /// The field opens on `f` with its value chosen.
    pub(crate) fn value_type(&mut self, f: Focus) -> bool {
        let Some((target, pan)) = self.branch_now(f.track, f.node) else {
            return false;
        };
        let text = match f.which {
            Which::Gain => gain_text(target.db),
            Which::Pan => pan_text(pan),
        };
        self.mixer.value_focus = Some(f);
        self.mixer.value_entry = Some(Entry {
            focus: f,
            field: Field::new(FieldId::Value, text),
            fresh: true,
        });
        true
    }

    /// What is typed goes in: one step when it reads as a value.
    pub(crate) fn value_commit(&mut self) -> bool {
        let Some(e) = self.mixer.value_entry.take() else {
            return false;
        };
        let text = e.field.text();
        let (key, v) = match e.focus.which {
            Which::Gain => ("gain", parse_gain(&text)),
            Which::Pan => ("pan", parse_pan(&text)),
        };
        match v {
            Some(v) => self.value_set(e.focus, key, v),
            None => false,
        }
    }

    fn value_set(&mut self, f: Focus, key: &str, v: f64) -> bool {
        let Some((target, pan)) = self.branch_now(f.track, f.node) else {
            return false;
        };
        let now = if key == "pan" { pan } else { target.db };
        if (now - v).abs() < 1e-9 {
            return true;
        }
        self.set_target(target, json!({ key: v }))
    }

    /// The value back to its default: 0 dB, or the middle.
    pub(crate) fn value_reset(&mut self, f: Focus) -> bool {
        match f.which {
            Which::Gain => self.value_set(f, "gain", 0.0),
            Which::Pan => self.value_set(f, "pan", 0.0),
        }
    }

    /// The keys of a branch's value (see the module).
    pub(crate) fn value_key(&mut self, key: &str, m: Mods) -> bool {
        // a value that is not on screen any more, typed or not, has let the keys go
        let held = self
            .mixer
            .value_entry
            .as_ref()
            .map(|e| e.focus)
            .or(self.mixer.value_focus);
        if held.is_some_and(|f| !self.value_shown(f)) {
            self.value_let_go();
            return false;
        }
        let blocked = self.input_blocked();
        if let Some(e) = &mut self.mixer.value_entry {
            if !e.field.marked.is_empty() {
                return true;
            }
            // F6 and the other function keys are not the field's: they put the value in and
            // go on to the screen
            if function_key(key) {
                self.value_let_go();
                return false;
            }
            if matches!(key, "enter" | "tab") && blocked {
                return true;
            }
            match key {
                "escape" => self.mixer.value_entry = None,
                "backspace" if e.fresh => {
                    e.fresh = false;
                    e.field.set(String::new());
                }
                "backspace" => e.field.backspace(),
                "enter" => {
                    self.value_commit();
                }
                "tab" if !m.ctrl && !m.alt => {
                    self.value_commit();
                    return self.value_key(key, m);
                }
                _ => {}
            }
            return true;
        }
        let Some(f) = self.mixer.value_focus else {
            return false;
        };
        let other = self.popup.is_some()
            || self.ime_target().is_some()
            || self.bar_edit.is_typing()
            || self.inspector.field.is_some()
            || self.field_typing().is_some();
        if other || m.ctrl {
            return false;
        }
        let Some((target, pan)) = self.branch_now(f.track, f.node) else {
            self.mixer.value_focus = None;
            return false;
        };
        if self
            .project()
            .tracks
            .get(&f.track)
            .is_some_and(|t| t.frozen.is_some())
        {
            return false;
        }
        let up = matches!(key, "right" | "up");
        let fine = m.shift;
        match (key, f.which) {
            ("escape", _) => self.mixer.value_focus = None,
            ("left" | "right" | "up" | "down", Which::Gain) if !m.alt => {
                let by = if fine { FINE } else { STEP };
                let db = step_gain(target.db, if up { by } else { -by });
                self.value_set(f, "gain", db);
            }
            ("left" | "right" | "up" | "down", Which::Pan) if !m.alt => {
                let by = if fine { PAN_FINE } else { PAN_STEP };
                self.value_set(f, "pan", step_pan(pan, if up { by } else { -by }));
            }
            ("pageup" | "pagedown", Which::Gain) => {
                let by = if key == "pageup" { PAGE } else { -PAGE };
                self.value_set(f, "gain", step_gain(target.db, by));
            }
            ("pageup" | "pagedown", Which::Pan) => {
                let by = if key == "pageup" { PAN_PAGE } else { -PAN_PAGE };
                self.value_set(f, "pan", step_pan(pan, by));
            }
            ("delete" | "backspace", _) => {
                self.value_reset(f);
            }
            ("enter", _) => {
                self.value_type(f);
            }
            ("m", _) if !m.alt => {
                self.row_mute(f.track, f.node);
            }
            ("tab", _) if !m.alt => {
                let stops = self.value_stops(f);
                let at = stops.iter().position(|s| *s == f);
                let next = match at {
                    Some(i) if m.shift => i.checked_sub(1),
                    Some(i) => Some(i + 1),
                    None => None,
                };
                match next.and_then(|i| stops.get(i)) {
                    Some(&n) => self.mixer.value_focus = Some(n),
                    // past the last value the keys go back to what had them before
                    None => self.mixer.value_focus = None,
                }
            }
            _ => return false,
        }
        true
    }

    /// The branch value under a point, with the rect a drag sweeps: a strip's level bar or
    /// pan, or an editor head's.
    pub(crate) fn value_at(&self, g: &Geometry, x: f32, y: f32) -> Option<(Focus, Rect)> {
        // The channel editor's own window is its own surface: its values are only there.
        if self.instruments.detached_channel {
            return self.channel_value_at(g, x, y);
        }
        if self.over_float(g, x, y) || !self.mixer_shown() {
            return None;
        }
        let (track, node, which) = match self.hit(g, x, y) {
            Hit::Mixer(MixerHit::Level(t, n)) => (t, n, Which::Gain),
            Hit::Mixer(MixerHit::BranchPan(t, n)) => (t, n, Which::Pan),
            _ => return None,
        };
        let f = Focus {
            track,
            node,
            which,
            place: Place::Strip,
        };
        let hit = match which {
            Which::Gain => MixerHit::Level(track, node),
            Which::Pan => MixerHit::BranchPan(track, node),
        };
        Some((f, self.mixer_rect(g, hit)?))
    }

    /// A press on a branch value takes the keys: with Alt it goes back to its default, a double
    /// press types it, and a pan follows the pointer from where it is. A level's single press
    /// is left to the bar's drag. True when the press is used up.
    pub(crate) fn value_press(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        clicks: u32,
        m: Mods,
    ) -> bool {
        let Some((f, r)) = self.value_at(g, x, y) else {
            return false;
        };
        let frozen = self
            .project()
            .tracks
            .get(&f.track)
            .is_none_or(|t| t.frozen.is_some());
        if frozen {
            return false;
        }
        if self
            .mixer
            .value_entry
            .as_ref()
            .is_some_and(|e| e.focus == f)
        {
            return true;
        }
        self.value_focus(f);
        if m.alt {
            self.value_reset(f);
            return true;
        }
        if clicks >= 2 {
            self.mixer.level_drag = None;
            return self.value_type(f);
        }
        match f.which {
            Which::Gain => false,
            Which::Pan => {
                let Some(target) = self.row_target(f.track, f.node) else {
                    return false;
                };
                self.mixer.level_drag = Some(LevelDrag::taken_pan(f.track, target, r, x));
                true
            }
        }
    }

    /// A press away from the branch values puts what is typed in and lets the keys go.
    pub(crate) fn value_blur(&mut self, g: &Geometry, x: f32, y: f32) {
        if self.mixer.value_focus.is_none() && self.mixer.value_entry.is_none() {
            return;
        }
        if self.value_at(g, x, y).is_some() {
            return;
        }
        self.value_commit();
        self.mixer.value_focus = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_045_typed_levels_and_pans_read_as_the_mock_writes_them() {
        assert_eq!(parse_gain("-3"), Some(-3.0));
        assert_eq!(parse_gain("-3 dB"), Some(-3.0));
        assert_eq!(parse_gain(" −4.5dB "), Some(-4.5));
        assert_eq!(parse_gain("-inf"), Some(MIN_DB));
        assert_eq!(parse_gain("40"), Some(MAX_DB));
        assert_eq!(parse_gain("loud"), None);
        assert_eq!(parse_gain(""), None);
        assert_eq!(parse_pan("C"), Some(0.0));
        assert_eq!(parse_pan("L20"), Some(-0.2));
        assert_eq!(parse_pan("r35"), Some(0.35));
        assert_eq!(parse_pan("-50"), Some(-0.5));
        assert_eq!(parse_pan("R200"), Some(1.0));
        assert_eq!(parse_pan("up"), None);
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-0.2), "L20");
        assert_eq!(pan_text(0.35), "R35");
        assert_eq!(gain_text(MIN_DB), "-inf");
        assert_eq!(gain_text(-3.0), "-3.0");
    }

    #[test]
    fn mix_045_steps_go_through_the_floor_to_silence_and_back() {
        assert_eq!(step_gain(0.0, 1.0), 1.0);
        assert_eq!(step_gain(0.0, -0.1), -0.1);
        assert_eq!(step_gain(-59.5, -1.0), MIN_DB);
        assert_eq!(step_gain(MIN_DB, 1.0), FLOOR + 1.0);
        assert_eq!(step_gain(10.0, 6.0), MAX_DB);
        assert_eq!(step_pan(0.98, 0.05), 1.0);
        assert_eq!(step_pan(0.0, -0.01), -0.01);
    }
}
