//! The record panel (REC-007, UI-086): the row under the top bar that holds what a recording
//! needs besides the buttons — auto-punch, pre-roll, post-roll, pre-count and input quantize.
//! The mock draws it as a strip of switches, each with its label, and a little field for the
//! number of bars after the ones that count in bars. The top bar's button at its right end shows
//! and hides it; the pre-roll switch is also the button beside the metronome.
//!
//! Every switch and field runs the settings command of the same name (`record.punch`,
//! `record.preroll`, `record.postroll`, `record.count-in`, `record.input-quantize`), so the
//! panel, the palette, the CLI and MCP change the same values. The post-roll only exists while
//! auto-punch is on.

use serde_json::json;
use xirius_design::{FontSize, Layout, Radius, Weight};

use crate::draw::{Align, Pen};
use crate::dropdown::{self, Face};
use crate::popup::{Action, Cell, Choice, Look, PopupKind};
use crate::studio::{Geometry, Hit, Rect, Studio};

/// How high the panel is.
pub const H: f32 = 40.0;
/// A switch, as the settings draw it.
const SWITCH_W: f32 = 34.0;
const SWITCH_H: f32 = 20.0;
/// The field for a number of bars, and the box of the input quantize's value.
const BARS_W: f32 = 30.0;
const FIELD_H: f32 = 22.0;
const VALUE_W: f32 = 72.0;
/// The room between a switch and its label, and between the groups.
const LABEL_GAP: f32 = 8.0;
const GROUP_GAP: f32 = 22.0;
const PAD_X: f32 = 12.0;
/// What follows a bars field.
const BARS_UNIT: &str = "小節";

/// The values of the input quantize's box, as the setting names them.
pub const QUANTIZE_VALUES: [&str; 4] = ["1/4", "1/8", "1/16", "1/8T"];

/// A part of the panel that takes a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecPart {
    Punch,
    PreRoll,
    PreRollBars,
    PostRoll,
    PostRollBars,
    Count,
    CountBars,
    Quantize,
    QuantizeValue,
}

impl RecPart {
    pub const ALL: [RecPart; 9] = [
        RecPart::Punch,
        RecPart::PreRoll,
        RecPart::PreRollBars,
        RecPart::PostRoll,
        RecPart::PostRollBars,
        RecPart::Count,
        RecPart::CountBars,
        RecPart::Quantize,
        RecPart::QuantizeValue,
    ];

    /// The settings command a switch runs; None for a field.
    pub fn switch_command(self) -> Option<&'static str> {
        match self {
            RecPart::Punch => Some("record.punch"),
            RecPart::PreRoll => Some("record.preroll"),
            RecPart::PostRoll => Some("record.postroll"),
            RecPart::Count => Some("record.count-in"),
            RecPart::Quantize => Some("record.input-quantize"),
            _ => None,
        }
    }

    /// The command a bars field sets (its argument is `bars`).
    fn bars_command(self) -> Option<&'static str> {
        match self {
            RecPart::PreRollBars => Some("record.preroll"),
            RecPart::PostRollBars => Some("record.postroll"),
            RecPart::CountBars => Some("record.count-in"),
            _ => None,
        }
    }

    /// The command the part stands for, for its tip and the operations table.
    pub fn command(self) -> &'static str {
        self.switch_command()
            .or(self.bars_command())
            .unwrap_or("record.input-quantize")
    }

    fn is_bars(self) -> bool {
        self.bars_command().is_some()
    }
}

/// What the panel shows: the settings it stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct RecView {
    pub punch: bool,
    pub preroll: bool,
    pub preroll_bars: u32,
    pub postroll: bool,
    pub postroll_bars: u32,
    /// The pre-count's bars; 0 when it is off (the field then shows 1, dim).
    pub count_bars: u32,
    pub quantize: bool,
    pub quantize_value: String,
}

