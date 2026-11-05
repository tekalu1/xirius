//! The strip's flow (MIX-044): the instrument area and the insert area, drawn with the same
//! parts (a heading, rows of 22 with a power dot, the ＋). An instrument's own effects sit 16
//! in under it with a structure line down their side; a serial chain has no line. A splitter is
//! one row with its branch count and a chevron; open, a hook joins it to the first row of each
//! branch (there is no merge dot).
//!
//! A row is picked with a press (MIX-045): it fills with the primary colour and, when it heads a
//! branch or is a layer, a row under it moves that level. The pointer on a row shows its M and,
//! at its bottom edge, the ＋ that puts a plug-in after it. Rows are 32 high for touch.

use std::collections::HashSet;

use gpui::Hsla;

use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::model::{Plugin, Track, TrackKind};
use xirius_core::rack::{Rack, RackNode, Source};
use xirius_design::{FontSize, LinePiece, LineState, Radius, Space, StructureLine, Weight};

use super::{MixerHit, ROW_H, meters};
use crate::draw::{Align, Pen};
use crate::fx_chain::{preset_mark_fits, remove_part};
use crate::studio::{Hit, Rect, Studio};

/// Inside an area: above the heading, at its sides and under its last row.
const AREA_TOP: f32 = 2.0;
const AREA_SIDE: f32 = 3.0;
const AREA_BOTTOM: f32 = 3.0;
/// Between the instrument area and the insert area.
const AREA_GAP: f32 = 6.0;
/// How far a row steps in for each level it is nested.
const INDENT: f32 = Space::X4.px();
/// Where a row's power dot starts, after the indent.
const DOT_X: f32 = 5.0;
const DOT_BUTTON: f32 = 16.0;
const DOT: f32 = 8.0;
/// Where the name starts, after the indent (past the dot's button and its gap).
const NAME_X: f32 = 24.0;
const CHIP: f32 = 10.0;
/// Room after a layer's colour chip.
const CHIP_GAP: f32 = 3.0;
/// The space a row keeps at its right end.
const END: f32 = 4.0;
const HEAD_X: f32 = 5.0;
/// The ring of a plug-in that is switched off.
const RING: f32 = 1.5;
/// The room of the heavy plugin's gauge before the close, and its size.
const HEAVY_W: f32 = 14.0;
const GAUGE: f32 = 12.0;
/// How strongly a locked row's lock shows.
const LOCKED: f32 = 0.5;
/// A row for touch (MIX-045).
pub const COARSE_H: f32 = 32.0;
/// A row's M: on the row under the pointer, and on the level row.
const MM_W: f32 = 18.0;
const MM_H: f32 = 16.0;
const MM_COARSE: f32 = 24.0;
/// The ＋ on a row's bottom edge, half over the next row.
const AFTER: f32 = 16.0;
/// A receiving plug-in's sidechain mark after its name (MIX-048).
const KEY_MARK: f32 = 12.0;
/// The gain reduction meter at a receiving row's right edge, as the mock's `.grm`: so wide, so
/// far in from the edge and from the top and bottom.
const GR_W: f32 = 3.0;
const GR_IN: f32 = 3.0;
const GR_TB: f32 = 4.0;
/// The reduction the meter's full height stands for, in dB (as the device's meter, UI-327).
const GR_SPAN: f32 = 24.0;
const AFTER_COARSE: f32 = 24.0;
/// The level row under a picked row: its margins, its inner room and its value's width.
const SUB_TOP: f32 = 1.0;
const SUB_BOTTOM: f32 = 6.0;
const SUB_PAD: f32 = 4.0;
const SUB_VAL: f32 = 26.0;
/// The level bar: its rail and its cap.
const RAIL: f32 = 3.0;
const CAP_W: f32 = 4.0;
const CAP_H: f32 = 10.0;
const CAP_COARSE: f32 = 12.0;

/// The two areas of a strip's flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    /// What makes the sound: the instrument, or a multi-instrument's layers and their own
    /// effects (instrument tracks only).
    Instrument,
    /// The track's chain of effects.
    Insert,
}

/// What one line of the flow is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// The area's heading, with the ＋ that adds to the area at its right end (`add`; not on a
    /// frozen track).
    Head {
        add: bool,
    },
    /// An instrument; `layer` is its place among the layers of a multi-instrument that has
    /// more than one.
    Instrument {
        id: PluginId,
        layer: Option<usize>,
    },
    Effect(PluginId),
    Split {
        id: NodeId,
        branches: usize,
        open: bool,
    },
    /// Under the picked row: the level of the branch it heads or of its layer (MIX-045).
    Level,
}

/// A row picked on a strip (MIX-045).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    Plugin(PluginId),
    Split(NodeId),
}

/// The level a row's M and level row move: the branch it heads, or its layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub id: NodeId,
    pub layer: bool,
    pub db: f64,
    pub mute: bool,
}

/// How the strips show their flows: the splitters shut, the row picked and whether rows are
/// for touch.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub folded: &'a HashSet<NodeId>,
    pub chosen: Option<(TrackId, Pick)>,
    pub coarse: bool,
}

impl Look<'_> {
    /// `t`'s lines, with the level row under its picked row.
    pub fn lines(&self, t: &Track) -> Vec<Line> {
        let mut out = lines(t, self.folded);
        if let Some((_, pick)) = self.chosen.filter(|(id, _)| *id == t.id)
            && let Some(i) = out.iter().position(|l| l.pick() == Some(pick))
            && out[i].target.is_some()
        {
            let mut level = out[i];
            level.kind = Kind::Level;
            level.heads = None;
            out.insert(i + 1, level);
        }
        out
    }

    pub fn lay(&self, t: &Track, region: Rect, scroll: f32) -> Flow {
        Flow::lay(&self.lines(t), region, scroll, self.coarse)
    }

    /// How tall `t`'s flow stands.
    pub fn height(&self, t: &Track) -> f32 {
        Flow::height(&self.lines(t), self.coarse)
    }
}

