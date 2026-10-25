//! Regression tests for acceptance, shutdown and ownership across asynchronous I/O.
use crate::{
    browser::Browser,
    browser_io_tests::{IO_GATES, Release},
    song::Song,
    studio::Studio,
    wave,
};
use std::{
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use xirius_session::{
    peaks,
    read_jobs::{QUEUED, ReadJobs, THREADS},
};
#[track_caller]
fn until(mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(60);
    while !ready() {
        assert!(Instant::now() < end, "worker did not finish");
        std::thread::yield_now();
    }
}
fn fill_readers() -> (ReadJobs<usize, ()>, Release) {
    let jobs = ReadJobs::default();
    let release = Release::new();
    let (tx, entered) = mpsc::channel();
    for key in 0..THREADS {
        let gate = release.0.clone();
        let tx = tx.clone();
        assert!(
            jobs.request(key, move || {
                tx.send(()).unwrap();
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |ready| !*ready).unwrap());
            })
            .is_some()
        );
    }
    for _ in 0..THREADS {
        entered.recv_timeout(Duration::from_secs(60)).unwrap();
    }
    for key in THREADS..THREADS + QUEUED {
        if jobs.request(key, || ()).is_none() {
            break;
        }
    }
    assert!(jobs.request(usize::MAX, || ()).is_none());
    (jobs, release)
}
fn wav(file: &Path) {
    xirius_session::export::write_wav(file, &[0.2; 256], &[0.2; 256], 48000, 16).unwrap();
}
#[test]
fn full_read_queue_keeps_all_waveforms_listing_and_the_final_preference_snapshot() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let prefs = tmp.path().join("prefs.json");
    let mut browser = Browser::new();
    browser.start_prefs(prefs.clone());
    browser.add_place_fixture(tmp.path()).unwrap();
    browser.set_location(1);
    browser.press_rail(crate::browser::RailTab::Files);
    let files: Vec<_> = (0..300)
        .map(|i| {
            let p = tmp.path().join(format!("clip-{i}.wav"));
            wav(&p);
            p
        })
        .collect();
    wave::forget();
    crate::browser_files::forget();
    let (jobs, release) = fill_readers();
    browser.rows();
    let store = tmp.path().join("peaks");
    let legacy = peaks::request(&files[0], &store);
    for file in &files {
        assert!(matches!(wave::look(file, &store), wave::Look::Making));
    }
    assert_eq!(wave::pending_peaks(), files.len());
    let mut prefs_browser = browser.clone();
    prefs_browser.toggle_favourite("last snapshot");
    drop(prefs_browser); // Acceptance never relies on Drop finding a free read slot.
    release.release();
    drop(jobs);
    peaks::wait();
    assert!(peaks::take_request(&legacy).unwrap().is_ok());
    drop(legacy);
    until(|| {
        wave::poll();
        wave::pending_peaks() == 0
    }); // No new paint/request.
    for file in &files {
        assert!(matches!(wave::look(file, &store), wave::Look::Ready(_)));
    }
    until(|| {
        xirius_session::places::load(&prefs).is_ok_and(|p| p.favourites.contains("last snapshot"))
    });
    // Mimic the existing timer -> invalidation -> redraw path after queue overflow.
    until(|| {
        if !crate::browser_files::poll() {
            return false;
        }
        browser
            .rows()
            .iter()
            .any(|r| matches!(r, crate::browser::BRow::File { .. }))
    });
    wave::forget();
    crate::browser_files::forget();
    until(|| peaks::retained_keys_under(tmp.path()) == 0);
}
#[test]
fn completed_waveforms_are_released_on_forget_and_subscribers_share_the_result() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join("peaks");
    for i in 0..40 {
        let file = tmp.path().join(format!("clip-{i}.wav"));
        wav(&file);
        let a = peaks::request_fresh(&file, &store);
        let b = peaks::request_fresh(&file, &store);
        peaks::wait();
        let data = peaks::take_request(&a).unwrap().unwrap();
        assert!(Arc::ptr_eq(
            &data,
            &peaks::take_request(&b).unwrap().unwrap()
        ));
        let weak = Arc::downgrade(&data);
        drop(data);
        drop(a);
        drop(b);
        assert!(weak.upgrade().is_none());
        assert!(matches!(wave::look(&file, &store), wave::Look::Making));
        peaks::wait();
        wave::poll();
        let data = match wave::look(&file, &store) {
            wave::Look::Ready(p) => p,
            _ => panic!("not ready"),
        };
        let weak = Arc::downgrade(&data);
        drop(data);
        wave::forget();
        assert!(weak.upgrade().is_none());
        assert_eq!(peaks::retained_keys_under(tmp.path()), 0);
    }
}
#[test]
fn fire_and_forget_peak_data_and_keys_are_released_without_a_window_or_wait() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join("peaks");
    for i in 0..40 {
        let file = tmp.path().join(format!("clip-{i}.wav"));
        wav(&file);
        let request = peaks::request(&file, &store);
        let mut data = None;
        until(|| {
            data = peaks::take_request(&request);
            data.is_some()
        });
        let data = data.unwrap().unwrap();
        let weak = Arc::downgrade(&data);
        drop(data);
        drop(request);
        until(|| weak.upgrade().is_none() && peaks::retained_keys_under(tmp.path()) == 0);
    }
}

