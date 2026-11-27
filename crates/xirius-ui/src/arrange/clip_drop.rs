//! An audio file dragged over the arrangement (UI-381, approved 2026-11-26): from the browser
//! or from the system. A see-through clip shows where it lands and how long it is, and the
//! ruler marks its start with the bar, beat and tick. Over an audio track it lands on that
//! track; over a boundary between rows it opens a seam: the rows below make room for a ghost
//! row, the new track it will be. The seam is timed the way the mixer's is (UI-362): a slow
//! pointer shows it at once, a fast one that stops after a moment, one going through not at
//! all. Over a track of another kind the file counts as the boundary below it, and below the
//! last row as the end of the song's rows (nothing moves there). The ruler, the section band
//! and the tempo row take no file: the pointer says so, and letting go there puts nothing in.
//!
//! Several files are stacked: the first where the pointer is, each other on a new track right
//! below it, in order. With Alt they go end to end on the one track.
//!
//! Let go of, the first file's ghost (and the gap it opened) stays as it was while the file is
//! read (UI-386): the clip takes its place in the frame it is put in, and comes up from the
//! ghost's opacity to full over 120 ms. A file that cannot be read leaves no ghost behind: it
//! goes, and the gap closes.

use std::path::{Path, PathBuf};
use std::time::Instant;

use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{Clip, ClipBody, Parent, Project, Track, TrackKind};
use xirius_core::time::Tick;
use xirius_design::{Duration as Tok, FontSize, Layout, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::mixer::seam::{CandOf, FIND, KEEP, LiveOf};
use crate::studio::{Geometry, Rect, Studio};
use crate::tree::{Row, RowKind};

/// A boundary between rows: the folder (or the root) and the place in it a new track takes.
pub type Key = (Parent, usize);

/// How much of the ghost clip shows: enough to read, clearly not a clip yet.
pub const GHOST_ALPHA: f32 = 0.62;

/// A boundary between rows, where a new track would go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamAt {
    pub key: Key,
    /// Its height, relative to the top of the track column, before any gap opens.
    pub edge: f32,
    /// How deep the new track sits: where the line starts.
    pub depth: usize,
    /// Below the last row: nothing moves for it.
    pub bottom: bool,
}

/// Where letting go puts the files.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aim {
    /// On this audio track, at `at`.
    Track { track: TrackId, at: Tick },
    /// On a new track at this boundary.
    Seam { seam: SeamAt, at: Tick },
    /// Nowhere: the ruler, the section band, the tempo row or off the tracks.
    Refused,
}

/// What is under the pointer, before the seam's timing has its say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Under {
    Track(TrackId),
    /// A boundary the pointer stands for wherever it is on the row (a row of another kind,
    /// below the last row).
    Seam(SeamAt),
    Refused,
}

/// The room a seam opens, growing or closing.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Gap {
    seam: SeamAt,
    from: f32,
    began: Instant,
    open: bool,
    amount: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ClipDrop {
    /// The seam on show, timed as the mixer's. A device dragged from the browser shows its
    /// line through the same one.
    pub live: LiveOf<(), Key>,
    gaps: Vec<Gap>,
    /// Where the seam on show is.
    pub shown_at: Option<SeamAt>,
    pub aim: Option<Aim>,
    /// Alt was held: several files go end to end on one track.
    pub alt: bool,
    /// Shift was held: the start is not snapped.
    pub shift: bool,
    /// The audio files held over the arrangement. Empty when none is.
    pub paths: Vec<PathBuf>,
    /// The files come from the system, not from the browser.
    pub from_os: bool,
    pub pointer: Option<(f32, f32)>,
    /// The file let go of, waiting to be read: its ghost stays.
    pub landing: Option<Landing>,
    /// The clip a landing became and when: it comes up to full opacity.
    pub landed: Option<(ClipId, Instant)>,
}

/// A file let go of whose import waits (UI-386).
#[derive(Clone, Debug, PartialEq)]
pub struct Landing {
    /// The import's ticket ([`Studio::queue_browser_import_with`]).
    pub ticket: u64,
    pub path: PathBuf,
    pub aim: Aim,
}

