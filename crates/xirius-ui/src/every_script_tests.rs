//! ARC-054 G7: every step of every script in [`crate::SCRIPTS`], played without a window at the
//! size the window plays them at (`--shot`, 1440×860).
//!
//! A script is played from the sample song, shot after shot, the way the window plays it: each
//! press, hover, drag and wheel turn must find its target on the screen, and no step may leave
//! an error. A step only the window can play (a [`Play::Window`] stage, the piano roll's ruler,
//! the plugin windows) ends its shot here, and the shots after it until one starts again from
//! the sample song; the window tests play the rest. A script registered in [`crate::SCRIPTS`] is
//! played without anything added here, and a `pub const` list of shots that is not registered
//! fails by its name. What a shot leaves is checked after it, from the `ENDS` table beside its
//! script ([`ends`]).
//!
//! `XIRIUS_SCRIPT=<name>` plays that script alone.

use std::panic::{AssertUnwindSafe, catch_unwind};

use xirius_session::output::OutputMode;

use crate::shot::{Act, Play, Shot, TestFolder, TestTemp, resolve};
use crate::studio::Studio;

/// The size the window plays the scripts at (`--shot`); the scripts' targets are placed for it.
const SIZE: (f32, f32) = (1440.0, 860.0);

/// Scripts this test does not play, each with why.
const NOT_PLAYED: &[(&str, &str)] = &[
    (
        "ara-real",
        "loads the real ARA plugin XIRIUS_ARA_REAL_PLUGIN names, which the PC running it has",
    ),
    (
        "ara-real-restart",
        "starts from an empty song (not the sample) and loads the real ARA plugin",
    ),
    (
        "mp3",
        "exports with the MP3 encoder beside the binary (scripts/build-lame.ps1), which a test \
         binary does not have",
    ),
    (
        "plugin-swap",
        "every stage reads the plugin's own window (its label and picture); \
         tests/plugin_swap_window.rs plays it",
    ),
];

