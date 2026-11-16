//! PLG-069: a pad bus appears in offline stems and the complete export.

use serde_json::{Value, json};
use xirius_core::{Session, id::TrackId, model::Project, session::RunOptions};
use xirius_session::{
    build::Builder,
    commands,
    export::{self, Rendered, write_wav},
    ext::ExtConfig,
    graph::Select,
};
mod crash_records;

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn render(s: &Session, select: Select) -> Rendered {
    export::render(
        &mut Builder::new(48_000, s.dir().map(std::path::Path::to_path_buf)),
        s.project(),
        select,
        0,
        8_192,
        0,
    )
    .unwrap()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |p, x| p.max(x.abs()))
}

#[test]
fn bouncing_routed_pads_does_not_repeat_the_main_path_into_a_bus() {
    let _crash = crash_records::CrashRecords::new();
    for route in ["output", "send", "folder-pad", "folder-downstream"] {
        for bounce in ["track.bounce", "range.bounce", "clip.bounce"] {
            let tmp = tempfile::tempdir().unwrap();
            let kick = tmp.path().join("kick.wav");
            let snare = tmp.path().join("snare.wav");
            write_wav(&kick, &vec![0.2; 2_048], &vec![0.2; 2_048], 48_000, 24).unwrap();
            write_wav(&snare, &vec![0.1; 2_048], &vec![0.1; 2_048], 48_000, 24).unwrap();
            let mut s =
                Session::with_registry(Project::new("pad bounce"), commands::shared_registry());
            commands::attach(&mut s, ExtConfig::default());
            let made = run(
                &mut s,
                "track.add.with-instrument",
                json!({"instrument":"Drum Kit"}),
            );
            let track: TrackId = made["id"].as_str().unwrap().parse().unwrap();
            let kit = made["plugin"].clone();
            for (pad, path) in [("A1", &kick), ("A2", &snare)] {
                run(
                    &mut s,
                    "plugin.samples.load",
                    json!({"id":kit,"pad":pad,"path":path}),
                );
            }
            let clip = run(
                &mut s,
                "clip.add.midi",
                json!({"track":track,"at":0,"bars":1}),
            )["id"]
                .clone();
            for pitch in [36, 37] {
                run(
                    &mut s,
                    "note.add",
                    json!({"clip":clip,"at":0,"pitch":pitch,"length":480,"velocity":100}),
                );
            }
            let bus: TrackId = run(
                &mut s,
                "plugin.pad_output.set",
                json!({"id":kit,"pads":[{"pad":"A1"}],"to":"new"}),
            )["output"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            let destination = if route.starts_with("folder") {
                let folder_bus: TrackId =
                    run(&mut s, "folder.pack", json!({"ids":[track,bus]}))["bus"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap();
                if route == "folder-pad" {
                    run(
                        &mut s,
                        "plugin.pad_output.set",
                        json!({"id":kit,"pads":[{"pad":"A1"}],"to":folder_bus}),
                    );
                }
                folder_bus
            } else {
                if route == "output" {
                    run(&mut s, "track.output.set", json!({"ids":[track],"to":bus}));
                } else {
                    run(&mut s, "send.add", json!({"ids":[track],"to":bus}));
                }
                bus
            };
            let before = render(&s, Select::All);
            let stem_before = render(&s, Select::Bus(destination));
            assert!(peak(&before.left) > 0.01, "{route} {bounce}");
            s.save_as(tmp.path().join("song")).unwrap();
            if bounce == "track.bounce" {
                run(&mut s, bounce, json!({"ids":[track]}));
            } else if bounce == "clip.bounce" {
                run(&mut s, bounce, json!({"ids":[clip],"new-track":true}));
            } else {
                run(
                    &mut s,
                    bounce,
                    json!({"from":0,"to":3840,"tracks":[track],"per-track":true,"tail":0}),
                );
            }
            let after = render(&s, Select::All);
            let stem_after = render(&s, Select::Bus(destination));
            for i in 0..before.left.len() {
                assert!(
                    (before.left[i] - after.left[i]).abs() < 1e-5,
                    "{route} {bounce} frame {i}: before={} after={}",
                    before.left[i],
                    after.left[i]
                );
                assert!(
                    (stem_before.left[i] - stem_after.left[i]).abs() < 1e-5,
                    "{route} {bounce} bus stem frame {i}: before={} after={}",
                    stem_before.left[i],
                    stem_after.left[i]
                );
            }
        }
    }
}

#[test]
fn pad_bus_stem_contains_only_its_pad_and_the_whole_export_keeps_the_mix() {
    let _crash = crash_records::CrashRecords::new();
    let tmp = tempfile::tempdir().unwrap();
    let kick = tmp.path().join("kick.wav");
    let snare = tmp.path().join("snare.wav");
    write_wav(&kick, &vec![0.2; 2_048], &vec![0.2; 2_048], 48_000, 24).unwrap();
    write_wav(&snare, &vec![0.1; 2_048], &vec![0.1; 2_048], 48_000, 24).unwrap();
    let mut s = Session::with_registry(Project::new("pad stems"), commands::shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    let made = run(
        &mut s,
        "track.add.with-instrument",
        json!({"instrument":"Drum Kit"}),
    );
    let track: TrackId = made["id"].as_str().unwrap().parse().unwrap();
    let kit = made["plugin"].clone();
    for (pad, path) in [("A1", &kick), ("A2", &snare)] {
        run(
            &mut s,
            "plugin.samples.load",
            json!({"id":kit,"pad":pad,"path":path}),
        );
    }
    let clip = run(
        &mut s,
        "clip.add.midi",
        json!({"track":track,"at":0,"bars":1}),
    )["id"]
        .clone();
    for pitch in [36, 37] {
        run(
            &mut s,
            "note.add",
            json!({"clip":clip,"at":0,"pitch":pitch,"length":480,"velocity":100}),
        );
    }
    let before = render(&s, Select::All);
    let bus: TrackId = run(
        &mut s,
        "plugin.pad_output.set",
        json!({"id":kit,"pads":[{"pad":"A1"}],"to":"new"}),
    )["output"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let main = render(&s, Select::Track(track));
    let routed = render(&s, Select::Bus(bus));
    let whole = render(&s, Select::All);
    assert!(peak(&main.left) > 0.01);
    assert!(peak(&routed.left) > 0.01);
    for i in 0..before.left.len() {
        assert!((whole.left[i] - before.left[i]).abs() < 1e-5, "frame {i}");
        assert!((whole.right[i] - before.right[i]).abs() < 1e-5, "frame {i}");
        assert!(
            (main.left[i] + routed.left[i] - whole.left[i]).abs() < 1e-5,
            "stem frame {i}: main={} bus={} whole={}",
            main.left[i],
            routed.left[i],
            whole.left[i]
        );
    }
    assert!(peak(&routed.left) > peak(&main.left));

    run(&mut s, "track.solo", json!({"ids":[bus]}));
    let bus_solo = render(&s, Select::All);
    for i in 0..routed.left.len() {
        assert!(
            (bus_solo.left[i] - routed.left[i]).abs() < 1e-5,
            "bus solo frame {i}: actual={} routed={}",
            bus_solo.left[i],
            routed.left[i]
        );
    }
    run(&mut s, "track.solo.clear", json!({}));
    run(&mut s, "track.solo", json!({"ids":[track]}));
    let kit_solo = render(&s, Select::All);
    for i in 0..whole.left.len() {
        assert!(
            (kit_solo.left[i] - whole.left[i]).abs() < 1e-5,
            "kit solo frame {i}"
        );
    }
    run(&mut s, "track.solo.clear", json!({}));
    run(&mut s, "track.mute", json!({"ids":[bus]}));
    let bus_muted = render(&s, Select::All);
    for i in 0..main.left.len() {
        assert!(
            (bus_muted.left[i] - main.left[i]).abs() < 1e-5,
            "bus mute frame {i}"
        );
    }
    run(&mut s, "track.mute", json!({"ids":[bus]}));
    run(&mut s, "track.mute", json!({"ids":[track]}));
    assert!(peak(&render(&s, Select::All).left) < 1e-7);
    run(&mut s, "track.mute", json!({"ids":[track]}));

    s.save_as(tmp.path().join("song")).unwrap();
    let args = json!({"from":0,"to":3840,"tracks":[track],"tail":0});
    commands::run(
        &mut s,
        None,
        "range.bounce",
        args.as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap();
    let bounced = render(&s, Select::All);
    for i in 0..whole.left.len() {
        assert!(
            (bounced.left[i] - whole.left[i]).abs() < 1e-5,
            "bounce frame {i}: before={} after={}",
            whole.left[i],
            bounced.left[i]
        );
    }
    s.undo(1).unwrap();
    let track_args = json!({"ids":[track]});
    commands::run(
        &mut s,
        None,
        "track.bounce",
        track_args.as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap();
    let bounced = render(&s, Select::All);
    for i in 0..whole.left.len() {
        assert!(
            (bounced.left[i] - whole.left[i]).abs() < 1e-5,
            "track bounce frame {i}: before={} after={}",
            whole.left[i],
            bounced.left[i]
        );
    }

    s.undo(1).unwrap();
    let other = run(&mut s, "track.add.audio", json!({"name":"other"}))["id"].clone();
    run(
        &mut s,
        "clip.add.audio",
        json!({"track":other,"at":0,"length":3840,"file":snare}),
    );
    run(&mut s, "track.output.set", json!({"ids":[other],"to":bus}));
    let with_other = render(&s, Select::All);
    let args = json!({"from":0,"to":3840,"tracks":[track],"tail":0});
    commands::run(
        &mut s,
        None,
        "range.bounce",
        args.as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap();
    let bounced = render(&s, Select::All);
    for i in 0..with_other.left.len() {
        assert!(
            (bounced.left[i] - with_other.left[i]).abs() < 1e-5,
            "other source frame {i}: before={} after={}",
            with_other.left[i],
            bounced.left[i]
        );
    }
}