impl ClipDrop {
    /// The seam whose gap a landing keeps open.
    fn landing_seam(&self) -> Option<SeamAt> {
        match self.landing.as_ref()?.aim {
            Aim::Seam { seam, .. } => Some(seam),
            Aim::Track { .. } | Aim::Refused => None,
        }
    }

    /// How far a row whose top is at `tree_y` (relative to the top of the track column) moves
    /// down for the gaps above it.
    pub fn shift_of(&self, tree_y: f32) -> f32 {
        self.gaps
            .iter()
            .filter(|g| !g.seam.bottom && tree_y >= g.seam.edge)
            .map(|g| g.amount)
            .sum()
    }

    /// How far the gap at `key` is open.
    pub fn gap_of(&self, key: Key) -> f32 {
        self.gaps
            .iter()
            .filter(|g| g.seam.key == key)
            .map(|g| g.amount)
            .sum()
    }

    /// Whether a gap is open or on its way.
    pub fn has_gaps(&self) -> bool {
        !self.gaps.is_empty()
    }

    /// A height on screen back in the rows' own terms: a point inside a gap is at its seam.
    fn unshift(&self, ty: f32) -> f32 {
        let mut gaps: Vec<&Gap> = self
            .gaps
            .iter()
            .filter(|g| !g.seam.bottom && g.amount > 0.0)
            .collect();
        gaps.sort_by(|a, b| a.seam.edge.total_cmp(&b.seam.edge));
        let mut acc = 0.0;
        for g in gaps {
            let shown = g.seam.edge + acc;
            if ty >= shown + g.amount {
                acc += g.amount;
            } else if ty > shown {
                return g.seam.edge;
            } else {
                break;
            }
        }
        ty - acc
    }
}

fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// `base`, or `base 2`, `base 3`, ... when a track or folder has it: the name the new track
/// will get.
fn free_name(p: &Project, base: &str) -> String {
    let taken =
        |n: &str| p.tracks.values().any(|t| t.name == n) || p.folders.values().any(|f| f.name == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken(n))
        .unwrap_or_else(|| base.to_string())
}

/// The file's name without its folder and extension, as the clip will be named.
pub fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Audio".to_string())
}

impl Studio {
    fn clip_drop_reduced(&self) -> bool {
        self.ai.motion.reduced
    }

    /// The bar at the drop's start: the ghost's length until the file's length is known.
    fn clip_drop_bar(&self) -> Tick {
        let tm = self.project().time_map();
        (tm.bar_start(2) - tm.bar_start(1)).max(1)
    }

    /// How long the ghost of `path` is when it starts at `at`: the file's length once its
    /// peaks are made, one bar until then.
    pub(crate) fn clip_drop_length(&self, path: &Path, at: Tick) -> Tick {
        let store = self.song().peak_store();
        match crate::wave::look(path, &store) {
            crate::wave::Look::Ready(peaks) if peaks.rate > 0 => {
                let secs = peaks.frames as f64 / f64::from(peaks.rate);
                xirius_session::import::length_ticks(&self.project(), at, secs, None).max(1)
            }
            _ => self.clip_drop_bar(),
        }
    }

    /// Where the file's start falls at `x`: on the grid, unless Shift is held.
    pub(crate) fn clip_drop_at(&self, g: &Geometry, x: f32, shift: bool) -> Tick {
        if self.start_shown() || x < g.lanes.x || x > g.lanes.right() {
            return 0;
        }
        self.snap_tick(self.tick_at(g, x).max(0), shift).max(0)
    }

