//! MP3 exports (EXP-005): each bitrate, the 48 kHz limit, the ID3 tags, loudness measured on
//! what the MP3 holds, and WAV and FLAC going on when the encoder cannot be loaded.
//!
//! The encoder is LAME's library, which is not in the repository (scripts/build-lame.ps1 on
//! Windows and scripts/build-lame.sh elsewhere make it). libtest has no way for a test to skip
//! itself, so this file runs its own tests (`harness = false`, [`main`] at the end): when the
//! encoder cannot be loaded, a test that needs it is reported as `ignored`, with why, in libtest's
//! own words, and the rest run as ever. Every render is offline.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, ScanOptions};
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, Format, Options};
use xirius_session::ext::ExtConfig;
use xirius_session::mp3;
use xirius_session::output::OutputMode;

// ---- helpers ------------------------------------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh folder under the temp dir, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-export-mp3-test-{}-{n}-{tag}",
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

fn song(name: &str, tmp: &Path) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach(&mut s, config(tmp));
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

/// An instrument track with one MIDI clip of `bars` bars holding a chord.
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

fn mp3_options(kbps: u16) -> Options {
    Options {
        format: Format::Mp3,
        bitrate: Some(kbps),
        ..Options::default()
    }
}

// ---- needs the encoder --------------------------------------------------------------------

fn each_bitrate_writes_a_constant_bitrate_file() {
    let dir = Dir::new("rates");
    let mut s = song("rates", dir.path());
    voice(&mut s, "A", 60, 2.0);
    for kbps in mp3::BITRATES {
        let out = dir.path().join(format!("m{kbps}.mp3"));
        let rep = export::mixdown(s.project(), &out, &mp3_options(kbps)).unwrap();
        assert_eq!(rep.format, Format::Mp3);
        assert_eq!(
            (rep.bitrate_kbps, rep.sample_rate, rep.bit_depth),
            (Some(kbps), 48_000, 0)
        );
        // A constant bitrate: the size is the bitrate times the length, give or take the
        // tag, LAME's info frame and the last frame padded out.
        let size = std::fs::metadata(&out).unwrap().len() as f64;
        let want = f64::from(kbps) * 1000.0 / 8.0 * rep.seconds();
        assert!(
            (0.98..1.08).contains(&(size / want)),
            "{kbps} kbps: {size} bytes for {:.2} s (want about {want:.0})",
            rep.seconds()
        );
        // Read back: the same rate, and as long as what was rendered (LAME's delay and padding
        // are in its info frame, so the decoder drops them).
        let d = mp3::read(&out).unwrap();
        assert_eq!(d.rate, 48_000);
        assert_eq!(d.left.len() as u64, rep.frames, "{kbps} kbps");
        assert!(
            d.left.iter().any(|x| x.abs() > 0.01),
            "{kbps} kbps is silent"
        );

        let j = rep.to_json();
        assert_eq!(j["format"], "mp3");
        assert_eq!(j["bitrate_kbps"], kbps);
        assert!(j["bit_depth"].is_null(), "{j}");
        assert!(
            rep.summary().contains(&format!("{kbps} kbps")),
            "{}",
            rep.summary()
        );
    }
}

fn a_song_above_48k_is_written_at_48k_or_44k1() {
    let dir = Dir::new("songrate");
    let mut s = song("songrate", dir.path());
    voice(&mut s, "A", 60, 1.0);
    for (song_rate, want) in [(96_000, 48_000), (88_200, 44_100), (44_100, 44_100)] {
        run(
            &mut s,
            "project.settings",
            json!({ "sample_rate": song_rate }),
        );
        let out = dir.path().join(format!("m{song_rate}.mp3"));
        let rep = export::mixdown(s.project(), &out, &mp3_options(192)).unwrap();
        assert_eq!(rep.sample_rate, want, "a song at {song_rate} Hz");
        assert_eq!(mp3::read(&out).unwrap().rate, want);
    }
}

fn id3_tags_name_the_song_and_the_track() {
    let dir = Dir::new("id3");
    let mut s = song("夜明けの歌", dir.path());
    voice(&mut s, "ピアノ", 60, 1.0);
    voice(&mut s, "Bass", 36, 1.0);

    let mix = dir.path().join("mix.mp3");
    let r = run(
        &mut s,
        "export.mixdown",
        json!({ "out": mix.display().to_string(), "bitrate": 256 }),
    );
    assert_eq!(r.data["format"], "mp3");
    assert_eq!(r.data["bitrate_kbps"], 256);
    let d = mp3::read(&mix).unwrap();
    assert_eq!(
        (d.title.as_deref(), d.album.as_deref()),
        (Some("夜明けの歌"), None)
    );
    // The report says what it was titled (the export card's balloon).
    assert_eq!(
        r.data["tags"],
        json!({ "title": "夜明けの歌", "album": null })
    );

    let stems = dir.path().join("stems");
    let r = run(
        &mut s,
        "export.stems",
        json!({ "what": "tracks", "format": "mp3", "dir": stems.display().to_string(), "name": "{トラック}" }),
    );
    for name in ["ピアノ", "Bass"] {
        let d = mp3::read(&stems.join(format!("{name}.mp3"))).unwrap();
        assert_eq!(
            (d.title.as_deref(), d.album.as_deref()),
            (Some(name), Some("夜明けの歌"))
        );
    }
    let files = r.data["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .all(|f| f["tags"]["album"] == "夜明けの歌" && f["tags"]["title"] == f["name"]),
        "{files:?}"
    );
}

