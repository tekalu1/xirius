//! The song model: tracks, buses, folders, clips, notes, tempo, meter and
//! arrangement sections.
//!
//! Signal flow of every channel (tracks, buses, master alike):
//!
//! ```text
//! input -> inserts (unlimited) -> fader/pan -> sends (unlimited, post by default) -> output
//! ```
//!
//! The output is the master or exactly one bus. Folders only organise the
//! track column; they carry no sound, but a folder may be *connected to a bus*,
//! in which case the tracks inside that do not name an output of their own
//! play into that bus.
//!
//! Tracks and clip note lists are held behind `Arc`, so a copy of a whole
//! `Project` is cheap and the history can keep one per step.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::auto::{AutoLane, AutoMode};
use crate::error::{CmdError, CmdResult};
use crate::id::*;
use crate::time::{DEFAULT_PPQ, Meter, Tick, TimeMap};

pub const MIN_DB: f64 = -120.0;
pub const MAX_DB: f64 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Audio,
    Instrument,
    Bus,
    Master,
}

/// What a track is for, chosen by the user; only decides the icon and the
/// default look. Audio and instrument tracks without one use their kind's icon.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackIcon {
    Drums,
    Keys,
    Mic,
    Wave,
}

impl TrackIcon {
    pub const ALL: [TrackIcon; 4] = [
        TrackIcon::Drums,
        TrackIcon::Keys,
        TrackIcon::Mic,
        TrackIcon::Wave,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TrackIcon::Drums => "drums",
            TrackIcon::Keys => "keys",
            TrackIcon::Mic => "mic",
            TrackIcon::Wave => "wave",
        }
    }
}

/// The sound a frozen track plays instead of its clips, instrument and inserts (EXP-004): the
/// track rendered up to its fader, from `start` on. The fader, the pan and the sends still work.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Frozen {
    /// The WAV, relative to the song's folder like a clip's file.
    pub file: String,
    #[serde(default)]
    pub start: Tick,
}

/// A MIDI output port a track sends its notes and controllers to (ENG-028).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MidiOut {
    /// The port's name, as the system lists it.
    pub port: String,
    /// 1..=16.
    pub channel: u8,
}

/// Where a channel plays to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Output {
    Master,
    Bus(TrackId),
}

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Master => f.write_str("master"),
            Output::Bus(b) => write!(f, "{b}"),
        }
    }
}

impl Serialize for Output {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Output {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let id: TrackId = s.parse().map_err(serde::de::Error::custom)?;
        Ok(if id.is_master() {
            Output::Master
        } else {
            Output::Bus(id)
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Instrument,
    Effect,
}

/// A plugin on an insert row. What the host does not understand (`uid`,
/// `params`, `state`) is kept verbatim so a song opened on a machine that
/// lacks the plugin still saves it back unchanged (PRJ-008).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Plugin {
    pub id: PluginId,
    pub name: String,
    pub kind: PluginKind,
    /// `vst3`, `clap`, `builtin`, ...
    pub format: String,
    /// Format-specific unique id of the plugin.
    pub uid: String,
    #[serde(default)]
    pub bypass: bool,
    /// Switched off to free its CPU: it is not loaded while the song plays, and its state and
    /// parameters stay in the song (ENG-008). Unlike `bypass`, the plugin does not run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deactivated: bool,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    /// The plugin's own saved state, as an opaque string (base64).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Which process the plugin runs in (PLG-022).
    #[serde(default, skip_serializing_if = "PluginIsolation::is_default")]
    pub isolation: PluginIsolation,
}

impl Plugin {
    /// Whether the plugin is part of the sound while the song plays.
    pub fn runs(&self) -> bool {
        !self.bypass && !self.deactivated
    }

    /// The built-in this is (PLG-045): format `builtin` and a uid or name of the catalogue.
    pub fn builtin(&self) -> Option<&'static xirius_params::BuiltinDef> {
        if !self.format.eq_ignore_ascii_case("builtin") {
            return None;
        }
        xirius_params::find(&self.uid).or_else(|| xirius_params::find(&self.name))
    }
}

/// Which process a plugin runs in (PLG-022). The engine's own process is the default; a
/// separate process keeps a crashing plugin from taking the engine down with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginIsolation {
    /// In the engine's process.
    #[default]
    InProcess,
    /// In the separate process shared with the other plugins of its format.
    Shared,
    /// In a separate process of its own.
    Dedicated,
}

impl PluginIsolation {
    /// The names the commands take.
    pub const NAMES: [&'static str; 3] = ["inprocess", "shared", "dedicated"];

    pub fn is_default(&self) -> bool {
        *self == PluginIsolation::InProcess
    }

    pub fn name(self) -> &'static str {
        match self {
            PluginIsolation::InProcess => "inprocess",
            PluginIsolation::Shared => "shared",
            PluginIsolation::Dedicated => "dedicated",
        }
    }

    pub fn from_name(s: &str) -> Option<PluginIsolation> {
        match s {
            "inprocess" | "in_process" => Some(PluginIsolation::InProcess),
            "shared" => Some(PluginIsolation::Shared),
            "dedicated" => Some(PluginIsolation::Dedicated),
            _ => None,
        }
    }

