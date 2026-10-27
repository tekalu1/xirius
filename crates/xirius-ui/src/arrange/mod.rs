//! Editing the arrangement with the pointer and the keys (ARR, AUT, EDT).
//!
//! A press finds a [`Spot`] (a clip's body or edge, the loop band, a section,
//! a tempo point, a track row, an automation point or line) and starts a
//! [`Drag`]. On every move [`Studio::plan`] works out what letting go would
//! do: the preview that is painted, the value in the balloon, and the one
//! core command it runs. So every gesture is one step of the history.
//!
//! The tool a press acts as is decided when it starts
//! ([`crate::tools::effective_tool`], or the holds changed in the settings):
//! Select with Ctrl is the range tool, and Cut, Eraser, Range and Mute with
//! Alt are Select. Snapping follows the top bar's grid; Shift turns it off
//! for the drag.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{Value, json};
use xirius_core::auto::{AutoLane, AutoMode, AutoTarget};
use xirius_core::id::{ClipId, FolderId, Node, SectionId, TrackId};
use xirius_core::model::{ClipBody, MAX_DB, MIN_DB, Parent, Project, Track, TrackKind};
use xirius_core::time::Tick;
use xirius_design::{Layout, Space};

use crate::studio::{Geometry, Hit, Rect, RowParts, Studio};
use crate::tools::{Area, Meter, Mods, Tool, round_in_bars};
use crate::tree::{LANE_H, Row, RowKind};

#[cfg(test)]
mod alt_tool_tests;
pub mod clip_loop;
pub(crate) mod clipfx;
pub mod clipfx_paint;
#[cfg(test)]
mod clipfx_tests;
#[cfg(test)]
mod dblclick_tests;
#[cfg(test)]
mod edit_tests;
mod editing;
#[cfg(test)]
mod feel2_tests;
pub mod folder_band;
mod open;
#[cfg(test)]
mod open_tests;
pub mod paint;
mod preview;
pub mod probe;
pub mod range_trim;
mod replace;
mod section;
pub mod speed;
#[cfg(test)]
mod speed_tests;
mod stroke;
#[cfg(test)]
mod tests;
pub mod write;

/// The arrangement's own view state.
#[derive(Clone, Debug)]
pub struct Arrange {
    pub swept: BTreeSet<(TrackId, ClipId)>,
    pub drag: Option<Drag>,
    /// The clips selected in the arrangement.
    pub clips: BTreeSet<(TrackId, ClipId)>,
    /// The section selected on the band (a press on it; Alt+D, Delete, Ctrl+C act on it).
    pub section: Option<SectionId>,
    /// The selected rows (UI-340): the ones that move together when one of them is dragged, and the
    /// tracks a control of one of them changes. The focus (`Studio::selected_row`) is one of them.
    pub tracks: Vec<RowKind>,
    /// Where a Shift press or Shift+arrow measures its range from: the row last chosen alone or
    /// with Ctrl.
    pub anchor: Option<RowKind>,
    /// The last press was on the track heads, so the arrows and Ctrl+A choose rows there.
    pub heads: bool,
    /// The height of every track row (Ctrl + wheel).
    pub track_h: f32,
    /// The tracks that show their automation lanes.
    pub open: HashSet<TrackId>,
    /// The last press was in the arrangement, so its keys (D, Delete, A) apply.
    pub focus: bool,
    /// Bumped whenever something the cached picture shows changes.
    pub generation: u64,
    pub hover: Spot,
    /// Where the pointer last was over the arrangement.
    pub pointer: Option<(f32, f32)>,
    pub mods: Mods,
    /// The time the range tool chose last.
    pub range: Option<(Tick, Tick)>,
    pub ranges: Vec<(Tick, Tick)>,
    /// Automation being written while the song plays (AUT-002).
    pub writing: Option<write::Writing>,
    /// A clip moved takes the automation under it along (AUT-012; the
    /// setting `automation.follows-clips`, on by default).
    pub follows_clips: bool,
    /// While a track shows its lanes, its clips cannot be taken hold of, so
    /// a press there is for the automation (AUT-012; the setting
    /// `automation.locks-clips`, on by default).
    pub locks_clips: bool,
    /// A click on the empty part of a lane moves the playhead there (the setting
    /// `click-empty.locate`, on by default).
    pub click_locates: bool,
    /// The points selected in one automation lane (CTX-028).
    pub points: crate::clipops::PointSel,
    /// How many presses in a row the last one was (2 is a double-click).
    pub clicks: u32,
    /// The path of the pencil in a lane or the tempo row, in window pixels, with the modifiers
    /// held on the way to each point (a wave reads Mod as its phase, AUT-010).
    pub stroke: Vec<(f32, f32, Mods)>,
    /// The transform frame round the range (AUT-010: Alt+T, or 「自由変形」 in its menu).
    pub frame: Option<range_trim::RangeFrame>,
}

impl Default for Arrange {
    fn default() -> Arrange {
        Arrange {
            swept: BTreeSet::new(),
            drag: None,
            clips: BTreeSet::new(),
            section: None,
            tracks: Vec::new(),
            anchor: None,
            heads: false,
            track_h: Layout::TRACK_ROW_H,
            open: HashSet::new(),
            focus: false,
            generation: 0,
            hover: Spot::None,
            pointer: None,
            mods: Mods::NONE,
            range: None,
            ranges: Vec::new(),
            writing: None,
            follows_clips: true,
            locks_clips: true,
            click_locates: true,
            points: crate::clipops::PointSel::default(),
            clicks: 1,
            stroke: Vec::new(),
            frame: None,
        }
    }
}

impl Arrange {
    pub const TRACK_H_MIN: f32 = 56.0;
    /// Tall enough to read a track's waveform or automation closely.
    pub const TRACK_H_MAX: f32 = 320.0;
    /// How much one notch of Ctrl + wheel changes a track's height.
    pub const TRACK_H_STEP: f32 = 8.0;
    /// The top of the ruler that holds the loop band.
    pub const LOOP_H: f32 = crate::ruler::LOOP_H;
    /// How wide the grips at a clip's, the loop's or a section's ends are.
    pub const GRIP: f32 = crate::ruler::GRIP;
    /// How far the pointer goes before a press becomes a drag.
    pub const DRAG_PX: f32 = 3.0;
    pub const TRACK_DRAG_PX: f32 = 5.0;
    /// The room above and below a lane's line.
    pub const LANE_PAD: f32 = 8.0;

    pub(crate) fn touched(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// How many lanes hang under each open track.
    pub fn lane_counts(&self, p: &Project) -> HashMap<TrackId, usize> {
        self.open
            .iter()
            .filter_map(|t| {
                let n = p
                    .tracks
                    .get(t)?
                    .automation
                    .iter()
                    .filter(|l| !l.hidden)
                    .count();
                (n > 0).then_some((*t, n))
            })
            .collect()
    }

    /// The lanes a track shows, top to bottom.
    pub fn lanes_of(&self, t: &Track) -> Vec<AutoLane> {
        if !self.open.contains(&t.id) {
            return Vec::new();
        }
        t.automation.iter().filter(|l| !l.hidden).cloned().collect()
    }
}

/// What is under the pointer, as far as editing the arrangement goes.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Spot {
    #[default]
    None,
    Loop(LoopGrip),
    Section {
        id: SectionId,
        index: usize,
        /// On its right edge: the drag changes its length.
        resize: bool,
    },
    /// The part of the section band that has no section.
    Band,
    Tempo(usize),
    /// The tempo row where there is no point.
    TempoGap,
    Clip {
        track: TrackId,
        clip: ClipId,
        grip: Grip,
    },
    /// A handle of an audio clip: a fade, the gain, a crossfade, the slip, or
    /// the join with the next clip.
    Fx {
        track: TrackId,
        clip: ClipId,
        part: clipfx::FxPart,
    },
    /// A row's lane where there is no clip.
    Space(RowKind),
    /// A row in the track column.
    Row(RowKind),
    /// The button that shows and hides a track's lanes.
    AutoToggle(TrackId),
    Point {
        track: TrackId,
        target: AutoTarget,
        index: usize,
    },
    /// The line from point `index` to the next.
    Line {
        track: TrackId,
        target: AutoTarget,
        index: usize,
    },
    Lane {
        track: TrackId,
        target: AutoTarget,
    },
    LaneHead {
        track: TrackId,
        target: AutoTarget,
        button: LaneButton,
    },
    /// The recording area under a track: its input gain, the Comp row, the takes.
    Rec {
        track: TrackId,
        part: crate::rec_area::RecPart,
    },
    /// The upper half of the range over a lane, or over the tempo row (None): a vertical drag
    /// moves every value of the stretch (AUT-010).
    RangeTrim(range_trim::RangePlace),
    /// A handle of the range's transform frame: the top edge's scales, the corner's tilts.
    FrameGrip {
        tilt: bool,
    },
}

pub use crate::ruler::LoopGrip;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Body,
    Left,
    Right,
    /// The lower right corner: the loop (EDT-041).
    Corner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneButton {
    /// What the lane moves.
    Param,
    /// The track head's current mode; a press chooses the next mode.
    Mode(AutoMode),
    Add,
    Remove,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub spot: Spot,
    pub tool: Tool,
    pub from: (f32, f32),
    pub at: (f32, f32),
    pub started: bool,
    pub mods: Mods,
    /// Alt was down at the press: the drag copies.
    pub copy: bool,
    /// The song position under the press.
    pub tick: Tick,
}

