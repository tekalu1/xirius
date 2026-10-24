use std::collections::HashMap;
use std::thread::ThreadId;

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

/// A slow place, like a drive on the network: each look at a file waits `stat`, each open
/// waits `open`, and each read gives at most `piece` bytes after `per_read`. It counts the opens
/// of each file and the threads that made them.
struct Slow {
    stat: Duration,
    open: Duration,
    piece: usize,
    per_read: Duration,
    opens: Mutex<HashMap<PathBuf, usize>>,
    threads: Mutex<Vec<ThreadId>>,
}

impl Slow {
    fn new(open: Duration) -> Arc<Slow> {
        Slow::reading(open, usize::MAX, Duration::ZERO)
    }

    fn reading(open: Duration, piece: usize, per_read: Duration) -> Arc<Slow> {
        Arc::new(Slow {
            stat: Duration::ZERO,
            open,
            piece,
            per_read,
            opens: Mutex::default(),
            threads: Mutex::default(),
        })
    }

    /// A place where even looking at a file waits `stat`.
    fn looking(stat: Duration, open: Duration) -> Arc<Slow> {
        Arc::new(Slow {
            stat,
            ..Arc::into_inner(Slow::new(open)).unwrap()
        })
    }

    fn opens(&self, path: &Path) -> usize {
        lock(&self.opens).get(path).copied().unwrap_or(0)
    }
}

struct Trickle {
    file: std::fs::File,
    piece: usize,
    per_read: Duration,
}

impl Read for Trickle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::thread::sleep(self.per_read);
        let n = buf.len().min(self.piece);
        self.file.read(&mut buf[..n])
    }
}

