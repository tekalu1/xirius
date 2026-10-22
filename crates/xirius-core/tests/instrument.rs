//! PLG-019: swapping an instrument or an insert for another in its place.

use serde_json::{Value, json};
use xirius_core::model::PluginKind;
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"].as_str().expect("id").to_string()
}

fn names(s: &Session, track: &str) -> Vec<(String, PluginKind)> {
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.id.to_string() == track)
        .expect("track");
    t.inserts.iter().map(|x| (x.name.clone(), x.kind)).collect()
}

#[test]
fn the_instrument_is_swapped_in_place_and_effects_stay_behind_it() {
    let mut s = Session::new("replace");
    let t = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({"name": "Keys"}),
    ));
    run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Synth", "kind": "instrument"}),
    );
    run(&mut s, "plugin.add", json!({"track": t, "name": "EQ"}));
    let r = run(
        &mut s,
        "plugin.replace",
        json!({"slot": "instrument", "track": t, "name": "Sampler"}),
    );
    assert_eq!(r.message, "「Synth」を「Sampler」に差し替えました");
    assert_eq!(
        names(&s, &t),
        [
            ("Sampler".into(), PluginKind::Instrument),
            ("EQ".into(), PluginKind::Effect)
        ]
    );
    // one step: undo brings the old instrument back
    s.undo(1).unwrap();
    assert_eq!(names(&s, &t)[0].0, "Synth");
}

#[test]
fn a_track_without_an_instrument_gets_one_in_front() {
    let mut s = Session::new("fill");
    let t = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({"name": "Keys"}),
    ));
    run(&mut s, "plugin.add", json!({"track": t, "name": "EQ"}));
    let r = run(
        &mut s,
        "plugin.replace",
        json!({"slot": "instrument", "track": t, "name": "Sampler"}),
    );
    assert_eq!(r.message, "インストゥルメントに「Sampler」を入れました");
    assert_eq!(names(&s, &t)[0], ("Sampler".into(), PluginKind::Instrument));
    assert_eq!(names(&s, &t)[1].0, "EQ");
}

#[test]
fn an_effect_keeps_its_place_and_its_kind() {
    let mut s = Session::new("effect");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vox"})));
    run(&mut s, "plugin.add", json!({"track": t, "name": "EQ"}));
    let comp = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Comp"}),
    ));
    run(&mut s, "plugin.add", json!({"track": t, "name": "Delay"}));
    run(
        &mut s,
        "plugin.replace",
        json!({"id": comp, "name": "Limiter", "format": "vst3", "uid": "42"}),
    );
    let got: Vec<String> = names(&s, &t).into_iter().map(|(n, _)| n).collect();
    assert_eq!(got, ["EQ", "Limiter", "Delay"]);
}

#[test]
fn only_an_instrument_track_takes_an_instrument() {
    let mut s = Session::new("audio");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let e = s
        .run_value(
            "plugin.replace",
            json!({"slot": "instrument", "track": t, "name": "Sampler"}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(e.message.contains("インストゥルメントトラック"), "{e:?}");
    let e = s
        .run_value(
            "plugin.replace",
            json!({"name": "EQ"}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(e.message.contains("--id"), "{e:?}");
}

#[test]
fn a_preset_sets_its_values_in_one_step() {
    let mut s = Session::new("preset");
    let t = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({"name": "Keys"}),
    ));
    let x = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Sampler", "kind": "instrument"}),
    ));
    let params = |s: &Session| {
        s.project()
            .tracks
            .values()
            .flat_map(|t| t.inserts.iter())
            .find(|p| p.id.to_string() == x)
            .unwrap()
            .params
            .clone()
    };
    // The built-in Sampler starts with its default values (PLG-045).
    let before = params(&s);
    let steps = s.steps().len();
    let r = run(
        &mut s,
        "plugin.preset.load",
        json!({"id": x, "preset": "Bright",
               "values": [{"param": "Cutoff", "value": 0.9}, {"param": "Attack", "value": 0.1}]}),
    );
    assert_eq!(r.message, "Sampler のプリセットを「Bright」にしました");
    assert_eq!(s.steps().len(), steps + 1);
    assert_eq!(params(&s).get("Cutoff"), Some(&0.9));
    assert_eq!(params(&s).get("Attack"), Some(&0.1));
    s.undo(1).unwrap();
    assert_eq!(params(&s), before);
    let e = s
        .run_value(
            "plugin.preset.load",
            json!({"id": x, "preset": "Bad", "values": [{"param": "Cutoff"}]}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(e.message.contains("1 番目"), "{e:?}");
}

#[test]
fn a_track_is_made_with_its_instrument_and_armed() {
    let mut s = Session::new("with");
    let r = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument": "Sampler", "arm": true}),
    );
    assert_eq!(r.message, "トラック「Sampler」を作成し、録音準備にしました");
    assert!(r.data["clip"].is_null());
    let t = id_of(&r);
    assert_eq!(names(&s, &t), [("Sampler".into(), PluginKind::Instrument)]);
    let track = s
        .project()
        .tracks
        .values()
        .find(|x| x.id.to_string() == t)
        .unwrap();
    assert!(track.armed && track.clips.is_empty());
    // the second one is named apart
    let r = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument": "Sampler"}),
    );
    let t2 = id_of(&r);
    let second = s
        .project()
        .tracks
        .values()
        .find(|x| x.id.to_string() == t2)
        .unwrap();
    assert_eq!(second.name, "Sampler 2");
    assert!(!second.armed);
    // one step each
    s.undo(1).unwrap();
    assert_eq!(
        s.project()
            .tracks
            .values()
            .filter(|x| x.name.starts_with("Sampler"))
            .count(),
        1
    );
}

