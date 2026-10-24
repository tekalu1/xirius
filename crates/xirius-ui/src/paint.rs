//! Low-level painting for the spike panes.
//!
//! Everything is drawn from canvases with `paint_quad`: only the rows, clips,
//! notes and pixel columns that are in view are touched. No lines and no
//! gradients: surfaces are flat, the ruler and the grid are a change of
//! surface, and the waveform is one flat colour (UI-007 / UI-008).

use gpui::{
    App, Bounds, ContentMask, Corners, Hsla, PathBuilder, Pixels, SharedString, TextAlign, TextRun,
    Window, fill, point, px, size,
};
use xirius_design::{
    Accent, Category, FloatShadow, GridLine, GridTier, Ink, LevelMeter, LineState, Overlay, Pair,
    PianoKey, Radius, Readable, ReadableOn, StructureLine, Surface, SurfaceKind, SwitchFace, Theme,
    Toggle, TrackColor, ink, surface,
};

use crate::data::{ARRANGEMENT_S, Project, Track};
use crate::meters::{DB_CEIL, HOT_DB, METER_COUNT, MeterState, OVER_DB, db_to_pos};
use crate::notes::{HIGHEST_PITCH, LOWEST_PITCH, TICKS_PER_BEAT, is_black_key};
use crate::peaks::SAMPLE_RATE;
use crate::theme::{METRICS, paint};
use crate::viewport::{Axis, Rows};

/// 120 bpm in 4/4: a bar is two seconds, four beats.
pub const BAR_S: f64 = 2.0;
pub const BEATS_PER_BAR: f64 = 4.0;

/// Number of pitch rows in the piano roll.
pub const PITCH_ROWS: usize = (HIGHEST_PITCH - LOWEST_PITCH) as usize + 1;

/// Points of the level meter's colour kept in [`Colors::meter_ramp`].
pub const METER_RAMP: usize = 64;

/// The scroll thumb of a list on the floating paper (UI-338): the off
/// switch's track, the neutral token kept at 3:1 on the surfaces around it
/// (the surface thumb is the paper itself in the light theme).
pub fn list_thumb(theme: Theme) -> xirius_design::Color {
    SwitchFace::off(theme, false).track
}

/// A fader's cap: the surface thumb, and the paper itself in the light theme.
pub fn fader_cap(theme: Theme) -> xirius_design::Color {
    match theme {
        Theme::Dark => Overlay::ScrollThumb.color(theme),
        Theme::Light => Surface::Paper.color(theme),
    }
}

/// The face a mixer strip flashes with when a head's empty part is double-clicked: the strip is
/// selected by then, so it steps away from the selected face (UI-340).
pub fn strip_flash(theme: Theme) -> xirius_design::Color {
    Overlay::StripFlash.color(theme)
}

