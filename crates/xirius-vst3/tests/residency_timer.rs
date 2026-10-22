//! A DLL timer can outlive the last plugin. Run it in a child to contain the old crash.
use std::os::windows::ffi::OsStrExt;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Diagnostics::Debug::{SEM_NOGPFAULTERRORBOX, SetErrorMode};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use xirius_vst3::{Module, Plugin};

#[test]
fn timer_child() {
    if std::env::var_os("XIRIUS_RESIDENCY_TIMER").is_none() {
        return;
    }
    // SAFETY: no error dialog in this deliberately faulting child.
    unsafe {
        SetErrorMode(SEM_NOGPFAULTERRORBOX);
    }
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let module = Arc::new(Module::load(&dll).unwrap());
    let class = module.classes().remove(0);
    let plugin = Plugin::new(module, &class).unwrap();
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain([0]).collect();
    let mut counter = AtomicU32::new(0);
    // SAFETY: the fixture retains this stack counter only until the timer is dispatched.
    let (timer, callback) = unsafe {
        let arm: unsafe extern "system" fn(*mut AtomicU32) -> usize = std::mem::transmute(
            GetProcAddress(
                GetModuleHandleW(wide.as_ptr()),
                c"FixtureLingeringTimer".as_ptr().cast(),
            )
            .unwrap(),
        );
        let timer = arm(&mut counter);
        assert_ne!(timer, 0);
        let callback: unsafe extern "system" fn(*mut std::ffi::c_void, u32, usize, u32) =
            std::mem::transmute(
                GetProcAddress(
                    GetModuleHandleW(wide.as_ptr()),
                    c"FixtureTimerCallback".as_ptr().cast(),
                )
                .unwrap(),
            );
        (timer, callback)
    };
    drop(plugin);
    assert_eq!(Module::users(&dll), 0);
    // Call the pending TIMERPROC directly: DispatchMessage suppresses timer exceptions on
    // Windows, which can hide the use of unloaded code. No OS input or window is involved.
    // SAFETY: the same callback/arguments registered above; intentionally faults on the old
    // host, only in this disposable child. The counter remains alive until the call returns.
    unsafe {
        callback(std::ptr::null_mut(), 0x0113, timer, 0);
    }
    assert_eq!(counter.load(Ordering::Acquire), 1);
}

#[test]
fn a_dll_timer_still_runs_after_the_last_plugin_is_dropped() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "timer_child", "--nocapture"])
        .env("XIRIUS_RESIDENCY_TIMER", "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "DLL callback child: {status}");
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("DLL callback child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
