//! The plugin windows, opened in the real window as a person opens them (UI-040, PLG-027):
//! the app runs `--script plugin` (`src/plugin_shots.rs`), which presses the keys button at the
//! head of Keys in the sample song and the plugin rows in the mixer, for a built-in instrument
//! and effect, and for a VST3 instrument and effect, in the app and in a plugin host process.
//!
//! The app's window is put off every screen and never takes the focus; the plugins' windows
//! open off every screen too (`XIRIUS_EDITOR_OFFSCREEN`); nothing is played to a speaker;
//! nothing is typed or clicked through the OS. The built-ins' panels are read from the shots,
//! the VST3s' windows from their own pictures (PrintWindow).
//!
//! A plugin host's window closes with the app.
//!
//! The VST3s are this PC's (kHs Gain; sforzando, else Vital). A kind whose plugin is missing
//! says why and is left out.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// What a stage of the run said: `plugin-check <shot> key=value …` lines, by shot.
struct Run {
    checks: HashMap<String, Vec<HashMap<String, String>>>,
    skipped: HashSet<String>,
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
    let out = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "plugin", "--shot"])
        .arg(&dir)
        .env("XIRIUS_EDITOR_OFFSCREEN", "1")
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .expect("the app runs");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "the run failed: {err}");
    assert!(!err.contains("panicked"), "{err}");
    let mut checks: HashMap<String, Vec<HashMap<String, String>>> = HashMap::new();
    let mut skipped = HashSet::new();
    for line in err.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("plugin-check") => {
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
            Some("plugin-skip") => {
                eprintln!("skipped: {line}");
                let kind = words.nth(1).unwrap_or_default().trim_end_matches(':');
                skipped.insert(kind.to_string());
            }
            _ => {}
        }
    }
    Run {
        checks,
        skipped,
        dir,
        _tmp: dir_tmp,
    }
}

/// A shot's pixels: a top-down 32-bit BMP, as the shots and `capture_window` write it.
struct Picture {
    width: usize,
    height: usize,
    bgra: Vec<u8>,
}

fn picture(path: &Path) -> Picture {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let int = |at: usize| i32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let offset = int(10) as usize;
    let (width, height) = (int(18), int(22));
    assert!(height < 0, "{}: a top-down picture", path.display());
    assert_eq!(u16::from_le_bytes([b[28], b[29]]), 32);
    let (width, height) = (width as usize, height.unsigned_abs() as usize);
    Picture {
        width,
        height,
        bgra: b[offset..offset + width * height * 4].to_vec(),
    }
}

impl Picture {
    fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 4;
        [self.bgra[i], self.bgra[i + 1], self.bgra[i + 2]]
    }

    /// The colours inside the rectangle, and the share of its pixels unlike `other`'s.
    fn region(&self, r: [usize; 4], other: &Picture) -> (usize, f64) {
        let [x0, y0, w, h] = r;
        assert!(
            x0 + w <= self.width && y0 + h <= self.height,
            "{r:?} in the shot"
        );
        let mut colors = HashSet::new();
        let mut unlike = 0usize;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let p = self.pixel(x, y);
                colors.insert(p);
                if p != other.pixel(x, y) {
                    unlike += 1;
                }
            }
        }
        (colors.len(), unlike as f64 / (w * h) as f64)
    }
}

/// The built-in's panel is drawn in `shot`'s picture, where `without` (the same screen with
/// no panel) has something else.
fn check_panel(run: &Run, shot: &str, without: &str, what: &str) {
    assert_eq!(run.value(shot, "built-in"), "1", "{what}: one panel open");
    assert_eq!(run.value(shot, "error"), "None", "{what}");
    let n = |k: &str| run.value(shot, k).parse::<f64>().unwrap() as usize;
    let r = [n("x"), n("y"), n("w"), n("h")];
    assert!(r[2] > 100 && r[3] > 100, "{what}: {r:?}");
    let with = picture(&run.dir.join(format!("{shot}.bmp")));
    let other = picture(&run.dir.join(format!("{without}.bmp")));
    let (colors, unlike) = with.region(r, &other);
    assert!(colors > 16, "{what}: the panel drew ({colors} colours)");
    assert!(
        unlike > 0.3,
        "{what}: the panel is in the shot ({:.0} % of it differs)",
        unlike * 100.0
    );
}

