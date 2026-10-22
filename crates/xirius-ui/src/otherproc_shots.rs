//! UI-289: another app saves the song open in the real window. The other app is the real
//! `xirius` program, run as a process of its own on the song's folder; the window is pressed
//! only through its own input (`dispatch_event`), parked, with a silent output.
//!
//! The program is `XIRIUS_OTHERPROC_CLI`, or `xirius` next to this one; the song's folder goes
//! under `XIRIUS_OTHERPROC_ROOT`, or under `temporary/`.

use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, KeyDownEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PlatformInput, Window, point, px,
};
use serde_json::json;

use crate::other_change::{ASK, LOADED, LOOK, SETTLE};
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::{Hit, Studio};
use crate::toast::ToastPart;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(begin), n)
}
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "otherproc01-loaded",
        acts: &[stage(0)],
    },
    Shot {
        name: "otherproc02-undo-button",
        acts: &[stage(1)],
    },
    Shot {
        name: "otherproc03-ask",
        acts: &[stage(2)],
    },
    Shot {
        name: "otherproc04-closed",
        acts: &[stage(3)],
    },
    Shot {
        name: "otherproc05-asked-again",
        acts: &[stage(4)],
    },
    Shot {
        name: "otherproc06-load-button",
        acts: &[stage(5)],
    },
    Shot {
        name: "otherproc07-ctrl-z",
        acts: &[stage(6)],
    },
    Shot {
        name: "otherproc08-own-save",
        acts: &[stage(7)],
    },
    Shot {
        name: "otherproc09-batch-once",
        acts: &[stage(8)],
    },
    Shot {
        name: "otherproc10-drag-waits",
        acts: &[stage(9)],
    },
    Shot {
        name: "otherproc11-after-drag",
        acts: &[stage(10)],
    },
];

/// The last stage; the test looks for a line from each.
pub const LAST: u8 = 10;

/// The tracks the other app adds in one `xirius batch`, a save after each.
const RUN: [&str; 5] = ["Run 1", "Run 2", "Run 3", "Run 4", "Run 5"];

struct State {
    stage: u8,
    entered: Instant,
    started: Option<Instant>,
    reported: bool,
    dir: PathBuf,
    cli: PathBuf,
    child: Option<Child>,
    /// When the other app's process ended.
    ended: Option<Instant>,
    /// The history's place when the stage began.
    cursor: u64,
    /// The most steps the stage saw added.
    most: u64,
    /// The toast that says the file came in showed in the stage.
    told: bool,
}
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }

fn key(window: &mut Window, cx: &mut App, key: &str) {
    window.dispatch_event(
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
}

fn at(x: f32, y: f32) -> gpui::Point<gpui::Pixels> {
    point(px(x), px(y + xirius_design::Layout::TOP_BAR_H))
}

fn down(window: &mut Window, cx: &mut App, x: f32, y: f32) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at(x, y),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn drag(window: &mut Window, cx: &mut App, x: f32, y: f32) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(x, y),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn up(window: &mut Window, cx: &mut App, x: f32, y: f32) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at(x, y),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn click_hit(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App, hit: Hit) {
    let (x, y) = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.center_of(&g, hit).expect("a place to press")
    };
    down(window, cx, x, y);
    up(window, cx, x, y);
}

fn click(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App, target: Target) {
    let hit = crate::shot::resolve(&studio.borrow(), target).expect("target");
    click_hit(studio, window, cx, hit);
}

/// An empty spot of the lanes, under the tracks and right of the toast.
fn empty_lane(studio: &Rc<RefCell<Studio>>, window: &Window) -> (f32, f32) {
    let g = crate::screen::geometry(&studio.borrow(), window);
    (g.lanes.x + g.lanes.w * 0.7, g.lanes.y + g.lanes.h - 60.0)
}

/// Starts the other app: `xirius --song <dir> <args…>`, its lines on its input.
fn other_app(state: &mut State, args: &[&str], input: Option<String>) {
    let mut command = Command::new(&state.cli);
    command
        .arg("--song")
        .arg(&state.dir)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window of its own.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().expect("the xirius program runs");
    if let Some(text) = input {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(text.as_bytes()).unwrap();
    }
    state.child = Some(child);
    state.ended = None;
}

