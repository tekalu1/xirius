//! PRJ-030: an actual material gate behind the parked GPUI window.
use crate::{
    shot::{Act, Play, Shot},
    studio::{FileRequest, Studio},
};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Condvar, Mutex, mpsc},
};
use xirius_session::output::{Drivers, Output, OutputMode, SilentOutput};

#[path = "prepared_fixture_tests.rs"]
mod fixtures;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "prepared-00-before",
        acts: &[Act::Reset],
    },
    Shot {
        name: "prepared-01a-reading",
        acts: &[Act::Stage(Play::Studio(stage), 0)],
    },
    Shot {
        name: "prepared-01b-moved",
        acts: &[Act::Stage(Play::Window(window_operation), 10)],
    },
    Shot {
        name: "prepared-01c-resized",
        acts: &[Act::Stage(Play::Window(window_operation), 11)],
    },
    Shot {
        name: "prepared-01d-restored",
        acts: &[Act::Stage(Play::Window(window_operation), 12)],
    },
    Shot {
        name: "prepared-02-cancelling",
        acts: &[Act::Stage(Play::Window(cancel), 1)],
    },
    Shot {
        name: "prepared-03-closed",
        acts: &[Act::Stage(Play::Studio(stage), 2)],
    },
    Shot {
        name: "prepared-04a-failed",
        acts: &[Act::Stage(Play::Studio(stage), 3)],
    },
    Shot {
        name: "prepared-04b-detail",
        acts: &[Act::Stage(Play::Window(detail), 6)],
    },
    Shot {
        name: "prepared-04c-detail-end",
        acts: &[Act::Stage(Play::Window(detail), 7)],
    },
    Shot {
        name: "prepared-05-retried",
        acts: &[Act::Stage(Play::Window(choice), 4)],
    },
    Shot {
        name: "prepared-06-silent",
        acts: &[Act::Stage(Play::Window(choice), 5)],
    },
];

struct Gate {
    release: Arc<(Mutex<bool>, Condvar)>,
    watch_blocked: Arc<std::sync::atomic::AtomicBool>,
    entered: mpsc::Receiver<()>,
    _observe: xirius_core::io_test::Observation,
}
impl Drop for Gate {
    fn drop(&mut self) {
        *self.release.0.lock().unwrap() = true;
        self.release.1.notify_all();
    }
}
impl Gate {
    fn new(root: PathBuf) -> Self {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let freed = release.clone();
        let watch_blocked = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let watch_guard = watch_blocked.clone();
        let (tx, entered) = mpsc::channel();
        let once = std::sync::atomic::AtomicBool::new(false);
        let held_operation = operation();
        let ui = std::thread::current().id();
        let observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if path.starts_with(&root)
                && once.load(std::sync::atomic::Ordering::SeqCst)
                && std::thread::current().id() == ui
            {
                assert!(
                    !matches!(op, "peaks_load" | "peaks_source" | "browser_read_dir"),
                    "preparation painting requested material I/O: {op}"
                );
                if watch_guard.load(std::sync::atomic::Ordering::Acquire) {
                    assert!(
                        !matches!(op, "watch_register" | "watch_metadata"),
                        "blocked window requested watcher I/O: {op}"
                    );
                }
            }
            if op == held_operation
                && (held_operation == "prepare_get_state" || path.starts_with(&root))
                && !once.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                tx.send(()).unwrap();
                let held = freed.0.lock().unwrap();
                drop(freed.1.wait_while(held, |done| !*done).unwrap());
            }
            Ok(())
        }));
        Self {
            release,
            watch_blocked,
            entered,
            _observe: observe,
        }
    }
}
struct Probe {
    seed: Option<std::thread::JoinHandle<Seed>>,
    gate: Option<Gate>,
    phase: u8,
    restarted: bool,
    window_task: Option<std::thread::JoinHandle<()>>,
    viewport_before_resize: Option<(f32, f32)>,
}
struct Seed {
    dir: PathBuf,
    catalog: Option<Arc<xirius_plugins::Catalog>>,
}
thread_local! { static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) }; }

