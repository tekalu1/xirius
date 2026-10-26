//! The pointer over the arrangement and the roll (UI-184): an OS shape for most places, and for
//! the tools the tool's own picture, drawn here and handed to the OS as the cursor itself, so
//! there is one pointer and not a pointer with a picture beside it.
//!
//! The pictures are white with a black outline, so they show on any background and in either
//! theme. Each has a working point (the pixel the click lands on): the pencil's tip, the line
//! of the scissors, the eraser's corner, the middle of the others. Where a tool has nothing to
//! act on, the pointer is the system's "not allowed" sign. `edit.cursor.crosshair` swaps the
//! pictures for the system's crosshair.

use gpui::CursorStyle;

use crate::roll::Cursor;
use crate::studio::{Geometry, Studio};

/// A tool's picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Pencil,
    Scissors,
    Eraser,
    Mute,
    Audition,
    Range,
}

impl Shape {
    pub const ALL: [Shape; 6] = [
        Shape::Pencil,
        Shape::Scissors,
        Shape::Eraser,
        Shape::Mute,
        Shape::Audition,
        Shape::Range,
    ];

    /// The working point in the 32-unit square the picture is drawn in.
    pub fn hot(self) -> (f32, f32) {
        match self {
            Shape::Pencil => (4.0, 26.0),
            Shape::Scissors | Shape::Mute | Shape::Audition | Shape::Range => (16.0, 16.0),
            Shape::Eraser => (4.0, 28.0),
        }
    }
}

/// What the window asks of the system for the pointer now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointer {
    /// The system's own shape: the one shown where there is no picture (or no way to show it).
    pub style: CursorStyle,
    /// The tool's picture, shown in place of `style` when there is one.
    pub shape: Option<Shape>,
}

impl Pointer {
    const fn style(style: CursorStyle) -> Pointer {
        Pointer { style, shape: None }
    }
}

/// The pointer for `cursor`. With `crosshair` the tools' pictures give way to the system's
/// crosshair (SET: ツールのカーソルを十字にする); the "not allowed" sign does not, it is not a tool.
pub fn pointer(cursor: Cursor, crosshair: bool) -> Pointer {
    let tool = |shape| {
        if crosshair {
            Pointer::style(CursorStyle::Crosshair)
        } else {
            Pointer {
                style: CursorStyle::Crosshair,
                shape: Some(shape),
            }
        }
    };
    match cursor {
        Cursor::Arrow => Pointer::style(CursorStyle::Arrow),
        Cursor::Move => Pointer::style(CursorStyle::ClosedHand),
        Cursor::ResizeH => Pointer::style(CursorStyle::ResizeLeftRight),
        Cursor::ResizeV => Pointer::style(CursorStyle::ResizeUpDown),
        Cursor::Cross => Pointer::style(CursorStyle::Crosshair),
        Cursor::Blocked => Pointer::style(CursorStyle::OperationNotAllowed),
        Cursor::Pencil => tool(Shape::Pencil),
        Cursor::Split => tool(Shape::Scissors),
        Cursor::Erase => tool(Shape::Eraser),
        Cursor::Mute => tool(Shape::Mute),
        Cursor::Audition => tool(Shape::Audition),
        Cursor::Range => tool(Shape::Range),
    }
}

/// The pointer over the window's body: the arrangement's shape where it has one, else the
/// roll's. Panels that cover the body (a pop-up, the palette, the settings) take the arrow.
pub fn of_studio(studio: &Studio, g: &Geometry) -> Pointer {
    if studio.popup.is_some()
        || studio.palette.is_some()
        || studio.settings.is_some()
        || studio.recovery.is_some()
        || studio.sound_preparing()
    {
        return Pointer::style(CursorStyle::Arrow);
    }
    if studio.edge_cursor() {
        return Pointer::style(CursorStyle::ResizeRow);
    }
    if studio.side_cursor() {
        return Pointer::style(CursorStyle::ResizeColumn);
    }
    if studio.pan.is_some() {
        return Pointer::style(CursorStyle::ClosedHand);
    }
    if studio.lock_hovered(g).is_some() {
        return Pointer::style(CursorStyle::OperationNotAllowed);
    }
    let cursor = crate::arrange::paint::cursor(studio).unwrap_or_else(|| {
        let ctx = studio.roll_ctx(g);
        studio.roll.cursor(ctx.as_ref())
    });
    pointer(cursor, studio.crosshair)
}

// ------------------------------------------------------------------ the pictures

