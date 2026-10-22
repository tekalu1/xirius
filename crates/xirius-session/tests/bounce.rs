//! `clip.bounce`, `range.bounce` and `track.bounce` (EDT-015, EXP-003), and `track.freeze`
//! (EXP-004): the song sounds the same after a bounce or a freeze, one use is one step of the
//! history, and the files go into the song's folder.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{ClipBody, Project, TrackKind};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, write_wav};
use xirius_session::ext::ExtConfig;
use xirius_session::graph::Select;

const RATE: u32 = 48_000;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p =
            std::env::temp_dir().join(format!("xirius-bounce-{}-{n}-{tag}", std::process::id()));
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

/// A song saved into `dir/song`, so the bounces land in `dir/song/Media/Bounces`.
fn song(dir: &Dir) -> Session {
    let mut s = Session::with_registry(Project::new("bounce"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    s.save_as(dir.0.join("song")).unwrap();
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

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// Two seconds of a tone that rises from 220 Hz, louder towards the end.
fn tone(path: &Path) {
    let x: Vec<f32> = (0..2 * RATE as usize)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            0.2 * (1.0 + t) * (t * (220.0 + 60.0 * t) * std::f32::consts::TAU).sin()
        })
        .collect();
    write_wav(path, &x, &x, RATE, 32).unwrap();
}

fn audio_track(s: &mut Session, name: &str) -> String {
    id_of(&run(s, "track.add.audio", json!({ "name": name })))
}

fn audio_clip(s: &mut Session, track: &str, file: &Path, at: i64, beats: i64) -> String {
    id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": track, "at": at, "length": beats * 960, "file": file.to_string_lossy()}),
    ))
}

fn add(s: &mut Session, track: &str, name: &str) -> String {
    id_of(&run(
        s,
        "plugin.add",
        json!({ "track": track, "name": name }),
    ))
}

fn delay(s: &mut Session, track: &str) {
    let d = add(s, track, "Delay");
    for (param, plain) in [("mix", "50"), ("sync", "off"), ("time", "250")] {
        run(
            s,
            "plugin.param.set",
            json!({ "id": d, "param": param, "plain": plain }),
        );
    }
}

/// The whole song offline, through `seconds`.
fn render(s: &Session, seconds: f64) -> (Vec<f32>, Vec<f32>) {
    let p = s.project();
    let mut b = Builder::new(p.settings.sample_rate, s.dir().map(Path::to_path_buf));
    let to = (seconds * f64::from(p.settings.sample_rate)) as u64;
    let r = export::render(&mut b, p, Select::All, 0, to, 0).unwrap();
    (r.left, r.right)
}

/// The largest difference between two renders, and the loudest sample of the first.
fn compare(a: &(Vec<f32>, Vec<f32>), b: &(Vec<f32>, Vec<f32>)) -> (f32, f32) {
    let mut diff = 0f32;
    let mut peak = 0f32;
    for (x, y) in [(&a.0, &b.0), (&a.1, &b.1)] {
        assert_eq!(x.len(), y.len());
        for (u, v) in x.iter().zip(y.iter()) {
            diff = diff.max((u - v).abs());
            peak = peak.max(u.abs());
        }
    }
    (diff, peak)
}

fn same_sound(before: &(Vec<f32>, Vec<f32>), after: &(Vec<f32>, Vec<f32>)) {
    let (diff, peak) = compare(before, after);
    assert!(peak > 0.05, "the song is silent ({peak})");
    assert!(diff < 1e-3, "differs by {diff} (peak {peak})");
}

fn clip_of(p: &Project, id: &str) -> xirius_core::model::Clip {
    p.clip(id.parse::<ClipId>().unwrap()).unwrap().1.clone()
}

fn track_of(p: &Project, id: &str) -> xirius_core::model::Track {
    (**p.tracks.get(&id.parse::<TrackId>().unwrap()).unwrap()).clone()
}

/// The ids of the tracks in the order they are shown.
fn column(p: &Project) -> Vec<String> {
    let mut v = Vec::new();
    for n in &p.root {
        match n {
            Node::Track(t) => v.push(t.to_string()),
            Node::Folder(f) => v.extend(p.tracks_in(*f).iter().map(ToString::to_string)),
        }
    }
    v
}