/// What letting go would do.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub command: Option<(&'static str, Value)>,
    /// The clips selected afterwards (a marquee or a range).
    pub select: Option<BTreeSet<(TrackId, ClipId)>>,
    /// The value shown next to the pointer.
    pub tip: String,
    pub preview: Preview,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    /// Clips where they would land.
    Clips(Vec<Ghost>),
    Loop(Tick, Tick),
    /// A section dragged by `dx` px, landing at `slot`.
    Section {
        id: SectionId,
        dx: f32,
        slot: Tick,
    },
    SectionLength {
        id: SectionId,
        length: Tick,
    },
    /// The section the pencil is painting on the band.
    NewSection {
        start: Tick,
        end: Tick,
    },
    Tempo {
        index: usize,
        tick: Tick,
        bpm: f64,
    },
    Drop {
        mark: DropMark,
        moving: Vec<RowKind>,
    },
    Point {
        track: TrackId,
        target: AutoTarget,
        index: usize,
        tick: Tick,
        value: f64,
    },
    Curve {
        track: TrackId,
        target: AutoTarget,
        index: usize,
        curve: f64,
    },
    /// Clips drawn as they would be: a fade, a gain, a point or a crossfade
    /// curve changed (the clips' own picture shows it).
    Fx(Vec<(ClipId, clipfx::FxEdit)>),
    /// Audio clips drawn at `percent`, the speed an Alt drag on an edge gives them.
    Speed {
        percent: f64,
        clips: Vec<ClipId>,
    },
    /// Screen rectangle.
    Marquee(Rect),
    /// What the pencil would draw in a lane or the tempo row (window points).
    Stroke(Vec<(f32, f32)>),
    /// A stretch of time over rows (screen y).
    Range {
        start: Tick,
        end: Tick,
        top: f32,
        bottom: f32,
    },
    /// The values of the range over a lane or the tempo row, moved as the drag on its upper
    /// half or on its frame would move them.
    RangeMove {
        place: range_trim::RangePlace,
        from: Tick,
        to: Tick,
        how: xirius_core::commands::RangeMove,
    },
    /// The loop the corner of `clip` is dragged to (the clip's own picture shows it).
    ClipLoop {
        clip: ClipId,
        looped: xirius_core::clipfx::ClipLoop,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ghost {
    pub track: TrackId,
    pub start: Tick,
    pub length: Tick,
    pub color: usize,
}

/// Where dropped rows go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropMark {
    /// A line between rows: `y` from the top of the column, indented to `depth`.
    Line { y: f32, depth: usize },
    /// Into this folder (its surface lights up).
    Into(FolderId),
}

/// The buttons in a lane's head, relative to the column's left and the lane's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneParts {
    pub param: Rect,
    pub add: Rect,
    pub remove: Rect,
}

impl LaneParts {
    /// The parameter, add and remove on one line, under the track's name.
    pub fn of(row: &Row, _first: bool) -> LaneParts {
        let x = row.content_x() + Layout::ROW_ICON + Space::X1.px();
        let right = row.head_w - Space::X2.px();
        let b = Layout::TOGGLE;
        let gap = Layout::TOGGLE_GAP;
        let top = (LANE_H - b) / 2.0;
        let remove = Rect::new(right - b, top, b, b);
        let add = Rect::new(remove.x - b - gap, top, b, b);
        LaneParts {
            param: Rect::new(x, top, (add.x - gap - x).max(b), b),
            add,
            remove,
        }
    }
}

/// The quietest level a volume lane spreads out in even steps of dB; the
/// little room below it goes down to silence (AUT-017).
pub const LANE_FLOOR_DB: f64 = -60.0;
/// The share of a volume lane's height below `LANE_FLOOR_DB`.
const LANE_FLOOR_SHARE: f64 = 0.05;

/// Where a value sits in a lane, from 0 at the bottom to 1 at the top.
/// Volume and sends go in even steps of dB, not the fader's scale (which
/// belongs to the mixer), so a low lane still tells -40 dB from 0 dB.
pub fn lane_norm(target: &AutoTarget, v: f64) -> f64 {
    match target {
        AutoTarget::Volume | AutoTarget::Send(_) => {
            let v = v.clamp(MIN_DB, MAX_DB);
            if v >= LANE_FLOOR_DB {
                LANE_FLOOR_SHARE
                    + (1.0 - LANE_FLOOR_SHARE) * (v - LANE_FLOOR_DB) / (MAX_DB - LANE_FLOOR_DB)
            } else {
                LANE_FLOOR_SHARE * (v - MIN_DB) / (LANE_FLOOR_DB - MIN_DB)
            }
        }
        _ => target.to_norm(v),
    }
}

/// The value at a place in a lane (the inverse of `lane_norm`).
pub fn lane_value(target: &AutoTarget, n: f64) -> f64 {
    let n = n.clamp(0.0, 1.0);
    match target {
        AutoTarget::Volume | AutoTarget::Send(_) => {
            if n >= LANE_FLOOR_SHARE {
                LANE_FLOOR_DB
                    + (n - LANE_FLOOR_SHARE) / (1.0 - LANE_FLOOR_SHARE) * (MAX_DB - LANE_FLOOR_DB)
            } else {
                MIN_DB + n / LANE_FLOOR_SHARE * (LANE_FLOOR_DB - MIN_DB)
            }
        }
        _ => target.from_norm(n),
    }
}

/// The height in a lane (from its top) of a value.
pub fn lane_value_y(target: &AutoTarget, v: f64) -> f32 {
    let n = lane_norm(target, v) as f32;
    Arrange::LANE_PAD + (1.0 - n) * (LANE_H - Arrange::LANE_PAD * 2.0)
}

/// What a lane moves, in words.
pub fn lane_name(p: &Project, t: &Track, target: &AutoTarget) -> String {
    match target {
        AutoTarget::Volume => "ボリューム".into(),
        AutoTarget::Pan => "パン".into(),
        AutoTarget::Mute => "ミュート".into(),
        AutoTarget::Send(s) => {
            let to = t
                .sends
                .iter()
                .find(|x| x.id == *s)
                .and_then(|x| p.tracks.get(&x.to))
                .map_or("?".to_string(), |b| b.name.clone());
            format!("センド · {to}")
        }
        AutoTarget::Param(pid, name) => {
            let plugin = t
                .inserts
                .iter()
                .find(|x| x.id == *pid)
                .map_or("?", |x| x.name.as_str());
            format!("{plugin} · {name}")
        }
    }
}

/// A value of a lane, in words.
pub fn value_text(target: &AutoTarget, v: f64) -> String {
    match target {
        AutoTarget::Volume | AutoTarget::Send(_) => {
            if v <= MIN_DB {
                "-∞ dB".into()
            } else {
                format!("{v:+.1} dB")
            }
        }
        AutoTarget::Pan => {
            let n = (v * 100.0).round();
            if n == 0.0 {
                "C".into()
            } else if n < 0.0 {
                format!("L{}", -n)
            } else {
                format!("R{n}")
            }
        }
        AutoTarget::Mute => if v >= 0.5 {
            "ミュート"
        } else {
            "ミュート解除"
        }
        .into(),
        AutoTarget::Param(..) => format!("{v:.2}"),
    }
}

/// Everything a track's lanes can move, in the order of the target list.
pub fn targets_of(t: &Track) -> Vec<AutoTarget> {
    let mut out = vec![AutoTarget::Volume, AutoTarget::Pan, AutoTarget::Mute];
    out.extend(t.sends.iter().map(|s| AutoTarget::Send(s.id)));
    for plugin in &t.inserts {
        if let Some(def) = plugin.builtin() {
            out.extend(
                def.params
                    .iter()
                    .map(|p| AutoTarget::Param(plugin.id, p.id.to_string())),
            );
        }
        out.extend(
            plugin
                .params
                .keys()
                .map(|k| AutoTarget::Param(plugin.id, k.clone())),
        );
    }
    out.extend(
        t.automation
            .iter()
            .filter(|l| l.target.exists_on(t))
            .map(|l| l.target.clone()),
    );
    let mut seen = std::collections::HashSet::new();
    out.retain(|target| seen.insert(target.clone()));
    out
}

/// What pressing `pressed` makes of a track in `now`.
pub fn pick_mode(now: AutoMode, pressed: AutoMode) -> AutoMode {
    if now == pressed {
        AutoMode::Off
    } else {
        pressed
    }
}

fn fits(body: &ClipBody, kind: TrackKind) -> bool {
    matches!(
        (body, kind),
        (ClipBody::Midi { .. }, TrackKind::Instrument) | (ClipBody::Audio { .. }, TrackKind::Audio)
    )
}

fn node_of(kind: RowKind) -> Node {
    match kind {
        RowKind::Track(t) => Node::Track(t),
        RowKind::Folder(f) => Node::Folder(f),
    }
}

fn node_text(kind: RowKind) -> String {
    match kind {
        RowKind::Track(t) => t.to_string(),
        RowKind::Folder(f) => f.to_string(),
    }
}

impl Studio {
    // ------------------------------------------------------------ time

    /// The song position under a window x (not clamped).
    pub fn tick_at(&self, g: &Geometry, x: f32) -> Tick {
        self.px_to_ticks(x - g.lanes.x + self.scroll.0)
    }

    pub fn meter(&self) -> Meter {
        self.meters.of(&self.project(), self.beat_w)
    }

    /// `t` on the grid (counted from the start of its bar), unless snapping is
    /// off or Shift is down.
    pub fn snap_tick(&self, t: Tick, shift: bool) -> Tick {
        self.tools().snap.snapped(&self.meter(), shift, t)
    }

    fn bar_ticks(&self) -> Tick {
        let tm = self.project().time_map();
        (tm.bar_start(2) - tm.bar_start(1)).max(1)
    }

    /// Where a tempo point sits in the tempo row.
    pub fn tempo_xy(&self, g: &Geometry, tick: Tick, bpm: f64) -> (f32, f32) {
        let n = ((bpm - 60.0) / 140.0).clamp(0.0, 1.0) as f32;
        let pad = 8.0;
        (
            self.tick_x(g, tick),
            g.tempo.y + pad + (1.0 - n) * (g.tempo.h - pad * 2.0).max(0.0),
        )
    }

    /// The top of a track's lane `i` on screen.
    pub fn lane_top(&self, g: &Geometry, row: &Row, i: usize) -> f32 {
        self.row_y(g, row) + row.h + i as f32 * LANE_H
    }

    // ------------------------------------------------------------ what is where