#[test]
fn import_reservation_covers_worker_completion_until_application_and_cancellation() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("closed");
    let file = folder.join("Media/Recordings/reserved.wav");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    wav(&file);
    let before_aside = Release::new();
    let prepared = Release::new();
    let ga = before_aside.0.clone();
    let gp = prepared.0.clone();
    let path = file.clone();
    let (at_aside, aside) = mpsc::channel();
    let (at_plan, plan) = mpsc::channel();
    let (at_second, second) = mpsc::channel();
    let observation = xirius_core::io_test::observe(Arc::new(move |op, p| {
        if p == path {
            match op {
                "cleanup_before_aside" => {
                    let _ = at_aside.send(());
                    let g = ga.0.lock().unwrap();
                    drop(ga.1.wait_while(g, |r| !*r).unwrap());
                }
                "browser_import_prepared" => {
                    let _ = at_plan.send(());
                    let g = gp.0.lock().unwrap();
                    drop(gp.1.wait_while(g, |r| !*r).unwrap());
                }
                "cleanup_second_check" => {
                    let _ = at_second.send(());
                }
                _ => {}
            }
        }
        Ok(())
    }));
    let project = xirius_core::model::Project::new("closed");
    let cleanup = std::thread::spawn(move || {
        let trash: xirius_session::cleanup::Trash =
            Arc::new(|_| panic!("reserved recording must not move"));
        xirius_session::cleanup::run(&project, &folder, false, &trash, Default::default()).unwrap()
    });
    aside.recv_timeout(Duration::from_secs(60)).unwrap();
    let mut s = Studio::new(Song::empty());
    s.import_audio(&file, None, 0).unwrap();
    s.poll_browser_import();
    plan.recv_timeout(Duration::from_secs(60)).unwrap();
    before_aside.release();
    second.recv_timeout(Duration::from_secs(60)).unwrap();
    cleanup.join().unwrap();
    assert!(
        file.is_file(),
        "second check must restore the reserved source before UI application"
    );
    assert!(s.project().tracks.values().all(|t| t.clips.is_empty()));
    prepared.release();
    s.settle_browser_import();
    assert!(s.error.is_none(), "{:?}", s.error);
    let key = xirius_core::in_use::key(None, &file);
    assert!(xirius_core::in_use::named(&key, None));
    drop(s);
    assert!(!xirius_core::in_use::named(&key, None));
    let mut s = Studio::new(Song::empty());
    s.import_audio(&file, None, 0).unwrap();
    assert!(xirius_core::in_use::named(&key, None));
    s.song = Song::empty();
    s.settle_browser_import();
    assert!(!xirius_core::in_use::named(&key, None));
    drop(observation);
}
#[test]
fn closing_waits_without_blocking_and_a_failed_preference_save_keeps_the_window() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    for fail in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("prefs.json");
        if fail {
            std::fs::create_dir(&file).unwrap();
        }
        let release = Release::new();
        let gate = release.0.clone();
        let path = file.clone();
        let (tx, entered) = mpsc::channel();
        let observation = xirius_core::io_test::observe(Arc::new(move |op, p| {
            if p == path && (op == "prefs_write" || (fail && op == "prefs_read")) {
                let _ = tx.send(());
                let g = gate.0.lock().unwrap();
                drop(gate.1.wait_while(g, |r| !*r).unwrap());
            }
            Ok(())
        }));
        let mut s = Studio::new(Song::empty());
        s.browser.start_prefs(file.clone());
        s.browser.toggle_favourite("before close");
        entered.recv_timeout(Duration::from_secs(60)).unwrap();
        assert!(!s.request_close());
        assert!(s.prefs_closing);
        s.idle();
        assert!(s.prefs_closing);
        assert!(s.window_ask.is_none());
        release.release();
        until(|| s.poll_browser_prefs_close());
        assert_eq!(s.error.is_some(), fail);
        assert_eq!(s.window_ask.is_some(), !fail);
        assert!(!s.prefs_closing);
        drop(observation);
    }
}

