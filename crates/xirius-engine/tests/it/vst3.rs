//! Installed VST3 plugins inside the mixer, offline (ARC-011). Each test skips when its plugin
//! is not installed.

use std::sync::Arc;

use xirius_engine::builtin::Sine;
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::plugin::Vst3Processor;
use xirius_engine::{
    Command, MixSpec, NoteKind, Plan, ProcessSetup, RenderMode, TrackSlot, TrackSpec,
};
use xirius_vst3::{Module, Plugin, default_vst3_dir};

const SETUP: ProcessSetup = ProcessSetup {
    sample_rate: 48_000.0,
    max_block: 256,
    mode: RenderMode::Offline,
};

fn load(relative: &str) -> Option<(Plugin, Vst3Processor)> {
    let path = default_vst3_dir().join(relative);
    if !path.exists() {
        eprintln!("skipped: {} is not installed", path.display());
        return None;
    }
    let module = Arc::new(Module::load(&path).expect("load"));
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .expect("an audio class");
    let mut plugin = Plugin::new(module, &class).expect("create");
    let processor = Vst3Processor::new(plugin.audio().expect("audio part"));
    Some((plugin, processor))
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
    let Some((plugin, gain)) = load("Kilohearts/kHs Gain.vst3") else {
        return;
    };
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
    eprintln!(
        "kHs Gain insert: RMS {rms:.6}, at {} RMS {quiet:.6}",
        plugin.param_text(id, 0.0).unwrap_or_default()
    );
    assert!(quiet < rms / 10.0, "RMS {quiet}");
}

#[test]
fn an_instrument_plugin_plays_engine_notes() {
    let Some((_plugin, vital)) = load("Vital.vst3") else {
        return;
    };
    let (mut audio, mut control) = engine::create(
        plan(TrackSpec::new("vital", Box::new(vital))),
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
    eprintln!("Vital track: note RMS {playing:.6}, after release {released:.2e}");
    assert!(playing > 1e-3, "RMS {playing}");
    assert!(released < playing / 100.0, "RMS {released}");
}
