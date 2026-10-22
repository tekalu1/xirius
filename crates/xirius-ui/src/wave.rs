//! Audio clips drawn from their files' peaks (UI-037), and zoomed in past them from the files' own
//! samples (UI-310).
//!
//! The first time a clip's file is drawn its peaks are read from the peak file kept beside the
//! song ([`xirius_session::peaks`]), or made off this thread when there is none; meanwhile the
//! clip shows a flat line, and [`poll`] (asked by [`crate::studio::Studio::idle`]) draws again
//! once they are there. A pixel column reads the coarsest level that still has a peak in it, so
//! a zoomed-out song reads few peaks and a zoomed-in one sees the single cycles. A file that
//! cannot be read (it is gone, or the song has no folder for a relative one) is drawn with the
//! stand-in waveform.
//!
//! Where a pixel is narrower than level 0 of the peaks ([`peaks::BASE`] frames), the columns are
//! the min and max of the samples themselves, and where it is narrower than a sample the samples
//! are joined by a line, with a dot on each once they are [`DOT_PX`] apart ([`detail_for`]). The
//! samples come in chunks read off this thread ([`xirius_session::samples`]); until the ones in
//! view are there, the clip is drawn from its peaks. The height is the clip's gain, fades and
//! gain line as it plays them, times the vertical zoom ([`Height`]); the sound is not changed.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Hsla, PathBuilder, point, px};
use xirius_core::clipfx::{Fade, GainPoint, envelope_db_at, fade_gain};
use xirius_core::id::ClipId;
use xirius_core::model::{Clip, ClipBody, Project, Track};
use xirius_core::time::Tick;
use xirius_design::Radius;
use xirius_session::peaks::{self, Peak, Peaks};
use xirius_session::samples::{self, CHUNK, Samples, Span};
use xirius_session::tempo::TempoMap;

use crate::arrange::clipfx::{FxEdit, FxView};
use crate::draw::Pen;
use crate::studio::{Rect, Studio};

/// Width of a column of the waveform.
pub const COLUMN: f32 = 1.0;
/// From this many pixels a sample, each sample of the line carries a dot.
pub const DOT_PX: f64 = 4.0;
/// The line joining the samples, and the dots on it.
const LINE: f32 = 1.5;
const DOT: f32 = 4.0;
/// Chunks of samples kept (each about 280 KB): five a file in view, for about 20 files. Ones
/// drawn in the last [`GRACE`] stay too, so a view that needs more does not read them again and
/// again.
const KEEP: usize = 96;
const GRACE: Duration = Duration::from_secs(2);

/// Where a file's peaks are.
#[derive(Clone)]
pub enum Look {
    Ready(Arc<Peaks>),
    /// Being made: draw a flat line until then.
    Making,
    /// The file cannot be read.
    Missing,
}

/// A chunk of a file's samples, read or not to be read.
#[derive(Clone)]
enum Chunk {
    Ready(Arc<Samples>),
    /// It cannot be read from where it is (a take past 4 GiB): the peaks are drawn.
    Failed,
}

/// Each chunk kept, and when it was last drawn.
type Chunks = HashMap<(PathBuf, u64), (Chunk, Instant)>;

thread_local! {
    static SLOTS: RefCell<HashMap<PathBuf, Look>> = RefCell::new(HashMap::new());
    static SEEN: Cell<u64> = const { Cell::new(0) };
    /// The waiting job of each file whose peaks are being made.
    static WAITING: RefCell<HashMap<PathBuf, crate::wait::Id>> = RefCell::new(HashMap::new());
    static CHUNKS: RefCell<Chunks> = RefCell::new(HashMap::new());
    /// The chunks this window waits for: one of them read is a reason to draw again.
    static ASKED: RefCell<HashSet<(PathBuf, u64)>> = RefCell::new(HashSet::new());
    static SAMPLES_SEEN: Cell<u64> = const { Cell::new(0) };
}

