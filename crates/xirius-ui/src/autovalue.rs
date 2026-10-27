//! The value typed into an automation point (AUT-008). 値を入力… of the point's menu
//! (`context/auto.rs`) turns into a small box: the lane's name and unit over the value,
//! shown selected so typing replaces it. Enter sets the point to what was typed
//! (`auto.point.set`, same place; every selected point of the lane when the point is
//! one of several); Esc closes the box.

use serde_json::json;
use xirius_core::auto::{AutoPoint, AutoTarget};
use xirius_core::id::TrackId;
use xirius_core::model::{MAX_DB, MIN_DB};
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::arrange::{lane_name, value_text};
use crate::browser::text_width;
use crate::draw::{Align, Pen};
use crate::popup::{Cell, LABEL_H, Look, PopupKind};
use crate::studio::{Rect, Studio};

/// The bend 曲線 gives the line to the next point.
pub const CURVE: f64 = 0.6;
const INPUT_H: f32 = 28.0;

/// A point: its track, its lane (an index into the track's lanes) and its place in the lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoRef {
    pub track: TrackId,
    pub lane: usize,
    pub index: usize,
}

/// What the box shows of the point.
#[derive(Clone, Debug, PartialEq)]
pub struct PointView {
    /// What the lane moves (`音量`, `センド · Reverb`).
    pub name: String,
    pub unit: &'static str,
    /// The value as it is typed (`-6.0`, `L30`, `1`, `50`).
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoLook {
    /// The value being typed; `hint` holds the value it had, shown selected.
    Value,
}

/// The unit the box asks for.
pub fn unit(target: &AutoTarget) -> &'static str {
    match target {
        AutoTarget::Volume | AutoTarget::Send(_) => "dB",
        AutoTarget::Pan => "L−100 … R+100",
        AutoTarget::Mute => "0 か 1",
        AutoTarget::Param(..) => "%",
    }
}

/// A value as the box shows it, in its unit.
pub fn text(target: &AutoTarget, v: f64) -> String {
    match target {
        AutoTarget::Volume | AutoTarget::Send(_) => {
            if v <= MIN_DB {
                "-∞".into()
            } else {
                format!("{v:.1}")
            }
        }
        AutoTarget::Pan => value_text(target, v),
        AutoTarget::Mute => if v >= 0.5 { "1" } else { "0" }.into(),
        AutoTarget::Param(..) => format!("{}", (v * 100.0).round()),
    }
}

/// What was typed, as a value of the lane; None when there is no number in it.
/// Full-width digits and signs are read as they would be typed half-width.
pub fn parse(target: &AutoTarget, typed: &str) -> Option<f64> {
    let s: String = typed
        .trim()
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from(b'0' + (c as u32 - '０' as u32) as u8),
            '－' | '−' | '‐' => '-',
            '＋' => '+',
            '．' => '.',
            c => c.to_ascii_uppercase(),
        })
        .collect();
    match target {
        AutoTarget::Volume | AutoTarget::Send(_) => {
            if s.contains('∞') || s.contains("INF") {
                return Some(MIN_DB);
            }
            number(&s).map(|v| v.clamp(MIN_DB, MAX_DB))
        }
        AutoTarget::Pan => {
            if s == "C" {
                return Some(0.0);
            }
            let n = number(&s)?;
            let n = if s.starts_with('L') {
                -n.abs()
            } else if s.starts_with('R') {
                n.abs()
            } else {
                n
            };
            Some((n / 100.0).clamp(-1.0, 1.0))
        }
        AutoTarget::Mute => number(&s).map(|n| if n >= 0.5 { 1.0 } else { 0.0 }),
        AutoTarget::Param(..) => number(&s).map(|n| (n / 100.0).clamp(0.0, 1.0)),
    }
}

/// The number in a text, its unit and side letters left out.
fn number(s: &str) -> Option<f64> {
    let digits: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
        .collect();
    digits.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The box: `音量（dB）` over the value. `typed` is None until a key is pressed.
pub fn value_cells(v: Option<&PointView>, typed: Option<&str>, w: f32) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    let inner = w - pad * 2.0;
    let Some(v) = v else {
        return Vec::new();
    };
    let typed = typed.unwrap_or_default();
    vec![
        Cell::new(Rect::new(pad + 2.0, pad, inner, LABEL_H), Look::Label)
            .text(format!("{}（{}）", v.name, v.unit)),
        Cell::new(
            Rect::new(pad, pad + LABEL_H, inner, INPUT_H),
            Look::Auto(AutoLook::Value),
        )
        .text(typed)
        .hint(v.value.clone()),
    ]
}

pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: AutoLook) {
    let c = pen.c;
    let r = cell.rect;
    let pad = Space::X2.px();
    match look {
        AutoLook::Value => {
            pen.round(r, c.surface_2, Radius::S);
            let tx = r.x + pad;
            let area = Rect::new(tx, r.y, r.w - pad * 2.0, r.h);
            // The value it had, selected: the first key replaces it.
            let (text, selected) = if cell.text.is_empty() {
                (cell.hint.as_str(), !cell.hint.is_empty())
            } else {
                (cell.text.as_str(), false)
            };
            if selected {
                let w = text_width(text, FontSize::Small);
                pen.round(
                    Rect::new(tx - 2.0, r.y + 6.0, w + 4.0, r.h - 12.0),
                    c.sel,
                    Radius::Xs,
                );
            }
            let w = pen.text(
                text,
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                area,
                Align::Left,
            );
            pen.fill(
                Rect::new(tx + w + 1.0, r.y + 7.0, 1.5, r.h - 14.0),
                c.ink_strong,
            );
        }
    }
}

impl Studio {
    /// The point's lane and the point.
    fn auto_point(&self, at: AutoRef) -> Option<(AutoTarget, AutoPoint)> {
        let p = self.project();
        let lane = p.tracks.get(&at.track)?.automation.get(at.lane)?;
        let pt = lane.points.get(at.index)?;
        Some((lane.target.clone(), *pt))
    }

    pub(crate) fn point_view(&self, at: AutoRef) -> Option<PointView> {
        let p = self.project();
        let t = p.tracks.get(&at.track)?;
        let (target, pt) = self.auto_point(at)?;
        Some(PointView {
            name: lane_name(&p, t, &target),
            unit: unit(&target),
            value: text(&target, pt.value),
        })
    }

    /// 値を入力…: the box over the point, where the menu was.
    pub(crate) fn auto_value_open(&mut self, at: AutoRef, x: f32, y: f32) -> bool {
        if self.auto_point(at).is_none() {
            return false;
        }
        self.open_popup(PopupKind::AutoValue(at), Rect::new(x, y, 0.0, 0.0));
        if let Some(p) = &mut self.popup {
            p.naming = Some(String::new());
        }
        true
    }