    pub fn arrange_spot(&self, g: &Geometry, x: f32, y: f32) -> Spot {
        if self.popup.is_some() {
            return Spot::None;
        }
        let p = self.project();
        if g.ruler.contains(x, y) {
            if !crate::ruler::on_band(g.ruler, y) {
                return Spot::None;
            }
            let scale = self.ruler_scale(g);
            let span = p.loop_range.map(|l| (scale.x(l.start), scale.x(l.end)));
            return Spot::Loop(crate::ruler::grip_at(span, x));
        }
        if g.section_band.contains(x, y) {
            for (index, s) in p.sections.iter().enumerate() {
                let (a, b) = (self.tick_x(g, s.start), self.tick_x(g, s.start + s.length));
                if x >= a && x < b {
                    return Spot::Section {
                        id: s.id,
                        index,
                        resize: x >= b - Arrange::GRIP,
                    };
                }
            }
            return Spot::Band;
        }
        if g.tempo.contains(x, y) {
            for (i, t) in p.tempo.iter().enumerate() {
                let (px, py) = self.tempo_xy(g, t.tick, t.bpm);
                if (x - px).abs() <= 7.0 && (y - py).abs() <= 7.0 {
                    return Spot::Tempo(i);
                }
            }
            return Spot::TempoGap;
        }
        let in_column = g.column.contains(x, y);
        if !in_column && !g.lanes.contains(x, y) {
            return Spot::None;
        }
        let Some(row) = self.tree().row_at(y - g.column.y + self.scroll.1) else {
            return Spot::None;
        };
        let top = self.row_y(g, row);
        if y - top >= row.h {
            return self.lane_spot(g, row, x, y, in_column);
        }
        if in_column {
            if let RowKind::Track(t) = row.kind
                && let Some(auto) = RowParts::of(row).auto
                && crate::automation_track_mode::rect(auto).contains(x - g.column.x, y - top)
            {
                return crate::automation_track_mode::spot(t, p.tracks[&t].auto_mode);
            }
            if let RowKind::Track(t) = row.kind
                && RowParts::of(row)
                    .auto
                    .is_some_and(|r| r.contains(x - g.column.x, y - top))
            {
                return Spot::AutoToggle(t);
            }
            return match self.hit(g, x, y) {
                Hit::Row(k) => Spot::Row(k),
                // The mark is part of the head: it presses, drags and opens the row's menu as
                // the rest of it does (UI-312).
                Hit::TrackMark(t) => Spot::Row(RowKind::Track(t)),
                _ => Spot::None,
            };
        }
        // A clip's count of takes is a button: it opens the take rows and folds them (UI-296).
        if let RowKind::Track(t) = row.kind
            && let Some(track) = p.tracks.get(&t)
        {
            let stacks = xirius_core::takes::stacks(track);
            let on_badge = track.clips.iter().any(|c| {
                crate::draw::takes_under(&stacks, c).is_some()
                    && crate::draw::takes_badge(self.clip_rect(g, row, c))
                        .is_some_and(|b| b.contains(x, y))
            });
            if on_badge {
                return Spot::Rec {
                    track: t,
                    part: crate::rec_area::RecPart::Fold,
                };
            }
        }
        if let RowKind::Track(t) = row.kind
            && let Some(track) = p.tracks.get(&t)
            && !self.clips_locked(t)
        {
            // A take that does not play is not drawn over the one that does (EDT-139).
            let stacks = xirius_core::takes::stacks(track);
            let shown =
                |c: &&xirius_core::model::Clip| !(c.muted && crate::draw::in_stack(&stacks, c));
            // The lower right corner is the loop's, over the clip's other handles (EDT-041).
            for clip in track.clips.iter().rev().filter(shown) {
                let r = self.clip_rect(g, row, clip);
                let e = Arrange::GRIP.min(r.w / 3.0);
                let h = clip_loop::CORNER.min(r.h / 3.0);
                if r.contains(x, y) && x >= r.right() - e && y >= r.bottom() - h {
                    return Spot::Clip {
                        track: t,
                        clip: clip.id,
                        grip: Grip::Corner,
                    };
                }
            }
            if let Some(spot) = self.fx_spot(g, row, track, x, y) {
                return spot;
            }
            for clip in track.clips.iter().filter(shown) {
                let r = self.clip_rect(g, row, clip);
                if r.contains(x, y) {
                    let e = Arrange::GRIP.min(r.w / 3.0);
                    let grip = if x < r.x + e {
                        Grip::Left
                    } else if x >= r.right() - e {
                        Grip::Right
                    } else {
                        Grip::Body
                    };
                    return Spot::Clip {
                        track: t,
                        clip: clip.id,
                        grip,
                    };
                }
            }
        }
        Spot::Space(row.kind)
    }

    /// The point under the pointer, where it is and its time and value in
    /// words, for the balloon while nothing is dragged (AUT-017).
    pub fn point_tip(&self, g: &Geometry) -> Option<((f32, f32), String)> {
        if self.arrange.drag.as_ref().is_some_and(|d| d.started) {
            return None;
        }
        let Spot::Point {
            track,
            target,
            index,
        } = &self.arrange.hover
        else {
            return None;
        };
        let row = *self.tree().row_of(RowKind::Track(*track))?;
        let p = self.project();
        let lanes = self.arrange.lanes_of(p.tracks.get(track)?);
        let i = lanes.iter().position(|l| l.target == *target)?;
        let pt = lanes[i].points.get(*index)?;
        let at = (
            self.tick_x(g, pt.tick),
            self.lane_top(g, &row, i) + lane_value_y(target, pt.value),
        );
        let text = format!(
            "{}　{}",
            p.time_map().format(pt.tick),
            value_text(target, pt.value)
        );
        Some((at, text))
    }

    /// The track shows its automation and its clips are not to be taken
    /// hold of meanwhile.
    pub fn clips_locked(&self, t: TrackId) -> bool {
        self.arrange.locks_clips && self.arrange.open.contains(&t)
    }

    fn lane_spot(&self, g: &Geometry, row: &Row, x: f32, y: f32, in_column: bool) -> Spot {
        let RowKind::Track(t) = row.kind else {
            return Spot::None;
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return Spot::None;
        };
        let i = ((y - self.row_y(g, row) - row.h) / LANE_H) as usize;
        let lanes = self.arrange.lanes_of(track);
        let Some(lane) = lanes.get(i) else {
            return if row.rec_h > 0.0 && y >= self.row_y(g, row) + row.rec_y() - row.y {
                self.rec_spot(g, row, x, y, in_column)
            } else {
                Spot::None
            };
        };
        let top = self.lane_top(g, row, i);
        let target = lane.target.clone();
        if in_column {
            let parts = LaneParts::of(row, i == 0);
            let (lx, ly) = (x - g.column.x, y - top);
            for (r, button) in [
                (parts.param, LaneButton::Param),
                (parts.add, LaneButton::Add),
                (parts.remove, LaneButton::Remove),
            ] {
                if r.contains(lx, ly) {
                    return Spot::LaneHead {
                        track: t,
                        target,
                        button,
                    };
                }
            }
            return Spot::None;
        }
        for (index, pt) in lane.points.iter().enumerate() {
            let (px, py) = (
                self.tick_x(g, pt.tick),
                top + lane_value_y(&target, pt.value),
            );
            if (x - px).abs() <= 6.0 && (y - py).abs() <= 6.0 {
                return Spot::Point {
                    track: t,
                    target,
                    index,
                };
            }
        }
        let tick = self.tick_at(g, x);
        let pts = &*lane.points;
        let seg = pts
            .windows(2)
            .position(|w| w[0].tick <= tick && tick < w[1].tick);
        // a stair has no line to bend
        if let Some(index) = seg
            && !target.stepped()
            && !pts[index].step
            && let Some(v) = lane.curve_at(tick as f64)
            && (y - top - lane_value_y(&target, v)).abs() <= 5.0
        {
            return Spot::Line {
                track: t,
                target,
                index,
            };
        }
        Spot::Lane { track: t, target }
    }

    // ------------------------------------------------------------ pointer

    /// The pointer went down somewhere: is it in the arrangement (so its keys apply)?
    pub fn arrange_focus(&mut self, g: &Geometry, x: f32, y: f32) {
        let in_arrange = [g.lanes, g.column, g.ruler, g.section_band, g.tempo]
            .iter()
            .any(|r| r.contains(x, y));
        if in_arrange {
            self.arrange.focus = true;
            self.arrange.heads = g.column.contains(x, y);
            self.choose_editor();
        }
    }

