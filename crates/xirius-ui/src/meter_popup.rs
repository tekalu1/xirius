//! The meter of the top bar (UI-119): a pop-over with the usual meters to pick from
//! (the one in effect is checked) and, under them, a numerator field and a
//! denominator stepper with a button that sets what they say. Every way ends in one
//! `timesig.set` at the bar where the meter in effect at the playhead begins, so the
//! CLI and MCP do the same under the same name.

use serde_json::json;
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::popup::{Action, Cell, Look, PopupKind};
use crate::studio::{Rect, Studio};

pub const W: f32 = 192.0;
/// The meters of the list.
pub const CANDIDATES: [(u32, u32); 7] = [(2, 4), (3, 4), (4, 4), (5, 4), (6, 8), (7, 8), (12, 8)];
/// The denominators the song takes (`timesig.set`).
pub const DENS: [u32; 6] = [1, 2, 4, 8, 16, 32];
pub const MAX_NUM: u32 = 64;
const FIELD_H: f32 = 28.0;
const NUM_W: f32 = 44.0;
const SLASH_W: f32 = 16.0;
const STEP_W: f32 = 24.0;
const DEN_W: f32 = 30.0;
const OK_W: f32 = 36.0;

/// What the pop-over shows: the meter in effect and what the fields say.
#[derive(Clone, Debug, PartialEq)]
pub struct MeterView {
    /// The bar where the meter in effect begins.
    pub bar: u32,
    pub now: (u32, u32),
    /// The numerator as it is typed.
    pub num: String,
    pub den: u32,
    /// The numerator is still selected: the first key replaces it.
    pub fresh: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MAct {
    /// A meter of the list: sets it.
    Pick(u32, u32),
    /// The numerator field: selects what is in it.
    Num,
    /// The denominator one step up or down.
    Step(bool),
    /// Sets what the fields say.
    Apply,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MLook {
    Num,
    Slash,
    Den,
    Rule,
}

pub fn label(num: u32, den: u32) -> String {
    format!("{num}/{den}")
}

pub fn cells(v: Option<&MeterView>) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    let inner = W - pad * 2.0;
    let Some(v) = v else {
        return Vec::new();
    };
    let mut cells = Vec::new();
    let mut y = pad;
    for (num, den) in CANDIDATES {
        cells.push(
            Cell::new(Rect::new(pad, y, inner, Layout::MENU_ROW_H), Look::Row)
                .text(label(num, den))
                .on(v.now == (num, den))
                .act(Action::Meter(MAct::Pick(num, den))),
        );
        y += Layout::MENU_ROW_H;
    }
    y += Space::X1.px();
    cells.push(Cell::new(
        Rect::new(pad, y, inner, 1.0),
        Look::Meter(MLook::Rule),
    ));
    y += 1.0 + Space::X2.px();
    let mut x = pad;
    cells.push(
        Cell::new(Rect::new(x, y, NUM_W, FIELD_H), Look::Meter(MLook::Num))
            .text(v.num.clone())
            .on(v.fresh)
            .act(Action::Meter(MAct::Num)),
    );
    x += NUM_W;
    cells.push(Cell::new(
        Rect::new(x, y, SLASH_W, FIELD_H),
        Look::Meter(MLook::Slash),
    ));
    x += SLASH_W;
    let step = |x: f32, text: &str, up: bool, can: bool| {
        let c = Cell::new(Rect::new(x, y + 2.0, STEP_W, FIELD_H - 4.0), Look::Button).text(text);
        if can {
            c.act(Action::Meter(MAct::Step(up)))
        } else {
            c.off()
        }
    };
    let at = DENS.iter().position(|d| *d == v.den).unwrap_or(2);
    cells.push(step(x, "−", false, at > 0));
    x += STEP_W;
    cells.push(
        Cell::new(Rect::new(x, y, DEN_W, FIELD_H), Look::Meter(MLook::Den)).text(v.den.to_string()),
    );
    x += DEN_W;
    cells.push(step(x, "+", true, at + 1 < DENS.len()));
    cells.push(
        Cell::new(
            Rect::new(W - pad - OK_W, y + 2.0, OK_W, FIELD_H - 4.0),
            Look::Primary,
        )
        .icon("check")
        .act(Action::Meter(MAct::Apply)),
    );
    cells
}

pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: MLook) {
    let c = pen.c;
    let r = cell.rect;
    match look {
        MLook::Rule => pen.fill(r, c.surface_3),
        MLook::Slash => {
            pen.text(
                "/",
                FontSize::Small,
                Weight::Medium,
                c.ink,
                r,
                Align::Center,
            );
        }
        MLook::Den => {
            pen.digits(
                &cell.text,
                FontSize::Small,
                Weight::Medium,
                c.ink_strong,
                r,
                Align::Center,
            );
        }
        MLook::Num => {
            pen.round(r, c.surface_2, Radius::S);
            let w = crate::browser::text_width(&cell.text, FontSize::Small).max(6.0);
            let x = r.x + (r.w - w) / 2.0;
            if cell.on {
                pen.round(
                    Rect::new(x - 3.0, r.y + 6.0, w + 6.0, r.h - 12.0),
                    c.fill,
                    Radius::Xs,
                );
            }
            pen.digits(
                &cell.text,
                FontSize::Small,
                Weight::Medium,
                if cell.on { c.on_fill } else { c.ink_strong },
                r,
                Align::Center,
            );
            if !cell.on {
                pen.fill(
                    Rect::new(x + w + 1.0, r.y + 7.0, 1.5, r.h - 14.0),
                    c.ink_strong,
                );
            }
        }
    }
}

