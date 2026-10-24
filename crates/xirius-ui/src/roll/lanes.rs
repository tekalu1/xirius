//! The controller lanes under the grid (EDT-018, EDT-058 … EDT-068): which
//! lanes are shown, the strip with its "+" and tools, and the drags inside a
//! lane. A lane drag keeps the points as they were at the press and commits
//! one `cc.replace` (or one `note.set` for the velocity bars) on release.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;
use xirius_core::model::{CtrlKind, CtrlPoint};
use xirius_core::shape::{Shape, Stroke, value_at as shape_value};
use xirius_core::time::Tick;

use super::frame::{Frame, LANE_PAD, View};
use super::notes::diff_set;
use super::{Balloon, Ctx, Drag, Edit, Menu, Mode, Out, Preview, Roll, Then};
use crate::studio::Rect;
use crate::tools::round_in_bars;
use xirius_core::id::NoteId;

/// What a lane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LaneKind {
    /// One bar per note; the notes' own velocity.
    Velocity,
    Ctrl(CtrlKind),
}

/// Controller numbers offered under "CC 番号を指定…".
pub const NUMBERED: [u8; 7] = [2, 7, 10, 71, 74, 91, 93];

/// The lanes offered by "+" before the numbered ones.
pub const COMMON: [LaneKind; 6] = [
    LaneKind::Velocity,
    LaneKind::Ctrl(CtrlKind::PitchBend),
    LaneKind::Ctrl(CtrlKind::Cc(1)),
    LaneKind::Ctrl(CtrlKind::Cc(11)),
    LaneKind::Ctrl(CtrlKind::Cc(64)),
    LaneKind::Ctrl(CtrlKind::Aftertouch),
];

impl LaneKind {
    /// The short name in the lane's head.
    pub fn name(self) -> String {
        match self {
            LaneKind::Velocity => "Vel".into(),
            LaneKind::Ctrl(CtrlKind::PitchBend) => "ベンド".into(),
            LaneKind::Ctrl(CtrlKind::Cc(1)) => "Mod".into(),
            LaneKind::Ctrl(CtrlKind::Cc(11)) => "Expr".into(),
            LaneKind::Ctrl(CtrlKind::Cc(64)) => "Sus".into(),
            LaneKind::Ctrl(CtrlKind::Aftertouch) => "AT".into(),
            LaneKind::Ctrl(CtrlKind::Cc(n)) => format!("CC {n}"),
        }
    }

    /// The name in the "+" menu.
    pub fn full_name(self) -> String {
        match self {
            LaneKind::Velocity => "ベロシティ".into(),
            LaneKind::Ctrl(CtrlKind::PitchBend) => "ピッチベンド".into(),
            LaneKind::Ctrl(CtrlKind::Cc(1)) => "モジュレーション (CC1)".into(),
            LaneKind::Ctrl(CtrlKind::Cc(11)) => "エクスプレッション (CC11)".into(),
            LaneKind::Ctrl(CtrlKind::Cc(64)) => "サステイン (CC64)".into(),
            LaneKind::Ctrl(CtrlKind::Aftertouch) => "アフタータッチ".into(),
            LaneKind::Ctrl(CtrlKind::Cc(n)) => format!("CC {n}"),
        }
    }

    pub fn range(self) -> (i32, i32) {
        match self {
            LaneKind::Velocity => (1, 127),
            LaneKind::Ctrl(k) => k.range(),
        }
    }

    pub fn ctrl(self) -> Option<CtrlKind> {
        match self {
            LaneKind::Velocity => None,
            LaneKind::Ctrl(k) => Some(k),
        }
    }

