//! Release-build probe: actual worker I/O is held at a gate while PrintWindow
//! captures the responsive window. No simulated result or OS input is used.
use crate::{
    shot::{Act, Play, Shot},
    studio::{FileRequest, Studio},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Condvar, Mutex, mpsc},
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "opening-00-before",
        acts: &[Act::Reset],
    },
    Shot {
        name: "opening-01-reading",
        acts: &[Act::Reset, Act::Stage(Play::Studio(stage), 0)],
    },
    Shot {
        name: "opening-02-cancelling",
        acts: &[Act::Stage(Play::Studio(stage), 1)],
    },
    Shot {
        name: "opening-03-lock",
        acts: &[Act::Stage(Play::Studio(stage), 2)],
    },
    Shot {
        name: "opening-04-close",
        acts: &[Act::Stage(Play::Studio(stage), 3)],
    },
    Shot {
        name: "opening-05-returned",
        acts: &[Act::Stage(Play::Studio(stage), 4)],
    },
    Shot {
        name: "opening-06-adopting",
        acts: &[Act::Stage(Play::Studio(stage), 5)],
    },
    Shot {
        name: "opening-07-adopting-close",
        acts: &[Act::Stage(Play::Studio(stage), 3)],
    },
    Shot {
        name: "opening-08-adopted",
        acts: &[Act::Stage(Play::Studio(stage), 6)],
    },
];
struct Gate {
    release: Arc<(Mutex<bool>, Condvar)>,
    entered: mpsc::Receiver<()>,
    _observe: xirius_core::io_test::Observation,
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.release();
    }
}
impl Gate {
    fn release(&self) {
        *self.release.0.lock().unwrap() = true;
        self.release.1.notify_all();
    }
    fn new(path: PathBuf, op: &'static str) -> Self {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let freed = release.clone();
        let (tx, entered) = mpsc::channel();
        let once = AtomicBool::new(false);
        let observe = xirius_core::io_test::observe(Arc::new(move |kind, file| {
            if kind == op
                && file.starts_with(&path)
                && (op != "read" || file.ends_with("project.json"))
                && !once.swap(true, Ordering::SeqCst)
            {
                tx.send(()).unwrap();
                let guard = freed.0.lock().unwrap();
                drop(freed.1.wait_while(guard, |done| !*done).unwrap());
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
    seed: Option<std::thread::JoinHandle<(PathBuf, String)>>,
    path: Option<PathBuf>,
    version: Option<String>,
    gate: Option<Gate>,
    reached: bool,
    phase: u8,
}
thread_local! { static PROBE:RefCell<Option<Probe>>=const{RefCell::new(None)}; }
fn stage(s: &mut Studio, n: u8) {
    match n {
        0 => {
            let seed = std::thread::spawn(|| {
                let base = std::env::var_os("XIRIUS_OPEN_SHOTS_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::temp_dir().join("xirius-opening-probe"));
                let dir = base
                    .join(format!(
                        "run-{}-{}",
                        std::process::id(),
                        xirius_core::store::now_ms()
                    ))
                    .join("長い親フォルダー/alternate-takes/history/春のストリングスとピアノ");
                std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
                let mut session = xirius_core::Session::new("春のストリングスとピアノ");
                session.save_as(&dir).unwrap();
                let version = session.save_version("shot").unwrap();
                drop(session);
                (dir, version.id)
            });
            PROBE.with(|p| {
                *p.borrow_mut() = Some(Probe {
                    seed: Some(seed),
                    path: None,
                    version: None,
                    gate: None,
                    reached: false,
                    phase: 0,
                })
            });
        }
        1 => {
            s.opening_key("escape");
            assert!(s.opening.as_ref().unwrap().cancelled);
        }
        2 => PROBE.with(|p| {
            let mut p = p.borrow_mut();
            let p = p.as_mut().unwrap();
            p.gate.take();
            p.reached = false;
            p.phase = 2;
        }),
        3 => {
            assert!(!s.request_close());
        }
        4 => {
            s.opening_key("escape");
            s.cancel_opening();
            PROBE.with(|p| {
                let mut p = p.borrow_mut();
                let p = p.as_mut().unwrap();
                p.gate.take();
                p.phase = 4;
            });
            crate::wait::script_time(None);
        }
        5 => PROBE.with(|p| {
            let mut p = p.borrow_mut();
            let p = p.as_mut().unwrap();
            p.gate.take();
            p.reached = false;
            p.phase = 5;
        }),
        6 => {
            s.opening_key("escape");
            assert!(s.opening.as_ref().unwrap().adopting);
            PROBE.with(|p| {
                let mut p = p.borrow_mut();
                let p = p.as_mut().unwrap();
                p.gate.take();
                p.phase = 4;
            });
            crate::wait::script_time(None);
        }
        _ => unreachable!(),
    }
}
pub(crate) fn ready(s: &Rc<RefCell<Studio>>) -> bool {
    PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let Some(p) = probe.as_mut() else {
            return s.borrow().opening.is_none();
        };
        if p.phase == 4 {
            return s.borrow().opening.is_none();
        }
        if let Some(seed) = &p.seed {
            if !seed.is_finished() {
                return false;
            }
            let (path, version) = p.seed.take().unwrap().join().unwrap();
            p.path = Some(path);
            p.version = Some(version);
        }
        if p.phase == 5 {
            s.borrow_mut()
                .start_file_request(FileRequest::Open, p.path.as_ref().unwrap());
            p.phase = 6;
            return false;
        }
        if p.phase == 6 {
            if s.borrow().opening.is_some() {
                return false;
            }
            let destination = p
                .path
                .as_ref()
                .unwrap()
                .parent()
                .unwrap()
                .join("published-copy");
            p.gate = Some(Gate::new(destination.clone(), "publish"));
            s.borrow_mut().start_file_request(
                FileRequest::OpenVersion(p.version.as_ref().unwrap().clone()),
                &destination,
            );
            p.phase = 7;
        }
        if p.gate.is_none() {
            if s.borrow().opening.is_some() {
                return false;
            }
            let path = p.path.as_ref().unwrap().clone();
            p.gate = Some(Gate::new(
                path.clone(),
                if p.phase == 2 { "lock" } else { "read" },
            ));
            s.borrow_mut().start_file_request(FileRequest::Open, &path);
        }
        if !p.reached {
            if p.gate.as_ref().unwrap().entered.try_recv().is_err() {
                return false;
            }
            p.reached = true;
            let since = s.borrow().opening.as_ref().unwrap().since;
            crate::wait::script_time(Some(since + std::time::Duration::from_millis(300)));
            s.borrow_mut().poll_opening();
        }
        true
    })
}
