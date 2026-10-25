//! The file browser on folders made for each test (BRW-002, BRW-005, BRW-006, BRW-008,
//! BRW-012, BRW-015).

use std::path::PathBuf;

use xirius_design::Layout;

use super::*;
use crate::browser::{BRow, Browser, BrowserHit, BrowserLayout, RailTab};
use crate::studio::Rect;

/// A folder of the test's own, removed when dropped (also when the test fails).
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-files-{name}-"))
        .tempdir()
        .unwrap()
}

/// A file of what the browser keeps, as a first run without a home folder writes it: a
/// missing file would start from the user's real Downloads and Music.
fn fresh(root: &Path) -> PathBuf {
    let file = root.join("browser.json");
    places::save(&file, &places::Prefs::first_run(None)).unwrap();
    file
}

/// A browser on the place `top` (its second tab), keeping nothing.
fn on(top: &Path) -> Browser {
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    b.add_place_fixture(top).unwrap();
    settle_rows(&b);
    b
}

fn layout(b: &Browser) -> BrowserLayout {
    settle_rows(b);
    b.layout(Rect::new(52.0, 6.0, Layout::BROWSER_W, 470.0))
}

fn names(b: &Browser) -> Vec<String> {
    settle_rows(b);
    b.rows()
        .iter()
        .map(|r| match r {
            BRow::Dir { name, depth, .. } => format!("{}{name}/", "  ".repeat(*depth)),
            BRow::File { name, depth, .. } => format!("{}{name}", "  ".repeat(*depth)),
            BRow::Audition { .. } => "<audition>".into(),
            BRow::Note(n) => (*n).into(),
            _ => "?".into(),
        })
        .collect()
}

