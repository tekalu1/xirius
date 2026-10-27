//! The quantize panel (EDT-073): what Q moves the roll's selected notes to — the
//! grid, with tuplets and swing, or a groove taken from another MIDI clip — and how
//! far (開始・終わり・ベロシティ %), for the notes within 範囲 of their line; with
//! five presets.
//!
//! It opens from the grid pop-over's クオンタイズの設定… row. Its settings stay on
//! the editor's view state, are saved with the song, and go with every
//! `note.quantize` the roll sends (Q, Alt+Q, AQ).
//! Left at 標準 they add nothing, so Q stays the plain quantize.

use serde_json::{Value, json};
use xirius_core::id::ClipId;
use xirius_design::{FontSize, Space, Weight};

use crate::draw::{Align, Pen};
use crate::popup::{
    Action, Cell, GRID_VALUES, LABEL_H, Look, PopupKind, SEGMENT_H, SEGMENT_INSET, segments,
    split_grid,
};
use crate::studio::{Geometry, Rect, Studio};
#[cfg(test)]
use crate::tools::Mods;
use crate::tools::SnapMode;

pub const W: f32 = 320.0;
/// One press of − or + moves a % this far.
pub const STEP: u8 = 5;
/// Groove clips listed at most.
pub const MAX_CLIPS: usize = 5;
const STEPPER_W: f32 = 28.0;
const VALUE_W: f32 = 84.0;
const CLIP_ROW_H: f32 = 26.0;

/// A % the panel sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Start,
    /// 0 leaves the lengths: the whole note moves with its start.
    End,
    /// Towards the groove's velocities; a grid has none.
    Velocity,
    /// Only notes within this % of half the space between lines move.
    Range,
    /// Every second line later by this % of half a step.
    Swing,
}

/// What the panel is set to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantSet {
    /// グルーヴ: the lines are the starts of `groove`'s notes.
    pub groove_on: bool,
    pub groove: Option<ClipId>,
    /// 5 or 7 notes in the time of 4, or 1. Three in two is the grid's 3連 (`1/16t`),
    /// which the snap uses too.
    pub tuplet: u8,
    pub swing: u8,
    pub start: u8,
    pub end: u8,
    pub velocity: u8,
    pub range: u8,
}

impl Default for QuantSet {
    fn default() -> QuantSet {
        QuantSet {
            groove_on: false,
            groove: None,
            tuplet: 1,
            swing: 0,
            start: 100,
            end: 0,
            velocity: 100,
            range: 100,
        }
    }
}

/// A preset: the %s it sets (開始・終わり・ベロシティ・範囲・スウィング).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub start: u8,
    pub end: u8,
    pub velocity: u8,
    pub range: u8,
    pub swing: u8,
}

const fn preset(name: &'static str, start: u8, end: u8, range: u8, swing: u8) -> Preset {
    Preset {
        name,
        start,
        end,
        velocity: start,
        range,
        swing,
    }
}

pub const PRESETS: [Preset; 5] = [
    // All the way to the line: Q as it always was.
    preset("標準", 100, 0, 100, 0),
    // Halfway: keeps some of the playing.
    preset("50%", 50, 0, 100, 0),
    // Only notes near a line; the rest are left as played.
    preset("近くだけ", 100, 0, 50, 0),
    preset("スウィング", 100, 0, 100, 50),
    // Ends to the lines too.
    preset("長さも", 100, 100, 100, 0),
];

impl QuantSet {
    pub fn to_json(self) -> Value {
        json!({"groove_on":self.groove_on,"groove":self.groove.map(|c| c.to_string()),"tuplet":self.tuplet,"swing":self.swing,"start":self.start,"end":self.end,"velocity":self.velocity,"range":self.range})
    }

    pub fn from_json(value: &Value) -> Self {
        let default = Self::default();
        let pct = |key: &str, fallback: u8| {
            value[key]
                .as_u64()
                .filter(|v| *v <= 100)
                .map_or(fallback, |v| v as u8)
        };
        Self {
            groove_on: value["groove_on"].as_bool().unwrap_or(false),
            groove: value["groove"].as_str().and_then(|s| s.parse().ok()),
            tuplet: value["tuplet"]
                .as_u64()
                .filter(|v| [1, 5, 7].contains(v))
                .unwrap_or(1) as u8,
            swing: pct("swing", default.swing),
            start: pct("start", default.start),
            end: pct("end", default.end),
            velocity: pct("velocity", default.velocity),
            range: pct("range", default.range),
        }
    }
    pub fn get(&self, f: Field) -> u8 {
        match f {
            Field::Start => self.start,
            Field::End => self.end,
            Field::Velocity => self.velocity,
            Field::Range => self.range,
            Field::Swing => self.swing,
        }
    }

