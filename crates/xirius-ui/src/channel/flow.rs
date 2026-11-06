//! Where the channel editor's flow puts things (MIX-042): the input at the top, the chain
//! down the middle, a splitter's branches side by side under it with a head over each, a
//! multi-instrument's layers fanned out from the MIDI, and the output at the bottom. The
//! structure lines join them with straight runs and quarter circles only (the mock's
//! `mixer-flow.html` s03); every line knows the parts it touches, so the one under the
//! pointer and the chosen one light their own paths.

use xirius_core::id::{NodeId, PluginId};
use xirius_core::model::{PluginKind, Project, Track, TrackKind};
use xirius_core::rack::{Branch, Layer, Rack, RackNode, Source, Split, SplitMode};
use xirius_design::{FontSize, LinePiece, StructureLine};

use crate::draw::estimated_text_width;
use crate::mixer::flow::Pick;
use crate::studio::Rect;

/// The flow's margin inside the body.
pub const PAD: f32 = 16.0;
/// A plug-in's card, and a splitter's at its least.
pub const CARD_W: f32 = 136.0;
pub const CARD_H: f32 = 30.0;
pub const SPLIT_W: f32 = 200.0;
/// The input's and the output's pills: their height, padding, icon and the gap after it.
pub const IO_H: f32 = 24.0;
pub const IO_PAD: f32 = 10.0;
pub const IO_ICON: f32 = 12.0;
pub const IO_GAP: f32 = 6.0;
/// The room between two parts of a chain.
pub const STEP: f32 = 32.0;
/// From a splitter's card (or the MIDI) down to its branches' heads.
const FAN: f32 = 16.0;
/// A branch's or a layer's head: its number, level, M and ×.
pub const HEAD_H: f32 = 26.0;
/// A layer's column's least width, a branch's (its head has a pan too: MIX-045) and the room
/// between columns.
const COL_MIN: f32 = 148.0;
const BRANCH_MIN: f32 = 176.0;
pub const COL_GAP: f32 = 8.0;
/// A splitter's ＋ right of its columns, and the room kept for it on each side so the
/// columns stay on the middle.
pub const ADD_BTN: f32 = 24.0;
const ADD_ROOM: f32 = COL_GAP + 4.0 + ADD_BTN;
/// The cut in a muted branch's line under its head (MIX-045, approved 2026-11-06).
pub const MUTE_CUT: f32 = 12.0;
/// A gap's ＋.
pub const GAP_BTN: f32 = 20.0;
const INSET: f32 = StructureLine::END_INSET;

/// A part a line touches: a plug-in, or a splitter, a branch or a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Plugin(PluginId),
    Node(NodeId),
}

impl Key {
    pub fn of(pick: Pick) -> Key {
        match pick {
            Pick::Plugin(id) => Key::Plugin(id),
            Pick::Split(id) | Pick::Branch(id) => Key::Node(id),
        }
    }
}

/// A chain of the rack, by what holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Chain {
    Top,
    Branch(NodeId),
    Layer(NodeId),
}

/// What a card is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardKind {
    Effect,
    /// The track's instrument, or a layer's (its colour's index).
    Instrument {
        layer: Option<usize>,
    },
    Split {
        mode: SplitMode,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub pick: Pick,
    pub rect: Rect,
    pub kind: CardKind,
    /// The chain it is in and its place there; an instrument is in none.
    pub chain: Option<(Chain, usize)>,
    /// The splitters, branches and layers it is inside.
    pub within: Vec<Key>,
}

/// What a column is: a splitter's branch, or a layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColKind {
    Branch { split: NodeId, mode: SplitMode },
    Layer { index: usize },
}

/// A branch's or a layer's column, its head over it.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub id: NodeId,
    pub kind: ColKind,
    /// 1 for the first.
    pub n: usize,
    pub head: Rect,
    /// The head and everything under it.
    pub rect: Rect,
    /// A frequency split's band ("〜200", "200–2k", "2k〜").
    pub band: Option<String>,
    pub within: Vec<Key>,
}

