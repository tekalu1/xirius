//! The channel editor (MIX-042): a window over the screen that shows one track's whole sound
//! path as a flow, from its input down through its chain, its splitters' branches and its
//! layers to its output. A card picks, opens and moves its device; a gap's ＋ adds one there;
//! a branch's or a layer's head sets its level, its mute and takes it away. The mixer's
//! «route» button opens it, and so does a device window's. A multi-instrument's window has
//! the key range figure under its flow (MIX-043).

pub mod flow;
pub mod keys;
pub mod paint;
pub mod preset;
#[cfg(test)]
mod tests;

use serde_json::json;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::model::{PluginKind, Project};
use xirius_core::rack::{RackNode, Split, SplitMode};
use xirius_design::FontSize;

use crate::context::Item;
use crate::draw::estimated_text_width;
use crate::mixer::flow::Pick;
use crate::popup::PopupKind;
use crate::studio::{Geometry, Rect, Studio};
use crate::title_bar::{self, BAR_H, Bar};
use flow::{CardKind, Chain, ColKind, Column, Flow, Key, Slot};

/// A row of the toolbar.
pub const TOOL_H: f32 = 40.0;
/// The macro row's band, its least room round the knobs, and its widest knob cell (MIX-041).
pub const MACRO_H: f32 = 80.0;
const MACRO_TOP: f32 = 8.0;
pub const MACRO_CELL: f32 = 60.0;
/// The window's least width (the mock's).
pub const MIN_W: f32 = 520.0;
/// How much of a dragged window stays on the screen.
const KEEP_W: f32 = 80.0;
/// Where a window that was not dragged sits, and the room it leaves under it.
const TOP: f32 = 72.0;
const ROOM_Y: f32 = 96.0;
/// The room a window keeps from the screen's sides.
const SIDE: f32 = 8.0;
/// How far a card goes before it is dragged.
const DRAG: f32 = 4.0;
/// The toolbar's buttons.
const TOOL_PAD: f32 = 8.0;
const TOOL_GAP: f32 = 4.0;
pub const TBTN_H: f32 = 28.0;
pub const TBTN_PAD: f32 = 10.0;
pub const TBTN_ICON: f32 = 12.0;
pub const TBTN_GAP: f32 = 6.0;
/// A card's power dot, its ⋯ and the room before its name.
pub const DOT: f32 = 16.0;
pub const MORE_W: f32 = 20.0;
pub const MORE_H: f32 = 22.0;
pub const CHIP: f32 = 10.0;
/// A splitter's mode pill.
pub const PILL_H: f32 = 22.0;
/// A head's parts: the level's bar, the value, the M and the ×.
pub const LEVEL_W: f32 = 44.0;
pub const VALUE_W: f32 = 28.0;
pub const MUTE_W: f32 = 18.0;
pub const MUTE_H: f32 = 16.0;
pub const REMOVE: f32 = 18.0;
/// The room between the head's left half and its right half, and inside each.
const HEAD_MID: f32 = 22.0;
const HEAD_GAP: f32 = 4.0;
/// How far a dragged card's slot may be from the pointer.
const SLOT_REACH: f32 = 28.0;

/// The window's name, as a reader says it.
pub const NAME: &str = "チャンネルエディター";
pub const POPOUT_LABEL: &str = "マクロコントロールを別の窓で開く";

/// The toolbar's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Insert,
    Splitter,
    /// Only on an instrument track: the instrument, or a layer when it has one.
    Instrument,
    Macros,
    Popout,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Insert => "Insert",
            Tool::Splitter => "Splitter",
            Tool::Instrument => "Instrument",
            Tool::Macros => "Macro Controls",
            Tool::Popout => POPOUT_LABEL,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Tool::Macros => "macro",
            Tool::Popout => "popout",
            _ => "plus",
        }
    }
}

/// A part of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    /// The bar: the window moves with the pointer.
    Head,
    Pin,
    Close,
    /// The bar's preset tag (its list) and its ‹ › (MIX-042).
    Preset,
    PrevPreset,
    NextPreset,
    Tool(Tool),
    Card(Pick),
    /// A device's power.
    Dot(Pick),
    /// A card's ⋯: its menu.
    More(Pick),
    /// A splitter's mode pill.
    Mode(NodeId),
    /// The ＋ of the `i`th gap.
    Gap(usize),
    Level(NodeId),
    Mute(NodeId),
    Remove(NodeId),
    /// A frequency split's `i`th crossover.
    Cross(NodeId, usize),
    /// A layer's box in the key range figure, and one of its sides.
    Range(NodeId),
    Side(NodeId, keys::Edge),
    /// The chosen layer's Transpose field.
    Transpose(NodeId),
    /// A knob of the macro row, and the row's button that opens the macro window.
    Macro(u8),
    MacroOut,
    Body,
}

/// What a press in the window took hold of.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Grab {
    Move {
        from: (f32, f32),
        pointer: (f32, f32),
    },
    /// A card: dragged once it went `DRAG` away, to the slot under the pointer.
    Card {
        pick: Pick,
        pointer: (f32, f32),
        moved: bool,
        to: Option<Slot>,
    },
    /// A crossover: the split's frequencies when it was taken and now.
    Cross {
        split: NodeId,
        i: usize,
        x0: f32,
        from: Vec<f64>,
        now: Vec<f64>,
    },
    /// A side of a layer's box: the layer's ranges when it was taken and now.
    Side {
        layer: NodeId,
        edge: keys::Edge,
        from: keys::Ranges,
        now: keys::Ranges,
    },
    /// A layer's Transpose field: where the press was, its transpose then and now.
    Transpose {
        layer: NodeId,
        y0: f32,
        from: i8,
        now: i8,
    },
}

/// The open channel editor.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelWin {
    pub track: TrackId,
    /// Where its corner is, once it was dragged.
    pub at: Option<(f32, f32)>,
    /// The pin is down: another track's editor does not take its place.
    pub pinned: bool,
    /// It is over the device windows (the last pressed of them).
    pub top: bool,
    /// The macro column shows (MIX-041).
    pub macros: bool,
    /// How far the flow is scrolled down and right.
    pub scroll: (f32, f32),
    /// Where the pointer is, over it or not.
    pub pointer: Option<(f32, f32)>,
    pub hot: Option<Part>,
    pub chosen: Option<Pick>,
    pub(crate) grab: Option<Grab>,
}

impl ChannelWin {
    pub fn new(track: TrackId) -> ChannelWin {
        ChannelWin {
            track,
            at: None,
            pinned: false,
            top: true,
            macros: false,
            scroll: (0.0, 0.0),
            pointer: None,
            hot: None,
            chosen: None,
            grab: None,
        }
    }
}

