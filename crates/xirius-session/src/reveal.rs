//! `file.reveal_os` (BRW-016): a file or a folder shown in the OS's file window (Explorer, or
//! Finder on a Mac), with the file picked where the OS can. A relative path is the song
//! folder's, and without a path the song folder itself is shown (BRW-004). `--dry-run` says what
//! would open and opens nothing.

use std::path::{Path, PathBuf};

use serde_json::json;
use xirius_core::{Args, CmdError, CmdResult, Output};

use crate::os;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 1] = ["file.reveal_os"];

/// The path asked for, made absolute against the song's folder `song` when it is relative.
pub fn target(song: Option<&Path>, given: &str) -> PathBuf {
    let p = Path::new(given.trim());
    match song {
        Some(dir) if p.is_relative() => dir.join(p),
        _ => p.to_path_buf(),
    }
}

/// Runs `file.reveal_os`. `open` is what opens the window (the OS's, but a test's own).
pub fn run_with(
    song: Option<&Path>,
    args: &Args,
    dry: bool,
    open: impl FnOnce(&Path) -> Result<(), String>,
) -> CmdResult<Output> {
    let path = match (args.opt_str("path"), song) {
        (Some(given), _) if given.trim().is_empty() => {
            return Err(CmdError::invalid("--path が空です"));
        }
        (Some(given), _) => target(song, given),
        (None, Some(dir)) => dir.to_path_buf(),
        (None, None) => {
            return Err(CmdError::invalid(
                "保存前の曲には曲のフォルダーがないため、--path が必要です",
            ));
        }
    };
    if !path.exists() {
        return Err(CmdError::conflict(format!(
            "{} はありません",
            path.display()
        )));
    }
    let path = std::path::absolute(&path).map_err(|e| CmdError::io(e.to_string()))?;
    if !dry {
        open(&path).map_err(CmdError::conflict)?;
    }
    Ok(Output::with(
        format!("{} を表示します", path.display()),
        json!({ "path": path.display().to_string() }),
    ))
}

pub fn run(song: Option<&Path>, args: &Args, dry: bool) -> CmdResult<Output> {
    run_with(song, args, dry, os::reveal)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A folder of the test's own, removed when dropped (also when the test fails).
    fn folder(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("xirius-reveal-{name}-"))
            .tempdir()
            .unwrap()
    }

    fn args(path: &str) -> Args {
        Args::from_json(json!({ "path": path }))
    }

    #[test]
    fn a_file_in_the_song_folder_is_opened_by_its_relative_path() {
        let song_tmp = folder("rel");
        let song = song_tmp.path().to_path_buf();
        std::fs::create_dir_all(song.join("Audio")).unwrap();
        std::fs::write(song.join("Audio").join("kick.wav"), b"x").unwrap();
        let opened = RefCell::new(Vec::new());
        let out = run_with(Some(&song), &args("Audio/kick.wav"), false, |p| {
            opened.borrow_mut().push(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        let want = std::path::absolute(song.join("Audio").join("kick.wav")).unwrap();
        assert_eq!(*opened.borrow(), vec![want.clone()]);
        assert_eq!(out.data["path"], want.display().to_string());
        let _ = std::fs::remove_dir_all(&song);
    }

    #[test]
    fn a_dry_run_opens_nothing_and_a_missing_file_is_said() {
        let song_tmp = folder("dry");
        let song = song_tmp.path().to_path_buf();
        std::fs::write(song.join("a.wav"), b"x").unwrap();
        let out = run_with(Some(&song), &args("a.wav"), true, |_| {
            panic!("a dry run opens nothing")
        })
        .unwrap();
        assert!(out.message.contains("a.wav"));
        let e = run_with(Some(&song), &args("none.wav"), false, |_| {
            panic!("a missing file opens nothing")
        })
        .unwrap_err();
        assert!(e.to_string().contains("none.wav"), "{e}");
        let e = run_with(Some(&song), &args("  "), false, |_| Ok(())).unwrap_err();
        assert!(e.to_string().contains("--path"), "{e}");
        let _ = std::fs::remove_dir_all(&song);
    }

    #[test]
    fn the_os_failing_to_open_is_the_commands_error() {
        let song_tmp = folder("fail");
        let song = song_tmp.path().to_path_buf();
        std::fs::write(song.join("a.wav"), b"x").unwrap();
        let e = run_with(Some(&song), &args("a.wav"), false, |_| {
            Err("開けませんでした".into())
        })
        .unwrap_err();
        assert!(e.to_string().contains("開けませんでした"), "{e}");
        let _ = std::fs::remove_dir_all(&song);
    }

    #[test]
    fn an_absolute_path_stays_as_it_is() {
        let song_tmp = folder("abs-song");
        let song = song_tmp.path().to_path_buf();
        let other_tmp = folder("abs-other");
        let other = other_tmp.path().to_path_buf();
        std::fs::write(other.join("b.wav"), b"x").unwrap();
        assert_eq!(
            target(Some(&song), other.join("b.wav").to_str().unwrap()),
            other.join("b.wav")
        );
        let _ = std::fs::remove_dir_all(&song);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn without_a_path_the_song_folder_is_shown_and_a_song_not_saved_needs_one() {
        let song_tmp = folder("none");
        let song = song_tmp.path().to_path_buf();
        let opened = RefCell::new(Vec::new());
        run_with(Some(&song), &Args::from_json(json!({})), false, |p| {
            opened.borrow_mut().push(p.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(*opened.borrow(), vec![std::path::absolute(&song).unwrap()]);
        let e = run_with(None, &Args::from_json(json!({})), false, |_| {
            panic!("no folder to open")
        })
        .unwrap_err();
        assert!(e.to_string().contains("--path"), "{e}");
    }
}
