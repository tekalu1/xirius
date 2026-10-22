//! A `.clap` file in a scanned folder is listed through the real scan process (PLG-028). The
//! file is `xirius-test-clap` under the name a CLAP file has.

use std::path::Path;
use std::time::Duration;

use xirius_core::model::PluginKind;
use xirius_plugins::{Catalog, ChildProgram, ScanOptions, scan};

/// A folder of the test's own, removed when dropped (also when the test fails).
fn temp_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-plugins-{name}-"))
        .tempdir()
        .unwrap()
}

fn options(plugins: &Path, cache: &Path) -> ScanOptions {
    ScanOptions {
        dirs: vec![plugins.to_path_buf()],
        cache: cache.to_path_buf(),
        program: Some(ChildProgram {
            program: env!("CARGO_BIN_EXE_xirius-plugin-scan").into(),
            args: Vec::new(),
        }),
        full: false,
        retry: Vec::new(),
        jobs: 2,
        timeout: Duration::from_secs(20),
        extra_args: Vec::new(),
    }
}

#[test]
fn a_clap_file_is_listed_by_its_id() {
    xirius_test_clap::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let root_tmp = temp_dir("clap");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("CLAP").join("Xirius");
    std::fs::create_dir_all(&plugins).unwrap();
    let file = plugins.join("Xirius Test Gain.clap");
    std::fs::copy(deps.join("xirius_test_clap.dll"), &file).unwrap();

    let opts = options(&root.join("CLAP"), &root.join("plugins.json"));
    let r = scan(&opts).expect("scan");
    assert_eq!((r.bundles, r.probed, r.failed), (1, 1, 0));
    let [e] = r.catalog.entries.as_slice() else {
        panic!("one entry: {:?}", r.catalog.entries);
    };
    assert_eq!(e.uid, xirius_test_clap::ID);
    assert_eq!(e.name, xirius_test_clap::NAME);
    assert_eq!(e.vendor, "Xirius");
    assert_eq!(e.format, "clap");
    assert_eq!(e.kind, PluginKind::Effect);
    assert!(e.category.split('|').any(|f| f == "audio-effect"), "{e:?}");
    assert_eq!(e.path, file);
    assert_eq!(r.catalog.find(xirius_test_clap::ID), Some(e));
    assert_eq!(Catalog::load(&opts.cache), r.catalog);

    // Unchanged, it comes from the cache the next time.
    let again = scan(&opts).expect("scan again");
    assert_eq!((again.cached, again.probed), (1, 0));
    let _ = std::fs::remove_dir_all(&root);
}
