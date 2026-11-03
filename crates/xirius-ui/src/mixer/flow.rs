//! The strip's flow (MIX-044): the instrument area and the insert area, drawn with the same
//! parts (a heading, rows of 22 with a power dot, the ＋). An instrument's own effects sit 16
//! in under it with a structure line down their side; a serial chain has no line. A splitter is
//! one row with its branch count and a chevron; open, a hook joins it to the first row of each
//! branch (there is no merge dot).

use std::collections::HashSet;

use gpui::Hsla;

use xirius_core::id::{NodeId, PluginId};
use xirius_core::model::{Plugin, Track, TrackKind};
use xirius_core::rack::{Rack, RackNode, Source};
use xirius_design::{FontSize, LinePiece, LineState, Radius, Space, StructureLine, Weight};

use super::{MixerHit, ROW_H};
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
    /// The area's heading.
    Head,
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
    /// The ＋ that adds to the area.
    Add,
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
        out.push(Line::new(Area::Instrument, Kind::Head, 0));
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
        if open {
            out.push(Line::new(Area::Instrument, Kind::Add, 0));
        }
    }
    out.push(Line::new(Area::Insert, Kind::Head, 0));
    chain(&mut out, Area::Insert, &rack.fx, 0, folded);
    if open {
        out.push(Line::new(Area::Insert, Kind::Add, 0));
    }
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

/// The flow laid out: the areas' panels and the rows, in screen space.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub panels: Vec<(Area, Rect)>,
    pub rows: Vec<(Line, Rect)>,
    /// From the top of the first panel to the bottom of the last.
    pub h: f32,
}

