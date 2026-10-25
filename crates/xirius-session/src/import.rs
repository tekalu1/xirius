//! `clip.import` (UI-075, UI-017, UI-046, ENG-031): reads an audio file's length and the tempo it
//! carries, and leaves one `clip.import.place` for the session to run, so one import is one step
//! of the history.
//!
//! A file from outside a saved song is copied into the song's media folder (`Media/Imports`)
//! when the song's `copy_imports` is on (BRW-004), and the clip names the copy; otherwise the
//! file stays where it is (BRW-003: an unsaved song collects it when it is saved). A file inside
//! the song folder is referred to relative to it, any other by its full path. Its length in the song is its length in seconds at the song's tempo; nothing is
//! stretched (ENG-031). With `--tempo auto` a file that carries a tempo (an ACID chunk, or
//! `92bpm` in its name) sets the song's when the song has no clips yet.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_core::model::{Media, Project};
use xirius_core::session::{copy_into, is_external, song_name};
use xirius_core::{Args, CmdError, CmdResult, ExtContext, Output};

use crate::decode::{self, Info};
use crate::tempo::TempoMap;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 1] = ["clip.import"];

/// What `clip.import` found and what it leaves for the song.
pub struct Plan {
    pub output: Output,
    pub edits: Vec<(String, Map<String, Value>)>,
}

/// The file as the song refers to it: relative to `dir` when inside it (with `/`), else full.
pub fn song_path(file: &Path, dir: Option<&Path>) -> String {
    if let Some(dir) = dir
        && let Ok(rel) = file.strip_prefix(dir)
    {
        return rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
    }
    file.to_string_lossy().into_owned()
}

/// `file` as a path on the disk: relative ones are in the song folder.
pub fn disk_path(file: &str, dir: Option<&Path>) -> CmdResult<PathBuf> {
    let p = Path::new(file.trim());
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    match dir {
        Some(dir) => Ok(dir.join(p)),
        None => Err(CmdError::invalid(format!(
            "保存前の曲では、音声ファイルを絶対パスで指定してください（{file}）"
        ))),
    }
}

/// The length of `secs` seconds placed at `at`, in ticks of `p`'s tempo (or of `bpm` when the
/// import sets the tempo).
pub fn length_ticks(p: &Project, at: i64, secs: f64, bpm: Option<f64>) -> i64 {
    let ticks = match bpm {
        Some(bpm) => (secs * bpm / 60.0 * f64::from(p.ppq)).round() as i64,
        None => {
            let map = TempoMap::new(p.ppq, &p.tempo);
            map.tick_at(map.seconds_at(at) + secs) - at
        }
    };
    ticks.max(1)
}