/// The window as it is laid out on the screen.
#[derive(Clone, Debug)]
pub struct View {
    pub r: Rect,
    pub name: String,
    pub bar: Bar,
    pub tools: Vec<(Tool, Rect)>,
    /// The toolbar's band.
    pub toolbar: Rect,
    /// Where the flow shows; it is cut to it.
    pub body: Rect,
    pub flow: Flow,
    /// A multi-instrument's key range figure, under the body, when the window is wide enough.
    pub keys: Option<keys::Keys>,
    /// The macro row, under the toolbar, while Macro Controls is on.
    pub macros: Option<MacroRow>,
}

/// The row of Macro 1–8 under the toolbar (MIX-041).
#[derive(Clone, Debug, PartialEq)]
pub struct MacroRow {
    pub band: Rect,
    pub cells: [Rect; xirius_core::controls::KNOBS],
    /// Opens the macro window.
    pub popout: Rect,
}

/// The macro row in its band: the knobs from the left, as wide as the band lets them up to
/// [`MACRO_CELL`], and the window's button at the top right.
pub fn macro_row(band: Rect) -> MacroRow {
    use xirius_core::controls::KNOBS;
    let popout = Rect::new(
        band.right() - TOOL_PAD - TBTN_H,
        band.y + MACRO_TOP + (crate::macro_pad::DIAL - TBTN_H) / 2.0,
        TBTN_H,
        TBTN_H,
    );
    let room = (popout.x - TOOL_GAP - band.x - TOOL_PAD).max(0.0);
    let cell = (room / KNOBS as f32).min(MACRO_CELL);
    let cells = std::array::from_fn(|i| {
        Rect::new(
            band.x + TOOL_PAD + i as f32 * cell,
            band.y + MACRO_TOP,
            cell,
            crate::macro_pad::KNOB_H,
        )
    });
    MacroRow {
        band,
        cells,
        popout,
    }
}

/// The width of a toolbar's button.
pub fn tool_w(tool: Tool) -> f32 {
    match tool {
        Tool::Popout => TBTN_H,
        _ => {
            TBTN_PAD
                + TBTN_ICON
                + TBTN_GAP
                + estimated_text_width(tool.label(), FontSize::Body)
                + TBTN_PAD
        }
    }
}

/// The toolbar's buttons in the window `r`: the adds at the left, the macros at the right;
/// when they do not fit side by side, the macros go to a row under them. Returns them and
/// the toolbar's height.
pub fn tools_of(r: Rect, instrument: bool) -> (Vec<(Tool, Rect)>, f32) {
    let y = r.y + BAR_H + (TOOL_H - TBTN_H) / 2.0;
    let mut left = vec![Tool::Insert, Tool::Splitter];
    if instrument {
        left.push(Tool::Instrument);
    }
    let mut out = Vec::new();
    let mut x = r.x + TOOL_PAD;
    for t in left {
        let w = tool_w(t);
        out.push((t, Rect::new(x, y, w, TBTN_H)));
        x += w + TOOL_GAP;
    }
    let right = [Tool::Macros, Tool::Popout];
    let right_w = right.iter().map(|t| tool_w(*t)).sum::<f32>() + TOOL_GAP;
    let fits = x + TOOL_PAD + right_w <= r.right() - TOOL_PAD;
    let (mut x, y, h) = if fits {
        (r.right() - TOOL_PAD - right_w, y, TOOL_H)
    } else {
        (
            r.x + TOOL_PAD,
            y + TBTN_H + TOOL_GAP,
            TOOL_H + TBTN_H + TOOL_GAP,
        )
    };
    for t in right {
        let w = tool_w(t);
        out.push((t, Rect::new(x, y, w, TBTN_H)));
        x += w + TOOL_GAP;
    }
    (out, h)
}

/// The window of `w` on a screen of `size`; none when its track is gone.
pub fn view(p: &Project, w: &ChannelWin, size: (f32, f32)) -> Option<View> {
    let t = p.tracks.get(&w.track)?;
    let probe = flow::layout(p, t, (0.0, 0.0));
    let multi = !keys::layers(t).is_empty();
    let least = if multi { keys::LEAST_W } else { MIN_W };
    let width = probe.w.max(least).min((size.0 - 2.0 * SIDE).max(KEEP_W));
    let instrument = t.kind == xirius_core::model::TrackKind::Instrument;
    let (_, tool_h) = tools_of(Rect::new(0.0, 0.0, width, 0.0), instrument);
    let panel_h = if multi && width >= keys::LEAST_W {
        keys::PANEL_H
    } else {
        0.0
    };
    let macro_h = if w.macros { MACRO_H } else { 0.0 };
    let height = (BAR_H + tool_h + macro_h + probe.h + panel_h).min((size.1 - ROOM_Y).max(200.0));
    let (x, y) = w.at.unwrap_or(((size.0 - width) / 2.0, TOP));
    let r = Rect::new(
        x.clamp(0.0, (size.0 - KEEP_W).max(0.0)),
        y.clamp(0.0, (size.1 - BAR_H).max(0.0)),
        width,
        height,
    );
    let name = t.shown_name().to_string();
    let bar = title_bar::layout(
        Rect::new(r.x, r.y, r.w, BAR_H),
        &name,
        "",
        title_bar::Seats::default(),
    );
    let (tools, _) = tools_of(r, instrument);
    let toolbar = Rect::new(r.x, r.y + BAR_H, r.w, tool_h);
    let macros = w
        .macros
        .then(|| macro_row(Rect::new(r.x, toolbar.bottom(), r.w, MACRO_H)));
    let body = Rect::new(
        r.x,
        toolbar.bottom() + macro_h,
        r.w,
        (r.h - BAR_H - tool_h - macro_h - panel_h).max(0.0),
    );
    let keys = (panel_h > 0.0).then(|| {
        let held = match &w.grab {
            Some(Grab::Side { layer, now, .. }) => Some((*layer, *now)),
            _ => None,
        };
        let turning = match &w.grab {
            Some(Grab::Transpose { layer, now, .. }) => Some((*layer, *now)),
            _ => None,
        };
        let panel = Rect::new(r.x, body.bottom(), r.w, panel_h);
        keys::layout(t, panel, keys::chosen_layer(t, w.chosen), held, turning)
    });
    let (sx, sy) = clamp_scroll(w.scroll, &probe, body);
    // A flow narrower than the window sits in its middle; a wider one shows from its left.
    let x0 = if probe.w <= body.w {
        body.x + (body.w - probe.w) / 2.0
    } else {
        body.x - sx
    };
    let flow = flow::layout(p, t, (x0, body.y - sy));
    Some(View {
        r,
        name,
        bar,
        tools,
        toolbar,
        body,
        flow,
        keys,
        macros,
    })
}

