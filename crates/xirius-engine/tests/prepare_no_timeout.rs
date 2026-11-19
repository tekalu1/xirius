//! Slow foreign setup stays asynchronous, may be abandoned, and does not delay another host.
#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_engine::server::{PluginLink, PluginServer, RemotePlugin, RemoteSpec};
use xirius_engine::{AudioIo, ProcessSetup, Processor, RenderMode};

mod crash_records;
use crash_records::CrashRecords;

fn remote(server: &PluginServer) -> (RemotePlugin, Arc<PluginLink>) {
    let bundle = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    assert!(bundle.exists(), "prepare the test fixture first");
    let link = PluginLink::new();
    let plugin = RemotePlugin::new(
        server.clone(),
        link.clone(),
        RemoteSpec {
            name: "slow fixture".into(),
            bundle,
            ..RemoteSpec::default()
        },
    );
    (plugin, link)
}

fn server() -> PluginServer {
    PluginServer::new(Some(PathBuf::from(env!(
        "CARGO_BIN_EXE_xirius-plugin-host"
    ))))
}

fn ready() -> ProcessSetup {
    ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 128,
        mode: RenderMode::Realtime,
    }
}

fn until(mut condition: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < end,
            "host did not reach the expected state"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_thirty_second_setup_can_be_cancelled_and_does_not_block_another_host() {
    let _records = CrashRecords::new();
    let slow_host = server();
    let (mut slow, slow_link) = remote(&slow_host);
    // The host inherits this before any plugin code runs. The client clears it before
    // creating another host, so the second host remains independent.
    unsafe { std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", "30000") };
    let started = Instant::now();
    slow.begin_prepare(&ready());
    assert!(started.elapsed() < Duration::from_secs(5));
    until(|| slow_host.pid().is_some());
    unsafe { std::env::remove_var("XIRIUS_TEST_VST3_OPEN_MS") };
    assert!(slow.preparing());

    let other_host = server();
    let (mut other, other_link) = remote(&other_host);
    other.prepare(&ready());
    assert_eq!(other.error(), None);
    other_host.set_param(&other_link, "1", 0.5);
    let mut left = [0.25; 128];
    let mut right = left;
    other.process(
        &mut AudioIo {
            left: &mut left,
            right: &mut right,
            key: None,
            pad_routes: None,
            route_owner: 0,
        },
        &[],
    );
    assert_eq!(other_host.status(&other_link).wet_blocks, 1);

    drop(slow);
    until(|| slow_host.pid().is_none() && slow_host.has_let_go(&slow_link));
    assert!(other_host.pid().is_some());

    let long_host = server();
    let (mut long, _) = remote(&long_host);
    unsafe { std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", "30000") };
    let began = Instant::now();
    long.begin_prepare(&ready());
    until(|| long_host.pid().is_some());
    unsafe { std::env::remove_var("XIRIUS_TEST_VST3_OPEN_MS") };
    long.finish_prepare(&ready());
    assert_eq!(long.error(), None);
    assert!(began.elapsed() >= Duration::from_secs(30));
}
