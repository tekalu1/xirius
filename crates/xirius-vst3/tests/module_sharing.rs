//! A bundle loaded again shares the loaded module, which stays until process exit.
//! One test in its own process, so no other test holds the module meanwhile.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use xirius_vst3::{Module, Plugin};

fn in_process(binary: &Path) -> bool {
    let w: Vec<u16> = binary.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: a NUL-terminated wide path; the handle is not kept.
    !unsafe { GetModuleHandleW(w.as_ptr()) }.is_null()
}

#[test]
fn a_module_is_loaded_once_and_kept_after_its_last_user() {
    xirius_test_vst3::fixture_marker();
    let path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    assert_eq!(Module::users(&path), 0);
    let first = Arc::new(Module::load(&path).expect("load"));
    let binary = first.binary().to_path_buf();
    let wide: Vec<u16> = binary.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: fixture export with the documented ABI, kept valid until process exit.
    let calls: unsafe extern "system" fn() -> u64 = unsafe {
        std::mem::transmute(
            GetProcAddress(
                GetModuleHandleW(wide.as_ptr()),
                c"FixtureModuleCalls".as_ptr().cast(),
            )
            .unwrap(),
        )
    };
    assert!(in_process(&binary));
    let second = Module::load(&path).expect("load again");
    assert_eq!(Module::users(&path), 2);
    assert_eq!(second.binary(), binary);

    let class = first
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .unwrap();
    let plugin = Plugin::new(first, &class).expect("create");
    drop(second);
    assert_eq!(Module::users(&path), 1, "the plugin keeps the module");
    assert!(in_process(&binary));

    drop(plugin);
    assert_eq!(Module::users(&path), 0);
    assert!(
        in_process(&binary),
        "callbacks may still point into the DLL"
    );

    // Loading again reuses the initialized library, even after zero users.
    let again = Module::load(&path).expect("load again");
    assert_eq!(Module::users(&path), 1);
    assert!(!again.classes().is_empty());
    // SAFETY: the module is loaded and the fixture getter only reads atomic counters.
    assert_eq!(
        unsafe { calls() },
        1,
        "InitDll once, no ExitDll at zero users"
    );
    drop(again);
    assert!(in_process(&binary));
}

#[test]
fn failed_init_child() {
    if std::env::var_os("XIRIUS_TEST_VST3_INIT_FAIL").is_none() {
        return;
    }
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    for _ in 0..2 {
        assert!(Module::load(&dll).is_err());
    }
    assert_eq!(Module::users(&dll), 0);
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the partially initialized fixture must still be mapped, with its counters intact.
    unsafe {
        let handle = GetModuleHandleW(wide.as_ptr());
        assert!(!handle.is_null());
        let calls: unsafe extern "system" fn() -> u64 = std::mem::transmute(
            GetProcAddress(handle, c"FixtureModuleCalls".as_ptr().cast()).unwrap(),
        );
        assert_eq!(
            calls(),
            1,
            "do not retry InitDll or call ExitDll after a failure"
        );
    }
}

#[test]
fn failed_initialization_is_not_repeated() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "failed_init_child"])
        .env("XIRIUS_TEST_VST3_INIT_FAIL", "1")
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("failed-initialization child timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