/// A ＋ between two parts, or in an empty chain: it adds an effect there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    /// The room it shows in under the pointer.
    pub rect: Rect,
    pub button: Rect,
    pub chain: Chain,
    pub index: usize,
    /// The part before it in its chain.
    pub after: Option<Pick>,
}

/// Where a dragged card can go: a chain and a place in it, marked by a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub chain: Chain,
    pub index: usize,
    pub mark: Rect,
}

/// A frequency split's crossover, between two of its columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cross {
    pub split: NodeId,
    pub i: usize,
    pub rect: Rect,
}

/// The input's or the output's pill.
#[derive(Clone, Debug, PartialEq)]
pub struct Io {
    pub rect: Rect,
    pub icon: &'static str,
    pub text: String,
}

/// One structure line and the parts it touches.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub pieces: Vec<LinePiece>,
    pub keys: Vec<Key>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flow {
    /// Its size with the margin.
    pub w: f32,
    pub h: f32,
    pub ios: Vec<Io>,
    pub cards: Vec<Card>,
    pub columns: Vec<Column>,
    pub gaps: Vec<Gap>,
    pub slots: Vec<Slot>,
    pub crosses: Vec<Cross>,
    pub links: Vec<Link>,
    /// Each splitter's ＋ that adds a branch; a channel split has none.
    pub adds: Vec<Add>,
}

/// A splitter's ＋ right of its columns.
#[derive(Clone, Debug, PartialEq)]
pub struct Add {
    pub split: NodeId,
    pub rect: Rect,
}

/// The end of a chain a line leaves from: its point, how far below it the line starts and
/// the parts the line carries on.
#[derive(Clone, Debug)]
struct End {
    x: f32,
    y: f32,
    inset: f32,
    /// The height the line turns at, when it is not just over the next part.
    ym: Option<f32>,
    keys: Vec<Key>,
}

/// A track's rack as the editor shows it: its own, or its inserts in a line.
pub fn rack_of(t: &Track) -> Rack {
    t.rack.clone().unwrap_or_else(|| Rack::serial(t))
}

fn pill_w(text: &str) -> f32 {
    IO_PAD + IO_ICON + IO_GAP + estimated_text_width(text, FontSize::Small) + IO_PAD
}

fn chain_w(nodes: &[RackNode]) -> f32 {
    nodes.iter().map(node_w).fold(CARD_W, f32::max)
}

fn node_w(n: &RackNode) -> f32 {
    match n {
        RackNode::Plugin(_) => CARD_W,
        RackNode::Split(s) => split_w(s),
    }
}

/// A splitter's width: its columns, and the room for its ＋ on both sides.
fn split_w(s: &Split) -> f32 {
    let add = if s.mode == SplitMode::Channel {
        0.0
    } else {
        2.0 * ADD_ROOM
    };
    (cols_w(s.branches.iter().map(|b| chain_w(&b.fx).max(BRANCH_MIN))) + add).max(SPLIT_W)
}

fn cols_w(widths: impl Iterator<Item = f32>) -> f32 {
    let (sum, n) = widths.fold((0.0, 0), |(s, n), w| (s + w.max(COL_MIN), n + 1));
    sum + COL_GAP * (n.max(1) - 1) as f32
}

fn chain_h(nodes: &[RackNode]) -> f32 {
    if nodes.is_empty() {
        return CARD_H;
    }
    nodes.iter().map(node_h).sum::<f32>() + STEP * (nodes.len() - 1) as f32
}

fn node_h(n: &RackNode) -> f32 {
    match n {
        RackNode::Plugin(_) => CARD_H,
        RackNode::Split(s) => {
            CARD_H + FAN + HEAD_H + s.branches.iter().map(branch_h).fold(0.0, f32::max)
        }
    }
}

/// A branch's room under its head: an empty one's ＋, else the room before its first part (its
/// ＋, as between two parts) and its chain.
fn branch_h(b: &Branch) -> f32 {
    if b.fx.is_empty() {
        CARD_H
    } else {
        STEP + chain_h(&b.fx)
    }
}

fn layer_w(l: &Layer) -> f32 {
    chain_w(&l.fx)
}

fn layer_h(l: &Layer) -> f32 {
    CARD_H
        + if l.fx.is_empty() {
            0.0
        } else {
            STEP + chain_h(&l.fx)
        }
}

