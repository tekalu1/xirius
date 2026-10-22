//! What the settings draw, as a list built without a window: the same list is
//! painted, hit-tested and read by the tests.
//!
//! Text is laid out with [`crate::browser::text_width`] (the same estimate the
//! browser uses), so a list built in a test is the list the window paints.

use gpui::{Bounds, Corners, Hsla, point, px, size};
use xirius_design::{FontSize, Radius, Weight};

use super::Do;
use crate::draw::{Align, Pen};
use crate::paint::Colors;
use crate::studio::Rect;

/// One thing to paint.
#[derive(Clone)]
pub enum Op {
    Fill(Rect, Hsla),
    Round(Rect, Hsla, Radius),
    Text(Text),
    Icon(&'static str, Rect, Hsla),
    /// An icon turned by a fraction of a round (a spinner).
    IconTurned(&'static str, Rect, Hsla, f32),
    /// The soft shadow all round a floating panel (UI-007).
    Shadow(Rect),
    /// What is painted inside a rectangle only (a list that scrolls).
    Mask(Rect, Vec<Op>),
    /// The common dropdown's closed box.
    Field {
        r: Rect,
        label: String,
        hot: bool,
        enabled: bool,
    },
}

#[derive(Clone)]
pub struct Text {
    pub s: String,
    pub size: FontSize,
    pub weight: Weight,
    pub color: Hsla,
    pub r: Rect,
    pub align: Align,
    /// Digits keep their width (numbers that change).
    pub digits: bool,
}

/// A place a press does something.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub r: Rect,
    /// The part of the screen the item is seen in (a scrolled list's box).
    pub clip: Rect,
    pub act: Do,
    pub tip: Option<String>,
    /// What a screen reader is told about its state (a switch, one choice of several).
    pub pick: Option<Pick>,
}

/// The state of an item that is on or off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    /// An on/off switch.
    Switch(bool),
    /// One choice of a group (a segmented control).
    Choice(bool),
}

impl Item {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.r.contains(x, y) && self.clip.contains(x, y)
    }
}

/// The list: what to paint and where a press goes.
#[derive(Clone, Default)]
pub struct Scene {
    pub ops: Vec<Op>,
    pub items: Vec<Item>,
    /// The tip of the item under the pointer: where, and what it says.
    pub tip: Option<(Rect, String)>,
    /// The text field that has the keyboard, for the IME's window.
    pub field: Option<Rect>,
    /// Every text put on the screen (the tests read it).
    pub texts: Vec<String>,
    /// The modal, and the part of it that scrolls.
    pub frame: Rect,
    pub body: Rect,
    /// How far the body can be scrolled down.
    pub scroll_max: f32,
}

impl Scene {
    /// The item a press at (x, y) reaches: the one drawn last on top.
    pub fn item_at(&self, x: f32, y: f32) -> Option<&Item> {
        self.items.iter().rev().find(|i| i.contains(x, y))
    }

    /// The first item that does this (the tests aim at it).
    pub fn find(&self, act: &Do) -> Option<&Item> {
        self.items.iter().find(|i| i.act == *act)
    }

