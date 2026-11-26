//! The pointer over the arrangement and the roll (UI-184): an OS shape for most places, and for
//! movable objects and tools a picture, drawn here and handed to the OS as the cursor itself, so
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
use crate::want::Want;

/// A custom pointer picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    OpenHand,
    ClosedHand,
    Pencil,
    Scissors,
    Eraser,
    Mute,
    Audition,
    Range,
    /// ↔ with the loop's mark: the lower half of a clip's right edge (UI-388).
    Loop,
}

impl Shape {
    pub const ALL: [Shape; 9] = [
        Shape::OpenHand,
        Shape::ClosedHand,
        Shape::Pencil,
        Shape::Scissors,
        Shape::Eraser,
        Shape::Mute,
        Shape::Audition,
        Shape::Range,
        Shape::Loop,
    ];

    /// The working point in the 32-unit square the picture is drawn in.
    pub fn hot(self) -> (f32, f32) {
        match self {
            Shape::OpenHand | Shape::ClosedHand => (16.0, 15.0),
            Shape::Pencil => (4.0, 26.0),
            Shape::Scissors | Shape::Mute | Shape::Audition | Shape::Range => (16.0, 16.0),
            Shape::Eraser => (4.0, 28.0),
            Shape::Loop => (16.0, 12.0),
        }
    }

