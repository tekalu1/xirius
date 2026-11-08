//! Exercise real Mach-O loading without an audio device or an NSView.
#![cfg(target_os = "macos")]

use std::sync::Arc;
use xirius_vst3::{Module, Plugin, default_vst3_dirs, find_bundles};

#[test]
fn bundle_executable_name_sharing_processing_and_editor_refusal() {
    let exe = std::env::current_exe().unwrap();
    let path = xirius_test_vst3::fixture_path(exe.parent().unwrap());
    // The executable is named Fixture, not the .vst3 directory's stem.
    assert!(find_bundles(exe.parent().unwrap()).contains(&path));
    let first = Arc::new(Module::load(&path).unwrap());
    let second = Module::load(&path).unwrap();
    assert_eq!(Module::users(&path), 2);
    assert_eq!(first.binary(), path.join("Contents/MacOS/Fixture"));
    let class = first.classes().into_iter().next().unwrap();
    let mut plugin = Plugin::new(first, &class).unwrap();
    let error = plugin.open_editor("fixture").err().expect("no NSView yet");
    assert!(error.to_string().contains("not available on macOS"));
    let mut audio = plugin.audio().unwrap();
    audio.prepare(48_000.0, 64, true).unwrap();
    let (mut left, mut right) = ([0.25; 64], [-0.25; 64]);
    audio.process(&mut left, &mut right, &[]);
    assert_eq!(left, [0.25; 64]);
    assert_eq!(right, [-0.25; 64]);
    drop(second);
    assert_eq!(Module::users(&path), 1);
    drop(audio);
    drop(plugin);
    assert_eq!(Module::users(&path), 0);
    assert!(Module::load(&path).is_ok());
}

#[test]
fn standard_search_roots_and_invalid_bundle() {
    let roots = default_vst3_dirs();
    assert_eq!(
        roots[0],
        std::path::Path::new("/Library/Audio/Plug-Ins/VST3")
    );
    if let Some(home) = std::env::var_os("HOME") {
        assert!(
            roots.contains(&std::path::PathBuf::from(home).join("Library/Audio/Plug-Ins/VST3"))
        );
    }
    let exe = std::env::current_exe().unwrap();
    assert!(Module::load(&exe.with_extension("missing.vst3")).is_err());
    assert!(Module::load(&exe).is_err());
}
