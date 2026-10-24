//! Painting of the real screen's body: the track column, the arrangement and
//! the drawer, from the numbers [`Studio`] and [`Geometry`] already settled.
//!
//! Everything is `paint_quad`, text and SVG masks. Surfaces are told apart by
//! lightness, there are no lines and no gradients; the one line-like mark is
//! the folder range line (MIX-038). Clips are a neutral surface and only what
//! is drawn on them (notes, waveform) takes the track colour.

use std::hash::{Hash, Hasher};

use gpui::{
    App, Bounds, ContentMask, Corners, Hsla, PathBuilder, Pixels, SharedString, TextAlign,
    TransformationMatrix, Window, fill, point, px, size,
};
use xirius_core::id::Node;
use xirius_core::model::{ClipBody, Project, Track};
use xirius_design::{
    FontSize, Layout, LineState, Radius, Space, StructureLine, TrackColor, Weight,
};

use crate::assets::icon;
use crate::paint::Colors;
use crate::song::palette_color;
use crate::studio::{DrawerTab, Geometry, Hit, Rect, RowParts, Studio};
use crate::theme::text_runs;
use crate::tree::{Row, RowKind};

/// Where text sits in the box it is given.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A painter with an origin: every rectangle is given relative to the body.
pub struct Pen<'a> {
    pub window: &'a mut Window,
    pub cx: &'a mut App,
    pub c: &'a Colors,
    pub origin: (f32, f32),
}

fn bounds(origin: (f32, f32), r: Rect) -> Bounds<Pixels> {
    Bounds::new(
        point(px(origin.0 + r.x), px(origin.1 + r.y)),
        size(px(r.w), px(r.h)),
    )
}

/// Which corners of a rectangle are round.
#[derive(Clone, Copy)]
pub struct Round {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Round {
    pub const ALL: Round = Round {
        top: true,
        bottom: true,
        left: true,
        right: true,
    };

    pub const NONE: Round = Round {
        top: false,
        bottom: false,
        left: false,
        right: false,
    };

    pub fn rows(top: bool, bottom: bool) -> Round {
        Round {
            top,
            bottom,
            left: true,
            right: true,
        }
    }

    fn corners(self, radius: Radius) -> Corners<Pixels> {
        let r = |on: bool| px(if on { radius.px() } else { 0.0 });
        Corners {
            top_left: r(self.top && self.left),
            top_right: r(self.top && self.right),
            bottom_left: r(self.bottom && self.left),
            bottom_right: r(self.bottom && self.right),
        }
    }
}

impl Pen<'_> {
    /// Batch non-overlapping quads without a bounds-tree lookup per note.
    pub fn layer(&mut self, r: Rect, paint: impl FnOnce(&mut Pen)) {
        let Pen {
            window,
            cx,
            c,
            origin,
        } = self;
        let (c, origin) = (*c, *origin);
        window.paint_layer(bounds(origin, r), |window| {
            paint(&mut Pen {
                window,
                cx,
                c,
                origin,
            });
        });
    }

    pub fn fill(&mut self, r: Rect, color: Hsla) {
        self.window.paint_quad(fill(bounds(self.origin, r), color));
    }

    /// A raised surface uses the same two soft shadows as the palette.
    #[allow(clippy::too_many_arguments)]
    pub fn transition(
        &mut self,
        motion: &crate::view_motion::ViewMotion,
        setting: xirius_design::Motion,
        r: Rect,
        key: &str,
        on: bool,
        off: Hsla,
        lit: Hsla,
    ) -> Hsla {
        let t = motion.value(
            key,
            f32::from(on),
            xirius_design::Duration::Fast,
            setting,
            std::time::Instant::now(),
        );
        let light = gpui::BoxShadow {
            color: lit.opacity(t * xirius_design::Toggle::GLOW.1),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(xirius_design::Toggle::GLOW.0),
            spread_radius: px(t),
            inset: false,
        };
        self.window.paint_drop_shadows(
            bounds(self.origin, r),
            Corners::all(px(Radius::S.px())),
            &[light],
        );
        let face = if on { lit } else { off };
        if motion.moving() {
            self.window.request_animation_frame();
        }
        face
    }

    /// [`Pen::transition`] for a face and the ink on it: both switch on the one
    /// decision, so the ink is never left over from the other face (UI-309).
    #[allow(clippy::too_many_arguments)]
    pub fn transition_pair(
        &mut self,
        motion: &crate::view_motion::ViewMotion,
        setting: xirius_design::Motion,
        r: Rect,
        key: &str,
        on: bool,
        off: (Hsla, Hsla),
        lit: (Hsla, Hsla),
    ) -> (Hsla, Hsla) {
        self.transition(motion, setting, r, key, on, off.0, lit.0);
        if on { lit } else { off }
    }

    pub fn floating(&mut self, r: Rect, color: Hsla, radius: Radius) {
        self.window.paint_drop_shadows(
            bounds(self.origin, r),
            Corners::all(px(radius.px())),
            &self.c.float,
        );
        self.round(r, color, radius);
    }

    pub fn round(&mut self, r: Rect, color: Hsla, radius: Radius) {
        self.shape(r, color, radius, Round::ALL);
    }

    /// A line `width` wide along the inside of `r`, nothing in it.
    pub fn outline(&mut self, r: Rect, color: Hsla, radius: Radius, width: f32) {
        let mut quad = gpui::outline(bounds(self.origin, r), color, gpui::BorderStyle::default());
        quad.corner_radii = Corners::all(px(radius.px().min((r.w.min(r.h) / 2.0).max(0.0))));
        quad.border_widths = gpui::Edges::all(px(width));
        self.window.paint_quad(quad);
    }

    pub fn shape(&mut self, r: Rect, color: Hsla, radius: Radius, round: Round) {
        let mut quad = fill(bounds(self.origin, r), color);
        // A corner radius larger than half the box would bulge the quad.
        let limit = (r.w.min(r.h) / 2.0).max(0.0);
        let radius_px = radius.px().min(limit);
        let corners = round.corners(radius);
        quad.corner_radii = Corners {
            top_left: px(f32::from(corners.top_left).min(radius_px)),
            top_right: px(f32::from(corners.top_right).min(radius_px)),
            bottom_left: px(f32::from(corners.bottom_left).min(radius_px)),
            bottom_right: px(f32::from(corners.bottom_right).min(radius_px)),
        };
        self.window.paint_quad(quad);
    }

    /// Paints inside `r` only.
    pub fn mask(&mut self, r: Rect, paint: impl FnOnce(&mut Pen)) {
        let Pen {
            window,
            cx,
            c,
            origin,
        } = self;
        let (c, origin) = (*c, *origin);
        window.with_content_mask(
            Some(ContentMask {
                bounds: bounds(origin, r),
            }),
            |window| {
                let mut inner = Pen {
                    window,
                    cx,
                    c,
                    origin,
                };
                paint(&mut inner);
            },
        );
    }

    /// One line of text. `r` is the box it is placed in (centred vertically);
    /// the width of the text is returned.
    pub fn text(
        &mut self,
        s: &str,
        font_size: FontSize,
        weight: Weight,
        color: Hsla,
        r: Rect,
        align: Align,
    ) -> f32 {
        self.text_with(s, font_size, weight, false, color, r, align)
    }

    /// Text whose digits keep their width (positions, tempo, levels).
    pub fn digits(
        &mut self,
        s: &str,
        font_size: FontSize,
        weight: Weight,
        color: Hsla,
        r: Rect,
        align: Align,
    ) -> f32 {
        self.text_with(s, font_size, weight, true, color, r, align)
    }

