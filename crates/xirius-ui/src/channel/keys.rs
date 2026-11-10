//! The key range figure (MIX-043): under a multi-instrument's flow, each layer's keys (across)
//! and velocities (up) as an outlined box over a keyboard, with a handle on each side of the
//! chosen layer's box. The boxes are not filled: a layer's colour is only its small chip.

use xirius_core::id::NodeId;
use xirius_core::model::Track;
use xirius_core::rack::{Layer, RackNode, Source};
use xirius_params::pitch;

use xirius_design::FontSize;

use crate::device::play::{KeySpec, Keyboard};
use crate::draw::estimated_text_width;
use crate::mixer::flow::Pick;
use crate::studio::Rect;

/// The least width of a window that shows the figure.
pub const LEAST_W: f32 = 720.0;
/// The room round the figure, its header's row, the map, the gap and the keyboard.
pub const PAD: f32 = 8.0;
pub const HEAD_H: f32 = 32.0;
pub const MAP_H: f32 = 100.0;
/// The gap between the map and the keyboard.
pub const GAP: f32 = 4.0;
pub const KEYS_H: f32 = 64.0;
pub const PANEL_H: f32 = HEAD_H + MAP_H + GAP + KEYS_H + PAD;
/// A panel whose map and keyboard scroll sideways: the keyboard's rail is added.
pub const PANEL_SCROLL_H: f32 = PANEL_H + RAIL_H;
/// The rail under a scrolling keyboard: its gap and thickness, and the pixel after it.
pub const RAIL_H: f32 = 12.0;
/// While a side is dragged, the pointer this near an end of the map scrolls it.
pub const EDGE_SCROLL: f32 = 12.0;
/// A handle as it is drawn (long and thin) and how far from its side it is caught.
pub const GRIP_L: f32 = 16.0;
pub const GRIP_T: f32 = 4.0;
const REACH: f32 = 5.0;
/// The Transpose field (the mock's `.field`): its height, least width and inset, the gap after
/// a label and between the header's items, and how far a drag goes for a semitone.
pub const FIELD_H: f32 = 22.0;
pub const FIELD_W: f32 = 40.0;
pub const FIELD_PAD: f32 = 6.0;
pub const LABEL_GAP: f32 = 6.0;
pub const ITEM_GAP: f32 = 16.0;
pub const SEMITONE: f32 = 8.0;
pub const TRANSPOSE: i8 = 48;

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
    /// The playable keyboard under the map; the map's keys are its (see [`Keyboard::edge`]).
    pub board: Keyboard,
    pub boxes: Vec<LayerBox>,
    /// The layer the header names and whose box has the handles.
    pub chosen: usize,
    /// The chosen layer's Transpose field and what it shows (while dragged, the value held).
    pub transpose: Option<(Rect, i8)>,
}

/// A layer's name: its instrument's.
pub fn layer_name(t: &Track, index: usize) -> String {
    let Some(l) = layers(t).get(index) else {
        return String::new();
    };
    t.inserts
        .iter()
        .find(|p| p.id == l.instrument)
        .map_or_else(|| format!("レイヤー {}", index + 1), |p| p.name.clone())
}

/// A transpose as the field shows it: a sign before a rise.
pub fn transpose_text(v: i8) -> String {
    if v > 0 {
        format!("+{v}")
    } else {
        v.to_string()
    }
}

/// The transpose a drag from `y0` to `y` makes of `from`: a semitone each `SEMITONE` up.
pub fn transposed(from: i8, y0: f32, y: f32) -> i8 {
    let steps = ((y0 - y) / SEMITONE).round() as i32;
    (i32::from(from) + steps).clamp(-i32::from(TRANSPOSE), i32::from(TRANSPOSE)) as i8
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

/// The height of velocity `v` (0..=127) in the map: the louder, the higher.
pub fn vel_y(map: Rect, v: f32) -> f32 {
    map.bottom() - map.h * v / 127.0
}

/// A layer's box for its ranges: its sides on the keyboard's key boundaries.
pub fn box_rect(map: Rect, board: &Keyboard, ((lo, hi), (soft, loud)): Ranges) -> Rect {
    let x0 = board.edge(lo);
    let x1 = board.edge(hi.saturating_add(1));
    let y0 = vel_y(map, f32::from(loud));
    let y1 = vel_y(map, f32::from(soft) - 1.0);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// The figure in `panel` for the track's layers, the keyboard scrolled `scroll`. `held` is a
/// layer's ranges while a side of it is dragged; `turning`, its transpose while its field is.
pub fn layout(
    t: &Track,
    panel: Rect,
    chosen: usize,
    held: Option<(NodeId, Ranges)>,
    turning: Option<(NodeId, i8)>,
    scroll: f32,
) -> Keys {
    let inner = Rect::new(
        panel.x + PAD,
        panel.y,
        (panel.w - 2.0 * PAD).max(0.0),
        panel.h,
    );
    let head = Rect::new(inner.x, inner.y, inner.w, HEAD_H);
    let map = Rect::new(inner.x, head.bottom(), inner.w, MAP_H);
    // Under the least width the keys keep their size and the map and keyboard scroll together.
    let scrolls = inner.w < KeySpec::LEAST_W;
    let keyboard = Rect::new(
        inner.x,
        map.bottom() + GAP,
        inner.w,
        if scrolls {
            Keyboard::view_height(KEYS_H)
        } else {
            KEYS_H
        },
    );
    let board = Keyboard::new(keyboard, scroll, KeySpec::full(inner.w, KEYS_H));
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
                rect: box_rect(map, &board, ranges),
                ranges,
            }
        })
        .collect();
    // The header: chip, name, then "Transpose" and its field.
    let transpose = layers(t).get(chosen).map(|l| {
        let v = turning
            .filter(|(id, _)| *id == l.id)
            .map_or(l.transpose, |(_, v)| v);
        let x = head.x
            + super::CHIP
            + 6.0
            + estimated_text_width(&layer_name(t, chosen), FontSize::Body)
            + ITEM_GAP
            + estimated_text_width("Transpose", FontSize::Small)
            + LABEL_GAP;
        let w = (estimated_text_width(&transpose_text(v), FontSize::Small) + 2.0 * FIELD_PAD)
            .max(FIELD_W);
        let y = head.y + (head.h - FIELD_H) / 2.0;
        (Rect::new(x, y, w, FIELD_H), v)
    });
    Keys {
        panel,
        head,
        map,
        board,
        boxes,
        chosen,
        transpose,
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

/// What in the figure is under the point (the map, 5 round it and down to the keyboard): the chosen box's sides first, then the smallest box.
pub fn hit(k: &Keys, x: f32, y: f32) -> Option<(NodeId, Option<Edge>)> {
    let m = k.map;
    let near = Rect::new(
        m.x - REACH,
        m.y - REACH,
        m.w + 2.0 * REACH,
        m.h + REACH + GAP,
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

/// The ranges after the side `e` was dragged to the point: a side does not pass the other. A
/// key side snaps to the nearest key boundary of the keyboard.
pub fn dragged(from: Ranges, e: Edge, map: Rect, board: &Keyboard, x: f32, y: f32) -> Ranges {
    let key = |x: f32| f32::from(board.nearest_edge(x));
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
