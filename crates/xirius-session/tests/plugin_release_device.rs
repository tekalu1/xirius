//! The same as plugin_release_thread.rs, on the device paths (ARC-044): a plugin in the app is
//! terminated on the thread that made it when the player goes, with the output opened through
//! `Output::open` by each driver — shared (`DeviceOutput`) and WASAPI exclusive
//! (`ExclusiveOutput`). No sound device is opened: `Drivers` takes stand-ins that run the real
//! output types over streams that hold no device. One test in its own process, so no other
//! test counts the fixture's terminations meanwhile.

use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::{Value, json};
use xirius_core::model::{PluginIsolation, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::AudioEngine;
use xirius_engine::device::Lost;
use xirius_engine::exclusive::{ExclusiveInfo, ExclusiveOutput};
use xirius_plugins::{Catalog, probe};
use xirius_session::commands::{self, shared_registry};
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::output::{DeviceOutput, Drivers, Output, OutputMode, Refused, SharedStream};
use xirius_session::plugins::Isolation;

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let Value::Object(raw) = args else {
        unreachable!()
    };
    commands::run(s, None, name, &raw, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// The instances the fixture has terminated, and of those, the ones terminated on a thread
/// other than the one that made them.
fn terminations(fixture: &libloading::Library) -> (u32, u32) {
    // SAFETY: the fixture's export, with the ABI it is declared with.
    let both = unsafe {
        let f = fixture
            .get::<unsafe extern "system" fn() -> u64>(b"FixtureTerminations")
            .unwrap();
        f()
    };
    (both as u32, (both >> 32) as u32)
}

/// A shared-mode stream that opens nothing: it holds the engine until `stop` gives it back,
/// like `CpalOutput` does for a real device.
struct FakeStream {
    audio: Option<AudioEngine>,
    lost: Lost,
}

impl SharedStream for FakeStream {
    fn name(&self) -> String {
        "Interface（48000 Hz、2 ch）".into()
    }
    fn lost(&self) -> Lost {
        self.lost.clone()
    }
    fn stop(mut self: Box<Self>) -> Option<AudioEngine> {
        self.audio.take()
    }
}

/// The stand-in exclusive loop: answers `ready`, waits for `stop`, and ends with the engine,
/// like `run` does for a real device.
fn fake_exclusive_run(
    audio: AudioEngine,
    stop: Arc<std::sync::atomic::AtomicBool>,
    ready: mpsc::Sender<xirius_engine::exclusive::Ready>,
) -> xirius_engine::exclusive::Ended {
    let _ = ready.send(Ok(ExclusiveInfo {
        device: "Interface".into(),
        sample_rate: audio.setup().sample_rate as u32,
        channels: 2,
        format: "f32".into(),
        requested_period_frames: 0,
        period_frames: 480,
        min_period_frames: 480,
        default_period_frames: 480,
    }));
    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(1));
    }
    (Ok(()), Some(audio))
}

/// Drivers that run the real `DeviceOutput` / `ExclusiveOutput` over stand-in streams.
fn drivers() -> Drivers {
    Drivers {
        exclusive_out: Some(Arc::new(|audio, _device, _buffer| {
            ExclusiveOutput::start_with(audio, fake_exclusive_run)
                .map(Output::Exclusive)
                .map_err(|n| Refused {
                    error: n.error.to_string(),
                    audio: n.audio,
                })
        })),
        shared_out: Arc::new(|audio, _device, _buffer| {
            DeviceOutput::open_with(audio, |audio| {
                Ok(Box::new(FakeStream {
                    audio: Some(audio),
                    lost: Lost::default(),
                }) as Box<dyn SharedStream>)
            })
            .map(Output::Device)
        }),
        exclusive_in: None,
        shared_in: Arc::new(|_rate, _device, _buffer| Err("not opened by this test".into())),
        devices: Arc::new(|| {
            vec![xirius_engine::devices::Device {
                name: "Interface".into(),
                inputs: vec!["In 1".into()],
                outputs: vec!["Out 1".into(), "Out 2".into()],
            }]
        }),
    }
}

fn prefs(driver: &str) -> xirius_prefs::Prefs {
    let mut p = xirius_prefs::Prefs::default();
    p.set("audio.driver", &json!(driver)).unwrap();
    p.set("audio.device", &json!("Interface")).unwrap();
    p
}

/// `wasapi-exclusive` takes the exclusive driver, anything else the shared one.
#[test]
fn a_plugin_in_the_app_is_terminated_on_its_own_thread_on_the_device_paths() {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    assert!(
        dll.exists(),
        "the fixture's cdylib is built with its dev dependency"
    );
    // SAFETY: the fixture runs nothing when it is loaded; it stays loaded with the host's.
    let fixture = unsafe { libloading::Library::new(&dll) }.unwrap();
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let mut s = Session::with_registry(Project::new("release"), shared_registry());
    let t = run(&mut s, "track.add.audio", json!({ "name": "A" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "Test Latency", "format": "vst3", "uid": uid }),
    );
    let id = r.data["id"].as_str().unwrap().to_string();
    run(
        &mut s,
        "plugin.isolation.set",
        json!({ "ids": id, "mode": PluginIsolation::InProcess.name() }),
    );

    let plugin = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .next()
        .unwrap()
        .id;

    // Each driver, and the host dropped before the output (the transport's order) or after it.
    let cases = [
        ("wasapi-exclusive", true),
        ("wasapi-exclusive", false),
        ("wasapi-shared", true),
        ("wasapi-shared", false),
    ];
    for (driver, host_first) in cases {
        let (made_before, away_before) = terminations(&fixture);
        let (mut host, audio, warnings) = SessionHost::start(
            s.project(),
            HostConfig {
                catalog: Some(catalog.clone()),
                isolation: Some(Isolation::InProcess),
                ..HostConfig::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(
            host.plugins().local(plugin).is_some(),
            "the instance is in the app"
        );
        let opened = Output::open(OutputMode::Device, audio, &prefs(driver), &drivers())
            .unwrap_or_else(|e| panic!("{driver}: {e}"));
        assert!(opened.notice.is_none(), "{driver}: {:?}", opened.notice);
        let output = opened.output.unwrap();
        for _ in 0..10 {
            host.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        if host_first {
            drop(host);
            drop(output);
        } else {
            drop(output);
            drop(host);
        }
        let (made, away) = terminations(&fixture);
        assert_eq!(made, made_before + 1, "the instance was terminated");
        assert_eq!(
            away, away_before,
            "terminated on the thread that made it (driver: {driver}, host first: {host_first})"
        );
    }
}
