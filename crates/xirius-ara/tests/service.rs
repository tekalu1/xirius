//! The ARA host against the fake plugin (`xirius_ara::fake`), which checks ARA's rules.
//!
//! The fake's edits and rule checks are per process: every test uses its own clip keys.

use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_ara::fake::{self, Config};
use xirius_ara::host::Request;
use xirius_ara::service::{modification_id, source_id};
use xirius_ara::{ClipSpec, Phase, Service, Song, SourceAudio};

const WAIT: Duration = Duration::from_secs(10);

fn tone(rate: f64, seconds: f64, amplitude: f32) -> Arc<SourceAudio> {
    let n = (rate * seconds) as usize;
    let l: Vec<f32> = (0..n)
        .map(|i| amplitude * (i as f32 * 0.05).sin())
        .collect();
    let r: Vec<f32> = l.iter().map(|v| -v).collect();
    Arc::new(SourceAudio::stereo(rate, l, r))
}

fn clip(
    key: u64,
    file: &str,
    audio: &Arc<SourceAudio>,
    start: f64,
    offset: f64,
    duration: f64,
) -> ClipSpec {
    ClipSpec {
        key,
        clone_of: None,
        source: source_id(file),
        name: format!("clip {key}"),
        audio: audio.clone(),
        start,
        offset,
        duration,
    }
}

fn song(clips: Vec<ClipSpec>) -> Song {
    Song {
        clips,
        ..Song::default()
    }
}