    /// A press. True when it was used here; otherwise it goes on to
    /// [`Studio::press`] (which selects the row or moves the playhead).
    pub fn arrange_down(&mut self, g: &Geometry, x: f32, y: f32, mods: Mods) -> bool {
        // The ruler's click and the mixer (which come through [`Studio::press`]) read the
        // modifiers of this press, also the one that closes a menu.
        self.arrange.mods = mods;
        if self.popup.is_some() {
            return false;
        }
        self.arrange_focus(g, x, y);
        self.arrange.drag = None;
        self.arrange.stroke.clear();
        self.comp_sel = None;
        self.arrange.swept.clear();
        let mut spot = self.arrange_spot(g, x, y);
        match spot {
            Spot::Section { id, .. } => self.select_section(id),
            // A press on the empty band keeps it when it adds (Shift), and lets go otherwise.
            Spot::Band if mods.shift => {}
            _ => self.clear_section(),
        }
        self.input_map.context_delete = None;
        self.input_map.context_clipboard = None;
        self.input_map.tempo_selected = match spot {
            Spot::Tempo(i) => self.project().tempo.get(i).map(|t| t.id),
            _ => None,
        };
        self.input_map.marker_selected = None;
        let mut tool = self.holds().tool(Area::Arrange, self.tool, mods);
        self.input_map.mouse_drag_disabled = false;
        self.input_map.mouse_click_action = None;
        self.apply_edit_prefs();
        let mut drag_action = None;
        if let Some((click, drag)) = self.mouse_actions(&spot, tool, mods) {
            self.input_map.mouse_drag_disabled = drag.as_deref() == Some("");
            if click.as_deref() == Some("") && self.input_map.mouse_drag_disabled {
                return true;
            }
            if click.as_deref() == Some("clip.mute") {
                tool = Tool::Mute;
            }
            self.input_map.mouse_click_action = click;
            drag_action = drag;
        }
        let click_action = self.input_map.mouse_click_action.clone();
        let raw_mods = mods;
        let mods = self.mouse_modifiers(&spot, tool, mods, false);
        if self.tool == Tool::Cut
            && mods.alt
            && !mods.ctrl
            && let Spot::Clip { clip, .. } | Spot::Fx { clip, .. } = spot
            && self.project().clip(clip).is_ok_and(|(_, c)| c.is_midi())
        {
            tool = Tool::Cut;
        }
        let tick = self.tick_at(g, x).max(0);
        if tool == Tool::Select && !mods.ctrl && !mods.alt && self.link_at(g, x, y, &spot) {
            tool = Tool::Range;
        }
        // Ctrl + Alt is the slip, not the range tool Ctrl alone is.
        if let Some(slip) = self.slip_spot(&spot, mods) {
            spot = slip;
            tool = Tool::Select;
        }
        // Only the selecting tool takes hold of the handles.
        if tool != Tool::Select
            && let Spot::Fx { track, clip, .. } = spot
        {
            spot = Spot::Clip {
                track,
                clip,
                grip: Grip::Body,
            };
        }
        match self.range_spot(g, (x, y), tool, &spot) {
            Some(taken) => spot = taken,
            None if self.arrange.frame.take().is_some() => self.arrange.touched(),
            None => {}
        }
        if tool == Tool::Range
            && let Spot::Clip { track, .. } | Spot::Lane { track, .. } = spot
        {
            spot = Spot::Space(RowKind::Track(track));
        }
        let mut drag = Drag {
            spot: spot.clone(),
            tool,
            from: (x, y),
            at: (x, y),
            started: false,
            mods,
            copy: mods.alt,
            tick,
        };
        if matches!(spot, Spot::Tempo(_) | Spot::TempoGap) {
            return self.tempo_down(g, drag, y);
        }
        // A point is selected the way the map's click says (a delete is the eraser's press).
        let on_point = matches!(spot, Spot::Point { .. });
        let deletes = on_point
            && (self.arrange.clicks >= 2
                || match click_action.as_deref() {
                    Some(a) => a == "auto.point.delete",
                    None => tool == Tool::Eraser || raw_mods.alt,
                });
        let mut press_mods = mods;
        if on_point && let Some(a) = click_action.as_deref() {
            press_mods.shift = a == "select.toggle";
            press_mods.alt = deletes;
        }
        if on_point && click_action.as_deref() == Some("select.none") {
            self.arrange.points.clear();
            self.arrange.touched();
        } else {
            self.point_press(&spot, if deletes { Tool::Eraser } else { tool }, press_mods);
        }
        let used = match spot {
            Spot::None => return false,
            // A click on the ruler still moves the playhead; one on the band does not.
            Spot::Loop(LoopGrip::New) => false,
            Spot::Section { .. } | Spot::Band => true,
            Spot::Loop(_) | Spot::Tempo(_) | Spot::TempoGap | Spot::Line { .. } => true,
            Spot::Space(_) => matches!(tool, Tool::Range | Tool::Draw),
            // The row is chosen by [`Studio::press`] (`press_row`), with the modifiers of this press.
            Spot::Row(_) => {
                self.arrange.touched();
                false
            }
            Spot::Clip { track, clip, .. } => {
                if !self.clip_down(&drag, track, clip) {
                    return true;
                }
                true
            }
            Spot::Fx { track, clip, part } => {
                if !self.fx_down(&drag, track, clip, part) {
                    return true;
                }
                true
            }
            Spot::AutoToggle(t) => {
                self.toggle_lanes(g, t);
                return true;
            }
            Spot::LaneHead {
                track,
                target,
                button,
            } => {
                self.lane_button(g, track, &target, button);
                return true;
            }
            // A take's row: a press picks the stretch between dividers, a drag a stretch of its own.
            Spot::Rec {
                part: crate::rec_area::RecPart::Bar { .. },
                ..
            } => true,
            Spot::Rec { track, part } => return self.rec_down(track, part),
            Spot::RangeTrim(_) | Spot::FrameGrip { .. } => true,
            Spot::Point {
                track,
                ref target,
                index,
            } => {
                if deletes {
                    self.arrange_run(
                        "auto.point.delete",
                        json!({ "track": track.to_string(), "target": target.to_string(),
                                "index": index }),
                    );
                    return true;
                }
                true
            }
            Spot::Lane { track, ref target } => {
                if !matches!(tool, Tool::Select | Tool::Draw) {
                    return true;
                }
                // The arrow draws a rectangle over the empty part of a lane and adds a point
                // with a double-click; the pencil adds on a press. The map can bind a click or
                // a drag to the add instead.
                let mapped = click_action.as_deref() == Some("auto.point.add")
                    || drag_action.as_deref() == Some("auto.point.add");
                let plain =
                    |a: &Option<String>| a.as_deref().is_none_or(|a| a.starts_with("select."));
                if tool == Tool::Select && !mapped {
                    if self.arrange.clicks < 2 {
                        self.arrange.drag = Some(drag);
                        return true;
                    }
                } else if !mapped
                    && !(tool == Tool::Draw && plain(&click_action) && plain(&drag_action))
                {
                    return true;
                }
                if tool == Tool::Draw && !mapped {
                    // A drag draws the shape; a click lays one point (`stroke_click`).
                    self.arrange.stroke = vec![(x, y, mods)];
                    self.arrange.drag = Some(drag);
                    return true;
                }
                let Some(row) = self.tree().row_of(RowKind::Track(track)).copied() else {
                    return true;
                };
                let p = self.project();
                let i = p
                    .tracks
                    .get(&track)
                    .map(|t| self.arrange.lanes_of(t))
                    .and_then(|l| l.iter().position(|l| l.target == *target))
                    .unwrap_or(0);
                let n = 1.0
                    - ((y - self.lane_top(g, &row, i) - Arrange::LANE_PAD)
                        / (LANE_H - Arrange::LANE_PAD * 2.0))
                        .clamp(0.0, 1.0);
                let value = lane_value(target, f64::from(n));
                let at = self.snap_tick(tick, mods.shift);
                let added = self.arrange_run(
                    "auto.point.add",
                    json!({ "track": track.to_string(), "target": target.to_string(),
                            "at": at, "value": value }),
                );
                // The new point follows the pointer until the button comes up.
                if let Some(index) = added.and_then(|d| d["index"].as_u64()) {
                    self.point_drawn(track, target, at);
                    drag.spot = Spot::Point {
                        track,
                        target: target.clone(),
                        index: index as usize,
                    };
                    drag.from.0 = self.tick_x(g, at);
                    drag.from.1 = self.lane_top(g, &row, i) + lane_value_y(target, value);
                } else {
                    return true;
                }
                true
            }
        };
        self.arrange.drag = Some(drag);
        used
    }

    /// The tempo that a height in the tempo row stands for (the inverse of [`Arrange::tempo_xy`]).
    fn tempo_at_y(&self, g: &Geometry, y: f32) -> f64 {
        let pad = 8.0;
        let n = 1.0 - ((y - g.tempo.y - pad) / (g.tempo.h - pad * 2.0).max(1.0)).clamp(0.0, 1.0);
        (60.0 + f64::from(n) * 140.0).round()
    }

    /// A press in the tempo row. The arrow adds a point with a double-click and deletes one the
    /// same way; the pencil adds on a press and the point follows the pointer. The tools that
    /// act on parts (split, mute, audition) have nothing to act on here.
    fn tempo_down(&mut self, g: &Geometry, drag: Drag, y: f32) -> bool {
        let p = self.project();
        let first = matches!(drag.spot, Spot::Tempo(0));
        match (drag.tool, &drag.spot) {
            (Tool::Cut | Tool::Mute | Tool::Audition, _) | (Tool::Eraser, Spot::TempoGap) => {
                return true;
            }
            (_, Spot::Tempo(i)) => {
                let id = p.tempo.get(*i).map(|t| t.id);
                let erase = drag.tool == Tool::Eraser || self.arrange.clicks >= 2;
                if let Some(id) = id
                    && erase
                    && !first
                {
                    self.arrange_run("tempo.delete", json!({ "id": id.to_string() }));
                    return true;
                }
            }
            (Tool::Select | Tool::Draw, Spot::TempoGap) => {
                if drag.tool == Tool::Draw {
                    // A drag draws the shape; a click lays one point (`stroke_click`).
                    self.arrange.stroke = vec![(drag.from.0, drag.from.1, drag.mods)];
                } else if self.arrange.clicks >= 2 {
                    let at = self.snap_tick(drag.tick, drag.mods.shift).max(0);
                    let bpm = self.tempo_at_y(g, y);
                    self.arrange_run("tempo.set", json!({ "at": at, "bpm": bpm }));
                    return true;
                }
            }
            _ => {}
        }
        self.arrange.drag = Some(drag);
        true
    }

    /// A press on a clip with the tool it acts as. False when the press is
    /// finished (cut, erased, muted, or taken out of the selection).
    fn clip_down(&mut self, d: &Drag, t: TrackId, c: ClipId) -> bool {
        self.error = None;
        let ids = json!([c.to_string()]);
        match d.tool {
            Tool::Cut => {
                let at = self.snap_tick(d.tick, d.mods.shift);
                self.arrange_run("clip.split", json!({ "ids": ids, "at": at }));
                return false;
            }
            Tool::Eraser => {
                self.arrange.swept.insert((t, c));
                return true;
            }
            Tool::Mute => {
                self.arrange.swept.insert((t, c));
                return true;
            }
            Tool::Select | Tool::Range | Tool::Draw | Tool::Audition => {}
        }
        let key = (t, c);
        self.select_row(Some(RowKind::Track(t)));
        self.arrange.touched();
        let toggle = match self.input_map.mouse_click_action.as_deref() {
            Some("select.toggle") => true,
            Some("select.set") => false,
            Some("select.none") => {
                self.arrange.clips.clear();
                self.selected_clip = None;
                return false;
            }
            _ => d.mods.shift && !d.copy,
        };
        if toggle {
            if self.arrange.clips.remove(&key) {
                if self.selected_clip == Some(key) {
                    self.selected_clip = self.arrange.clips.iter().next().copied();
                }
                return false;
            }
            self.arrange.clips.insert(key);
        } else if !self.arrange.clips.contains(&key) {
            self.arrange.clips = BTreeSet::from([key]);
        }
        self.selected_clip = Some(key);
        true
    }