fn loudness_and_true_peak_are_those_of_the_mp3() {
    let dir = Dir::new("lufs");
    let mut s = song("lufs", dir.path());
    voice(&mut s, "A", 60, 2.0);
    voice(&mut s, "B", 43, 2.0);
    let wav = export::mixdown(s.project(), &dir.path().join("m.wav"), &Options::default()).unwrap();
    for kbps in [128, 320] {
        let out = dir.path().join(format!("m{kbps}.mp3"));
        let rep = export::mixdown(s.project(), &out, &mp3_options(kbps)).unwrap();
        let d = mp3::read(&out).unwrap();
        let (lufs, tp) = export::loudness(&d.left, &d.right, d.rate);
        let (lufs, tp) = (lufs.unwrap(), tp.unwrap());
        // The report is the decoded MP3's, not the render's before encoding.
        assert!((rep.lufs.unwrap() - lufs).abs() < 1e-9, "{kbps} kbps");
        assert!((rep.true_peak_db.unwrap() - tp).abs() < 1e-9, "{kbps} kbps");
        let peak = d
            .left
            .iter()
            .chain(&d.right)
            .fold(0.0f32, |m, x| m.max(x.abs()));
        assert_eq!(rep.peak, peak, "{kbps} kbps");
        // And close to the WAV's: the encoder changes the sound only a little.
        assert!(
            (lufs - wav.lufs.unwrap()).abs() < 0.5,
            "{kbps} kbps: {lufs} vs {}",
            wav.lufs.unwrap()
        );
        // The decoded MP3 is not the WAV sample for sample.
        assert_ne!(rep.true_peak_db, wav.true_peak_db, "{kbps} kbps");
    }
}

// ---- refused before the encoder is needed -------------------------------------------------

fn rates_above_48k_and_other_bitrates_are_refused() {
    let dir = Dir::new("refuse");
    let mut s = song("refuse", dir.path());
    voice(&mut s, "A", 60, 1.0);
    let out = dir.path().join("m.mp3");
    for rate in [88_200, 96_000, 192_000] {
        let o = Options {
            sample_rate: Some(rate),
            ..mp3_options(320)
        };
        let e = export::mixdown(s.project(), &out, &o).unwrap_err().0;
        assert!(e.contains(mp3::OVER_48K), "{e}");
    }
    let o = Options {
        sample_rate: Some(22_050),
        ..mp3_options(320)
    };
    let e = export::mixdown(s.project(), &out, &o).unwrap_err().0;
    assert!(e.contains("44100"), "{e}");
    let e = export::mixdown(s.project(), &out, &mp3_options(160))
        .unwrap_err()
        .0;
    assert!(e.contains("ビットレート"), "{e}");
    assert!(!out.exists());

    // Through the command too, before a job starts.
    let e = try_run(
        &mut s,
        "export.mixdown",
        json!({ "format": "mp3", "sample_rate": 96000, "out": out.display().to_string() }),
    )
    .unwrap_err();
    assert!(e.contains(mp3::OVER_48K), "{e}");
    assert!(!out.exists());
}

fn without_the_encoder_only_mp3_stops() {
    let dir = Dir::new("nolame");
    let mut s = song("nolame", dir.path());
    voice(&mut s, "A", 60, 1.0);
    let garbage = dir.path().join(mp3::LAME_FILE);
    std::fs::write(&garbage, b"not a library").unwrap();
    for lame in [dir.path().join("missing").join(mp3::LAME_FILE), garbage] {
        let with = |format| Options {
            format,
            lame: Some(lame.clone()),
            ..Options::default()
        };
        let out = dir.path().join("m.mp3");
        let e = export::mixdown(s.project(), &out, &with(Format::Mp3))
            .unwrap_err()
            .0;
        assert!(e.starts_with(mp3::CANNOT_LOAD), "{e}");
        // The reason names the file.
        assert!(e.contains(&lame.display().to_string()), "{e}");
        assert!(!out.exists());
        for (format, ext) in [(Format::Wav, "wav"), (Format::Flac, "flac")] {
            let out = dir.path().join(format!("m.{ext}"));
            let rep = export::mixdown(s.project(), &out, &with(format)).unwrap();
            assert_eq!(rep.format, format);
            assert!(out.exists());
        }
    }
    let e = mp3::load(Some(&dir.path().join("none.dll"))).unwrap_err();
    assert!(e.contains("none.dll"), "{e}");
}