fn key(b: &Browser, name: &str) -> String {
    settle_rows(b);
    b.rows()
        .into_iter()
        .find_map(|r| match r {
            BRow::Dir { key, name: n, .. } | BRow::File { key, name: n, .. } if n == name => {
                Some(key)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no row {name}"))
}

#[test]
fn blocked_navigation_and_wide_search_are_bounded_and_revisits_coalesce() {
    let _serial = crate::browser_io_tests::IO_GATES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = folder("bounded");
    let a = tmp.path().join("a");
    let bpath = tmp.path().join("b");
    let wide = tmp.path().join("wide");
    for p in [&a, &bpath, &wide] {
        std::fs::create_dir(p).unwrap();
    }
    for i in 0..600 {
        std::fs::create_dir(wide.join(format!("child{i}"))).unwrap();
    }
    let mut b = on(&wide);
    b.add_place_fixture(&a).unwrap();
    b.add_place_fixture(&bpath).unwrap();
    b.set_location(1);
    settle_rows(&b);
    let release = Release::new();
    let gate = release.0.clone();
    let root = tmp.path().to_path_buf();
    let (tx, entered) = channel();
    let observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "browser_read_dir" && path.starts_with(&root) && path != root.join("wide") {
            let _ = tx.send(path.to_path_buf());
            let guard = gate.0.lock().unwrap();
            drop(gate.1.wait_while(guard, |done| !*done).unwrap());
        }
        Ok(())
    }));
    b.set_location(2);
    b.rows();
    entered.recv_timeout(Duration::from_secs(15)).unwrap();
    let first = LISTS.with(|l| l.borrow().dirs[&a].pending.unwrap());
    for _ in 0..40 {
        b.set_location(3);
        b.rows();
        b.set_location(2);
        b.rows();
        assert_eq!(LISTS.with(|l| l.borrow().dirs[&a].pending), Some(first));
        let (running, queued) = LISTS.with(|l| l.borrow().jobs.counts());
        assert!(running <= xirius_session::read_jobs::THREADS);
        assert!(queued <= xirius_session::read_jobs::QUEUED);
    }
    b.set_location(1);
    b.rows(); // Only two readers are stopped: the wide root can still arrive.
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        poll();
        b.rows();
        if LISTS.with(|l| !l.borrow().dirs[&wide].entries.is_empty()) {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    b.search = "hit".into();
    b.rows();
    let (running, queued) = LISTS.with(|l| l.borrow().jobs.counts());
    assert!(running <= 4);
    assert!(queued <= 128);
    // An explicit queued bound is observable before threads get scheduled, unlike
    // checking a channel immediately after submitting work.
    assert!(running + queued <= 4 + 128);
    b.search.clear();
    b.rows();
    let (_, queued) = LISTS.with(|l| l.borrow().jobs.counts());
    assert_eq!(queued, 0);
    release.release();
    forget();
    while LISTS.with(|l| l.borrow().jobs.pending()) {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    drop(observe);
}

#[test]
fn missing_and_unlistable_folders_keep_the_main_branch_messages() {
    for (operation, error, expected) in [
        ("browser_top", std::io::ErrorKind::NotFound, GONE),
        (
            "browser_read_dir",
            std::io::ErrorKind::PermissionDenied,
            EMPTY,
        ),
    ] {
        let tmp = folder("errors");
        let path = tmp.path().to_path_buf();
        let mut b = Browser::new();
        b.press_rail(RailTab::Files);
        b.add_place_fixture(&path).unwrap();
        let target = path.clone();
        let observe = xirius_core::io_test::observe(Arc::new(move |op, p| {
            if op == operation && p == target {
                Err(error.into())
            } else {
                Ok(())
            }
        }));
        assert_eq!(names(&b), [expected]);
        drop(observe);
        forget();
    }
}

#[test]
fn a_failed_type_hint_falls_back_on_the_worker_like_the_main_branch() {
    let tmp = folder("type-error");
    let root = tmp.path().to_path_buf();
    std::fs::create_dir(root.join("folder")).unwrap();
    std::fs::write(root.join("audio.wav"), b"").unwrap();
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    b.add_place_fixture(&root).unwrap();
    let path = root.clone();
    let ui = std::thread::current().id();
    let (tx, trace) = channel();
    let observation = xirius_core::io_test::observe(Arc::new(move |op, p| {
        if p.starts_with(&path) && op == "browser_file_type" {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        if p.starts_with(&path) && op == "browser_type_fallback" {
            assert_ne!(std::thread::current().id(), ui);
            let _ = tx.send(());
        }
        Ok(())
    }));
    assert_eq!(names(&b), ["folder/", "audio.wav"]);
    assert_eq!(trace.try_iter().count(), 2);
    drop(observation);
    forget();
}

#[test]
fn directory_and_file_links_match_the_main_branch_listing() {
    let tmp = folder("links");
    let root = tmp.path();
    let target = root.join("target");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("hit.wav"), b"").unwrap();
    std::fs::write(root.join("file.wav"), b"").unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Junctions require no elevation and exercise the Windows mount-point tag.
        let result = std::process::Command::new("cmd")
            .creation_flags(0x08000000)
            .args(["/c", "mklink", "/J"])
            .arg(root.join("junction.wav"))
            .arg(&target)
            .output()
            .unwrap();
        assert!(result.status.success());
        let missing = root.join("removed-target");
        std::fs::create_dir(&missing).unwrap();
        let result = std::process::Command::new("cmd")
            .creation_flags(0x08000000)
            .args(["/c", "mklink", "/J"])
            .arg(root.join("broken-junction.wav"))
            .arg(&missing)
            .output()
            .unwrap();
        assert!(result.status.success());
        std::fs::remove_dir(&missing).unwrap();
        for (path, to, dir) in [
            ("directory.wav", target.clone(), true),
            ("linked.wav", root.join("file.wav"), false),
            ("broken.wav", root.join("absent.wav"), false),
            ("broken-dir", root.join("absent"), true),
        ] {
            let result = if dir {
                std::os::windows::fs::symlink_dir(&to, root.join(path))
            } else {
                std::os::windows::fs::symlink_file(&to, root.join(path))
            };
            if let Err(e) = result {
                assert!(
                    e.kind() == std::io::ErrorKind::PermissionDenied
                        || e.raw_os_error() == Some(1314),
                    "{e}"
                );
                eprintln!("symbolic-link fixture unavailable: {e}");
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&target, root.join("directory.wav")).unwrap();
        symlink(root.join("file.wav"), root.join("linked.wav")).unwrap();
        symlink(root.join("absent.wav"), root.join("broken.wav")).unwrap();
        symlink(root.join("absent"), root.join("broken-dir")).unwrap();
    }
    let mut old: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let dir = path.is_dir();
            (dir || takes(&path)).then_some((e.file_name().to_string_lossy().into_owned(), dir))
        })
        .collect();
    old.sort();
    let b = on(root);
    let mut now: Vec<_> = list(root).iter().map(|e| (e.name.clone(), e.dir)).collect();
    now.sort();
    assert_eq!(now, old);
    let mut b = b;
    b.search = "hit".into();
    assert_eq!(
        names(&b).iter().filter(|n| n.as_str() == "hit.wav").count(),
        old.iter().filter(|(_, dir)| *dir).count()
    );
    forget();
}

