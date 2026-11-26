//! Fades, the gain line and crossfades reach the sound (EDT-039, EDT-040, EDT-017): the song is
//! mixed down to a file, as an export does, and every check compares the samples with the same
//! song before the edit. The source is a steady tone of 0.5, so a ratio of the two renders is the
//! gain the edit put on the clip. Nothing here opens an audio device.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::clipfx::fade_gain;
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, Options};
use xirius_session::ext::ExtConfig;
use xirius_session::host::{HostConfig, SessionHost};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-clipfx-test-{}-{n}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    commands::run(s, None, name, &map(args), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn write_steady(path: &Path, secs: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..48000 * secs {
        w.write_sample(16384i16).unwrap();
    }
    w.finalize().unwrap();
}

/// 120 bpm, 4/4, 960 ticks to the beat: a bar is 3840 ticks and 96 000 frames at 48 kHz, so a
/// tick is 25 frames.
const BAR: i64 = 3840;
const FRAMES_PER_TICK: usize = 25;

struct Song {
    dir: Dir,
    s: Session,
    track: String,
    /// The clips of the track, in order.
    clips: Vec<String>,
}

/// One audio track with one clip of `bars` bars.
fn song(tag: &str, bars: i64) -> Song {
    let dir = Dir::new(tag);
    write_steady(&dir.path().join("steady.wav"), (bars * 2) as u32 + 1);
    let mut s = Session::with_registry(Project::new(tag), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    run(&mut s, "tempo.set", json!({ "bpm": 120 }));
    let track = id_of(&run(&mut s, "track.add.audio", json!({ "name": "A" })));
    let clip = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({ "track": track, "at": 0, "bars": bars, "file": "steady.wav" }),
    ));
    Song {
        dir,
        s,
        track,
        clips: vec![clip],
    }
}

/// The left channel of the mixdown of the song as it is now.
fn mix(song: &Song, name: &str) -> Vec<f32> {
    let out = song.dir.path().join(format!("{name}.wav"));
    let o = Options {
        dir: Some(song.dir.path().to_path_buf()),
        bit_depth: Some(32),
        ..Options::default()
    };
    export::mixdown(song.s.project(), &out, &o).unwrap();
    let mut r = hound::WavReader::open(&out).unwrap();
    r.samples::<f32>()
        .map(|x| x.unwrap())
        .step_by(2)
        .collect::<Vec<f32>>()
}

/// What the edit did to the sound at `frame`: edited over plain.
fn ratio(edited: &[f32], plain: &[f32], frame: usize) -> f32 {
    assert!(plain[frame].abs() > 0.01, "silent at {frame}");
    edited[frame] / plain[frame]
}

fn near(a: f32, b: f32, tol: f32, what: &str) {
    assert!((a - b).abs() <= tol, "{what}: {a} vs {b}");
}

/// Reuses the running engine across edits, as the app does; no audio device is opened.
fn live_mix(
    host: &mut SessionHost,
    audio: &mut xirius_engine::AudioEngine,
    frames: usize,
) -> Vec<f32> {
    host.stop();
    audio.render(&mut [0.0; 512], 2);
    assert!(host.seek_frame(0));
    host.play();
    let mut out = vec![0.0; frames * 2];
    for block in out.chunks_mut(512) {
        audio.render(block, 2);
    }
    host.poll();
    out.into_iter().step_by(2).collect()
}