    /// A value the lane can hold: the sustain pedal is on or off, the bend
    /// sticks to the centre.
    pub fn conform(self, value: i32) -> i32 {
        let (a, b) = self.range();
        let v = value.clamp(a, b);
        match self {
            LaneKind::Ctrl(CtrlKind::Cc(64)) => {
                if v >= 64 {
                    127
                } else {
                    0
                }
            }
            LaneKind::Ctrl(CtrlKind::PitchBend) if v.abs() < 400 => 0,
            _ => v,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lane {
    pub kind: LaneKind,
    pub folded: bool,
}

impl Lane {
    pub fn new(kind: LaneKind) -> Lane {
        Lane {
            kind,
            folded: false,
        }
    }
}

/// The tools of the strip (EDT-060).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneTool {
    Pencil,
    Line,
    Select,
}

impl LaneTool {
    pub fn from_tools(tools: crate::tools::Tools, mods: crate::tools::Mods) -> Self {
        use crate::tools::{Area, Tool};
        match tools.held(Area::Roll, mods) {
            Tool::Draw if mods.shift => Self::Line,
            Tool::Draw => Self::Pencil,
            _ => Self::Select,
        }
    }

    pub const ALL: [LaneTool; 3] = [LaneTool::Pencil, LaneTool::Line, LaneTool::Select];

    pub fn icon(self) -> &'static str {
        match self {
            LaneTool::Pencil => "pencil",
            LaneTool::Line => "line",
            LaneTool::Select => "select",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LaneTool::Pencil => "鉛筆",
            LaneTool::Line => "ライン",
            LaneTool::Select => "選択",
        }
    }
}

/// A drag inside a controller lane.
#[derive(Clone, Debug)]
pub struct LaneDrag {
    pub index: usize,
    pub what: LaneWhat,
    /// What a release would write: the stretch to replace and its points.
    pub out: Option<(Tick, Tick, Vec<CtrlPoint>)>,
}

#[derive(Clone, Debug)]
pub enum LaneWhat {
    /// Velocity bars: the notes dragged are in [`Drag::orig`].
    Bars,
    PaintBars {
        line: bool,
        values: BTreeMap<NoteId, u8>,
    },
    /// A stroke of the pencil, or a line from where it began.
    Draw {
        start: (Tick, i32),
        last: (Tick, i32),
        stroke: BTreeMap<Tick, i32>,
        line: bool,
        /// The `cc.draw` arguments when the pencil lays a shape rather than its path.
        shape: Option<serde_json::Value>,
    },
    /// A rectangle drawn in the empty part of the lane: the points (or, in the velocity
    /// lane, the bar tips) inside it are selected, added to what was selected when `add`.
    Rect {
        points: BTreeSet<Tick>,
        notes: BTreeSet<NoteId>,
    },
    /// The handles round the selected velocity bars (EDT-060): the one on top scales the values
    /// about their mean, the one at the right-hand corner tilts them along the time. `scale` is
    /// a percentage and `by` the amount the last bar gains; a release writes both.
    Transform { tilt: bool, scale: i64, by: i64 },
    /// Selected points being moved.
    Points {
        all: Vec<CtrlPoint>,
        moving: Vec<Tick>,
        head: Tick,
        /// How far the drag has moved them in time.
        dt: Tick,
    },
}

/// What a row of the "+" menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAct {
    Show(LaneKind),
    Numbers,
}

/// The frame round the selected bars of the velocity lane, with its two handles.
#[derive(Clone, Copy, Debug)]
pub struct Handles {
    pub frame: Rect,
    pub scale: Rect,
    /// None where the bars start together: there is nothing to tilt along.
    pub tilt: Option<Rect>,
}

/// A handle takes the pointer this far outside its square.
const HANDLE_GRAB: f32 = 3.0;

/// A point within this many pixels is hit.
const POINT_HIT: f32 = 6.0;
/// A velocity bar within this many pixels of the pointer is hit.
const BAR_HIT: f32 = 4.0;

fn inner(plot: &Rect) -> (f32, f32) {
    (plot.y + LANE_PAD, (plot.h - LANE_PAD * 2.0).max(1.0))
}

/// The value at window y in a lane's plot.
pub fn value_at(kind: LaneKind, plot: &Rect, y: f32) -> i32 {
    let (a, b) = kind.range();
    let (top, h) = inner(plot);
    let f = 1.0 - ((y - top) / h).clamp(0.0, 1.0);
    (a as f32 + f * (b - a) as f32).round() as i32
}

/// Window y of a value.
pub fn y_of(kind: LaneKind, plot: &Rect, value: i32) -> f32 {
    let (a, b) = kind.range();
    let (top, h) = inner(plot);
    top + (1.0 - (value - a) as f32 / (b - a) as f32) * h
}

impl Roll {
    /// The points of a controller as the song has them.
    pub fn song_points(&self, ctx: &Ctx, kind: CtrlKind) -> Vec<CtrlPoint> {
        ctx.clip
            .controllers
            .iter()
            .find(|l| l.kind == kind)
            .map(|l| l.points.to_vec())
            .unwrap_or_default()
    }

    /// The points of lane `index` as they are shown now.
    pub fn shown_points(&self, ctx: &Ctx, index: usize) -> Vec<CtrlPoint> {
        if let Some((i, pts)) = &self.preview.lane
            && *i == index
        {
            return pts.clone();
        }
        match self.lanes.get(index).and_then(|l| l.kind.ctrl()) {
            Some(k) => self.song_points(ctx, k),
            None => Vec::new(),
        }
    }

    /// What the "+" menu lists now.
    /// The frame and handles of the bars selected in the velocity lane, when two or more are.
    pub fn velocity_handles(&self, ctx: &Ctx, v: &View, plot: &Rect) -> Option<Handles> {
        let sel: Vec<_> = ctx
            .notes()
            .iter()
            .filter(|n| self.selected.contains(&n.id))
            .collect();
        if sel.len() < 2 {
            return None;
        }
        let x0 = sel.iter().map(|n| v.x(n.start)).fold(f32::MAX, f32::min);
        let x1 = sel.iter().map(|n| v.x(n.start)).fold(f32::MIN, f32::max);
        let loudest = sel.iter().map(|n| i32::from(n.velocity)).max()?;
        let top = (y_of(LaneKind::Velocity, plot, loudest) - 10.0).max(plot.y + 1.0);
        Some(Handles {
            frame: Rect::new(
                x0 - 6.0,
                top + 4.0,
                x1 - x0 + 12.0,
                plot.bottom() - top - 5.0,
            ),
            scale: Rect::new((x0 + x1) / 2.0 - 4.0, top, 8.0, 8.0),
            tilt: (x1 - x0 >= 20.0).then(|| Rect::new(x1 + 2.0, top, 8.0, 8.0)),
        })
    }