impl Studio {
    /// The meter in effect at the playhead: the bar where it begins and what it is.
    pub(crate) fn meter_target(&self) -> (u32, (u32, u32)) {
        let p = self.project();
        let bar = p.time_map().to_bbt(self.playhead).0;
        p.timesig
            .iter()
            .rev()
            .find(|s| s.bar <= bar)
            .map_or((1, (4, 4)), |s| (s.bar, (s.num, s.den)))
    }

    /// Opens the fields on the meter in effect, or follows it when it changed.
    pub(crate) fn refresh_meter(&mut self) {
        if self.popup.as_ref().map(|p| p.kind) != Some(PopupKind::Meter) {
            return;
        }
        let (bar, now) = self.meter_target();
        if let Some(p) = &mut self.popup {
            match &mut p.meter {
                Some(m) => {
                    m.bar = bar;
                    m.now = now;
                }
                None => {
                    p.meter = Some(MeterView {
                        bar,
                        now,
                        num: now.0.to_string(),
                        den: now.1,
                        fresh: true,
                    });
                }
            }
        }
    }

    /// Sets the meter that begins at the bar in effect; nothing happens when it
    /// already is that.
    pub(crate) fn set_meter(&mut self, num: u32, den: u32) -> bool {
        let (bar, now) = self.meter_target();
        if (num, den) == now {
            return false;
        }
        self.run_command("timesig.set", json!({ "bar": bar, "num": num, "den": den }))
    }