#[test]
fn repeated_fade_edits_and_undo_reach_the_running_engine() {
    for later in [false, true] {
        let mut song = song("live-fades", 1);
        let c = song.clips[0].clone();
        let other = id_of(&run(
            &mut song.s,
            "track.add.audio",
            json!({ "name": "Unchanged" }),
        ));
        run(
            &mut song.s,
            "clip.add.audio",
            json!({ "track": other, "at": 0, "bars": 1, "file": "steady.wav" }),
        );
        run(
            &mut song.s,
            "clip.fade.set",
            json!({ "ids": [c], "in": 480, "out": 480 }),
        );
        let (mut host, mut audio, warnings) = SessionHost::start(
            song.s.project(),
            HostConfig {
                dir: Some(song.dir.path().to_path_buf()),
                ..HostConfig::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        for (name, args) in [
            (
                "clip.fade.set",
                json!({ "ids": [c], "in": 960, "out": 1920 }),
            ),
            (
                "clip.fade.set",
                json!({ "ids": [c], "in_curve": 0.5, "out_curve": -0.5 }),
            ),
            (
                "clip.fade.set",
                json!({ "ids": [c], "in": 240, "out": 480 }),
            ),
            ("clip.fade.clear", json!({ "ids": [c] })),
            ("history.undo", json!({})),
            ("history.redo", json!({})),
        ] {
            run(&mut song.s, name, args);
            let report = if later {
                host.sync_later(song.s.project())
            } else {
                host.sync(song.s.project())
            };
            assert!(!report.swapped, "{name}: {report:?}");
            assert_eq!(report.deferred, 0, "{name}: {report:?}");
            let offline = mix(&song, "expected");
            let live = live_mix(&mut host, &mut audio, offline.len());
            let max_error = live
                .iter()
                .zip(&offline)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(
                max_error < 1e-5,
                "{name}, later={later}: sample error {max_error}"
            );
            assert_eq!(
                report.commands, 1,
                "{name}: only the edited track's timeline"
            );
            let again = host.sync(song.s.project());
            assert_eq!(again.commands, 0, "{name}: {again:?}");
        }
    }
}

#[test]
fn a_fade_edit_is_retried_after_a_full_command_queue() {
    let mut song = song("live-fade-retry", 1);
    let c = song.clips[0].clone();
    let (mut host, mut audio, _) = SessionHost::start(
        song.s.project(),
        HostConfig {
            dir: Some(song.dir.path().to_path_buf()),
            engine: xirius_engine::EngineConfig {
                command_capacity: 8,
                ..Default::default()
            },
            ..HostConfig::default()
        },
    )
    .unwrap();
    audio.render(&mut [0.0; 512], 2);
    while host.controller().send(xirius_engine::Command::Stop).is_ok() {}
    run(
        &mut song.s,
        "clip.fade.set",
        json!({ "ids": [c], "in": 1920 }),
    );
    assert!(host.sync_later(song.s.project()).deferred > 0);
    audio.render(&mut [0.0; 512], 2);
    let retry = host.sync_later(song.s.project());
    assert_eq!(
        (retry.commands, retry.deferred, retry.swapped),
        (1, 0, false)
    );
    let offline = mix(&song, "expected");
    let live = live_mix(&mut host, &mut audio, offline.len());
    assert!(live.iter().zip(offline).all(|(a, b)| (a - b).abs() < 1e-5));
    assert_eq!(host.sync_later(song.s.project()).commands, 0);
}

#[test]
fn a_fade_in_and_a_fade_out_shape_the_ends_of_the_export() {
    let mut song = song("fades", 1);
    let plain = mix(&song, "plain");
    let c = song.clips[0].clone();
    run(
        &mut song.s,
        "clip.fade.set",
        json!({ "ids": [c], "in": 960, "out": 1920 }),
    );
    let m = mix(&song, "faded");
    assert_eq!(m.len(), plain.len());

    // 960 ticks = 24 000 frames of fade-in, a line
    near(m[0], 0.0, 1e-4, "the very first frame");
    near(ratio(&m, &plain, 12_000), 0.5, 1e-3, "half way in");
    near(ratio(&m, &plain, 6_000), 0.25, 1e-3, "a quarter in");
    near(ratio(&m, &plain, 24_100), 1.0, 1e-4, "after the fade-in");
    // the middle is untouched
    near(ratio(&m, &plain, 40_000), 1.0, 1e-4, "the middle");
    // 1920 ticks = 48 000 frames of fade-out
    let end = 96_000;
    near(ratio(&m, &plain, end - 24_000), 0.5, 1e-3, "half way out");
    near(
        ratio(&m, &plain, end - 12_000),
        0.25,
        1e-3,
        "a quarter left",
    );
    assert!(m[end - 1].abs() < 0.01 * plain[end - 1].abs());
}

#[test]
fn the_curve_of_a_fade_is_the_one_the_core_computes() {
    let mut song = song("curves", 1);
    let plain = mix(&song, "plain");
    let c = song.clips[0].clone();
    for curve in [-1.0, -0.5, 0.0, 0.3, 0.5, 1.0] {
        run(
            &mut song.s,
            "clip.fade.set",
            json!({ "ids": [c], "in": 960, "curve": curve }),
        );
        let m = mix(&song, "curved");
        for k in 1..10 {
            let x = k as f64 / 10.0;
            let frame = (x * 24_000.0) as usize;
            near(
                ratio(&m, &plain, frame),
                fade_gain(x, curve) as f32,
                2e-3,
                &format!("curve {curve} at {x}"),
            );
        }
    }
    // equal power sits above the line, the other bend below it
    run(
        &mut song.s,
        "clip.fade.set",
        json!({ "ids": [c], "in": 960, "curve": 0.5 }),
    );
    let loud = mix(&song, "loud");
    near(
        ratio(&loud, &plain, 12_000),
        0.5f32.sqrt(),
        1e-3,
        "equal power",
    );
}

#[test]
fn the_clip_gain_and_its_line_scale_the_export() {
    let mut song = song("gain", 1);
    let plain = mix(&song, "plain");
    let c = song.clips[0].clone();
    run(
        &mut song.s,
        "clip.gain.set",
        json!({ "ids": [c], "db": -6.0 }),
    );
    let m = mix(&song, "quiet");
    let want = 10f32.powf(-6.0 / 20.0);
    near(ratio(&m, &plain, 10_000), want, 1e-3, "-6 dB");
    near(ratio(&m, &plain, 90_000), want, 1e-3, "-6 dB at the end");

    // a line over the clip: -12 dB at tick 960, 0 dB at tick 2880, moving in dB between them
    run(
        &mut song.s,
        "clip.gain.set",
        json!({ "ids": [c], "reset": true }),
    );
    run(
        &mut song.s,
        "clip.gain.point.add",
        json!({ "id": c, "at": 960, "db": -12.0 }),
    );
    run(
        &mut song.s,
        "clip.gain.point.add",
        json!({ "id": c, "at": 2880, "db": 0.0 }),
    );
    let m = mix(&song, "line");
    let db = |d: f32| 10f32.powf(d / 20.0);
    near(
        ratio(&m, &plain, 100),
        db(-12.0),
        1e-3,
        "held before the first point",
    );
    near(
        ratio(&m, &plain, 960 * FRAMES_PER_TICK),
        db(-12.0),
        1e-3,
        "at the first point",
    );
    near(
        ratio(&m, &plain, 1920 * FRAMES_PER_TICK),
        db(-6.0),
        1e-3,
        "half way",
    );
    near(
        ratio(&m, &plain, 2880 * FRAMES_PER_TICK),
        1.0,
        1e-3,
        "at the last point",
    );
    near(
        ratio(&m, &plain, 95_000),
        1.0,
        1e-3,
        "held after the last point",
    );

    // the gain and the line add up
    run(
        &mut song.s,
        "clip.gain.set",
        json!({ "ids": [c], "db": -6.0 }),
    );
    let m = mix(&song, "both");
    near(
        ratio(&m, &plain, 1920 * FRAMES_PER_TICK),
        db(-12.0),
        1e-3,
        "-6 dB and the line's -6 dB",
    );
}

/// A clip of two bars cut at the end of the first, so the right half has audio before its start
/// to cross into.
fn cut_song(tag: &str) -> Song {
    let mut song = song(tag, 2);
    let right = id_of(&run(
        &mut song.s,
        "clip.split",
        json!({ "ids": [song.clips[0]], "at": BAR }),
    ));
    song.clips.push(right);
    song
}

#[test]
fn a_crossfade_over_a_join_changes_the_export_around_the_join_only() {
    let mut song = cut_song("cross");
    let plain = mix(&song, "plain");
    let right = song.clips[1].clone();
    run(
        &mut song.s,
        "clip.fade.cross",
        json!({ "ids": [right], "length": 960 }),
    );
    let m = mix(&song, "crossed");
    let join = 96_000;
    // The two halves play the same steady tone; with an equal-power curve the sum of the two
    // gains, sqrt(x) + sqrt(1 - x), is 1 at the ends of the overlap and 1.414 in its middle.
    near(
        ratio(&m, &plain, join),
        2f32.sqrt(),
        5e-3,
        "the middle of the crossfade",
    );
    for k in 1..10 {
        let x = k as f32 / 10.0;
        let frame = join - 12_000 + (x * 24_000.0) as usize;
        near(
            ratio(&m, &plain, frame),
            x.sqrt() + (1.0 - x).sqrt(),
            5e-3,
            &format!("{x} of the way"),
        );
    }
    near(
        ratio(&m, &plain, join - 20_000),
        1.0,
        1e-4,
        "before the crossfade",
    );
    near(
        ratio(&m, &plain, join + 20_000),
        1.0,
        1e-4,
        "after the crossfade",
    );
    assert_eq!(m.len(), plain.len());

    // a line crossfade keeps the level of a steady tone
    run(
        &mut song.s,
        "clip.fade.cross",
        json!({ "ids": [right], "curve": 0.0 }),
    );
    let m = mix(&song, "line");
    for k in 1..10 {
        let frame = join - 12_000 + k * 2_400;
        near(ratio(&m, &plain, frame), 1.0, 5e-3, "a line crossfade");
    }

    // taking it off leaves the clips where they are: the overlap now plays both, with no fade
    run(
        &mut song.s,
        "clip.fade.cross",
        json!({ "ids": [right], "remove": true }),
    );
    let bare = mix(&song, "bare");
    near(
        ratio(&bare, &plain, join),
        2.0,
        1e-4,
        "the overlap, uncrossed",
    );
    near(
        ratio(&bare, &plain, join - 20_000),
        1.0,
        1e-4,
        "outside the overlap",
    );
    // and undoing that brings the line crossfade back
    run(&mut song.s, "history.undo", json!({}));
    near(
        ratio(&mix(&song, "again"), &plain, join),
        1.0,
        5e-3,
        "undone",
    );
}

#[test]
fn undo_puts_the_sound_back() {
    let mut song = song("undo", 1);
    let plain = mix(&song, "plain");
    let c = song.clips[0].clone();
    run(
        &mut song.s,
        "clip.fade.set",
        json!({ "ids": [c], "in": 960 }),
    );
    assert!(mix(&song, "faded")[100] < plain[100] * 0.5);
    run(&mut song.s, "history.undo", json!({}));
    assert_eq!(mix(&song, "undone"), plain);
    let _ = &song.track;
}