/// The file a clip names, on the disk: relative ones are in the song folder.
pub fn source(dir: Option<&Path>, file: &str) -> Option<PathBuf> {
    let p = Path::new(file);
    if p.is_absolute() {
        Some(p.to_path_buf())
    } else {
        dir.map(|d| d.join(p))
    }
}

/// The peaks of `path` (kept in `store`), asking for them the first time.
pub fn look(path: &Path, store: &Path) -> Look {
    if let Some(l) = SLOTS.with(|s| s.borrow().get(path).cloned()) {
        return l;
    }
    let l = if !path.is_file() {
        Look::Missing
    } else if let Some(p) = peaks::load(path, store) {
        Look::Ready(Arc::new(p))
    } else {
        peaks::request(path, store);
        let id = crate::wait::begin(crate::wait::Kind::Reflect, "波形");
        WAITING.with(|w| w.borrow_mut().insert(path.to_path_buf(), id));
        Look::Making
    };
    SLOTS.with(|s| s.borrow_mut().insert(path.to_path_buf(), l.clone()));
    l
}

/// Takes the peaks and samples read since the last call; true when a clip should be drawn again.
pub fn poll() -> bool {
    let samples = poll_samples();
    let now = peaks::finished();
    if SEEN.with(|s| s.replace(now)) == now {
        return samples;
    }
    SLOTS.with(|s| {
        let mut changed = samples;
        for (path, l) in s.borrow_mut().iter_mut() {
            if matches!(l, Look::Making)
                && let Some(built) = peaks::take(path)
            {
                *l = built.map_or(Look::Missing, Look::Ready);
                if let Some(id) = WAITING.with(|w| w.borrow_mut().remove(path)) {
                    crate::wait::end(id);
                }
                changed = true;
            }
        }
        changed
    })
}

/// Keeps the chunks of samples read since the last call; true when one this window waits for is
/// among them.
fn poll_samples() -> bool {
    let now = samples::finished();
    if SAMPLES_SEEN.with(|s| s.replace(now)) == now {
        return false;
    }
    let got = samples::take();
    if got.is_empty() {
        return false;
    }
    let waited = ASKED.with(|a| {
        let mut a = a.borrow_mut();
        got.iter().fold(false, |any, (key, _)| a.remove(key) | any)
    });
    let now = Instant::now();
    CHUNKS.with(|c| {
        let mut c = c.borrow_mut();
        for (key, read) in got {
            c.insert(key, (read.map_or(Chunk::Failed, Chunk::Ready), now));
        }
        while c.len() > KEEP {
            let Some((oldest, at)) = c
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(k, (_, at))| (k.clone(), *at))
            else {
                break;
            };
            if now.duration_since(at) < GRACE {
                break;
            }
            c.remove(&oldest);
        }
    });
    waited
}

/// The chunks that hold the frames `[a, b)` of `path`, once all of them are read; until then
/// they are asked for and the peaks are drawn.
fn chunks(path: &Path, a: f64, b: f64) -> Option<Vec<Arc<Samples>>> {
    let first = (a.max(0.0) / CHUNK as f64).floor() as u64;
    let last = ((b.max(1.0) - 1.0) / CHUNK as f64)
        .floor()
        .max(first as f64) as u64;
    let now = Instant::now();
    CHUNKS.with(|c| {
        let mut c = c.borrow_mut();
        let mut out = Vec::new();
        let mut missing = false;
        for i in first..=last {
            match c.get_mut(&(path.to_path_buf(), i)) {
                Some((Chunk::Ready(s), at)) => {
                    *at = now;
                    out.push(s.clone());
                }
                Some((Chunk::Failed, _)) => return None,
                None => {
                    samples::request(path, i);
                    ASKED.with(|a| a.borrow_mut().insert((path.to_path_buf(), i)));
                    missing = true;
                }
            }
        }
        (!missing).then_some(out)
    })
}