    pub fn step(&mut self, f: Field, up: bool) {
        let v = self.get(f);
        let v = if up {
            v.saturating_add(STEP).min(100)
        } else {
            v.saturating_sub(STEP)
        };
        match f {
            Field::Start => self.start = v,
            Field::End => self.end = v,
            Field::Velocity => self.velocity = v,
            Field::Range => self.range = v,
            Field::Swing => self.swing = v,
        }
    }

    /// The preset it is set to, if any. Velocity only counts for a groove.
    pub fn preset(&self) -> Option<usize> {
        PRESETS.iter().position(|p| {
            (p.start, p.end, p.range, p.swing) == (self.start, self.end, self.range, self.swing)
                && (!self.groove_on || p.velocity == self.velocity)
        })
    }

    pub fn load(&mut self, i: usize) {
        let Some(p) = PRESETS.get(i) else { return };
        self.start = p.start;
        self.end = p.end;
        self.velocity = p.velocity;
        self.range = p.range;
        self.swing = p.swing;
        if p.swing > 0 {
            self.tuplet = 1;
        }
    }

    /// Can swing be set? Only on a plain grid: tuplets have no second line.
    pub fn swings(&self, triplet: bool) -> bool {
        !self.groove_on && self.tuplet == 1 && !triplet
    }

    /// A `note.quantize`'s args with the panel's settings: the strength scaled by
    /// 開始 (Alt+Q's 50 % is half of it), and only what differs from a plain grid.
    pub fn args(&self, mut args: Value, triplet: bool) -> Value {
        if args["restore"].as_bool() == Some(true) || args["freeze"].as_bool() == Some(true) {
            return args;
        }
        let Some(map) = args.as_object_mut() else {
            return args;
        };
        let strength = map.get("strength").and_then(Value::as_u64).unwrap_or(100);
        map.insert(
            "strength".into(),
            json!(strength * u64::from(self.start) / 100),
        );
        match (self.groove_on, self.groove) {
            (true, Some(clip)) => {
                map.insert("groove".into(), json!(clip.to_string()));
                map.insert("velocity".into(), json!(self.velocity));
            }
            _ => {
                if self.tuplet > 1 && !triplet {
                    map.insert("tuplet".into(), json!(self.tuplet));
                }
                if self.swing > 0 && self.swings(triplet) {
                    map.insert("swing".into(), json!(self.swing));
                }
            }
        }
        if self.end > 0 {
            map.insert("end".into(), json!(self.end));
        }
        if self.range < 100 {
            map.insert("range".into(), json!(self.range));
        }
        args
    }
}

/// A press in the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QAct {
    /// クオンタイズの設定… in the grid pop-over.
    Open,
    /// グリッド or グルーヴ.
    Groove(bool),
    /// なし (1), 3 (the grid's 3連), 5, 7.
    Tuplet(u8),
    Step(Field, bool),
    Clip(ClipId),
    External,
    Preset(usize),
    /// 選択をクオンタイズ: Q on the roll's selected notes.
    Apply,
    Restore,
    Freeze,
    Humanize(bool),
}

/// What the panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct QuantView {
    pub external_name: Option<String>,
    pub set: QuantSet,
    /// MIDI clips with notes, other than the roll's, as `Bass · 2 小節目`.
    pub clips: Vec<(ClipId, String)>,
    /// Is the roll open with notes selected?
    pub can_apply: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QLook {
    /// A %, or what 0 means for it.
    Value,
}

/// The value a field shows.
pub fn value_text(f: Field, v: u8) -> String {
    match (f, v) {
        (Field::End, 0) => "長さを保つ".into(),
        _ => format!("{v}%"),
    }
}

