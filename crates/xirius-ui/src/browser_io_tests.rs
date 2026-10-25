//! Exercise the actual idle and input callbacks while individual disk operations
//! are held. Timeouts are deadlock guards, never speed-based acceptance criteria.
use crate::{
    browser::{BRow, BrowserHit, RailTab},
    song::Song,
    studio::{FileRequest, Studio},
    wave,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, mpsc},
    time::{Duration, Instant},
};
use xirius_session::peaks;
pub(crate) static IO_GATES: Mutex<()> = Mutex::new(());
pub(crate) struct Release(pub(crate) Arc<(Mutex<bool>, Condvar)>);
impl Release {
    pub(crate) fn new() -> Self {
        Self(Arc::new((Mutex::new(false), Condvar::new())))
    }
    pub(crate) fn release(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.release();
    }
}
fn setup(root: &Path) -> (Studio, PathBuf, PathBuf) {
    let samples = root.join("samples");
    crate::import_shots::make_place(&samples);
    let mut s = Studio::new(Song::empty());
    let song = root.join("song");
    s.song.save_as(&song).unwrap();
    s.song
        .run(
            "clip.import",
            serde_json::json!({"file":samples.join("pad_44k.wav")}),
        )
        .unwrap();
    for dir in xirius_session::record::take_folders(&s.project().media, &song) {
        xirius_session::export::write_wav(
            &dir.join("take.wav"),
            &[0.2; 256],
            &[0.2; 256],
            48_000,
            16,
        )
        .unwrap();
    }
    s.browser.press_rail(RailTab::Files);
    s.browser.add_place_fixture(&samples).unwrap();
    crate::browser_files::settle_rows(&s.browser);
    s.follow_browser();
    s.settle_pool();
    let prefs = root.join("browser.json");
    let p = s.browser.prefs();
    xirius_session::places::save(&prefs, &p).unwrap();
    s.browser.use_prefs_fixture(prefs).unwrap();
    s.browser.settle_prefs_io();
    peaks::wait();
    let file = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.clips)
        .find_map(|c| match &c.body {
            xirius_core::model::ClipBody::Audio { file, .. } => Some(file.clone()),
            _ => None,
        })
        .unwrap();
    let source = wave::source(Some(&song), &file).unwrap();
    (s, samples, source)
}
#[test]
fn actual_idle_and_browser_callbacks_do_not_wait_for_each_io_route() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    for held in [
        "pool_metadata",
        "pool_read_dir",
        "pool_file_type",
        "prefs_read",
        "prefs_write",
        "place_validate",
        "place_canonicalize",
        "browser_menu",
        "import_source",
        "import_probe",
        "import_copy",
        "midi_import_read",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let (mut s, samples, source) = setup(&root);
        let extra = root.join("extra");
        std::fs::create_dir(&extra).unwrap();
        let ui = std::thread::current().id();
        let release = Release::new();
        let gate = release.0.clone();
        let (tx, entered) = mpsc::channel();
        let (trace_tx, trace) = mpsc::channel();
        let observed = root.clone();
        let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if path.starts_with(&observed)
                && (op.starts_with("pool_")
                    || op.starts_with("prefs_")
                    || op.starts_with("place_")
                    || op == "browser_menu"
                    || op.starts_with("import_")
                    || op == "midi_import_read")
            {
                let thread = std::thread::current().id();
                assert_ne!(thread, ui, "{op} performed I/O on the UI thread");
                let _ = trace_tx.send((op.to_string(), thread));
                if op == held {
                    let _ = tx.send(());
                    let guard = gate.0.lock().unwrap();
                    drop(gate.1.wait_while(guard, |done| !*done).unwrap());
                }
            }
            Ok(())
        }));
        match held {
            "pool_metadata" | "pool_read_dir" | "pool_file_type" => {
                s.browser.invalidate_pool();
                s.follow_pool();
            }
            "prefs_read" => {
                s.browser.prefs_io.at = None;
                s.browser.reload_prefs();
            }
            "prefs_write" => {
                s.browser.toggle_favourite("fixture");
                s.browser.reload_prefs();
            }
            "place_validate" | "place_canonicalize" => {
                s.finish_picked(FileRequest::Place, &[extra]);
                s.browser.reload_prefs();
            }
            "browser_menu" => {
                let g = s.geometry(1440.0, 860.0);
                let l = s.browser.layout(g.panel);
                let i = l
                    .rows
                    .iter()
                    .position(|p| matches!(&p.row,BRow::File{name,..} if name=="pad_44k.wav"))
                    .unwrap();
                let (x, y) = s.browser.center_of(&l, BrowserHit::File(i)).unwrap();
                assert!(s.lib_context(&g, x, y));
            }
            "midi_import_read" => {
                s.put_files(&[samples.join("chords.mid")]);
                s.poll_browser_import();
            }
            _ => {
                let g = s.geometry(1440.0, 860.0);
                assert!(s.drop_files(&g, 0.0, 0.0, &[samples.join("pad_44k.wav")]));
                s.poll_browser_import();
            }
        }
        entered.recv_timeout(Duration::from_secs(15)).expect(held);
        for _ in 0..4 {
            s.idle();
            let g = s.geometry(1440.0, 860.0);
            s.browser.layout(g.panel);
            wave::look(&source, &peaks::store_dir(s.song.dir().as_deref()));
            wave::poll();
        }
        release.release();
        s.settle_browser_import();
        s.browser.settle_prefs_io();
        s.settle_browser_menu();
        s.settle_pool();
        peaks::wait();
        wave::poll();
        let seen: Vec<_> = trace.try_iter().collect();
        assert!(seen.iter().any(|(op, _)| op == held), "{held}");
        assert!(seen.iter().all(|(_, thread)| *thread != ui));
        drop(observation);
        wave::forget();
    }
}
#[test]
fn uncollected_old_peak_success_is_not_the_new_request_result() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let wav = tmp.path().join("clip.wav");
    let other = tmp.path().join("other.wav");
    let store = tmp.path().join("peaks");
    for path in [&wav, &other] {
        xirius_session::export::write_wav(path, &[0.3; 256], &[0.3; 256], 48_000, 16).unwrap();
    }
    let _old = peaks::request(&wav, &store);
    peaks::wait(); // Keep the old subscriber and its successful data alive.
    std::fs::remove_file(&wav).unwrap();
    wave::forget();
    let release = Release::new();
    let gate = release.0.clone();
    let path = wav.clone();
    let (tx, entered) = mpsc::channel();
    let observation = xirius_core::io_test::observe(Arc::new(move |op, p| {
        if op == "peaks_source" && p == path {
            let _ = tx.send(());
            let guard = gate.0.lock().unwrap();
            drop(gate.1.wait_while(guard, |done| !*done).unwrap());
        }
        Ok(())
    }));
    assert!(matches!(wave::look(&wav, &store), wave::Look::Making));
    entered.recv_timeout(Duration::from_secs(15)).unwrap();
    let request = peaks::request(&other, &store);
    let until = Instant::now() + Duration::from_secs(15);
    while peaks::take_request(&request).is_none() {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    wave::poll();
    assert!(matches!(wave::look(&wav, &store), wave::Look::Making));
    // A window generation change must discard even an in-flight prior request.
    wave::forget();
    assert!(matches!(wave::look(&wav, &store), wave::Look::Making));
    release.release();
    peaks::wait();
    wave::poll();
    assert!(matches!(wave::look(&wav, &store), wave::Look::Missing));
    drop(observation);
    wave::forget();
}

