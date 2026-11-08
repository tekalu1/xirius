//! PLG-058: a plugin that faults or hangs on the ARA thread does not take the process down.
//! A test binary of its own: the last-chance filter is the process's.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use xirius_ara::fake::{self, Config};
use xirius_ara::service::source_id;
use xirius_ara::{ClipSpec, Phase, Service, Song, SourceAudio};

fn song(key: u64) -> Song {
    let audio = Arc::new(SourceAudio::stereo(
        48000.0,
        vec![0.25; 48000],
        vec![0.25; 48000],
    ));
    Song {
        clips: vec![ClipSpec {
            regions: Vec::new(),
            key,
            clone_of: None,
            source: source_id("crash.wav"),
            name: "clip".into(),
            audio,
            start: 0.0,
            offset: 0.0,
            duration: 1.0,
        }],
        ..Song::default()
    }
}

fn until(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_fault_and_a_hang_in_the_plugin_are_contained() {
    xirius_ara::contain::install_own_filter();

    // A fault on the ARA thread.
    let told = Arc::new(AtomicU64::new(0));
    let t = told.clone();
    let s = Service::start(
        fake::loader(Config {
            fault_on_analysis: true,
            ..Config::default()
        }),
        None,
        Arc::new(move || {
            t.fetch_add(1, Ordering::AcqRel);
        }),
    );
    s.sync(&song(1));
    until("the crash", || s.crashed().is_some());
    until("the clip's state", || {
        s.state(1).is_some_and(|c| c.phase == Phase::Crashed)
    });
    assert!(told.load(Ordering::Acquire) > 0);
    assert!(xirius_ara::contain::faults() >= 1);
    assert_eq!(xirius_ara::contain::last_code(), 0xC000_0005);
    assert!(s.store(Duration::from_secs(1)).is_none());
    drop(s);

    // A plugin that never returns from rendering.
    let s = Service::start_with(
        fake::loader(Config {
            hang_in_render: true,
            ..Config::default()
        }),
        None,
        Arc::new(|| {}),
        Duration::from_millis(300),
    );
    s.sync(&song(2));
    until("the hang", || s.crashed().is_some());
    assert!(
        s.crashed().unwrap().contains("answering"),
        "{}",
        s.crashed().unwrap()
    );
    drop(s);

    // On a plugin's shared thread, a fault in one document crashes all of them; the next
    // document starts a thread of its own.
    let calm = Service::start_shared(
        "fake:shared",
        fake::loader(Config::default()),
        None,
        Arc::new(|| {}),
    );
    calm.sync(&song(4));
    until("the calm render", || {
        calm.state(4).is_some_and(|c| c.phase == Phase::Ready)
    });
    let faulty = Service::start_shared(
        "fake:shared",
        fake::loader(Config {
            fault_on_analysis: true,
            ..Config::default()
        }),
        None,
        Arc::new(|| {}),
    );
    faulty.sync(&song(5));
    until("the shared crash", || {
        faulty.crashed().is_some() && calm.crashed().is_some()
    });
    assert_eq!(calm.state(4).unwrap().phase, Phase::Crashed);
    drop((calm, faulty));
    let again = Service::start_shared(
        "fake:shared",
        fake::loader(Config::default()),
        None,
        Arc::new(|| {}),
    );
    again.sync(&song(6));
    until("the render on a new thread", || {
        again.state(6).is_some_and(|c| c.phase == Phase::Ready)
    });
    drop(again);

    // A fault on a thread the plugin made itself: the ARA thread learns of it in its next
    // turn, and the plugin's documents count as crashed (no thread waits on the dead one).
    let s = Service::start(
        fake::loader(Config {
            fault_on_own_thread: true,
            ..Config::default()
        }),
        None,
        Arc::new(|| {}),
    );
    s.sync(&song(7));
    until("the crash on the plugin's thread", || s.crashed().is_some());
    let why = s.crashed().unwrap();
    assert!(why.contains("thread of its own"), "{why}");
    until("the clip's state", || {
        s.state(7).is_some_and(|c| c.phase == Phase::Crashed)
    });
    // A crashed clip is terminal, even with no waiting budget left.
    assert!(s.wait_idle(Instant::now()));
    drop(s);

    // The process goes on, and a plugin that behaves still works.
    let s = Service::start(fake::loader(Config::default()), None, Arc::new(|| {}));
    s.sync(&song(3));
    until("the render", || {
        s.state(3).is_some_and(|c| c.phase == Phase::Ready)
    });
    drop(s);
}