fn bounces(dir: &Dir) -> Vec<PathBuf> {
    match std::fs::read_dir(dir.0.join("song").join("Media").join("Bounces")) {
        Ok(r) => r.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn a_clip_bounced_in_place_plays_its_processed_sound_plainly() {
    let dir = Dir::new("in-place");
    let file = dir.0.join("a.wav");
    tone(&file);
    let mut s = song(&dir);
    let t = audio_track(&mut s, "A");
    let a = audio_clip(&mut s, &t, &file, 1920, 4);
    run(&mut s, "clip.speed.set", json!({"ids": [a], "percent": 50}));
    run(&mut s, "clip.gain.set", json!({"ids": [a], "db": -6}));
    run(&mut s, "clip.slip", json!({"ids": [a], "by": -960}));
    let was = clip_of(s.project(), &a);
    let before = render(&s, 6.0);

    // A dry run says what it would do and writes nothing.
    let n = s.history().len();
    let r = commands::run(
        &mut s,
        None,
        "clip.bounce",
        &map(json!({"ids": [a]})),
        &RunOptions {
            dry_run: true,
            ..RunOptions::human()
        },
    )
    .unwrap();
    assert!(r.message.contains("バウンス"), "{}", r.message);
    assert_eq!(s.history().len(), n);
    assert!(bounces(&dir).is_empty());

    let r = run(&mut s, "clip.bounce", json!({"ids": [a]}));
    assert_eq!(s.history().len(), n + 1, "one step");
    assert_eq!(r.message, "クリップ 1 件をバウンスしました");
    let now = clip_of(s.project(), &a);
    let ClipBody::Audio {
        file: f,
        offset,
        gain_db,
        process,
    } = &now.body
    else {
        panic!("not audio");
    };
    assert!(f.starts_with("Media/Bounces/"), "{f}");
    assert_eq!(*offset, 0);
    assert_eq!(*gain_db, -6.0, "the gain stays a gain");
    assert_eq!(*process, Default::default());
    assert_eq!((now.start, now.length), (was.start, was.length));
    // EDT-111: the clip says it is a bounce, and so does the file.
    assert!(!was.name.is_empty());
    assert_eq!(now.name, format!("{} バウンス", was.name));
    assert!(f.contains(&format!("{} バウンス-", was.name)), "{f}");
    assert_eq!(bounces(&dir).len(), 1);
    same_sound(&before, &render(&s, 6.0));

    s.undo(1).unwrap();
    assert_eq!(clip_of(s.project(), &a).body, was.body);
    assert_eq!(clip_of(s.project(), &a).name, was.name);
}

#[test]
fn a_clip_bounced_to_a_new_track_goes_through_the_inserts() {
    let dir = Dir::new("new-track");
    let file = dir.0.join("a.wav");
    tone(&file);
    let mut s = song(&dir);
    let t = audio_track(&mut s, "Gt");
    let other = audio_track(&mut s, "B");
    let a = audio_clip(&mut s, &t, &file, 0, 2);
    // A clip later on the track stops the tail before it.
    let later = audio_clip(&mut s, &t, &file, 3840 * 2, 2);
    delay(&mut s, &t);
    run(&mut s, "track.volume.set", json!({"ids": [t], "db": -4}));
    run(&mut s, "track.pan.set", json!({"ids": [t], "pan": 0.4}));
    let before = render(&s, 6.0);

    let n = s.history().len();
    let r = run(
        &mut s,
        "clip.bounce",
        json!({"ids": [a], "new-track": true}),
    );
    assert_eq!(s.history().len(), n + 1, "one step");
    assert_eq!(r.message, "クリップ 1 件を新しいトラックにバウンスしました");
    let p = s.project();
    assert!(clip_of(p, &a).muted);
    assert!(!clip_of(p, &later).muted);
    let order = column(p);
    let at = order.iter().position(|x| *x == t).unwrap();
    let made = order[at + 1].clone();
    assert_eq!(order[at + 2], other, "right below the source");
    let nt = track_of(p, &made);
    assert_eq!(nt.name, "Gt バウンス");
    assert_eq!(nt.kind, TrackKind::Audio);
    assert!(nt.inserts.is_empty());
    assert_eq!((nt.volume_db, nt.pan), (-4.0, 0.4));
    assert_eq!(nt.clips.len(), 1);
    let c = &nt.clips[0];
    assert_eq!(c.start, 0);
    assert_eq!(c.name, format!("{} バウンス", clip_of(p, &a).name));
    // The delay's tail runs past the clip's two beats, up to the later clip.
    assert!(c.length > 2 * 960 && c.length <= 3840 * 2, "{}", c.length);
    same_sound(&before, &render(&s, 6.0));

    s.undo(1).unwrap();
    assert!(!clip_of(s.project(), &a).muted);
    assert_eq!(column(s.project()), vec![t, other]);
}

#[test]
fn a_midi_clip_always_goes_to_a_new_track() {
    let dir = Dir::new("midi");
    let mut s = song(&dir);
    let t = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 1 }),
    ));
    run(
        &mut s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": 60, "length": 1920, "velocity": 100 }),
    );
    let before = render(&s, 4.0);
    run(&mut s, "clip.bounce", json!({"ids": [c]}));
    let p = s.project();
    assert!(clip_of(p, &c).muted);
    let made = track_of(p, &column(p)[1]);
    assert_eq!(made.kind, TrackKind::Audio);
    assert_eq!(made.name, "Keys バウンス");
    same_sound(&before, &render(&s, 4.0));
}