fn clamp_scroll(s: (f32, f32), flow: &Flow, body: Rect) -> (f32, f32) {
    (
        s.0.clamp(0.0, (flow.w - body.w).max(0.0)),
        s.1.clamp(0.0, (flow.h - body.h).max(0.0)),
    )
}

/// A card's parts: its power dot, its ⋯ and, for a splitter, its mode pill.
pub struct CardParts {
    pub dot: Option<Rect>,
    pub chip: Option<Rect>,
    /// Where the name goes.
    pub name: Rect,
    pub more: Rect,
    pub mode: Option<Rect>,
}

/// A splitter's mode as its pill and its menu say it.
pub fn mode_text(mode: SplitMode) -> &'static str {
    match mode {
        SplitMode::Normal => "ノーマル",
        SplitMode::Channel => "チャンネル分割",
        SplitMode::Frequency => "周波数分割",
    }
}

pub fn card_parts(card: &flow::Card) -> CardParts {
    let r = card.rect;
    let cy = r.y + r.h / 2.0;
    let more = Rect::new(r.right() - 4.0 - MORE_W, cy - MORE_H / 2.0, MORE_W, MORE_H);
    match card.kind {
        CardKind::Split { mode } => {
            let w = 8.0 + estimated_text_width(mode_text(mode), FontSize::Small) + 2.0 + 12.0 + 4.0;
            let pill = Rect::new(more.x - 6.0 - w, cy - PILL_H / 2.0, w, PILL_H);
            let x = r.x + 12.0;
            CardParts {
                dot: None,
                chip: None,
                name: Rect::new(x, r.y, (pill.x - 6.0 - x).max(0.0), r.h),
                more,
                mode: Some(pill),
            }
        }
        kind => {
            let dot = Rect::new(r.x + 6.0, cy - DOT / 2.0, DOT, DOT);
            let mut x = dot.right() + 6.0;
            let chip = matches!(kind, CardKind::Instrument { layer: Some(_) }).then(|| {
                let chip = Rect::new(x, cy - CHIP / 2.0, CHIP, CHIP);
                x = chip.right() + 6.0;
                chip
            });
            CardParts {
                dot: Some(dot),
                chip,
                name: Rect::new(x, r.y, (more.x - 4.0 - x).max(0.0), r.h),
                more,
                mode: None,
            }
        }
    }
}

/// A column's head's parts, centred as the mock's: «n ━━▌» left of the middle, «value M ×»
/// right of it.
pub struct HeadParts {
    pub label: String,
    pub num: Rect,
    pub level: Rect,
    pub value: Rect,
    pub mute: Rect,
    /// Not on a channel split's branches: they stay two.
    pub remove: Option<Rect>,
    /// What shows at its right while the parts do not: M or a level away from 0.
    pub keep: Rect,
}

/// A column's number as its head says it: 1…, or L and R on a channel split.
pub fn col_label(col: &Column) -> String {
    match col.kind {
        ColKind::Branch {
            mode: SplitMode::Channel,
            ..
        } => if col.n == 1 { "L" } else { "R" }.into(),
        _ => col.n.to_string(),
    }
}

/// The x of a head's middle, between the level and its value: on the line, as the mock's, where
/// a branch's ＋ shows. A column too narrow for M and × right of it moves it left.
pub fn head_mid(h: Rect) -> f32 {
    let right = HEAD_MID / 2.0 + VALUE_W + HEAD_GAP + MUTE_W + HEAD_GAP + REMOVE + 2.0;
    (h.x + h.w / 2.0).min(h.right() - right)
}

pub fn head_parts(col: &Column) -> HeadParts {
    let h = col.head;
    let cy = h.y + h.h / 2.0;
    let label = col_label(col);
    let num_w = estimated_text_width(&label, FontSize::Caption).max(6.0);
    let removable = !matches!(
        col.kind,
        ColKind::Branch {
            mode: SplitMode::Channel,
            ..
        }
    );
    let cx = head_mid(h);
    let level = Rect::new(
        cx - HEAD_MID / 2.0 - LEVEL_W,
        cy - MUTE_H / 2.0,
        LEVEL_W,
        MUTE_H,
    );
    let num = Rect::new(level.x - HEAD_GAP - num_w, h.y, num_w, h.h);
    let value = Rect::new(cx + HEAD_MID / 2.0, h.y, VALUE_W, h.h);
    let mute = Rect::new(value.right() + HEAD_GAP, cy - MUTE_H / 2.0, MUTE_W, MUTE_H);
    let remove =
        removable.then(|| Rect::new(mute.right() + HEAD_GAP, cy - REMOVE / 2.0, REMOVE, REMOVE));
    HeadParts {
        label,
        num,
        level,
        value,
        mute,
        remove,
        keep: Rect::new(h.right() - 4.0 - 40.0, h.y, 40.0, h.h),
    }
}

/// Does the column show its head's parts: the pointer is over it, its level is held, or the
/// chosen card is in it.
pub fn head_shown(w: &ChannelWin, col: &Column, flow: &Flow, held: Option<NodeId>) -> bool {
    if held == Some(col.id) {
        return true;
    }
    if w.grab.is_none()
        && let Some((x, y)) = w.pointer
        && col.rect.contains(x, y)
    {
        return true;
    }
    w.chosen.is_some_and(|pick| {
        flow.cards
            .iter()
            .find(|c| c.pick == pick)
            .is_some_and(|c| c.within.contains(&Key::Node(col.id)))
    })
}

