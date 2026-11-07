//! Explicit audio for tests of mixing, effects and exports. Empty tracks stay silent.
use serde_json::json;
use xirius_core::{
    Session,
    id::{PluginId, TrackId},
    session::RunOptions,
};

pub fn tone(s: &mut Session, track: TrackId) {
    let opts = RunOptions::human();
    s.run_value("plugin.replace", json!({"track":track, "slot":"instrument", "name":"Sampler", "format":"builtin", "uid":"Sampler"}), &opts).unwrap();
    let id = s.project().tracks[&track].inserts[0].id;
    sample(s, id);
}

pub fn sample(s: &mut Session, id: PluginId) {
    thread_local! {
        static FILES: std::cell::RefCell<Vec<tempfile::TempDir>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let opts = RunOptions::human();
    let touched = s.state.touched.clone();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tone_A3.wav");
    let data: Vec<f32> = (0..48000)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 0.5)
        .collect();
    xirius_session::export::write_wav(&file, &data, &data, 48000, 32).unwrap();
    s.run_value("plugin.samples.add", json!({"id":id, "file":file}), &opts)
        .unwrap();
    s.run_value(
        "plugin.param.set",
        json!({"id":id,"param":"file1.loop","value":1.0}),
        &opts,
    )
    .unwrap();
    s.state.touched = touched;
    FILES.with(|f| f.borrow_mut().push(dir));
}
