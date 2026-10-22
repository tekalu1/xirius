//! Waveforms from real peaks (UI-037): the columns follow the sound, the offset and the zoom,
//! and the peaks come once the background has made them.

use std::time::{Duration, Instant};

use xirius_core::clipfx::{Fade, GainPoint};
use xirius_core::id::ClipId;
use xirius_core::id::TempoId;
use xirius_core::model::{ClipBody, TempoPoint};

use super::*;

const PPQ: u32 = 960;

/// A folder of the test's own, removed when dropped (also when the test fails).
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-wave-{name}-"))
        .tempdir()
        .unwrap()
}

/// One second at half scale, then one second of silence, at 48 kHz.
fn loud_then_quiet() -> Vec<f32> {
    (0..96_000)
        .map(|i| {
            if i < 48_000 {
                if i % 2 == 0 { 0.5 } else { -0.5 }
            } else {
                0.0
            }
        })
        .collect()
}

fn clip(start: i64, length: i64) -> Clip {
    Clip {
        id: ClipId(1),
        name: "Loop".into(),
        color: None,
        start,
        length,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: "Loop.wav".into(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    }
}

fn at_120() -> TempoMap {
    TempoMap::new(
        PPQ,
        &[TempoPoint {
            id: TempoId(2),
            tick: 0,
            bpm: 120.0,
            ramp: false,
        }],
    )
}

#[test]
fn the_columns_follow_the_sound_its_offset_and_the_zoom() {
    let x = loud_then_quiet();
    let peaks = Peaks::from_samples(48_000, &x, &x);
    let map = at_120();
    // 100 px a beat: a second (two beats at 120) is 200 px.
    let ppt = 100.0 / f64::from(PPQ);
    let c = clip(4 * i64::from(PPQ), 8 * i64::from(PPQ));
    let cols = columns(&peaks, &map, &c, 0, ppt, 0.0, 800.0);
    // Two seconds of sound and nothing after it.
    assert_eq!(cols.len(), 400);
    assert!(cols[..199].iter().all(|(_, p)| p.max == 64 && p.min == -64));
    assert!(cols[201..].iter().all(|(_, p)| p.max == 0 && p.min == 0));
    assert_eq!(cols[10].0, 10.0);

    // Half a second into the file: the loud part is half as long.
    let half = columns(&peaks, &map, &c, i64::from(PPQ), ppt, 0.0, 800.0);
    assert_eq!(half.len(), 300);
    assert!(half[..99].iter().all(|(_, p)| p.max == 64));
    assert!(half[101..].iter().all(|(_, p)| p.max == 0));

    // Only the columns asked for (those in the window).
    let some = columns(&peaks, &map, &c, 0, ppt, 150.0, 250.0);
    assert_eq!(some.first().unwrap().0, 150.0);
    assert_eq!(some.len(), 100);

    // Zoomed out, a column reads a coarse level and still sees the loud second.
    let far = 2.0 / f64::from(PPQ);
    let cols = columns(&peaks, &map, &c, 0, far, 0.0, 800.0);
    assert_eq!(cols.len(), 8);
    assert!(peaks.level_for(48_000.0 / 4.0) > 0);
    assert_eq!(cols[0].1.max, 64);
    assert_eq!(cols[7].1.max, 0);

    // At 60 BPM the same second is a beat: twice as many columns per second.
    let slow = TempoMap::new(
        PPQ,
        &[TempoPoint {
            id: TempoId(2),
            tick: 0,
            bpm: 60.0,
            ramp: false,
        }],
    );
    let cols = columns(&peaks, &slow, &c, 0, ppt, 0.0, 800.0);
    assert_eq!(cols.len(), 200);
    assert!(cols[..99].iter().all(|(_, p)| p.max == 64));
    assert!(cols[101..].iter().all(|(_, p)| p.max == 0));
}

#[test]
fn the_columns_follow_the_clips_speed_and_run_backwards_when_it_is_reversed() {
    let x = loud_then_quiet();
    let peaks = Peaks::from_samples(48_000, &x, &x);
    let map = at_120();
    let ppt = 100.0 / f64::from(PPQ);
    let with = |speed: f64, reverse: bool| {
        let mut c = clip(0, 8 * i64::from(PPQ));
        if let ClipBody::Audio { process, .. } = &mut c.body {
            process.speed = speed;
            process.reverse = reverse;
        }
        columns(&peaks, &map, &c, 0, ppt, 0.0, 800.0)
    };
    // Twice as fast: the two seconds play in one, the loud one in half a second.
    let fast = with(200.0, false);
    assert_eq!(fast.len(), 200);
    assert!(fast[..99].iter().all(|(_, p)| p.max == 64));
    assert!(fast[101..].iter().all(|(_, p)| p.max == 0));
    // Half as fast: four seconds.
    let slow = with(50.0, false);
    assert_eq!(slow.len(), 800);
    assert!(slow[..399].iter().all(|(_, p)| p.max == 64));
    assert!(slow[401..].iter().all(|(_, p)| p.max == 0));
    // Backwards: the silence first, then the loud second.
    let back = with(100.0, true);
    assert_eq!(back.len(), 400);
    assert!(back[..199].iter().all(|(_, p)| p.max == 0));
    assert!(back[201..].iter().all(|(_, p)| p.max == 64 && p.min == -64));
}

#[test]
fn peaks_are_made_in_the_background_once_and_read_from_the_peak_file_after() {
    let dir_tmp = folder("bg");
    let dir = dir_tmp.path().to_path_buf();
    let file = dir.join("Loop.wav");
    let x = loud_then_quiet();
    xirius_session::export::write_wav(&file, &x, &x, 48_000, 16).unwrap();
    let store = peaks::store_dir(Some(&dir));

    assert!(matches!(look(&file, &store), Look::Making));
    let start = Instant::now();
    while !poll() {
        assert!(start.elapsed() < Duration::from_secs(20), "no peaks came");
        std::thread::sleep(Duration::from_millis(5));
    }
    let Look::Ready(p) = look(&file, &store) else {
        panic!("not ready");
    };
    assert_eq!(p.frames, 96_000);
    assert!(store.read_dir().unwrap().count() >= 1);

    // Opened again: read from the peak file at once.
    forget();
    assert!(matches!(look(&file, &store), Look::Ready(_)));

    // A file that is not there is drawn with the stand-in; a relative one needs the folder.
    assert!(matches!(look(&dir.join("gone.wav"), &store), Look::Missing));
    assert_eq!(source(None, "Audio/x.wav"), None);
    assert_eq!(
        source(Some(&dir), "Audio/x.wav"),
        Some(dir.join("Audio/x.wav"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_clip_whose_peaks_are_being_made_is_a_change_waiting_to_land() {
    let dir_tmp = folder("waiting");
    let dir = dir_tmp.path().to_path_buf();
    let file = dir.join("Loop.wav");
    let x = loud_then_quiet();
    xirius_session::export::write_wav(&file, &x, &x, 48_000, 16).unwrap();
    let store = peaks::store_dir(Some(&dir));
    forget();
    assert!(!crate::wait::view().hatch);

    assert!(matches!(look(&file, &store), Look::Making));
    let waiting = crate::wait::view();
    assert!(waiting.hatch && waiting.line && !waiting.reflecting);

    let start = Instant::now();
    while !poll() {
        assert!(start.elapsed() < Duration::from_secs(20), "no peaks came");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(crate::wait::view(), crate::wait::View::default());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- zoomed in past the peaks (UI-310) ----------------------------------------------------

/// A rising ramp of `n` frames, both channels alike.
fn ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32 / n as f32).collect()
}

fn chunk_of(x: &[f32]) -> Vec<Arc<Samples>> {
    vec![Arc::new(Samples::from_channels(48_000, 0, x, x))]
}

/// Pixels a tick when a beat is `beat_w` pixels wide.
fn ppt(beat_w: f64) -> f64 {
    beat_w / f64::from(PPQ)
}

#[test]
fn a_zoom_draws_peaks_then_samples_then_a_line_then_dots() {
    assert_eq!(detail_for(100_000.0), Detail::Peaks);
    assert_eq!(detail_for(128.0), Detail::Peaks);
    assert_eq!(detail_for(127.9), Detail::Samples);
    assert_eq!(detail_for(1.0), Detail::Samples);
    assert_eq!(detail_for(0.99), Detail::Line { dots: false });
    assert_eq!(
        detail_for(1.0 / DOT_PX + 0.01),
        Detail::Line { dots: false }
    );
    assert_eq!(detail_for(1.0 / DOT_PX), Detail::Line { dots: true });
    assert_eq!(detail_for(1.0 / 6.0), Detail::Line { dots: true });

    // At 120 BPM and 48 kHz a beat holds 24,000 frames.
    let map = at_120();
    let c = clip(0, 8 * i64::from(PPQ));
    let at = |beat_w: f64| Place::new(&map, &c, 0, ppt(beat_w), 48_000, 96_000).frames_per_px(10.0);
    assert_eq!(detail_for(at(100.0)), Detail::Peaks);
    assert_eq!(detail_for(at(1_000.0)), Detail::Samples);
    assert_eq!(detail_for(at(48_000.0)), Detail::Line { dots: false });
    assert_eq!(detail_for(at(144_000.0)), Detail::Line { dots: true });
}

#[test]
fn zoomed_in_all_the_way_a_sample_is_six_pixels_at_any_tempo_and_rate() {
    let point = |bpm: f64| TempoPoint {
        id: TempoId(2),
        tick: 0,
        bpm,
        ramp: false,
    };
    // 24,000 samples a beat at 120 BPM and 48 kHz, each 6 px.
    assert_eq!(
        crate::studio::beat_w_max(48_000, &[point(120.0)]),
        144_000.0
    );
    assert_eq!(
        crate::studio::beat_w_max(44_100, &[point(120.0)]),
        132_300.0
    );
    assert_eq!(
        crate::studio::beat_w_max(96_000, &[point(120.0)]),
        288_000.0
    );
    // The slowest tempo of the song sets it, and 20 BPM at the least.
    assert_eq!(
        crate::studio::beat_w_max(48_000, &[point(120.0), point(60.0)]),
        288_000.0
    );
    assert_eq!(crate::studio::beat_w_max(48_000, &[point(5.0)]), 864_000.0);
    assert_eq!(crate::studio::beat_w_max(48_000, &[]), 144_000.0);
    // Never less than one beat over a 4K window.
    assert_eq!(
        crate::studio::beat_w_max(2_000, &[point(300.0)]),
        crate::studio::Studio::BEAT_W_MAX
    );

    let map = at_120();
    let c = clip(0, 8 * i64::from(PPQ));
    let place = Place::new(&map, &c, 0, ppt(144_000.0), 48_000, 96_000);
    assert!((place.frames_per_px(0.0) - 1.0 / 6.0).abs() < 1e-9);
    assert!((place.frames_per_px(500_000.0) - 1.0 / 6.0).abs() < 1e-9);
}

#[test]
fn zoomed_in_all_the_way_each_sample_is_a_point_six_pixels_from_the_last() {
    let x = loud_then_quiet();
    let chunks = chunk_of(&x[..4_096]);
    let map = at_120();
    let c = clip(0, 8 * i64::from(PPQ));
    let place = Place::new(&map, &c, 0, ppt(144_000.0), 48_000, 96_000);
    let pts = sample_points(&place, &chunks, 0.0, 60.0);
    // Frames 0 to 11, one either side of the view (none before frame 0).
    assert_eq!(pts.len(), 12);
    for (i, (x, s)) in pts.iter().enumerate() {
        assert!((x - 6.0 * i as f32).abs() < 1e-3, "{i}: {x}");
        let v = if i % 2 == 0 { 16_384 } else { -16_384 };
        assert_eq!((s.min, s.max), (v, v), "{i}");
    }
    // Further in, the same spacing from where the view starts.
    let pts = sample_points(&place, &chunks, 600.0, 660.0);
    assert!((pts[0].0 - 594.0).abs() < 1e-3);
    assert!(pts.windows(2).all(|w| (w[1].0 - w[0].0 - 6.0).abs() < 1e-3));
    // Without the chunk there is nothing to join (the peaks are drawn).
    assert!(sample_points(&place, &[], 0.0, 60.0).is_empty());
}

#[test]
fn a_clip_played_backwards_joins_its_samples_from_the_end() {
    let x = ramp(2_000);
    let chunks = chunk_of(&x);
    let map = at_120();
    let mut c = clip(0, 8 * i64::from(PPQ));
    if let ClipBody::Audio { process, .. } = &mut c.body {
        process.reverse = true;
    }
    let place = Place::new(&map, &c, 0, ppt(144_000.0), 48_000, 2_000);
    let pts = sample_points(&place, &chunks, 0.0, 30.0);
    assert_eq!(pts[0].1, chunks[0].at(1_999).unwrap());
    assert!((pts[0].0).abs() < 1e-3);
    assert!(pts.windows(2).all(|w| w[1].1.max < w[0].1.max));
    let forward = Place::new(
        &map,
        &clip(0, 8 * i64::from(PPQ)),
        0,
        ppt(144_000.0),
        48_000,
        2_000,
    );
    let ahead = sample_points(&forward, &chunks, 0.0, 30.0);
    assert_eq!(ahead[0].1, chunks[0].at(0).unwrap());
    assert!(ahead.windows(2).all(|w| w[1].1.max > w[0].1.max));
}

#[test]
fn columns_of_samples_are_the_frames_each_pixel_holds() {
    let x = loud_then_quiet();
    // 10 frames a pixel: a beat of 24,000 frames is 2,400 px.
    let map = at_120();
    let c = clip(0, 8 * i64::from(PPQ));
    let place = Place::new(&map, &c, 0, ppt(2_400.0), 48_000, 96_000);
    assert_eq!(detail_for(place.frames_per_px(0.0)), Detail::Samples);
    let chunks: Vec<Arc<Samples>> = [0usize, 65_536]
        .iter()
        .map(|&a| {
            let b = (a + 65_536).min(x.len());
            Arc::new(Samples::from_channels(48_000, a as u64, &x[a..b], &x[a..b]))
        })
        .collect();
    // The loud second is 4,800 px, then silence; a column across the two chunks reads both.
    let cols = sample_columns(&place, &chunks, 4_790.0, 4_810.0);
    assert_eq!(cols.len(), 20);
    assert!(
        cols[..10]
            .iter()
            .all(|(_, s)| (s.min, s.max) == (-16_384, 16_384))
    );
    assert!(cols[11..].iter().all(|(_, s)| (s.min, s.max) == (0, 0)));
    let across = sample_columns(&place, &chunks, 6_553.0, 6_554.0);
    assert_eq!(across[0].0, 6_553.0);
    assert_eq!((across[0].1.min, across[0].1.max), (0, 0));
    // Only the chunk in view: columns outside it are left out.
    let only = sample_columns(&place, &chunks[..1], 6_550.0, 6_560.0);
    assert!(only.iter().all(|(x, _)| *x < 6_554.0));
}

#[test]
fn the_height_is_the_gain_the_fades_and_the_gain_line_times_the_zoom() {
    let n = 8 * i64::from(PPQ);
    let flat = Height::flat(1.0, 0.0);
    assert_eq!(flat.at(0.0), 1.0);
    assert_eq!(Height::flat(4.0, 0.0).at(100.0), 4.0);

    let mut c = clip(0, n);
    if let ClipBody::Audio { gain_db, .. } = &mut c.body {
        *gain_db = -6.0;
    }
    let gain = Height::of_gain(&c, 2.0, 0.0);
    assert!((gain.at(50.0) - 2.0 * 0.501_187).abs() < 1e-4);

    let h = Height {
        gain_db: 0.0,
        fade_in: Fade {
            length: 960,
            curve: 0.0,
        },
        fade_out: Fade {
            length: 960,
            curve: 0.0,
        },
        points: vec![
            GainPoint {
                tick: 2_000,
                db: 0.0,
            },
            GainPoint {
                tick: 4_000,
                db: -20.0,
            },
        ],
        length: n,
        zoom: 1.0,
        left: 0.0,
    };
    // A straight fade: silent at the clip's edges, halfway up in its middle.
    assert_eq!(h.at(0.0), 0.0);
    assert!((h.at(480.0) - 0.5).abs() < 1e-6);
    assert!((h.at(1_500.0) - 1.0).abs() < 1e-6);
    // The gain line falls 20 dB to a tenth and stays there.
    assert!((h.at(3_000.0) - 10f32.powf(-0.5)).abs() < 1e-4);
    assert!((h.at(5_000.0) - 0.1).abs() < 1e-6);
    // The fade out under the gain line.
    assert!((h.at((n - 480) as f64) - 0.05).abs() < 1e-6);
    assert_eq!(h.at(n as f64), 0.0);
    let tall = Height {
        zoom: 4.0,
        ..h.clone()
    };
    assert!((tall.at(480.0) - 2.0).abs() < 1e-6);
}

#[test]
fn a_clip_in_a_track_stands_as_its_gain_and_fades_and_a_drag_shows_at_once() {
    use xirius_core::id::TrackId;
    use xirius_core::model::{Project, TrackKind};
    let mut project = Project::new("t");
    let mut c = clip(0, 8 * i64::from(PPQ));
    if let ClipBody::Audio { gain_db, .. } = &mut c.body {
        *gain_db = -6.0;
    }
    project.fx_mut(c.id).fade_in = Fade {
        length: 960,
        curve: 0.0,
    };
    let mut track = Track::new(TrackId(1), TrackKind::Audio, "Audio");
    track.clips.push(c.clone());
    let h = Height::shown(&project, &track, &c, &[], 1.0, 0.0);
    assert!((h.at(480.0) - 0.5 * 0.501_187).abs() < 1e-4);
    assert!((h.at(2_000.0) - 0.501_187).abs() < 1e-4);
    // A gain drag under way shows before it lands, and so does the vertical zoom.
    let edits = [(c.id, FxEdit::GainBy(6.0))];
    let dragged = Height::shown(&project, &track, &c, &edits, 2.0, 0.0);
    assert!((dragged.at(2_000.0) - 2.0).abs() < 1e-4);
}
