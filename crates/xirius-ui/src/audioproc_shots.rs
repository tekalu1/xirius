//! `--script audioproc`: the audio processing of 1.0 (EDT-007, EDT-015, EDT-016, EDT-038,
//! EXP-003, EXP-004) on a loop of a real WAV at 100 BPM on the Vocal track of a song at 120:
//! its right-click menu with the fields and 「処理 ›」, following the song's tempo while the
//! sound is made and after, a sound that cannot be made, Alt on the right edge, the speed,
//! transposed, normalized, reversed, bounced, the track's menu, frozen in the mixer and
//! refused, the song made faster, and a file with a tempo of its own put in. The menus are
//! opened and chosen by real pointer events on the window; nothing takes the PC's focus.
//!
//! The file is three minutes long (the clip shows its first four bars) so that making its
//! stretched sound takes a few seconds and the shot sees it working.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use xirius_core::id::{ClipId, TrackId};
use xirius_design::Layout;
use xirius_session::peaks;

use crate::arrange::probe::{hold, track};
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::{DrawerTab, Geometry, Rect, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;
use crate::wave::{self, Look};

const fn laid(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

const fn windowed(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const AUDIOPROC: &[Shot] = &[
    // The right button on the loop: the fields at the head, 「処理 ›」 and 「バウンス ›」.
    Shot {
        name: "ap01-menu",
        acts: &[Act::Reset, laid(0), windowed(1)],
    },
    // 「処理 ›」 chosen: its rows in the menu's place.
    Shot {
        name: "ap02-process",
        acts: &[Act::Reset, laid(0), windowed(1), windowed(2)],
    },
    // 「曲のテンポに合わせる」 chosen: the sound is being made, the clip says how far.
    Shot {
        name: "ap03-follow-working",
        acts: &[Act::Reset, laid(0), windowed(1), windowed(3)],
    },
    // The sound made: the waveform follows the song's tempo.
    Shot {
        name: "ap04-follow-done",
        acts: &[Act::Reset, laid(0), laid(11)],
    },
    // The menu again: 「曲のテンポに合わせる」 has its check.
    Shot {
        name: "ap05-follow-menu",
        acts: &[Act::Reset, laid(0), laid(11), windowed(1)],
    },
    // The file cannot be read: the clip says so and the toast offers to try again.
    Shot {
        name: "ap06-follow-failed",
        acts: &[Act::Reset, laid(0), laid(12)],
    },
    // Alt on the loop's right edge, dragged a bar on and held: length and speed.
    Shot {
        name: "ap07-alt-drag",
        acts: &[Act::Reset, laid(0), laid(2)],
    },
    // The loop at 80%.
    Shot {
        name: "ap08-speed",
        acts: &[Act::Reset, laid(0), laid(13)],
    },
    // Two semitones up, and the menu that shows it.
    Shot {
        name: "ap09-transpose-menu",
        acts: &[Act::Reset, laid(0), laid(14), windowed(1)],
    },
    // Normalized: how much it went up, over the clip.
    Shot {
        name: "ap10-normalize",
        acts: &[Act::Reset, laid(0), laid(15)],
    },
    // Reversed: the waveform from its end.
    Shot {
        name: "ap11-reverse",
        acts: &[Act::Reset, laid(0), laid(16)],
    },
    // 「バウンス › 新しいトラックにバウンス」 chosen while the sound is still to be made.
    Shot {
        name: "ap12-bounce-working",
        acts: &[Act::Reset, laid(0), laid(10), windowed(1), windowed(4)],
    },
    // The bounce landed on a new track under Vocal.
    Shot {
        name: "ap13-bounce-done",
        acts: &[Act::Reset, laid(0), laid(17)],
    },
    // The right button on the Vocal track's head: bounce and freeze.
    Shot {
        name: "ap14-track-menu",
        acts: &[Act::Reset, laid(0), windowed(5)],
    },
    // 「トラックをフリーズ」 chosen: the head and the clips say how far.
    Shot {
        name: "ap15-freeze-working",
        acts: &[Act::Reset, laid(0), laid(21), windowed(5), windowed(6)],
    },
    // Frozen, in the mixer: the snowflake, the instrument and inserts locked.
    Shot {
        name: "ap16-frozen-mixer",
        acts: &[
            Act::Reset,
            laid(0),
            laid(18),
            Act::Press(Target::Tab(DrawerTab::Mixer)),
        ],
    },
    // An edit of the frozen track refused: the toast offers to unfreeze it.
    Shot {
        name: "ap17-frozen-refused",
        acts: &[Act::Reset, laid(0), laid(18), laid(19)],
    },
    // The song at 150 BPM: the loop follows it.
    Shot {
        name: "ap18-tempo-faster",
        acts: &[Act::Reset, laid(0), laid(11), laid(3)],
    },
    // A file at 140 BPM put in: the line stays with 「曲のテンポに合わせる」.
    Shot {
        name: "ap19-drop-tempo",
        acts: &[Act::Reset, laid(0), laid(20)],
    },
    // The square on the new track's clip pressed: the bounce stopped and its row closed.
    Shot {
        name: "ap20-bounce-stopped",
        acts: &[
            Act::Reset,
            laid(0),
            laid(10),
            windowed(1),
            windowed(4),
            windowed(7),
        ],
    },
];

const VOCAL: &str = "Vocal";
/// The loop's tempo: four bars of it are 9.6 seconds.
const LOOP_BPM: f64 = 100.0;
/// The file: three minutes of the loop, so its stretched sound takes a few seconds to make.
const LOOP_SECS: f64 = 180.0;

fn shots_dir() -> PathBuf {
    std::env::temp_dir().join(format!("xirius-audioproc-shots-{}", std::process::id()))
}

/// A loop at `bpm` named after it: a hit on every beat and a louder one on every bar.
/// A file already written is kept, so the sounds made of it stay made between shots.
fn write_loop(name: &str, bpm: f64, secs: f64) -> PathBuf {
    let path = shots_dir().join(name);
    let rate = 48_000u32;
    let n = (f64::from(rate) * secs) as usize;
    let bytes = 44 + 4 * n as u64;
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= bytes) {
        return path;
    }
    let (beat, bar) = (60.0 / bpm, 240.0 / bpm);
    let x: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            let level = if t % bar < beat { 0.95 } else { 0.6 };
            let tone = (t * 70.0 * std::f64::consts::TAU).sin();
            (tone * level * (-(t % beat) / 0.07).exp()) as f32
        })
        .collect();
    let _ = std::fs::create_dir_all(shots_dir());
    let _ = xirius_session::export::write_wav(&path, &x, &x, rate, 16);
    path
}