impl RecView {
    pub fn of(studio: &Studio) -> RecView {
        let Ok(s) = studio.song.settings().lock() else {
            return RecView::default();
        };
        let p = &s.prefs;
        let bars = |key: &str| (p.number(key).round().max(1.0)) as u32;
        RecView {
            punch: p.bool("record.punch"),
            preroll: p.bool("record.preroll"),
            preroll_bars: bars("record.preroll-bars"),
            postroll: p.bool("record.postroll"),
            postroll_bars: bars("record.postroll-bars"),
            count_bars: p.number("audio.count-in").round().max(0.0) as u32,
            quantize: p.bool("edit.record.input-quantize"),
            quantize_value: p.text("edit.record.input-quantize-value"),
        }
    }

    /// Whether the switch of `part` is on.
    pub fn on(&self, part: RecPart) -> bool {
        match part {
            RecPart::Punch => self.punch,
            RecPart::PreRoll | RecPart::PreRollBars => self.preroll,
            RecPart::PostRoll | RecPart::PostRollBars => self.postroll,
            RecPart::Count | RecPart::CountBars => self.count_bars > 0,
            RecPart::Quantize | RecPart::QuantizeValue => self.quantize,
        }
    }

    /// The number a bars field shows.
    pub fn bars(&self, part: RecPart) -> u32 {
        match part {
            RecPart::PreRollBars => self.preroll_bars,
            RecPart::PostRollBars => self.postroll_bars,
            _ => self.count_bars.max(1),
        }
    }
}

impl Default for RecView {
    fn default() -> RecView {
        RecView {
            punch: false,
            preroll: false,
            preroll_bars: 1,
            postroll: false,
            postroll_bars: 1,
            count_bars: 0,
            quantize: false,
            quantize_value: "1/16".into(),
        }
    }
}

/// The panel's own state: whether it is shown, and which bars field takes the digits typed.
/// Neither is kept in the song or the settings: a window starts with the panel shut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecPanel {
    pub open: bool,
    pub focus: Option<RecPart>,
}

/// A part's place and what it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub part: RecPart,
    pub rect: Rect,
    pub label: &'static str,
}

fn label_of(part: RecPart) -> &'static str {
    match part {
        RecPart::Punch => "オートパンチ",
        RecPart::PreRoll => "プリロール",
        RecPart::PostRoll => "ポストロール",
        RecPart::Count => "プリカウント",
        RecPart::Quantize => "入力クオンタイズ",
        _ => "",
    }
}

/// Where each part sits, left to right as in the mock. The post-roll is left out while
/// auto-punch is off. In a strip too narrow for the mock's gaps, the gaps between the groups
/// close up to fit.
pub fn layout(rect: Rect, view: &RecView) -> Vec<Slot> {
    let mut gap = GROUP_GAP;
    loop {
        let out = layout_with(rect, view, gap);
        let over = out
            .last()
            .map_or(0.0, |s| s.rect.right() + PAD_X - rect.right());
        if over <= 0.0 || gap <= 4.0 {
            return out;
        }
        // The gaps are 4 or 5 in a row; shrink by what is over, split among them.
        gap = (gap - over / 5.0).max(4.0).min(gap - 0.5);
    }
}

fn layout_with(rect: Rect, view: &RecView, group_gap: f32) -> Vec<Slot> {
    let mut out = Vec::new();
    let mut x = rect.x + PAD_X;
    let mid = rect.y + rect.h / 2.0;
    let mut put = |part: RecPart, w: f32, h: f32, x: &mut f32| {
        let r = Rect::new(*x, mid - h / 2.0, w, h);
        out.push(Slot {
            part,
            rect: r,
            label: label_of(part),
        });
        *x += w;
    };
    let switch_w = |part: RecPart| {
        SWITCH_W + LABEL_GAP + crate::browser::text_width(label_of(part), FontSize::Small).ceil()
    };
    put(RecPart::Punch, switch_w(RecPart::Punch), SWITCH_H, &mut x);
    x += group_gap;
    let mut groups = vec![(RecPart::PreRoll, RecPart::PreRollBars)];
    if view.punch {
        groups.push((RecPart::PostRoll, RecPart::PostRollBars));
    }
    groups.push((RecPart::Count, RecPart::CountBars));
    for (switch, bars) in groups {
        put(switch, switch_w(switch), SWITCH_H, &mut x);
        x += LABEL_GAP;
        put(bars, BARS_W, FIELD_H, &mut x);
        x += LABEL_GAP - 2.0 + crate::browser::text_width(BARS_UNIT, FontSize::Small).ceil();
        x += group_gap;
    }
    put(
        RecPart::Quantize,
        switch_w(RecPart::Quantize),
        SWITCH_H,
        &mut x,
    );
    x += LABEL_GAP;
    put(RecPart::QuantizeValue, VALUE_W, FIELD_H, &mut x);
    out
}