/// The part of the window `v` under a point.
pub fn part_at(v: &View, x: f32, y: f32) -> Option<Part> {
    if !v.r.contains(x, y) {
        return None;
    }
    if y < v.r.y + BAR_H {
        if v.bar.pin.contains(x, y) {
            return Some(Part::Pin);
        }
        if v.bar.close.contains(x, y) {
            return Some(Part::Close);
        }
        if v.bar.prev.is_some_and(|b| b.contains(x, y)) {
            return Some(Part::PrevPreset);
        }
        if v.bar.next.is_some_and(|b| b.contains(x, y)) {
            return Some(Part::NextPreset);
        }
        if v.bar.tag.contains(x, y) {
            return Some(Part::Preset);
        }
        return Some(Part::Head);
    }
    if v.toolbar.contains(x, y) {
        return Some(
            v.tools
                .iter()
                .find(|(_, r)| r.contains(x, y))
                .map_or(Part::Body, |(t, _)| Part::Tool(*t)),
        );
    }
    if let Some(m) = &v.macros
        && m.band.contains(x, y)
    {
        if m.popout.contains(x, y) {
            return Some(Part::MacroOut);
        }
        return Some(
            m.cells
                .iter()
                .position(|c| c.contains(x, y))
                .map_or(Part::Body, |i| Part::Macro(i as u8)),
        );
    }
    if let Some(k) = &v.keys
        && k.panel.contains(x, y)
    {
        if let Some((f, _)) = k.transpose
            && f.contains(x, y)
            && let Some(b) = k.boxes.get(k.chosen)
        {
            return Some(Part::Transpose(b.id));
        }
        return Some(match keys::hit(k, x, y) {
            Some((id, Some(e))) => Part::Side(id, e),
            Some((id, None)) => Part::Range(id),
            None => Part::Body,
        });
    }
    if !v.body.contains(x, y) {
        return Some(Part::Body);
    }
    let f = &v.flow;
    for cross in &f.crosses {
        if cross.rect.contains(x, y) {
            return Some(Part::Cross(cross.split, cross.i));
        }
    }
    for col in &f.columns {
        if !col.head.contains(x, y) {
            continue;
        }
        let hp = head_parts(col);
        if hp.level.contains(x, y) {
            return Some(Part::Level(col.id));
        }
        if hp.mute.contains(x, y) {
            return Some(Part::Mute(col.id));
        }
        if hp.remove.is_some_and(|r| r.contains(x, y)) {
            return Some(Part::Remove(col.id));
        }
    }
    for card in &f.cards {
        if !card.rect.contains(x, y) {
            continue;
        }
        let cp = card_parts(card);
        if cp.more.contains(x, y) {
            return Some(Part::More(card.pick));
        }
        if let (Some(mode), Pick::Split(id)) = (cp.mode, card.pick)
            && mode.contains(x, y)
        {
            return Some(Part::Mode(id));
        }
        if cp.dot.is_some_and(|d| d.contains(x, y)) {
            return Some(Part::Dot(card.pick));
        }
        return Some(Part::Card(card.pick));
    }
    for (i, gap) in f.gaps.iter().enumerate() {
        if gap.button.contains(x, y) {
            return Some(Part::Gap(i));
        }
    }
    Some(Part::Body)
}

/// The id a command takes for a card.
fn pick_id(pick: Pick) -> String {
    match pick {
        Pick::Plugin(id) => id.to_string(),
        Pick::Split(id) => id.to_string(),
    }
}

/// The splitter `id` in a chain, wherever it is.
fn find_split(nodes: &[RackNode], id: NodeId) -> Option<&Split> {
    nodes.iter().find_map(|n| match n {
        RackNode::Plugin(_) => None,
        RackNode::Split(s) if s.id == id => Some(s),
        RackNode::Split(s) => s.branches.iter().find_map(|b| find_split(&b.fx, id)),
    })
}

/// The splitter `id` of track `t`'s rack.
pub fn split_of(p: &Project, t: TrackId, id: NodeId) -> Option<Split> {
    let rack = p.tracks.get(&t)?.rack.as_ref()?;
    if let Some(s) = find_split(&rack.fx, id) {
        return Some(s.clone());
    }
    match &rack.source {
        Some(xirius_core::rack::Source::Multi { layers, .. }) => {
            layers.iter().find_map(|l| find_split(&l.fx, id)).cloned()
        }
        _ => None,
    }
}

/// The place a card dropped at `slot` goes to, from its place `from`: none where it already is.
pub fn drop_index(from: Option<(Chain, usize)>, slot: &Slot) -> Option<usize> {
    match from {
        Some((chain, i)) if chain == slot.chain => {
            if slot.index == i || slot.index == i + 1 {
                None
            } else if slot.index > i {
                Some(slot.index - 1)
            } else {
                Some(slot.index)
            }
        }
        _ => Some(slot.index),
    }
}

/// The slot a card dragged to `(x, y)` would go to.
fn slot_near(flow: &Flow, pick: Pick, x: f32, y: f32) -> Option<Slot> {
    let card = flow.cards.iter().find(|c| c.pick == pick)?;
    let from = card.chain;
    flow.slots
        .iter()
        .filter(|s| x >= s.mark.x - SLOT_REACH && x <= s.mark.right() + SLOT_REACH)
        .filter(|s| (s.mark.y - y).abs() <= SLOT_REACH)
        .filter(|s| drop_index(from, s).is_some())
        // A splitter does not go into its own branches.
        .filter(|s| match (pick, s.chain) {
            (Pick::Split(sid), Chain::Branch(b)) => !flow
                .columns
                .iter()
                .any(|c| c.id == b && (c.within.contains(&Key::Node(sid)))),
            _ => true,
        })
        .min_by(|a, b| {
            let da = (a.mark.y - y).abs() + (a.mark.x + a.mark.w / 2.0 - x).abs() * 0.25;
            let db = (b.mark.y - y).abs() + (b.mark.x + b.mark.w / 2.0 - x).abs() * 0.25;
            da.total_cmp(&db)
        })
        .copied()
}

/// A crossover dragged `dx` from where it was taken: twice as high for every 24 px, between
/// its neighbours.
pub fn crossed(from: &[f64], i: usize, dx: f32) -> Vec<f64> {
    let mut now = from.to_vec();
    let Some(f0) = from.get(i) else {
        return now;
    };
    let lo = if i == 0 { 20.0 } else { from[i - 1] * 1.05 };
    let hi = from.get(i + 1).map_or(20000.0, |f| f / 1.05);
    let f = f0 * 2f64.powf(f64::from(dx) / 24.0);
    now[i] = f.clamp(lo, hi.max(lo)).round();
    now
}

impl Studio {
    /// The open channel editor as it is laid out.
    pub fn channel_view(&self, g: &Geometry) -> Option<View> {
        view(&self.project(), self.channel.as_ref()?, g.size)
    }

    /// Opens the channel editor of `t`: the open one comes to the top; another track's takes
    /// its place unless its pin is down.
    pub fn open_channel(&mut self, t: TrackId) {
        if !self.project().tracks.contains_key(&t) {
            return;
        }
        match &mut self.channel {
            Some(w) if w.track == t || w.pinned => {
                w.top = true;
            }
            Some(w) => {
                let at = w.at;
                *w = ChannelWin {
                    at,
                    ..ChannelWin::new(t)
                };
            }
            None => self.channel = Some(ChannelWin::new(t)),
        }
    }