/// Design tokens converted once (colour conversion is too slow for inner loops).
#[derive(Clone)]
pub struct Colors {
    pub surface_0: Hsla,
    pub surface_1: Hsla,
    pub surface_2: Hsla,
    pub surface_3: Hsla,
    /// The face of a track row, between the region surface and a hovered row.
    pub paper: Hsla,
    pub hover: Hsla,
    pub sel: Hsla,
    pub clip: Hsla,
    /// A selected clip's face: brighter (darker in the light theme), no ring.
    pub clip_sel: Hsla,
    /// The face of a selected track's head and mixer strip, one or several (UI-340).
    pub track_sel: Hsla,
    /// [`strip_flash`].
    pub strip_flash: Hsla,
    /// A primary button and its hover face.
    pub fill: Hsla,
    pub fill_hover: Hsla,
    /// The primary face and its ink, at rest and under the pointer: the pair is
    /// bound by type to `ink_on_fill`, so no other ink goes on it (UI-309).
    pub primary: (Hsla, Hsla),
    pub primary_hover: (Hsla, Hsla),
    /// The keyboard focus ring (a selected clip).
    pub ring: Hsla,
    /// What the AI changed and is changing: the ring of its marks and its glow (AI-005).
    pub ai_line: Hsla,
    /// The AI's ink: its stars and badge.
    pub ink_ai: Hsla,
    pub ink: Hsla,
    pub ink_strong: Hsla,
    /// Ruler numbers and ticks: the blue-grey ink, readable on the ruler surface.
    pub ink_blue: Hsla,
    /// A mark that something moved off its stored state (the preset tag's dot, UI-322).
    pub ink_mark: Hsla,
    pub ink_on_toggle: Hsla,
    pub alert: Hsla,
    pub stale: Hsla,
    /// The close button's hover face and the ink on it.
    pub danger: Hsla,
    pub on_fill: Hsla,
    pub ok: Hsla,
    pub hot: Hsla,
    pub over: Hsla,
    pub well: Hsla,
    /// The level meter's colour at 64 points up its scale: a meter is painted
    /// as slices of these, so the one value gradient is followed exactly.
    pub meter_ramp: [Hsla; METER_RAMP],
    /// A fader's cap.
    pub thumb: Hsla,
    pub key_white: Hsla,
    pub key_black: Hsla,
    /// The note name on a white key, and on a key that is lit.
    pub key_label: Hsla,
    pub key_label_lit: Hsla,
    /// The text on a face painted in a track's colour.
    pub ink_on_track: Hsla,
    pub tracks: [Hsla; TrackColor::COUNT],
    pub waves: [Hsla; TrackColor::COUNT],
    pub wave_preview: Hsla,
    pub wave_rest: Hsla,
    /// What an arranged section (a coloured band) is painted with, and its label ink.
    pub sections: [(Hsla, Hsla); TrackColor::COUNT],
    /// The toggles when they are on: (face, label).
    pub mute_on: (Hsla, Hsla),
    pub solo_on: (Hsla, Hsla),
    pub arm_on: (Hsla, Hsla),
    pub monitor_on: (Hsla, Hsla),
    /// The off switch's track and knob, at rest and hovered (UI-317).
    pub switch_off: [(Hsla, Hsla); 2],
    /// A list's scroll thumb: [`list_thumb`], 3:1 on the floating paper in
    /// both themes (UI-338).
    pub list_thumb: Hsla,
    /// The folder range line at rest, hovered and selected (MIX-038).
    pub line: [Hsla; 3],
    /// The time grid's lines by [`xirius_design::GridTier`], strongest first: a
    /// bar's, a beat's, a split's and a finer step's (UI-339). Drawn only through
    /// [`crate::grid_lines`], with the width and place from `GridLine`.
    pub grid: [Hsla; 4],
    /// The scroll thumb's tones, grabbed or hovered and at rest.
    pub scroll_thumb: [Hsla; 2],
    /// What dims the screen under a dialog (the command palette).
    pub veil: Hsla,
    /// The soft shadow all round a floating panel, far layer first (UI-007).
    pub float: [gpui::BoxShadow; 2],
    /// Red text for what went wrong (a key conflict, a plug-in that does not load).
    pub ink_alert: Hsla,
    /// The five colours of the kinds of command (the settings' keyboard), in
    /// [`Category::ALL`] order.
    pub cat: [Hsla; 5],
    /// The theme these colours are for (the settings' faces differ by theme).
    pub theme: Theme,
}

