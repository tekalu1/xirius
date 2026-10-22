//! Which exported files the song has moved away from (EXP-019, EXP-020).
//!
//! An export keeps in its record ([`crate::jobs`]) a fingerprint of every track and bus that
//! plays into each file it writes, with the range it covered and the song's tempo, metre and
//! settings ([`base`]). [`check`] compares those with the song as it is now: a file is stale
//! when a channel that plays into it sounds different, when one is gone or newly plays into it,
//! when the range comes out differently, or when the tempo, metre or settings changed. The
//! record is on disk, so a song opened again knows its stale files, and a change undone makes
//! them current again.
//!
//! What a channel is called, its colour and its icon are not part of its sound, nor are its
//! input, arming and monitoring (an export does not record). A track that could hold clips but
//! holds none plays nothing, whatever its settings.
//!
//! What the song does not show (a plugin's own window) is told with `export.touch` (EXP-023):
//! the files the tracks named play into are stale from then on ([`touch`]), kept in the
//! record's `touched` until they are written again.
//!
//! A MIDI file (EXP-026) keeps fingerprints of the notes of its tracks instead
//! ([`crate::midi_export::base`]): what a track sounds like does not change its notes, and a
//! file holds only the tracks chosen for it.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value, json};
use xirius_core::Args;
use xirius_core::id::TrackId;
use xirius_core::model::{Project, Track, TrackKind};

use crate::export::resolve_range;
use crate::graph::feeding;

/// The fingerprint of a track that plays nothing.
const QUIET: &str = "0";

/// Where the file of a mix is kept in [`base`]'s `files`.
const MIX: &str = "mix";

/// Fields of a track that do not change what it sounds like.
const NOT_SOUND: [&str; 8] = [
    "name",
    "color",
    "icon",
    "armed",
    "monitor",
    "input",
    "input_gain_db",
    "preset",
];

/// FNV-1a, 64 bit: the same in every process and every run.
pub(crate) fn hash(v: &Value) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in v.to_string().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn plays_nothing(t: &Track) -> bool {
    t.has_clips() && t.clips.is_empty()
}

/// What a track sounds like, as a fingerprint: its settings, plugins, clips and automation,
/// where it plays to and the fades of its clips.
fn track_print(p: &Project, t: &Track) -> String {
    if plays_nothing(t) {
        return QUIET.to_string();
    }
    let Ok(Value::Object(mut v)) = serde_json::to_value(t) else {
        return QUIET.to_string();
    };
    for k in NOT_SOUND {
        v.remove(k);
    }
    if let Some(Value::Array(clips)) = v.get_mut("clips") {
        for c in clips.iter_mut().filter_map(Value::as_object_mut) {
            c.remove("name");
            c.remove("color");
        }
    }
    v.insert(
        "out".into(),
        json!(p.effective_output(t.id).map(|o| o.to_string())),
    );
    let fx: Map<String, Value> = t
        .clips
        .iter()
        .filter_map(|c| {
            let fx = p.clip_fx.get(&c.id)?;
            Some((c.id.to_string(), serde_json::to_value(fx).ok()?))
        })
        .collect();
    if !fx.is_empty() {
        v.insert("clip_fx".into(), Value::Object(fx));
    }
    hash(&Value::Object(v))
}

/// What every channel of the song shares: tempo, metre and the song's settings.
fn song_print(p: &Project) -> String {
    hash(&json!({
        "ppq": p.ppq,
        "tempo": p.tempo,
        "timesig": p.timesig,
        "settings": p.settings,
    }))
}

/// The fingerprints of a song, made once and compared with any number of records.
pub struct Prints {
    song: String,
    tracks: HashMap<TrackId, String>,
}

pub fn prints(p: &Project) -> Prints {
    Prints {
        song: song_print(p),
        tracks: p
            .tracks
            .values()
            .map(|t| (t.id, track_print(p, t)))
            .collect(),
    }
}

/// The channels that play into the file of `strip` (the mix when `None`), in id order.
fn sources(p: &Project, strip: Option<TrackId>) -> Vec<TrackId> {
    let mut v: Vec<TrackId> = match strip {
        None => p.tracks.keys().copied().collect(),
        Some(s) => feeding(p, s).into_iter().collect(),
    };
    v.sort();
    v
}

/// What an export keeps to tell later which of its files are stale: the range in ticks, and
/// for the mix (`strips` `None`) or each stem the channels that play into it with their
/// fingerprints.
pub fn base(p: &Project, span: (i64, i64), strips: Option<&[TrackId]>) -> Value {
    let pr = prints(p);
    let keyed: Vec<(String, Vec<TrackId>)> = match strips {
        None => vec![(MIX.to_string(), sources(p, None))],
        Some(s) => s
            .iter()
            .map(|id| (id.to_string(), sources(p, Some(*id))))
            .collect(),
    };
    let mut prints_of = BTreeMap::new();
    let mut names = BTreeMap::new();
    for id in keyed.iter().flat_map(|(_, ids)| ids) {
        if let (Some(t), Some(h)) = (p.tracks.get(id), pr.tracks.get(id)) {
            prints_of.insert(id.to_string(), json!(h));
            names.insert(id.to_string(), json!(t.shown_name()));
        }
    }
    let files: Map<String, Value> = keyed
        .into_iter()
        .map(|(k, ids)| {
            let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
            (k, json!(ids))
        })
        .collect();
    json!({
        "span": [span.0, span.1],
        "song": pr.song,
        "strips": prints_of,
        "names": names,
        "files": files,
    })
}