    /// Is the channel editor under the point, over what it covers?
    pub(crate) fn over_channel(&self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(w) = &self.channel else {
            return false;
        };
        if !w.top && self.over_instrument(g, x, y) {
            return false;
        }
        self.channel_view(g).is_some_and(|v| v.r.contains(x, y))
    }

    /// A press: true when the window took it.
    pub fn channel_press(&mut self, g: &Geometry, x: f32, y: f32, clicks: u32) -> bool {
        if !self.over_channel(g, x, y) {
            return false;
        }
        let Some(v) = self.channel_view(g) else {
            self.channel = None;
            return false;
        };
        // the window's ground takes the press: it does not reach what is under it
        let Some(part) = part_at(&v, x, y) else {
            return true;
        };
        self.error = None;
        let Some(w) = self.channel.as_mut() else {
            return false;
        };
        w.top = true;
        let t = w.track;
        match part {
            Part::Close => self.channel = None,
            Part::Pin => w.pinned = !w.pinned,
            Part::Preset => self.open_inst_menu(crate::instrument::menu::Menu::Chain(t), v.bar.tag),
            Part::PrevPreset | Part::NextPreset => {
                self.step_chain_preset(t, part == Part::NextPreset);
            }
            Part::Head => {
                w.grab = Some(Grab::Move {
                    from: (v.r.x, v.r.y),
                    pointer: (x, y),
                })
            }
            Part::Tool(tool) => {
                let rect = v.tools.iter().find(|(x, _)| *x == tool).map(|(_, r)| *r);
                self.channel_tool(t, tool, rect.unwrap_or(v.r), &v.flow);
            }
            Part::Card(pick) => {
                w.chosen = Some(pick);
                if clicks >= 2 {
                    self.channel_open(t, pick);
                    return true;
                }
                let movable = v
                    .flow
                    .cards
                    .iter()
                    .any(|c| c.pick == pick && c.chain.is_some());
                if movable && let Some(w) = self.channel.as_mut() {
                    w.grab = Some(Grab::Card {
                        pick,
                        pointer: (x, y),
                        moved: false,
                        to: None,
                    });
                }
            }
            Part::Dot(pick) => {
                if let Pick::Plugin(id) = pick {
                    self.channel_power(t, id);
                }
            }
            Part::More(pick) => {
                w.chosen = Some(pick);
                let at = v
                    .flow
                    .cards
                    .iter()
                    .find(|c| c.pick == pick)
                    .map(|c| card_parts(c).more)
                    .unwrap_or(v.r);
                self.channel_menu(t, pick, at.x, at.bottom());
            }
            Part::Mode(id) => {
                w.chosen = Some(Pick::Split(id));
                let at = v
                    .flow
                    .cards
                    .iter()
                    .find(|c| c.pick == Pick::Split(id))
                    .and_then(|c| card_parts(c).mode)
                    .unwrap_or(v.r);
                self.open_popup(PopupKind::SplitMode(t, id), at);
            }
            Part::Gap(i) => {
                let Some(gap) = v.flow.gaps.get(i).copied() else {
                    return true;
                };
                match gap.after {
                    Some(after) => self.insert_after(t, after, gap.button),
                    None => {
                        self.mixer.insert_after = None;
                        self.mixer.insert_into = Some((t, gap.chain, gap.index));
                        self.open_popup(PopupKind::Insert(t), gap.button);
                        if self.popup.is_none() {
                            self.mixer.insert_into = None;
                        }
                    }
                }
            }
            Part::Level(n) => {
                let bar = v
                    .flow
                    .columns
                    .iter()
                    .find(|c| c.id == n)
                    .map(|c| head_parts(c).level);
                if let (Some(bar), Some(target)) = (bar, self.row_target(t, n)) {
                    self.mixer.level_drag = Some(crate::mixer::LevelDrag::taken(t, target, bar, x));
                    self.level_move(x, false);
                }
            }
            Part::Mute(n) => {
                self.row_mute(t, n);
            }
            Part::Remove(n) => {
                let layer = v
                    .flow
                    .columns
                    .iter()
                    .any(|c| c.id == n && matches!(c.kind, ColKind::Layer { .. }));
                let command = if layer {
                    "rack.layer.remove"
                } else {
                    "rack.branch.remove"
                };
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(command, json!({ "id": n.to_string() }));
            }
            Part::Cross(split, i) => {
                let found = split_of(&self.project(), t, split);
                if let (Some(s), Some(w)) = (found, self.channel.as_mut()) {
                    w.grab = Some(Grab::Cross {
                        split,
                        i,
                        x0: x,
                        from: s.crossovers.clone(),
                        now: s.crossovers,
                    });
                }
            }
            Part::Range(id) => {
                let pick = layer_pick(&self.project(), t, id);
                if let Some(w) = self.channel.as_mut() {
                    w.chosen = pick;
                }
            }
            Part::Side(id, edge) => {
                let p = self.project();
                let layer = p
                    .tracks
                    .get(&t)
                    .and_then(|t| keys::layers(t).iter().find(|l| l.id == id).cloned());
                if let (Some(l), Some(w)) = (layer, self.channel.as_mut()) {
                    w.chosen = Some(Pick::Plugin(l.instrument));
                    let from = (l.keys, l.velocity);
                    w.grab = Some(Grab::Side {
                        layer: id,
                        edge,
                        from,
                        now: from,
                    });
                }
            }
            Part::Transpose(id) => {
                let p = self.project();
                let from = p
                    .tracks
                    .get(&t)
                    .and_then(|t| keys::layers(t).iter().find(|l| l.id == id))
                    .map(|l| l.transpose);
                if let (Some(from), Some(w)) = (from, self.channel.as_mut()) {
                    w.grab = Some(Grab::Transpose {
                        layer: id,
                        y0: y,
                        from,
                        now: from,
                    });
                }
            }
            Part::Macro(i) => {
                self.control_drag(t, xirius_core::controls::ControlId::Knob(i), y);
            }
            Part::MacroOut => self.open_macros(t),
            Part::Body => w.chosen = None,
        }
        true
    }