#[test]
fn accepted_preference_save_survives_the_browser_without_ui_join() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("prefs.json");
    xirius_session::places::save(&file, &xirius_session::places::Prefs::first_run(None)).unwrap();
    let mut b = crate::browser::Browser::new();
    b.use_prefs_fixture(file.clone()).unwrap();
    let release = Release::new();
    let gate = release.0.clone();
    let path = file.clone();
    let ui = std::thread::current().id();
    let (tx, entered) = mpsc::channel();
    let (done, written) = mpsc::channel();
    let observation = xirius_core::io_test::observe(Arc::new(move |op, p| {
        if p == path && op == "prefs_write" {
            assert_ne!(std::thread::current().id(), ui);
            let _ = tx.send(());
            let guard = gate.0.lock().unwrap();
            drop(gate.1.wait_while(guard, |ready| !*ready).unwrap());
        }
        if p == path && op == "prefs_written" {
            let _ = done.send(());
        }
        Ok(())
    }));
    b.toggle_favourite("saved");
    drop(b);
    entered.recv_timeout(Duration::from_secs(15)).unwrap();
    release.release();
    written.recv_timeout(Duration::from_secs(15)).unwrap();
    drop(observation);
    assert!(
        xirius_session::places::load(&file)
            .unwrap()
            .favourites
            .contains("saved")
    );
}

