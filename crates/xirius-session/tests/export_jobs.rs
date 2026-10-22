//! Exports as the panel asks for them: FLAC next to WAV, the file name patterns, the ranges and
//! stems the panel offers, and the record of each export kept in the song folder. Every render
//! is offline; nothing here opens an audio device, a file window or the recycle bin.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, ScanOptions};
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, Format, Options, StemSet};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::jobs::{self, Store};
use xirius_session::output::OutputMode;

// ---- helpers ------------------------------------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh folder under the temp dir, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-export-jobs-test-{}-{n}-{tag}",
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

/// A config that never touches the user's plugin cache.
fn config(dir: &Path) -> ExtConfig {
    ExtConfig {
        output: OutputMode::None,
        scan: ScanOptions {
            dirs: vec![],
            cache: dir.join("plugins.json"),
            ..ScanOptions::default()
        },
        catalog: Some(Arc::new(Catalog::default())),
    }
}

/// The recycle bin of the tests: `<tmp>/bin`, so a file an export replaces is kept there and
/// nothing is sent to the person's own.
fn extras(tmp: &Path) -> ExtExtras {
    let bin = tmp.join("bin");
    ExtExtras {
        trash: Some(Arc::new(move |p: &Path| {
            std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let mut to = bin.join(&name);
            let mut n = 1;
            while to.exists() {
                n += 1;
                to = bin.join(format!("{n}-{name}"));
            }
            std::fs::rename(p, to).map_err(|e| e.to_string())
        })),
        ..ExtExtras::default()
    }
}

/// A song not saved anywhere.
fn song(name: &str, tmp: &Path) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach_with(&mut s, config(tmp), extras(tmp));
    s
}

/// A song folder `<tmp>/song`, made the first time and opened as the app opens it.
fn song_in(tmp: &Path, name: &str) -> Session {
    let dir = tmp.join("song");
    if !dir.exists() {
        drop(Session::create(&dir, Project::new(name)).unwrap());
    }
    let mut s = Session::open_with(&dir, shared_registry()).unwrap();
    commands::attach_with(&mut s, config(tmp), extras(tmp));
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
    try_run(s, name, args).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// An instrument track with one MIDI clip of `bars` bars holding a chord, so the sound is
/// not one clean sine.
fn voice(s: &mut Session, name: &str, pitch: i64, bars: f64) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": name })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": bars }),
    ));
    for (i, p) in [pitch, pitch + 4, pitch + 7].into_iter().enumerate() {
        run(
            s,
            "note.add",
            json!({ "clip": c, "at": i as i64 * 480, "pitch": p, "length": (bars * 3840.0) as i64 - i as i64 * 480, "velocity": 90 + i as i64 * 10 }),
        );
    }
    t
}

fn wav_ints(path: &Path) -> (hound::WavSpec, Vec<i32>) {
    let mut r = hound::WavReader::open(path).unwrap();
    let spec = r.spec();
    (spec, r.samples::<i32>().map(|x| x.unwrap()).collect())
}

