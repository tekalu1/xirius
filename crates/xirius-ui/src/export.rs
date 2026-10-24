//! The export panel (EXP-009..018, EXP-024): a mixdown, stems or MIDI files (UI-280), the
//! range, the format, which strips and the file names, and the files written so far with what
//! can be done with them.
//!
//! Every export runs in the background (`background: true`), so the screen
//! never waits for one: [`Studio::poll_export`] asks the session how the
//! running ones are getting on, a few times a second, while the screen ticks.
//! The panel is a pop-over ([`crate::popup::PopupKind::Export`]): its
//! settings live here, in [`Export`], and [`View`] is what one picture of it
//! shows, so laying it out and painting it need neither the song nor a window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_core::id::{FolderId, SectionId, TrackId};
use xirius_core::model::{Output, Project};
use xirius_core::time::Tick;
use xirius_design::{FontSize, Layout, Radius, Space, TrackColor, Weight};
use xirius_session::export::{Format, StemSet, names_track};
use xirius_session::export_stale::{self, Staleness};
use xirius_session::{midi_export, mp3};

use crate::browser::text_width;
use crate::draw::{Align, Pen, track_color, track_icon};
use crate::popup::{Action, Cell, Look, PopupKind};
use crate::song::palette_color;
use crate::studio::{Geometry, Rect, Studio};

/// The panel's width.
pub const W: f32 = 576.0;
/// The most finished or running exports listed (the newest ones).
pub const CARDS: usize = 4;
/// The most files of a stems export listed under it whole; more fold to [`FOLDED`].
pub const KIDS: usize = 8;
/// The files listed of a stems export with more than [`KIDS`], until asked for the rest.
pub const FOLDED: usize = 4;
const PAD: f32 = 16.0;
const LABEL_W: f32 = 64.0;
/// Where the rows' contents start, and how wide they are.
const X: f32 = PAD + LABEL_W + Space::X3.px();
const CW: f32 = W - X - PAD;
const ROW_H: f32 = 28.0;
const GAP: f32 = 12.0;
const STRIP_H: f32 = 30.0;
const CARD_H: f32 = 52.0;
const KID_H: f32 = 22.0;
/// The row of a card's buttons.
const FOOT_H: f32 = 36.0;
/// Where a card's files start, from its left.
const FILES_X: f32 = 48.0;
/// How often the running exports are asked how far they are.
const POLL: Duration = Duration::from_millis(100);

const SONG_TAG: &str = "{曲}";
const TRACK_TAG: &str = "{トラック}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Song,
    Loop,
    Sections,
    Selection,
}

/// Which strips the stems are of (EXP-013).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Every track.
    Tracks,
    /// Each bus, and the tracks that go straight to the master.
    Buses,
    /// The ones ticked.
    Selected,
}

/// What [`Export::staleness`] worked out last, and for what.
#[derive(Default)]
struct StaleCache {
    song: Option<Arc<Project>>,
    /// Each card's job and state.
    jobs: Vec<(String, String)>,
    out: HashMap<String, Option<Staleness>>,
}

/// The panel's settings and the exports it knows of.
pub struct Export {
    pub preview: Option<String>,
    pub preview_peak: f64,
    pub stems: bool,
    /// MIDI files (UI-280), whatever `stems` is.
    pub midi: bool,
    /// One MIDI file for each track, not one for them all.
    pub each: bool,
    /// The instrument tracks ticked for the MIDI files; None for every one.
    pub midi_picked: Option<Vec<TrackId>>,
    pub midi_name: String,
    pub each_name: String,
    pub range: Range,
    pub sections: Vec<SectionId>,
    pub fixed_span: Option<(Tick, Tick)>,
    pub(crate) fixed_song: Option<PathBuf>,
    pub format: Format,
    pub rate: u32,
    /// Ignored for MP3, which has a bitrate instead.
    pub bits: u16,
    /// MP3's bitrate in kbps (EXP-005).
    pub kbps: u16,
    /// Why MP3 cannot be written (its encoder cannot be loaded), as `export.open` says.
    pub mp3_error: Option<String>,
    pub group: Group,
    /// The ticked strips, when `group` is [`Group::Selected`].
    pub picked: Vec<TrackId>,
    /// Folders whose bus in `picked` is written as the folder's own stem (CTX-018): named after
    /// the folder, holding what plays into it.
    pub folders: Vec<FolderId>,
    pub mix_name: String,
    pub stems_name: String,
    /// The song folder the settings were read for (`export.open`).
    loaded: Option<Option<PathBuf>>,
    /// `export.status`, newest first.
    pub jobs: Vec<Value>,
    /// How the files of each card stand against the song (EXP-019), worked out again only
    /// when the song or the list changes.
    stale: Mutex<StaleCache>,
    polled: Option<Instant>,
    /// The exports whose every file is listed, though they have more than [`KIDS`].
    pub expanded: Vec<String>,
}

impl Default for Export {
    fn default() -> Export {
        Export {
            preview: None,
            preview_peak: 0.0,
            stems: false,
            midi: false,
            each: false,
            midi_picked: None,
            midi_name: midi_export::ONE_NAME.to_string(),
            each_name: midi_export::EACH_NAME.to_string(),
            range: Range::Song,
            sections: Vec::new(),
            fixed_span: None,
            fixed_song: None,
            format: Format::Wav,
            rate: 48_000,
            bits: 24,
            kbps: mp3::DEFAULT_BITRATE,
            mp3_error: None,
            group: Group::Tracks,
            picked: Vec::new(),
            folders: Vec::new(),
            mix_name: SONG_TAG.to_string(),
            stems_name: format!("{SONG_TAG}_{TRACK_TAG}"),
            loaded: None,
            jobs: Vec::new(),
            stale: Mutex::default(),
            polled: None,
            expanded: Vec::new(),
        }
    }
}

impl Export {
    /// The song was replaced: what was worked out for the cards of the old one is of no use to
    /// the new one.
    pub(crate) fn forget_bases(&mut self) {
        *self.stale.lock().unwrap() = StaleCache::default();
    }

    /// How the files of each export listed stand against `p`, by job; `None` for one that has
    /// not finished or was made before the record kept what the song was.
    fn staleness(&self, p: &Arc<Project>, shown: &[&Value]) -> HashMap<String, Option<Staleness>> {
        let jobs: Vec<(String, String)> = shown
            .iter()
            .map(|j| {
                (
                    j["job"].as_str().unwrap_or_default().to_string(),
                    j["state"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let mut cache = self.stale.lock().unwrap();
        if cache.song.as_ref().is_some_and(|s| Arc::ptr_eq(s, p)) && cache.jobs == jobs {
            return cache.out.clone();
        }
        let prints = export_stale::prints(p);
        let out: HashMap<String, Option<Staleness>> = shown
            .iter()
            .map(|j| {
                (
                    j["job"].as_str().unwrap_or_default().to_string(),
                    export_stale::check(p, &prints, j),
                )
            })
            .collect();
        *cache = StaleCache {
            song: Some(p.clone()),
            jobs,
            out: out.clone(),
        };
        out
    }

    /// The file name of the mode shown.
    pub fn name(&self) -> &str {
        if self.midi {
            if self.each {
                &self.each_name
            } else {
                &self.midi_name
            }
        } else if self.stems {
            &self.stems_name
        } else {
            &self.mix_name
        }
    }

    /// Keeps `name` as the file name of the mode shown.
    pub fn set_name(&mut self, name: String) {
        if self.midi {
            if self.each {
                self.each_name = name;
            } else {
                self.midi_name = name;
            }
        } else if self.stems {
            self.stems_name = name;
        } else {
            self.mix_name = name;
        }
    }

    /// Shows the mixdown (false) or the stems.
    pub fn set_stems(&mut self, stems: bool) {
        self.stems = stems;
        self.midi = false;
    }

    /// The instrument tracks the MIDI files would be of, top to bottom.
    pub fn midi_picks(&self, p: &Project) -> Vec<TrackId> {
        let all = midi_export::instrument_tracks(p);
        match &self.midi_picked {
            Some(picked) => all.into_iter().filter(|t| picked.contains(t)).collect(),
            None => all,
        }
    }

    /// The strips the stems would be of.
    pub fn picks(&self, p: &Project) -> Vec<TrackId> {
        match self.group {
            Group::Tracks => StemSet::Tracks.strips(p),
            Group::Buses => StemSet::ByBus.strips(p),
            Group::Selected => StemSet::All
                .strips(p)
                .into_iter()
                .filter(|id| self.picked.contains(id))
                .collect(),
        }
    }

    /// The folders whose stem is written, and the strips left for `ids`: a folder counts while
    /// its bus is ticked.
    pub fn folder_stems(&self, p: &Project) -> (Vec<FolderId>, Vec<TrackId>) {
        let picks = self.picks(p);
        let folders: Vec<FolderId> = self
            .folders
            .iter()
            .copied()
            .filter(|f| {
                p.folders
                    .get(f)
                    .and_then(|f| f.bus)
                    .is_some_and(|b| picks.contains(&b))
            })
            .collect();
        let buses: Vec<TrackId> = folders.iter().filter_map(|f| p.folders[f].bus).collect();
        let rest = picks.into_iter().filter(|t| !buses.contains(t)).collect();
        (folders, rest)
    }

    /// Is a job running?
    pub fn running(&self) -> bool {
        self.jobs.iter().any(|j| j["state"] == "running")
    }
}

/// What a press in the panel does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Preview(String),
    Import(String),
    /// Stems (true) or a mixdown.
    Mode(bool),
    /// MIDI files.
    Midi,
    /// One MIDI file for each track (true), or one for them all.
    Each(bool),
    Range(Range),
    Section(SectionId),
    Format(Format),
    Rate(u32),
    Bits(u16),
    /// MP3's bitrate.
    Kbps(u16),
    Group(Group),
    Pick(TrackId),
    EditName,
    Go,
    /// `export.update`: writes the job's files again from the song as it is now.
    Update(String),
    /// `export.reveal_os`
    Reveal(String),
    /// `export.delete`: the files to the trash.
    Delete(String),
    DeleteConfirmed(String),
    Cancel(String),
    Close,
    /// Lists every file of the export.
    More(String),
}

/// The bus a folder is connected to, if there is one.
pub fn folder_bus(p: &Project, f: FolderId) -> Option<TrackId> {
    p.folders.get(&f)?.bus.filter(|b| p.tracks.contains_key(b))
}

/// A section on the range strip.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionBar {
    pub id: SectionId,
    pub name: String,
    pub start: Tick,
    pub end: Tick,
    pub color: usize,
    pub on: bool,
}

/// A track or bus that can be ticked for the stems.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub id: TrackId,
    pub name: String,
    pub icon: &'static str,
    pub color: usize,
    pub on: bool,
    /// Not ticked, but in the stem of a bus that is.
    pub inside: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Done,
    Failed,
}

/// A track or bus that changed since an export, as a dot of its color.
#[derive(Clone, Debug, PartialEq)]
pub struct Dot {
    pub color: Option<usize>,
    pub name: String,
}

/// A file of a stems export, under its card.
#[derive(Clone, Debug, PartialEq)]
pub struct Kid {
    /// The track or bus it is the stem of, by name.
    pub name: String,
    /// The file's name, once written.
    pub file: String,
    pub path: String,
    /// The track or bus, by id.
    pub strip: Option<String>,
    pub color: Option<usize>,
    /// How far it is, while the export runs.
    pub fraction: Option<f32>,
    /// Integrated loudness.
    pub lufs: Option<f64>,
    /// Has what plays into it changed since? None when it is not known.
    pub stale: Option<bool>,
    /// The other tracks and buses that changed and play into it.
    pub dots: Vec<Dot>,
    /// Kept as it is while the others of its export are written again.
    pub wait: bool,
    /// How many tracks a MIDI file holds, said where a stem says its loudness.
    pub tracks: Option<usize>,
}

impl Kid {
    /// The name its row shows.
    pub fn shown(&self) -> &str {
        if self.file.is_empty() {
            &self.name
        } else {
            &self.file
        }
    }
}

/// An export, as its card shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub playing: bool,
    pub muted: Vec<String>,
    pub job: String,
    pub stems: bool,
    /// MIDI files, which cannot be listened to.
    pub midi: bool,
    pub title: String,
    /// Format, rate, depth and length (or how far it is).
    pub meta: String,
    pub state: State,
    pub fraction: f32,
    /// Seconds of sound written per second.
    pub speed: Option<f64>,
    /// Integrated loudness and true peak of a mixdown, one to a line.
    pub loudness: String,
    pub error: String,
    /// Has the song changed since? None when it is not known (an export of
    /// an earlier run of the program).
    pub stale: Option<bool>,
    pub newest: bool,
    pub kids: Vec<Kid>,
    /// Files not listed.
    pub more: usize,
    /// The tracks and buses that changed since and play into its files.
    pub dots: Vec<Dot>,
    /// How many of its files are stale.
    pub old: usize,
    /// The width the buttons take at the right, so the text stops short of them.
    pub right: f32,
    /// What an MP3 was titled, said in a balloon over the name (EXP-005).
    pub tags: String,
}