/// Carries into `base` what the files of `keys` (strips of a stems export, kept as they were
/// by a partial `export.update`) were made from in `old`, the base of the export before.
pub fn keep(base: &mut Value, old: &Value, keys: &[String]) {
    for key in keys {
        let ids: Vec<String> = old["files"][key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let held: Map<String, Value> = ids
            .iter()
            .map(|id| {
                let print = old["held"][key][id]
                    .as_str()
                    .or(old["strips"][id].as_str())
                    .unwrap_or(QUIET);
                (id.clone(), json!(print))
            })
            .collect();
        base["files"][key] = json!(ids);
        base["held"][key] = Value::Object(held);
        for id in &ids {
            if base["names"].get(id).is_none()
                && let Some(n) = old["names"].get(id)
            {
                base["names"][id] = n.clone();
            }
        }
    }
}

/// A channel that changed since an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub id: String,
    pub name: String,
}

/// One written file and whether it is stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStale {
    pub name: String,
    pub path: String,
    /// The stem's track or bus; `None` for a mix.
    pub strip: Option<String>,
    pub stale: bool,
    /// The channels changed that play into this file.
    pub tracks: Vec<String>,
}

/// How an export's files stand against the song as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staleness {
    pub stale: bool,
    /// The tempo, the metre or the song's settings changed: every file is stale.
    pub song_changed: bool,
    /// The range comes out differently now (or cannot be found): every file is stale.
    pub range_changed: bool,
    /// The channels changed, in id order.
    pub changed: Vec<Changed>,
    pub files: Vec<FileStale>,
}

impl Staleness {
    /// Adds `stale`, `stale_tracks`, `stale_files`, `song_changed` and `range_changed` to a
    /// record.
    pub fn write_to(&self, record: &mut Value) {
        let Some(m) = record.as_object_mut() else {
            return;
        };
        m.insert("stale".into(), json!(self.stale));
        m.insert("song_changed".into(), json!(self.song_changed));
        m.insert("range_changed".into(), json!(self.range_changed));
        m.insert(
            "stale_tracks".into(),
            json!(
                self.changed
                    .iter()
                    .map(|c| json!({ "id": c.id, "name": c.name }))
                    .collect::<Vec<_>>()
            ),
        );
        m.insert(
            "stale_files".into(),
            json!(
                self.files
                    .iter()
                    .map(|f| json!({
                        "name": f.name,
                        "path": f.path,
                        "strip": f.strip,
                        "stale": f.stale,
                        "tracks": f.tracks,
                    }))
                    .collect::<Vec<_>>()
            ),
        );
    }
}

/// The range a record's arguments cover in the song as it is now.
fn span_now(p: &Project, record: &Value) -> Option<(i64, i64)> {
    let args = record["args"].as_object()?.clone();
    let o = crate::commands::options(p, None, &Args(args)).ok()?;
    resolve_range(p, &o.range).ok()
}

/// Where the exported range began: the kept range, else the record's range in the song as it
/// is now, else the song's head.
pub fn head(p: &Project, record: &Value) -> i64 {
    record["base"]["span"][0]
        .as_i64()
        .or_else(|| span_now(p, record).map(|s| s.0))
        .unwrap_or(0)
}

/// The files a finished record wrote: name, path and the strip of a stem.
fn written(record: &Value) -> Vec<(String, String, Option<String>)> {
    let r = &record["result"];
    match r["files"].as_array() {
        Some(files) => files
            .iter()
            .map(|f| {
                (
                    f["name"].as_str().unwrap_or_default().to_string(),
                    f["path"].as_str().unwrap_or_default().to_string(),
                    f["strip"].as_str().map(str::to_string),
                )
            })
            .collect(),
        None => r["path"]
            .as_str()
            .map(|path| ("mixdown".to_string(), path.to_string(), None))
            .into_iter()
            .collect(),
    }
}

