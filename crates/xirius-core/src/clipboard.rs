//! What `edit.copy` and `edit.cut` keep for `edit.paste` (CTX-028, KEY-003): notes, clips and
//! the points of a controller lane or an automation lane.
//!
//! The session holds one clipboard, outside the history. Every position is a song position, so a
//! paste can put things back where they were (`--at original`), or move the first of them to a
//! place and keep the rest in step.

use crate::auto::{AutoPoint, AutoTarget};
use crate::id::{ClipId, SectionId, TrackId};
use crate::model::{AuxSend, Clip, CtrlKind, CtrlPoint, Note, Plugin};
use crate::time::Tick;

/// A note and the clip it was taken from; `note.start` is the song position.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteCopy {
    pub clip: ClipId,
    pub note: Note,
}

/// A clip and the track it was taken from.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipCopy {
    pub track: TrackId,
    pub clip: Clip,
}

/// An effect and the channel it was taken from.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginCopy {
    pub track: TrackId,
    pub plugin: Plugin,
}

/// A channel's settings: the effects (the instrument is the channel's own and stays out), the
/// sends, the fader and the pan (MIX-007).
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelCopy {
    pub source: TrackId,
    /// Taken from the master: it has no sends and no pan to hand on.
    pub master: bool,
    pub inserts: Vec<Plugin>,
    pub sends: Vec<AuxSend>,
    pub volume_db: f64,
    pub pan: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Clipboard {
    Tracks {
        source: std::sync::Arc<crate::model::Project>,
        ids: Vec<TrackId>,
    },
    /// A section with all that lies in it (the song as it was when it was copied).
    Section {
        source: std::sync::Arc<crate::model::Project>,
        id: SectionId,
    },
    Notes(Vec<NoteCopy>),
    Clips(Vec<ClipCopy>),
    /// Effects of a channel's inserts; they have no place in time.
    Plugins(Vec<PluginCopy>),
    /// A channel's effects, sends, fader and pan.
    Channel(ChannelCopy),
    /// Points of one lane of a MIDI clip; `tick` is the song position.
    Controller {
        clip: ClipId,
        kind: CtrlKind,
        points: Vec<CtrlPoint>,
    },
    Automation {
        track: TrackId,
        target: AutoTarget,
        points: Vec<AutoPoint>,
    },
}

impl Clipboard {
    /// The word that tells the kinds apart in a command's answer.
    pub fn kind(&self) -> &'static str {
        match self {
            Clipboard::Tracks { .. } => "tracks",
            Clipboard::Section { .. } => "section",
            Clipboard::Notes(_) => "notes",
            Clipboard::Clips(_) => "clips",
            Clipboard::Plugins(_) => "plugins",
            Clipboard::Channel(_) => "channel",
            Clipboard::Controller { .. } => "controller",
            Clipboard::Automation { .. } => "automation",
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Clipboard::Tracks { ids, .. } => ids.len(),
            Clipboard::Section { .. } => 1,
            Clipboard::Notes(v) => v.len(),
            Clipboard::Clips(v) => v.len(),
            Clipboard::Plugins(v) => v.len(),
            Clipboard::Channel(_) => 1,
            Clipboard::Controller { points, .. } => points.len(),
            Clipboard::Automation { points, .. } => points.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The song position of the earliest thing.
    pub fn first_tick(&self) -> Tick {
        self.ends().0
    }

    /// The song position the last thing ends at (a point ends where it starts).
    pub fn last_tick(&self) -> Tick {
        self.ends().1
    }

    fn ends(&self) -> (Tick, Tick) {
        let spans: Vec<(Tick, Tick)> = match self {
            Clipboard::Tracks { .. } => Vec::new(),
            Clipboard::Section { source, id } => source
                .sections
                .iter()
                .filter(|s| s.id == *id)
                .map(|s| (s.start, s.start + s.length))
                .collect(),
            Clipboard::Notes(v) => v
                .iter()
                .map(|c| (c.note.start, c.note.start + c.note.length))
                .collect(),
            Clipboard::Clips(v) => v.iter().map(|c| (c.clip.start, c.clip.end())).collect(),
            Clipboard::Plugins(_) | Clipboard::Channel(_) => Vec::new(),
            Clipboard::Controller { points, .. } => {
                points.iter().map(|p| (p.tick, p.tick)).collect()
            }
            Clipboard::Automation { points, .. } => {
                points.iter().map(|p| (p.tick, p.tick)).collect()
            }
        };
        let first = spans.iter().map(|s| s.0).min().unwrap_or(0);
        let last = spans.iter().map(|s| s.1).max().unwrap_or(first);
        (first, last)
    }

    /// What a message says it holds (「ノート 3 個」).
    pub fn describe(&self) -> String {
        let n = self.len();
        match self {
            Clipboard::Tracks { .. } => format!("トラック {n} 本"),
            Clipboard::Section { source, id } => {
                source.sections.iter().find(|s| s.id == *id).map_or_else(
                    || "セクション".into(),
                    |s| format!("セクション「{}」", s.name),
                )
            }
            Clipboard::Notes(_) => format!("ノート {n} 個"),
            Clipboard::Clips(_) => format!("クリップ {n} 件"),
            Clipboard::Plugins(_) => format!("プラグイン {n} 個"),
            Clipboard::Channel(_) => "チャンネルの設定".to_string(),
            Clipboard::Controller { kind, .. } => {
                format!("コントローラー {} のポイント {n} 個", kind.key())
            }
            Clipboard::Automation { target, .. } => {
                format!("{target} のポイント {n} 個")
            }
        }
    }
}
