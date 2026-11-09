//! Retained host mailboxes must survive both empty and nonempty transfer pulls.
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::{Session, model::Project, session::RunOptions};
use xirius_session::{
    commands,
    ext::ExtConfig,
    host::{HostConfig, SessionHost},
    plugins::Isolation,
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

/// The test's private fixture path. Windows gets a copy beside the temp files;
/// on macOS the fixture is already a per-process bundle directory, which
/// `fs::copy` cannot clone, so it is used in place.
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

fn check(other_capture: bool) {
    let tmp = tempfile::tempdir().unwrap();
    let dll = fixture(tmp.path());
    let entries = xirius_plugins::probe(&dll).unwrap();
    let mut s = Session::with_registry(
        Project::new("passive drum edit"),
        commands::shared_registry(),
    );
    commands::attach(&mut s, ExtConfig::default());
    let drum = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument":"Drum Kit"}),
    );
    let other = run(&mut s, "track.add.audio", json!({}));
    let add = |s: &mut Session, track: &Value| {
        run(
            s,
            "plugin.add",
            json!({
                "track":track,"name":entries[0].name,"uid":entries[0].uid,"format":"vst3"
            }),
        )
    };
    let retained = add(&mut s, &other["id"]);
    if other_capture {
        add(&mut s, &drum["id"]);
    }
    let id = retained["id"].as_str().unwrap().parse().unwrap();
    run(
        &mut s,
        "plugin.param.set",
        json!({"id":retained["id"],"param":"2","value":0.5}),
    );
    let (mut h, mut audio, warnings) = SessionHost::start(
        s.project(),
        HostConfig {
            catalog: Some(Arc::new(xirius_plugins::Catalog {
                entries,
                failures: vec![],
            })),
            isolation: Some(Isolation::Separate(
                xirius_engine::server::find_host().expect("prepared host"),
            )),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    h.set_windowed();
    let link = h.plugins().link(id).unwrap();
    let before = link.live_revision();
    h.plugins().server_of(id).unwrap().simulate_edit(
        &link,
        xirius_test_vst3::MIX_PARAMETER,
        &[0.375],
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while link.live_revision() == before || !link.has_pending_state() {
        audio.render(&mut [0.0; 960], 2);
        h.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let file = tmp.path().join("hit.wav");
    xirius_session::export::write_wav(&file, &vec![0.25; 12000], &vec![0.25; 12000], 48000, 24)
        .unwrap();
    run(
        &mut s,
        "plugin.samples.add",
        json!({"id":drum["plugin"],"file":file,"pad":"A13"}),
    );
    h.sync(s.project());
    while h.swap_pending() {
        if let Some(r) = h.step_swap() {
            assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        }
        audio.render(&mut [0.0; 960], 2);
        h.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        Arc::ptr_eq(&link, &h.plugins().link(id).unwrap()),
        "retained instance link"
    );
    assert!(
        link.has_pending_state(),
        "transfer left the retained mailbox intact"
    );
    let collected =
        xirius_session::host::collect_plugin_states(&mut s, &mut h, &RunOptions::human()).unwrap();
    assert!(
        collected.result.is_some(),
        "retained editor mailbox was not consumed by transfer"
    );
    let plugin = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|p| p.id == id)
        .unwrap();
    assert!(
        plugin
            .params
            .iter()
            .any(|(key, v)| (key == "2" || key == "Mix") && (*v - 0.375).abs() < 1e-6),
        "passive Mix value survived: {:?}",
        plugin.params
    );
}

#[test]
fn retained_editor_mailbox_survives_empty_and_selective_graph_capture() {
    xirius_test_vst3::fixture_marker();
    for other_capture in [false, true] {
        std::thread::spawn(move || check(other_capture))
            .join()
            .unwrap();
    }
}