    /// Every text put on the screen with the box it is in (the fields' shown value too).
    pub fn text_boxes(&self) -> Vec<(&str, Rect)> {
        fn each<'a>(ops: &'a [Op], out: &mut Vec<(&'a str, Rect)>) {
            for op in ops {
                match op {
                    Op::Text(t) => out.push((&t.s, t.r)),
                    Op::Field { r, label, .. } => out.push((label, *r)),
                    Op::Mask(_, inner) => each(inner, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        each(&self.ops, &mut out);
        out
    }

    pub fn has_text(&self, s: &str) -> bool {
        self.texts.iter().any(|t| t.contains(s))
    }
}

/// Builds a [`Scene`].
pub struct B<'a> {
    pub c: &'a Colors,
    pub ops: Vec<Op>,
    stack: Vec<(Rect, Vec<Op>, Rect)>,
    pub scene: Scene,
    pub clip: Rect,
    pub pointer: Option<(f32, f32)>,
    /// What cannot be used now is drawn faint and cannot be pressed.
    pub dim: bool,
}

/// How faint a part that cannot be used now is drawn.
const DIM: f32 = 0.45;

impl<'a> B<'a> {
    pub fn new(c: &'a Colors, screen: Rect, pointer: Option<(f32, f32)>) -> B<'a> {
        B {
            c,
            ops: Vec::new(),
            stack: Vec::new(),
            scene: Scene::default(),
            clip: screen,
            pointer,
            dim: false,
        }
    }

    pub fn finish(mut self) -> Scene {
        while !self.stack.is_empty() {
            self.pop_mask();
        }
        self.scene.ops = self.ops;
        self.scene
    }

    /// Is the pointer over `r` (and is `r` seen)?
    pub fn hot(&self, r: Rect) -> bool {
        self.pointer
            .is_some_and(|(x, y)| r.contains(x, y) && self.clip.contains(x, y))
    }

    pub fn fade(&self, mut h: Hsla) -> Hsla {
        if self.dim {
            h.a *= DIM;
        }
        h
    }

    pub fn fill(&mut self, r: Rect, color: Hsla) {
        let color = self.fade(color);
        self.ops.push(Op::Fill(r, color));
    }

    pub fn round(&mut self, r: Rect, color: Hsla, radius: Radius) {
        let color = self.fade(color);
        self.ops.push(Op::Round(r, color, radius));
    }

    /// A ring of `width` round `r` on a face: two rounded boxes, no stroke.
    pub fn ring(&mut self, r: Rect, ring: Hsla, face: Hsla, radius: Radius, width: f32) {
        self.round(r, ring, radius);
        self.round(
            Rect::new(
                r.x + width,
                r.y + width,
                r.w - width * 2.0,
                r.h - width * 2.0,
            ),
            face,
            radius,
        );
    }

    /// One line of text; gives its width.
    pub fn text(
        &mut self,
        s: &str,
        size: FontSize,
        weight: Weight,
        color: Hsla,
        r: Rect,
        align: Align,
    ) -> f32 {
        self.text_with(s, size, weight, color, r, align, false)
    }

    pub fn digits(
        &mut self,
        s: &str,
        size: FontSize,
        weight: Weight,
        color: Hsla,
        r: Rect,
        align: Align,
    ) -> f32 {
        self.text_with(s, size, weight, color, r, align, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn text_with(
        &mut self,
        s: &str,
        size: FontSize,
        weight: Weight,
        color: Hsla,
        r: Rect,
        align: Align,
        digits: bool,
    ) -> f32 {
        let color = self.fade(color);
        self.scene.texts.push(s.to_string());
        self.ops.push(Op::Text(Text {
            s: s.to_string(),
            size,
            weight,
            color,
            r,
            align,
            digits,
        }));
        width(s, size)
    }

    pub fn icon(&mut self, name: &'static str, r: Rect, color: Hsla) {
        let color = self.fade(color);
        self.ops.push(Op::Icon(name, r, color));
    }

    /// `name` turned `turn` of a round (0..1): the spinner of what is being read.
    pub fn icon_turned(&mut self, name: &'static str, r: Rect, color: Hsla, turn: f32) {
        let color = self.fade(color);
        self.ops.push(Op::IconTurned(name, r, color, turn));
    }

    pub fn shadow(&mut self, r: Rect) {
        self.ops.push(Op::Shadow(r));
    }

    pub fn field(&mut self, r: Rect, label: &str, enabled: bool) {
        let enabled = enabled && !self.dim;
        let hot = self.hot(r);
        self.scene.texts.push(label.to_string());
        self.ops.push(Op::Field {
            r,
            label: label.to_string(),
            hot,
            enabled,
        });
    }

    /// A press on `r` does `act`; `tip` shows while the pointer is on it.
    /// Nothing happens on a faint part.
    pub fn item(&mut self, r: Rect, act: Do, tip: Option<String>) {
        self.item_pick(r, act, tip, None);
    }

    /// [`item`](Self::item) that says whether it is on (for a screen reader).
    pub fn item_pick(&mut self, r: Rect, act: Do, tip: Option<String>, pick: Option<Pick>) {
        if self.hot(r)
            && let Some(t) = &tip
        {
            self.scene.tip = Some((r, t.clone()));
        }
        if self.dim {
            return;
        }
        self.scene.items.push(Item {
            r,
            clip: self.clip,
            act,
            tip,
            pick,
        });
    }

    /// A tip with no press (a lock, a sign).
    pub fn tip(&mut self, r: Rect, tip: &str) {
        if self.hot(r) {
            self.scene.tip = Some((r, tip.to_string()));
        }
    }

    /// From here what is painted is cut to `r`, and seen only there.
    pub fn push_mask(&mut self, r: Rect) {
        let ops = std::mem::take(&mut self.ops);
        let clip = self.clip;
        self.stack.push((r, ops, clip));
        self.clip = intersect(clip, r);
    }

    pub fn pop_mask(&mut self) {
        let Some((r, outer, clip)) = self.stack.pop() else {
            return;
        };
        let inner = std::mem::replace(&mut self.ops, outer);
        self.ops.push(Op::Mask(r, inner));
        self.clip = clip;
    }
}

pub fn width(s: &str, size: FontSize) -> f32 {
    crate::browser::text_width(s, size)
}

pub fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let r = a.right().min(b.right());
    let bt = a.bottom().min(b.bottom());
    Rect::new(x, y, (r - x).max(0.0), (bt - y).max(0.0))
}

/// Paints the list.
pub fn paint(pen: &mut Pen, ops: &[Op]) {
    for op in ops {
        match op {
            Op::Fill(r, c) => pen.fill(*r, *c),
            Op::Round(r, c, radius) => pen.round(*r, *c, *radius),
            Op::Text(t) => {
                if t.digits {
                    pen.digits(&t.s, t.size, t.weight, t.color, t.r, t.align);
                } else {
                    pen.text(&t.s, t.size, t.weight, t.color, t.r, t.align);
                }
            }
            Op::Icon(name, r, c) => pen.icon(name, *r, *c),
            Op::IconTurned(name, r, c, turn) => pen.icon_turned(name, *r, *c, *turn),
            Op::Shadow(r) => {
                let at = Bounds::new(
                    point(px(pen.origin.0 + r.x), px(pen.origin.1 + r.y)),
                    size(px(r.w), px(r.h)),
                );
                pen.window
                    .paint_drop_shadows(at, Corners::all(px(Radius::M.px())), &pen.c.float);
            }
            Op::Mask(r, inner) => pen.mask(*r, |pen| paint(pen, inner)),
            Op::Field {
                r,
                label,
                hot,
                enabled,
            } => crate::dropdown::field(
                pen,
                *r,
                label,
                crate::dropdown::Face::Raised,
                *hot,
                *enabled,
            ),
        }
    }
}