    /// Which handle (true for the tilt one) is under the pointer in the velocity lane.
    pub fn handle_at(&self, ctx: &Ctx, plot: &Rect, x: f32, y: f32) -> Option<bool> {
        let h = self.velocity_handles(ctx, &self.view(ctx), plot)?;
        let grab = |r: Rect| {
            Rect::new(
                r.x - HANDLE_GRAB,
                r.y - HANDLE_GRAB,
                r.w + HANDLE_GRAB * 2.0,
                r.h + HANDLE_GRAB * 2.0,
            )
            .contains(x, y)
        };
        if h.tilt.is_some_and(grab) {
            Some(true)
        } else if grab(h.scale) {
            Some(false)
        } else {
            None
        }
    }

    fn transform_down(&mut self, ctx: &Ctx, index: usize, tilt: bool, x: f32, y: f32) -> Out {
        let plot = self.frame(ctx.body).lanes[index].plot;
        let (_, h) = inner(&plot);
        let mut d = Drag::new(Mode::Lane, (x, y));
        d.vel_per_px = 126.0 / h;
        d.orig = ctx
            .notes()
            .iter()
            .filter(|n| self.selected.contains(&n.id))
            .map(|n| (n.id, super::NoteView::from(n)))
            .collect();
        d.map_context = Some("lane.select.velocity-bar.drag".into());
        d.lane = Some(LaneDrag {
            index,
            what: LaneWhat::Transform {
                tilt,
                scale: 100,
                by: 0,
            },
            out: None,
        });
        self.drag = Some(d);
        Out::used()
    }

    pub fn menu_entries(&self) -> Vec<(String, MenuAct)> {
        let shown = |k: LaneKind| self.lanes.iter().any(|l| l.kind == k);
        match self.menu {
            Some(Menu::Numbers) => NUMBERED
                .iter()
                .map(|n| LaneKind::Ctrl(CtrlKind::Cc(*n)))
                .filter(|k| !shown(*k))
                .map(|k| (k.full_name(), MenuAct::Show(k)))
                .collect(),
            Some(Menu::Add) => {
                let mut v: Vec<(String, MenuAct)> = COMMON
                    .iter()
                    .filter(|k| !shown(**k))
                    .map(|k| (k.full_name(), MenuAct::Show(*k)))
                    .collect();
                v.push(("CC 番号を指定…".into(), MenuAct::Numbers));
                v
            }
            None => Vec::new(),
        }
    }

    fn show_lane(&mut self, kind: LaneKind) {
        if let Some(k) = kind.ctrl() {
            self.hidden.remove(&k);
        }
        if let Some(i) = self.lanes.iter().position(|l| l.kind == kind) {
            self.active_lane = i;
        } else {
            self.lanes.push(Lane::new(kind));
            self.active_lane = self.lanes.len() - 1;
        }
        self.lanes_folded = false;
        self.points.clear();
    }

    pub fn menu_down(&mut self, ctx: &Ctx, x: f32, y: f32) -> Out {
        let frame = self.frame(ctx.body);
        let entries = self.menu_entries();
        let rect = self.menu_rect(&frame, entries.len());
        if rect.contains(x, y) {
            for (i, (_, act)) in entries.iter().enumerate() {
                if self.menu_item_rect(rect, i).contains(x, y) {
                    match act {
                        MenuAct::Show(k) => {
                            self.menu = None;
                            self.show_lane(*k);
                        }
                        MenuAct::Numbers => self.menu = Some(Menu::Numbers),
                    }
                    break;
                }
            }
        } else {
            self.menu = None;
        }
        Out::used()
    }

    pub fn strip_down(&mut self, ctx: &Ctx, frame: &Frame, x: f32, y: f32) -> Out {
        if let Some(out) = self.zoom_down(ctx, frame, x, y) {
            return out;
        }
        if frame.follow.contains(x, y) {
            return Out {
                follow: true,
                ..Out::used()
            };
        }
        if self.lanes_folded {
            if frame.fold.contains(x, y) {
                self.lanes_folded = false;
            }
            return Out::used();
        }
        if frame.plus.contains(x, y) {
            self.menu = if self.menu.is_some() {
                None
            } else {
                Some(Menu::Add)
            };
        } else if frame.fold.contains(x, y) {
            self.lanes_folded = !self.lanes_folded;
        } else if let Some(i) = frame.tabs.iter().position(|r| r.contains(x, y)) {
            if self.active_lane != i {
                self.points.clear();
            }
            self.active_lane = i;
            super::lanemark::show(self, i);
        }
        Out::used()
    }

    /// A lane's step in ticks: the grid, but never finer than an eighth of a
    /// beat, and fine when the grid is off.
    pub(super) fn lane_tick(&self, ctx: &Ctx, v: &View, x: f32) -> Tick {
        let eighth = (ctx.ppq() / 8).max(1);
        let step = ctx
            .snap(self.beat_px, self.mods.shift)
            .map_or((ctx.ppq() / 16).max(1), |s| s.max(eighth));
        let t = round_in_bars(&ctx.project.time_map(), v.tick(x) + ctx.clip.start, step)
            - ctx.clip.start;
        t.clamp(0, (ctx.clip_len() - 1).max(0))
    }

