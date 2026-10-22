//! Scanning folders of bundles through the real scan process (`xirius-plugin-scan`).
//!
//! The bundles are made here: a DLL that is not one, a folder without a binary, and names the
//! scan process is told to crash or hang on. When kHs Gain is installed, a copy of it is
//! scanned as well; without it that part is skipped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use xirius_core::model::PluginKind;
use xirius_plugins::{
    Catalog, ChildProgram, FailureKind, ScanOptions, scan, scan_seeing, scan_until,
};

/// A folder of the test's own, removed when dropped (also when the test fails).
fn temp_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-plugins-{name}-"))
        .tempdir()
        .unwrap()
}

/// A folder bundle whose binary is `bytes` (not a loadable DLL).
fn fake_bundle(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let b = dir.join(format!("{name}.vst3"));
    let bin = b.join("Contents").join("x86_64-win");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(format!("{name}.vst3")), bytes).unwrap();
    b
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
        jobs: 4,
        timeout: Duration::from_secs(20),
        extra_args: Vec::new(),
    }
}

fn khs_gain() -> Option<PathBuf> {
    let p = xirius_vst3::default_vst3_dir().join(r"Kilohearts\kHs Gain.vst3");
    if p.is_file() {
        Some(p)
    } else {
        eprintln!("skipped the real plugin: {} is not installed", p.display());
        None
    }
}