#[test]
fn notes_go_in_a_clip_of_whole_bars() {
    let mut s = Session::new("notes");
    let r = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument": "Sampler", "name": "Song", "clip": "song.mid", "notes": [
            {"at": 0, "pitch": 60, "length": 480, "velocity": 90},
            {"at": 3840, "pitch": 64, "length": 960}
        ]}),
    );
    assert_eq!(r.message, "トラック「Song」にノート 2 個を置きました");
    let t = id_of(&r);
    let track = s
        .project()
        .tracks
        .values()
        .find(|x| x.id.to_string() == t)
        .unwrap();
    let clip = &track.clips[0];
    assert_eq!(clip.name, "song.mid");
    assert_eq!(r.data["clip"], json!(clip.id.to_string()));
    // 4/4 at 960 ticks a beat: the last note ends in bar 2, so the clip is 2 bars
    assert_eq!((clip.start, clip.length), (0, 7680));
    let notes = clip.notes();
    assert_eq!(notes.len(), 2);
    assert_eq!((notes[0].pitch, notes[0].velocity), (60, 90));
    assert_eq!(notes[1].velocity, 100);
    assert_ne!(notes[0].id, notes[1].id);
    let e = s
        .run_value(
            "track.add.with-instrument",
            json!({"instrument": "Sampler", "notes": [{"at": 0, "pitch": 200, "length": 1}]}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(e.message.contains("pitch"), "{e:?}");
}

/// B-4: a note's `at` and `length` take `bar.beat.tick` as the top-level `at` does. The
/// position is read in the song and the note's start is taken from the clip's start.
#[test]
fn notes_take_a_position_as_bar_beat_tick() {
    let mut s = Session::new("bbt");
    let r = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument": "Sampler", "at": "1.2.0", "notes": [
            {"at": "1.3.0", "pitch": 60, "length": "1.4.0"},
            {"at": 480, "pitch": 64, "length": 240}
        ]}),
    );
    let t = id_of(&r);
    let track = s
        .project()
        .tracks
        .values()
        .find(|x| x.id.to_string() == t)
        .unwrap();
    let clip = &track.clips[0];
    assert_eq!(clip.start, 960);
    let notes = clip.notes();
    assert_eq!(
        (notes[0].start, notes[0].length, notes[0].pitch),
        (960, 2880, 60)
    );
    assert_eq!(
        (notes[1].start, notes[1].length, notes[1].pitch),
        (480, 240, 64)
    );
}

/// B-4: a value that is neither a tick nor `bar.beat.tick` is refused with the shape to use,
/// not with "there is no at".
#[test]
fn a_note_position_that_cannot_be_read_says_the_shape() {
    let mut s = Session::new("bbt-bad");
    let e = s
        .run_value(
            "track.add.with-instrument",
            json!({"instrument": "Sampler", "notes": [{"at": "later", "pitch": 60, "length": 10}]}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(e.message.contains("tick の整数か"), "{e:?}");
    assert!(e.message.contains("小節.拍.tick"), "{e:?}");
    assert!(!e.message.contains("ありません"), "{e:?}");
}