struct Release(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl Release {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(false), std::sync::Condvar::new())))
    }
    fn release(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.release();
    }
}

#[test]
fn listing_and_clip_drawing_inputs_return_while_place_io_is_held() {
    let _serial = crate::browser_io_tests::IO_GATES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    use crate::wave::{self, Look};
    use xirius_session::peaks;

    // These gates stay closed until the UI has prepared repeated frames. There is
    // no elapsed-time assertion: even indefinitely stalled I/O must not block it.
    for held in [
        "browser_read_dir",
        "browser_file_type",
        "peaks_source",
        "peaks_load",
    ] {
        let tmp = folder("io-gate");
        let root = tmp.path().to_path_buf();
        let wav = root.join("clip.wav");
        let store = root.join("cache");
        xirius_session::export::write_wav(&wav, &[0.5; 256], &[0.5; 256], 48_000, 16).unwrap();
        // Also exercise the already-built peak file, which used to be loaded in look.
        peaks::ensure(&wav, &store).unwrap();
        let b = on(&root);
        forget();
        wave::forget();
        let ui = std::thread::current().id();
        let release = Release::new();
        let gate = release.0.clone();
        let (tx, entered) = channel();
        let (trace_tx, trace) = channel();
        let observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if path.starts_with(&root) && (op.starts_with("browser_") || op.starts_with("peaks_")) {
                let thread = std::thread::current().id();
                assert_ne!(thread, ui, "{op} must not run on the UI thread");
                let _ = trace_tx.send((op.to_string(), thread));
                if op == held {
                    let _ = tx.send(());
                    let guard = gate.0.lock().unwrap();
                    drop(gate.1.wait_while(guard, |done| !*done).unwrap());
                }
            }
            Ok(())
        }));
        b.rows();
        assert!(matches!(wave::look(&wav, &store), Look::Making));
        entered.recv_timeout(Duration::from_secs(10)).expect(held);
        for _ in 0..8 {
            poll();
            wave::poll();
            // The same list/layout and peak lookup used by the renderer, without
            // opening a window or touching the desktop or audio output.
            b.layout(Rect::new(52.0, 6.0, Layout::BROWSER_W, 470.0));
            let look = wave::look(&wav, &store);
            if held.starts_with("peaks_") {
                assert!(matches!(look, Look::Making));
            }
        }
        release.release();
        settle_rows(&b);
        peaks::wait();
        wave::poll();
        assert!(matches!(wave::look(&wav, &store), Look::Ready(_)));
        let ops: Vec<_> = trace
            .try_iter()
            .map(|(op, thread)| {
                assert_ne!(thread, ui);
                op
            })
            .collect();
        assert!(ops.iter().any(|op| op == held));
        assert!(names(&b).iter().any(|n| n == "clip.wav"));
        drop(observe);
        wave::forget();
        forget();
    }
}