/// Forgets the peaks and samples read so far, so a changed file is read again (a song opened
/// anew).
pub fn forget() {
    SLOTS.with(|s| s.borrow_mut().clear());
    CHUNKS.with(|c| c.borrow_mut().clear());
    ASKED.with(|a| a.borrow_mut().clear());
    for (_, id) in WAITING.with(|w| w.take()) {
        crate::wait::end(id);
    }
}

/// Bytes of samples kept for the waveforms.
pub fn kept_bytes() -> usize {
    CHUNKS.with(|c| {
        c.borrow()
            .values()
            .map(|(chunk, _)| match chunk {
                Chunk::Ready(s) => s.bytes(),
                Chunk::Failed => 0,
            })
            .sum()
    })
}

// ---- what is drawn ------------------------------------------------------------------------

/// What a zoom draws a clip from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// A pixel holds level 0 of the peaks or more: columns of peaks.
    Peaks,
    /// A pixel holds a sample or more, but fewer than a peak: columns of samples.
    Samples,
    /// A sample is wider than a pixel: the samples joined by a line, with dots once they are
    /// [`DOT_PX`] apart.
    Line { dots: bool },
}

/// What a zoom of `frames_per_px` frames of the file a pixel draws.
pub fn detail_for(frames_per_px: f64) -> Detail {
    if frames_per_px >= f64::from(peaks::BASE) {
        Detail::Peaks
    } else if frames_per_px >= 1.0 {
        Detail::Samples
    } else {
        Detail::Line {
            dots: frames_per_px * DOT_PX <= 1.0,
        }
    }
}

/// How tall a clip's waveform stands (UI-310): the clip's gain, its fades and its gain line, as
/// the engine plays them, times the vertical zoom. 1.0 draws a full-scale sample at the edge.
#[derive(Clone, Debug, PartialEq)]
pub struct Height {
    pub gain_db: f64,
    pub fade_in: Fade,
    pub fade_out: Fade,
    pub points: Vec<GainPoint>,
    /// The clip's length in ticks.
    pub length: Tick,
    pub zoom: f32,
    /// Window x of the clip's start: a looped clip's passes are drawn shifted, the level is not.
    pub left: f32,
}

impl Height {
    /// Only the vertical zoom.
    pub fn flat(zoom: f32, left: f32) -> Height {
        Height {
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
            points: Vec::new(),
            length: 0,
            zoom,
            left,
        }
    }

    /// The level `view` shows for `clip`.
    pub fn of(view: &FxView, clip: &Clip, zoom: f32, left: f32) -> Height {
        Height {
            gain_db: view.gain_db,
            fade_in: view.fade_in,
            fade_out: view.fade_out,
            points: view.points.clone(),
            length: clip.length,
            zoom,
            left,
        }
    }

    /// The level of `clip` in `track`, with what a drag under way does to it (`edits`).
    pub fn shown(
        project: &Project,
        track: &Track,
        clip: &Clip,
        edits: &[(ClipId, FxEdit)],
        zoom: f32,
        left: f32,
    ) -> Height {
        let Some(mut view) = FxView::of(project, track, clip) else {
            return Height::flat(zoom, left);
        };
        for (id, edit) in edits {
            if *id == clip.id {
                view.apply(edit);
            }
        }
        Height::of(&view, clip, zoom, left)
    }

    /// Only the clip's own gain (a take in its row, which plays through the clip it is in).
    pub fn of_gain(clip: &Clip, zoom: f32, left: f32) -> Height {
        let gain_db = match &clip.body {
            ClipBody::Audio { gain_db, .. } => *gain_db,
            ClipBody::Midi { .. } => 0.0,
        };
        Height {
            gain_db,
            ..Height::flat(zoom, left)
        }
    }