fn stage(s: &mut Studio, n: u8) {
    match n {
        0 => {
            let seed = std::thread::spawn(|| {
                let root = std::env::var_os("XIRIUS_PREPARED_SHOTS_DIR")
                    .map(PathBuf::from)
                    .expect("isolated preparation fixture directory");
                let mut song = crate::song::Song::empty();
                let dir = root.join("fixture-song");
                song.save_as(&dir).unwrap();
                for index in 0..2 {
                    song.run(
                        "track.add.audio",
                        serde_json::json!({"name":format!("Ready {index}")}),
                    )
                    .unwrap();
                }
                let wav = root.join("長いトラック名・ストリングスとピアノを重ねた最後のテイク.wav");
                let large = operation() == "prepare_prime";
                let audio = vec![0.0; if large { 4_200_000 } else { 4096 }];
                xirius_session::export::write_wav(
                    &wav,
                    &audio,
                    &audio,
                    if large { 48_000 } else { 22_050 },
                    16,
                )
                .unwrap();
                song.run("clip.import", serde_json::json!({"file":wav}))
                    .unwrap();
                for index in 0..9 {
                    song.run(
                        "track.add.audio",
                        serde_json::json!({"name":format!("Waiting {index}")}),
                    )
                    .unwrap();
                }
                let catalog = if operation() == "prepare_get_state" {
                    let fixture = std::env::current_exe()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join("deps/xirius_test_vst3.dll");
                    let catalog = Arc::new(xirius_plugins::Catalog {
                        entries: xirius_plugins::probe(&fixture).unwrap(),
                        failures: Vec::new(),
                    });
                    let track = song
                        .project()
                        .tracks
                        .values()
                        .find(|t| t.kind == xirius_core::model::TrackKind::Audio)
                        .unwrap()
                        .id;
                    song.run("plugin.add", serde_json::json!({"track":track.to_string(),"name":"Fixture","format":"vst3","uid":catalog.entries[0].uid})).unwrap();
                    Some(catalog)
                } else {
                    None
                };
                song.session_mut().flush_writes();
                drop(song);
                let mut failed = crate::song::Song::empty();
                for index in 0..3 {
                    let id = failed
                        .run(
                            "track.add.audio",
                            serde_json::json!({"name": fixtures::long_track_name()}),
                        )
                        .unwrap()["id"]
                        .clone();
                    if index != 1 {
                        failed.run("plugin.add", serde_json::json!({"track":id,"name":"Missing","format":"vst3","uid":"00000000000000000000000000000000"})).unwrap();
                    }
                }
                failed.save_as(&root.join("failed-song")).unwrap();
                failed.session_mut().flush_writes();
                Seed { dir, catalog }
            });
            PROBE.with(|p| {
                *p.borrow_mut() = Some(Probe {
                    seed: Some(seed),
                    gate: None,
                    phase: 0,
                    restarted: false,
                    window_task: None,
                    viewport_before_resize: None,
                })
            });
        }
        1 => {
            let before = s.project();
            assert!(!s.run_command("track.add.audio", serde_json::json!({"name":"blocked"})));
            assert!(!s.start_save());
            if operation() != "prepare_get_state" {
                assert!(!s.toggle_play());
                assert!(!s.toggle_record());
            }
            assert!(Arc::ptr_eq(&before, &s.project()));
            PROBE.with(|p| p.borrow_mut().as_mut().unwrap().phase = 2);
        }
        2 => PROBE.with(|p| {
            let mut p = p.borrow_mut();
            let p = p.as_mut().unwrap();
            p.gate.take();
            crate::wait::script_time(None);
            p.phase = 3;
        }),
        3 => {
            let root = PathBuf::from(std::env::var_os("XIRIUS_PREPARED_SHOTS_DIR").unwrap());
            s.start_file_request(FileRequest::Open, &root.join("failed-song"));
            PROBE.with(|p| p.borrow_mut().as_mut().unwrap().phase = 5);
        }
        _ => unreachable!(),
    }
}
fn choice(n: u8, s: &Rc<RefCell<Studio>>, window: &mut gpui::Window, cx: &mut gpui::App) {
    let button = {
        let s = s.borrow();
        let g = crate::screen::geometry(&s, window);
        let layout = s.preparing.as_ref().unwrap().layout(g.size);
        if n == 4 {
            layout.retry.unwrap()
        } else {
            layout.proceed.unwrap()
        }
    };
    let position = gpui::point(
        gpui::px(button.x + button.w / 2.0),
        gpui::px(button.y + button.h / 2.0 + xirius_design::Layout::TOP_BAR_H),
    );
    window.dispatch_event(
        gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Default::default(),
        }),
        cx,
    );
    window.dispatch_event(
        gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position,
            modifiers: Default::default(),
            click_count: 1,
        }),
        cx,
    );
    PROBE.with(|p| p.borrow_mut().as_mut().unwrap().phase = if n == 4 { 7 } else { 9 });
}