impl Colors {
    pub fn new(theme: Theme) -> Colors {
        let meter = LevelMeter::default().gradient(theme);
        let stops = meter.stops();
        Colors {
            surface_0: paint(Surface::S0.color(theme)),
            surface_1: paint(Surface::S1.color(theme)),
            surface_2: paint(Surface::S2.color(theme)),
            surface_3: paint(Surface::S3.color(theme)),
            paper: paint(Surface::Paper.color(theme)),
            hover: paint(Surface::Hover.color(theme)),
            sel: paint(Surface::Sel.color(theme)),
            clip: paint(Surface::Clip.color(theme)),
            clip_sel: paint(Surface::ClipSel.color(theme)),
            track_sel: paint(Surface::TrackSel.color(theme)),
            strip_flash: paint(strip_flash(theme)),
            fill: paint(Surface::Fill.color(theme)),
            fill_hover: paint(Surface::FillHover.color(theme)),
            primary: resolved(Pair::<surface::Fill, ink::OnFill>::new(), theme),
            primary_hover: resolved(Pair::<surface::FillHover, ink::OnFill>::new(), theme),
            ring: paint(Accent::FocusRing.color(theme)),
            ai_line: paint(Accent::AiLine.color(theme)),
            ink_ai: paint(
                Readable::new(Surface::S1, Ink::Ai)
                    .expect("the AI ink is readable on S1")
                    .ink_color(theme),
            ),
            ink: paint(Surface::S1.text().ink_color(theme)),
            ink_strong: paint(Surface::S1.strong_text().ink_color(theme)),
            ink_blue: paint(
                Readable::new(Surface::S1, Ink::Blue)
                    .expect("Blue is readable on S1")
                    .ink_color(theme),
            ),
            ink_mark: paint(
                Readable::new(Surface::S2, Ink::Mark)
                    .expect("Mark is readable on S2")
                    .ink_color(theme),
            ),
            ink_on_toggle: paint(Toggle::off().ink_color(theme)),
            alert: paint(LevelMeter::clip_light(theme)),
            stale: paint(Accent::StaleDot.color(theme)),
            danger: paint(Surface::Danger.color(theme)),
            on_fill: paint(Surface::Danger.text().ink_color(theme)),
            ok: paint(stops[0].color),
            hot: paint(stops[1].color),
            over: paint(stops[2].color),
            well: paint(LevelMeter::well(theme)),
            meter_ramp: std::array::from_fn(|i| {
                paint(meter.color_at(i as f32 / (METER_RAMP - 1) as f32))
            }),
            thumb: paint(fader_cap(theme)),
            key_white: paint(PianoKey::White.color(theme)),
            key_black: paint(PianoKey::Black.color(theme)),
            key_label: paint(PianoKey::label_on(theme, false)),
            key_label_lit: paint(PianoKey::label_on(theme, true)),
            ink_on_track: paint(TrackColor::new(0).section(theme).label),
            tracks: std::array::from_fn(|i| paint(TrackColor::new(i).content(theme))),
            waves: std::array::from_fn(|i| {
                paint(xirius_design::WaveColor::for_track(
                    TrackColor::new(i),
                    theme,
                ))
            }),
            wave_preview: paint(xirius_design::WaveColor::preview(theme)),
            wave_rest: paint(xirius_design::WaveColor::rest(theme)),
            sections: std::array::from_fn(|i| {
                let face = TrackColor::new(i).section(theme);
                (paint(face.fill), paint(face.label))
            }),
            mute_on: toggle_face(Toggle::Mute, theme),
            solo_on: toggle_face(Toggle::Solo, theme),
            arm_on: toggle_face(Toggle::Arm, theme),
            monitor_on: toggle_face(Toggle::Monitor, theme),
            switch_off: [false, true].map(|hovered| {
                let face = SwitchFace::off(theme, hovered);
                (paint(face.track), paint(face.knob))
            }),
            list_thumb: paint(list_thumb(theme)),
            line: LineState::ALL.map(|state| paint(StructureLine::color(state, theme))),
            grid: GridTier::ALL.map(|tier| paint(GridLine::color(tier, theme))),
            scroll_thumb: [
                paint(Overlay::ScrollThumbHover.color(theme)),
                paint(Overlay::ScrollThumb.color(theme)),
            ],
            veil: paint(Overlay::Veil.color(theme)),
            float: FloatShadow::layers(theme).map(|l| gpui::BoxShadow {
                color: paint(l.color),
                offset: point(px(0.0), px(l.offset_y)),
                blur_radius: px(l.blur),
                spread_radius: px(0.0),
                inset: false,
            }),
            ink_alert: paint(
                Readable::new(Surface::S1, Ink::Alert)
                    .expect("Alert is readable on S1")
                    .ink_color(theme),
            ),
            cat: Category::ALL.map(|k| paint(k.color(theme))),
            theme,
        }
    }
}

fn resolved<S: SurfaceKind, I: ReadableOn<S>>(pair: Pair<S, I>, theme: Theme) -> (Hsla, Hsla) {
    let pair = pair.resolve(theme);
    (paint(pair.surface), paint(pair.ink))
}

