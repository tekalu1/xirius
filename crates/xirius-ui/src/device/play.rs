//! The keys of a Sampler's window and the pads of a Drum Kit's (PLG-066): a press plays the
//! note on the device's track the way its input does (not recorded), and letting go, the
//! pointer leaving it and the window closing end it.

use xirius_design::{FontSize, Radius, Weight};

use super::layout::{Mark, PAD, Spot};
use crate::draw::{Align, Pen};
use crate::notes::is_black_key;
use crate::studio::Rect;

/// The Sampler's keyboard: C1 to C5 (C3 = 60, UI-335).
pub const LOW: u8 = 36;
pub const HIGH: u8 = 84;
/// The note of the first pad (A1, bottom left): pad `n` plays `FIRST_PAD + n`.
pub const FIRST_PAD: u8 = 36;
/// The pads of a bank: four rows of four.
pub const PADS: usize = 16;
pub const KEYS_H: f32 = 56.0;
/// A black key's size, over the line between two white keys.
const BLACK_W: f32 = 8.0;
const BLACK_H: f32 = 34.0;
/// The gap between the pads.
pub const GAP: f32 = 4.0;
/// A body whose content (less its padding) is this wide or narrower hides the keys' names:
/// the mock's `@container spb (max-width: 520px)` on `.sp-body`, whose width is its content's.
const NAMES_W: f32 = 520.0;

/// Whether the keys of a body `w` wide show the white keys' C named: by the body's content,
/// as the rest of the narrow shape is (the fourth review's low 2).
pub fn key_names(w: f32) -> bool {
    w - 2.0 * PAD > NAMES_W
}

/// A key or a pad: the note it plays and where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub pitch: u8,
    pub r: Rect,
    pub black: bool,
}

/// The keys from [`LOW`] to [`HIGH`] in the frame `r`: the white ones a line apart, then the
/// black ones over the lines.
pub fn keys(r: Rect) -> Vec<Key> {
    let whites = (LOW..=HIGH).filter(|&p| !is_black_key(p)).count() as f32;
    let w = (r.w - 1.0) / whites;
    let mut out = Vec::new();
    let mut x = r.x + 1.0;
    for pitch in (LOW..=HIGH).filter(|&p| !is_black_key(p)) {
        out.push(Key {
            pitch,
            r: Rect::new(x, r.y + 1.0, w - 1.0, r.h - 1.0),
            black: false,
        });
        x += w;
    }
    for pitch in (LOW..=HIGH).filter(|&p| is_black_key(p)) {
        // A black key sits on the line after the white key below it.
        let below = out.iter().find(|k| k.pitch == pitch - 1).map(|k| k.r);
        if let Some(b) = below {
            let line = b.right() + 0.5;
            out.push(Key {
                pitch,
                r: Rect::new(line - BLACK_W / 2.0, b.y, BLACK_W, BLACK_H),
                black: true,
            });
        }
    }
    out
}

/// The pads of the bank `bank` (A is 0) across `r`, the first at the bottom left and on to
/// the right. Pad `n` of the kit plays `FIRST_PAD + n`, from 0 again past 127.
pub fn pads(r: Rect, bank: usize) -> Vec<Key> {
    let w = (r.w - 3.0 * GAP) / 4.0;
    let h = (r.h - 3.0 * GAP) / 4.0;
    (0..PADS)
        .map(|n| {
            let (col, row) = ((n % 4) as f32, (3 - n / 4) as f32);
            Key {
                pitch: ((usize::from(FIRST_PAD) + bank * PADS + n) % 128) as u8,
                r: Rect::new(r.x + col * (w + GAP), r.y + row * (h + GAP), w, h),
                black: false,
            }
        })
        .collect()
}

/// The key under a point: a black key before the white one under it.
pub fn key_at(keys: &[Key], x: f32, y: f32) -> Option<u8> {
    keys.iter()
        .rev()
        .find(|k| k.r.contains(x, y))
        .map(|k| k.pitch)
}

/// The pad a note plays (A1 is 0).
pub fn pad_of(pitch: u8) -> usize {
    (usize::from(pitch) + 128 - usize::from(FIRST_PAD)) % 128
}

