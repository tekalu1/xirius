//! A plugin's window stays open while its instance is swapped (ENG-066 to ENG-071), in the real
//! window: the app runs `--script plugin-swap` (`src/plugin_swap_shots.rs`). The test VST3
//! plays on Vocal in a plugin host process of its own and its window is opened from its row in
//! the mixer; then, with the window open, its value is moved in the window (which changes its
//! latency), a track is added, it is moved to the shared host, into the app's process and back
//! to a host of its own. After each the window is on the new instance and shows the moved
//! value. Last, the window is closed while a move is on its way (to the shared host, and into
//! the app): it does not open again.
//!
//! Each instance takes a while to be made (`XIRIUS_TEST_VST3_OPEN_MS`), so the new mix is on
//! its way long enough to close the window in it.
//!
//! The app's window is put off every screen and never takes the focus; the plugin's window
//! opens off every screen too (`XIRIUS_EDITOR_OFFSCREEN`); nothing is played to a speaker;
//! nothing is typed or clicked through the OS.

use std::collections::HashMap;
use std::process::{Command, Stdio};

/// One `plugin-check <shot> <what> k=v …` line of a stage.
#[derive(Debug)]
struct Seen {
    what: String,
    values: HashMap<String, String>,
}

impl Seen {
    fn n(&self, k: &str) -> u32 {
        self.values
            .get(k)
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("{self:?}: no {k}"))
    }
    fn yes(&self, k: &str) -> bool {
        self.values.get(k).map(String::as_str) == Some("true")
    }
}

#[test]
fn the_window_follows_the_plugin_through_each_swap() {
    if xirius_engine::server::find_host().is_none() {
        eprintln!("the plugin host binary is not built");
        return;
    }
    xirius_test_vst3::fixture_marker();
    let dir = tempfile::Builder::new()
        .prefix("xirius-ui-plugin-swap-")
        .tempdir()
        .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "plugin-swap", "--shot"])
        .arg(dir.path())
        .env("XIRIUS_EDITOR_OFFSCREEN", "1")
        .env("XIRIUS_TEST_VST3_OPEN_MS", "900")
        .env_remove("XIRIUS_SWAP_VST3")
        .env("XIRIUS_CRASH_DIR", dir.path().join("crashes"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the app runs");
    let app = child.id();
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "the run failed: {err}");
    assert!(!err.contains("panicked"), "{err}");
    let mut seen: HashMap<String, Seen> = HashMap::new();
    let mut times: HashMap<String, u32> = HashMap::new();
    for line in err.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let values = || {
            w.iter()
                .skip(2)
                .filter_map(|p| p.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<HashMap<_, _>>()
        };
        match w.first() {
            Some(&"plugin-check") if w.len() > 3 && !w[2].contains('=') => {
                seen.insert(
                    w[1].to_string(),
                    Seen {
                        what: w[2].to_string(),
                        values: values(),
                    },
                );
            }
            Some(&"plugin-time") => {
                let v = values();
                times.insert(w[1].to_string(), v["drawn_ms"].parse().unwrap());
            }
            _ => {}
        }
    }
    let at = |shot: &str| {
        let s = seen
            .get(shot)
            .unwrap_or_else(|| panic!("{shot} said nothing: {err}"));
        if s.what != "closed" {
            assert!(
                dir.path().join(format!("{shot}-editor.bmp")).is_file(),
                "{shot}: the window's picture"
            );
            assert!(times.contains_key(shot), "{shot}: how long it took");
        }
        s
    };
    let open = at("swap01-open");
    let latency = at("swap02-latency");
    let added = at("swap03-track-added");
    let shared = at("swap04-shared");
    let inprocess = at("swap05-inprocess");
    let dedicated = at("swap06-dedicated");
    assert_eq!(open.n("delay"), 192, "the song's value");
    assert_ne!(open.n("pid"), app, "its own host");
    for (s, before) in [(latency, open), (added, latency)] {
        assert_eq!(s.n("delay"), 128, "the value moved in the window");
        assert_eq!(s.n("pid"), before.n("pid"), "the same host");
        assert_ne!(s.n("instance"), before.n("instance"), "a new instance");
    }
    for s in [shared, inprocess, dedicated] {
        assert_eq!(s.n("delay"), 128, "the value moved in the window");
        assert_eq!(s.n("pid"), s.n("window-pid"), "the window of the instance");
    }
    assert_ne!(shared.n("pid"), added.n("pid"), "the shared host");
    assert_eq!(inprocess.n("pid"), app, "in the app");
    assert_ne!(dedicated.n("pid"), app, "out of the app");
    assert_ne!(dedicated.n("pid"), shared.n("pid"), "a host of its own");
    for shot in ["swap07-closed-shared", "swap09-closed-inprocess"] {
        let s = at(shot);
        assert_eq!(s.what, "closed");
        assert!(
            s.yes("pending"),
            "{shot}: closed while the new mix was on its way"
        );
        assert!(!s.yes("reopened"), "{shot}: the window opened again");
    }
    // To another host, the new instance takes a while: the screen sees the window closed
    // before it plays. One made in the app comes in within a frame of the screen.
    assert!(at("swap07-closed-shared").yes("noticed"));
    let reopened = at("swap08-reopened");
    assert_ne!(reopened.n("pid"), app);
}
