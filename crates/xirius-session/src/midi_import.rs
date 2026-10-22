//! `midi.import` (UI-076): reads a standard MIDI file's parts, tempo and time signatures, and
//! leaves one `midi.import.place` for the session to run, so one import is one step of the
//! history. Each track of a format 1 file (each channel of a format 0 one) becomes a new track of
//! the instrument, named after the file's track. With `--tempo auto` the song takes the file's
//! tempo and time signatures when it has no clips yet, as `clip.import` does with a file's tempo.

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::{Args, CmdError, CmdResult, ExtContext, Output};

use crate::import::{Plan, disk_path};
use crate::smf::{self, Smf};

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 1] = ["midi.import"];

/// Runs `midi.import`: reads the file and plans the edit.
pub fn run(cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Plan> {
    let p = cx.project.as_ref();
    let file = args.str("file")?;
    let path = disk_path(file, cx.dir)?;
    if !path.is_file() {
        return Err(CmdError::not_found(format!(
            "MIDI ファイルが見つかりません: {}",
            path.display()
        )));
    }
    let bytes =
        std::fs::read(&path).map_err(|e| CmdError::invalid(format!("開けませんでした（{e}）")))?;
    let song = smf::read_song(&bytes, p.ppq).map_err(CmdError::invalid)?;
    if song.parts.is_empty() {
        return Err(CmdError::invalid("ノートがありません"));
    }
    let at = match args.0.get("at") {
        None | Some(Value::Null) => 0,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => p.time_map().parse(s)?,
        Some(_) => return Err(CmdError::invalid("--at は位置で指定してください")),
    };
    if at < 0 {
        return Err(CmdError::range("位置は 0 以上で指定してください"));
    }
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string());

    let mut place = Map::new();
    let parts: Vec<Value> = song
        .parts
        .iter()
        .map(|part| {
            let notes: Vec<Value> = part
                .notes
                .iter()
                .map(|n| {
                    json!({ "at": n.at, "pitch": n.pitch, "length": n.length, "velocity": n.velocity })
                })
                .collect();
            json!({ "name": part.name, "clip": stem, "notes": notes })
        })
        .collect();
    place.insert("parts".into(), json!(parts));
    place.insert("at".into(), json!(at));
    place.insert(
        "instrument".into(),
        json!(args.opt_str("instrument").unwrap_or("Sampler")),
    );
    let takes = takes_tempo(p, args.opt_str("tempo").unwrap_or("auto"));
    let (_, beat, tick) = p.time_map().to_bbt(at);
    let on_bar = (beat, tick) == (1, 0);
    if takes && !song.tempo.is_empty() {
        let tempo: Vec<Value> = song
            .tempo
            .iter()
            .map(|(tick, bpm)| json!({ "tick": tick, "bpm": bpm.clamp(1.0, 999.0) }))
            .collect();
        place.insert("tempo".into(), json!(tempo));
    }
    if takes && on_bar && !song.meter.is_empty() {
        place.insert("meter".into(), json!(bars(&song, p.ppq)));
    }

    let mut msg = format!(
        "「{name}」をインポートしました（{} トラック）",
        song.parts.len()
    );
    if dry {
        msg = format!("「{name}」をインポートします");
    }
    let data = json!({
        "file": path.display().to_string(),
        "at": at,
        "tracks": song.parts.len(),
        "tempo": place.get("tempo"),
        "meter": place.get("meter"),
    });
    Ok(Plan {
        output: Output::with(msg, data),
        edits: if dry {
            Vec::new()
        } else {
            vec![("midi.import.place".into(), place)]
        },
    })
}

/// Whether the song takes the file's tempo and meter: `file` always, `keep` never, `auto` when
/// the song has no clips yet.
fn takes_tempo(p: &Project, how: &str) -> bool {
    match how {
        "file" => true,
        "keep" => false,
        _ => p.tracks.values().all(|t| t.clips.is_empty()),
    }
}

/// The file's time signatures by bar from 1, where a change between bar lines moves to the next
/// bar line. Before the first one the file is in 4/4.
fn bars(song: &Smf, ppq: u32) -> Vec<Value> {
    let (mut at, mut bar, mut len) = (0i64, 1u32, i64::from(ppq) * 4);
    let mut out = Vec::new();
    for &(tick, num, den) in &song.meter {
        let whole = ((tick - at).max(0) + len - 1) / len;
        bar += u32::try_from(whole).unwrap_or(0);
        at += whole * len;
        len = (i64::from(ppq) * 4 / i64::from(den.max(1)) * i64::from(num)).max(1);
        out.push(json!({ "bar": bar, "num": num, "den": den }));
    }
    out
}