/// A band's name: "〜200", "200–2k", "2k〜".
pub fn band_text(crossovers: &[f64], i: usize) -> String {
    let hz = |f: f64| {
        if f >= 1000.0 {
            let k = (f / 100.0).round() / 10.0;
            if k.fract() == 0.0 {
                format!("{k:.0}k")
            } else {
                format!("{k:.1}k")
            }
        } else {
            format!("{f:.0}")
        }
    };
    match (
        i.checked_sub(1).and_then(|j| crossovers.get(j)),
        crossovers.get(i),
    ) {
        (None, Some(&hi)) => format!("〜{}", hz(hi)),
        (Some(&lo), Some(&hi)) => format!("{}–{}", hz(lo), hz(hi)),
        (Some(&lo), None) => format!("{}〜", hz(lo)),
        (None, None) => String::new(),
    }
}

/// The flow of track `t`, its top left at `at`.
pub fn layout(p: &Project, t: &Track, at: (f32, f32)) -> Flow {
    let rack = rack_of(t);
    let input = crate::input_row::label(t).to_string();
    let output = crate::draw::output_label(p, t);
    // The widest part sets the width; everything is centred on it.
    let mut inner = chain_w(&rack.fx).max(pill_w(&input)).max(pill_w(&output));
    match &rack.source {
        Some(Source::Multi { layers, .. }) => {
            inner = inner.max(cols_w(layers.iter().map(layer_w)));
        }
        Some(Source::Single(_)) => inner = inner.max(CARD_W),
        None => {}
    }
    let w = inner + 2.0 * PAD;
    let cx = at.0 + w / 2.0;
    let mut f = Flow {
        w,
        ..Flow::default()
    };
    let midi = t.kind == TrackKind::Instrument;
    let input_icon = if midi { "midi" } else { "input" };
    let y = at.1 + PAD;
    let pill = f.io(cx, y, input_icon, input);
    let mut ends = vec![End {
        x: cx,
        y: pill.bottom(),
        inset: INSET,
        ym: None,
        keys: Vec::new(),
    }];
    let mut y = pill.bottom() + STEP;
    match &rack.source {
        Some(Source::Single(id)) => {
            let card = f.card(
                Pick::Plugin(*id),
                cx,
                y,
                CARD_W,
                CardKind::Instrument { layer: None },
                None,
                &[],
            );
            f.join(&ends, cx, card.y, Key::Plugin(*id), &[]);
            ends = vec![end_of(card, vec![Key::Plugin(*id)])];
            y = card.bottom() + STEP;
            // The first effect goes right after the instrument.
            f.gap_before(Chain::Top, 0, None, cx, card.bottom(), y);
        }
        Some(Source::Multi { id, layers }) => {
            let (out, bottom) = f.layers(&ends, *id, layers, cx, pill.bottom());
            ends = out;
            y = bottom + STEP;
        }
        None => {
            f.gap_before(Chain::Top, 0, None, cx, pill.bottom(), y);
        }
    }
    let (out, bottom) = f.chain(&rack.fx, Chain::Top, cx, y, ends, &[]);
    let y = if rack.fx.is_empty() {
        bottom
    } else {
        bottom + STEP
    };
    if let Some(last) = rack.fx.last() {
        f.gap_before(
            Chain::Top,
            rack.fx.len(),
            Some(pick_of(last)),
            cx,
            bottom,
            y,
        );
    }
    let pill = f.io(cx, y, "output", output);
    for e in &out {
        let from = (e.x, e.y + e.inset);
        let to = (cx, pill.y - INSET);
        let ym = e.ym.unwrap_or(pill.y - FAN);
        f.line(from, to, ym, e.keys.clone());
    }
    f.h = pill.bottom() + PAD - at.1;
    f
}

fn pick_of(n: &RackNode) -> Pick {
    match n {
        RackNode::Plugin(id) => Pick::Plugin(*id),
        RackNode::Split(s) => Pick::Split(s.id),
    }
}

fn end_of(card: Rect, keys: Vec<Key>) -> End {
    End {
        x: card.x + card.w / 2.0,
        y: card.bottom(),
        inset: INSET,
        ym: None,
        keys,
    }
}

