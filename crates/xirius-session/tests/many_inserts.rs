use serde_json::{Value, json};
use xirius_core::{Session, model::Project, session::RunOptions};
use xirius_session::{
    commands::{self, shared_registry},
    ext::ExtConfig,
    output::OutputMode,
};
fn run(s: &mut Session, n: &str, a: Value) -> Value {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"))
        .data
}
fn session() -> Session {
    let mut s = Session::with_registry(Project::new("preset"), shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            ..ExtConfig::default()
        },
    );
    s
}

#[test]
fn sixty_four_effects_render_the_note_offline() {
    let mut s = session();
    let t = run(&mut s, "track.add.instrument", json!({}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    run(
        &mut s,
        "plugin.add",
        json!({"track":t,"name":"Sampler","kind":"instrument"}),
    );
    let c = run(&mut s, "clip.add.midi", json!({"track":t,"at":0,"bars":1}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    run(
        &mut s,
        "note.add",
        json!({"clip":c,"at":0,"length":960,"pitch":60,"velocity":100}),
    );
    fn render(s: &Session) -> Vec<f32> {
        let mut b = xirius_session::build::Builder::new(48000, None);
        let built = b.build(s.project(), xirius_session::graph::Select::All);
        assert!(built.warnings.is_empty(), "{:?}", built.warnings);
        let expected = s
            .project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .filter(|p| p.kind == xirius_core::model::PluginKind::Effect)
            .count();
        let xirius_engine::TrackSlot::New(track) = &built.spec.tracks[0] else {
            panic!("a fresh track")
        };
        assert_eq!(track.inserts.len(), expected);
        drop(built);
        let audio = xirius_session::export::render(
            &mut b,
            s.project(),
            xirius_session::graph::Select::All,
            0,
            24000,
            0,
        )
        .unwrap();
        assert!(audio.left.iter().all(|x| x.is_finite()));
        audio.left
    }
    let dry = render(&s);
    for _ in 0..64 {
        run(&mut s, "plugin.add", json!({"track":t,"name":"EQ"}));
    }
    let wet = render(&s);
    assert!(wet.iter().any(|x| x.abs() > 0.01));
    let error = dry
        .iter()
        .zip(&wet)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(error < 0.001, "flat EQ error {error}");
}
