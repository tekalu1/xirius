//! The plugin scan from the screen (PLG-021): `plugin.rescan --background` through the song's
//! session, polled by `Studio::tick`, with this crate's own program opening each bundle.
//!
//! The bundles are made here: one the child is told to crash on, one it is told to hang on,
//! and a DLL that is not one. When kHs Gain is installed, a copy of it is opened as well.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use xirius_core::model::PluginKind;
use xirius_plugins::{
    BundleRecord, CHILD_ARG, CacheFile, ChildProgram, Entry, ScanOptions, fingerprint,
};
use xirius_ui::browser::{BAD_TITLE, BRow, BrowserHit, RailTab};
use xirius_ui::mixer::MixerHit;
use xirius_ui::popup::Action;
use xirius_ui::song::Song;
use xirius_ui::studio::{DrawerTab, Hit, Studio};

/// A folder of the test's own, removed when dropped (also when the test fails).
fn temp_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-{name}-"))
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

/// The bundles of every test; the number of them, and whether kHs Gain is one.
fn bundles(plugins: &Path) -> (usize, bool) {
    fake_bundle(plugins, "Crashes On Load", b"MZ");
    fake_bundle(plugins, "Hangs On Load", b"MZ");
    fake_bundle(plugins, "Not A Dll", b"this is not a library");
    let src = xirius_vst3::default_vst3_dir().join(r"Kilohearts\kHs Gain.vst3");
    if src.is_file() {
        std::fs::copy(src, plugins.join("kHs Gain.vst3")).unwrap();
        (4, true)
    } else {
        eprintln!(
            "skipped the real plugin: {} is not installed",
            src.display()
        );
        (3, false)
    }
}

fn options(plugins: &Path, cache: &Path) -> ScanOptions {
    ScanOptions {
        dirs: vec![plugins.to_path_buf()],
        cache: cache.to_path_buf(),
        // The screen's own program answers `__scan-plugin`.
        program: Some(ChildProgram {
            program: env!("CARGO_BIN_EXE_xirius-ui").into(),
            args: vec![CHILD_ARG.to_string()],
        }),
        full: false,
        retry: Vec::new(),
        jobs: 4,
        timeout: BUNDLE_TIMEOUT,
        extra_args: vec![
            "--crash-on".into(),
            "Crashes On Load".into(),
            "--hang-on".into(),
            "Hangs On Load".into(),
        ],
    }
}

/// How long the scan gives one bundle. The hanging bundle holds the scan this long, so its
/// progress can be seen. On Windows only the child's own time counts, so the real plugin, opened
/// by a child kept waiting for a processor on a busy machine, is not taken for a hanging one
/// (ARC-037).
const BUNDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// What the frames saw while the scan ran.
#[derive(Default)]
struct Seen {
    /// Progress rows of the insert list, in order, without repeats.
    progress: Vec<String>,
    /// (done, total) while running.
    counts: Vec<(u64, u64)>,
    /// The longest frame (tick + insert list).
    slowest: Duration,
    frames: usize,
}

/// Only a guard against waiting forever; how soon it happens is not what is tested.
const GUARD: Duration = Duration::from_secs(60);