    pub fn lane_down(
        &mut self,
        ctx: &Ctx,
        frame: &Frame,
        index: usize,
        x: f32,
        y: f32,
        clicks: u32,
    ) -> Out {
        self.lane_tool = LaneTool::from_tools(ctx.tools, self.mods);
        let lb = frame.lanes[index];
        if self.active_lane != index {
            self.points.clear();
        }
        if lb.head.contains(x, y) {
            if self.active_lane != index {
                self.points.clear();
            }
            if lb.fold.contains(x, y) {
                self.lanes[index].folded = !self.lanes[index].folded;
                self.active_lane = index;
            } else {
                self.active_lane = index;
            }
            return Out::used();
        }
        if lb.folded {
            self.lanes[index].folded = false;
            self.lanes_folded = false;
            self.active_lane = index;
            return Out::used();
        }
        if self.active_lane != index {
            self.points.clear();
        }
        self.active_lane = index;
        let kind = self.lanes[index].kind;
        let v = self.view(ctx);
        if kind == LaneKind::Velocity
            && self.lane_tool == LaneTool::Select
            && let Some(tilt) = self.handle_at(ctx, &lb.plot, x, y)
        {
            return self.transform_down(ctx, index, tilt, x, y);
        }
        // The mousemap's name for what is under the pointer (`lane.select.velocity-bar.drag`).
        let raw = self.mods;
        let on_target = match kind {
            LaneKind::Velocity => bar_hit(ctx, &v, x).is_some(),
            LaneKind::Ctrl(ck) => {
                nearest_point(&self.song_points(ctx, ck), kind, &lb.plot, &v, x, y).is_some()
            }
        };
        let tool = crate::mouse_dispatch::lane_tool(ctx.tools.held(crate::tools::Area::Roll, raw));
        let zone = crate::mouse_dispatch::lane_zone(kind == LaneKind::Velocity, on_target);
        let context = format!("lane.{tool}.{zone}.drag");
        let click =
            crate::mouse_dispatch::action(&ctx.mousemap, &context.replace(".drag", ".click"), raw);
        let gesture = crate::mouse_dispatch::action(&ctx.mousemap, &context, raw);
        if gesture.as_deref() == Some("") && click.as_deref() == Some("") {
            return Out::used();
        }
        self.mods = crate::mouse_dispatch::modifiers(&ctx.mousemap, &context, raw, false);
        if let Some(action) = click.as_deref() {
            self.mods.shift = action == "select.toggle";
        }
        self.lane_tool = LaneTool::from_tools(ctx.tools, self.mods);
        let rect = match gesture.as_deref() {
            Some(g @ ("select.rect" | "select.rect --add")) if !on_target && clicks < 2 => {
                Some(g.ends_with("--add"))
            }
            _ => None,
        };
        let out = match kind {
            _ if rect.is_some() => self.rect_down(index, x, y, rect == Some(true)),
            LaneKind::Velocity => self.bar_down(ctx, &v, index, x, y),
            LaneKind::Ctrl(ck) => {
                let pts = self.song_points(ctx, ck);
                let near = nearest_point(&pts, kind, &lb.plot, &v, x, y);
                match self.lane_tool {
                    LaneTool::Select => {
                        self.points_down(ctx, &v, index, &lb.plot, pts, near, x, y, clicks)
                    }
                    LaneTool::Pencil | LaneTool::Line => {
                        let t = self.lane_tick(ctx, &v, x);
                        let val = kind.conform(value_at(kind, &lb.plot, y));
                        let mut d = Drag::new(Mode::Lane, (x, y));
                        d.lane = Some(LaneDrag {
                            index,
                            what: LaneWhat::Draw {
                                start: (t, val),
                                last: (t, val),
                                stroke: BTreeMap::from([(t, val)]),
                                line: self.lane_tool == LaneTool::Line,
                                shape: None,
                            },
                            out: None,
                        });
                        self.drag = Some(d);
                        self.recompute(ctx);
                        Out::used()
                    }
                }
            }
        };
        if let Some(d) = &mut self.drag {
            d.map_context = Some(context);
            d.click_action = click;
            d.drag_enabled = gesture.as_deref() != Some("");
        }
        out
    }

    fn rect_down(&mut self, index: usize, x: f32, y: f32, add: bool) -> Out {
        let mut d = Drag::new(Mode::Lane, (x, y));
        let (points, notes) = if add {
            (self.points.clone(), self.selected.clone())
        } else {
            (BTreeSet::new(), BTreeSet::new())
        };
        if !add {
            if self.lanes[index].kind == LaneKind::Velocity {
                self.selected.clear();
            } else {
                self.points.clear();
            }
        }
        d.lane = Some(LaneDrag {
            index,
            what: LaneWhat::Rect { points, notes },
            out: None,
        });
        self.drag = Some(d);
        Out::used()
    }

