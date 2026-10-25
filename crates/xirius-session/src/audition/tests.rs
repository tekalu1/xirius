use std::collections::HashMap;

use super::*;

/// A folder of the test's own, removed when dropped (also when the test fails).
fn dir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-audition-{tag}-"))
        .tempdir()
        .unwrap()
}

/// A 16 bit stereo WAV of `seconds` at `rate`.
fn wav(dir: &Path, name: &str, seconds: f64, rate: u32) -> PathBuf {
    let n = (seconds * f64::from(rate)) as usize;
    let x: Vec<f32> = (0..n)
        .map(|i| ((i % 100) as f32 / 100.0 - 0.5) * 0.5)
        .collect();
    let path = dir.join(name);
    crate::export::write_wav(&path, &x, &x, rate, 16).unwrap();
    path
}

const SILENT: Sink = Sink::Silent { paced: false };

/// Waits until `ok` holds of what the audition says, and gives that.
fn until(a: &Audition, what: &str, ok: impl Fn(&Now) -> bool) -> Now {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let n = a.now();
        if ok(&n) {
            return n;
        }
        assert!(Instant::now() < end, "{what} did not happen: {n:?}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn kept_names(a: &Audition) -> Vec<String> {
    a.kept()
        .0
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_press_hands_the_file_over_and_it_is_opened_once_elsewhere() {
    let d = dir("once");
    let file = wav(d.path(), "a.wav", 0.5, 44_100);
    let store = d.path().join("peaks");
    let reader = Arc::new(ReaderGates::default());
    let gate = hold(&reader.open, &file);
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&file, &store, SILENT);
    gate.0.entered(1);
    assert!(
        a.now().loading(&file),
        "play returned while open remains blocked"
    );
    gate.0.release();
    let n = until(&a, "ending", |n| n.phase == Phase::Ended);
    assert!(n.complete(&file));
    assert!(!n.kept);
    assert_eq!(n.whole, Some(22_050));
    assert_eq!(reader.opens(&file), 1);
    assert!(peaks::load(&file, &store).is_some());
}

#[test]
fn the_same_file_again_plays_at_once_without_being_opened() {
    let d = dir("again");
    let file = wav(d.path(), "a.wav", 0.5, 48_000);
    let store = d.path().join("peaks");
    let reader = Arc::new(ReaderGates::default());
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&file, &store, SILENT);
    until(&a, "warm cache", |n| n.phase == Phase::Ended);
    let gate = hold(&reader.stat, &file);
    a.play(&file, &store, SILENT);
    gate.0.entered(1);
    let n = until(&a, "cue independent of stat", |n| {
        n.kept && n.phase != Phase::Loading
    });
    assert!(n.complete(&file));
    assert_eq!(reader.opens(&file), 1);
    gate.0.release();
    drained(&a, &reader);
    until(&a, "ending", |n| n.phase == Phase::Ended);
}

#[test]
fn a_kept_file_asked_for_before_does_not_play_for_the_newer_one() {
    let d = dir("kept-older");
    let store = d.path().join("peaks");
    let old = wav(d.path(), "old.wav", 0.5, 48_000);
    let file = wav(d.path(), "b.wav", 0.25, 48_000);
    let reader = Arc::new(ReaderGates::default());
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&old, &store, SILENT);
    until(&a, "warm cache", |n| n.phase == Phase::Ended);
    let gate = hold(&reader.stat, &old);
    a.play(&old, &store, SILENT);
    gate.0.entered(1);
    a.play(&file, &store, SILENT);
    watch_to_end(&a, &file, |n| {
        !n.kept && n.peaks.as_ref().is_none_or(|p| p.frames <= 12_000)
    });
    gate.0.release();
    drained(&a, &reader);
    assert_eq!(a.now().path.as_deref(), Some(file.as_path()));
    assert_eq!(reader.opens(&old), 1);
}

