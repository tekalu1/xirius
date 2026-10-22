//! The piano roll and the controller lanes under it (EDT-003, EDT-004,
//! EDT-012 … EDT-014, EDT-047 … EDT-068).
//!
//! [`Roll`] is plain data: the view (zoom, scroll), the selection, the drag in
//! progress and what that drag would do. The input methods take a pointer
//! position in the body, the modifier keys held right now and a [`Ctx`] (the
//! song as it is), and answer with an [`Out`]: what changed on screen and the
//! core commands to run. Nothing here opens a window or reads the OS, so the
//! whole feel is tested by feeding positions and keys in.
//!
//! One drag is one history step. While the pointer is down nothing is written
//! to the song: the drag keeps the notes as they were at the press and works
//! out a preview from them, the pointer and the keys held now (so a key
//! pressed or let go mid-drag changes the drag at once, EDT-053). The release
//! turns the preview into one core command.

mod ai;
mod bridge;
mod drum_rows;
#[cfg(test)]
mod edit_tests;
mod editing;
#[cfg(test)]
mod feel2_tests;
mod frame;
mod keys;
mod lanemark;
mod lanes;
pub mod layer;
pub mod menu;
mod notes;
pub mod paint;
pub mod parts;
mod pointclip;
pub mod probe;
mod ruler;
#[cfg(test)]
mod tests;
mod visible;
mod zoom;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::id::{ClipId, NoteId};
use xirius_core::model::{Clip, CtrlPoint, Note, Project};
use xirius_core::time::Tick;

use crate::ruler::LoopGrip;
use crate::studio::{ROLL_BEAT_W, Rect};
use crate::tools::{Meter, Mods, Tools};

pub use frame::{Frame, LaneBox, Rows, View};
pub use lanes::{COMMON, Lane, LaneKind, LaneTool, NUMBERED, y_of};
pub use zoom::{STEP as ZOOM_STEP, THUMB as ZOOM_THUMB, pos_of as zoom_pos};

pub const ROW_H: f32 = 14.0;
/// The width of the name column in the drum view (the keyboard's is `Layout::PIANO_KEYS_W`).
pub const DRUM_KEYS_W: f32 = 124.0;
pub const ROW_H_MIN: f32 = 10.0;
pub const ROW_H_MAX: f32 = 48.0;
pub const BEAT_PX_MIN: f32 = 8.0;
/// As [`crate::studio::Studio::BEAT_W_MAX`]: one beat fills the roll.
pub const BEAT_PX_MAX: f32 = 4096.0;
/// A drag shorter than this is still a click.
pub const SLOP: f32 = 3.0;
/// Velocity per pixel of vertical drag inside the roll.
pub const VEL_PER_PX: f32 = 1.1;

/// The song as the roll sees it at the moment of an event.
#[derive(Clone)]
pub struct Ctx {
    pub mousemap: xirius_prefs::Mousemap,
    pub prefs: xirius_prefs::Prefs,
    pub project: Arc<Project>,
    pub clip: Clip,
    pub tools: Tools,
    /// The shape the pencil draws (the top bar's).
    pub shape: xirius_core::shape::Shape,
    /// The drawer's content: the roll's whole region.
    pub body: Rect,
    /// The drum map the clip is shown in (EDT-006), or none for the keyboard.
    pub drum: Option<String>,
    /// What was changed in the track's drum rows: their order, the hidden ones and their names.
    pub drum_rows: xirius_core::drum::RowEdits,
    /// The other parts edited with the clip (EDT-062).
    pub others: Vec<parts::Part>,
}

impl Ctx {
    pub fn ppq(&self) -> Tick {
        Tick::from(self.project.ppq)
    }

    pub fn notes(&self) -> &[Note] {
        self.clip.notes()
    }

    /// The rows the grid has: the keyboard, or the drum map's with a row for each pitch the clip
    /// uses outside the map, without the hidden ones but those with notes (so no note is hidden),
    /// in the order set for the track.
    pub fn rows(&self) -> Rows {
        let Some(map) = &self.drum else {
            return Rows::KEYS;
        };
        Rows::ordered(xirius_core::drum::shown_rows(
            map,
            &self.drum_rows,
            self.notes().iter().map(|n| n.pitch),
        ))
    }

    /// The name of a drum row: the one given for the track, else the map's.
    pub fn drum_name(&self, pitch: u8) -> String {
        self.drum_rows.name(pitch)
    }

