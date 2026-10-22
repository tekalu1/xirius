//! The files the reference library of DAWproject writes (`testdata/reference`, MIT, from
//! github.com/bitwig/dawproject): each opens as a song, and the song written out and read again
//! is the same song.

use std::path::{Path, PathBuf};

use super::zip::ZipReader;
use super::*;
use crate::auto::AutoTarget;
use crate::model::{ClipBody, CtrlKind, CtrlShape, Project, Track, TrackKind};
use crate::time::Tick;

const REF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/dawproject/testdata/reference"
);

fn reference(name: &str) -> PathBuf {
    Path::new(REF).join(name)
}

fn track<'a>(p: &'a Project, name: &str) -> &'a Track {
    p.tracks
        .values()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no track {name}"))
}

/// The audio every audio example plays (`white-glasses.wav`, 3.097 seconds of stereo).
fn white_glasses() -> Vec<u8> {
    let f = std::fs::File::open(reference("AudioWARPEDWithFades.dawproject")).unwrap();
    let mut zip = ZipReader::new(std::io::BufReader::new(f)).unwrap();
    zip.read("white-glasses.wav").unwrap().unwrap()
}

fn wav_info(path: &Path) -> Option<AudioInfo> {
    path.is_file().then_some(AudioInfo {
        channels: 2,
        sample_rate: 44_100,
        seconds: 3.097,
    })
}

/// What a song holds that DAWproject carries, in a form two songs can be held against: the
/// tracks by name with their clips (place, notes, controllers, audio and fades) and
/// automation, the tempo, the meter and the markers.
fn summary(p: &Project) -> Vec<String> {
    let mut out = Vec::new();
    let tempo: Vec<_> = p.tempo.iter().map(|t| (t.tick, t.bpm, t.ramp)).collect();
    out.push(format!("tempo {tempo:?}"));
    let meter: Vec<_> = p.timesig.iter().map(|t| (t.bar, t.num, t.den)).collect();
    out.push(format!("meter {meter:?}"));
    for m in &p.markers {
        out.push(format!("marker {} {}", m.tick, m.name));
    }
    let mut tracks: Vec<&Track> = p.tracks.values().map(|t| &**t).collect();
    tracks.sort_by(|a, b| a.name.cmp(&b.name));
    for t in tracks {
        out.push(format!(
            "track {} {:?} {:.3} dB pan {:.3} {:?}",
            t.name, t.kind, t.volume_db, t.pan, t.color
        ));
        for c in &t.clips {
            let fx = p
                .fx(c.id)
                .map_or((0, 0), |f| (f.fade_in.length, f.fade_out.length));
            out.push(format!(
                "  clip {} {}+{} muted {} fades {fx:?}",
                c.name, c.start, c.length, c.muted
            ));
            match &c.body {
                ClipBody::Midi { notes, .. } => {
                    for n in notes.iter() {
                        out.push(format!(
                            "    note {}+{} {} {}",
                            n.start, n.length, n.pitch, n.velocity
                        ));
                    }
                }
                ClipBody::Audio {
                    file,
                    offset,
                    process,
                    ..
                } => {
                    let name = Path::new(file).file_name().unwrap().to_string_lossy();
                    out.push(format!("    audio {name} from {offset} {process:?}"));
                }
            }
            for l in &c.controllers {
                let pts: Vec<_> = l
                    .points
                    .iter()
                    .map(|p| (p.tick, p.value, p.shape))
                    .collect();
                out.push(format!("    {:?} {pts:?}", l.kind));
            }
        }
        for l in &t.automation {
            let what = match &l.target {
                AutoTarget::Param(_, k) => format!("param {k}"),
                AutoTarget::Send(_) => "send".into(),
                other => other.to_string(),
            };
            let pts: Vec<_> = l
                .points
                .iter()
                .map(|p| format!("{} {:.4} {}", p.tick, p.value, p.step))
                .collect();
            out.push(format!("  automation {what} {pts:?}"));
        }
    }
    out
}