    fn channel_tool(&mut self, t: TrackId, tool: Tool, at: Rect, flow: &Flow) {
        match tool {
            Tool::Insert => {
                self.mixer.insert_after = None;
                self.mixer.insert_into = None;
                self.open_popup(PopupKind::Insert(t), at);
            }
            Tool::Splitter => {
                let chosen = self.channel.as_ref().and_then(|w| w.chosen);
                let place = chosen
                    .filter(|pick| {
                        flow.cards
                            .iter()
                            .any(|c| c.pick == *pick && c.chain.is_some())
                    })
                    .map_or("track".to_string(), |pick| {
                        format!("after:{}", pick_id(pick))
                    });
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "rack.split.add",
                    json!({ "track": t.to_string(), "at": place }),
                );
            }
            Tool::Instrument => {
                let has =
                    self.project().tracks.get(&t).is_some_and(|x| {
                        x.inserts.iter().any(|p| p.kind == PluginKind::Instrument)
                    });
                let kind = if has {
                    PopupKind::Layer(t)
                } else {
                    PopupKind::Instrument(t)
                };
                self.open_popup(kind, at);
            }
            Tool::Macros => {
                if let Some(w) = self.channel.as_mut() {
                    w.macros = !w.macros;
                }
            }
            Tool::Popout => self.open_macros(t),
        }
    }

    /// A card opened: a built-in device's window here, a plug-in's own.
    fn channel_open(&mut self, t: TrackId, pick: Pick) {
        let Pick::Plugin(id) = pick else {
            return;
        };
        let built_in = self
            .project()
            .tracks
            .get(&t)
            .and_then(|x| x.inserts.iter().find(|p| p.id == id))
            .is_some_and(crate::instrument::window::is_built_in);
        if built_in {
            self.show_instrument(t, id);
            if let Some(w) = self.channel.as_mut() {
                w.top = false;
            }
        } else if self.editor_shown(id) {
            self.raise_editor(id);
        } else {
            self.open_plugin(t, id);
        }
    }

    /// The power dot: a bypassed device plays again, a switched-off one turns on.
    fn channel_power(&mut self, t: TrackId, id: PluginId) {
        let off = self
            .project()
            .tracks
            .get(&t)
            .and_then(|x| x.inserts.iter().find(|p| p.id == id))
            .is_some_and(|p| p.deactivated);
        let command = if off {
            "plugin.activate"
        } else {
            "plugin.bypass"
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(command, json!({ "ids": [id.to_string()] }));
    }

    /// A card's menu: the mixer's for a device with 「名前を変更」, a splitter's own.
    fn channel_menu(&mut self, t: TrackId, pick: Pick, x: f32, y: f32) {
        let items = match pick {
            Pick::Plugin(id) => {
                let name = self
                    .project()
                    .tracks
                    .get(&t)
                    .and_then(|x| x.inserts.iter().find(|p| p.id == id))
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                let mut items = self.plugin_items(t, id);
                if items.is_empty() {
                    return;
                }
                let rename = Item::ask(
                    "名前を変更",
                    "plugin.rename",
                    json!({ "id": id.to_string() }),
                    "name",
                    &name,
                );
                items.insert(1.min(items.len()), rename);
                items
            }
            Pick::Split(id) => vec![
                self.ctx_as(
                    "分岐を足す",
                    "rack.branch.add",
                    json!({ "split": id.to_string() }),
                ),
                Item::gap(),
                self.ctx_as(
                    "スプリッターを外す",
                    "rack.split.remove",
                    json!({ "id": id.to_string() }),
                ),
            ],
        };
        self.open_context(items, x, y);
    }

    /// A row of a splitter's mode list: the splitter takes the mode.
    pub(crate) fn split_mode(&mut self, id: NodeId, word: &str) -> bool {
        self.popup = None;
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "rack.split.mode",
            json!({ "id": id.to_string(), "mode": word }),
        );
        true
    }

    /// The pointer moves with something of the window held.
    pub fn channel_moved(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(grab) = self.channel.as_ref().and_then(|w| w.grab.clone()) else {
            return false;
        };
        match grab {
            Grab::Move { from, pointer } => {
                let at = (
                    (from.0 + x - pointer.0).clamp(0.0, (g.size.0 - KEEP_W).max(0.0)),
                    (from.1 + y - pointer.1).clamp(0.0, (g.size.1 - BAR_H).max(0.0)),
                );
                if let Some(w) = self.channel.as_mut() {
                    w.at = Some(at);
                }
            }
            Grab::Card {
                pick,
                pointer,
                moved,
                ..
            } => {
                let moved = moved || (x - pointer.0).hypot(y - pointer.1) > DRAG;
                let to = if moved {
                    self.channel_view(g)
                        .and_then(|v| slot_near(&v.flow, pick, x, y))
                } else {
                    None
                };
                if let Some(w) = self.channel.as_mut() {
                    w.pointer = Some((x, y));
                    w.grab = Some(Grab::Card {
                        pick,
                        pointer,
                        moved,
                        to,
                    });
                }
            }
            Grab::Side {
                layer, edge, from, ..
            } => {
                let Some(k) = self.channel_view(g).and_then(|v| v.keys) else {
                    return true;
                };
                let now = keys::dragged(from, edge, k.map, k.span, x, y);
                if let Some(w) = self.channel.as_mut() {
                    w.grab = Some(Grab::Side {
                        layer,
                        edge,
                        from,
                        now,
                    });
                }
            }
            Grab::Transpose {
                layer, y0, from, ..
            } => {
                if let Some(w) = self.channel.as_mut() {
                    w.grab = Some(Grab::Transpose {
                        layer,
                        y0,
                        from,
                        now: keys::transposed(from, y0, y),
                    });
                }
            }
            Grab::Cross {
                split, i, x0, from, ..
            } => {
                let now = crossed(&from, i, x - x0);
                if let Some(w) = self.channel.as_mut() {
                    w.grab = Some(Grab::Cross {
                        split,
                        i,
                        x0,
                        from,
                        now,
                    });
                }
            }
        }
        true
    }

    /// Lets go: a dragged card moves to its slot, a crossover takes its frequency.
    /// A release the screen does not hand on (a dialog or the palette has the window): the
    /// editor's card drag, the mixer's grip and bar are let go without a drop, and a pad held
    /// in the macro window ends where it was.
    pub fn let_go_drags(&mut self) {
        if let Some(w) = self.channel.as_mut() {
            w.grab = None;
        }
        self.macro_up();
        self.mixer.grip_drag = None;
        self.mixer.bar_grab = None;
    }

    pub fn channel_up(&mut self, g: &Geometry) -> bool {
        let Some(grab) = self.channel.as_mut().and_then(|w| w.grab.take()) else {
            return false;
        };
        let Some(t) = self.channel.as_ref().map(|w| w.track) else {
            return true;
        };
        match grab {
            Grab::Card {
                pick,
                moved: true,
                to: Some(slot),
                ..
            } => {
                let from = self
                    .channel_view(g)
                    .and_then(|v| v.flow.cards.iter().find(|c| c.pick == pick)?.chain);
                if let Some(index) = drop_index(from, &slot) {
                    let to = match slot.chain {
                        Chain::Top => t.to_string(),
                        Chain::Branch(n) | Chain::Layer(n) => n.to_string(),
                    };
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.run_command(
                        "rack.node.move",
                        json!({ "id": pick_id(pick), "to": to, "index": index }),
                    );
                }
            }
            Grab::Cross {
                split, from, now, ..
            } if from != now => {
                let list = now
                    .iter()
                    .map(|f| format!("{f:.0}"))
                    .collect::<Vec<_>>()
                    .join(",");
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "rack.split.mode",
                    json!({ "id": split.to_string(), "mode": "frequency", "crossovers": list }),
                );
            }
            Grab::Side {
                layer, from, now, ..
            } if from != now => {
                let mut args = json!({ "id": layer.to_string() });
                if now.0 != from.0 {
                    args["keys"] = json!(format!("{}-{}", now.0.0, now.0.1));
                }
                if now.1 != from.1 {
                    args["velocity"] = json!(format!("{}-{}", now.1.0, now.1.1));
                }
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command("rack.layer.set", args);
            }
            // One step when it is let go, as the mock's.
            Grab::Transpose {
                layer, from, now, ..
            } if from != now => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "rack.layer.set",
                    json!({ "id": layer.to_string(), "transpose": now }),
                );
            }
            _ => {}
        }
        true
    }

    /// The pointer moves with nothing held: the part under it lights.
    pub fn channel_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.channel.is_none() {
            return false;
        }
        let over = self.over_channel(g, x, y) && !self.over_macros(g, x, y);
        let hot = if over {
            self.channel_view(g).and_then(|v| part_at(&v, x, y))
        } else {
            None
        };
        let pointer = over.then_some((x, y));
        let Some(w) = self.channel.as_mut() else {
            return false;
        };
        let changed = w.hot != hot || w.pointer.is_some() != pointer.is_some() || over;
        w.hot = hot;
        w.pointer = pointer;
        changed
    }

    /// A right press: a card's menu. True over the window.
    pub fn channel_right(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if !self.over_channel(g, x, y) {
            return false;
        }
        let Some(v) = self.channel_view(g) else {
            return false;
        };
        let Some(t) = self.channel.as_ref().map(|w| w.track) else {
            return false;
        };
        match part_at(&v, x, y) {
            Some(Part::Card(pick) | Part::Dot(pick) | Part::More(pick)) => {
                if let Some(w) = self.channel.as_mut() {
                    w.chosen = Some(pick);
                }
                self.channel_menu(t, pick, x, y);
            }
            Some(Part::Macro(k)) => self.macro_knob_menu(t, k, x, y),
            _ => {}
        }
        true
    }

    /// A turn of the wheel over the flow scrolls it (Shift: sideways).
    pub fn channel_wheel(&mut self, g: &Geometry, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        if !self.over_channel(g, x, y) {
            return false;
        }
        let Some(v) = self.channel_view(g) else {
            return false;
        };
        if !v.body.contains(x, y) {
            return true;
        }
        let probe_w = v.flow.w;
        let probe_h = v.flow.h;
        if let Some(w) = self.channel.as_mut() {
            let (sx, sy) = (w.scroll.0 - dx, w.scroll.1 - dy);
            w.scroll = (
                sx.clamp(0.0, (probe_w - v.body.w).max(0.0)),
                sy.clamp(0.0, (probe_h - v.body.h).max(0.0)),
            );
        }
        true
    }
}