/// True once the other app's process has ended well.
fn other_ended(state: &mut State) -> bool {
    if let Some(child) = &mut state.child {
        match child.try_wait().unwrap() {
            Some(status) => {
                assert!(status.success(), "xirius ended with {status}");
                state.child = None;
                state.ended = Some(Instant::now());
            }
            None => return false,
        }
    }
    state.ended.is_some()
}

fn names(s: &Studio) -> Vec<String> {
    s.song
        .session()
        .project()
        .tracks
        .values()
        .map(|t| t.name.clone())
        .collect()
}

fn has(s: &Studio, name: &str) -> bool {
    names(s).iter().any(|n| n == name)
}

fn text(s: &Studio) -> Option<&str> {
    s.toast_shown().map(|t| t.text.as_str())
}

fn begin(n: u8, studio: &Rc<RefCell<Studio>>, _window: &mut Window, _cx: &mut App) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.is_none() {
            let root = std::env::var_os("XIRIUS_OTHERPROC_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../temporary")
                        .join(format!("otherproc-window-{}", std::process::id()))
                });
            std::fs::create_dir_all(&root).unwrap();
            let cli = std::env::var_os("XIRIUS_OTHERPROC_CLI")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::current_exe()
                        .unwrap()
                        .with_file_name(format!("xirius{}", std::env::consts::EXE_SUFFIX))
                });
            assert!(cli.is_file(), "no xirius program at {}", cli.display());
            let dir = root.join("song");
            let mut s = studio.borrow_mut();
            *s = Studio::new(crate::song::Song::empty());
            s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
            // Two minutes of a silent WAV, so the song plays all through the stages; no
            // microphone, MIDI device or speaker is opened.
            let bytes = 48_000 * 2 * 120u32;
            let mut wav = Vec::new();
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(bytes + 36).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&48000u32.to_le_bytes());
            wav.extend_from_slice(&96000u32.to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&bytes.to_le_bytes());
            wav.resize(bytes as usize + 44, 0);
            let audio = root.join("silence.wav");
            std::fs::write(&audio, wav).unwrap();
            s.song
                .run(
                    "clip.import.place",
                    json!({"file": audio, "length": 4 * 960 * 50}),
                )
                .unwrap();
            s.song
                .run("track.add.audio", json!({"name": "Vocal"}))
                .unwrap();
            s.song
                .run("track.add.audio", json!({"name": "Guitar"}))
                .unwrap();
            s.song.save_as(&dir).unwrap();
            let project = s.song.project();
            s.keep.sync(&project);
            s.after_edit();
            *state = Some(State {
                stage: n,
                entered: Instant::now(),
                started: None,
                reported: false,
                dir,
                cli,
                child: None,
                ended: None,
                cursor: 0,
                most: 0,
                told: false,
            });
        }
        let state = state.as_mut().unwrap();
        state.stage = n;
        state.entered = Instant::now();
        state.started = None;
        state.reported = false;
    });
}

