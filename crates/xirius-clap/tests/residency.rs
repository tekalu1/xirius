//! A plugin can retain callbacks beyond its last instance or bundle (ARC-043).
#![cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use xirius_clap::{Bundle, Plugin};

#[test]
fn the_entry_stays_loaded_after_the_last_bundle_and_instance() {
    xirius_test_clap::fixture_marker();
    let path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_clap.dll");
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let bundle = Bundle::load(&path).unwrap();
    let plugin = Plugin::new(&bundle, xirius_test_clap::ID).unwrap();
    drop(bundle);
    drop(plugin);
    // SAFETY: a NUL-terminated module path; this does not take or release a reference.
    assert!(!unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null());
    assert!(!Bundle::load(&path).unwrap().descriptors().is_empty());
}

#[test]
fn a_binary_stays_mapped_even_when_the_clap_entry_cannot_be_loaded() {
    xirius_test_vst3::fixture_marker();
    let path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    for _ in 0..2 {
        assert!(Bundle::load(&path).is_err(), "VST3 has no CLAP entry");
    }
    // SAFETY: looking up the retained DLL reference, without changing its count.
    assert!(!unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null());
}