fn detail(n: u8, s: &Rc<RefCell<Studio>>, window: &mut gpui::Window, cx: &mut gpui::App) {
    let row = {
        let s = s.borrow();
        let g = crate::screen::geometry(&s, window);
        s.preparing.as_ref().unwrap().layout(g.size).rows[0].1
    };
    let position = gpui::point(
        gpui::px(row.x + 10.0),
        gpui::px(row.y + 10.0 + xirius_design::Layout::TOP_BAR_H),
    );
    if n == 6 {
        window.dispatch_event(
            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }),
            cx,
        );
        assert!(
            s.borrow()
                .preparing
                .as_ref()
                .unwrap()
                .detail_progress()
                .is_some()
        );
    } else {
        {
            let s = s.borrow();
            let card = s.preparing.as_ref().unwrap();
            assert_eq!(
                card.detail_content().unwrap(),
                format!("{}{}", card.failures[0].reason, card.failures[0].name),
                "wrapping preserves the full Japanese/emoji name and reason"
            );
        }
        let max = s
            .borrow()
            .preparing
            .as_ref()
            .unwrap()
            .detail_progress()
            .unwrap()
            .1;
        assert!(max > 0, "the full long name exceeds the visible detail");
        for _ in 0..=max {
            window.dispatch_event(
                gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.0), gpui::px(-40.0))),
                    modifiers: Default::default(),
                    touch_phase: gpui::TouchPhase::Moved,
                }),
                cx,
            );
        }
        assert_eq!(
            s.borrow().preparing.as_ref().unwrap().detail_progress(),
            Some((max, max))
        );
    }
    eprintln!(
        "prepared-check stage={} ok=true",
        if n == 6 { "detail" } else { "detail-end" }
    );
    window.refresh();
}
fn cancel(n: u8, s: &Rc<RefCell<Studio>>, window: &mut gpui::Window, cx: &mut gpui::App) {
    stage(&mut s.borrow_mut(), n);
    let frozen = s
        .borrow()
        .preparing
        .as_ref()
        .map(|card| card.progress.clone());
    window.dispatch_event(
        gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("escape").unwrap(),
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
    if operation() == "prepare_get_state" {
        assert!(s.borrow().saving.as_ref().unwrap().progress.cancelled());
        // A cancelled save returns to the current song without waiting for the
        // abandoned SDK call. Unlike preparation cleanup, it removes the veil.
        PROBE.with(|p| {
            p.borrow()
                .as_ref()
                .unwrap()
                .gate
                .as_ref()
                .unwrap()
                .watch_blocked
                .store(false, std::sync::atomic::Ordering::Release)
        });
    } else {
        assert!(s.borrow().preparing.as_ref().unwrap().cancelling);
        assert_eq!(
            Some(s.borrow().preparing.as_ref().unwrap().progress.clone()),
            frozen,
            "cancel keeps the count, current track and list until cleanup"
        );
    }
    eprintln!("prepared-check stage=cancel ok=true");
}
pub(crate) fn ready(s: &Rc<RefCell<Studio>>) -> bool {
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        let Some(p) = p.as_mut() else { return true };
        if let Some(task) = &p.window_task {
            if !task.is_finished() {
                return false;
            }
            p.window_task
                .take()
                .unwrap()
                .join()
                .expect("native window operation");
            assert!(
                s.borrow().input_blocked(),
                "the material gate is still closed"
            );
        }
        if let Some(seed) = &p.seed {
            if !seed.is_finished() {
                return false;
            }
            let Seed { dir, catalog } = p.seed.take().unwrap().join().unwrap();
            p.gate = Some(Gate::new(dir.parent().unwrap().to_path_buf()));
            let mut s = s.borrow_mut();
            s.transport.mode = OutputMode::Device;
            if let Some(catalog) = catalog {
                s.transport.set_catalog(catalog);
            }
            s.transport.set_drivers(Drivers {
                exclusive_out: None,
                shared_out: Arc::new(|audio, _, _| {
                    Ok(Output::Silent(SilentOutput::start(audio, true)))
                }),
                exclusive_in: None,
                shared_in: Arc::new(|_, _, _| Err("no probe input".into())),
                devices: Arc::new(Vec::new),
            });
            s.start_file_request(FileRequest::Open, &dir);
            p.phase = 1;
            return false;
        }
        if p.phase == 1 {
            if operation() == "prepare_output_stop" && !p.restarted {
                let mut s = s.borrow_mut();
                if s.preparing.is_some() || s.opening.is_some() || !s.transport.is_live() {
                    return false;
                }
                s.transport.reset();
                s.prepare_song();
                p.restarted = true;
            }
            if operation() == "prepare_get_state" && !p.restarted {
                let mut s = s.borrow_mut();
                if s.preparing.is_some() || s.opening.is_some() || !s.transport.is_live() {
                    return false;
                }
                let root = PathBuf::from(std::env::var_os("XIRIUS_PREPARED_SHOTS_DIR").unwrap());
                crate::wait::script_time(Some(std::time::Instant::now()));
                s.start_file_request(FileRequest::SaveAs, &root.join("state-copy"));
                p.restarted = true;
            }
            if p.gate.as_ref().unwrap().entered.try_recv().is_err() {
                return false;
            }
            let mut s = s.borrow_mut();
            if operation() == "prepare_get_state" {
                s.saving.as_mut().unwrap().since -= crate::wait::OVERLAY_AFTER;
                assert!(s.transport.host().is_some());
            } else {
                assert!(s.preparing.as_ref().unwrap().background);
                assert!(s.transport.host().is_none());
                let card = s.preparing.as_ref().unwrap();
                assert_eq!(card.progress.units.len(), 12);
                let layout = card.layout((880.0, 600.0));
                assert_eq!(layout.rows.len(), 8);
                assert!(layout.rows.last().unwrap().1.bottom() <= layout.button.y);
            }
            eprintln!("prepared-check stage=reading ok=true");
            p.phase = 2;
        }
        if p.phase == 3 {
            let s = s.borrow();
            if s.preparing.is_some() || s.opening.is_some() || s.saving.is_some() {
                return false;
            }
            if operation() == "prepare_get_state" {
                let root = PathBuf::from(std::env::var_os("XIRIUS_PREPARED_SHOTS_DIR").unwrap());
                assert!(!root.join("state-copy").exists());
                assert!(s.song.dir().is_some());
                assert!(s.transport.host().is_some());
            } else {
                assert!(s.song.dir().is_none());
                assert!(s.transport.host().is_none());
            }
            assert!(!s.transport.playing);
            eprintln!(
                "prepared-check stage={} ok=true",
                if operation() == "prepare_get_state" {
                    "save-cancelled"
                } else {
                    "closed"
                }
            );
            p.phase = 4;
        }
        if p.phase == 5 || p.phase == 7 {
            let s = s.borrow();
            let Some(card) = &s.preparing else {
                return false;
            };
            if card.failures.len() != 2 {
                return false;
            }
            assert!(card.failure.is_none());
            assert_eq!(card.progress.done, 1);
            assert!(s.transport.host().is_none());
            eprintln!(
                "prepared-check stage={} ok=true",
                if p.phase == 5 { "failed" } else { "retried" }
            );
            p.phase = if p.phase == 5 { 6 } else { 8 };
        }
        if p.phase == 9 {
            let s = s.borrow();
            if s.preparing.is_some() {
                return false;
            }
            assert_eq!(s.transport.host().unwrap().unavailable_tracks().len(), 2);
            assert!(!s.transport.playing);
            eprintln!("prepared-check stage=silent ok=true");
            p.phase = 10;
        }
        true
    })
}