    /// Runs outside the engine's process.
    pub fn is_separate(self) -> bool {
        self != PluginIsolation::InProcess
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct AuxSend {
    pub id: SendId,
    pub to: TrackId,
    pub level_db: f64,
    /// Before the fader. The default (`false`) is after.
    #[serde(default)]
    pub pre: bool,
    /// The fader value held while the send is silent. level_db is MIN_DB while muted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted_level: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    /// Relative to the start of the clip.
    pub start: Tick,
    pub length: Tick,
    pub pitch: u8,
    pub velocity: u8,
    #[serde(default)]
    pub muted: bool,
    /// Last changed by the AI (a human touching the note clears it).
    #[serde(default)]
    pub ai: bool,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClipBody {
    Midi {
        notes: Arc<Vec<Note>>,
        /// Semitones the clip plays above (+) or below (-) the notes as written (-24..=24,
        /// UI-311). The notes are not rewritten: a clip back at 0 plays its notes again. Older
        /// songs have none, and a plain clip writes none.
        #[serde(
            default,
            skip_serializing_if = "is_zero_i8",
            deserialize_with = "transpose_in"
        )]
        transpose: i8,
    },
    Audio {
        /// Path of the source, relative to the song folder where possible.
        file: String,
        /// Where in the source the clip starts, in ticks.
        #[serde(default)]
        offset: Tick,
        #[serde(default)]
        gain_db: f64,
        /// Tempo, speed, pitch and direction (EDT-007, EDT-016). Left out of the file when plain.
        #[serde(default, skip_serializing_if = "AudioProcess::is_plain")]
        process: AudioProcess,
    },
}

/// How an audio clip plays its source. The source file is never rewritten: the stretched,
/// shifted or reversed sound is worked out from these when the song plays.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioProcess {
    /// The tempo the source was played at, in bpm. `None` until it is set or detected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tempo: Option<f64>,
    /// Follows the song's tempo: the clip keeps its length in beats and the sound is stretched
    /// from `tempo` to the song's. Off, the clip keeps its length in seconds.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub follow: bool,
    /// Playback speed in percent (50..=200), on top of following the tempo. The pitch stays.
    #[serde(skip_serializing_if = "AudioProcess::is_full_speed")]
    pub speed: f64,
    /// Pitch shift in semitones (-24..=24) and cents (-100..=100). The length stays.
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub semis: i32,
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub cents: i32,
    /// Plays the source backwards.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reverse: bool,
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

fn is_zero_i8(v: &i8) -> bool {
    *v == 0
}

/// A clip's transpose from a song file, held inside the range (a song written by another
/// version may hold more).
fn transpose_in<'de, D>(d: D) -> Result<i8, D::Error>
where
    D: Deserializer<'de>,
{
    let v = i64::deserialize(d)?;
    Ok(clamp_semitones(v))
}

/// `v` inside the clip transpose's range.
pub fn clamp_semitones(v: i64) -> i8 {
    let max = i64::from(Clip::SEMIS);
    v.clamp(-max, max) as i8
}

/// A track's transpose from a song file, held inside the range (UI-312).
fn track_transpose_in<'de, D>(d: D) -> Result<i8, D::Error>
where
    D: Deserializer<'de>,
{
    let v = i64::deserialize(d)?;
    Ok(clamp_track_semitones(v))
}

/// A track's velocity from a song file, held inside the range (UI-312).
fn track_velocity_in<'de, D>(d: D) -> Result<i8, D::Error>
where
    D: Deserializer<'de>,
{
    let v = i64::deserialize(d)?;
    Ok(clamp_track_velocity(v))
}

/// `v` inside a track's transpose range.
pub fn clamp_track_semitones(v: i64) -> i8 {
    let max = i64::from(Track::SEMIS);
    v.clamp(-max, max) as i8
}

/// `v` inside a track's velocity range.
pub fn clamp_track_velocity(v: i64) -> i8 {
    let max = i64::from(Track::VELOCITY);
    v.clamp(-max, max) as i8
}

/// A note's velocity after the track's velocity, in the keyboard's 1..=127: `percent` of the
/// note is added to it (-100 brings it to 0, +100 doubles it), held inside the range.
pub fn track_velocity(velocity: u8, percent: i8) -> u8 {
    let scaled = f64::from(velocity) * (1.0 + f64::from(percent) / 100.0);
    (scaled.round() as i64).clamp(1, 127) as u8
}

impl Default for AudioProcess {
    fn default() -> AudioProcess {
        AudioProcess {
            tempo: None,
            follow: false,
            speed: 100.0,
            semis: 0,
            cents: 0,
            reverse: false,
        }
    }
}

impl AudioProcess {
    pub const MIN_SPEED: f64 = 50.0;
    pub const MAX_SPEED: f64 = 200.0;
    pub const SEMIS: i32 = 24;
    pub const CENTS: i32 = 100;

    pub fn is_plain(&self) -> bool {
        *self == AudioProcess::default()
    }

    fn is_full_speed(v: &f64) -> bool {
        *v == 100.0
    }

    /// Needs the stretcher (anything but reversing).
    pub fn stretches(&self) -> bool {
        self.follow || self.speed != 100.0 || self.semis != 0 || self.cents != 0
    }

    /// Output seconds per source second when the song plays at `song_bpm`.
    pub fn ratio(&self, song_bpm: f64) -> f64 {
        let follow = match (self.follow, self.tempo) {
            (true, Some(t)) if t > 0.0 && song_bpm > 0.0 => t / song_bpm,
            _ => 1.0,
        };
        follow * 100.0 / self.speed
    }

    /// Within the ranges the commands take.
    pub fn is_valid(&self) -> bool {
        (AudioProcess::MIN_SPEED..=AudioProcess::MAX_SPEED).contains(&self.speed)
            && self.semis.abs() <= AudioProcess::SEMIS
            && self.cents.abs() <= AudioProcess::CENTS
            && self.tempo.is_none_or(|t| (1.0..=999.0).contains(&t))
    }

    /// The pitch multiplier.
    pub fn transpose(&self) -> f64 {
        2f64.powf((self.semis as f64 + self.cents as f64 / 100.0) / 12.0)
    }
}

/// Which controller a lane of a MIDI clip carries.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CtrlKind {
    /// A control change, by number (1 = modulation, 11 = expression, 64 = sustain).
    Cc(u8),
    PitchBend,
    /// Channel pressure.
    Aftertouch,
}