fn flac_ints(path: &Path) -> (claxon::metadata::StreamInfo, Vec<i32>) {
    let mut r = claxon::FlacReader::open(path).unwrap();
    let info = r.streaminfo();
    (info, r.samples().map(|x| x.unwrap()).collect())
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Polls `export.status --job` until the job is no longer running.
fn wait_job(s: &mut Session, job: &str) -> Value {
    let start = Instant::now();
    loop {
        let r = run(s, "export.status", json!({ "job": job }));
        if r.data["state"] != "running" {
            return r.data;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "the job did not end: {:?}",
            r.data
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn jobs_of(s: &mut Session) -> Vec<Value> {
    run(s, "export.status", json!({})).data["jobs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

// ---- FLAC (EXP-001, EXP-011) --------------------------------------------------------------

#[test]
fn flac_holds_the_same_samples_as_the_wav() {
    let dir = Dir::new("flac");
    let mut s = song("flac", dir.path());
    voice(&mut s, "A", 60, 2.0);
    voice(&mut s, "B", 43, 2.0);
    for (rate, bits) in [(48000u32, 16u16), (44100, 24), (96000, 24)] {
        let o = |format| Options {
            sample_rate: Some(rate),
            bit_depth: Some(bits),
            format,
            ..Options::default()
        };
        let wav = dir.path().join(format!("m{rate}-{bits}.wav"));
        let flac = dir.path().join(format!("m{rate}-{bits}.flac"));
        let rw = export::mixdown(s.project(), &wav, &o(Format::Wav)).unwrap();
        let rf = export::mixdown(s.project(), &flac, &o(Format::Flac)).unwrap();
        assert_eq!(rf.format, Format::Flac);
        assert_eq!(rw.frames, rf.frames);

        let (spec, w) = wav_ints(&wav);
        let (info, f) = flac_ints(&flac);
        assert_eq!(spec.bits_per_sample, bits);
        assert_eq!(
            (info.sample_rate, info.bits_per_sample, info.channels),
            (rate, u32::from(bits), 2)
        );
        assert_eq!(info.samples, Some(rf.frames));
        assert_eq!(w.len(), f.len(), "{rate} Hz {bits} bit");
        assert!(w.iter().any(|x| x.abs() > 1000), "the song must be heard");
        let first = w.iter().zip(&f).position(|(a, b)| a != b);
        assert_eq!(
            first, None,
            "{rate} Hz {bits} bit: the first differing sample"
        );
        // Lossless, and still smaller than the WAV.
        let (ws, fs) = (
            std::fs::metadata(&wav).unwrap().len(),
            std::fs::metadata(&flac).unwrap().len(),
        );
        assert!(fs < ws, "{fs} vs {ws}");
    }
}

#[test]
fn flac_refuses_32_bit_float_before_anything_is_written() {
    let dir = Dir::new("flac32");
    let mut s = song_in(dir.path(), "f32");
    voice(&mut s, "A", 60, 1.0);
    let out = dir.path().join("x.flac");
    let o = Options {
        bit_depth: Some(32),
        format: Format::Flac,
        ..Options::default()
    };
    let e = export::mixdown(s.project(), &out, &o).unwrap_err();
    assert_eq!(e.0, export::FLAC_NO_FLOAT);
    assert!(!out.exists());

    // Through the command: refused before it starts, so it leaves no record behind.
    let e = try_run(
        &mut s,
        "export.mixdown",
        json!({ "format": "flac", "bit_depth": 32 }),
    )
    .unwrap_err();
    assert!(e.contains("32 bit"), "{e}");
    assert!(jobs_of(&mut s).is_empty());

    // The format comes from --out when not given, and the two must agree.
    let e = try_run(
        &mut s,
        "export.mixdown",
        json!({ "out": out.display().to_string(), "format": "wav" }),
    )
    .unwrap_err();
    assert!(e.contains("flac") || e.contains("FLAC"), "{e}");
    let e = try_run(
        &mut s,
        "export.mixdown",
        json!({ "out": dir.path().join("x.ogg").display().to_string() }),
    )
    .unwrap_err();
    assert!(e.contains(".ogg"), "{e}");
    let r = run(
        &mut s,
        "export.mixdown",
        json!({ "out": out.display().to_string(), "bit_depth": 16 }),
    );
    assert_eq!(r.data["format"], "flac");
    assert!(out.exists());
}

#[test]
fn the_report_gives_loudness_and_true_peak() {
    let dir = Dir::new("lufs");
    let mut s = song("lufs", dir.path());
    voice(&mut s, "A", 60, 2.0);
    let out = dir.path().join("m.wav");
    let rep = export::mixdown(s.project(), &out, &Options::default()).unwrap();
    let lufs = rep.lufs.expect("a song of 4 s has a loudness");
    let tp = rep.true_peak_db.expect("and a true peak");
    assert!((-60.0..0.0).contains(&lufs), "{lufs}");
    // The true peak is the peak between the samples too, so never below the sample peak.
    assert!(tp >= rep.peak_db() - 0.05, "{tp} vs {}", rep.peak_db());
    let j = rep.to_json();
    assert!(
        j["lufs"].is_number() && j["true_peak_db"].is_number(),
        "{j}"
    );

    // Silence has no loudness.
    let (l, t) = export::loudness(&[0.0; 48000], &[0.0; 48000], 48000);
    assert_eq!((l, t.is_some_and(f64::is_finite)), (None, false));
}

// ---- what the panel picks (EXP-010, EXP-013, EXP-015, OPEN-10) ------------------------------

#[test]
fn a_list_of_sections_runs_from_the_first_to_the_last() {
    let dir = Dir::new("sections");
    let mut s = song("sections", dir.path());
    voice(&mut s, "A", 60, 6.0);
    run(
        &mut s,
        "section.add",
        json!({ "name": "Verse", "at": 3840, "bars": 1 }),
    );
    run(
        &mut s,
        "section.add",
        json!({ "name": "Chorus", "at": 11520, "bars": 2 }),
    );
    let out = dir.path().join("s.wav");
    let frames = |s: &mut Session, section: &str| {
        let r = run(
            s,
            "export.mixdown",
            json!({ "range": "section", "section": section, "out": out.display().to_string(), "overwrite": true }),
        );
        r.data["frames"].as_u64().unwrap()
    };
    // At 120 bpm a bar is 2 s.
    assert_eq!(frames(&mut s, "Verse"), 96000);
    assert_eq!(frames(&mut s, "Verse,Chorus"), 4 * 96000);
    // In either order: the start of the first to the end of the last.
    assert_eq!(frames(&mut s, "Chorus, Verse"), 4 * 96000);
    let e = try_run(
        &mut s,
        "export.mixdown",
        json!({ "range": "section", "section": "Verse,Bridge", "out": out.display().to_string() }),
    )
    .unwrap_err();
    assert!(e.contains("Bridge"), "{e}");
}

#[test]
fn one_section_picked_in_the_panel_is_found_by_its_id() {
    let dir = Dir::new("section-id");
    let mut s = song("section-id", dir.path());
    voice(&mut s, "A", 60, 6.0);
    run(
        &mut s,
        "section.add",
        json!({ "name": "Verse", "at": 3840, "bars": 1 }),
    );
    let id = run(&mut s, "export.open", json!({})).data["sections"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    // The panel names the sections it picked by id, one or many.
    let out = dir.path().join("one.wav");
    let r = run(
        &mut s,
        "export.mixdown",
        json!({ "range": "section", "section": id, "out": out.display().to_string() }),
    );
    assert_eq!(r.data["frames"].as_u64().unwrap(), 96000);
}

#[test]
fn stems_by_bus_or_picked_tracks() {
    let dir = Dir::new("bybus");
    let mut s = song("bb", dir.path());
    let a = voice(&mut s, "A", 60, 1.0);
    let b = voice(&mut s, "B", 67, 1.0);
    let _c = voice(&mut s, "C", 72, 1.0);
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Grp" })));
    run(
        &mut s,
        "track.output.set",
        json!({ "ids": [a, b], "to": bus }),
    );
    let names = |s: &Session, set: StemSet| -> Vec<String> {
        set.strips(s.project())
            .iter()
            .map(|id| s.project().tracks[id].name.clone())
            .collect()
    };
    // A and B go out through the bus, so its stem holds them.
    assert_eq!(names(&s, StemSet::ByBus), vec!["C", "Grp"]);

    let out = dir.path().join("by");
    run(
        &mut s,
        "export.stems",
        json!({ "what": "by_bus", "dir": out.display().to_string() }),
    );
    assert_eq!(file_names(&out), vec!["bb_C.wav", "bb_Grp.wav"]);

    // The tracks picked on the screen.
    let picked = dir.path().join("picked");
    let r = run(
        &mut s,
        "export.stems",
        json!({ "what": "selected", "ids": [b], "dir": picked.display().to_string(), "name": "{トラック}" }),
    );
    assert_eq!(file_names(&picked), vec!["B.wav"], "{:?}", r.data);
    let e = try_run(
        &mut s,
        "export.stems",
        json!({ "what": "selected", "dir": picked.display().to_string() }),
    )
    .unwrap_err();
    assert!(e.contains("ids"), "{e}");

    // Every stem must get a name of its own.
    let e = try_run(
        &mut s,
        "export.stems",
        json!({ "what": "tracks", "name": "{曲}", "dir": picked.display().to_string() }),
    )
    .unwrap_err();
    assert!(e.contains("{トラック}"), "{e}");
}

#[test]
fn a_second_export_of_the_same_name_replaces_the_file_and_its_line() {
    let dir = Dir::new("replace");
    let mut s = song_in(dir.path(), "Demo");
    let a = voice(&mut s, "A", 60, 1.0);
    voice(&mut s, "B", 43, 1.0);
    let song = dir.path().join("song");
    let (mix, stems, bin) = (
        song.join("Mixdown"),
        song.join("Stems"),
        dir.path().join("bin"),
    );
    let first = run(&mut s, "export.mixdown", json!({ "bit_depth": 16 })).data["record"]
        .as_str()
        .unwrap()
        .to_string();
    let before = std::fs::read(mix.join("Demo.wav")).unwrap();
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [a], "db": -12.0 }),
    );

    // A dry run names the same file and moves nothing.
    let mut o = RunOptions::human();
    o.dry_run = true;
    let r = commands::run(&mut s, None, "export.mixdown", &Map::new(), &o).unwrap();
    assert!(
        r.data["path"].as_str().unwrap().ends_with("Demo.wav"),
        "{:?}",
        r.data
    );
    assert!(file_names(&bin).is_empty());

    let r = run(&mut s, "export.mixdown", json!({ "bit_depth": 16 }));
    assert!(r.data["path"].as_str().unwrap().ends_with("Demo.wav"));
    let second = r.data["record"].as_str().unwrap().to_string();
    assert_eq!(file_names(&mix), vec!["Demo.wav"], "no Demo-2.wav");
    assert_ne!(std::fs::read(mix.join("Demo.wav")).unwrap(), before);
    // The file it replaced is in the recycle bin, as it was.
    assert_eq!(file_names(&bin), vec!["Demo.wav"]);
    assert_eq!(std::fs::read(bin.join("Demo.wav")).unwrap(), before);
    // One line: the new export, which says which it replaced.
    let all = jobs_of(&mut s);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0]["job"], second.as_str());
    assert_eq!(all[0]["replaces"], first.as_str());
    let store = Store::of_song(&song);
    assert!(store.read(&first).is_none(), "the old record is gone");

    // Stems: each file of the same name, and the line of the export that wrote them.
    run(&mut s, "export.stems", json!({ "bit_depth": 16 }));
    run(&mut s, "export.stems", json!({ "bit_depth": 16 }));
    assert_eq!(file_names(&stems), vec!["Demo_A.wav", "Demo_B.wav"]);
    assert_eq!(
        file_names(&bin),
        vec!["Demo.wav", "Demo_A.wav", "Demo_B.wav"]
    );
    assert_eq!(jobs_of(&mut s).len(), 2);
    // Stems of A alone replace A's file; the line of A and B stays, it still has B's.
    run(
        &mut s,
        "export.stems",
        json!({ "bit_depth": 16, "what": "selected", "ids": [a] }),
    );
    assert_eq!(file_names(&stems), vec!["Demo_A.wav", "Demo_B.wav"]);
    assert_eq!(jobs_of(&mut s).len(), 3);
    drop(s);
    let mut s = song_in(dir.path(), "Demo");
    assert_eq!(jobs_of(&mut s).len(), 3, "the same lines when opened again");
}

#[test]
fn a_second_mixdown_gets_a_number_when_asked_and_overwrites_when_asked() {
    let dir = Dir::new("numbers");
    let mut s = song_in(dir.path(), "Demo");
    voice(&mut s, "A", 60, 1.0);
    let mix = dir.path().join("song").join("Mixdown");
    let r = run(&mut s, "export.mixdown", json!({}));
    assert!(r.data["path"].as_str().unwrap().ends_with("Demo.wav"));
    run(&mut s, "export.mixdown", json!({ "numbered": true }));
    run(&mut s, "export.mixdown", json!({ "format": "flac" }));
    assert_eq!(
        file_names(&mix),
        vec!["Demo-2.wav", "Demo.flac", "Demo.wav"]
    );
    assert_eq!(
        jobs_of(&mut s).len(),
        3,
        "a numbered file is a line of its own"
    );
    // Written over in place: nothing goes to the recycle bin.
    run(&mut s, "export.mixdown", json!({ "overwrite": true }));
    assert_eq!(file_names(&mix).len(), 3);
    assert!(file_names(&dir.path().join("bin")).is_empty());

    // A dry run says where it would go and writes nothing.
    let mut o = RunOptions::human();
    o.dry_run = true;
    let r = commands::run(
        &mut s,
        None,
        "export.mixdown",
        &map(json!({ "numbered": true })),
        &o,
    )
    .unwrap();
    assert!(
        r.data["path"].as_str().unwrap().ends_with("Demo-3.wav"),
        "{:?}",
        r.data
    );
    assert_eq!(file_names(&mix).len(), 3);
}

#[test]
fn the_name_pattern_is_remembered_with_the_song() {
    let dir = Dir::new("pattern");
    {
        let mut s = song_in(dir.path(), "Tune");
        voice(&mut s, "Keys", 60, 1.0);
        run(&mut s, "export.mixdown", json!({ "name": "{曲}_master" }));
        run(&mut s, "export.stems", json!({ "name": "st_{トラック}" }));
        let song = dir.path().join("song");
        assert!(song.join("Mixdown").join("Tune_master.wav").exists());
        assert!(song.join("Stems").join("st_Keys.wav").exists());
    }
    // Opened again: the next export uses the same patterns.
    let mut s = song_in(dir.path(), "Tune");
    voice(&mut s, "Keys", 60, 1.0);
    let open = run(&mut s, "export.open", json!({})).data;
    assert_eq!(open["mixdown"]["name"], "{曲}_master", "{open}");
    assert_eq!(open["stems"]["name"], "st_{トラック}", "{open}");
    let r = run(&mut s, "export.mixdown", json!({}));
    assert!(
        r.data["path"]
            .as_str()
            .unwrap()
            .ends_with("Tune_master.wav"),
        "{:?}",
        r.data
    );

    // An unsaved song keeps them for as long as it is open.
    let mut u = song("Loose", dir.path());
    voice(&mut u, "Keys", 60, 1.0);
    run(
        &mut u,
        "export.mixdown",
        json!({ "name": "x_{曲}", "out": dir.path().join("loose.wav").display().to_string() }),
    );
    assert_eq!(
        run(&mut u, "export.open", json!({})).data["mixdown"]["name"],
        "x_{曲}"
    );
}

#[test]
fn the_range_last_exported_is_remembered_with_the_song() {
    let dir = Dir::new("range");
    let (verse, chorus) = {
        let mut s = song_in(dir.path(), "Ranged");
        voice(&mut s, "A", 60, 6.0);
        run(
            &mut s,
            "section.add",
            json!({ "name": "Verse", "at": 3840, "bars": 1 }),
        );
        run(
            &mut s,
            "section.add",
            json!({ "name": "Chorus", "at": 11520, "bars": 2 }),
        );
        // Nothing exported yet: the whole song, as before.
        let open = run(&mut s, "export.open", json!({})).data;
        assert_eq!(open["range"], "song", "{open}");
        let id = |n: usize| open["sections"][n]["id"].as_str().unwrap().to_string();
        let (verse, chorus) = (id(0), id(1));
        run(
            &mut s,
            "export.mixdown",
            json!({ "range": "section", "section": chorus }),
        );
        (verse, chorus)
    };
    // Opened again: the panel starts from the sections that were exported.
    let mut s = song_in(dir.path(), "Ranged");
    let open = run(&mut s, "export.open", json!({})).data;
    assert_eq!(open["range"], "section", "{open}");
    assert_eq!(open["picked_sections"], json!([chorus]), "{open}");
    assert_ne!(verse, chorus);

    // The loop is remembered too, and the song comes back when the song no longer has one.
    run(&mut s, "loop.set", json!({ "start": 3840, "end": 7680 }));
    run(&mut s, "export.mixdown", json!({ "range": "loop" }));
    let mut s = song_in(dir.path(), "Ranged");
    assert_eq!(run(&mut s, "export.open", json!({})).data["range"], "loop");
    run(&mut s, "loop.clear", json!({}));
    assert_eq!(
        run(&mut s, "export.open", json!({})).data["range"],
        "song",
        "a loop the song no longer has is not offered"
    );

    // A range typed in (not one of the panel's) is not kept: the song comes back.
    run(
        &mut s,
        "export.mixdown",
        json!({ "range": "1.1..2.1", "overwrite": true }),
    );
    assert_eq!(run(&mut s, "export.open", json!({})).data["range"], "song");
}

// ---- the record of each export (AI-002, AI-010, OPEN-11) -----------------------------------

#[test]
fn an_export_leaves_a_record_the_next_session_reads() {
    let dir = Dir::new("record");
    let (id, path) = {
        let mut s = song_in(dir.path(), "Kept");
        voice(&mut s, "A", 60, 1.0);
        let r = run(&mut s, "export.mixdown", json!({ "format": "flac" }));
        let id = r.data["record"].as_str().unwrap().to_string();
        (id, r.data["path"].as_str().unwrap().to_string())
    };
    let store = Store::of_song(&dir.path().join("song"));
    assert_eq!(store.dir(), dir.path().join("song").join(jobs::FOLDER));
    assert!(store.dir().join(format!("{id}.json")).exists());

    let mut s = song_in(dir.path(), "Kept");
    let all = jobs_of(&mut s);
    assert_eq!(all.len(), 1, "{all:?}");
    let v = &all[0];
    assert_eq!(v["job"], id.as_str());
    assert_eq!(v["state"], "done");
    assert_eq!(v["command"], "export.mixdown");
    assert_eq!(v["result"]["path"], path.as_str());
    assert_eq!(v["args"]["format"], "flac");
    assert!(v["ended_ms"].as_u64() >= v["started_ms"].as_u64());
    // How far it got, file by file, and how fast (EXP-017).
    assert_eq!(v["fraction"], 1.0);
    assert_eq!(v["files"].as_array().unwrap().len(), 1);
    assert!(v["speed"].as_f64().unwrap() > 0.0, "{v}");

    // Its settings, to start the panel from.
    let open = run(&mut s, "export.open", json!({ "job": id })).data;
    assert_eq!(open["args"]["format"], "flac");
    assert_eq!(open["command"], "export.mixdown");

    // What a delete would move to the recycle bin, and what a reveal would show: dry runs, so
    // no window opens and nothing is moved.
    let mut dry = RunOptions::human();
    dry.dry_run = true;
    let del = commands::run(
        &mut s,
        None,
        "export.delete",
        &map(json!({ "job": id })),
        &dry,
    )
    .unwrap();
    assert_eq!(del.data["files"], json!([path]));
    let rev = commands::run(
        &mut s,
        None,
        "export.reveal_os",
        &map(json!({ "job": id })),
        &dry,
    )
    .unwrap();
    assert_eq!(rev.data["path"], path.as_str());
    assert!(Path::new(&path).exists());
    assert_eq!(jobs_of(&mut s).len(), 1);
}

#[test]
fn update_writes_the_same_file_again_from_the_song_as_it_is_now() {
    let dir = Dir::new("update");
    let mut s = song_in(dir.path(), "Upd");
    let a = voice(&mut s, "A", 60, 1.0);
    let r = run(&mut s, "export.mixdown", json!({ "bit_depth": 16 }));
    let (id, path) = (
        r.data["record"].as_str().unwrap().to_string(),
        PathBuf::from(r.data["path"].as_str().unwrap()),
    );
    let before = std::fs::read(&path).unwrap();
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [a], "db": -12.0 }),
    );

    let r = run(&mut s, "export.update", json!({ "job": id }));
    assert_eq!(r.data["path"], path.display().to_string().as_str());
    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        before.len(),
        after.len(),
        "the same settings: 16 bit, same length"
    );
    assert_ne!(before, after, "the quieter song");
    assert_eq!(
        file_names(path.parent().unwrap()).len(),
        1,
        "no numbered copy"
    );

    // One line in the list: the new record, which says what it replaced.
    let all = jobs_of(&mut s);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0]["replaces"], id.as_str());
    assert_eq!(all[0]["args"]["bit_depth"], 16);
    let store = Store::of_song(&dir.path().join("song"));
    assert!(store.read(&id).is_none(), "the old record is gone");
    drop(s);
    let mut s = song_in(dir.path(), "Upd");
    assert_eq!(jobs_of(&mut s).len(), 1);

    // Only an export that has ended.
    let e = try_run(&mut s, "export.update", json!({ "job": "export-nope" })).unwrap_err();
    assert!(e.contains("export-nope"), "{e}");
}

