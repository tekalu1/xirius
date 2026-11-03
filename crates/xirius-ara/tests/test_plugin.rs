//! The host against the ARA SDK's `ARATestPlugIn` (VST3), PLG-031 to PLG-038.
//!
//! Needs the plugin built from the ARA SDK (`ARA_Examples`): set `XIRIUS_ARA_TEST_PLUGIN` to
//! its `ARATestPlugIn.vst3`. Without it the test says so and passes. Prints what it saw, for
//! the record of PLG-038.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_ara::service::{Loader, source_id};
use xirius_ara::vst3::Vst3Binary;
use xirius_ara::{ClipSpec, Phase, Service, Song, SourceAudio};

const FACTORY_ID: &str = "org.ara-audio.examples.testplugin.arafactory";
const ARCHIVE_ID: &str = "org.ara-audio.examples.testplugin.aradocumentarchive.version1";
const WAIT: Duration = Duration::from_secs(20);

fn plugin() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("XIRIUS_ARA_TEST_PLUGIN")?);
    path.exists().then_some(path)
}

fn loader(path: &std::path::Path) -> Loader {
    let path = path.to_path_buf();
    Box::new(move || {
        Ok(Box::new(Vst3Binary::load(&path, None)?) as Box<dyn xirius_ara::service::Binary>)
    })
}

/// Bursts of a tone with silence between, so the plugin finds notes.
fn bursts(rate: f64, seconds: f64) -> Arc<SourceAudio> {
    let n = (rate * seconds) as usize;
    let l: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 / rate;
            if t.fract() < 0.5 {
                0.5 * (t * 440.0 * std::f64::consts::TAU).sin() as f32
            } else {
                0.0
            }
        })
        .collect();
    Arc::new(SourceAudio::stereo(rate, l.clone(), l))
}

fn clip(key: u64, audio: &Arc<SourceAudio>, start: f64, offset: f64, duration: f64) -> ClipSpec {
    ClipSpec {
        key,
        clone_of: None,
        source: source_id("bursts.wav"),
        name: "bursts".into(),
        audio: audio.clone(),
        start,
        offset,
        duration,
    }
}