impl Flow {
    /// Is the gap right before or after the card `pick`? Its ＋ shows while that card is
    /// chosen, as the mock's (and for a touch or the keys, which have no pointer over it).
    pub fn beside(&self, pick: Pick, gap: &Gap) -> bool {
        if gap.after == Some(pick) {
            return true;
        }
        self.cards
            .iter()
            .find(|c| c.pick == pick)
            .and_then(|c| c.chain)
            .is_some_and(|(chain, k)| gap.chain == chain && gap.index == k)
    }

    fn io(&mut self, cx: f32, y: f32, icon: &'static str, text: String) -> Rect {
        let w = pill_w(&text);
        let rect = Rect::new(cx - w / 2.0, y, w, IO_H);
        self.ios.push(Io { rect, icon, text });
        rect
    }

    #[allow(clippy::too_many_arguments)]
    fn card(
        &mut self,
        pick: Pick,
        cx: f32,
        y: f32,
        w: f32,
        kind: CardKind,
        chain: Option<(Chain, usize)>,
        within: &[Key],
    ) -> Rect {
        let rect = Rect::new(cx - w / 2.0, y, w, CARD_H);
        self.cards.push(Card {
            pick,
            rect,
            kind,
            chain,
            within: within.to_vec(),
        });
        rect
    }

    fn line(&mut self, from: (f32, f32), to: (f32, f32), ym: f32, keys: Vec<Key>) {
        if to.1 - from.1 < 1.0 {
            return;
        }
        self.links.push(Link {
            pieces: StructureLine::elbow(from, to, ym),
            keys,
        });
    }

    /// The lines from `ends` into a part whose top is at `top` on `cx`.
    fn join(&mut self, ends: &[End], cx: f32, top: f32, key: Key, within: &[Key]) {
        for e in ends {
            let mut keys = e.keys.clone();
            keys.push(key);
            keys.extend_from_slice(within);
            let ym = e.ym.unwrap_or(top - FAN);
            self.line((e.x, e.y + e.inset), (cx, top - INSET), ym, keys);
        }
    }

    /// The ＋ in the room between `top` and `bottom` on `cx`, before `index` of `chain`.
    fn gap_before(
        &mut self,
        chain: Chain,
        index: usize,
        after: Option<Pick>,
        cx: f32,
        top: f32,
        bottom: f32,
    ) {
        let rect = Rect::new(cx - CARD_W / 2.0, top, CARD_W, bottom - top);
        let my = (top + bottom) / 2.0;
        let button = Rect::new(cx - GAP_BTN / 2.0, my - GAP_BTN / 2.0, GAP_BTN, GAP_BTN);
        self.gaps.push(Gap {
            rect,
            button,
            chain,
            index,
            after,
        });
        self.slots.push(Slot {
            chain,
            index,
            mark: Rect::new(cx - CARD_W / 2.0, my - 1.0, CARD_W, 2.0),
        });
    }

    /// Lays `nodes` down from `y` on `cx`, joined to `ends`; returns the chain's ends and
    /// its bottom. The ＋ before the first part is the caller's.
    #[allow(clippy::too_many_arguments)]
    fn chain(
        &mut self,
        nodes: &[RackNode],
        chain: Chain,
        cx: f32,
        mut y: f32,
        mut ends: Vec<End>,
        within: &[Key],
    ) -> (Vec<End>, f32) {
        if nodes.is_empty() {
            return (ends, y);
        }
        let mut bottom = y;
        for (i, n) in nodes.iter().enumerate() {
            if i > 0 {
                self.gap_before(chain, i, Some(pick_of(&nodes[i - 1])), cx, bottom, y);
            }
            let (out, b) = match n {
                RackNode::Plugin(id) => {
                    let key = Key::Plugin(*id);
                    let r = self.card(
                        Pick::Plugin(*id),
                        cx,
                        y,
                        CARD_W,
                        CardKind::Effect,
                        Some((chain, i)),
                        within,
                    );
                    self.join(&ends, cx, r.y, key, within);
                    let mut keys = vec![key];
                    keys.extend_from_slice(within);
                    (vec![end_of(r, keys)], r.bottom())
                }
                RackNode::Split(s) => self.split(s, chain, i, cx, y, &ends, within),
            };
            ends = out;
            bottom = b;
            y = b + STEP;
        }
        (ends, bottom)
    }