    fn meter_apply(&mut self) -> bool {
        let Some(m) = self.popup.as_ref().and_then(|p| p.meter.clone()) else {
            return false;
        };
        let num = m
            .num
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|n| *n >= 1)
            .map_or(m.now.0, |n| n.min(MAX_NUM));
        self.popup = None;
        self.set_meter(num, m.den);
        true
    }

    pub(crate) fn meter_action(&mut self, act: MAct) -> bool {
        match act {
            MAct::Pick(num, den) => {
                self.popup = None;
                self.set_meter(num, den);
            }
            MAct::Num => {
                if let Some(m) = self.popup.as_mut().and_then(|p| p.meter.as_mut()) {
                    m.fresh = true;
                }
            }
            MAct::Step(up) => self.meter_step(up),
            MAct::Apply => return self.meter_apply(),
        }
        true
    }

    fn meter_step(&mut self, up: bool) {
        if let Some(m) = self.popup.as_mut().and_then(|p| p.meter.as_mut()) {
            let at = DENS.iter().position(|d| *d == m.den).unwrap_or(2);
            let to = if up {
                (at + 1).min(DENS.len() - 1)
            } else {
                at.saturating_sub(1)
            };
            m.den = DENS[to];
        }
    }

    /// A key while the meter's pop-over is open: digits go to the numerator, the
    /// arrows step the denominator, Enter sets, Esc closes. None when it is not open.
    pub(crate) fn meter_typed(&mut self, key: &str, ch: Option<&str>) -> Option<bool> {
        if self.popup.as_ref().map(|p| p.kind) != Some(PopupKind::Meter) {
            return None;
        }
        match key {
            "escape" => self.popup = None,
            "enter" => return Some(self.meter_apply()),
            "up" | "down" => self.meter_step(key == "up"),
            "backspace" => {
                if let Some(m) = self.popup.as_mut().and_then(|p| p.meter.as_mut()) {
                    if m.fresh {
                        m.num.clear();
                        m.fresh = false;
                    } else {
                        m.num.pop();
                    }
                }
            }
            _ => {
                let Some(c) = ch.filter(|c| c.len() == 1 && c.chars().all(|c| c.is_ascii_digit()))
                else {
                    return Some(false);
                };
                if let Some(m) = self.popup.as_mut().and_then(|p| p.meter.as_mut()) {
                    if m.fresh {
                        m.num.clear();
                        m.fresh = false;
                    }
                    if m.num.len() < 2 {
                        m.num.push_str(c);
                    }
                }
            }
        }
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::{Geometry, Hit};
    use crate::topbar::BarButton;

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = false;
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    fn open(s: &mut Studio, g: &Geometry) {
        let r = s
            .center_of(g, Hit::Bar(BarButton::Meter))
            .expect("the meter is on the bar");
        let hit = s.hit(g, r.0, r.1);
        assert_eq!(hit, Hit::Bar(BarButton::Meter));
        assert!(s.press(g, hit));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Meter));
    }

    fn cells_of(s: &Studio, g: &Geometry) -> Vec<Cell> {
        s.popup.as_ref().expect("a popup").layout(g.size).cells
    }

    /// Presses the live cell that says `text` (the nth of them), where it is on screen.
    fn press_text(s: &mut Studio, g: &Geometry, text: &str) {
        let cells = cells_of(s, g);
        let i = cells
            .iter()
            .position(|c| c.text == text && c.live())
            .unwrap_or_else(|| panic!("no live {text}"));
        let r = cells[i].rect;
        let hit = s.hit(g, r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert_eq!(hit, Hit::Item(i));
        assert!(s.press(g, hit));
    }

    fn press_ok(s: &mut Studio, g: &Geometry) {
        let cells = cells_of(s, g);
        let i = cells
            .iter()
            .position(|c| c.action == Some(Action::Meter(MAct::Apply)))
            .expect("a set button");
        let r = cells[i].rect;
        let hit = s.hit(g, r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert_eq!(hit, Hit::Item(i));
        assert!(s.press(g, hit));
    }

    fn meter(s: &Studio) -> (u32, u32) {
        s.meter_target().1
    }

    #[test]
    fn the_meter_opens_a_list_with_the_one_in_effect_checked() {
        let (mut s, g) = studio();
        open(&mut s, &g);
        let cells = cells_of(&s, &g);
        let rows: Vec<(&str, bool)> = cells
            .iter()
            .filter(|c| c.look == Look::Row)
            .map(|c| (c.text.as_str(), c.on))
            .collect();
        assert_eq!(
            rows,
            [
                ("2/4", false),
                ("3/4", false),
                ("4/4", true),
                ("5/4", false),
                ("6/8", false),
                ("7/8", false),
                ("12/8", false),
            ]
        );
        // The fields start on the meter in effect, the numerator selected.
        let fields: Vec<&str> = cells
            .iter()
            .filter(|c| matches!(c.look, Look::Meter(MLook::Num | MLook::Den)))
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(fields, ["4", "4"]);
        assert!(
            cells
                .iter()
                .any(|c| c.look == Look::Meter(MLook::Num) && c.on)
        );
        // The popup stays inside the body.
        let rect = s.popup.as_ref().unwrap().layout(g.size).rect;
        assert!(rect.x >= 0.0 && rect.right() <= g.size.0);
    }

    #[test]
    fn a_meter_of_the_list_is_set_in_one_step_and_the_popup_closes() {
        let (mut s, g) = studio();
        open(&mut s, &g);
        let before = s.song.history().len();
        press_text(&mut s, &g, "6/8");
        assert!(s.popup.is_none());
        assert_eq!(meter(&s), (6, 8));
        assert_eq!(s.song.history().len(), before + 1);
        assert_eq!(
            s.project().timesig.len(),
            1,
            "the same point, not a new one"
        );
        // The readout of the bar says it.
        assert_eq!(s.bar_view().meter, "6/8");
        // The one in effect is checked now, and picking it again changes nothing.
        open(&mut s, &g);
        let on: Vec<String> = cells_of(&s, &g)
            .iter()
            .filter(|c| c.look == Look::Row && c.on)
            .map(|c| c.text.clone())
            .collect();
        assert_eq!(on, ["6/8"]);
        let before = s.song.history().len();
        press_text(&mut s, &g, "6/8");
        assert_eq!(s.song.history().len(), before);
    }

    #[test]
    fn the_fields_take_a_numerator_and_step_the_denominator() {
        let (mut s, g) = studio();
        open(&mut s, &g);
        // The first digit replaces the selected numerator.
        assert!(s.typed(&g, "1", Some("1")));
        assert!(s.typed(&g, "1", Some("1")));
        press_text(&mut s, &g, "+");
        press_text(&mut s, &g, "+");
        let texts: Vec<String> = cells_of(&s, &g)
            .iter()
            .filter(|c| matches!(c.look, Look::Meter(MLook::Num | MLook::Den)))
            .map(|c| c.text.clone())
            .collect();
        assert_eq!(texts, ["11", "16"]);
        press_ok(&mut s, &g);
        assert!(s.popup.is_none());
        assert_eq!(meter(&s), (11, 16));
        assert_eq!(s.bar_view().meter, "11/16");
    }

    #[test]
    fn enter_sets_the_fields_escape_leaves_and_the_numerator_is_kept_in_range() {
        let (mut s, g) = studio();
        open(&mut s, &g);
        s.typed(&g, "9", Some("9"));
        s.typed(&g, "9", Some("9"));
        assert!(s.typed(&g, "down", None));
        assert!(s.typed(&g, "enter", None));
        assert_eq!(meter(&s), (MAX_NUM, 2), "99 is kept to the longest bar");

        // Esc leaves everything as it was.
        open(&mut s, &g);
        s.typed(&g, "5", Some("5"));
        assert!(s.typed(&g, "escape", None));
        assert!(s.popup.is_none());
        assert_eq!(meter(&s), (MAX_NUM, 2));

        // An empty numerator keeps the numerator it had; letters are not taken.
        open(&mut s, &g);
        assert!(s.typed(&g, "backspace", None));
        assert!(!s.typed(&g, "a", Some("a")));
        assert!(!s.typed(&g, "0", Some("٠")));
        s.typed(&g, "0", Some("0"));
        assert!(s.typed(&g, "enter", None));
        assert_eq!(meter(&s), (MAX_NUM, 2), "0 is not a meter");
    }

    #[test]
    fn the_denominator_stops_at_the_ends_and_a_numerator_press_selects_it() {
        let (mut s, g) = studio();
        open(&mut s, &g);
        for _ in 0..10 {
            s.typed(&g, "down", None);
        }
        let cells = cells_of(&s, &g);
        let minus = cells.iter().find(|c| c.text == "−").unwrap();
        assert!(!minus.live(), "1 is the least");
        assert_eq!(
            cells
                .iter()
                .find(|c| c.look == Look::Meter(MLook::Den))
                .unwrap()
                .text,
            "1"
        );
        s.typed(&g, "7", Some("7"));
        s.typed(&g, "8", Some("8"));
        assert!(!s.popup.as_ref().unwrap().meter.as_ref().unwrap().fresh);
        let cells = cells_of(&s, &g);
        let i = cells
            .iter()
            .position(|c| c.look == Look::Meter(MLook::Num))
            .unwrap();
        let r = cells[i].rect;
        let hit = s.hit(&g, r.x + 4.0, r.y + 4.0);
        assert_eq!(hit, Hit::Item(i));
        assert!(s.press(&g, hit));
        assert!(s.popup.as_ref().unwrap().meter.as_ref().unwrap().fresh);
    }

    #[test]
    fn the_meter_in_effect_is_the_one_at_the_playhead() {
        let (mut s, g) = studio();
        assert!(s.run_command(
            "timesig.set",
            serde_json::json!({"bar": 3, "num": 3, "den": 4})
        ));
        s.playhead = 0;
        assert_eq!(s.meter_target(), (1, (4, 4)));
        open(&mut s, &g);
        press_text(&mut s, &g, "5/4");
        assert_eq!(s.project().timesig.len(), 2);
        // The playhead is in bar 5 now: that is the point of bar 3 that changes.
        let bar5 = s.project().time_map().bar_start(5);
        s.playhead = bar5;
        assert_eq!(s.meter_target(), (3, (3, 4)));
        open(&mut s, &g);
        press_text(&mut s, &g, "7/8");
        assert_eq!(s.project().timesig.len(), 2);
        s.playhead = 0;
        assert_eq!(meter(&s), (5, 4));
        assert_eq!(s.bar_view().meter, "5/4");
    }

    #[test]
    fn pressing_the_meter_again_or_elsewhere_closes_it() {
        let (mut s, g) = studio();
        open(&mut s, &g);
        let r = s
            .center_of(&g, Hit::Bar(BarButton::Meter))
            .expect("the meter is on the bar");
        let hit = s.hit(&g, r.0, r.1);
        s.press(&g, hit);
        assert!(s.popup.is_none());
    }
}