/// One line of the flow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub area: Area,
    pub kind: Kind,
    /// How deep the row is nested (0 at the top of its area).
    pub depth: u8,
    /// The splitter this row is the first of a branch of.
    pub heads: Option<NodeId>,
    /// The level of a branch this row heads, or of a layer, when it is not the default: dB and
    /// whether it is muted.
    pub level: Option<(f64, bool)>,
    /// The instrument whose own effects this row is among.
    pub under: Option<PluginId>,
    /// The branch this row heads or its layer, whose level its M and level row move.
    pub target: Option<Target>,
}

impl Line {
    fn new(area: Area, kind: Kind, depth: u8) -> Line {
        Line {
            area,
            kind,
            depth,
            heads: None,
            level: None,
            under: None,
            target: None,
        }
    }

    /// What picking this row picks.
    pub fn pick(&self) -> Option<Pick> {
        match self.kind {
            Kind::Instrument { id, .. } | Kind::Effect(id) => Some(Pick::Plugin(id)),
            Kind::Split { id, .. } => Some(Pick::Split(id)),
            _ => None,
        }
    }

    /// The plug-in of an instrument or effect row.
    pub fn plugin(&self) -> Option<PluginId> {
        match self.kind {
            Kind::Instrument { id, .. } | Kind::Effect(id) => Some(id),
            _ => None,
        }
    }

    fn indent(&self) -> f32 {
        f32::from(self.depth) * INDENT
    }
}

/// A level worth showing: not the default 0 dB, or muted.
fn shown(gain_db: f64, mute: bool) -> Option<(f64, bool)> {
    (mute || gain_db.abs() >= 0.05).then_some((gain_db, mute))
}

/// The lines of `t`'s flow, top to bottom; `folded` holds the splitters that are closed.
pub fn lines(t: &Track, folded: &HashSet<NodeId>) -> Vec<Line> {
    let rack = t.rack.clone().unwrap_or_else(|| Rack::serial(t));
    let open = t.frozen.is_none();
    let mut out = Vec::new();
    if t.kind == TrackKind::Instrument {
        out.push(Line::new(Area::Instrument, Kind::Head { add: open }, 0));
        match &rack.source {
            None => {}
            Some(Source::Single(id)) => out.push(Line::new(
                Area::Instrument,
                Kind::Instrument {
                    id: *id,
                    layer: None,
                },
                0,
            )),
            Some(Source::Multi { layers, .. }) => {
                let multi = layers.len() > 1;
                for (i, l) in layers.iter().enumerate() {
                    let mut row = Line::new(
                        Area::Instrument,
                        Kind::Instrument {
                            id: l.instrument,
                            layer: multi.then_some(i),
                        },
                        0,
                    );
                    if multi {
                        row.level = shown(l.gain_db, l.mute);
                        row.target = Some(Target {
                            id: l.id,
                            layer: true,
                            db: l.gain_db,
                            mute: l.mute,
                        });
                    }
                    out.push(row);
                    let from = out.len();
                    chain(&mut out, Area::Instrument, &l.fx, 1, folded);
                    for r in &mut out[from..] {
                        r.under = Some(l.instrument);
                    }
                }
            }
        }
    }
    out.push(Line::new(Area::Insert, Kind::Head { add: open }, 0));
    chain(&mut out, Area::Insert, &rack.fx, 0, folded);
    out
}

fn chain(out: &mut Vec<Line>, area: Area, nodes: &[RackNode], depth: u8, folded: &HashSet<NodeId>) {
    for n in nodes {
        match n {
            RackNode::Plugin(id) => out.push(Line::new(area, Kind::Effect(*id), depth)),
            RackNode::Split(s) => {
                let open = !folded.contains(&s.id);
                out.push(Line::new(
                    area,
                    Kind::Split {
                        id: s.id,
                        branches: s.branches.len(),
                        open,
                    },
                    depth,
                ));
                if !open {
                    continue;
                }
                for b in &s.branches {
                    let from = out.len();
                    chain(out, area, &b.fx, depth + 1, folded);
                    if let Some(first) = out.get_mut(from) {
                        first.heads = Some(s.id);
                        first.level = shown(b.gain_db, b.mute);
                        first.target = Some(Target {
                            id: b.id,
                            layer: false,
                            db: b.gain_db,
                            mute: b.mute,
                        });
                    }
                }
            }
        }
    }
}

/// Whether `t`'s rack holds the splitter `id` (in its chain or a layer's).
pub fn has_split(t: &Track, id: NodeId) -> bool {
    fn within(nodes: &[RackNode], id: NodeId) -> bool {
        nodes.iter().any(|n| match n {
            RackNode::Plugin(_) => false,
            RackNode::Split(s) => s.id == id || s.branches.iter().any(|b| within(&b.fx, id)),
        })
    }
    let Some(rack) = &t.rack else {
        return false;
    };
    within(&rack.fx, id)
        || matches!(&rack.source, Some(Source::Multi { layers, .. })
            if layers.iter().any(|l| within(&l.fx, id)))
}