    #[allow(clippy::too_many_arguments)]
    fn text_with(
        &mut self,
        s: &str,
        font_size: FontSize,
        weight: Weight,
        tabular: bool,
        color: Hsla,
        r: Rect,
        align: Align,
    ) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        let size_px = font_size.px();
        let line = self.window.text_system().shape_line(
            SharedString::from(s.to_string()),
            px(size_px),
            &text_runs(s, weight, tabular, color),
            None,
        );
        let width = f32::from(line.width);
        let height = size_px * 1.3;
        let x = match align {
            Align::Left => r.x,
            Align::Center => r.x + (r.w - width) / 2.0,
            Align::Right => r.right() - width,
        };
        let y = r.y + (r.h - height) / 2.0;
        let _ = line.paint(
            point(px(self.origin.0 + x), px(self.origin.1 + y)),
            px(height),
            TextAlign::Left,
            None,
            self.window,
            self.cx,
        );
        width
    }

    /// `s`, cut with 「…」 when it is wider than `w`. A name keeps as much as the space leaves
    /// (UI-312).
    pub fn fit(&mut self, s: &str, font_size: FontSize, weight: Weight, w: f32) -> String {
        fit_text(s, w, |text| self.measure(text, font_size, weight))
    }

    /// How wide `s` comes out, without drawing it.
    pub fn measure(&mut self, s: &str, font_size: FontSize, weight: Weight) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        let line = self.window.text_system().shape_line(
            SharedString::from(s.to_string()),
            px(font_size.px()),
            &text_runs(s, weight, false, self.c.ink),
            None,
        );
        f32::from(line.width)
    }

    /// An icon from `assets/icons`, drawn in `color` into `r`.
    pub fn icon(&mut self, name: &str, r: Rect, color: Hsla) {
        self.svg(icon(name), r, color);
    }

    /// A bundled SVG mask, including the brand artwork.
    pub fn svg(&mut self, path: SharedString, r: Rect, color: Hsla) {
        let _ = self.window.paint_svg(
            bounds(self.origin, r),
            path,
            None,
            TransformationMatrix::unit(),
            color,
            self.cx,
        );
    }

    /// The brand's star into `r`: its full shape in the fill, its light half over it (UI-303).
    pub fn brand_mark(&mut self, r: Rect) {
        let colors = [self.c.fill, self.c.ink_blue];
        for (n, color) in colors.into_iter().enumerate() {
            let _ = self.window.paint_svg(
                bounds(self.origin, r),
                crate::assets::mark_layer(n),
                None,
                TransformationMatrix::unit(),
                color,
                self.cx,
            );
        }
    }

    /// An icon turned `turn` of a full circle (clockwise) round the middle of `r`.
    pub fn icon_turned(&mut self, name: &str, r: Rect, color: Hsla, turn: f32) {
        let b = bounds(self.origin, r);
        let scale = self.window.scale_factor();
        let centre = b.center().scale(scale);
        let matrix = TransformationMatrix::unit()
            .translate(centre)
            .rotate(gpui::radians(turn * std::f32::consts::TAU))
            .translate(centre.map(|v| gpui::ScaledPixels(-v.0)));
        let _ = self
            .window
            .paint_svg(b, icon(name), None, matrix, color, self.cx);
    }

    /// A diamond filling `r` (a hit in the drum view).
    pub(crate) fn diamond(&mut self, r: Rect, color: Hsla) {
        let (ox, oy) = self.origin;
        let (cx, cy) = (ox + r.x + r.w / 2.0, oy + r.y + r.h / 2.0);
        let mut path = PathBuilder::fill();
        path.move_to(point(px(cx), px(cy - r.h / 2.0)));
        path.line_to(point(px(cx + r.w / 2.0), px(cy)));
        path.line_to(point(px(cx), px(cy + r.h / 2.0)));
        path.line_to(point(px(cx - r.w / 2.0), px(cy)));
        path.close();
        if let Ok(path) = path.build() {
            self.window.paint_path(path, color);
        }
    }

    /// The small downward triangle at the top of the playhead.
    pub(crate) fn playhead_head(&mut self, x: f32, top: f32, color: Hsla) {
        let (ox, oy) = self.origin;
        let half = Layout::PLAYHEAD_W * 3.0;
        let mut path = PathBuilder::fill();
        path.move_to(point(px(ox + x - half), px(oy + top)));
        path.line_to(point(px(ox + x + half), px(oy + top)));
        path.line_to(point(px(ox + x), px(oy + top + half * 1.5)));
        path.close();
        if let Ok(path) = path.build() {
            self.window.paint_path(path, color);
        }
    }
}