    /// The seam close to `(x, y)`, if any, and what is under it.
    pub(crate) fn clip_drop_find(&self, g: &Geometry, x: f32, y: f32) -> (Option<SeamAt>, Under) {
        let p = self.project();
        let end = |edge: f32| SeamAt {
            key: (Parent::Root, p.root.len()),
            edge,
            depth: 0,
            bottom: true,
        };
        if self.start_shown() {
            return (None, Under::Seam(end(0.0)));
        }
        if g.ruler.contains(x, y)
            || g.section_band.contains(x, y)
            || g.tempo.contains(x, y)
            || !(g.column.contains(x, y) || g.lanes.contains(x, y))
        {
            return (None, Under::Refused);
        }
        let d = &self.arrange.clip_drop;
        let ty = d.unshift(y - g.column.y + self.scroll.1);
        let tree = self.tree();
        let Some(last) = tree.rows.last() else {
            return (None, Under::Seam(end(0.0)));
        };
        let lb = last.bottom();
        let bottom = end(lb);
        let before = |row: &Row| -> Option<SeamAt> {
            let (parent, i) = p.find_node(super::node_of(row.kind))?;
            Some(SeamAt {
                key: (parent, i),
                edge: row.y,
                depth: row.depth,
                bottom: false,
            })
        };
        let after = |row: &Row| -> Option<SeamAt> {
            let (parent, i) = p.find_node(super::node_of(row.kind))?;
            Some(SeamAt {
                key: (parent, i + 1),
                edge: row.bottom(),
                depth: row.depth,
                bottom: row.bottom() >= lb - 0.5,
            })
        };

        let live = d.live.live().map(|s| s.spot);
        let mut near: Option<(f32, SeamAt)> = None;
        let mut consider = |s: SeamAt| {
            let reach = if Some(s.key) == live { KEEP } else { FIND };
            let dist = (ty - s.edge).abs();
            if dist > reach {
                return;
            }
            let better = match near {
                None => true,
                Some((nd, ns)) => {
                    dist < nd - 0.01
                        || (dist <= nd + 0.01 && Some(s.key) == live && Some(ns.key) != live)
                }
            };
            if better {
                near = Some((dist, s));
            }
        };
        for row in &tree.rows {
            if let Some(s) = before(row) {
                consider(s);
            }
        }
        consider(bottom);
        if let Some(s) = d.shown_at {
            consider(s);
        }

        let under = if ty >= lb {
            Under::Seam(bottom)
        } else {
            match tree.row_at(ty) {
                Some(row) => match row.kind {
                    RowKind::Track(t) => match p.tracks.get(&t) {
                        Some(track) if track.kind == TrackKind::Audio => Under::Track(t),
                        _ => after(row).map_or(Under::Refused, Under::Seam),
                    },
                    RowKind::Folder(f) => {
                        let kids = p.folders.get(&f).map_or(0, |x| x.children.len());
                        let bus = usize::from(p.row_bus(f).is_some());
                        if !self.is_collapsed(f) && kids > bus {
                            Under::Seam(SeamAt {
                                key: (Parent::Folder(f), bus),
                                edge: row.bottom(),
                                depth: row.depth + 1,
                                bottom: false,
                            })
                        } else {
                            after(row).map_or(Under::Refused, Under::Seam)
                        }
                    }
                },
                // Between two blocks: the boundary above the next row.
                None => tree
                    .rows
                    .iter()
                    .find(|r| r.y > ty)
                    .and_then(before)
                    .map_or(Under::Seam(bottom), Under::Seam),
            }
        };
        (near.map(|(_, s)| s), under)
    }

    /// Works out the seam, the gap and where the files go, from the pointer last seen.
    fn clip_drop_update(&mut self, g: &Geometry, now: Instant) {
        let Some((x, y)) = self.arrange.clip_drop.pointer else {
            return;
        };
        let (near, under) = self.clip_drop_find(g, x, y);
        let at = self.clip_drop_at(g, x, self.arrange.clip_drop.shift);
        let reduced = self.clip_drop_reduced();
        let (cx, cy, scroll_y) = (g.column.x, g.column.y, self.scroll.1);
        let cand = move |s: SeamAt| CandOf {
            track: (),
            spot: s.key,
            at: (cx, cy + s.edge - scroll_y),
            mid: true,
        };
        let forced = match under {
            Under::Seam(s) => Some(s),
            _ => None,
        };
        let pick = near.or(forced);
        let d = &mut self.arrange.clip_drop;
        if forced.is_some() {
            d.live.dragging(now, pick.map(cand), None);
        } else {
            d.live.pointer(now, near.map(cand), None, false);
        }
        d.live.advance(
            now,
            reduced,
            |_, k| pick.filter(|s| s.key == k).map(cand),
            None,
        );
        let live = d.live.live().map(|s| s.spot);
        match pick.filter(|s| Some(s.key) == live) {
            Some(s) => d.shown_at = Some(s),
            None if d.live.shown.is_none() => d.shown_at = None,
            None => {}
        }
        d.aim = Some(match pick.filter(|s| Some(s.key) == live) {
            Some(seam) => Aim::Seam { seam, at },
            None => match under {
                Under::Track(track) => Aim::Track { track, at },
                Under::Seam(seam) => Aim::Seam { seam, at },
                Under::Refused => Aim::Refused,
            },
        });
        self.clip_drop_gaps(now);
    }