pub fn cells(v: Option<&QuantView>, grid: &str, w: f32) -> Vec<Cell> {
    let Some(v) = v else {
        return Vec::new();
    };
    let q = &v.set;
    let pad = Space::X3.px() - SEGMENT_INSET;
    let inner = w - pad * 2.0;
    let row_h = SEGMENT_H + SEGMENT_INSET * 2.0;
    let gap = Space::X2.px();
    let (base, triplet) = split_grid(grid);
    let mut cells = Vec::new();
    let mut y = pad;
    let label = |cells: &mut Vec<Cell>, y: &mut f32, text: &str| {
        cells.push(Cell::new(Rect::new(pad + 2.0, *y, inner, LABEL_H), Look::Label).text(text));
        *y += LABEL_H;
    };
    let lines = [("グリッド", false), ("グルーヴ", true)]
        .map(|(text, on)| (text, q.groove_on == on, Action::Quant(QAct::Groove(on))));
    segments(&mut cells, Rect::new(pad, y, inner, row_h), &lines);
    y += row_h + gap;
    if q.groove_on {
        label(&mut cells, &mut y, "グルーヴにするクリップ");
        if let Some(name) = &v.external_name {
            cells.push(
                Cell::new(Rect::new(pad, y, inner, CLIP_ROW_H), Look::Row)
                    .text(name.clone())
                    .on(q.groove.is_none())
                    .act(Action::Quant(QAct::External)),
            );
            y += CLIP_ROW_H;
        }
        for (id, name) in v.clips.iter().take(MAX_CLIPS) {
            cells.push(
                Cell::new(Rect::new(pad, y, inner, CLIP_ROW_H), Look::Row)
                    .text(name.clone())
                    .on(q.groove == Some(*id))
                    .act(Action::Quant(QAct::Clip(*id))),
            );
            y += CLIP_ROW_H;
        }
        y += gap;
    } else {
        label(&mut cells, &mut y, "グリッド");
        let values = GRID_VALUES.map(|g| (g, base == g, Action::GridValue(g)));
        segments(&mut cells, Rect::new(pad, y, inner, row_h), &values);
        y += row_h + gap;
        label(&mut cells, &mut y, "連符");
        let tuplets = [("なし", 1), ("3", 3), ("5", 5), ("7", 7)].map(|(text, n)| {
            let on = if triplet { n == 3 } else { q.tuplet == n };
            (text, on, Action::Quant(QAct::Tuplet(n)))
        });
        segments(&mut cells, Rect::new(pad, y, inner, row_h), &tuplets);
        y += row_h + gap;
        stepper(
            &mut cells,
            pad,
            inner,
            &mut y,
            q,
            Field::Swing,
            q.swings(triplet),
        );
        y += gap;
    }
    label(&mut cells, &mut y, "強さ");
    for f in [Field::Start, Field::End, Field::Velocity] {
        let live = f != Field::Velocity || q.groove_on;
        stepper(&mut cells, pad, inner, &mut y, q, f, live);
    }
    stepper(&mut cells, pad, inner, &mut y, q, Field::Range, true);
    y += gap;
    label(&mut cells, &mut y, "プリセット");
    let lit = q.preset();
    let presets: Vec<(&str, bool, Action)> = PRESETS
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name, lit == Some(i), Action::Quant(QAct::Preset(i))))
        .collect();
    segments(&mut cells, Rect::new(pad, y, inner, row_h), &presets);
    y += row_h + gap + Space::X1.px();
    let apply = Cell::new(Rect::new(pad, y, inner, 30.0), Look::Primary).text("選択をクオンタイズ");
    let ready = v.can_apply && (!q.groove_on || q.groove.is_some() || v.external_name.is_some());
    cells.push(if ready {
        apply.act(Action::Quant(QAct::Apply))
    } else {
        apply.off()
    });
    y += 30.0 + gap;
    for (text, act) in [
        ("タイミングを復元", QAct::Restore),
        ("クオンタイズを固定", QAct::Freeze),
        ("ヒューマナイズ", QAct::Humanize(false)),
        ("ヒューマナイズを弱く", QAct::Humanize(true)),
    ] {
        cells.push(
            Cell::new(Rect::new(pad, y, inner, CLIP_ROW_H), Look::Row)
                .text(text)
                .act(Action::Quant(act)),
        );
        y += CLIP_ROW_H;
    }
    cells
}