#[test]
fn refresh_keeps_the_old_rows_and_navigation_discards_the_old_receiver() {
    let _serial = crate::browser_io_tests::IO_GATES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = folder("refresh");
    let old = tmp.path().join("old");
    let new = tmp.path().join("new");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(old.join("old.wav"), b"").unwrap();
    std::fs::write(new.join("new.mid"), b"").unwrap();
    let mut b = on(&old);
    b.add_place_fixture(&new).unwrap();
    b.set_location(1);
    settle_rows(&b);
    let release = Release::new();
    let gate = release.0.clone();
    let path = old.clone();
    let ui = std::thread::current().id();
    let (tx, entered) = channel();
    let observe = xirius_core::io_test::observe(Arc::new(move |op, p| {
        if op == "browser_read_dir" && p == path {
            assert_ne!(std::thread::current().id(), ui);
            let _ = tx.send(());
            let guard = gate.0.lock().unwrap();
            drop(gate.1.wait_while(guard, |done| !*done).unwrap());
        }
        Ok(())
    }));
    LISTS.with(|l| l.borrow_mut().dirs.get_mut(&old).unwrap().at = Instant::now() - FRESH);
    assert!(
        b.rows()
            .iter()
            .any(|r| matches!(r, BRow::File { name, .. } if name == "old.wav"))
    );
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    // Repeated paints do not queue additional refreshes.
    for _ in 0..8 {
        b.rows();
    }
    assert!(entered.try_recv().is_err());
    b.set_location(2);
    settle_rows(&b);
    assert_eq!(names(&b), ["new.mid"]);
    assert!(LISTS.with(|l| !l.borrow().dirs.contains_key(&old)));
    release.release();
    let until = Instant::now() + Duration::from_secs(10);
    while LISTS.with(|l| l.borrow().jobs.pending()) {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    drop(observe);
    // A result arriving after navigation has no receiver and cannot repopulate
    // even the cache for the old directory.
    poll();
    assert_eq!(names(&b), ["new.mid"]);
    forget();
}

#[test]
fn a_real_folder_lists_its_folders_first_and_only_sound_and_midi_files() {
    let top_tmp = folder("list");
    let top = top_tmp.path().join("素材");
    crate::import_shots::make_place(&top);
    let b = on(&top);
    assert_eq!(b.location().name, "素材");
    // notes.txt and .cache are left out.
    assert_eq!(
        names(&b),
        ["Drums/", "Loops/", "chords.mid", "idea.mid", "pad_44k.wav"]
    );
}

#[test]
fn the_tree_opens_a_folder_under_itself_and_the_crumbs_go_back() {
    let top_tmp = folder("tree");
    let top = top_tmp.path().join("素材");
    crate::import_shots::make_place(&top);
    let mut b = on(&top);
    let l = layout(&b);
    b.press(&l, BrowserHit::Dir(0));
    assert_eq!(
        names(&b)[..6],
        [
            "Drums/",
            "  hat_closed.wav",
            "  kick_01.wav",
            "  kick_02.wav",
            "  snare_01.wav",
            "Loops/"
        ]
    );
    let l = layout(&b);
    b.press(&l, BrowserHit::Dir(0));
    assert_eq!(names(&b)[1], "Loops/");

    b.enter_dir(&key(&b, "Loops"));
    assert_eq!(b.path, ["Loops"]);
    let l = layout(&b);
    assert_eq!(
        l.crumbs.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["素材", "Loops"]
    );
    assert_eq!(names(&b), ["loop_120bpm.wav", "loop_96bpm.wav"]);
    b.press(&l, BrowserHit::Crumb(0));
    assert!(b.path.is_empty());
    assert_eq!(names(&b)[0], "Drums/");
}

#[test]
fn the_search_finds_files_inside_closed_folders_of_the_place_only() {
    let root_tmp = folder("search");
    let root = root_tmp.path().to_path_buf();
    let top = root.join("素材");
    crate::import_shots::make_place(&top);
    // Outside the place: not found.
    std::fs::write(root.join("kick_outside.wav"), b"").unwrap();
    let mut b = on(&top);
    b.search = "KICK".into();
    assert_eq!(names(&b), ["kick_01.wav", "kick_02.wav"]);
    b.search = "zzz".into();
    assert_eq!(names(&b), ["見つかりません"]);
}

#[test]
fn rows_show_how_long_each_file_is_once_read() {
    let top_tmp = folder("length");
    let top = top_tmp.path().join("素材");
    crate::import_shots::make_place(&top);
    let b = on(&top);
    let _ = b.rows();
    settle(Duration::from_secs(10));
    let lens: Vec<(String, String)> = b
        .rows()
        .into_iter()
        .filter_map(|r| match r {
            BRow::File { name, len, .. } => Some((name, len)),
            _ => None,
        })
        .collect();
    assert_eq!(
        lens,
        [
            ("chords.mid".to_string(), "4 小節".to_string()),
            ("idea.mid".to_string(), "2 小節".to_string()),
            ("pad_44k.wav".to_string(), "0:06".to_string()),
        ]
    );
    assert_eq!(clock(83.4), "1:23");
    assert_eq!(clock(0.2), "0.2 秒");
    assert_eq!(clock(0.0), "0.1 秒");
    assert_eq!(clock(0.96), "0:01");
    assert_eq!(clock(3725.0), "1:02:05");
}

#[test]
fn a_slow_place_guesses_the_lengths_off_screen_and_reads_those_on_it() {
    let tmp = folder("slow");
    let top = tmp.path().join("遅い");
    std::fs::create_dir_all(&top).unwrap();
    for (i, n) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        std::fs::write(top.join(format!("{n}.wav")), vec![0u8; 1_000_000 * (i + 1)]).unwrap();
    }
    // Each file takes longer to read than a slow place's; it is 10 s a megabyte.
    let read = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let r = read.clone();
    measure_with(Measure {
        length: Box::new(move |p| {
            std::thread::sleep(SLOW + Duration::from_millis(50));
            r.lock().unwrap().push(p.to_path_buf());
            let secs = std::fs::metadata(p).unwrap().len() as f64 / 100_000.0;
            (clock(secs), Some(secs))
        }),
        size: Box::new(|p| std::fs::metadata(p).map(|m| m.len()).ok()),
    });
    let b = on(&top);
    let lens = |b: &Browser| -> Vec<String> {
        b.rows()
            .into_iter()
            .filter_map(|r| match r {
                BRow::File { len, .. } => Some(len),
                _ => None,
            })
            .collect()
    };
    let _ = lens(&b);
    settle(Duration::from_secs(10));
    // The first is read, and was slow: the rest are guessed at its bytes a second.
    assert_eq!(
        lens(&b),
        ["0:10", "約 0:20", "約 0:30", "約 0:40", "約 0:50"]
    );
    assert_eq!(read.lock().unwrap().len(), 1);
    // The row of d comes on screen, and is read.
    let d = top.join("d.wav");
    let started = Instant::now();
    while lens(&b)[3] != "0:40" {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            lens(&b)
        );
        seen([d.as_path()].into_iter());
        poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(lens(&b), ["0:10", "約 0:20", "約 0:30", "0:40", "約 0:50"]);
    assert_eq!(*read.lock().unwrap(), [top.join("a.wav"), d]);
}