    /// A key while the value box is open: Enter sets the point to what was
    /// typed, Esc closes. None when the box is not open.
    pub(crate) fn auto_typed(&mut self, key: &str, ch: Option<&str>) -> Option<bool> {
        let Some(PopupKind::AutoValue(at)) = self.popup.as_ref().map(|p| p.kind) else {
            return None;
        };
        let p = self.popup.as_mut()?;
        let typed = p.naming.get_or_insert_with(String::new);
        match key {
            "escape" => {
                self.popup = None;
            }
            "enter" => {
                let typed = typed.trim().to_string();
                self.popup = None;
                if let Some((target, pt)) = self.auto_point(at)
                    && !typed.is_empty()
                    && let Some(v) = parse(&target, &typed)
                    && v != pt.value
                {
                    // A point among several selected ones: they all take the value.
                    let together = self.arrange.points.has(at.track, &target, pt.tick)
                        && self.arrange.points.ticks().len() > 1;
                    let ticks: Vec<String> = if together {
                        self.arrange
                            .points
                            .ticks()
                            .iter()
                            .map(ToString::to_string)
                            .collect()
                    } else {
                        vec![pt.tick.to_string()]
                    };
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.run_command(
                        "auto.point.set",
                        json!({ "track": at.track.to_string(), "target": target.to_string(),
                                "ticks": ticks.join(","), "value": v }),
                    );
                }
            }
            // On the selected value, Backspace takes it all away.
            "backspace" if typed.is_empty() => {
                if let Some(v) = &mut p.auto {
                    v.value.clear();
                }
            }
            _ => return Some(p.typed(key, ch)),
        }
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrange::probe::{point_xy, two_lanes};
    use crate::studio::{Geometry, Hit};

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = false;
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    fn texts(s: &Studio, g: &Geometry) -> Vec<String> {
        let p = s.popup.as_ref().expect("a popup");
        p.layout(g.size)
            .cells
            .into_iter()
            .map(|c| if c.text.is_empty() { c.hint } else { c.text })
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// Presses the popup's button that says `text`, where it is on screen.
    fn press(s: &mut Studio, g: &Geometry, text: &str) {
        let layout = s.popup.as_ref().expect("a popup").layout(g.size);
        let i = layout
            .cells
            .iter()
            .position(|c| c.text == text && c.live())
            .unwrap_or_else(|| panic!("no {text}"));
        let r = layout.cells[i].rect;
        let hit = s.hit(g, r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert_eq!(hit, Hit::Item(i));
        assert!(s.press(g, hit));
    }

    fn type_in(s: &mut Studio, g: &Geometry, typed: &str) {
        for c in typed.chars() {
            let c = c.to_string();
            assert!(s.typed(g, &c, Some(&c)));
        }
    }

    fn point(s: &Studio, t: TrackId, target: &AutoTarget, i: usize) -> (i64, f64, f64) {
        let p = s.project();
        let l = p.tracks[&t]
            .automation
            .iter()
            .find(|l| l.target == *target)
            .unwrap();
        (l.points[i].tick, l.points[i].value, l.points[i].curve)
    }

    /// Opens the box of a point: the right press, then the row 値を入力….
    fn open_box(s: &mut Studio, g: &Geometry, x: f32, y: f32) {
        assert!(s.scene_context(g, x, y));
        press(s, g, "値を入力…");
    }

    #[test]
    fn a_points_value_is_typed_from_its_menu_and_enter_sets_it() {
        let (mut s, g) = studio();
        let t = two_lanes(&mut s, &g, "Bass");
        let vol = AutoTarget::Volume;
        let (tick, _, _) = point(&s, t, &vol, 1);
        let (x, y) = point_xy(&s, &g, t, 0, 1);
        open_box(&mut s, &g, x, y);
        assert!(matches!(
            s.popup.as_ref().map(|p| p.kind),
            Some(PopupKind::AutoValue(_))
        ));
        // The heading, and the value it has, selected.
        assert_eq!(texts(&s, &g), ["ボリューム（dB）", "0.0"]);
        let before = s.song.history().len();
        type_in(&mut s, &g, "-6.5");
        assert_eq!(texts(&s, &g), ["ボリューム（dB）", "-6.5"]);
        assert!(s.typed(&g, "enter", None));
        assert!(s.popup.is_none());
        assert_eq!(point(&s, t, &vol, 1), (tick, -6.5, 0.0));
        assert_eq!(s.song.history().len(), before + 1);

        // Esc closes the box and leaves the point.
        let (x, y) = point_xy(&s, &g, t, 0, 1);
        open_box(&mut s, &g, x, y);
        type_in(&mut s, &g, "3");
        assert!(s.typed(&g, "escape", None));
        assert!(s.popup.is_none());
        assert_eq!(point(&s, t, &vol, 1).1, -6.5);

        // Backspace takes the selected value away; Enter on nothing changes nothing.
        let (x, y) = point_xy(&s, &g, t, 0, 1);
        open_box(&mut s, &g, x, y);
        assert!(s.typed(&g, "backspace", None));
        assert_eq!(texts(&s, &g), ["ボリューム（dB）"]);
        assert!(s.typed(&g, "enter", None));
        assert!(s.popup.is_none());
        assert_eq!(point(&s, t, &vol, 1).1, -6.5);
        assert_eq!(s.song.history().len(), before + 1);
    }

    #[test]
    fn a_pan_point_takes_l_and_r() {
        let (mut s, g) = studio();
        let t = two_lanes(&mut s, &g, "Bass");
        let pan = AutoTarget::Pan;
        let (x, y) = point_xy(&s, &g, t, 1, 0);
        open_box(&mut s, &g, x, y);
        assert_eq!(texts(&s, &g), ["パン（L−100 … R+100）", "L60"]);
        type_in(&mut s, &g, "R25");
        s.typed(&g, "enter", None);
        assert_eq!(point(&s, t, &pan, 0).1, 0.25);
    }

    #[test]
    fn typed_values_are_read_in_the_lanes_units() {
        let vol = AutoTarget::Volume;
        assert_eq!(parse(&vol, "-6"), Some(-6.0));
        assert_eq!(parse(&vol, " -6.5 dB "), Some(-6.5));
        assert_eq!(parse(&vol, "－３"), Some(-3.0));
        assert_eq!(parse(&vol, "+40"), Some(MAX_DB));
        assert_eq!(parse(&vol, "-∞"), Some(MIN_DB));
        assert_eq!(parse(&vol, "-inf"), Some(MIN_DB));
        assert_eq!(parse(&vol, "dB"), None);
        let pan = AutoTarget::Pan;
        assert_eq!(parse(&pan, "L30"), Some(-0.3));
        assert_eq!(parse(&pan, "r 100"), Some(1.0));
        assert_eq!(parse(&pan, "-50"), Some(-0.5));
        assert_eq!(parse(&pan, "c"), Some(0.0));
        assert_eq!(parse(&pan, "L200"), Some(-1.0));
        assert_eq!(parse(&AutoTarget::Mute, "1"), Some(1.0));
        assert_eq!(parse(&AutoTarget::Mute, "0"), Some(0.0));
        let param = AutoTarget::Param(xirius_core::id::PluginId(1), "cutoff".into());
        assert_eq!(parse(&param, "25%"), Some(0.25));
        assert_eq!(parse(&param, "150"), Some(1.0));
    }

    #[test]
    fn the_box_shows_the_value_as_it_would_be_typed() {
        let vol = AutoTarget::Volume;
        assert_eq!(text(&vol, -6.0), "-6.0");
        assert_eq!(text(&vol, MIN_DB), "-∞");
        assert_eq!(text(&AutoTarget::Pan, -0.3), "L30");
        assert_eq!(text(&AutoTarget::Pan, 0.0), "C");
        assert_eq!(text(&AutoTarget::Mute, 1.0), "1");
        let param = AutoTarget::Param(xirius_core::id::PluginId(1), "cutoff".into());
        assert_eq!(text(&param, 0.5), "50");
        // What the box shows reads back as the same value.
        for (t, v) in [(vol, -6.0), (AutoTarget::Pan, -0.3), (param, 0.5)] {
            assert_eq!(parse(&t, &text(&t, v)), Some(v));
        }
        assert_eq!(unit(&AutoTarget::Pan), "L−100 … R+100");
        assert_eq!(unit(&AutoTarget::Mute), "0 か 1");
    }
}