/// The tracks of a project in the order the column shows them, folders opened.
pub fn ordered_tracks(project: &Project) -> Vec<&Track> {
    fn walk<'a>(project: &'a Project, nodes: &[Node], out: &mut Vec<&'a Track>) {
        for node in nodes {
            match node {
                Node::Track(t) => out.extend(project.tracks.get(t).map(|t| &**t)),
                Node::Folder(f) => {
                    if let Some(folder) = project.folders.get(f) {
                        walk(project, &folder.children, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(project, &project.root, &mut out);
    out
}

/// The palette index of a track's colour (the first one when it has none).
pub fn track_color(track: &Track) -> usize {
    track
        .color
        .as_deref()
        .and_then(palette_color)
        .map_or(0, TrackColor::index)
}

pub(crate) fn color_of(c: &Colors, track: &Track) -> Hsla {
    c.tracks[track_color(track) % c.tracks.len()]
}

// ------------------------------------------------------------------- body

pub fn paint_body(pen: &mut Pen, studio: &Studio, w: f32, h: f32) {
    let _cache = crate::opening::PaintScope::enter(studio.opening.is_some());
    let g = studio.geometry(w, h);
    let hover = if let Hit::Browser(h) = studio.hover {
        Some(h)
    } else {
        None
    };
    crate::browser::paint(
        pen,
        &studio.browser,
        hover,
        g.rail,
        g.panel,
        studio.ai.motion,
    );
    if studio.inspector_open() {
        let hover = if let Hit::Inspector(h) = studio.hover {
            Some(h)
        } else {
            None
        };
        crate::inspector::paint(pen, studio, hover, &g);
    }
    paint_head(pen, studio, &g);
    let grid = Grid::of(studio, &g);
    paint_ruler(pen, studio, &g, &grid);
    paint_rows(pen, studio, &g, &grid);
    paint_drawer(pen, studio, &g);
}

/// What floats over the body and the roll: what moves while the song plays
/// (the playhead, the mixer and its meters), the arrangement's drags, the open pop-over
/// and the line that says why the last command failed. It is painted after
/// the roll, so the cached body under it does not have to change when it does.
pub fn paint_overlay(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let _cache = crate::opening::PaintScope::enter(studio.opening.is_some());
    crate::start::paint(pen, studio, g);
    crate::rec_panel::paint(pen, studio, g);
    paint_takes(pen, studio, g);
    paint_playhead(pen, studio, g);
    paint_head_meters(pen, studio, g);
    crate::arrange::paint::overlay(pen, studio, g);
    crate::track_name::paint(pen, studio, g);
    crate::zoom_input::paint(pen, studio);
    crate::ai::overlay(pen, studio, g);
    if studio.mixer_shown() {
        crate::mixer::paint::paint(pen, studio, g);
    }
    crate::fx_chain::paint_headers(pen, studio, g);
    crate::instrument::window::paint(pen, studio, g);
    crate::browser_inst::paint(pen, studio, g);
    crate::fx_chain::paint_drag(pen, studio, g);
    crate::name_drag::paint(pen, studio, g);
    crate::fx_preset::paint(pen, studio);
    crate::hover_tip::paint(pen, studio, g);
    crate::gain_tip::paint(pen, studio, g);
    if let Some(p) = &studio.popup {
        let hover = if let Hit::Item(i) = studio.hover {
            Some(i)
        } else {
            None
        };
        let fresh = studio.ai.fresh();
        if p.kind == crate::popup::PopupKind::Cleanup {
            crate::cleanup_dialog::paint_veil(pen, g);
        }
        crate::popup::paint_with(pen, &p.layout(g.size), hover, &|pen, cell| {
            crate::ai::paint_fresh(pen, cell, fresh)
        });
        crate::cleanup_dialog::paint_ring(pen, p, g);
        crate::hover_tip::popup(pen, studio, g);
    }
    crate::notice::paint(pen, studio, g);
    crate::midi_assign::paint(pen, studio, g);
    crate::toast::paint(pen, studio, g);
    crate::dawproject_ui::paint_drop(pen, studio, g);
    if let Some(error) = &studio.error {
        let r = Rect::new(g.lanes.x, g.drawer.y - 36.0, g.lanes.w, 30.0);
        pen.round(r, pen.c.surface_3, Radius::S);
        pen.mask(r, |pen| {
            pen.text(
                error,
                FontSize::Small,
                Weight::Medium,
                pen.c.ink,
                r,
                Align::Center,
            );
        });
    }
    if let Some(palette) = &studio.palette {
        crate::palette::paint(pen, palette, g);
    }
    crate::settings::paint(pen, studio, g);
    if let Some(card) = &studio.recovery {
        crate::recovery_card::paint(pen, card, g);
    }
    if let Some(card) = &studio.preparing {
        crate::prepare_card::paint(pen, card, g, studio.spin());
    }
    crate::wait::paint(pen, studio, g);
}

/// Each track's level at the right edge of its head: the louder of the two
/// channels of its mixer meter.
fn paint_head_meters(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let tree = studio.tree();
    pen.mask(region(g, g.column), |pen| {
        for row in &tree.rows {
            let RowKind::Track(t) = row.kind else {
                continue;
            };
            let top = studio.row_y(g, row);
            if top + row.full_h() < g.column.y || top > g.column.bottom() {
                continue;
            }
            let Some(r) = RowParts::of(row).meter else {
                continue;
            };
            let m = studio
                .mixer
                .meters
                .get(xirius_session::host::Strip::Track(t));
            let ch = usize::from(m.level[1] > m.level[0]);
            crate::mixer::paint::meter(
                pen,
                Rect::new(g.column.x + r.x, top + r.y, r.w, r.h),
                &m,
                ch,
            );
        }
    });
}

pub(crate) fn region(g: &Geometry, view: Rect) -> Rect {
    Rect::new(
        view.x,
        view.y,
        view.w,
        g.drawer.y - Layout::REGION_GAP - view.y,
    )
}

/// A switch's face and icon ink: the primary pair when it is on, the neutral
/// face (lighter under the pointer) and body ink when it is off.
pub(crate) fn toggle_colors(c: &Colors, on: bool, hover: bool) -> (Hsla, Hsla) {
    if on {
        c.primary
    } else if hover {
        (c.hover, c.ink)
    } else {
        (c.surface_1, c.ink)
    }
}

/// A switch in the head of the column or in the roll's strip: its face when
/// it is on (or under the pointer), and its icon at the centre.
pub(crate) fn toggle_button(
    pen: &mut Pen,
    studio: &Studio,
    r: Rect,
    icon: &str,
    on: bool,
    hover: bool,
) {
    let c = pen.c;
    let (face, ink) = pen.transition_pair(
        &studio.motion,
        studio.ai.motion,
        r,
        &format!("toggle-{icon}-{}", r.x),
        on,
        toggle_colors(c, false, hover),
        toggle_colors(c, true, hover),
    );
    pen.round(r, face, Radius::S);
    let size = Layout::ICON_SIZE;
    pen.icon(
        icon,
        Rect::new(
            r.x + (r.w - size) / 2.0,
            r.y + (r.h - size) / 2.0,
            size,
            size,
        ),
        ink,
    );
}

/// The corner above the track column and the section band + ruler.
fn paint_head(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let c = pen.c;
    pen.round(g.head, c.surface_2, Radius::S);
    // UI-066: ‹ folds the list, › opens it again; folded, nothing else of the head is left.
    let fold = g.toggle(Hit::Fold).unwrap();
    let icon = if g.panels.folded {
        "chevron-right"
    } else {
        "chevron-left"
    };
    toggle_button(pen, studio, fold, icon, false, studio.hover == Hit::Fold);
    if g.panels.folded {
        return;
    }
    for (hit, icon, on) in [
        (Hit::AddTrack, "plus", false),
        (Hit::ToggleArrange, "arrange", studio.arrange_visible),
        (Hit::ToggleTempo, "tempo", studio.tempo_visible),
        (Hit::ZoomOut, "zoom-out", false),
        (Hit::ZoomIn, "zoom-in", false),
        (Hit::WaveZoom, "wave-zoom", studio.wave_zoom > 1.0),
        (Hit::Follow, "follow", studio.follow.on),
    ] {
        let r = g.toggle(hit).unwrap();
        toggle_button(pen, studio, r, icon, on, studio.hover == hit);
    }
    for (row, icon, name) in [
        (g.section_band, "arrange", "アレンジ"),
        (g.tempo, "tempo", "テンポ"),
    ] {
        if row.h > 0.0 {
            let x = g.head.x + Space::X3.px();
            pen.icon(
                icon,
                Rect::new(
                    x,
                    row.y + (row.h - Layout::ICON_SIZE) / 2.0,
                    Layout::ICON_SIZE,
                    Layout::ICON_SIZE,
                ),
                c.ink,
            );
            let text_x = x + Layout::ICON_SIZE + Space::X2.px();
            pen.text(
                name,
                FontSize::Small,
                Weight::Medium,
                c.ink,
                Rect::new(text_x, row.y, g.head.right() - text_x, row.h),
                Align::Left,
            );
        }
    }
}

/// The arrangement's grid, found once a frame for the ruler and the lanes.
struct Grid {
    meter: crate::tools::Meter,
    first: xirius_core::time::Tick,
    last: xirius_core::time::Tick,
    lines: Vec<crate::grid_lines::Line>,
    /// Zoomed in to the samples the seconds show too (UI-339).
    times: Option<crate::grid_lines::Times>,
}

impl Grid {
    fn of(studio: &Studio, g: &Geometry) -> Grid {
        let (first, last) = arrange_span(studio, g);
        let meter = studio.meter();
        Grid {
            lines: crate::grid_lines::lines(&studio.tools().snap, &meter, first, last),
            times: crate::grid_lines::times(&meter, first, last),
            meter,
            first,
            last,
        }
    }
}

fn paint_ruler(pen: &mut Pen, studio: &Studio, g: &Geometry, grid: &Grid) {
    let c = pen.c;
    let project = studio.project();
    let whole = Rect::new(g.ruler.x, g.ruler.y, g.ruler.w, g.header_h());
    pen.round(whole, c.surface_2, Radius::S);
    pen.mask(whole, |pen| {
        let inset = Space::X1.px() / 2.0;
        for section in &project.sections {
            if g.section_band.h == 0.0 {
                break;
            }
            let x0 = studio.tick_x(g, section.start);
            let x1 = studio.tick_x(g, section.start + section.length);
            if x1 < g.section_band.x || x0 > g.section_band.right() {
                continue;
            }
            let index = section
                .color
                .as_deref()
                .and_then(palette_color)
                .map_or(0, TrackColor::index);
            let (face, ink) = pen.c.sections[index % pen.c.sections.len()];
            let r = Rect::new(
                x0,
                g.section_band.y + inset,
                x1 - x0,
                g.section_band.h - inset * 2.0,
            );
            if studio.arrange.section == Some(section.id) {
                // The selected section: a ring inside its own edge (ARR-002).
                pen.round(r, pen.c.fill, Radius::Clip);
                let ring = 1.5;
                let inner = Rect::new(r.x + ring, r.y + ring, r.w - ring * 2.0, r.h - ring * 2.0);
                if inner.w > 0.0 && inner.h > 0.0 {
                    pen.round(inner, face, Radius::Clip);
                }
            } else {
                pen.round(r, face, Radius::Clip);
            }
            pen.mask(r, |pen| {
                let at = Rect::new(r.x + Space::X2.px(), r.y, r.w, r.h);
                pen.text(
                    &section.name,
                    FontSize::Caption,
                    Weight::Semibold,
                    ink,
                    at,
                    Align::Left,
                );
            });
        }
        if g.tempo.h > 0.0 {
            crate::arrange::paint::tempo(pen, studio, &project, g);
        }
        crate::arrange::paint::loop_band(pen, studio, &project, g);
        let scale = studio.ruler_scale(g);
        if studio.song.session().state.view.ruler_seconds {
            crate::context::marker::paint_seconds(pen, studio, g);
        } else {
            // The grid's ticks, then the numbers (bars thinned out until they
            // fit, beats and 16ths as it zooms in), none under a marker's name.
            crate::grid_lines::paint_ticks(pen, &grid.lines, g.ruler, |t| scale.x(t));
            if let Some(times) = &grid.times {
                crate::grid_lines::paint_time_ticks(pen, times, g.ruler, |t| studio.tick_x_f(g, t));
            }
            let marks: Vec<_> = crate::ruler::labels(
                &grid.meter.time,
                project.ppq,
                studio.beat_w,
                studio.tools().snap.triplets(),
                grid.first,
                grid.last,
            )
            .into_iter()
            .filter(|mark| {
                !project.markers.iter().any(|m| {
                    studio
                        .marker_rect(g, m.id)
                        .is_some_and(|r| r.contains(scale.x(mark.tick) + 4.0, r.y + 2.0))
                })
            })
            .collect();
            crate::ruler::paint_labels(pen, &marks, g.ruler, |t| scale.x(t));
            // The times, where no number of the grid's is.
            if let Some(times) = &grid.times {
                let taken: Vec<f32> = marks.iter().map(|m| scale.x(m.tick)).collect();
                let free = |x: f32| taken.iter().all(|t| (x - t).abs() >= crate::ruler::TIME_W);
                let shown: Vec<_> = times
                    .labels
                    .iter()
                    .map(|(l, text)| (studio.tick_x_f(g, l.at), text.as_str()))
                    .filter(|(x, _)| free(*x))
                    .collect();
                crate::ruler::paint_times(pen, &shown, g.ruler);
            }
        }
        crate::context::marker::paint(pen, studio, g);
    });
}

/// The song ticks at the lanes' left and right edges.
fn arrange_span(
    studio: &Studio,
    g: &Geometry,
) -> (xirius_core::time::Tick, xirius_core::time::Tick) {
    (
        studio.px_to_ticks(studio.scroll.0),
        studio.px_to_ticks(studio.scroll.0 + g.lanes.w),
    )
}

fn row_face(studio: &Studio, c: &Colors, row: &Row) -> Hsla {
    // Every selected row is lit the same, one or several, with no outline (UI-340).
    if studio.is_picked(row.kind) {
        c.track_sel
    } else if studio.hover_row() == Some(row.kind) {
        c.hover
    } else {
        c.paper
    }
}

fn paint_rows(pen: &mut Pen, studio: &Studio, g: &Geometry, grid: &Grid) {
    let c = pen.c;
    let project = studio.project();
    let column_region = region(g, g.column);
    let lanes_region = region(g, g.lanes);
    pen.round(column_region, c.surface_2, Radius::S);
    pen.round(lanes_region, c.surface_2, Radius::S);
    let tree = studio.tree();

    pen.mask(column_region, |pen| {
        for row in &tree.rows {
            let top = studio.row_y(g, row);
            if top + row.full_h() < g.column.y || top > g.column.bottom() {
                continue;
            }
            let face = row_face(studio, pen.c, row);
            let r = Rect::new(g.column.x, top, g.column.w, row.full_h());
            pen.shape(
                r,
                face,
                Radius::S,
                Round::rows(row.round_top, row.round_bottom),
            );
            if g.panels.folded {
                paint_strip(pen, &project, g, row, top);
                continue;
            }
            paint_row_parts(pen, studio, &project, g, row, top);
            crate::arrange::paint::head(pen, studio, &project, g, row, top);
            crate::rec_area::paint_head(pen, studio, &project, g, row, top);
            crate::bounce_row::paint_head(pen, studio, &project, g, row);
        }
        for bar in tree.bars.iter().filter(|_| !g.panels.folded) {
            let state = studio.line_state(bar);
            let color = pen.c.line[LineState::ALL.iter().position(|s| *s == state).unwrap_or(0)];
            let x = g.column.x + bar.x;
            let y0 = g.column.y + bar.y0 - studio.scroll.1;
            let y1 = g.column.y + bar.y1 - studio.scroll.1;
            pen.shape(
                Rect::new(x, y0, StructureLine::THICKNESS, y1 - y0),
                color,
                StructureLine::CAP,
                Round::ALL,
            );
        }
    });

    let speed = studio.speed_preview(g);
    let speed_map = speed
        .as_ref()
        .map(|_| xirius_core::tempo::TempoMap::from_project(&project));
    let (lines, times) = (&grid.lines, &grid.times);
    let note_lines: std::borrow::Cow<[crate::grid_lines::Line]> = match times {
        Some(_) => crate::grid_lines::under_times(lines).into(),
        None => lines.into(),
    };
    pen.mask(lanes_region, |pen| {
        for row in &tree.rows {
            let top = studio.row_y(g, row);
            if top + row.full_h() < g.lanes.y || top > g.lanes.bottom() {
                continue;
            }
            let face = row_face(studio, pen.c, row);
            let r = Rect::new(g.lanes.x, top, g.lanes.w, row.full_h());
            let round = Round::rows(row.round_top, row.round_bottom);
            pen.shape(r, face, Radius::S, round);
            crate::grid_lines::paint(pen, &note_lines, top, r.h, |t| studio.tick_x(g, t));
            if let Some(times) = &times {
                crate::grid_lines::paint_times(pen, &times.lines, lines, top, r.h, |t| {
                    studio.tick_x_f(g, t)
                });
            }
            if let RowKind::Folder(f) = row.kind {
                crate::arrange::folder_band::paint(pen, studio, &project, g, row, f);
            }
            if let RowKind::Track(t) = row.kind
                && let Some(track) = project.tracks.get(&t)
            {
                let stacks = xirius_core::takes::stacks(track);
                let edits = crate::arrange::clipfx_paint::drag_edits(studio, g);
                for clip in &track.clips {
                    // An Alt drag on the edge shows the clip at its new speed.
                    let stretched = match (&speed, &speed_map) {
                        (Some((percent, ids)), Some(map)) if ids.contains(&clip.id) => {
                            Some(crate::arrange::speed::sped(map, clip, *percent))
                        }
                        _ => None,
                    };
                    let clip = stretched.as_ref().unwrap_or(clip);
                    let cr = studio.clip_rect(g, row, clip);
                    if cr.right() < g.lanes.x || cr.x > g.lanes.right() {
                        continue;
                    }
                    // A take that does not play is in its row under the track, not over the one
                    // that does.
                    if clip.muted && in_stack(&stacks, clip) {
                        continue;
                    }
                    let selected = studio.selected_clip == Some((t, clip.id))
                        || studio.arrange.clips.contains(&(t, clip.id));
                    // The take that plays carries how many takes lie under it (REC-006).
                    let takes = takes_under(&stacks, clip);
                    // The waveform stands as tall as the clip plays (UI-310).
                    let height = crate::wave::Height::shown(
                        &project,
                        track,
                        clip,
                        &edits,
                        studio.wave_zoom,
                        cr.x,
                    );
                    paint_clip(pen, track, clip, cr, selected, takes, &height, studio);
                    crate::arrange::speed::paint_mark(pen, studio, clip.id, cr);
                    crate::arrange::clip_loop::paint_mark(pen, studio, clip, cr);
                }
                crate::arrange::clipfx_paint::paint_track(pen, studio, &project, g, row, track);
                crate::arrange::paint::lanes(pen, studio, &project, g, row, track, top);
                crate::rec_area::paint_lanes(pen, studio, &project, g, row, track, top);
                crate::bounce_row::paint_lanes(pen, studio, &project, g, row);
            }
        }
        crate::arrange::paint::loop_shade(pen, studio, &project, g);
    });
}

/// A row of the folded track list (UI-066): a bar of the track's colour and nothing else; the
/// name comes as a tip. A folder has no colour of its own, so its bar is neutral.
fn paint_strip(pen: &mut Pen, project: &Project, g: &Geometry, row: &Row, top: f32) {
    let c = pen.c;
    let inset = Space::X1.px() + 2.0;
    let color = match row.kind {
        RowKind::Track(t) => project.tracks.get(&t).map_or(c.line[0], |t| color_of(c, t)),
        RowKind::Folder(_) => c.line[0],
    };
    let w = (g.column.w - Space::X2.px() * 2.0).max(2.0);
    pen.round(
        Rect::new(
            g.column.x + (g.column.w - w) / 2.0,
            top + inset,
            w,
            (row.h - inset * 2.0).max(2.0),
        ),
        color,
        Radius::S,
    );
}

fn paint_row_parts(
    pen: &mut Pen,
    studio: &Studio,
    project: &Project,
    g: &Geometry,
    row: &Row,
    top: f32,
) {
    let parts = RowParts::of(row);
    let at = |r: Rect| Rect::new(g.column.x + r.x, top + r.y, r.w, r.h);
    let c = pen.c;
    match row.kind {
        RowKind::Folder(f) => {
            let Some(folder) = project.folders.get(&f) else {
                return;
            };
            if let Some(chevron) = parts.chevron {
                let name = if studio.is_collapsed(f) {
                    "chevron-right"
                } else {
                    "chevron-down"
                };
                let hot = studio.hover == Hit::Chevron(f);
                pen.icon(name, at(chevron), if hot { c.ink_strong } else { c.ink });
            }
            pen.icon("folder", at(parts.icon), c.ink);
            // The name stops before the output bus button (EDT-073).
            let bus = studio.folder_bus_rect(row, f);
            let name_right = parts.name_right.min(bus.x - Space::X1.px());
            let name_r = Rect::new(
                g.column.x + parts.name_x,
                top,
                name_right - parts.name_x,
                row.h,
            );
            pen.mask(name_r, |pen| {
                pen.text(
                    &folder.name,
                    FontSize::Body,
                    Weight::Semibold,
                    pen.c.ink_strong,
                    name_r,
                    Align::Left,
                );
            });
            crate::folder_bus::paint(pen, studio, project, g, row, top, f);
        }
        RowKind::Track(t) => {
            let Some(track) = project.tracks.get(&t) else {
                return;
            };
            // The colour square: the one place a track's colour is read from; it opens the
            // colour choice. The kind icon beside it stays neutral.
            if let Some(cell) = parts.swatch {
                let cell = at(cell);
                if studio.hover == Hit::TrackColor(t) {
                    pen.round(cell, c.hover, Radius::S);
                }
                let m = RowParts::SWATCH_MARK;
                let mark = Rect::new(
                    cell.x + (cell.w - m) / 2.0,
                    cell.y + (cell.h - m) / 2.0,
                    m,
                    m,
                );
                pen.round(mark, color_of(c, track), Radius::Xs);
            }
            // A plugin of the track went down: a bolt that starts it again (PLG-053).
            if studio.crashed_on(t).is_some() {
                pen.icon("alert", at(parts.icon), c.alert);
            } else {
                pen.icon(track_icon(track), at(parts.icon), c.ink);
            }
            // A transpose or velocity of the track's own: one pill after the name. Measure both
            // the name and the pill's texts here, so the drawing, the hit test and the name's
            // field share the same rectangles (UI-312).
            let widths = MarkWidths {
                name: pen.measure(&track.name, FontSize::Body, Weight::Medium),
                full: track_mark(track)
                    .map_or(0.0, |t| pen.measure(&t, FontSize::Micro, Weight::Semibold)),
                short: track_mark_short(track)
                    .map_or(0.0, |t| pen.measure(&t, FontSize::Micro, Weight::Semibold)),
            };
            studio.set_mark_widths(track.id, widths);
            let mark = studio.row_mark(row, track);
            let area = track_name_rect(row, &mark);
            let name_r = Rect::new(g.column.x + area.x, top + area.y, area.w, area.h);
            let fitted = pen.fit(&track.name, FontSize::Body, Weight::Medium, name_r.w);
            pen.mask(name_r, |pen| {
                pen.text(
                    &fitted,
                    FontSize::Body,
                    Weight::Medium,
                    pen.c.ink_strong,
                    name_r,
                    Align::Left,
                );
            });
            if let Some(text) = mark.text.as_deref() {
                let r = track_mark_rect(row, &mark);
                let r = Rect::new(g.column.x + r.x, top + r.y, r.w, r.h);
                pen.round(r, c.surface_3, Radius::Xs);
                if text == MARK_DOT {
                    let dot = Rect::new(r.x + (r.w - 4.0) / 2.0, r.y + (r.h - 4.0) / 2.0, 4.0, 4.0);
                    pen.round(dot, c.ink, Radius::Pill);
                } else {
                    pen.text(
                        text,
                        FontSize::Micro,
                        Weight::Semibold,
                        c.ink,
                        r,
                        Align::Center,
                    );
                }
            }
            let freezing = studio.job_freezing(t).map(|j| j.fraction());
            if studio.freezable(t)
                && let Some(r) = parts.freeze
            {
                paint_freeze(pen, studio, track, at(r), freezing.is_some());
            }
            if let Some(f) = freezing {
                let line = Rect::new(
                    g.column.x + parts.name_x,
                    top + RowParts::LINE_2_Y,
                    parts.freeze.map_or(parts.name_right, |r| r.x) - parts.name_x,
                    Layout::TOGGLE,
                );
                let text = format!("フリーズ中 {}%", (f.clamp(0.0, 1.0) * 100.0).floor());
                pen.text(
                    &text,
                    FontSize::Caption,
                    Weight::Semibold,
                    c.ink,
                    line,
                    Align::Left,
                );
            }
            for (rect, label, on, lit, hot) in [
                (
                    parts.mute,
                    "M",
                    track.mute,
                    c.mute_on,
                    studio.hover == Hit::Mute(t),
                ),
                (
                    parts.solo,
                    "S",
                    track.solo,
                    c.solo_on,
                    studio.hover == Hit::Solo(t),
                ),
            ] {
                let Some(rect) = rect.filter(|_| freezing.is_none()) else {
                    continue;
                };
                let r = at(rect);
                let (face, ink) = pen.transition_pair(
                    &studio.motion,
                    studio.ai.motion,
                    r,
                    &format!("{label}-{t}"),
                    on,
                    if hot {
                        (c.hover, c.ink_strong)
                    } else {
                        (c.surface_3, c.ink_on_toggle)
                    },
                    lit,
                );
                pen.round(r, face, Radius::Clip);
                pen.text(
                    label,
                    FontSize::Caption,
                    Weight::Semibold,
                    ink,
                    r,
                    Align::Center,
                );
            }
            for (rect, icon, on, enabled) in [
                (parts.arm, "record", track.armed, true),
                (
                    parts.ear,
                    "monitor",
                    track.monitor,
                    track.kind == xirius_core::model::TrackKind::Audio,
                ),
                (
                    parts
                        .keys
                        .filter(|_| track.kind == xirius_core::model::TrackKind::Instrument),
                    "keys",
                    false,
                    true,
                ),
                (
                    parts.widen,
                    // Not a chevron: the automation's one is the only one on
                    // a track's head.
                    if row.tall { "row-shrink" } else { "row-grow" },
                    false,
                    true,
                ),
            ] {
                if let Some(r) = rect.filter(|_| freezing.is_none()) {
                    let r = at(r);
                    // The record button sent the eye here: a ring in the alert colour.
                    if icon == "record" && studio.arm_cue == Some(t) && !track.armed {
                        let ring = Rect::new(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0);
                        pen.round(ring, c.alert, Radius::Clip);
                    }
                    let lit = on && matches!(icon, "monitor" | "record");
                    let pair = if icon == "monitor" {
                        c.monitor_on
                    } else {
                        c.arm_on
                    };
                    // A switch that does not apply has no face, so not the lit ink either.
                    let ink = if enabled {
                        let (face, ink) = pen.transition_pair(
                            &studio.motion,
                            studio.ai.motion,
                            r,
                            &format!("{icon}-{t}"),
                            lit,
                            (c.surface_3, c.ink),
                            pair,
                        );
                        pen.round(r, face, Radius::Clip);
                        ink
                    } else {
                        c.ink_on_toggle
                    };
                    pen.icon(
                        icon,
                        Rect::new(
                            r.x + (r.w - Layout::ICON_SIZE) / 2.0,
                            r.y + (r.h - Layout::ICON_SIZE) / 2.0,
                            Layout::ICON_SIZE,
                            Layout::ICON_SIZE,
                        ),
                        ink,
                    );
                }
            }
            if let Some(r) = parts.output {
                let r = at(r);
                pen.round(r, c.surface_3, Radius::S);
                let label = output_label(project, track);
                let text = Rect::new(
                    r.x + Space::X2.px(),
                    r.y,
                    r.w - Layout::ICON_SIZE - Space::X3.px(),
                    r.h,
                );
                pen.mask(text, |pen| {
                    pen.text(
                        &label,
                        FontSize::Caption,
                        Weight::Medium,
                        c.ink,
                        text,
                        Align::Left,
                    );
                });
                pen.icon(
                    "chevron-down",
                    Rect::new(
                        r.right() - Layout::ICON_SIZE - Space::X1.px(),
                        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
                        Layout::ICON_SIZE,
                        Layout::ICON_SIZE,
                    ),
                    c.ink,
                );
            }
        }
    }
    for i in 0..row.depth_dots().min(Layout::DEPTH_DOTS_MAX) {
        pen.round(at(RowParts::depth_dot(row, i)), c.ink, Radius::Pill);
    }
}

/// The snowflake (EXP-004): lit while the track is frozen, a square that stops the freeze
/// while it runs.
fn paint_freeze(pen: &mut Pen, studio: &Studio, track: &Track, r: Rect, running: bool) {
    let c = pen.c;
    if running {
        pen.round(r, c.surface_3, Radius::Clip);
        let s = crate::clip_marks::STOP_SQUARE;
        let square = Rect::new(r.x + (r.w - s) / 2.0, r.y + (r.h - s) / 2.0, s, s);
        pen.round(square, c.ink, Radius::Xs);
        return;
    }
    let frozen = track.frozen.is_some();
    let ink = if frozen {
        pen.round(r, c.ring, Radius::Clip);
        c.on_fill
    } else if studio.hover == Hit::Freeze(track.id) {
        pen.round(r, c.hover, Radius::Clip);
        c.ink_strong
    } else {
        c.ink
    };
    let i = Rect::new(
        r.x + (r.w - Layout::ICON_SIZE) / 2.0,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    );
    pen.icon("freeze", i, ink);
}

pub fn track_icon(track: &Track) -> &'static str {
    use xirius_core::model::{TrackIcon, TrackKind};
    match track.icon {
        Some(TrackIcon::Drums) => "drum",
        Some(TrackIcon::Keys) => "keys",
        Some(TrackIcon::Mic) => "mic",
        Some(TrackIcon::Wave) => "wave",
        None => match track.kind {
            TrackKind::Audio => "audio",
            TrackKind::Bus => "bus",
            TrackKind::Master => "output",
            TrackKind::Instrument => "instrument",
        },
    }
}

/// The dot that stands for a value when the head has no room for the numbers (UI-312).
pub const MARK_DOT: &str = "●";

/// The smallest place a name keeps, three full-width characters. A mark never takes this.
pub fn min_name_width() -> f32 {
    FontSize::Body.px() * 3.0
}

const MARK_PAD: f32 = 4.0;
/// The padding the pill falls back to when the measured width is a little too wide (UI-312).
const MARK_TIGHT_PAD: f32 = 3.0;

/// The text widths of a track's name and of its two marks, as the text system measured them
/// while painting. `full` and `short` are the texts' widths, without the pill's padding
/// (UI-312).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MarkWidths {
    pub name: f32,
    pub full: f32,
    pub short: f32,
}

impl MarkWidths {
    /// A fallback for before anything has been painted (and for tests): close to Inter's
    /// advances, unlike the coarse [`crate::browser::text_width`] (UI-312).
    pub fn estimate(track: &Track) -> MarkWidths {
        MarkWidths {
            name: estimated_text_width(&track.name, FontSize::Body),
            full: track_mark(track).map_or(0.0, |t| estimated_text_width(&t, FontSize::Micro)),
            short: track_mark_short(track)
                .map_or(0.0, |t| estimated_text_width(&t, FontSize::Micro)),
        }
    }
}

/// A close estimate of a line's advance width, from Inter's per-character widths. It is only
/// used until a paint has measured the real thing (UI-312).
pub fn estimated_text_width(s: &str, size: FontSize) -> f32 {
    let px = size.px();
    s.chars()
        .map(|c| {
            let k = match c {
                ' ' => 0.26,
                '−' | '+' | '-' => 0.58,
                '%' => 0.78,
                '0'..='9' => 0.58,
                c if c.is_ascii_uppercase() => 0.68,
                c if c.is_ascii_lowercase() => 0.52,
                c if c.is_ascii() => 0.55,
                _ => 1.0,
            };
            px * k
        })
        .sum()
}

/// The form of the mark a row shows and where its pill goes (UI-312).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarkChoice {
    pub text: Option<String>,
    /// The pill's width, padding included.
    pub width: f32,
    /// The room between the name and the pill.
    pub gap: f32,
}

/// The mark a row has room for, given the measured (or estimated) widths: all its numbers, one
/// of them, or only the dot. The name keeps at least [`min_name_width`]. If the mark's usual
/// padding does not fit, it is tightened before the numbers are dropped (UI-312).
pub fn choose_mark(row: &crate::tree::Row, widths: MarkWidths, track: &Track) -> MarkChoice {
    let Some(full) = track_mark(track) else {
        return MarkChoice::default();
    };
    let short = track_mark_short(track).unwrap_or_else(|| full.clone());
    let parts = RowParts::of(row);
    let avail = (parts.name_right - parts.name_x).max(0.0);
    let reserve = widths.name.min(min_name_width());
    let gap = Space::X1.px();
    let gap_tight = (gap - 1.0).max(0.0);
    let room = (avail - reserve - gap).max(0.0);
    let room_tight = (avail - reserve - gap_tight).max(0.0);
    if widths.full + MARK_PAD * 2.0 <= room {
        return MarkChoice {
            text: Some(full),
            width: widths.full + MARK_PAD * 2.0,
            gap,
        };
    }
    if widths.full + MARK_TIGHT_PAD * 2.0 <= room_tight {
        return MarkChoice {
            text: Some(full),
            width: widths.full + MARK_TIGHT_PAD * 2.0,
            gap: gap_tight,
        };
    }
    if widths.short + MARK_PAD * 2.0 <= room {
        return MarkChoice {
            text: Some(short),
            width: widths.short + MARK_PAD * 2.0,
            gap,
        };
    }
    if widths.short + MARK_TIGHT_PAD * 2.0 <= room_tight {
        return MarkChoice {
            text: Some(short),
            width: widths.short + MARK_TIGHT_PAD * 2.0,
            gap: gap_tight,
        };
    }
    MarkChoice {
        text: Some(MARK_DOT.to_string()),
        width: 10.0_f32.min(room),
        gap,
    }
}

impl Studio {
    /// Remembers what a paint measured for a track's head (UI-312).
    pub(crate) fn set_mark_widths(&self, track: xirius_core::id::TrackId, widths: MarkWidths) {
        self.mark_widths.borrow_mut().insert(track, widths);
    }

    /// The mark `row` shows: the measured widths when a paint has stored them, else the
    /// estimate. The drawing, the hit test, the name's field and the balloon all use this.
    pub fn row_mark(&self, row: &crate::tree::Row, track: &Track) -> MarkChoice {
        let widths = self
            .mark_widths
            .borrow()
            .get(&track.id)
            .copied()
            .unwrap_or_else(|| MarkWidths::estimate(track));
        choose_mark(row, widths, track)
    }
}

/// Where the name of a track sits in its row (row-local), leaving the mark and the right
/// buttons their places (UI-312).
pub fn track_name_rect(row: &crate::tree::Row, mark: &MarkChoice) -> Rect {
    let parts = RowParts::of(row);
    let avail = (parts.name_right - parts.name_x).max(0.0);
    let (mark_w, gap) = if mark.text.is_some() {
        (mark.width, mark.gap)
    } else {
        (0.0, 0.0)
    };
    Rect::new(
        parts.name_x,
        RowParts::LINE_1_Y,
        (avail - mark_w - gap).max(0.0),
        Layout::ROW_ICON,
    )
}

/// Where the mark's pill sits in its row (row-local), after the name.
pub fn track_mark_rect(row: &crate::tree::Row, mark: &MarkChoice) -> Rect {
    let name = track_name_rect(row, mark);
    Rect::new(
        name.right() + mark.gap,
        RowParts::LINE_1_Y + (Layout::ROW_ICON - 13.0) / 2.0,
        mark.width,
        13.0,
    )
}

/// One value only: the transpose when it is not 0, else the velocity (UI-312).
pub fn track_mark_short(track: &Track) -> Option<String> {
    let signed = |v: i8| match v {
        0 => "0".to_string(),
        n if n > 0 => format!("+{n}"),
        n => format!("−{}", -n),
    };
    if track.transpose != 0 {
        Some(signed(track.transpose))
    } else if track.velocity != 0 {
        Some(format!("{}%", signed(track.velocity)))
    } else {
        None
    }
}

/// What the mark says when the pointer rests on it (UI-312): the values it stands for.
pub fn track_mark_tip(track: &Track) -> Option<String> {
    let signed = |v: i8| match v {
        0 => "0".to_string(),
        n if n > 0 => format!("+{n}"),
        n => format!("−{}", -n),
    };
    let mut parts = Vec::new();
    if track.transpose != 0 {
        parts.push(format!("トランスポーズ {}", signed(track.transpose)));
    }
    if track.velocity != 0 {
        parts.push(format!("ベロシティ {}%", signed(track.velocity)));
    }
    (!parts.is_empty()).then(|| parts.join("・"))
}

pub fn track_mark(track: &Track) -> Option<String> {
    let signed = |v: i8| match v {
        0 => "0".to_string(),
        n if n > 0 => format!("+{n}"),
        n => format!("−{}", -n),
    };
    let mut parts = Vec::new();
    if track.transpose != 0 {
        parts.push(signed(track.transpose));
    }
    if track.velocity != 0 {
        parts.push(format!("{}%", signed(track.velocity)));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

pub fn output_label(project: &Project, track: &Track) -> String {
    use xirius_core::model::Output;
    if let Some(label) = crate::midi_out_row::label(track) {
        return label;
    }
    match project.effective_output(track.id) {
        Some(Output::Bus(id)) => project
            .tracks
            .get(&id)
            .map_or("マスター".into(), |t| t.name.clone()),
        _ => "マスター".into(),
    }
}

const CLIP_BADGE: f32 = 12.0;
/// How wide the AI's edge on a clip or a note is (AI-030).
pub(crate) const AI_EDGE: f32 = 1.5;

/// `r` made smaller by `by` on each side.
pub(crate) fn inset(r: Rect, by: f32) -> Rect {
    Rect::new(
        r.x + by,
        r.y + by,
        (r.w - by * 2.0).max(0.0),
        (r.h - by * 2.0).max(0.0),
    )
}

/// The icon before a clip's name: a lock while the track's lanes are open and
/// its clips cannot be taken hold of (AUT-012), so a press that does not move
/// the clip is not a surprise.
pub fn clip_badge(studio: &Studio, track: xirius_core::id::TrackId) -> Option<&'static str> {
    studio.clips_locked(track).then_some("lock")
}

/// Whether `clip` is one of the takes in a stack.
pub(crate) fn in_stack(
    stacks: &[xirius_core::takes::TakeStack],
    clip: &xirius_core::model::Clip,
) -> bool {
    stacks
        .iter()
        .any(|s| s.slots.iter().any(|x| x.clip == clip.id))
}

/// The count of takes at the lower left of a clip of `r` (UI-296): the icon and the number, a
/// button that opens the take rows. None when the clip is too low to carry it.
pub(crate) fn takes_badge(r: Rect) -> Option<Rect> {
    let name_h = 16.0;
    (r.h > name_h + CLIP_BADGE * 2.0).then(|| {
        let pad = Space::X1.px();
        Rect::new(
            r.x + pad - 1.0,
            r.bottom() - pad - CLIP_BADGE - 1.0,
            CLIP_BADGE + 2.0 + 18.0 + 4.0,
            CLIP_BADGE + 2.0,
        )
    })
}

/// How many takes lie in the stack of `clip`, for the clip that plays; none for a clip outside
/// a stack and for a take that is not playing.
pub(crate) fn takes_under(
    stacks: &[xirius_core::takes::TakeStack],
    clip: &xirius_core::model::Clip,
) -> Option<usize> {
    if clip.muted {
        return None;
    }
    stacks
        .iter()
        .find(|s| s.slots.iter().any(|x| x.clip == clip.id))
        .map(|s| s.rows)
        .filter(|n| *n > 1)
}

#[allow(clippy::too_many_arguments)]
fn paint_clip(
    pen: &mut Pen,
    track: &Track,
    clip: &xirius_core::model::Clip,
    r: Rect,
    selected: bool,
    takes: Option<usize>,
    height: &crate::wave::Height,
    studio: &Studio,
) {
    let c = pen.c;
    let muted = studio.clip_muted_shown(clip.id, clip.muted);
    let faint = |x: Hsla| if muted { x.opacity(0.4) } else { x };
    // Selection is the face's brightness, never a ring (UI-007).
    let face = if selected { c.clip_sel } else { c.clip };
    if crate::ai::clip_marked(studio, clip) {
        // The AI's: a purple edge until a person touches it (AI-030).
        pen.round(r, c.ai_line, Radius::Clip);
        pen.round(inset(r, AI_EDGE), face, Radius::Clip);
    } else {
        pen.round(r, face, Radius::Clip);
    }
    if track.frozen.is_some() {
        pen.round(r, c.ring.opacity(0.14), Radius::Clip);
    }
    let state = crate::clip_marks::status(studio, clip);
    let color = faint(color_of(c, track)).opacity(crate::clip_marks::dim(state));
    let marks = crate::clip_marks::marks(track, clip);
    pen.mask(r, |pen| {
        let pad = Space::X1.px();
        let name_h = 16.0;
        let mut name_x = r.x + pad;
        if let Some(badge) = clip_badge(studio, track.id) {
            let at = Rect::new(
                name_x,
                r.y + (name_h - CLIP_BADGE) / 2.0 + 1.0,
                CLIP_BADGE,
                CLIP_BADGE,
            );
            pen.icon(badge, at, pen.c.ink);
            name_x += CLIP_BADGE + Space::X1.px();
        }
        let name_end = if marks.is_empty() {
            r.right()
        } else {
            let name_w = pen.measure(&clip.name, FontSize::Caption, Weight::Medium);
            crate::clip_marks::paint_marks(pen, &marks, r, name_w) - 2.0
        };
        let name_r = Rect::new(name_x, r.y + 1.0, (name_end - name_x).max(0.0), name_h);
        pen.mask(name_r, |pen| {
            // design-lint: allow(translucent-text-surface) フリーズの薄い色は不透明なクリップの面に重ねたもので、文字の下は不透明
            pen.text(
                &clip.name,
                FontSize::Caption,
                Weight::Medium,
                pen.c.ink,
                name_r,
                Align::Left,
            );
        });
        let inner = Rect::new(
            r.x + pad,
            r.y + name_h + 1.0,
            r.w - pad * 2.0,
            r.h - name_h - pad - 1.0,
        );
        // A looped clip shows each pass of its loop (EDT-041).
        crate::arrange::clip_loop::paint_passes(pen, studio, clip, r, inner, |pen, r, inner| {
            match &clip.body {
                ClipBody::Midi { notes, .. } => {
                    paint_mini_notes(pen, studio, clip.start, notes, r, inner, color)
                }
                ClipBody::Audio { file, offset, .. }
                    if crate::wave::paint(
                        pen, studio, clip, file, *offset, r, inner, color, height,
                    ) => {}
                _ => {
                    let bpm = xirius_session::tempo::TempoMap::from_project(&studio.project())
                        .bpm_at(clip.start);
                    let shape = crate::wave::shape(clip, bpm);
                    paint_wave(pen, clip.id, shape, r, inner, color)
                }
            }
        });
        if let Some(state) = state {
            crate::clip_marks::paint_status(pen, state, r);
        }
        if let Some(n) = takes
            && let Some(badge) = takes_badge(r)
        {
            // Lower left: the take icon and the number of takes, a button that opens the take
            // rows (UI-296); under the pointer it lights as the mock's does.
            let hot = matches!(
                studio.arrange.hover,
                crate::arrange::Spot::Rec { track: h, part: crate::rec_area::RecPart::Fold } if h == track.id
            );
            if hot {
                pen.round(badge, pen.c.clip_sel, Radius::Xs);
            }
            let ink = if hot { pen.c.ink_strong } else { pen.c.ink };
            let y = r.bottom() - pad - CLIP_BADGE;
            pen.icon(
                "layers",
                Rect::new(r.x + pad, y, CLIP_BADGE, CLIP_BADGE),
                ink,
            );
            // design-lint: allow(translucent-text-surface) フリーズの薄い色は不透明なクリップの面に重ねたもので、文字の下は不透明
            pen.text(
                &n.to_string(),
                FontSize::Caption,
                Weight::Medium,
                ink,
                Rect::new(
                    r.x + pad + CLIP_BADGE + 2.0,
                    y - 2.0,
                    24.0,
                    CLIP_BADGE + 4.0,
                ),
                Align::Left,
            );
        }
    });
}

/// What a clip holds, drawn into `inner` in `color`: the notes, or the waveform. For the take
/// rows, where the bar has no name line; a take plays through the clip it is in, so its waveform
/// stands as tall as its own gain makes it.
pub(crate) fn paint_clip_body(
    pen: &mut Pen,
    studio: &Studio,
    clip: &xirius_core::model::Clip,
    r: Rect,
    inner: Rect,
    color: Hsla,
) {
    match &clip.body {
        ClipBody::Midi { notes, .. } => {
            paint_mini_notes(pen, studio, clip.start, notes, r, inner, color)
        }
        ClipBody::Audio { file, offset, .. }
            if crate::wave::paint(
                pen,
                studio,
                clip,
                file,
                *offset,
                r,
                inner,
                color,
                &crate::wave::Height::of_gain(clip, studio.wave_zoom, r.x),
            ) => {}
        _ => {
            let bpm =
                xirius_session::tempo::TempoMap::from_project(&studio.project()).bpm_at(clip.start);
            paint_wave(pen, clip.id, crate::wave::shape(clip, bpm), r, inner, color)
        }
    }
}

fn paint_mini_notes(
    pen: &mut Pen,
    studio: &Studio,
    _start: i64,
    notes: &[xirius_core::model::Note],
    clip: Rect,
    inner: Rect,
    color: Hsla,
) {
    if notes.is_empty() || inner.h <= 0.0 {
        return;
    }
    let (lo, hi) = notes.iter().fold((127u8, 0u8), |(lo, hi), n| {
        (lo.min(n.pitch), hi.max(n.pitch))
    });
    let rows = f32::from(hi - lo) + 1.0;
    let step = (inner.h / rows).min(6.0);
    let used = step * rows;
    let top = inner.y + (inner.h - used) / 2.0;
    for n in notes {
        let x = clip.x + studio.ticks_to_px(n.start);
        let w = studio.ticks_to_px(n.length).max(2.0);
        if x + w < clip.x || x > clip.right() {
            continue;
        }
        let y = top + f32::from(hi - n.pitch) * step;
        pen.round(Rect::new(x, y, w, step.max(2.0) - 1.0), color, Radius::Xs);
    }
}

/// A stand-in waveform (the clip's file cannot be read): mirrored columns whose heights come
/// from the clip's id, so a clip always looks the same. Like the real one, it stretches with the
/// clip's speed and runs backwards when the clip does.
fn paint_wave(
    pen: &mut Pen,
    id: xirius_core::id::ClipId,
    (ratio, reverse): (f64, bool),
    clip: Rect,
    inner: Rect,
    color: Hsla,
) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    let seed = hasher.finish();
    let mid = inner.y + inner.h / 2.0;
    let col_w = 2.0;
    let gap = 1.0;
    let left = clip.x + Space::X1.px();
    let count = ((clip.right() - left) / (col_w + gap)).ceil().max(1.0);
    let mut x = left;
    let mut i = 0u64;
    while x < clip.right() {
        let at = if reverse {
            count - 1.0 - i as f32
        } else {
            i as f32
        };
        let u = at / ratio as f32;
        let k = u.floor() as u64;
        let n = (seed ^ k.wrapping_mul(0x9e37_79b9_7f4a_7c15)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        let noise = ((n >> 40) & 0xff) as f32 / 255.0;
        let swell = (u * 0.07 + (seed & 0xff) as f32).sin().abs();
        let amp = (0.15 + 0.85 * swell * (0.4 + 0.6 * noise)) * inner.h / 2.0;
        pen.round(
            Rect::new(x, mid - amp, col_w, (amp * 2.0).max(2.0)),
            color,
            Radius::Xs,
        );
        x += col_w + gap;
        i += 1;
    }
}

/// Window x of the playhead in the arrangement: the map the bar numbers use.
pub(crate) fn playhead_x(studio: &Studio, g: &Geometry) -> f32 {
    studio.ruler_scale(g).x(studio.playhead)
}

fn paint_playhead(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let x = playhead_x(studio, g);
    if x < g.lanes.x || x > g.lanes.right() {
        return;
    }
    let view = Rect::new(
        g.lanes.x,
        g.ruler.y,
        g.lanes.w,
        region(g, g.lanes).bottom() - g.ruler.y,
    );
    crate::ruler::paint_playhead(pen, studio, x, view);
}

/// The takes a recording grows (REC-001): from where it began to the
/// playhead, on each track it records into, in the alert colour. They become
/// clips when the recording stops.
fn paint_takes(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let Some(take) = &studio.transport.take else {
        return;
    };
    let project = studio.project();
    let tree = studio.tree();
    // Nothing grows during a pre-roll, and a punch range ends the take at its end.
    if studio.playhead < take.from {
        return;
    }
    let end = take
        .keep
        .and_then(|k| k.1)
        .map_or(studio.playhead, |to| studio.playhead.min(to));
    pen.mask(region(g, g.lanes), |pen| {
        for t in take.audio.iter().chain(&take.midi) {
            let Some(row) = tree.rows.iter().find(|r| r.kind == RowKind::Track(*t)) else {
                continue;
            };
            let Some(track) = project.tracks.get(t) else {
                continue;
            };
            let inset = Space::X1.px();
            let x0 = studio.tick_x(g, take.from);
            let r = Rect::new(
                x0,
                studio.row_y(g, row) + inset,
                (studio.tick_x(g, end) - x0).max(2.0),
                row.h - inset * 2.0,
            );
            paint_take(pen, studio, take.from, track, *t, r);
        }
    });
}

fn paint_take(
    pen: &mut Pen,
    studio: &Studio,
    from: i64,
    track: &Track,
    id: xirius_core::id::TrackId,
    r: Rect,
) {
    let c = pen.c;
    let color = color_of(c, track);
    pen.round(r, c.clip, Radius::Clip);
    pen.mask(r, |pen| {
        let pad = Space::X1.px();
        let name_h = 16.0;
        // A red dot at the head marks it as recording; the clip is the track's color and
        // carries the name it will have, the track's (REC-001, UI-296).
        let dot = 6.0;
        pen.round(
            Rect::new(r.x + pad, r.y + (name_h - dot) / 2.0 + 1.0, dot, dot),
            pen.c.alert,
            Radius::Pill,
        );
        pen.text(
            &track.name,
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink,
            Rect::new(
                r.x + pad * 2.0 + dot,
                r.y + 1.0,
                (r.w - pad * 2.0 - dot).max(0.0),
                name_h,
            ),
            Align::Left,
        );
        let inner = Rect::new(
            r.x + pad,
            r.y + name_h + 1.0,
            r.w - pad * 2.0,
            r.h - name_h - pad - 1.0,
        );
        if track.kind == xirius_core::model::TrackKind::Audio {
            paint_wave(
                pen,
                xirius_core::id::ClipId(id.0),
                (1.0, false),
                r,
                inner,
                color,
            );
        } else {
            // The notes played so far, where the clip's notes will be.
            let notes: Vec<xirius_core::model::Note> = studio
                .transport
                .live_notes
                .iter()
                .map(|n| xirius_core::model::Note {
                    id: xirius_core::id::NoteId(0),
                    start: (n.start - from).max(0),
                    length: n.length,
                    pitch: n.pitch,
                    velocity: 100,
                    muted: false,
                    ai: false,
                })
                .collect();
            paint_mini_notes(pen, studio, from, &notes, r, inner, color);
        }
    });
}

// ----------------------------------------------------------------- drawer

fn paint_drawer(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let c = pen.c;
    pen.round(g.drawer, c.surface_2, Radius::M);
    for tab in DrawerTab::ALL {
        let r = g.tab(tab);
        let active = studio.drawer_open && studio.tab == tab;
        let hot = studio.hover == Hit::Tab(tab);
        let face = pen.transition(
            &studio.motion,
            studio.ai.motion,
            r,
            &format!("tab-{tab:?}"),
            active,
            if hot { c.hover } else { c.surface_2 },
            c.sel,
        );
        pen.round(r, face, Radius::S);
        let ink = if active || hot { c.ink_strong } else { c.ink };
        pen.icon(tab.icon(), icon_rect(r), ink);
    }
    // What the roll is showing, beside the toggle.
    if studio.drawer_open
        && studio.tab == DrawerTab::Roll
        && let Some(target) = studio.roll_target()
    {
        let drum = studio.roll_drum();
        paint_roll_switch(pen, studio, g, drum.as_deref());
        if drum.is_none() {
            paint_roll_length(pen, studio, g);
        }
        let r = g.roll_name(drum.is_some());
        let text = format!("{} / {}", target.track.name, target.clip.name);
        if r.w > 0.0 {
            let text = pen.fit(&text, FontSize::Small, Weight::Regular, r.w);
            pen.mask(r, |pen| {
                pen.text(
                    &text,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    r,
                    Align::Right,
                );
            });
        }
    }
    if studio.drawer_open && g.drawer_body.h > 0.0 {
        let body = g.drawer_body;
        if studio.tab == DrawerTab::Mixer {
            pen.fill(body, c.surface_0);
        }
    }
}

/// The roll's "鍵盤｜ドラム" switch and, in the drum view, the button that picks the drum map.
fn paint_roll_switch(pen: &mut Pen, studio: &Studio, g: &Geometry, drum: Option<&str>) {
    let c = pen.c;
    let (keys, drums) = (g.roll_switch(false), g.roll_switch(true));
    let whole = Rect::new(keys.x - 2.0, keys.y, 60.0, keys.h);
    pen.round(whole, c.surface_0, Radius::S);
    for (on, r, icon) in [
        (drum.is_none(), keys, "piano"),
        (drum.is_some(), drums, "drum"),
    ] {
        let hot = studio.hover == Hit::RollView(r == drums);
        let (face, ink) = roll_control_colors(c, on, hot);
        pen.round(inset(r, 2.0), face, Radius::S);
        pen.icon(icon, icon_rect(r), ink);
    }
    if let Some(map) = drum {
        let r = g.roll_map();
        let hot = studio.hover == Hit::RollMap
            || studio
                .popup
                .as_ref()
                .is_some_and(|p| p.kind == crate::popup::PopupKind::Context);
        pen.round(r, if hot { c.hover } else { c.surface_0 }, Radius::S);
        let ink = if hot { c.ink_strong } else { c.ink };
        pen.text(
            xirius_core::drum::map_label(map),
            FontSize::Small,
            Weight::Medium,
            ink,
            Rect::new(
                r.x + Space::X3.px(),
                r.y,
                r.w - Layout::ICON_SIZE - Space::X3.px() - Space::X2.px(),
                r.h,
            ),
            Align::Left,
        );
        pen.icon(
            "chevron-down",
            Rect::new(
                r.right() - Layout::ICON_SIZE - Space::X1.px(),
                r.y + (r.h - Layout::ICON_SIZE) / 2.0,
                Layout::ICON_SIZE,
                Layout::ICON_SIZE,
            ),
            ink,
        );
    }
}

/// Measure-based truncation, also exercised without a window for narrow drawer names.
fn fit_text(s: &str, w: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if w <= 0.0 {
        return String::new();
    }
    if measure(s) <= w {
        return s.to_string();
    }
    if measure("…") > w {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    let cut = |n: usize| -> String { chars[..n].iter().chain(std::iter::once(&'…')).collect() };
    let (mut lo, mut hi) = (0usize, chars.len().saturating_sub(1));
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if measure(&cut(mid)) <= w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
}

/// Both mode radios and the length toggle use the approved fill/on_fill pair (UI-343).
fn roll_control_colors(c: &Colors, on: bool, hot: bool) -> (Hsla, Hsla) {
    if on {
        c.primary
    } else if hot {
        (c.hover, c.ink_strong)
    } else {
        (c.surface_0, c.ink)
    }
}

/// An idle length toggle keeps the drawer's ground; only the view radios have a tray.
fn roll_length_colors(c: &Colors, on: bool, hot: bool) -> (Hsla, Hsla) {
    if !on && !hot {
        (c.surface_2, c.ink)
    } else {
        roll_control_colors(c, on, hot)
    }
}

fn icon_rect(r: Rect) -> Rect {
    Rect::new(
        r.x + (r.w - Layout::ICON_SIZE) / 2.0,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    )
}

/// A single toggle, visible only in the keys view.
fn paint_roll_length(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let r = g.roll_length();
    let hot = matches!(studio.hover, Hit::RollLength(_));
    let on = studio.draw_length_last();
    let (face, ink) = roll_length_colors(pen.c, on, hot);
    pen.round(r, face, Radius::S);
    pen.icon("length-last", icon_rect(r), ink);
}

pub fn paint_roll(pen: &mut Pen, studio: &Studio, body: Rect) {
    let _cache = crate::opening::PaintScope::enter(studio.opening.is_some());
    let started = std::time::Instant::now();
    crate::roll::paint::paint(pen, studio, body);
    studio
        .roll
        .roll_paint_us
        .set(started.elapsed().as_micros() as u64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::is_black_key;
    use crate::song::Song;

    #[test]
    fn drawer_names_fit_without_crossing_the_controls_in_either_view() {
        for width in [1600.0, 960.0, 320.0, 200.0] {
            let g = Geometry::new(width, 600.0, true);
            for drums in [false, true] {
                let r = g.roll_name(drums);
                let end = if drums {
                    g.roll_map().right()
                } else {
                    g.roll_length().right()
                };
                assert!(r.x >= end + Space::X4.px());
                if r.w > 0.0 {
                    assert!(r.right() <= g.drawer_bar.right() - Space::X3.px());
                }
                let name = "長いトラック名 Lead Synth Double Layer With Octave Doubling / Chorus B Phrase (take 3)".repeat(8);
                let measure = |text: &str| {
                    text.chars()
                        .map(|c| if c.is_ascii() { 6.0 } else { 12.0 })
                        .sum::<f32>()
                };
                let fitted = fit_text(&name, r.w, measure);
                assert!(measure(&fitted) <= r.w);
                if !fitted.is_empty() {
                    assert!(fitted.ends_with('…'));
                }
            }
        }
        assert_eq!(
            fit_text("短い", 24.0, |t| t.chars().count() as f32 * 12.0),
            "短い"
        );
        assert_eq!(fit_text("長い", 1.0, |_| 12.0), "");
    }

    #[test]
    fn drawer_modes_and_length_use_fill_on_fill_in_both_themes() {
        use xirius_design::{Surface, Theme, contrast_ratio};
        for theme in Theme::ALL {
            let c = Colors::new(theme);
            for hot in [false, true] {
                assert_eq!(roll_control_colors(&c, true, hot), (c.fill, c.on_fill));
                assert_eq!(roll_length_colors(&c, true, hot), (c.fill, c.on_fill));
                assert_eq!(
                    roll_length_colors(&c, false, hot),
                    if hot {
                        (c.hover, c.ink_strong)
                    } else {
                        (c.surface_2, c.ink)
                    }
                );
                assert_eq!(
                    roll_control_colors(&c, false, hot),
                    if hot {
                        (c.hover, c.ink_strong)
                    } else {
                        (c.surface_0, c.ink)
                    }
                );
            }
            for ground in [Surface::S0, Surface::S2] {
                assert!(
                    contrast_ratio(Surface::Fill.color(theme), ground.color(theme)) >= 3.0,
                    "{theme:?} {ground:?}"
                );
            }
        }
    }

    #[test]
    fn tracks_come_in_the_columns_order_folders_opened() {
        let project = Song::sample().project();
        let names: Vec<&str> = ordered_tracks(&project)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Drums", "Bass", "Shaker", "Conga", "Keys", "Vocal", "Guitar"
            ]
        );
    }

    #[test]
    fn every_sample_track_has_a_palette_colour() {
        let project = Song::sample().project();
        for track in ordered_tracks(&project) {
            assert!(
                track.color.as_deref().and_then(palette_color).is_some(),
                "{}",
                track.name
            );
        }
    }

    #[test]
    fn rounding_a_row_of_a_block_leaves_the_inner_corners_square() {
        let first = Round::rows(true, false).corners(Radius::S);
        assert_eq!(f32::from(first.top_left), Radius::S.px());
        assert_eq!(f32::from(first.bottom_left), 0.0);
        let only = Round::rows(true, true).corners(Radius::S);
        assert_eq!(f32::from(only.bottom_right), Radius::S.px());
    }

    #[test]
    fn a_lit_toggle_button_draws_its_icon_in_the_ink_on_the_fill() {
        use xirius_design::{Pair, Theme, contrast_ratio, ink, surface};
        for theme in Theme::ALL {
            let c = Colors::new(theme);
            for hover in [false, true] {
                let (face, ink) = toggle_colors(&c, true, hover);
                assert_eq!(face, c.fill, "{theme:?}");
                assert_eq!(ink, c.on_fill, "{theme:?}");
                assert_eq!(toggle_colors(&c, false, hover).1, c.ink, "{theme:?}");
            }
            assert_eq!(c.primary, (c.fill, c.on_fill));
            assert_eq!(c.primary_hover, (c.fill_hover, c.on_fill));
            let lit = Pair::<surface::Fill, ink::OnFill>::new().resolve(theme);
            let hot = Pair::<surface::FillHover, ink::OnFill>::new().resolve(theme);
            for pair in [lit, hot] {
                assert!(contrast_ratio(pair.surface, pair.ink) >= 4.5, "{theme:?}");
            }
        }
    }

    #[test]
    fn black_keys_are_the_five_in_an_octave() {
        assert_eq!((0..12u8).filter(|p| is_black_key(*p)).count(), 5);
        assert!(!is_black_key(60));
        assert!(is_black_key(61));
    }
}