    /// Opens the gap of the seam on show and closes the others; a landing keeps its own open.
    /// True while one moves.
    fn clip_drop_gaps(&mut self, now: Instant) -> bool {
        let full = self.arrange.track_h;
        let reduced = self.clip_drop_reduced();
        let d = &mut self.arrange.clip_drop;
        let target = d
            .live
            .live()
            .map(|s| s.spot)
            .and_then(|k| d.shown_at.filter(|s| s.key == k));
        let landing = d.landing_seam().map(|s| s.key);
        for gap in &mut d.gaps {
            let want = target
                .filter(|t| t.key == gap.seam.key)
                .or_else(|| (landing == Some(gap.seam.key)).then_some(gap.seam));
            if want.is_some() != gap.open {
                gap.from = gap.amount;
                gap.began = now;
                gap.open = want.is_some();
            }
            if let Some(t) = want {
                gap.seam = t;
            }
        }
        if let Some(t) = target
            && !d.gaps.iter().any(|g| g.seam.key == t.key)
        {
            d.gaps.push(Gap {
                seam: t,
                from: 0.0,
                began: now,
                open: true,
                amount: 0.0,
            });
        }
        let span = Tok::Fast.ms() as f32;
        let (mut moving, mut changed) = (false, false);
        for gap in &mut d.gaps {
            let k = if reduced {
                1.0
            } else {
                ease(now.saturating_duration_since(gap.began).as_secs_f32() * 1000.0 / span)
            };
            let amount = if gap.open {
                gap.from + (full - gap.from) * k
            } else {
                gap.from * (1.0 - k)
            };
            if (amount - gap.amount).abs() > 0.01 {
                changed |= !gap.seam.bottom;
            }
            gap.amount = amount;
            moving |= k < 1.0;
        }
        let before = d.gaps.len();
        d.gaps.retain(|g| g.open || g.amount > 0.01);
        changed |= d.gaps.len() != before;
        if changed || moving {
            self.arrange.touched();
        }
        moving
    }