/// `開始 ……… 100% [−][+]`.
fn stepper(
    cells: &mut Vec<Cell>,
    pad: f32,
    inner: f32,
    y: &mut f32,
    q: &QuantSet,
    f: Field,
    live: bool,
) {
    let name = match f {
        Field::Start => "開始",
        Field::End => "終わり",
        Field::Velocity => "ベロシティ",
        Field::Range => "範囲",
        Field::Swing => "スウィング",
    };
    let h = SEGMENT_H + SEGMENT_INSET * 2.0;
    let gap = SEGMENT_INSET * 2.0;
    let plus_x = pad + inner - STEPPER_W;
    let minus_x = plus_x - gap - STEPPER_W;
    let value_x = minus_x - gap - VALUE_W;
    let v = q.get(f);
    let mut name_cell =
        Cell::new(Rect::new(pad + 2.0, *y, value_x - pad, h), Look::Label).text(name);
    let mut value = Cell::new(
        Rect::new(value_x, *y, VALUE_W, h),
        Look::Quant(QLook::Value),
    )
    .text(value_text(f, v));
    let button = |x: f32, text: &str, up: bool, can: bool| {
        let c = Cell::new(Rect::new(x, *y + 2.0, STEPPER_W, h - 4.0), Look::Button).text(text);
        if live && can {
            c.act(Action::Quant(QAct::Step(f, up)))
        } else {
            c.off()
        }
    };
    let minus = button(minus_x, "−", false, v > 0);
    let plus = button(plus_x, "+", true, v < 100);
    if !live {
        name_cell.enabled = false;
        value.enabled = false;
    }
    cells.extend([name_cell, value, minus, plus]);
    *y += h;
}

pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: QLook) {
    let c = pen.c;
    match look {
        QLook::Value => {
            let ink = if cell.enabled {
                c.ink_strong
            } else {
                c.ink_on_toggle
            };
            pen.digits(
                &cell.text,
                FontSize::Small,
                Weight::Medium,
                ink,
                Rect::new(
                    cell.rect.x,
                    cell.rect.y,
                    cell.rect.w - Space::X2.px(),
                    cell.rect.h,
                ),
                Align::Right,
            );
        }
    }
}

impl Studio {
    /// Is the grid 3連 (`1/16t`)?
    fn triplet(&self) -> bool {
        self.song.grid().ends_with('t')
    }

    /// A `note.quantize` from the roll, with the panel's settings.
    pub(crate) fn quant_args(&self, args: Value) -> Value {
        let mut args = self.quant.args(args, self.triplet());
        if self.quant.groove_on
            && self.quant.groove.is_none()
            && args["restore"].as_bool() != Some(true)
            && args["freeze"].as_bool() != Some(true)
            && let Some(groove) = &self.song.session().state.view.roll.external_groove
        {
            args["groove-hits"] = json!(
                groove
                    .hits
                    .iter()
                    .map(|(at, velocity)| json!({"at":at,"velocity":velocity}))
                    .collect::<Vec<_>>()
            );
            args["groove-period"] = json!(groove.period);
            args["velocity"] = json!(self.quant.velocity);
        }
        args
    }

    pub(crate) fn quant_view(&self) -> QuantView {
        let p = self.project();
        let map = p.time_map();
        let own = self.selected_clip.map(|(_, c)| c);
        let mut clips: Vec<_> = p
            .tracks
            .values()
            .flat_map(|t| t.clips.iter().map(move |c| (t, c)))
            .filter(|(_, c)| Some(c.id) != own && !c.notes().is_empty())
            .map(|(t, c)| {
                let name = if c.name.is_empty() { &t.name } else { &c.name };
                (c.id, format!("{name} · {} 小節目", map.to_bbt(c.start).0))
            })
            .collect();
        // The newest first: a groove just played or brought in is at the top.
        clips.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
        QuantView {
            external_name: self
                .song
                .session()
                .state
                .view
                .roll
                .external_groove
                .as_ref()
                .map(|g| g.name.clone()),
            set: self.quant,
            clips,
            can_apply: self.drawer_open
                && self.tab == crate::studio::DrawerTab::Roll
                && self.roll_target().is_some()
                && !self.roll.selected.is_empty(),
        }
    }