fn toggle_face(toggle: Toggle, theme: Theme) -> (Hsla, Hsla) {
    let face = toggle.on(theme);
    (paint(face.fill), paint(face.label))
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

fn f(p: Pixels) -> f32 {
    f32::from(p)
}

/// One line of text at `(x, y)` (top-left).
pub fn text(
    window: &mut Window,
    cx: &mut App,
    font: &SharedString,
    s: &str,
    size_px: f32,
    color: Hsla,
    at: (f32, f32),
) {
    let run = TextRun {
        len: s.len(),
        font: gpui::font(font.clone()),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(s.to_string()),
        px(size_px),
        &[run],
        None,
    );
    let _ = line.paint(
        point(px(at.0), px(at.1)),
        px(size_px * 1.3),
        TextAlign::Left,
        None,
        window,
        cx,
    );
}

fn masked(window: &mut Window, bounds: Bounds<Pixels>, paint: impl FnOnce(&mut Window)) {
    window.with_content_mask(Some(ContentMask { bounds }), paint);
}

// ---------------------------------------------------------------- timeline

#[derive(Clone, Copy)]
pub struct TimelineView {
    pub axis: Axis,
    pub rows: Rows,
    pub playhead_s: f64,
    pub track_h: f32,
}

pub fn paint_headers(
    window: &mut Window,
    cx: &mut App,
    b: Bounds<Pixels>,
    view: &TimelineView,
    project: &Project,
    c: &Colors,
    font: &SharedString,
) {
    let (left, top, w) = (f(b.origin.x), f(b.origin.y), f(b.size.width));
    let view_h = f64::from(f(b.size.height) - METRICS.ruler_h);
    window.paint_quad(fill(b, c.surface_0));
    masked(window, b, |window| {
        let th = view.track_h;
        let text_size = (th * 0.55).clamp(9.0, METRICS.text_size);
        for i in view.rows.visible(view_h) {
            let y = top + METRICS.ruler_h + view.rows.row_top(i) as f32;
            let track = &project.tracks[i];
            window.paint_quad(fill(rect(left, y, w - METRICS.track_gap, th), c.surface_1));
            let chip = (th * 0.3).clamp(5.0, 10.0);
            let mut dot = fill(
                rect(left + 8.0, y + (th - chip) / 2.0, chip, chip),
                c.tracks[track.color % c.tracks.len()],
            );
            dot.corner_radii = Corners::all(px(Radius::Xs.px()));
            window.paint_quad(dot);
            text(
                window,
                cx,
                font,
                &track.name,
                text_size,
                c.ink,
                (left + 8.0 + chip + 8.0, y + (th - text_size * 1.3) / 2.0),
            );
            if th >= 24.0 {
                for (k, label) in ["M", "S"].into_iter().enumerate() {
                    let bx = left + w - METRICS.track_gap - 24.0 - k as f32 * 22.0;
                    let by = y + (th - 18.0) / 2.0;
                    let mut btn = fill(rect(bx, by, 18.0, 18.0), c.surface_3);
                    btn.corner_radii = Corners::all(px(Radius::Clip.px()));
                    window.paint_quad(btn);
                    text(
                        window,
                        cx,
                        font,
                        label,
                        10.0,
                        c.ink_on_toggle,
                        (bx + 5.5, by + 2.5),
                    );
                }
            }
        }
        // The corner above the list, drawn last so rows never poke into it.
        window.paint_quad(fill(rect(left, top, w, METRICS.ruler_h), c.surface_0));
    });
}

pub fn paint_timeline(
    window: &mut Window,
    cx: &mut App,
    b: Bounds<Pixels>,
    view: &TimelineView,
    project: &Project,
    c: &Colors,
    font: &SharedString,
) {
    let (left, top, w, h) = (
        f(b.origin.x),
        f(b.origin.y),
        f(b.size.width),
        f(b.size.height),
    );
    let right = left + w;
    let view_h = f64::from(h - METRICS.ruler_h);
    let axis = &view.axis;
    let from_s = axis.to_unit(0.0);
    let to_s = axis.to_unit(f64::from(w));
    window.paint_quad(fill(b, c.surface_0));
    masked(window, b, |window| {
        let th = view.track_h;
        for i in view.rows.visible(view_h) {
            let y = top + METRICS.ruler_h + view.rows.row_top(i) as f32;
            window.paint_quad(fill(rect(left, y, w, th), c.surface_2));
            let track = &project.tracks[i];
            let color = c.tracks[track.color % c.tracks.len()];
            for clip in track.visible_clips(from_s, to_s) {
                let x0 = left + axis.to_px(clip.start_s) as f32;
                let x1 = left + axis.to_px(clip.end_s()) as f32;
                let vx0 = x0.max(left);
                let vx1 = x1.min(right);
                if vx1 <= vx0 {
                    continue;
                }
                let vw = (vx1 - vx0).max(1.0);
                paint_clip(window, c, (vx0, y, vw, th), x0 >= left, x1 <= right);
                paint_waveform(
                    window,
                    track,
                    clip.start_s,
                    clip.src_s,
                    axis,
                    left,
                    vx0,
                    vx1,
                    y,
                    th,
                    color,
                );
            }
        }
        paint_ruler(window, cx, b, view, c, font);
        paint_playhead(window, b, view, c);
    });
}

/// A clip is a neutral rounded surface; the track colour appears only in
/// what is drawn on it (waveform, notes).
fn paint_clip(
    window: &mut Window,
    c: &Colors,
    rect_px: (f32, f32, f32, f32),
    round_left: bool,
    round_right: bool,
) {
    let (x, y, w, th) = rect_px;
    let r = Radius::Clip.px().min(th / 3.0);
    let l = if round_left { px(r) } else { px(0.0) };
    let rr = if round_right { px(r) } else { px(0.0) };
    let mut face = fill(rect(x, y, w, th), c.clip);
    face.corner_radii = Corners {
        top_left: l,
        top_right: rr,
        bottom_left: l,
        bottom_right: rr,
    };
    window.paint_quad(face);
}

/// One flat-colour quad per pixel column, from the pre-reduced peaks.
#[allow(clippy::too_many_arguments)]
fn paint_waveform(
    window: &mut Window,
    track: &Track,
    clip_start_s: f64,
    clip_src_s: f64,
    axis: &Axis,
    view_left: f32,
    x0: f32,
    x1: f32,
    y: f32,
    th: f32,
    color: Hsla,
) {
    let amp = th / 2.0 - 2.0;
    if amp < 1.0 {
        return;
    }
    let mid = y + th / 2.0;
    let k = amp / 127.0;
    let samples_per_px = SAMPLE_RATE / axis.scale;
    let level = track.peaks.level_for(samples_per_px);
    let first = x0.floor();
    let last = x1.ceil();
    let mut x = first;
    while x < last {
        let t = axis.to_unit(f64::from(x - view_left));
        let start = (t - clip_start_s + clip_src_s) * SAMPLE_RATE;
        if let Some(pk) = track
            .peaks
            .reduce(level, start.max(0.0), start.max(0.0) + samples_per_px)
        {
            let top = mid - f32::from(pk.max) * k;
            let bottom = mid - f32::from(pk.min) * k;
            window.paint_quad(fill(rect(x, top, 1.0, (bottom - top).max(1.0)), color));
        }
        x += 1.0;
    }
}

fn paint_ruler(
    window: &mut Window,
    cx: &mut App,
    b: Bounds<Pixels>,
    view: &TimelineView,
    c: &Colors,
    font: &SharedString,
) {
    let (left, top, w) = (f(b.origin.x), f(b.origin.y), f(b.size.width));
    window.paint_quad(fill(rect(left, top, w, METRICS.ruler_h), c.surface_1));
    let axis = &view.axis;
    let step_bars = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0]
        .into_iter()
        .find(|s| s * BAR_S * axis.scale >= 72.0)
        .unwrap_or(64.0);
    let step_s = step_bars * BAR_S;
    let from_s = axis.to_unit(0.0).max(0.0);
    let to_s = axis.to_unit(f64::from(w)).min(ARRANGEMENT_S);
    let mut n = (from_s / step_s).floor() as i64;
    loop {
        let t = n as f64 * step_s;
        if t > to_s {
            break;
        }
        let x = left + axis.to_px(t) as f32;
        if x >= left - 40.0 {
            window.paint_quad(fill(
                rect(x.round(), top + METRICS.ruler_h - 6.0, 1.0, 6.0),
                c.ink_blue,
            ));
            let label = format!("{}", (t / BAR_S).round() as i64 + 1);
            text(
                window,
                cx,
                font,
                &label,
                METRICS.small_text_size,
                c.ink_blue,
                (x + 5.0, top + 5.0),
            );
        }
        n += 1;
    }
}

