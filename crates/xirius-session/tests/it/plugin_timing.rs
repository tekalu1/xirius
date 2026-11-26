//! Each plugin's processing time is measured while the song renders (ENG-008), and a plugin
//! switched off does not run.
use serde_json::{Value, json};
use xirius_core::{Session, model::Project, session::RunOptions};
use xirius_session::{
    build::Builder,
    commands::{self, shared_registry},
    ext::ExtConfig,
    graph::Select,
    output::OutputMode,
    perf::{self, Run},
};

fn run(s: &mut Session, n: &str, a: Value) -> Value {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"))
        .data
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_owned()
}

fn song() -> (Session, String, String, String) {
    let mut s = Session::with_registry(Project::new("timing"), shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            ..ExtConfig::default()
        },
    );
    let t = id(&run(&mut s, "track.add.instrument", json!({})));
    run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Sampler", "kind": "instrument"}),
    );
    let a = id(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Reverb", "kind": "effect", "format": "builtin", "uid": "Reverb"}),
    ));
    let b = id(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Delay", "kind": "effect", "format": "builtin", "uid": "Delay"}),
    ));
    let c = id(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "bars": 1}),
    ));
    run(
        &mut s,
        "note.add",
        json!({"clip": c, "at": 0, "length": 960, "pitch": 60, "velocity": 100}),
    );
    (s, t, a, b)
}

fn render(s: &Session) -> Builder {
    let mut b = Builder::new(48_000, None);
    xirius_session::export::render(&mut b, s.project(), Select::All, 0, 48_000, 0).expect("render");
    b
}

#[test]
fn every_running_plugin_has_a_reading_after_a_render() {
    let (s, t, _a, _b) = song();
    let b = render(&s);
    let rows = perf::loads(s.project(), &b.timings(), None);
    assert_eq!(rows.len(), 3);
    for r in &rows {
        assert_eq!(r.run, Run::Running);
        assert!(r.reading.blocks > 0, "{} was not timed", r.name);
        assert!(r.reading.mean_us > 0.0, "{}", r.name);
        assert!(r.reading.peak_us >= r.reading.max_us);
    }
    // Heaviest first.
    assert!(
        rows.windows(2)
            .all(|w| w[0].reading.load >= w[1].reading.load)
    );
    // Only the track asked for.
    let t: xirius_core::id::TrackId = t.parse().unwrap();
    assert_eq!(perf::loads(s.project(), &b.timings(), Some(t)).len(), 3);
}

#[test]
fn a_deactivated_or_bypassed_plugin_is_not_run_and_has_no_reading() {
    let (mut s, _t, a, bp) = song();
    run(&mut s, "plugin.deactivate", json!({"ids": [a]}));
    run(&mut s, "plugin.bypass", json!({"ids": [bp]}));
    let b = render(&s);
    let rows = perf::loads(s.project(), &b.timings(), None);
    let by = |n: &str| rows.iter().find(|r| r.name == n).unwrap().clone();
    assert_eq!(by("Reverb").run, Run::Deactivated);
    assert_eq!(by("Reverb").reading.blocks, 0);
    assert_eq!(by("Delay").run, Run::Bypassed);
    assert_eq!(by("Delay").reading.blocks, 0);
    assert!(by("Sampler").reading.blocks > 0);
    // The listed ones sort after the running one.
    assert_eq!(rows[0].name, "Sampler");
}

#[test]
fn plugin_stats_lists_the_plugins_even_when_nothing_plays() {
    let (mut s, t, _a, _b) = song();
    let d = run(&mut s, "plugin.stats", json!({}));
    assert_eq!(d["playing"], false);
    assert_eq!(d["plugins"].as_array().unwrap().len(), 3);
    assert_eq!(d["plugins"][0]["state"], "running");
    let d = run(&mut s, "plugin.stats", json!({"track": t}));
    assert_eq!(d["plugins"].as_array().unwrap().len(), 3);
    let e = run(&mut s, "engine.stats", json!({}));
    assert_eq!(e["playing"], false);
    assert_eq!(e["xruns"], 0);
    assert!(
        s.run_value(
            "plugin.stats",
            json!({"track": "trk-ffffffff"}),
            &RunOptions::human()
        )
        .is_err()
    );
}
