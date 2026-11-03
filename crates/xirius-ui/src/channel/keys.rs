//! The key range figure (MIX-043): under a multi-instrument's flow, each layer's keys (across)
//! and velocities (up) as an outlined box over a keyboard, with a handle on each side of the
//! chosen layer's box. The boxes are not filled: a layer's colour is only its small chip.

use xirius_core::id::NodeId;
use xirius_core::model::Track;
use xirius_core::rack::{Layer, RackNode, Source};
use xirius_params::pitch;

use crate::mixer::flow::Pick;
use crate::studio::Rect;

/// The least width of a window that shows the figure.
pub const LEAST_W: f32 = 720.0;
/// The room round the figure, its header's row, the map and the keyboard.
pub const PAD: f32 = 8.0;
pub const HEAD_H: f32 = 32.0;
pub const MAP_H: f32 = 100.0;
pub const KEYS_H: f32 = 64.0;
pub const PANEL_H: f32 = HEAD_H + MAP_H + KEYS_H + PAD;
/// A handle as it is drawn (long and thin) and how far from its side it is caught.
pub const GRIP_L: f32 = 16.0;
pub const GRIP_T: f32 = 4.0;
const REACH: f32 = 5.0;
/// A black key's height in the keyboard.
pub const BLACK_H: f32 = 40.0;

/// A side of a layer's box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The lowest key.
    Low,
    /// The highest key.
    High,
    /// The softest velocity.
    Soft,
    /// The loudest velocity.
    Loud,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Low, Edge::High, Edge::Soft, Edge::Loud];

    /// What a reader calls it, after the layer's name.
    pub fn label(self) -> &'static str {
        match self {
            Edge::Low => "の下の鍵",
            Edge::High => "の上の鍵",
            Edge::Soft => "のベロシティの下",
            Edge::Loud => "のベロシティの上",
        }
    }
}

/// A layer's keys and velocities, each `low..=high`.
pub type Ranges = ((u8, u8), (u8, u8));
/// Keys and where each is drawn.
pub type KeyRects = Vec<(u8, Rect)>;
/// The keys the map and the keyboard show: the first and how many.
pub type Span = (f32, f32);
/// The fewest keys shown: six octaves.
const LEAST_KEYS: u8 = 72;

/// One layer's box.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerBox {
    pub id: NodeId,
    pub index: usize,
    pub rect: Rect,
    pub ranges: Ranges,
}

/// The figure as it is laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct Keys {
    pub panel: Rect,
    pub head: Rect,
    pub map: Rect,
    pub keyboard: Rect,
    pub span: Span,
    pub boxes: Vec<LayerBox>,
    /// The layer the header names and whose box has the handles.
    pub chosen: usize,
}

/// The layers of a multi-instrument track; none for any other.
pub fn layers(t: &Track) -> &[Layer] {
    match t.rack.as_ref().and_then(|r| r.source.as_ref()) {
        Some(Source::Multi { layers, .. }) => layers,
        _ => &[],
    }
}

fn holds(nodes: &[RackNode], pick: Pick) -> bool {
    nodes.iter().any(|n| match n {
        RackNode::Plugin(id) => pick == Pick::Plugin(*id),
        RackNode::Split(s) => {
            pick == Pick::Split(s.id) || s.branches.iter().any(|b| holds(&b.fx, pick))
        }
    })
}

/// The layer the chosen card is in (its instrument or one of its effects); the first if none.
pub fn chosen_layer(t: &Track, chosen: Option<Pick>) -> usize {
    let Some(pick) = chosen else {
        return 0;
    };
    layers(t)
        .iter()
        .position(|l| pick == Pick::Plugin(l.instrument) || holds(&l.fx, pick))
        .unwrap_or(0)
}

/// The keys the figure shows for the layers: from the C at or under the lowest key to the C at
/// or over the highest, and six octaves at the least.
pub fn span(layers: &[Layer]) -> Span {
    // a range kept backwards counts from its lower key
    let lo = (layers.iter().map(|l| l.keys.0.min(l.keys.1)).min())
        .unwrap_or(0)
        .min(127);
    let hi = (layers.iter().map(|l| l.keys.0.max(l.keys.1)).max())
        .unwrap_or(127)
        .min(127);
    let mut lo = lo / 12 * 12;
    let mut end = (hi.div_ceil(12) * 12).saturating_add(1).min(128);
    while end - lo < LEAST_KEYS {
        if end < 128 {
            end = (end + 12).min(128);
        } else {
            lo = lo.saturating_sub(12);
        }
    }
    (f32::from(lo), f32::from(end - lo))
}

/// The left of key `k` in the map.
pub fn key_x(map: Rect, span: Span, k: f32) -> f32 {
    map.x + map.w * (k - span.0) / span.1
}

/// The height of velocity `v` (0..=127) in the map: the louder, the higher.
pub fn vel_y(map: Rect, v: f32) -> f32 {
    map.bottom() - map.h * v / 127.0
}