fn operation() -> String {
    std::env::var("XIRIUS_PREPARED_GATE").unwrap_or_else(|_| "prepare_decode".into())
}

#[cfg(windows)]
fn window_operation(n: u8, s: &Rc<RefCell<Studio>>, window: &mut gpui::Window, _: &mut gpui::App) {
    let viewport = window.viewport_size();
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        let probe = p.as_mut().unwrap();
        if n == 11 {
            probe.viewport_before_resize =
                Some((f32::from(viewport.width), f32::from(viewport.height)));
        } else if n == 12 {
            let (width, height) = probe.viewport_before_resize.unwrap();
            assert!(
                f32::from(viewport.width) > width && f32::from(viewport.height) > height,
                "GPUI must adopt the new client size before the next shot"
            );
        }
        let hwnd = crate::park::own_handle(crate::screen::WINDOW_TITLE)
            .expect("this process's probe window");
        assert!(s.borrow().input_blocked());
        // Calling SetWindowPos inside App::update re-enters GPUI's callback
        // while its context is borrowed. A real user's sizing message arrives
        // outside that callback. Send only to our HWND from a separate thread.
        probe.window_task = Some(std::thread::spawn(move || native_window_operation(n, hwnd)));
    });
    window.refresh();
}

#[cfg(windows)]
fn native_window_operation(n: u8, hwnd: isize) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsIconic, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
        SWP_NOZORDER, SetWindowPos, ShowWindow,
    };
    assert!(!crate::park::is_foreground(hwnd));
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    // SAFETY: own_handle verifies the process; all calls target only this parked
    // fixture window. No activation, global input, or other app's window is used.
    unsafe {
        assert_ne!(GetWindowRect(hwnd as _, &mut rect), 0);
        match n {
            10 | 11 => {
                let width = rect.right - rect.left + if n == 11 { 80 } else { 0 };
                let height = rect.bottom - rect.top + if n == 11 { 40 } else { 0 };
                let x = crate::park::PARK_AT + 200;
                let y = crate::park::PARK_AT + 160;
                assert_ne!(
                    SetWindowPos(
                        hwnd as _,
                        std::ptr::null_mut(),
                        x,
                        y,
                        width,
                        height,
                        SWP_NOACTIVATE | SWP_NOZORDER
                    ),
                    0
                );
                assert_ne!(GetWindowRect(hwnd as _, &mut rect), 0);
                assert_eq!((rect.left, rect.top), (x, y));
                assert_eq!(
                    (rect.right - rect.left, rect.bottom - rect.top),
                    (width, height)
                );
            }
            12 => {
                ShowWindow(hwnd as _, SW_SHOWMINNOACTIVE);
                assert_ne!(IsIconic(hwnd as _), 0);
                ShowWindow(hwnd as _, SW_SHOWNOACTIVATE);
                assert_eq!(IsIconic(hwnd as _), 0);
            }
            _ => unreachable!(),
        }
    }
    assert!(!crate::park::is_foreground(hwnd));
    eprintln!(
        "prepared-check stage={} ok=true",
        match n {
            10 => "moved",
            11 => "resized",
            _ => "restored",
        }
    );
}

#[cfg(not(windows))]
fn window_operation(_: u8, _: &Rc<RefCell<Studio>>, _: &mut gpui::Window, _: &mut gpui::App) {}