/// The VST3's own window opened, off every screen, not in front, on the instance that plays,
/// and drew something.
fn check_window(run: &Run, shot: &str, place: &str, what: &str) {
    let editor = run.value(shot, "editor");
    assert!(
        editor != "missing" && editor != "failed",
        "{what}: {:?}",
        run.lines(shot)
    );
    assert_eq!(run.value(shot, "place"), place, "{what}");
    assert_eq!(run.value(shot, "heard"), "true", "{what}: heard");
    assert_eq!(
        run.value(shot, "foreground"),
        "false",
        "{what}: not in front"
    );
    assert_eq!(
        run.value(shot, "offscreen"),
        "true",
        "{what}: off every screen"
    );
    assert_eq!(run.value(shot, "vst3"), "1", "{what}: one window open");
    assert_eq!(run.value(shot, "error"), "None", "{what}");
    let n = |k: &str| run.value(shot, k).parse::<usize>().unwrap();
    assert!(n("width") > 0 && n("height") > 0, "{what}");
    assert!(n("colors") > 1, "{what}: the window drew");
    let p = picture(&run.dir.join(format!("{shot}-editor.bmp")));
    assert_eq!((p.width, p.height), (n("width"), n("height")), "{what}");
}

#[test]
fn the_plugin_windows_open_and_draw_in_the_real_window() {
    let run = run_script();

    // The keys button of a track with no instrument opens the list of instruments.
    let shot = "p01-keys-without-instrument";
    assert!(run.value(shot, "popup").starts_with("Some(Instrument("));
    assert_eq!(run.value(shot, "error"), "None");

    // A built-in instrument, from the keys button.
    assert_eq!(run.value("p02-keys-built-in", "keys-open"), "Some(true)");
    check_panel(
        &run,
        "p02-keys-built-in",
        "p03-keys-vst3",
        "built-in instrument",
    );

    // A built-in effect, from its row in the mixer.
    check_panel(
        &run,
        "p05-mixer-built-in-effect",
        "p06-mixer-vst3-effect",
        "built-in effect",
    );

    // A VST3 instrument, from the keys button and from its row in the mixer.
    if run.skipped.contains("instrument") {
        eprintln!("skipped: the VST3 instrument's window (no VST3 instrument on this PC)");
    } else {
        assert_eq!(run.value("p03-keys-vst3", "keys-open"), "Some(true)");
        check_window(&run, "p04-keys-vst3-window", "app", "VST3 instrument, keys");
        assert_eq!(
            run.value("p08-mixer-vst3-instrument", "keys-open"),
            "Some(true)"
        );
        check_window(
            &run,
            "p09-mixer-vst3-instrument-window",
            "app",
            "VST3 instrument, mixer",
        );
    }

    // A VST3 effect, from its row in the mixer: in the app, and in a plugin host process.
    if run.skipped.contains("effect") {
        eprintln!("skipped: the VST3 effect's window (no VST3 effect on this PC)");
    } else {
        check_window(&run, "p07-mixer-vst3-effect-window", "app", "VST3 effect");
        if run.skipped.contains("host") {
            eprintln!("skipped: the plugin host's window (xirius-plugin-host is not built)");
        } else {
            let shot = "p11-host-vst3-effect-window";
            check_window(&run, shot, "host", "VST3 effect, plugin host");
            // The app has closed: the host's window has gone with it.
            let title = run.value(shot, "title");
            let t = Instant::now();
            while xirius_ui::park::any_window(&title).is_some() {
                assert!(
                    t.elapsed() < Duration::from_secs(10),
                    "{title} stays open after the app closed"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}