    /// A splitter's card and its branches under it.
    #[allow(clippy::too_many_arguments)]
    fn split(
        &mut self,
        s: &Split,
        chain: Chain,
        index: usize,
        cx: f32,
        y: f32,
        ends: &[End],
        within: &[Key],
    ) -> (Vec<End>, f32) {
        let key = Key::Node(s.id);
        let card = self.card(
            Pick::Split(s.id),
            cx,
            y,
            SPLIT_W,
            CardKind::Split { mode: s.mode },
            Some((chain, index)),
            within,
        );
        self.join(ends, cx, card.y, key, within);
        let mut inside = within.to_vec();
        inside.push(key);
        let widths: Vec<f32> = s
            .branches
            .iter()
            .map(|b| chain_w(&b.fx).max(BRANCH_MIN))
            .collect();
        let used = widths.iter().sum::<f32>() + COL_GAP * (widths.len().max(1) - 1) as f32;
        let mut x = cx - used / 2.0;
        let head_y = card.bottom() + FAN;
        let col_top = head_y + HEAD_H;
        let deepest = s.branches.iter().map(branch_h).fold(0.0, f32::max);
        let mut out = Vec::new();
        let mut cols = Vec::new();
        for (i, (b, bw)) in s.branches.iter().zip(&widths).enumerate() {
            let col = Rect::new(x, head_y, *bw, HEAD_H + deepest);
            cols.push(col);
            let ccx = x + bw / 2.0;
            let band = (s.mode == SplitMode::Frequency).then(|| band_text(&s.crossovers, i));
            self.columns.push(Column {
                id: b.id,
                kind: ColKind::Branch {
                    split: s.id,
                    mode: s.mode,
                },
                n: i + 1,
                head: Rect::new(x, head_y, *bw, HEAD_H),
                rect: col,
                band,
                within: inside.clone(),
            });
            out.extend(self.column(b, ccx, card, col_top, &inside));
            x += bw + COL_GAP;
        }
        if s.mode != SplitMode::Channel
            && let Some(last) = cols.last()
        {
            self.adds.push(Add {
                split: s.id,
                rect: Rect::new(last.right() + COL_GAP + 4.0, head_y + 1.0, ADD_BTN, ADD_BTN),
            });
        }
        if s.mode == SplitMode::Frequency {
            for i in 0..cols.len().saturating_sub(1) {
                let bx = (cols[i].right() + cols[i + 1].x) / 2.0;
                self.crosses.push(Cross {
                    split: s.id,
                    i,
                    rect: Rect::new(bx - 6.0, head_y, 12.0, HEAD_H),
                });
            }
        }
        for e in &mut out {
            e.keys.push(key);
        }
        (out, col_top + deepest)
    }