/// A splitter's modes as a list (UI-320), the one it has checked.
pub fn mode_pick(p: &Project, t: TrackId, id: NodeId) -> crate::pick_list::PickList {
    use crate::pick_list::{PickList, PickRow};
    let now = split_of(p, t, id).map(|s| s.mode);
    PickList::of(
        [
            (SplitMode::Normal, "normal"),
            (SplitMode::Channel, "channel"),
            (SplitMode::Frequency, "frequency"),
        ]
        .into_iter()
        .map(|(mode, word)| {
            PickRow::new(mode_text(mode))
                .act(crate::popup::Action::SplitMode(id, word))
                .on(now == Some(mode))
        })
        .collect(),
    )
}

/// What a branch's or a layer's head speaks for: 「分岐 N」 (L, R on a channel split) or the
/// layer's instrument.
/// The card that picks a layer: its instrument.
fn layer_pick(p: &Project, t: TrackId, id: NodeId) -> Option<Pick> {
    let t = p.tracks.get(&t)?;
    keys::layers(t)
        .iter()
        .find(|l| l.id == id)
        .map(|l| Pick::Plugin(l.instrument))
}

fn col_who(t: &xirius_core::model::Track, f: &Flow, col: &Column) -> String {
    match col.kind {
        ColKind::Branch { .. } => format!("分岐 {}", col_label(col)),
        ColKind::Layer { index } => f
            .cards
            .iter()
            .find(|c| matches!(c.kind, CardKind::Instrument { layer: Some(i) } if i == index))
            .and_then(|c| match c.pick {
                Pick::Plugin(id) => t.inserts.iter().find(|p| p.id == id),
                Pick::Split(_) => None,
            })
            .map_or_else(
                || format!("レイヤー {}", col.n),
                |p| format!("レイヤー {}", p.name),
            ),
    }
}