/// Writes `p` out and reads it back; the two must hold the same.
fn round_trip(p: &Project, dir: &Path) -> Project {
    let out = dir.join(format!("{}.dawproject", p.name));
    let opts = Export {
        song_dir: Some(dir),
        embed_audio: true,
        audio: &wav_info,
    };
    let left = export(p, &out, &opts).unwrap();
    assert!(left.is_empty(), "{} left out {:?}", p.name, left.lines());
    keep_for_checks(&out);
    let back = import(&out, &dir.join("media back")).unwrap();
    assert!(
        back.skipped.is_empty(),
        "{} on the way back {:?}",
        p.name,
        back.skipped.lines()
    );
    assert_eq!(summary(&back.project), summary(p), "{}", p.name);
    back.project
}

/// With `XIRIUS_DAWPROJECT_OUT` set, the files written go there too, for checking them
/// against the schema (`scripts/dawproject_xsd.py`) or opening them in another DAW.
pub(super) fn keep_for_checks(file: &Path) {
    if let Ok(dir) = std::env::var("XIRIUS_DAWPROJECT_OUT") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::copy(file, dir.join(file.file_name().unwrap())).unwrap();
    }
}

#[test]
fn the_reference_librarys_files_read_with_the_zip_they_are_in() {
    // Java's ZIP writer puts the sizes after the data (bit 3), so only the central directory
    // knows them.
    let f = std::fs::File::open(reference("test.dawproject")).unwrap();
    let mut zip = ZipReader::new(std::io::BufReader::new(f)).unwrap();
    assert_eq!(zip.names(), ["metadata.xml", "project.xml"]);
    let xml = String::from_utf8(zip.read("project.xml").unwrap().unwrap()).unwrap();
    assert!(xml.starts_with("<?xml") && xml.trim_end().ends_with("</Project>"));
    assert_eq!(white_glasses().len(), 546_438);
}

#[test]
fn the_reference_librarys_song_of_notes_opens() {
    let dir = tempfile::tempdir().unwrap();
    let got = import(&reference("test.dawproject"), dir.path()).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped.lines());
    let p = &got.project;
    let ppq = Tick::from(p.ppq);
    assert_eq!(p.name, "test");
    assert_eq!(p.tracks.len(), 4, "three tracks and the master");
    for (i, (name, color, keys)) in [
        ("Track 1", "#000000", vec![36, 36]),
        ("Track 2", "#111111", vec![36, 48]),
        ("Track 3", "#222222", vec![36, 48, 60]),
    ]
    .into_iter()
    .enumerate()
    {
        let t = track(p, name);
        assert_eq!(t.kind, TrackKind::Instrument);
        assert_eq!(t.color.as_deref(), Some(color));
        assert_eq!(t.volume_db, 0.0);
        assert_eq!(t.clips.len(), 1);
        let c = &t.clips[0];
        assert_eq!(c.name, format!("Clip {i}"));
        assert_eq!((c.start, c.length), (8 * ppq * i as Tick, 4 * ppq));
        let ClipBody::Midi { notes, .. } = &c.body else {
            panic!("{name} plays notes")
        };
        assert_eq!(notes.len(), 8);
        for (k, n) in notes.iter().enumerate() {
            assert_eq!((n.start, n.length), (k as Tick * ppq / 2, ppq / 2));
            assert_eq!(n.pitch, keys[k % keys.len()]);
            assert_eq!(n.velocity, 102, "0.8 of 127");
        }
    }
    round_trip(p, dir.path());
}