impl CtrlKind {
    /// The text form used in commands and in the song file: `cc1`, `bend`, `at`.
    pub fn key(self) -> String {
        match self {
            CtrlKind::Cc(n) => format!("cc{n}"),
            CtrlKind::PitchBend => "bend".to_string(),
            CtrlKind::Aftertouch => "at".to_string(),
        }
    }

    pub fn parse(s: &str) -> Option<CtrlKind> {
        match s {
            "bend" => Some(CtrlKind::PitchBend),
            "at" => Some(CtrlKind::Aftertouch),
            _ => s
                .strip_prefix("cc")
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| *n < 128)
                .map(CtrlKind::Cc),
        }
    }

    /// The lowest and highest value a point of this controller may have.
    pub fn range(self) -> (i32, i32) {
        match self {
            CtrlKind::PitchBend => (-8192, 8191),
            _ => (0, 127),
        }
    }

    /// The value at rest (where a lane starts before any point).
    pub fn rest(self) -> i32 {
        match self {
            CtrlKind::PitchBend => 0,
            CtrlKind::Cc(11) => 127,
            _ => 0,
        }
    }
}

impl Serialize for CtrlKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for CtrlKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<CtrlKind, D::Error> {
        let s = String::deserialize(d)?;
        CtrlKind::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("controller `{s}`")))
    }
}

/// One value of a controller, at a time inside the clip.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CtrlPoint {
    /// Relative to the start of the clip.
    pub tick: Tick,
    pub value: i32,
    /// How the value gets from this point to the next one.
    #[serde(default, skip_serializing_if = "CtrlShape::is_step")]
    pub shape: CtrlShape,
}

/// The way a controller goes from one point to the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CtrlShape {
    /// The value holds until the next point.
    #[default]
    Step,
    /// The value moves in a straight line to the next point.
    Linear,
}

impl CtrlShape {
    fn is_step(&self) -> bool {
        *self == CtrlShape::Step
    }

    pub fn parse(s: &str) -> Option<CtrlShape> {
        match s {
            "step" => Some(CtrlShape::Step),
            "linear" => Some(CtrlShape::Linear),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            CtrlShape::Step => "step",
            CtrlShape::Linear => "linear",
        }
    }
}

impl CtrlPoint {
    /// A point that holds its value until the next one.
    pub fn new(tick: Tick, value: i32) -> CtrlPoint {
        CtrlPoint {
            tick,
            value,
            shape: CtrlShape::Step,
        }
    }

    /// The point as `cc.replace` reads it (`shape` only when it is not the plain step).
    pub fn to_json(&self) -> serde_json::Value {
        match self.shape {
            CtrlShape::Step => serde_json::json!({ "tick": self.tick, "value": self.value }),
            shape => serde_json::json!({
                "tick": self.tick, "value": self.value, "shape": shape.key(),
            }),
        }
    }
}

impl CtrlKind {
    /// Only the sustain pedal is a switch: it has no stretch between two values.
    pub fn steps_only(self) -> bool {
        self == CtrlKind::Cc(64)
    }
}

/// The points of one controller in a clip, in time order. The value holds from
/// a point until the next one.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CtrlLane {
    pub kind: CtrlKind,
    pub points: Arc<Vec<CtrlPoint>>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub start: Tick,
    pub length: Tick,
    #[serde(default)]
    pub muted: bool,
    /// The recorded take this clip is (a piece of): the clip it was recorded as. The pieces of
    /// one take cut apart keep it, which tells them from the other takes, all named after the
    /// track (a MIDI take has no file to tell it by, UI-296). None for a clip not recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<ClipId>,
    /// Controller lanes (MIDI clips only; a lane with no points is dropped).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controllers: Vec<CtrlLane>,
    #[serde(flatten)]
    pub body: ClipBody,
}

impl Clip {
    /// How far a MIDI clip's transpose may go, either way (UI-311).
    pub const SEMIS: i32 = 24;

    pub fn end(&self) -> Tick {
        self.start + self.length
    }
    pub fn notes(&self) -> &[Note] {
        match &self.body {
            ClipBody::Midi { notes, .. } => notes,
            ClipBody::Audio { .. } => &[],
        }
    }
    pub fn is_midi(&self) -> bool {
        matches!(self.body, ClipBody::Midi { .. })
    }

    /// The semitones a MIDI clip plays above (or below) its notes as written; 0 for an audio
    /// clip, whose transpose is its `AudioProcess`.
    pub fn transpose(&self) -> i8 {
        match &self.body {
            ClipBody::Midi { transpose, .. } => *transpose,
            ClipBody::Audio { .. } => 0,
        }
    }

    /// Sets a MIDI clip's transpose, held inside the range. False for an audio clip.
    pub fn set_transpose(&mut self, semis: i64) -> bool {
        match &mut self.body {
            ClipBody::Midi { transpose, .. } => {
                *transpose = clamp_semitones(semis);
                true
            }
            ClipBody::Audio { .. } => false,
        }
    }
}