/// The channel editor as a reader meets it: a group named 「チャンネルエディター <track>」 with
/// the bar's preset tag and its ‹ › (dim at the list's ends),
/// its toolbar's buttons, a Button for each card (a device's with its power as a toggle, a
/// splitter's with its mode) and, for each branch and layer, a Slider of its level in dB, a
/// toggle Button for its mute and a Button that takes it away.
pub fn marks(s: &Studio, g: &Geometry) -> Vec<crate::a11y::Mark> {
    use crate::a11y::{Mark, bounds};
    use gpui::accesskit;
    let Some(w) = &s.channel else {
        return Vec::new();
    };
    let Some(v) = s.channel_view(g) else {
        return Vec::new();
    };
    let p = s.project();
    let Some(t) = p.tracks.get(&w.track) else {
        return Vec::new();
    };
    let mark = |key: String, node: accesskit::Node| Mark {
        key,
        children: Vec::new(),
        node,
        fold: None,
        pick: None,
        press: None,
        turn: None,
    };
    let button = |name: &str, r: Rect| {
        let mut node = accesskit::Node::new(accesskit::Role::Button);
        node.set_label(name);
        node.set_bounds(bounds(r));
        node
    };
    let toggled = |node: &mut accesskit::Node, on: bool| {
        node.set_toggled(if on {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        });
    };
    let mut children = Vec::new();
    // The bar's preset tag and its ‹ › (MIX-042), named as the mock's.
    let (preset, modified) = s.chain_presets.tag(t);
    let (back, on) = s.chain_presets.steps(t);
    let tag = format!(
        "プリセット: {preset}{}",
        if modified { "（変更あり）" } else { "" }
    );
    let steps = [
        ("channel-preset-prev", "前のプリセット", v.bar.prev, back),
        ("channel-preset", tag.as_str(), Some(v.bar.tag), true),
        ("channel-preset-next", "次のプリセット", v.bar.next, on),
    ];
    for (key, name, r, can) in steps {
        let Some(r) = r else {
            continue;
        };
        let mut node = button(name, r);
        if !can {
            node.set_disabled();
        }
        children.push(mark(key.into(), node));
    }
    for (tool, r) in &v.tools {
        let mut node = button(tool.label(), *r);
        if *tool == Tool::Macros {
            toggled(&mut node, w.macros);
        }
        children.push(mark(format!("channel-tool-{tool:?}"), node));
    }
    if let Some(m) = &v.macros {
        for (i, cell) in m.cells.iter().enumerate() {
            let k = i as u8;
            let id = xirius_core::controls::ControlId::Knob(k);
            let value = crate::macro_pad::knob_value(s, t, k);
            let mut node = accesskit::Node::new(accesskit::Role::Slider);
            node.set_label(t.control_label(id));
            node.set_bounds(bounds(*cell));
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(100.0);
            node.set_numeric_value((value * 100.0).round());
            node.set_value(crate::macro_pad::percent(value));
            children.push(mark(format!("channel-macro-{i}"), node));
        }
        children.push(mark(
            "channel-macro-out".into(),
            button(POPOUT_LABEL, m.popout),
        ));
    }
    let shown = |r: Rect| v.body.contains(r.x + r.w / 2.0, r.y + r.h / 2.0);
    for card in v.flow.cards.iter().filter(|c| shown(c.rect)) {
        let cp = card_parts(card);
        match card.pick {
            Pick::Plugin(id) => {
                let Some(plugin) = t.inserts.iter().find(|p| p.id == id) else {
                    continue;
                };
                let mut node = button(&plugin.name, card.rect);
                node.set_selected(w.chosen == Some(card.pick));
                children.push(mark(format!("channel-card-{id}"), node));
                if let Some(d) = cp.dot {
                    let mut node = button(&format!("{} の電源", plugin.name), d);
                    toggled(&mut node, !plugin.bypass && !plugin.deactivated);
                    children.push(mark(format!("channel-power-{id}"), node));
                }
            }
            Pick::Split(id) => {
                let mut node = button("スプリッター", card.rect);
                node.set_selected(w.chosen == Some(card.pick));
                children.push(mark(format!("channel-card-{id}"), node));
                if let (Some(r), CardKind::Split { mode }) = (cp.mode, card.kind) {
                    let mut node = button("分け方", r);
                    node.set_value(mode_text(mode));
                    children.push(mark(format!("channel-mode-{id}"), node));
                }
            }
        }
    }
    for col in v.flow.columns.iter().filter(|c| shown(c.head)) {
        let Some(target) = s.row_target(t.id, col.id) else {
            continue;
        };
        let who = col_who(t, &v.flow, col);
        let hp = head_parts(col);
        let db = s
            .mixer
            .level_drag
            .filter(|d| d.track == t.id && d.node == col.id)
            .map_or(target.db, |d| d.value);
        let mut level = accesskit::Node::new(accesskit::Role::Slider);
        level.set_label(format!("{who} の音量"));
        level.set_bounds(bounds(hp.level));
        level.set_min_numeric_value(f64::from(crate::mixer::meters::FLOOR_DB));
        level.set_max_numeric_value(f64::from(crate::mixer::meters::CEIL_DB));
        level.set_numeric_value(db);
        level.set_value(crate::mixer::flow::level_text((db, false)));
        children.push(mark(format!("channel-level-{}", col.id), level));
        let mut mute = button(&format!("{who} をミュート"), hp.mute);
        toggled(&mut mute, target.mute);
        children.push(mark(format!("channel-mute-{}", col.id), mute));
        if let Some(r) = hp.remove {
            children.push(mark(
                format!("channel-remove-{}", col.id),
                button(&format!("{who} を外す"), r),
            ));
        }
    }
    if let Some(k) = &v.keys {
        for b in &k.boxes {
            let name = keys::layer_name(t, b.index);
            if b.index == k.chosen
                && let Some((f, v)) = k.transpose
            {
                let mut node = accesskit::Node::new(accesskit::Role::Slider);
                node.set_label(format!("{name} の Transpose"));
                node.set_bounds(bounds(f));
                node.set_min_numeric_value(-f64::from(keys::TRANSPOSE));
                node.set_max_numeric_value(f64::from(keys::TRANSPOSE));
                node.set_numeric_value(f64::from(v));
                node.set_value(keys::transpose_text(v));
                children.push(mark(format!("channel-transpose-{}", b.id), node));
            }
            let mut node = button(&format!("{name} の範囲"), b.rect);
            node.set_value(format!(
                "{} · {}",
                keys::keys_text(b.ranges.0),
                keys::velocity_text(b.ranges.1)
            ));
            node.set_selected(b.index == k.chosen);
            children.push(mark(format!("channel-range-{}", b.id), node));
            let ((lo, hi), (soft, loud)) = b.ranges;
            for (e, v) in keys::Edge::ALL.into_iter().zip([lo, hi, soft, loud]) {
                let mut node = accesskit::Node::new(accesskit::Role::Slider);
                node.set_label(format!("{name} {}", e.label()));
                node.set_bounds(bounds(keys::reach(b, e)));
                let (min, max) = match e {
                    keys::Edge::Low | keys::Edge::High => (0.0, 127.0),
                    keys::Edge::Soft | keys::Edge::Loud => (1.0, 127.0),
                };
                node.set_min_numeric_value(min);
                node.set_max_numeric_value(max);
                node.set_numeric_value(f64::from(v));
                node.set_value(match e {
                    keys::Edge::Low | keys::Edge::High => xirius_params::pitch::name(v),
                    keys::Edge::Soft | keys::Edge::Loud => v.to_string(),
                });
                children.push(mark(format!("channel-side-{}-{e:?}", b.id), node));
            }
        }
    }
    let mut node = accesskit::Node::new(accesskit::Role::Group);
    node.set_label(format!("{NAME} {}", t.shown_name()));
    node.set_bounds(bounds(v.r));
    vec![Mark {
        key: "channel-editor".into(),
        children,
        node,
        fold: None,
        pick: None,
        press: None,
        turn: None,
    }]
}