#[test]
fn the_scan_says_each_bundle_it_opens_and_not_the_ones_the_cache_had() {
    let root_tmp = temp_dir("seeing");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let a = fake_bundle(&plugins, "Alpha", b"not a dll");
    let b = fake_bundle(&plugins, "Beta", b"not a dll 2");
    let opts = options(&plugins, &root.join("plugins.json"));
    let seen = std::sync::Mutex::new(Vec::<PathBuf>::new());
    let see = |p: &Path| seen.lock().unwrap().push(p.to_path_buf());
    let r = scan_seeing(&opts, &|_, _| {}, &see, &|| false).expect("scan");
    assert_eq!(r.probed, 2);
    let mut got = seen.lock().unwrap().clone();
    got.sort();
    assert_eq!(got, [a, b]);

    // The next one opens nothing: it says nothing.
    seen.lock().unwrap().clear();
    let r = scan_seeing(&opts, &|_, _| {}, &see, &|| false).expect("scan");
    assert_eq!((r.cached, r.probed), (2, 0));
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn a_crashing_or_hanging_bundle_does_not_stop_the_scan() {
    let root_tmp = temp_dir("crash");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    fake_bundle(&plugins, "Crashes On Load", b"MZ");
    fake_bundle(&plugins, "Not A Dll", b"this is not a library");
    std::fs::create_dir_all(plugins.join("No Binary.vst3")).unwrap();
    let real = khs_gain().map(|src| {
        let dst = plugins.join("kHs Gain.vst3");
        std::fs::copy(src, &dst).unwrap();
        dst
    });

    let mut opts = options(&plugins, &root.join("plugins.json"));
    opts.extra_args = vec!["--crash-on".into(), "Crashes On Load".into()];
    let r = scan(&opts).expect("scan");
    let expected = 3 + real.is_some() as usize;
    assert_eq!(r.bundles, expected);
    assert_eq!(r.probed, expected);
    assert_eq!(r.failed, 3, "{:?}", r.catalog.failures);
    let kind_of = |c: &Catalog, name: &str| {
        c.failures
            .iter()
            .find(|f| f.path.to_string_lossy().contains(name))
            .map(|f| f.kind)
    };
    assert_eq!(
        kind_of(&r.catalog, "Crashes On Load"),
        Some(FailureKind::Crashed)
    );
    assert_eq!(kind_of(&r.catalog, "Not A Dll"), Some(FailureKind::Failed));
    assert_eq!(kind_of(&r.catalog, "No Binary"), Some(FailureKind::Failed));
    if let Some(real) = real {
        let gain = r
            .catalog
            .entries
            .iter()
            .find(|e| e.path == real)
            .expect("kHs Gain is listed");
        assert_eq!(gain.kind, PluginKind::Effect);
        assert_eq!(gain.format, "vst3");
        assert_eq!(gain.uid.len(), 32);
        assert!(gain.vendor.contains("Kilohearts"), "{gain:?}");
        assert_eq!(r.catalog.search("gain", None).len(), 1);
        assert!(
            r.catalog
                .search("", Some(PluginKind::Instrument))
                .is_empty()
        );
    }
    // What the scan wrote is what the browser reads.
    assert_eq!(Catalog::load(&opts.cache), r.catalog);

    // A bundle that hangs is given up on after the timeout, and the scan goes on. Scanned
    // apart from the real plugin, with a shorter timeout that still leaves a busy machine time
    // to start the scan process for the bundle that is not a DLL.
    let hanging = root.join("Hanging");
    fake_bundle(&hanging, "Hangs On Load", b"MZ");
    fake_bundle(&hanging, "Not A Dll", b"this is not a library");
    let mut opts = options(&hanging, &root.join("hanging.json"));
    opts.timeout = Duration::from_secs(10);
    opts.extra_args = vec!["--hang-on".into(), "Hangs On Load".into()];
    let r = scan(&opts).expect("scan");
    assert_eq!((r.bundles, r.probed, r.failed), (2, 2, 2));
    assert_eq!(
        kind_of(&r.catalog, "Hangs On Load"),
        Some(FailureKind::Timeout)
    );
    assert_eq!(kind_of(&r.catalog, "Not A Dll"), Some(FailureKind::Failed));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_retried_bundle_is_opened_again_and_the_rest_stay_cached() {
    let root_tmp = temp_dir("retry");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let a = fake_bundle(&plugins, "Alpha", b"not a dll 1");
    fake_bundle(&plugins, "Beta", b"not a dll 2");
    let mut opts = options(&plugins, &root.join("plugins.json"));
    let first = scan(&opts).unwrap();
    assert_eq!((first.cached, first.probed, first.failed), (0, 2, 2));

    // Nothing changed: a plain scan opens nothing, a retry opens the one asked for.
    let again = scan(&opts).unwrap();
    assert_eq!((again.cached, again.probed), (2, 0));
    opts.retry = vec![a];
    let retried = scan(&opts).unwrap();
    assert_eq!((retried.cached, retried.probed, retried.failed), (1, 1, 1));
    assert_eq!(retried.catalog.failures.len(), 2, "Beta keeps its record");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_second_scan_opens_only_the_bundles_that_changed() {
    let root_tmp = temp_dir("cache");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let a = fake_bundle(&plugins, "Alpha", b"not a dll 1");
    fake_bundle(&plugins, "Beta", b"not a dll 2");
    let c = fake_bundle(&plugins, "Gamma", b"not a dll 3");
    let mut opts = options(&plugins, &root.join("cache").join("plugins.json"));

    let first = scan(&opts).unwrap();
    assert_eq!(
        (first.bundles, first.cached, first.probed, first.failed),
        (3, 0, 3, 3)
    );

    // Unchanged: nothing is opened, failures included.
    let second = scan(&opts).unwrap();
    assert_eq!((second.cached, second.probed), (3, 0));
    assert_eq!(second.catalog, first.catalog);
    assert!(second.elapsed < first.elapsed, "{second:?} / {first:?}");

    // One binary changes and one bundle goes away.
    std::fs::write(
        a.join(r"Contents\x86_64-win\Alpha.vst3"),
        b"changed, longer",
    )
    .unwrap();
    std::fs::remove_dir_all(&c).unwrap();
    let third = scan(&opts).unwrap();
    assert_eq!(
        (third.bundles, third.cached, third.probed),
        (2, 1, 1),
        "{third:?}"
    );
    assert_eq!(third.catalog.failures.len(), 2);

    // A full scan opens everything again.
    opts.full = true;
    let full = scan(&opts).unwrap();
    assert_eq!((full.cached, full.probed), (0, 2));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_missing_scan_program_is_reported_not_cached() {
    let root_tmp = temp_dir("noprogram");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    fake_bundle(&plugins, "Alpha", b"x");
    let mut opts = options(&plugins, &root.join("plugins.json"));
    opts.program = None;
    let e = scan(&opts).unwrap_err();
    assert!(e.contains("xirius-plugin-scan"), "{e}");
    assert!(!opts.cache.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_stopped_scan_ends_the_open_process_at_once_and_keeps_what_it_opened() {
    let root_tmp = temp_dir("stop");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    fake_bundle(&plugins, "Alpha", b"not a dll 1");
    fake_bundle(&plugins, "Beta", b"not a dll 2");
    fake_bundle(&plugins, "Hangs On Load", b"MZ");
    let mut opts = options(&plugins, &root.join("plugins.json"));
    opts.timeout = Duration::from_secs(60);
    opts.extra_args = vec!["--hang-on".into(), "Hangs On Load".into()];

    // Stopped once the other two are done, while the hanging one is still open.
    let done = std::sync::atomic::AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let stopped_at = std::sync::Mutex::new(None);
    let start = Instant::now();
    let r = std::thread::scope(|s| {
        s.spawn(|| {
            while done.load(Ordering::Relaxed) < 2 && start.elapsed() < Duration::from_secs(30) {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(300));
            *stopped_at.lock().unwrap() = Some(Instant::now());
            stop.store(true, Ordering::Relaxed);
        });
        scan_until(&opts, &|n, _| done.store(n, Ordering::Relaxed), &|| {
            stop.load(Ordering::Relaxed)
        })
        .expect("scan")
    });
    // Long before the 60 s the hanging bundle had, however slow the machine made the others.
    let after_stop = stopped_at.lock().unwrap().expect("stopped").elapsed();
    assert!(after_stop < Duration::from_secs(10), "{after_stop:?}");
    assert!(r.stopped);
    assert_eq!((r.bundles, r.probed, r.failed), (3, 2, 2));
    assert!(
        !r.catalog
            .failures
            .iter()
            .any(|f| f.path.to_string_lossy().contains("Hangs")),
        "a stopped bundle is not a failure: {:?}",
        r.catalog.failures
    );
    assert_eq!(Catalog::load(&opts.cache), r.catalog);

    // The next scan opens only the one it did not reach.
    opts.timeout = Duration::from_secs(1);
    let next = scan(&opts).unwrap();
    assert_eq!((next.cached, next.probed, next.stopped), (2, 1, false));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_scan_whose_program_cannot_start_leaves_the_list_as_it_was() {
    let root_tmp = temp_dir("no-program");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    fake_bundle(&plugins, "Alpha", b"not a dll");
    fake_bundle(&plugins, "Beta", b"not a dll 2");
    let cache = root.join("plugins.json");
    let opts = options(&plugins, &cache);
    scan(&opts).expect("the first scan");
    let before = std::fs::read(&cache).unwrap();

    let mut broken = options(&plugins, &cache);
    broken.full = true;
    broken.program = Some(ChildProgram {
        program: root.join("no-such-scan-program.exe"),
        args: Vec::new(),
    });
    let e = scan(&broken).expect_err("a scan that cannot start says so");
    assert!(!e.contains('\n'), "one line: {e}");
    assert_eq!(std::fs::read(&cache).unwrap(), before);
}

#[test]
fn a_scan_in_which_nearly_every_bundle_fails_leaves_the_list_as_it_was() {
    let root_tmp = temp_dir("mostly-failed");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    for i in 0..9 {
        fake_bundle(&plugins, &format!("Bundle {i}"), b"not a dll");
    }
    let cache = root.join("plugins.json");
    // A list that was there (empty here; what matters is that the file is not rewritten).
    std::fs::write(&cache, br#"{"version":0,"bundles":[]}"#).unwrap();
    let before = std::fs::read(&cache).unwrap();

    let mut opts = options(&plugins, &cache);
    opts.extra_args = vec!["--crash-on".into(), "Bundle".into()];
    let e = scan(&opts).expect_err("a scan that fails nearly everywhere says so");
    assert!(!e.contains('\n'), "one line: {e}");
    assert_eq!(std::fs::read(&cache).unwrap(), before);
}