    /// A press in the panel, or its row in the grid pop-over.
    pub(crate) fn quant_action(&mut self, g: &Geometry, act: QAct) -> bool {
        if !matches!(act, QAct::Open) {
            self.invalidate_groove_import();
        }
        match act {
            QAct::Open => {
                let anchor = self
                    .popup
                    .as_ref()
                    .map_or(Rect::new(0.0, 0.0, 0.0, 0.0), |p| p.anchor);
                self.popup = None;
                self.open_popup(PopupKind::Quantize, anchor);
                return true;
            }
            QAct::Groove(on) => self.quant.groove_on = on,
            QAct::Tuplet(n) => {
                let (base, triplet) = split_grid(self.song.grid());
                let base = base.to_string();
                if (n == 3) != triplet {
                    let grid = if n == 3 { format!("{base}t") } else { base };
                    self.set_grid_mode(SnapMode::Quantize);
                    self.command(|s| s.set_grid(&grid));
                }
                self.quant.tuplet = if n == 3 { 1 } else { n };
            }
            QAct::Step(f, up) => self.quant.step(f, up),
            QAct::Clip(id) => self.quant.groove = Some(id),
            QAct::External => self.quant.groove = None,
            QAct::Preset(i) => self.quant.load(i),
            QAct::Apply => {
                if let Some(ctx) = self.roll_ctx(g) {
                    let out = self.roll.auto_quantize(&ctx);
                    self.apply_roll(g, out);
                }
            }
            QAct::Restore | QAct::Freeze | QAct::Humanize(_) => {
                if let Some(ctx) = self.roll_ctx(g) {
                    let ids = self.roll.target_ids(&ctx, true);
                    let (command, args) = match act {
                        QAct::Restore => (
                            "note.quantize",
                            json!({"ids":crate::roll::ids_json(&ids),"restore":true}),
                        ),
                        QAct::Freeze => (
                            "note.quantize",
                            json!({"ids":crate::roll::ids_json(&ids),"freeze":true}),
                        ),
                        QAct::Humanize(less) => (
                            "note.humanize",
                            json!({"ids":crate::roll::ids_json(&ids),"less":less}),
                        ),
                        _ => unreachable!(),
                    };
                    self.apply_roll(
                        g,
                        crate::roll::Out::edit(crate::roll::Edit::new(
                            command,
                            args,
                            crate::roll::Then::Keep,
                        )),
                    );
                }
            }
        }
        self.save_quant_settings();
        self.refresh_popup();
        true
    }

    /// Q while the panel is open quantizes the selection with what it shows;
    /// None for other keys and other pop-overs.
    pub(crate) fn quant_command(
        &mut self,
        g: &Geometry,
        command: &str,
        args: &serde_json::Map<String, Value>,
    ) -> Option<bool> {
        let open = self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Quantize);
        if !open || command != "note.quantize" {
            return None;
        }
        let ready = self
            .popup
            .as_ref()
            .and_then(|p| p.quant.as_ref())
            .is_some_and(|v| {
                v.can_apply
                    && (!v.set.groove_on || v.set.groove.is_some() || v.external_name.is_some())
            });
        let timing =
            args.get("restore") == Some(&json!(true)) || args.get("freeze") == Some(&json!(true));
        if !ready && !timing {
            return Some(false);
        }
        let ctx = self.roll_ctx(g)?;
        let out = self.roll.command(&ctx, command, args);
        Some(self.apply_roll(g, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Hit;
    use xirius_core::id::NoteId;

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    /// The roll on a clip of its own with notes at `starts` (ticks into the clip), all selected.
    fn roll_on(s: &mut Studio, g: &Geometry, starts: &[(i64, u8)]) -> (ClipId, Vec<NoteId>) {
        let id = |v: Value| {
            v["id"]
                .as_str()
                .or_else(|| v["ids"][0].as_str())
                .unwrap()
                .to_string()
        };
        let t = id(s
            .song
            .run("track.add.instrument", json!({ "name": "Q" }))
            .unwrap());
        let c: ClipId = id(s
            .song
            .run(
                "clip.add.midi",
                json!({ "track": t, "at": 3840, "bars": 1 }),
            )
            .unwrap())
        .parse()
        .unwrap();
        let ids = starts
            .iter()
            .map(|(at, v)| {
                id(s.song
                    .run(
                        "note.add",
                        json!({ "clip": c.to_string(), "at": 3840 + at, "pitch": 60,
                                "length": 100, "velocity": v }),
                    )
                    .unwrap())
                .parse()
                .unwrap()
            })
            .collect::<Vec<NoteId>>();
        s.selected_clip = Some((t.parse().unwrap(), c));
        s.drawer_open = true;
        s.tab = crate::studio::DrawerTab::Roll;
        s.rebuild(g);
        if let Some(ctx) = s.roll_ctx(g) {
            s.roll.sync(&ctx);
        }
        s.roll.selected = ids.iter().copied().collect();
        (c, ids)
    }

    fn starts(s: &Studio, c: ClipId) -> Vec<(i64, i64, u8)> {
        let p = s.project();
        let (_, clip) = p.clip(c).unwrap();
        let mut v: Vec<_> = clip
            .notes()
            .iter()
            .map(|n| (n.start, n.length, n.velocity))
            .collect();
        v.sort_unstable();
        v
    }

    fn texts(s: &Studio, g: &Geometry) -> Vec<String> {
        s.popup
            .as_ref()
            .expect("a popup")
            .layout(g.size)
            .cells
            .into_iter()
            .map(|c| c.text)
            .filter(|t| !t.is_empty())
            .collect()
    }

    fn lit(s: &Studio, g: &Geometry) -> Vec<String> {
        s.popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .into_iter()
            .filter(|c| c.on)
            .map(|c| c.text)
            .collect()
    }

    /// Presses the `nth` live button that says `text`, where it is on screen.
    fn press_nth(s: &mut Studio, g: &Geometry, text: &str, nth: usize) {
        let layout = s.popup.as_ref().expect("a popup").layout(g.size);
        let i = layout
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.text == text && c.live())
            .map(|(i, _)| i)
            .nth(nth)
            .unwrap_or_else(|| panic!("no live {text}"));
        let r = layout.cells[i].rect;
        let hit = s.hit(g, r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert_eq!(hit, Hit::Item(i), "{text}");
        assert!(s.press(g, hit));
    }

    fn press(s: &mut Studio, g: &Geometry, text: &str) {
        press_nth(s, g, text, 0);
    }

    /// Opens the panel from the grid pop-over, as a press on the top bar's grid button would.
    fn open(s: &mut Studio, g: &Geometry) {
        s.open_popup(PopupKind::Grid, Rect::new(700.0, -34.0, 90.0, 28.0));
        press(s, g, "クオンタイズの設定…");
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Quantize));
    }