#[test]
fn a_record_left_running_with_no_one_holding_it_reads_as_interrupted() {
    let dir = Dir::new("crash");
    let mut s = song_in(dir.path(), "Crash");
    let store = Store::of_song(&dir.path().join("song"));
    let id = "export-0-1-1";
    let record = json!({
        "job": id,
        "command": "export.mixdown",
        "pid": 1,
        "started_ms": 1,
        "state": "running",
        "done": 10,
        "total": 100,
        "args": {},
        "path": "x.wav",
    });

    // A process that still holds the lock: running.
    let held = store.lock(id).expect("the lock is free");
    store.write(&record).unwrap();
    assert_eq!(store.read(id).unwrap()["state"], "running");
    assert_eq!(
        run(&mut s, "export.status", json!({ "job": id })).data["state"],
        "running"
    );

    // The process is gone (its lock with it): interrupted, and the record says so from now on.
    drop(held);
    let v = run(&mut s, "export.status", json!({ "job": id }));
    assert_eq!(v.data["state"], "interrupted", "{:?}", v.data);
    assert_eq!(v.data["error"], jobs::INTERRUPTED);
    assert!(v.message.contains("中断"), "{}", v.message);
    let list = run(&mut s, "export.status", json!({}));
    assert!(list.message.contains("中断 1 件"), "{}", list.message);
    // Nothing to cancel.
    let r = run(&mut s, "export.cancel", json!({ "job": id }));
    assert!(r.message.contains("すでに終了"), "{}", r.message);
}