#[test]
fn a_slow_place_guesses_each_kind_by_its_own_and_is_measured_again_once_quick() {
    let tmp = folder("slow-kinds");
    let top = tmp.path().join("遅い");
    std::fs::create_dir_all(&top).unwrap();
    for n in ["a.wav", "b.wav", "c.mp3", "d.flac"] {
        std::fs::write(top.join(n), vec![0u8; 2_000_000]).unwrap();
    }
    // Slow until told otherwise; a WAV is 10 s a megabyte, an MP3 and a FLAC have no measure.
    let slow = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let s = slow.clone();
    measure_with(Measure {
        length: Box::new(move |p| {
            if s.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(SLOW + Duration::from_millis(50));
            }
            let secs = std::fs::metadata(p).unwrap().len() as f64 / 100_000.0;
            (clock(secs), Some(secs))
        }),
        size: Box::new(|p| std::fs::metadata(p).map(|m| m.len()).ok()),
    });
    let b = on(&top);
    let lens = |b: &Browser| -> Vec<String> {
        b.rows()
            .into_iter()
            .filter_map(|r| match r {
                BRow::File { len, .. } => Some(len),
                _ => None,
            })
            .collect()
    };
    let _ = lens(&b);
    settle(Duration::from_secs(10));
    // The WAV's bytes a second say nothing of the compressed kinds: they wait for their rows.
    assert_eq!(lens(&b), ["0:20", "約 0:20", "", ""]);
    // The place has become quick (the files are cached): a row read now is quick, and the
    // files asked for after are read, not guessed.
    slow.store(false, std::sync::atomic::Ordering::Relaxed);
    let c = top.join("c.mp3");
    let started = Instant::now();
    while lens(&b)[2] != "0:20" {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            lens(&b)
        );
        seen([c.as_path()].into_iter());
        poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    let e = top.join("e.wav");
    std::fs::write(&e, vec![0u8; 3_000_000]).unwrap();
    let _ = length(&e);
    settle(Duration::from_secs(10));
    assert_eq!(length(&e), "0:30");
}