    /// The factor at `tick` from the clip's start.
    pub fn at(&self, tick: f64) -> f32 {
        let db = self.gain_db + envelope_db_at(&self.points, tick.floor() as Tick);
        let mut g = 10f64.powf(db / 20.0);
        if !self.fade_in.is_none() && tick < self.fade_in.length as f64 {
            g *= fade_gain(tick / self.fade_in.length as f64, self.fade_in.curve);
        }
        let to_end = self.length as f64 - tick;
        if !self.fade_out.is_none() && to_end <= self.fade_out.length as f64 {
            g *= fade_gain(to_end / self.fade_out.length as f64, self.fade_out.curve);
        }
        g as f32 * self.zoom
    }
}

/// Where in its file a clip is at each pixel: the frames it plays there, as
/// [`xirius_session::build`] plays them (`offset` ticks into the sound it plays). A clip played
/// faster or slower reads its file at that speed, and one played backwards reads it from the end.
pub struct Place<'a> {
    map: &'a TempoMap,
    start: i64,
    px_per_tick: f64,
    rate: f64,
    /// The file's length in frames.
    total: f64,
    ratio: f64,
    reverse: bool,
    head: f64,
    skip: f64,
}

impl<'a> Place<'a> {
    pub fn new(
        map: &'a TempoMap,
        clip: &Clip,
        offset: i64,
        px_per_tick: f64,
        rate: u32,
        frames: u64,
    ) -> Place<'a> {
        let (ratio, reverse) = shape(clip, map.bpm_at(clip.start));
        let head = map.seconds_at(clip.start);
        Place {
            map,
            start: clip.start,
            px_per_tick,
            rate: f64::from(rate),
            total: frames as f64,
            ratio,
            reverse,
            head,
            skip: map.seconds_at(clip.start + offset) - head,
        }
    }

    /// How far into the sound it plays the clip is `px` from its left edge, in frames of the file.
    pub fn played(&self, px: f64) -> f64 {
        let tick = self.start as f64 + px / self.px_per_tick;
        let secs = self.skip + self.map.seconds_at(tick.floor() as i64) - self.head
            + (tick.fract() / f64::from(self.map.ppq()))
                * (60.0 / self.map.bpm_at(tick.floor() as i64).max(1.0));
        secs / self.ratio * self.rate
    }

    /// The frames of the file `[a, b)` the pixels `[x0, x1)` play; None past its end.
    fn frames(&self, x0: f64, x1: f64) -> Option<(f64, f64)> {
        let (f0, f1) = (self.played(x0), self.played(x1));
        if f0 >= self.total {
            return None;
        }
        Some(if self.reverse {
            ((self.total - f1).max(0.0), self.total - f0)
        } else {
            (f0, f1)
        })
    }

    /// Frames of the file a pixel holds around `px`.
    pub fn frames_per_px(&self, px: f64) -> f64 {
        (self.played(px + 1.0) - self.played(px)).abs()
    }
}

