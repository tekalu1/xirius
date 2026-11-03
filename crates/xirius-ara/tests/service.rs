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