/// The name the master track is stored under.
const MASTER_NAME: &str = "Master";

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub kind: TrackKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Input port / device channel name; `None` means unassigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default)]
    pub inserts: Vec<Plugin>,
    pub volume_db: f64,
    /// -1 (left) ..= 1 (right)
    pub pan: f64,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub armed: bool,
    /// While armed, the input is heard on the output (REC-002).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub monitor: bool,
    /// Trim of the input this track records and monitors, in dB (REC-011).
    #[serde(default, skip_serializing_if = "is_zero_gain")]
    pub input_gain_db: f64,
    #[serde(default)]
    pub sends: Vec<AuxSend>,
    /// `None` follows the folder's bus, or the master when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Output>,
    /// A MIDI port instead of an instrument: the track's notes go there and the track makes no
    /// sound of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_out: Option<MidiOut>,
    #[serde(default)]
    pub clips: Vec<Clip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<TrackIcon>,
    /// Automation lanes, in the order they were added (AUT-004).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<AutoLane>,
    #[serde(default, skip_serializing_if = "AutoMode::is_read")]
    pub auto_mode: AutoMode,
    /// The track preset this track was made from, saved as or last applied (MIX-030).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<Frozen>,
    /// The semitones every note on the track plays above (or below) the notes as written
    /// (-64..=64). The notes are not moved on screen: the value is added when they play (UI-312).
    #[serde(
        default,
        skip_serializing_if = "is_zero_i8",
        deserialize_with = "track_transpose_in"
    )]
    pub transpose: i8,
    /// The velocity of every note on the track, in percent (-100..=100), added before the
    /// instrument plays it (UI-312).
    #[serde(
        default,
        skip_serializing_if = "is_zero_i8",
        deserialize_with = "track_velocity_in"
    )]
    pub velocity: i8,
}

fn is_zero_gain(v: &f64) -> bool {
    *v == 0.0
}

impl Track {
    /// How far a track's transpose may go, either way (UI-312).
    pub const SEMIS: i32 = 64;
    /// How far a track's velocity may be boosted or attenuated, in percent (UI-312).
    pub const VELOCITY: i32 = 100;

    pub fn new(id: TrackId, kind: TrackKind, name: impl Into<String>) -> Track {
        Track {
            id,
            kind,
            name: name.into(),
            color: None,
            input: None,
            inserts: Vec::new(),
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            monitor: false,
            input_gain_db: 0.0,
            sends: Vec::new(),
            output: None,
            midi_out: None,
            clips: Vec::new(),
            icon: None,
            automation: Vec::new(),
            auto_mode: AutoMode::Read,
            preset: None,
            frozen: None,
            transpose: 0,
            velocity: 0,
        }
    }

    /// Sets the track's transpose, held inside the range (UI-312).
    pub fn set_transpose(&mut self, semis: i64) {
        self.transpose = clamp_track_semitones(semis);
    }

    /// Sets the track's velocity, held inside the range (UI-312).
    pub fn set_velocity(&mut self, percent: i64) {
        self.velocity = clamp_track_velocity(percent);
    }

    /// Needs a transpose or a velocity of its own on the head: either is not 0 (UI-312).
    pub fn has_marks(&self) -> bool {
        self.transpose != 0 || self.velocity != 0
    }

    pub fn has_clips(&self) -> bool {
        matches!(self.kind, TrackKind::Audio | TrackKind::Instrument)
    }

    /// The name the screens show. The master track is stored as "Master" in every song file, and
    /// shown as "マスター" like the output chooser; a name the user gave it is shown as it is.
    pub fn shown_name(&self) -> &str {
        match (self.kind, self.name.as_str()) {
            (TrackKind::Master, MASTER_NAME) => "マスター",
            _ => &self.name,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Connected bus: tracks inside without their own output play into it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus: Option<TrackId>,
    #[serde(default)]
    pub children: Vec<Node>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TempoPoint {
    pub id: TempoId,
    pub tick: Tick,
    pub bpm: f64,
    /// Glide to the next point instead of jumping.
    #[serde(default)]
    pub ramp: bool,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TimeSigPoint {
    pub id: TimeSigId,
    /// Bar number, 1-based.
    pub bar: u32,
    pub num: u32,
    pub den: u32,
}

/// A named stretch of the arrangement (verse, chorus, ...).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Section {
    pub id: SectionId,
    pub name: String,
    pub start: Tick,
    pub length: Tick,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parent {
    Root,
    Folder(FolderId),
}

/// Rates and depths a song is played and exported at (EXP-011).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub sample_rate: u32,
    /// 16, 24 or 32 (32 is floating point).
    pub bit_depth: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sample_rate: 48_000,
            bit_depth: 24,
        }
    }
}

/// Where a saved song writes the files it makes (BRW-004): recordings, bounces, freezes and
/// the copies of imported files, each in its own folder under [`Media::folder`]. Changing it
/// moves nothing: the files already made stay where the clips name them (a song made before
/// it has its `Recordings/`, `Bounces/`, `Freeze/` and `Audio/` read where they are).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Media {
    /// Relative to the song's folder (with `/`), or a full path.
    pub folder: String,
    /// Files imported from outside the song's folder are copied into `Imports`, when
    /// imported into a saved song and when the song is saved.
    pub copy_imports: bool,
}

impl Default for Media {
    fn default() -> Self {
        Media {
            folder: Media::FOLDER.into(),
            copy_imports: true,
        }
    }
}

impl Media {
    pub const FOLDER: &'static str = "Media";
    pub const RECORDINGS: &'static str = "Recordings";
    pub const BOUNCES: &'static str = "Bounces";
    pub const FREEZE: &'static str = "Freeze";
    pub const IMPORTS: &'static str = "Imports";

    pub fn is_default(&self) -> bool {
        *self == Media::default()
    }

    /// The folder on the disk, for a song saved in `song`.
    pub fn root(&self, song: &std::path::Path) -> std::path::PathBuf {
        match self.folder.trim() {
            "" => song.join(Media::FOLDER),
            f => song.join(f),
        }
    }

    /// The folder of one kind (`Media::RECORDINGS`...) on the disk.
    pub fn place(&self, song: &std::path::Path, kind: &str) -> std::path::PathBuf {
        self.root(song).join(kind)
    }
}