fn loop_path() -> PathBuf {
    shots_dir().join("drumloop_100bpm.wav")
}

/// The loop's clip: the last clip of Vocal, by start.
fn the_loop(s: &Studio) -> (TrackId, ClipId) {
    let t = track(s, VOCAL);
    let p = s.project();
    let mut v: Vec<_> = p.tracks[&t].clips.iter().collect();
    v.sort_by_key(|c| c.start);
    (t, v.last().expect("the loop").id)
}

fn ids(s: &Studio) -> serde_json::Value {
    json!([the_loop(s).1.to_string()])
}

fn rect_of(s: &Studio, g: &Geometry, t: TrackId, c: ClipId) -> Rect {
    let p = s.project();
    let clip = p.tracks[&t].clips.iter().find(|k| k.id == c).unwrap();
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    s.clip_rect(g, &row, clip)
}

/// Turns the screen's poll until the peaks of `path` are there to draw, or ten seconds.
fn wait_peaks(s: &mut Studio, path: &std::path::Path) {
    let store = peaks::store_dir(s.song().dir().as_deref());
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end && !matches!(wave::look(path, &store), Look::Ready(_)) {
        s.idle();
        s.tick();
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// The loop's state: a bounce or a freeze reading it, or its sound being made.
fn status(s: &Studio) -> Option<crate::clip_marks::Status> {
    let (t, c) = the_loop(s);
    let p = s.project();
    let clip = p.tracks[&t].clips.iter().find(|k| k.id == c)?;
    crate::clip_marks::status(s, clip)
}

/// Turns the screen's poll until the loop's sound is made.
fn wait_made(s: &mut Studio) {
    crate::shot::run_for(s, 60.0, |s| status(s).is_none());
}

/// Makes the loop follow the song's tempo with the file's tempo at `bpm` (each stage that
/// shows the sound being made asks for a sound not made before).
fn follow(s: &mut Studio, bpm: f64) {
    s.arrange_run("clip.tempo.set", json!({ "ids": ids(s), "bpm": bpm }));
    s.arrange_run("clip.stretch", json!({ "ids": ids(s), "follow": "on" }));
}

/// Plays stage `n` of [`AUDIOPROC`] on the screen's state.
fn stage(s: &mut Studio, g: &Geometry, n: u8) {
    s.arrange.drag = None;
    s.arrange.hover = crate::arrange::Spot::None;
    s.drawer_open = false;
    if n == 0 {
        setup(s, g);
    }
    let g = &s.geometry(g.size.0, g.size.1);
    s.rebuild(g);
    let (t, c) = the_loop(s);
    match n {
        0 => {
            s.arrange.clips = [(t, c)].into();
            s.selected_clip = Some((t, c));
            s.arrange.touched();
        }
        2 => {
            let r = rect_of(s, g, t, c);
            let from = (r.right() - 1.0, r.y + r.h * 0.6);
            let bar = s.ticks_to_px(i64::from(s.project().ppq) * 4);
            let alt = Mods {
                alt: true,
                ..Mods::default()
            };
            hold(s, g, from, (from.0 + bar * 0.75, from.1), alt);
        }
        3 => {
            s.arrange_run("tempo.set", json!({ "at": 0, "bpm": 150 }));
            wait_made(s);
            wait_peaks(s, &loop_path());
        }
        // A sound no other stage made, left to be made.
        10 => follow(s, 96.0),
        21 => follow(s, 90.0),
        11 => {
            follow(s, LOOP_BPM);
            wait_made(s);
        }
        12 => {
            // What was read of the file stays; a file that is no longer a sound cannot be
            // stretched.
            let _ = std::fs::write(loop_path(), b"not a sound");
            follow(s, 92.0);
            crate::shot::run_for(s, 20.0, |s| {
                matches!(status(s), Some(crate::clip_marks::Status::Failed))
            });
            crate::shot::run_for(s, 1.0, |s| s.toast_shown().is_some());
        }
        13 => {
            s.arrange_run("clip.speed.set", json!({ "ids": ids(s), "percent": 80 }));
            wait_made(s);
        }
        14 => {
            s.arrange_run("clip.transpose", json!({ "ids": ids(s), "semis": 2 }));
            wait_made(s);
        }
        15 => {
            s.arrange_run("clip.normalize", json!({ "ids": ids(s), "peak": 0 }));
        }
        16 => {
            s.arrange_run("clip.reverse", json!({ "ids": ids(s) }));
            wait_made(s);
        }
        17 => {
            s.job_start("clip.bounce", json!({ "ids": ids(s), "new-track": true }));
            s.job_wait();
            s.rebuild(g);
        }
        18 => {
            s.job_start("track.freeze", json!({ "ids": [t.to_string()] }));
            s.job_wait();
            s.rebuild(g);
        }
        19 => {
            s.arrange_run("clip.reverse", json!({ "ids": ids(s) }));
        }
        20 => {
            // Two bars at 140, past the loop.
            let hat = write_loop("hat_140bpm.wav", 140.0, 2.0 * 240.0 / 140.0);
            let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
            let y = s.row_y(g, &row) + row.h / 2.0;
            let bar = i64::from(s.project().ppq) * 4;
            let x = s.tick_x(g, 16 * bar);
            s.drop_files(g, x, y, std::slice::from_ref(&hat));
            wait_peaks(s, &hat);
        }
        _ => {}
    }
}

/// The loop at bar 13 of Vocal, made by the command the browser's drop runs, at the file's
/// tempo; the player's session started (silent) so the clip can say how its sound is made.
fn setup(s: &mut Studio, g: &Geometry) {
    let ppq = i64::from(s.project().ppq);
    let bar = ppq * 4;
    s.arrange_run("tempo.set", json!({ "at": 0, "bpm": 120 }));
    s.beat_w = (g.lanes.w / 18.0 / 4.0).max(8.0);
    s.scroll.0 = 0.0;
    let path = write_loop("drumloop_100bpm.wav", LOOP_BPM, LOOP_SECS);
    let vocal = track(s, VOCAL);
    s.arrange_run(
        "clip.add.audio",
        json!({ "track": vocal.to_string(), "at": 12 * bar, "length": 4 * bar,
                "file": path.to_string_lossy() }),
    );
    s.arrange_run("clip.tempo.set", json!({ "ids": ids(s), "bpm": LOOP_BPM }));
    let (p, dir) = (s.project(), s.song().dir());
    let _ = s.transport.start_host(&p, dir);
    wait_peaks(s, &loop_path());
}

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    let p = at(p);
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: p,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position: p,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position: p,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

/// Presses the row `label` of the menu that is open.
fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    let p = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let layout = s.popup.as_ref().expect("menu").layout(g.size);
        let cell = layout
            .cells
            .iter()
            .find(|c| c.text == label && !matches!(c.look, crate::popup::Look::Input))
            .unwrap_or_else(|| panic!("missing row {label}"));
        assert!(cell.live(), "disabled row {label}");
        (
            cell.rect.x + cell.rect.w / 2.0,
            cell.rect.y + cell.rect.h / 2.0,
        )
    };
    press(p, MouseButton::Left, window, cx);
}

