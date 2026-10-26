//! Watching the input of the armed tracks before they record (REC-002, REC-011, REC-012): the
//! level of each channel is measured with the transport stopped, the track's input gain trims it,
//! the input monitor makes it heard on the output and nothing else does, and a recording that
//! starts takes the watched input over. The inputs are stand-ins; no device is opened and
//! nothing is played to the speakers (the test renders the engine into buffers).

use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::AudioEngine;
use xirius_engine::record::MockInput;
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::record::{Input, RecordOptions};
use xirius_session::watch::input_plan;

const RATE: f64 = 48_000.0;

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    commands::run(s, None, name, &map(args), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn armed_audio(s: &mut Session, name: &str) -> String {
    let r = run(s, "track.add.audio", json!({ "name": name }));
    let t = r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .expect("an id")
        .to_string();
    run(s, "track.arm", json!({ "ids": [t], "on": true }));
    t
}

fn session() -> Session {
    let mut s = Session::with_registry(Project::new("watch"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    s
}

struct Rig {
    host: SessionHost,
    audio: AudioEngine,
}

impl Rig {
    fn new(s: &Session) -> Rig {
        let (host, audio, warnings) = SessionHost::start(
            s.project(),
            HostConfig {
                dir: s.dir().map(Path::to_path_buf),
                ..HostConfig::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        Rig { host, audio }
    }

    /// Renders `frames` frames as a device would take them; gives the left channel.
    fn pull(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; 960];
        let mut left = Vec::with_capacity(frames);
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(480);
            self.audio.render(&mut out[..n * 2], 2);
            left.extend(out[..n * 2].iter().step_by(2));
            done += n;
            self.host.poll();
        }
        left
    }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn watch(rig: &mut Rig, amp: f32) {
    let input = MockInput::sine(RATE, 220.0, amp);
    assert_eq!(
        rig.host.watch_start(Input::Given(Box::new(input))).unwrap(),
        2
    );
    assert!(rig.host.is_watching());
}

#[test]
fn device_enumeration_and_open_gates_never_hold_watch_or_rec_and_cancel_discards_input() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex, mpsc};
    use std::time::Instant;
    use xirius_session::output::{DeviceInput, Drivers};
    struct Release(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }
    for hold_enumeration in [true, false] {
        let mut s = session();
        armed_audio(&mut s, "Vox");
        let mut rig = Rig::new(&s);
        rig.host.set_windowed();
        let gate = Release(Arc::new((Mutex::new(false), Condvar::new())));
        let (entered_tx, entered_rx) = mpsc::channel();
        let held = gate.0.clone();
        let hold = Arc::new(move || {
            entered_tx.send(std::thread::current().id()).unwrap();
            drop(
                held.1
                    .wait_while(held.0.lock().unwrap(), |done| !*done)
                    .unwrap(),
            );
        });
        let opened = Arc::new(AtomicUsize::new(0));
        let count = opened.clone();
        let enumeration_gate = hold.clone();
        rig.host.drivers = Drivers {
            exclusive_out: None,
            shared_out: Arc::new(|_, _, _| panic!("no output device")),
            exclusive_in: None,
            devices: Arc::new(move || {
                if hold_enumeration {
                    enumeration_gate();
                }
                Vec::new()
            }),
            shared_in: Arc::new(move |_, _, _| {
                if !hold_enumeration {
                    hold();
                }
                count.fetch_add(1, Ordering::SeqCst);
                Ok(DeviceInput::new(
                    "stand-in".into(),
                    false,
                    Arc::new(std::sync::atomic::AtomicU64::new(1)),
                    Box::new(MockInput::sine(RATE, 220.0, 0.3)),
                ))
            }),
        };
        assert_eq!(rig.host.watch_start(Input::Device).unwrap(), 0);
        assert_ne!(
            entered_rx.recv_timeout(Duration::from_secs(30)).unwrap(),
            std::thread::current().id()
        );
        for _ in 0..32 {
            assert!(!rig.host.record_input_ready().unwrap());
            assert!(!rig.host.is_recording());
            assert!(!rig.host.is_watching());
            rig.pull(480);
        }
        rig.host.watch_stop();
        drop(gate);
        let deadline = Instant::now() + Duration::from_secs(30);
        while opened.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        rig.pull(480);
        assert!(
            !rig.host.is_watching(),
            "late device cannot attach after cancellation"
        );
        assert!(!rig.host.is_recording());
    }
}

#[test]
fn the_level_of_an_input_is_measured_with_the_transport_stopped() {
    let mut s = session();
    armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    watch(&mut rig, 0.3);
    rig.pull(4_800);
    let levels = rig.host.input_levels();
    assert!((levels[0] - 0.3).abs() < 0.01, "{levels:?}");
    assert!((levels[1] - 0.3).abs() < 0.01, "{levels:?}");
    assert_eq!(levels[2], 0.0);
    // A level is taken once; the next is from what came after.
    rig.pull(480);
    assert!(rig.host.input_levels()[0] > 0.2);
}

#[test]
fn the_input_gain_of_a_track_trims_the_level_of_its_channels() {
    let mut s = session();
    let vox = armed_audio(&mut s, "Vox");
    run(
        &mut s,
        "track.input.set",
        json!({ "ids": [vox], "input": "In 1" }),
    );
    run(
        &mut s,
        "track.input.gain",
        json!({ "ids": [vox], "db": -6 }),
    );
    let mut rig = Rig::new(&s);
    watch(&mut rig, 0.4);
    rig.host.watch_sync(s.project());
    rig.pull(4_800);
    let levels = rig.host.input_levels();
    assert!((levels[0] - 0.4 * 0.501).abs() < 0.01, "{levels:?}");
    assert!(
        (levels[1] - 0.4).abs() < 0.01,
        "channel 2 is nobody's: {levels:?}"
    );
}

#[test]
fn only_the_input_monitor_makes_the_input_heard() {
    let mut s = session();
    let vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    watch(&mut rig, 0.3);
    rig.host.watch_sync(s.project());
    // Watching alone is silent: the level moves, the output does not.
    let quiet = rig.pull(4_800);
    assert_eq!(peak(&quiet), 0.0);
    assert!(rig.host.input_levels()[0] > 0.2);

    run(&mut s, "track.monitor", json!({ "ids": [vox] }));
    rig.host.watch_sync(s.project());
    let heard = rig.pull(4_800);
    assert!((peak(&heard[480..]) - 0.3).abs() < 0.01, "{}", peak(&heard));

    run(&mut s, "track.monitor", json!({ "ids": [vox] }));
    rig.host.watch_sync(s.project());
    rig.pull(480);
    assert_eq!(peak(&rig.pull(4_800)), 0.0, "off again");
}

#[test]
fn a_track_that_is_not_armed_neither_trims_nor_monitors() {
    let mut s = session();
    let vox = armed_audio(&mut s, "Vox");
    run(&mut s, "track.monitor", json!({ "ids": [vox] }));
    run(&mut s, "track.input.gain", json!({ "ids": [vox], "db": 6 }));
    let (gain, (l, r)) = input_plan(s.project(), 2);
    assert!(gain[0] > 1.9 && gain[1] > 1.9);
    assert_eq!(
        (l, r),
        (0b01, 0b10),
        "a stereo input is heard on its two sides"
    );
    run(&mut s, "track.arm", json!({ "ids": [vox], "on": false }));
    let (gain, (l, r)) = input_plan(s.project(), 2);
    assert_eq!(gain[0], 1.0);
    assert_eq!((l, r), (0, 0));
}

#[test]
fn two_channels_of_a_pair_are_heard_one_on_each_side() {
    let mut s = session();
    let vox = armed_audio(&mut s, "Vox");
    run(
        &mut s,
        "track.input.set",
        json!({ "ids": [vox], "input": "In 1+2" }),
    );
    run(&mut s, "track.monitor", json!({ "ids": [vox] }));
    let (_, (l, r)) = input_plan(s.project(), 2);
    assert_eq!((l, r), (0b01, 0b10));
}

#[test]
fn a_recording_takes_over_the_watched_input_and_lets_go_of_it() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-watch-test-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().to_path_buf();
    let mut s = session();
    s.save_as(dir.join("song")).unwrap();
    armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    watch(&mut rig, 0.3);
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                latency: Some(0),
                input: Input::Given(Box::new(MockInput::sine(RATE, 220.0, 0.2))),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    assert!(!rig.host.is_watching(), "the recording has it now");
    rig.pull(9_600);
    assert!(rig.host.record_stop());
    rig.pull(24_000);
    rig.host.record_takes(Duration::from_secs(5)).unwrap();
    // A stand-in is not kept: the engine's input is gone, so nothing is measured.
    rig.host.input_levels();
    rig.pull(4_800);
    assert_eq!(rig.host.input_levels()[0], 0.0);
}