    fn bar_down(&mut self, ctx: &Ctx, v: &View, index: usize, x: f32, _y: f32) -> Out {
        if self.lane_tool != LaneTool::Select {
            let mut d = Drag::new(Mode::Lane, (x, _y));
            d.orig = ctx
                .notes()
                .iter()
                .map(|n| (n.id, super::NoteView::from(n)))
                .collect();
            d.lane = Some(LaneDrag {
                index,
                what: LaneWhat::PaintBars {
                    line: self.lane_tool == LaneTool::Line,
                    values: BTreeMap::new(),
                },
                out: None,
            });
            self.drag = Some(d);
            self.recompute(ctx);
            return Out::used();
        }
        let Some(id) = bar_hit(ctx, v, x) else {
            return Out::used();
        };
        if self.mods.shift {
            self.selected.insert(id);
        } else if !self.selected.contains(&id) {
            self.selected.clear();
            self.selected.insert(id);
        }
        let frame = self.frame(ctx.body);
        let plot = frame.lanes[index].plot;
        let (_, h) = inner(&plot);
        let mut d = Drag::new(Mode::Lane, (x, _y));
        d.vel_per_px = 126.0 / h;
        d.orig = ctx
            .notes()
            .iter()
            .filter(|n| self.selected.contains(&n.id))
            .map(|n| (n.id, super::NoteView::from(n)))
            .collect();
        d.head = d.orig.iter().position(|(i, _)| *i == id).unwrap_or(0);
        d.lane = Some(LaneDrag {
            index,
            what: LaneWhat::Bars,
            out: None,
        });
        self.drag = Some(d);
        Out::used()
    }

    #[allow(clippy::too_many_arguments)]
    fn points_down(
        &mut self,
        ctx: &Ctx,
        v: &View,
        index: usize,
        plot: &Rect,
        pts: Vec<CtrlPoint>,
        near: Option<usize>,
        x: f32,
        y: f32,
        clicks: u32,
    ) -> Out {
        let kind = self.lanes[index].kind;
        let ck = kind.ctrl().unwrap_or(CtrlKind::Aftertouch);
        let key = ck.key();
        if clicks >= 2 {
            let clip = ctx.clip.id.to_string();
            return match near {
                Some(i) => {
                    let t = pts[i].tick;
                    self.points.remove(&t);
                    Out::edit(Edit::new(
                        "cc.replace",
                        json!({ "clip": clip, "lane": key, "from": t, "to": t }),
                        Then::Keep,
                    ))
                }
                None => {
                    let t = self.lane_tick(ctx, v, x);
                    let val = kind.conform(value_at(kind, plot, y));
                    self.points = [t].into();
                    Out::edit(Edit::new(
                        "cc.replace",
                        json!({
                            "clip": clip, "lane": key, "from": t, "to": t,
                            "points": [{ "tick": t, "value": val }],
                        }),
                        Then::Keep,
                    ))
                }
            };
        }
        let Some(i) = near else {
            if !self.mods.shift {
                self.points.clear();
            }
            return Out::used();
        };
        let t = pts[i].tick;
        if self.mods.shift {
            if !self.points.remove(&t) {
                self.points.insert(t);
            }
        } else if !self.points.contains(&t) {
            self.points = [t].into();
        }
        let moving: Vec<Tick> = pts
            .iter()
            .map(|p| p.tick)
            .filter(|t| self.points.contains(t))
            .collect();
        let mut d = Drag::new(Mode::Lane, (x, y));
        d.lane = Some(LaneDrag {
            index,
            what: LaneWhat::Points {
                all: pts,
                moving,
                head: t,
                dt: 0,
            },
            out: None,
        });
        self.drag = Some(d);
        Out::used()
    }