/// One picture of the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// Stems, and not MIDI files.
    pub stems: bool,
    /// MIDI files; `tracks` are then the instrument tracks.
    pub midi: bool,
    pub each: bool,
    pub range: Range,
    pub format: Format,
    pub rate: u32,
    pub bits: u16,
    pub kbps: u16,
    /// What the song itself is set to (EXP-012): the rate and depth the panel starts from.
    pub song_rate: u32,
    pub song_bits: u16,
    /// Why MP3 cannot be written, when it cannot.
    pub mp3_error: Option<String>,
    pub group: Group,
    pub has_dir: bool,
    pub song: String,
    pub name: String,
    pub sections: Vec<SectionBar>,
    /// Where the strip ends: the end of the last clip, section or loop.
    pub end: Tick,
    pub loop_span: Option<(Tick, Tick)>,
    pub selected_span: Option<(Tick, Tick)>,
    pub tracks: Vec<Strip>,
    pub buses: Vec<Strip>,
    /// How many files the button would write.
    pub count: usize,
    pub cards: Vec<Card>,
}

impl View {
    /// The span the export covers, on the strip.
    pub fn span(&self) -> (Tick, Tick) {
        match self.range {
            Range::Loop => self.loop_span.unwrap_or((0, self.end)),
            Range::Sections => {
                let on = self.sections.iter().filter(|s| s.on);
                let a = on.clone().map(|s| s.start).min();
                let b = on.map(|s| s.end).max();
                match (a, b) {
                    (Some(a), Some(b)) => (a, b),
                    _ => (0, self.end),
                }
            }
            Range::Selection => self.selected_span.unwrap_or((0, 0)),
            Range::Song => (0, self.end),
        }
    }

    /// Does the stems name tell the files apart?
    pub fn bad_name(&self) -> bool {
        self.name.trim().is_empty()
            || ((self.stems || (self.midi && self.each)) && !names_track(&self.name))
    }

    /// MP3 is picked and its encoder cannot be loaded.
    pub fn no_mp3(&self) -> bool {
        !self.midi && self.format == Format::Mp3 && self.mp3_error.is_some()
    }

    /// The instrument tracks ticked for the MIDI files.
    pub fn midi_count(&self) -> usize {
        self.tracks.iter().filter(|s| s.on).count()
    }

    pub fn can_go(&self) -> bool {
        self.has_dir
            && !self.bad_name()
            && (!self.stems || self.count > 0)
            && (!self.midi || self.midi_count() > 0)
            && !self.no_mp3()
            && (self.range != Range::Selection || self.selected_span.is_some())
    }
}

/// How each part of the panel is painted.
#[derive(Clone, Debug, PartialEq)]
pub enum XpLook {
    /// The well a row of [`XpLook::Seg`] sits in.
    Tray {
        big: bool,
    },
    Seg {
        big: bool,
        /// The setting on differs from the song's own (EXP-012): its text is blue.
        moved: bool,
    },
    /// `kHz`, `bit` and `kbps` after their buttons.
    Unit,
    /// The range strip.
    Strip,
    Section {
        color: usize,
    },
    /// The part of the strip left out.
    Dim,
    /// The ring around the part written.
    Window,
    /// Where the loop is, under the strip.
    LoopMark,
    Check {
        color: usize,
        inside: bool,
    },
    /// The folder; `text` is the song, `hint` the folder in it.
    Path,
    Field {
        ext: &'static str,
        bad: bool,
        editing: bool,
    },
    /// An icon button without a face.
    Bare,
    Go {
        badge: Option<usize>,
    },
    Card(Box<Card>),
    /// A file of a stems export: its name, its loudness or how far it is.
    File(Box<Kid>),
    /// The files of a card left out.
    More,
    /// Stale or as the song is now, in a circle.
    Mark {
        stale: bool,
    },
    /// A track or bus that changed.
    Dot {
        color: Option<usize>,
    },
    /// Why the button cannot be pressed, in red.
    Alert,
    Space,
}

// ------------------------------------------------------------------- the view