#[test]
fn a_range_mixes_the_tracks_into_one_new_track() {
    let dir = Dir::new("range");
    let file = dir.0.join("a.wav");
    tone(&file);
    let mut s = song(&dir);
    let a = audio_track(&mut s, "A");
    let b = audio_track(&mut s, "B");
    let ca = audio_clip(&mut s, &a, &file, 0, 2);
    let cb = audio_clip(&mut s, &b, &file, 960, 2);
    // Starts inside the range and ends after it: it stays as it is.
    let partly = audio_clip(&mut s, &b, &file, 3840 - 480, 2);
    delay(&mut s, &b);
    run(&mut s, "track.volume.set", json!({"ids": [a], "db": -3}));
    run(&mut s, "track.pan.set", json!({"ids": [b], "pan": -0.5}));
    let only_range = {
        let mut s2 = Session::with_registry(s.project().clone(), shared_registry());
        commands::attach(&mut s2, ExtConfig::default());
        run(&mut s2, "clip.mute", json!({"ids": [partly]}));
        render(&s2, 8.0)
    };

    let n = s.history().len();
    let r = run(
        &mut s,
        "range.bounce",
        json!({"from": 0, "to": 3840, "tracks": [b, a]}),
    );
    assert_eq!(s.history().len(), n + 1, "one step");
    assert_eq!(r.message, "選択範囲を新しいトラックにバウンスしました");
    let p = s.project();
    assert!(clip_of(p, &ca).muted && clip_of(p, &cb).muted);
    assert!(!clip_of(p, &partly).muted);
    let order = column(p);
    assert_eq!(order.len(), 3);
    assert_eq!(order[..2], [a.clone(), b.clone()]);
    let nt = track_of(p, &order[2]);
    assert_eq!(nt.name, "ミックスダウン");
    assert_eq!((nt.volume_db, nt.pan), (0.0, 0.0));
    assert_eq!(nt.clips.len(), 1);
    let c = &nt.clips[0];
    assert_eq!(c.start, 0);
    assert!(c.length > 3840, "the delay's tail is kept ({})", c.length);
    assert!(
        c.length < 3840 * 6,
        "the tail ends where it falls silent ({})",
        c.length
    );

    // Without the clip that only partly lies in the range, the mix is what the range played.
    run(&mut s, "clip.mute", json!({"ids": [partly]}));
    same_sound(&only_range, &render(&s, 8.0));
}

