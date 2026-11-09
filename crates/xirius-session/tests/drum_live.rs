//! A failed hosted capture must not hide a live stock edit or lose an editor
//! mailbox. Pull the actual engine; never open an audio device or an editor.
#![cfg(feature = "test-io")]

use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use xirius_core::{Session, id::TrackId, model::Project, session::RunOptions};
use xirius_engine::{AudioEngine, NoteKind, record::MockMidi};
use xirius_plugins::{Catalog, decode_state};
use xirius_session::{
    commands,
    ext::ExtConfig,
    host::{HostConfig, SessionHost},
    plugins::Isolation,
    record::RecordOptions,
};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    commands::run(
        s,
        None,
        name,
        args.as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap()
    .data
}

fn pull(h: &mut SessionHost, a: &mut AudioEngine, blocks: usize) -> f32 {
    let mut peak = 0.0_f32;
    for _ in 0..blocks {
        let mut out = [0.0; 960];
        a.render(&mut out, 2);
        peak = out.iter().fold(peak, |p, x| p.max(x.abs()));
        h.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    peak
}

fn settle(h: &mut SessionHost, a: &mut AudioEngine) -> Vec<String> {
    let end = Instant::now() + Duration::from_secs(20);
    let mut warnings = Vec::new();
    while h.swap_pending() {
        if let Some(r) = h.step_swap() {
            warnings.extend(r.warnings);
        }
        pull(h, a, 1);
        assert!(Instant::now() < end, "live graph deadline");
    }
    pull(h, a, 100);
    warnings
}

fn tone(path: &std::path::Path, amp: f32) {
    let mut w = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for i in 0..12_000 {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * amp * 8_388_607.0)
            as i32;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

/// The test's private fixture path. Windows gets a copy beside the temp files
/// so its gate files stay private; on macOS the fixture is already a signed
/// per-process bundle directory, which `fs::copy` cannot clone, so it loads in
/// place.
fn fixture(tmp: &std::path::Path) -> std::path::PathBuf {
    let original =
        xirius_test_vst3::fixture_path(std::env::current_exe().unwrap().parent().unwrap());
    #[cfg(target_os = "macos")]
    {
        let _ = tmp;
        original
    }
    #[cfg(not(target_os = "macos"))]
    {
        let dll = tmp.join(original.file_name().unwrap());
        std::fs::copy(&original, &dll).unwrap();
        dll
    }
}

/// The binary `dlopen` needs inside a fixture path: the bundle's executable on
/// macOS, the file itself elsewhere.
fn fixture_binary(path: &std::path::Path) -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    {
        path.join("Contents/MacOS/Fixture")
    }
    #[cfg(not(target_os = "macos"))]
    {
        path.to_path_buf()
    }
}

fn capture(h: &mut SessionHost, a: &mut AudioEngine) -> xirius_session::plugins::StatePull {
    let receipt = h.begin_pull_states();
    let end = Instant::now() + Duration::from_secs(15);
    while !receipt.ready() {
        pull(h, a, 1);
        assert!(Instant::now() < end);
    }
    receipt
}

fn scenario(kept: bool) {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("hit.wav");
    let other = tmp.path().join("new.wav");
    tone(&file, 0.8);
    tone(&other, 0.4);
    xirius_test_vst3::fixture_marker();
    let dll = fixture(tmp.path());
    // SAFETY: only this private, built test fixture is loaded; the library
    // outlives the queried function and all instances used below.
    let module = unsafe { libloading::Library::new(fixture_binary(&dll)).unwrap() };
    let creations = unsafe {
        module
            .get::<unsafe extern "system" fn() -> u32>(b"FixtureCreations")
            .unwrap()
    };
    let entries = xirius_plugins::probe(&dll).unwrap();
    let mut s = Session::with_registry(Project::new("live drum"), commands::shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    let t = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument":"Drum Kit"}),
    );
    let track: TrackId = t["id"].as_str().unwrap().parse().unwrap();
    let drum = t["plugin"].clone();
    let external_track = if kept {
        run(
            &mut s,
            "track.add.with-instrument",
            json!({"instrument":"Sampler"}),
        )["id"]
            .clone()
    } else {
        t["id"].clone()
    };
    let ext = run(
        &mut s,
        "plugin.add",
        json!({
            "track":external_track, "name":entries[0].name, "format":"vst3", "uid":entries[0].uid
        }),
    );
    let ext_id = ext["id"].as_str().unwrap().parse().unwrap();
    run(&mut s, "track.arm", json!({"ids":[track],"on":true}));
    let clip = run(
        &mut s,
        "clip.add.midi",
        json!({"track":track,"at":0,"bars":1}),
    );
    let note = run(
        &mut s,
        "note.add",
        json!({"clip":clip["id"],"at":0,"pitch":48,"length":480,"velocity":100}),
    );
    let (mut h, mut a, warnings) = SessionHost::start(
        s.project(),
        HostConfig {
            catalog: Some(Arc::new(Catalog {
                entries,
                failures: vec![],
            })),
            isolation: Some(Isolation::InProcess),
            threaded_local: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    // An uncollected patch on the real owner, absent from the project.
    assert!(h.plugins().set_live_param(ext_id, "1", 0.25));
    pull(&mut h, &mut a, 100);
    let made = unsafe { creations() };
    h.set_windowed();
    xirius_session::host::mark_window_thread();
    let fault = Arc::new(AtomicBool::new(true));
    let requests = Arc::new(AtomicUsize::new(0));
    let (f, n) = (fault.clone(), requests.clone());
    let _observe = xirius_core::io_test::observe(Arc::new(move |op, _| {
        if op == "prepare_get_state" {
            n.fetch_add(1, Ordering::SeqCst);
            if f.load(Ordering::SeqCst) {
                return Err(std::io::Error::other("fixture capture failure"));
            }
        }
        Ok(())
    }));
    for pad in ["A13", "A14"] {
        run(
            &mut s,
            "plugin.samples.load",
            json!({"id":drum,"path":file,"pad":pad}),
        );
    }
    h.sync(s.project());
    let warnings = settle(&mut h, &mut a);
    let velocity = 100.0 / 127.0;
    if kept {
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            requests.load(Ordering::SeqCst),
            0,
            "retained owner is not captured"
        );
        assert_eq!(unsafe { creations() }, made, "instance/owner kept");
        assert!(h.failed_graph_retry().is_none());
    } else {
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("fixture capture failure"))
        );
        assert!(h.instrument_status(track).is_err());
        assert!(!h.instrument_ready(track));
        assert!(!h.live_note_allowed(
            track,
            &NoteKind::On {
                pitch: 48,
                velocity
            }
        ));
        for release in [
            NoteKind::Off { pitch: 48 },
            NoteKind::control(120, 0),
            NoteKind::control(123, 0),
        ] {
            assert!(
                h.live_note_allowed(track, &release),
                "release/panic admission is independent of readiness"
            );
        }
        assert!(!h.note(
            track,
            NoteKind::On {
                pitch: 48,
                velocity
            }
        ));
        assert!(
            h.note(track, NoteKind::Off { pitch: 48 }),
            "release reaches old graph"
        );
        h.play();
        assert_eq!(pull(&mut h, &mut a, 100), 0.0, "old empty graph");
        h.stop();
        let ticket = h.failed_graph_retry().unwrap();
        let count = requests.load(Ordering::SeqCst);
        for _ in 0..5 {
            h.sync(s.project());
            settle(&mut h, &mut a);
            assert!(!h.note(
                track,
                NoteKind::On {
                    pitch: 49,
                    velocity
                }
            ));
        }
        assert_eq!(
            requests.load(Ordering::SeqCst),
            count,
            "poll/key cannot retry"
        );
        assert!(h.retry_failed_graph(ticket.clone()));
        assert!(!h.retry_failed_graph(ticket.clone()), "one request");
        assert!(!settle(&mut h, &mut a).is_empty(), "fault still active");
        assert!(
            !h.retry_failed_graph(ticket),
            "old save cannot retry newer failure"
        );
    }
    let receipt = capture(&mut h, &mut a);
    assert!(
        receipt.error().is_some(),
        "saving still sees the capture failure"
    );
    fault.store(false, Ordering::SeqCst);
    if let Some(ticket) = h.failed_graph_retry() {
        assert!(h.retry_failed_graph(ticket));
        assert!(settle(&mut h, &mut a).is_empty());
    }
    assert_eq!(h.instrument_status(track), Ok(true));
    for pitch in [48u8, 49] {
        assert!(h.note(track, NoteKind::On { pitch, velocity }));
        assert!(pull(&mut h, &mut a, 100) > 0.1);
        h.note(track, NoteKind::Off { pitch });
        run(
            &mut s,
            "note.set",
            json!({"notes":[{"id":note["id"],"pitch":pitch}]}),
        );
        h.sync(s.project());
        assert!(settle(&mut h, &mut a).is_empty());
        h.seek_tick(0);
        h.play();
        assert!(pull(&mut h, &mut a, 100) > 0.1);
        h.stop();
        run(
            &mut s,
            "note.set",
            json!({"notes":[{"id":note["id"],"pitch":60}]}),
        );
        h.sync(s.project());
        assert!(settle(&mut h, &mut a).is_empty());
        h.record_start(
            s.project(),
            Some(tmp.path()),
            RecordOptions {
                at: Some(0),
                midi: Some(Box::new(MockMidi::new(vec![
                    (0, [0x90, pitch, 100]),
                    (12000, [0x80, pitch, 0]),
                ]))),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(pull(&mut h, &mut a, 100) > 0.1);
        assert!(!h.live_notes().is_empty());
        h.record_stop();
        pull(&mut h, &mut a, 40);
        assert!(
            h.record_takes(Duration::from_secs(2))
                .unwrap()
                .warnings
                .is_empty()
        );
    }
    run(
        &mut s,
        "plugin.samples.load",
        json!({"id":drum,"path":other,"index":2}),
    );
    h.sync(s.project());
    assert!(settle(&mut h, &mut a).is_empty());
    h.note(
        track,
        NoteKind::On {
            pitch: 49,
            velocity,
        },
    );
    let peak = pull(&mut h, &mut a, 100);
    assert!(
        (0.20..0.30).contains(&peak),
        "new half-level sample: {peak}"
    );
    if kept {
        // A decode that finishes after undo must never publish its louder file.
        let delayed = tmp.path().join("late.wav");
        tone(&delayed, 0.9);
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let release = Arc::new(AtomicBool::new(false));
        struct Release(Arc<AtomicBool>);
        impl Drop for Release {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let guard = Release(release.clone());
        let path = delayed.clone();
        let _gate = xirius_core::io_test::observe(Arc::new(move |op, p| {
            if op == "prepare_decode" && p == path {
                let _ = entered_tx.send(());
                let deadline = Instant::now() + Duration::from_secs(15);
                while !release.load(Ordering::SeqCst) {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            Ok(())
        }));
        run(
            &mut s,
            "plugin.samples.add",
            json!({"id":drum,"file":delayed,"pad":"A14"}),
        );
        h.sync(s.project());
        h.step_swap();
        entered_rx.recv_timeout(Duration::from_secs(15)).unwrap();
        run(&mut s, "history.undo", json!({}));
        h.sync(s.project());
        drop(guard);
        assert!(settle(&mut h, &mut a).is_empty());
        h.note(
            track,
            NoteKind::On {
                pitch: 49,
                velocity,
            },
        );
        assert!(
            (0.20..0.30).contains(&pull(&mut h, &mut a, 100)),
            "late decoded candidate discarded"
        );
    } else {
        // Keep a voice running in the accepted graph, then fail a replacement.
        run(
            &mut s,
            "plugin.param.set",
            json!({"id":drum,"param":"A14.trigger","value":1.0/3.0}),
        );
        h.sync(s.project());
        assert!(settle(&mut h, &mut a).is_empty());
        h.note(
            track,
            NoteKind::On {
                pitch: 49,
                velocity,
            },
        );
        assert!(pull(&mut h, &mut a, 100) > 0.1);
        fault.store(true, Ordering::SeqCst);
        run(
            &mut s,
            "plugin.samples.load",
            json!({"id":drum,"path":file,"index":2}),
        );
        h.sync(s.project());
        assert!(!settle(&mut h, &mut a).is_empty());
        assert!(h.instrument_status(track).is_err());
        let old = pull(&mut h, &mut a, 100);
        assert!(
            (0.20..0.30).contains(&old),
            "failed replacement retains old looping PCM"
        );
        assert!(h.note(track, NoteKind::Off { pitch: 49 }));
        pull(&mut h, &mut a, 100);
        assert_eq!(
            pull(&mut h, &mut a, 100),
            0.0,
            "release is not blocked by failure"
        );
        fault.store(false, Ordering::SeqCst);
        assert!(h.retry_failed_graph(h.failed_graph_retry().unwrap()));
        assert!(settle(&mut h, &mut a).is_empty());
    }
    let receipt = capture(&mut h, &mut a);
    assert!(receipt.error().is_none());
    xirius_session::host::collect_pulled_states(&mut s, &mut h, &receipt, &RunOptions::human())
        .unwrap();
    let state = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|p| p.id == ext_id)
        .unwrap()
        .state
        .as_ref()
        .unwrap();
    assert_eq!(
        decode_state(state).unwrap().component,
        64u32.to_le_bytes(),
        "uncollected external patch survived every stock update"
    );
}

#[test]
fn live_stock_edits_keep_owners_and_required_capture_failures_retry_once() {
    // mark_window_thread is thread-local; each scenario starts an independent
    // real owner before registering its host as windowed.
    for kept in [true, false] {
        std::thread::spawn(move || scenario(kept)).join().unwrap();
    }
}

#[test]
fn keyed_routes_keep_playing_during_unrelated_failure_but_reject_changed_key_signals() {
    // Track, downstream bus, and master consumers; the bus case also taps a
    // key bus, so its upstream contributor must participate in readiness.
    for placement in [0, 1, 2] {
        std::thread::spawn(move || keyed_route_case(placement, false))
            .join()
            .unwrap();
    }
}

#[test]
fn saved_retry_and_process_refresh_keep_automated_healthy_tracks_ready_after_another_failure() {
    std::thread::spawn(|| keyed_route_case(0, true))
        .join()
        .unwrap();
}

fn keyed_route_case(placement: usize, automated: bool) {
    use xirius_core::id::SendId;
    use xirius_core::model::{KeyMode, KeySource, Sidechain};
    let tmp = tempfile::tempdir().unwrap();
    let hit = tmp.path().join("hit.wav");
    let changed = tmp.path().join("changed.wav");
    tone(&hit, 0.8);
    tone(&changed, 0.4);
    xirius_test_vst3::fixture_marker();
    let dll = fixture(tmp.path());
    let entries = xirius_plugins::probe(&dll).unwrap();
    let mut s = Session::with_registry(
        Project::new("keyed live route"),
        commands::shared_registry(),
    );
    commands::attach(&mut s, ExtConfig::default());
    let key = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument":"Drum Kit"}),
    );
    let heard = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument":"Drum Kit"}),
    );
    let other = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument":"Drum Kit"}),
    );
    for row in [&key, &heard] {
        run(
            &mut s,
            "plugin.samples.add",
            json!({"id":row["plugin"],"file":hit,"pad":"A13"}),
        );
    }
    run(
        &mut s,
        "plugin.add",
        json!({"track":other["id"],"name":"Fixture","format":"vst3","uid":entries[0].uid}),
    );
    let track: TrackId = heard["id"].as_str().unwrap().parse().unwrap();
    let key_track: TrackId = key["id"].as_str().unwrap().parse().unwrap();
    let mut source = key_track;
    let consumer = match placement {
        0 => track,
        1 => {
            let bus = run(&mut s, "track.add.bus", json!({"name":"Heard bus"}));
            run(
                &mut s,
                "track.output.set",
                json!({"ids":[track],"to":bus["id"]}),
            );
            let key_bus = run(&mut s, "track.add.bus", json!({"name":"Key bus"}));
            source = key_bus["id"].as_str().unwrap().parse().unwrap();
            run(
                &mut s,
                "track.output.set",
                json!({"ids":[key_track],"to":source}),
            );
            bus["id"].as_str().unwrap().parse().unwrap()
        }
        _ => TrackId::MASTER,
    };
    let comp = run(
        &mut s,
        "plugin.add",
        json!({"track":consumer,"name":"Compressor"}),
    );
    let comp_id = comp["id"].as_str().unwrap().parse().unwrap();
    if automated {
        run(
            &mut s,
            "clip.add.midi",
            json!({"track":track,"at":0,"bars":128}),
        );
    }
    let mut p = s.project().clone();
    if automated {
        use xirius_core::auto::{AutoLane, AutoPoint, AutoTarget};
        let t = Arc::make_mut(p.tracks.get_mut(&track).unwrap());
        let instrument = &mut t.inserts[0];
        let (_, volume) = instrument.builtin().unwrap().param("volume").unwrap();
        instrument
            .params
            .insert("volume".into(), volume.to_norm(0.0));
        let mut lane = AutoLane::new(AutoTarget::Param(instrument.id, "volume".into()));
        lane.points = Arc::new(vec![AutoPoint::new(0, volume.to_norm(-6.0))]);
        t.automation.push(lane);
    }
    Arc::make_mut(p.tracks.get_mut(&consumer).unwrap())
        .inserts
        .iter_mut()
        .find(|x| x.id == comp_id)
        .unwrap()
        .sidechain = Some(Sidechain {
        on: true,
        sources: vec![KeySource {
            id: SendId(9000),
            track: source,
            mode: KeyMode::Send {
                pre: true,
                level_db: 0.0,
                muted_level: None,
            },
        }],
    });
    let (mut h, mut audio, warnings) = SessionHost::start(
        &p,
        HostConfig {
            catalog: Some(Arc::new(Catalog {
                entries,
                failures: vec![],
            })),
            isolation: Some(Isolation::InProcess),
            threaded_local: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    h.set_windowed();
    if automated {
        assert!(h.play());
        pull(&mut h, &mut audio, 20);
        assert!(audio.is_playing());
    }
    let note = NoteKind::On {
        pitch: 48,
        velocity: 100.0 / 127.0,
    };
    assert!(h.note(track, note));
    let baseline = pull(&mut h, &mut audio, 100);
    assert!(baseline > 0.1);
    let fault = Arc::new(AtomicBool::new(true));
    let f = fault.clone();
    let _fault = xirius_core::io_test::observe(Arc::new(move |op, _| {
        if op == "prepare_get_state" && f.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("fixture capture failure"));
        }
        Ok(())
    }));
    let other_track: TrackId = other["id"].as_str().unwrap().parse().unwrap();
    Arc::make_mut(p.tracks.get_mut(&other_track).unwrap()).inserts[0]
        .samples
        .push(changed.to_string_lossy().into_owned());
    h.sync(&p);
    assert!(!settle(&mut h, &mut audio).is_empty());
    assert!(h.has_failed_graph());
    assert_eq!(
        h.instrument_status(track),
        Ok(true),
        "unrelated failed drum is not a key dependency"
    );
    assert!(h.note(track, note));
    assert!((pull(&mut h, &mut audio, 100) - baseline).abs() < 0.001);

    if automated {
        // Same Session API used by the complete-save UI path; the existing UI
        // integration covers disk success/generation authorization separately.
        // Keep the raw stored volume different from the audible automation.
        for process_refresh in [false, true] {
            fault.store(false, Ordering::SeqCst);
            if process_refresh {
                h.refresh_plugin_processes();
            } else {
                let ticket = h.failed_graph_retry().unwrap();
                assert!(h.retry_failed_graph_after_save(ticket.clone(), &p));
                assert!(settle(&mut h, &mut audio).is_empty());
                assert!(h.graph_retry_applied(&ticket));
            }
            assert!(settle(&mut h, &mut audio).is_empty());
            assert_eq!(
                h.song().tracks[&track].inserts[0].params,
                p.tracks[&track].inserts[0].params
            );
            fault.store(true, Ordering::SeqCst);
            Arc::make_mut(p.tracks.get_mut(&other_track).unwrap()).inserts[0]
                .samples
                .push(hit.to_string_lossy().into_owned());
            h.sync(&p);
            assert!(!settle(&mut h, &mut audio).is_empty());
            assert!(h.has_failed_graph());
            assert_eq!(h.instrument_status(track), Ok(true));
            assert!(h.note(track, note));
            assert!((pull(&mut h, &mut audio, 100) - baseline).abs() < 0.001);
            assert!(audio.is_playing());
        }
        return;
    }

    // The key's sample change is upstream of the consumer even when that key
    // arrives through a bus. This target must not be reported as accepted.
    Arc::make_mut(p.tracks.get_mut(&key_track).unwrap()).inserts[0].samples[0] =
        changed.to_string_lossy().into_owned();
    h.sync(&p);
    assert!(!settle(&mut h, &mut audio).is_empty());
    assert!(h.instrument_status(track).is_err());
    assert!(!h.note(track, note));
    assert!(h.note(track, NoteKind::Off { pitch: 48 }));
    assert_eq!(pull(&mut h, &mut audio, 100), 0.0);
}

#[test]
fn failed_live_updates_retain_latency_before_failure_and_after_failure_until_explicit_retry() {
    for before in [true, false] {
        std::thread::spawn(move || failed_latency_case(before))
            .join()
            .unwrap();
    }
}

fn failed_latency_case(before: bool) {
    use std::collections::BTreeMap;
    use xirius_core::id::{ClipId, Node, PluginId};
    use xirius_core::model::{
        Clip, ClipBody, Plugin, PluginIsolation, PluginKind, Track, TrackKind,
    };
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("impulse.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for frame in 0..12_000 {
        let sample = if frame == 4096 { 4_194_304i32 } else { 0 };
        writer.write_sample(sample).unwrap();
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
    xirius_test_vst3::fixture_marker();
    let dll = fixture(tmp.path());
    let entries = xirius_plugins::probe(&dll).unwrap();
    let pid = PluginId(200);
    let mut p = Project::new("failed latency rebuild");
    for i in 0..2 {
        let id = TrackId(100 + i);
        let mut track = Track::new(id, TrackKind::Audio, i.to_string());
        track.clips.push(Clip {
            id: ClipId(300 + i),
            name: "impulse".into(),
            color: None,
            start: 0,
            length: 400,
            muted: false,
            take: None,
            controllers: vec![],
            body: ClipBody::Audio {
                file: path.to_string_lossy().into_owned(),
                offset: 0,
                gain_db: 0.0,
                process: Default::default(),
            },
        });
        if i == 0 {
            track.inserts.push(Plugin {
                samples: vec![],
                id: pid,
                name: "Latency fixture".into(),
                kind: PluginKind::Effect,
                format: "vst3".into(),
                uid: entries[0].uid.clone(),
                bypass: false,
                deactivated: false,
                params: BTreeMap::new(),
                state: None,
                isolation: PluginIsolation::InProcess,
                sidechain: None,
                preset: None,
            });
        }
        p.tracks.insert(id, Arc::new(track));
        p.root.push(Node::Track(id));
    }
    let (mut h, mut audio, warnings) = SessionHost::start(
        &p,
        HostConfig {
            catalog: Some(Arc::new(Catalog {
                entries,
                failures: vec![],
            })),
            isolation: Some(Isolation::InProcess),
            threaded_local: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    h.set_windowed();
    pull(&mut h, &mut audio, 20);
    let initial = audio.latency();
    let fault = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let f = fault.clone();
    let c = calls.clone();
    let _hook = xirius_core::io_test::observe(Arc::new(move |op, _| {
        if op == "prepare_get_state" {
            c.fetch_add(1, Ordering::SeqCst);
            if f.load(Ordering::SeqCst) {
                return Err(std::io::Error::other("fixture capture failure"));
            }
        }
        Ok(())
    }));
    if !before {
        // The existing latency integration also uses a renamed hosted insert
        // to require transfer without changing its DSP state.
        Arc::make_mut(p.tracks.get_mut(&TrackId(100)).unwrap()).inserts[0].name = "Renamed".into();
        h.sync(&p);
        assert!(!settle(&mut h, &mut audio).is_empty());
        assert!(h.has_failed_graph());
    }
    assert!(h.plugins().set_live_param(pid, "1", 0.5));
    // An owner-thread plugin's restart flag rides the edits poll that a window
    // runs each frame; collect states on a spare session the same way (PRJ-030).
    let mut s = Session::with_registry(p.clone(), commands::shared_registry());
    let end = Instant::now() + Duration::from_secs(15);
    while !h.swap_pending() {
        pull(&mut h, &mut audio, 1);
        xirius_session::host::collect_plugin_states(&mut s, &mut h, &RunOptions::human()).unwrap();
        assert!(
            Instant::now() < end,
            "real latency notification did not arrive"
        );
    }
    let warnings = settle(&mut h, &mut audio);
    assert_eq!(warnings.is_empty(), !before);
    assert!(h.has_failed_graph());
    assert_eq!(audio.latency(), initial, "failed attempt did not publish");
    let first = h.failed_graph_retry().unwrap();
    let captured = calls.load(Ordering::SeqCst);
    assert!(captured > 0);
    for _ in 0..8 {
        h.sync(&p);
        let _ = settle(&mut h, &mut audio);
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        captured,
        "poll/sync must not retry capture"
    );

    // A second explicit failure must retain the debt and invalidate the first
    // ticket. Reusing that old ticket cannot consume the pending latency.
    assert!(h.retry_failed_graph(first.clone()));
    assert!(!settle(&mut h, &mut audio).is_empty());
    assert!(!h.retry_failed_graph(first));
    fault.store(false, Ordering::SeqCst);
    let latest = h.failed_graph_retry().unwrap();
    assert!(h.retry_failed_graph(latest.clone()));
    assert!(settle(&mut h, &mut audio).is_empty());
    assert!(h.graph_retry_applied(&latest));
    assert_eq!(
        audio.latency(),
        initial + 128,
        "retry rebuilt the retained latency debt"
    );

    // Offline PCM checks compensation of the untouched dry track as well as
    // the hosted track. Both half-scale impulses must coincide, not merely
    // report the same nominal plan latency.
    assert!(h.seek_tick(0));
    assert!(h.play());
    let mut out = Vec::new();
    for _ in 0..40 {
        let mut block = [0.0; 960];
        audio.render(&mut block, 2);
        out.extend(block);
        h.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame = 4096 + audio.latency() as usize;
    assert_eq!(&out[frame * 2..frame * 2 + 2], &[1.0, 1.0]);
    assert_eq!(out.iter().filter(|v| **v != 0.0).count(), 2);
}
