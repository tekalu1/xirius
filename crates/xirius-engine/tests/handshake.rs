//! The engine and `xirius-plugin-host` check that they speak the same protocol when the host
//! starts, so an executable left over from another build gives a clear error instead of
//! misread messages. The host is told to pretend to be another version through its test
//! switch; this file holds one test, since it changes the environment the hosts inherit.

use std::sync::Arc;

use xirius_engine::server::{PluginLink, PluginServer, RemotePlugin, RemoteSpec};
use xirius_engine::wire::PROTOCOL;
use xirius_engine::{ProcessSetup, Processor, RenderMode};

const SWITCH: &str = "XIRIUS_PLUGIN_HOST_PROTOCOL";

fn server() -> PluginServer {
    PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()))
}

fn claim(v: Option<&str>) {
    // SAFETY: the only test in this binary; no other thread reads the environment meanwhile.
    unsafe {
        match v {
            Some(v) => std::env::set_var(SWITCH, v),
            None => std::env::remove_var(SWITCH),
        }
    }
}

/// Opens a plugin on `server` and returns why it could not.
fn open_error(server: &PluginServer) -> Option<String> {
    let link = PluginLink::new();
    let mut plugin = RemotePlugin::new(
        server.clone(),
        Arc::clone(&link),
        RemoteSpec {
            name: "any".into(),
            bundle: "C:/nowhere/missing.vst3".into(),
            ..RemoteSpec::default()
        },
    );
    plugin.prepare(&ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Offline,
    });
    link.error()
}

#[test]
fn a_host_of_another_version_is_refused_with_a_clear_error() {
    claim(None);
    server().start().expect("the host of this build");

    // A host from before the check does not know `--protocol` and ends without a word.
    claim(Some("none"));
    let e = server().start().unwrap_err();
    eprintln!("old host: {e}");
    assert!(
        e.contains("xirius-plugin-host") && e.contains("古い"),
        "{e}"
    );
    assert!(e.contains("同じビルド"), "says what to do: {e}");

    // A host that speaks another protocol says which.
    let other = PROTOCOL + 1;
    claim(Some(&other.to_string()));
    let s = server();
    let e = s.start().unwrap_err();
    eprintln!("other protocol: {e}");
    assert!(e.contains("バージョンが一致しません"), "{e}");
    assert!(
        e.contains(&format!("は {other}")) && e.contains(&format!("エンジンは {PROTOCOL}")),
        "{e}"
    );
    // Opening a plugin on it reports the same, and it is not taken for a crash.
    let e = open_error(&s).expect("an error");
    assert!(e.contains("バージョンが一致しません"), "{e}");
    assert!(!s.crashed());
    assert_eq!(s.status(&PluginLink::default()).crashes, 0);

    claim(None);
    server().start().expect("the right host again");
}