fn start(config: Config, archive: Option<(Vec<u8>, String)>) -> Service {
    Service::start(fake::loader(config), archive, Arc::new(|| {}))
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn ready(s: &Service, key: u64) -> Arc<xirius_ara::service::Rendered> {
    until("the clip's render", || {
        s.state(key).is_some_and(|c| c.phase == Phase::Ready)
    });
    s.state(key).unwrap().rendered.unwrap()
}

fn no_violations() {
    let v = fake::violations();
    assert!(v.is_empty(), "the host broke ARA's rules: {v:?}");
}

fn peak(v: &[f32]) -> f32 {
    v.iter().fold(0.0f32, |p, x| p.max(x.abs()))
}

/// PLG-031: the plugin reaches all five of the host's interfaces.
#[test]
fn the_plugin_calls_all_five_host_interfaces() {
    let audio = tone(48000.0, 1.0, 0.5);
    let s = start(Config::default(), None);
    s.sync(&song(vec![clip(3101, "a.wav", &audio, 1.0, 0.0, 1.0)]));
    ready(&s, 3101);
    assert!(s.store(WAIT).is_some());
    fake::edit(&modification_id(3101), 0.5);
    until("the edit", || s.take_dirty());
    let p = s.probe(WAIT).unwrap();
    assert!(p.all_five, "calls per interface: {:?}", p.calls);
    assert_eq!(p.info.plugin_name, "Fake ARA");
    assert_eq!(p.generation, xirius_ara::sys::API_GENERATION_2_3_FINAL);
    assert!(
        s.take_requests()
            .iter()
            .any(|r| matches!(r, Request::Position(t) if (*t - 1.0).abs() < 1e-9))
    );
    drop(s);
    no_violations();
}

/// PLG-032: a clip becomes a playback region, and every change is made inside editing.
#[test]
fn a_clip_plays_through_its_region() {
    let rate = 48000.0;
    let audio = tone(rate, 2.0, 0.5);
    let s = start(Config::default(), None);
    s.sync(&song(vec![clip(3201, "b.wav", &audio, 0.5, 0.25, 1.0)]));
    let r = ready(&s, 3201);
    assert_eq!(r.rate, rate);
    assert_eq!(r.left.len(), rate as usize);
    let from = (0.25 * rate) as usize;
    for i in [0, 1, 1000, 47_999] {
        assert!(
            (r.left[i] - audio.channels[0][from + i]).abs() < 1e-6,
            "frame {i}"
        );
        assert!(
            (r.right[i] - audio.channels[1][from + i]).abs() < 1e-6,
            "frame {i}"
        );
    }
    // Shorter: the region is updated, not made again.
    s.sync(&song(vec![clip(3201, "b.wav", &audio, 0.5, 0.25, 0.5)]));
    until("the new length", || {
        s.state(3201)
            .and_then(|c| c.rendered)
            .is_some_and(|r| r.left.len() == (0.5 * rate) as usize)
    });
    drop(s);
    no_violations();
}

/// PLG-034: the analysis reads on the plugin's threads with two readers at once, and the
/// plugin's notices reach the host on its own thread only.
#[test]
fn analysis_reads_on_other_threads_and_notices_stay_on_the_model_thread() {
    let audio = tone(48000.0, 3.0, 0.5);
    let s = start(
        Config {
            analysis_ms: 100,
            ..Config::default()
        },
        None,
    );
    s.sync(&song(vec![clip(3401, "c.wav", &audio, 0.0, 0.0, 3.0)]));
    ready(&s, 3401);
    let p = s.probe(WAIT).unwrap();
    assert!(
        p.most_readers >= 2,
        "most readers at once: {}",
        p.most_readers
    );
    assert!(p.reads > 0 && p.reads == p.reads_off_thread);
    assert!(p.notices > 0);
    assert_eq!(p.notices_off_thread, 0);
    drop(s);
    no_violations();
}

/// PLG-035: the host sees the analysis end and reads its notes.
#[test]
fn the_end_of_analysis_is_seen_and_the_notes_are_read() {
    let audio = tone(48000.0, 1.0, 0.5);
    let s = start(
        Config {
            analysis_ms: 200,
            ..Config::default()
        },
        None,
    );
    s.sync(&song(vec![clip(3501, "d.wav", &audio, 0.0, 0.0, 1.0)]));
    let mut seen_analysing = false;
    until("the analysis", || {
        let c = s.state(3501);
        seen_analysing |= c.as_ref().is_some_and(|c| c.phase == Phase::Analysing);
        c.is_some_and(|c| c.phase == Phase::Ready)
    });
    assert!(seen_analysing);
    assert_eq!(s.state(3501).unwrap().progress, 1.0);
    let notes = s.notes(3501, false, WAIT);
    assert_eq!(notes.len(), 1);
    assert!((notes[0].volume - peak(&audio.channels[0])).abs() < 1e-4);
    assert!((notes[0].duration - 1.0).abs() < 1e-6);
    // The modification's note is a beat at the song's tempo, read by the plugin through the
    // host's content access.
    let beat = s.notes(3501, true, WAIT);
    assert!((beat[0].duration - 0.5).abs() < 1e-6);
    s.sync(&Song {
        bpm: 90.0,
        ..song(vec![clip(3501, "d.wav", &audio, 0.0, 0.0, 1.0)])
    });
    until("the new tempo", || {
        s.notes(3501, true, WAIT)
            .first()
            .is_some_and(|n| (n.duration - 60.0 / 90.0).abs() < 1e-6)
    });
    drop(s);
    no_violations();
}

/// Clips that go away are taken down in ARA's order; the rest keep playing.
#[test]
fn removing_a_clip_keeps_the_others() {
    let audio = tone(48000.0, 1.0, 0.5);
    let other = tone(48000.0, 1.0, 0.25);
    let s = start(Config::default(), None);
    s.sync(&song(vec![
        clip(5101, "j.wav", &audio, 0.0, 0.0, 1.0),
        clip(5102, "k.wav", &other, 1.0, 0.0, 1.0),
    ]));
    ready(&s, 5101);
    let kept = ready(&s, 5102);
    s.sync(&song(vec![clip(5102, "k.wav", &other, 1.0, 0.0, 1.0)]));
    std::thread::sleep(Duration::from_millis(150));
    assert!(Arc::ptr_eq(
        &kept,
        s.state(5102).unwrap().rendered.as_ref().unwrap()
    ));
    drop(s);
    no_violations();
}
