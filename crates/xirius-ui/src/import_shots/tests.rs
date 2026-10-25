//! [`IMPORT`] played without a window: each press finds its target, and each stage leaves
//! the song as its picture says.

use xirius_session::output::OutputMode;

use super::*;
use crate::shot::resolve;

/// Starts the player of the file the browser chose and waits until the engine has played some
/// of it. Waits on what the engine says, not a time: on a busy machine it starts late, and the
/// file is short enough to play to its end before the stage after it is done. The minute is
/// only a bound.
fn wait_until_heard(s: &mut Studio) {
    let key = s.browser.playing.clone().expect("a file chosen");
    s.follow_audition();
    let begun = Instant::now();
    loop {
        assert_eq!(s.error, None);
        if crate::audition::polled(&key).is_some_and(|p| p > 0.0) {
            return;
        }
        assert!(
            begun.elapsed() < Duration::from_secs(60),
            "the file never played"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn play(s: &mut Studio, act: Act) {
    crate::browser_files::settle_rows(&s.browser);
    let g = s.geometry(1440.0, 860.0 - xirius_design::Layout::TOP_BAR_H);
    match act {
        Act::Import(n) => stage(s, &g, n),
        Act::Key(key) => {
            s.edit_key(
                &g,
                key.rsplit('-').next().unwrap(),
                crate::tools::Mods {
                    ctrl: key.contains("ctrl"),
                    shift: key.contains("shift"),
                    alt: false,
                },
            );
        }
        Act::Press(t) | Act::Hover(t) => {
            let hit = resolve(s, t).unwrap_or_else(|| panic!("{t:?}"));
            let (x, y) = s
                .center_of(&g, hit)
                .unwrap_or_else(|| panic!("{t:?} off screen"));
            let actual = s.hit(&g, x, y);
            assert_eq!(actual, hit, "{t:?}");
            if matches!(act, Act::Press(_)) {
                s.press(&g, actual);
            } else {
                s.hover = actual;
            }
        }
        other => panic!("{other:?} is not in the script"),
    }
    s.settle_browser_import();
    s.browser.settle_prefs_io();
    s.settle_browser_menu();
}

#[test]
fn the_import_script_puts_real_files_and_takes_them_back() {
    let mut s = Studio::sample();
    s.transport.mode = OutputMode::Silent { paced: true };
    let tracks = s.project().tracks.len();
    // The saved song copies what it reads (BRW-004): its clip names the copy, read from where
    // the song is.
    let on = |s: &Studio| {
        let copy = imported(s, "loop_120bpm.wav");
        let dir = s.song().dir().expect("saved");
        s.project()
            .tracks
            .values()
            .flat_map(|t| &t.clips)
            .filter(|c| matches!(&c.body, xirius_core::model::ClipBody::Audio { file, .. } if dir.join(file) == copy))
            .count()
    };
    let mut played = false;
    for shot in IMPORT {
        for &act in shot.acts {
            play(&mut s, act);
            assert_eq!(s.error, None, "{}", shot.name);
            if shot.name == "m03-audition" && matches!(act, Act::Press(_)) {
                wait_until_heard(&mut s);
                played = true;
            }
        }
        match shot.name {
            "m01-place-tab" => {
                assert_eq!(s.browser.location().name, PLACE);
                assert!(s.song().dir().is_some());
            }
            // It may have played to its end by now: that it played is what is checked.
            "m03-audition" => assert!(played),
            "m05-drag" => {
                let g = s.geometry(1440.0, 860.0 - xirius_design::Layout::TOP_BAR_H);
                let d = s.browser.file_drag.clone().expect("held");
                assert!(d.moved);
                let label = crate::browser_pick::drop_label(&s, &g, &d).unwrap();
                assert!(
                    label.ends_with(" に置く") && !label.starts_with("新しい"),
                    "{label}"
                );
            }
            "m06-dropped" => {
                assert_eq!(on(&s), 1);
                assert!(imported(&s, "loop_120bpm.wav").is_file(), "copied");
                assert_eq!(s.project().tracks.len(), tracks, "on a track it had");
            }
            "m07-undo" => assert_eq!(on(&s), 0),
            "m08-from-file" => assert_eq!(s.project().tracks.len(), tracks + 2),
            "m09-tab-added" => assert_eq!(s.browser.locations().len(), 3),
            "m10-tab-removed" => assert_eq!(s.browser.locations().len(), 2),
            _ => {}
        }
    }
    crate::audition::stop();
    let _ = std::fs::remove_dir_all(dir());
}