fn show_bpm(bpm: f64) -> String {
    let r = (bpm * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

fn song_bpm(p: &Project) -> f64 {
    p.tempo
        .iter()
        .find(|t| t.tick == 0)
        .map_or(120.0, |t| t.bpm)
}

/// Whether the file's tempo becomes the song's.
fn takes_tempo(p: &Project, how: &str, info: &Info) -> Option<f64> {
    let bpm = info.bpm?;
    let empty = p.tracks.values().all(|t| t.clips.is_empty());
    let apply = match how {
        "file" => true,
        "keep" => false,
        _ => empty,
    };
    (apply && (bpm - song_bpm(p)).abs() > 1e-6).then_some(bpm)
}

/// Runs `clip.import`: reads the file and plans the edit.
pub fn run(cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Plan> {
    let p = cx.project.as_ref();
    let file = args.str("file")?;
    let path = disk_path(file, cx.dir)?;
    #[cfg(feature = "test-io")]
    xirius_core::io_test::before("import_source", &path)
        .map_err(|e| CmdError::io(e.to_string()))?;
    if !path.is_file() {
        return Err(CmdError::not_found(format!(
            "音声ファイルが見つかりません: {}",
            path.display()
        )));
    }
    #[cfg(feature = "test-io")]
    xirius_core::io_test::before("import_probe", &path).map_err(|e| CmdError::io(e.to_string()))?;
    let info = decode::probe(&path).map_err(CmdError::invalid)?;
    let at = match args.0.get("at") {
        None | Some(Value::Null) => 0,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => p.time_map().parse(s)?,
        Some(_) => return Err(CmdError::invalid("--at は位置で指定してください")),
    };
    if at < 0 {
        return Err(CmdError::range("位置は 0 以上で指定してください"));
    }
    if let Some(track) = args.opt_str("track")
        && args.flag("new-track")
    {
        return Err(CmdError::invalid(format!(
            "--track（{track}）と --new-track は同時に指定できません"
        )));
    }
    let how = args.opt_str("tempo").unwrap_or("auto");
    let bpm = takes_tempo(p, how, &info);
    let length = length_ticks(p, at, info.seconds(), bpm);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string());

    // A saved song that copies what it reads takes the file in now.
    let mut copied = None;
    let mut path = path;
    if let Some(dir) = cx.dir
        && !dry
        && p.media.copy_imports
        && is_external(&p.media, dir, &path.to_string_lossy())
    {
        #[cfg(feature = "test-io")]
        xirius_core::io_test::before("import_copy", &path)
            .map_err(|e| CmdError::io(e.to_string()))?;
        let dest = copy_into(&p.media.place(dir, Media::IMPORTS), &path, &[])?;
        copied = Some(song_name(&dest, dir));
        path = dest;
    }
    let mut place = Map::new();
    let named = match &copied {
        Some(c) => c.clone(),
        None => song_path(&path, cx.dir),
    };
    place.insert("file".into(), json!(named));
    place.insert("length".into(), json!(length));
    place.insert("at".into(), json!(at));
    for key in ["track", "name", "into"] {
        if let Some(v) = args.opt_str(key) {
            place.insert(key.into(), json!(v));
        }
    }
    if let Some(i) = args.opt_i64("index") {
        place.insert("index".into(), json!(i));
    }
    if let Some(bpm) = bpm {
        place.insert("bpm".into(), json!(bpm));
    }
    // The clip knows the file's tempo even when the song keeps its own, so it can follow the
    // song's later (EDT-007).
    if let Some(tempo) = info.bpm.filter(|b| (1.0..=999.0).contains(b)) {
        place.insert("tempo".into(), json!(tempo));
    }

    let mut msg = format!(
        "「{name}」をインポートしました（{:.2} 秒・{} Hz・{} ch）",
        info.seconds(),
        info.rate,
        info.channels
    );
    if let (Some(file_bpm), None) = (info.bpm, bpm)
        && (file_bpm - song_bpm(p)).abs() > 1e-6
    {
        msg.push_str(&format!(
            "。ファイルのテンポは {} BPM です（曲のテンポは変えていません）",
            show_bpm(file_bpm)
        ));
    }
    if copied.is_some() {
        msg.push_str("。曲のフォルダーにコピーしました");
    }
    if dry {
        msg = format!("「{name}」をインポートします");
    } else if cx.dir.is_some() {
        // The waveform's peaks, made meanwhile in the song's `Peaks/` (UI-037).
        crate::peaks::request(&path, &crate::peaks::store_dir(cx.dir));
    }
    let data = json!({
        "file": place["file"],
        "length": length,
        "at": at,
        "rate": info.rate,
        "channels": info.channels,
        "seconds": info.seconds(),
        "file_bpm": info.bpm,
        "bpm_from": info.bpm_from,
        "bpm": bpm,
        "copied": copied.is_some(),
    });
    Ok(Plan {
        output: Output::with(msg, data),
        edits: if dry {
            Vec::new()
        } else {
            vec![("clip.import.place".into(), place)]
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_in_the_song_folder_is_referred_to_from_it() {
        let dir = Path::new("D:/Songs/A");
        assert_eq!(
            song_path(&dir.join("Audio").join("x.wav"), Some(dir)),
            "Audio/x.wav"
        );
        let other = Path::new("E:/Samples/y.wav");
        assert_eq!(song_path(other, Some(dir)), other.to_string_lossy());
        assert_eq!(song_path(other, None), other.to_string_lossy());
    }
}
