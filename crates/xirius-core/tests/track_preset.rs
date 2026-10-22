//! A track preset put on tracks and made into a track (MIX-030), and a preset file read the way a
//! song is (PRJ-007): what is missing is filled, what is unknown is left out, what is out of
//! range is rounded, what points at nothing is turned down, and every one of these is reported.

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::{PluginKind, Project, Track, TrackKind};
use xirius_core::session::{RunOptions, Session};
use xirius_core::track_preset::TrackPreset;

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_string()
}

fn track(s: &Session, t: &str) -> std::sync::Arc<Track> {
    let t: TrackId = t.parse().expect("a track id");
    s.project().tracks[&t].clone()
}

fn names(s: &Session, t: &str) -> Vec<String> {
    track(s, t).inserts.iter().map(|x| x.name.clone()).collect()
}

fn plugin(s: &mut Session, track: &str, name: &str, kind: &str) {
    run(
        s,
        "plugin.add",
        json!({ "track": track, "name": name, "kind": kind, "format": "builtin", "uid": name }),
    );
}

struct Song {
    s: Session,
    a: String,
    b: String,
    synth: String,
    bus: String,
}

/// A: two effects, a send to Verb, -6 dB, pan 0.5, orange. B: another effect. Synth: an
/// instrument and an effect.
fn song() -> Song {
    let mut s = Session::new("presets");
    let a = id(&run(&mut s, "track.add.audio", json!({ "name": "A" })));
    let b = id(&run(&mut s, "track.add.audio", json!({ "name": "B" })));
    let synth = id(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Synth" }),
    ));
    let bus = id(&run(&mut s, "track.add.bus", json!({ "name": "Verb" })));
    plugin(&mut s, &a, "Compressor", "effect");
    plugin(&mut s, &a, "EQ", "effect");
    plugin(&mut s, &b, "Reverb", "effect");
    plugin(&mut s, &synth, "Sampler", "instrument");
    plugin(&mut s, &synth, "Chorus", "effect");
    run(
        &mut s,
        "send.add",
        json!({ "ids": [a], "to": bus, "level": -3.0, "pre": true }),
    );
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [a], "db": -6.0 }),
    );
    run(&mut s, "track.pan.set", json!({ "ids": [a], "pan": 0.5 }));
    run(
        &mut s,
        "track.color.set",
        json!({ "ids": [a], "color": "#ff8800" }),
    );
    Song {
        s,
        a,
        b,
        synth,
        bus,
    }
}

fn preset_of(s: &Session, t: &str, name: &str) -> Value {
    let id: TrackId = t.parse().unwrap();
    TrackPreset::snapshot(s.project(), id, name)
        .unwrap()
        .to_value()
}

fn put(s: &mut Session, ids: &[&str], preset: &Value) -> Value {
    run(
        s,
        "track.preset.put",
        json!({ "ids": ids, "preset": preset.to_string() }),
    )
}

#[test]
fn a_preset_put_on_a_track_takes_its_effects_sends_fader_pan_and_colour() {
    let Song {
        mut s, a, b, bus, ..
    } = song();
    let preset = preset_of(&s, &a, "Vocal");
    let out = put(&mut s, &[&b], &preset);
    assert_eq!(out["adjusted"], json!([]));
    let t = track(&s, &b);
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
    assert_eq!((t.volume_db, t.pan), (-6.0, 0.5));
    assert_eq!(t.color.as_deref(), Some("#ff8800"));
    assert_eq!(t.name, "B", "the name stays");
    assert_eq!(t.preset.as_deref(), Some("Vocal"));
    assert_eq!(t.sends.len(), 1);
    assert_eq!(t.sends[0].to.to_string(), bus);
    assert_eq!((t.sends[0].level_db, t.sends[0].pre), (-3.0, true));
    let src = track(&s, &a);
    assert_ne!(t.inserts[0].id, src.inserts[0].id, "new ids");
}

#[test]
fn one_undo_takes_back_a_preset_put_on_many_tracks() {
    let Song {
        mut s, a, b, synth, ..
    } = song();
    let before = s.project().content_hash();
    let preset = preset_of(&s, &a, "Vocal");
    put(&mut s, &[&b, &synth], &preset);
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
    assert_eq!(names(&s, &synth), ["Sampler", "Compressor", "EQ"]);
    s.undo(1).unwrap();
    assert_eq!(s.project().content_hash(), before);
    s.redo(1).unwrap();
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
}