    /// Works out the lane drag's preview from the pointer and the keys held.
    pub fn lane_update(&mut self, ctx: &Ctx, d: &mut Drag, pv: &mut Preview) {
        let Some((px, py)) = self.pointer else {
            return;
        };
        let Some(mut ld) = d.lane.take() else {
            return;
        };
        let frame = self.frame(ctx.body);
        let Some(lb) = frame.lanes.iter().find(|l| l.index == ld.index).copied() else {
            d.lane = Some(ld);
            return;
        };
        let Some(kind) = self.lanes.get(ld.index).map(|l| l.kind) else {
            d.lane = Some(ld);
            return;
        };
        let v = self.view(ctx);
        let alt = self.mods.alt;
        let ctrl = self.mods.ctrl;
        match &mut ld.what {
            LaneWhat::PaintBars { line, values } => {
                let (ax, ay) = if *line || alt { d.press } else { d.last };
                if *line || alt {
                    values.clear();
                }
                let left = ax.min(px) - BAR_HIT;
                let right = ax.max(px) + BAR_HIT;
                for (id, n) in &d.orig {
                    let x = v.x(n.start);
                    if x < left || x > right {
                        continue;
                    }
                    let t = if (px - ax).abs() < 0.01 {
                        1.0
                    } else {
                        ((x - ax) / (px - ax)).clamp(0.0, 1.0)
                    };
                    values.insert(*id, value_at(kind, &lb.plot, ay + (py - ay) * t) as u8);
                }
                // A shape from the top bar lays its height over the bars it passes (UI-128).
                let shape = ctx.shape;
                if !*line
                    && shape.is_generated()
                    && shape != Shape::Line
                    && (shape.is_wave() || !alt)
                {
                    let (a, b) = {
                        let (t0, t1) = (v.tick(d.press.0), v.tick(px));
                        (t0.min(t1), t0.max(t1))
                    };
                    let first = value_at(kind, &lb.plot, d.press.1);
                    let now = value_at(kind, &lb.plot, py);
                    let (left_v, right_v) = if v.tick(d.press.0) <= v.tick(px) {
                        (first, now)
                    } else {
                        (now, first)
                    };
                    let strayed = (now - first).abs();
                    let amp = if strayed * 50 > 126 {
                        f64::from(strayed)
                    } else {
                        126.0 / 4.0
                    };
                    let s = Stroke {
                        from: a,
                        to: b,
                        v0: f64::from(if shape.is_wave() { first } else { left_v }),
                        v1: f64::from(if shape.is_wave() { first } else { right_v }),
                        amp,
                        period: if alt {
                            (b - a) as f64
                        } else {
                            ctx.ppq() as f64
                        },
                        phase: 0.0,
                    };
                    values.clear();
                    for (id, n) in &d.orig {
                        if n.start >= a && n.start <= b {
                            values.insert(*id, at_value(shape, &s, n.start, 1, 127) as u8);
                        }
                    }
                }
                for (id, n) in &d.orig {
                    if let Some(value) = values.get(id) {
                        pv.changed.insert(
                            *id,
                            super::NoteView {
                                velocity: *value,
                                ..*n
                            },
                        );
                    }
                }
                pv.balloon = Some(Balloon {
                    at: (px as i32 + 14, py as i32 - 28),
                    text: format!("ベロシティ {}", value_at(kind, &lb.plot, py)),
                });
            }
            LaneWhat::Rect { points, notes } => {
                let (ax, ay) = (
                    d.press.0.clamp(lb.plot.x, lb.plot.right()),
                    d.press.1.clamp(lb.plot.y, lb.plot.bottom()),
                );
                let (bx, by) = (
                    px.clamp(lb.plot.x, lb.plot.right()),
                    py.clamp(lb.plot.y, lb.plot.bottom()),
                );
                let (x0, x1) = (ax.min(bx), ax.max(bx));
                let (y0, y1) = (ay.min(by), ay.max(by));
                let inside = |x: f32, y: f32| x >= x0 && x <= x1 && y >= y0 && y <= y1;
                match kind.ctrl() {
                    None => {
                        let mut sel = notes.clone();
                        for n in ctx.notes() {
                            let y = y_of(kind, &lb.plot, i32::from(n.velocity));
                            if inside(v.x(n.start), y) {
                                sel.insert(n.id);
                            }
                        }
                        self.selected = sel;
                    }
                    Some(ck) => {
                        let mut sel = points.clone();
                        for p in self.song_points(ctx, ck) {
                            if inside(v.x(p.tick), y_of(kind, &lb.plot, p.value)) {
                                sel.insert(p.tick);
                            }
                        }
                        self.points = sel;
                    }
                }
                pv.band = Some(((x0, y0), (x1, y1)));
            }
            LaneWhat::Transform { tilt, scale, by } => {
                if d.moved {
                    let dv = (-(py - d.press.1) * d.vel_per_px).round() as i64;
                    let vels: Vec<f64> =
                        d.orig.iter().map(|(_, o)| f64::from(o.velocity)).collect();
                    let mean = vels.iter().sum::<f64>() / vels.len().max(1) as f64;
                    let loudest = vels.iter().copied().fold(0.0, f64::max);
                    if *tilt {
                        (*scale, *by) = (100, dv.clamp(-126, 126));
                    } else {
                        // The handle sits over the loudest bar and carries its tip with it.
                        *by = 0;
                        *scale = if loudest > mean {
                            ((loudest + dv as f64 - mean) / (loudest - mean) * 100.0)
                                .round()
                                .clamp(0.0, 400.0) as i64
                        } else {
                            100
                        };
                    }
                    let lo = d.orig.iter().map(|(_, o)| o.start).min().unwrap_or(0);
                    let hi = d.orig.iter().map(|(_, o)| o.start).max().unwrap_or(0);
                    for (id, o) in &d.orig {
                        let along = if hi == lo {
                            0.0
                        } else {
                            (o.start - lo) as f64 / (hi - lo) as f64
                        };
                        let value = mean
                            + (f64::from(o.velocity) - mean) * (*scale as f64 / 100.0)
                            + *by as f64 * along;
                        let mut n = *o;
                        n.velocity = value.round().clamp(1.0, 127.0) as u8;
                        pv.changed.insert(*id, n);
                    }
                    pv.balloon = Some(Balloon {
                        at: (px as i32 + 14, py as i32 - 28),
                        text: if *tilt {
                            format!("傾き {:+}", *by)
                        } else {
                            format!("縮尺 {}%", *scale)
                        },
                    });
                }
            }
            LaneWhat::Bars => {
                if d.moved {
                    let dv = (-(py - d.press.1) * d.vel_per_px).round() as i32;
                    for (id, o) in &d.orig {
                        let mut n = *o;
                        n.velocity = (i32::from(o.velocity) + dv).clamp(1, 127) as u8;
                        pv.changed.insert(*id, n);
                    }
                    if let Some(n) = d.orig.get(d.head).and_then(|(id, _)| pv.changed.get(id)) {
                        pv.balloon = Some(Balloon {
                            at: (px as i32 + 14, py as i32 - 28),
                            text: format!("ベロシティ {}", n.velocity),
                        });
                    }
                }
            }
            LaneWhat::Draw {
                start,
                last,
                stroke,
                line,
                shape: cmd,
            } => {
                let ck = kind.ctrl().unwrap_or(CtrlKind::Aftertouch);
                let t = self.lane_tick(ctx, &v, px);
                let val = kind.conform(value_at(kind, &lb.plot, py));
                // Fill in between the last sample and this one.
                let eighth = (ctx.ppq() / 8).max(1);
                let st = ctx
                    .snap(self.beat_px, self.mods.shift)
                    .map_or((ctx.ppq() / 16).max(1), |s| s.max(eighth));
                let (t0, v0) = *last;
                if t != t0 {
                    let n = ((t - t0).abs() / st).max(1);
                    for i in 1..=n {
                        let ti = t0 + (t - t0) * i / n;
                        let vi = v0 + ((val - v0) as f64 * i as f64 / n as f64).round() as i32;
                        stroke.insert(ti, kind.conform(vi));
                    }
                } else {
                    stroke.insert(t, val);
                }
                *last = (t, val);
                let shape = ctx.shape;
                let generated = !*line
                    && shape.is_generated()
                    && shape != Shape::Line
                    && (shape.is_wave() || !alt);
                *cmd = None;
                let pts: Vec<CtrlPoint> = if generated && t != start.0 {
                    let ((a, v0), (b, v1)) = if start.0 <= t {
                        (*start, (t, val))
                    } else {
                        ((t, val), *start)
                    };
                    let (lo, hi) = ck.range();
                    let strayed = stroke
                        .values()
                        .map(|v| (v - start.1).abs())
                        .max()
                        .unwrap_or(0);
                    let amp = if strayed * 50 > hi - lo {
                        f64::from(strayed)
                    } else {
                        f64::from(hi - lo) / 4.0
                    };
                    let period = if alt {
                        (b - a) as f64
                    } else {
                        ctx.ppq() as f64
                    };
                    let s = Stroke {
                        from: a,
                        to: b,
                        v0: f64::from(if shape.is_wave() { start.1 } else { v0 }),
                        v1: f64::from(if shape.is_wave() { start.1 } else { v1 }),
                        amp,
                        period,
                        phase: 0.0,
                    };
                    let step = (ctx.ppq() / 4).max(1);
                    let mut at = a;
                    let mut l = Vec::new();
                    while at < b {
                        l.push(CtrlPoint::new(at, at_value(shape, &s, at, lo, hi)));
                        at += step;
                    }
                    l.push(CtrlPoint::new(b, at_value(shape, &s, b, lo, hi)));
                    *cmd = Some(json!({
                        "shape": shape.name(), "from": a, "to": b,
                        "value": s.v0, "end-value": s.v1, "amp": amp,
                        "period": period.round() as i64, "step": step,
                    }));
                    l
                } else if *line || alt || shape == Shape::Line {
                    let mut l = vec![CtrlPoint::new(start.0, start.1)];
                    if t != start.0 {
                        l.push(CtrlPoint::new(t, val));
                    }
                    l.sort_by_key(|p| p.tick);
                    l
                } else {
                    stroke.iter().map(|(t, v)| CtrlPoint::new(*t, *v)).collect()
                };
                let from = pts.first().map_or(0, |p| p.tick);
                let to = pts.last().map_or(0, |p| p.tick);
                let mut merged: Vec<CtrlPoint> = self
                    .song_points(ctx, ck)
                    .into_iter()
                    .filter(|p| p.tick < from || p.tick > to)
                    .collect();
                merged.extend(pts.iter().copied());
                merged.sort_by_key(|p| p.tick);
                pv.lane = Some((ld.index, merged));
                pv.balloon = Some(Balloon {
                    at: (px as i32 + 14, py as i32 - 28),
                    text: format!("{} {}", kind.name(), val),
                });
                ld.out = Some((from, to, pts));
            }
            LaneWhat::Points {
                all,
                moving,
                head,
                dt: moved_by,
            } => {
                if d.moved {
                    let (a, b) = kind.range();
                    let (_, h) = inner(&lb.plot);
                    let dv = (-(py - d.press.1) / h * (b - a) as f32).round() as i32;
                    let dt = if ctrl {
                        0
                    } else {
                        let want = *head + ((px - d.press.0) / v.beat_px * v.ppq).round() as Tick;
                        let tick = ctx.snapped(self.beat_px, self.mods.shift, want);
                        tick - *head
                    };
                    let lo = moving.iter().copied().min().unwrap_or(0);
                    let hi = moving.iter().copied().max().unwrap_or(0);
                    let dt = dt.clamp(-lo, (ctx.clip_len() - 1 - hi).max(-lo));
                    *moved_by = dt;
                    let mut out: Vec<CtrlPoint> = all
                        .iter()
                        .filter(|p| !moving.contains(&p.tick))
                        .copied()
                        .collect();
                    for p in all.iter().filter(|p| moving.contains(&p.tick)) {
                        out.push(CtrlPoint {
                            tick: p.tick + dt,
                            value: kind.conform(p.value + dv),
                            ..*p
                        });
                    }
                    out.sort_by_key(|p| p.tick);
                    out.dedup_by_key(|p| p.tick);
                    let to = out.last().map_or(0, |p| p.tick).max(ctx.clip_len());
                    let shown = out
                        .iter()
                        .find(|p| p.tick == *head + dt)
                        .map_or(0, |p| p.value);
                    pv.balloon = Some(Balloon {
                        at: (px as i32 + 14, py as i32 - 28),
                        text: format!("{} · {}", ctx.bbt(*head + dt), shown),
                    });
                    pv.lane = Some((ld.index, out.clone()));
                    ld.out = Some((0, to, out));
                }
            }
        }
        d.lane = Some(ld);
    }

