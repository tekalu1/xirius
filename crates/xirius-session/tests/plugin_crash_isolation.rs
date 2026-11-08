//! Real fatal fixture initialization, recovery, and a dedicated host fault without windows.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use xirius_core::{
    id::PluginId,
    model::{Plugin, PluginIsolation, PluginKind},
};
use xirius_engine::{ProcessSetup, RenderMode, processor::AudioIo};
use xirius_plugins::{Catalog, Entry};
use xirius_session::{plugin_policy, plugins::Isolation};

fn entry(dll: PathBuf) -> Entry {
    Entry {
        uid: xirius_plugins::uid_hex(&xirius_test_vst3::CID.map(|byte| byte as u8)),
        name: "Crash fixture".into(),
        vendor: "Fixture".into(),
        kind: PluginKind::Instrument,
        format: "vst3".into(),
        category: "Instrument".into(),
        version: "1".into(),
        path: dll,
        ara: false,
        requires_process_main: false,
    }
}
fn plugin(e: &Entry) -> Plugin {
    Plugin {
        id: PluginId(9),
        name: e.name.clone(),
        kind: e.kind,
        format: e.format.clone(),
        uid: e.uid.clone(),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        sidechain: None,
        preset: None,
        samples: vec![],
        isolation: PluginIsolation::InProcess,
    }
}
fn run_child(root: &Path, mode: &str, host: &Path, dll: &Path) -> std::process::Output {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fixture_child", "--nocapture"])
        .env("XIRIUS_ISOLATE_TEST_CHILD", mode)
        .env("XIRIUS_CONFIG_DIR", root.join("prefs"))
        .env("XIRIUS_CRASH_DIR", root.join("crashes"))
        .env("XIRIUS_ISOLATE_TEST_ROOT", root)
        .env("XIRIUS_ISOLATE_TEST_DLL", dll)
        .env("XIRIUS_ISOLATE_TEST_HOST", host)
        .env(
            "XIRIUS_TEST_VST3_CRASH_OPEN",
            if mode == "switch" { "0" } else { "1" },
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(45);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= end {
            let _ = child.kill();
            panic!("fixture child timed out: {mode}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}
#[test]
fn fatal_initialization_is_attributed_once_and_next_initialization_is_dedicated_and_silent() {
    xirius_test_vst3::fixture_marker();
    let dll = xirius_test_vst3::fixture_path(std::env::current_exe().unwrap().parent().unwrap());
    assert!(dll.exists(), "a DLL on Windows, a bundle on macOS");
    let host = xirius_engine::server::find_host()
        .expect("build xirius-plugin-host before this integration test");
    let root = tempfile::tempdir().unwrap();
    let local = run_child(root.path(), "local", &host, &dll);
    assert!(
        !local.status.success(),
        "the first in-process insertion must be fatal"
    );
    let mut settings = xirius_prefs::Settings::open(root.path().join("prefs"));
    let catalog = Catalog {
        entries: vec![entry(dll.clone())],
        failures: vec![],
    };
    let recovered =
        plugin_policy::recover(&mut settings, &catalog, &root.path().join("crashes")).unwrap();
    assert_eq!(
        recovered.len(),
        1,
        "real last-chance log must identify the owner call"
    );
    assert!(
        plugin_policy::recover(&mut settings, &catalog, &root.path().join("crashes"))
            .unwrap()
            .is_empty()
    );
    let remote = run_child(root.path(), "remote", &host, &dll);
    assert!(
        remote.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&remote.stdout),
        String::from_utf8_lossy(&remote.stderr)
    );
    assert!(root.path().join("survived-silent").is_file());
    let switch_root = tempfile::tempdir().unwrap();
    let switched = run_child(switch_root.path(), "switch", &host, &dll);
    assert!(
        switched.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&switched.stdout),
        String::from_utf8_lossy(&switched.stderr)
    );
}
#[test]
fn fixture_child() {
    let Ok(mode) = std::env::var("XIRIUS_ISOLATE_TEST_CHILD") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("XIRIUS_ISOLATE_TEST_ROOT").unwrap());
    xirius_engine::crash::install_at(&root.join("crashes"), "xirius-ui", true).unwrap();
    let e = entry(PathBuf::from(
        std::env::var_os("XIRIUS_ISOLATE_TEST_DLL").unwrap(),
    ));
    let x = plugin(&e);
    if mode == "switch" {
        switch_running_processes(&root, e, x);
        return;
    }
    let mut builder = xirius_session::build::Builder::new(48000, None);
    builder.set_catalog(Some(Arc::new(Catalog {
        entries: vec![e],
        failures: vec![],
    })));
    builder.set_isolation(Isolation::PerPlugin(Some(PathBuf::from(
        std::env::var_os("XIRIUS_ISOLATE_TEST_HOST").unwrap(),
    ))));
    builder.set_threaded_local(true);
    let loader = builder.plugins_mut();
    let (mut processor, _) = loader.instantiate("Track", &x).unwrap();
    assert_eq!(
        mode, "remote",
        "in-process createInstance should never return"
    );
    assert_eq!(loader.placement_of(x.id), Some(PluginIsolation::Dedicated));
    let setup = ProcessSetup {
        sample_rate: 48000.0,
        max_block: 128,
        mode: RenderMode::Realtime,
    };
    processor.prepare(&setup);
    let (mut left, mut right) = ([1.0; 128], [1.0; 128]);
    processor.process(&mut AudioIo::new(&mut left, &mut right), &[]);
    assert!(
        left.iter().chain(&right).all(|s| *s == 0.0),
        "a faulted instrument must be silent"
    );
    assert!(
        !loader.opening_warnings().is_empty(),
        "host initialization failure must be reported"
    );
    std::fs::write(
        root.join("survived-silent"),
        b"dedicated host fault; parent survived; silent instrument",
    )
    .unwrap();
}

fn switch_running_processes(root: &Path, mut e: Entry, mut x: Plugin) {
    use xirius_session::host::{HostConfig, SessionHost};
    e.kind = PluginKind::Effect;
    x.kind = PluginKind::Effect;
    let mut session = xirius_core::Session::new("process-switch");
    session
        .run_value(
            "track.add.audio",
            serde_json::json!({"name":"Track"}),
            &xirius_core::session::RunOptions::human(),
        )
        .unwrap();
    let mut song = session.project().clone();
    Arc::make_mut(song.tracks.values_mut().next().unwrap())
        .inserts
        .push(x.clone());
    let (mut host, mut audio, warnings) = SessionHost::start(
        &song,
        HostConfig {
            threaded_local: true,
            catalog: Some(Arc::new(Catalog {
                entries: vec![e],
                failures: vec![],
            })),
            isolation: Some(Isolation::PerPlugin(Some(PathBuf::from(
                std::env::var_os("XIRIUS_ISOLATE_TEST_HOST").unwrap(),
            )))),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        host.plugins()
            .reports()
            .iter()
            .any(|r| r.id == x.id && r.placement == PluginIsolation::InProcess)
    );
    host.set_windowed();
    let mut settings = xirius_prefs::Settings::open(root.join("prefs"));
    for (value, expected) in [
        ("shared", Some(PluginIsolation::Shared)),
        ("dedicated", Some(PluginIsolation::Dedicated)),
        ("same", None),
    ] {
        settings
            .prefs
            .set(
                &xirius_prefs::isolation::key(&x.format, &x.uid),
                &serde_json::json!(value),
            )
            .unwrap();
        settings.save(xirius_prefs::File::Prefs).unwrap();
        host.refresh_plugin_processes();
        // A normal poll with the identical song must not cancel this rebuild.
        host.sync_later(&song);
        let end = Instant::now() + Duration::from_secs(15);
        while host.swap_pending() {
            if let Some(report) = host.step_swap() {
                assert!(report.warnings.is_empty(), "{value}: {:?}", report.warnings);
            }
            audio.render(&mut [0.0; 512], 2);
            host.poll();
            assert!(Instant::now() < end, "process change timed out: {value}");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(host.plugins().placement_of(x.id), expected, "{value}");
        if expected.is_none() {
            assert!(
                host.plugins()
                    .reports()
                    .iter()
                    .any(|r| r.id == x.id && r.placement == PluginIsolation::InProcess)
            );
        }
    }
}
