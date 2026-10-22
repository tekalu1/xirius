//! `export.dawproject` (EXP-007): the whole song as a `.dawproject`, the way to take it to
//! another DAW. The format itself is in [`xirius_core::dawproject`]; this adds what the session
//! knows: how long the audio files are, where the song's folder is and where replaced files go.
//!
//! The audio the clips play goes inside the file by default, so one file carries the song. A
//! file already at the place is sent to the recycle bin first, as the other exports do.

use std::path::{Path, PathBuf};

use serde_json::json;
use xirius_core::dawproject::{self, AudioInfo, Export, Skips};
use xirius_core::model::Project;
use xirius_core::{Args, CmdError, CmdResult, ExtContext, Output};

use crate::cleanup::Trash;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 1] = ["export.dawproject"];

/// The channels, rate and length of an audio file, read from its header.
pub fn audio_info(path: &Path) -> Option<AudioInfo> {
    let i = crate::decode::probe(path).ok()?;
    Some(AudioInfo {
        channels: i.channels,
        sample_rate: i.rate,
        seconds: if i.rate == 0 {
            0.0
        } else {
            i.frames as f64 / i.rate as f64
        },
    })
}

/// The file name the song is written to: its name, made safe for every system.
pub fn file_name(p: &Project) -> String {
    format!(
        "{}.{}",
        crate::export::sanitize(&p.name),
        dawproject::EXTENSION
    )
}

/// Where the song goes when no place is given: its own folder.
pub fn default_path(p: &Project, dir: Option<&Path>) -> Option<PathBuf> {
    dir.map(|d| d.join(file_name(p)))
}

/// What [`write`] would leave out of the song.
pub fn skipped(p: &Project, dir: Option<&Path>, embed: bool) -> Skips {
    dawproject::skipped_on_export(
        p,
        &Export {
            song_dir: dir,
            embed_audio: embed,
            audio: &audio_info,
        },
    )
}

/// Writes the song to `out`, telling `step` how far it is (0..=1). A file already there goes
/// to `trash` first. Returns what the file could not carry.
pub fn write(
    p: &Project,
    dir: Option<&Path>,
    out: &Path,
    embed: bool,
    trash: &Trash,
    step: &mut dyn FnMut(f64),
) -> Result<Skips, String> {
    if let Some(parent) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} を作れません: {e}", parent.display()))?;
    }
    if out.is_file() {
        trash(out)?;
    }
    dawproject::export_with(
        p,
        out,
        &Export {
            song_dir: dir,
            embed_audio: embed,
            audio: &audio_info,
        },
        step,
    )
}

/// `export.dawproject`, written at once.
pub fn run(cx: &ExtContext<'_>, args: &Args, dry: bool, trash: &Trash) -> CmdResult<Output> {
    let out = match args.opt_str("out") {
        Some(o) => {
            let mut o = PathBuf::from(o);
            if !dawproject::is_dawproject(&o) {
                let mut s = o.into_os_string();
                s.push(".");
                s.push(dawproject::EXTENSION);
                o = PathBuf::from(s);
            }
            o
        }
        None => default_path(cx.project, cx.dir).ok_or_else(|| {
            CmdError::invalid("保存していない曲では --out に書き出すファイルを指定してください")
        })?,
    };
    let embed = !args.flag("external");
    let shown = out.display().to_string();
    if dry {
        let skipped = skipped(cx.project, cx.dir, embed);
        return Ok(Output::with(
            format!("{shown} に書き出します{}", more(&skipped)),
            json!({ "path": shown, "embed_audio": embed, "skipped": skipped.to_json() }),
        ));
    }
    let skipped =
        write(cx.project, cx.dir, &out, embed, trash, &mut |_| {}).map_err(CmdError::conflict)?;
    Ok(Output::with(
        format!("{shown} に書き出しました{}", more(&skipped)),
        json!({ "path": shown, "embed_audio": embed, "skipped": skipped.to_json() }),
    ))
}

/// 「（含めなかったもの N）」 when something was left out.
fn more(skipped: &Skips) -> String {
    if skipped.is_empty() {
        String::new()
    } else {
        format!("（含めなかったもの {}）", skipped.len())
    }
}
