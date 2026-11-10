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

#[cfg(test)]
mod keyboard_tests {
    use super::*;

    #[test]
    fn piano_keeps_targets_and_every_note_reachable_at_narrow_widths() {
        for width in [316.0, 320.0, 360.0, 1280.0] {
            let k = Keyboard::new(
                Rect::new(12.0, 20.0, width, Keyboard::HEIGHT),
                0.0,
                KeySpec::SAMPLER,
            );
            assert_eq!(k.keys.len(), usize::from(HIGH - LOW + 1));
            for key in &k.keys {
                assert_eq!(key.r.w, if key.black { 24.0 } else { 39.0 });
                let scroll = (key.r.x - k.view.x - width / 2.0).max(0.0);
                let moved = Keyboard::new(k.view, scroll, KeySpec::SAMPLER);
                let target = moved.keys.iter().find(|p| p.pitch == key.pitch).unwrap();
                let x = target.r.x + target.r.w / 2.0;
                let y = target.r.y + if target.black { 20.0 } else { 70.0 };
                assert_eq!(moved.key_at(x, y), Some(key.pitch));
            }
            for x in 0..width.min(k.extent) as usize {
                assert!(
                    k.key_at(k.view.x + x as f32 + 0.5, 90.0).is_some(),
                    "a seam must not release a drag"
                );
            }
            assert_eq!(k.key_at(k.view.x - 1.0, 90.0), None);
            assert_eq!(k.key_at(k.view.x + 2.0, k.view.y + 93.0), None);
        }
    }

    #[test]
    fn scrollbar_drag_reaches_both_ends_without_shrinking_keys() {
        let k = Keyboard::new(
            Rect::new(10.0, 10.0, 320.0, Keyboard::HEIGHT),
            0.0,
            KeySpec::SAMPLER,
        );
        let thumb = k.thumb();
        let drag = k.scroll_press(thumb.x + 5.0, thumb.y + 2.0).unwrap();
        assert_eq!(drag.position(-100.0), 0.0);
        assert_eq!(drag.position(10000.0), k.extent - k.view.w);
        assert!(k.scroll_press(20.0, 30.0).is_none());
    }

    /// The Sampler's keys as the code drew them before `Keyboard` took a [`KeySpec`]: view
    /// (12, 20, 700), unscrolled. White keys `13 + 40 i`, black keys by pitch (x only; all
    /// 24 x 58 at y 21).
    const SAMPLER_WHITES: [u8; 29] = [
        36, 38, 40, 41, 43, 45, 47, 48, 50, 52, 53, 55, 57, 59, 60, 62, 64, 65, 67, 69, 71, 72, 74,
        76, 77, 79, 81, 83, 84,
    ];
    const SAMPLER_BLACKS: [(u8, f32); 20] = [
        (37, 40.5),
        (39, 80.5),
        (42, 160.5),
        (44, 200.5),
        (46, 240.5),
        (49, 320.5),
        (51, 360.5),
        (54, 440.5),
        (56, 480.5),
        (58, 520.5),
        (61, 600.5),
        (63, 640.5),
        (66, 720.5),
        (68, 760.5),
        (70, 800.5),
        (73, 880.5),
        (75, 920.5),
        (78, 1000.5),
        (80, 1040.5),
        (82, 1080.5),
    ];

    #[test]
    fn the_sampler_keyboard_looks_as_it_did() {
        let k = Keyboard::new(
            Rect::new(12.0, 20.0, 700.0, Keyboard::HEIGHT),
            0.0,
            KeySpec::SAMPLER,
        );
        assert_eq!(k.keys.len(), 49);
        assert_eq!(k.extent, 1161.0);
        let whites: Vec<_> = k.keys.iter().filter(|k| !k.black).collect();
        assert_eq!(whites.len(), SAMPLER_WHITES.len());
        for (i, (key, pitch)) in whites.iter().zip(SAMPLER_WHITES).enumerate() {
            assert_eq!(key.pitch, pitch);
            assert_eq!(key.r, Rect::new(13.0 + 40.0 * i as f32, 21.0, 39.0, 91.0));
        }
        let blacks: Vec<_> = k.keys.iter().filter(|k| k.black).collect();
        assert_eq!(blacks.len(), SAMPLER_BLACKS.len());
        for (key, (pitch, x)) in blacks.iter().zip(SAMPLER_BLACKS) {
            assert_eq!((key.pitch, key.r), (pitch, Rect::new(x, 21.0, 24.0, 58.0)));
        }
        // Whites come first, then blacks: the paint order.
        assert!(k.keys.iter().position(|k| k.black) > k.keys.iter().rposition(|k| !k.black));
        assert_eq!(k.rail(), Rect::new(12.0, 116.0, 700.0, 6.0));
        let thumb = k.thumb();
        assert_eq!((thumb.x, thumb.y, thumb.h), (12.0, 116.0, 6.0));
        assert!((thumb.w - 422.04996).abs() < 1e-3);
        // Scrolled by a fraction: the keys move by exactly that much.
        let k = Keyboard::new(
            Rect::new(12.0, 20.0, 700.0, Keyboard::HEIGHT),
            100.5,
            KeySpec::SAMPLER,
        );
        assert_eq!(k.scroll, 100.5);
        assert_eq!(k.keys[0].r, Rect::new(-87.5, 21.0, 39.0, 91.0));
        assert_eq!(k.keys[30].r, Rect::new(-20.0, 21.0, 24.0, 58.0));
        // The view is the Sampler's: keys 92 high, then the rail.
        assert_eq!(Keyboard::view_height(92.0), Keyboard::HEIGHT);
    }