fn mmss(s: f64) -> String {
    let s = s.round().max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn rate_text(rate: u64) -> String {
    if rate.is_multiple_of(1000) {
        format!("{} kHz", rate / 1000)
    } else {
        format!("{:.1} kHz", rate as f64 / 1000.0)
    }
}

/// How far a file of a running export is.
fn part_done(f: &Value) -> f32 {
    let total = f["total"].as_f64().unwrap_or(0.0);
    if total > 0.0 {
        (f["done"].as_f64().unwrap_or(0.0) / total) as f32
    } else {
        0.0
    }
}

/// The exports a record writes again: one id, or a list of them.
fn replaced(j: &Value) -> Vec<&str> {
    match &j["replaces"] {
        Value::Array(ids) => ids.iter().filter_map(Value::as_str).collect(),
        v => v.as_str().into_iter().collect(),
    }
}

/// The card of a finished export shows `run`, which writes its files again: the files written
/// again fill up, the others wait.
fn fold(card: &mut Card, run: &Value) {
    card.state = State::Running;
    card.job = run["job"].as_str().unwrap_or_default().to_string();
    card.fraction = run["fraction"].as_f64().unwrap_or(0.0) as f32;
    card.speed = run["speed"].as_f64();
    card.stale = None;
    card.dots.clear();
    card.playing = false;
    let files = run["files"].as_array().cloned().unwrap_or_default();
    for kid in &mut card.kids {
        match files.iter().find(|f| f["name"] == kid.name.as_str()) {
            Some(f) => kid.fraction = Some(part_done(f)),
            None => kid.wait = true,
        }
    }
}

fn card_of(j: &Value, p: &Project, stale: Option<&Staleness>, newest: bool) -> Card {
    let state = match j["state"].as_str() {
        Some("running") => State::Running,
        Some("done") => State::Done,
        _ => State::Failed,
    };
    if j["command"] == "export.midi" {
        return midi_card(j, p, stale, newest, state);
    }
    let stems = j["command"] == "export.stems";
    let args = &j["args"];
    let result = &j["result"];
    let fraction = j["fraction"].as_f64().unwrap_or(0.0) as f32;
    let mut meta = vec![args["format"].as_str().unwrap_or("wav").to_uppercase()];
    if let Some(r) = args["sample_rate"]
        .as_u64()
        .or_else(|| result["sample_rate"].as_u64())
    {
        meta.push(rate_text(r));
    }
    let mp3 = args["format"] == "mp3";
    if mp3 {
        if let Some(k) = args["bitrate"]
            .as_u64()
            .or_else(|| result["bitrate_kbps"].as_u64())
        {
            meta.push(format!("{k} kbps"));
        }
    } else if let Some(b) = args["bit_depth"]
        .as_u64()
        .or_else(|| result["bit_depth"].as_u64())
    {
        meta.push(if b == 32 {
            "32 bit float".to_string()
        } else {
            format!("{b} bit")
        });
    }
    let files = if stems && state == State::Done {
        result["files"].as_array().cloned().unwrap_or_default()
    } else {
        j["files"].as_array().cloned().unwrap_or_default()
    };
    match state {
        // How far it is shows under the files.
        State::Running => {}
        State::Done => {
            let seconds = if stems {
                files.first().and_then(|f| f["seconds"].as_f64())
            } else {
                result["seconds"].as_f64()
            };
            if let Some(s) = seconds {
                meta.push(mmss(s));
            }
        }
        State::Failed => {}
    }
    let title = if stems {
        format!("ステム {} 本", files.len())
    } else {
        let path = result["path"].as_str().or_else(|| j["path"].as_str());
        path.and_then(|s| Path::new(s).file_name()).map_or_else(
            || "ミックスダウン".to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    let loudness = if !stems && state == State::Done {
        let peak = result["true_peak_db"]
            .as_f64()
            .map(|d| format!("{d:.1} dBTP"));
        let lufs = result["lufs"].as_f64().map(|l| format!("{l:.1} LUFS"));
        [lufs, peak]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };
    let color_of = |name: &str| {
        p.tracks
            .values()
            .find(|t| t.name == name)
            .map(|t| track_color(t))
    };
    let by_id = |id: &str| {
        id.parse::<TrackId>()
            .ok()
            .and_then(|t| p.tracks.get(&t))
            .map(|t| track_color(t))
    };
    let stale = stale.filter(|_| state == State::Done);
    let dot = |id: &String| Dot {
        color: by_id(id),
        name: stale
            .and_then(|s| s.changed.iter().find(|c| c.id == *id))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| id.clone()),
    };
    let kids: Vec<Kid> = if stems {
        files
            .iter()
            .map(|f| {
                let name = f["name"].as_str().unwrap_or_default().to_string();
                let path = f["path"].as_str().unwrap_or_default().to_string();
                let strip = f["strip"].as_str().map(str::to_string);
                let fraction = (state == State::Running).then(|| part_done(f));
                let st = stale.and_then(|s| {
                    s.files.iter().find(|x| {
                        (!path.is_empty() && x.path.eq_ignore_ascii_case(&path))
                            || (strip.is_some() && x.strip == strip)
                    })
                });
                Kid {
                    color: strip.as_deref().and_then(by_id).or_else(|| color_of(&name)),
                    file: Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    lufs: f["lufs"].as_f64(),
                    stale: st.map(|x| x.stale),
                    dots: st
                        .map(|x| {
                            x.tracks
                                .iter()
                                .filter(|id| Some(*id) != strip.as_ref())
                                .map(dot)
                                .collect()
                        })
                        .unwrap_or_default(),
                    wait: false,
                    tracks: None,
                    name,
                    path,
                    strip,
                    fraction,
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let dots: Vec<Dot> = stale
        .map(|s| s.changed.iter().map(|c| dot(&c.id)).collect())
        .unwrap_or_default();
    let old = stale.map_or(0, |s| s.files.iter().filter(|f| f.stale).count());
    // A stem's tags are its first file's; the album is the same for all.
    let first = files.first().cloned().unwrap_or_default();
    let tagged = if stems {
        &first["tags"]
    } else {
        &result["tags"]
    };
    let tags = [
        ("タイトル", &tagged["title"]),
        ("アルバム", &tagged["album"]),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.as_str().map(|v| format!("{k}: {v}")))
    .collect::<Vec<_>>()
    .join(" · ");
    let stale = stale.filter(|_| state == State::Done).map(|s| s.stale);
    let muted = result["muted_tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .chain(
            result["skipped"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|v| v["why"] == "ミュート中")
                .filter_map(|v| v["name"].as_str().map(String::from)),
        )
        .collect();
    Card {
        playing: false,
        muted,
        job: j["job"].as_str().unwrap_or_default().to_string(),
        stems,
        midi: false,
        title,
        meta: meta.join(" · "),
        state,
        fraction,
        speed: j["speed"].as_f64().filter(|_| state == State::Running),
        loudness,
        error: if state == State::Failed {
            let e = j["error"].as_str().unwrap_or_default();
            // The encoder's own words are in the record; the card says what failed.
            if e.starts_with(mp3::ENCODE_FAILED) {
                mp3::ENCODE_FAILED.to_string()
            } else {
                e.to_string()
            }
        } else {
            String::new()
        },
        stale,
        newest,
        more: 0,
        kids,
        dots,
        old,
        right: 0.0,
        tags,
    }
}

/// The card of MIDI files (UI-280): one file by its name, more as rows saying how many tracks
/// each holds; nothing to listen to.
fn midi_card(
    j: &Value,
    p: &Project,
    stale: Option<&Staleness>,
    newest: bool,
    state: State,
) -> Card {
    let args = &j["args"];
    let result = &j["result"];
    let each = result["each"]
        .as_bool()
        .or_else(|| args["each"].as_bool())
        .unwrap_or(false);
    let mut meta = vec![
        "MIDI".to_string(),
        if each {
            "トラックごと"
        } else {
            "1 つにまとめる"
        }
        .to_string(),
    ];
    let span = result["from"]
        .as_i64()
        .zip(result["to"].as_i64())
        .or_else(|| args["from"].as_i64().zip(args["to"].as_i64()));
    if state == State::Done
        && let Some((a, b)) = span
    {
        let tempo = xirius_session::tempo::TempoMap::from_project(p);
        meta.push(mmss(tempo.seconds_at(b) - tempo.seconds_at(a)));
    }
    let files = result["files"].as_array().cloned().unwrap_or_default();
    let file_name = |s: Option<&str>| {
        s.and_then(|s| Path::new(s).file_name())
            .map(|n| n.to_string_lossy().into_owned())
    };
    let title = match files.as_slice() {
        [one] => file_name(one["path"].as_str()),
        [] => file_name(result["path"].as_str().or_else(|| j["path"].as_str())),
        all => Some(format!("MIDI {} 本", all.len())),
    }
    .unwrap_or_else(|| "MIDI".to_string());
    let by_id = |id: &str| {
        id.parse::<TrackId>()
            .ok()
            .and_then(|t| p.tracks.get(&t))
            .map(|t| track_color(t))
    };
    let stale = stale.filter(|_| state == State::Done);
    let dot = |id: &String| Dot {
        color: by_id(id),
        name: stale
            .and_then(|s| s.changed.iter().find(|c| c.id == *id))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| id.clone()),
    };
    // One file is the card itself; more are a row each.
    let kids: Vec<Kid> = if files.len() > 1 {
        files
            .iter()
            .map(|f| {
                let path = f["path"].as_str().unwrap_or_default().to_string();
                let strip = f["strip"].as_str().map(str::to_string);
                let st = stale.and_then(|s| {
                    s.files.iter().find(|x| {
                        (!path.is_empty() && x.path.eq_ignore_ascii_case(&path))
                            || (strip.is_some() && x.strip == strip)
                    })
                });
                Kid {
                    color: strip.as_deref().and_then(by_id),
                    file: file_name(Some(&path)).unwrap_or_default(),
                    lufs: None,
                    stale: st.map(|x| x.stale),
                    dots: st
                        .map(|x| {
                            x.tracks
                                .iter()
                                .filter(|id| Some(*id) != strip.as_ref())
                                .map(dot)
                                .collect()
                        })
                        .unwrap_or_default(),
                    wait: false,
                    tracks: f["tracks"].as_u64().map(|n| n as usize),
                    name: f["name"].as_str().unwrap_or_default().to_string(),
                    path,
                    strip,
                    fraction: None,
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let dots: Vec<Dot> = stale
        .map(|s| s.changed.iter().map(|c| dot(&c.id)).collect())
        .unwrap_or_default();
    let old = stale.map_or(0, |s| s.files.iter().filter(|f| f.stale).count());
    Card {
        playing: false,
        muted: Vec::new(),
        job: j["job"].as_str().unwrap_or_default().to_string(),
        stems: false,
        midi: true,
        title,
        meta: meta.join(" · "),
        state,
        fraction: j["fraction"].as_f64().unwrap_or(0.0) as f32,
        speed: None,
        loudness: String::new(),
        error: if state == State::Failed {
            j["error"].as_str().unwrap_or_default().to_string()
        } else {
            String::new()
        },
        stale: stale.map(|s| s.stale),
        newest,
        more: 0,
        kids,
        dots,
        old,
        right: 0.0,
        tags: String::new(),
    }
}

impl Export {
    pub fn stale(&self, project: &Arc<Project>) -> bool {
        self.view(project, true)
            .cards
            .iter()
            .any(|c| c.state == State::Done && c.stale == Some(true))
    }

    /// What the panel shows, for the song `p`.
    pub fn view(&self, p: &Arc<Project>, has_dir: bool) -> View {
        let mut end: Tick = p.time_map().bar_start(2);
        for t in p.tracks.values() {
            for c in &t.clips {
                end = end.max(c.end());
            }
        }
        for s in &p.sections {
            end = end.max(s.start + s.length);
        }
        if let Some(l) = &p.loop_range {
            end = end.max(l.end);
        }
        let mut sections: Vec<SectionBar> = p
            .sections
            .iter()
            .map(|s| SectionBar {
                id: s.id,
                name: s.name.clone(),
                start: s.start,
                end: s.start + s.length,
                color: s
                    .color
                    .as_deref()
                    .and_then(palette_color)
                    .map_or(0, TrackColor::index),
                on: self.range == Range::Sections && self.sections.contains(&s.id),
            })
            .collect();
        sections.sort_by_key(|s| s.start);
        let picks = self.picks(p);
        let strip = |id: &TrackId| {
            let t = &p.tracks[id];
            let on = picks.contains(id);
            let inside = !on
                && self.group == Group::Buses
                && matches!(p.effective_output(*id), Some(Output::Bus(b)) if picks.contains(&b));
            Strip {
                id: *id,
                name: t.name.clone(),
                icon: track_icon(t),
                color: track_color(t),
                on,
                inside,
            }
        };
        let (tracks, buses) = if self.midi {
            // Only instrument tracks have notes.
            let on = self.midi_picks(p);
            let tracks = midi_export::instrument_tracks(p)
                .iter()
                .map(|id| Strip {
                    on: on.contains(id),
                    inside: false,
                    ..strip(id)
                })
                .collect();
            (tracks, Vec::new())
        } else {
            (
                StemSet::Tracks.strips(p).iter().map(strip).collect(),
                StemSet::Buses.strips(p).iter().map(strip).collect(),
            )
        };
        // An export written again runs on the card of the one it replaces.
        let listed = |id: &str| {
            self.jobs
                .iter()
                .any(|o| o["job"] == id && o["state"] != "running")
        };
        let over: HashMap<&str, &Value> = self
            .jobs
            .iter()
            .filter(|j| j["state"] == "running")
            .flat_map(|j| {
                replaced(j)
                    .into_iter()
                    .filter(|r| listed(r))
                    .map(move |r| (r, j))
            })
            .collect();
        let shown: Vec<&Value> = self
            .jobs
            .iter()
            .filter(|j| !over.values().any(|o| o["job"] == j["job"]))
            .take(CARDS)
            .collect();
        let stale = self.staleness(p, &shown);
        let cards = shown
            .iter()
            .enumerate()
            .map(|(i, j)| {
                let id = j["job"].as_str().unwrap_or_default();
                let mut card = card_of(j, p, stale.get(id).and_then(Option::as_ref), i == 0);
                card.playing = self.preview.as_deref() == Some(id);
                if let Some(run) = over.get(id) {
                    fold(&mut card, run);
                }
                if card.kids.len() > KIDS
                    && !self.expanded.iter().any(|e| e == id || *e == card.job)
                {
                    card.more = card.kids.len() - FOLDED;
                    card.kids.truncate(FOLDED);
                }
                card
            })
            .collect();
        View {
            stems: self.stems && !self.midi,
            midi: self.midi,
            each: self.each,
            range: self.range,
            format: self.format,
            rate: self.rate,
            bits: self.bits,
            kbps: self.kbps,
            song_rate: p.settings.sample_rate,
            song_bits: p.settings.bit_depth,
            mp3_error: self.mp3_error.clone(),
            group: self.group,
            has_dir,
            song: p.name.clone(),
            name: self.name().to_string(),
            sections,
            end,
            loop_span: p.loop_range.as_ref().map(|l| (l.start, l.end)),
            selected_span: self.fixed_span,
            tracks,
            buses,
            count: if self.midi && self.each {
                self.midi_picks(p).len()
            } else if self.stems && !self.midi {
                picks.len()
            } else {
                1
            },
            cards,
        }
    }
}

// ------------------------------------------------------------------ the cells

fn xp(rect: Rect, look: XpLook) -> Cell {
    Cell::new(rect, Look::Xp(look))
}

/// A tray with a button of its text's width for each item.
fn seg(cells: &mut Vec<Cell>, x: f32, y: f32, big: bool, items: &[(&str, bool, bool, Act)]) -> f32 {
    let inset = if big { 3.0 } else { 2.0 };
    let h = if big { 28.0 } else { 24.0 };
    let size = if big { FontSize::Body } else { FontSize::Small };
    let pad = if big { 14.0 } else { 10.0 };
    let widths: Vec<f32> = items
        .iter()
        .map(|(t, ..)| text_width(t, size) + pad * 2.0)
        .collect();
    let w = widths.iter().sum::<f32>() + inset * 2.0;
    cells.push(xp(
        Rect::new(x, y, w, h + inset * 2.0),
        XpLook::Tray { big },
    ));
    let mut bx = x + inset;
    for ((text, on, enabled, act), bw) in items.iter().zip(widths) {
        let mut c = xp(
            Rect::new(bx, y + inset, bw, h),
            XpLook::Seg { big, moved: false },
        )
        .text(*text)
        .on(*on)
        .act(Action::Xp(act.clone()));
        if !enabled {
            c = c.off();
        }
        cells.push(c);
        bx += bw;
    }
    w
}

/// Marks the button that is on among `cells` as off the song's own setting, and says
/// in its balloon what it starts from (EXP-012).
fn moved(cells: &mut [Cell], tip: &str) {
    if let Some(c) = cells.iter_mut().find(|c| c.on)
        && let Look::Xp(XpLook::Seg { moved, .. }) = &mut c.look
    {
        *moved = true;
        c.tip = tip.to_string();
    }
}

fn bits_text(bits: u16) -> String {
    if bits == 32 {
        "32 bit float".to_string()
    } else {
        format!("{bits} bit")
    }
}

fn label(cells: &mut Vec<Cell>, y: f32, text: &str) {
    cells.push(Cell::new(Rect::new(PAD, y, LABEL_W, ROW_H), Look::Label).text(text));
}

fn bare(rect: Rect, icon: &'static str, act: Act) -> Cell {
    xp(rect, XpLook::Bare).icon(icon).act(Action::Xp(act))
}

/// The panel's cells from (0, 0) = its top-left corner; `naming` is the name
/// being typed.
pub fn cells(v: &View, naming: Option<&str>) -> Vec<Cell> {
    let mut cells = Vec::new();
    // Head: what to write, and close.
    seg(
        &mut cells,
        PAD,
        12.0,
        true,
        &[
            (
                "ミックスダウン",
                !v.stems && !v.midi,
                true,
                Act::Mode(false),
            ),
            ("ステム", v.stems, true, Act::Mode(true)),
            ("MIDI", v.midi, true, Act::Midi),
        ],
    );
    cells.push(bare(
        Rect::new(W - 12.0 - 28.0, 15.0, 28.0, 28.0),
        "close",
        Act::Close,
    ));
    let mut y = 12.0 + 34.0 + GAP;

    // Range.
    label(&mut cells, y, "範囲");
    seg(
        &mut cells,
        X,
        y,
        false,
        &[
            (
                "曲全体",
                v.range == Range::Song,
                true,
                Act::Range(Range::Song),
            ),
            (
                "ループ範囲",
                v.range == Range::Loop,
                v.loop_span.is_some(),
                Act::Range(Range::Loop),
            ),
            (
                "セクション",
                v.range == Range::Sections,
                !v.sections.is_empty(),
                Act::Range(Range::Sections),
            ),
            (
                "選択範囲",
                v.range == Range::Selection,
                v.selected_span.is_some(),
                Act::Range(Range::Selection),
            ),
        ],
    );
    y += ROW_H + 6.0;
    let at = |t: Tick| X + CW * (t as f32 / v.end.max(1) as f32).clamp(0.0, 1.0);
    cells.push(xp(Rect::new(X, y, CW, STRIP_H), XpLook::Strip));
    for s in &v.sections {
        let (a, b) = (at(s.start), at(s.end));
        cells.push(
            xp(
                Rect::new(a + 1.0, y + 2.0, (b - a - 2.0).max(2.0), STRIP_H - 4.0),
                XpLook::Section { color: s.color },
            )
            .text(&s.name)
            .on(s.on)
            .act(Action::Xp(Act::Section(s.id))),
        );
    }
    let (a, b) = v.span();
    let (sa, sb) = (at(a), at(b));
    if v.range != Range::Song {
        cells.push(xp(Rect::new(X, y, sa - X, STRIP_H), XpLook::Dim));
        cells.push(xp(Rect::new(sb, y, X + CW - sb, STRIP_H), XpLook::Dim));
    }
    cells.push(xp(Rect::new(sa, y, sb - sa, STRIP_H), XpLook::Window));
    if let Some((la, lb)) = v.loop_span {
        let (la, lb) = (at(la), at(lb));
        cells.push(xp(
            Rect::new(la, y + STRIP_H + 4.0, lb - la, 3.0),
            XpLook::LoopMark,
        ));
    }
    y += STRIP_H + 7.0 + GAP;

    // The tracks of the MIDI files, and how many files (UI-280).
    if v.midi {
        label(&mut cells, y, "トラック");
        check_grid(&mut cells, &v.tracks, y);
        y += v.tracks.len().div_ceil(2).max(1) as f32 * ROW_H + GAP;
        label(&mut cells, y, "ファイル");
        seg(
            &mut cells,
            X,
            y,
            false,
            &[
                ("1 つにまとめる", !v.each, true, Act::Each(false)),
                ("トラックごと", v.each, true, Act::Each(true)),
            ],
        );
        y += ROW_H + GAP;
    }

    // Format.
    if !v.midi {
        label(&mut cells, y, "形式");
        let mut x = X;
        let first = cells.len();
        x += seg(
            &mut cells,
            x,
            y,
            false,
            &[
                (
                    "WAV",
                    v.format == Format::Wav,
                    true,
                    Act::Format(Format::Wav),
                ),
                (
                    "FLAC",
                    v.format == Format::Flac,
                    true,
                    Act::Format(Format::Flac),
                ),
                (
                    "MP3",
                    v.format == Format::Mp3,
                    true,
                    Act::Format(Format::Mp3),
                ),
            ],
        ) + Space::X2.px();
        if v.format != Format::Wav {
            moved(&mut cells[first..], "既定: WAV");
        }
        let mp3 = v.format == Format::Mp3;
        let at = cells.len();
        x += seg(
            &mut cells,
            x,
            y,
            false,
            &[
                ("44.1", v.rate == 44_100, true, Act::Rate(44_100)),
                ("48", v.rate == 48_000, true, Act::Rate(48_000)),
                ("96", v.rate == 96_000, !mp3, Act::Rate(96_000)),
            ],
        );
        if mp3 && let Some(c) = cells[at..].iter_mut().find(|c| c.text == "96") {
            c.tip = mp3::OVER_48K.to_string();
        }
        if v.rate != v.song_rate {
            moved(
                &mut cells[at..],
                &format!("曲の設定: {}", rate_text(u64::from(v.song_rate))),
            );
        }
        cells.push(xp(Rect::new(x + 4.0, y, 28.0, ROW_H), XpLook::Unit).text("kHz"));
        x += 4.0 + 28.0 + Space::X2.px();
        if mp3 {
            // MP3 has no bit depth: its bitrate goes on a row of its own under the format.
            y += ROW_H + 6.0;
            let names: Vec<String> = mp3::BITRATES.iter().map(|k| k.to_string()).collect();
            let items: Vec<(&str, bool, bool, Act)> = names
                .iter()
                .zip(mp3::BITRATES)
                .map(|(t, k)| (t.as_str(), v.kbps == k, true, Act::Kbps(k)))
                .collect();
            let w = seg(&mut cells, X, y, false, &items);
            cells.push(xp(Rect::new(X + w + 4.0, y, 32.0, ROW_H), XpLook::Unit).text("kbps"));
        } else {
            let at = cells.len();
            x += seg(
                &mut cells,
                x,
                y,
                false,
                &[
                    ("16", v.bits == 16, true, Act::Bits(16)),
                    ("24", v.bits == 24, true, Act::Bits(24)),
                    ("32f", v.bits == 32, v.format == Format::Wav, Act::Bits(32)),
                ],
            );
            if v.bits != v.song_bits {
                moved(
                    &mut cells[at..],
                    &format!("曲の設定: {}", bits_text(v.song_bits)),
                );
            }
            cells.push(xp(Rect::new(x + 4.0, y, 24.0, ROW_H), XpLook::Unit).text("bit"));
        }
        y += ROW_H + GAP;
    }

    // Which strips.
    if v.stems {
        label(&mut cells, y, "ステム");
        seg(
            &mut cells,
            X,
            y,
            false,
            &[
                (
                    "トラックごと",
                    v.group == Group::Tracks,
                    true,
                    Act::Group(Group::Tracks),
                ),
                (
                    "バスごと",
                    v.group == Group::Buses,
                    !v.buses.is_empty(),
                    Act::Group(Group::Buses),
                ),
                (
                    "選んだもの",
                    v.group == Group::Selected,
                    true,
                    Act::Group(Group::Selected),
                ),
            ],
        );
        y += ROW_H + 6.0;
        for (n, list) in [&v.tracks, &v.buses].into_iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            if n == 1 && !v.tracks.is_empty() {
                y += 6.0;
            }
            check_grid(&mut cells, list, y);
            y += list.len().div_ceil(2) as f32 * ROW_H;
        }
        y += GAP;
    }

    // Where, and the name.
    label(&mut cells, y, "置き場所");
    let folder = if v.midi {
        midi_export::FOLDER
    } else if v.stems {
        "Stems"
    } else {
        "Mixdown"
    };
    let head = format!("{} /", v.song);
    let pw = 8.0
        + Layout::ICON_SIZE
        + 6.0
        + text_width(&head, FontSize::Small)
        + 4.0
        + text_width(folder, FontSize::Small)
        + 10.0;
    let pw = pw.min(CW * 0.55);
    cells.push(
        xp(Rect::new(X, y, pw, ROW_H), XpLook::Path)
            .text(head)
            .hint(folder),
    );
    let editing = naming.is_some();
    cells.push(
        xp(
            Rect::new(X + pw + 6.0, y, CW - pw - 6.0, ROW_H),
            XpLook::Field {
                ext: if v.midi {
                    ".mid"
                } else {
                    match v.format {
                        Format::Wav => ".wav",
                        Format::Flac => ".flac",
                        Format::Mp3 => ".mp3",
                    }
                },
                bad: v.bad_name() && !editing,
                editing,
            },
        )
        .text(naming.unwrap_or(&v.name))
        .act(Action::Xp(Act::EditName)),
    );
    y += ROW_H + GAP;

    // The button.
    let text = "書き出す";
    let badge = if v.midi {
        (v.count > 1).then_some(v.count)
    } else {
        v.stems.then_some(v.count)
    };
    let gw = Space::X3.px()
        + Layout::ICON_SIZE
        + Space::X2.px()
        + text_width(text, FontSize::Body)
        + badge.map_or(0.0, |n| {
            Space::X2.px() + 12.0 + text_width(&n.to_string(), FontSize::Caption)
        })
        + Space::X4.px();
    let mut go = xp(Rect::new(W - PAD - gw, y, gw, 36.0), XpLook::Go { badge })
        .text(text)
        .icon("export")
        .act(Action::Xp(Act::Go));
    if !v.can_go() {
        go = go.off();
    }
    cells.push(go);
    if !v.has_dir {
        cells.push(
            Cell::new(
                Rect::new(PAD - Space::X2.px(), y, W - PAD * 2.0 - gw, 36.0),
                Look::Note,
            )
            .text("書き出すには、先に曲の保存が必要です"),
        );
    } else if v.no_mp3() {
        // Only MP3 stops: WAV and FLAC are written as ever (EXP-005).
        cells.push(
            xp(
                Rect::new(PAD, y, W - PAD * 2.0 - gw - Space::X2.px(), 36.0),
                XpLook::Alert,
            )
            .text(mp3::CANNOT_LOAD),
        );
    } else if (v.stems || (v.midi && v.each)) && v.bad_name() {
        cells.push(
            Cell::new(
                Rect::new(PAD - Space::X2.px(), y, W - PAD * 2.0 - gw, 36.0),
                Look::Note,
            )
            .text(if v.midi {
                format!("ファイル名には {TRACK_TAG} が必要です")
            } else {
                format!("ステムの名前には {TRACK_TAG} が必要です")
            }),
        );
    }
    y += 36.0;

    // What was written.
    if !v.cards.is_empty() {
        y += GAP;
    }
    for card in &v.cards {
        y += card_cells(&mut cells, card, y) + 6.0;
    }
    cells.push(xp(Rect::new(0.0, y, 1.0, 10.0), XpLook::Space));
    cells
}

/// Two columns of strips to tick, from `y`.
fn check_grid(cells: &mut Vec<Cell>, list: &[Strip], y: f32) {
    let col = (CW - Space::X2.px()) / 2.0;
    for (i, s) in list.iter().enumerate() {
        let r = Rect::new(
            X + (i % 2) as f32 * (col + Space::X2.px()),
            y + (i / 2) as f32 * ROW_H,
            col,
            ROW_H,
        );
        cells.push(
            xp(
                r,
                XpLook::Check {
                    color: s.color,
                    inside: s.inside,
                },
            )
            .text(&s.name)
            .icon(s.icon)
            .on(s.on)
            .act(Action::Xp(Act::Pick(s.id))),
        );
    }
}

fn tipped(mut c: Cell, tip: impl Into<String>) -> Cell {
    c.tip = tip.into();
    c
}

/// The height of a card.
fn card_h(card: &Card) -> f32 {
    let mut h = CARD_H + card.muted.len() as f32 * KID_H + FOOT_H;
    if !card.kids.is_empty() {
        h += (card.kids.len() + usize::from(card.more > 0)) as f32 * KID_H + 6.0;
    }
    h
}

/// A circle of a file as the song is now, or of a stale one, with its balloon.
fn mark(r: Rect, stale: bool) -> Cell {
    tipped(
        xp(r, XpLook::Mark { stale }),
        if stale {
            "書き出しの後に曲が変わった"
        } else {
            "今の曲と同じ"
        },
    )
}

/// The dots of the tracks that changed, right to left from `right` on the line through `cy`;
/// returns where they end.
fn dots(cells: &mut Vec<Cell>, dots: &[Dot], mut right: f32, cy: f32, size: f32) -> f32 {
    for d in dots.iter().rev() {
        right -= size;
        cells.push(tipped(
            xp(
                Rect::new(right, cy - size / 2.0, size, size),
                XpLook::Dot { color: d.color },
            ),
            d.name.clone(),
        ));
        right -= 3.0;
    }
    right
}

/// A card and its buttons at `y`; returns its height.
fn card_cells(cells: &mut Vec<Cell>, card: &Card, y: f32) -> f32 {
    let x = PAD;
    let w = W - PAD * 2.0;
    let h = card_h(card);
    let at = cells.len();
    cells.push(xp(
        Rect::new(x, y, w, h),
        XpLook::Card(Box::new(card.clone())),
    ));
    // The files, one to a row: the marks first, so the pointer finds their balloons.
    let mut ky = y + CARD_H;
    let row_x = x + FILES_X;
    let row_w = x + w - 10.0 - row_x;
    for kid in &card.kids {
        let cy = ky + KID_H / 2.0;
        let mark_right = row_x + row_w - 4.0;
        if kid.fraction.is_none()
            && let Some(stale) = kid.stale
        {
            let m = Rect::new(mark_right - 18.0, cy - 9.0, 18.0, 18.0);
            cells.push(mark(m, stale));
            dots(cells, &kid.dots, m.x - 3.0, cy, 7.0);
        }
        cells.push(xp(
            Rect::new(row_x, ky, row_w, KID_H),
            XpLook::File(Box::new(kid.clone())),
        ));
        ky += KID_H;
    }
    if card.more > 0 {
        cells.push(
            xp(Rect::new(row_x, ky, row_w, KID_H), XpLook::More)
                .text(format!("ほか {} 本", card.more))
                .act(Action::Xp(Act::More(card.job.clone()))),
        );
    }
    let by = y + h - 32.0;
    let mut right = x + w - 10.0;
    let mut put = |cells: &mut Vec<Cell>, width: f32, c: Cell| {
        right -= width;
        let mut c = c;
        c.rect = Rect::new(right, c.rect.y, width, c.rect.h);
        cells.push(c);
        right -= 4.0;
    };
    let job = card.job.clone();
    let button = |text: &str, act: Act| {
        let bw = (Space::X3.px() * 2.0 + text_width(text, FontSize::Small)).max(60.0);
        (
            bw,
            Cell::new(Rect::new(0.0, by, bw, 28.0), Look::Button)
                .text(text)
                .act(Action::Xp(act)),
        )
    };
    match card.state {
        State::Running => {
            put(
                cells,
                28.0,
                tipped(
                    bare(Rect::new(0.0, by, 28.0, 28.0), "close", Act::Cancel(job)),
                    "止める",
                ),
            );
        }
        State::Done => {
            let (bw, c) = button("曲に取り込む", Act::Import(job.clone()));
            put(cells, bw, c);
            // A MIDI file has no sound of its own to listen to.
            if !card.midi {
                let text = if card.playing { "停止" } else { "聴く" };
                let (bw, c) = button(text, Act::Preview(job.clone()));
                put(cells, bw, c);
            }
            put(
                cells,
                28.0,
                tipped(
                    bare(
                        Rect::new(0.0, by, 28.0, 28.0),
                        "trash",
                        Act::Delete(job.clone()),
                    ),
                    "ごみ箱へ",
                ),
            );
            put(
                cells,
                28.0,
                tipped(
                    bare(
                        Rect::new(0.0, by, 28.0, 28.0),
                        "folder-open",
                        Act::Reveal(job.clone()),
                    ),
                    "エクスプローラーで表示",
                ),
            );
            let cy = by + 14.0;
            if card.stale == Some(true) {
                let t = "更新";
                let bw = Space::X2.px() * 2.0
                    + Layout::ICON_SIZE
                    + Space::X2.px()
                    + text_width(t, FontSize::Small);
                put(
                    cells,
                    bw,
                    tipped(
                        Cell::new(Rect::new(0.0, by + 1.0, bw, 26.0), Look::Primary)
                            .text(t)
                            .icon("sync")
                            .act(Action::Xp(Act::Update(job))),
                        if card.kids.len() > 1 || card.more > 0 {
                            format!("変わった {} 本を書き直す", card.old)
                        } else {
                            "書き直す".to_string()
                        },
                    ),
                );
            }
            if let Some(stale) = card.stale {
                put(
                    cells,
                    20.0,
                    mark(Rect::new(0.0, cy - 10.0, 20.0, 20.0), stale),
                );
                if stale {
                    right = dots(cells, &card.dots, right, cy, 8.0) - 1.0;
                }
            }
        }
        State::Failed => {
            put(
                cells,
                28.0,
                tipped(
                    bare(
                        Rect::new(0.0, by, 28.0, 28.0),
                        "trash",
                        Act::Delete(job.clone()),
                    ),
                    "一覧から削除",
                ),
            );
            let (bw, c) = button("書き直す", Act::Update(job));
            put(cells, bw, c);
        }
    }
    if let Look::Xp(XpLook::Card(c)) = &mut cells[at].look {
        c.right = x + w - right;
    }
    if !card.tags.is_empty() {
        // Over the name, which the loudness leaves room for.
        let tx = x + 52.0;
        let room = (x + w - 10.0 - 96.0 - tx).max(0.0);
        let mut c = xp(
            Rect::new(
                tx,
                y + 6.0,
                text_width(&card.title, FontSize::Small).min(room),
                22.0,
            ),
            XpLook::Space,
        );
        c.tip = card.tags.clone();
        cells.push(c);
    }
    h
}

// ------------------------------------------------------------------- painting

fn icon_at(r: Rect, x: f32) -> Rect {
    Rect::new(
        x,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    )
}

/// A ring of `t` px inside `r`.
fn ring(pen: &mut Pen, r: Rect, t: f32, color: gpui::Hsla) {
    pen.fill(Rect::new(r.x, r.y, r.w, t), color);
    pen.fill(Rect::new(r.x, r.bottom() - t, r.w, t), color);
    pen.fill(Rect::new(r.x, r.y + t, t, r.h - t * 2.0), color);
    pen.fill(Rect::new(r.right() - t, r.y + t, t, r.h - t * 2.0), color);
}

/// Paints one of the panel's own cells.
pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: &XpLook, hot: bool) {
    let c = pen.c;
    let r = cell.rect;
    let hot = hot && cell.live();
    let pad = Space::X2.px();
    match look {
        XpLook::Tray { .. } | XpLook::Strip => pen.round(r, c.surface_2, Radius::S),
        XpLook::Seg { big, moved } => {
            let ink = if !cell.enabled {
                c.ink
            } else if cell.on {
                pen.round(r, c.surface_3, Radius::S);
                if *moved { c.ink_blue } else { c.ink_strong }
            } else if hot {
                c.ink_strong
            } else {
                c.ink
            };
            let (size, weight) = if *big {
                (FontSize::Body, Weight::Medium)
            } else {
                (FontSize::Small, Weight::Regular)
            };
            pen.text(&cell.text, size, weight, ink, r, Align::Center);
        }
        XpLook::Unit => {
            pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                r,
                Align::Left,
            );
        }
        XpLook::Section { color } => {
            let (face, ink) = c.sections[color % c.sections.len()];
            let face = if cell.on {
                face
            } else if hot {
                c.hover
            } else {
                c.surface_3
            };
            pen.round(r, face, Radius::Clip);
            let ink = if cell.on { ink } else { c.ink_strong };
            pen.mask(r, |pen| {
                pen.text(
                    &cell.text,
                    FontSize::Micro,
                    Weight::Medium,
                    ink,
                    Rect::new(r.x + 6.0, r.y, r.w, r.h),
                    Align::Left,
                );
            });
        }
        XpLook::Dim => pen.fill(r, c.surface_2.opacity(0.74)),
        XpLook::Window => ring(pen, r, 1.5, c.ring),
        XpLook::LoopMark => pen.round(r, c.ring.opacity(0.55), Radius::Xs),
        XpLook::Check { color, inside } => {
            if hot {
                pen.round(r, c.hover, Radius::S);
            }
            let tone = c.tracks[color % c.tracks.len()];
            let b = Rect::new(r.x + pad, r.y + (r.h - 16.0) / 2.0, 16.0, 16.0);
            if cell.on {
                pen.round(b, tone, Radius::Clip);
                pen.icon(
                    "check",
                    Rect::new(b.x + 2.0, b.y + 2.0, 12.0, 12.0),
                    c.surface_0,
                );
            } else if *inside {
                pen.round(b, tone.opacity(0.4), Radius::Clip);
            } else {
                pen.round(b, c.surface_3, Radius::Clip);
            }
            let mut x = b.right() + pad;
            let ink = if *inside { c.ink_on_toggle } else { c.ink };
            if let Some(icon) = cell.icon {
                pen.icon(icon, icon_at(r, x), ink);
                x += Layout::ICON_SIZE + pad;
            }
            pen.mask(Rect::new(x, r.y, r.right() - x - pad, r.h), |pen| {
                pen.text(
                    &cell.text,
                    FontSize::Small,
                    Weight::Regular,
                    if cell.on { c.ink_strong } else { ink },
                    Rect::new(x, r.y, r.w, r.h),
                    Align::Left,
                );
            });
        }
        XpLook::Path => {
            pen.round(r, c.surface_2, Radius::S);
            pen.icon("folder", icon_at(r, r.x + pad), c.ink);
            let x = r.x + pad + Layout::ICON_SIZE + 6.0;
            pen.mask(Rect::new(x, r.y, r.right() - x - 6.0, r.h), |pen| {
                let w = pen.text(
                    &cell.text,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    Rect::new(x, r.y, r.w, r.h),
                    Align::Left,
                );
                pen.text(
                    &cell.hint,
                    FontSize::Small,
                    Weight::Medium,
                    c.ink_strong,
                    Rect::new(x + w + 4.0, r.y, r.w, r.h),
                    Align::Left,
                );
            });
        }
        XpLook::Field { ext, bad, editing } => {
            pen.round(
                r,
                if *editing {
                    c.well
                } else if hot {
                    c.hover
                } else {
                    c.surface_2
                },
                Radius::S,
            );
            if *bad {
                ring(pen, r, 1.5, c.alert);
            }
            let ext_w = text_width(ext, FontSize::Small) + 10.0;
            let tx = r.x + 10.0;
            let room = Rect::new(tx, r.y, r.right() - ext_w - tx, r.h);
            let mut x = tx;
            pen.mask(room, |pen| {
                if *editing {
                    x += pen.text(
                        &cell.text,
                        FontSize::Small,
                        Weight::Regular,
                        c.ink_strong,
                        Rect::new(x, r.y, r.w, r.h),
                        Align::Left,
                    );
                    return;
                }
                // The tags the session fills in, in blue.
                let mut rest = cell.text.as_str();
                while !rest.is_empty() {
                    let next = [SONG_TAG, TRACK_TAG]
                        .iter()
                        .filter_map(|t| rest.find(t).map(|i| (i, *t)))
                        .min();
                    let (plain, tag) = match next {
                        Some((i, t)) => (&rest[..i], Some(t)),
                        None => (rest, None),
                    };
                    if !plain.is_empty() {
                        x += pen.text(
                            plain,
                            FontSize::Small,
                            Weight::Regular,
                            c.ink_strong,
                            Rect::new(x, r.y, r.w, r.h),
                            Align::Left,
                        );
                    }
                    let Some(t) = tag else { break };
                    x += pen.text(
                        t,
                        FontSize::Small,
                        Weight::Medium,
                        c.ink_blue,
                        Rect::new(x, r.y, r.w, r.h),
                        Align::Left,
                    );
                    rest = &rest[plain.len() + t.len()..];
                }
            });
            if *editing {
                pen.fill(
                    Rect::new(x.min(room.right()) + 1.0, r.y + 7.0, 1.5, r.h - 14.0),
                    c.ink_strong,
                );
            }
            pen.text(
                ext,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                Rect::new(r.x, r.y, r.w - 10.0, r.h),
                Align::Right,
            );
        }
        XpLook::Bare => {
            if hot {
                pen.round(r, c.hover, Radius::S);
            }
            if let Some(icon) = cell.icon {
                let ink = if hot { c.ink_strong } else { c.ink };
                pen.icon(icon, icon_at(r, r.x + (r.w - Layout::ICON_SIZE) / 2.0), ink);
            }
        }
        XpLook::Go { badge } => {
            let (face, ink) = if !cell.enabled {
                (c.surface_3, c.ink_on_toggle)
            } else if hot {
                (c.fill_hover, c.on_fill)
            } else {
                (c.fill, c.on_fill)
            };
            pen.round(r, face, Radius::S);
            let mut x = r.x + Space::X3.px();
            if let Some(icon) = cell.icon {
                pen.icon(icon, icon_at(r, x), ink);
                x += Layout::ICON_SIZE + Space::X2.px();
            }
            x += pen.text(
                &cell.text,
                FontSize::Body,
                Weight::Semibold,
                ink,
                Rect::new(x, r.y, r.w, r.h),
                Align::Left,
            );
            if let Some(n) = badge {
                let t = n.to_string();
                let bw = 12.0 + text_width(&t, FontSize::Caption);
                let b = Rect::new(
                    x + Space::X2.px(),
                    r.y + (r.h - 20.0) / 2.0,
                    bw.max(20.0),
                    20.0,
                );
                pen.round(b, c.surface_3, Radius::Pill);
                pen.digits(
                    &t,
                    FontSize::Caption,
                    Weight::Semibold,
                    ink,
                    b,
                    Align::Center,
                );
            }
        }
        XpLook::Card(card) => paint_card(pen, r, card),
        XpLook::File(kid) => paint_file(pen, r, kid),
        XpLook::More => {
            if hot {
                pen.round(r, c.hover, Radius::S);
            }
            pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Regular,
                if hot { c.ink_strong } else { c.ink },
                Rect::new(r.x + 15.0, r.y, r.w, r.h),
                Align::Left,
            );
        }
        XpLook::Mark { stale } => {
            let i = Rect::new(
                r.x + (r.w - 14.0) / 2.0,
                r.y + (r.h - 14.0) / 2.0,
                14.0,
                14.0,
            );
            if *stale {
                pen.round(r, c.stale.opacity(0.16), Radius::Pill);
                pen.icon("history", i, c.stale);
            } else {
                pen.icon("saved", i, c.ink.opacity(0.7));
            }
        }
        XpLook::Dot { color } => {
            let tone = color.map_or(c.line[0], |i| c.tracks[i % c.tracks.len()]);
            pen.round(r, tone, Radius::Pill);
        }
        XpLook::Alert => {
            pen.mask(r, |pen| {
                pen.text(
                    &cell.text,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink_alert,
                    r,
                    Align::Left,
                );
            });
        }
        XpLook::Space => {}
    }
}

/// The width at the right of a file's row its marks take.
fn marks_w(kid: &Kid) -> f32 {
    (18.0 + kid.dots.len() as f32 * 10.0).max(40.0)
}

fn paint_file(pen: &mut Pen, r: Rect, kid: &Kid) {
    let c = pen.c;
    let tone = kid
        .color
        .map_or(c.line[0], |i| c.tracks[i % c.tracks.len()]);
    // design-lint: allow(exception-family) ファイルの行の頭の点は、そのファイルのトラックの色（トラックの表示）
    pen.round(
        Rect::new(r.x + 3.0, r.y + (r.h - 7.0) / 2.0, 7.0, 7.0),
        tone,
        Radius::Pill,
    );
    let mut end = r.right() - 4.0 - marks_w(kid) - Space::X2.px();
    if let Some(f) = kid.fraction {
        let bar = Rect::new(end - 80.0, r.y + (r.h - 4.0) / 2.0, 80.0, 4.0);
        pen.round(bar, c.surface_3, Radius::Xs);
        pen.round(
            Rect::new(bar.x, bar.y, bar.w * f.clamp(0.0, 1.0), bar.h),
            c.ring,
            Radius::Xs,
        );
        end = bar.x - Space::X2.px();
    } else if let Some(n) = kid.tracks {
        let w = pen.digits(
            &format!("{n} トラック"),
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(r.x, r.y, end - r.x, r.h),
            Align::Right,
        );
        end -= w + Space::X2.px();
    } else if let Some(l) = kid.lufs {
        // A stale file's loudness steps down to the body ink.
        let ink = if kid.stale == Some(true) {
            c.ink
        } else {
            c.ink_strong
        };
        let w = pen.digits(
            &format!("{l:.1} LUFS"),
            FontSize::Caption,
            Weight::Regular,
            ink,
            Rect::new(r.x, r.y, end - r.x, r.h),
            Align::Right,
        );
        end -= w + Space::X2.px();
    }
    let x = r.x + 15.0;
    let ink = if kid.wait { c.ink } else { c.ink_strong };
    pen.mask(Rect::new(x, r.y, (end - x).max(0.0), r.h), |pen| {
        pen.text(
            kid.shown(),
            FontSize::Caption,
            Weight::Regular,
            ink,
            Rect::new(x, r.y, r.w, r.h),
            Align::Left,
        );
    });
}

fn paint_card(pen: &mut Pen, r: Rect, card: &Card) {
    let c = pen.c;
    pen.round(r, c.surface_2, Radius::M);
    let tile = Rect::new(r.x + 11.0, r.y + 11.0, 30.0, 30.0);
    pen.round(tile, c.surface_3, Radius::S);
    pen.icon(
        if card.midi {
            "midi"
        } else if card.stems {
            "layers"
        } else {
            "output"
        },
        icon_at(tile, tile.x + 7.0),
        c.ink_strong,
    );
    let x = r.x + 52.0;
    let mut right = r.right() - 10.0 - Space::X2.px();
    // Loudness (LUFS over dBTP), or how fast it goes, at the right.
    if card.state == State::Running {
        if let Some(s) = card.speed {
            let w = pen.digits(
                &format!("×{s}"),
                FontSize::Caption,
                Weight::Medium,
                c.ink_blue,
                Rect::new(x, r.y, right - x, CARD_H),
                Align::Right,
            );
            right -= w + Space::X3.px();
        }
    } else if !card.loudness.is_empty() {
        let lines: Vec<&str> = card.loudness.lines().collect();
        let top = r.y + (CARD_H - 16.0 * lines.len() as f32) / 2.0;
        let ink = if card.stale == Some(true) {
            c.ink
        } else {
            c.ink_strong
        };
        let mut w: f32 = 0.0;
        for (i, line) in lines.iter().enumerate() {
            w = w.max(pen.digits(
                line,
                FontSize::Caption,
                Weight::Medium,
                ink,
                Rect::new(x, top + 16.0 * i as f32, right - x, 16.0),
                Align::Right,
            ));
        }
        right -= w + Space::X3.px();
    }
    pen.mask(Rect::new(x, r.y, (right - x).max(0.0), CARD_H), |pen| {
        pen.text(
            &card.title,
            FontSize::Small,
            Weight::Medium,
            c.ink_strong,
            Rect::new(x, r.y + 8.0, r.w, 18.0),
            Align::Left,
        );
        pen.text(
            &card.meta,
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(x, r.y + 26.0, r.w, 16.0),
            Align::Left,
        );
    });
    let muted_y = r.bottom() - card.muted.len() as f32 * KID_H - FOOT_H;
    for (i, name) in card.muted.iter().enumerate() {
        pen.text(
            &format!("ミュート: {name}"),
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(x, muted_y + i as f32 * KID_H, r.right() - 10.0 - x, KID_H),
            Align::Left,
        );
    }
    // At the left of the buttons: how far it is, or why it failed.
    let foot = Rect::new(r.x + 12.0, r.bottom() - 32.0, 0.0, 28.0);
    let foot = Rect::new(
        foot.x,
        foot.y,
        (r.right() - card.right - Space::X2.px() - foot.x).max(0.0),
        foot.h,
    );
    match card.state {
        State::Running => {
            pen.digits(
                &format!("{}%", (card.fraction * 100.0).floor()),
                FontSize::Caption,
                Weight::Medium,
                c.ink,
                foot,
                Align::Left,
            );
            let bar = Rect::new(r.x + 12.0, r.bottom() - 4.0, r.w - 24.0, 2.0);
            pen.round(bar, c.surface_3, Radius::Xs);
            pen.round(
                Rect::new(bar.x, bar.y, bar.w * card.fraction.clamp(0.0, 1.0), bar.h),
                c.ring,
                Radius::Xs,
            );
        }
        State::Failed if !card.error.is_empty() => {
            pen.icon("alert", icon_at(foot, foot.x), c.alert);
            let tx = foot.x + Layout::ICON_SIZE + 6.0;
            pen.mask(
                Rect::new(tx, foot.y, (foot.right() - tx).max(0.0), foot.h),
                |pen| {
                    pen.text(
                        &card.error,
                        FontSize::Caption,
                        Weight::Regular,
                        c.ink_alert,
                        Rect::new(tx, foot.y, r.w, foot.h),
                        Align::Left,
                    );
                },
            );
        }
        _ => {}
    }
}

// --------------------------------------------------------------- the studio

impl Studio {
    /// Opens the panel under `anchor` (or closes it when it is open), with
    /// the settings and the exports the session has.
    pub(crate) fn open_export(&mut self, anchor: Rect) {
        self.open_popup(PopupKind::Export, anchor);
        if self
            .popup
            .as_ref()
            .is_some_and(|p| p.kind == PopupKind::Export)
        {
            self.load_export();
            self.refresh_popup();
        }
    }

    /// The card under a point of the panel, when it is open.
    pub(crate) fn export_card_at(&self, g: &Geometry, x: f32, y: f32) -> Option<Card> {
        let p = self
            .popup
            .as_ref()
            .filter(|p| p.kind == PopupKind::Export)?;
        p.layout(g.size)
            .cells
            .into_iter()
            .find_map(|c| match c.look {
                Look::Xp(XpLook::Card(card)) if c.rect.contains(x, y) => Some(*card),
                _ => None,
            })
    }

    /// The session's record of an export.
    pub(crate) fn export_job(&self, job: &str) -> Option<&Value> {
        self.export.jobs.iter().find(|j| j["job"] == job)
    }

    /// The panel's settings become those an export was started with.
    pub(crate) fn export_like(&mut self, job: &str) -> bool {
        let Some(j) = self.export_job(job).cloned() else {
            return false;
        };
        let a = &j["args"];
        let p = self.project();
        let fixed_span = if a["range"] == "custom" {
            match xirius_session::export_selection::saved_span(&p, a) {
                Ok(span) => Some(span),
                Err(e) => {
                    self.error = Some(e.message);
                    return false;
                }
            }
        } else {
            None
        };
        let dir = self.song.dir();
        let x = &mut self.export;
        x.stems = j["command"] == "export.stems";
        x.midi = j["command"] == "export.midi";
        if x.midi {
            x.each = a["each"].as_bool().unwrap_or(false);
            let ids: Vec<TrackId> = a["tracks"]
                .as_array()
                .map(|v| v.iter().filter_map(|i| i.as_str()?.parse().ok()).collect())
                .unwrap_or_default();
            x.midi_picked = (!ids.is_empty()).then_some(ids);
        }
        if let Some(f) = a["format"].as_str().and_then(Format::parse) {
            x.format = f;
        }
        if let Some(r) = a["sample_rate"].as_u64() {
            x.rate = r as u32;
        }
        if let Some(b) = a["bit_depth"].as_u64() {
            x.bits = b as u16;
        }
        if let Some(k) = a["bitrate"].as_u64() {
            x.kbps = k as u16;
        }
        if let Some(n) = a["name"].as_str() {
            x.set_name(n.to_string());
        }
        x.sections.clear();
        x.fixed_span = None;
        x.range = match a["range"].as_str() {
            Some("custom") => {
                x.fixed_span = fixed_span;
                x.fixed_song = dir;
                Range::Selection
            }
            Some("loop") => Range::Loop,
            Some("section") => {
                x.sections = a["section"]
                    .as_str()
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                Range::Sections
            }
            _ => Range::Song,
        };
        if x.stems {
            let mut ids: Vec<TrackId> = a["ids"]
                .as_array()
                .map(|v| v.iter().filter_map(|i| i.as_str()?.parse().ok()).collect())
                .unwrap_or_default();
            x.folders = a["folders"]
                .as_array()
                .map(|v| v.iter().filter_map(|i| i.as_str()?.parse().ok()).collect())
                .unwrap_or_default();
            ids.extend(x.folders.iter().filter_map(|f| folder_bus(&p, *f)));
            if !ids.is_empty() {
                x.picked = ids;
                x.group = Group::Selected;
            }
        }
        true
    }

    pub(crate) fn export_view(&self) -> View {
        let project = self.project();
        let mut view = self.export.view(&project, self.song.dir().is_some());
        view.selected_span = self
            .export
            .fixed_span
            .filter(|_| self.export.fixed_song == self.song.dir())
            .or_else(|| {
                xirius_session::export_selection::span(&project, &self.export_clip_ids()).ok()
            });
        view
    }

    /// The settings the song remembers, once per song folder, and the exports. A caller that
    /// picks the range itself reads them first, so what the song remembers does not replace it.
    pub(crate) fn load_export(&mut self) {
        let dir = self.song.dir();
        if let Ok(v) = self.song.run("export.open", json!({})) {
            let x = &mut self.export;
            // Whether MP3 can be written is read on every opening; the settings only once a song.
            let st = &v["mp3"];
            x.mp3_error = (st["available"] == false)
                .then(|| st["error"].as_str().unwrap_or(mp3::CANNOT_LOAD).to_string());
            if x.loaded.as_ref() != Some(&dir) {
                if x.fixed_song != dir {
                    x.fixed_span = None;
                    x.fixed_song = None;
                    if x.range == Range::Selection {
                        x.range = Range::Song;
                    }
                }
                if let Some(n) = v["mixdown"]["name"].as_str() {
                    x.mix_name = n.to_string();
                }
                if let Some(n) = v["stems"]["name"].as_str() {
                    x.stems_name = n.to_string();
                }
                if let Some(n) = v["midi"]["name"].as_str() {
                    x.midi_name = n.to_string();
                }
                if let Some(n) = v["midi"]["each_name"].as_str() {
                    x.each_name = n.to_string();
                }
                if let Some(r) = v["sample_rate"].as_u64() {
                    x.rate = r as u32;
                }
                if let Some(b) = v["bit_depth"].as_u64() {
                    x.bits = b as u16;
                }
                // The range last exported with this song, when the song still has it.
                x.sections.clear();
                x.range = match v["range"].as_str() {
                    Some("loop") => Range::Loop,
                    Some("section") => {
                        x.sections = v["picked_sections"]
                            .as_array()
                            .map(|a| a.iter().filter_map(|i| i.as_str()?.parse().ok()).collect())
                            .unwrap_or_default();
                        Range::Sections
                    }
                    // A selection fixed in this song stays; any other range was the last song's.
                    _ if x.range == Range::Selection => Range::Selection,
                    _ => Range::Song,
                };
                x.format = v["format"]
                    .as_str()
                    .and_then(Format::parse)
                    .unwrap_or_default();
                if let Some(k) = v["bitrate"].as_u64() {
                    x.kbps = k as u16;
                }
                x.loaded = Some(dir);
            }
        }
        self.reload_exports();
    }

    pub(crate) fn reload_exports(&mut self) {
        if let Ok(v) = self.song.run("export.status", json!({})) {
            let mut jobs: Vec<Value> = v["jobs"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|j| j["args"].is_object())
                .collect();
            jobs.reverse();
            self.export.jobs = jobs;
        }
    }

    /// Keeps the name being typed, when there is one.
    pub(crate) fn commit_export_name(&mut self) {
        let Some(p) = &mut self.popup else { return };
        if p.kind != PopupKind::Export {
            return;
        }
        if let Some(name) = p.naming.take() {
            let name = name.trim().to_string();
            if !name.is_empty() {
                self.export.set_name(name);
            }
        }
    }

    pub(crate) fn export_run(&mut self, command: &str, args: Value) -> Option<Value> {
        match self.song.run(command, args) {
            Ok(v) => {
                self.error = None;
                Some(v)
            }
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }

    pub(crate) fn export_action(&mut self, act: Act) -> bool {
        if act != Act::EditName {
            self.commit_export_name();
        }
        let p = self.project();
        let x = &mut self.export;
        match act {
            Act::Preview(job) => {
                let stopping = x.preview.as_deref() == Some(&job);
                if !stopping && (self.transport.recording || self.transport.take.is_some()) {
                    self.error = Some("録音中は試聴できません".into());
                    return true;
                }
                if !stopping {
                    self.browser.playing = None;
                    crate::audition::stop();
                    self.transport.suspend_for_export();
                }
                let output = match self.transport.mode {
                    xirius_session::output::OutputMode::Device => "device",
                    xirius_session::output::OutputMode::Silent { .. } => "silent",
                    xirius_session::output::OutputMode::None => "none",
                };
                if self
                    .export_run(
                        "export.preview",
                        json!({"job":job,"stop":stopping,"output":output}),
                    )
                    .is_some()
                {
                    self.export.preview = (!stopping).then_some(job);
                    self.export.preview_peak = 0.0;
                }
                self.transport.auditioning_export = self.export.preview.is_some();
            }
            Act::Import(job) => {
                // At the head of the range it was written from, where its beat lines up.
                self.run_command("export.import", json!({"job":job}));
            }
            Act::Mode(stems) => x.set_stems(stems),
            Act::Midi => x.midi = true,
            Act::Each(each) => x.each = each,
            Act::Range(r) => {
                x.fixed_span = None;
                if r == Range::Sections && x.sections.is_empty() {
                    let first = p.sections.iter().min_by_key(|s| s.start);
                    x.sections.extend(first.map(|s| s.id));
                }
                x.range = r;
            }
            Act::Section(id) => {
                if x.range != Range::Sections {
                    x.sections.clear();
                }
                if let Some(i) = x.sections.iter().position(|s| *s == id) {
                    x.sections.remove(i);
                } else {
                    x.sections.push(id);
                }
                x.range = if x.sections.is_empty() {
                    Range::Song
                } else {
                    Range::Sections
                };
            }
            Act::Format(f) => {
                x.format = f;
                if f == Format::Flac && x.bits == 32 {
                    x.bits = 24;
                }
                if f == Format::Mp3 && x.rate > mp3::MAX_RATE {
                    x.rate = if x.rate.is_multiple_of(44_100) {
                        44_100
                    } else {
                        mp3::MAX_RATE
                    };
                }
            }
            Act::Rate(r) => x.rate = r,
            Act::Bits(b) => x.bits = b,
            Act::Kbps(k) => x.kbps = k,
            Act::Group(g) => {
                if g == Group::Selected {
                    x.picked = x.picks(&p);
                }
                x.group = g;
            }
            Act::Pick(id) if x.midi => {
                let mut picked = x.midi_picks(&p);
                if let Some(i) = picked.iter().position(|t| *t == id) {
                    picked.remove(i);
                } else {
                    picked.push(id);
                }
                x.midi_picked = Some(picked);
            }
            Act::Pick(id) => {
                let mut picked = x.picks(&p);
                if let Some(i) = picked.iter().position(|t| *t == id) {
                    picked.remove(i);
                } else {
                    picked.push(id);
                }
                x.picked = picked;
                x.group = Group::Selected;
            }
            Act::EditName => {
                let name = x.name().to_string();
                if let Some(pop) = &mut self.popup {
                    pop.naming = Some(name);
                }
            }
            Act::Go => {
                let view = self.export_view();
                if !view.can_go() {
                    return true;
                }
                let x = &self.export;
                if x.midi {
                    let args = self.midi_args(&view);
                    self.export_run("export.midi", args);
                    self.reload_exports();
                    self.refresh_popup();
                    return true;
                }
                let mut args = json!({
                    "background": true,
                    "format": x.format.name(),
                    "sample_rate": x.rate,
                    "name": x.name(),
                });
                if x.format == Format::Mp3 {
                    args["bitrate"] = json!(x.kbps);
                } else {
                    args["bit_depth"] = json!(x.bits);
                }
                args["range"] = json!(match x.range {
                    Range::Song => "song",
                    Range::Loop => "loop",
                    Range::Sections => "section",
                    Range::Selection => "custom",
                });
                if x.range == Range::Selection {
                    let (start, end) = view.span();
                    args["from"] = json!(start);
                    args["to"] = json!(end);
                }
                if x.range == Range::Sections {
                    let ids: Vec<String> = view
                        .sections
                        .iter()
                        .filter(|s| s.on)
                        .map(|s| s.id.to_string())
                        .collect();
                    args["section"] = json!(ids.join(","));
                }
                let command = if x.stems {
                    let (folders, rest) = x.folder_stems(&p);
                    let ids: Vec<String> = rest.iter().map(ToString::to_string).collect();
                    let folders: Vec<String> = folders.iter().map(ToString::to_string).collect();
                    args["what"] = json!("selected");
                    args["ids"] = json!(ids);
                    if !folders.is_empty() {
                        args["folders"] = json!(folders);
                    }
                    "export.stems"
                } else if x.range == Range::Selection && x.fixed_span.is_none() {
                    args["ids"] = json!(
                        self.export_clip_ids()
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                    );
                    "export.selection"
                } else {
                    "export.mixdown"
                };
                self.export_run(command, args);
                self.reload_exports();
            }
            Act::Update(job) => {
                self.export_run("export.update", json!({ "job": job, "background": true }));
                self.reload_exports();
            }
            Act::Reveal(job) => {
                self.export_run("export.reveal_os", json!({ "job": job }));
            }
            Act::Delete(job) => {
                let anchor = self
                    .popup
                    .as_ref()
                    .map(|p| p.anchor)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
                if self.export_confirm_trash(&job, anchor) {
                    return true;
                }
                return self.export_action(Act::DeleteConfirmed(job));
            }
            Act::DeleteConfirmed(job) => {
                if self.export.preview.as_deref() == Some(&job) {
                    self.export_run("export.preview", json!({"job":job,"stop":true}));
                    self.export.preview = None;
                    self.transport.auditioning_export = false;
                }
                self.export_run("export.delete", json!({ "job": job }));
                self.reload_exports();
            }
            Act::Cancel(job) => {
                self.export_run("export.cancel", json!({ "job": job }));
                self.reload_exports();
            }
            Act::Close => {
                self.popup = None;
                return true;
            }
            Act::More(job) => {
                if !x.expanded.contains(&job) {
                    x.expanded.push(job);
                }
            }
        }
        self.refresh_popup();
        true
    }

    /// The arguments of `export.midi` for what the panel shows (UI-280).
    fn midi_args(&self, view: &View) -> Value {
        let x = &self.export;
        let p = self.project();
        let tracks: Vec<String> = x.midi_picks(&p).iter().map(ToString::to_string).collect();
        let mut args = json!({
            "tracks": tracks,
            "each": x.each,
            "name": x.name(),
        });
        match x.range {
            Range::Song => args["range"] = json!("song"),
            Range::Loop => args["range"] = json!("loop"),
            Range::Sections => {
                let ids: Vec<String> = view
                    .sections
                    .iter()
                    .filter(|s| s.on)
                    .map(|s| s.id.to_string())
                    .collect();
                args["range"] = json!("section");
                args["section"] = json!(ids.join(","));
            }
            Range::Selection => {
                let (start, end) = view.span();
                args["from"] = json!(start);
                args["to"] = json!(end);
            }
        }
        args
    }

    /// Asks the session how the running exports are getting on. True while
    /// one runs, so the screen keeps ticking.
    pub(crate) fn stop_export_preview(&mut self) {
        if let Some(job) = self.export.preview.take() {
            let _ = self
                .song
                .run("export.preview", json!({"job":job,"stop":true}));
            self.refresh_popup();
        }
        self.transport.auditioning_export = false;
    }

    pub(crate) fn poll_export(&mut self) -> bool {
        let selection_changed = self
            .popup
            .as_ref()
            .and_then(|p| p.export.as_ref())
            .is_some_and(|v| v.selected_span != self.export_view().selected_span);
        if selection_changed {
            self.refresh_popup();
        }
        let preview = self.export.preview.clone();
        if let Some(job) = &preview
            && let Ok(v) = self
                .song
                .run("export.preview", json!({"job":job,"status":true}))
        {
            self.export.preview_peak = self
                .export
                .preview_peak
                .max(v["peak"].as_f64().unwrap_or(0.0));
            if v["playing"] == false {
                self.export.preview = None;
                self.transport.auditioning_export = false;
                self.refresh_popup();
            }
        }
        if !self.export.running() {
            return preview.is_some() || selection_changed;
        }
        let now = Instant::now();
        if self
            .export
            .polled
            .is_some_and(|t| now.duration_since(t) < POLL)
        {
            return true;
        }
        self.export.polled = Some(now);
        let running: Vec<String> = self
            .export
            .jobs
            .iter()
            .filter(|j| j["state"] == "running")
            .filter_map(|j| j["job"].as_str().map(str::to_string))
            .collect();
        let mut ended = false;
        for id in running {
            if let Ok(v) = self.song.run("export.status", json!({ "job": id })) {
                ended |= v["state"] != "running";
                if let Some(slot) = self
                    .export
                    .jobs
                    .iter_mut()
                    .find(|j| j["job"] == id.as_str())
                {
                    *slot = v;
                }
            }
        }
        if ended {
            self.reload_exports();
        }
        if self
            .popup
            .as_ref()
            .is_some_and(|p| p.kind == PopupKind::Export)
        {
            self.refresh_popup();
        }
        true
    }
}

#[cfg(test)]
#[path = "export_midi_tests.rs"]
mod midi_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::Popup;
    use crate::studio::Geometry;

    fn until(s: &mut Studio, secs: u64, mut done: impl FnMut(&Studio) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            s.tick();
            if done(s) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        false
    }

    fn act(s: &mut Studio, g: &Geometry, a: Act) {
        assert!(s.popup_action(g, Action::Xp(a)));
    }

    fn view(s: &Studio) -> View {
        *s.popup
            .as_ref()
            .and_then(|p| p.export.clone())
            .expect("the panel is open")
    }

    fn live(s: &Studio, g: &Geometry) -> Vec<Act> {
        s.popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .into_iter()
            .filter(|c| c.live())
            .filter_map(|c| match c.action {
                Some(Action::Xp(a)) => Some(a),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_unsaved_song_cannot_be_exported_and_says_why() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        let v = view(&s);
        assert!(!v.has_dir && !v.can_go());
        let cells = s.popup.as_ref().unwrap().layout(g.size).cells;
        assert!(
            cells
                .iter()
                .any(|c| c.text == "書き出すには、先に曲の保存が必要です")
        );
        assert!(!live(&s, &g).contains(&Act::Go));
    }

    #[test]
    fn the_settings_change_what_the_panel_shows() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        act(&mut s, &g, Act::Bits(32));
        act(&mut s, &g, Act::Format(Format::Flac));
        let v = view(&s);
        assert_eq!(v.format, Format::Flac);
        assert_eq!(v.bits, 24, "FLAC has no 32 bit float");
        assert!(!live(&s, &g).contains(&Act::Bits(32)));

        act(&mut s, &g, Act::Mode(true));
        let v = view(&s);
        assert!(v.stems);
        assert_eq!(v.count, v.tracks.len());
        assert!(v.tracks.iter().all(|t| t.on));
        let first = v.tracks[0].id;
        act(&mut s, &g, Act::Pick(first));
        let v = view(&s);
        assert_eq!(v.group, Group::Selected);
        assert_eq!(v.count, v.tracks.len() - 1);
        assert!(!v.tracks[0].on);

        // A stems name has to tell the files apart.
        act(&mut s, &g, Act::EditName);
        s.popup.as_mut().unwrap().naming = Some("mix".into());
        assert!(s.typed(&g, "enter", None));
        let v = view(&s);
        assert_eq!(v.name, "mix");
        assert!(v.bad_name());

        if let Some(sec) = s.project().sections.first() {
            act(&mut s, &g, Act::Section(sec.id));
            let v = view(&s);
            assert_eq!(v.range, Range::Sections);
            assert_eq!(v.span(), (sec.start, sec.start + sec.length));
        }
        let _ = Popup::new(PopupKind::Export, Rect::default());
    }

    fn cells(s: &Studio, g: &Geometry) -> Vec<Cell> {
        s.popup.as_ref().unwrap().layout(g.size).cells
    }

    /// The sample saved in a folder of its own, which goes when the guard is dropped (also
    /// when the test fails).
    fn saved(tag: &str) -> (Studio, std::path::PathBuf, tempfile::TempDir) {
        let dir_tmp = tempfile::Builder::new()
            .prefix(&format!("xirius-export-ui-{tag}-"))
            .tempdir()
            .unwrap();
        let dir = dir_tmp.path().join("song");
        let mut s = Studio::sample();
        s.command(|song| song.save_as(&dir));
        assert!(s.error.is_none(), "{:?}", s.error);
        (s, dir, dir_tmp)
    }

    /// The buttons on that are blue, with the balloon of what they start from.
    fn moved_cells(s: &Studio, g: &Geometry) -> Vec<(String, String)> {
        cells(s, g)
            .into_iter()
            .filter(|c| matches!(c.look, Look::Xp(XpLook::Seg { moved: true, .. })))
            .map(|c| (c.text, c.tip))
            .collect()
    }

    #[test]
    fn only_the_values_off_the_songs_settings_are_blue() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        assert_eq!(view(&s).song_rate, 48_000);
        assert!(
            moved_cells(&s, &g).is_empty(),
            "the panel starts as the song"
        );

        act(&mut s, &g, Act::Rate(44_100));
        assert_eq!(
            moved_cells(&s, &g),
            [("44.1".to_string(), "曲の設定: 48 kHz".to_string())]
        );
        act(&mut s, &g, Act::Bits(16));
        act(&mut s, &g, Act::Format(Format::Flac));
        let mut now = moved_cells(&s, &g);
        now.sort();
        assert_eq!(
            now,
            [
                ("16".to_string(), "曲の設定: 24 bit".to_string()),
                ("44.1".to_string(), "曲の設定: 48 kHz".to_string()),
                ("FLAC".to_string(), "既定: WAV".to_string()),
            ]
        );
        act(&mut s, &g, Act::Rate(48_000));
        act(&mut s, &g, Act::Bits(24));
        act(&mut s, &g, Act::Format(Format::Wav));
        assert!(moved_cells(&s, &g).is_empty(), "back on the song's: plain");
    }

    #[test]
    fn mp3_folds_the_bit_depth_into_bitrates_and_says_why_96_is_out() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        act(&mut s, &g, Act::Rate(96_000));
        act(&mut s, &g, Act::Format(Format::Mp3));
        let v = view(&s);
        assert_eq!(v.format, Format::Mp3);
        assert_eq!(v.rate, 48_000, "MP3 goes up to 48 kHz");
        assert_eq!(v.kbps, mp3::DEFAULT_BITRATE);

        let acts = live(&s, &g);
        for k in mp3::BITRATES {
            assert!(acts.contains(&Act::Kbps(k)), "{k}");
        }
        assert!(!acts.iter().any(|a| matches!(a, Act::Bits(_))));
        assert!(!acts.contains(&Act::Rate(96_000)));
        let all = cells(&s, &g);
        assert!(all.iter().any(|c| c.text == "kbps"));
        assert!(!all.iter().any(|c| c.text == "bit"));
        let i = all.iter().position(|c| c.text == "96").unwrap();
        assert_eq!(all[i].tip, mp3::OVER_48K);
        let r = all[i].rect;
        let l = s.popup.as_ref().unwrap().layout(g.size);
        assert_eq!(
            l.item_at(r.x + r.w / 2.0, r.y + r.h / 2.0),
            Some(i),
            "the pointer finds 96 for its balloon"
        );
        assert!(
            all.iter()
                .any(|c| matches!(c.look, Look::Xp(XpLook::Field { ext: ".mp3", .. })))
        );

        act(&mut s, &g, Act::Kbps(192));
        assert_eq!(view(&s).kbps, 192);

        // Back to WAV, the bit depth comes back and 96 kHz can be chosen again.
        act(&mut s, &g, Act::Format(Format::Wav));
        let acts = live(&s, &g);
        assert!(acts.contains(&Act::Bits(32)) && acts.contains(&Act::Rate(96_000)));
        assert!(!acts.iter().any(|a| matches!(a, Act::Kbps(_))));
        assert!(cells(&s, &g).iter().all(|c| c.tip.is_empty()));
    }

    #[test]
    fn without_the_encoder_only_mp3_is_held_back() {
        let (mut s, dir, _tmp) = saved("nolame");
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        s.export.mp3_error = Some(mp3::CANNOT_LOAD.to_string());
        s.refresh_popup();
        assert!(view(&s).can_go(), "WAV still goes");
        assert!(!cells(&s, &g).iter().any(|c| c.text == mp3::CANNOT_LOAD));

        act(&mut s, &g, Act::Format(Format::Mp3));
        let v = view(&s);
        assert!(v.no_mp3() && !v.can_go());
        assert!(!live(&s, &g).contains(&Act::Go));
        let all = cells(&s, &g);
        let note = all.iter().find(|c| c.text == mp3::CANNOT_LOAD).unwrap();
        assert_eq!(note.look, Look::Xp(XpLook::Alert));
        let go = all
            .iter()
            .find(|c| matches!(c.look, Look::Xp(XpLook::Go { .. })))
            .unwrap();
        assert!(
            note.rect.right() <= go.rect.x,
            "the reason is left of the button"
        );

        act(&mut s, &g, Act::Format(Format::Flac));
        assert!(view(&s).can_go(), "FLAC still goes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_mp3_card_says_it_could_not_convert() {
        let p = Project::new("x");
        let j = json!({
            "job": "j1",
            "command": "export.mixdown",
            "state": "failed",
            "args": { "format": "mp3", "sample_rate": 48000, "bitrate": 320 },
            "error": format!("{}: lame_encode_flush returned -1", mp3::ENCODE_FAILED),
        });
        let card = card_of(&j, &p, None, true);
        assert_eq!(card.state, State::Failed);
        assert_eq!(card.error, mp3::ENCODE_FAILED);
        assert!(
            card.meta.starts_with("MP3 · 48 kHz · 320 kbps"),
            "{}",
            card.meta
        );
    }

    #[test]
    fn an_mp3_card_shows_its_bitrate_and_loudness_over_true_peak() {
        if let Err(why) = mp3::load(None) {
            eprintln!(
                "an_mp3_card_shows_its_bitrate_and_loudness_over_true_peak: lame_enc.dll が見つからないため飛ばす: {why}"
            );
            return;
        }
        let (mut s, dir, _tmp) = saved("mp3");
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        assert!(view(&s).mp3_error.is_none());
        act(&mut s, &g, Act::Format(Format::Mp3));
        act(&mut s, &g, Act::Go);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(until(&mut s, 60, |s| view(s).cards[0].state != State::Running));
        let card = view(&s).cards[0].clone();
        assert_eq!(card.state, State::Done, "{}", card.error);
        assert!(
            card.meta.starts_with("MP3 · 48 kHz · 320 kbps · "),
            "{}",
            card.meta
        );
        let lines: Vec<&str> = card.loudness.lines().collect();
        assert_eq!(lines.len(), 2, "{}", card.loudness);
        assert!(
            lines[0].ends_with("LUFS") && lines[1].ends_with("dBTP"),
            "{lines:?}"
        );
        let path = s.export.jobs[0]["result"]["path"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(path.ends_with(".mp3"), "{path}");
        // The tags it was given, in a balloon over its name.
        let title = format!("タイトル: {}", s.project().name);
        assert_eq!(card.tags, title);
        let over = cells(&s, &g).into_iter().find(|c| c.tip == title).unwrap();
        let l = s.popup.as_ref().unwrap().layout(g.size);
        let at = l.item_at(over.rect.x + 4.0, over.rect.y + over.rect.h / 2.0);
        assert_eq!(at.map(|i| l.cells[i].tip.as_str()), Some(title.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_mixdown_runs_in_the_background_and_its_card_shows_loudness_and_buttons() {
        let dir_tmp = tempfile::Builder::new()
            .prefix("xirius-export-ui-")
            .tempdir()
            .unwrap();
        let dir = dir_tmp.path().join("export-ui");
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Studio::sample();
        s.command(|song| song.save_as(&dir));
        assert!(s.error.is_none(), "{:?}", s.error);
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        assert!(view(&s).can_go());
        act(&mut s, &g, Act::Go);
        assert!(s.error.is_none(), "{:?}", s.error);
        let v = view(&s);
        assert_eq!(v.cards.len(), 1);
        let job = v.cards[0].job.clone();
        let mut seen_running = v.cards[0].state == State::Running;
        assert!(until(&mut s, 30, |s| {
            let c = &view(s).cards[0];
            seen_running |= c.state == State::Running;
            c.state != State::Running
        }));
        let card = view(&s).cards[0].clone();
        assert_eq!(card.state, State::Done, "{}", card.error);
        assert!(card.loudness.contains("dBTP"), "{}", card.loudness);
        assert!(
            card.meta.starts_with("WAV · 48 kHz · 24 bit"),
            "{}",
            card.meta
        );
        assert_eq!(card.stale, Some(false));
        assert!(!s.bar_view().export_stale);
        assert!(seen_running, "the card showed the export while it ran");
        let acts = live(&s, &g);
        assert!(!acts.contains(&Act::Update(job.clone())));
        assert!(acts.contains(&Act::Reveal(job.clone())));
        assert!(acts.contains(&Act::Delete(job.clone())));

        // A change to the song puts the card out of date and shows 更新.
        let t = *s.project().tracks.keys().next().unwrap();
        s.command(|song| song.toggle_mute(t));
        s.refresh_popup();
        assert_eq!(view(&s).cards[0].stale, Some(true));
        assert!(s.bar_view().export_stale);
        assert!(live(&s, &g).contains(&Act::Update(job.clone())));

        // The files go first, so nothing reaches the real trash.
        let job_json = s.export.jobs[0].clone();
        let path = job_json["result"]["path"].as_str().unwrap().to_string();
        std::fs::remove_file(&path).unwrap();
        act(&mut s, &g, Act::Delete(job));
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(view(&s).cards.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changing_the_song_drops_what_was_worked_out_for_the_cards_of_the_old_one() {
        let dir_tmp = tempfile::Builder::new()
            .prefix("xirius-export-base-")
            .tempdir()
            .unwrap();
        let dir = dir_tmp.path().join("export-base");
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Studio::sample();
        s.command(|song| song.save_as(&dir));
        assert!(s.error.is_none(), "{:?}", s.error);
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        act(&mut s, &g, Act::Go);
        assert!(until(&mut s, 30, |s| view(s).cards[0].state != State::Running));
        assert_eq!(view(&s).cards[0].stale, Some(false));
        assert!(
            s.export.stale.lock().unwrap().song.is_some(),
            "the card was compared with the song"
        );

        s.new_song_sync();
        let now = s.project();
        assert!(
            s.export
                .stale
                .lock()
                .unwrap()
                .song
                .as_ref()
                .is_none_or(|p| Arc::ptr_eq(p, &now)),
            "what was worked out for the closed song stays for the next one"
        );
        s.popup = None;
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        assert!(view(&s).cards.is_empty(), "the new song has no exports");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_the_song_again_starts_from_the_range_last_exported() {
        let (mut s, dir, _tmp) = saved("range");
        let g = s.geometry(1600.0, 900.0);
        let sections = s.project().sections.clone();
        assert!(sections.len() >= 2, "the sample song has sections");
        let picked = sections[1].id;
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        assert_eq!(
            view(&s).range,
            Range::Song,
            "a song never exported starts whole"
        );
        act(&mut s, &g, Act::Section(picked));
        act(&mut s, &g, Act::Go);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(until(&mut s, 30, |s| view(s).cards[0].state != State::Running));

        // Another song, whose panel starts whole, and back to this one.
        s.new_song_sync();
        s.popup = None;
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        assert_eq!(view(&s).range, Range::Song);
        s.finish_file_request_sync(crate::studio::FileRequest::Open, &dir);
        assert!(s.error.is_none(), "{:?}", s.error);
        s.popup = None;
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        let v = view(&s);
        assert_eq!(v.range, Range::Sections);
        let on: Vec<_> = v.sections.iter().filter(|c| c.on).map(|c| c.id).collect();
        assert_eq!(on, vec![picked]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_range_a_row_asks_for_is_not_replaced_by_the_one_the_song_remembers() {
        let (mut s, dir, _tmp) = saved("asked");
        let g = s.geometry(1600.0, 900.0);
        s.song
            .run("loop.set", json!({"start": 3840, "end": 7680}))
            .unwrap();
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        act(&mut s, &g, Act::Range(Range::Loop));
        act(&mut s, &g, Act::Go);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(until(&mut s, 30, |s| view(s).cards[0].state != State::Running));
        s.new_song_sync();
        s.popup = None;
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        s.finish_file_request_sync(crate::studio::FileRequest::Open, &dir);
        assert!(s.error.is_none(), "{:?}", s.error);
        s.popup = None;
        // The song remembers the loop; a row that exports a span asks for that span.
        let args = json!({"range": "1.1..2.1"});
        assert_eq!(
            s.screen_command(&g, "view.export", args.as_object().unwrap()),
            Some(true)
        );
        assert_eq!(s.export.range, Range::Selection);
        assert!(s.export.fixed_span.is_some());
        // And it is closed by name (EXP-023), when open.
        let none = serde_json::Map::new();
        assert_eq!(s.screen_command(&g, "view.export.close", &none), Some(true));
        assert!(s.popup.is_none());
        assert_eq!(
            s.screen_command(&g, "view.export.close", &none),
            Some(false)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stems_list_their_files_under_one_card() {
        let dir_tmp = tempfile::Builder::new()
            .prefix("xirius-stems-ui-")
            .tempdir()
            .unwrap();
        let dir = dir_tmp.path().join("stems-ui");
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Studio::sample();
        s.command(|song| song.save_as(&dir));
        let g = s.geometry(1600.0, 900.0);
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        act(&mut s, &g, Act::Mode(true));
        let first = view(&s).tracks[0].id;
        act(&mut s, &g, Act::Group(Group::Selected));
        let keep: Vec<TrackId> = view(&s).tracks.iter().skip(2).map(|t| t.id).collect();
        for id in keep {
            act(&mut s, &g, Act::Pick(id));
        }
        let v = view(&s);
        assert_eq!(v.count, 2);
        assert!(v.tracks[0].on && v.tracks[0].id == first);
        act(&mut s, &g, Act::Go);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(until(&mut s, 30, |s| view(s).cards[0].state != State::Running));
        let card = view(&s).cards[0].clone();
        assert_eq!(card.state, State::Done, "{}", card.error);
        assert!(card.stems);
        assert_eq!(card.title, "ステム 2 本");
        assert_eq!(card.kids.len(), 2);
        assert!(card.kids.iter().all(|k| k.color.is_some()));
        for f in s.export.jobs[0]["result"]["files"].as_array().unwrap() {
            std::fs::remove_file(f["path"].as_str().unwrap()).unwrap();
        }
        act(&mut s, &g, Act::Delete(card.job));
        assert!(s.error.is_none(), "{:?}", s.error);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A finished stems export of `strips`, one file each, as `export.status` lists it.
    fn stems_job(job: &str, strips: &[(String, String)]) -> Value {
        let files: Vec<Value> = strips
            .iter()
            .map(|(id, name)| {
                json!({
                    "name": name,
                    "path": format!("C:/x/song_{name}.wav"),
                    "strip": id,
                    "seconds": 10.0,
                    "lufs": -14.25,
                })
            })
            .collect();
        json!({
            "job": job,
            "state": "done",
            "command": "export.stems",
            "args": { "format": "wav", "sample_rate": 48_000, "bit_depth": 24 },
            "result": { "files": files },
        })
    }

    /// The first `n` tracks of the sample, by id and name.
    fn strips(p: &Project, n: usize) -> Vec<(String, String)> {
        p.tracks
            .iter()
            .take(n)
            .map(|(id, t)| (id.to_string(), t.name.clone()))
            .collect()
    }

    #[test]
    fn each_file_of_stems_says_whether_it_is_stale_and_what_changed_into_it() {
        use xirius_session::export_stale::{Changed, FileStale};
        let s = Studio::sample();
        let p = s.project();
        let st = strips(&p, 3);
        let ((a, a_name), (b, _), (c, c_name)) = (st[0].clone(), st[1].clone(), st[2].clone());
        let j = stems_job("j", &st[..2]);
        let file = |i: usize| {
            j["result"]["files"][i]["path"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let stale = Staleness {
            stale: true,
            song_changed: false,
            range_changed: false,
            changed: vec![
                Changed {
                    id: a.clone(),
                    name: a_name.clone(),
                },
                Changed {
                    id: c.clone(),
                    name: c_name.clone(),
                },
            ],
            files: vec![
                FileStale {
                    name: "a".into(),
                    // The path the record keeps may differ in case only.
                    path: file(0).to_uppercase(),
                    strip: Some(a.clone()),
                    stale: true,
                    tracks: vec![a.clone(), c.clone()],
                },
                FileStale {
                    name: "b".into(),
                    path: file(1),
                    strip: Some(b.clone()),
                    stale: false,
                    tracks: Vec::new(),
                },
            ],
        };
        let card = card_of(&j, &p, Some(&stale), true);
        let color = |id: &str| Some(track_color(&p.tracks[&id.parse::<TrackId>().unwrap()]));
        assert_eq!(card.stale, Some(true));
        assert_eq!(card.old, 1, "one of its files is stale");
        assert_eq!(
            card.dots,
            vec![
                Dot {
                    color: color(&a),
                    name: a_name.clone()
                },
                Dot {
                    color: color(&c),
                    name: c_name.clone()
                },
            ]
        );
        let [one, two] = &card.kids[..] else {
            panic!("{:?}", card.kids)
        };
        assert_eq!(one.file, format!("song_{a_name}.wav"));
        assert_eq!(one.lufs, Some(-14.25));
        assert_eq!(one.stale, Some(true));
        // Its own track is its color bar, not a dot.
        assert_eq!(
            one.dots,
            vec![Dot {
                color: color(&c),
                name: c_name
            }]
        );
        assert_eq!(two.stale, Some(false));
        assert!(two.dots.is_empty());

        // An export not finished says nothing of staleness.
        let mut running = j.clone();
        running["state"] = json!("running");
        let card = card_of(&running, &p, Some(&stale), true);
        assert_eq!(card.stale, None);
        assert!(card.dots.is_empty());
        assert!(
            card.kids
                .iter()
                .all(|k| k.stale.is_none() && k.dots.is_empty())
        );
    }

    #[test]
    fn writing_again_runs_on_the_card_it_replaces_and_the_files_kept_wait() {
        let s = Studio::sample();
        let p = s.project();
        let st = strips(&p, 2);
        let done = stems_job("d", &st);
        let run = json!({
            "job": "r",
            "state": "running",
            "command": "export.stems",
            "args": { "format": "wav", "sample_rate": 48_000, "bit_depth": 24 },
            "replaces": "d",
            "fraction": 0.25,
            "files": [{ "name": st[0].1, "done": 1, "total": 2 }],
        });
        let x = Export {
            jobs: vec![run, done],
            ..Export::default()
        };
        let v = x.view(&p, true);
        assert_eq!(v.cards.len(), 1, "one card, not two");
        let card = &v.cards[0];
        assert_eq!(card.state, State::Running);
        assert_eq!(card.job, "r", "its button stops the one running");
        assert_eq!(card.title, "ステム 2 本");
        assert_eq!(card.kids[0].fraction, Some(0.5));
        assert!(!card.kids[0].wait);
        assert_eq!(card.kids[1].fraction, None);
        assert!(card.kids[1].wait);
        assert_eq!(card.kids[1].file, format!("song_{}.wav", st[1].1));
    }

    #[test]
    fn many_files_fold_under_a_row_that_lists_them_all() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        let p = s.project();
        let names: Vec<(String, String)> = (0..10)
            .map(|i| (p.tracks.keys().next().unwrap().to_string(), format!("s{i}")))
            .collect();
        s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
        s.export.jobs = vec![stems_job("d", &names[..KIDS])];
        s.refresh_popup();
        let card = view(&s).cards[0].clone();
        assert_eq!((card.kids.len(), card.more), (KIDS, 0), "as many as fit");

        s.export.jobs = vec![stems_job("d", &names)];
        s.refresh_popup();
        let card = view(&s).cards[0].clone();
        assert_eq!((card.kids.len(), card.more), (FOLDED, 10 - FOLDED));
        let more = cells(&s, &g)
            .into_iter()
            .find(|c| c.text == "ほか 6 本")
            .expect("the row of the rest");
        assert!(more.live());
        assert_eq!(more.action, Some(Action::Xp(Act::More("d".into()))));
        act(&mut s, &g, Act::More("d".into()));
        let card = view(&s).cards[0].clone();
        assert_eq!((card.kids.len(), card.more), (10, 0));
        assert!(!cells(&s, &g).iter().any(|c| c.text.starts_with("ほか ")));
    }
}