    /// One branch's column on `cx` under the splitter's `card`: fanned to from it, its chain
    /// laid under its head, whose bottom is `top`.
    fn column(&mut self, b: &Branch, cx: f32, card: Rect, top: f32, within: &[Key]) -> Vec<End> {
        let mut inside = within.to_vec();
        inside.push(Key::Node(b.id));
        let fan = End {
            x: card.x + card.w / 2.0,
            y: card.bottom(),
            inset: INSET,
            ym: Some(card.bottom() + FAN),
            keys: inside.clone(),
        };
        if b.fx.is_empty() {
            // The line passes through the empty room; its ＋ adds the branch's first. A muted
            // branch's is cut in the room's middle.
            let room = Rect::new(cx - 1.0, top, 2.0, CARD_H);
            let (stop, go) = if b.mute {
                let mid = top + CARD_H / 2.0;
                (mid - MUTE_CUT / 2.0, mid + MUTE_CUT / 2.0)
            } else {
                (room.y, room.y)
            };
            self.line(
                (fan.x, fan.y + fan.inset),
                (cx, stop),
                card.bottom() + FAN,
                inside.clone(),
            );
            self.gap_before(Chain::Branch(b.id), 0, None, cx, top, top + CARD_H);
            return vec![End {
                x: cx,
                y: go,
                inset: 0.0,
                ym: None,
                keys: inside,
            }];
        }
        // A muted branch's line stops under its head and goes on after the cut.
        let fan = if b.mute {
            let mid = top + STEP / 2.0;
            self.line(
                (fan.x, fan.y + fan.inset),
                (cx, mid - MUTE_CUT / 2.0),
                card.bottom() + FAN,
                inside.clone(),
            );
            End {
                x: cx,
                y: mid + MUTE_CUT / 2.0,
                inset: 0.0,
                ym: None,
                keys: inside.clone(),
            }
        } else {
            fan
        };
        // The room under the head, as between two parts: its ＋ adds the branch's first, and a
        // card dragged there goes first.
        self.gap_before(Chain::Branch(b.id), 0, None, cx, top, top + STEP);
        let (ends, bottom) = self.chain(
            &b.fx,
            Chain::Branch(b.id),
            cx,
            top + STEP,
            vec![fan],
            &inside,
        );
        // The ＋ after the branch's last part, before the merge.
        if let Some(last) = b.fx.last() {
            self.gap_before(
                Chain::Branch(b.id),
                b.fx.len(),
                Some(pick_of(last)),
                cx,
                bottom,
                bottom + STEP,
            );
        }
        // The end of the branch, before the merge.
        ends
    }

    /// A multi-instrument's layers fanned out from the MIDI's pill (bottom at `from`).
    fn layers(
        &mut self,
        ends: &[End],
        id: NodeId,
        layers: &[Layer],
        cx: f32,
        from: f32,
    ) -> (Vec<End>, f32) {
        let widths: Vec<f32> = layers.iter().map(|l| layer_w(l).max(COL_MIN)).collect();
        let used = widths.iter().sum::<f32>() + COL_GAP * (widths.len().max(1) - 1) as f32;
        let head_y = from + FAN;
        let top = head_y + HEAD_H;
        let deepest = layers.iter().map(layer_h).fold(0.0, f32::max);
        let src = ends.first().map_or((cx, from), |e| (e.x, e.y));
        let mut x = cx - used / 2.0;
        let mut out = Vec::new();
        let multi = Key::Node(id);
        for (i, (l, lw)) in layers.iter().zip(&widths).enumerate() {
            let lcx = x + lw / 2.0;
            let inside = vec![multi, Key::Node(l.id)];
            self.columns.push(Column {
                id: l.id,
                kind: ColKind::Layer { index: i },
                n: i + 1,
                head: Rect::new(x, head_y, *lw, HEAD_H),
                rect: Rect::new(x, head_y, *lw, HEAD_H + deepest),
                band: None,
                within: vec![multi],
            });
            let key = Key::Plugin(l.instrument);
            let card = self.card(
                Pick::Plugin(l.instrument),
                lcx,
                top,
                CARD_W,
                CardKind::Instrument { layer: Some(i) },
                None,
                &inside,
            );
            let mut keys = inside.clone();
            keys.push(key);
            self.line(
                (src.0, src.1 + INSET),
                (lcx, card.y - INSET),
                from + FAN,
                keys.clone(),
            );
            let start = vec![end_of(card, keys)];
            let below = card.bottom() + STEP;
            self.gap_before(
                Chain::Layer(l.id),
                0,
                None,
                lcx,
                card.bottom(),
                if l.fx.is_empty() {
                    card.bottom() + STEP.min(deepest - CARD_H).max(GAP_BTN + 4.0)
                } else {
                    below
                },
            );
            let (ends, _) = self.chain(&l.fx, Chain::Layer(l.id), lcx, below, start, &inside);
            out.extend(ends.into_iter().map(|mut e| {
                e.keys.push(multi);
                e
            }));
            x += lw + COL_GAP;
        }
        (out, top + deepest)
    }
}

/// A plug-in's kind on its track, if it is there.
pub fn kind_of(t: &Track, id: PluginId) -> Option<PluginKind> {
    t.inserts.iter().find(|x| x.id == id).map(|x| x.kind)
}