    /// The pointer moved. True when the picture changes.
    pub fn arrange_move(&mut self, g: &Geometry, x: f32, y: f32, mods: Mods) -> bool {
        let mut changed = self.arrange.mods != mods;
        let cut = self.cut_preview(g);
        self.arrange.mods = mods;
        self.arrange.pointer = Some((x, y));
        if self.input_map.mouse_drag_disabled && self.arrange.drag.is_some() {
            return false;
        }
        let mods = self
            .arrange
            .drag
            .as_ref()
            .map(|d| self.mouse_modifiers(&d.spot, d.tool, mods, true))
            .unwrap_or(mods);
        self.sweep_tools(g, x, y);
        if let Some(d) = &mut self.arrange.drag {
            d.at = (x, y);
            d.mods = mods;
            let limit = if matches!(d.spot, Spot::Row(_)) {
                Arrange::TRACK_DRAG_PX
            } else {
                Arrange::DRAG_PX
            };
            if !d.started && ((x - d.from.0).abs() > limit || (y - d.from.1).abs() > limit) {
                d.started = true;
            }
            // Points, curves, tempo and the loop are previewed in the picture itself.
            let started = d.started;
            if d.tool == Tool::Draw
                && matches!(d.spot, Spot::Lane { .. } | Spot::TempoGap)
                && self.arrange.stroke.last().map(|p| (p.0, p.1)) != Some((x, y))
            {
                self.arrange.stroke.push((x, y, mods));
            }
            if started {
                self.arrange.touched();
            }
            return started;
        }
        let spot = self.arrange_spot(g, x, y);
        let tool = self.holds().tool(Area::Arrange, self.tool, mods);
        let spot = self.range_spot(g, (x, y), tool, &spot).unwrap_or(spot);
        if spot != self.arrange.hover {
            self.arrange.hover = spot;
            self.arrange.touched();
            changed = true;
        }
        changed || cut != self.cut_preview(g)
    }

    /// A key went up or down in the middle of a drag (Shift turns snapping
    /// off, Alt makes a move a copy).
    pub fn arrange_mods(&mut self, mods: Mods) -> bool {
        let changed = self.arrange.mods != mods;
        self.arrange.mods = mods;
        let mods = self
            .arrange
            .drag
            .as_ref()
            .map(|d| self.mouse_modifiers(&d.spot, d.tool, mods, true))
            .unwrap_or(mods);
        let started = match &mut self.arrange.drag {
            Some(d) => {
                d.mods = mods;
                d.started
            }
            None => false,
        };
        if started {
            self.arrange.touched();
        }
        started || changed
    }

    /// The button came up: run what the drag planned.
    pub fn arrange_up(&mut self, g: &Geometry, x: f32, y: f32, mods: Mods) -> bool {
        let mods = self
            .arrange
            .drag
            .as_ref()
            .map(|d| self.mouse_modifiers(&d.spot, d.tool, mods, true))
            .unwrap_or(mods);
        let Some(mut d) = self.arrange.drag.take() else {
            return false;
        };
        self.arrange.touched();
        if self.input_map.mouse_drag_disabled
            && ((x - d.from.0).abs() > Arrange::DRAG_PX || (y - d.from.1).abs() > Arrange::DRAG_PX)
        {
            return true;
        }
        d.at = (x, y);
        d.mods = mods;
        if self.finish_tools(g, &d) {
            return true;
        }
        if !d.started {
            return self.arrange_click(g, &d);
        }
        let plan = self.plan(g, &d);
        self.arrange.stroke.clear();
        let Some(plan) = plan else {
            return true;
        };
        self.point_followed(&d.spot, &plan.preview);
        if let Spot::Lane { track, target } = &d.spot
            && d.tool == Tool::Select
            && let Some((_, ticks)) = self.lane_marquee(g, &d, *track, target)
        {
            let mut all = ticks;
            if d.mods.shift && self.arrange.points.lane() == Some(&(*track, target.clone())) {
                all.extend(self.arrange.points.ticks().iter().copied());
            }
            self.arrange.points.set_all(*track, target, all);
            self.arrange.clips.clear();
            self.arrange.touched();
            return true;
        }
        self.comp_followed(&d.spot, &plan.preview);
        if let Some(sel) = plan.select {
            self.arrange.clips = sel;
            self.arrange.touched();
        }
        if let Spot::Space(_) | Spot::TempoGap = d.spot
            && d.tool == Tool::Range
            && let Preview::Range { start, end, .. } = plan.preview
        {
            self.arrange.range = Some((start, end));
            if !d.mods.shift {
                self.arrange.ranges.clear();
            }
            self.arrange.ranges.push((start, end));
        }
        if let Some((command, args)) = plan.command {
            let data = self.arrange_run(command, args);
            if let Some(data) = data
                && matches!(command, "clip.duplicate" | "clip.split")
            {
                self.select_made(&data);
            }
        }
        true
    }

    /// A press and release without moving.
    fn arrange_click(&mut self, g: &Geometry, d: &Drag) -> bool {
        // `select.after`: the clips from the pointer on (the click on a clip or on the empty part).
        // Only for the tool that is chosen: Alt that turns Cut or Eraser into Select stays a plain select.
        if d.tool == Tool::Select
            && self.tool == Tool::Select
            && self.input_map.mouse_click_action.as_deref() == Some("select.after")
            && matches!(d.spot, Spot::Clip { .. } | Spot::Space(_))
        {
            let under = match d.spot {
                Spot::Clip { track, clip, .. } => Some((track, clip)),
                _ => None,
            };
            self.select_clips_after(d.tick, under);
            return true;
        }
        if self.smart_click(g, d) {
            return true;
        }
        if self.stroke_click(g, d) {
            return true;
        }
        match &d.spot {
            Spot::Space(_) => {
                if d.tool == Tool::Range
                    && self
                        .arrange
                        .ranges
                        .iter()
                        .chain(self.arrange.range.iter())
                        .any(|(a, b)| *a <= d.tick && d.tick <= *b)
                {
                    self.seek(d.tick);
                    return true;
                }
                if !d.mods.shift {
                    self.arrange.clips.clear();
                }
                self.arrange.range = None;
                self.arrange.ranges.clear();
                if self.arrange.click_locates {
                    self.seek(d.tick);
                }
                self.arrange.touched();
                true
            }
            Spot::Clip { track, clip, .. } if !d.mods.shift && d.tool == Tool::Select => {
                self.arrange.clips = BTreeSet::from([(*track, *clip)]);
                self.arrange.touched();
                true
            }
            Spot::Fx { track, clip, part } => self.fx_click(d, *track, *clip, *part),
            // A click in the range moves the playhead, as the range tool's does.
            Spot::RangeTrim(_) => {
                self.seek(d.tick);
                true
            }
            Spot::FrameGrip { .. } => true,
            // The press selected the section (or let go of it); the playhead stays.
            Spot::Section { .. } | Spot::Band => true,
            Spot::Rec { track, part } => self.rec_down(*track, part.clone()),
            // A plain click on one of several selected rows (no drag) chooses it alone.
            Spot::Row(kind) if !d.mods.shift && !d.mods.ctrl => {
                self.pick_row(*kind, Mods::NONE);
                true
            }
            Spot::Line {
                track,
                target,
                index: _,
            } => {
                let p = self.project();
                let at = self.snap_tick(d.tick, d.mods.shift);
                let value = p
                    .tracks
                    .get(track)
                    .and_then(|t| t.automation.iter().find(|l| l.target == *target))
                    .and_then(|l| l.curve_at(at as f64));
                if let Some(value) = value {
                    self.arrange_run(
                        "auto.point.add",
                        json!({ "track": track.to_string(), "target": target.to_string(),
                                "at": at, "value": value }),
                    );
                }
                let _ = g;
                true
            }
            _ => false,
        }
    }

    /// Selects the clips a command made (`{"ids": [...]}`).
    pub(crate) fn select_made(&mut self, data: &Value) {
        let made: HashSet<ClipId> = data["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str()?.parse().ok())
            .collect();
        if made.is_empty() {
            return;
        }
        let p = self.project();
        self.arrange.clips = p
            .tracks
            .values()
            .flat_map(|t| t.clips.iter().map(move |c| (t.id, c.id)))
            .filter(|(_, c)| made.contains(c))
            .collect();
        self.selected_clip = self.arrange.clips.iter().next().copied();
        self.arrange.touched();
    }

    /// Runs one core command (one history step).
    pub(crate) fn arrange_run(&mut self, command: &str, args: Value) -> Option<Value> {
        if self.sound_preparing() {
            return None;
        }
        if self.transport.is_windowed() && crate::io_command::handles(command) {
            if self.input_blocked() {
                return None;
            }
            self.io_command_on_worker(command, args.as_object().cloned().unwrap_or_default());
            return Some(json!({}));
        }
        if self.transport.is_windowed()
            && let Some(raw) = args.as_object()
            && self.song.session().save_request(command, raw).is_some()
        {
            if self.input_blocked() {
                return None;
            }
            self.save_command_on_worker(command, raw.clone());
            return Some(json!({}));
        }
        // A wheel that turned a field has ended its step: anything else is a step of its own
        // (UI-313).
        if !self.wheel_running {
            self.close_wheel_group();
        }
        if command.starts_with("history.") {
            self.drag_drop();
        }
        let said = args.clone();
        if crate::audio_job::NAMES.contains(&command) {
            return self.job_start(command, args).then(|| json!({}));
        }
        match self.song.run(command, args) {
            Ok(data) => {
                self.error = None;
                self.after_edit();
                self.arrange.touched();
                self.say_command(command, &said, &data);
                self.after_command(command, &data);
                Some(data)
            }
            Err(e) => {
                self.fail(e);
                None
            }
        }
    }

    // ------------------------------------------------------------ lanes

    /// Shows or hides a track's lanes. A track with none gets a volume lane.
    pub fn toggle_lanes(&mut self, g: &Geometry, t: TrackId) {
        if !self.arrange.open.remove(&t) {
            self.arrange.open.insert(t);
            let p = self.project();
            let none = p
                .tracks
                .get(&t)
                .is_some_and(|tr| tr.automation.iter().all(|l| l.hidden));
            if none {
                self.arrange_run(
                    "auto.lane.add",
                    json!({ "track": t.to_string(), "target": "volume" }),
                );
            }
        }
        self.arrange.touched();
        self.rebuild(g);
    }

    pub(crate) fn lane_button(
        &mut self,
        g: &Geometry,
        t: TrackId,
        target: &AutoTarget,
        button: LaneButton,
    ) {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return;
        };
        let tid = t.to_string();
        match button {
            LaneButton::Param => self.open_auto_targets(g, t, target, true),
            LaneButton::Mode(m) => {
                let mode = if m == track.auto_mode {
                    crate::automation_track_mode::next(m)
                } else {
                    pick_mode(track.auto_mode, m)
                };
                self.arrange_run("auto.mode", json!({ "ids": [tid], "mode": mode.name() }));
            }
            LaneButton::Add => self.open_auto_targets(g, t, target, false),
            LaneButton::Remove => {
                self.arrange_run(
                    "auto.lane.hide",
                    json!({ "track": tid, "target": target.to_string() }),
                );
                let left = self
                    .project()
                    .tracks
                    .get(&t)
                    .is_some_and(|tr| tr.automation.iter().any(|l| !l.hidden));
                if !left {
                    self.arrange.open.remove(&t);
                }
            }
        }
        self.arrange.touched();
        self.rebuild(g);
    }