/// The part under a point.
pub fn part_at(rect: Rect, view: &RecView, x: f32, y: f32) -> Option<RecPart> {
    layout(rect, view)
        .into_iter()
        .find(|s| s.rect.contains(x, y))
        .map(|s| s.part)
}

/// The panel, over the strip the body leaves for it.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    if g.rec.h <= 0.0 {
        return;
    }
    let c = pen.c;
    pen.round(g.rec, c.surface_2, Radius::S);
    let view = RecView::of(studio);
    let light = c.theme == xirius_design::Theme::Light;
    for slot in layout(g.rec, &view) {
        let hot = studio.hover == Hit::Rec(slot.part);
        let on = view.on(slot.part);
        let r = slot.rect;
        match slot.part {
            part if part.switch_command().is_some() => {
                let sw = Rect::new(r.x, r.y, SWITCH_W, SWITCH_H);
                let (face, knob) = if on {
                    (if hot { c.fill_hover } else { c.fill }, c.on_fill)
                } else {
                    let face = if light || hot { c.thumb } else { c.surface_3 };
                    (face, if light { c.key_white } else { c.ink })
                };
                pen.round(sw, face, Radius::Pill);
                let kx = if on {
                    sw.right() - 3.0 - 14.0
                } else {
                    sw.x + 3.0
                };
                pen.round(Rect::new(kx, sw.y + 3.0, 14.0, 14.0), knob, Radius::Pill);
                let t = Rect::new(sw.right() + LABEL_GAP, r.y, r.right() - sw.right(), r.h);
                pen.text(
                    slot.label,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink_strong,
                    t,
                    Align::Left,
                );
            }
            RecPart::QuantizeValue => {
                dropdown::field(pen, r, &view.quantize_value, Face::Well, hot, on);
            }
            part => {
                // A bars field: a sunk box with its number, ringed while it takes digits.
                let focused = studio.rec_panel.focus == Some(part);
                if focused {
                    pen.round(r, c.fill, Radius::S);
                    pen.round(
                        Rect::new(r.x + 1.5, r.y + 1.5, r.w - 3.0, r.h - 3.0),
                        c.surface_3,
                        Radius::S,
                    );
                } else {
                    pen.round(r, if hot { c.thumb } else { c.surface_3 }, Radius::S);
                }
                let ink = if on || focused {
                    c.ink_strong
                } else {
                    c.ink_on_toggle
                };
                pen.digits(
                    &view.bars(part).to_string(),
                    FontSize::Small,
                    Weight::Medium,
                    ink,
                    r,
                    Align::Center,
                );
                let t = Rect::new(r.right() + LABEL_GAP - 2.0, r.y, 40.0, r.h);
                pen.text(
                    BARS_UNIT,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    t,
                    Align::Left,
                );
            }
        }
    }
}

/// The rows of the input quantize's menu.
pub fn cells(choices: &[Choice], current: &str, w: f32) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    choices
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            Cell::new(
                Rect::new(
                    pad,
                    pad + i as f32 * Layout::MENU_ROW_H,
                    w - pad * 2.0,
                    Layout::MENU_ROW_H,
                ),
                Look::Row,
            )
            .text(ch.label.clone())
            .on(ch.to == current)
            .act(Action::RecQuantize(ch.to.clone()))
        })
        .collect()
}

impl Studio {
    /// The panel's rect and what it shows, when it is on screen.
    fn rec_slots(&self, g: &Geometry) -> Vec<Slot> {
        if g.rec.h <= 0.0 {
            return Vec::new();
        }
        layout(g.rec, &RecView::of(self))
    }

    /// The part of the panel under a point (the empty strip answers `Hit::None`).
    pub(crate) fn rec_hit(&self, g: &Geometry, x: f32, y: f32) -> Option<Hit> {
        if g.rec.h <= 0.0 || !g.rec.contains(x, y) {
            return None;
        }
        Some(
            self.rec_slots(g)
                .into_iter()
                .find(|s| s.rect.contains(x, y))
                .map_or(Hit::None, |s| Hit::Rec(s.part)),
        )
    }

