// The last-chance native fault log and automatic recovery require Windows SEH.
#![cfg(target_os = "windows")]

//! A real fatal fixture initialization is contained by its plugin host.
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
use xirius_session::plugins::Isolation;

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
        pad_outputs: Default::default(),
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
        note_source: None,
        preset: None,
        samples: vec![],
        isolation: PluginIsolation::Dedicated,
    }
}
fn run_child(root: &Path, mode: &str, host: &Path, dll: &Path) -> std::process::Output {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "plugin_crash_isolation::fixture_child",
            "--nocapture",
        ])
        .env("XIRIUS_ISOLATE_TEST_CHILD", mode)
        .env("XIRIUS_CONFIG_DIR", root.join("prefs"))
        .env("XIRIUS_CRASH_DIR", root.join("crashes"))
        .env("XIRIUS_ISOLATE_TEST_ROOT", root)
        .env("XIRIUS_ISOLATE_TEST_DLL", dll)
        .env("XIRIUS_ISOLATE_TEST_HOST", host)
        .env("XIRIUS_TEST_VST3_CRASH_OPEN", "1")
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
fn fatal_initialization_in_the_host_keeps_the_parent_alive_and_silent() {
    xirius_test_vst3::fixture_marker();
    let dll = xirius_test_vst3::fixture_path(std::env::current_exe().unwrap().parent().unwrap());
    assert!(dll.exists(), "a DLL on Windows, a bundle on macOS");
    let host = xirius_engine::server::find_host()
        .expect("build xirius-plugin-host before this integration test");
    let root = tempfile::tempdir().unwrap();
    let remote = run_child(root.path(), "remote", &host, &dll);
    assert!(
        remote.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&remote.stdout),
        String::from_utf8_lossy(&remote.stderr)
    );
    assert!(root.path().join("survived-silent").is_file());
}
/// The child process `run_child` starts. Ignored so it is not counted as a test of its own.
#[test]
#[ignore = "the child process's body; run_child starts it with --ignored"]
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
    let mut builder = xirius_session::build::Builder::new(48000, None);
    builder.set_catalog(Some(Arc::new(Catalog {
        entries: vec![e],
        failures: vec![],
    })));
    builder.set_isolation(Isolation::PerPlugin(Some(PathBuf::from(
        std::env::var_os("XIRIUS_ISOLATE_TEST_HOST").unwrap(),
    ))));
    let loader = builder.plugins_mut();
    let (mut processor, _) = loader.instantiate("Track", &x).unwrap();
    assert_eq!(mode, "remote");
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