fn the_tag_is_id3v2_3_in_utf16() {
    let tag = mp3::id3v2(&mp3::Tags {
        title: "曲".into(),
        album: Some("A".into()),
    });
    assert_eq!(&tag[..5], b"ID3\x03\x00");
    let size = tag[6..10]
        .iter()
        .fold(0usize, |n, b| (n << 7) | usize::from(*b));
    assert_eq!(size + 10, tag.len());
    let body = &tag[10..];
    assert_eq!(&body[..4], b"TIT2");
    // Encoding 1 (UTF-16 with a BOM), then the text.
    assert_eq!(&body[10..13], &[1, 0xFF, 0xFE]);
    assert!(tag.windows(4).any(|w| w == b"TALB"));
    assert!(
        !mp3::id3v2(&mp3::Tags::default())
            .windows(4)
            .any(|w| w == b"TALB")
    );
}

// ---- the harness --------------------------------------------------------------------------

/// The tests, whether each needs the encoder, and the test.
const TESTS: &[(&str, bool, fn())] = &[
    (
        "each_bitrate_writes_a_constant_bitrate_file",
        true,
        each_bitrate_writes_a_constant_bitrate_file,
    ),
    (
        "a_song_above_48k_is_written_at_48k_or_44k1",
        true,
        a_song_above_48k_is_written_at_48k_or_44k1,
    ),
    (
        "id3_tags_name_the_song_and_the_track",
        true,
        id3_tags_name_the_song_and_the_track,
    ),
    (
        "loudness_and_true_peak_are_those_of_the_mp3",
        true,
        loudness_and_true_peak_are_those_of_the_mp3,
    ),
    (
        "rates_above_48k_and_other_bitrates_are_refused",
        false,
        rates_above_48k_and_other_bitrates_are_refused,
    ),
    (
        "without_the_encoder_only_mp3_stops",
        false,
        without_the_encoder_only_mp3_stops,
    ),
    (
        "the_tag_is_id3v2_3_in_utf16",
        false,
        the_tag_is_id3v2_3_in_utf16,
    ),
];

/// Runs [`TESTS`] the way libtest would and says so in its lines: a name filter (`--exact`,
/// `--skip`) picks the tests, a test that needs the encoder is `ignored` with why when the encoder
/// cannot be loaded, and a failure makes the exit code 101.
fn main() {
    let mut filters = Vec::new();
    let mut skips = Vec::new();
    let mut exact = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--list" => {
                for (name, ..) in TESTS {
                    println!("{name}: test");
                }
                return;
            }
            "--exact" => exact = true,
            "--skip" => skips.extend(args.next()),
            "--test-threads" | "--format" | "--color" | "--logfile" | "--shuffle-seed" | "-Z" => {
                args.next();
            }
            a if a.starts_with('-') => {}
            _ => filters.push(a),
        }
    }
    let picked: Vec<_> = TESTS
        .iter()
        .filter(|(name, ..)| {
            let hit = |f: &String| {
                if exact {
                    name == f
                } else {
                    name.contains(f.as_str())
                }
            };
            (filters.is_empty() || filters.iter().any(hit)) && !skips.iter().any(hit)
        })
        .collect();
    let lame = mp3::load(None).err();
    let script = if cfg!(windows) {
        "scripts/build-lame.ps1"
    } else {
        "scripts/build-lame.sh"
    };
    println!("\nrunning {} tests", picked.len());
    let (mut passed, mut ignored, mut failed) = (0, 0, Vec::new());
    for &&(name, needs_lame, test) in &picked {
        if needs_lame && let Some(why) = &lame {
            println!(
                "test {name} ... ignored, {} が無いため（{script} で作れる）: {why}",
                mp3::LAME_FILE
            );
            ignored += 1;
            continue;
        }
        if std::panic::catch_unwind(test).is_ok() {
            println!("test {name} ... ok");
            passed += 1;
        } else {
            println!("test {name} ... FAILED");
            failed.push(name);
        }
    }
    if !failed.is_empty() {
        println!("\nfailures:");
        for name in &failed {
            println!("    {name}");
        }
    }
    println!(
        "\ntest result: {}. {passed} passed; {} failed; {ignored} ignored; 0 measured; {} filtered out\n",
        if failed.is_empty() { "ok" } else { "FAILED" },
        failed.len(),
        TESTS.len() - picked.len()
    );
    if !failed.is_empty() {
        std::process::exit(101);
    }
}