/// Plays the stages of [`AUDIOPROC`] that press the window.
fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let spot = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        match n {
            1 => {
                let (t, c) = the_loop(&s);
                let r = rect_of(&s, &g, t, c);
                Some((r.x + r.w * 0.4, r.y + r.h * 0.5))
            }
            5 => Some(crate::arrange::probe::row_mid(
                &s,
                &g,
                RowKind::Track(track(&s, VOCAL)),
            )),
            _ => None,
        }
    };
    if let Some(spot) = spot {
        press(spot, MouseButton::Right, window, cx);
        assert!(studio.borrow().context_open(), "the menu opened");
        return;
    }
    match n {
        2 => choose(studio, "処理", window, cx),
        3 => choose(studio, "曲のテンポに合わせる", window, cx),
        4 => {
            choose(studio, "バウンス", window, cx);
            choose(studio, "新しいトラックにバウンス", window, cx);
            let s = studio.borrow();
            assert!(s.job.is_some(), "the bounce runs");
            let g = crate::screen::geometry(&s, window);
            assert_eq!(
                crate::bounce_row::stop_rects(&s, &g).len(),
                1,
                "the new track has its place"
            );
        }
        7 => {
            let stop = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                crate::bounce_row::stop_rects(&s, &g)[0]
            };
            press(
                (stop.x + stop.w / 2.0, stop.y + stop.h / 2.0),
                MouseButton::Left,
                window,
                cx,
            );
            let s = studio.borrow();
            let job = s.job.as_ref().expect("it ends on the next frame");
            assert!(job.progress.cancelled(), "the square stopped it");
        }
        6 => {
            choose(studio, "トラックをフリーズ", window, cx);
            assert!(studio.borrow().job.is_some(), "the freeze runs");
        }
        _ => {}
    }
}