fn paint_playhead(window: &mut Window, b: Bounds<Pixels>, view: &TimelineView, c: &Colors) {
    let (left, top, w, h) = (
        f(b.origin.x),
        f(b.origin.y),
        f(b.size.width),
        f(b.size.height),
    );
    let x = left + view.axis.to_px(view.playhead_s) as f32;
    if x < left - 6.0 || x > left + w + 6.0 {
        return;
    }
    window.paint_quad(fill(
        rect(x - METRICS.playhead_w / 2.0, top, METRICS.playhead_w, h),
        c.ink_strong,
    ));
    paint_head(window, x, top, c.ink_strong);
}

/// The small downward triangle at the top of a playhead.
fn paint_head(window: &mut Window, x: f32, top: f32, color: Hsla) {
    let mut path = PathBuilder::fill();
    path.move_to(point(px(x - 6.0), px(top)));
    path.line_to(point(px(x + 6.0), px(top)));
    path.line_to(point(px(x), px(top + 9.0)));
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

// -------------------------------------------------------------- piano roll

#[derive(Clone, Copy)]
pub struct RollView {
    /// Axis in beats.
    pub axis: Axis,
    /// One row per pitch, highest pitch at row 0.
    pub rows: Rows,
    pub playhead_beat: f64,
}

pub fn paint_roll(
    window: &mut Window,
    cx: &mut App,
    b: Bounds<Pixels>,
    view: &RollView,
    project: &Project,
    c: &Colors,
    font: &SharedString,
) {
    let (left, top, w, h) = (
        f(b.origin.x),
        f(b.origin.y),
        f(b.size.width),
        f(b.size.height),
    );
    let keys_w = METRICS.piano_keys_w;
    let nx = left + keys_w;
    let nw = w - keys_w;
    let rh = METRICS.note_row_h;
    window.paint_quad(fill(b, c.surface_0));
    masked(window, b, |window| {
        let axis = &view.axis;
        let rows = view.rows.visible(f64::from(h));
        // Row backgrounds: black-key rows are the darker surface. Every other
        // bar steps one surface lighter; this is the grid (no lines). Rows that
        // share a shade are painted as one quad per bar, so a far-out view does
        // not pay rows × bars (UI-024).
        let bar_beats = BEATS_PER_BAR;
        let from_beat = axis.to_unit(0.0).max(0.0);
        let to_beat = axis.to_unit(f64::from(nw));
        // (row, odd-bar shade) runs of rows that share one.
        let mut runs: Vec<(usize, usize, Hsla)> = Vec::new();
        for i in rows.clone() {
            let pitch = HIGHEST_PITCH - i as u8;
            let y = top + view.rows.row_top(i) as f32;
            let black = is_black_key(pitch);
            let (even, odd) = if black {
                (c.surface_2, c.surface_1)
            } else {
                (c.surface_1, c.surface_3)
            };
            window.paint_quad(fill(rect(nx, y, nw, rh), even));
            match runs.last_mut() {
                Some((_, last, color)) if *color == odd => *last = i,
                _ => runs.push((i, i, odd)),
            }
        }
        let mut bar = (from_beat / bar_beats).floor() as i64;
        while (bar as f64) * bar_beats < to_beat {
            if bar % 2 == 1 {
                let x0 = (nx + axis.to_px(bar as f64 * bar_beats) as f32).max(nx);
                let x1 = (nx + axis.to_px((bar + 1) as f64 * bar_beats) as f32).min(nx + nw);
                if x1 > x0 {
                    for &(first, last, color) in &runs {
                        let y0 = top + view.rows.row_top(first) as f32;
                        let y1 = top + view.rows.row_top(last) as f32 + rh;
                        window.paint_quad(fill(rect(x0, y0, x1 - x0, y1 - y0), color));
                    }
                }
            }
            bar += 1;
        }
        // Notes. Far enough out, notes on a row blur into one band: quads that
        // overlap once snapped to the pixel grid are painted as one (UI-024).
        let from_tick = (from_beat * f64::from(TICKS_PER_BEAT)) as u32;
        let to_tick = (to_beat.max(0.0) * f64::from(TICKS_PER_BEAT)) as u32 + 1;
        let color = c.tracks[0];
        let ticks = f64::from(TICKS_PER_BEAT);
        // Notes of one row merge into bands across the whole span, not just while they are
        // next to each other: sorted by start, a row's notes alternate with other rows', so a
        // single `pending` would paint a quad per note when the view is far out.
        let first_row = rows.start;
        let mut pending = vec![None::<(f32, f32)>; rows.clone().count()];
        for n in project.notes.visible(from_tick, to_tick) {
            let row = usize::from(HIGHEST_PITCH - n.pitch);
            let Some(local) = row.checked_sub(first_row).filter(|l| *l < pending.len()) else {
                continue;
            };
            let x0 = nx + axis.to_px(f64::from(n.start) / ticks) as f32;
            let x1 = nx + axis.to_px(f64::from(n.end()) / ticks) as f32;
            let (cx0, cx1) = (x0.max(nx), x1.min(nx + nw));
            if cx1 <= cx0 {
                continue;
            }
            let (cx0, cx1) = (cx0.floor(), (cx1 - cx0).max(1.0) + cx0.floor());
            match &mut pending[local] {
                Some((_, px1)) if cx0 <= *px1 => *px1 = (*px1).max(cx1),
                slot => *slot = Some((cx0, cx1)),
            }
        }
        for (local, p) in pending.iter().enumerate() {
            let Some((x0, x1)) = *p else { continue };
            let y = top + view.rows.row_top(first_row + local) as f32;
            let mut q = fill(rect(x0, y + 1.0, x1 - x0, rh - 2.0), color);
            if x1 - x0 >= 5.0 {
                q.corner_radii = Corners::all(px(Radius::Xs.px()));
            }
            window.paint_quad(q);
        }
        // Keys.
        window.paint_quad(fill(rect(left, top, keys_w, h), c.surface_0));
        for i in rows {
            let pitch = HIGHEST_PITCH - i as u8;
            let y = top + view.rows.row_top(i) as f32;
            let black = is_black_key(pitch);
            let (kw, color) = if black {
                (keys_w * 0.62, c.key_black)
            } else {
                (keys_w - 2.0, c.key_white)
            };
            let mut key = fill(rect(left, y + 0.5, kw, rh - 1.0), color);
            key.corner_radii = Corners {
                top_left: px(0.0),
                bottom_left: px(0.0),
                top_right: px(Radius::Xs.px()),
                bottom_right: px(Radius::Xs.px()),
            };
            window.paint_quad(key);
            if pitch.is_multiple_of(12) {
                let label = format!("C{}", i32::from(pitch) / 12 - 1);
                text(
                    window,
                    cx,
                    font,
                    &label,
                    9.0,
                    c.key_label,
                    (left + keys_w - 22.0, y - 0.5),
                );
            }
        }
        let x = nx + axis.to_px(view.playhead_beat) as f32;
        if x >= nx && x <= nx + nw {
            window.paint_quad(fill(
                rect(x - METRICS.playhead_w / 2.0, top, METRICS.playhead_w, h),
                c.ink_strong,
            ));
        }
    });
}

// ------------------------------------------------------------------ meters

pub fn paint_meters(window: &mut Window, b: Bounds<Pixels>, state: &MeterState, c: &Colors) {
    let (left, top, w, h) = (
        f(b.origin.x),
        f(b.origin.y),
        f(b.size.width),
        f(b.size.height),
    );
    window.paint_quad(fill(b, c.surface_1));
    let pad = 12.0;
    let inner_l = left + pad;
    let inner_w = w - pad * 2.0;
    let well_top = top + pad + 6.0;
    let well_h = h - pad * 2.0 - 6.0;
    let slot = inner_w / METER_COUNT as f32;
    let bar_w = (slot - METRICS.meter_gap).max(1.0);
    let hot_at = db_to_pos(HOT_DB);
    let over_at = db_to_pos(OVER_DB);
    for ch in 0..METER_COUNT {
        let x = inner_l + ch as f32 * slot;
        let mut well = fill(rect(x, well_top, bar_w, well_h), c.well);
        well.corner_radii = Corners::all(px(Radius::Xs.px()));
        window.paint_quad(well);
        let pos = state.pos[ch];
        let bottom = well_top + well_h;
        let seg = |window: &mut Window, from: f32, to: f32, color: Hsla| {
            if to > from {
                let y0 = bottom - to * well_h;
                let y1 = bottom - from * well_h;
                window.paint_quad(fill(rect(x, y0, bar_w, y1 - y0), color));
            }
        };
        seg(window, 0.0, pos.min(hot_at), c.ok);
        seg(window, hot_at, pos.min(over_at), c.hot);
        seg(window, over_at, pos, c.over);
        let hold = state.hold[ch];
        if hold > 0.0 {
            let y = bottom - hold * well_h;
            let color = if hold >= over_at {
                c.over
            } else if hold >= hot_at {
                c.hot
            } else {
                c.ok
            };
            window.paint_quad(fill(
                rect(
                    x,
                    (y - METRICS.meter_hold_h).max(well_top),
                    bar_w,
                    METRICS.meter_hold_h,
                ),
                color,
            ));
        }
        // Clip light above the well.
        let light = if state.over[ch] > 0.0 {
            c.alert
        } else {
            c.surface_3
        };
        let mut dot = fill(rect(x, top + pad - 2.0, bar_w, 4.0), light);
        dot.corner_radii = Corners::all(px(Radius::Xs.px()));
        window.paint_quad(dot);
    }
    // Scale ceiling is kept so the zones stay in step with `meters`.
    const { assert!(DB_CEIL > OVER_DB) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_rows_cover_the_note_range() {
        assert_eq!(PITCH_ROWS, 85);
        assert_eq!(usize::from(HIGHEST_PITCH - LOWEST_PITCH) + 1, PITCH_ROWS);
    }

    #[test]
    fn colors_has_one_entry_per_track_colour() {
        let c = Colors::new(crate::theme::THEME);
        assert_eq!(c.tracks.len(), TrackColor::COUNT);
        assert_ne!(c.tracks[0], c.tracks[1]);
    }

    /// CIE L* (0-100), the measure of the surface steps (xirius-design's `surface_steps.rs`).
    fn lightness(c: xirius_design::Color) -> f64 {
        let y = c.relative_luminance();
        if y > 216.0 / 24389.0 {
            116.0 * y.cbrt() - 16.0
        } else {
            24389.0 / 27.0 * y
        }
    }

    #[test]
    fn a_flashing_strip_stands_out_from_the_selected_face_and_the_cap_from_the_selection() {
        for theme in Theme::ALL {
            let selected = Surface::TrackSel.color(theme);
            // The flash fades out in a second: at its height it is four times the step that
            // tells two still surfaces apart (1.25). The master strip is never selected.
            for face in [selected, Surface::S1.color(theme)] {
                let gap = (lightness(strip_flash(theme)) - lightness(face)).abs();
                assert!(
                    gap >= 5.0,
                    "{theme:?}: 光りと面 {face:?} の L* の差 {gap:.2}"
                );
            }
            let gap = (lightness(selected) - lightness(fader_cap(theme))).abs();
            assert!(
                gap >= 1.25,
                "{theme:?}: つまみと選んだ面の L* の差 {gap:.2}"
            );
        }
    }

    #[test]
    fn rect_keeps_its_numbers() {
        let r = rect(1.0, 2.0, 3.0, 4.0);
        assert_eq!(f(r.origin.x), 1.0);
        assert_eq!(f(r.size.height), 4.0);
    }
}