#[test]
fn another_session_cancels_a_running_export_through_the_song_folder() {
    let dir = Dir::new("xcancel");
    let mut a = song_in(dir.path(), "Far");
    voice(&mut a, "A", 60, 1.0);
    a.save().unwrap();
    let r = run(
        &mut a,
        "export.mixdown",
        json!({ "background": true, "tail": 600.0 }),
    );
    let job = r.data["job"].as_str().unwrap().to_string();

    // A second session on the same song (as a one-shot `xirius` next to the app) sees it running
    // and cancels it.
    let mut b = song_in(dir.path(), "Far");
    let start = Instant::now();
    loop {
        let v = run(&mut b, "export.status", json!({ "job": job })).data;
        assert_eq!(v["state"], "running", "{v}");
        if v["done"].as_u64().unwrap_or(0) > 0 || start.elapsed() > Duration::from_secs(5) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let r = run(&mut b, "export.cancel", json!({ "job": job }));
    assert!(r.message.contains("別のプロセス"), "{}", r.message);

    let v = wait_job(&mut a, &job);
    assert_eq!(v["state"], "cancelled", "{v}");
    let start = Instant::now();
    while run(&mut b, "export.status", json!({ "job": job })).data["state"] == "running" {
        assert!(start.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        run(&mut b, "export.status", json!({ "job": job })).data["state"],
        "cancelled"
    );
}

#[test]
fn closing_the_song_mid_export_leaves_it_interrupted() {
    let dir = Dir::new("close");
    let job = {
        let mut s = song_in(dir.path(), "Shut");
        voice(&mut s, "A", 60, 1.0);
        s.save().unwrap();
        let r = run(
            &mut s,
            "export.mixdown",
            json!({ "background": true, "tail": 600.0 }),
        );
        r.data["job"].as_str().unwrap().to_string()
    };
    let mut s = song_in(dir.path(), "Shut");
    let v = run(&mut s, "export.status", json!({ "job": job })).data;
    assert_eq!(v["state"], "interrupted", "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("終了したため中止"),
        "{v}"
    );
    let path = PathBuf::from(v["path"].as_str().unwrap());
    assert!(!path.exists(), "a cut-off export leaves no file");
}