/// A pad's name: its bank and number ("A1", "B5").
pub fn pad_name(pitch: u8) -> &'static str {
    xirius_params::PAD_NAMES[pad_of(pitch)]
}

/// The keyboard, `down` the key held: the white keys' C named while `names` ([`key_names`]).
pub fn paint_keys(pen: &mut Pen, keys: &[Key], down: Option<u8>, names: bool) {
    let (Some(first), Some(last)) = (keys.first(), keys.iter().rfind(|k| !k.black)) else {
        return;
    };
    let c = pen.c;
    let frame = Rect::new(
        first.r.x - 1.0,
        first.r.y - 1.0,
        last.r.right() + 2.0 - first.r.x,
        first.r.h + 1.0,
    );
    pen.round(frame, c.key_black, Radius::Clip);
    for k in keys {
        let lit = down == Some(k.pitch);
        let face = match (lit, k.black) {
            (true, _) => c.fill,
            (false, true) => c.key_black,
            (false, false) => c.key_white,
        };
        pen.round(k.r, face, Radius::Xs);
        if names && !k.black && k.pitch % 12 == 0 {
            pen.text(
                &crate::roll::pitch_name(k.pitch),
                FontSize::Micro,
                Weight::Regular,
                if lit { c.on_fill } else { c.key_black },
                Rect::new(k.r.x - 4.0, k.r.bottom() - 14.0, k.r.w + 8.0, 12.0),
                Align::Center,
            );
        }
    }
}

/// The pads: `down` the one held, `hot` the one under the pointer.
pub fn paint_pads(pen: &mut Pen, pads: &[Key], spots: &[Spot], lit: &[u8], hot: Option<u8>) {
    let c = pen.c;
    let none = Spot::new(Rect::default(), Mark::Pad(0), "");
    for k in pads {
        let s = spots
            .iter()
            .find(|s| s.mark == Mark::Pad(pad_of(k.pitch)))
            .unwrap_or(&none);
        let lit = lit.contains(&k.pitch);
        let over = hot == Some(k.pitch);
        // A pad that has files is lighter than an empty one; a press lights it a moment.
        let face = match (lit, s.ghost, s.dot) {
            (true, _, _) => c.fill,
            (_, true, _) => c.surface_1,
            (_, _, true) if over => c.clip,
            (_, _, true) => c.surface_3,
            _ if over => c.surface_3,
            _ => c.surface_2,
        };
        pen.round(k.r, face, Radius::S);
        if s.ghost {
            pen.outline(k.r, c.ring, Radius::S, 2.0);
        } else if s.on {
            pen.outline(k.r, c.ink_blue, Radius::S, 2.0);
        }
        // What does not sound is fainter: its name and its sample's, in the ink of what is
        // off (no ink is see-through).
        let strong = if lit {
            c.on_fill
        } else if s.ghost {
            c.ink_blue
        } else if s.dim {
            c.scroll_thumb[0]
        } else {
            c.ink_strong
        };
        let soft = if lit { c.on_fill } else { c.ink };
        // Its name at the top left, its note at the top right.
        let text = Rect::new(k.r.x + 6.0, k.r.y + 5.0, k.r.w - 12.0, 14.0);
        pen.text(
            pad_name(k.pitch),
            FontSize::Micro,
            Weight::Semibold,
            strong,
            text,
            Align::Left,
        );
        pen.text(
            &crate::roll::pitch_name(k.pitch),
            FontSize::Micro,
            Weight::Regular,
            soft,
            text,
            Align::Right,
        );
        let r = k.r;
        if !s.text.is_empty() {
            let t = Rect::new(r.x + 6.0, r.y + r.h / 2.0 - 8.0, r.w - 12.0, 14.0);
            let name = pen.fit(&s.text, FontSize::Micro, Weight::Semibold, t.w);
            pen.text(
                &name,
                FontSize::Micro,
                Weight::Semibold,
                strong,
                t,
                Align::Left,
            );
        }
        if !s.sub.is_empty() {
            let t = Rect::new(r.x + 6.0, r.bottom() - 18.0, r.w / 2.0, 14.0);
            let ink = if s.ghost && !lit { c.ink_blue } else { soft };
            pen.text(
                &s.sub,
                FontSize::Micro,
                Weight::Regular,
                ink,
                t,
                Align::Left,
            );
        }
    }
}