    #[test]
    fn the_black_keys_follow_the_white_pitch_and_the_key_height() {
        for (pitch, height, w, h) in [
            (40.0, 92.0, 24.0, 58.0),
            ((704.0 - 1.0) / 75.0, 64.0, 6.0, 40.0),
            ((984.0 - 1.0) / 75.0, 64.0, 8.0, 40.0),
            ((1184.0 - 1.0) / 75.0, 64.0, 9.0, 40.0),
        ] {
            let spec = KeySpec {
                pitch,
                height,
                ..KeySpec::SAMPLER
            };
            let k = Keyboard::new(Rect::new(0.0, 0.0, 2000.0, 80.0), 0.0, spec);
            for key in k.keys.iter().filter(|k| k.black) {
                assert_eq!((key.r.w, key.r.h), (w, h), "pitch {pitch}");
            }
            for key in k.keys.iter().filter(|k| !k.black) {
                assert_eq!(key.r.h, height - 1.0);
            }
        }
    }

    #[test]
    fn the_full_range_fits_its_width_and_every_note_and_pixel_is_a_key() {
        for width in [704.0, 984.0, 1184.0] {
            let spec = KeySpec::full(width, 64.0);
            assert_eq!((spec.low, spec.high), (0, 127));
            let view = Rect::new(8.0, 30.0, width, Keyboard::view_height(64.0));
            let k = Keyboard::new(view, 0.0, spec);
            assert_eq!(k.keys.len(), 128);
            assert_eq!(k.keys.iter().filter(|k| !k.black).count(), 75);
            assert_eq!(k.extent, width, "no scrolling at {width}");
            assert!(k.scroll_press(view.x + 1.0, k.rail().y).is_none());
            // The white keys tile the width a line apart, on whole pixels.
            let whites: Vec<_> = k.keys.iter().filter(|k| !k.black).collect();
            assert_eq!(whites[0].r.x, view.x + 1.0);
            for pair in whites.windows(2) {
                assert_eq!(pair[1].r.x, pair[0].r.right() + 1.0);
            }
            for key in &whites {
                assert_eq!(key.r.x.fract(), 0.0);
                assert_eq!(key.r.w.fract(), 0.0);
                assert!((key.r.w - (spec.pitch - 1.0)).abs() < 1.0);
            }
            assert_eq!(
                whites[74].r.right() + 1.0,
                view.x + width,
                "the frame ends with the view"
            );
            for key in &k.keys {
                let (x, y) = (
                    key.r.x + key.r.w / 2.0,
                    key.r.y + if key.black { 5.0 } else { key.r.h - 5.0 },
                );
                assert_eq!(k.key_at(x, y), Some(key.pitch), "note {}", key.pitch);
            }
            for black in k.keys.iter().filter(|k| k.black) {
                assert_eq!(black.r.x.fract(), 0.0, "black keys land on whole pixels");
                let below = k.keys.iter().find(|k| k.pitch == black.pitch - 1).unwrap();
                let centre = black.r.x + black.r.w / 2.0;
                assert!((centre - (below.r.right() + 0.5)).abs() <= 0.5);
            }
            for x in 0..width as usize {
                assert!(k.key_at(view.x + x as f32 + 0.5, view.y + 60.0).is_some());
            }
            assert_eq!(
                k.key_at(view.x + 5.0, view.y + 64.0),
                None,
                "the gap is no key"
            );
        }
    }

    #[test]
    fn a_narrow_view_of_the_full_range_scrolls_at_the_least_width() {
        let spec = KeySpec::full(500.0, 64.0);
        assert_eq!(spec, KeySpec::full(KeySpec::LEAST_W, 64.0));
        let view = Rect::new(0.0, 0.0, 500.0, Keyboard::view_height(64.0));
        let k = Keyboard::new(view, 0.0, spec);
        assert_eq!(k.extent, 704.0);
        let drag = k.scroll_press(k.thumb().x + 2.0, k.rail().y + 2.0).unwrap();
        assert_eq!(drag.position(10000.0), 204.0);
        let end = Keyboard::new(view, 10000.0, spec);
        assert_eq!(end.scroll, 204.0);
        assert!(end.key_at(499.0, 30.0).is_some());
    }

