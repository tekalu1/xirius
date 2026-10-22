use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use xirius_core::session::RunOptions;
use xirius_core::{Project, Session};
use xirius_engine::devices::{Backend, Device, Latency, Simulated};
use xirius_engine::record::{MidiInput, MockInput, MockMidi};
use xirius_session::audio_settings::{Io, IoExt};

fn backend() -> Box<dyn Backend> {
    Box::new(Simulated::new(
        vec![Device {
            name: "Test".into(),
            inputs: vec!["In 1".into(), "In 2".into()],
            outputs: vec!["1".into(), "2".into(), "3".into(), "4".into()],
        }],
        [(
            256,
            Latency {
                rate: 96000,
                input: 360,
                output: 510,
                conversion: 48,
            },
        )]
        .into(),
        Box::new(MockInput::sine(96000.0, 1000.0, 0.25)),
    ))
}

#[test]
fn commands_validate_and_persist_without_editing_the_song() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-audioset-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().join("prefs");
    let settings = xirius_prefs::Settings::open(&dir).shared();
    let io = Arc::new(Mutex::new(Io::new(backend())));
    let mut s = Session::with_registry(
        Project::new("test"),
        xirius_session::commands::shared_registry(),
    );
    s.add_extension(Box::new(IoExt {
        io: io.clone(),
        settings: settings.clone(),
    }));
    let before = serde_json::to_value(s.project()).unwrap();
    let run =
        |s: &mut Session, command, args| s.run_value(command, args, &RunOptions::human()).unwrap();
    run(
        &mut s,
        "audio.set",
        json!({"key":"record-offset","value":"-12.5"}),
    );
    run(&mut s, "audio.input.select", json!({"channel":2}));
    run(&mut s, "audio.output.set", json!({"left":3,"right":4}));
    run(
        &mut s,
        "midi.set",
        json!({"key":"inputs.Keys","value":"false"}),
    );
    assert_eq!(before, serde_json::to_value(s.project()).unwrap());
    assert_eq!(
        xirius_prefs::Settings::open(&dir)
            .prefs
            .number("audio.record-offset"),
        -12.5
    );
    let saved = xirius_session::io_store::read(&dir).unwrap().unwrap();
    assert_eq!(saved.input, Some(1));
    assert_eq!(saved.output, [2, 3]);
    assert!(
        s.run_value(
            "audio.set",
            json!({"key":"buffer","value":"33"}),
            &RunOptions::human()
        )
        .is_err()
    );
    assert!(
        s.run_value(
            "audio.output.set",
            json!({"left":5,"right":6}),
            &RunOptions::human()
        )
        .is_err()
    );
    assert_eq!(io.lock().unwrap().view.output, [2, 3]);
    let read = run(&mut s, "audio.get", json!({}));
    assert_eq!(read.data["settings"]["audio.record-offset"], -12.5);
    assert!(read.message.contains("audio.buffer"));
    let read = run(&mut s, "midi.get", json!({}));
    assert_eq!(read.data["settings"]["midi.inputs.Keys"], false);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn silence_does_not_choose_a_nonexistent_input_and_unknown_reports_stay_unknown() {
    let mut io = Io::new(backend());
    let mut p = xirius_prefs::Prefs::default();
    io.sync(&p).unwrap();
    assert_eq!(io.view.latency.unwrap().parts_ms(), [3.75, 5.3125, 0.5]);
    p.set("audio.buffer", &json!(128)).unwrap();
    io.sync(&p).unwrap();
    assert!(io.view.latency.is_none());
    io.replace(Box::new(xirius_engine::devices::Unopened::default()));
    io.sync(&p).unwrap();
    let now = Instant::now();
    io.detect(now);
    io.poll(now + Duration::from_secs(2));
    assert!(io.view.detected.is_empty());
    assert!(io.view.selected.is_none());
}

#[test]
fn disabled_midi_ports_are_filtered_before_recording() {
    let midi = MockMidi::with_ports(
        vec!["A".into(), "B".into()],
        vec![(0, 0, [0x90, 60, 100]), (0, 1, [0x90, 64, 100])],
    );
    let mut prefs = xirius_prefs::Prefs::default();
    prefs.set("midi.inputs.A", &json!(false)).unwrap();
    let mut filtered = xirius_session::midi_settings::EnabledInput::new(Box::new(midi), &prefs);
    let mut heard = Vec::new();
    filtered.read_ports(128, &mut |at, port, msg| heard.push((at, port, msg)));
    assert_eq!(heard, [(0, 1, [0x90, 64, 100])]);
}

