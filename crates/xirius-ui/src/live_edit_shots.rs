//! Own parked window, silent output and fixture editor during held live work.
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
use xirius_session::output::{DeviceInput, Drivers, Output, OutputMode, SilentOutput};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "live-00-before",
        acts: &[Act::Stage(Play::Studio(stage), 0)],
    },
    Shot {
        name: "live-01-held",
        acts: &[],
    },
    Shot {
        name: "live-02-moved",
        acts: &[Act::Stage(Play::Window(window_op), 10)],
    },
    Shot {
        name: "live-03-resized",
        acts: &[Act::Stage(Play::Window(window_op), 11)],
    },
    Shot {
        name: "live-04-restored",
        acts: &[Act::Stage(Play::Window(window_op), 12)],
    },
    Shot {
        name: "live-05-finished",
        acts: &[Act::Stage(Play::Studio(stage), 1)],
    },
];

struct Gate {
    release: Arc<(Mutex<bool>, Condvar)>,
    entered: mpsc::Receiver<std::thread::ThreadId>,
    _observe: xirius_core::io_test::Observation,
}
impl Gate {
    fn new() -> Self {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let held = release.clone();
        let (tx, entered) = mpsc::channel();
        let once = std::sync::atomic::AtomicBool::new(false);
        let wanted = operation();
        let ui = std::thread::current().id();
        let observe = xirius_core::io_test::observe(Arc::new(move |op, _| {
            if op == wanted && !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
                assert_ne!(
                    std::thread::current().id(),
                    ui,
                    "live I/O must not run on UI"
                );
                tx.send(std::thread::current().id()).unwrap();
                drop(
                    held.1
                        .wait_while(held.0.lock().unwrap(), |done| !*done)
                        .unwrap(),
                );
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
impl Drop for Gate {
    fn drop(&mut self) {
        *self.release.0.lock().unwrap() = true;
        self.release.1.notify_all();
    }
}
struct Seed {
    dir: PathBuf,
    wav: PathBuf,
    catalog: Arc<xirius_plugins::Catalog>,
}
struct Probe {
    seed: Option<std::thread::JoinHandle<Seed>>,
    fixture: Option<Seed>,
    gate: Option<Gate>,
    phase: u8,
    editor: isize,
    window_task: Option<std::thread::JoinHandle<()>>,
    viewport: Option<(f32, f32)>,
}
thread_local! { static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) }; }
fn operation() -> String {
    std::env::var("XIRIUS_LIVE_GATE").unwrap()
}
fn fixture_plugin(s: &Studio) -> xirius_core::model::Plugin {
    s.project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|p| p.format == "vst3")
        .unwrap()
        .clone()
}
fn stage(s: &mut Studio, n: u8) {
    if n == 0 {
        let seed = std::thread::spawn(|| {
            let root = PathBuf::from(std::env::var_os("XIRIUS_LIVE_SHOTS_DIR").unwrap());
            let dll = std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("deps/xirius_test_vst3.dll");
            let catalog = Arc::new(xirius_plugins::Catalog {
                entries: xirius_plugins::probe(&dll).unwrap(),
                failures: Vec::new(),
            });
            let mut song = crate::song::Song::empty();
            let dir = root.join("song");
            song.save_as(&dir).unwrap();
            let track = song
                .run("track.add.audio", serde_json::json!({"name":"Live"}))
                .unwrap()["id"]
                .clone();
            let base = root.join("base.wav");
            let audio = vec![0.1; 960_000];
            xirius_session::export::write_wav(&base, &audio, &audio, 48_000, 16).unwrap();
            song.run(
                "clip.import",
                serde_json::json!({"file":base,"track":track}),
            )
            .unwrap();
            if operation() == "export_metadata" {
                xirius_session::jobs::Store::of_song(&dir)
                    .write(&serde_json::json!({
                        "job":"export-fixture", "command":"export.mixdown", "state":"done",
                        "result":{"path":base}, "args":{}
                    }))
                    .unwrap();
            }
            song.run("plugin.add", serde_json::json!({"track":track,"name":"Live Fixture","format":"vst3","uid":catalog.entries[0].uid})).unwrap();
            let wav = root.join("new-material.wav");
            let prime = operation() == "prepare_prime";
            let audio = vec![0.1; if prime { 4_200_000 } else { 4096 }];
            xirius_session::export::write_wav(
                &wav,
                &audio,
                &audio,
                if prime { 48_000 } else { 22_050 },
                16,
            )
            .unwrap();
            song.session_mut().flush_writes();
            Seed { dir, wav, catalog }
        });
        PROBE.with(|p| {
            *p.borrow_mut() = Some(Probe {
                seed: Some(seed),
                fixture: None,
                gate: None,
                phase: 0,
                editor: 0,
                window_task: None,
                viewport: None,
            })
        });
    } else {
        PROBE.with(|p| {
            let mut p = p.borrow_mut();
            let p = p.as_mut().unwrap();
            p.gate.take();
            p.phase = 5;
        });
        let _ = s;
    }
}
pub(crate) fn ready(s: &Rc<RefCell<Studio>>) -> bool {
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        let Some(p) = p.as_mut() else { return true };
        if let Some(task) = &p.window_task {
            if !task.is_finished() { return false; }
            p.window_task.take().unwrap().join().unwrap();
            assert_eq!(crate::park::own_handle("Live Fixture"), Some(p.editor));
            assert!(s.borrow().editor_shown(fixture_plugin(&s.borrow()).id));
        }
        if let Some(seed) = &p.seed {
            if !seed.is_finished() { return false; }
            let seed = p.seed.take().unwrap().join().unwrap();
            let mut s = s.borrow_mut();
            s.transport.set_catalog(seed.catalog.clone());
            s.transport.mode = OutputMode::Device;
            s.transport.set_drivers(Drivers {
                exclusive_out: None,
                shared_out: Arc::new(|audio, _, _| Ok(Output::Silent(SilentOutput::start(audio, true)))),
                exclusive_in: None,
                shared_in: Arc::new(|rate, _, _| {
                    xirius_core::io_test::before("input_open", std::path::Path::new("fixture-input")).map_err(|e| e.to_string())?;
                    Ok(DeviceInput::new("fixture".into(), false,
                        Arc::new(std::sync::atomic::AtomicU64::new(1)),
                        Box::new(xirius_engine::record::MockInput::sine(f64::from(rate), 220.0, 0.1))))
                }),
                devices: Arc::new(Vec::new),
            });
            s.start_file_request(FileRequest::Open, &seed.dir);
            eprintln!("live-probe seed loaded");
            p.fixture = Some(seed); p.phase = 1;
            return false;
        }
        if p.phase == 1 {
            let mut s = s.borrow_mut();
            if s.opening.is_some() || s.preparing.is_some() { return false; }
            s.use_plugins((*p.fixture.as_ref().unwrap().catalog).clone());
            eprintln!("live-probe song opened");
            let plugin = fixture_plugin(&s);
            let editor = s.open_vst3_window(&plugin, "Live Fixture".into(), None).unwrap();
            s.mixer.editors.push(editor);
            assert!(s.toggle_play());
            eprintln!("live-probe editor requested");
            p.phase = 2;
            return false;
        }
        if p.phase == 2 {
            let mut s = s.borrow_mut();
            let plugin = fixture_plugin(&s);
            assert!(s.mixer.editors.iter().any(|e| e.id == plugin.id), "initial editor intent was lost");
            for (_, _, status) in s.transport.host().unwrap().plugin_statuses() {
                assert!(status.error.is_none(), "editor error: {:?}", status.error);
            }

            if !s.transport.host().unwrap().plugin_statuses().iter().any(|(id, _, status)| *id == plugin.id && status.editor_open) {
                return false;
            }
            let Some(editor) = crate::park::own_handle("Live Fixture") else { return false; };
            assert!(!crate::park::is_foreground(editor));
            p.editor = editor;
            if operation() == "export_metadata" && s.export.jobs.is_empty() {
                s.reload_exports();
                return false;
            }
            eprintln!("live-probe editor found");
            p.gate = Some(Gate::new());
            let track = s.project().tracks.values().find(|t| !t.inserts.is_empty()).unwrap().id;
            match operation().as_str() {
                "log_write" => { crate::ancillary_io::log!("PRJ-031 log fixture"); }
                "view_read" => { s.read_view(p.fixture.as_ref().unwrap().dir.join("view.json")); }
                "version_list" => { s.song.invalidate_file_reads(); s.song.versions(); }
                "cleanup_list" => { s.open_cleanup(); }
                "groove_read" => { s.import_groove(&p.fixture.as_ref().unwrap().wav).unwrap(); }
                "notice_read" => { s.take_left(&p.fixture.as_ref().unwrap().dir.join("cleaned.txt")); }
                "watch_register" | "external_project_read" => { s.other=Default::default(); }
                "crash_reports" => {
                    let g=s.geometry(1200.0,800.0);
                    let anchor=s.about_anchor(&g);
                    s.open_about(anchor);
                    s.about_action(crate::about::Act::Crashes);
                }
                "update_startup" => {
                    let check=xirius_session::update::UpdateCheck::in_dir(&p.fixture.as_ref().unwrap().dir,Arc::new(|_|Err("fixture".into())));
                    s.check_for_update(check,0);
                }
                "input_close" => {
                    struct Input;
                    impl xirius_engine::record::AudioInput for Input {
                        fn read(&mut self, l: &mut [f32], r: &mut [f32]) { l.fill(0.0); r.fill(0.0); }
                        fn close(self: Box<Self>) {
                            let _ = xirius_core::io_test::before("input_close", std::path::Path::new("fixture-input"));
                        }
                    }
                    let input = DeviceInput::new("Fixture".into(), false, Arc::new(std::sync::atomic::AtomicU64::new(1)), Box::new(Input));
                    let host = s.transport.host_mut().unwrap();
                    assert!(host.watch_start(xirius_session::record::Input::Given(Box::new(input))).unwrap() > 0);
                    host.watch_stop();
                }
                "export_metadata" => {
                    s.export.files = Default::default();
                    s.poll_export_files();
                }
                "live_midi_close" => {
                    struct Midi;
                    impl xirius_engine::record::MidiInput for Midi {
                        fn read(&mut self, _: usize, _: &mut dyn FnMut(usize, [u8; 3])) {}
                    }
                    impl Drop for Midi {
                        fn drop(&mut self) {
                            let _ = xirius_core::io_test::before("live_midi_close", std::path::Path::new("fixture-midi"));
                        }
                    }
                    s.live_midi_input(Box::new(Midi));
                    s.invalidate_midi_open();
                }
                "input_open" | "record_writer" | "record_directory" => {
                    s.song.toggle_arm(track).unwrap();
                    s.toggle_record();
                }
                "settings_write" => {
                    // Screenshot scripts intentionally keep settings in memory.
                    // Exercise a real file in this probe's private fixture.
                    s.song.settings().lock().unwrap().dir = Some(p.fixture.as_ref().unwrap().dir.join("prefs"));
                    s.io_command_on_worker("prefs.set", serde_json::json!({"key":"ui.theme", "value":"light"}).as_object().unwrap().clone());
                }
                "live_midi_open" => {
                    struct Midi;
                    impl xirius_engine::record::MidiInput for Midi {
                        fn ports(&self) -> Vec<String> { vec!["Fixture".into()] }
                        fn read(&mut self, _: usize, _: &mut dyn FnMut(usize, [u8; 3])) {}
                    }
                    s.midi_opener(|| {
                        xirius_core::io_test::before("live_midi_open", std::path::Path::new("fixture-midi")).map_err(|e| e.to_string())?;
                        Ok(Box::new(Midi))
                    });
                    s.midi_ports_now(vec!["Fixture".into()]);
                }
                "prepare_get_state" => {
                    s.song.run("plugin.add", serde_json::json!({"track":track.to_string(),"name":"Gain","kind":"effect","format":"builtin","uid":"gain"})).unwrap();
                    let (project, dir) = (s.project(), s.song.dir());
                    s.transport.sync(&project, dir);
                }
                _ => {
                    s.song.run("clip.add.audio", serde_json::json!({"track":track.to_string(),"file":p.fixture.as_ref().unwrap().wav,"at":1920,"length":1920})).unwrap();
                    let (project, dir) = (s.project(), s.song.dir());
                    s.transport.sync(&project, dir);
                }
            }
            eprintln!("live-probe work requested {}", operation());
            p.phase = 3;
            return false;
        }
        if p.phase == 3 {
            if p.gate.as_ref().unwrap().entered.try_recv().is_err() { return false; }
            let mut s = s.borrow_mut();
            if operation() == "export_metadata" {
                assert!(s.export_confirm_trash("export-fixture", crate::studio::Rect { x: 20.0, y: 20.0, w: 10.0, h: 10.0 }));
            }
            if matches!(operation().as_str(), "input_open" | "record_writer" | "record_directory") {
                s.stop_transport();
            } else {
                assert!(s.transport.playing);
            }
            assert!(s.preparing.is_none());
            assert!(s.editor_shown(fixture_plugin(&s).id));
            assert_eq!(crate::park::own_handle("Live Fixture"), Some(p.editor));
            eprintln!("live-check stage=held ok=true");
            p.phase = 4;
        }
        if p.phase == 5 {
            let mut s = s.borrow_mut();
            if s.transport.host().unwrap().swap_pending() || s.transport.take.is_some() || s.saving.is_some() { return false; }
            if operation() == "live_midi_open" && s.live_midi.input.is_none() { return false; }
            if operation() == "live_midi_close" && s.live_midi.retiring.iter().any(|job| !job.finished()) { return false; }
            assert!(s.preparing.is_none());
            assert!(s.editor_shown(fixture_plugin(&s).id));
            assert!(crate::park::own_handle("Live Fixture").is_some());
            if !matches!(operation().as_str(), "input_open" | "record_writer" | "record_directory") {
                assert!(s.transport.playing);
            }
            if operation() != "prepare_get_state" {
                assert_eq!(crate::park::own_handle("Live Fixture"), Some(p.editor), "unaffected SDK/editor survives");
            }
            if operation() == "prepare_decode" {
                // Also prove a successful (ungated) replacement hands the open
                // editor to its new owner without stopping live playback.
                let track = s.project().tracks.values().find(|t| !t.inserts.is_empty()).unwrap().id;
                s.song.run("plugin.add", serde_json::json!({"track":track.to_string(),"name":"Gain","kind":"effect","format":"builtin","uid":"gain"})).unwrap();
                let (project, dir) = (s.project(), s.song.dir());
                s.transport.sync(&project, dir);
                p.phase = 6;
                return false;
            }
            eprintln!("live-check stage=finished ok=true");
            p.phase = 7;
        }
        if p.phase == 6 {
            let s = s.borrow();
            if s.transport.host().unwrap().swap_pending() { return false; }
            let Some(editor) = crate::park::own_handle("Live Fixture").filter(|hwnd| *hwnd != p.editor) else { return false; };
            assert!(s.editor_shown(fixture_plugin(&s).id));
            assert!(s.transport.playing);
            assert!(s.error.is_none(), "{:?}", s.error);
            assert!(!crate::park::is_foreground(editor));
            eprintln!("live-check stage=handoff ok=true");
            eprintln!("live-check stage=finished ok=true");
            p.phase = 7;
        }
        true
    })
}
fn window_op(n: u8, _: &Rc<RefCell<Studio>>, window: &mut gpui::Window, _: &mut gpui::App) {
    let viewport = window.viewport_size();
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        let p = p.as_mut().unwrap();
        if n == 11 {
            p.viewport = Some((f32::from(viewport.width), f32::from(viewport.height)));
        }
        if n == 12 {
            let (w, h) = p.viewport.unwrap();
            assert!(f32::from(viewport.width) > w && f32::from(viewport.height) > h);
        }
        let hwnd = crate::park::own_handle(crate::screen::WINDOW_TITLE).unwrap();
        p.window_task = Some(std::thread::spawn(move || {
            crate::prepared_shots::native_window_operation(n, hwnd)
        }));
    });
    window.refresh();
}