/// Where a picked node sits: the branch or layer that holds it (none: the track's own chain),
/// its place there and how many nodes are there.
pub fn spot(t: &Track, pick: Pick) -> Option<(Option<NodeId>, usize, usize)> {
    fn is(n: &RackNode, pick: Pick) -> bool {
        match (n, pick) {
            (RackNode::Plugin(x), Pick::Plugin(y)) => *x == y,
            (RackNode::Split(s), Pick::Split(y)) => s.id == y,
            _ => false,
        }
    }
    fn within(
        nodes: &[RackNode],
        at: Option<NodeId>,
        pick: Pick,
    ) -> Option<(Option<NodeId>, usize, usize)> {
        if let Some(i) = nodes.iter().position(|n| is(n, pick)) {
            return Some((at, i, nodes.len()));
        }
        nodes.iter().find_map(|n| match n {
            RackNode::Plugin(_) => None,
            RackNode::Split(s) => s
                .branches
                .iter()
                .find_map(|b| within(&b.fx, Some(b.id), pick)),
        })
    }
    let Some(rack) = &t.rack else {
        let Pick::Plugin(id) = pick else {
            return None;
        };
        let fx: Vec<_> = t
            .inserts
            .iter()
            .filter(|p| p.kind != xirius_core::model::PluginKind::Instrument)
            .collect();
        let i = fx.iter().position(|p| p.id == id)?;
        return Some((None, i, fx.len()));
    };
    if let Some(Source::Multi { layers, .. }) = &rack.source {
        for l in layers {
            if let Some(x) = within(&l.fx, Some(l.id), pick) {
                return Some(x);
            }
        }
    }
    within(&rack.fx, None, pick)
}

/// The flow laid out: the areas' panels and the rows, in screen space.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub panels: Vec<(Area, Rect)>,
    pub rows: Vec<(Line, Rect)>,
    /// From the top of the first panel to the bottom of the last.
    pub h: f32,
}

impl Flow {
    /// `lines` from the top of `region`, moved up by `scroll` px; rows are 32 high when
    /// `coarse` (touch).
    pub fn lay(lines: &[Line], region: Rect, scroll: f32, coarse: bool) -> Flow {
        let row_h = if coarse { COARSE_H } else { ROW_H };
        let top = region.y - scroll;
        let mut y = top;
        let mut panels = Vec::new();
        let mut rows = Vec::new();
        let mut area: Option<(Area, f32)> = None;
        let close = |area: (Area, f32), y: f32, panels: &mut Vec<(Area, Rect)>| {
            let bottom = y + AREA_BOTTOM;
            panels.push((
                area.0,
                Rect::new(region.x, area.1, region.w, bottom - area.1),
            ));
            bottom
        };
        for l in lines {
            if area.map(|a| a.0) != Some(l.area) {
                if let Some(a) = area.take() {
                    y = close(a, y, &mut panels) + AREA_GAP;
                }
                area = Some((l.area, y));
                y += AREA_TOP;
            }
            if l.kind == Kind::Level {
                // in from the picked row's indent, with room above and below
                let x = region.x + AREA_SIDE + l.indent() + SUB_PAD;
                let w = (region.right() - AREA_SIDE - x).max(0.0);
                rows.push((*l, Rect::new(x, y + SUB_TOP, w, row_h)));
                y += SUB_TOP + row_h + SUB_BOTTOM;
                continue;
            }
            rows.push((
                *l,
                Rect::new(
                    region.x + AREA_SIDE,
                    y,
                    (region.w - AREA_SIDE * 2.0).max(0.0),
                    row_h,
                ),
            ));
            y += row_h;
        }
        if let Some(a) = area {
            y = close(a, y, &mut panels);
        }
        Flow {
            panels,
            rows,
            h: y - top,
        }
    }

    /// How tall `lines` stand.
    pub fn height(lines: &[Line], coarse: bool) -> f32 {
        Self::lay(lines, Rect::new(0.0, 0.0, 0.0, 0.0), 0.0, coarse).h
    }

    /// The row a pick names.
    pub fn picked(&self, pick: Pick) -> Option<&(Line, Rect)> {
        self.rows.iter().find(|(l, _)| l.pick() == Some(pick))
    }

    /// The level row under the picked row.
    pub fn level(&self) -> Option<&(Line, Rect)> {
        self.rows.iter().find(|(l, _)| l.kind == Kind::Level)
    }

    /// The row under a point.
    pub fn at(&self, x: f32, y: f32) -> Option<&(Line, Rect)> {
        self.rows.iter().find(|(_, r)| r.contains(x, y))
    }

    /// Whether rows are for touch.
    fn coarse(&self) -> bool {
        self.rows
            .iter()
            .any(|(l, r)| l.kind != Kind::Level && r.h > ROW_H)
    }

    /// The ＋ that puts a plug-in after `pick`'s row: on the bottom edge of its level row
    /// when it has one.
    pub fn after(&self, pick: Pick) -> Option<Rect> {
        let (l, row) = self.picked(pick)?;
        let (x, edge) = match self.level() {
            Some((sl, sub)) if l.target.is_some() && sl.target == l.target => (sub.x, sub.bottom()),
            _ => (row.x + l.indent() + 2.0, row.bottom()),
        };
        let d = if self.coarse() { AFTER_COARSE } else { AFTER };
        Some(Rect::new(x, edge - d / 2.0, d, d))
    }

    /// The row of a plug-in.
    pub fn plugin(&self, id: PluginId) -> Option<&(Line, Rect)> {
        self.rows.iter().find(|(l, _)| l.plugin() == Some(id))
    }

    /// The ＋ of an area, at the right end of its heading.
    pub fn add(&self, area: Area) -> Option<Rect> {
        self.rows
            .iter()
            .find(|(l, _)| l.area == area && l.kind == Kind::Head { add: true })
            .map(|(_, r)| add_part(*r))
    }