#[test]
fn recording_port_switch_releases_notes_and_reports_received_messages() {
    let input = MockMidi::with_ports(
        vec!["Keys".into()],
        vec![(0, 0, [0x90, 60, 100]), (256, 0, [0x90, 64, 100])],
    );
    let mut prefs = xirius_prefs::Prefs::default();
    let mut input = xirius_session::midi_settings::EnabledInput::new(Box::new(input), &prefs);
    let control = input.control();
    let mut messages = Vec::new();
    input.read_ports(128, &mut |_, _, msg| messages.push(msg));
    assert_eq!(messages, [[0x90, 60, 100]]);
    prefs.set("midi.inputs.Keys", &json!(false)).unwrap();
    let mut received = Vec::new();
    control.update(&prefs, |p| received.push(p.to_string()));
    assert_eq!(received, ["Keys"]);
    messages.clear();
    input.read_ports(256, &mut |_, _, msg| messages.push(msg));
    assert!(messages.contains(&[0x80, 60, 0]));
    assert!(!messages.iter().any(|m| m[0] & 0xf0 == 0x90));
    control.update(&prefs, |_| panic!("disabled port reported activity"));
}

#[test]
fn recording_offsets_have_both_signs_and_clamp_at_the_song_start() {
    use xirius_session::midi_settings::shift_frame;
    assert_eq!(shift_frame(48000, 7.5, 48000.0), 48360);
    assert_eq!(shift_frame(48000, -7.5, 48000.0), 47640);
    assert_eq!(shift_frame(12, -7.5, 48000.0), 0);
}

#[test]
fn negative_midi_offset_moves_a_note_inside_a_take_from_the_song_start() {
    use xirius_session::host::{HostConfig, SessionHost};
    use xirius_session::record::RecordOptions;
    for offset in [-100.0, 0.0, 100.0] {
        let mut s = Session::new("offset");
        s.run_value(
            "track.add.with-instrument",
            json!({"instrument":"Sampler", "arm":true}),
            &RunOptions::human(),
        )
        .unwrap();
        let (mut host, mut audio, _) =
            SessionHost::start(s.project(), HostConfig::default()).unwrap();
        host.record_start(
            s.project(),
            None,
            RecordOptions {
                at: Some(0),
                offsets_ms: [0.0, offset],
                midi: Some(Box::new(MockMidi::new(vec![
                    (24000, [0x90, 60, 100]),
                    (36000, [0x80, 60, 0]),
                ]))),
                ..RecordOptions::default()
            },
        )
        .unwrap();
        let mut out = [0.0; 960];
        for _ in 0..100 {
            audio.render(&mut out, 2);
        }
        assert!(host.record_stop());
        for _ in 0..50 {
            audio.render(&mut out, 2);
        }
        let take = host.record_takes(Duration::from_secs(5)).unwrap();
        let take = &take.takes[0];
        let tick = take["start"].as_i64().unwrap() + take["notes"][0]["start"].as_i64().unwrap();
        let frame = host.tempo_map().frame_at(tick, 48000.0);
        assert_eq!(frame, (24000.0 + offset * 48.0) as u64);
    }
}

#[test]
fn no_devices_and_no_channels_yield_no_levels() {
    let mut backend = Simulated::new(
        vec![],
        Default::default(),
        Box::new(MockInput::sine(48000.0, 220.0, 1.0)),
    );
    assert!(backend.levels().is_empty());
}

#[test]
fn the_initial_capture_interface_matches_the_displayed_inputs() {
    let mut backend = backend();
    let mut devices = backend.devices();
    devices.push(Device {
        name: "Other microphone".into(),
        inputs: vec!["In 1".into()],
        outputs: vec![],
    });
    let mut prefs = xirius_prefs::Prefs::default();
    let mut io = Io::new(Box::new(xirius_engine::devices::Unopened {
        devices: devices.clone(),
    }));
    io.sync(&prefs).unwrap();
    let capture = xirius_session::audio_settings::configured_device(&prefs, &devices);
    assert_eq!(capture, io.view.device);
    assert_eq!(capture, "Test");
    backend.configure("", &capture, 256).unwrap();
    prefs
        .set("audio.device", &json!("Other microphone"))
        .unwrap();
    io.sync(&prefs).unwrap();
    assert_eq!(
        xirius_session::audio_settings::configured_device(&prefs, &[]),
        io.view.device
    );
}