#[test]
fn the_instrument_goes_only_to_an_instrument_track() {
    let Song {
        mut s, a, b, synth, ..
    } = song();
    let preset = preset_of(&s, &synth, "Keys");
    // on an audio track the instrument is left out, and the row says so
    let out = put(&mut s, &[&b], &preset);
    assert_eq!(names(&s, &b), ["Chorus"]);
    let rows = out["adjusted"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], json!("dropped"));
    assert_eq!(rows[0]["what"], json!("Sampler"));
    // on another instrument track it replaces the instrument
    let other = id(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Other" }),
    ));
    plugin(&mut s, &other, "Pad", "instrument");
    put(&mut s, &[&other], &preset);
    assert_eq!(names(&s, &other), ["Sampler", "Chorus"]);
    assert_eq!(track(&s, &other).inserts[0].kind, PluginKind::Instrument);
    // a preset of an audio track leaves the instrument of an instrument track alone
    let audio = preset_of(&s, &a, "Vocal");
    put(&mut s, &[&other], &audio);
    assert_eq!(names(&s, &other), ["Sampler", "Compressor", "EQ"]);
}

#[test]
fn a_new_track_is_made_from_a_preset_and_named_after_it() {
    let Song { mut s, synth, .. } = song();
    let preset = preset_of(&s, &synth, "Keys");
    let before = s.project().content_hash();
    let out = run(
        &mut s,
        "track.preset.make",
        json!({ "preset": preset.to_string() }),
    );
    let t = track(&s, &id(&out));
    assert_eq!((t.name.as_str(), t.kind), ("Keys", TrackKind::Instrument));
    assert_eq!(names(&s, &t.id.to_string()), ["Sampler", "Chorus"]);
    assert_eq!(t.preset.as_deref(), Some("Keys"));
    // a second one does not take a name that is in use
    let again = run(
        &mut s,
        "track.preset.make",
        json!({ "preset": preset.to_string() }),
    );
    assert_ne!(track(&s, &id(&again)).name, "Keys");
    s.undo(2).unwrap();
    assert_eq!(s.project().content_hash(), before);
}

#[test]
fn the_order_of_the_effects_is_the_saved_order() {
    let Song { mut s, a, .. } = song();
    let second = track(&s, &a).inserts[1].id.to_string();
    run(&mut s, "plugin.move", json!({ "id": second, "index": 0 }));
    assert_eq!(names(&s, &a), ["EQ", "Compressor"]);
    let preset = preset_of(&s, &a, "Reordered");
    let out = run(
        &mut s,
        "track.preset.make",
        json!({ "preset": preset.to_string() }),
    );
    assert_eq!(names(&s, &id(&out)), ["EQ", "Compressor"]);
}

#[test]
fn a_preset_from_another_version_is_read_and_every_change_is_reported() {
    let Song { mut s, b, .. } = song();
    let preset = json!({
        "version": 7,
        "name": "Wild",
        "kind": "audio",
        "color": "orange",
        "volume_db": 40.0,
        "future": { "x": 1 },
        "inserts": [
            { "name": "Reverb", "kind": "effect", "format": "builtin", "uid": "Reverb",
              "bypass": false, "params": { "mix": 1.5, "size": "big" }, "state": "***",
              "later": 1 },
            { "name": "Mystery", "kind": "effect" },
            42
        ],
        "sends": [
            { "to": "t99999", "to_name": "Gone", "level_db": -200.0 },
            {}
        ]
    });
    let color = track(&s, &b).color.clone();
    let out = put(&mut s, &[&b], &preset);
    let rows = out["adjusted"].as_array().unwrap();
    let find = |kind: &str, what: &str| {
        rows.iter()
            .any(|r| r["kind"] == json!(kind) && r["what"].as_str().unwrap().contains(what))
    };
    assert!(find("dropped", "future"), "{rows:?}");
    assert!(find("dropped", "later"), "{rows:?}");
    assert!(find("dropped", "色"), "{rows:?}");
    assert!(find("rounded", "ボリューム"), "{rows:?}");
    assert!(find("rounded", "mix"), "{rows:?}");
    assert!(find("dropped", "size"), "{rows:?}");
    assert!(find("dropped", "状態"), "{rows:?}");
    assert!(find("filled", "パン"), "{rows:?}");
    assert!(find("rejected", "エフェクト 2"), "{rows:?}");
    assert!(find("rejected", "エフェクト 3"), "{rows:?}");
    assert!(find("rejected", "Gone"), "{rows:?}");
    assert!(find("rejected", "センド 2"), "{rows:?}");
    let t = track(&s, &b);
    assert_eq!(t.volume_db, 12.0);
    assert_eq!(t.pan, 0.0);
    assert_eq!(t.color, color);
    assert_eq!(names(&s, &b), ["Reverb"]);
    assert_eq!(t.inserts[0].params["mix"], 1.0);
    assert!(!t.inserts[0].params.contains_key("size"));
    assert!(t.sends.is_empty());
}