/// How the files of a finished export stand against `p`; `None` for a record that did not
/// finish or that has no fingerprints (made before they were kept).
pub fn check(p: &Project, pr: &Prints, record: &Value) -> Option<Staleness> {
    if record["state"] != "done" {
        return None;
    }
    let base = record.get("base")?.as_object()?;
    let span: Option<(i64, i64)> = base["span"]
        .as_array()
        .and_then(|a| Some((a.first()?.as_i64()?, a.get(1)?.as_i64()?)));
    let range_changed = span.is_none() || span_now(p, record) != span;
    let midi = crate::midi_export::is_midi(base);
    let song_now = if midi {
        crate::midi_export::song_print(p)
    } else {
        pr.song.clone()
    };
    let song_changed = base["song"].as_str() != Some(song_now.as_str());
    let print_now = |t: TrackId| {
        if midi {
            p.tracks.get(&t).map(|t| crate::midi_export::track_print(t))
        } else {
            pr.tracks.get(&t).cloned()
        }
    };
    // A file kept as it was by a partial `export.update` holds the prints it was made from.
    let stored = |key: &str, id: &str| {
        base.get("held")
            .and_then(|h| h[key][id].as_str())
            .or(base["strips"][id].as_str())
            .unwrap_or(QUIET)
            .to_string()
    };
    let mut all: Vec<String> = Vec::new();
    let mut files = Vec::new();
    for (name, path, strip) in written(record) {
        let key = strip.clone().unwrap_or_else(|| MIX.to_string());
        let mut tracks: Vec<String> = Vec::new();
        for id in touched_of(record, &key) {
            if !tracks.contains(&id) {
                tracks.push(id);
            }
        }
        let then: Option<Vec<String>> = base["files"][&key].as_array().map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        });
        if let Some(then) = &then {
            for id in then {
                let now = id
                    .parse::<TrackId>()
                    .ok()
                    .and_then(print_now)
                    .unwrap_or_else(|| QUIET.to_string());
                if now != stored(&key, id) && !tracks.contains(id) {
                    tracks.push(id.clone());
                }
            }
            let target = strip.as_deref().and_then(|s| s.parse::<TrackId>().ok());
            // A MIDI file holds the tracks chosen for it, and no other.
            if !midi && (strip.is_none() || target.is_some_and(|t| p.tracks.contains_key(&t))) {
                for id in sources(p, target) {
                    let key = id.to_string();
                    let plays = p.tracks.get(&id).is_some_and(|t| {
                        matches!(t.kind, TrackKind::Audio | TrackKind::Instrument)
                    });
                    if plays
                        && !then.contains(&key)
                        && !tracks.contains(&key)
                        && pr.tracks.get(&id).is_some_and(|h| h != QUIET)
                    {
                        tracks.push(key);
                    }
                }
            }
        }
        tracks.sort_by_key(|id| id.parse::<TrackId>().ok());
        for id in &tracks {
            if !all.contains(id) {
                all.push(id.clone());
            }
        }
        files.push(FileStale {
            name,
            path,
            strip,
            stale: range_changed || song_changed || then.is_none() || !tracks.is_empty(),
            tracks,
        });
    }
    all.sort_by_key(|id| id.parse::<TrackId>().ok());
    let changed = all
        .into_iter()
        .map(|id| {
            let name = id
                .parse::<TrackId>()
                .ok()
                .and_then(|t| p.tracks.get(&t))
                .map(|t| t.shown_name().to_string())
                .or_else(|| base["names"][&id].as_str().map(str::to_string))
                .unwrap_or_else(|| id.clone());
            Changed { id, name }
        })
        .collect();
    Some(Staleness {
        stale: files.iter().any(|f| f.stale),
        song_changed,
        range_changed,
        changed,
        files,
    })
}

/// The tracks told edited (`export.touch`) since the file of `key` was written.
fn touched_of(record: &Value, key: &str) -> Vec<String> {
    record["touched"][key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The `touched` of a finished record with fingerprints once `ids` are told edited: each is
/// added to the files it plays into, now or when they were written. `None` when no file of
/// the record is new to any of them.
pub fn touch(p: &Project, record: &Value, ids: &[TrackId]) -> Option<Map<String, Value>> {
    if record["state"] != "done" {
        return None;
    }
    let base = record.get("base")?.as_object()?;
    // What the song does not show is in the sound, not in the notes.
    if crate::midi_export::is_midi(base) {
        return None;
    }
    let mut touched = record["touched"].as_object().cloned().unwrap_or_default();
    let mut added = false;
    for (_, _, strip) in written(record) {
        let key = strip.clone().unwrap_or_else(|| MIX.to_string());
        let then: Vec<String> = base["files"][&key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let target = strip.as_deref().and_then(|s| s.parse::<TrackId>().ok());
        let now: Vec<TrackId> =
            if strip.is_none() || target.is_some_and(|t| p.tracks.contains_key(&t)) {
                sources(p, target)
            } else {
                Vec::new()
            };
        let mut list = touched_of(record, &key);
        for id in ids {
            let s = id.to_string();
            if (then.contains(&s) || now.contains(id)) && !list.contains(&s) {
                list.push(s);
                added = true;
            }
        }
        if !list.is_empty() {
            list.sort_by_key(|id| id.parse::<TrackId>().ok());
            touched.insert(key, json!(list));
        }
    }
    added.then_some(touched)
}

/// Carries into a record the tracks told edited for the files `keys` kept as they were by a
/// partial `export.update`, from `old`, the record before.
pub fn keep_touched(old: &Value, keys: &[String]) -> Map<String, Value> {
    keys.iter()
        .filter_map(|k| Some((k.clone(), old["touched"].get(k)?.clone())))
        .collect()
}

/// Adds to each finished record with fingerprints how its files stand against `p`.
pub fn annotate(p: &Project, records: &mut [Value]) {
    let pr = prints(p);
    for v in records {
        if let Some(st) = check(p, &pr, v) {
            st.write_to(v);
        }
    }
}
