//! The ARA host against the fake plugin (`xirius_ara::fake`), which checks ARA's rules.
//!
//! The fake's edits and rule checks are per process: every test uses its own clip keys.

use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_ara::fake::{self, Config};
use xirius_ara::host::Request;
use xirius_ara::service::{Placement, modification_id, source_id};
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
        regions: Vec::new(),
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
    assert!(s.close(Instant::now() + WAIT));
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
    assert!(s.close(Instant::now() + WAIT));
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
    assert!(s.close(Instant::now() + WAIT));
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
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// PLG-036: moving a clip keeps its sound: the render stays, nothing is made again.
#[test]
fn moving_a_region_keeps_its_sound() {
    let audio = tone(48000.0, 1.0, 0.5);
    let s = start(Config::default(), None);
    s.sync(&song(vec![clip(3601, "e.wav", &audio, 0.0, 0.0, 1.0)]));
    let before = ready(&s, 3601);
    let generation = s.generation();
    for step in 1..=5 {
        s.sync(&song(vec![clip(
            3601,
            "e.wav",
            &audio,
            step as f64 * 0.1,
            0.0,
            1.0,
        )]));
        let c = s.state(3601).unwrap();
        assert_eq!(c.phase, Phase::Ready);
        assert!(c.rendered.is_some());
    }
    std::thread::sleep(Duration::from_millis(200));
    let after = s.state(3601).unwrap();
    assert_eq!(after.phase, Phase::Ready);
    assert!(Arc::ptr_eq(&before, after.rendered.as_ref().unwrap()));
    assert!(s.generation() >= generation);
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// PLG-037 and ARC-025: the plugin's edits come back after saving and opening, a cloned
/// modification keeps them, and opening alone is not a change.
#[test]
fn edits_survive_saving_and_opening_and_a_clone_keeps_them() {
    let audio = tone(48000.0, 1.0, 0.5);
    let a = clip(3701, "f.wav", &audio, 0.0, 0.0, 1.0);
    let s = start(Config::default(), None);
    s.sync(&song(vec![a.clone()]));
    let full = peak(&ready(&s, 3701).left);
    fake::edit(&modification_id(3701), 0.5);
    until("the edit", || s.take_dirty());
    until("the edited render", || {
        s.state(3701)
            .and_then(|c| c.rendered)
            .is_some_and(|r| (peak(&r.left) - full * 0.5).abs() < 1e-4)
    });
    let (bytes, id) = s.store(WAIT).unwrap();
    assert_eq!(id, fake::ARCHIVE_ID);
    assert!(s.close(Instant::now() + WAIT));

    let s = start(Config::default(), Some((bytes.clone(), id.clone())));
    // Before any clip, the archive is kept as it was given.
    assert_eq!(s.store(WAIT).unwrap(), (bytes, id));
    s.sync(&song(vec![a.clone()]));
    until("the restored render", || {
        s.state(3701)
            .filter(|c| c.phase == Phase::Ready)
            .and_then(|c| c.rendered)
            .is_some_and(|r| (peak(&r.left) - full * 0.5).abs() < 1e-4)
    });
    // Duplicating the clip clones its modification with the edit.
    let mut b = clip(3702, "f.wav", &audio, 2.0, 0.0, 1.0);
    b.clone_of = Some(3701);
    s.sync(&song(vec![a, b]));
    until("the clone's render", || {
        [3701, 3702].iter().all(|k| {
            s.state(*k)
                .filter(|c| c.phase == Phase::Ready)
                .and_then(|c| c.rendered)
                .is_some_and(|r| (peak(&r.left) - full * 0.5).abs() < 1e-4)
        })
    });
    let volume = peak(&audio.channels[0]) * 0.5;
    assert!((s.notes(3702, true, WAIT)[0].volume - volume).abs() < 1e-4);
    std::thread::sleep(Duration::from_millis(200));
    assert!(!s.take_dirty(), "opening the song made it look changed");
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// PLG-047: each clip renders at its source's rate, whatever the song's, so a new song rate
/// leaves the document and the user's edits alone (the caller converts).
#[test]
fn a_clip_renders_at_its_source_rate_with_its_edits() {
    let slow = tone(44100.0, 1.0, 0.5);
    let fast = tone(48000.0, 1.0, 0.5);
    let s = start(Config::default(), None);
    s.sync(&song(vec![
        clip(4701, "g.wav", &slow, 0.0, 0.0, 1.0),
        clip(4702, "g2.wav", &fast, 1.0, 0.0, 1.0),
    ]));
    let first = ready(&s, 4702);
    fake::edit(&modification_id(4701), 0.5);
    let full = peak(&slow.channels[0]);
    until("the edited render", || {
        s.state(4701)
            .filter(|c| c.phase == Phase::Ready)
            .and_then(|c| c.rendered)
            .is_some_and(|r| (peak(&r.left) - full * 0.5).abs() < 1e-4)
    });
    let r = s.state(4701).unwrap().rendered.unwrap();
    assert_eq!((r.rate, r.left.len()), (44100.0, 44100));
    assert!(Arc::ptr_eq(
        &first,
        &s.state(4702).unwrap().rendered.unwrap()
    ));
    assert_eq!(first.rate, 48000.0);
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// PLG-034: a wait right after a change does not end on the render from before it.
#[test]
fn waiting_for_idle_after_a_change_ends_on_the_new_render() {
    let audio = tone(48000.0, 2.0, 0.5);
    let s = start(Config::default(), None);
    s.sync(&song(vec![clip(4801, "w.wav", &audio, 0.0, 0.0, 2.0)]));
    ready(&s, 4801);
    s.sync(&song(vec![clip(4801, "w.wav", &audio, 0.0, 0.25, 0.5)]));
    assert!(s.wait_idle(Instant::now() + WAIT));
    let c = s.state(4801).unwrap();
    assert_eq!(c.phase, Phase::Ready);
    let r = c.rendered.unwrap();
    assert_eq!((r.offset, r.duration), (0.25, 0.5));
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// A long clip renders a second at a time, so the ARA thread answers in between.
#[test]
fn a_long_render_still_answers() {
    let audio = tone(48000.0, 10.0, 0.5);
    let s = start(
        Config {
            render_spin_us: 1000,
            ..Config::default()
        },
        None,
    );
    let begun = Instant::now();
    s.sync(&song(vec![clip(4901, "long.wav", &audio, 0.0, 0.0, 10.0)]));
    let mut slowest = Duration::ZERO;
    let mut answered = 0;
    while s.state(4901).is_none_or(|c| c.phase != Phase::Ready) {
        let asked = Instant::now();
        assert!(s.probe(WAIT).is_some());
        slowest = slowest.max(asked.elapsed());
        answered += 1;
        assert!(begun.elapsed() < WAIT, "the render did not end");
    }
    // About 940 blocks of a millisecond: the whole render takes most of a second, a turn a
    // tenth of one.
    assert!(
        begun.elapsed() >= Duration::from_millis(800),
        "{:?}",
        begun.elapsed()
    );
    assert!(answered >= 3, "{answered}");
    assert!(slowest < Duration::from_millis(400), "{slowest:?}");
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// ENG-025: an export waits for the analysis.
#[test]
fn waiting_for_idle_ends_after_the_analysis() {
    let audio = tone(48000.0, 2.0, 0.5);
    let s = start(
        Config {
            analysis_ms: 300,
            ..Config::default()
        },
        None,
    );
    s.sync(&song(vec![
        clip(2501, "h.wav", &audio, 0.0, 0.0, 2.0),
        clip(2502, "i.wav", &audio, 3.0, 0.0, 2.0),
    ]));
    let begun = Instant::now();
    assert!(s.wait_idle(begun + WAIT));
    assert!(begun.elapsed() >= Duration::from_millis(100));
    assert!(s.states().values().all(|c| c.phase == Phase::Ready));
    assert!(s.close(Instant::now() + WAIT));
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
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// Two documents of one plugin on its one thread: both are edited and rendered, and one
/// leaving does not stop the other.
#[test]
fn documents_of_one_plugin_share_its_thread() {
    let audio = tone(48000.0, 1.0, 0.5);
    let shared =
        |config| Service::start_shared("fake:two", fake::loader(config), None, Arc::new(|| {}));
    let a = shared(Config {
        analysis_ms: 20,
        ..Config::default()
    });
    let b = shared(Config::default());
    a.sync(&song(vec![clip(5201, "l.wav", &audio, 0.0, 0.0, 1.0)]));
    b.sync(&song(vec![clip(5202, "l.wav", &audio, 0.0, 0.0, 1.0)]));
    ready(&a, 5201);
    ready(&b, 5202);
    assert_eq!(
        a.probe(WAIT).unwrap().info.plugin_name,
        b.probe(WAIT).unwrap().info.plugin_name
    );
    drop(a);
    b.sync(&song(vec![clip(5202, "l.wav", &audio, 0.5, 0.0, 1.0)]));
    std::thread::sleep(Duration::from_millis(100));
    assert!(b.crashed().is_none() && b.failed().is_none());
    assert!(b.state(5202).unwrap().rendered.is_some());
    drop(b);
    no_violations();
}

/// An archive of a kind the factory does not read is never handed to the plugin.
#[test]
fn an_archive_of_another_kind_is_not_restored() {
    let audio = tone(48000.0, 1.0, 0.5);
    let s = start(
        Config::default(),
        Some((vec![1, 2, 3], "com.example.other.document".into())),
    );
    s.sync(&song(vec![clip(5201, "k.wav", &audio, 0.0, 0.0, 1.0)]));
    until("the refusal", || s.failed().is_some());
    assert!(
        s.failed().unwrap().contains("another kind"),
        "{:?}",
        s.failed()
    );
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// A first song without clips (its files still being read) keeps the archive for the clips that
/// come after.
#[test]
fn an_archive_waits_for_the_first_clips() {
    let audio = tone(48000.0, 1.0, 0.5);
    let s = start(
        Config::default(),
        Some((vec![1, 2, 3], "com.example.other.document".into())),
    );
    s.sync(&song(Vec::new()));
    s.wait_idle(Instant::now() + WAIT);
    assert!(s.failed().is_none());
    s.sync(&song(vec![clip(5202, "k.wav", &audio, 0.0, 0.0, 1.0)]));
    // Handed over (and refused, being of another kind) only now.
    until("the archive", || s.failed().is_some());
    assert!(s.close(Instant::now() + WAIT));
    no_violations();
}

/// PLG-036: the plugin's editor opens with the clips shown in it, one window per document; a
/// second open raises it and shows the new clips, and its close box or the document's end takes
/// it down. The only test here that opens editors (the fake's windows are per process).
#[test]
fn the_editor_opens_once_per_document_and_closes_by_its_close_box() {
    let s = start(Config::default(), None);
    let a = tone(48_000.0, 1.0, 0.5);
    s.sync(&song(vec![
        clip(9101, "editor.wav", &a, 0.0, 0.0, 0.5),
        clip(9102, "editor.wav", &a, 0.5, 0.5, 0.5),
    ]));
    ready(&s, 9101);
    let mine = || {
        fake::windows()
            .iter()
            .filter(|t| *t == "editor test")
            .count()
    };
    assert!(!s.editing());
    s.open_editor("editor test", Placement::default(), &[9101, 9102], WAIT)
        .unwrap();
    assert!(s.editing());
    assert_eq!(mine(), 1);
    assert_eq!(fake::selection(), 2);
    s.open_editor("editor test", Placement::default(), &[9102], WAIT)
        .unwrap();
    assert_eq!(mine(), 1, "a second open raises the window it has");
    assert_eq!(fake::selection(), 1);
    fake::close_windows();
    until("the editor taken down", || !s.editing());
    assert_eq!(mine(), 0);
    s.open_editor("editor test", Placement::default(), &[9101], WAIT)
        .unwrap();
    assert!(s.close(Instant::now() + WAIT));
    assert_eq!(mine(), 0, "the document's end closes its editor");
    no_violations();
}

#[test]
fn changing_the_full_musical_map_sends_one_notification() {
    let audio = tone(48000.0, 1.0, 0.5);
    let service = start(Config::default(), None);
    let mut song = song(vec![clip(9001, "tempo-map.wav", &audio, 0.0, 0.0, 1.0)]);
    service.sync(&song);
    let before = ready(&service, 9001);
    song.tempos = vec![(0.0, 0.0), (2.0, 4.0), (6.0, 8.0), (6.25, 9.0)];
    song.bars = vec![(0.0, 4, 4), (4.0, 3, 4)];
    service.sync(&song);
    assert!(service.wait_idle(Instant::now() + WAIT));
    assert_eq!(service.probe(WAIT).unwrap().musical_updates, 1);
    assert_eq!(ready(&service, 9001).left, before.left);
    service.sync(&song);
    assert!(service.wait_idle(Instant::now() + WAIT));
    assert_eq!(service.probe(WAIT).unwrap().musical_updates, 1);
    assert!(service.close(Instant::now() + WAIT));
    no_violations();
}

#[test]
fn loop_regions_share_edits_and_resize_without_preparing_again() {
    use xirius_ara::document::RegionProps;
    let audio = Arc::new(SourceAudio::stereo(
        48000.0,
        vec![0.4; 96000],
        vec![0.2; 96000],
    ));
    let service = start(Config::default(), None);
    let mut spec = clip(9002, "loop.wav", &audio, 0.0, 0.25, 2.25);
    let set = |spec: &mut ClipSpec, count: usize| {
        spec.duration = (count - 1) as f64 * 0.5 + 0.25;
        spec.regions = (0..count)
            .map(|i| RegionProps {
                start_in_modification: 0.25,
                duration_in_modification: if i + 1 == count { 0.25 } else { 0.5 },
                start_in_playback: i as f64 * 0.5,
                duration_in_playback: if i + 1 == count { 0.25 } else { 0.5 },
            })
            .collect();
    };
    set(&mut spec, 5);
    service.sync(&song(vec![spec.clone()]));
    ready(&service, 9002);
    fake::edit(&modification_id(9002), 0.5);
    until("the plugin edit", || service.take_dirty());
    let edited = ready(&service, 9002);
    for i in (20..edited.left.len()).step_by(97) {
        assert!((edited.left[i] - 0.2).abs() < 1e-6, "frame {i}");
    }
    for count in [9, 2, 5, 1, 5] {
        set(&mut spec, count);
        service.sync(&song(vec![spec.clone()]));
        assert!(service.wait_idle(Instant::now() + WAIT));
        let rendered = ready(&service, 9002);
        assert_eq!(
            rendered.left.len(),
            (spec.duration * 48000.0).ceil() as usize
        );
        assert!((rendered.left[100] - 0.2).abs() < 1e-6);
        let probe = service.probe(WAIT).unwrap();
        assert_eq!(probe.regions, count);
        assert_eq!(probe.preparations, 1);
    }
    assert!(service.close(Instant::now() + WAIT));
    no_violations();
}
