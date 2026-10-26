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

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "prepared-00-before",
        acts: &[Act::Reset],
    },
    Shot {
        name: "prepared-01-reading",
        acts: &[Act::Stage(Play::Studio(stage), 0)],
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
        name: "prepared-04-failed",
        acts: &[Act::Stage(Play::Studio(stage), 3)],
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
        let (tx, entered) = mpsc::channel();
        let once = std::sync::atomic::AtomicBool::new(false);
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
            }
            if op == "prepare_decode"
                && path.starts_with(&root)
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
            entered,
            _observe: observe,
        }
    }
}
struct Probe {
    seed: Option<std::thread::JoinHandle<PathBuf>>,
    gate: Option<Gate>,
    phase: u8,
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
                let wav = root.join("長いトラック名・ストリングスとピアノを重ねた最後のテイク.wav");
                xirius_session::export::write_wav(&wav, &[0.0; 4096], &[0.0; 4096], 22_050, 16)
                    .unwrap();
                song.run("clip.import", serde_json::json!({"file":wav}))
                    .unwrap();
                song.session_mut().flush_writes();
                drop(song);
                let mut failed = crate::song::Song::empty();
                for index in 0..3 {
                    let id = failed.run("track.add.audio", serde_json::json!({"name": "長い同名トラック🎹・ストリングスとピアノを重ねた最後のテイク"})).unwrap()["id"].clone();
                    if index != 1 {
                        failed.run("plugin.add", serde_json::json!({"track":id,"name":"Missing","format":"vst3","uid":"00000000000000000000000000000000"})).unwrap();
                    }
                }
                failed.save_as(&root.join("failed-song")).unwrap();
                failed.session_mut().flush_writes();
                dir
            });
            PROBE.with(|p| {
                *p.borrow_mut() = Some(Probe {
                    seed: Some(seed),
                    gate: None,
                    phase: 0,
                })
            });
        }
        1 => {
            let before = s.project();
            assert!(!s.run_command("track.add.audio", serde_json::json!({"name":"blocked"})));
            assert!(!s.start_save());
            assert!(!s.toggle_play());
            assert!(!s.toggle_record());
            assert!(Arc::ptr_eq(&before, &s.project()));
            PROBE.with(|p| p.borrow_mut().as_mut().unwrap().phase = 2);
        }
        2 => PROBE.with(|p| {
            let mut p = p.borrow_mut();
            let p = p.as_mut().unwrap();
            p.gate.take();
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
fn cancel(n: u8, s: &Rc<RefCell<Studio>>, window: &mut gpui::Window, cx: &mut gpui::App) {
    stage(&mut s.borrow_mut(), n);
    window.dispatch_event(
        gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("escape").unwrap(),
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
    assert!(s.borrow().preparing.as_ref().unwrap().cancelling);
    eprintln!("prepared-check stage=cancel ok=true");
}
pub(crate) fn ready(s: &Rc<RefCell<Studio>>) -> bool {
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        let Some(p) = p.as_mut() else { return true };
        if let Some(seed) = &p.seed {
            if !seed.is_finished() {
                return false;
            }
            let dir = p.seed.take().unwrap().join().unwrap();
            p.gate = Some(Gate::new(dir.parent().unwrap().to_path_buf()));
            let mut s = s.borrow_mut();
            s.transport.mode = OutputMode::Device;
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
            if p.gate.as_ref().unwrap().entered.try_recv().is_err() {
                return false;
            }
            let s = s.borrow();
            assert!(s.preparing.as_ref().unwrap().background);
            assert!(s.transport.host().is_none());
            eprintln!("prepared-check stage=reading ok=true");
            p.phase = 2;
        }
        if p.phase == 3 {
            let s = s.borrow();
            if s.preparing.is_some() || s.opening.is_some() {
                return false;
            }
            assert!(s.song.dir().is_none());
            assert!(s.transport.host().is_none());
            assert!(!s.transport.playing);
            eprintln!("prepared-check stage=closed ok=true");
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