impl Flow {
    /// `lines` from the top of `region`, moved up by `scroll` px.
    pub fn lay(lines: &[Line], region: Rect, scroll: f32) -> Flow {
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
            rows.push((
                *l,
                Rect::new(
                    region.x + AREA_SIDE,
                    y,
                    (region.w - AREA_SIDE * 2.0).max(0.0),
                    ROW_H,
                ),
            ));
            y += ROW_H;
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
    pub fn height(lines: &[Line]) -> f32 {
        Self::lay(lines, Rect::new(0.0, 0.0, 0.0, 0.0), 0.0).h
    }

    /// The row under a point.
    pub fn at(&self, x: f32, y: f32) -> Option<&(Line, Rect)> {
        self.rows.iter().find(|(_, r)| r.contains(x, y))
    }

    /// The row of a plug-in.
    pub fn plugin(&self, id: PluginId) -> Option<&(Line, Rect)> {
        self.rows.iter().find(|(l, _)| l.plugin() == Some(id))
    }

    /// The ＋ of an area.
    pub fn add(&self, area: Area) -> Option<Rect> {
        self.rows
            .iter()
            .find(|(l, _)| l.area == area && l.kind == Kind::Add)
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

    /// Whether an area holds nothing yet: its ＋ shows without the pointer on it.
    fn empty(&self, area: Area) -> bool {
        !self
            .rows
            .iter()
            .any(|(l, _)| l.area == area && !matches!(l.kind, Kind::Head | Kind::Add))
    }
}

/// The 16 × 16 button round a row's power dot.
pub fn dot_button(row: Rect, l: &Line) -> Rect {
    Rect::new(
        row.x + l.indent() + DOT_X,
        row.y + (ROW_H - DOT_BUTTON) / 2.0,
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
fn name_part(row: Rect, l: &Line, chip: bool, level: f32, heavy: bool) -> Rect {
    let mut x = row.x + l.indent() + NAME_X;
    if chip {
        x += CHIP + CHIP_GAP;
    }
    let mut end = row.right() - END;
    if level > 0.0 {
        end -= level + CHIP_GAP;
    }
    if heavy {
        end = end.min(remove_part(item(row, l)).x - HEAVY_W);
    }
    Rect::new(x, row.y, (end - x).max(0.0), row.h)
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

/// What a point on a row of `t`'s flow is.
pub fn hit(s: &Studio, t: &Track, flow: &Flow, x: f32, y: f32) -> MixerHit {
    let Some((l, row)) = flow.at(x, y) else {
        return MixerHit::Strip(t.id);
    };
    match l.kind {
        Kind::Head => MixerHit::Strip(t.id),
        Kind::Add => match l.area {
            Area::Insert => MixerHit::AddInsert(t.id),
            Area::Instrument if flow.has_instrument() => MixerHit::AddLayer(t.id),
            Area::Instrument => MixerHit::AddInstrument(t.id),
        },
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
        (Kind::Instrument { id, .. } | Kind::Effect(id), MixerHit::Open(_, p))
        | (Kind::Instrument { id, .. } | Kind::Effect(id), MixerHit::Bypass(_, p)) => id == p,
        (Kind::Split { id, .. }, MixerHit::Fold(_, s)) => id == s,
        (Kind::Add, MixerHit::AddInsert(_)) => l.area == Area::Insert,
        (Kind::Add, MixerHit::AddInstrument(_) | MixerHit::AddLayer(_)) => {
            l.area == Area::Instrument
        }
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
    let near = |area: Area| {
        hover.is_some_and(|h| flow.rows.iter().any(|(l, _)| l.area == area && on(l, h)))
    };
    pen.mask(region, |pen| {
        for (_, r) in &flow.panels {
            pen.round(*r, c.surface_2, Radius::S);
        }
        for (l, row) in &flow.rows {
            let row = *row;
            let hot = hover.is_some_and(|h| on(l, h));
            match l.kind {
                Kind::Head => {
                    let r = Rect::new(row.x + HEAD_X, row.y, row.w - HEAD_X, row.h);
                    let label = match l.area {
                        Area::Instrument => "インストゥルメント",
                        Area::Insert => "インサート",
                    };
                    pen.text(
                        label,
                        FontSize::Micro,
                        Weight::Regular,
                        c.ink,
                        r,
                        Align::Left,
                    );
                }
                Kind::Add => {
                    // The ＋ keeps its room, and shows where the pointer is in its area or the
                    // area is empty.
                    if !(near(l.area) || flow.empty(l.area)) {
                        continue;
                    }
                    if hot {
                        pen.round(row, c.surface_3, Radius::Clip);
                    }
                    pen.icon(
                        "plus",
                        Rect::new(
                            row.x + (row.w - 12.0) / 2.0,
                            row.y + (row.h - 12.0) / 2.0,
                            12.0,
                            12.0,
                        ),
                        c.ink,
                    );
                }
                Kind::Split { branches, open, .. } => {
                    if hot {
                        pen.round(row, c.surface_3, Radius::Clip);
                    }
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
                            c.ink,
                            Rect::new(row.x, row.y, row.right() - END - row.x, row.h),
                            Align::Right,
                        );
                        end -= w + CHIP_GAP;
                    }
                    let x = row.x + l.indent() + NAME_X;
                    let name = pen.fit(
                        "スプリッター",
                        FontSize::Small,
                        Weight::Regular,
                        (end - x).max(0.0),
                    );
                    let w = pen.text(
                        &name,
                        FontSize::Small,
                        Weight::Regular,
                        c.ink_strong,
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
                        c.ink,
                    );
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
                    plugin_row(pen, s, p, l, row, hot, frozen, chip);
                }
            }
        }
        structure(pen, flow);
    });
}

#[allow(clippy::too_many_arguments)]
fn plugin_row(
    pen: &mut Pen,
    s: &Studio,
    p: &Plugin,
    l: &Line,
    row: Rect,
    hot: bool,
    frozen: bool,
    chip: Option<Hsla>,
) {
    let c = pen.c;
    let pinged = s.perf.pinged(p.id);
    if hot {
        pen.round(row, c.surface_3, Radius::Clip);
    }
    if pinged {
        pen.round(row, c.fill.opacity(0.35), Radius::Clip);
    }
    let b = dot_button(row, l);
    let glyph = Rect::new(b.x + 2.0, b.y + 2.0, 12.0, 12.0);
    if frozen {
    } else if s.health.crashed(p.id) {
        pen.icon("alert", glyph, c.alert);
    } else if p.deactivated {
        pen.icon("power", glyph, c.ink.opacity(LOCKED));
    } else if p.bypass {
        pen.outline(dot(row, l), c.line[0], Radius::Pill, RING);
    } else {
        pen.round(dot(row, l), c.solo_on.0, Radius::Pill);
    }
    if let Some(color) = chip {
        let x = row.x + l.indent() + NAME_X;
        let chip = Rect::new(x, row.y + (ROW_H - CHIP) / 2.0, CHIP, CHIP);
        pen.round(chip, color, Radius::Clip);
    }
    let it = item(row, l);
    let remove = remove_part(it);
    let level = l.level.map_or(0.0, |level| {
        pen.digits(
            &level_text(level),
            FontSize::Micro,
            Weight::Regular,
            c.ink,
            level_part(row),
            Align::Right,
        )
    });
    // The gauge takes the place before the close; the name is cut short of it.
    let heavy = s.perf.heavy.is_heavy(p.id) && !frozen && !p.deactivated;
    let name = name_part(row, l, chip.is_some(), level, heavy);
    // The blue of an open plug-in falls under 4.5:1 on the ping's tint (light theme): while
    // the row is pinged its name is strong (UI-309).
    let off = p.bypass || p.deactivated;
    let ink = if off || frozen {
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
        pen.icon(
            "gauge",
            Rect::new(remove.x - GAUGE, remove.y + 5.0, GAUGE, GAUGE),
            c.alert,
        );
    }
    if hot {
        // The marks sit on the tail of a name that runs under them.
        if name.x + wide > remove.x - 16.0 {
            pen.round(
                Rect::new(remove.x - 16.0, row.y, row.right() - remove.x + 16.0, row.h),
                c.surface_3,
                Radius::Clip,
            );
        }
        if preset_mark_fits(it, &p.name) {
            let preset = crate::fx_preset::part(it);
            pen.icon(
                "file",
                Rect::new(preset.x + 3.0, preset.y + 6.0, 10.0, 10.0),
                c.ink,
            );
        }
        pen.icon(
            "close",
            Rect::new(remove.x + 3.0, remove.y + 6.0, 10.0, 10.0),
            c.ink,
        );
    }
    if frozen {
        pen.icon(
            "lock",
            Rect::new(remove.x + 3.0, remove.y + 6.0, 10.0, 10.0),
            c.ink.opacity(LOCKED),
        );
    }
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