/// A piece of a picture, in the 32-unit square.
enum Prim {
    /// A line of width `w` with round ends.
    Line((f32, f32), (f32, f32), f32),
    Poly(&'static [(f32, f32)]),
    /// A ring of outer radius `r` and width `w`.
    Ring((f32, f32), f32, f32),
    /// The right half of a ring.
    Arc((f32, f32), f32, f32),
}

impl Prim {
    fn covers(&self, (x, y): (f32, f32)) -> bool {
        match *self {
            Prim::Line(a, b, w) => segment_distance((x, y), a, b) <= w / 2.0,
            Prim::Poly(points) => inside(points, (x, y)),
            Prim::Ring(c, r, w) => {
                let d = (x - c.0).hypot(y - c.1);
                d <= r && d >= r - w
            }
            Prim::Arc(c, r, w) => {
                let d = (x - c.0).hypot(y - c.1);
                x >= c.0 && (d - (r - w / 2.0)).abs() <= w / 2.0
            }
        }
    }
}

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / len2).clamp(0.0, 1.0)
    };
    (p.0 - (a.0 + t * abx)).hypot(p.1 - (a.1 + t * aby))
}

/// Even-odd point in polygon.
fn inside(points: &[(f32, f32)], (x, y): (f32, f32)) -> bool {
    let mut odd = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[j]);
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            odd = !odd;
        }
        j = i;
    }
    odd
}

const SPEAKER: &[(f32, f32)] = &[
    (4.0, 12.0),
    (10.0, 12.0),
    (18.0, 5.0),
    (18.0, 27.0),
    (10.0, 20.0),
    (4.0, 20.0),
];
const PENCIL_TIP: &[(f32, f32)] = &[(13.5, 21.5), (8.5, 16.5), (4.0, 26.0)];
const ERASER: &[(f32, f32)] = &[(4.0, 28.0), (14.0, 28.0), (26.0, 16.0), (16.0, 16.0)];

/// What the picture is made of: the white body, and the black marks on it.
fn parts(shape: Shape) -> (Vec<Prim>, Vec<Prim>) {
    match shape {
        Shape::Pencil => (
            vec![
                Prim::Line((11.0, 19.0), (25.0, 5.0), 7.0),
                Prim::Poly(PENCIL_TIP),
            ],
            vec![Prim::Line((8.5, 16.5), (13.5, 21.5), 1.2)],
        ),
        Shape::Scissors => (
            vec![
                Prim::Line((16.0, 2.0), (16.0, 30.0), 2.4),
                Prim::Line((9.5, 25.0), (16.0, 12.0), 2.4),
                Prim::Line((22.5, 25.0), (16.0, 12.0), 2.4),
                Prim::Ring((9.5, 26.5), 5.0, 2.6),
                Prim::Ring((22.5, 26.5), 5.0, 2.6),
            ],
            vec![],
        ),
        Shape::Eraser => (
            vec![Prim::Poly(ERASER)],
            vec![Prim::Line((10.6, 21.4), (20.6, 21.4), 1.4)],
        ),
        Shape::Mute => (
            vec![
                Prim::Poly(SPEAKER),
                Prim::Line((22.0, 12.0), (28.0, 20.0), 2.4),
                Prim::Line((22.0, 20.0), (28.0, 12.0), 2.4),
            ],
            vec![],
        ),
        Shape::Audition => (
            vec![
                Prim::Poly(SPEAKER),
                Prim::Arc((18.0, 16.0), 7.0, 2.4),
                Prim::Arc((18.0, 16.0), 12.0, 2.4),
            ],
            vec![],
        ),
        Shape::Range => (
            vec![
                Prim::Line((16.0, 2.0), (16.0, 12.0), 2.4),
                Prim::Line((16.0, 20.0), (16.0, 30.0), 2.4),
                Prim::Line((2.0, 16.0), (12.0, 16.0), 2.4),
                Prim::Line((20.0, 16.0), (30.0, 16.0), 2.4),
            ],
            vec![],
        ),
    }
}

/// A picture drawn at `size` pixels square: straight-alpha BGRA, row by row from the top.
pub struct Picture {
    pub size: usize,
    pub hot: (usize, usize),
    pub bgra: Vec<u8>,
}

/// Draws `shape` at `size` pixels square (32 at the usual scale). Each pixel is the mean of
/// 4 × 4 samples; the outline is the body grown by a little over a unit.
pub fn picture(shape: Shape, size: usize) -> Picture {
    const SAMPLES: usize = 4;
    let unit = size as f32 / 32.0;
    let (body, marks) = parts(shape);
    let mut white = vec![0.0f32; size * size];
    let mut black = vec![0.0f32; size * size];
    for y in 0..size {
        for x in 0..size {
            let (mut w, mut k) = (0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let p = (
                        (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / unit,
                        (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / unit,
                    );
                    if body.iter().any(|b| b.covers(p)) {
                        w += 1;
                        if marks.iter().any(|m| m.covers(p)) {
                            k += 1;
                        }
                    }
                }
            }
            let n = (SAMPLES * SAMPLES) as f32;
            white[y * size + x] = w as f32 / n;
            black[y * size + x] = k as f32 / n;
        }
    }
    // The outline: the strongest body coverage within a radius of the pixel.
    let reach = (1.3 * unit).max(1.0);
    let r = reach.ceil() as i32;
    let mut bgra = vec![0u8; size * size * 4];
    for y in 0..size as i32 {
        for x in 0..size as i32 {
            let mut near = 0.0f32;
            for dy in -r..=r {
                for dx in -r..=r {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= size as i32 || ny >= size as i32 {
                        continue;
                    }
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    if d <= reach {
                        near = near.max(white[ny as usize * size + nx as usize]);
                    }
                }
            }
            let i = y as usize * size + x as usize;
            let body = white[i];
            let alpha = near.max(body);
            if alpha <= 0.0 {
                continue;
            }
            // How much of the pixel is the white body (not its marks).
            let light = ((body - black[i]).max(0.0) / alpha).min(1.0);
            let v = (255.0 * light).round() as u8;
            bgra[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, (alpha * 255.0).round() as u8]);
        }
    }
    let (hx, hy) = shape.hot();
    let at = |v: f32| ((v * unit) as usize).min(size - 1);
    Picture {
        size,
        hot: (at(hx), at(hy)),
        bgra,
    }
}