#[test]
fn a_track_bounce_mutes_the_source_and_keeps_its_settings() {
    let dir = Dir::new("track");
    let file = dir.0.join("a.wav");
    tone(&file);
    let mut s = song(&dir);
    let t = audio_track(&mut s, "Vox");
    audio_clip(&mut s, &t, &file, 0, 2);
    audio_clip(&mut s, &t, &file, 3840, 2);
    delay(&mut s, &t);
    run(&mut s, "track.volume.set", json!({"ids": [t], "db": -2}));
    let before = render(&s, 6.0);

    run(&mut s, "track.bounce", json!({"ids": [t]}));
    let p = s.project();
    assert!(track_of(p, &t).mute);
    let nt = track_of(p, &column(p)[1]);
    assert_eq!(nt.name, "Vox バウンス");
    assert!(!nt.mute && nt.inserts.is_empty());
    assert_eq!(nt.volume_db, -2.0);
    assert_eq!(nt.clips.len(), 1);
    assert_eq!(nt.clips[0].start, 0);
    same_sound(&before, &render(&s, 6.0));
}

#[test]
fn a_bus_or_an_empty_track_is_turned_away_and_nothing_changes() {
    let dir = Dir::new("refuse");
    let mut s = song(&dir);
    let empty = audio_track(&mut s, "Empty");
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Bus" })));
    let n = s.history().len();
    let e = try_run(&mut s, "track.bounce", json!({"ids": [bus]})).unwrap_err();
    assert!(e.contains("バウンスできません"), "{e}");
    let e = try_run(&mut s, "track.bounce", json!({"ids": [empty]})).unwrap_err();
    assert!(e.contains("鳴らすクリップがありません"), "{e}");
    let e = try_run(
        &mut s,
        "range.bounce",
        json!({"from": 3840, "to": 0, "tracks": [empty]}),
    )
    .unwrap_err();
    assert!(!e.is_empty());
    assert_eq!(s.history().len(), n);
    assert!(bounces(&dir).is_empty());
}

// ---- freezing (EXP-004) ----------------------------------------------------------

fn frozen_files(dir: &Dir) -> Vec<PathBuf> {
    match std::fs::read_dir(dir.0.join("song").join("Media").join("Freeze")) {
        Ok(r) => r.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Whether any file the song is saved in mentions `word`.
fn saved_mentions(dir: &Path, word: &str) -> bool {
    std::fs::read_dir(dir).unwrap().any(|e| {
        let p = e.unwrap().path();
        if p.is_dir() {
            saved_mentions(&p, word)
        } else {
            std::fs::read(&p).is_ok_and(|b| String::from_utf8_lossy(&b).contains(word))
        }
    })
}

#[test]
fn a_frozen_track_sounds_the_same_and_keeps_its_fader() {
    let dir = Dir::new("freeze");
    let file = dir.0.join("a.wav");
    tone(&file);
    let mut s = song(&dir);
    let t = audio_track(&mut s, "A");
    let a = audio_clip(&mut s, &t, &file, 1920, 4);
    run(&mut s, "clip.speed.set", json!({"ids": [a], "percent": 50}));
    delay(&mut s, &t);
    let before = render(&s, 6.0);

    let n = s.history().len();
    let r = run(&mut s, "track.freeze", json!({"ids": [t]}));
    assert_eq!(r.message, "トラックをフリーズしました");
    assert_eq!(s.history().len(), n + 1, "one step");
    assert_eq!(frozen_files(&dir).len(), 1);
    let frozen = track_of(s.project(), &t).frozen.expect("frozen");
    assert!(frozen.file.starts_with("Media/Freeze"), "{}", frozen.file);
    assert_eq!(frozen.start, 1920);
    // The clips and the inserts stay, so unfreezing brings them back.
    assert_eq!(track_of(s.project(), &t).inserts.len(), 1);
    same_sound(&before, &render(&s, 6.0));

    // Again does nothing.
    assert!(try_run(&mut s, "track.freeze", json!({"ids": [t]})).is_err());
    assert_eq!(s.history().len(), n + 1);

    // The fader and the pan still work: frozen and not, they sound the same.
    run(&mut s, "track.volume.set", json!({"ids": [t], "db": -6}));
    run(&mut s, "track.pan.set", json!({"ids": [t], "pan": 0.5}));
    run(&mut s, "track.rename", json!({"id": t, "name": "A2"}));
    let quieter = render(&s, 6.0);
    let (diff, _) = compare(&before, &quieter);
    assert!(diff > 0.05, "the fader did nothing ({diff})");

    // What the sound was made from cannot change.
    let n = s.history().len();
    for (name, args) in [
        ("clip.move", json!({"ids": [a], "by": 960})),
        ("clip.speed.set", json!({"ids": [a], "percent": 100})),
        ("clip.delete", json!({"ids": [a]})),
        ("plugin.add", json!({"track": t, "name": "Delay"})),
    ] {
        let e = try_run(&mut s, name, args).expect_err(name);
        assert_eq!(e, "「A2」はフリーズ中のため編集できません", "{name}");
    }
    assert!(try_run(&mut s, "clip.bounce", json!({"ids": [a]})).is_err());
    assert_eq!(s.history().len(), n, "nothing changed");

    // Unfrozen it plays its clips and inserts again, at the same fader.
    let r = run(&mut s, "track.unfreeze", json!({"ids": [t]}));
    assert_eq!(r.message, "フリーズを解除しました");
    assert!(track_of(s.project(), &t).frozen.is_none());
    same_sound(&quieter, &render(&s, 6.0));
    run(&mut s, "clip.move", json!({"ids": [a], "by": 960}));
    assert!(try_run(&mut s, "track.unfreeze", json!({"ids": [t]})).is_err());

    // Undo brings the frozen sound back; the file was kept.
    s.undo(2).unwrap();
    assert_eq!(track_of(s.project(), &t).frozen, Some(frozen));
    same_sound(&quieter, &render(&s, 6.0));
}

#[test]
fn a_frozen_instrument_sounds_the_same() {
    let dir = Dir::new("freeze-inst");
    let mut s = song(&dir);
    let t = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({ "track": t, "at": 960, "bars": 1 }),
    ));
    run(
        &mut s,
        "note.add",
        json!({ "clip": c, "at": 960, "pitch": 60, "length": 1920, "velocity": 100 }),
    );
    let before = render(&s, 4.0);
    run(&mut s, "track.freeze", json!({"ids": [t]}));
    same_sound(&before, &render(&s, 4.0));
    let e = try_run(
        &mut s,
        "note.add",
        json!({ "clip": c, "at": 1920, "pitch": 64, "length": 960, "velocity": 100 }),
    )
    .unwrap_err();
    assert!(e.contains("フリーズ中"), "{e}");
}