#[test]
fn a_preset_that_is_not_an_object_is_an_error() {
    let Song { mut s, b, .. } = song();
    let e = s
        .run_value(
            "track.preset.put",
            json!({ "ids": [b], "preset": "[1]" }),
            &RunOptions::human(),
        )
        .expect_err("not a preset");
    assert!(format!("{e:?}").contains("プリセット"));
    s.run_value(
        "track.preset.put",
        json!({ "ids": [b], "preset": "not json" }),
        &RunOptions::human(),
    )
    .expect_err("not json");
}

#[test]
fn a_plugin_that_is_not_on_this_pc_stays_as_an_empty_slot_with_a_row() {
    let Song { mut s, a, b, .. } = song();
    let preset = preset_of(&s, &a, "Vocal");
    let out = run(
        &mut s,
        "track.preset.put",
        json!({ "ids": [b], "preset": preset.to_string(), "missing": "EQ" }),
    );
    assert_eq!(names(&s, &b), ["Compressor", "EQ"], "the slot stays");
    let rows = out["adjusted"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["what"], json!("EQ"));
    assert!(
        rows[0]["reason"]
            .as_str()
            .unwrap()
            .contains("この PC に無い")
    );
}

#[test]
fn a_send_finds_its_bus_by_name_in_another_song() {
    let Song { s, a, .. } = song();
    let preset = preset_of(&s, &a, "Vocal");
    let mut other = Session::new("other");
    let t = id(&run(&mut other, "track.add.audio", json!({ "name": "T" })));
    let verb = id(&run(&mut other, "track.add.bus", json!({ "name": "Verb" })));
    let out = put(&mut other, &[&t], &preset);
    assert_eq!(out["adjusted"], json!([]));
    assert_eq!(track(&other, &t).sends[0].to.to_string(), verb);
    // and with no bus of that name, the send is turned down
    let mut bare = Session::new("bare");
    let t = id(&run(&mut bare, "track.add.audio", json!({ "name": "T" })));
    let out = put(&mut bare, &[&t], &preset);
    assert_eq!(out["adjusted"][0]["kind"], json!("rejected"));
    assert!(track(&bare, &t).sends.is_empty());
}

#[test]
fn the_master_has_no_preset() {
    let Song { mut s, a, .. } = song();
    let preset = preset_of(&s, &a, "Vocal");
    s.run_value(
        "track.preset.put",
        json!({ "ids": ["master"], "preset": preset.to_string() }),
        &RunOptions::human(),
    )
    .expect_err("master");
    assert!(TrackPreset::snapshot(s.project(), "master".parse().unwrap(), "M").is_err());
}

#[test]
fn a_saved_preset_reads_back_the_same_without_a_row() {
    let Song { s, synth, .. } = song();
    let preset = preset_of(&s, &synth, "Keys");
    let (back, rows) = TrackPreset::parse(&preset).unwrap();
    assert!(rows.is_empty(), "{rows:?}");
    assert_eq!(back.to_value(), preset);
    assert_eq!(back.kind, TrackKind::Instrument);
}

#[test]
fn the_origin_survives_a_save_and_a_reopen_and_an_old_song_has_none() {
    let Song { mut s, a, b, .. } = song();
    let preset = preset_of(&s, &a, "Vocal");
    put(&mut s, &[&b], &preset);
    let json = serde_json::to_string(s.project()).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    let t = &back.tracks[&b.parse::<TrackId>().unwrap()];
    assert_eq!(t.preset.as_deref(), Some("Vocal"));
    // a song written before the key existed has no origin
    let mut v: Value = serde_json::from_str(&json).unwrap();
    for t in v["tracks"].as_object_mut().unwrap().values_mut() {
        t.as_object_mut().unwrap().remove("preset");
    }
    let old: Project = serde_json::from_value(v).unwrap();
    assert!(old.tracks.values().all(|t| t.preset.is_none()));
}

#[test]
fn origin_is_one_undoable_step() {
    let Song { mut s, a, .. } = song();
    let before = s.project().content_hash();
    run(
        &mut s,
        "track.preset.origin",
        json!({ "ids": [a], "name": "Vocal" }),
    );
    assert_eq!(track(&s, &a).preset.as_deref(), Some("Vocal"));
    s.undo(1).unwrap();
    assert_eq!(s.project().content_hash(), before);
}

#[test]
fn a_bus_takes_a_preset_too() {
    let Song { mut s, a, bus, .. } = song();
    let preset = preset_of(&s, &a, "Vocal");
    // the send to itself is turned down; the effects and the fader are taken
    let out = put(&mut s, &[&bus], &preset);
    assert_eq!(names(&s, &bus), ["Compressor", "EQ"]);
    assert_eq!(out["adjusted"][0]["kind"], json!("rejected"));
    assert!(track(&s, &bus).sends.is_empty());
}