    pub fn lane_up(&mut self, ctx: &Ctx, d: Drag, pv: Preview) -> Out {
        let Some(ld) = d.lane.clone() else {
            return Out::used();
        };
        match ld.what {
            LaneWhat::Rect { .. } => Out::used(),
            LaneWhat::Transform { scale, by, .. } => {
                if !d.moved || (scale == 100 && by == 0) {
                    return Out::used();
                }
                let ids: Vec<NoteId> = d.orig.iter().map(|(id, _)| *id).collect();
                Out::edit(Edit::new(
                    "note.velocity.transform",
                    json!({ "ids": super::ids_json(&ids), "scale": scale, "tilt": by }),
                    Then::Keep,
                ))
            }
            LaneWhat::Bars | LaneWhat::PaintBars { .. } => {
                if !d.moved && matches!(ld.what, LaneWhat::Bars) {
                    return Out::used();
                }
                match diff_set(&d.orig, &pv) {
                    Some(e) => Out::edit(e),
                    None => Out::used(),
                }
            }
            LaneWhat::Draw { .. } | LaneWhat::Points { .. } => {
                if let LaneWhat::Draw {
                    shape: Some(args), ..
                } = &ld.what
                    && let Some(ck) = self.lanes.get(ld.index).and_then(|l| l.kind.ctrl())
                {
                    let mut args = args.clone();
                    args["clip"] = json!(ctx.clip.id.to_string());
                    args["lane"] = json!(ck.key());
                    return Out::edit(Edit::new("cc.draw", args, Then::Keep));
                }
                let Some((from, to, pts)) = ld.out else {
                    return Out::used();
                };
                let Some(ck) = self.lanes.get(ld.index).and_then(|l| l.kind.ctrl()) else {
                    return Out::used();
                };
                if let LaneWhat::Points { moving, dt, .. } = &ld.what {
                    self.points = moving.iter().map(|t| t + dt).collect();
                }
                let list: Vec<_> = pts.iter().map(CtrlPoint::to_json).collect();
                Out::edit(Edit::new(
                    "cc.replace",
                    json!({
                        "clip": ctx.clip.id.to_string(),
                        "lane": ck.key(),
                        "from": from,
                        "to": to,
                        "points": list,
                    }),
                    Then::Keep,
                ))
            }
        }
    }