    pub fn note(&self, id: NoteId) -> Option<&Note> {
        self.notes().iter().find(|n| n.id == id)
    }

    pub fn beats_per_bar(&self) -> u32 {
        self.project.timesig.first().map_or(4, |t| t.num)
    }

    pub fn meter(&self, beat_px: f32) -> Meter {
        Meter {
            ppq: self.project.ppq,
            beats_per_bar: self.beats_per_bar(),
            bpm: self.project.tempo.first().map_or(120.0, |t| t.bpm),
            beat_px,
        }
    }

    /// The grid step in ticks (also when snapping is off).
    pub fn step(&self, beat_px: f32) -> Tick {
        self.tools.snap.step(&self.meter(beat_px))
    }

    /// The step a pointer snaps to now: none when snapping is off or Shift
    /// turns it off for this drag.
    pub fn snap(&self, beat_px: f32, shift: bool) -> Option<Tick> {
        self.tools.snap.active(&self.meter(beat_px), shift)
    }

    /// The shortest a note may be made by dragging.
    pub fn min_len(&self, beat_px: f32, shift: bool) -> Tick {
        let quarter = (self.ppq() / 4).max(1);
        self.snap(beat_px, shift)
            .unwrap_or((self.ppq() / 16).max(1))
            .min(quarter)
    }

    /// `tick` (from the clip's head) rounded to the grid in song time, so a
    /// clip that does not start on a grid line still snaps to the bar lines.
    pub fn snapped(&self, beat_px: f32, shift: bool, tick: Tick) -> Tick {
        match self.snap(beat_px, shift) {
            Some(step) => crate::tools::snap_round(tick + self.clip.start, step) - self.clip.start,
            None => tick,
        }
    }

    pub fn floored(&self, beat_px: f32, shift: bool, tick: Tick) -> Tick {
        match self.snap(beat_px, shift) {
            Some(step) => crate::tools::snap_floor(tick + self.clip.start, step) - self.clip.start,
            None => tick,
        }
    }

    pub fn clip_len(&self) -> Tick {
        self.clip.length
    }

    /// "5.1.0" for a tick from the clip's head.
    pub fn bbt(&self, tick: Tick) -> String {
        self.project.time_map().format(self.clip.start + tick)
    }
}

/// What a note looks like: the song's, or the drag's preview of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteView {
    pub start: Tick,
    pub length: Tick,
    pub pitch: u8,
    pub velocity: u8,
    pub muted: bool,
}

impl From<&Note> for NoteView {
    fn from(n: &Note) -> NoteView {
        NoteView {
            start: n.start,
            length: n.length,
            pitch: n.pitch,
            velocity: n.velocity,
            muted: n.muted,
        }
    }
}

/// The four places on a note that act differently (EDT-047).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Body,
    EdgeL,
    EdgeR,
    /// The upper strip: velocity.
    Velocity,
    /// The lower strip: split.
    Split,
}

/// The pointer shapes the roll asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Move,
    ResizeH,
    ResizeV,
    Split,
    Cross,
    Pencil,
    Erase,
    Mute,
    Audition,
    Range,
    /// The tool has nothing to act on where the pointer is.
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Move,
    ResizeR,
    ResizeL,
    Velocity,
    Draw,
    Erase,
    Marquee,
    Audition,
    Part,
    /// A drag in a controller lane (see [`lanes::LaneDrag`]).
    Lane,
    /// A drag on the loop band of the ruler (see [`ruler`]).
    Loop(LoopGrip),
    /// A drag on the clip's end handle on the ruler.
    ClipEnd,
    /// A drag on the zoom slider in the strip.
    Zoom,
}

/// What is held down.
#[derive(Clone, Debug)]
pub struct Drag {
    pub map_context: Option<String>,
    pub click_action: Option<String>,
    pub drag_enabled: bool,
    pub mode: Mode,
    pub press: (f32, f32),
    /// Where the pointer was at the previous event.
    pub last: (f32, f32),
    pub moved: bool,
    /// The notes dragged and how they were at the press.
    pub orig: Vec<(NoteId, NoteView)>,
    pub head: usize,
    pub zone: Zone,
    /// Shift on an already selected note: a click takes it out.
    pub toggle_off: bool,
    pub clicked: Option<NoteId>,
    pub vel_per_px: f32,
    /// Where a drawn note starts, and how it starts.
    pub draw: Option<(Tick, u8, u8)>,
    /// The selection to keep under a rubber band.
    pub base: BTreeSet<NoteId>,
    pub marked: BTreeSet<NoteId>,
    pub lane: Option<lanes::LaneDrag>,
}

