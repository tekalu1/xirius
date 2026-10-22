//! A CLAP plugin in the host process (PLG-028), through the same server and messages as a
//! VST3 one: it plays offline with its parameters, an edit in it comes back with its state,
//! and after the host is killed a restart brings that state back. The plugin is
//! `xirius-test-clap`, copied as a `.clap` file. Nothing goes to a device.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use xirius_engine::builtin::Sine;
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::server::{
    EditPhase, PluginLink, PluginServer, PluginStatus, RemotePlugin, RemoteSpec,
};
use xirius_engine::{AudioEngine, MixSpec, Plan, ProcessSetup, RenderMode, TrackSlot, TrackSpec};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 256;
const SINE_RMS: f64 = 0.353_553;
/// Only a guard against waiting forever.
const GUARD: Duration = Duration::from_secs(60);

/// The test plugin as a `.clap` file of its own (a host that is still going down may hold
/// another test's copy).
fn bundle(name: &str) -> PathBuf {
    xirius_test_clap::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sandbox_clap");
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join(format!("{name}-{}.clap", std::process::id()));
    std::fs::copy(deps.join("xirius_test_clap.dll"), &to).unwrap();
    to
}

fn remote(
    server: &PluginServer,
    bundle: PathBuf,
    params: &[(&str, f64)],
) -> (RemotePlugin, std::sync::Arc<PluginLink>) {
    let link = PluginLink::new();
    let plugin = RemotePlugin::new(
        server.clone(),
        link.clone(),
        RemoteSpec {
            name: "clap".into(),
            bundle,
            class: Some(xirius_clap::class_key(xirius_test_clap::ID)),
            params: params.iter().map(|&(k, v)| (k.to_string(), v)).collect(),
            latency: BLOCK as u32,
            ..RemoteSpec::default()
        },
    );
    (plugin, link)
}

fn engine_for(plugin: RemotePlugin) -> (AudioEngine, u32) {
    let mut t = TrackSpec::new("sine", Box::new(Sine::new(440.0, 0.5)));
    t.inserts.push(Box::new(plugin));
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    };
    let setup = ProcessSetup {
        sample_rate: RATE,
        max_block: BLOCK,
        mode: RenderMode::Offline,
    };
    let plan = Plan::build(spec, setup, None).unwrap();
    let latency = plan.latency();
    let (audio, control) = engine::create(plan, EngineConfig::default());
    std::mem::forget(control);
    (audio, latency)
}

fn render(audio: &mut AudioEngine, left: &mut Vec<f32>, frames: usize) {
    let mut out = vec![0.0f32; 480 * 2];
    let end = left.len() + frames;
    while left.len() < end {
        audio.render(&mut out, 2);
        left.extend(out.iter().step_by(2));
    }
    left.truncate(end);
    assert!(left.iter().all(|x| x.is_finite()));
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

/// The level of the last 9600 frames once two stretches in a row went through the plugin on
/// every block. Offline, a block the host does not answer in time plays dry by design.
fn wet_level(
    audio: &mut AudioEngine,
    out: &mut Vec<f32>,
    server: &PluginServer,
    link: &PluginLink,
) -> f64 {
    let mut before = false;
    let end = Instant::now() + GUARD;
    while Instant::now() < end {
        let a = server.status(link);
        render(audio, out, 9600);
        let b = server.status(link);
        let clean = b.dry_blocks == a.dry_blocks && b.wet_blocks > a.wet_blocks;
        if clean && before {
            return rms(&out[out.len() - 9600..]);
        }
        before = clean;
    }
    panic!(
        "the plugin never played through its host: {:?}",
        server.status(link)
    );
}

fn level(rms: f64) -> f64 {
    rms / SINE_RMS
}

#[test]
fn a_clap_plugin_plays_in_the_host_with_its_parameters() {
    let server = PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()));
    let (plugin, link) = remote(&server, bundle("params"), &[("Gain", 0.5)]);
    let (mut audio, latency) = engine_for(plugin);
    assert_eq!(latency as usize, BLOCK);
    let mut out = Vec::new();
    let wet = wet_level(&mut audio, &mut out, &server, &link);
    let st: PluginStatus = server.status(&link);
    assert_eq!(st.error, None);
    assert!((level(wet) - 0.5).abs() < 1e-3, "{}", level(wet));
    server.kill();
    server.wait_down(GUARD);
}

#[test]
fn an_edit_in_a_clap_plugin_comes_back_with_its_state_after_a_restart() {
    let server = PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()));
    let (plugin, link) = remote(&server, bundle("state"), &[]);
    let (mut audio, _) = engine_for(plugin);
    let mut out = Vec::new();
    let before = wet_level(&mut audio, &mut out, &server, &link);
    assert!((level(before) - 1.0).abs() < 1e-3, "{}", level(before));

    // The user turns the gain down in the plugin; the host sends the state after it.
    server.simulate_edit(&link, xirius_test_clap::GAIN, &[0.5, 0.25]);
    let end = Instant::now() + GUARD;
    while !link.has_pending_state() {
        assert!(Instant::now() < end, "the state after the edit never came");
        render(&mut audio, &mut out, 4800);
    }
    let edits = link.take_edits();
    let state = edits.state.expect("the state after the edit");
    assert_eq!(&state.component[..4], b"XTCL");
    assert!(state.controller.is_empty());
    let phases: Vec<EditPhase> = edits.params.iter().map(|e| e.phase).collect();
    assert_eq!(
        phases,
        [
            EditPhase::Begin,
            EditPhase::Change,
            EditPhase::Change,
            EditPhase::End
        ]
    );
    assert!(edits.params.iter().all(|e| e.id == xirius_test_clap::GAIN));
    assert_eq!(edits.params[0].title, "Gain");
    assert_eq!(edits.params[2].value, Some(0.25));
    let edited = wet_level(&mut audio, &mut out, &server, &link);
    assert!((level(edited) - 0.25).abs() < 1e-3, "{}", level(edited));

    // The host dies; a restart opens the plugin again with the state it had.
    server.kill();
    assert!(server.wait_down(GUARD), "the host never went down");
    assert!(server.status(&link).crashed);
    server.restart().unwrap();
    let back = wet_level(&mut audio, &mut out, &server, &link);
    let st = server.status(&link);
    assert_eq!(st.error, None);
    assert!(!st.crashed);
    assert!((level(back) - 0.25).abs() < 1e-3, "{}", level(back));
    server.kill();
    server.wait_down(GUARD);
}