    #[test]
    fn the_panel_opens_from_the_grid_popover_and_shows_the_grid_and_the_percents() {
        let (mut s, g) = studio();
        roll_on(&mut s, &g, &[(130, 90)]);
        open(&mut s, &g);
        let t = texts(&s, &g);
        for want in [
            "グリッド",
            "グルーヴ",
            "1/16",
            "連符",
            "なし",
            "スウィング",
            "開始",
            "100%",
            "終わり",
            "長さを保つ",
            "ベロシティ",
            "範囲",
            "標準",
            "長さも",
            "選択をクオンタイズ",
        ] {
            assert!(t.iter().any(|x| x == want), "{want} in {t:?}");
        }
        assert_eq!(lit(&s, &g), ["グリッド", "1/16", "なし", "標準"]);
        // The panel fits the window under the grid button.
        let r = s.popup.as_ref().unwrap().layout(g.size).rect;
        assert!(r.bottom() <= g.size.1 && r.w == W, "{r:?}");
    }

    #[test]
    fn values_set_in_the_panel_go_with_q_and_a_preset_brings_them_back() {
        let (mut s, g) = studio();
        // 1/16 = 240: 130 is 110 past 0 and 110 before 240.
        let (c, _) = roll_on(&mut s, &g, &[(100, 90), (590, 90)]);
        open(&mut s, &g);
        // 開始 50 %: ten presses of −.
        for _ in 0..10 {
            press_nth(&mut s, &g, "−", 0);
        }
        assert!(texts(&s, &g).iter().any(|t| t == "50%"));
        assert_eq!(lit(&s, &g), ["グリッド", "1/16", "なし", "50%"]);
        // 終わり 100 %.
        for _ in 0..20 {
            press_nth(&mut s, &g, "+", 2);
        }
        // Close the panel and press Q in the roll.
        assert!(s.edit_key(&g, "escape", Mods::NONE));
        assert!(s.popup.is_none());
        let before = s.song.history().len();
        assert!(s.edit_key(&g, "q", Mods::NONE));
        // Starts halfway to their lines (0, 240, 480, 720): 100 → 50, 590 → 535.
        // Ends all the way: 200 → 240, 690 → 720.
        assert_eq!(starts(&s, c), [(50, 190, 90), (535, 185, 90)]);
        assert_eq!(s.song.history().len(), before + 1);

        // The preset 標準 sets them back: Q is the plain quantize again.
        s.undo(&g);
        open(&mut s, &g);
        press(&mut s, &g, "標準");
        assert_eq!(lit(&s, &g), ["グリッド", "1/16", "なし", "標準"]);
        assert_eq!(s.quant, QuantSet::default());
        assert!(s.edit_key(&g, "escape", Mods::NONE));
        s.edit_key(&g, "q", Mods::NONE);
        assert_eq!(starts(&s, c), [(0, 100, 90), (480, 100, 90)]);
    }