#[test]
fn a_row_off_screen_again_before_it_is_read_is_read_when_it_comes_back() {
    let tmp = folder("slow-back");
    let top = tmp.path().join("遅い");
    std::fs::create_dir_all(&top).unwrap();
    for n in ["a.wav", "b.wav", "c.wav"] {
        std::fs::write(top.join(n), vec![0u8; 2_000_000]).unwrap();
    }
    let (began, started_read) = std::sync::mpsc::channel();
    let (go, proceed) = std::sync::mpsc::channel();
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(go) = self.0.take() {
                let _ = go.send(());
            }
        }
    }
    let mut release = Release(Some(go));
    measure_with(Measure {
        length: Box::new(move |p| {
            let b = p.file_name().is_some_and(|n| n == "b.wav");
            if b {
                began.send(()).unwrap();
                proceed.recv_timeout(Duration::from_secs(20)).unwrap();
            }
            // This delay supplies the slow-device classification itself. Gate
            // notifications, rather than this duration, order b/c visibility.
            std::thread::sleep(SLOW + Duration::from_millis(50));
            let secs = std::fs::metadata(p).unwrap().len() as f64 / 100_000.0;
            (clock(secs), Some(secs))
        }),
        size: Box::new(|p| std::fs::metadata(p).map(|m| m.len()).ok()),
    });
    let b = on(&top);
    let lens = |b: &Browser| -> Vec<String> {
        b.rows()
            .into_iter()
            .filter_map(|r| match r {
                BRow::File { len, .. } => Some(len),
                _ => None,
            })
            .collect()
    };
    let _ = lens(&b);
    settle(Duration::from_secs(10));
    assert_eq!(lens(&b), ["0:20", "約 0:20", "約 0:20"]);
    let (rb, rc) = (top.join("b.wav"), top.join("c.wav"));
    // b comes on screen and is read; c comes on screen and goes off again while it waits.
    seen([rb.as_path()].into_iter());
    started_read.recv_timeout(Duration::from_secs(20)).unwrap();
    seen([rc.as_path()].into_iter());
    seen(std::iter::empty());
    release.0.take().unwrap().send(()).unwrap();
    let started = Instant::now();
    while lens(&b)[1] != "0:20" {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            lens(&b)
        );
        poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    settle(Duration::from_secs(10));
    while lengths(|l| l.again.contains(&rc)) {
        assert!(started.elapsed() < Duration::from_secs(20));
        poll();
        std::thread::yield_now();
    }
    assert!(lens(&b)[2].starts_with("約 "));
    // c came back as a guess; on screen again, it is read.
    while lens(&b)[2] != "0:20" {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            lens(&b)
        );
        seen([rc.as_path()].into_iter());
        poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_song_tab_waits_for_a_folder_and_hides_what_the_song_keeps() {
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    assert_eq!(names(&b), [NO_FOLDER]);
    let song_tmp = folder("song");
    let song = song_tmp.path().to_path_buf();
    for own in SONG_OWN {
        std::fs::create_dir_all(song.join(own)).unwrap();
    }
    b.set_song_dir(Some(song.clone()));
    assert_eq!(names(&b), [EMPTY]);
    crate::import_shots::make_place(&song.join("Audio"));
    forget();
    assert_eq!(names(&b), ["Audio/"]);
    // Deeper down the same names are shown.
    b.enter_dir(&key(&b, "Audio"));
    std::fs::create_dir_all(song.join("Audio/history")).unwrap();
    forget();
    assert_eq!(names(&b)[0], "Drums/");
    assert!(names(&b).contains(&"history/".to_string()));

    let _ = std::fs::remove_dir_all(&song);
    forget();
    assert_eq!(names(&b), [GONE]);
}