/// The picture waits for the other app and the watch; it never blocks the window thread.
pub(crate) fn ready(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) -> bool {
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let Some(state) = state.as_mut() else {
            return true;
        };
        assert!(
            state.entered.elapsed() < Duration::from_secs(60),
            "otherproc window stage {} timed out: {:?} {:?}",
            state.stage,
            names(&studio.borrow()),
            text(&studio.borrow()),
        );
        if state.started.is_none() {
            if studio.borrow().saving.is_some() {
                return false;
            }
            state.started = Some(Instant::now());
            state.cursor = studio.borrow().song.session().cursor();
            state.most = 0;
            state.told = false;
            match state.stage {
                0 => {
                    key(window, cx, "space");
                    assert!(studio.borrow().transport.playing, "plays");
                    other_app(state, &["track.add.audio", "--name", "Bass"], None);
                }
                1 => click_hit(studio, window, cx, Hit::Toast(ToastPart::Button)),
                2 => {
                    click(studio, window, cx, Target::Mute("Vocal"));
                    assert!(studio.borrow().song.session().is_dirty());
                    other_app(state, &["track.add.audio", "--name", "Keys"], None);
                }
                3 => click_hit(studio, window, cx, Hit::Toast(ToastPart::Close)),
                4 => other_app(state, &["track.add.audio", "--name", "Pad"], None),
                5 => click_hit(studio, window, cx, Hit::Toast(ToastPart::Button)),
                6 => key(window, cx, "ctrl-z"),
                7 => key(window, cx, "ctrl-s"),
                8 => {
                    let lines: String = RUN
                        .iter()
                        .map(|n| format!("track.add.audio --name \"{n}\"\n"))
                        .collect();
                    other_app(state, &["batch"], Some(lines));
                }
                9 => {
                    let (x, y) = empty_lane(studio, window);
                    down(window, cx, x, y);
                    drag(window, cx, x + 40.0, y - 20.0);
                    assert!(studio.borrow().pointer_held);
                    other_app(state, &["track.add.audio", "--name", "Lead"], None);
                }
                10 => {
                    let (x, y) = empty_lane(studio, window);
                    up(window, cx, x + 40.0, y - 20.0);
                }
                _ => unreachable!(),
            }
        }
        let ended = other_ended(state);
        let s = studio.borrow();
        let elapsed = state.started.unwrap().elapsed();
        let added = s.song.session().cursor().saturating_sub(state.cursor);
        state.most = state.most.max(added);
        state.told |= text(&s) == Some(LOADED);
        // The window plays all along, and no song is opened again (ENG-064's card).
        assert!(
            s.transport.playing,
            "stage {}: playing stopped",
            state.stage
        );
        assert!(
            s.preparing.is_none(),
            "stage {}: prepared again",
            state.stage
        );
        let quiet =
            |since: Option<Instant>| since.is_some_and(|t| t.elapsed() >= LOOK + SETTLE * 2);
        let done = match state.stage {
            0 => ended && text(&s) == Some(LOADED) && has(&s, "Bass") && added == 1,
            1 => !has(&s, "Bass") && text(&s) != Some(LOADED),
            2 => ended && text(&s) == Some(ASK) && !has(&s, "Keys"),
            3 => quiet(state.started) && s.toast.is_none() && !has(&s, "Keys"),
            4 => ended && text(&s) == Some(ASK) && !has(&s, "Keys"),
            5 => {
                text(&s) == Some(LOADED)
                    && has(&s, "Keys")
                    && has(&s, "Pad")
                    && added == 1
                    && !s.song.session().is_dirty()
            }
            6 => !has(&s, "Keys") && !has(&s, "Pad") && s.toast.is_none() && added == 0,
            7 => {
                s.saving.is_none()
                    && quiet(state.started)
                    && s.toast_shown().is_none()
                    && added == 0
                    && !s.song.session().is_dirty()
            }
            8 => {
                ended
                    && quiet(state.ended)
                    && RUN.iter().all(|n| has(&s, n))
                    // The other app went on from the screen's save, not from its own last run.
                    && !has(&s, "Keys")
                    && state.told
                    && state.most == 1
                    && added == 1
            }
            9 => {
                ended
                    && quiet(state.ended)
                    && s.pointer_held
                    && !has(&s, "Lead")
                    && s.toast.is_none()
            }
            10 => !s.pointer_held && text(&s) == Some(LOADED) && has(&s, "Lead") && added == 1,
            _ => false,
        };
        assert!(
            state.most <= 1,
            "stage {}: {} steps for one change",
            state.stage,
            state.most
        );
        if done && !state.reported {
            eprintln!(
                "otherproc-check stage={} ok=true elapsed_ms={} names={:?}",
                state.stage,
                elapsed.as_millis(),
                names(&s)
            );
            state.reported = true;
        }
        if done && state.stage == 0 {
            // The toast goes 4 s after it came; the picture may take longer on a busy machine,
            // so it is held until the next stage presses its button.
            drop(s);
            if let Some(t) = studio.borrow_mut().toast.as_mut() {
                t.since = Instant::now();
            }
        }
        done
    })
}