    /// Delete / Backspace in a controller lane: the selected points go.
    pub fn delete_points(&mut self, ctx: &Ctx) -> Option<Edit> {
        let kind = self.active()?.kind.ctrl()?;
        if self.points.is_empty() {
            return None;
        }
        let pts = self.song_points(ctx, kind);
        let keep: Vec<_> = pts
            .iter()
            .filter(|p| !self.points.contains(&p.tick))
            .map(CtrlPoint::to_json)
            .collect();
        if keep.len() == pts.len() {
            return None;
        }
        let to = pts.last().map_or(0, |p| p.tick).max(ctx.clip_len());
        self.points.clear();
        Some(Edit::new(
            "cc.replace",
            json!({
                "clip": ctx.clip.id.to_string(),
                "lane": kind.key(),
                "from": 0,
                "to": to,
                "points": keep,
            }),
            Then::Keep,
        ))
    }
}

/// The index of the point nearest to `(x, y)` within reach.
pub(super) fn nearest_point(
    pts: &[CtrlPoint],
    kind: LaneKind,
    plot: &Rect,
    v: &View,
    x: f32,
    y: f32,
) -> Option<usize> {
    pts.iter()
        .enumerate()
        .map(|(i, p)| {
            let (dx, dy) = (v.x(p.tick) - x, y_of(kind, plot, p.value) - y);
            (i, dx.hypot(dy))
        })
        .filter(|(_, d)| *d <= POINT_HIT)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The velocity bar within reach of window x: the nearest note start.
pub(super) fn bar_hit(ctx: &Ctx, v: &View, x: f32) -> Option<NoteId> {
    ctx.notes()
        .iter()
        .filter(|n| (v.x(n.start) - x).abs() <= BAR_HIT)
        .min_by(|a, b| {
            let da = (v.x(a.start) - x).abs();
            let db = (v.x(b.start) - x).abs();
            da.total_cmp(&db)
        })
        .map(|n| n.id)
}

/// The height `shape` has at `tick`, kept within a controller's range.
fn at_value(shape: Shape, s: &Stroke, tick: Tick, lo: i32, hi: i32) -> i32 {
    (shape_value(shape, s, tick as f64).round() as i32).clamp(lo, hi)
}
