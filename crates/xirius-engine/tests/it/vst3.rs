//! Plugins inside the mixer in process, offline (ARC-011): the test VST3 effect
//! (`xirius-test-vst3`, its "Gain") as an insert, and the test CLAP plugin (`xirius-test-clap`,
//! which plays a held key) as an instrument.

use std::path::PathBuf;
use std::sync::Arc;

use xirius_engine::builtin::Sine;
use xirius_engine::clap_plugin::ClapProcessor;
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::plugin::Vst3Processor;
use xirius_engine::{
    Command, MixSpec, NoteKind, Plan, ProcessSetup, RenderMode, TrackSlot, TrackSpec,
};
use xirius_vst3::{Module, Plugin};

const SETUP: ProcessSetup = ProcessSetup {
    sample_rate: 48_000.0,
    max_block: 256,
    mode: RenderMode::Offline,
};

fn deps() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn load() -> (Plugin, Vst3Processor) {
    xirius_test_vst3::fixture_marker();
    let module = Arc::new(Module::load(&xirius_test_vst3::fixture_path(&deps())).expect("load"));
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .expect("an audio class");
    let mut plugin = Plugin::new(module, &class).expect("create");
    let processor = Vst3Processor::new(plugin.audio().expect("audio part"));
    (plugin, processor)
}

fn plan(track: TrackSpec) -> Plan {
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(track)],
        ..MixSpec::default()
    };
    Plan::build(spec, SETUP, None).unwrap()
}

/// RMS of the left channel of `frames` frames, rendered in device buffers of 480 frames (not a
/// multiple of the max block, so the engine chops them).
fn render_rms(audio: &mut engine::AudioEngine, frames: usize) -> f64 {
    let mut out = vec![0.0f32; 480 * 2];
    let (mut sum, mut n) = (0.0f64, 0usize);
    for _ in 0..frames / 480 {
        audio.render(&mut out, 2);
        for s in out.iter().step_by(2) {
            assert!(s.is_finite());
            sum += (*s as f64) * (*s as f64);
            n += 1;
        }
    }
    (sum / n as f64).sqrt()
}

#[test]
fn an_effect_plugin_runs_as_an_insert() {
    let (plugin, gain) = load();
    let mut t = TrackSpec::new("sine", Box::new(Sine::new(440.0, 0.5)));
    t.inserts.push(Box::new(gain));
    let (mut audio, _control) = engine::create(plan(t), EngineConfig::default());
    // A centred track: pan law unity, so the plugin at its default (0 dB) passes the sine.
    let rms = render_rms(&mut audio, 48_000);
    assert!((rms - 0.5 / 2f64.sqrt()).abs() < 1e-3, "RMS {rms}");

    let id = plugin
        .params()
        .into_iter()
        .find(|p| p.title == "Gain")
        .expect("a Gain parameter")
        .id;
    assert!(plugin.set_param(id, 0.0));
    let quiet = render_rms(&mut audio, 48_000);
    eprintln!("test gain insert: RMS {rms:.6}, at 0 RMS {quiet:.6}");
    assert!(quiet < rms / 10.0, "RMS {quiet}");
}

#[test]
fn an_instrument_plugin_plays_engine_notes() {
    xirius_test_clap::fixture_marker();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vst3");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("instrument-{}.clap", std::process::id()));
    std::fs::copy(
        deps().join(format!(
            "{}xirius_test_clap{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        )),
        &file,
    )
    .unwrap();
    let bundle = xirius_clap::Bundle::load(&file).unwrap();
    let synth = ClapProcessor::new(
        xirius_clap::Plugin::new(&bundle, xirius_test_clap::ID)
            .unwrap()
            .audio()
            .unwrap(),
    );
    let (mut audio, mut control) = engine::create(
        plan(TrackSpec::new("synth", Box::new(synth))),
        EngineConfig::default(),
    );
    assert_eq!(render_rms(&mut audio, 4800), 0.0);
    control
        .send(Command::Note {
            track: 0,
            kind: NoteKind::On {
                pitch: 60,
                velocity: 0.8,
            },
        })
        .ok()
        .expect("queue a note");
    let playing = render_rms(&mut audio, 9600);
    control
        .send(Command::Note {
            track: 0,
            kind: NoteKind::Off { pitch: 60 },
        })
        .ok()
        .expect("queue a note");
    render_rms(&mut audio, 4 * 48_000);
    let released = render_rms(&mut audio, 9600);
    eprintln!("instrument track: note RMS {playing:.6}, after release {released:.2e}");
    assert!(playing > 1e-3, "RMS {playing}");
    assert!(released < playing / 100.0, "RMS {released}");
}
