//! UI-288: saves through the real window's key, menu and toast input paths.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, KeyDownEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    PlatformInput, Window, point, px,
};
use serde_json::json;

use crate::keep::SaveMark;
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::{Hit, Studio};
use crate::topbar::BarButton;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(begin), n)
}
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "saving01-ctrl-s",
        acts: &[stage(0)],
    },
    Shot {
        name: "saving02-saved",
        acts: &[stage(1)],
    },
    Shot {
        name: "saving03-menu-slow-save",
        acts: &[stage(2)],
    },
    Shot {
        name: "saving04-copy-progress",
        acts: &[stage(3)],
    },
    Shot {
        name: "saving05-esc-cancelled",
        acts: &[stage(4)],
    },
    Shot {
        name: "saving06-save-as-done",
        acts: &[stage(5)],
    },
    Shot {
        name: "saving07-failure",
        acts: &[stage(6)],
    },
    Shot {
        name: "saving08-retry-failed-once",
        acts: &[stage(7)],
    },
    Shot {
        name: "saving09-dismissed",
        acts: &[stage(8)],
    },
    Shot {
        name: "saving10-recovered",
        acts: &[stage(9)],
    },
    Shot {
        name: "saving11-retry-button",
        acts: &[stage(10)],
    },
    Shot {
        name: "saving12-close-drain",
        acts: &[stage(11)],
    },
    Shot {
        name: "saving13-drained",
        acts: &[stage(12)],
    },
    Shot {
        name: "saving14-history-failure",
        acts: &[stage(13)],
    },
    Shot {
        name: "saving15-autosave-failure",
        acts: &[stage(14)],
    },
    Shot {
        name: "saving16-toast-save-as",
        acts: &[stage(15)],
    },
    Shot {
        name: "saving17-cancel-button",
        acts: &[stage(16)],
    },
];

