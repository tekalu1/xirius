//! Scan a real macOS VST3 bundle in the isolated scanner and reuse its cache.
#![cfg(target_os = "macos")]
use xirius_plugins::{Catalog, ChildProgram, ScanOptions, scan};

#[test]
fn macos_bundle_is_scanned_in_a_child_and_cached() {
    let exe = std::env::current_exe().unwrap();
    let fixture = xirius_test_vst3::fixture_path(exe.parent().unwrap());
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("VST3");
    let bundle = plugins.join("Different Name.vst3");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    for file in ["Contents/Info.plist", "Contents/MacOS/Fixture"] {
        std::fs::copy(fixture.join(file), bundle.join(file)).unwrap();
    }
    let options = ScanOptions {
        dirs: vec![plugins],
        cache: root.path().join("plugins.json"),
        program: Some(ChildProgram {
            program: env!("CARGO_BIN_EXE_xirius-plugin-scan").into(),
            args: Vec::new(),
        }),
        ..ScanOptions::default()
    };
    let result = scan(&options).unwrap();
    assert_eq!(
        (result.bundles, result.probed, result.failed),
        (1, 1, 0),
        "{:?}",
        result.catalog.failures
    );
    assert_eq!(result.catalog.entries.len(), 1);
    assert_eq!(result.catalog.entries[0].format, "vst3");
    assert_eq!(result.catalog.entries[0].path, bundle);
    assert_eq!(Catalog::load(&options.cache), result.catalog);
    let cached = scan(&options).unwrap();
    assert_eq!((cached.cached, cached.probed), (1, 0));
}