/// A layer's box for its ranges.
pub fn box_rect(map: Rect, span: Span, ((lo, hi), (soft, loud)): Ranges) -> Rect {
    let x0 = key_x(map, span, f32::from(lo));
    let x1 = key_x(map, span, f32::from(hi) + 1.0);
    let y0 = vel_y(map, f32::from(loud));
    let y1 = vel_y(map, f32::from(soft) - 1.0);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// The figure in `panel` for the track's layers; `held` is a layer being dragged and its
/// ranges now.
pub fn layout(t: &Track, panel: Rect, chosen: usize, held: Option<(NodeId, Ranges)>) -> Keys {
    let inner = Rect::new(
        panel.x + PAD,
        panel.y,
        (panel.w - 2.0 * PAD).max(0.0),
        panel.h,
    );
    let head = Rect::new(inner.x, inner.y, inner.w, HEAD_H);
    let map = Rect::new(inner.x, head.bottom(), inner.w, MAP_H);
    let keyboard = Rect::new(inner.x, map.bottom(), inner.w, KEYS_H);
    let span = span(layers(t));
    let boxes = layers(t)
        .iter()
        .enumerate()
        .map(|(index, l)| {
            let ranges = held
                .filter(|(id, _)| *id == l.id)
                .map_or((l.keys, l.velocity), |(_, r)| r);
            LayerBox {
                id: l.id,
                index,
                rect: box_rect(map, span, ranges),
                ranges,
            }
        })
        .collect();
    Keys {
        panel,
        head,
        map,
        keyboard,
        span,
        boxes,
        chosen,
    }
}

/// A handle as it is drawn: in the middle of its side.
pub fn grip(b: &LayerBox, e: Edge) -> Rect {
    let r = b.rect;
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    match e {
        Edge::Low => Rect::new(r.x - GRIP_T / 2.0, cy - GRIP_L / 2.0, GRIP_T, GRIP_L),
        Edge::High => Rect::new(r.right() - GRIP_T / 2.0, cy - GRIP_L / 2.0, GRIP_T, GRIP_L),
        Edge::Loud => Rect::new(cx - GRIP_L / 2.0, r.y - GRIP_T / 2.0, GRIP_L, GRIP_T),
        Edge::Soft => Rect::new(cx - GRIP_L / 2.0, r.bottom() - GRIP_T / 2.0, GRIP_L, GRIP_T),
    }
}

/// Where a side is caught: all along it.
pub fn reach(b: &LayerBox, e: Edge) -> Rect {
    let r = b.rect;
    match e {
        Edge::Low => Rect::new(r.x - REACH, r.y, 2.0 * REACH, r.h),
        Edge::High => Rect::new(r.right() - REACH, r.y, 2.0 * REACH, r.h),
        Edge::Loud => Rect::new(r.x, r.y - REACH, r.w, 2.0 * REACH),
        Edge::Soft => Rect::new(r.x, r.bottom() - REACH, r.w, 2.0 * REACH),
    }
}

/// What in the figure is under the point: the chosen box's sides first, then the smallest box.
pub fn hit(k: &Keys, x: f32, y: f32) -> Option<(NodeId, Option<Edge>)> {
    let m = k.map;
    let near = Rect::new(
        m.x - REACH,
        m.y - REACH,
        m.w + 2.0 * REACH,
        m.h + 2.0 * REACH,
    );
    if !near.contains(x, y) {
        return None;
    }
    if let Some(b) = k.boxes.get(k.chosen)
        && let Some(e) = Edge::ALL.into_iter().find(|e| reach(b, *e).contains(x, y))
    {
        return Some((b.id, Some(e)));
    }
    k.boxes
        .iter()
        .filter(|b| b.rect.contains(x, y))
        .min_by(|a, b| (a.rect.w * a.rect.h).total_cmp(&(b.rect.w * b.rect.h)))
        .map(|b| (b.id, None))
}

/// The ranges after the side `e` was dragged to the point: a side does not pass the other. Past
/// the map's ends, the keys go on at the same scale.
pub fn dragged(from: Ranges, e: Edge, map: Rect, span: Span, x: f32, y: f32) -> Ranges {
    let key = |x: f32| ((x - map.x) / map.w * span.1 + span.0).round();
    let vel = |y: f32| ((map.bottom() - y) / map.h * 127.0).round();
    let ((lo, hi), (soft, loud)) = from;
    match e {
        Edge::Low => ((key(x).clamp(0.0, f32::from(hi)) as u8, hi), (soft, loud)),
        Edge::High => (
            (lo, (key(x) - 1.0).clamp(f32::from(lo), 127.0) as u8),
            (soft, loud),
        ),
        Edge::Loud => ((lo, hi), (soft, vel(y).clamp(f32::from(soft), 127.0) as u8)),
        Edge::Soft => (
            (lo, hi),
            ((vel(y) + 1.0).clamp(1.0, f32::from(loud)) as u8, loud),
        ),
    }
}

/// A key range as the header says it: 「E0 – B1」.
pub fn keys_text((lo, hi): (u8, u8)) -> String {
    format!("{} – {}", pitch::name(lo), pitch::name(hi))
}

/// A velocity range as the header says it: 「1 – 127」.
pub fn velocity_text((soft, loud): (u8, u8)) -> String {
    format!("{soft} – {loud}")
}

/// Is key `k` a black one?
pub fn black(k: u8) -> bool {
    matches!(k % 12, 1 | 3 | 6 | 8 | 10)
}

/// The keyboard's keys, on the map's scale of a semitone each: a black key takes its own
/// semitone, a white key its own and half of each black one beside it. Each white key's rect,
/// then each black key's, with its key.
pub fn key_rects(keyboard: Rect, span: Span) -> (KeyRects, KeyRects) {
    let x = |k: f32| key_x(keyboard, span, k);
    let mut whites = Vec::new();
    let mut blacks = Vec::new();
    let (first, n) = (span.0 as u8, span.1 as u8);
    for k in first..first + n {
        let kf = f32::from(k);
        if black(k) {
            blacks.push((
                k,
                Rect::new(x(kf), keyboard.y, x(kf + 1.0) - x(kf), BLACK_H),
            ));
        } else {
            let left = if k > first && black(k - 1) {
                kf - 0.5
            } else {
                kf
            };
            let right = if k + 1 < first + n && black(k + 1) {
                kf + 1.5
            } else {
                kf + 1.0
            };
            whites.push((
                k,
                Rect::new(x(left), keyboard.y, x(right) - x(left), keyboard.h),
            ));
        }
    }
    (whites, blacks)
}