    /// The middle of a part, for scripted input.
    pub(crate) fn rec_center(&self, g: &Geometry, part: RecPart) -> Option<(f32, f32)> {
        let r = self.rec_slots(g).into_iter().find(|s| s.part == part)?.rect;
        Some((r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    /// A press on the panel: a switch runs its command, a bars field takes the digits typed
    /// next, the value box opens its menu.
    pub(crate) fn rec_press(&mut self, g: &Geometry, part: RecPart) -> bool {
        self.rec_panel.focus = None;
        if let Some(command) = part.switch_command() {
            self.rec_set(command, json!({}));
            return true;
        }
        if part.is_bars() {
            self.rec_panel.focus = Some(part);
            return true;
        }
        let Some(r) = self.rec_slots(g).into_iter().find(|s| s.part == part) else {
            return false;
        };
        self.open_popup(PopupKind::RecQuantize, r.rect);
        true
    }

    /// Runs a settings command from the panel; a refusal is shown as the screen shows any.
    pub(crate) fn rec_set(&mut self, command: &str, args: serde_json::Value) {
        if let Err(e) = self.song.set(command, args) {
            self.error = Some(e);
        }
    }

    /// Shows or hides the panel; the body is laid out again under it.
    pub(crate) fn toggle_rec_panel(&mut self, g: &Geometry) {
        self.rec_panel.open = !self.rec_panel.open;
        self.rec_panel.focus = None;
        let g = self.geometry(g.size.0, g.size.1);
        self.resized(&g);
    }

    /// Keys while a bars field has the focus: digits set the number, ↑ ↓ step it, Enter, Tab
    /// and Escape let go. Everything else is taken so a typed digit is not a shortcut.
    pub(crate) fn rec_key(&mut self, key: &str) -> bool {
        let Some(part) = self.rec_panel.focus else {
            return false;
        };
        let Some(command) = part.bars_command() else {
            return false;
        };
        let now = RecView::of(self).bars(part) as i64;
        let to = match key {
            "escape" | "enter" | "tab" => {
                self.rec_panel.focus = None;
                return true;
            }
            "up" => Some(now + 1),
            "down" => Some(now - 1),
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => {
                Some(i64::from(k.as_bytes()[0] - b'0'))
            }
            _ => None,
        };
        if let Some(n) = to.filter(|n| (1..=8).contains(n)) {
            self.rec_set(command, json!({ "bars": n }));
        }
        true
    }

    /// The menu of the input quantize's value and the one that is lit.
    pub(crate) fn rec_quantize_view(&self) -> (Vec<Choice>, String) {
        let choices = QUANTIZE_VALUES
            .iter()
            .map(|v| Choice {
                to: (*v).into(),
                label: (*v).into(),
            })
            .collect();
        (choices, RecView::of(self).quantize_value)
    }

    /// A row of that menu.
    pub(crate) fn rec_quantize_action(&mut self, value: String) -> bool {
        self.popup = None;
        self.rec_set("record.input-quantize", json!({ "value": value }));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topbar::BarButton;

    fn studio(w: f32) -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let g = s.geometry(w, 900.0);
        s.resized(&g);
        (s, g)
    }

    fn open(s: &mut Studio, g: &Geometry) -> Geometry {
        assert!(s.press(g, Hit::Bar(BarButton::RecPanel)));
        let g = s.geometry(g.size.0, g.size.1);
        s.resized(&g);
        g
    }

    fn pref_bool(s: &Studio, key: &str) -> bool {
        s.song.settings().lock().unwrap().prefs.bool(key)
    }

    fn pref_num(s: &Studio, key: &str) -> f64 {
        s.song.settings().lock().unwrap().prefs.number(key)
    }

    fn press_part(s: &mut Studio, g: &Geometry, part: RecPart) {
        let (x, y) = s.center_of(g, Hit::Rec(part)).expect("the part is shown");
        let hit = s.hit(g, x, y);
        assert_eq!(hit, Hit::Rec(part));
        assert!(s.press(g, hit));
    }

    #[test]
    fn the_panel_is_shut_in_a_new_window_and_the_button_opens_and_shuts_it() {
        let (mut s, g) = studio(1280.0);
        assert_eq!(g.rec.h, 0.0);
        assert_eq!(s.rec_hit(&g, 100.0, 10.0), None);
        let top = g.lanes.y;
        let g = open(&mut s, &g);
        assert_eq!(g.rec.h, H);
        assert!(g.lanes.y >= top + H, "the lanes make room for the strip");
        let g = open(&mut s, &g);
        assert_eq!(g.rec.h, 0.0);
        assert_eq!(g.lanes.y, top);
    }

    #[test]
    fn the_parts_sit_inside_the_strip_without_touching_and_the_post_roll_waits_for_punch() {
        for w in [960.0, 1280.0, 1920.0] {
            let (mut s, g) = studio(w);
            let g = open(&mut s, &g);
            for punch in [false, true] {
                let view = RecView {
                    punch,
                    ..RecView::default()
                };
                let slots = layout(g.rec, &view);
                for (i, a) in slots.iter().enumerate() {
                    assert!(
                        a.rect.x >= g.rec.x && a.rect.right() <= g.rec.right(),
                        "{:?} at {w}: {:?} leaves the strip",
                        a.part,
                        a.rect
                    );
                    assert!(a.rect.y >= g.rec.y && a.rect.bottom() <= g.rec.bottom());
                    for b in &slots[i + 1..] {
                        assert!(
                            a.rect.right() <= b.rect.x,
                            "{:?} and {:?} overlap at {w}",
                            a.part,
                            b.part
                        );
                    }
                }
                let has = |p: RecPart| slots.iter().any(|s| s.part == p);
                assert_eq!(has(RecPart::PostRoll), punch);
                assert_eq!(has(RecPart::PostRollBars), punch);
                assert!(has(RecPart::Quantize) && has(RecPart::QuantizeValue));
            }
        }
    }

    #[test]
    fn every_part_is_found_under_its_own_middle() {
        let (mut s, g) = studio(1280.0);
        let g = open(&mut s, &g);
        s.rec_set("record.punch", json!({ "on": true }));
        for part in RecPart::ALL {
            let (x, y) = s.center_of(&g, Hit::Rec(part)).expect("shown");
            assert_eq!(s.hit(&g, x, y), Hit::Rec(part), "{part:?}");
        }
        // The empty end of the strip answers that it is the strip, not the lanes behind it.
        assert_eq!(s.hit(&g, g.rec.right() - 4.0, g.rec.y + 4.0), Hit::None);
    }

    #[test]
    fn the_switches_run_their_commands_and_the_post_roll_shows_only_with_punch() {
        let (mut s, g) = studio(1280.0);
        let g = open(&mut s, &g);
        assert!(s.center_of(&g, Hit::Rec(RecPart::PostRoll)).is_none());
        press_part(&mut s, &g, RecPart::Punch);
        assert!(pref_bool(&s, "record.punch"));
        assert!(s.center_of(&g, Hit::Rec(RecPart::PostRoll)).is_some());
        press_part(&mut s, &g, RecPart::PreRoll);
        assert!(pref_bool(&s, "record.preroll"));
        press_part(&mut s, &g, RecPart::PostRoll);
        assert!(pref_bool(&s, "record.postroll"));
        press_part(&mut s, &g, RecPart::Count);
        assert_eq!(pref_num(&s, "audio.count-in"), 1.0);
        press_part(&mut s, &g, RecPart::Count);
        assert_eq!(pref_num(&s, "audio.count-in"), 0.0);
        press_part(&mut s, &g, RecPart::Quantize);
        assert!(pref_bool(&s, "edit.record.input-quantize"));
        press_part(&mut s, &g, RecPart::Punch);
        assert!(!pref_bool(&s, "record.punch"));
        assert!(s.center_of(&g, Hit::Rec(RecPart::PostRoll)).is_none());
        assert!(s.error.is_none());
    }

    #[test]
    fn the_bar_button_for_the_pre_roll_and_the_panel_switch_are_one_setting() {
        let (mut s, g) = studio(1280.0);
        let g = open(&mut s, &g);
        let (x, y) = s.center_of(&g, Hit::Bar(BarButton::PreRoll)).unwrap();
        assert_eq!(s.hit(&g, x, y), Hit::Bar(BarButton::PreRoll));
        assert!(s.press(&g, Hit::Bar(BarButton::PreRoll)));
        assert!(pref_bool(&s, "record.preroll"));
        assert!(RecView::of(&s).on(RecPart::PreRoll));
        press_part(&mut s, &g, RecPart::PreRoll);
        assert!(!pref_bool(&s, "record.preroll"));
    }

    #[test]
    fn a_bars_field_takes_the_digits_typed_and_lets_go_on_escape() {
        let (mut s, g) = studio(1280.0);
        let g = open(&mut s, &g);
        // Without the focus a digit is not the panel's.
        assert!(!s.rec_key("3"));
        press_part(&mut s, &g, RecPart::PreRollBars);
        assert_eq!(s.rec_panel.focus, Some(RecPart::PreRollBars));
        assert!(s.rec_key("3"));
        assert_eq!(pref_num(&s, "record.preroll-bars"), 3.0);
        assert!(s.rec_key("up"));
        assert_eq!(pref_num(&s, "record.preroll-bars"), 4.0);
        assert!(s.rec_key("down"));
        assert_eq!(pref_num(&s, "record.preroll-bars"), 3.0);
        // Out of 1..=8 changes nothing, and letters are swallowed.
        assert!(s.rec_key("9"));
        assert!(s.rec_key("0"));
        assert!(s.rec_key("x"));
        assert_eq!(pref_num(&s, "record.preroll-bars"), 3.0);
        assert!(s.rec_key("escape"));
        assert_eq!(s.rec_panel.focus, None);
        assert!(!s.rec_key("3"));
        // Setting the bars does not turn the switch on.
        assert!(!pref_bool(&s, "record.preroll"));
        // The pre-count's field writes the count itself.
        press_part(&mut s, &g, RecPart::CountBars);
        assert!(s.rec_key("2"));
        assert_eq!(pref_num(&s, "audio.count-in"), 2.0);
        assert!(s.error.is_none());
    }

    #[test]
    fn pressing_elsewhere_lets_go_of_the_field() {
        let (mut s, g) = studio(1280.0);
        let g = open(&mut s, &g);
        press_part(&mut s, &g, RecPart::CountBars);
        assert!(s.rec_panel.focus.is_some());
        press_part(&mut s, &g, RecPart::Punch);
        assert_eq!(s.rec_panel.focus, None);
        press_part(&mut s, &g, RecPart::CountBars);
        s.press(&g, Hit::Bar(BarButton::Play));
        assert_eq!(s.rec_panel.focus, None);
    }

    #[test]
    fn the_quantize_value_is_picked_from_its_menu() {
        let (mut s, g) = studio(1280.0);
        let g = open(&mut s, &g);
        press_part(&mut s, &g, RecPart::QuantizeValue);
        let l = s.popup.as_ref().expect("the menu opens").layout(g.size);
        let texts: Vec<_> = l.cells.iter().map(|c| c.text.clone()).collect();
        assert_eq!(texts, QUANTIZE_VALUES);
        let i = texts.iter().position(|t| t == "1/8T").unwrap();
        let c = &l.cells[i];
        let hit = s.hit(&g, c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0);
        assert_eq!(hit, Hit::Item(i));
        assert!(s.press(&g, hit));
        assert!(s.popup.is_none());
        let value = s
            .song
            .settings()
            .lock()
            .unwrap()
            .prefs
            .text("edit.record.input-quantize-value");
        assert_eq!(value, "1/8T");
        // Picking a value does not turn the switch on by itself.
        assert!(!pref_bool(&s, "edit.record.input-quantize"));
    }

    #[test]
    fn the_panel_command_shows_and_hides_it() {
        let (mut s, g) = studio(1280.0);
        assert!(!s.rec_panel.open);
        assert!(s.run_parsed(&g, "view.record-panel".into(), Default::default()));
        assert!(s.rec_panel.open);
        assert!(s.run_parsed(&g, "view.record-panel".into(), Default::default()));
        assert!(!s.rec_panel.open);
    }
}