/// A row of a script's `ENDS` table: a shot's name, and what must hold after it.
pub(crate) type End = (&'static str, fn(&Studio));

/// What the shots leave, by shot: each script's `ENDS` table.
fn ends() -> Vec<End> {
    [
        crate::devices_shots::tests::ENDS,
        crate::ara_shots::tests::ENDS,
    ]
    .concat()
}

fn sample() -> Studio {
    let mut s = Studio::sample();
    s.transport.mode = OutputMode::Silent { paced: true };
    s
}

/// Whether only the window can play the act.
fn needs_a_window(act: &Act) -> bool {
    matches!(
        act,
        Act::Stage(Play::Window(_), _) | Act::RollRuler(_) | Act::Plugin(_)
    )
}

/// The body of the window: everything between the top bar and the bottom bar.
fn geometry(studio: &Studio) -> crate::studio::Geometry {
    studio.geometry(
        SIZE.0,
        SIZE.1 - xirius_design::Layout::TOP_BAR_H - xirius_design::Layout::BOTTOM_BAR_H,
    )
}

/// What a frame of the window does to the studio before it draws.
fn frame(studio: &mut Studio) {
    studio.macro_follow();
    let g = geometry(studio);
    studio.resized(&g);
    if !studio.input_blocked() {
        studio.run_midi_commands(&g);
        studio.run_asked(&g);
    }
    studio.tick();
}

/// Between shots the window draws until the song is open, then [`SETTLE_FRAMES`] more.
fn settle(studio: &mut Studio) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while studio.opening.is_some() && std::time::Instant::now() < end {
        frame(studio);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    for _ in 0..SETTLE_FRAMES {
        frame(studio);
    }
}

/// The frames the window draws after a shot's steps before it takes the picture.
const SETTLE_FRAMES: usize = 8;

/// Plays one act the way [`crate::screen`]'s driver does, without a window.
fn play(studio: &mut Studio, act: &Act) -> Result<(), String> {
    studio.settle_browser_import();
    studio.browser.settle_prefs_io();
    studio.settle_browser_menu();
    crate::browser_files::settle_rows(&studio.browser);
    let g = geometry(studio);
    match *act {
        Act::Reset => *studio = sample(),
        Act::Stage(play, stage) => {
            play.without_window(studio, &g, stage);
        }
        Act::Import(stage) => crate::import_shots::stage(studio, &g, stage),
        Act::ClipEdit(stage) => crate::clipedit_shots::stage(studio, &g, stage),
        Act::Settings(stage) => crate::settings::shots::stage(studio, &g, stage),
        Act::RollRuler(_) | Act::Plugin(_) => unreachable!("played by the window"),
        Act::Key(source) => key(studio, &g, source)?,
        Act::Hover(target)
        | Act::Press(target)
        | Act::Wheel { at: target, .. }
        | Act::Drag { at: target, .. } => {
            let hit = resolve(studio, target).ok_or("its target is not there")?;
            let (x, y) = studio
                .center_of(&g, hit)
                .ok_or("its target is off the screen")?;
            let actual = studio.hit(&g, x, y);
            match *act {
                Act::Press(_) => {
                    studio.press(&g, actual);
                }
                Act::Hover(_) => studio.hover = actual,
                Act::Drag { dy, .. } => {
                    studio.press(&g, actual);
                    if !studio.drag_begin(&g, actual, y) {
                        return Err("the drag did not start".into());
                    }
                    studio.drag_move(y + dy, false);
                    if !studio.drag_end() {
                        return Err("the drag did not end".into());
                    }
                }
                Act::Wheel { dx, dy, ctrl, .. } => {
                    if ctrl {
                        studio.zoom(&g, x, dy / 60.0);
                    } else {
                        studio.scroll_by(&g, dx, dy, false);
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    match &studio.error {
        Some(e) => Err(format!("it left an error: {e:?}")),
        None => Ok(()),
    }
}

/// A key the way the window's key handler takes it ([`crate::screen`]), without the clipboard,
/// then its letter, as Windows sends `WM_CHAR` after the key.
fn key(studio: &mut Studio, g: &crate::studio::Geometry, source: &str) -> Result<(), String> {
    let k = gpui::Keystroke::parse(source).map_err(|e| format!("a bad key: {e}"))?;
    let m = k.modifiers;
    let mods = crate::tools::Mods {
        ctrl: m.control || m.platform,
        shift: m.shift,
        alt: m.alt,
    };
    let key = k.key.as_str();
    let letter = k
        .key_char
        .clone()
        .or_else(|| (key.chars().count() == 1).then(|| key.to_string()))
        .filter(|_| !m.control && !m.alt && !m.platform);
    let taken = studio.unsaved_key(key)
        || studio.prepare_key(key)
        || studio.opening_key(key)
        || studio.saving_key(key)
        || (key == "escape" && crate::wait::cancel())
        || studio.recovery_key(g, key)
        || studio.cleanup_key(g, key)
        || studio.home_key(g, key, mods)
        || !matches!(
            studio.text_key(key, mods),
            crate::interaction::TextKey::Pass
        );
    let logical = k.key_char.as_deref().filter(|c| {
        c.chars().count() == 1
            && !c
                .chars()
                .any(|ch| ch.is_alphanumeric() || ch.is_whitespace() || ch.is_control())
    });
    let settings_mods = if logical.is_some() {
        crate::tools::Mods {
            shift: false,
            ..mods
        }
    } else {
        mods
    };
    let taken = taken
        || (studio.settings.is_some()
            && studio.settings_key(logical.unwrap_or(key), settings_mods))
        || (studio.palette.is_some() && studio.palette_key(g, key, mods));
    if !taken {
        studio.song_key(
            g,
            crate::input_dispatch::KeyIn {
                key,
                mapped: key,
                key_char: k.key_char.as_deref(),
                character: k.key_char.as_deref(),
                code: None,
                mods,
                held: false,
            },
        );
    }
    if let Some(letter) = letter {
        studio.ime_replace(None, &letter, false);
    }
    Ok(())
}

/// Runs `f`, turning a panic into its message.
fn caught(f: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(done) => done,
        Err(p) => Err(format!(
            "panicked: {}",
            p.downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| p.downcast_ref::<&str>().copied())
                .unwrap_or("?")
        )),
    }
}

/// What playing the scripts did.
#[derive(Default)]
struct Played {
    shots: usize,
    /// Shots ended at a step only the window plays.
    cut: usize,
    acts: usize,
    /// The shots whose ends were checked.
    ended: Vec<&'static str>,
}

/// Plays `shots`; the first failure names the shot and the step.
fn play_script(name: &str, shots: &[Shot], done: &mut Played) -> Result<(), String> {
    let ends = ends();
    let mut studio = sample();
    settle(&mut studio);
    // After a step of the window, the song is not what the next shots start from, until one
    // of them starts again from the sample song.
    let mut cut = false;
    for shot in shots {
        done.shots += 1;
        if cut && !matches!(shot.acts.first(), Some(Act::Reset)) {
            done.cut += 1;
            continue;
        }
        cut = false;
        for (i, act) in shot.acts.iter().enumerate() {
            if needs_a_window(act) {
                done.cut += 1;
                cut = true;
                break;
            }
            caught(|| play(&mut studio, act))
                .map_err(|e| format!("{name} / {} / step {i} ({act:?}): {e}", shot.name))?;
            done.acts += 1;
        }
        if cut {
            continue;
        }
        caught(|| {
            settle(&mut studio);
            Ok(())
        })
        .map_err(|e| format!("{name} / {} / the frames after it: {e}", shot.name))?;
        for (_, end) in ends.iter().filter(|(n, _)| *n == shot.name) {
            caught(|| {
                end(&studio);
                Ok(())
            })
            .map_err(|e| format!("{name} / {} / its end (ENDS): {e}", shot.name))?;
            done.ended.push(shot.name);
        }
    }
    Ok(())
}

/// Plays the scripts whose place in [`crate::SCRIPTS`] falls to `part`, each with a temporary
/// folder and a folder for its files of its own.
fn play_part(part: usize) {
    // A plugin's window opens off every screen, as the parked window opens it.
    // SAFETY: as in `TestTemp::new`, before the test starts a thread.
    unsafe { std::env::set_var(xirius_vst3::OFFSCREEN_ENV, "1") };
    let _part_tmp = TestTemp::new();
    let part_dir = std::env::temp_dir();
    let only = std::env::var("XIRIUS_SCRIPT").ok();
    let mut failures = Vec::new();
    let mut done = Played::default();
    let mut scripts = Vec::new();
    for (i, (name, shots)) in crate::SCRIPTS.iter().enumerate() {
        let picked = match &only {
            Some(o) => o == name,
            None => i % PARTS == part,
        };
        if !picked || NOT_PLAYED.iter().any(|(n, _)| n == name) {
            continue;
        }
        scripts.push(*name);
        let started = std::time::Instant::now();
        // Each script's folder is made in the part's, not in the last script's (gone by now).
        for var in ["TMP", "TEMP", "TMPDIR"] {
            // SAFETY: as in `TestTemp::new`; the last script's jobs have ended (waited for
            // below), and its songs' threads do not read the variables.
            unsafe { std::env::set_var(var, &part_dir) };
        }
        let _tmp = TestTemp::new();
        // A file an export replaces goes here, not to the PC's recycle bin.
        // SAFETY: as in `TestTemp::new`.
        unsafe { std::env::set_var("XIRIUS_TRASH_DIR", std::env::temp_dir().join("trash")) };
        let _folder = TestFolder::lent("xirius-ui-every-script-");
        if let Err(e) = play_script(name, shots, &mut done) {
            failures.push(e);
        }
        // The bounces and freezes a script started run on after its song went.
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while xirius_core::running::list()
            .iter()
            .any(|j| j.pid == std::process::id())
            && std::time::Instant::now() < end
        {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        eprintln!("{name}: {:?}", started.elapsed());
    }
    eprintln!(
        "{} scripts: {} shots ({} ended at a step of the window), {} steps",
        scripts.len(),
        done.shots,
        done.cut,
        done.acts
    );
    // A row of `ENDS` whose shot this part played but not to its end checks nothing.
    let stale: Vec<&str> = ends()
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !done.ended.contains(n))
        .filter(|n| {
            crate::SCRIPTS
                .iter()
                .any(|(s, shots)| scripts.contains(s) && shots.iter().any(|shot| shot.name == *n))
        })
        .collect();
    if failures.is_empty() {
        assert!(
            stale.is_empty(),
            "rows of ENDS whose shots were not played to their ends: {stale:?}"
        );
    }
    assert!(
        failures.is_empty(),
        "{} scripts failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// One test per part, which nextest runs side by side; [`PARTS`] is how many there are.
macro_rules! parts {
    ($($test:ident = $part:literal),* $(,)?) => {
        /// The scripts are shared out among this many tests, by their place in `SCRIPTS`.
        const PARTS: usize = [$($part),*].len();
        $(
            #[test]
            fn $test() {
                play_part($part);
            }
        )*
    };
}

// Eleven, so the slowest scripts (audioproc, wire, feel2 and export-v1, 20 s to 40 s each in a
// debug build) fall in parts of their own.
parts! {
    every_step_of_the_scripts_plays_without_a_window_part_0 = 0,
    every_step_of_the_scripts_plays_without_a_window_part_1 = 1,
    every_step_of_the_scripts_plays_without_a_window_part_2 = 2,
    every_step_of_the_scripts_plays_without_a_window_part_3 = 3,
    every_step_of_the_scripts_plays_without_a_window_part_4 = 4,
    every_step_of_the_scripts_plays_without_a_window_part_5 = 5,
    every_step_of_the_scripts_plays_without_a_window_part_6 = 6,
    every_step_of_the_scripts_plays_without_a_window_part_7 = 7,
    every_step_of_the_scripts_plays_without_a_window_part_8 = 8,
    every_step_of_the_scripts_plays_without_a_window_part_9 = 9,
    every_step_of_the_scripts_plays_without_a_window_part_10 = 10,
}

/// Every row of `NOT_PLAYED` and `ENDS` names a script or a shot there is.
#[test]
fn the_rows_left_out_and_the_ends_name_scripts_and_shots() {
    for (name, why) in NOT_PLAYED {
        assert!(
            crate::SCRIPTS.iter().any(|(n, _)| n == name),
            "NOT_PLAYED names no script: {name} ({why})"
        );
        assert!(!why.is_empty(), "NOT_PLAYED gives no reason for {name}");
    }
    for (shot, _) in ends() {
        assert!(
            crate::SCRIPTS
                .iter()
                .any(|(_, shots)| shots.iter().any(|s| s.name == shot)),
            "ENDS names no shot: {shot}"
        );
    }
}

/// The `pub const X: &[Shot]` lists in the crate's sources, as `module::X`.
fn shot_lists(dir: &std::path::Path, module: &str, found: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let inner = match (module, name.as_str()) {
            (_, "lib" | "main" | "mod") => module.to_string(),
            ("", _) => name.clone(),
            _ => format!("{module}::{name}"),
        };
        if path.is_dir() {
            shot_lists(&path, &inner, found);
            continue;
        }
        if path.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            if let Some(rest) = line.trim().strip_prefix("pub const ")
                && let Some((list, _)) = rest.split_once(": &[Shot]")
            {
                found.push(format!("{inner}::{list}"));
            }
        }
    }
}

#[test]
fn every_list_of_shots_is_a_registered_script() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    shot_lists(&src, "", &mut found);
    let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
    let start = lib.find("pub const SCRIPTS").unwrap();
    let scripts = &lib[start..start + lib[start..].find("\n];").unwrap()];
    let missing: Vec<&String> = found
        .iter()
        .filter(|list| {
            ![")", ","]
                .iter()
                .any(|end| scripts.contains(&format!(" {list}{end}")))
        })
        .collect();
    assert!(
        found.len() > 100,
        "only {} lists of shots were found",
        found.len()
    );
    assert!(
        missing.is_empty(),
        "lists of shots not registered in SCRIPTS (lib.rs), so not played: {missing:?}"
    );
}