    /// The heading of an area.
    pub fn head(&self, area: Area) -> Option<Rect> {
        self.rows
            .iter()
            .find(|(l, _)| l.area == area && matches!(l.kind, Kind::Head { .. }))
            .map(|(_, r)| *r)
    }

    /// The row of a splitter.
    pub fn split(&self, id: NodeId) -> Option<&(Line, Rect)> {
        self.rows
            .iter()
            .find(|(l, _)| matches!(l.kind, Kind::Split { id: s, .. } if s == id))
    }

    /// Whether the instrument area holds an instrument.
    pub fn has_instrument(&self) -> bool {
        self.rows
            .iter()
            .any(|(l, _)| matches!(l.kind, Kind::Instrument { .. }))
    }

    /// The end of the insert area: where inserts put after the last go (MIX-045).
    pub fn insert_end(&self) -> Option<f32> {
        self.rows
            .iter()
            .filter(|(l, _)| l.area == Area::Insert && l.kind != Kind::Level)
            .map(|(_, r)| r.bottom())
            .reduce(f32::max)
    }
}

/// The ＋ at the right end of an area's heading.
pub fn add_part(head: Rect) -> Rect {
    Rect::new(
        head.right() - END - DOT_BUTTON,
        head.y + (head.h - DOT_BUTTON) / 2.0,
        DOT_BUTTON,
        DOT_BUTTON,
    )
}

/// The 16 × 16 button round a row's power dot.
pub fn dot_button(row: Rect, l: &Line) -> Rect {
    Rect::new(
        row.x + l.indent() + DOT_X,
        row.y + (row.h - DOT_BUTTON) / 2.0,
        DOT_BUTTON,
        DOT_BUTTON,
    )
}

/// The power dot itself.
fn dot(row: Rect, l: &Line) -> Rect {
    let b = dot_button(row, l);
    Rect::new(
        b.x + (DOT_BUTTON - DOT) / 2.0,
        b.y + (DOT_BUTTON - DOT) / 2.0,
        DOT,
        DOT,
    )
}

/// The part of a row a press switches the plug-in on and off with: up to its name.
pub fn dot_part(row: Rect, l: &Line) -> Rect {
    Rect::new(row.x + l.indent(), row.y, NAME_X, row.h)
}

/// The row from its indent on: where the marks of an insert row (the preset and the close)
/// are found.
/// Where a row's level is written, flush right.
fn level_part(row: Rect) -> Rect {
    Rect::new(row.x, row.y, row.right() - END - row.x, row.h)
}

/// Where a plug-in row's name is written: after its dot (and a layer's chip), short of its
/// level (`level` wide) and, on a heavy plug-in, of the gauge before the close.
fn name_part(row: Rect, l: &Line, chip: bool, level: f32, heavy: bool, mute: bool) -> Rect {
    let mut x = row.x + l.indent() + NAME_X;
    if chip {
        x += CHIP + CHIP_GAP;
    }
    let mut end = row.right() - END;
    if level > 0.0 {
        end -= level + CHIP_GAP;
    }
    if mute {
        end = end.min(mute_part(row, l).x - SUB_PAD);
    }
    if heavy {
        let at = if mute {
            mute_part(row, l).x - SUB_PAD
        } else {
            remove_part(item(row, l)).x
        };
        end = end.min(at - HEAVY_W);
    }
    Rect::new(x, row.y, (end - x).max(0.0), row.h)
}

/// The M of a row under the pointer that has a level: in the close's place at the row's end,
/// so the name keeps its room (the close is not shown on such a row).
pub fn mute_part(row: Rect, l: &Line) -> Rect {
    let (w, h) = if row.h > ROW_H {
        (MM_COARSE, MM_COARSE)
    } else {
        (MM_W, MM_H)
    };
    let end = remove_part(item(row, l)).right();
    Rect::new(end - w, row.y + (row.h - h) / 2.0, w, h)
}

/// The parts of the level row: the bar, the value and the M.
pub fn sub_parts(sub: Rect) -> (Rect, Rect, Rect) {
    let (w, h) = if sub.h > ROW_H {
        (MM_COARSE, MM_COARSE)
    } else {
        (MM_W, MM_H)
    };
    let mute = Rect::new(sub.right() - SUB_PAD - w, sub.y + (sub.h - h) / 2.0, w, h);
    let val = Rect::new(mute.x - SUB_PAD - SUB_VAL, sub.y, SUB_VAL, sub.h);
    let x = sub.x + SUB_PAD;
    let bar = Rect::new(x, sub.y, (val.x - SUB_PAD - x).max(0.0), sub.h);
    (bar, val, mute)
}

/// Where a level sits along the level row's bar, 0..1 (the fader's scale).
pub fn level_t(db: f64) -> f32 {
    meters::db_to_t(db as f32)
}

/// The level at a point of the bar.
pub fn level_db(t: f32) -> f64 {
    f64::from(meters::t_to_db(t))
}

pub fn item(row: Rect, l: &Line) -> Rect {
    let x = row.x + l.indent() + DOT_X - 2.0;
    Rect::new(x, row.y, (row.right() - x).max(0.0), row.h)
}

/// A splitter row's chevron.
pub fn fold_part(row: Rect, l: &Line) -> Rect {
    let dot = dot_part(row, l);
    Rect::new(dot.right(), row.y, row.right() - dot.right(), row.h)
}