#[test]
fn the_reference_librarys_aliases_automation_and_markers_open() {
    let dir = tempfile::tempdir().unwrap();
    let got = import(&reference("test-complex.dawproject"), dir.path()).unwrap();
    // The limiter on the master is a VST3 without an id, and its state is not in the file.
    assert_eq!(got.skipped.lines(), ["Master の Limiter の状態"]);
    let p = &got.project;
    let ppq = Tick::from(p.ppq);

    // An alias clip plays the notes of the clip it names.
    for (i, name) in ["Track 1", "Track 2", "Track 3"].into_iter().enumerate() {
        let t = track(p, name);
        let at: Vec<_> = t
            .clips
            .iter()
            .map(|c| (c.name.as_str(), c.start, c.length))
            .collect();
        let (one, alias) = (format!("Clip {i}"), format!("Alias Clip {i}"));
        let i = i as Tick;
        assert_eq!(
            at,
            [
                (one.as_str(), 8 * ppq * i, 4 * ppq),
                (alias.as_str(), (32 + 8 * i) * ppq, 4 * ppq)
            ]
        );
        let notes = |c: &crate::model::Clip| match &c.body {
            ClipBody::Midi { notes, .. } => notes
                .iter()
                .map(|n| (n.start, n.length, n.pitch))
                .collect::<Vec<_>>(),
            _ => panic!("{name} plays notes"),
        };
        assert_eq!(notes(&t.clips[0]).len(), 8);
        assert_eq!(notes(&t.clips[0]), notes(&t.clips[1]));
    }

    // The fader of track 1 rises from silence to 0 dB over two bars.
    let t1 = track(p, "Track 1");
    assert_eq!(t1.automation.len(), 1);
    let lane = &t1.automation[0];
    assert_eq!(lane.target, AutoTarget::Volume);
    let pts: Vec<_> = lane.points.iter().map(|p| (p.tick, p.value)).collect();
    assert_eq!(pts.len(), 2);
    assert_eq!(pts[0], (0, crate::model::MIN_DB));
    assert_eq!(pts[1].0, 8 * ppq);
    assert!(pts[1].1.abs() < 1e-9, "{}", pts[1].1);

    let marks: Vec<_> = p
        .markers
        .iter()
        .map(|m| (m.tick, m.name.as_str()))
        .collect();
    assert_eq!(marks, [(0, "Verse"), (24 * ppq, "Chorus")]);

    // The master keeps the limiter, to be found on a PC that has it.
    let master = p
        .tracks
        .values()
        .find(|t| t.kind == TrackKind::Master)
        .unwrap();
    assert_eq!(master.inserts.len(), 1);
    assert_eq!(
        (
            master.inserts[0].name.as_str(),
            master.inserts[0].format.as_str()
        ),
        ("Limiter", "vst3")
    );
    assert_eq!(master.inserts[0].state, None);

    round_trip(p, dir.path());
}

/// The CC 1 and pitch bend examples draw the same shape: 0, 0, ½, ½, 1, 1, ½ linear, then 1
/// and ½ held, a point a beat.
fn drawn(kind: CtrlKind) -> Vec<(Tick, i32, CtrlShape)> {
    let (lo, mid, hi) = match kind {
        CtrlKind::PitchBend => (-8192, 0, 8191),
        _ => (0, 64, 127),
    };
    let lin = CtrlShape::Linear;
    let step = CtrlShape::Step;
    [
        (lo, lin),
        (lo, lin),
        (mid, lin),
        (mid, lin),
        (hi, lin),
        (hi, lin),
        (mid, lin),
        (hi, step),
        (mid, step),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (v, s))| (i as Tick * 960, v, s))
    .collect()
}