#[test]
fn coalescing_keeps_a_reverted_edit_even_if_an_earlier_save_is_already_running() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("prefs.json");
    xirius_session::places::save(&file, &xirius_session::places::Prefs::first_run(None)).unwrap();
    let mut b = Browser::new();
    b.use_prefs_fixture(file.clone()).unwrap();
    let release = Release::new();
    let gate = release.0.clone();
    let path = file.clone();
    let once = std::sync::atomic::AtomicBool::new(true);
    let (tx, entered) = mpsc::channel();
    let observation = xirius_core::io_test::observe(Arc::new(move |op, p| {
        if p == path && op == "prefs_write" && once.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            let _ = tx.send(());
            let g = gate.0.lock().unwrap();
            drop(gate.1.wait_while(g, |r| !*r).unwrap());
        }
        Ok(())
    }));
    b.toggle_favourite("reverted");
    entered.recv_timeout(Duration::from_secs(60)).unwrap();
    b.toggle_favourite("reverted");
    drop(b);
    release.release();
    until(|| crate::browser_prefs_save::status(&file).is_some());
    assert!(
        !xirius_session::places::load(&file)
            .unwrap()
            .favourites
            .contains("reverted")
    );
    drop(observation);
}
#[test]
fn a_failed_snapshot_is_kept_in_memory_and_included_in_the_next_save() {
    let _serial = IO_GATES.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("prefs.json");
    xirius_session::places::save(&file, &xirius_session::places::Prefs::first_run(None)).unwrap();
    let mut b = Browser::new();
    b.use_prefs_fixture(file.clone()).unwrap();
    let path = file.clone();
    let once = std::sync::atomic::AtomicBool::new(true);
    let observation = xirius_core::io_test::observe(Arc::new(move |op, p| {
        if p == path && op == "prefs_write" && once.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "write refused",
            ));
        }
        Ok(())
    }));
    b.toggle_favourite("first");
    b.settle_prefs_io();
    assert!(b.prefs_error.is_some());
    assert!(b.favourites.contains("first"));
    b.toggle_favourite("second");
    b.settle_prefs_io();
    let saved = xirius_session::places::load(&file).unwrap();
    assert!(saved.favourites.contains("first") && saved.favourites.contains("second"));
    drop(observation);
}

#[test]
fn preference_shutdown_child() {
    let Some(root) = std::env::var_os("XIRIUS_PREFS_SHUTDOWN_TEST") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let release = root.join("release");
    let entered = root.join("entered");
    let fail = std::env::var_os("XIRIUS_PREFS_FAIL_TEST").is_some();
    let observation = xirius_core::io_test::observe(Arc::new(move |op, _| {
        if op == "prefs_write" || (fail && op == "prefs_read") {
            std::fs::write(&entered, b"entered").unwrap();
            until(|| release.exists());
        }
        Ok(())
    }));
    let file = if std::env::var_os("XIRIUS_PREFS_FAIL_TEST").is_some() {
        root.join("directory")
    } else {
        root.join("prefs.json")
    };
    if std::env::var_os("XIRIUS_PREFS_FAIL_TEST").is_some() {
        std::fs::create_dir(&file).unwrap();
    }
    let mut browser = Browser::new();
    browser.start_prefs(file);
    browser.toggle_favourite("before exit");
    drop(browser);
    let mut finished = None;
    until(|| {
        finished = crate::browser_prefs_save::finish();
        if finished.is_none() {
            std::fs::write(root.join("pumping"), b"alive").unwrap();
        }
        finished.is_some()
    });
    assert_eq!(
        !finished.unwrap().is_empty(),
        std::env::var_os("XIRIUS_PREFS_FAIL_TEST").is_some()
    );
    drop(observation);
}
#[test]
fn process_shutdown_keeps_pumping_until_preferences_finish_or_report_failure() {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    for fail in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "browser_review_tests::preference_shutdown_child",
            "--nocapture",
        ])
        .env("XIRIUS_PREFS_SHUTDOWN_TEST", tmp.path())
        .creation_flags(0x08000000);
        if fail {
            cmd.env("XIRIUS_PREFS_FAIL_TEST", "1");
        }
        let mut child = cmd.spawn().unwrap();
        until(|| tmp.path().join("entered").exists() && tmp.path().join("pumping").exists());
        assert!(child.try_wait().unwrap().is_none());
        std::fs::write(tmp.path().join("release"), b"go").unwrap();
        until(|| child.try_wait().unwrap().is_some());
        assert!(child.wait().unwrap().success());
        if !fail {
            assert!(
                xirius_session::places::load(&tmp.path().join("prefs.json"))
                    .unwrap()
                    .favourites
                    .contains("before exit")
            );
        }
    }
}