#[test]
fn a_place_is_added_shown_kept_and_taken_away_but_never_the_song() {
    let root_tmp = folder("places");
    let root = root_tmp.path().to_path_buf();
    let file = fresh(&root);
    let (a, c) = (root.join("Samples"), root.join("Loops"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&c).unwrap();
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    b.use_prefs_fixture(file.clone()).unwrap();
    assert_eq!(b.add_place_fixture(&a), Ok("Samples".into()));
    assert_eq!(b.add_place_fixture(&c), Ok("Loops".into()));
    assert_eq!(b.location().name, "Loops");
    assert!(b.add_place_fixture(&a).is_err(), "the same folder twice");

    b.settle_prefs_io();
    // Another browser (after a restart) has the tabs.
    let mut again = Browser::new();
    again.use_prefs_fixture(file.clone()).unwrap();
    let tabs: Vec<String> = again.locations().into_iter().map(|l| l.name).collect();
    assert_eq!(tabs, [places::SONG, "Samples", "Loops"]);

    // The cross sits on the shown tab only, and never on the song's.
    let l = layout(&b);
    assert!(l.remove.is_some());
    let (x, y) = b.center_of(&l, BrowserHit::LocRemove(2)).unwrap();
    assert_eq!(b.hit(&l, x, y), Some(BrowserHit::LocRemove(2)));
    b.press(&l, BrowserHit::LocRemove(2));
    assert_eq!(b.location().name, "Samples");
    assert!(c.is_dir(), "the folder itself stays");
    b.set_location(0);
    assert_eq!(layout(&b).remove, None);
    assert!(b.remove_place(0).is_err());

    b.settle_prefs_io();
    let mut again = Browser::new();
    again.use_prefs_fixture(file).unwrap();
    assert_eq!(again.locations().len(), 2);
}

#[test]
fn the_plus_asks_for_a_folder() {
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    let l = layout(&b);
    let (x, y) = b.center_of(&l, BrowserHit::LocAdd).unwrap();
    assert_eq!(b.hit(&l, x, y), Some(BrowserHit::LocAdd));
    assert_eq!(
        b.press(&l, BrowserHit::LocAdd),
        crate::browser::BrowserEffect::AddPlace
    );
}

#[test]
fn many_tabs_share_the_width_and_stay_inside_the_panel() {
    let root_tmp = folder("many");
    let root = root_tmp.path().to_path_buf();
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    for i in 0..6 {
        let d = root.join(format!("とても長い名前のフォルダー {i}"));
        std::fs::create_dir_all(&d).unwrap();
        b.add_place_fixture(&d).unwrap();
    }
    let l = layout(&b);
    assert_eq!(l.tabs.len(), 7);
    let panel_right = 52.0 + Layout::BROWSER_W;
    let add = l.add.expect("the plus is there");
    assert!(add.x + add.w <= panel_right + 0.5);
    for r in &l.tabs {
        assert!(r.x + r.w <= add.x + 0.5, "{r:?} {add:?}");
    }
}

#[test]
fn favourites_and_hidden_are_kept_and_another_programs_change_is_read() {
    let root_tmp = folder("prefs");
    let root = root_tmp.path().to_path_buf();
    let file = fresh(&root);
    let mut b = Browser::new();
    b.use_prefs_fixture(file.clone()).unwrap();
    b.toggle_favourite("Dexed");
    b.set_hidden("Surge XT", true);
    b.settle_prefs_io();
    let p = places::load(&file).unwrap();
    assert!(p.favourites.contains("Dexed"));
    assert!(p.hidden.contains("Surge XT"));

    // The CLI shows hidden things meanwhile: the screen follows on its next poll.
    std::thread::sleep(Duration::from_millis(20));
    let mut p = p;
    p.show_hidden = true;
    p.favourites.remove("Dexed");
    places::save(&file, &p).unwrap();
    b.prefs_io.at = None;
    b.settle_prefs_io();
    assert!(b.show_hidden);
    assert!(!b.favourites.contains("Dexed"));
    assert!(!b.reload_prefs(), "nothing new");
}

#[test]
fn a_file_that_cannot_be_read_says_so() {
    let root_tmp = folder("broken");
    let root = root_tmp.path().to_path_buf();
    let file = root.join("browser.json");
    std::fs::write(&file, "{ not json").unwrap();
    let mut b = Browser::new();
    assert!(b.use_prefs_fixture(file).is_err());
}

#[test]
fn choosing_an_audio_file_starts_its_audition_and_a_midi_file_does_not() {
    let top_tmp = folder("audition");
    let top = top_tmp.path().join("素材");
    crate::import_shots::make_place(&top);
    let mut b = on(&top);
    let pad = key(&b, "pad_44k.wav");
    let l = layout(&b);
    let i = l
        .rows
        .iter()
        .position(|p| matches!(&p.row, BRow::File { name, .. } if name == "pad_44k.wav"))
        .unwrap();
    b.press(&l, BrowserHit::File(i));
    assert_eq!(b.playing.as_deref(), Some(pad.as_str()));
    assert_eq!(names(&b)[i + 1], "<audition>");
    let l = layout(&b);
    let (x, y) = b.center_of(&l, BrowserHit::Audition).unwrap();
    assert_eq!(b.hit(&l, x, y), Some(BrowserHit::Audition));
    b.press(&l, BrowserHit::Audition);
    assert_eq!(b.playing, None);
    b.press(&l, BrowserHit::Audition);
    assert!(b.playing.is_some());

    // A MIDI file: chosen, no audition row, and what played stops.
    b.select_file(&key(&b, "idea.mid"));
    assert_eq!(b.playing, None);
    assert!(!names(&b).contains(&"<audition>".to_string()));
}