#[test]
fn the_reference_librarys_controllers_open_in_clips_and_on_tracks() {
    for (file, kind, on_track) in [
        ("MIDI-CC1-AutomationInClips", CtrlKind::Cc(1), false),
        ("MIDI-CC1-AutomationOnTrack", CtrlKind::Cc(1), true),
        (
            "MIDI-PitchBend-AutomationInClips",
            CtrlKind::PitchBend,
            false,
        ),
        (
            "MIDI-PitchBend-AutomationOnTrack",
            CtrlKind::PitchBend,
            true,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let got = import(&reference(&format!("{file}.dawproject")), dir.path()).unwrap();
        assert!(got.skipped.is_empty(), "{file} {:?}", got.skipped.lines());
        let p = &got.project;
        let ppq = Tick::from(p.ppq);
        assert_eq!(ppq, 960, "the shape above is a point every 960 ticks");
        let t = track(p, "Notes");
        assert_eq!(t.kind, TrackKind::Instrument, "{file}");
        assert!(t.automation.is_empty(), "{file}");
        assert_eq!(t.clips.len(), 1, "{file}");
        let c = &t.clips[0];
        // In a clip of 8 beats the point on its end is past it; drawn on the track, a clip of
        // their own holds them all.
        let mut want = drawn(kind);
        if on_track {
            assert_eq!((c.start, c.length), (0, 9 * ppq), "{file}");
        } else {
            assert_eq!((c.start, c.length), (0, 8 * ppq), "{file}");
            want.pop();
        }
        assert_eq!(c.controllers.len(), 1, "{file}");
        assert_eq!(c.controllers[0].kind, kind, "{file}");
        let pts: Vec<_> = c.controllers[0]
            .points
            .iter()
            .map(|p| (p.tick, p.value, p.shape))
            .collect();
        assert_eq!(pts, want, "{file}");
        round_trip(p, dir.path());
    }
}

#[test]
fn controllers_drawn_on_a_track_go_into_its_clips() {
    let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Structure>
    <Track contentType="notes" id="t" name="Keys">
      <Channel role="regular" id="c"><Volume unit="linear" value="1" id="v"/></Channel>
    </Track>
  </Structure>
  <Arrangement id="a">
    <Lanes timeUnit="beats" id="l">
      <Clips track="t" id="cs">
        <Clip time="4" duration="4"><Notes id="n"><Note time="0" duration="1" key="60" vel="0.5"/></Notes></Clip>
      </Clips>
      <Points track="t" id="p" unit="normalized">
        <Target expression="channelController" controller="11"/>
        <RealPoint time="0" value="0.25" interpolation="hold"/>
        <RealPoint time="2" value="0.5" interpolation="hold"/>
        <RealPoint time="6" value="1" interpolation="hold"/>
        <RealPoint time="10" value="0" interpolation="hold"/>
      </Points>
    </Lanes>
  </Arrangement>
</Project>"##;
    let dir = tempfile::tempdir().unwrap();
    let got = import_from(xml, "Track CC", &mut |_| None, dir.path()).unwrap();
    // Three of the points are outside the clip; the value in force where it starts comes in.
    assert_eq!(
        got.skipped.lines(),
        ["Keys のクリップの外のオートメーション 3 点"]
    );
    let t = track(&got.project, "Keys");
    assert_eq!(t.clips.len(), 1);
    let c = &t.clips[0];
    assert_eq!(c.controllers.len(), 1);
    assert_eq!(c.controllers[0].kind, CtrlKind::Cc(11));
    let pts: Vec<_> = c.controllers[0]
        .points
        .iter()
        .map(|p| (p.tick, p.value))
        .collect();
    assert_eq!(pts, [(0, 64), (2 * 960, 127)]);
}

/// The audio examples (`xml/`): the arrangement in beats or seconds, the audio warped or not,
/// with fades, starting later in the audio (`Offset`) and later in the song (`Clipstart`).
#[test]
fn the_reference_librarys_audio_clips_open() {
    let wav = white_glasses();
    let beat = 60.0 / 155.0;
    for name in [
        "AudioWARPED",
        "AudioWARPEDWithFades",
        "AudioWARPEDOffset",
        "AudioWARPEDClipstart",
        "AudioRAW_BEATS",
        "AudioRAW_BEATSWithFades",
        "AudioRAW_BEATSOffset",
        "AudioRAW_BEATSClipstart",
        "AudioRAW_SECONDS",
        "AudioRAW_SECONDSWithFades",
        "AudioRAW_SECONDSOffset",
        "AudioRAW_SECONDSClipstart",
    ] {
        let xml = std::fs::read_to_string(reference(&format!("xml/{name}.xml"))).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut files = |n: &str| (n == "white-glasses.wav").then(|| wav.clone());
        let got = import_from(&xml, name, &mut files, &dir.path().join("media")).unwrap();
        assert!(got.skipped.is_empty(), "{name} {:?}", got.skipped.lines());
        assert_eq!(got.media.len(), 1, "{name}");
        assert_eq!(std::fs::read(&got.media[0]).unwrap(), wav, "{name}");
        let p = &got.project;
        let ppq = Tick::from(p.ppq);
        assert_eq!(p.tempo[0].bpm, 155.0, "{name}");
        let t = track(p, "Audio");
        assert_eq!(t.kind, TrackKind::Audio, "{name}");
        assert_eq!(t.clips.len(), 1, "{name}");
        let c = &t.clips[0];
        let ClipBody::Audio {
            file,
            offset,
            process,
            ..
        } = &c.body
        else {
            panic!("{name} plays audio")
        };
        assert!(file.ends_with("white-glasses.wav"), "{name} {file}");
        assert_eq!(*process, crate::model::AudioProcess::default(), "{name}");

        let seconds = name.contains("SECONDS");
        let ticks = |s: f64| (s / beat * ppq as f64).round() as Tick;
        // The clip starts a beat (or a second) later in the song.
        let start = match (name.ends_with("Clipstart"), seconds) {
            (false, _) => 0,
            (true, false) => ppq,
            (true, true) => ticks(1.0),
        };
        // The audio is 8 beats long at 155 BPM; the clips written in seconds are as long as it.
        let length = if seconds { ticks(3.097) } else { 8 * ppq };
        assert!((c.start - start).abs() <= 1, "{name} {} {start}", c.start);
        assert!(
            (c.length - length).abs() <= 1,
            "{name} {} {length}",
            c.length
        );
        // It plays from a beat into the warped audio, or a second into the plain one.
        let from = match (name.ends_with("Offset"), name.contains("WARPED")) {
            (false, _) => 0,
            (true, true) => ppq,
            (true, false) => ticks(1.0),
        };
        assert!((offset - from).abs() <= 1, "{name} {offset} {from}");
        let fades = p
            .fx(c.id)
            .map_or((0, 0), |f| (f.fade_in.length, f.fade_out.length));
        // A quarter of a beat on the warped audio, a tenth of a second on the plain one.
        let want = match (name.ends_with("WithFades"), name.contains("WARPED")) {
            (false, _) => (0, 0),
            (true, true) => (ppq / 4, ppq / 4),
            (true, false) => (ticks(0.1), ticks(0.1)),
        };
        assert!(
            (fades.0 - want.0).abs() <= 1 && (fades.1 - want.1).abs() <= 1,
            "{name} {fades:?} {want:?}"
        );
        round_trip(p, dir.path());
    }
}

#[test]
fn audio_outside_the_file_is_found_beside_it() {
    let wav = white_glasses();
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("test-data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("white-glasses.wav"), &wav).unwrap();
    let songs = dir.path().join("target");
    std::fs::create_dir_all(&songs).unwrap();
    let zip_of = |xml: &str, out: &Path| {
        let mut z = zip::ZipWriter::new(std::fs::File::create(out).unwrap());
        z.add("project.xml", xml.as_bytes(), true).unwrap();
        z.finish().unwrap();
    };

    // `../test-data/white-glasses.wav` is read from the folder of the file.
    let xml = std::fs::read_to_string(reference("xml/AudioFILE_WITH_RELATIVE_PATH.xml")).unwrap();
    let relative = songs.join("Relative.dawproject");
    zip_of(&xml, &relative);
    let got = import(&relative, &dir.path().join("media")).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped.lines());
    assert!(
        got.media.is_empty(),
        "audio outside the file stays where it is"
    );
    let ClipBody::Audio { file, .. } = &track(&got.project, "Audio").clips[0].body else {
        panic!("audio")
    };
    assert_eq!(std::fs::read(file).unwrap(), wav);

    // A path of the PC that wrote it: here, by its name beside the file.
    let xml = std::fs::read_to_string(reference("xml/AudioFILE_WITH_ABSOLUTE_PATH.xml")).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let written = doc
        .descendants()
        .find(|n| n.has_tag_name("File"))
        .and_then(|n| n.attribute("path"))
        .unwrap()
        .to_string();
    let elsewhere = xml.replace(&written, "Z:/no such folder/white-glasses.wav");
    let absolute = songs.join("Absolute.dawproject");
    zip_of(&elsewhere, &absolute);
    let none = import(&absolute, &dir.path().join("media")).unwrap();
    assert_eq!(
        none.skipped.lines(),
        ["見つからないファイル white-glasses.wav"]
    );
    std::fs::copy(
        data.join("white-glasses.wav"),
        songs.join("white-glasses.wav"),
    )
    .unwrap();
    let got = import(&absolute, &dir.path().join("media")).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped.lines());
    let ClipBody::Audio { file, .. } = &track(&got.project, "Audio").clips[0].body else {
        panic!("audio")
    };
    assert_eq!(Path::new(file), songs.join("white-glasses.wav"));
}