    // ------------------------------------------------------------ keys and wheel

    /// D duplicates the selected clips, Delete removes them, A shows the
    /// selected track's lanes. Only while the arrangement has the focus.
    pub fn arrange_key(&mut self, g: &Geometry, key: &str, mods: Mods) -> bool {
        if key == "f2" && !mods.ctrl && !mods.shift {
            return self.open_selected_clip(g, mods.alt);
        }
        if !self.arrange.focus || mods.ctrl || mods.alt {
            return false;
        }
        if self.fx_key(key, mods) {
            return true;
        }
        let p = self.project();
        let ids: Vec<String> = self
            .arrange
            .clips
            .iter()
            .filter(|(t, c)| {
                p.tracks
                    .get(t)
                    .is_some_and(|t| t.clips.iter().any(|k| k.id == *c))
            })
            .map(|(_, c)| c.to_string())
            .collect();
        match key {
            "d" if !ids.is_empty() => {
                if let Some(data) = self.arrange_run("clip.duplicate", json!({ "ids": ids })) {
                    self.select_made(&data);
                }
                true
            }
            "delete" | "backspace" if !ids.is_empty() => {
                self.arrange_run("clip.delete", json!({ "ids": ids }));
                self.arrange.clips.clear();
                if self.selected_clip.is_some_and(|(t, c)| {
                    !self.project().tracks[&t].clips.iter().any(|k| k.id == c)
                }) {
                    self.selected_clip = None;
                }
                true
            }
            "a" => {
                let Some(RowKind::Track(t)) = self.selected_row else {
                    return false;
                };
                self.toggle_lanes(g, t);
                true
            }
            "escape" if self.arrange.drag.take().is_some() => {
                self.arrange.touched();
                true
            }
            "escape" if self.arrange.frame.take().is_some() => {
                self.arrange.touched();
                true
            }
            _ => false,
        }
    }

    /// Ctrl + wheel over the rows makes every track taller or shorter;
    /// Ctrl + Shift + wheel zooms the time axis. Both keep what is under the
    /// pointer where it is. `steps` is notches, up is positive.
    pub fn arrange_wheel(&mut self, g: &Geometry, x: f32, y: f32, steps: f32, mods: Mods) -> bool {
        if mods.alt && !mods.ctrl && self.automation_point_wheel(g, x, y, steps) {
            return true;
        }
        if !mods.ctrl || !(g.lanes.contains(x, y) || g.column.contains(x, y)) {
            return false;
        }
        if mods.shift {
            self.zoom(g, x, steps);
            return true;
        }
        let ty = y - g.column.y + self.scroll.1;
        let anchor = self
            .tree()
            .row_at(ty)
            .map(|r| (r.kind, (ty - r.y) / r.full_h().max(1.0)));
        let h = (self.arrange.track_h + steps * Arrange::TRACK_H_STEP)
            .round()
            .clamp(Arrange::TRACK_H_MIN, Arrange::TRACK_H_MAX);
        if h == self.arrange.track_h {
            return false;
        }
        self.arrange.track_h = h;
        self.arrange.touched();
        self.rebuild(g);
        if let Some((kind, f)) = anchor
            && let Some(row) = self.tree().row_of(kind)
        {
            self.scroll.1 = row.y + f * row.full_h() - (y - g.column.y);
            self.clamp_scroll(g);
        }
        true
    }

    // ------------------------------------------------------------ plans

    /// What letting go of `d` now would do. None when it would do nothing.
    pub fn plan(&self, g: &Geometry, d: &Drag) -> Option<Plan> {
        if d.tool == Tool::Draw && matches!(d.spot, Spot::Space(_)) {
            return self.draw_part_plan(g, d);
        }
        if d.tool == Tool::Draw && matches!(d.spot, Spot::Band) {
            return self.draw_section_plan(g, d);
        }
        if !d.started {
            return None;
        }
        match &d.spot {
            Spot::Clip { track, clip, grip } => match d.tool {
                Tool::Select | Tool::Draw | Tool::Audition => {
                    self.plan_clip(g, d, *track, *clip, *grip)
                }
                _ => None,
            },
            Spot::Fx { track, clip, part } if d.tool == Tool::Select => {
                self.plan_fx(g, d, *track, *clip, *part)
            }
            Spot::Space(_) => self.plan_marquee(g, d),
            Spot::RangeTrim(place) => self.plan_range_trim(d, place),
            Spot::FrameGrip { tilt } => self.plan_frame(g, d, *tilt),
            Spot::Rec {
                track,
                part: crate::rec_area::RecPart::Bar { take, .. },
            } => self.plan_comp(g, d, *track, *take),
            Spot::Loop(grip) => self.plan_loop(g, d, *grip),
            Spot::Section { id, index, resize } => self.plan_section(d, *id, *index, *resize),
            Spot::Tempo(i) => self.plan_tempo(d, *i),
            Spot::Lane { .. } | Spot::TempoGap if d.tool == Tool::Draw => self.plan_stroke(g, d),
            Spot::TempoGap if d.tool == Tool::Range => self.plan_tempo_range(g, d),
            Spot::Lane { track, target } if d.tool == Tool::Select => {
                let (r, ticks) = self.lane_marquee(g, d, *track, target)?;
                Some(Plan {
                    command: None,
                    select: None,
                    tip: format!("{} 個", ticks.len()),
                    preview: Preview::Marquee(r),
                })
            }
            Spot::Row(kind) => self.plan_rows(g, d, *kind),
            Spot::Point {
                track,
                target,
                index,
            } => self.plan_point(d, *track, target, *index),
            Spot::Line {
                track,
                target,
                index,
            } => self.plan_curve(d, *track, target, *index),
            _ => None,
        }
    }

    /// The selected clips a drag on `clip` takes along.
    fn moving_clips(&self, t: TrackId, c: ClipId) -> Vec<(TrackId, ClipId)> {
        if self.arrange.clips.contains(&(t, c)) {
            self.arrange.clips.iter().copied().collect()
        } else {
            vec![(t, c)]
        }
    }

    fn plan_clip(&self, g: &Geometry, d: &Drag, t: TrackId, c: ClipId, grip: Grip) -> Option<Plan> {
        let p = self.project();
        let tm = p.time_map();
        let find = |(t, c): (TrackId, ClipId)| {
            p.tracks
                .get(&t)
                .and_then(|tr| tr.clips.iter().find(|k| k.id == c))
        };
        let clip = find((t, c))?;
        let sel: Vec<(TrackId, ClipId)> = self
            .moving_clips(t, c)
            .into_iter()
            .filter(|k| find(*k).is_some())
            .collect();
        let ids: Vec<String> = sel.iter().map(|(_, c)| c.to_string()).collect();
        let shift = d.mods.shift;
        let dt = self.px_to_ticks(d.at.0 - d.from.0);
        let min_len = self
            .tools()
            .snap
            .active(&self.meter(), shift)
            .unwrap_or(i64::from(p.ppq) / 16)
            .max(1);
        let color = |tid: TrackId| {
            p.tracks
                .get(&tid)
                .map_or(0, |tr| crate::draw::track_color(tr))
        };
        match grip {
            Grip::Body => {
                let min_start = sel.iter().filter_map(|k| find(*k)).map(|k| k.start).min()?;
                let start = self.snap_tick(clip.start + dt, shift);
                let delta = (start - clip.start).max(-min_start);
                let order: Vec<TrackId> = crate::draw::ordered_tracks(&p)
                    .iter()
                    .map(|t| t.id)
                    .collect();
                let index = |id: TrackId| order.iter().position(|x| *x == id);
                let mut rows = 0i64;
                if let Some(row) = self.tree().row_at(d.at.1 - g.column.y + self.scroll.1)
                    && let RowKind::Track(to) = row.kind
                    && let (Some(a), Some(b)) = (index(t), index(to))
                {
                    rows = b as i64 - a as i64;
                }
                let dest_by = |tid: TrackId, rows: i64| {
                    let i = index(tid)? as i64 + rows;
                    order.get(usize::try_from(i).ok()?).copied()
                };
                let all_fit = sel.iter().all(|(tid, cid)| {
                    dest_by(*tid, rows).is_some_and(|to| {
                        let body = find((*tid, *cid)).map(|k| &k.body);
                        p.tracks
                            .get(&to)
                            .zip(body)
                            .is_some_and(|(tr, b)| fits(b, tr.kind))
                    })
                });
                if !all_fit {
                    rows = 0;
                }
                if delta == 0 && rows == 0 {
                    return None;
                }
                let dest = |tid: TrackId| dest_by(tid, rows);
                let copy = d.copy || d.mods.alt;
                let mut args = json!({ "ids": ids, "by": delta });
                if rows != 0 {
                    args["rows"] = json!(rows);
                }
                if !copy {
                    let follow = if self.arrange.follows_clips {
                        "follow"
                    } else {
                        "stay"
                    };
                    args["automation"] = json!(follow);
                }
                let ghosts = sel
                    .iter()
                    .filter_map(|(tid, cid)| {
                        let k = find((*tid, *cid))?;
                        let to = dest(*tid).unwrap_or(*tid);
                        Some(Ghost {
                            track: to,
                            start: k.start + delta,
                            length: k.length,
                            color: color(*tid),
                        })
                    })
                    .collect();
                let mut tip = String::new();
                if copy {
                    tip.push_str("複製　");
                }
                tip.push_str(&tm.format(clip.start + delta));
                if rows != 0
                    && let Some(to) = dest(t).and_then(|to| p.tracks.get(&to))
                {
                    tip.push('　');
                    tip.push_str(&to.name);
                }
                Some(Plan {
                    command: Some((if copy { "clip.duplicate" } else { "clip.move" }, args)),
                    select: None,
                    tip,
                    preview: Preview::Clips(ghosts),
                })
            }
            Grip::Corner => self.plan_clip_loop(d, clip),
            Grip::Right if d.mods.alt && !d.mods.ctrl && !clip.is_midi() => {
                self.plan_speed(d, t, clip)
            }
            Grip::Right => {
                let end = clip.start + clip.length;
                let shortest = sel
                    .iter()
                    .filter_map(|k| find(*k))
                    .map(|k| k.length)
                    .min()?;
                let new_end = self.snap_tick(end + dt, shift);
                let by = (new_end - end).max(min_len.min(shortest) - shortest);
                if by == 0 {
                    return None;
                }
                let ghosts = sel
                    .iter()
                    .filter_map(|k| {
                        let c = find(*k)?;
                        Some(Ghost {
                            track: k.0,
                            start: c.start,
                            length: c.length + by,
                            color: color(k.0),
                        })
                    })
                    .collect();
                Some(Plan {
                    command: Some(("clip.resize", json!({ "ids": ids, "by": by }))),
                    select: None,
                    tip: format!(
                        "長さ {:.2} 小節",
                        (clip.length + by) as f64 / self.bar_ticks() as f64
                    ),
                    preview: Preview::Clips(ghosts),
                })
            }
            Grip::Left => {
                let new_start = self.snap_tick(clip.start + dt, shift);
                let mut by = new_start - clip.start;
                for k in sel.iter().filter_map(|k| find(*k)) {
                    let lo = match &k.body {
                        ClipBody::Audio { offset, .. } => -(*offset).min(k.start),
                        ClipBody::Midi { .. } => -k.start,
                    };
                    let hi = k.length - min_len.min(k.length);
                    by = by.clamp(lo, hi.max(lo));
                }
                if by == 0 {
                    return None;
                }
                let ghosts = sel
                    .iter()
                    .filter_map(|k| {
                        let c = find(*k)?;
                        Some(Ghost {
                            track: k.0,
                            start: c.start + by,
                            length: c.length - by,
                            color: color(k.0),
                        })
                    })
                    .collect();
                Some(Plan {
                    command: Some(("clip.trim", json!({ "ids": ids, "by": by }))),
                    select: None,
                    tip: format!("開始 {}", tm.format(clip.start + by)),
                    preview: Preview::Clips(ghosts),
                })
            }
        }
    }