#[test]
fn what_is_kept_stays_under_the_limit_and_the_oldest_goes_first() {
    let d = dir("limit");
    let store = d.path().join("peaks");
    let files: Vec<PathBuf> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| wav(d.path(), &format!("{n}.wav"), 1.0, 48_000))
        .collect();
    let long = wav(d.path(), "long.wav", 2.0, 48_000);
    // One second is 384,000 bytes of sound and about 1,000 of peaks: two fit, a third does not,
    // and two seconds are more than one file may keep.
    let limits = Limits {
        total: 1_200_000,
        one: 800_000,
    };
    let a = Audition::with(Arc::new(Disk), limits);
    let play = |p: &Path| {
        a.play(p, &store, SILENT);
        until(&a, "ending", |n| n.phase == Phase::Ended);
    };
    play(&files[0]);
    play(&files[1]);
    assert_eq!(kept_names(&a), ["a.wav", "b.wav"]);
    play(&files[2]);
    assert_eq!(kept_names(&a), ["b.wav", "c.wav"]);
    // Played again, b is the newest; d then pushes out c.
    play(&files[1]);
    assert!(a.now().kept);
    assert_eq!(kept_names(&a), ["c.wav", "b.wav"]);
    play(&files[3]);
    assert_eq!(kept_names(&a), ["b.wav", "d.wav"]);
    assert!(a.kept().1 <= limits.total);
    play(&long);
    assert_eq!(kept_names(&a), ["b.wav", "d.wav"]);
}

#[test]
fn the_file_asked_for_last_is_the_one_that_plays() {
    let d = dir("newest");
    let store = d.path().join("peaks");
    let first = wav(d.path(), "first.wav", 0.5, 48_000);
    let second = wav(d.path(), "second.wav", 0.25, 48_000);
    let reader = Arc::new(ReaderGates::default());
    let gate = hold(&reader.open, &first);
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&first, &store, SILENT);
    gate.0.entered(1);
    a.play(&second, &store, SILENT);
    let n = until(&a, "newest finishes while older open is held", |n| {
        n.phase == Phase::Ended
    });
    assert_eq!(n.path.as_deref(), Some(second.as_path()));
    assert_eq!(n.whole, Some(12_000));
    gate.0.release();
    drained(&a, &reader);
    assert_eq!(a.now().path.as_deref(), Some(second.as_path()));
    assert_eq!(reader.opens(&second), 1);
}

/// Watches what the audition says until `file` has played to its end, holding `ok` of each look.
fn watch_to_end(a: &Audition, file: &Path, ok: impl Fn(&Now) -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let n = a.now();
        assert_eq!(n.path.as_deref(), Some(file));
        assert!(ok(&n), "{n:?}");
        if n.phase == Phase::Ended {
            return;
        }
        assert!(Instant::now() < end, "{n:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn what_comes_of_a_file_asked_for_before_is_not_said_of_the_newer_one() {
    let d = dir("older");
    let store = d.path().join("peaks");
    let gone = d.path().join("gone.wav");
    let file = wav(d.path(), "b.wav", 0.25, 48_000);
    let reader = Arc::new(ReaderGates::default());
    let gate = hold(&reader.stat, &gone);
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&gone, &store, SILENT);
    gate.0.entered(1);
    a.play(&file, &store, SILENT);
    watch_to_end(&a, &file, |n| !matches!(n.phase, Phase::Failed(_)));
    gate.0.release();
    drained(&a, &reader);
    assert_eq!(a.now().phase, Phase::Ended);
}

#[test]
fn the_rows_read_ahead_play_at_once_when_asked_for() {
    let d = dir("ahead");
    let store = d.path().join("peaks");
    let files: Vec<_> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| wav(d.path(), &format!("{n}.wav"), 0.5, 48_000))
        .collect();
    let reader = Arc::new(ReaderGates::default());
    let a = Audition::with(reader.clone(), Limits::default());
    a.prefetch(files.iter().map(|f| (f.clone(), store.clone())).collect());
    let end = Instant::now() + Duration::from_secs(20);
    while a.kept().0.len() < files.len() {
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    let gate = hold(&reader.stat, &files[2]);
    a.play(&files[2], &store, SILENT);
    gate.0.entered(1);
    until(&a, "prefetched sound is cued without stat", |n| {
        n.kept && n.phase != Phase::Loading
    });
    gate.0.release();
    drained(&a, &reader);
    for f in &files {
        assert_eq!(reader.opens(f), 1);
    }
}