    #[test]
    fn a_held_key_is_filled_the_same_at_any_size() {
        assert_eq!(Face::of(true, false), Face::of(true, true));
        assert_eq!(Face::of(true, false), Face::Held);
        assert_eq!(Face::of(false, true), Face::Black);
        assert_eq!(Face::of(false, false), Face::White);
    }
}
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

/// What a piano shows: the notes `low..=high`, one white key's pitch (its width with the line
/// before it) and the keys' height. The black keys follow from these (a black key's width is
/// 0.6 of the white pitch and its height 58/91 of a white key's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeySpec {
    pub low: u8,
    pub high: u8,
    pub pitch: f32,
    pub height: f32,
}

impl KeySpec {
    /// The Sampler's keys: C1 to C5, a white pitch of 40, keys 92 high.
    pub const SAMPLER: Self = Self {
        low: LOW,
        high: HIGH,
        pitch: 40.0,
        height: 92.0,
    };

    /// The notes `0..=127` over `width` (a width under [`Self::LEAST_W`] is read as it): the
    /// white pitch makes 75 white keys fill it.
    pub fn full(width: f32, height: f32) -> Self {
        Self {
            low: 0,
            high: 127,
            pitch: (width.max(Self::LEAST_W) - 1.0) / 75.0,
            height,
        }
    }

    /// The least width the full range is spread over; a narrower view scrolls.
    pub const LEAST_W: f32 = 704.0;

    fn whites(self) -> usize {
        (self.low..=self.high).filter(|&p| !is_black_key(p)).count()
    }

    /// The left edge of the white key `i` (the line before it), snapped to a pixel.
    fn edge(self, i: usize) -> f32 {
        (i as f32 * self.pitch).round()
    }

    /// A black key's width and height.
    fn black(self) -> (f32, f32) {
        (
            (self.pitch * 0.6).round().max(1.0),
            ((self.height - 1.0) * 58.0 / 91.0).round(),
        )
    }
}

/// A playable piano keeps its targets large; the viewport scrolls across its range.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyboard {
    pub view: Rect,
    pub keys: Vec<Key>,
    pub scroll: f32,
    pub extent: f32,
    pub spec: KeySpec,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollDrag {
    rail: Rect,
    thumb: f32,
    offset: f32,
    range: f32,
}

impl ScrollDrag {
    pub fn position(self, x: f32) -> f32 {
        let travel = self.rail.w - self.thumb;
        if travel <= 0.0 {
            return 0.0;
        }
        ((x - self.rail.x - self.offset) / travel).clamp(0.0, 1.0) * self.range
    }
}

impl Keyboard {
    /// The Sampler's view: its keys and the rail under them.
    pub const HEIGHT: f32 = 104.0;
    pub const PITCH: f32 = 40.0;

    /// The height of a view of keys `key_h` high: the gap under them and the rail.
    pub fn view_height(key_h: f32) -> f32 {
        key_h + 12.0
    }

    pub fn new(view: Rect, scroll: f32, spec: KeySpec) -> Self {
        let whites = spec.whites();
        let extent = spec.edge(whites) + 1.0;
        let scroll = scroll.clamp(0.0, (extent - view.w).max(0.0));
        let (x0, y0) = (view.x - scroll, view.y);
        let white_h = spec.height - 1.0;
        let mut keys = Vec::new();
        for (i, pitch) in (spec.low..=spec.high)
            .filter(|&p| !is_black_key(p))
            .enumerate()
        {
            let (a, b) = (spec.edge(i), spec.edge(i + 1));
            keys.push(Key {
                pitch,
                r: Rect::new(x0 + 1.0 + a, y0 + 1.0, b - a - 1.0, white_h),
                black: false,
            });
        }
        let (black_w, black_h) = spec.black();
        for pitch in (spec.low..=spec.high).filter(|&p| is_black_key(p)) {
            // A black key sits on the line after the white key below it.
            let Some(below) = keys.iter().find(|k| k.pitch == pitch - 1).map(|k| k.r) else {
                continue;
            };
            let line = below.right() + 0.5;
            let mut x = line - black_w / 2.0;
            if spec.pitch.fract() != 0.0 {
                // Keys of a fractional pitch land on whole pixels; a whole pitch keeps its
                // half-pixel centring (the Sampler's).
                x = x.floor();
            }
            keys.push(Key {
                pitch,
                r: Rect::new(x, below.y, black_w, black_h),
                black: true,
            });
        }
        Self {
            view,
            keys,
            scroll,
            extent,
            spec,
        }
    }