impl Drag {
    pub fn new(mode: Mode, press: (f32, f32)) -> Drag {
        Drag {
            map_context: None,
            click_action: None,
            drag_enabled: true,
            mode,
            press,
            last: press,
            moved: false,
            orig: Vec::new(),
            head: 0,
            zone: Zone::Body,
            toggle_off: false,
            clicked: None,
            vel_per_px: VEL_PER_PX,
            draw: None,
            base: BTreeSet::new(),
            marked: BTreeSet::new(),
            lane: None,
        }
    }
}

/// The value balloon that follows the pointer during a drag (EDT-063).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Balloon {
    pub at: (i32, i32),
    pub text: String,
}

/// What the drag would do if it ended now.
#[derive(Clone, Debug, Default)]
pub struct Preview {
    pub changed: HashMap<NoteId, NoteView>,
    /// Notes that do not exist yet: copies, a drawn note, a line of notes.
    pub ghosts: Vec<NoteView>,
    pub balloon: Option<Balloon>,
    /// The rubber band, in world pixels (x from the head, y from pitch 127).
    pub marquee: Option<((f32, f32), (f32, f32))>,
    /// Notes the eraser has marked.
    pub erasing: BTreeSet<NoteId>,
    pub lane: Option<(usize, Vec<CtrlPoint>)>,
    /// The rubber band drawn in a controller lane, in window pixels.
    pub band: Option<((f32, f32), (f32, f32))>,
    /// The loop a drag on the ruler's band would leave (song ticks).
    pub loop_span: Option<(Tick, Tick)>,
    /// The clip's length (ticks from its head) a drag on its end handle would leave.
    pub clip_end: Option<Tick>,
    /// The spread (ticks) a sideways drag with Mod+Alt would strum the chords by (EDT-054).
    pub strum: Option<Tick>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    Keep,
    /// Select the notes the command answered with (`ids`).
    Select,
    Clear,
    Part,
}

/// One core command the roll wants run.
#[derive(Clone, Debug)]
pub struct Edit {
    pub command: &'static str,
    pub args: Value,
    pub then: Then,
}

impl Edit {
    pub fn new(command: &'static str, args: Value, then: Then) -> Edit {
        Edit {
            command,
            args,
            then,
        }
    }
}

/// What an input did.
#[derive(Clone, Debug, Default)]
pub struct Out {
    pub next_velocity: Option<u8>,
    pub tool: Option<crate::tools::Tool>,
    pub edits: Vec<Edit>,
    pub redraw: bool,
    /// The input belonged to the roll (so nothing else acts on it).
    pub used: bool,
    /// Where the playhead goes (song ticks): a press on the ruler.
    pub seek: Option<Tick>,
    /// The autoscroll switch was pressed (it runs the same toggle as F).
    pub follow: bool,
}

impl Out {
    pub fn used() -> Out {
        Out {
            used: true,
            redraw: true,
            ..Out::default()
        }
    }

    pub fn edit(edit: Edit) -> Out {
        Out {
            edits: vec![edit],
            ..Out::used()
        }
    }

    pub fn quiet() -> Out {
        Out::default()
    }