/// Runs frames until the scan ends (or the guard runs out).
fn frames_until_done(s: &mut Studio) -> Seen {
    let mut seen = Seen::default();
    let start = Instant::now();
    loop {
        let t = Instant::now();
        let more = s.tick();
        let list = s.insert_list();
        seen.slowest = seen.slowest.max(t.elapsed());
        seen.frames += 1;
        if s.mixer.scan.running() {
            assert!(more, "a running scan keeps the frames coming");
            seen.counts.push((s.mixer.scan.done, s.mixer.scan.total));
            let row = list[0].label.clone();
            assert!(list[0].to.is_empty(), "the progress row is not a choice");
            if seen.progress.last() != Some(&row) {
                seen.progress.push(row);
            }
        } else if seen.frames > 1 {
            return seen;
        }
        assert!(start.elapsed() < GUARD, "the scan never ended");
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn effect_names(s: &mut Studio) -> Vec<String> {
    s.insert_list()
        .into_iter()
        .filter(|c| {
            !c.to.is_empty()
                && !c.to.starts_with("builtin:")
                && c.to != xirius_ui::mixer::scan::RESCAN
        })
        .map(|c| c.label)
        .collect()
}

#[test]
fn with_no_list_the_window_scans_by_itself_and_a_crash_does_not_stop_it() {
    let root_tmp = temp_dir("autoscan");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let (n, real) = bundles(&plugins);
    let cache = root.join("cache").join("plugins.json");
    let mut s = Studio::new(Song::sample_with(options(&plugins, &cache)));
    let steps = s.song().steps();

    // What the real window does: no list kept yet, so a scan starts on the first frame.
    s.use_installed_plugins();
    assert!(!cache.exists());
    let seen = frames_until_done(&mut s);

    // The progress was there to see: the number of bundles, then some of them done.
    assert!(
        seen.counts.iter().any(|&(_, total)| total == n as u64),
        "{:?}",
        seen.counts
    );
    assert!(
        seen.counts
            .iter()
            .any(|&(done, total)| total == n as u64 && done < total),
        "{:?}",
        seen.counts
    );
    assert!(
        seen.progress.iter().any(|p| p.ends_with(&format!("/ {n}"))),
        "{:?}",
        seen.progress
    );
    // The screen did not wait for the scan: a frame that did would have held at least the
    // hanging bundle's time.
    assert!(seen.slowest < BUNDLE_TIMEOUT / 2, "{:?}", seen.slowest);
    eprintln!(
        "{} frames, slowest {:?}, progress {:?}",
        seen.frames, seen.slowest, seen.progress
    );

    // The crash, the hang and the broken DLL are failures; the scan still ended and kept a list.
    assert_eq!(s.mixer.scan.error, None);
    assert!(cache.exists());
    let catalog = s.mixer.catalog.clone().expect("the scan's list");
    assert_eq!(catalog.failures.len(), 3, "{:?}", catalog.failures);
    let names = effect_names(&mut s);
    if real {
        assert!(names.iter().any(|x| x.starts_with("kHs Gain")), "{names:?}");
        s.browser.press_rail(RailTab::Effects);
        assert!(s.browser.library().iter().any(|x| x.name == "kHs Gain"));
    }
    // Scanning is not an edit of the song.
    assert_eq!(s.song().steps(), steps);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn scanning_again_from_the_insert_list_fills_it_when_done() {
    let root_tmp = temp_dir("rescan");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let (_, real) = bundles(&plugins);
    let cache = root.join("plugins.json");
    let mut s = Studio::new(Song::sample_with(options(&plugins, &cache)));

    // Nothing scanned yet: the list says so and offers a scan.
    let list = s.insert_list();
    assert!(effect_names(&mut s).is_empty());
    assert_eq!(
        list.last().map(|c| c.to.as_str()),
        Some(xirius_ui::mixer::scan::RESCAN)
    );

    // A plugin installed since: kept in the list file the scan reads (it is not opened again).
    let comp = fake_bundle(&plugins, "Comp", b"MZ");
    let record = BundleRecord {
        path: comp.clone(),
        fingerprint: fingerprint(&comp),
        entries: vec![Entry {
            uid: "0123456789ABCDEF0123456789ABCDEF".into(),
            name: "Comp".into(),
            vendor: "Test Audio".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            category: "Fx|Dynamics".into(),
            version: "1.0".into(),
            path: comp,
            ara: false,
        }],
        failure: None,
        probe_ms: 1,
    };
    CacheFile {
        version: 2,
        bundles: vec![record],
    }
    .write(&cache)
    .unwrap();
    assert!(
        effect_names(&mut s).is_empty(),
        "read once, until a scan ends"
    );

    s.start_plugin_scan().unwrap();
    assert!(s.mixer.scan.running());
    // A second press while it runs does not start another.
    s.start_plugin_scan().unwrap();
    let seen = frames_until_done(&mut s);
    assert!(!seen.progress.is_empty());

    let names = effect_names(&mut s);
    assert!(
        names.contains(&"Comp — Test Audio".to_string()),
        "{names:?}"
    );
    assert_eq!(
        real,
        names.iter().any(|x| x.starts_with("kHs Gain")),
        "{names:?} {:?}",
        s.mixer.catalog.as_ref().map(|c| &c.failures)
    );
    s.browser.press_rail(RailTab::Effects);
    let lib = s.browser.library();
    let comp = lib
        .iter()
        .find(|x| x.name == "Comp")
        .expect("Comp in the browser");
    assert_eq!((comp.vendor, comp.kind), ("Test Audio", "ダイナミクス"));
    // The mock's sample plugins are gone; the built-in effects stay.
    assert!(!lib.iter().any(|x| x.name == "TDR Nova"));
    assert!(lib.iter().any(|x| x.name == "Compressor"));
    let _ = std::fs::remove_dir_all(&root);
}

// ---- stopping a scan (PLG-021) ------------------------------------------------------------

/// A bundle that opens at once and one that hangs for the whole 20 s it is given.
fn one_quick_one_hanging(plugins: &Path, cache: &Path) -> ScanOptions {
    fake_bundle(plugins, "Not A Dll", b"this is not a library");
    fake_bundle(plugins, "Hangs On Load", b"MZ");
    let mut opts = options(plugins, cache);
    opts.timeout = HANG;
    opts
}

/// How long the hanging bundle would hold a scan that is not stopped: far longer than the
/// guard, so a stop that waited for it fails the test.
const HANG: Duration = Duration::from_secs(600);

/// Frames until the quick bundle is done and the hanging one is still open.
fn frames_until_one_is_left(s: &mut Studio) {
    let start = Instant::now();
    while !(s.mixer.scan.total == 2 && s.mixer.scan.done == 1) {
        s.tick();
        assert!(
            s.mixer.scan.running(),
            "ended before it could be stopped: {:?} {:?}",
            s.mixer.scan.error,
            s.mixer.catalog.as_ref().map(|c| &c.failures)
        );
        assert!(
            start.elapsed() < GUARD,
            "{} / {}",
            s.mixer.scan.done,
            s.mixer.scan.total
        );
        std::thread::sleep(Duration::from_millis(16));
    }
}

#[test]
fn the_insert_list_stops_a_scan_at_once_and_keeps_what_it_opened() {
    let root_tmp = temp_dir("stop-insert");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let cache = root.join("plugins.json");
    let mut s = Studio::new(Song::sample_with(one_quick_one_hanging(&plugins, &cache)));
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(1600.0, 1000.0);
    let t = *s.project().tracks.keys().next().unwrap();

    s.start_plugin_scan().unwrap();
    frames_until_one_is_left(&mut s);

    // The list shows how far it is, and under it the stop.
    assert!(s.press(&g, Hit::Mixer(MixerHit::AddInsert(t))));
    let popup = s.popup.as_ref().expect("the insert list");
    assert_eq!(popup.choices[0].label, "スキャン中 1 / 2");
    assert_eq!(popup.choices[1].label, "スキャンを中止");
    let stop = popup
        .layout(g.size)
        .cells
        .iter()
        .position(|c| c.action == Some(Action::Pick(xirius_ui::mixer::scan::STOP.into())))
        .expect("the stop is a choice");
    let pressed = Instant::now();
    s.press(&g, Hit::Item(stop));

    // The list stays open and says it is stopping, with nothing more to press.
    let popup = s.popup.as_ref().expect("still open");
    assert_eq!(popup.choices[0].label, "中止しています…");
    assert!(
        !popup
            .choices
            .iter()
            .any(|c| c.to == xirius_ui::mixer::scan::STOP)
    );
    assert!(s.browser.scan_stopping);
    while s.mixer.scan.running() {
        s.tick();
        assert!(pressed.elapsed() < GUARD, "never stopped");
        std::thread::sleep(Duration::from_millis(16));
    }
    // Long before the hanging bundle's time (the loop above stops at the guard).
    eprintln!("stopped {:?} after the press", pressed.elapsed());
    // Not a failure: no error line, the scan can start again, and the list kept the bundle it
    // opened (a failure here) but not the one it was stopped on.
    assert_eq!(s.mixer.scan.error, None);
    assert!(!s.browser.scanning && !s.browser.scan_stopping);
    let list = s.insert_list();
    assert_eq!(
        list.last().map(|c| c.to.as_str()),
        Some(xirius_ui::mixer::scan::RESCAN)
    );
    let catalog = s.mixer.catalog.clone().unwrap();
    let failed: Vec<String> = catalog
        .failures
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(failed, ["Not A Dll.vst3"]);
    // The list file too: the next scan opens only the bundle it did not reach.
    let kept = CacheFile::read(&cache);
    assert_eq!(kept.bundles.len(), 1);
    assert!(kept.get(&plugins.join("Hangs On Load.vst3")).is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_instrument_list_shows_and_stops_the_scan_like_the_insert_list() {
    let root_tmp = temp_dir("stop-instrument");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let cache = root.join("plugins.json");
    let mut s = Studio::new(Song::sample_with(one_quick_one_hanging(&plugins, &cache)));
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(1600.0, 1000.0);
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .map(|t| t.id)
        .expect("an instrument track without an instrument");

    // Not scanned yet: the list ends with the scan.
    assert!(s.press(&g, Hit::Mixer(MixerHit::AddInstrument(t))));
    let popup = s.popup.as_ref().expect("the instrument list");
    assert_eq!(
        popup.choices.last().map(|c| c.to.as_str()),
        Some(xirius_ui::mixer::scan::RESCAN)
    );
    s.popup = None;

    s.start_plugin_scan().unwrap();
    frames_until_one_is_left(&mut s);
    assert!(s.press(&g, Hit::Mixer(MixerHit::AddInstrument(t))));
    let popup = s.popup.as_ref().expect("the instrument list");
    assert_eq!(popup.choices[0].label, "スキャン中 1 / 2");
    assert!(popup.choices[0].to.is_empty());
    assert_eq!(popup.choices[1].to, xirius_ui::mixer::scan::STOP);
    let stop = popup
        .layout(g.size)
        .cells
        .iter()
        .position(|c| c.action == Some(Action::Pick(xirius_ui::mixer::scan::STOP.into())))
        .expect("the stop is a choice");
    s.press(&g, Hit::Item(stop));

    // It stays open, says it is stopping, and ends on a later frame.
    let popup = s.popup.as_ref().expect("still open");
    assert_eq!(popup.choices[0].label, "中止しています…");
    let pressed = Instant::now();
    while s.mixer.scan.running() {
        s.tick();
        assert!(pressed.elapsed() < GUARD, "never stopped");
        std::thread::sleep(Duration::from_millis(16));
    }
    assert_eq!(s.mixer.scan.error, None);
    let popup = s.popup.as_ref().expect("the list is still there");
    assert_eq!(
        popup.choices.last().map(|c| c.to.as_str()),
        Some(xirius_ui::mixer::scan::RESCAN)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The place of the first unusable plugin's row in the browser's list.
fn bad_row(s: &Studio, g: &xirius_ui::studio::Geometry) -> Option<usize> {
    s.browser
        .layout(g.panel)
        .rows
        .iter()
        .position(|p| matches!(p.row, BRow::Bad { .. }))
}

#[test]
fn the_browser_lists_what_could_not_be_used_and_tries_one_again() {
    let root_tmp = temp_dir("bad-group");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let (n, _) = bundles(&plugins);
    let cache = root.join("plugins.json");
    let mut s = Studio::new(Song::sample_with(options(&plugins, &cache)));
    s.browser.press_rail(RailTab::Effects);
    let g = s.geometry(1600.0, 1000.0);

    // While the first scan runs the group is not there (the list is being made).
    s.start_plugin_scan().unwrap();
    assert!(s.browser.scanning);
    assert!(bad_row(&s, &g).is_none());
    frames_until_done(&mut s);

    // Three bundles failed: the group is at the end, with a reason for each.
    assert_eq!(
        s.browser.bad.len(),
        3,
        "{:?}",
        s.mixer.catalog.as_ref().map(|c| &c.failures)
    );
    let rows = s.browser.rows();
    let at = rows
        .iter()
        .position(|r| matches!(r, BRow::Group { name, .. } if *name == BAD_TITLE))
        .expect("the group of unusable plugins");
    assert_eq!(rows.len() - at, 1 + 3);
    let whys: Vec<&str> = s.browser.bad.iter().map(|b| b.why).collect();
    assert!(whys.contains(&"応答がありません"), "{whys:?}");
    assert!(whys.contains(&"スキャン中に終了しました"), "{whys:?}");
    assert!(whys.contains(&"読み込めませんでした"), "{whys:?}");
    let _ = n;

    // A search leaves it out; clearing the search brings it back.
    s.browser.search = "kHs".into();
    assert!(
        s.browser
            .rows()
            .iter()
            .all(|r| !matches!(r, BRow::Bad { .. }))
    );
    s.browser.search.clear();

    // Pressing a row's button scans that bundle again, and only that: the rest stays cached.
    let i = bad_row(&s, &g).expect("a row on the list");
    let center = s
        .center_of(&g, Hit::Browser(BrowserHit::Retry(i)))
        .expect("the retry button is on its row");
    assert_eq!(
        s.hit(&g, center.0, center.1),
        Hit::Browser(BrowserHit::Retry(i))
    );
    let path = match &s.browser.layout(g.panel).rows[i].row {
        BRow::Bad { path, .. } => path.clone(),
        _ => unreachable!(),
    };
    assert!(s.press(&g, Hit::Browser(BrowserHit::Retry(i))));
    assert!(s.mixer.scan.running());
    assert_eq!(s.browser.retrying.as_ref(), Some(&path));
    // The group stays while one is retried, with that row marked.
    assert!(
        s.browser
            .rows()
            .iter()
            .any(|r| matches!(r, BRow::Bad { retrying: true, .. }))
    );
    // Its button is a spinner meanwhile, and a press starts nothing more.
    assert_eq!(s.center_of(&g, Hit::Browser(BrowserHit::Retry(i))), None);
    s.press(&g, Hit::Browser(BrowserHit::Retry(i)));
    assert_eq!(s.browser.retrying.as_ref(), Some(&path));
    frames_until_done(&mut s);
    assert_eq!(s.mixer.scan.error, None);
    assert_eq!(s.browser.retrying, None);
    // It failed the same way, so it is still on the list.
    assert_eq!(s.browser.bad.len(), 3);
    assert!(s.browser.bad.iter().any(|b| b.path == path));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_browser_row_stops_a_scan_too() {
    let root_tmp = temp_dir("stop-browser");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let cache = root.join("plugins.json");
    let mut s = Studio::new(Song::sample_with(one_quick_one_hanging(&plugins, &cache)));
    s.browser.press_rail(RailTab::Effects);
    let g = s.geometry(1600.0, 1000.0);

    s.start_plugin_scan().unwrap();
    frames_until_one_is_left(&mut s);
    let at = s
        .center_of(&g, Hit::Browser(BrowserHit::StopScan))
        .expect("the stop button is on the scanning row");
    assert_eq!(s.hit(&g, at.0, at.1), Hit::Browser(BrowserHit::StopScan));
    let pressed = Instant::now();
    assert!(s.press(&g, Hit::Browser(BrowserHit::StopScan)));
    // The row says it is stopping and its button is gone.
    assert_eq!(s.browser.scanning_text(), "中止しています…");
    assert_eq!(s.center_of(&g, Hit::Browser(BrowserHit::StopScan)), None);
    while s.mixer.scan.running() {
        s.tick();
        assert!(pressed.elapsed() < GUARD, "never stopped");
        std::thread::sleep(Duration::from_millis(16));
    }
    assert_eq!(s.mixer.scan.error, None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_settings_plugin_page_scans_again_lists_the_result_and_tries_one_bundle_again() {
    use xirius_ui::settings::{Do, Section};
    let root_tmp = temp_dir("settings-page");
    let root = root_tmp.path().to_path_buf();
    let plugins = root.join("VST3");
    let (_, _) = bundles(&plugins);
    let cache = root.join("plugins.json");
    let mut s = Studio::new(Song::sample_with(options(&plugins, &cache)));
    let g = s.geometry(1600.0, 1000.0);
    s.open_settings(Section::Plugins);
    let at = xirius_ui::studio::Rect::new(0.0, 0.0, 0.0, 0.0);

    // Nothing scanned yet: the page lists nothing (and none of the old mock's plugins).
    let scene = s.settings_scene(&g);
    assert!(scene.has_text("なし"));
    assert!(!scene.has_text("Pro-Q 4"));

    // 再スキャン runs `plugin.rescan` and the page shows the scan while it runs.
    assert!(scene.find(&Do::Rescan).is_some());
    s.settings_do(&g, Do::Rescan, at);
    assert!(s.mixer.scan.running());
    assert!(s.settings_scene(&g).has_text("スキャン中") || s.settings_scene(&g).has_text("検索中"));
    assert!(s.settings_scene(&g).find(&Do::StopScan).is_some());
    frames_until_done(&mut s);

    // The bundles that could not be opened are on the page, each with its reason.
    let scene = s.settings_scene(&g);
    for name in ["Crashes On Load", "Hangs On Load", "Not A Dll"] {
        assert!(scene.has_text(name), "{name} is on the page");
    }
    for why in [
        "応答がありません",
        "スキャン中に終了しました",
        "読み込めませんでした",
    ] {
        assert!(
            scene.has_text(why),
            "{why} is on the page: {:?}",
            s.mixer.catalog.as_ref().map(|c| &c.failures)
        );
    }
    assert!(scene.has_text("読み込めない（3）"));
    assert!(scene.find(&Do::Rescan).is_some(), "the scan is over");
    assert!(scene.find(&Do::StopScan).is_none());

    // One row's button opens that bundle again and only that one.
    let path = s.browser.bad[0].path.clone();
    s.settings_do(&g, Do::Retry(path.clone()), at);
    assert!(s.mixer.scan.running());
    assert_eq!(s.browser.retrying.as_ref(), Some(&path));
    frames_until_done(&mut s);
    assert_eq!(s.browser.retrying, None);
    assert_eq!(s.mixer.scan.error, None);
    assert_eq!(s.browser.bad.len(), 3);
    let _ = std::fs::remove_dir_all(&root);
}