    /// Audio files held at `(x, y)` over the arrangement. True: draw again.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn clip_drop_moved(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        shift: bool,
        now: Instant,
        paths: &[PathBuf],
        from_os: bool,
    ) -> bool {
        let d = &mut self.arrange.clip_drop;
        d.pointer = Some((x, y));
        d.shift = shift;
        d.from_os = from_os;
        if d.paths != paths {
            d.paths = paths.to_vec();
        }
        d.live.feed(now, x, y);
        self.clip_drop_update(g, now);
        self.arrange.touched();
        true
    }

    /// A frame: the seam's wait and grace, the gap's growing. True while something moves.
    pub(crate) fn clip_drop_tick(&mut self, g: &Geometry, now: Instant) -> bool {
        let landing = self.clip_drop_landing_tick(now);
        self.clip_drop_held_tick(g, now) || landing
    }

    /// A landing's frame: the import is looked at every frame rather than at the idle poll, so
    /// the clip takes the ghost's place as soon as it can. True while one waits or fades.
    fn clip_drop_landing_tick(&mut self, now: Instant) -> bool {
        if let Some(ticket) = self.arrange.clip_drop.landing.as_ref().map(|l| l.ticket) {
            self.poll_browser_import();
            if self.arrange.clip_drop.landing.is_some() && !self.browser_import_io.holds(ticket) {
                // It failed, or the song changed: the ghost goes and its gap closes.
                self.arrange.clip_drop.landing = None;
                self.arrange.touched();
            }
        }
        let reduced = self.clip_drop_reduced();
        let d = &mut self.arrange.clip_drop;
        let fading = d.landed.is_some_and(|(_, at)| {
            !reduced && now.saturating_duration_since(at).as_millis() < u128::from(Tok::Fast.ms())
        });
        if !fading && d.landed.take().is_some() {
            self.arrange.touched();
        }
        self.arrange.clip_drop.landing.is_some() || fading
    }

    /// The opacity the clip a landing became is painted at, if it is still coming up.
    pub(crate) fn clip_drop_landed_alpha(&self, clip: ClipId, now: Instant) -> Option<f32> {
        let (id, at) = self.arrange.clip_drop.landed?;
        if id != clip || self.clip_drop_reduced() {
            return None;
        }
        let k = now.saturating_duration_since(at).as_secs_f32() * 1000.0 / Tok::Fast.ms() as f32;
        (k < 1.0).then(|| GHOST_ALPHA + (1.0 - GHOST_ALPHA) * ease(k))
    }

    /// The file let go of with `ticket` is put in, as `clip`: its ghost and the gap go in the
    /// same frame, so the new row stands where the ghost row stood.
    pub(crate) fn clip_drop_landed(&mut self, ticket: u64, clip: Option<ClipId>) {
        let d = &mut self.arrange.clip_drop;
        if d.landing.as_ref().is_none_or(|l| l.ticket != ticket) {
            return;
        }
        let key = d.landing_seam().map(|s| s.key);
        d.landing = None;
        d.gaps.retain(|g| Some(g.seam.key) != key);
        d.landed = clip.map(|c| (c, Instant::now()));
        self.arrange.touched();
    }

    fn clip_drop_held_tick(&mut self, g: &Geometry, now: Instant) -> bool {
        let reduced = self.clip_drop_reduced();
        if self.arrange.clip_drop.paths.is_empty() {
            // A device dragged from the browser shows its line through the same seam.
            let held = self.browser.drag.as_ref().is_some_and(|d| d.moved);
            let d = &mut self.arrange.clip_drop;
            if !held {
                d.live.end_drag(now, reduced);
            }
            d.live.advance(now, reduced, |_, _| None, None);
            let gaps = self.clip_drop_gaps(now);
            return self.arrange.clip_drop.live.moving(now, reduced) || gaps;
        }
        let held = if self.arrange.clip_drop.from_os {
            self.drop_over
        } else {
            self.browser.file_drag.is_some()
        };
        if !held {
            self.clip_drop_end();
            return true;
        }
        self.clip_drop_update(g, now);
        let gaps = self.arrange.clip_drop.gaps.iter().any(|g| {
            let span = Tok::Fast.ms() as f32;
            !reduced && now.saturating_duration_since(g.began).as_secs_f32() * 1000.0 < span
        });
        self.arrange.clip_drop.live.moving(now, reduced) || gaps
    }

    /// The files are let go of or the drag is called off: the ghost, the seam and the gap go,
    /// all but a landing's.
    pub(crate) fn clip_drop_end(&mut self) {
        let d = &mut self.arrange.clip_drop;
        let had = !d.gaps.is_empty() || !d.paths.is_empty();
        d.live.clear();
        d.live.forget();
        let keep = d.landing_seam().map(|s| s.key);
        d.gaps.retain(|g| Some(g.seam.key) == keep);
        d.shown_at = None;
        d.aim = None;
        d.paths.clear();
        d.pointer = None;
        d.from_os = false;
        d.shift = false;
        d.alt = false;
        if had {
            self.arrange.touched();
        }
    }

    /// The files held are over a place that takes none: the pointer says so.
    pub(crate) fn clip_drop_refused(&self) -> bool {
        let d = &self.arrange.clip_drop;
        !d.paths.is_empty() && d.aim == Some(Aim::Refused)
    }

    /// Where letting go of `paths` at `(x, y)` puts them: what shows, while they are held;
    /// worked out afresh otherwise.
    pub(crate) fn clip_drop_aim(&self, g: &Geometry, x: f32, y: f32, shift: bool) -> Aim {
        let d = &self.arrange.clip_drop;
        if !d.paths.is_empty()
            && let Some(aim) = d.aim
        {
            return aim;
        }
        let at = self.clip_drop_at(g, x, shift);
        match self.clip_drop_find(g, x, y) {
            (Some(seam), _) if d.live.live().is_some_and(|s| s.spot == seam.key) => {
                Aim::Seam { seam, at }
            }
            (_, Under::Track(track)) => Aim::Track { track, at },
            (_, Under::Seam(seam)) => Aim::Seam { seam, at },
            (_, Under::Refused) => Aim::Refused,
        }
    }

    /// Puts the files where `aim` says: stacked, the first there and each other on a new
    /// track right below; with `alt`, end to end on the first one's track. The first file put
    /// in and its import's ticket, when one was.
    pub(crate) fn place_audio(
        &mut self,
        aim: Aim,
        paths: &[&Path],
        alt: bool,
    ) -> Option<(u64, PathBuf)> {
        let p = self.project();
        type Step = (Option<TrackId>, Tick, Option<Key>, bool);
        let plan: Vec<Step> = match aim {
            Aim::Refused => return None,
            Aim::Track { track, at } => {
                let next = p
                    .find_node(Node::Track(track))
                    .map(|(parent, i)| (parent, i + 1));
                (0..paths.len())
                    .map(|k| match k {
                        0 => (Some(track), at, None, false),
                        _ if alt => (Some(track), at, None, true),
                        _ => (None, at, next.map(|(parent, i)| (parent, i + k - 1)), false),
                    })
                    .collect()
            }
            Aim::Seam { seam, at } => {
                let (parent, i) = seam.key;
                (0..paths.len())
                    .map(|k| (None, at, Some((parent, i + k)), alt && k > 0))
                    .collect()
            }
        };
        let mut placed = Vec::new();
        let mut first = None;
        for (path, (track, at, into, chain)) in paths.iter().zip(plan) {
            match self.queue_browser_import_with(path.to_path_buf(), track, at, false, into, chain)
            {
                Ok(ticket) => {
                    first.get_or_insert_with(|| (ticket, path.to_path_buf()));
                    placed.push(crate::drop_audio::Placed {
                        line: String::new(),
                        follows: None,
                    });
                }
                Err(e) => self.error = Some(e),
            }
        }
        let n = placed.len();
        self.tell_placed(placed, format!("音声ファイル {n} 個を置きました"));
        first
    }

    /// The file let go of waits for its import with its ghost where it showed.
    pub(crate) fn clip_drop_land(&mut self, ticket: u64, path: PathBuf, aim: Aim) {
        self.arrange.clip_drop.landing = Some(Landing { ticket, path, aim });
        self.arrange.touched();
    }
}