#[cfg(windows)]
pub use crate::pointer_os as os;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cursor_has_one_pointer_and_only_the_tools_have_a_picture() {
        use Cursor::*;
        for (cursor, style, shape) in [
            (Arrow, CursorStyle::Arrow, None),
            (Move, CursorStyle::ClosedHand, None),
            (ResizeH, CursorStyle::ResizeLeftRight, None),
            (ResizeV, CursorStyle::ResizeUpDown, None),
            (Cross, CursorStyle::Crosshair, None),
            (Blocked, CursorStyle::OperationNotAllowed, None),
            (Pencil, CursorStyle::Crosshair, Some(Shape::Pencil)),
            (Split, CursorStyle::Crosshair, Some(Shape::Scissors)),
            (Erase, CursorStyle::Crosshair, Some(Shape::Eraser)),
            (Mute, CursorStyle::Crosshair, Some(Shape::Mute)),
            (Audition, CursorStyle::Crosshair, Some(Shape::Audition)),
            (Range, CursorStyle::Crosshair, Some(Shape::Range)),
        ] {
            assert_eq!(
                pointer(cursor, false),
                Pointer { style, shape },
                "{cursor:?}"
            );
            let with_crosshair = pointer(cursor, true);
            assert_eq!(
                with_crosshair.shape, None,
                "{cursor:?}: no picture with the crosshair"
            );
            let tool = shape.is_some();
            let want = if tool { CursorStyle::Crosshair } else { style };
            assert_eq!(with_crosshair.style, want, "{cursor:?}");
        }
    }

    #[test]
    fn the_pictures_are_white_with_a_black_outline_and_the_working_point_is_on_them() {
        for shape in Shape::ALL {
            for size in [32, 48] {
                let pic = picture(shape, size);
                assert_eq!(pic.bgra.len(), size * size * 4);
                assert!(
                    pic.hot.0 < size && pic.hot.1 < size,
                    "{shape:?}: the point is inside"
                );
                let px = |x: usize, y: usize| &pic.bgra[(y * size + x) * 4..(y * size + x) * 4 + 4];
                let solid: Vec<_> = pic.bgra.chunks(4).filter(|p| p[3] == 255).collect();
                assert!(solid.iter().any(|p| p[0] == 255), "{shape:?}: a white body");
                assert!(
                    pic.bgra.chunks(4).any(|p| p[3] > 0 && p[0] < 64),
                    "{shape:?}: a dark outline"
                );
                // The working point is on or beside the picture.
                let near = |x: usize, y: usize| {
                    (x.saturating_sub(size / 8)..=(x + size / 8).min(size - 1)).any(|nx| {
                        (y.saturating_sub(size / 8)..=(y + size / 8).min(size - 1))
                            .any(|ny| px(nx, ny)[3] > 0)
                    })
                };
                assert!(
                    near(pic.hot.0, pic.hot.1),
                    "{shape:?}: the point is on the picture"
                );
            }
        }
    }

    /// Writes each picture as raw BGRA (`<shape>-<size>.bgra`, with its working point in
    /// `<shape>-<size>.hot`) into `XIRIUS_POINTER_DUMP`, for the screenshots: the system's
    /// capture of a window does not hold the cursor. `cargo test -p xirius-ui dump -- --ignored`.
    #[test]
    #[ignore = "writes files for the screenshots"]
    fn dump_the_pictures() {
        let dir = std::env::var("XIRIUS_POINTER_DUMP").expect("XIRIUS_POINTER_DUMP");
        for shape in Shape::ALL {
            for size in [32, 48, 64] {
                let pic = picture(shape, size);
                let name = format!("{shape:?}-{size}").to_lowercase();
                std::fs::write(format!("{dir}/{name}.bgra"), &pic.bgra).unwrap();
                std::fs::write(
                    format!("{dir}/{name}.hot"),
                    format!("{} {}", pic.hot.0, pic.hot.1),
                )
                .unwrap();
            }
        }
    }
}