/// The stretch that repeats while playing and that "export the loop" takes (ARR-001).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct LoopRange {
    pub start: Tick,
    pub end: Tick,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Project {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub original_timing: BTreeMap<NoteId, crate::editing::Timing>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub packed: BTreeMap<FolderId, Vec<crate::packing::Place>>,
    pub name: String,
    pub ppq: u32,
    /// Next id to hand out. Never decreases while a song is open.
    pub next_id: u64,
    pub tracks: BTreeMap<TrackId, Arc<Track>>,
    pub folders: BTreeMap<FolderId, Folder>,
    /// The track column, top to bottom (the master is not in it).
    pub root: Vec<Node>,
    pub tempo: Vec<TempoPoint>,
    pub timesig: Vec<TimeSigPoint>,
    pub sections: Vec<Section>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<crate::marker::Marker>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default, skip_serializing_if = "Media::is_default")]
    pub media: Media,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_range: Option<LoopRange>,
    /// Fades, gain envelopes and crossfades of audio clips (see `clipfx`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub clip_fx: BTreeMap<ClipId, crate::clipfx::ClipFx>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Project {
        let mut p = Project {
            original_timing: BTreeMap::new(),
            packed: BTreeMap::new(),
            name: name.into(),
            ppq: DEFAULT_PPQ,
            next_id: 1,
            tracks: BTreeMap::new(),
            folders: BTreeMap::new(),
            root: Vec::new(),
            tempo: Vec::new(),
            timesig: Vec::new(),
            sections: Vec::new(),
            markers: Vec::new(),
            settings: Settings::default(),
            media: Media::default(),
            loop_range: None,
            clip_fx: BTreeMap::new(),
        };
        p.tracks.insert(
            TrackId::MASTER,
            Arc::new(Track::new(TrackId::MASTER, TrackKind::Master, MASTER_NAME)),
        );
        let id = TempoId(p.alloc());
        p.tempo.push(TempoPoint {
            id,
            tick: 0,
            bpm: 120.0,
            ramp: false,
        });
        let id = TimeSigId(p.alloc());
        p.timesig.push(TimeSigPoint {
            id,
            bar: 1,
            num: 4,
            den: 4,
        });
        p
    }

    /// The colour (`#rrggbb`) a track made now takes (UI-077): the identification colours in
    /// turn, starting after as many as there are tracks, and passing over one a track already
    /// has. A song of older tracks without a colour is not recoloured; only the new ones differ.
    pub fn next_track_color(&self) -> String {
        use xirius_design::{Theme, TrackColor};
        let made = self
            .tracks
            .values()
            .filter(|t| t.kind != TrackKind::Master)
            .count();
        let hex = |c: TrackColor| format!("#{:06x}", c.content(Theme::Dark).to_rgb_u32());
        let used: HashSet<String> = self
            .tracks
            .values()
            .filter_map(|t| t.color.clone())
            .map(|c| c.to_lowercase())
            .collect();
        (0..TrackColor::COUNT)
            .map(|i| hex(TrackColor::new(made + i)))
            .find(|c| !used.contains(c))
            .unwrap_or_else(|| hex(TrackColor::new(made)))
    }

    /// Hands out a fresh id number.
    pub fn alloc(&mut self) -> u64 {
        let n = self.next_id;
        self.next_id += 1;
        n
    }

    pub fn time_map(&self) -> TimeMap {
        let changes: Vec<(u32, Meter)> = self
            .timesig
            .iter()
            .map(|t| {
                (
                    t.bar,
                    Meter {
                        num: t.num,
                        den: t.den,
                    },
                )
            })
            .collect();
        TimeMap::new(self.ppq, &changes)
    }

    // ---- lookups --------------------------------------------------------

    pub fn track(&self, id: TrackId) -> CmdResult<&Track> {
        self.tracks
            .get(&id)
            .map(|t| &**t)
            .ok_or_else(|| CmdError::not_found(format!("トラック {id} が見つかりません")))
    }

    pub fn track_mut(&mut self, id: TrackId) -> CmdResult<&mut Track> {
        self.tracks
            .get_mut(&id)
            .map(Arc::make_mut)
            .ok_or_else(|| CmdError::not_found(format!("トラック {id} が見つかりません")))
    }

    pub fn folder(&self, id: FolderId) -> CmdResult<&Folder> {
        self.folders
            .get(&id)
            .ok_or_else(|| CmdError::not_found(format!("フォルダー {id} が見つかりません")))
    }

    pub fn folder_mut(&mut self, id: FolderId) -> CmdResult<&mut Folder> {
        self.folders
            .get_mut(&id)
            .ok_or_else(|| CmdError::not_found(format!("フォルダー {id} が見つかりません")))
    }

    pub fn bus(&self, id: TrackId) -> CmdResult<&Track> {
        let t = self.track(id)?;
        if t.kind == TrackKind::Bus {
            Ok(t)
        } else {
            Err(CmdError::conflict(format!(
                "{id}（{}）はバスではありません",
                t.name
            )))
        }
    }

    /// Finds a clip: its track and index in `Track::clips`.
    pub fn find_clip(&self, id: ClipId) -> Option<(TrackId, usize)> {
        self.tracks
            .iter()
            .find_map(|(tid, t)| t.clips.iter().position(|c| c.id == id).map(|i| (*tid, i)))
    }

    pub fn clip(&self, id: ClipId) -> CmdResult<(&Track, &Clip)> {
        let (tid, i) = self
            .find_clip(id)
            .ok_or_else(|| CmdError::not_found(format!("クリップ {id} が見つかりません")))?;
        let t = &*self.tracks[&tid];
        Ok((t, &t.clips[i]))
    }

    pub fn clip_mut(&mut self, id: ClipId) -> CmdResult<(TrackId, &mut Clip)> {
        let (tid, i) = self
            .find_clip(id)
            .ok_or_else(|| CmdError::not_found(format!("クリップ {id} が見つかりません")))?;
        let t = Arc::make_mut(self.tracks.get_mut(&tid).expect("found above"));
        Ok((tid, &mut t.clips[i]))
    }

    pub fn display_name(&self, node: Node) -> String {
        match node {
            Node::Track(t) => self
                .tracks
                .get(&t)
                .map_or_else(|| t.to_string(), |x| x.name.clone()),
            Node::Folder(f) => self
                .folders
                .get(&f)
                .map_or_else(|| f.to_string(), |x| x.name.clone()),
        }
    }

    // ---- the track column -----------------------------------------------

    pub fn children(&self, parent: Parent) -> &Vec<Node> {
        match parent {
            Parent::Root => &self.root,
            Parent::Folder(f) => &self.folders[&f].children,
        }
    }

    pub fn children_mut(&mut self, parent: Parent) -> &mut Vec<Node> {
        match parent {
            Parent::Root => &mut self.root,
            Parent::Folder(f) => &mut self.folders.get_mut(&f).expect("folder exists").children,
        }
    }

    /// Where each node sits: its parent and index.
    pub fn parent_map(&self) -> HashMap<Node, (Parent, usize)> {
        let mut m = HashMap::new();
        for (i, n) in self.root.iter().enumerate() {
            m.insert(*n, (Parent::Root, i));
        }
        for (fid, f) in &self.folders {
            for (i, n) in f.children.iter().enumerate() {
                m.insert(*n, (Parent::Folder(*fid), i));
            }
        }
        m
    }

    pub fn find_node(&self, node: Node) -> Option<(Parent, usize)> {
        if let Some(i) = self.root.iter().position(|n| *n == node) {
            return Some((Parent::Root, i));
        }
        self.folders.iter().find_map(|(fid, f)| {
            f.children
                .iter()
                .position(|n| *n == node)
                .map(|i| (Parent::Folder(*fid), i))
        })
    }

    pub fn detach(&mut self, node: Node) -> Option<(Parent, usize)> {
        let (p, i) = self.find_node(node)?;
        self.children_mut(p).remove(i);
        Some((p, i))
    }

    /// Is `inner` the same folder as `outer` or nested anywhere inside it?
    pub fn folder_contains(&self, outer: FolderId, inner: FolderId) -> bool {
        if outer == inner {
            return true;
        }
        self.folders.get(&outer).is_some_and(|f| {
            f.children
                .iter()
                .any(|n| matches!(n, Node::Folder(c) if self.folder_contains(*c, inner)))
        })
    }

    /// Every track inside the folder at any depth, in column order.
    pub fn tracks_in(&self, folder: FolderId) -> Vec<TrackId> {
        let mut out = Vec::new();
        if let Some(f) = self.folders.get(&folder) {
            for n in &f.children {
                match n {
                    Node::Track(t) => out.push(*t),
                    Node::Folder(c) => out.extend(self.tracks_in(*c)),
                }
            }
        }
        out
    }

    /// The ancestors of a node, nearest first.
    pub fn ancestors(&self, node: Node) -> Vec<FolderId> {
        let pm = self.parent_map();
        let mut out = Vec::new();
        let mut cur = node;
        while let Some((Parent::Folder(f), _)) = pm.get(&cur) {
            out.push(*f);
            cur = Node::Folder(*f);
        }
        out
    }

    // ---- routing ---------------------------------------------------------

    /// Where a channel really plays to, following the folder's bus when the
    /// channel has no output of its own. The master has no output.
    pub fn effective_output(&self, id: TrackId) -> Option<Output> {
        if id.is_master() {
            return None;
        }
        let t = self.tracks.get(&id)?;
        if let Some(o) = t.output {
            return Some(o);
        }
        for f in self.ancestors(Node::Track(id)) {
            if let Some(b) = self.folders[&f].bus {
                return Some(Output::Bus(b));
            }
        }
        Some(Output::Master)
    }

    /// Fails with the loop spelled out when outputs, folder buses and sends
    /// make a cycle.
    pub fn check_routing(&self) -> CmdResult<()> {
        let pm = self.parent_map();
        let eff = |id: TrackId, t: &Track| -> Option<TrackId> {
            if let Some(o) = t.output {
                return match o {
                    Output::Bus(b) => Some(b),
                    Output::Master => None,
                };
            }
            let mut cur = Node::Track(id);
            while let Some((Parent::Folder(f), _)) = pm.get(&cur) {
                if let Some(b) = self.folders[f].bus {
                    return Some(b);
                }
                cur = Node::Folder(*f);
            }
            None
        };
        let mut edges: HashMap<TrackId, Vec<TrackId>> = HashMap::new();
        for (id, t) in &self.tracks {
            if id.is_master() {
                continue;
            }
            let e = edges.entry(*id).or_default();
            if let Some(b) = eff(*id, t) {
                e.push(b);
            }
            for s in &t.sends {
                e.push(s.to);
            }
        }
        // iterative DFS with colours; 1 = on the stack, 2 = done
        let mut colour: HashMap<TrackId, u8> = HashMap::new();
        for start in edges.keys().copied().collect::<Vec<_>>() {
            if colour.contains_key(&start) {
                continue;
            }
            let mut stack: Vec<(TrackId, usize)> = vec![(start, 0)];
            colour.insert(start, 1);
            while let Some(&mut (n, ref mut i)) = stack.last_mut() {
                let next = edges.get(&n).and_then(|v| v.get(*i)).copied();
                *i += 1;
                match next {
                    None => {
                        colour.insert(n, 2);
                        stack.pop();
                    }
                    Some(m) => match colour.get(&m) {
                        Some(1) => {
                            let mut path: Vec<String> = stack
                                .iter()
                                .skip_while(|(x, _)| *x != m)
                                .map(|(x, _)| self.display_name(Node::Track(*x)))
                                .collect();
                            path.push(self.display_name(Node::Track(m)));
                            return Err(CmdError::conflict(format!(
                                "出力がループしています: {}",
                                path.join(" → ")
                            )));
                        }
                        Some(_) => {}
                        None => {
                            colour.insert(m, 1);
                            stack.push((m, 0));
                        }
                    },
                }
            }
        }
        Ok(())
    }

    // ---- structure edits shared by commands -----------------------------

    /// Removes a track and every reference to it. The tracks that used it as
    /// their output fall back to the master.
    pub fn delete_track(&mut self, id: TrackId) -> CmdResult<()> {
        if id.is_master() {
            return Err(CmdError::conflict("マスターは削除できません"));
        }
        self.track(id)?;
        self.detach(Node::Track(id));
        self.tracks.remove(&id);
        for t in self.tracks.values_mut() {
            let refs = t.output == Some(Output::Bus(id)) || t.sends.iter().any(|s| s.to == id);
            if refs {
                let t = Arc::make_mut(t);
                if t.output == Some(Output::Bus(id)) {
                    t.output = Some(Output::Master);
                }
                t.sends.retain(|s| s.to != id);
                t.automation.retain(|l|!matches!(l.target,crate::auto::AutoTarget::Send(id) if !t.sends.iter().any(|s|s.id==id)));
            }
        }
        for f in self.folders.values_mut() {
            if f.bus == Some(id) {
                f.bus = None;
            }
        }
        Ok(())
    }

    // ---- invariants --------------------------------------------------------

    /// The rules every command must leave intact. Cheap enough to run after
    /// every step (it does not look for duplicate note ids; see
    /// `validate_deep`).
    pub fn validate(&self) -> CmdResult<()> {
        if self
            .markers
            .iter()
            .any(|m| m.tick < 0 || m.name.trim().is_empty())
            || !self.markers.windows(2).all(|w| w[0].tick < w[1].tick)
        {
            return Err(CmdError::range("マーカーの名前または位置が不正です"));
        }
        let master = self
            .tracks
            .get(&TrackId::MASTER)
            .ok_or_else(|| CmdError::conflict("マスターがありません"))?;
        if master.kind != TrackKind::Master || !master.sends.is_empty() || master.output.is_some() {
            return Err(CmdError::conflict(
                "マスターには出力もセンドも設定できません",
            ));
        }
        if !master.clips.is_empty() {
            return Err(CmdError::conflict("マスターにはクリップを配置できません"));
        }

        if let Some(l) = self.loop_range
            && (l.start < 0 || l.end <= l.start)
        {
            return Err(CmdError::range(
                "ループ範囲は 0 以上で、終了位置を開始位置より後にしてください",
            ));
        }

        // the track column holds every track (except the master) and folder once
        let mut seen: HashSet<Node> = HashSet::new();
        let mut stack: Vec<(Parent, &Vec<Node>)> = vec![(Parent::Root, &self.root)];
        let mut visited_folders: HashSet<FolderId> = HashSet::new();
        while let Some((_, kids)) = stack.pop() {
            for n in kids {
                if !seen.insert(*n) {
                    return Err(CmdError::conflict(format!(
                        "{n} がトラックの順序に 2 回含まれています"
                    )));
                }
                match n {
                    Node::Track(t) => match self.tracks.get(t) {
                        None => {
                            return Err(CmdError::conflict(format!(
                                "トラック {t} がトラックの順序に含まれていません"
                            )));
                        }
                        Some(x) if x.kind == TrackKind::Master => {
                            return Err(CmdError::conflict(
                                "マスターはトラックの順序に含められません",
                            ));
                        }
                        _ => {}
                    },
                    Node::Folder(f) => {
                        let folder = self.folders.get(f).ok_or_else(|| {
                            CmdError::conflict(format!(
                                "フォルダー {f} がトラックの順序に含まれていません"
                            ))
                        })?;
                        if !visited_folders.insert(*f) {
                            return Err(CmdError::conflict("フォルダーの入れ子がループしています"));
                        }
                        stack.push((Parent::Folder(*f), &folder.children));
                    }
                }
            }
        }
        for id in self.tracks.keys().filter(|t| !t.is_master()) {
            if !seen.contains(&Node::Track(*id)) {
                return Err(CmdError::conflict(format!(
                    "{id} がトラックの順序に含まれていません"
                )));
            }
        }
        for id in self.folders.keys() {
            if !seen.contains(&Node::Folder(*id)) {
                return Err(CmdError::conflict(format!(
                    "{id} がトラックの順序に含まれていません"
                )));
            }
        }

        for (id, t) in &self.tracks {
            if t.id != *id {
                return Err(CmdError::conflict(format!("{id} の中の ID が一致しません")));
            }
            if !(MIN_DB..=MAX_DB).contains(&t.volume_db) || !(-1.0..=1.0).contains(&t.pan) {
                return Err(CmdError::range(format!(
                    "{} のボリュームまたはパンが範囲外です",
                    t.name
                )));
            }
            if let Some(Output::Bus(b)) = t.output {
                self.bus(b).map_err(|_| {
                    CmdError::conflict(format!("{} の出力先 {b} はバスではありません", t.name))
                })?;
            }
            for s in &t.sends {
                self.bus(s.to).map_err(|_| {
                    CmdError::conflict(format!(
                        "{} のセンド先 {} はバスではありません",
                        t.name, s.to
                    ))
                })?;
            }
            if !t.has_clips() && !t.clips.is_empty() {
                return Err(CmdError::conflict(format!(
                    "{} にはクリップを配置できません",
                    t.name
                )));
            }
            let instruments = t
                .inserts
                .iter()
                .filter(|p| p.kind == PluginKind::Instrument)
                .count();
            match t.kind {
                TrackKind::Instrument => {
                    let first_ok = t
                        .inserts
                        .first()
                        .is_some_and(|p| p.kind == PluginKind::Instrument);
                    if instruments > 1 || (instruments == 1 && !first_ok) {
                        return Err(CmdError::conflict(format!(
                            "{}: インストゥルメントは先頭に 1 つだけ挿入できます",
                            t.name
                        )));
                    }
                }
                _ if instruments > 0 => {
                    return Err(CmdError::conflict(format!(
                        "{} にはインストゥルメントを挿入できません",
                        t.name
                    )));
                }
                _ => {}
            }
            let mut targets = HashSet::new();
            for l in &t.automation {
                if !targets.insert(&l.target) {
                    return Err(CmdError::conflict(format!(
                        "{} のオートメーション {} が重複しています",
                        t.name, l.target
                    )));
                }
                let (lo, hi) = l.target.range();
                let ok = l.points.windows(2).all(|w| w[0].tick < w[1].tick)
                    && l.points.iter().all(|p| {
                        p.tick >= 0
                            && (lo..=hi).contains(&p.value)
                            && (-1.0..=1.0).contains(&p.curve)
                    });
                if !ok {
                    return Err(CmdError::range(format!(
                        "{} のオートメーション {} のポイントが範囲外か、時間順になっていません",
                        t.name, l.target
                    )));
                }
            }
            for c in &t.clips {
                if c.length < 1 || c.start < 0 {
                    return Err(CmdError::range(format!(
                        "クリップ {} の位置または長さが範囲外です",
                        c.id
                    )));
                }
                match (&c.body, t.kind) {
                    (ClipBody::Midi { .. }, TrackKind::Instrument)
                    | (ClipBody::Audio { .. }, TrackKind::Audio) => {}
                    _ => {
                        return Err(CmdError::conflict(format!(
                            "クリップ {} の種類が {} と合いません",
                            c.id, t.name
                        )));
                    }
                }
                if let ClipBody::Audio { process: a, .. } = &c.body
                    && !a.is_valid()
                {
                    return Err(CmdError::range(format!(
                        "クリップ {} のテンポ・速さ・トランスポーズが範囲外です",
                        c.id
                    )));
                }
                for n in c.notes() {
                    if n.pitch > 127
                        || n.velocity == 0
                        || n.velocity > 127
                        || n.length < 1
                        || n.start < 0
                    {
                        return Err(CmdError::range(format!("ノート {} の値が範囲外です", n.id)));
                    }
                }
            }
        }
        for f in self.folders.values() {
            if let Some(b) = f.bus {
                self.bus(b).map_err(|_| {
                    CmdError::conflict(format!(
                        "フォルダー {} の出力先 {b} はバスではありません",
                        f.name
                    ))
                })?;
            }
        }
        self.check_routing()?;

        let first = self.tempo.first();
        if !first.is_some_and(|t| t.tick == 0)
            || !self.tempo.windows(2).all(|w| w[0].tick < w[1].tick)
        {
            return Err(CmdError::conflict(
                "テンポポイントは位置 0 から始まり、時間順に並んでいる必要があります",
            ));
        }
        if self.tempo.iter().any(|t| !(1.0..=999.0).contains(&t.bpm)) {
            return Err(CmdError::range("テンポは 1〜999 の範囲で指定してください"));
        }
        let first = self.timesig.first();
        if !first.is_some_and(|t| t.bar == 1)
            || !self.timesig.windows(2).all(|w| w[0].bar < w[1].bar)
        {
            return Err(CmdError::conflict(
                "拍子は 1 小節目から始まり、小節順に並んでいる必要があります",
            ));
        }
        if self
            .timesig
            .iter()
            .any(|t| t.num < 1 || t.num > 64 || !matches!(t.den, 1 | 2 | 4 | 8 | 16 | 32))
        {
            return Err(CmdError::range("拍子が範囲外です"));
        }
        if self.sections.iter().any(|s| s.length < 1 || s.start < 0) {
            return Err(CmdError::range("セクションの位置または長さが範囲外です"));
        }
        Ok(())
    }

    /// `validate` plus the checks that scan every id.
    pub fn validate_deep(&self) -> CmdResult<()> {
        self.validate()?;
        let mut seen: HashSet<u64> = HashSet::new();
        let mut take = |n: u64, what: &str| -> CmdResult<()> {
            if n >= self.next_id {
                return Err(CmdError::conflict(format!(
                    "{what} の ID が次に割り当てる ID 以上になっています"
                )));
            }
            if !seen.insert(n) {
                return Err(CmdError::conflict(format!("{what} の ID が重複しています")));
            }
            Ok(())
        };
        for (id, t) in &self.tracks {
            if !id.is_master() {
                take(id.0, "トラック")?;
            }
            for p in &t.inserts {
                take(p.id.0, "プラグイン")?;
            }
            for s in &t.sends {
                take(s.id.0, "センド")?;
            }
            for c in &t.clips {
                take(c.id.0, "クリップ")?;
                for n in c.notes() {
                    take(n.id.0, "ノート")?;
                }
            }
        }
        for f in self.folders.keys() {
            take(f.0, "フォルダー")?;
        }
        for t in &self.tempo {
            take(t.id.0, "テンポ")?;
        }
        for t in &self.timesig {
            take(t.id.0, "拍子")?;
        }
        for s in &self.sections {
            take(s.id.0, "セクション")?;
        }
        for m in &self.markers {
            take(m.id.0, "マーカー")?;
        }
        Ok(())
    }

    /// A stable fingerprint of the content (FNV-1a over the JSON form).
    pub fn content_hash(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{h:016x}")
    }
}