impl Reader for Slow {
    fn stat(&self, path: &Path) -> Result<Stamp, String> {
        std::thread::sleep(self.stat);
        Disk.stat(path)
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>, String> {
        *lock(&self.opens).entry(path.to_path_buf()).or_default() += 1;
        lock(&self.threads).push(std::thread::current().id());
        std::thread::sleep(self.open);
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        Ok(Box::new(Trickle {
            file,
            piece: self.piece,
            per_read: self.per_read,
        }))
    }
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

#[test]
fn a_press_hands_the_file_over_and_it_is_opened_once_elsewhere() {
    let d = dir("once");
    let file = wav(d.path(), "a.wav", 0.5, 44_100);
    let store = d.path().join("peaks");
    let slow = Slow::new(Duration::from_millis(600));
    let a = Audition::with(slow.clone());
    let t = Instant::now();
    a.play(&file, &store, SILENT);
    // Far less than opening takes, however busy the machine.
    assert!(
        t.elapsed() < Duration::from_millis(300),
        "{:?}",
        t.elapsed()
    );
    assert!(a.now().loading(&file));
    let n = until(&a, "ending", |n| n.phase == Phase::Ended);
    assert_eq!(n.path.as_deref(), Some(file.as_path()));
    assert!(n.complete(&file));
    assert_eq!(n.whole, Some(22_050));
    assert_eq!(slow.opens(&file), 1);
    assert!(!lock(&slow.threads).contains(&std::thread::current().id()));
    // The peaks are kept for the song's lanes, the same as the ones the lanes make.
    assert!(peaks::load(&file, &store).is_some());
}

#[test]
fn the_file_asked_for_last_is_the_one_that_plays() {
    let d = dir("newest");
    let store = d.path().join("peaks");
    let first = wav(d.path(), "first.wav", 0.5, 48_000);
    let second = wav(d.path(), "second.wav", 0.25, 48_000);
    let slow = Slow::new(Duration::from_millis(300));
    let a = Audition::with(slow.clone());
    a.play(&first, &store, SILENT);
    std::thread::sleep(Duration::from_millis(50));
    a.play(&second, &store, SILENT);
    let n = until(&a, "playing", |n| n.phase != Phase::Loading);
    assert_eq!(n.path.as_deref(), Some(second.as_path()));
    assert_eq!(n.whole, Some(12_000));
    // The first one's reading stops once nothing wants it any more; it is not played after.
    let n = until(&a, "ending", |n| n.phase == Phase::Ended);
    assert_eq!(n.path.as_deref(), Some(second.as_path()));
    std::thread::sleep(Duration::from_millis(400));
    let n = a.now();
    assert_eq!(n.path.as_deref(), Some(second.as_path()));
    assert_eq!(n.phase, Phase::Ended);
    assert_eq!(slow.opens(&second), 1);
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
    // Looking at a file takes a while, so the one asked for first fails after the second is.
    let slow = Slow::looking(Duration::from_millis(300), Duration::from_millis(100));
    let a = Audition::with(slow.clone());
    a.play(&gone, &store, SILENT);
    std::thread::sleep(Duration::from_millis(50));
    a.play(&file, &store, SILENT);
    watch_to_end(&a, &file, |n| !matches!(n.phase, Phase::Failed(_)));
}

#[test]
fn the_peaks_grow_as_the_file_is_read() {
    let d = dir("grow");
    let store = d.path().join("peaks");
    let file = wav(d.path(), "a.wav", 3.0, 48_000);
    // About 576 KB, 32 KB a read every 25 ms: the reading takes about half a second.
    let slow = Slow::reading(
        Duration::from_millis(50),
        32 << 10,
        Duration::from_millis(25),
    );
    let a = Audition::with(slow.clone());
    a.play(&file, &store, SILENT);
    let part = until(&a, "some peaks", |n| n.peaks.is_some());
    assert_eq!(part.phase, Phase::Loading);
    assert!(
        part.covered > 0 && part.covered < 144_000,
        "{}",
        part.covered
    );
    assert_eq!(part.whole, Some(144_000));
    let n = until(&a, "playing", |n| n.phase != Phase::Loading);
    assert!(n.complete(&file));
    assert_eq!(slow.opens(&file), 1);
}

#[test]
fn peaks_kept_from_before_are_shown_before_the_file_is_read() {
    let d = dir("stored");
    let store = d.path().join("peaks");
    let file = wav(d.path(), "a.wav", 0.5, 48_000);
    peaks::ensure(&file, &store).unwrap();
    let slow = Slow::new(Duration::from_millis(400));
    let a = Audition::with(slow.clone());
    a.play(&file, &store, SILENT);
    let n = until(&a, "peaks", |n| n.peaks.is_some());
    assert_eq!(n.phase, Phase::Loading);
    assert!(n.complete(&file));
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
    let slow = Slow::new(Duration::ZERO);
    let a = Audition::with(slow.clone());
    a.play(&file, &store, SILENT);
    let n = until(&a, "ending", |n| n.phase == Phase::Ended);
    assert_eq!(n.whole, Some(12_000));
    assert_eq!(slow.opens(&file), 1);
}

#[test]
fn a_file_that_cannot_be_read_says_so() {
    let d = dir("fail");
    let store = d.path().join("peaks");
    let a = Audition::with(Arc::new(Disk));
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
    let a = Audition::with(Arc::new(Disk));
    a.play(&file, &store, Sink::Silent { paced: true });
    until(&a, "playing", |n| {
        n.phase == Phase::Playing && n.played > 0.0
    });
    a.stop();
    let n = a.now();
    assert_eq!((n.path, n.phase), (None, Phase::Idle));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(a.now().phase, Phase::Idle);
}

#[test]
fn release_stops_what_plays_and_does_not_wait_on_a_slow_place() {
    let d = dir("release");
    let store = d.path().join("peaks");
    let file = wav(d.path(), "a.wav", 5.0, 48_000);
    let a = Audition::with(Arc::new(Disk));
    a.play(&file, &store, Sink::Silent { paced: true });
    until(&a, "playing", |n| n.phase == Phase::Playing);
    a.release();
    assert_eq!(a.now().phase, Phase::Idle);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(a.now().phase, Phase::Idle);
    // A file in a place where even looking at it takes long: letting go is still quick.
    let slow = Slow::looking(Duration::from_secs(2), Duration::ZERO);
    let a = Audition::with(slow);
    a.play(&file, &store, SILENT);
    std::thread::sleep(Duration::from_millis(20));
    let t = Instant::now();
    a.release();
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    a.play(&file, &store, SILENT);
    let t = Instant::now();
    a.stop();
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
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
    let (audio, mut control) = engine::create(cue::plan(setup).unwrap(), EngineConfig::default());
    let stats = control.stats();
    let _out = crate::output::SilentOutput::start(audio, true);
    let a = Audition::new();
    let sink = Sink::Mix(control.audition());
    a.play(&file, &d.path().join("peaks"), sink.clone());
    until(&a, "resampled preview sounds", |n| n.played > 0.1);
    assert!(stats.take_output_peak().iter().any(|p| *p > 0.1));
    until(&a, "preview ends", |n| n.phase == Phase::Ended);
    control.poll(|e| {
        if let EngineEvent::Transport {
            playing, position, ..
        } = e
        {
            assert!(!playing);
            assert_eq!(position, 0, "preview never seeks the song");
        }
    });
    a.play(&file, &d.path().join("peaks"), sink);
    until(&a, "second preview sounds", |n| n.played > 0.1);
    a.stop();
    std::thread::sleep(Duration::from_millis(50));
    stats.take_output_peak();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        stats.take_output_peak(),
        [0.0; 2],
        "queued preview is discarded on stop"
    );
}