    #[test]
    fn tuplets_and_swing_and_the_apply_button_quantize_the_selection() {
        let (mut s, g) = studio();
        let (c, _) = roll_on(&mut s, &g, &[(200, 90)]);
        open(&mut s, &g);
        press(&mut s, &g, "5");
        assert_eq!(s.quant.tuplet, 5);
        // No swing on tuplets.
        let swing_live = s
            .popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .iter()
            .any(|c| c.look == Look::Label && c.text == "スウィング" && c.enabled);
        assert!(!swing_live);
        // Q while the panel is open: 5 in a beat puts 200 on 192.
        assert!(s.edit_key(&g, "q", Mods::NONE));
        assert_eq!(starts(&s, c), [(192, 100, 90)]);
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Quantize));

        // 3 is the grid's 3連.
        press(&mut s, &g, "3");
        assert_eq!(s.song.grid(), "1/16t");
        assert_eq!(s.quant.tuplet, 1);
        press(&mut s, &g, "なし");
        assert_eq!(s.song.grid(), "1/16");

        // The スウィング preset moves the off sixteenth, 240, to 300: 192 goes there.
        press(&mut s, &g, "スウィング");
        assert!(lit(&s, &g).contains(&"スウィング".to_string()));
        press(&mut s, &g, "選択をクオンタイズ");
        assert_eq!(starts(&s, c), [(300, 100, 90)]);
    }

    #[test]
    fn a_groove_comes_from_another_clip_and_moves_velocities_too() {
        let (mut s, g) = studio();
        let (groove, _) = roll_on(&mut s, &g, &[(0, 120), (1000, 40)]);
        let (c, _) = roll_on(&mut s, &g, &[(30, 100), (980, 100)]);
        open(&mut s, &g);
        press(&mut s, &g, "グルーヴ");
        // No clip picked yet: nothing to apply.
        let apply = s
            .popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .into_iter()
            .find(|c| c.text == "選択をクオンタイズ")
            .unwrap();
        assert!(!apply.live());
        let t = texts(&s, &g);
        assert!(t.contains(&"グルーヴにするクリップ".to_string()), "{t:?}");
        let name = s
            .popup
            .as_ref()
            .unwrap()
            .quant
            .as_ref()
            .unwrap()
            .clips
            .iter()
            .find(|(id, _)| *id == groove)
            .unwrap()
            .1
            .clone();
        assert!(name.ends_with("2 小節目"), "{name}");
        press(&mut s, &g, &name);
        assert!(lit(&s, &g).contains(&name));
        // ベロシティ is live for a groove, all the way at 標準.
        let velocity_live = s
            .popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .iter()
            .any(|c| c.look == Look::Label && c.text == "ベロシティ" && c.enabled);
        assert!(velocity_live);
        press(&mut s, &g, "選択をクオンタイズ");
        assert_eq!(starts(&s, c), [(0, 100, 120), (1000, 100, 40)]);
    }

    #[test]
    fn left_at_standard_the_panel_adds_nothing_to_q() {
        let q = QuantSet::default();
        let args = json!({ "ids": ["nte1"], "grid": "240", "strength": 50 });
        assert_eq!(q.args(args.clone(), false), args);
        let mut q = QuantSet {
            tuplet: 7,
            swing: 30,
            start: 80,
            end: 40,
            range: 60,
            ..QuantSet::default()
        };
        assert_eq!(
            q.args(args.clone(), false),
            json!({ "ids": ["nte1"], "grid": "240", "strength": 40, "tuplet": 7,
                    "end": 40, "range": 60 })
        );
        q.tuplet = 1;
        assert_eq!(q.args(args.clone(), false)["swing"], 30);
        // On 3連 neither tuplets nor swing go.
        let on_triplet = q.args(args, true);
        assert!(on_triplet.get("swing").is_none() && on_triplet.get("tuplet").is_none());
        for (i, p) in PRESETS.iter().enumerate() {
            let mut q = QuantSet::default();
            q.load(i);
            assert_eq!(q.preset(), Some(i), "{}", p.name);
        }
    }
}