    pub fn push(mut self, edit: Edit) -> Out {
        self.edits.push(edit);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Menu {
    /// "+" in the strip: the lanes that are not shown.
    Add,
    /// "番号で選ぶ…": a controller by number.
    Numbers,
}

#[derive(Clone, Debug)]
pub struct Roll {
    pub extended: bool,
    pub scale: Option<(u8, u16)>,
    pub clipboard: Vec<NoteView>,
    /// Last immutable song snapshot seen by selection reconciliation.
    pub synced_project: Option<Arc<Project>>,
    /// Last body paint duration, used by the drag benchmark.
    pub paint_us: std::cell::Cell<u64>,
    pub roll_paint_us: std::cell::Cell<u64>,
    pub note_paint_us: std::cell::Cell<u64>,
    pub disjoint_notes: std::cell::Cell<bool>,
    /// The index of the clip's notes by span, for what the grid shows.
    pub spatial: crate::spatial::NoteCache,
    pub row_h: f32,
    pub beat_px: f32,
    /// Distance scrolled right (from the clip's head) and down (from pitch 127).
    pub scroll: (f32, f32),
    pub clip: Option<ClipId>,
    /// The drum map the grid was last centered for, to center again when the view switches.
    pub drum_seen: Option<String>,
    /// The width of the column left of the grid: the piano keys, or the drum names.
    pub keys_w: f32,
    pub selected: BTreeSet<NoteId>,
    /// The velocity a drawn note starts with: the last one made by hand.
    pub next_vel: u8,
    /// The length of the note last drawn, resized or selected (EDT-131): what the pencil lays down
    /// when the setting says "直前のノート". 0 until there is one.
    pub next_length: Tick,
    pub drag: Option<Drag>,
    pub preview: Preview,
    pub pointer: Option<(f32, f32)>,
    pub mods: Mods,
    pub hover: Option<(NoteId, Zone)>,
    /// The loop grip under the pointer on the ruler's band.
    pub ruler_hover: Option<LoopGrip>,
    /// The pointer is on the clip's end handle on the ruler.
    pub end_hover: bool,
    /// The tick in the middle of the grid when a drag of the zoom slider began.
    pub zoom_anchor: Option<f64>,
    /// The key that lights up (an audition, or the note being dragged).
    pub audition: Option<u8>,
    /// The song tick the audition tool was pressed at: while it is held, the clip's track plays
    /// alone from there (EDT-027).
    pub listen: Option<Tick>,
    /// `edit.audition`: a note picked, drawn or moved sounds too, not only the keyboard and the
    /// audition tool.
    pub audition_edits: bool,
    /// The lit key is a note picked or drawn, not the keyboard or the audition tool.
    pub audition_by_edit: bool,
    /// The lit key is a note a key moved or picked: it lets go when the key comes up.
    pub audition_by_key: bool,
    pub lanes: Vec<Lane>,
    pub lane_tool: LaneTool,
    pub lanes_folded: bool,
    pub active_lane: usize,
    pub menu: Option<Menu>,
    /// Points selected in the active lane, by tick.
    pub points: BTreeSet<Tick>,
    /// Controllers whose lane was hidden by hand: they stay hidden while they have points.
    pub hidden: BTreeSet<xirius_core::model::CtrlKind>,
    /// The names the drum rows' instrument gave its keys when last looked (EDT-134).
    pub drum_names: std::collections::BTreeMap<u8, String>,
    /// The track the player was started for, so that its instrument is loaded before a key or
    /// a note sounds and its names can be read (EDT-134, UI-285).
    pub host_for: Option<xirius_core::id::TrackId>,
}

impl Default for Roll {
    fn default() -> Roll {
        Roll {
            extended: false,
            scale: None,
            clipboard: Vec::new(),
            synced_project: None,
            paint_us: std::cell::Cell::new(0),
            roll_paint_us: std::cell::Cell::new(0),
            note_paint_us: std::cell::Cell::new(0),
            disjoint_notes: std::cell::Cell::new(false),
            spatial: crate::spatial::NoteCache::default(),
            row_h: ROW_H,
            beat_px: ROLL_BEAT_W,
            scroll: (0.0, 0.0),
            clip: None,
            drum_seen: None,
            keys_w: xirius_design::Layout::PIANO_KEYS_W,
            selected: BTreeSet::new(),
            next_vel: 100,
            next_length: 0,
            drag: None,
            preview: Preview::default(),
            pointer: None,
            mods: Mods::NONE,
            hover: None,
            ruler_hover: None,
            end_hover: false,
            zoom_anchor: None,
            audition: None,
            listen: None,
            audition_edits: true,
            audition_by_edit: false,
            audition_by_key: false,
            lanes: vec![Lane::new(LaneKind::Velocity)],
            lane_tool: LaneTool::Pencil,
            lanes_folded: false,
            active_lane: 0,
            menu: None,
            points: BTreeSet::new(),
            hidden: BTreeSet::new(),
            drum_names: Default::default(),
            host_for: None,
        }
    }
}

pub fn ids_json<'a>(ids: impl IntoIterator<Item = &'a NoteId>) -> Value {
    Value::Array(ids.into_iter().map(|i| json!(i.to_string())).collect())
}

/// "C4", "F#3": the name of a pitch (60 is C4).
pub fn pitch_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(pitch % 12)],
        i32::from(pitch) / 12 - 1
    )
}