#[test]
fn a_file_read_ahead_and_not_kept_is_not_read_again() {
    let d = dir("ahead-once");
    let store = d.path().join("peaks");
    let long = wav(d.path(), "long.wav", 1.0, 48_000);
    let broken = d.path().join("broken.wav");
    std::fs::write(&broken, b"not a wav").unwrap();
    let reader = Arc::new(ReaderGates::default());
    let a = Audition::with(
        reader.clone(),
        Limits {
            one: 100_000,
            ..Limits::default()
        },
    );
    a.prefetch(vec![
        (long.clone(), store.clone()),
        (broken.clone(), store.clone()),
    ]);
    drained(&a, &reader);
    assert_eq!(reader.opens(&long), 1);
    assert_eq!(reader.opens(&broken), 1);
    assert!(a.kept().0.is_empty());
    a.prefetch(vec![(broken.clone(), store.clone()), (long.clone(), store)]);
    drained(&a, &reader);
    assert_eq!(reader.opens(&long), 1);
    assert_eq!(reader.opens(&broken), 1);
}

#[test]
fn files_read_ahead_go_before_the_one_asked_for() {
    let d = dir("ahead-order");
    let store = d.path().join("peaks");
    let files: Vec<PathBuf> = ["a", "b", "c"]
        .iter()
        .map(|n| wav(d.path(), &format!("{n}.wav"), 1.0, 48_000))
        .collect();
    // Two files of one second fit, a third does not.
    let limits = Limits {
        total: 1_200_000,
        one: 800_000,
    };
    let a = Audition::with(Arc::new(Disk), limits);
    a.play(&files[0], &store, SILENT);
    until(&a, "ending", |n| n.phase == Phase::Ended);
    let ahead = |f: &PathBuf| {
        a.prefetch(vec![(f.clone(), store.clone())]);
        let end = Instant::now() + Duration::from_secs(20);
        while !a.kept().0.contains(f) {
            assert!(Instant::now() < end, "{:?}", kept_names(&a));
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    ahead(&files[1]);
    assert_eq!(kept_names(&a), ["b.wav", "a.wav"]);
    // c pushes out b, read ahead, not a, the one chosen.
    ahead(&files[2]);
    assert_eq!(kept_names(&a), ["c.wav", "a.wav"]);
}

#[test]
fn a_file_being_read_ahead_is_not_opened_again_when_asked_for() {
    let d = dir("adopt");
    let store = d.path().join("peaks");
    let file = wav(d.path(), "a.wav", 0.5, 48_000);
    let reader = Arc::new(ReaderGates::default());
    let gate = hold(&reader.open, &file);
    let a = Audition::with(reader.clone(), Limits::default());
    a.prefetch(vec![(file.clone(), store.clone())]);
    gate.0.entered(1);
    a.play(&file, &store, SILENT);
    gates::barrier(&a);
    gate.0.release();
    until(&a, "ending", |n| n.phase == Phase::Ended);
    assert_eq!(reader.opens(&file), 1);
}

#[test]
fn the_peaks_grow_as_the_file_is_read() {
    let d = dir("grow");
    let store = d.path().join("peaks");
    let file = wav(d.path(), "a.wav", 3.0, 48_000);
    let reader = Arc::new(ReaderGates::default());
    let gate = hold(&reader.read, &file);
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&file, &store, SILENT);
    gate.0.entered(1);
    let part = until(&a, "partial peaks with the rest of the read held", |n| {
        n.peaks.is_some()
    });
    assert_eq!(part.phase, Phase::Loading);
    assert!(part.covered > 0 && part.covered < 144_000);
    assert_eq!(part.whole, Some(144_000));
    gate.0.release();
    let n = until(&a, "ending", |n| n.phase == Phase::Ended);
    assert!(n.complete(&file));
    assert_eq!(reader.opens(&file), 1);
}

#[test]
fn peaks_kept_from_before_are_shown_before_the_file_is_read() {
    let d = dir("stored");
    let store = d.path().join("peaks");
    let file = wav(d.path(), "a.wav", 0.5, 48_000);
    peaks::ensure(&file, &store).unwrap();
    let reader = Arc::new(ReaderGates::default());
    let gate = hold(&reader.open, &file);
    let a = Audition::with(reader, Limits::default());
    a.play(&file, &store, SILENT);
    gate.0.entered(1);
    let n = a.now();
    assert_eq!(n.phase, Phase::Loading);
    assert!(n.complete(&file));
    gate.0.release();
    until(&a, "ending", |n| n.phase == Phase::Ended);
}

#[test]
fn an_rf64_take_is_opened_once_too() {
    let d = dir("rf64");
    let store = d.path().join("peaks");
    let file = d.path().join("take.wav");
    let mut w = xirius_engine::wavfile::SafeWav::create(&file, 2, 48_000)
        .unwrap()
        .turning_rf64_after(4000);
    for i in 0..12_000 {
        let v = (i % 100) as f32 / 400.0;
        w.write_sample(v).unwrap();
        w.write_sample(-v).unwrap();
    }
    w.finalize().unwrap();
    let slow = Arc::new(ReaderGates::default());
    let a = Audition::with(slow.clone(), Limits::default());
    a.play(&file, &store, SILENT);
    let n = until(&a, "ending", |n| n.phase == Phase::Ended);
    assert_eq!(n.whole, Some(12_000));
    assert_eq!(slow.opens(&file), 1);
}

#[test]
fn a_file_that_cannot_be_read_says_so() {
    let d = dir("fail");
    let store = d.path().join("peaks");
    let a = Audition::with(Arc::new(Disk), Limits::default());
    let gone = d.path().join("gone.wav");
    a.play(&gone, &store, SILENT);
    let n = until(&a, "failing", |n| matches!(n.phase, Phase::Failed(_)));
    assert_eq!(n.path.as_deref(), Some(gone.as_path()));
    let bad = d.path().join("bad.wav");
    std::fs::write(&bad, b"not a wav at all").unwrap();
    a.play(&bad, &store, SILENT);
    let n = until(&a, "failing", |n| matches!(n.phase, Phase::Failed(_)));
    assert_eq!(n.path.as_deref(), Some(bad.as_path()));
    // A good file after them still plays.
    let good = wav(d.path(), "good.wav", 0.25, 48_000);
    a.play(&good, &store, SILENT);
    until(&a, "ending", |n| n.phase == Phase::Ended);
}

#[test]
fn stop_lets_go_of_the_file() {
    let d = dir("stop");
    let store = d.path().join("peaks");
    let file = wav(d.path(), "a.wav", 5.0, 48_000);
    let a = Audition::with(Arc::new(Disk), Limits::default());
    a.play(&file, &store, Sink::Silent { paced: true });
    until(&a, "playing", |n| {
        n.phase == Phase::Playing && n.played > 0.0
    });
    a.stop();
    let n = a.now();
    assert_eq!((n.path, n.phase), (None, Phase::Idle));
    let end = Instant::now() + Duration::from_secs(20);
    while !a.halted() {
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    assert_eq!(a.now().phase, Phase::Idle);
}

#[test]
fn release_stops_what_plays_and_does_not_wait_on_a_slow_place() {
    let d = dir("release");
    let store = d.path().join("peaks");
    let file = wav(d.path(), "a.wav", 5.0, 48_000);
    let reader = Arc::new(ReaderGates::default());
    let gate = hold(&reader.stat, &file);
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&file, &store, SILENT);
    gate.0.entered(1);
    let ack = a.release_async();
    ack.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_eq!(a.now().phase, Phase::Idle);
    a.play(&file, &store, SILENT);
    a.stop();
    assert_eq!(a.now().phase, Phase::Idle);
    gate.0.release();
    drained(&a, &reader);
    assert_eq!(a.now().phase, Phase::Idle);
}

#[test]
fn mixed_preview_resamples_ends_and_stops_without_moving_the_song() {
    let d = dir("mixed");
    let file = wav(d.path(), "a.wav", 0.5, 48_000);
    let setup = ProcessSetup {
        sample_rate: 44_100.0,
        max_block: 256,
        mode: RenderMode::Realtime,
    };
    let (mut audio, mut control) =
        engine::create(cue::plan(setup).unwrap(), EngineConfig::default());
    audio.set_silent(true);
    let stats = control.stats();
    let reader = Arc::new(ReaderGates::default());
    let a = Audition::with(reader.clone(), Limits::default());
    let sink = Sink::Mix(control.audition());
    fn ready(a: &Audition, reader: &Arc<ReaderGates>) {
        drained(a, reader);
        // Offline rendering starts after the voice has cued and fed the bus;
        // it must not race loading by running empty callbacks as fast as possible.
        let (tx, rx) = std::sync::mpsc::channel();
        a.voice.send(super::Call::Barrier(tx)).unwrap();
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
    }
    fn drive(a: &Audition, audio: &mut xirius_engine::AudioEngine, done: impl Fn(&Now) -> bool) {
        let end = Instant::now() + Duration::from_secs(20);
        let mut buf = [0.0; 512];
        while !done(&a.now()) {
            assert!(Instant::now() < end, "{:?}", a.now());
            audio.render(&mut buf, 2);
            std::thread::yield_now();
        }
    }
    a.play(&file, &d.path().join("peaks"), sink.clone());
    ready(&a, &reader);
    drive(&a, &mut audio, |n| n.phase == Phase::Ended);
    assert!(stats.take_output_peak().iter().any(|p| *p > 0.1));
    control.poll(|e| {
        if let EngineEvent::Transport {
            playing, position, ..
        } = e
        {
            assert!(!playing);
            assert_eq!(position, 0);
        }
    });
    a.play(&file, &d.path().join("peaks"), sink);
    ready(&a, &reader);
    drive(&a, &mut audio, |n| n.played > 0.1);
    a.stop();
    let end = Instant::now() + Duration::from_secs(20);
    while !a.halted() {
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    stats.take_output_peak();
    audio.render(&mut [0.0; 512], 2);
    assert_eq!(
        stats.take_output_peak(),
        [0.0; 2],
        "the next controlled callback contains no preview"
    );
}

mod review;

mod gates;
use gates::{ReaderGates, drained, hold};

#[test]
fn a_stale_cached_cue_is_stopped_before_the_fresh_file_opens() {
    let d = dir("stale-cache");
    let file = wav(d.path(), "a.wav", 0.5, 48_000);
    let store = d.path().join("peaks");
    let reader = Arc::new(ReaderGates::default());
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&file, &store, SILENT);
    until(&a, "warm cache", |n| n.phase == Phase::Ended);
    wav(d.path(), "a.wav", 0.25, 48_000);
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Realtime,
    };
    let (mut audio, control) = engine::create(cue::plan(setup).unwrap(), EngineConfig::default());
    audio.set_silent(true);
    let stat = hold(&reader.stat, &file);
    let open = hold(&reader.open, &file);
    a.play(&file, &store, Sink::Mix(control.audition()));
    stat.0.entered(1);
    let end = Instant::now() + Duration::from_secs(20);
    while control.stats().take_output_peak() == [0.0; 2] {
        assert!(Instant::now() < end);
        audio.render(&mut [0.0; 512], 2);
        std::thread::yield_now();
    }
    // Hold the output at the first audible callback. The cached sound cannot end.
    until(&a, "optimistic cache cue", |n| {
        n.kept && n.phase == Phase::Playing
    });
    stat.0.release();
    open.0.entered(1);
    gates::barrier(&a);
    let n = a.now();
    assert_eq!(n.phase, Phase::Loading);
    assert!(!n.kept);
    assert_eq!(n.played, 0.0);
    open.0.release();
    while a.now().phase != Phase::Ended {
        assert!(Instant::now() < end, "{:?}", a.now());
        audio.render(&mut [0.0; 512], 2);
        std::thread::yield_now();
    }
    assert_eq!(a.now().whole, Some(12_000));
    assert_eq!(reader.opens(&file), 2);
}

#[test]
fn validation_after_a_short_cached_sound_ends_only_refreshes_the_cache() {
    let d = dir("ended-cache");
    let file = wav(d.path(), "a.wav", 0.1, 48_000);
    let store = d.path().join("peaks");
    let reader = Arc::new(ReaderGates::default());
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&file, &store, SILENT);
    until(&a, "warm cache", |n| n.phase == Phase::Ended);
    wav(d.path(), "a.wav", 0.25, 48_000);
    let stat = hold(&reader.stat, &file);
    a.play(&file, &store, SILENT);
    stat.0.entered(1);
    until(&a, "cached sound ends before stat", |n| {
        n.kept && n.phase == Phase::Ended
    });
    stat.0.release();
    drained(&a, &reader);
    assert_eq!(
        a.now().phase,
        Phase::Ended,
        "validation must not revive the ended preview"
    );
    a.play(&file, &store, SILENT);
    let n = until(&a, "fresh cached sound", |n| {
        n.kept && n.phase == Phase::Ended
    });
    assert_eq!(n.whole, Some(12_000));
    assert_eq!(reader.opens(&file), 2);
}

mod cache;
