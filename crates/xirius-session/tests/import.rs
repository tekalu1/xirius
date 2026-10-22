//! `clip.import`: an audio file read, placed as one step and played at the song's rate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::model::{ClipBody, Project, TrackKind};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, write_wav};
use xirius_session::ext::ExtConfig;
use xirius_session::graph::Select;
use xirius_session::tempo::TempoMap;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-import-test-{}-{n}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn song() -> Session {
    let mut s = Session::with_registry(Project::new("import"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    s
}

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, String> {
    commands::run(s, None, name, &map(args), &RunOptions::human()).map_err(|e| e.message)
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    try_run(s, name, args.clone()).unwrap_or_else(|e| panic!("{name} {args}: {e}"))
}

fn sine(path: &Path, rate: u32, secs: f64, amp: f32) {
    let n = (f64::from(rate) * secs).round() as usize;
    let x: Vec<f32> = (0..n)
        .map(|i| amp * (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin())
        .collect();
    write_wav(path, &x, &x, rate, 24).unwrap();
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

fn audio_clips(p: &Project) -> Vec<(String, String, i64, i64)> {
    let mut out = Vec::new();
    for t in p.tracks.values() {
        for c in &t.clips {
            if let ClipBody::Audio { file, .. } = &c.body {
                out.push((t.name.clone(), file.clone(), c.start, c.length));
            }
        }
    }
    out
}

fn bpm(p: &Project) -> f64 {
    p.tempo
        .iter()
        .find(|t| t.tick == 0)
        .map_or(120.0, |t| t.bpm)
}

#[test]
fn a_file_at_another_rate_is_placed_on_a_new_track_plays_and_undoes_in_one_step() {
    let d = Dir::new("rate");
    let file = d.0.join("Tone.wav");
    sine(&file, 44_100, 1.0, 0.5);
    let mut s = song();
    let ppq = i64::from(s.project().ppq);
    let r = run(
        &mut s,
        "clip.import",
        json!({ "file": file.to_str().unwrap() }),
    );
    assert!(
        r.message.contains("「Tone.wav」をインポートしました"),
        "{}",
        r.message
    );
    assert!(
        r.message.contains("トラック「Tone」を作成し"),
        "{}",
        r.message
    );
    assert_eq!(r.data["rate"], 44_100);

    // Unsaved song: the full path. 1 s at 120 BPM is two beats.
    let p = s.project().clone();
    let clips = audio_clips(&p);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].0, "Tone");
    assert_eq!(Path::new(&clips[0].1), file);
    assert_eq!((clips[0].2, clips[0].3), (0, 2 * ppq));
    let track = p.tracks.values().find(|t| t.name == "Tone").unwrap();
    assert_eq!(track.kind, TrackKind::Audio);

    // Played at the song's 48 kHz: the second of sound, at its level, then silence.
    let rate = p.settings.sample_rate;
    let mut b = Builder::new(rate, None);
    let end = TempoMap::from_project(&p).frame_at(4 * ppq, f64::from(rate));
    let out = export::render(&mut b, &p, Select::All, 0, end, 0).unwrap();
    let r48 = rate as usize;
    assert!((rms(&out.left[r48 / 10..r48 * 9 / 10]) - 0.5 / 2f64.sqrt()).abs() < 0.03);
    assert!(out.left[r48 + r48 / 20..].iter().all(|x| x.abs() < 1e-6));

    // One undo takes the track and the clip away together.
    s.undo(1).unwrap();
    assert!(audio_clips(s.project()).is_empty());
    assert!(!s.project().tracks.values().any(|t| t.name == "Tone"));
    s.redo(1).unwrap();
    assert_eq!(audio_clips(s.project()).len(), 1);
}

#[test]
fn a_loop_sets_the_tempo_of_an_empty_song_and_not_of_one_with_clips() {
    let d = Dir::new("tempo");
    let drums = d.0.join("Drums 92bpm.wav");
    sine(&drums, 48_000, 60.0 / 92.0 * 4.0, 0.2);
    let mut s = song();
    let ppq = i64::from(s.project().ppq);
    let r = run(
        &mut s,
        "clip.import",
        json!({ "file": drums.to_str().unwrap() }),
    );
    assert!(
        r.message.contains("テンポを 92 BPM に設定しました"),
        "{}",
        r.message
    );
    assert_eq!(bpm(s.project()), 92.0);
    // One bar of 92 BPM is one bar of the song.
    assert_eq!(audio_clips(s.project())[0].3, 4 * ppq);
    // The tempo and the clip are one step.
    s.undo(1).unwrap();
    assert_eq!(bpm(s.project()), 120.0);
    assert!(audio_clips(s.project()).is_empty());
    s.redo(1).unwrap();

    // The song has a clip now: a loop at 140 keeps the song at 92, and says so.
    let fast = d.0.join("Hat 140 BPM.wav");
    sine(&fast, 48_000, 1.0, 0.2);
    let r = run(
        &mut s,
        "clip.import",
        json!({ "file": fast.to_str().unwrap(), "at": "2.1.0" }),
    );
    assert!(
        r.message.contains("ファイルのテンポは 140 BPM です"),
        "{}",
        r.message
    );
    assert_eq!(bpm(s.project()), 92.0);
    let hat = audio_clips(s.project())
        .into_iter()
        .find(|c| c.0 == "Hat 140 BPM")
        .unwrap();
    assert_eq!(hat.2, 4 * ppq);
    // 1 s at 92 BPM.
    assert_eq!(hat.3, (92.0 / 60.0 * ppq as f64).round() as i64);
    // The clip knows the file's tempo, and plays as it sounds until it follows the song's.
    let process = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.clips)
        .find(|c| c.name == "Hat 140 BPM")
        .map(|c| match &c.body {
            ClipBody::Audio { process, .. } => *process,
            ClipBody::Midi { .. } => unreachable!(),
        })
        .unwrap();
    assert_eq!((process.tempo, process.follow), (Some(140.0), false));

    // --tempo file applies it anyway; --tempo keep never does.
    run(
        &mut s,
        "clip.import",
        json!({ "file": fast.to_str().unwrap(), "tempo": "file" }),
    );
    assert_eq!(bpm(s.project()), 140.0);
    let mut e = song();
    run(
        &mut e,
        "clip.import",
        json!({ "file": drums.to_str().unwrap(), "tempo": "keep" }),
    );
    assert_eq!(bpm(e.project()), 120.0);
}

#[test]
fn in_a_saved_song_a_file_in_its_folder_is_referred_to_from_it_and_tracks_are_checked() {
    let d = Dir::new("saved");
    let song_dir = d.0.join("Song");
    let mut s = song();
    s.save_as(&song_dir).unwrap();
    std::fs::create_dir_all(song_dir.join("Samples")).unwrap();
    let inside = song_dir.join("Samples").join("Kick.wav");
    sine(&inside, 48_000, 0.25, 0.5);
    let outside = d.0.join("Vox.wav");
    sine(&outside, 48_000, 0.5, 0.5);

    let audio = run(&mut s, "track.add.audio", json!({ "name": "Beat" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = run(
        &mut s,
        "clip.import",
        json!({ "file": inside.to_str().unwrap(), "track": audio, "at": 960 }),
    );
    assert!(
        r.message.contains("トラック「Beat」に「Kick」を置きました"),
        "{}",
        r.message
    );
    // A relative path is in the song folder.
    run(
        &mut s,
        "clip.import",
        json!({ "file": "Samples/Kick.wav", "track": audio }),
    );
    run(
        &mut s,
        "clip.import",
        json!({ "file": outside.to_str().unwrap() }),
    );
    let clips = audio_clips(s.project());
    let files: Vec<&str> = clips.iter().map(|c| c.1.as_str()).collect();
    assert_eq!(files[..2], ["Samples/Kick.wav", "Samples/Kick.wav"]);
    // A file from outside the saved song is copied in as it is read (BRW-004), and stays
    // where it was too.
    assert_eq!(files[2], "Media/Imports/Vox.wav");
    assert_eq!(
        std::fs::read(song_dir.join("Media/Imports/Vox.wav")).unwrap(),
        std::fs::read(&outside).unwrap()
    );
    assert!(outside.exists());

    let before = s.project().clone();
    let midi = run(&mut s, "track.add.instrument", json!({ "name": "Keys" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let n = s.project().next_id;
    let e = try_run(
        &mut s,
        "clip.import",
        json!({ "file": outside.to_str().unwrap(), "track": midi }),
    )
    .unwrap_err();
    assert!(e.contains("オーディオトラックにのみ"), "{e}");
    let txt = d.0.join("notes.txt");
    std::fs::write(&txt, "x").unwrap();
    let e = try_run(
        &mut s,
        "clip.import",
        json!({ "file": txt.to_str().unwrap() }),
    )
    .unwrap_err();
    assert!(e.contains("WAV・AIFF・FLAC"), "{e}");
    let e = try_run(&mut s, "clip.import", json!({ "file": "gone.wav" })).unwrap_err();
    assert!(e.contains("見つかりません"), "{e}");
    assert_eq!(s.project().next_id, n);
    assert_eq!(audio_clips(s.project()), audio_clips(&before));

    // The waveforms' peaks were made meanwhile, in the song's Peaks/.
    xirius_session::peaks::wait();
    let store = xirius_session::peaks::store_dir(Some(&song_dir));
    assert_eq!(store, song_dir.join("Peaks"));
    let kick = xirius_session::peaks::load(&inside, &store).expect("the kick's peaks");
    assert_eq!(kick.frames, 12_000);
    // The outside file's are the copy's, which the clip plays.
    let copy = song_dir.join("Media/Imports/Vox.wav");
    assert!(xirius_session::peaks::load(&copy, &store).is_some());
}

#[test]
fn a_dry_run_reads_the_file_and_changes_nothing() {
    let d = Dir::new("dry");
    let f = d.0.join("Pad.wav");
    sine(&f, 48_000, 0.5, 0.5);
    let mut s = song();
    let raw = map(json!({ "file": f.to_str().unwrap() }));
    let r = commands::run(
        &mut s,
        None,
        "clip.import",
        &raw,
        &RunOptions::human().dry(),
    )
    .unwrap();
    assert!(r.dry_run);
    assert!(s.project().tracks.is_empty() || audio_clips(s.project()).is_empty());
    let unsaved = try_run(&mut s, "clip.import", json!({ "file": "Pad.wav" })).unwrap_err();
    assert!(unsaved.contains("絶対パス"), "{unsaved}");
}