#[test]
fn a_frozen_track_is_kept_through_save_and_reopen() {
    let dir = Dir::new("freeze-save");
    let file = dir.0.join("a.wav");
    tone(&file);
    let mut s = song(&dir);
    let t = audio_track(&mut s, "A");
    audio_clip(&mut s, &t, &file, 0, 4);
    delay(&mut s, &t);
    // A song that was never frozen is saved as before: without the new value.
    s.save().unwrap();
    let folder = dir.0.join("song");
    assert!(!saved_mentions(&folder, "frozen"));
    let again = Session::open_with(&folder, shared_registry()).unwrap();
    assert!(track_of(again.project(), &t).frozen.is_none());

    run(&mut s, "track.freeze", json!({"ids": [t]}));
    let before = render(&s, 4.0);
    let frozen = track_of(s.project(), &t).frozen;
    s.save().unwrap();
    assert!(saved_mentions(&folder, "frozen"));
    let mut again = Session::open_with(&folder, shared_registry()).unwrap();
    commands::attach(&mut again, ExtConfig::default());
    assert_eq!(track_of(again.project(), &t).frozen, frozen);
    same_sound(&before, &render(&again, 4.0));
}

#[test]
fn a_bus_cannot_be_frozen() {
    let dir = Dir::new("freeze-bus");
    let mut s = song(&dir);
    let b = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Bus" })));
    let e = try_run(&mut s, "track.freeze", json!({"ids": [b]})).unwrap_err();
    assert!(e.contains("フリーズできません"), "{e}");
    let t = audio_track(&mut s, "Empty");
    let e = try_run(&mut s, "track.freeze", json!({"ids": [t]})).unwrap_err();
    assert!(e.contains("鳴らすクリップがありません"), "{e}");
    assert!(frozen_files(&dir).is_empty());
}

