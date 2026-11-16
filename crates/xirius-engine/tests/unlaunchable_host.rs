//! A host file that is not a program fails to start with an error, instead of the start waiting
//! on a dialog nobody answers. This file holds one test, since it changes the process's error
//! mode.

use xirius_engine::server::PluginServer;

#[test]
fn a_host_file_that_is_not_a_program_fails_to_start() {
    // Critical errors on, as in the app, whose crash handling sets the error mode without them; the
    // runner of the tests may have turned them off for its children.
    #[cfg(windows)]
    // SAFETY: no preconditions; the only test in this binary.
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode(0);
    }
    let dir = tempfile::tempdir().unwrap();
    let host = dir.path().join("not-a-host.exe");
    std::fs::write(&host, b"not an executable").unwrap();
    let e = PluginServer::new(Some(host.clone())).start().unwrap_err();
    assert!(e.contains(&host.display().to_string()), "{e}");
}