    /// The rectangle an arrow dragged over a lane, and the times of the points inside it.
    fn lane_marquee(
        &self,
        g: &Geometry,
        d: &Drag,
        track: TrackId,
        target: &AutoTarget,
    ) -> Option<(Rect, BTreeSet<Tick>)> {
        let p = self.project();
        let row = *self.tree().row_of(RowKind::Track(track))?;
        let lanes = self.arrange.lanes_of(p.tracks.get(&track)?);
        let i = lanes.iter().position(|l| l.target == *target)?;
        let top = self.lane_top(g, &row, i);
        let (x0, x1) = (d.from.0.min(d.at.0), d.from.0.max(d.at.0));
        let (y0, y1) = (d.from.1.min(d.at.1), d.from.1.max(d.at.1));
        let (lx, rx) = (x0.max(g.lanes.x), x1.min(g.lanes.right()));
        let (ty, by) = (y0.max(top), y1.min(top + LANE_H));
        let r = Rect::new(lx, ty, (rx - lx).max(0.0), (by - ty).max(0.0));
        let ticks = lanes[i]
            .points
            .iter()
            .filter(|pt| {
                let (px, py) = (
                    self.tick_x(g, pt.tick),
                    top + lane_value_y(target, pt.value),
                );
                px >= r.x && px <= r.right() && py >= r.y && py <= r.bottom()
            })
            .map(|pt| pt.tick)
            .collect();
        Some((r, ticks))
    }

    /// A drag in the tempo row with the range tool: a stretch of time over the row (UI-241).
    fn plan_tempo_range(&self, g: &Geometry, d: &Drag) -> Option<Plan> {
        let a = self.snap_tick(d.tick, d.mods.shift);
        let b = self.snap_tick(self.tick_at(g, d.at.0).max(0), d.mods.shift);
        let (start, end) = (a.min(b), a.max(b));
        if end <= start {
            return None;
        }
        let tm = self.project().time_map();
        Some(Plan {
            command: None,
            select: Some(BTreeSet::new()),
            tip: format!("{} → {}", tm.format(start), tm.format(end)),
            preview: Preview::Range {
                start,
                end,
                top: g.tempo.y,
                bottom: g.tempo.bottom(),
            },
        })
    }

    /// A drag on empty lanes: a marquee with Select, a stretch of time with Range.
    fn plan_marquee(&self, g: &Geometry, d: &Drag) -> Option<Plan> {
        let p = self.project();
        let (x0, x1) = (d.from.0.min(d.at.0), d.from.0.max(d.at.0));
        let (y0, y1) = (d.from.1.min(d.at.1), d.from.1.max(d.at.1));
        let mut set = if d.mods.shift {
            self.arrange.clips.clone()
        } else {
            BTreeSet::new()
        };
        if d.tool == Tool::Range {
            let a = self.snap_tick(d.tick, d.mods.shift);
            let b = self.snap_tick(self.tick_at(g, d.at.0).max(0), d.mods.shift);
            let (start, end) = (a.min(b), a.max(b));
            if end <= start {
                return None;
            }
            let rows: Vec<&Row> = self
                .tree()
                .rows
                .iter()
                .filter(|r| {
                    let top = self.row_y(g, r);
                    top + r.h > y0 && top < y1.max(y0 + 1.0)
                })
                .collect();
            let top = rows.first().map_or(y0, |r| self.row_y(g, r));
            let bottom = rows.last().map_or(y1, |r| self.row_y(g, r) + r.h);
            for r in &rows {
                if let RowKind::Track(t) = r.kind
                    && let Some(tr) = p.tracks.get(&t)
                {
                    for c in &tr.clips {
                        if c.start < end && c.start + c.length > start {
                            set.insert((t, c.id));
                        }
                    }
                }
            }
            let tm = p.time_map();
            return Some(Plan {
                command: None,
                select: Some(set),
                tip: format!("{} → {}", tm.format(start), tm.format(end)),
                preview: Preview::Range {
                    start,
                    end,
                    top,
                    bottom,
                },
            });
        }
        let lanes = g.lanes;
        let r = Rect::new(
            x0.max(lanes.x),
            y0.max(lanes.y),
            (x1.min(lanes.right()) - x0.max(lanes.x)).max(0.0),
            (y1.min(lanes.bottom()) - y0.max(lanes.y)).max(0.0),
        );
        for row in &self.tree().rows {
            let RowKind::Track(t) = row.kind else {
                continue;
            };
            let Some(tr) = p.tracks.get(&t) else {
                continue;
            };
            for c in &tr.clips {
                let k = self.clip_rect(g, row, c);
                if k.x < r.right() && k.right() > r.x && k.y < r.bottom() && k.bottom() > r.y {
                    set.insert((t, c.id));
                }
            }
        }
        Some(Plan {
            command: None,
            tip: format!("{} 個", set.len()),
            select: Some(set),
            preview: Preview::Marquee(r),
        })
    }

    fn plan_loop(&self, g: &Geometry, d: &Drag, grip: LoopGrip) -> Option<Plan> {
        let p = self.project();
        let shift = d.mods.shift;
        let scale = self.ruler_scale(g);
        let now = p.loop_range.map(|l| (l.start, l.end));
        let (start, end) = crate::ruler::loop_drag(
            now,
            grip,
            d.tick,
            scale.tick(d.at.0),
            scale.delta(d.at.0 - d.from.0),
            |t| self.snap_tick(t, shift),
        )?;
        let tm = p.time_map();
        Some(Plan {
            command: Some(("loop.set", json!({ "start": start, "end": end }))),
            select: None,
            tip: format!("{} → {}", tm.format(start), tm.format(end)),
            preview: Preview::Loop(start, end),
        })
    }

    fn plan_section(&self, d: &Drag, id: SectionId, index: usize, resize: bool) -> Option<Plan> {
        let p = self.project();
        let s = p.sections.get(index).filter(|s| s.id == id)?;
        let dx = d.at.0 - d.from.0;
        let dt = self.px_to_ticks(dx);
        if resize {
            // The end lands on the grid's lines (UI-339): the bar lines of the
            // meter there, and with Shift the quarters counted from the bar's start.
            let tm = p.time_map();
            let beat = i64::from(p.ppq);
            let step = if d.mods.shift { beat } else { Tick::MAX };
            let end = round_in_bars(&tm, s.start + s.length + dt, step);
            let length = (end - s.start).max(beat);
            // The next section is a wall (ARR-007): the picture stops where
            // the command will.
            let room = p
                .sections
                .iter()
                .filter(|o| o.id != id && o.start > s.start)
                .map(|o| o.start - s.start)
                .min();
            let stopped = room.is_some_and(|r| length > r);
            let length = room.map_or(length, |r| length.min(r));
            if length == s.length {
                return None;
            }
            let bars = format!("{:.2} 小節", bars_between(&tm, s.start, s.start + length));
            return Some(Plan {
                command: Some((
                    "section.resize",
                    json!({ "id": id.to_string(), "length": length }),
                )),
                select: None,
                tip: if stopped {
                    format!("{bars}（次のセクションまで）")
                } else {
                    bars
                },
                preview: Preview::SectionLength { id, length },
            });
        }
        let centre = s.start + dt + s.length / 2;
        let to = p
            .sections
            .iter()
            .enumerate()
            .filter(|(i, o)| *i != index && o.start + o.length / 2 < centre)
            .count();
        if to == index {
            return None;
        }
        let o = &p.sections[to];
        let slot = if to < index {
            o.start
        } else {
            o.start + o.length
        };
        // Alt keeps the original where it is and puts a copy at the slot (ARR-002).
        let (command, tip) = if d.mods.alt {
            // The copy's place among all of them, the original still there.
            let place = if to < index { to } else { to + 1 };
            (
                (
                    "section.duplicate",
                    json!({ "id": id.to_string(), "index": place }),
                ),
                format!("{} 番目に複製", place + 1),
            )
        } else {
            (
                (
                    "section.reorder",
                    json!({ "id": id.to_string(), "index": to }),
                ),
                format!("{} → {} 番目", s.name, to + 1),
            )
        };
        Some(Plan {
            command: Some(command),
            select: None,
            tip,
            preview: Preview::Section { id, dx, slot },
        })
    }