    /// Whether a point is over the keys (not the rail under them).
    pub fn in_keys(&self, x: f32, y: f32) -> bool {
        self.view.contains(x, y) && y < self.view.y + self.spec.height
    }

    pub fn key_at(&self, x: f32, y: f32) -> Option<u8> {
        if !self.in_keys(x, y) {
            return None;
        }
        // Fill the one-pixel seams with the adjoining white key. Black keys win first.
        self.keys
            .iter()
            .rev()
            .find(|k| {
                let mut r = k.r;
                if !k.black {
                    r.x -= 1.0;
                    r.w += 2.0;
                }
                r.contains(x, y)
            })
            .map(|k| k.pitch)
    }

    pub fn rail(&self) -> Rect {
        Rect::new(
            self.view.x,
            self.view.y + self.spec.height + 4.0,
            self.view.w,
            6.0,
        )
    }

    pub fn scroll_press(&self, x: f32, y: f32) -> Option<ScrollDrag> {
        let rail = self.rail();
        let hit = Rect::new(rail.x, rail.y - 4.0, rail.w, rail.h + 8.0);
        if self.extent <= self.view.w || !hit.contains(x, y) {
            return None;
        }
        let thumb = self.thumb();
        Some(ScrollDrag {
            rail,
            thumb: thumb.w,
            range: self.extent - self.view.w,
            offset: if x >= thumb.x && x <= thumb.right() {
                x - thumb.x
            } else {
                thumb.w / 2.0
            },
        })
    }

    pub fn thumb(&self) -> Rect {
        let rail = self.rail();
        let width = (rail.w * self.view.w / self.extent).min(rail.w).max(24.0);
        let range = (self.extent - self.view.w).max(0.0);
        let x = if range > 0.0 {
            self.scroll / range * (rail.w - width)
        } else {
            0.0
        };
        Rect::new(rail.x + x, rail.y, width, rail.h)
    }

    pub fn at(self, dx: f32, dy: f32) -> Self {
        Self::new(
            Rect::new(self.view.x + dx, self.view.y + dy, self.view.w, self.view.h),
            self.scroll,
            self.spec,
        )
    }

    pub fn paint(&self, pen: &mut Pen, held: &[u8]) {
        pen.mask(self.view, |pen| {
            paint_held_keys(pen, &self.keys, held, true)
        });
        if self.extent > self.view.w {
            pen.round(self.rail(), pen.c.surface_3, Radius::Xs);
            pen.round(self.thumb(), pen.c.scroll_thumb[0], Radius::Xs);
        }
    }
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
    paint_held_keys(pen, keys, &down.into_iter().collect::<Vec<_>>(), names);
}

/// How a key is filled: a held key one way, whatever its size or colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Held,
    Black,
    White,
}

impl Face {
    pub fn of(held: bool, black: bool) -> Self {
        match (held, black) {
            (true, _) => Self::Held,
            (false, true) => Self::Black,
            (false, false) => Self::White,
        }
    }
}

/// A white key this wide or wider has its C centred under it; a narrower one has it at its
/// left, over the next keys.
const NAME_CENTRED_W: f32 = 24.0;

fn paint_held_keys(pen: &mut Pen, keys: &[Key], down: &[u8], names: bool) {
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
    let paint = |pen: &mut Pen, k: &Key| {
        let face = match Face::of(down.contains(&k.pitch), k.black) {
            Face::Held => c.fill,
            Face::Black => c.key_black,
            Face::White => c.key_white,
        };
        pen.round(k.r, face, Radius::Xs);
    };
    // The white keys, then their names (a narrow key's runs over the next), then the black.
    for k in keys.iter().filter(|k| !k.black) {
        paint(pen, k);
    }
    for k in keys
        .iter()
        .filter(|k| names && !k.black && k.pitch % 12 == 0)
    {
        let ink = if down.contains(&k.pitch) {
            c.on_fill
        } else {
            c.key_black
        };
        let (r, align) = if k.r.w + 1.0 >= NAME_CENTRED_W {
            (
                Rect::new(k.r.x - 4.0, k.r.bottom() - 14.0, k.r.w + 8.0, 12.0),
                Align::Center,
            )
        } else {
            (
                Rect::new(k.r.x + 2.0, k.r.bottom() - 14.0, 32.0, 12.0),
                Align::Left,
            )
        };
        pen.text(
            &crate::roll::pitch_name(k.pitch),
            FontSize::Micro,
            Weight::Regular,
            ink,
            r,
            align,
        );
    }
    for k in keys.iter().filter(|k| k.black) {
        paint(pen, k);
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
