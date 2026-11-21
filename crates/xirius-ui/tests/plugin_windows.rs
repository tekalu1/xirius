//! Opens the built-in and VST3 instrument and effect editors from the track and mixer.
//! The silent, parked script uses virtual clicks and captures each editor's own HWND.

#![cfg(windows)]

use std::collections::HashMap;
mod support;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// What a stage of the run said: `plugin-check <shot> key=value …` lines, by shot.
struct Run {
    checks: HashMap<String, Vec<HashMap<String, String>>>,
    dir: PathBuf,
    /// The shots' folder, removed when dropped (also when the test fails).
    _tmp: tempfile::TempDir,
}

impl Run {
    fn lines(&self, shot: &str) -> &[HashMap<String, String>] {
        self.checks.get(shot).map_or(&[], |v| v.as_slice())
    }

    /// The value of `key` on the first line of `shot` that has it.
    fn value(&self, shot: &str, key: &str) -> String {
        self.lines(shot)
            .iter()
            .find_map(|l| l.get(key).cloned())
            .unwrap_or_else(|| panic!("{shot} says no {key}: {:?}", self.lines(shot)))
    }
}

fn run_script() -> Run {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-ui-plugin-windows-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().to_path_buf();
    support::sample_audio(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "plugin", "--shot"])
        .arg(&dir)
        .current_dir(&dir)
        .env("XIRIUS_EDITOR_OFFSCREEN", "1")
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .expect("the app runs");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "the run failed: {err}");
    assert!(!err.contains("panicked"), "{err}");
    let mut checks: HashMap<String, Vec<HashMap<String, String>>> = HashMap::new();
    for line in err.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("plugin-check") {
            continue;
        }
        let shot = words.next().unwrap_or_default().to_string();
        // A window's title, last on its line, has spaces.
        let (line, title) = match line.split_once(" title=") {
            Some((l, t)) => (l, Some(t)),
            None => (line, None),
        };
        let mut pairs: HashMap<String, String> = line
            .split_whitespace()
            .skip(2)
            .filter_map(|w| w.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        if let Some(t) = title {
            pairs.insert("title".into(), t.to_string());
        }
        checks.entry(shot).or_default().push(pairs);
    }
    Run {
        checks,
        dir,
        _tmp: dir_tmp,
    }
}

/// A shot's pixels: a top-down 32-bit BMP, as the shots and `capture_window` write it.
struct Picture {
    width: usize,
    height: usize,
}

fn picture(path: &Path) -> Picture {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(&b[..2], b"BM");
    let width = i32::from_le_bytes(b[18..22].try_into().unwrap());
    let height = i32::from_le_bytes(b[22..26].try_into().unwrap());
    assert!(height < 0, "{}: a top-down picture", path.display());
    Picture {
        width: width as usize,
        height: (-height) as usize,
    }
}

fn dimensions(run: &Run, shot: &str, minimum: usize, colors: usize) {
    let n = |key: &str| run.value(shot, key).parse::<usize>().unwrap();
    assert!(n("width") > minimum && n("height") > minimum, "{shot}");
    assert!(n("colors") > colors, "{shot}: the editor drew");
    let bmp = picture(&run.dir.join(format!("{shot}-editor.bmp")));
    assert_eq!((bmp.width, bmp.height), (n("width"), n("height")));
}

fn check_builtin(run: &Run, shot: &str, capture: &str) {
    assert_eq!(run.value(shot, "built-in"), "1");
    assert_eq!(run.value(shot, "error"), "None");
    for field in ["built_in", "owned", "visible", "offscreen"] {
        assert_eq!(run.value(capture, field), "true", "{capture}: {field}");
    }
    assert_eq!(run.value(capture, "foreground"), "false");
    dimensions(run, capture, 100, 16);
    check_closed(run, capture);
}

fn check_window(run: &Run, shot: &str) {
    assert_eq!(run.value(shot, "in_host"), "true");
    assert_eq!(run.value(shot, "foreground"), "false");
    assert_eq!(run.value(shot, "offscreen"), "true");
    assert_eq!(run.value(shot, "vst3"), "1");
    assert_eq!(run.value(shot, "error"), "None");
    dimensions(run, shot, 0, 1);
    check_closed(run, shot);
}

fn check_closed(run: &Run, shot: &str) {
    let title = run.value(shot, "title");
    let t = Instant::now();
    while xirius_ui::park::any_window(&title).is_some() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "{title} stayed open after the app closed"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_plugin_windows_open_and_draw_in_the_real_window() {
    let run = run_script();

    let picker = "p01-keys-without-instrument";
    assert!(run.value(picker, "popup").starts_with("Some(Instrument("));
    assert_eq!(run.value(picker, "error"), "None");

    for shot in [
        "p02-keys-built-in",
        "p03-keys-vst3",
        "p08-mixer-vst3-instrument",
    ] {
        assert_eq!(run.value(shot, "keys-open"), "Some(true)", "{shot}");
    }
    check_builtin(&run, "p02-keys-built-in", "p02-keys-built-in-window");
    check_builtin(
        &run,
        "p05-mixer-built-in-effect",
        "p05-mixer-built-in-effect-window",
    );

    for shot in [
        "p04-keys-vst3-window",
        "p07-mixer-vst3-effect-window",
        "p09-mixer-vst3-instrument-window",
        "p11-host-vst3-effect-window",
    ] {
        check_window(&run, shot);
    }
}