#[test]
fn a_waveform_retries_back_pressure_from_idle_without_another_paint() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("clip.wav");
    let store = tmp.path().join("peaks");
    xirius_session::export::write_wav(&file, &[0.2; 256], &[0.2; 256], 48_000, 16).unwrap();
    wave::forget();
    let jobs = xirius_session::read_jobs::ReadJobs::<usize, ()>::default();
    let release = Release::new();
    let (tx, entered) = mpsc::channel();
    for key in 0..xirius_session::read_jobs::THREADS {
        let gate = release.0.clone();
        let tx = tx.clone();
        assert!(
            jobs.request(key, move || {
                let _ = tx.send(());
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |ready| !*ready).unwrap());
            })
            .is_some()
        );
    }
    for _ in 0..xirius_session::read_jobs::THREADS {
        entered.recv_timeout(Duration::from_secs(15)).unwrap();
    }
    let mut rejected = false;
    for key in 4..1000 {
        let gate = release.0.clone();
        if jobs
            .request(key, move || {
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |ready| !*ready).unwrap());
            })
            .is_none()
        {
            rejected = true;
            break;
        }
    }
    assert!(rejected);
    assert!(matches!(wave::look(&file, &store), wave::Look::Making));
    release.release();
    drop(jobs);
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        wave::poll();
        if matches!(wave::look(&file, &store), wave::Look::Ready(_)) {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    wave::forget();
}

#[test]
fn an_old_pool_result_cannot_replace_a_new_revision_or_generation() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    for revision in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _, source) = setup(tmp.path());
        let folder = s.song.dir().unwrap();
        let release = Release::new();
        let gate = release.0.clone();
        let once = std::sync::atomic::AtomicBool::new(true);
        let (tx, entered) = mpsc::channel();
        let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if op == "pool_result"
                && path == folder
                && once.swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                let _ = tx.send(());
                let guard = gate.0.lock().unwrap();
                drop(gate.1.wait_while(guard, |ready| !*ready).unwrap());
            }
            Ok(())
        }));
        s.browser.invalidate_pool();
        s.follow_pool();
        entered.recv_timeout(Duration::from_secs(15)).unwrap();
        if revision {
            let ids: Vec<_> = s
                .project()
                .tracks
                .values()
                .flat_map(|t| t.clips.iter().map(|c| c.id.to_string()))
                .collect();
            s.song
                .run("clip.delete", serde_json::json!({"ids":ids}))
                .unwrap();
        } else {
            std::fs::remove_file(&source).unwrap();
            s.browser.invalidate_pool();
        }
        s.settle_pool();
        if revision {
            assert!(!s.browser.pool.iter().any(|e| e.path == source));
        } else {
            assert!(s.browser.pool.iter().any(|e| e.path == source && !e.exists));
        }
        let kept = s.browser.pool.clone();
        release.release();
        let until = Instant::now() + Duration::from_secs(15);
        while s.pool_workers_pending() {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        s.follow_pool();
        assert_eq!(s.browser.pool, kept);
        drop(observation);
    }
}

#[test]
fn preference_loading_preserves_edits_made_while_the_read_is_held() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("prefs.json");
    let mut prefs = xirius_session::places::Prefs::first_run(None);
    prefs.favourites.insert("stored favourite".into());
    prefs.places.push(xirius_session::places::Place {
        name: "stored".into(),
        path: tmp.path().join("place"),
    });
    xirius_session::places::save(&file, &prefs).unwrap();
    let release = Release::new();
    let gate = release.0.clone();
    let path = file.clone();
    let (tx, entered) = mpsc::channel();
    let (done, written) = mpsc::channel();
    let first = std::sync::atomic::AtomicBool::new(true);
    let observation = xirius_core::io_test::observe(Arc::new(move |op, p| {
        if op == "prefs_written" && p == path {
            let _ = done.send(());
        }
        if op == "prefs_read" && p == path && first.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            let _ = tx.send(());
            let guard = gate.0.lock().unwrap();
            drop(gate.1.wait_while(guard, |ready| !*ready).unwrap());
        }
        Ok(())
    }));
    let mut b = crate::browser::Browser::new();
    b.start_prefs(file.clone());
    b.reload_prefs();
    entered.recv_timeout(Duration::from_secs(15)).unwrap();
    b.toggle_favourite("local edit");
    written.recv_timeout(Duration::from_secs(15)).unwrap();
    release.release();
    b.settle_prefs_io();
    assert_eq!(b.places, prefs.places);
    assert!(b.favourites.contains("local edit"));
    assert!(b.favourites.contains("stored favourite"));
    drop(observation);
    let saved = xirius_session::places::load(&file).unwrap();
    assert_eq!(saved.places, prefs.places);
    assert!(saved.favourites.contains("local edit"));
}

#[test]
fn queued_imports_do_not_enter_a_replacement_song() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("clip.wav");
    xirius_session::export::write_wav(&file, &[0.1; 256], &[0.1; 256], 48_000, 16).unwrap();
    let mut s = Studio::new(Song::empty());
    s.put_files(&[file]);
    s.song = Song::empty();
    s.settle_browser_import();
    assert!(s.project().tracks.values().all(|t| t.clips.is_empty()));
}
