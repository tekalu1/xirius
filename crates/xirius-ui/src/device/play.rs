//! The keys of a Sampler's window and the pads of a Drum Kit's (PLG-066): a press plays the
//! note on the device's track the way its input does (not recorded), and letting go, the
//! pointer leaving it and the window closing end it.

use xirius_design::{FontSize, Radius, Weight};

use super::layout::PAD;
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
/// The gap between the pads, and how tall a pad is of its width.
pub const GAP: f32 = 4.0;
/// A window this narrow or less hides the keys' names.
const NAMES_W: f32 = 520.0;

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

/// The keyboard, `down` the key held: the white keys' C named while the window is wider
/// than [`NAMES_W`].
pub fn paint_keys(pen: &mut Pen, keys: &[Key], down: Option<u8>) {
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
    let names = frame.w + 2.0 * PAD > NAMES_W;
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
pub fn paint_pads(pen: &mut Pen, pads: &[Key], down: Option<u8>, hot: Option<u8>) {
    let c = pen.c;
    for k in pads {
        let lit = down == Some(k.pitch);
        let face = if lit {
            c.fill
        } else if hot == Some(k.pitch) {
            c.clip
        } else {
            c.surface_3
        };
        pen.round(k.r, face, Radius::S);
        // Its name at the top left, its note at the top right.
        let text = Rect::new(k.r.x + 6.0, k.r.y + 5.0, k.r.w - 12.0, 14.0);
        pen.text(
            pad_name(k.pitch),
            FontSize::Micro,
            Weight::Semibold,
            if lit { c.on_fill } else { c.ink_strong },
            text,
            Align::Left,
        );
        pen.text(
            &crate::roll::pitch_name(k.pitch),
            FontSize::Micro,
            Weight::Regular,
            if lit { c.on_fill } else { c.ink },
            text,
            Align::Right,
        );
    }
}
