//! Stable object ids.
//!
//! Every object gets an id from one per-project counter, so an id is never
//! reused for another object (not even after undo). Ids are written as
//! `<prefix>_<n>` (`trk_12`, `nte_7`); the master channel is always `master`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u64);

        impl $name {
            pub const PREFIX: &'static str = $prefix;
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if $prefix == "trk" && self.0 == 0 {
                    return f.write_str("master");
                }
                write!(f, "{}_{}", $prefix, self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }

        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, String> {
                if $prefix == "trk" && s == "master" {
                    return Ok($name(0));
                }
                let rest = s
                    .strip_prefix(concat!($prefix, "_"))
                    .ok_or_else(|| format!("`{s}` は {} の ID ではありません（{}_<番号> の形式で指定してください）", $prefix, $prefix))?;
                rest.parse::<u64>()
                    .map($name)
                    .map_err(|_| format!("`{s}` の番号を読み取れません"))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

id_type!(
    /// A channel: audio track, instrument track, bus or the master.
    TrackId,
    "trk"
);
id_type!(FolderId, "fld");
id_type!(ClipId, "clp");
id_type!(NoteId, "nte");
id_type!(PluginId, "plg");
id_type!(SendId, "snd");
id_type!(SectionId, "sec");
id_type!(MarkerId, "mrk");
id_type!(TempoId, "tmp");
id_type!(TimeSigId, "tsg");

impl TrackId {
    /// The master channel. Its id is fixed so scripts can say `master`.
    pub const MASTER: TrackId = TrackId(0);
}

impl TrackId {
    pub fn is_master(self) -> bool {
        self == Self::MASTER
    }
}

/// An id of any kind, used where a command accepts several (selection,
/// `track.move`, `project.get`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum AnyId {
    Track(TrackId),
    Folder(FolderId),
    Clip(ClipId),
    Note(NoteId),
    Plugin(PluginId),
    Send(SendId),
    Section(SectionId),
    Marker(MarkerId),
    Tempo(TempoId),
    TimeSig(TimeSigId),
}

impl fmt::Display for AnyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnyId::Track(i) => fmt::Display::fmt(i, f),
            AnyId::Folder(i) => fmt::Display::fmt(i, f),
            AnyId::Clip(i) => fmt::Display::fmt(i, f),
            AnyId::Note(i) => fmt::Display::fmt(i, f),
            AnyId::Plugin(i) => fmt::Display::fmt(i, f),
            AnyId::Send(i) => fmt::Display::fmt(i, f),
            AnyId::Section(i) => fmt::Display::fmt(i, f),
            AnyId::Marker(i) => fmt::Display::fmt(i, f),
            AnyId::Tempo(i) => fmt::Display::fmt(i, f),
            AnyId::TimeSig(i) => fmt::Display::fmt(i, f),
        }
    }
}

impl FromStr for AnyId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        if s == "master" {
            return Ok(AnyId::Track(TrackId::MASTER));
        }
        let prefix = s.split('_').next().unwrap_or("");
        Ok(match prefix {
            TrackId::PREFIX => AnyId::Track(s.parse()?),
            FolderId::PREFIX => AnyId::Folder(s.parse()?),
            ClipId::PREFIX => AnyId::Clip(s.parse()?),
            NoteId::PREFIX => AnyId::Note(s.parse()?),
            PluginId::PREFIX => AnyId::Plugin(s.parse()?),
            SendId::PREFIX => AnyId::Send(s.parse()?),
            SectionId::PREFIX => AnyId::Section(s.parse()?),
            MarkerId::PREFIX => AnyId::Marker(s.parse()?),
            TempoId::PREFIX => AnyId::Tempo(s.parse()?),
            TimeSigId::PREFIX => AnyId::TimeSig(s.parse()?),
            _ => {
                return Err(format!(
                    "`{s}` は ID の形式ではありません（trk_1・clp_2・master など）"
                ));
            }
        })
    }
}

impl Serialize for AnyId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AnyId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// What a command argument that holds ids may point at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdKind {
    Track,
    Folder,
    /// A track or a folder (the things in the track column).
    Node,
    Clip,
    Note,
    /// A note, or a clip (meaning all its notes).
    NoteScope,
    /// What a copy takes: notes, clips (meaning all their notes) or tracks.
    Copyable,
    Plugin,
    Send,
    Section,
    Marker,
    Tempo,
    TimeSig,
    Any,
}

impl IdKind {
    pub fn label(self) -> &'static str {
        match self {
            IdKind::Track => "track",
            IdKind::Folder => "folder",
            IdKind::Node => "track|folder",
            IdKind::Clip => "clip",
            IdKind::Note => "note",
            IdKind::NoteScope => "note|clip",
            IdKind::Copyable => "note|clip|track",
            IdKind::Plugin => "plugin",
            IdKind::Send => "send",
            IdKind::Section => "section",
            IdKind::Marker => "marker",
            IdKind::Tempo => "tempo",
            IdKind::TimeSig => "timesig",
            IdKind::Any => "any",
        }
    }

    pub fn accepts(self, id: &AnyId) -> bool {
        matches!(
            (self, id),
            (IdKind::Any, _)
                | (IdKind::Track, AnyId::Track(_))
                | (IdKind::Folder, AnyId::Folder(_))
                | (IdKind::Node, AnyId::Track(_) | AnyId::Folder(_))
                | (IdKind::Clip, AnyId::Clip(_))
                | (IdKind::Note, AnyId::Note(_))
                | (IdKind::NoteScope, AnyId::Note(_) | AnyId::Clip(_))
                | (
                    IdKind::Copyable,
                    AnyId::Note(_) | AnyId::Clip(_) | AnyId::Track(_)
                )
                | (IdKind::Plugin, AnyId::Plugin(_))
                | (IdKind::Send, AnyId::Send(_))
                | (IdKind::Section, AnyId::Section(_))
                | (IdKind::Marker, AnyId::Marker(_))
                | (IdKind::Tempo, AnyId::Tempo(_))
                | (IdKind::TimeSig, AnyId::TimeSig(_))
        )
    }
}

/// A reference to something in the track column.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Node {
    Track(TrackId),
    Folder(FolderId),
}

impl Node {
    pub fn any(self) -> AnyId {
        match self {
            Node::Track(t) => AnyId::Track(t),
            Node::Folder(f) => AnyId::Folder(f),
        }
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.any(), f)
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.parse::<AnyId>().map_err(serde::de::Error::custom)? {
            AnyId::Track(t) => Ok(Node::Track(t)),
            AnyId::Folder(f) => Ok(Node::Folder(f)),
            other => Err(serde::de::Error::custom(format!(
                "{other} はトラックの順序に含められません"
            ))),
        }
    }
}