/// A length as a note value ("1/16") when it is one, else in beats.
pub fn length_text(length: Tick, ppq: Tick) -> String {
    let f = length as f64 / ppq as f64 / 4.0;
    for d in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0] {
        let x = f * d;
        if x >= 0.999 && (x - x.round()).abs() < 1e-4 {
            return format!("{}/{}", x.round() as i64, d as i64);
        }
    }
    let beats = length as f64 / ppq as f64;
    let s = format!("{beats:.2}");
    format!("{} 拍", s.trim_end_matches('0').trim_end_matches('.'))
}

impl Roll {
    pub fn new() -> Roll {
        Roll::default()
    }

    /// The lane the pointer last asked about, if the active index is valid.
    pub fn active(&self) -> Option<&Lane> {
        self.lanes.get(self.active_lane)
    }

    /// The pointer shape for the pointer where it is now.
    pub fn cursor(&self, ctx: Option<&Ctx>) -> Cursor {
        use crate::tools::{Area, Tool};
        let Some(ctx) = ctx else {
            return Cursor::Arrow;
        };
        if let Some(d) = &self.drag {
            return match d.mode {
                Mode::Move => Cursor::Move,
                Mode::ResizeL | Mode::ResizeR => Cursor::ResizeH,
                Mode::Velocity => Cursor::ResizeV,
                Mode::Erase => Cursor::Erase,
                Mode::ClipEnd | Mode::Zoom => Cursor::ResizeH,
                Mode::Lane
                    if matches!(
                        d.lane,
                        Some(lanes::LaneDrag {
                            what: lanes::LaneWhat::Transform { .. },
                            ..
                        })
                    ) =>
                {
                    Cursor::ResizeV
                }
                _ => Cursor::Cross,
            };
        }
        let Some((x, y)) = self.pointer else {
            return Cursor::Arrow;
        };
        let f = self.frame(ctx.body);
        if self.end_hover {
            return Cursor::ResizeH;
        }
        if !f.grid.contains(x, y) {
            let in_lane = f.lanes.iter().find(|l| l.plot.contains(x, y));
            let Some(lane) = in_lane else {
                return Cursor::Arrow;
            };
            if self.lanes[lane.index].kind == LaneKind::Velocity
                && self.handle_at(ctx, &lane.plot, x, y).is_some()
            {
                return Cursor::ResizeV;
            }
            // Split and mute have nothing to act on in a controller lane.
            return match ctx.tools.held(Area::Roll, self.mods) {
                Tool::Cut | Tool::Mute => Cursor::Blocked,
                Tool::Draw => Cursor::Pencil,
                _ => Cursor::Cross,
            };
        }
        let tool = ctx.tools.held(Area::Roll, self.mods);
        // The tools that act on a note have nothing to act on between the notes.
        let on_note = |shape| {
            if self.hover.is_some() {
                shape
            } else {
                Cursor::Blocked
            }
        };
        match tool {
            Tool::Draw => Cursor::Pencil,
            Tool::Eraser => on_note(Cursor::Erase),
            Tool::Cut => on_note(Cursor::Split),
            Tool::Mute => on_note(Cursor::Mute),
            Tool::Audition => on_note(Cursor::Audition),
            Tool::Range => Cursor::Range,
            Tool::Select => match self.hover {
                Some((_, Zone::EdgeL | Zone::EdgeR)) => Cursor::ResizeH,
                Some((_, Zone::Velocity)) => Cursor::ResizeV,
                Some((_, Zone::Split)) => Cursor::Split,
                Some((_, Zone::Body)) => Cursor::Move,
                None => Cursor::Arrow,
            },
        }
    }

    /// Where note `i` of the clip is drawn now, for the scripts that press on it.
    pub fn note_box(&self, ctx: &Ctx, i: usize) -> Option<Rect> {
        let n = ctx.notes().get(i)?;
        Some(notes::note_rect(&self.view(ctx), &self.shown(n)))
    }

    /// The notes as they are shown now: the song's, with the drag's changes.
    pub fn shown(&self, n: &Note) -> NoteView {
        self.preview
            .changed
            .get(&n.id)
            .copied()
            .unwrap_or_else(|| NoteView::from(n))
    }

    /// The notes in `ctx`, keyed for lookups.
    pub fn by_id(ctx: &Ctx) -> BTreeMap<NoteId, &Note> {
        ctx.notes().iter().map(|n| (n.id, n)).collect()
    }
}