    fn plan_tempo(&self, d: &Drag, index: usize) -> Option<Plan> {
        let p = self.project();
        let t = p.tempo.get(index)?;
        let dy = f64::from(d.at.1 - d.from.1);
        let bpm = if dy == 0.0 {
            t.bpm
        } else {
            (if d.mods.shift {
                ((t.bpm - dy * 0.1) * 10.0).round() / 10.0
            } else {
                (t.bpm - dy * 0.5).round()
            })
            .clamp(20.0, 300.0)
        };
        let dx = d.at.0 - d.from.0;
        let tick = if index == 0 || dx == 0.0 {
            t.tick
        } else {
            self.snap_tick(t.tick + self.px_to_ticks(dx), d.mods.shift)
                .clamp(
                    p.tempo[index - 1].tick + 1,
                    p.tempo.get(index + 1).map_or(i64::MAX, |n| n.tick - 1),
                )
        };
        if bpm == t.bpm && tick == t.tick {
            return None;
        }
        let tm = p.time_map();
        Some(Plan {
            command: Some(("tempo.move", json!({ "id":t.id,"bpm":bpm,"at":tick }))),
            select: None,
            tip: format!("{}　{} BPM", tm.format(tick), bpm),
            preview: Preview::Tempo { index, tick, bpm },
        })
    }

    /// Where rows dropped at the pointer would go: the parent, the index in
    /// it before anything is taken out, and the mark to paint.
    pub fn drop_at(
        &self,
        g: &Geometry,
        y: f32,
        moving: &[RowKind],
    ) -> Option<(Parent, usize, DropMark)> {
        let p = self.project();
        let tree = self.tree();
        let ty = y - g.column.y + self.scroll.1;
        let Some(row) = tree.row_at(ty) else {
            let last = tree.rows.last()?;
            return (ty >= last.bottom()).then(|| {
                (
                    Parent::Root,
                    p.root.len(),
                    DropMark::Line {
                        y: tree.height,
                        depth: 0,
                    },
                )
            });
        };
        // Not onto a row that moves, nor into a folder that moves.
        let mut up = Some(*row);
        while let Some(r) = up {
            if moving.contains(&r.kind) {
                return None;
            }
            up = r
                .parent
                .and_then(|f| tree.row_of(RowKind::Folder(f)).copied());
        }
        let fr = (ty - row.y) / row.full_h().max(1.0);
        let (parent, i) = p.find_node(node_of(row.kind))?;
        let before = (
            parent,
            i,
            DropMark::Line {
                y: row.y,
                depth: row.depth,
            },
        );
        let after = (
            parent,
            i + 1,
            DropMark::Line {
                y: row.bottom(),
                depth: row.depth,
            },
        );
        match row.kind {
            RowKind::Track(_) => {
                if fr < 0.5 {
                    return Some(before);
                }
                if fr > 0.75
                    && i + 1 == p.children(parent).len()
                    && let Parent::Folder(f) = parent
                    && let Some((pp, j)) = p.find_node(Node::Folder(f))
                {
                    return Some((
                        pp,
                        j + 1,
                        DropMark::Line {
                            y: row.bottom(),
                            depth: row.depth.saturating_sub(1),
                        },
                    ));
                }
                Some(after)
            }
            RowKind::Folder(f) => {
                let children = p.children(Parent::Folder(f)).len();
                if fr < 0.3 {
                    Some(before)
                } else if fr > 0.7 {
                    if !self.is_collapsed(f) && children > 0 {
                        Some((
                            Parent::Folder(f),
                            0,
                            DropMark::Line {
                                y: row.bottom(),
                                depth: row.depth + 1,
                            },
                        ))
                    } else {
                        Some(after)
                    }
                } else {
                    Some((Parent::Folder(f), children, DropMark::Into(f)))
                }
            }
        }
    }

    fn plan_rows(&self, g: &Geometry, d: &Drag, kind: RowKind) -> Option<Plan> {
        let p = self.project();
        let mut moving: Vec<RowKind> = if self.arrange.tracks.contains(&kind) {
            self.tree()
                .rows
                .iter()
                .map(|r| r.kind)
                .filter(|k| self.arrange.tracks.contains(k))
                .collect()
        } else {
            vec![kind]
        };
        // A row inside a folder that moves goes with the folder.
        let inside = |k: &RowKind| {
            let mut up = self.tree().row_of(*k).and_then(|r| r.parent);
            while let Some(f) = up {
                if moving.contains(&RowKind::Folder(f)) {
                    return true;
                }
                up = self
                    .tree()
                    .row_of(RowKind::Folder(f))
                    .and_then(|r| r.parent);
            }
            false
        };
        let keep: Vec<RowKind> = moving.iter().copied().filter(|k| !inside(k)).collect();
        moving = keep;
        let (parent, index, mark) = self.drop_at(g, d.at.1, &moving)?;
        let copy = d.copy || d.mods.alt;
        let into = match parent {
            Parent::Root => "root".to_string(),
            Parent::Folder(f) => f.to_string(),
        };
        let name = |k: &RowKind| match k {
            RowKind::Track(t) => p.tracks.get(t).map(|t| t.name.clone()),
            RowKind::Folder(f) => p.folders.get(f).map(|f| f.name.clone()),
        };
        let mut tip = if moving.len() == 1 {
            name(&moving[0]).unwrap_or_default()
        } else {
            format!("{} トラック", moving.len())
        };
        let command = if copy {
            let tracks: Vec<String> = moving
                .iter()
                .filter(|k| matches!(k, RowKind::Track(_)))
                .map(|k| node_text(*k))
                .collect();
            if tracks.is_empty() {
                return None;
            }
            tip.push_str(" を複製");
            (
                "track.duplicate",
                json!({ "ids": tracks, "into": into, "index": index }),
            )
        } else {
            // The index counts the list after the moving rows are taken out.
            let above = moving
                .iter()
                .filter_map(|k| p.find_node(node_of(*k)))
                .filter(|(par, i)| *par == parent && *i < index)
                .count();
            let index = index - above;
            if let [one] = moving.as_slice()
                && p.find_node(node_of(*one)) == Some((parent, index))
            {
                return None;
            }
            let ids: Vec<String> = moving.iter().map(|k| node_text(*k)).collect();
            (
                "track.move",
                json!({ "ids": ids, "into": into, "index": index }),
            )
        };
        Some(Plan {
            command: Some(command),
            select: None,
            tip,
            preview: Preview::Drop { mark, moving },
        })
    }

    fn lane_of(&self, t: TrackId, target: &AutoTarget) -> Option<AutoLane> {
        self.project()
            .tracks
            .get(&t)?
            .automation
            .iter()
            .find(|l| l.target == *target)
            .cloned()
    }

    fn plan_point(&self, d: &Drag, t: TrackId, target: &AutoTarget, index: usize) -> Option<Plan> {
        let lane = self.lane_of(t, target)?;
        let pts = &*lane.points;
        let pt = *pts.get(index)?;
        let (dx, dy) = (d.at.0 - d.from.0, d.at.1 - d.from.1);
        // Ctrl keeps the point on the axis it moves most along.
        let (move_x, move_y) = if d.mods.ctrl {
            (dx.abs() >= dy.abs(), dy.abs() > dx.abs())
        } else {
            (true, true)
        };
        let fine = if d.mods.shift { 0.1 } else { 1.0 };
        let tick = if move_x {
            self.snap_tick(pt.tick + self.px_to_ticks(dx * fine), d.mods.shift)
                .max(0)
        } else {
            pt.tick
        };
        let value = if move_y {
            let n = lane_norm(target, pt.value)
                - f64::from(dy * fine / (LANE_H - Arrange::LANE_PAD * 2.0));
            lane_value(target, n)
        } else {
            pt.value
        };
        let mut pushed = lane.clone();
        let tick = pushed.push_point(index, tick, value)?;
        if tick == pt.tick && value == pt.value {
            return None;
        }
        let tm = self.project().time_map();
        Some(Plan {
            command: Some((
                "auto.point.move",
                json!({ "track": t.to_string(), "target": target.to_string(),
                        "index": index, "at": tick, "value": value }),
            )),
            select: None,
            tip: format!("{}　{}", tm.format(tick), value_text(target, value)),
            preview: Preview::Point {
                track: t,
                target: target.clone(),
                index,
                tick,
                value,
            },
        })
    }

    fn plan_curve(&self, d: &Drag, t: TrackId, target: &AutoTarget, index: usize) -> Option<Plan> {
        let lane = self.lane_of(t, target)?;
        let (a, b) = (lane.points.get(index)?, lane.points.get(index + 1)?);
        let dir = if lane_norm(target, b.value) >= lane_norm(target, a.value) {
            1.0
        } else {
            -1.0
        };
        let dy = f64::from(d.at.1 - d.from.1);
        let curve = ((a.curve + -dy / 36.0 * dir) * 100.0).round() / 100.0;
        let curve = curve.clamp(-1.0, 1.0);
        if curve == a.curve {
            return None;
        }
        Some(Plan {
            command: Some((
                "auto.segment.shape",
                json!({ "track": t.to_string(), "target": target.to_string(),
                        "index": index, "curve": curve }),
            )),
            select: None,
            tip: format!("カーブ {curve:+.2}"),
            preview: Preview::Curve {
                track: t,
                target: target.clone(),
                index,
                curve,
            },
        })
    }
}

/// How many bars from `a` to `b`, each bar counted in its own meter.
fn bars_between(tm: &xirius_core::time::TimeMap, a: Tick, b: Tick) -> f64 {
    let at = |t: Tick| {
        let (bar, _, _) = tm.to_bbt(t);
        let (s, e) = (tm.bar_start(bar), tm.bar_start(bar + 1));
        f64::from(bar) + (t - s) as f64 / (e - s).max(1) as f64
    };
    at(b) - at(a)
}