#[test]
fn a_vst3_is_named_by_its_class_id_however_the_file_writes_it() {
    let state = vstpreset::State {
        component: b"comp".to_vec(),
        controller: b"cont".to_vec(),
    };
    let class = "565354414D504C4573616D706C650000";
    let preset = vstpreset::encode(class, &state);
    let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Structure>
    <Track contentType="audio" id="t" name="Vox">
      <Channel role="regular" id="c">
        <Devices>
          <Vst3Plugin deviceName="Braced" deviceID="{56535441-4d50-4c45-7361-6d706c650000}" deviceRole="audioFX" id="d1">
            <State path="plugins/a.vstpreset"/>
          </Vst3Plugin>
          <Vst3Plugin deviceName="No id" deviceRole="audioFX" id="d2">
            <State path="plugins/b.vstpreset"/>
          </Vst3Plugin>
        </Devices>
      </Channel>
    </Track>
  </Structure>
</Project>"##;
    let dir = tempfile::tempdir().unwrap();
    let mut files = |n: &str| n.starts_with("plugins/").then(|| preset.clone());
    let got = import_from(xml, "Ids", &mut files, dir.path()).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped.lines());
    let vox = track(&got.project, "Vox");
    let ids: Vec<_> = vox.inserts.iter().map(|x| x.uid.as_str()).collect();
    assert_eq!(ids, [class, class]);
    let back = vstpreset::from_text(vox.inserts[0].state.as_deref().unwrap()).unwrap();
    assert_eq!(back.component, b"comp");
    assert_eq!(back.controller, b"cont");
}

#[test]
fn a_plugin_this_pc_does_not_have_is_kept_and_listed() {
    let dir = tempfile::tempdir().unwrap();
    let mut got = import_from(
        include_str!("testdata/bitwig-5.0-readme.xml"),
        "Bitwig",
        &mut |n| n.starts_with("plugins/").then(|| b"surge state".to_vec()),
        dir.path(),
    )
    .unwrap();
    let before = got.skipped.lines();
    // No scan to go by: nothing is said to be missing.
    not_here(&got.project, &mut got.skipped, &mut |_| None);
    assert_eq!(got.skipped.lines(), before);
    // A scan without Surge XT.
    not_here(&got.project, &mut got.skipped, &mut |_| Some(false));
    let mut want = before.clone();
    want.push("Bass の Surge XT（この PC に無い）".into());
    assert_eq!(got.skipped.lines(), want);
    let bass = track(&got.project, "Bass");
    assert_eq!(bass.inserts[0].name, "Surge XT");
    assert!(bass.inserts[0].state.is_some(), "the state goes with it");
}