#[test]
fn audio_offsets_move_the_captured_transient_in_both_directions() {
    use xirius_engine::record::AudioInput;
    use xirius_session::host::{HostConfig, SessionHost};
    use xirius_session::record::{Input, RecordOptions};
    struct Pulse(usize);
    impl AudioInput for Pulse {
        fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
            for (l, r) in left.iter_mut().zip(right) {
                *l = if self.0 == 24000 { 0.75 } else { 0.0 };
                *r = *l;
                self.0 += 1;
            }
        }
    }
    for offset in [-100.0, 0.0, 100.0] {
        let tmp = tempfile::Builder::new()
            .prefix("xirius-audio-offset-")
            .tempdir()
            .unwrap();
        let dir = tmp.path().to_path_buf();
        let mut s = Session::new("offset");
        let added = s
            .run_value(
                "track.add.audio",
                json!({"name":"Input"}),
                &RunOptions::human(),
            )
            .unwrap();
        s.run_value(
            "track.arm",
            json!({"ids":[added.data["id"]],"on":true}),
            &RunOptions::human(),
        )
        .unwrap();
        let (mut host, mut audio, _) =
            SessionHost::start(s.project(), HostConfig::default()).unwrap();
        host.record_start(
            s.project(),
            Some(&dir),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(Pulse(0))),
                latency: Some(0),
                offsets_ms: [offset, 0.0],
                ..RecordOptions::default()
            },
        )
        .unwrap();
        let mut out = [0.0; 960];
        for _ in 0..100 {
            audio.render(&mut out, 2);
        }
        assert!(host.record_stop());
        for _ in 0..50 {
            audio.render(&mut out, 2);
        }
        let finished = host.record_takes(Duration::from_secs(5)).unwrap();
        let take = &finished.takes[0];
        let mut wav = hound::WavReader::open(&finished.files[0]).unwrap();
        let channels = wav.spec().channels as usize;
        let sample = wav
            .samples::<f32>()
            .map(Result::unwrap)
            .position(|v| v.abs() > 0.1)
            .unwrap()
            / channels;
        let start = host
            .tempo_map()
            .frame_at(take["start"].as_i64().unwrap(), 48000.0);
        assert_eq!(start + sample as u64, (24000.0 + offset * 48.0) as u64);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn physical_output_routes_only_to_the_selected_pair() {
    use xirius_engine::NoteKind;
    use xirius_session::host::{HostConfig, SessionHost};
    let mut s = Session::new("route");
    let made = s
        .run_value(
            "track.add.with-instrument",
            json!({"instrument":"Sampler"}),
            &RunOptions::human(),
        )
        .unwrap();
    let t = made.data["id"].as_str().unwrap().parse().unwrap();
    let (mut host, mut audio, _) = SessionHost::start(s.project(), HostConfig::default()).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("xirius-saved-output-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().join("prefs");
    xirius_session::io_store::write(
        &dir,
        &xirius_session::io_store::Routes {
            device: "Test".into(),
            input: None,
            output: [2, 3],
        },
    )
    .unwrap();
    host.io_settings = Some(xirius_prefs::Settings::open(&dir).shared());
    xirius_session::io_store::apply(&mut host).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    host.note(
        t,
        NoteKind::On {
            pitch: 60,
            velocity: 0.8,
        },
    );
    let mut buf = vec![0.0; 4096 * 4];
    audio.render(&mut buf, 4);
    assert!(
        buf.as_chunks::<4>()
            .0
            .iter()
            .all(|f| f[0] == 0.0 && f[1] == 0.0)
    );
    assert!(
        buf.as_chunks::<4>()
            .0
            .iter()
            .any(|f| f[2].abs() > 0.001 && f[3].abs() > 0.001)
    );
}

#[test]
fn assigned_knobs_are_left_out_of_the_recording_and_handed_to_the_screen() {
    use xirius_prefs::midimap::{Message, Target};
    let midi = MockMidi::with_ports(
        vec!["Pad".into(), "Keys".into()],
        vec![
            (0, 0, [0xb0, 21, 10]),
            (0, 1, [0xb0, 21, 20]),
            (0, 0, [0x90, 60, 100]),
            (64, 0, [0xb0, 21, 90]),
        ],
    );
    let prefs = xirius_prefs::Prefs::default();
    let mut map = xirius_prefs::Midimap::default();
    map.assign(Some("Pad"), Message::Cc(21), Target::Volume("trk_1".into()));
    let mut input = xirius_session::midi_settings::EnabledInput::new(Box::new(midi), &prefs)
        .with_assignments(&map);
    let control = input.control();
    let mut heard = Vec::new();
    input.read_ports(128, &mut |_, port, msg| heard.push((port, msg)));
    // The knob of the Pad is not recorded; the same number on another port is.
    assert_eq!(heard, [(1, [0xb0, 21, 20]), (0, [0x90, 60, 100])]);
    let taken: Vec<_> = control.take().into_iter().map(|t| t.message[2]).collect();
    assert_eq!(taken, [10, 90]);
    assert!(control.take().is_empty());
    // Freed, the knob is recorded again.
    control.assign(&xirius_prefs::Midimap::default());
    let again = MockMidi::with_ports(vec!["Pad".into()], vec![(0, 0, [0xb0, 21, 5])]);
    let mut input = xirius_session::midi_settings::EnabledInput::new(Box::new(again), &prefs);
    let mut heard = Vec::new();
    input.read_ports(128, &mut |_, _, msg| heard.push(msg));
    assert_eq!(heard, [[0xb0, 21, 5]]);
}

fn device(name: &str, inputs: usize, outputs: usize) -> Device {
    Device {
        name: name.into(),
        inputs: (1..=inputs).map(|i| format!("In {i}")).collect(),
        outputs: (1..=outputs).map(|i| format!("Out {i}")).collect(),
    }
}

#[test]
fn with_nothing_chosen_the_default_output_is_taken_when_it_has_inputs() {
    use xirius_session::audio_settings::configured_device;
    let prefs = xirius_prefs::Prefs::default();
    // The system's default output is listed first.
    let devices = [device("Interface", 2, 2), device("Speakers", 0, 2)];
    assert_eq!(configured_device(&prefs, &devices), "Interface");
}

#[test]
fn with_nothing_chosen_a_device_with_inputs_and_outputs_comes_before_bare_speakers() {
    use xirius_session::audio_settings::configured_device;
    let prefs = xirius_prefs::Prefs::default();
    let devices = [
        device("Speakers", 0, 2),
        device("USB microphone", 1, 0),
        device("Headset", 1, 2),
    ];
    assert_eq!(configured_device(&prefs, &devices), "Headset");
    // Without one, the default output: playing still works.
    let devices = [device("Speakers", 0, 2), device("USB microphone", 1, 0)];
    assert_eq!(configured_device(&prefs, &devices), "Speakers");
    assert_eq!(configured_device(&prefs, &[]), "");
}

#[test]
fn the_device_chosen_before_is_kept_and_chosen_again_once_unplugged() {
    use xirius_session::audio_settings::configured_device;
    let mut prefs = xirius_prefs::Prefs::default();
    prefs.set("audio.device", &json!("Speakers")).unwrap();
    let devices = [device("Headset", 1, 2), device("Speakers", 0, 2)];
    assert_eq!(configured_device(&prefs, &devices), "Speakers", "kept");
    let unplugged = [device("Laptop", 0, 2), device("Headset", 1, 2)];
    assert_eq!(configured_device(&prefs, &unplugged), "Headset");
    // The screen follows: the interface it shows is the one chosen again.
    let mut io = Io::new(Box::new(xirius_engine::devices::Unopened {
        devices: unplugged.to_vec(),
    }));
    io.sync(&prefs).unwrap();
    assert_eq!(io.view.device, "Headset");
    assert_eq!(
        prefs.text("audio.device"),
        "Speakers",
        "the choice is not lost"
    );
}

#[test]
fn audio_devices_lists_inputs_and_outputs_and_can_keep_only_those_with_inputs() {
    let settings = xirius_prefs::Settings::memory().shared();
    let mut io = Io::new(Box::new(xirius_engine::devices::Unopened {
        devices: vec![device("Speakers", 0, 2), device("Headset", 1, 2)],
    }));
    io.view.ports = vec!["KeyStep".into()];
    let mut s = Session::with_registry(
        Project::new("test"),
        xirius_session::commands::shared_registry(),
    );
    s.add_extension(Box::new(IoExt {
        io: Arc::new(Mutex::new(io)),
        settings,
    }));
    let run = |s: &mut Session, command, args| {
        s.run_value(command, args, &RunOptions::human())
            .unwrap()
            .data
    };
    let all = run(&mut s, "audio.devices", json!({}));
    // Nothing chosen: the default output has no inputs, so the headset is taken (ENG-061).
    assert_eq!(all["device"], "Headset");
    assert_eq!(
        all["devices"],
        json!([
            {"name": "Speakers", "inputs": 0, "outputs": 2, "current": false},
            {"name": "Headset", "inputs": 1, "outputs": 2, "current": true},
        ])
    );
    let inputs = run(&mut s, "audio.devices", json!({"inputs": true}));
    assert_eq!(inputs["devices"].as_array().unwrap().len(), 1);
    assert_eq!(inputs["devices"][0]["name"], "Headset");

    // The PC's keys come first among the MIDI inputs, with or without ports (UI-263).
    let midi = run(&mut s, "midi.get", json!({}));
    assert_eq!(
        midi["inputs"],
        json!([
            {"name": "PC のキーボード", "kind": "keyboard"},
            {"name": "KeyStep", "kind": "port", "on": true},
        ])
    );
}