struct State {
    stage: u8,
    entered: Instant,
    started: Option<Instant>,
    reported: bool,
    root: PathBuf,
    toast_since: Option<Instant>,
    save_started: Option<Instant>,
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

fn click_at(window: &mut Window, cx: &mut App, x: f32, y: f32) {
    let position = point(px(x), px(y + xirius_design::Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn click(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App, target: Target) {
    let (x, y) = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.center_of(&g, crate::shot::resolve(&s, target).expect("save target"))
            .expect("save position")
    };
    click_at(window, cx, x, y);
}

fn toast(
    studio: &Rc<RefCell<Studio>>,
    window: &mut Window,
    cx: &mut App,
    part: crate::toast::ToastPart,
) {
    let (x, y) = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.center_of(&g, Hit::Toast(part)).expect("toast position")
    };
    click_at(window, cx, x, y);
}

fn begin(n: u8, studio: &Rc<RefCell<Studio>>, _window: &mut Window, _cx: &mut App) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.is_none() {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("temporary")
                .join(format!("saving-window-{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let mut s = studio.borrow_mut();
            s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
            // A valid silent WAV; no microphone, MIDI device or speaker is opened.
            let bytes = 8 * 1024 * 1024u32;
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
                .run("clip.import.place", json!({"file":audio,"length":1920}))
                .unwrap();
            s.song.save_as(&root.join("original")).unwrap();
            let project = s.song.project();
            s.keep.sync(&project);
            *state = Some(State {
                stage: n,
                entered: Instant::now(),
                started: None,
                reported: false,
                root,
                toast_since: None,
                save_started: None,
            });
        }
        let state = state.as_mut().unwrap();
        state.stage = n;
        state.entered = Instant::now();
        state.started = None;
        state.reported = false;
    });
}

/// The picture waits for a real clock/worker condition; it never blocks the window thread.
pub(crate) fn ready(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) -> bool {
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let Some(state) = state.as_mut() else {
            return true;
        };
        assert!(
            state.entered.elapsed() < Duration::from_secs(30),
            "save window stage {} timed out",
            state.stage
        );
        if state.started.is_none() {
            if !matches!(state.stage, 4 | 16) && studio.borrow().saving.is_some() {
                return false;
            }
            state.started = Some(Instant::now());
            match state.stage {
                0 => {
                    state.save_started = state.started;
                    key(window, cx, "ctrl-s");
                }
                1 => {}
                2 => {
                    studio.borrow_mut().save_test_delay = Duration::from_millis(600);
                    click(studio, window, cx, Target::Top(BarButton::SongName));
                    click(studio, window, cx, Target::PopupText("保存"));
                }
                3 | 5 => {
                    studio.borrow_mut().save_test_delay = Duration::from_millis(75);
                    let destination = state.root.join(if state.stage == 3 {
                        "cancelled"
                    } else {
                        "completed"
                    });
                    studio.borrow_mut().save_test_destination = Some(destination.clone());
                    click(studio, window, cx, Target::Top(BarButton::SongName));
                    click(studio, window, cx, Target::PopupText("別名で保存…"));
                    assert_eq!(
                        studio
                            .borrow()
                            .saving
                            .as_ref()
                            .and_then(|save| save.target.as_ref()),
                        Some(&destination)
                    );
                }
                4 => {
                    let original = studio.borrow().project();
                    key(window, cx, "ctrl-z");
                    click(studio, window, cx, Target::Mute("Drums"));
                    assert_eq!(*studio.borrow().project(), *original, "edits blocked");
                    key(window, cx, "space");
                    assert!(studio.borrow().transport.playing);
                    key(window, cx, "space");
                    assert!(!studio.borrow().transport.playing);
                    key(window, cx, "escape");
                }
                6 => {
                    studio.borrow_mut().save_test_delay = Duration::ZERO;
                    let path = state.root.join("completed/project.json");
                    std::fs::rename(&path, path.with_extension("backup")).unwrap();
                    std::fs::create_dir(&path).unwrap();
                    key(window, cx, "ctrl-s");
                }
                7 => {
                    state.toast_since = studio.borrow().toast.as_ref().map(|t| t.since);
                    toast(studio, window, cx, crate::toast::ToastPart::Also);
                }
                8 => {
                    toast(studio, window, cx, crate::toast::ToastPart::Close);
                    key(window, cx, "ctrl-s");
                }
                9 => {
                    let path = state.root.join("completed/project.json");
                    std::fs::remove_dir(&path).unwrap();
                    std::fs::rename(path.with_extension("backup"), &path).unwrap();
                    key(window, cx, "ctrl-s");
                }
                10 => {
                    let error = xirius_core::error::CmdError::from(std::io::Error::from(
                        std::io::ErrorKind::StorageFull,
                    ));
                    studio.borrow_mut().save_failed(&error);
                    toast(studio, window, cx, crate::toast::ToastPart::Also);
                }
                11 => {
                    let mut s = studio.borrow_mut();
                    s.song
                        .session()
                        .simulate_slow_disk(Duration::from_millis(350));
                    for i in 0..4 {
                        s.run_command("track.add.audio", json!({"name":format!("pending {i}")}));
                    }
                    drop(s);
                    // The same close request the native caption calls, without closing this
                    // picture window before its waiting card has been captured.
                    key(window, cx, "ctrl-w");
                }
                12 => {
                    studio.borrow_mut().window_ask = None;
                }
                13 => {
                    let mut s = studio.borrow_mut();
                    s.leaving_ready = false;
                    s.song.session().simulate_slow_disk(Duration::ZERO);
                    let next = s.song.session().cursor() + 1;
                    std::fs::create_dir(state.root.join(format!("completed/history/{next}.json")))
                        .unwrap();
                    drop(s);
                    click(studio, window, cx, Target::Mute("Drums"));
                }
                14 => {
                    let mut s = studio.borrow_mut();
                    let cursor = s.song.session().cursor();
                    std::fs::remove_dir(
                        state.root.join(format!("completed/history/{cursor}.json")),
                    )
                    .unwrap();
                    s.save_succeeded();
                    let versions = state.root.join("completed/versions");
                    if versions.is_dir() {
                        std::fs::rename(&versions, state.root.join("versions-backup")).unwrap();
                    }
                    std::fs::write(state.root.join("completed/versions"), b"blocked").unwrap();
                    s.keep_beat(xirius_core::store::now_ms() + 31_000);
                }
                15 => {
                    std::fs::remove_file(state.root.join("completed/versions")).unwrap();
                    let backup = state.root.join("versions-backup");
                    if backup.exists() {
                        std::fs::rename(backup, state.root.join("completed/versions")).unwrap();
                    }
                    studio.borrow_mut().save_test_delay = Duration::from_millis(75);
                    studio.borrow_mut().save_test_destination =
                        Some(state.root.join("button-cancelled"));
                    toast(studio, window, cx, crate::toast::ToastPart::Button);
                    assert!(studio.borrow().saving.is_some());
                }
                16 => {
                    let g = crate::screen::geometry(&studio.borrow(), window);
                    let r = crate::wait::save_cancel_rect(&g);
                    click_at(window, cx, r.x + r.w / 2.0, r.y + r.h / 2.0);
                }
                _ => unreachable!(),
            }
        }
        let s = studio.borrow();
        let elapsed = state.started.unwrap().elapsed();
        if state.stage == 1 && state.save_started.unwrap().elapsed() < Duration::from_millis(450) {
            assert_eq!(
                s.save_mark(),
                SaveMark::Saving,
                "Ctrl+S must stay visible for 450 ms"
            );
        }
        let done = match state.stage {
            0 => elapsed >= Duration::from_millis(80) && s.save_mark() == SaveMark::Saving,
            1 => {
                s.saving.is_none()
                    && s.save_mark() == SaveMark::Saved
                    && state.save_started.unwrap().elapsed() >= Duration::from_millis(450)
            }
            2 | 11 => s.saving.as_ref().is_some_and(|job| job.veil()),
            3 | 15 => s.saving.as_ref().is_some_and(|job| {
                job.veil() && job.progress.fraction() > 0.0 && job.progress.fraction() < 1.0
            }),
            4 => {
                s.saving.is_none()
                    && !state.root.join("cancelled").exists()
                    && s.song.dir() == Some(state.root.join("original"))
            }
            5 => {
                s.saving.is_none()
                    && s.project().name == "completed"
                    && s.save_mark() == SaveMark::Saved
            }
            6 => {
                s.saving.is_none()
                    && s.save_mark() == SaveMark::Failed
                    && s.toast.is_some()
                    && s.error.is_none()
            }
            7 => {
                s.saving.is_none()
                    && s.save_mark() == SaveMark::Failed
                    && s.toast.as_ref().map(|t| t.since) == state.toast_since
            }
            8 => s.saving.is_none() && s.save_mark() == SaveMark::Failed && s.toast.is_none(),
            9 | 10 => s.saving.is_none() && s.save_mark() == SaveMark::Saved && s.toast.is_none(),
            12 => s.saving.is_none() && s.leaving_ready,
            13 | 14 => {
                s.save_mark() == SaveMark::Failed
                    && s.toast.as_ref().is_some_and(|t| t.alert)
                    && s.error.is_none()
            }
            16 => {
                s.saving.is_none()
                    && !state.root.join("button-cancelled").exists()
                    && s.song.dir() == Some(state.root.join("completed"))
            }
            _ => false,
        };
        if done && !state.reported {
            eprintln!(
                "saving-check stage={} ok=true elapsed_ms={}",
                state.stage,
                elapsed.as_millis()
            );
            state.reported = true;
        }
        done
    })
}