/// Columns `[from, to)` of a clip, each the span `read` gives for its frames.
fn columns_by<T>(
    place: &Place,
    from: f32,
    to: f32,
    mut read: impl FnMut(f64, f64) -> Option<T>,
) -> Vec<(f32, T)> {
    if place.px_per_tick <= 0.0 || to <= from {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut x = (from / COLUMN).floor() * COLUMN;
    while x < to {
        let Some((a, b)) = place.frames(f64::from(x), f64::from(x + COLUMN)) else {
            break;
        };
        if let Some(p) = read(a, b) {
            out.push((x, p));
        }
        x += COLUMN;
    }
    out
}

/// Min and max of each column `[from, to)` of a clip drawn `px_per_tick` wide, in pixels from
/// the clip's left edge, from the peaks (see [`Place`]).
pub fn columns(
    peaks: &Peaks,
    map: &TempoMap,
    clip: &Clip,
    offset: i64,
    px_per_tick: f64,
    from: f32,
    to: f32,
) -> Vec<(f32, Peak)> {
    let place = Place::new(map, clip, offset, px_per_tick, peaks.rate, peaks.frames);
    columns_by(&place, from, to, |a, b| {
        peaks.reduce(peaks.level_for(b - a), a, b)
    })
}

/// Min and max of the frames `[a, b)` over `chunks`.
fn span_of(chunks: &[Arc<Samples>], a: f64, b: f64) -> Option<Span> {
    chunks
        .iter()
        .filter(|c| (c.start as f64) < b.ceil() && (c.end() as f64) > a.floor())
        .filter_map(|c| c.reduce(a, b))
        .reduce(|x, y| Span {
            min: x.min.min(y.min),
            max: x.max.max(y.max),
        })
}

/// Columns of a clip from its samples: each the min and max of its frames and the one before
/// them, so the columns join where the sound crosses between two of them.
pub fn sample_columns(
    place: &Place,
    chunks: &[Arc<Samples>],
    from: f32,
    to: f32,
) -> Vec<(f32, Span)> {
    let back = if place.reverse { 0.0 } else { 1.0 };
    let ahead = if place.reverse { 1.0 } else { 0.0 };
    columns_by(place, from, to, |a, b| {
        span_of(chunks, (a - back).max(0.0), b + ahead)
    })
}

/// The samples a clip plays in pixels `[from, to)` (and one either side): the x of each, from the
/// clip's left edge, and its span.
pub fn sample_points(
    place: &Place,
    chunks: &[Arc<Samples>],
    from: f32,
    to: f32,
) -> Vec<(f32, Span)> {
    if place.px_per_tick <= 0.0 || to <= from {
        return Vec::new();
    }
    // The pixel of a played frame: the frames a few pixels apart, joined by straight lines.
    const STEP: f64 = 16.0;
    let (from, to) = (f64::from(from), f64::from(to));
    let marks: Vec<(f64, f64)> =
        std::iter::successors(Some(from - STEP), |x| (*x < to + STEP).then_some(x + STEP))
            .map(|x| (x, place.played(x)))
            .collect();
    let px_of = |f: f64| {
        let i = marks
            .partition_point(|m| m.1 <= f)
            .clamp(1, marks.len() - 1);
        let ((x0, f0), (x1, f1)) = (marks[i - 1], marks[i]);
        x0 + (f - f0) / (f1 - f0).max(1e-12) * (x1 - x0)
    };
    let first = place.played(from).floor() - 1.0;
    let last = place.played(to).ceil() + 1.0;
    let mut out = Vec::new();
    let mut k = first.max(0.0);
    while k <= last && k < place.total {
        let frame = if place.reverse {
            place.total - 1.0 - k
        } else {
            k
        };
        if let Some(s) = chunks.iter().find_map(|c| c.at(frame as u64)) {
            out.push((px_of(k) as f32, s));
        }
        k += 1.0;
    }
    out
}

/// Output seconds per source second of `clip` where the song plays at `bpm`, and whether it
/// plays backwards.
pub fn shape(clip: &Clip, bpm: f64) -> (f64, bool) {
    match &clip.body {
        ClipBody::Audio { process, .. } => (process.ratio(bpm).max(1e-6), process.reverse),
        ClipBody::Midi { .. } => (1.0, false),
    }
}

/// The y of `v` (full scale 1.0) times `g` in `inner`, kept inside it.
fn y_of(inner: Rect, v: f32, g: f32) -> f32 {
    let mid = inner.y + inner.h / 2.0;
    let half = (inner.h / 2.0).max(1.0);
    (mid - v * g * half).clamp(inner.y, inner.bottom())
}

/// Draws an audio clip's waveform in `inner` from its file's peaks, or its samples where they are
/// finer; false when the file cannot be read, so the caller draws the stand-in.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    pen: &mut Pen,
    studio: &Studio,
    clip: &Clip,
    file: &str,
    offset: i64,
    r: Rect,
    inner: Rect,
    color: Hsla,
    height: &Height,
) -> bool {
    let dir = studio.song().dir();
    let Some(path) = source(dir.as_deref(), file) else {
        return false;
    };
    let store = peaks::store_dir(dir.as_deref());
    let mid = inner.y + inner.h / 2.0;
    let peaks = match look(&path, &store) {
        Look::Missing => return false,
        Look::Making => {
            pen.fill(Rect::new(r.x, mid, r.w, 1.0), color.opacity(0.6));
            crate::wait::hatch(pen, inner, color.opacity(0.35));
            return true;
        }
        Look::Ready(p) => p,
    };
    let project = studio.project();
    let map = TempoMap::from_project(&project);
    let px_per_tick = f64::from(studio.beat_w) / f64::from(project.ppq);
    // Only the columns inside the window.
    let width = f32::from(pen.window.viewport_size().width);
    let from = (-r.x).max(0.0);
    let to = r.w.min(width - r.x);
    if to <= from {
        return true;
    }
    let place = Place::new(&map, clip, offset, px_per_tick, peaks.rate, peaks.frames);
    let level = |x: f32| height.at(f64::from(r.x + x - height.left) / px_per_tick);
    let detail = detail_for(place.frames_per_px(f64::from(from + to) / 2.0));
    let held = match detail {
        Detail::Peaks => None,
        _ => place
            .frames(f64::from(from) - 2.0, f64::from(to) + 2.0)
            .and_then(|(a, b)| chunks(&path, a - 2.0, (b + 2.0).min(place.total))),
    };
    match (detail, held) {
        (Detail::Line { dots }, Some(held)) => {
            let pts = sample_points(&place, &held, from, to);
            pen.fill(
                Rect::new(r.x + from, mid.round(), to - from, 1.0),
                color.opacity(0.35),
            );
            let line = |pick: fn(Span) -> i16| -> Vec<(f32, f32)> {
                pts.iter()
                    .map(|(x, s)| {
                        let v = f32::from(pick(*s)) / 32767.0;
                        (r.x + x, y_of(inner, v, level(*x)))
                    })
                    .collect()
            };
            let top = line(|s| s.max);
            stroke(pen, &top, color);
            let two = pts.iter().any(|(_, s)| s.min != s.max);
            let bottom = if two { line(|s| s.min) } else { Vec::new() };
            stroke(pen, &bottom, color);
            if dots {
                for (x, y) in top.iter().chain(&bottom) {
                    pen.round(
                        Rect::new(x - DOT / 2.0, y - DOT / 2.0, DOT, DOT),
                        color,
                        Radius::Pill,
                    );
                }
            }
        }
        (Detail::Samples, Some(held)) => {
            for (x, s) in sample_columns(&place, &held, from, to) {
                let (lo, hi) = s.scale();
                bar(pen, r.x + x, inner, lo, hi, level(x + COLUMN / 2.0), color);
            }
        }
        _ => {
            for (x, p) in columns_by(&place, from, to, |a, b| {
                peaks.reduce(peaks.level_for(b - a), a, b)
            }) {
                let (lo, hi) = (f32::from(p.min) / 127.0, f32::from(p.max) / 127.0);
                bar(pen, r.x + x, inner, lo, hi, level(x + COLUMN / 2.0), color);
            }
        }
    }
    true
}

/// A column from `lo` to `hi` (full scale 1.0) times `g`, at least a pixel tall.
fn bar(pen: &mut Pen, x: f32, inner: Rect, lo: f32, hi: f32, g: f32, color: Hsla) {
    let top = y_of(inner, hi, g);
    let bottom = y_of(inner, lo, g);
    let top = top.min(inner.bottom() - 1.0);
    pen.fill(Rect::new(x, top, COLUMN, (bottom - top).max(1.0)), color);
}

/// The line through `pts`.
fn stroke(pen: &mut Pen, pts: &[(f32, f32)], color: Hsla) {
    let [first, rest @ ..] = pts else {
        return;
    };
    let (ox, oy) = pen.origin;
    let mut path = PathBuilder::stroke(px(LINE));
    path.move_to(point(px(ox + first.0), px(oy + first.1)));
    for p in rest {
        path.line_to(point(px(ox + p.0), px(oy + p.1)));
    }
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests;
