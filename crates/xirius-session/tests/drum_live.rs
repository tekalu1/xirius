//! A stock instrument keeps sounding while a hosted insert is rebuilt.
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::{Session, model::Project, session::RunOptions};
use xirius_engine::{AudioEngine, NoteKind};
use xirius_session::{
    commands,
    ext::ExtConfig,
    host::{HostConfig, SessionHost},
    plugins::Isolation,
};

fn run(song: &mut Session, name: &str, args: Value) -> Value {
    commands::run(
        song,
        None,
        name,
        args.as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap()
    .data
}

fn render(host: &mut SessionHost, audio: &mut AudioEngine) -> f32 {
    let mut peak = 0.0_f32;
    for _ in 0..100 {
        let mut out = [0.0; 960];
        audio.render(&mut out, 2);
        peak = out.iter().fold(peak, |p, x| p.max(x.abs()));
        host.poll();
    }
    peak
}

fn settle(host: &mut SessionHost, audio: &mut AudioEngine) {
    let until = Instant::now() + Duration::from_secs(20);
    while host.swap_pending() {
        if let Some(report) = host.step_swap() {
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        }
        render(host, audio);
        assert!(Instant::now() < until);
    }
}

#[test]
fn live_drum_edits_sound_while_an_insert_is_hosted() {
    xirius_test_vst3::fixture_marker();
    let fixture =
        xirius_test_vst3::fixture_path(std::env::current_exe().unwrap().parent().unwrap());
    let entries = xirius_plugins::probe(&fixture).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let sample = tmp.path().join("hit.wav");
    xirius_session::export::write_wav(&sample, &[0.5; 12_000], &[0.5; 12_000], 48_000, 24).unwrap();
    let mut song = Session::with_registry(Project::new("live drum"), commands::shared_registry());
    commands::attach(&mut song, ExtConfig::default());
    let added = run(
        &mut song,
        "track.add.with-instrument",
        json!({"instrument":"Drum Kit"}),
    );
    let track = added["id"].as_str().unwrap().parse().unwrap();
    let drum = added["plugin"].clone();
    run(
        &mut song,
        "plugin.add",
        json!({"track":added["id"],"name":entries[0].name,
            "format":"vst3","uid":entries[0].uid}),
    );
    run(&mut song, "track.arm", json!({"ids":[track],"on":true}));
    let (mut host, mut audio, warnings) = SessionHost::start(
        song.project(),
        HostConfig {
            catalog: Some(Arc::new(xirius_plugins::Catalog {
                entries,
                failures: Vec::new(),
            })),
            isolation: Some(Isolation::PerPlugin(xirius_engine::server::find_host())),
            process_prefs: Some(xirius_prefs::Prefs::default()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    host.set_windowed();
    run(
        &mut song,
        "plugin.samples.load",
        json!({"id":drum,"path":sample,"pad":"A13"}),
    );
    host.sync(song.project());
    settle(&mut host, &mut audio);
    assert_eq!(host.instrument_status(track), Ok(true));
    assert!(host.note(
        track,
        NoteKind::On {
            pitch: 48,
            velocity: 100.0 / 127.0,
        }
    ));
    assert!(render(&mut host, &mut audio) > 0.1);
    assert!(host.note(track, NoteKind::Off { pitch: 48 }));
}