#[test]
fn job_cancel_stops_a_bounce_from_another_session_and_leaves_the_song_alone() {
    use std::time::{Duration, Instant};
    use xirius_core::running;

    let dir = Dir::new("cancel");
    let file = dir.0.join("a.wav");
    tone(&file);
    let mut s = song(&dir);
    let t = audio_track(&mut s, "A");
    audio_clip(&mut s, &t, &file, 0, 4);
    let song_dir = s.dir().unwrap().display().to_string();
    let n = s.history().len();
    let tracks = s.project().tracks.len();
    // 400 bars: long enough to be stopped while it renders.
    let bounce = std::thread::spawn(move || {
        let r = try_run(
            &mut s,
            "range.bounce",
            json!({"from": 0, "to": 3840 * 400, "tracks": [t], "tail": 0}),
        );
        (s, r)
    });
    let start = Instant::now();
    let job = loop {
        if let Some(j) = running::list()
            .into_iter()
            .find(|j| j.song.as_deref() == Some(song_dir.as_str()))
        {
            break j;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "no job");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(job.command, "range.bounce");

    let mut other = Session::with_registry(Project::new("other"), shared_registry());
    commands::attach(&mut other, ExtConfig::default());
    let listed = run(&mut other, "job.list", json!({}));
    assert!(
        listed.data["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["id"] == job.id.as_str()),
        "{}",
        listed.data
    );
    let r = run(&mut other, "job.cancel", json!({"id": job.id}));
    assert_eq!(r.message, "処理 1 件をキャンセルしました");
    assert_eq!(r.data["jobs"][0]["id"], job.id.as_str());

    let (s, r) = bounce.join().unwrap();
    assert_eq!(r.unwrap_err(), xirius_session::bounce::CANCELLED);
    assert_eq!(s.history().len(), n, "no step");
    assert_eq!(s.project().tracks.len(), tracks);
    assert!(bounces(&dir).is_empty(), "{:?}", bounces(&dir));
    assert!(!running::list().iter().any(|j| j.id == job.id));
    assert!(try_run(&mut other, "job.cancel", json!({"id": job.id})).is_err());
}

#[test]
fn the_tracks_a_bounce_adds_are_known_before_it_runs() {
    use xirius_session::bounce::{Planned, planned};
    let dir = Dir::new("planned");
    let file = dir.0.join("a.wav");
    tone(&file);
    let mut s = song(&dir);
    let a = audio_track(&mut s, "A");
    let b = audio_track(&mut s, "B");
    let ca = audio_clip(&mut s, &a, &file, 0, 2);
    let cb = audio_clip(&mut s, &b, &file, 960, 2);
    let p = s.project().clone();
    let reg = shared_registry();
    let args = |name: &str, v: Value| reg.normalize(reg.get(name).unwrap(), &map(v)).unwrap();
    let id = |x: &str| x.parse::<TrackId>().unwrap();

    // In place: nothing new.
    let none = planned(
        &p,
        "clip.bounce",
        &args("clip.bounce", json!({"ids": [ca]})),
    )
    .unwrap();
    assert!(none.is_empty());
    let none = planned(
        &p,
        "track.freeze",
        &args("track.freeze", json!({"ids": [a]})),
    )
    .unwrap();
    assert!(none.is_empty());

    // To a new track: one below each source track, where its clips are.
    let two = planned(
        &p,
        "clip.bounce",
        &args("clip.bounce", json!({"ids": [ca, cb], "new-track": true})),
    )
    .unwrap();
    assert_eq!(
        two,
        vec![
            Planned {
                after: id(&a),
                like: Some(id(&a)),
                name: "A バウンス".into(),
                spans: vec![(0, 1920)],
            },
            Planned {
                after: id(&b),
                like: Some(id(&b)),
                name: "B バウンス".into(),
                spans: vec![(960, 2880)],
            },
        ]
    );

    // A range of two tracks mixes into one, below the last.
    let mixed = planned(
        &p,
        "range.bounce",
        &args(
            "range.bounce",
            json!({"from": 0, "to": 3840, "tracks": [a, b]}),
        ),
    )
    .unwrap();
    assert_eq!(mixed.len(), 1);
    assert_eq!((mixed[0].after, mixed[0].like), (id(&b), None));
    assert_eq!(mixed[0].spans, vec![(0, 3840)]);

    // What cannot be bounced says so, as the bounce does.
    assert!(
        planned(
            &p,
            "clip.bounce",
            &args("clip.bounce", json!({"ids": ["clp_999"]}))
        )
        .is_err()
    );
    // Nothing was written.
    assert!(bounces(&dir).is_empty());
}