/// Where an effect let go at `y` goes among `t`'s inserts: before or after the effect row
/// under it, before the next effect for any other row of the insert area, and first for the
/// instrument area.
pub fn drop_index(t: &Track, flow: &Flow, y: f32) -> usize {
    let n = t.inserts.len();
    let pos = |id: PluginId| t.inserts.iter().position(|p| p.id == id);
    let Some(i) = flow
        .rows
        .iter()
        .position(|(_, r)| y >= r.y && y < r.bottom())
    else {
        return n;
    };
    let (l, r) = &flow.rows[i];
    if let (Area::Insert, Kind::Effect(id)) = (l.area, l.kind) {
        return pos(id).map_or(n, |k| k + usize::from(y > r.y + r.h / 2.0));
    }
    let from = if l.area == Area::Insert { i + 1 } else { 0 };
    flow.rows[from..]
        .iter()
        .filter(|(l, _)| l.area == Area::Insert)
        .find_map(|(l, _)| l.plugin())
        .and_then(pos)
        .unwrap_or(n)
}

/// Whether `l` is the row picked on `t`.
fn picked(s: &Studio, t: TrackId, l: &Line) -> bool {
    l.kind != Kind::Level && l.pick().is_some() && s.mixer.chosen == l.pick().map(|p| (t, p))
}

/// Whether the pointer is on `l` (on any part of it).
fn hovered(s: &Studio, t: TrackId, l: &Line) -> bool {
    matches!(s.hover, Hit::Mixer(h) if h.track() == t && on(l, h))
}

/// Whether a row shows its ＋ after it: picked, or under the pointer.
fn shows_after(s: &Studio, t: TrackId, l: &Line) -> bool {
    l.kind != Kind::Level && l.pick().is_some() && (picked(s, t, l) || hovered(s, t, l))
}

/// Whether a row shows its M: one with a level, under the pointer, and not picked (the
/// picked row's M is on its level row).
fn shows_mute(s: &Studio, t: TrackId, l: &Line) -> bool {
    l.kind != Kind::Level && l.target.is_some() && !picked(s, t, l) && hovered(s, t, l)
}

/// The name a row goes by: its plug-in's, or 「スプリッター」.
pub fn row_name(t: &Track, l: &Line) -> String {
    match l.kind {
        Kind::Split { .. } => "スプリッター".into(),
        _ => l
            .plugin()
            .and_then(|id| t.inserts.iter().find(|p| p.id == id))
            .map(|p| p.name.clone())
            .unwrap_or_default(),
    }
}

/// What a point on a row of `t`'s flow is.
pub fn hit(s: &Studio, t: &Track, flow: &Flow, x: f32, y: f32) -> MixerHit {
    // the ＋ after a row lies over the top of the next
    if t.frozen.is_none() {
        for (l, _) in &flow.rows {
            if let Some(pick) = l.pick()
                && shows_after(s, t.id, l)
                && flow.after(pick).is_some_and(|r| r.contains(x, y))
            {
                return MixerHit::After(t.id, pick);
            }
        }
    }
    let Some((l, row)) = flow.at(x, y) else {
        return MixerHit::Strip(t.id);
    };
    if let Some(target) = l.target
        && t.frozen.is_none()
    {
        if l.kind == Kind::Level {
            let (_, _, mute) = sub_parts(*row);
            return if mute.contains(x, y) {
                MixerHit::RowMute(t.id, target.id)
            } else {
                MixerHit::Level(t.id, target.id)
            };
        }
        if shows_mute(s, t.id, l) && mute_part(*row, l).contains(x, y) {
            return MixerHit::RowMute(t.id, target.id);
        }
    }
    match l.kind {
        Kind::Head { add: true } if add_part(*row).contains(x, y) => match l.area {
            Area::Insert => MixerHit::AddInsert(t.id),
            Area::Instrument if flow.has_instrument() => MixerHit::AddLayer(t.id),
            Area::Instrument => MixerHit::AddInstrument(t.id),
        },
        Kind::Head { .. } => MixerHit::Heading(t.id, l.area),
        Kind::Level => MixerHit::Strip(t.id),
        Kind::Split { id, .. } if fold_part(*row, l).contains(x, y) => MixerHit::Fold(t.id, id),
        Kind::Split { .. } => MixerHit::Strip(t.id),
        Kind::Instrument { id, .. } | Kind::Effect(id) => {
            if !t.inserts.iter().any(|p| p.id == id) {
                MixerHit::Strip(t.id)
            } else if !s.health.crashed(id) && dot_part(*row, l).contains(x, y) {
                MixerHit::Bypass(t.id, id)
            } else {
                MixerHit::Open(t.id, id)
            }
        }
    }
}

/// Whether the pointer's part `h` is on line `l`.
fn on(l: &Line, h: MixerHit) -> bool {
    match (l.kind, h) {
        (_, MixerHit::RowMute(_, n) | MixerHit::Level(_, n)) => l.target.is_some_and(|x| x.id == n),
        (_, MixerHit::After(_, p)) => l.pick() == Some(p),
        (Kind::Instrument { id, .. } | Kind::Effect(id), MixerHit::Open(_, p))
        | (Kind::Instrument { id, .. } | Kind::Effect(id), MixerHit::Bypass(_, p)) => id == p,
        (Kind::Split { id, .. }, MixerHit::Fold(_, s)) => id == s,
        (Kind::Head { .. }, MixerHit::AddInsert(_)) => l.area == Area::Insert,
        (Kind::Head { .. }, MixerHit::AddInstrument(_) | MixerHit::AddLayer(_)) => {
            l.area == Area::Instrument
        }
        (Kind::Head { .. }, MixerHit::Heading(_, a)) => l.area == a,
        _ => false,
    }
}

