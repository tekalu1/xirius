//! Which driver opens the sound device (ENG-027): WASAPI exclusive is the default, a device that
//! will not open that way is opened in shared mode with one line saying so, and the input
//! follows the output. The drivers are stand-ins: no sound device is opened and nothing is
//! played to the speakers.

use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, Session};
use xirius_engine::AudioEngine;
use xirius_engine::record::MockInput;
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::output::{
    DeviceInput, Drivers, Output, OutputMode, Refused, SilentOutput, open_input,
};
use xirius_session::record::Input;

fn prefs(driver: &str) -> xirius_prefs::Prefs {
    let mut p = xirius_prefs::Prefs::default();
    p.set("audio.driver", &json!(driver)).unwrap();
    p.set("audio.device", &json!("Interface")).unwrap();
    p.set("audio.buffer", &json!(64)).unwrap();
    p
}

fn run(s: &mut Session, name: &str, args: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(args) = args else {
        unreachable!()
    };
    commands::run(s, None, name, &args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

/// A song with one long note, so that the transport has somewhere to run.
fn engine() -> (SessionHost, AudioEngine) {
    let mut s = Session::with_registry(Project::new("drivers"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    let t = run(&mut s, "track.add.instrument", json!({ "name": "A" }))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let c = run(
        &mut s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 8 }),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(
        &mut s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": 60, "length": 30720, "velocity": 100 }),
    );
    let (host, audio, _) = SessionHost::start(
        s.project(),
        HostConfig {
            dir: s.dir().map(Path::to_path_buf),
            ..HostConfig::default()
        },
    )
    .unwrap();
    (host, audio)
}

/// What the stand-in drivers were asked.
#[derive(Default)]
struct Calls {
    exclusive: AtomicUsize,
    shared: AtomicUsize,
    asked: Mutex<Vec<(String, Option<u32>)>>,
}

/// Drivers whose exclusive mode `opens` (or refuses, returning the engine when `keeps`).
fn drivers(calls: &Arc<Calls>, opens: bool, keeps: bool) -> Drivers {
    let (c1, c2, c3, c4) = (calls.clone(), calls.clone(), calls.clone(), calls.clone());
    Drivers {
        exclusive_out: Some(Arc::new(move |audio, device, buffer| {
            c1.exclusive.fetch_add(1, Ordering::SeqCst);
            c1.asked.lock().unwrap().push((device, buffer));
            if opens {
                Ok(Output::Silent(SilentOutput::start(audio, false)))
            } else {
                Err(Refused {
                    error: "initialize exclusive: 0x88890008\n詳しい説明".into(),
                    audio: keeps.then(|| Box::new(audio)),
                })
            }
        })),
        shared_out: Arc::new(move |audio, device, buffer| {
            c2.shared.fetch_add(1, Ordering::SeqCst);
            c2.asked.lock().unwrap().push((device, buffer));
            Ok(Output::Silent(SilentOutput::start(audio, false)))
        }),
        exclusive_in: Some(Arc::new(move |_rate, device, buffer| {
            c3.exclusive.fetch_add(1, Ordering::SeqCst);
            c3.asked.lock().unwrap().push((device, buffer));
            if opens {
                Ok(mock_input(true))
            } else {
                Err("no exclusive format at 48000 Hz, 2 ch".into())
            }
        })),
        shared_in: Arc::new(move |_rate, device, buffer| {
            c4.shared.fetch_add(1, Ordering::SeqCst);
            c4.asked.lock().unwrap().push((device, buffer));
            Ok(mock_input(false))
        }),
        devices: Arc::new(|| {
            vec![xirius_engine::devices::Device {
                name: "Interface".into(),
                inputs: vec!["In 1".into(), "In 2".into()],
                outputs: vec!["Out 1".into(), "Out 2".into()],
            }]
        }),
    }
}

fn mock_input(exclusive: bool) -> DeviceInput {
    DeviceInput::new(
        if exclusive {
            "Interface 排他"
        } else {
            "Interface"
        }
        .into(),
        exclusive,
        Arc::new(AtomicU64::new(0)),
        Box::new(MockInput::sine(48_000.0, 440.0, 0.5)),
    )
}

#[test]
fn exclusive_is_the_default_driver() {
    let p = xirius_prefs::Prefs::default();
    assert_eq!(p.text("audio.driver"), "wasapi-exclusive");
}

#[test]
fn an_output_that_opens_exclusively_is_used_with_nothing_to_say() {
    let calls = Arc::new(Calls::default());
    let (_host, audio) = engine();
    let opened = Output::open(
        OutputMode::Device,
        audio,
        &prefs("wasapi-exclusive"),
        &drivers(&calls, true, true),
    )
    .unwrap();
    assert!(opened.output.is_some());
    assert_eq!(opened.notice, None);
    assert_eq!(calls.exclusive.load(Ordering::SeqCst), 1);
    assert_eq!(calls.shared.load(Ordering::SeqCst), 0);
    assert_eq!(
        calls.asked.lock().unwrap().as_slice(),
        [("Interface".to_string(), Some(64))],
        "the device and the buffer of the settings"
    );
}

#[test]
fn an_output_that_will_not_open_exclusively_falls_back_to_shared_in_one_line() {
    let calls = Arc::new(Calls::default());
    let (mut host, audio) = engine();
    let opened = Output::open(
        OutputMode::Device,
        audio,
        &prefs("wasapi-exclusive"),
        &drivers(&calls, false, true),
    )
    .unwrap();
    let out = opened.output.expect("shared mode opened");
    assert_eq!(calls.exclusive.load(Ordering::SeqCst), 1);
    assert_eq!(calls.shared.load(Ordering::SeqCst), 1);
    let notice = opened.notice.expect("the fall back is said");
    assert!(!notice.contains('\n'), "{notice:?}");
    assert!(
        notice.contains("排他") && notice.contains("共有"),
        "{notice}"
    );
    assert!(notice.contains("0x88890008"), "the reason: {notice}");
    assert!(!notice.contains("詳しい説明"), "one line only: {notice}");
    // The engine that came back plays: the song runs on the shared output.
    assert!(host.play());
    let mut moved = false;
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        if host.poll().frame > 0 {
            moved = true;
            break;
        }
    }
    assert!(moved, "the engine runs on the output it fell back to");
    drop(out);
}

#[test]
fn an_output_that_loses_the_engine_is_an_error_not_a_fall_back() {
    let calls = Arc::new(Calls::default());
    let (_host, audio) = engine();
    let r = Output::open(
        OutputMode::Device,
        audio,
        &prefs("wasapi-exclusive"),
        &drivers(&calls, false, false),
    );
    let Err(e) = r else {
        panic!("nothing to fall back with")
    };
    assert!(e.contains("0x88890008"), "{e}");
    assert_eq!(calls.shared.load(Ordering::SeqCst), 0);
}

#[test]
fn the_shared_driver_never_tries_exclusive() {
    let calls = Arc::new(Calls::default());
    let (_host, audio) = engine();
    let opened = Output::open(
        OutputMode::Device,
        audio,
        &prefs("wasapi-shared"),
        &drivers(&calls, true, true),
    )
    .unwrap();
    assert!(opened.output.is_some() && opened.notice.is_none());
    assert_eq!(calls.exclusive.load(Ordering::SeqCst), 0);
    assert_eq!(calls.shared.load(Ordering::SeqCst), 1);
    // The Windows audio engine sets the buffer in shared mode: none is asked for.
    assert_eq!(
        calls.asked.lock().unwrap().as_slice(),
        [("Interface".to_string(), None)]
    );
    let (_input, _) = open_input(48_000, &prefs("wasapi-shared"), "Interface".into(), &{
        let mut d = drivers(&calls, true, true);
        d.shared_in = Arc::new(|_, _, buffer| {
            assert_eq!(buffer, None, "the shared input asks for no buffer");
            Ok(mock_input(false))
        });
        d
    })
    .unwrap();
}

#[test]
fn a_device_another_program_holds_is_said_in_words() {
    use xirius_engine::device::IN_USE;
    use xirius_session::output::{fallback_notice, unavailable};
    let e = format!("build output stream: {IN_USE}");
    assert_eq!(
        unavailable(&e),
        "オーディオデバイスを使用できません（ほかのアプリが使用中です）"
    );
    let n = fallback_notice("出力", &format!("initialize exclusive: {IN_USE}"));
    assert!(n.ends_with("（ほかのアプリが使用中です）"), "{n}");
    // Other reasons are kept as the system gave them, on one line.
    assert_eq!(
        unavailable("open: 0x88890004\nmore"),
        "オーディオデバイスを使用できません（open: 0x88890004）"
    );
}

#[test]
fn an_output_says_when_its_device_stopped_by_itself() {
    let (_host, audio) = engine();
    let silent = SilentOutput::start(audio, true);
    let lost = silent.lost();
    let out = Output::Silent(silent);
    assert_eq!(out.lost(), None);
    lost.set("the device was unplugged".into());
    lost.set("a later error".into());
    assert_eq!(out.lost().as_deref(), Some("the device was unplugged"));
}

#[test]
fn where_there_is_no_exclusive_mode_shared_is_used_without_a_word() {
    let calls = Arc::new(Calls::default());
    let (_host, audio) = engine();
    let mut d = drivers(&calls, true, true);
    d.exclusive_out = None;
    d.exclusive_in = None;
    let opened = Output::open(OutputMode::Device, audio, &prefs("wasapi-exclusive"), &d).unwrap();
    assert!(opened.output.is_some());
    assert_eq!(opened.notice, None);
    let (input, notice) =
        open_input(48_000, &prefs("wasapi-exclusive"), String::new(), &d).unwrap();
    assert!(!input.exclusive);
    assert_eq!(notice, None);
}

#[test]
fn a_silent_output_ignores_the_driver() {
    let calls = Arc::new(Calls::default());
    let (_host, audio) = engine();
    let opened = Output::open(
        OutputMode::Silent { paced: false },
        audio,
        &prefs("wasapi-exclusive"),
        &drivers(&calls, true, true),
    )
    .unwrap();
    assert!(matches!(opened.output, Some(Output::Silent(_))));
    assert_eq!(calls.exclusive.load(Ordering::SeqCst), 0);
}

#[test]
fn the_input_follows_the_driver_and_falls_back_with_a_line() {
    // Opens exclusively.
    let calls = Arc::new(Calls::default());
    let d = drivers(&calls, true, true);
    let (input, notice) =
        open_input(48_000, &prefs("wasapi-exclusive"), "Interface".into(), &d).unwrap();
    assert!(input.exclusive && notice.is_none());
    assert_eq!(calls.shared.load(Ordering::SeqCst), 0);

    // Will not: shared, and the line.
    let calls = Arc::new(Calls::default());
    let d = drivers(&calls, false, true);
    let (input, notice) =
        open_input(48_000, &prefs("wasapi-exclusive"), "Interface".into(), &d).unwrap();
    assert!(!input.exclusive);
    let notice = notice.expect("said");
    assert!(
        notice.contains("入力") && notice.contains("共有"),
        "{notice}"
    );
    assert!(!notice.contains('\n'));
    assert_eq!(calls.shared.load(Ordering::SeqCst), 1);

    // Shared asked for: exclusive is not tried.
    let calls = Arc::new(Calls::default());
    let d = drivers(&calls, true, true);
    let (input, notice) =
        open_input(48_000, &prefs("wasapi-shared"), "Interface".into(), &d).unwrap();
    assert!(!input.exclusive && notice.is_none());
    assert_eq!(calls.exclusive.load(Ordering::SeqCst), 0);
}

#[test]
fn a_watched_input_comes_from_the_driver_and_its_fall_back_is_taken_once() {
    let calls = Arc::new(Calls::default());
    let (mut host, audio) = engine();
    let mut settings = xirius_prefs::Settings::memory();
    settings.prefs = prefs("wasapi-exclusive");
    host.io_settings = Some(settings.shared());
    host.drivers = drivers(&calls, false, true);

    let channels = host.watch_start(Input::Device).unwrap();
    assert_eq!(channels, 2);
    assert!(host.is_watching());
    let notice = host
        .take_driver_notice()
        .expect("the exclusive input would not open");
    assert!(
        notice.contains("入力") && notice.contains("共有"),
        "{notice}"
    );
    assert_eq!(host.take_driver_notice(), None, "said once");
    assert_eq!(calls.exclusive.load(Ordering::SeqCst), 1);
    assert_eq!(calls.shared.load(Ordering::SeqCst), 1);
    drop(audio);
}

#[test]
fn a_watched_input_opened_exclusively_measures_levels_like_any_other() {
    let calls = Arc::new(Calls::default());
    let (mut host, mut audio) = engine();
    let mut settings = xirius_prefs::Settings::memory();
    settings.prefs = prefs("wasapi-exclusive");
    host.io_settings = Some(settings.shared());
    host.drivers = drivers(&calls, true, true);
    assert_eq!(host.watch_start(Input::Device).unwrap(), 2);
    assert_eq!(host.take_driver_notice(), None);
    assert_eq!(calls.shared.load(Ordering::SeqCst), 0);
    let mut out = vec![0.0f32; 960];
    for _ in 0..10 {
        audio.render(&mut out, 2);
    }
    let levels = host.input_levels();
    assert!(
        levels[0] > 0.3,
        "the sine of the stand-in is measured: {levels:?}"
    );
}

#[test]
fn an_unplugged_device_is_chosen_again_for_the_output() {
    let calls = Arc::new(Calls::default());
    let (_host, audio) = engine();
    let mut d = drivers(&calls, true, true);
    let device = |name: &str, inputs: usize| xirius_engine::devices::Device {
        name: name.into(),
        inputs: (1..=inputs).map(|i| format!("In {i}")).collect(),
        outputs: vec!["Out 1".into(), "Out 2".into()],
    };
    let now = vec![device("Speakers", 0), device("Headset", 1)];
    d.devices = Arc::new(move || now.clone());
    let opened = Output::open(OutputMode::Device, audio, &prefs("wasapi-exclusive"), &d).unwrap();
    assert!(opened.output.is_some());
    assert_eq!(
        calls.asked.lock().unwrap().as_slice(),
        [("Headset".to_string(), Some(64))],
        "\"Interface\" is gone: the one with inputs and outputs"
    );
}

/// Drivers whose input opens in shared mode with `open`, handing what it opens back through
/// `held`, so a test can stop it as the system would.
fn input_drivers(
    held: &Arc<Mutex<Option<xirius_engine::device::Lost>>>,
    open: Result<(), String>,
) -> Drivers {
    let calls = Arc::new(Calls::default());
    let mut d = drivers(&calls, true, true);
    d.exclusive_in = None;
    let h = held.clone();
    d.shared_in = Arc::new(move |_, _, _| {
        open.clone()?;
        let input = mock_input(false);
        *h.lock().unwrap() = Some(input.lost());
        Ok(input)
    });
    d
}

#[test]
fn an_input_another_program_holds_is_said_like_the_output() {
    use xirius_engine::device::IN_USE;
    use xirius_engine::record::NO_INPUT;
    use xirius_session::output::{input_unavailable, is_input_unavailable};
    let said = input_unavailable(&format!("入力デバイスを開けません: {IN_USE}"));
    assert_eq!(
        said,
        "オーディオ入力を使用できません（ほかのアプリが使用中です）"
    );
    assert!(is_input_unavailable(&said));
    assert_eq!(
        input_unavailable(NO_INPUT),
        "オーディオ入力を使用できません（入力デバイスから 1 秒間データが届きません）"
    );
    assert!(!is_input_unavailable(
        "オーディオデバイスを使用できません（ほかのアプリが使用中です）"
    ));
}

#[test]
fn a_watched_input_that_will_not_open_says_why_instead_of_listening_to_silence() {
    use xirius_engine::device::IN_USE;
    use xirius_engine::record::NO_INPUT;
    for (refused, said) in [
        (
            format!("入力デバイスを開けません: {IN_USE}"),
            "オーディオ入力を使用できません（ほかのアプリが使用中です）",
        ),
        (
            NO_INPUT.to_string(),
            "オーディオ入力を使用できません（入力デバイスから 1 秒間データが届きません）",
        ),
    ] {
        let held = Arc::new(Mutex::new(None));
        let (mut host, _audio) = engine();
        host.drivers = input_drivers(&held, Err(refused));
        let e = host.watch_start(Input::Device).unwrap_err();
        assert_eq!(e.message, said);
        assert!(!host.is_watching());
        assert_eq!(host.input_lost(), None);
    }
}

#[test]
fn a_watched_input_that_stops_says_why() {
    let held = Arc::new(Mutex::new(None));
    let (mut host, _audio) = engine();
    host.drivers = input_drivers(&held, Ok(()));
    host.watch_start(Input::Device).unwrap();
    assert_eq!(host.input_lost(), None);
    let lost = held.lock().unwrap().take().expect("opened");
    lost.set("the device was unplugged".into());
    lost.set("a later error".into());
    assert_eq!(
        host.input_lost().as_deref(),
        Some("the device was unplugged")
    );
    // Let go, it says nothing more: the next one opened is asked afresh.
    host.watch_stop();
    assert_eq!(host.input_lost(), None);
    host.watch_start(Input::Device).unwrap();
    assert_eq!(host.input_lost(), None);
}