fn until(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ready(s: &Service, key: u64) -> Arc<xirius_ara::service::Rendered> {
    until("the render", || {
        if let Some(why) = s.crashed().or(s.failed()) {
            panic!("the plugin stopped: {why}");
        }
        s.state(key).is_some_and(|c| c.phase == Phase::Ready)
    });
    s.state(key).unwrap().rendered.unwrap()
}

#[test]
fn the_ara_test_plugin_passes_every_step() {
    let Some(path) = plugin() else {
        eprintln!("XIRIUS_ARA_TEST_PLUGIN is not set; skipped");
        return;
    };
    xirius_ara::contain::install_own_filter();
    let rate = 48000.0;
    let audio = bursts(rate, 4.0);
    let song = |clips| Song {
        rate,
        clips,
        ..Song::default()
    };

    // 1. Load, make the document, play a clip through its region (PLG-015, PLG-032, PLG-033).
    let s = Service::start(loader(&path), None, Arc::new(|| {}));
    let begun = Instant::now();
    s.sync(&song(vec![clip(1, &audio, 1.0, 0.5, 3.0)]));
    let r = ready(&s, 1);
    println!("step 1: analysed and rendered in {:?}", begun.elapsed());
    let info = s.info().unwrap();
    assert_eq!(info.factory_id, FACTORY_ID);
    assert_eq!(info.archive_id, ARCHIVE_ID);
    println!(
        "step 1: {} {} by {}",
        info.plugin_name, info.version, info.manufacturer
    );
    let from = (0.5 * rate) as usize;
    assert_eq!(r.left.len(), (3.0 * rate) as usize);
    let differs = (0..r.left.len())
        .filter(|i| (r.left[*i] - audio.channels[0][from + i]).abs() > 1e-6)
        .count();
    assert_eq!(differs, 0, "the plugin's render is not the region's audio");

    // 2. The analysis on the plugin's threads, its notes, the notices on the host's thread
    // (PLG-034, PLG-035).
    let notes = s.notes(1, false, WAIT);
    println!(
        "step 2: {} notes, the first {:?}",
        notes.len(),
        notes.first()
    );
    assert!(notes.len() >= 3, "{notes:?}");
    let p = s.probe(WAIT).unwrap();
    println!(
        "step 2: generation {}, calls {:?}, reads {} (off the host's thread {}), readers at once {}, notices {} (off thread {}), asserts {}",
        p.generation,
        p.calls,
        p.reads,
        p.reads_off_thread,
        p.most_readers,
        p.notices,
        p.notices_off_thread,
        p.asserts
    );
    assert!(p.generation >= xirius_ara::sys::API_GENERATION_2_0_FINAL);
    assert!(p.reads > 0 && p.reads_off_thread > 0);
    assert_eq!(p.notices_off_thread, 0);
    assert_eq!(p.asserts, 0, "the plugin asserted: {}", p.last_assert);

    // 3. Moving the clip keeps its sound (PLG-036).
    s.sync(&song(vec![clip(1, &audio, 2.0, 0.5, 3.0)]));
    std::thread::sleep(Duration::from_millis(300));
    let moved = s.state(1).unwrap();
    assert_eq!(moved.phase, Phase::Ready);
    assert!(Arc::ptr_eq(&r, moved.rendered.as_ref().unwrap()));
    println!("step 3: moved; the render was kept");

    // 4. Store, reopen, restore (PLG-037, ARC-025).
    let (bytes, id) = s.store(WAIT).expect("an archive");
    assert_eq!(id, ARCHIVE_ID);
    println!("step 4: archive of {} bytes", bytes.len());
    let calls = s.probe(WAIT).unwrap().calls;
    println!("step 4: calls per interface {calls:?}");
    drop(s);
    let s = Service::start(loader(&path), Some((bytes, id)), Arc::new(|| {}));
    s.sync(&song(vec![clip(1, &audio, 2.0, 0.5, 3.0)]));
    let again = ready(&s, 1);
    assert_eq!(again.left, r.left);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!s.take_dirty(), "opening the song made it look changed");
    let restored = s.notes(1, false, WAIT);
    assert_eq!(restored.len(), notes.len());
    println!(
        "step 4: restored, {} notes, not changed by opening",
        restored.len()
    );

    // 5. A new sample rate (PLG-047). The test plugin renders only at its source's rate, so
    // this checks the host's steps, not the sound.
    s.sync(&Song {
        rate: 44100.0,
        ..song(vec![clip(1, &audio, 2.0, 0.5, 3.0)])
    });
    until("the render at 44.1 kHz", || {
        s.state(1).is_some_and(|c| {
            c.phase == Phase::Ready && c.rendered.is_some_and(|r| r.rate == 44100.0)
        })
    });
    s.sync(&song(vec![clip(1, &audio, 2.0, 0.5, 3.0)]));
    until("the render at 48 kHz", || {
        s.state(1)
            .is_some_and(|c| c.phase == Phase::Ready && c.rendered.is_some_and(|r| r.rate == rate))
    });
    let back = ready(&s, 1);
    assert_eq!(back.left, r.left);
    println!("step 5: 48 -> 44.1 -> 48 kHz rendered again");

    // 6. An export waits for the analysis of a new clip (ENG-025).
    let other = bursts(rate, 8.0);
    let mut c2 = clip(2, &other, 6.0, 0.0, 8.0);
    c2.source = source_id("bursts-2.wav");
    s.sync(&song(vec![clip(1, &audio, 2.0, 0.5, 3.0), c2]));
    let begun = Instant::now();
    assert!(s.wait_idle(begun + Duration::from_secs(10)));
    println!("step 6: waited {:?} for the analysis", begun.elapsed());

    let p = s.probe(WAIT).unwrap();
    assert_eq!(p.asserts, 0, "the plugin asserted: {}", p.last_assert);
    drop(s);
    println!("step 7: the document was taken down without asserts");
}