/// A level as a branch head or a layer shows it: its dB, or M when it is muted.
pub fn level_text((db, mute): (f64, bool)) -> String {
    if mute {
        "M".into()
    } else if db > 0.0 {
        format!("+{db:.1}")
    } else {
        format!("{db:.1}")
    }
}

/// Paints `t`'s flow, cut to `region`.
pub fn paint(pen: &mut Pen, s: &Studio, t: &Track, flow: &Flow, region: Rect) {
    let c = pen.c;
    // A frozen track's instrument and inserts are locked (EXP-004): dim, with the lock where
    // the close was, and nothing lights under the pointer.
    let frozen = t.frozen.is_some();
    let hover = match s.hover {
        Hit::Mixer(h) if !frozen && h.track() == t.id => Some(h),
        _ => None,
    };
    pen.mask(region, |pen| {
        for (_, r) in &flow.panels {
            pen.round(*r, c.surface_2, Radius::S);
        }
        for (l, row) in &flow.rows {
            let row = *row;
            let hot = hover.is_some_and(|h| on(l, h));
            let chosen = picked(s, t.id, l);
            match l.kind {
                Kind::Level => {
                    if let Some(target) = l.target {
                        level_row(pen, s, t.id, target, row);
                    }
                }
                Kind::Head { add } => {
                    // The ＋ is always there at the right end (MIX-044); a narrow strip cuts the
                    // heading, not the ＋.
                    let end = if add {
                        add_part(row).x - CHIP_GAP
                    } else {
                        row.right() - END
                    };
                    let r = Rect::new(
                        row.x + HEAD_X,
                        row.y,
                        (end - row.x - HEAD_X).max(0.0),
                        row.h,
                    );
                    let label = match l.area {
                        Area::Instrument => "インストゥルメント",
                        Area::Insert => "インサート",
                    };
                    let label = pen.fit(label, FontSize::Micro, Weight::Regular, r.w);
                    pen.text(
                        &label,
                        FontSize::Micro,
                        Weight::Regular,
                        c.ink,
                        r,
                        Align::Left,
                    );
                    if add {
                        let b = add_part(row);
                        let on_plus = matches!(
                            hover,
                            Some(
                                MixerHit::AddInsert(_)
                                    | MixerHit::AddInstrument(_)
                                    | MixerHit::AddLayer(_)
                            )
                        );
                        if hot && on_plus {
                            pen.round(b, c.surface_3, Radius::Xs);
                        }
                        pen.icon("plus", Rect::new(b.x + 2.0, b.y + 2.0, 12.0, 12.0), c.ink);
                    }
                }
                Kind::Split { branches, open, .. } => {
                    let ink = if chosen {
                        pen.round(row, c.primary.0, Radius::Clip);
                        c.primary.1
                    } else {
                        if hot {
                            pen.round(row, c.surface_3, Radius::Clip);
                        }
                        c.ink_strong
                    };
                    let soft = if chosen { c.primary.1 } else { c.ink };
                    pen.round(dot(row, l), c.solo_on.0, Radius::Pill);
                    // the chevron follows the name, as a folder's does; a narrow strip cuts
                    // the name, not the chevron or the count
                    let mut end = row.right() - END - DOT_BUTTON;
                    if !open {
                        let n = branches.to_string();
                        let w = pen.digits(
                            &n,
                            FontSize::Micro,
                            Weight::Regular,
                            soft,
                            Rect::new(row.x, row.y, row.right() - END - row.x, row.h),
                            Align::Right,
                        );
                        end -= w + CHIP_GAP;
                    }
                    if shows_mute(s, t.id, l) {
                        end = end.min(mute_part(row, l).x - SUB_PAD);
                    }
                    let x = row.x + l.indent() + NAME_X;
                    // the device's own name, as its window's title says it: short enough for a
                    // strip of 116 (the screen reader still says 「スプリッター」)
                    let name = pen.fit(
                        "Splitter",
                        FontSize::Small,
                        Weight::Regular,
                        (end - x).max(0.0),
                    );
                    let w = pen.text(
                        &name,
                        FontSize::Small,
                        Weight::Regular,
                        ink,
                        Rect::new(x, row.y, (end - x).max(0.0), row.h),
                        Align::Left,
                    );
                    pen.icon(
                        if open {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        Rect::new(x + w + 2.0, row.y + (row.h - 12.0) / 2.0, 12.0, 12.0),
                        soft,
                    );
                    if let Some(target) = l.target.filter(|_| shows_mute(s, t.id, l)) {
                        row_mute(pen, row, l, target.mute);
                    }
                }
                Kind::Instrument { id, .. } | Kind::Effect(id) => {
                    let Some(p) = t.inserts.iter().find(|p| p.id == id) else {
                        continue;
                    };
                    let layer = match l.kind {
                        Kind::Instrument { layer, .. } => layer,
                        _ => None,
                    };
                    let chip = layer.map(|i| c.tracks[i % c.tracks.len()]);
                    let mute = !frozen && shows_mute(s, t.id, l);
                    plugin_row(pen, s, p, l, row, (hot, chosen, mute), frozen, chip);
                }
            }
        }
        structure(pen, flow);
        if !frozen {
            for (l, _) in &flow.rows {
                if let Some(pick) = l.pick()
                    && shows_after(s, t.id, l)
                    && let Some(r) = flow.after(pick)
                {
                    let lit = picked(s, t.id, l);
                    let (face, ink) = if lit {
                        c.primary
                    } else {
                        (c.surface_3, c.ink_strong)
                    };
                    pen.floating(r, face, Radius::Pill);
                    let i = (r.w * 0.625).round();
                    pen.icon(
                        "plus",
                        Rect::new(r.x + (r.w - i) / 2.0, r.y + (r.h - i) / 2.0, i, i),
                        ink,
                    );
                }
            }
        }
    });
}

/// The M of a row under the pointer, on a strip of the hover colour that covers the name's
/// tail.
fn row_mute(pen: &mut Pen, row: Rect, l: &Line, on: bool) {
    let c = pen.c;
    let m = mute_part(row, l);
    pen.round(
        Rect::new(m.x - SUB_PAD, row.y, m.w + SUB_PAD, row.h),
        c.surface_3,
        Radius::Clip,
    );
    mute_button(pen, m, on);
}

fn mute_button(pen: &mut Pen, m: Rect, on: bool) {
    let c = pen.c;
    let (face, ink) = if on { c.mute_on } else { (c.surface_2, c.ink) };
    pen.round(m, face, Radius::Clip);
    pen.text(
        "M",
        FontSize::Micro,
        Weight::Semibold,
        ink,
        m,
        Align::Center,
    );
}

/// The level row under the picked row: the bar, the value and the M. While its bar is held,
/// the value is the one heard.
fn level_row(pen: &mut Pen, s: &Studio, t: TrackId, target: Target, sub: Rect) {
    let c = pen.c;
    pen.round(sub, c.surface_3, Radius::Clip);
    let (bar, val, mute) = sub_parts(sub);
    let db = s
        .mixer
        .level_drag
        .filter(|d| d.track == t && d.node == target.id)
        .map_or(target.db, |d| d.value);
    let coarse = sub.h > ROW_H;
    let mid = bar.y + bar.h / 2.0;
    let rail = Rect::new(bar.x, mid - RAIL / 2.0, bar.w, RAIL);
    pen.round(rail, c.well, Radius::Pill);
    let x = bar.x + bar.w * level_t(db);
    pen.round(
        Rect::new(bar.x, rail.y, x - bar.x, RAIL),
        c.ring,
        Radius::Pill,
    );
    let ch = if coarse { CAP_COARSE } else { CAP_H };
    pen.round(
        Rect::new(x - CAP_W / 2.0, mid - ch / 2.0, CAP_W, ch),
        c.ring,
        Radius::Pill,
    );
    pen.digits(
        &level_text((db, false)),
        FontSize::Micro,
        Weight::Regular,
        c.ink_strong,
        val,
        Align::Right,
    );
    mute_button(pen, mute, target.mute);
}

/// The sidechain mark of a keyed plug-in's row, ending at `end` (MIX-048).
fn key_mark_part(row: Rect, end: f32) -> Rect {
    Rect::new(
        end - KEY_MARK,
        row.y + (row.h - KEY_MARK) / 2.0,
        KEY_MARK,
        KEY_MARK,
    )
}

/// One plug-in row; `(hot, chosen, mute)` is whether the pointer is on it, whether it is
/// picked and whether it shows its M.
#[allow(clippy::too_many_arguments)]
fn plugin_row(
    pen: &mut Pen,
    s: &Studio,
    p: &Plugin,
    l: &Line,
    row: Rect,
    (hot, chosen, mute): (bool, bool, bool),
    frozen: bool,
    chip: Option<Hsla>,
) {
    let c = pen.c;
    let pinged = s.perf.pinged(p.id);
    // the face under the marks: the picked row's fill, or the hover colour
    let face = if chosen { c.primary.0 } else { c.surface_3 };
    if chosen {
        pen.round(row, c.primary.0, Radius::Clip);
    } else if hot {
        pen.round(row, c.surface_3, Radius::Clip);
    }
    if pinged {
        pen.round(row, c.fill.opacity(0.35), Radius::Clip);
    }
    let b = dot_button(row, l);
    let glyph = Rect::new(b.x + 2.0, b.y + 2.0, 12.0, 12.0);
    // on the picked row's fill the marks wear its ink
    let alert = if chosen { c.primary.1 } else { c.alert };
    let locked = if chosen {
        c.primary.1
    } else {
        c.ink.opacity(LOCKED)
    };
    if frozen {
    } else if s.health.crashed(p.id) {
        pen.icon("alert", glyph, alert);
    } else if p.deactivated {
        pen.icon("power", glyph, locked);
    } else if p.bypass {
        pen.outline(dot(row, l), c.line[0], Radius::Pill, RING);
    } else {
        pen.round(dot(row, l), c.solo_on.0, Radius::Pill);
    }
    if let Some(color) = chip {
        let x = row.x + l.indent() + NAME_X;
        let chip = Rect::new(x, row.y + (row.h - CHIP) / 2.0, CHIP, CHIP);
        pen.round(chip, color, Radius::Clip);
    }
    let it = item(row, l);
    let remove = remove_part(it);
    let soft = if chosen { c.primary.1 } else { c.ink };
    // A plug-in keyed from another strip has the sidechain mark after its name and, while it
    // listens to the key, the meter of its gain reduction at the right edge (MIX-048). The
    // marks of a row under the pointer take their place.
    let keyed = p
        .sidechain
        .as_ref()
        .filter(|sc| !sc.is_empty() && !(hot && !mute))
        .map(|sc| sc.on);
    let room = if keyed == Some(true) {
        GR_IN + GR_W
    } else {
        0.0
    };
    let mut lp = level_part(row);
    lp.w -= room;
    // the M takes the level's place while it shows; a picked row's level is in the row under it
    let level = l.level.filter(|_| !mute && !chosen).map_or(0.0, |level| {
        pen.digits(
            &level_text(level),
            FontSize::Micro,
            Weight::Regular,
            soft,
            lp,
            Align::Right,
        )
    });
    // The gauge takes the place before the close; the name is cut short of it.
    let heavy = s.perf.heavy.is_heavy(p.id) && !frozen && !p.deactivated;
    let mut name = name_part(row, l, chip.is_some(), level, heavy, mute);
    let key_mark = keyed.map(|on| {
        let mut end = lp.right();
        if level > 0.0 {
            end -= level + CHIP_GAP;
        }
        if mute {
            end = end.min(mute_part(row, l).x - SUB_PAD);
        }
        let r = key_mark_part(row, end);
        name.w = name.w.min((r.x - CHIP_GAP - name.x).max(0.0));
        (r, on)
    });
    // The blue of an open plug-in falls under 4.5:1 on the ping's tint (light theme): while
    // the row is pinged its name is strong (UI-309).
    let off = p.bypass || p.deactivated;
    let ink = if chosen {
        c.primary.1
    } else if off || frozen {
        c.ink
    } else if !pinged && (s.mixer.is_open(p.id) || s.instruments.is_open(p.id)) {
        c.ink_blue
    } else {
        c.ink_strong
    };
    let mut wide = 0.0;
    let shown = pen.fit(&p.name, FontSize::Small, Weight::Regular, name.w);
    pen.mask(name, |pen| {
        wide = if p.bypass {
            pen.struck(
                &shown,
                FontSize::Small,
                Weight::Regular,
                ink,
                name,
                Align::Left,
            )
        } else {
            pen.text(
                &shown,
                FontSize::Small,
                Weight::Regular,
                ink,
                name,
                Align::Left,
            )
        };
    });
    if heavy {
        let x = if mute {
            mute_part(row, l).x - SUB_PAD
        } else {
            remove.x
        };
        pen.icon(
            "gauge",
            Rect::new(x - GAUGE, remove.y + 5.0, GAUGE, GAUGE),
            alert,
        );
    }
    if hot && !mute {
        // The marks sit on the tail of a name that runs under them.
        if name.x + wide > remove.x - 16.0 {
            pen.round(
                Rect::new(remove.x - 16.0, row.y, row.right() - remove.x + 16.0, row.h),
                face,
                Radius::Clip,
            );
        }
        let mark = (row.h - 10.0) / 2.0;
        if preset_mark_fits(it, &p.name) {
            let preset = crate::fx_preset::part(it);
            pen.icon(
                "file",
                Rect::new(preset.x + 3.0, preset.y + mark, 10.0, 10.0),
                soft,
            );
        }
        pen.icon(
            "close",
            Rect::new(remove.x + 3.0, remove.y + mark, 10.0, 10.0),
            soft,
        );
    }
    if let Some(target) = l.target.filter(|_| mute) {
        row_mute(pen, row, l, target.mute);
    }
    if frozen {
        pen.icon(
            "lock",
            Rect::new(remove.x + 3.0, remove.y + 6.0, 10.0, 10.0),
            locked,
        );
    }
    if let Some((r, on)) = key_mark {
        let ink = if chosen {
            c.primary.1
        } else if on {
            c.ink_mark
        } else {
            c.ink
        };
        pen.icon("sc", r, ink);
    }
    if keyed == Some(true) {
        let well = gr_part(row);
        pen.round(well, c.well, Radius::Clip);
        let gr = s.mixer.gr.get(&p.id).copied().unwrap_or(0.0);
        let h = (gr / GR_SPAN).clamp(0.0, 1.0) * well.h;
        if h > 0.0 {
            let ink = if chosen { c.primary.1 } else { c.ink_blue };
            pen.mask(well, |pen| {
                pen.round(Rect::new(well.x, well.y, 3.0, h), ink, Radius::Clip);
            });
        }
    }
    if s.mixer_partners().plugins.contains(&p.id) {
        super::paint::partner_ring(pen, row);
    }
}

/// The meter of a receiving row's gain reduction (MIX-048): a thin well at its right edge.
pub fn gr_part(row: Rect) -> Rect {
    Rect::new(
        row.right() - GR_IN - GR_W,
        row.y + GR_TB,
        GR_W,
        row.h - 2.0 * GR_TB,
    )
}

/// The structure lines of a flow: a hook from each open splitter's dot to the dot of each
/// branch's first row, and a line down the side of an instrument's own effects.
pub fn pieces(flow: &Flow) -> Vec<Vec<LinePiece>> {
    let inset = StructureLine::inset(true);
    let mut out = Vec::new();
    for (i, (l, row)) in flow.rows.iter().enumerate() {
        match l.kind {
            Kind::Split { id, open: true, .. } => {
                let d = dot(*row, l);
                let from = (d.x + d.w / 2.0, d.bottom() + inset);
                for (h, hr) in flow.rows[i + 1..]
                    .iter()
                    .filter(|(h, _)| h.heads == Some(id))
                {
                    let hd = dot(*hr, h);
                    out.push(StructureLine::hook(from, (hd.x - inset, hd.y + hd.h / 2.0)));
                }
            }
            Kind::Instrument { id, .. } => {
                let kids: Vec<Rect> = flow.rows[i + 1..]
                    .iter()
                    .take_while(|(k, _)| k.under == Some(id))
                    .map(|(_, r)| *r)
                    .collect();
                if let (Some(a), Some(b)) = (kids.first(), kids.last()) {
                    let x = dot(*row, l).x + DOT / 2.0;
                    out.push(vec![LinePiece::Straight {
                        from: (x, a.y + inset),
                        to: (x, b.bottom() - inset),
                    }]);
                }
            }
            _ => {}
        }
    }
    out
}

fn structure(pen: &mut Pen, flow: &Flow) {
    for p in pieces(flow) {
        pen.structure(&p, LineState::Rest);
    }
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