/// The ghost of the first file held: on its track, or in the ghost row of the seam it opened,
/// with the seam's line; and its start marked on the ruler. Once let go of, the ghost alone
/// stays until the file is read.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let d = &studio.arrange.clip_drop;
    let (path, aim, landing) = match (d.paths.first(), d.aim, &d.landing) {
        (Some(path), Some(aim), _) => (path, aim, false),
        (None, _, Some(l)) => (&l.path, l.aim, true),
        _ => return,
    };
    if studio.start_shown() || studio.over != crate::independent::Over::Main {
        return;
    }
    let at = match aim {
        Aim::Track { at, .. } | Aim::Seam { at, .. } => at,
        Aim::Refused => return,
    };
    let c = pen.c;
    let p = studio.project();
    let now = Instant::now();
    let reduced = studio.ai.motion.reduced;
    let name = stem(path);
    let clip = Clip {
        id: ClipId(u64::MAX),
        name: name.clone(),
        color: None,
        start: at,
        length: studio.clip_drop_length(path, at),
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: path.to_string_lossy().into_owned(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    };
    let tracks = Rect::new(
        g.column.x,
        g.column.y,
        g.lanes.right() - g.column.x,
        g.column.h,
    );
    let tree = studio.tree();
    match aim {
        Aim::Track { track, .. } => {
            if let (Some(t), Some(row)) = (p.tracks.get(&track), tree.row_of(RowKind::Track(track)))
            {
                let r = studio.clip_rect(g, row, &clip);
                let height = crate::wave::Height::flat(studio.wave_zoom, r.x);
                pen.mask(g.lanes, |pen| {
                    crate::draw::paint_clip(
                        pen,
                        t,
                        &clip,
                        r,
                        false,
                        None,
                        &height,
                        &[],
                        studio,
                        Some(GHOST_ALPHA),
                    );
                });
            }
        }
        Aim::Seam { seam, .. } => {
            let own = d.gap_of(seam.key);
            let edge = seam.edge + d.shift_of(seam.edge) - if seam.bottom { 0.0 } else { own };
            let top = g.column.y + edge - studio.scroll.1;
            if own > 0.5 {
                let full = studio.arrange.track_h;
                let mut track =
                    Track::new(TrackId(u64::MAX), TrackKind::Audio, free_name(&p, &name));
                track.color = Some(p.next_track_color());
                let color = crate::draw::color_of(c, &track);
                let head = Rect::new(g.column.x, top, g.column.w, full);
                let lane = Rect::new(g.lanes.x, top, g.lanes.w, full);
                let pad = Space::X2.px();
                let inset = Space::X1.px();
                pen.mask(tracks, |pen| {
                    pen.mask(Rect::new(tracks.x, top, tracks.w, own), |pen| {
                        pen.round(head, c.paper, Radius::S);
                        pen.round(lane, c.paper, Radius::S);
                        // The track's color marks its icon, as on a track's own row.
                        let icon = Rect::new(
                            head.x + pad,
                            top + pad,
                            Layout::ICON_SIZE,
                            Layout::ICON_SIZE,
                        );
                        pen.icon(crate::draw::track_icon(&track), icon, color);
                        let name_x = icon.right() + inset;
                        pen.text(
                            &track.name,
                            FontSize::Body,
                            Weight::Medium,
                            c.ink_strong,
                            Rect::new(
                                name_x,
                                icon.y,
                                (head.right() - pad - name_x).max(0.0),
                                Layout::ICON_SIZE,
                            ),
                            Align::Left,
                        );
                        let r = Rect::new(
                            studio.tick_x(g, at),
                            top + inset,
                            studio.ticks_to_px(clip.length),
                            full - inset * 2.0,
                        );
                        let height = crate::wave::Height::flat(studio.wave_zoom, r.x);
                        pen.mask(g.lanes, |pen| {
                            crate::draw::paint_clip(
                                pen,
                                &track,
                                &clip,
                                r,
                                false,
                                None,
                                &height,
                                &[],
                                studio,
                                Some(GHOST_ALPHA),
                            );
                        });
                    });
                });
            }
            if !landing {
                let grow = d
                    .live
                    .look(now, reduced)
                    .filter(|l| l.spot == seam.key)
                    .map_or(1.0, |l| l.grow);
                crate::arrange::paint::paint_track_seam_at(
                    pen, studio, g, edge, seam.depth, grow, false,
                );
            }
        }
        Aim::Refused => {}
    }
    if landing {
        return;
    }
    // The start on the ruler: a line and its bar, beat and tick.
    let x = studio.tick_x(g, at);
    if x >= g.lanes.x && x <= g.lanes.right() {
        pen.round(
            Rect::new(x - 1.0, g.ruler.y, 2.0, g.ruler.h),
            c.ring,
            Radius::Pill,
        );
        let tag = p.time_map().format(at);
        let pad = Space::X1.px();
        let w = pen.measure(&tag, FontSize::Caption, Weight::Medium) + pad * 2.0;
        let h = (FontSize::Caption.px() + 4.0).min(g.ruler.h);
        let r = Rect::new(
            (x + 3.0).min(g.lanes.right() - w),
            g.ruler.y + (g.ruler.h - h) / 2.0,
            w,
            h,
        );
        pen.round(r, c.primary.0, Radius::Xs);
        pen.text(
            &tag,
            FontSize::Caption,
            Weight::Medium,
            c.primary.1,
            Rect::new(r.x + pad, r.y, w - pad * 2.0, h),
            Align::Left,
        );
    }
}