    /// How much of its 32-unit drawing the picture keeps, drawn in toward the working point
    /// (UI-382): about two thirds, so that in the system's cursor box the hand is as big as
    /// the system's pointing hand. The long cross and scissors keep a little less.
    fn scale(self) -> f32 {
        match self {
            Shape::Scissors | Shape::Range => 0.6,
            Shape::OpenHand
            | Shape::ClosedHand
            | Shape::Pencil
            | Shape::Eraser
            | Shape::Mute
            | Shape::Audition
            | Shape::Loop => 0.68,
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

/// The pointer for `cursor`, not held (see [`show`]).
pub fn pointer(cursor: Cursor, crosshair: bool) -> Pointer {
    show(Want::of(cursor), crosshair, false)
}

/// The one table from what a place does to the shape shown (UI-383). With `crosshair` the
/// tools' pictures give way to the system's crosshair (SET: ツールのカーソルを十字にする); the
/// hand and the "not allowed" sign do not, they are not tools. `held`: a grab is under way.
pub fn show(want: Want, crosshair: bool, held: bool) -> Pointer {
    match want {
        Want::Plain => Pointer::style(CursorStyle::Arrow),
        Want::Press => Pointer::style(CursorStyle::PointingHand),
        Want::Text => Pointer::style(CursorStyle::IBeam),
        Want::DragH => Pointer::style(CursorStyle::ResizeLeftRight),
        Want::DragV => Pointer::style(CursorStyle::ResizeUpDown),
        Want::Grab => hand(held),
        Want::Corner => Pointer::style(CursorStyle::ResizeUpLeftDownRight),
        Want::Locate => Pointer::style(CursorStyle::Crosshair),
        Want::Blocked => Pointer::style(CursorStyle::OperationNotAllowed),
        // Not a tool: the crosshair setting leaves it.
        Want::Loop => Pointer {
            style: CursorStyle::ResizeLeftRight,
            shape: Some(Shape::Loop),
        },
        Want::Tool(_) if crosshair => Pointer::style(CursorStyle::Crosshair),
        Want::Tool(shape) => Pointer {
            style: CursorStyle::Crosshair,
            shape: Some(shape),
        },
    }
}

impl Studio {
    /// The pointer moves over a card, the settings or the palette, which take the move before
    /// the press flow does: its place still follows, so the shape answers what is under it.
    /// True when the shape changed (the window repaints to show it).
    pub(crate) fn covered_moved(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let before = of_studio(self, g).style;
        self.input_map.pointer = (x, y);
        of_studio(self, g).style != before
    }
}

/// The pointer over the window's body: the arrangement's shape where it has one, else the
/// roll's. Panels that cover the body (a pop-up, the palette, the settings) take the arrow.
pub fn of_studio(studio: &Studio, g: &Geometry) -> Pointer {
    let (x, y) = studio.input_map.pointer;
    let hit = studio.hit(g, x, y);
    if studio.popup.is_some() {
        return Pointer::style(match hit {
            crate::studio::Hit::Item(_) | crate::studio::Hit::SearchClear => {
                CursorStyle::PointingHand
            }
            _ => CursorStyle::Arrow,
        });
    }
    if let Some(settings) = &studio.settings {
        if let Some(menu) = &settings.menu {
            let cell = menu
                .menu
                .layout
                .cells
                .iter()
                .rev()
                .find(|c| c.rect.contains(x, y));
            return Pointer::style(match cell {
                Some(c) if c.clear_rect().is_some_and(|r| r.contains(x, y)) => {
                    CursorStyle::PointingHand
                }
                Some(c)
                    if c.enabled
                        && matches!(
                            c.look,
                            crate::popup::Look::Input | crate::popup::Look::Field
                        ) =>
                {
                    CursorStyle::IBeam
                }
                Some(c) if c.enabled && c.action.is_some() => CursorStyle::PointingHand,
                _ => CursorStyle::Arrow,
            });
        }
        return Pointer::style(
            match studio.settings_scene(g).item_at(x, y).map(|i| &i.act) {
                Some(crate::settings::Do::Field(_)) => CursorStyle::IBeam,
                Some(crate::settings::Do::Nothing) | None => CursorStyle::Arrow,
                Some(_) => CursorStyle::PointingHand,
            },
        );
    }
    // The cards and the palette take every press; their buttons and rows have the hand.
    if studio.palette.is_some() {
        return Pointer::style(studio.palette_pointer(g, x, y));
    }
    if studio.recovery.is_some() || studio.sound_preparing() {
        let over = studio.recovery_over_button(g, x, y) || studio.prepare_over_button(g, x, y);
        return Pointer::style(if over {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        });
    }
    if studio.instruments.sizing() {
        return Pointer::style(CursorStyle::ResizeUpLeftDownRight);
    }
    if studio.edge_cursor() {
        return Pointer::style(CursorStyle::ResizeRow);
    }
    if studio.side_cursor() {
        return Pointer::style(CursorStyle::ResizeColumn);
    }
    if studio.pan.is_some() {
        return hand(true);
    }
    if let Some(cursor) = studio.ruler_grab.as_ref().and_then(|grab| grab.cursor()) {
        return show(Want::of(cursor), studio.crosshair, true);
    }
    if studio.lock_hovered(g).is_some()
        || studio.insert_drag_refused(g)
        || studio.clip_drop_refused()
    {
        return Pointer::style(CursorStyle::OperationNotAllowed);
    }
    if studio.head_fader.dragging.is_some()
        || studio.mixer.level_drag.is_some()
        || studio.mixer.key_drag.is_some()
    {
        return Pointer::style(CursorStyle::ResizeLeftRight);
    }
    if studio.mixer.drag.is_some() {
        return Pointer::style(CursorStyle::ResizeUpDown);
    }
    // The notice's button lies over whatever is under it (UI-384).
    if studio.over_notice_button(g, x, y) {
        return Pointer::style(CursorStyle::PointingHand);
    }
    if studio.text_index(g, x, y, false).is_some() {
        return Pointer::style(CursorStyle::IBeam);
    }
    let held =
        studio.arrange.drag.as_ref().is_some_and(|d| d.started) || studio.roll.drag.is_some();
    // A press on a button decides at once; a drag from the arrangement or the roll keeps its
    // own shape over the buttons it passes.
    let dragging = studio.arrange.drag.is_some() || studio.roll.drag.is_some();
    if let Some(want) = hit.want().filter(|_| !dragging) {
        return show(want, studio.crosshair, held);
    }
    let want = Want::of(crate::arrange::paint::cursor(studio).unwrap_or_else(|| {
        let ctx = studio.roll_ctx(g);
        studio.roll.cursor(ctx.as_ref())
    }));
    let over_ruler = matches!(hit, crate::studio::Hit::Ruler(_))
        || studio
            .roll_ctx(g)
            .is_some_and(|ctx| studio.roll.frame(ctx.body).ruler.contains(x, y));
    if want == Want::Plain && over_ruler && !held {
        return show(Want::Locate, studio.crosshair, held);
    }
    show(want, studio.crosshair, held)
}

fn hand(grabbing: bool) -> Pointer {
    Pointer {
        style: if grabbing {
            CursorStyle::ClosedHand
        } else {
            CursorStyle::OpenHand
        },
        shape: Some(if grabbing {
            Shape::ClosedHand
        } else {
            Shape::OpenHand
        }),
    }
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
        Shape::OpenHand => (
            vec![
                Prim::Poly(&[
                    (8.0, 16.0),
                    (8.0, 27.0),
                    (13.0, 30.0),
                    (23.0, 28.0),
                    (27.0, 21.0),
                    (25.0, 15.0),
                    (20.0, 14.0),
                    (17.0, 17.0),
                ]),
                Prim::Line((10.0, 16.0), (10.0, 5.0), 3.5),
                Prim::Line((15.0, 16.0), (15.0, 3.0), 3.5),
                Prim::Line((20.0, 16.0), (20.0, 5.0), 3.5),
                Prim::Line((25.0, 18.0), (25.0, 10.0), 3.5),
            ],
            vec![],
        ),
        Shape::ClosedHand => (
            vec![
                Prim::Poly(&[
                    (6.0, 11.0),
                    (24.0, 10.0),
                    (28.0, 15.0),
                    (26.0, 26.0),
                    (20.0, 30.0),
                    (10.0, 28.0),
                    (5.0, 20.0),
                ]),
                Prim::Line((10.0, 9.0), (10.0, 16.0), 4.0),
                Prim::Line((16.0, 8.0), (16.0, 16.0), 4.0),
                Prim::Line((22.0, 9.0), (22.0, 16.0), 4.0),
            ],
            vec![Prim::Line((9.0, 17.0), (23.0, 17.0), 1.2)],
        ),
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
                // Wide enough to keep two pixels at 32 once drawn in to 0.6.
                Prim::Line((16.0, 2.0), (16.0, 12.0), 3.4),
                Prim::Line((16.0, 20.0), (16.0, 30.0), 3.4),
                Prim::Line((2.0, 16.0), (12.0, 16.0), 3.4),
                Prim::Line((20.0, 16.0), (30.0, 16.0), 3.4),
            ],
            vec![],
        ),
        Shape::Loop => (
            vec![
                // The ↔ of the edge, and the loop's circling arrow under its right end.
                Prim::Line((7.0, 12.0), (25.0, 12.0), 3.0),
                Prim::Poly(&[(2.0, 12.0), (9.0, 6.0), (9.0, 18.0)]),
                Prim::Poly(&[(30.0, 12.0), (23.0, 6.0), (23.0, 18.0)]),
                Prim::Ring((21.0, 24.0), 6.0, 2.6),
                Prim::Poly(&[(13.0, 22.0), (19.0, 22.0), (16.0, 27.0)]),
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

/// Draws `shape` at `size` pixels square (the system's cursor box). Each pixel is the mean of
/// 4 × 4 samples; the drawing is shrunk toward its working point by `Shape::scale`, and the
/// outline is the body grown by a unit.
pub fn picture(shape: Shape, size: usize) -> Picture {
    const SAMPLES: usize = 4;
    let unit = size as f32 / 32.0;
    let (body, marks) = parts(shape);
    let (hx, hy) = shape.hot();
    let scale = shape.scale();
    let mut white = vec![0.0f32; size * size];
    let mut black = vec![0.0f32; size * size];
    for y in 0..size {
        for x in 0..size {
            let (mut w, mut k) = (0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let at = (
                        (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / unit,
                        (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / unit,
                    );
                    let p = (hx + (at.0 - hx) / scale, hy + (at.1 - hy) / scale);
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
    let reach = unit.max(1.0);
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
    fn mixer_controls_follow_their_drag_direction_even_outside_the_control() {
        use crate::mixer::MixerHit;
        use crate::studio::{DrawerTab, Hit};
        let mut studio = Studio::sample();
        studio.tab = DrawerTab::Mixer;
        let g = studio.geometry(1440.0, 860.0);
        studio.resized(&g);
        let track = studio
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        for (hit, want) in [
            (MixerHit::Fader(track), CursorStyle::ResizeUpDown),
            (MixerHit::Pan(track), CursorStyle::ResizeUpDown),
            (MixerHit::Mute(track), CursorStyle::PointingHand),
        ] {
            studio.input_map.pointer = studio
                .center_of(&g, Hit::Mixer(hit))
                .expect("visible control");
            assert_eq!(of_studio(&studio, &g).style, want);
        }
        let (x, y) = studio
            .center_of(&g, Hit::Mixer(MixerHit::Fader(track)))
            .unwrap();
        assert!(studio.drag_begin(&g, Hit::Mixer(MixerHit::Fader(track)), y));
        studio.input_map.pointer = (x, 0.0);
        assert_eq!(of_studio(&studio, &g).style, CursorStyle::ResizeUpDown);
        studio.drag_end();
    }

    #[test]
    fn settings_buttons_and_text_fields_keep_their_cursor_over_the_modal() {
        let mut studio = Studio::sample();
        let g = studio.geometry(1440.0, 860.0);
        studio.open_settings(crate::settings::Section::Keys);
        studio.settings_do(
            &g,
            crate::settings::Do::KeyMode(false),
            crate::studio::Rect::default(),
        );
        let scene = studio.settings_scene(&g);
        for (action, want) in [
            (
                crate::settings::Do::Nav(crate::settings::Section::General),
                CursorStyle::PointingHand,
            ),
            (
                crate::settings::Do::Field(crate::settings::FieldId::Search),
                CursorStyle::IBeam,
            ),
        ] {
            let item = scene.find(&action).expect("visible settings control");
            studio.input_map.pointer = (item.r.x + item.r.w / 2.0, item.r.y + item.r.h / 2.0);
            assert_eq!(of_studio(&studio, &g).style, want);
        }
    }

    /// The table is a copy of `pointer()`'s match, on purpose: going round every cursor is
    /// what it is for, so a cursor added there is to be added here too.
    #[test]
    fn every_cursor_has_one_pointer_and_only_custom_shapes_have_a_picture() {
        use Cursor::*;
        for (cursor, style, shape) in [
            (Arrow, CursorStyle::Arrow, None),
            (Press, CursorStyle::PointingHand, None),
            (Move, CursorStyle::OpenHand, Some(Shape::OpenHand)),
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
            (Loop, CursorStyle::ResizeLeftRight, Some(Shape::Loop)),
        ] {
            assert_eq!(
                pointer(cursor, false),
                Pointer { style, shape },
                "{cursor:?}"
            );
            // The crosshair takes the tools' place; the hand is not a tool and stays.
            let with_crosshair = pointer(cursor, true);
            let tool = style == CursorStyle::Crosshair && shape.is_some();
            let (style, shape) = if tool {
                (CursorStyle::Crosshair, None)
            } else {
                (style, shape)
            };
            assert_eq!(with_crosshair, Pointer { style, shape }, "{cursor:?}");
        }
        // Held, the hand closes.
        assert_eq!(
            show(Want::Grab, false, true),
            Pointer {
                style: CursorStyle::ClosedHand,
                shape: Some(Shape::ClosedHand)
            }
        );
    }

    /// Every button the report found showing the arrow (UI-383) now shows the pointing hand,
    /// and the drags their own arrows.
    #[test]
    fn every_kind_of_hit_has_its_shape() {
        use crate::studio::Hit;
        for hit in [
            Hit::MidiCancel,
            Hit::WaveZoom,
            Hit::RollMap,
            Hit::Toast(crate::toast::ToastPart::Close),
            Hit::Rec(crate::rec_panel::RecPart::Punch),
            Hit::Inspector(crate::inspector::Part::Field(0)),
            Hit::Start(crate::start::Entry::Audio),
            Hit::Entrance(crate::start::Part::NoDevice),
            Hit::Home(crate::home::Spot::Device),
            Hit::Bar(crate::topbar::BarButton::Play),
            Hit::Rail(crate::browser::RailTab::Files),
        ] {
            assert_eq!(hit.want(), Some(Want::Press), "{hit:?}");
            assert_eq!(
                show(Want::Press, false, false).style,
                CursorStyle::PointingHand
            );
        }
        assert_eq!(
            Hit::Bar(crate::topbar::BarButton::Tempo).want(),
            Some(Want::DragV)
        );
        assert_eq!(
            Hit::Browser(crate::browser::BrowserHit::Search).want(),
            Some(Want::Text)
        );
        assert_eq!(
            Hit::Toast(crate::toast::ToastPart::Body).want(),
            Some(Want::Plain)
        );
        assert_eq!(Hit::None.want(), None);
    }

    #[test]
    fn the_track_columns_buttons_and_the_bars_show_the_pointing_hand() {
        use crate::studio::Hit;
        let mut studio = Studio::sample();
        // Tall enough that every row is in view.
        let g = studio.geometry(1440.0, 1100.0);
        let track = studio
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        for hit in [
            Hit::Mute(track),
            Hit::Solo(track),
            Hit::Arm(track),
            Hit::TrackColor(track),
            Hit::Bar(crate::topbar::BarButton::Play),
            Hit::Bar(crate::topbar::BarButton::Loop),
            Hit::AddTrack,
        ] {
            let Some(at) = studio.center_of(&g, hit) else {
                panic!("{hit:?}: on the screen");
            };
            studio.input_map.pointer = at;
            assert_eq!(studio.hit(&g, at.0, at.1), hit, "{hit:?}: found there");
            assert_eq!(
                of_studio(&studio, &g).style,
                CursorStyle::PointingHand,
                "{hit:?}"
            );
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

    /// The opaque extent (alpha of a half or more) of a picture: its width and height in pixels.
    fn extent(pic: &Picture) -> (usize, usize) {
        let size = pic.size;
        let solid = |x: usize, y: usize| pic.bgra[(y * size + x) * 4 + 3] >= 128;
        let cols: Vec<_> = (0..size)
            .filter(|&x| (0..size).any(|y| solid(x, y)))
            .collect();
        let rows: Vec<_> = (0..size)
            .filter(|&y| (0..size).any(|x| solid(x, y)))
            .collect();
        let span = |v: &[usize]| v.last().map_or(0, |l| l - v[0] + 1);
        (span(&cols), span(&rows))
    }

    #[test]
    fn the_pictures_are_as_big_as_the_systems_own_cursors() {
        for shape in Shape::ALL {
            for size in [32, 40, 48, 64] {
                let (w, h) = extent(&picture(shape, size));
                let most = size * 7 / 10;
                assert!(
                    w <= most && h <= most,
                    "{shape:?} at {size}: {w}×{h} fills more than 70 % of the box"
                );
            }
        }
        // In the 48-pixel box of 175 %, the open hand is about the system's pointing hand
        // (25 × 32 there), not the 37 × 54 it was in a box of 56.
        let (w, h) = extent(&picture(Shape::OpenHand, 48));
        assert!((20..=28).contains(&w) && (27..=34).contains(&h), "{w}×{h}");
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
