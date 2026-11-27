//! A parameter's change reaches the processing and comes back from the saved state, on the test
//! VST3 (`xirius-test-vst3`), whose "Gain" is the output times its value. Installed plugins
//! get the same calls in `real_plugins.rs`.

use std::sync::Arc;

use xirius_vst3::{AudioPart, Module, Plugin};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 512;

fn load() -> Plugin {
    xirius_test_vst3::fixture_marker();
    let path = xirius_test_vst3::fixture_path(std::env::current_exe().unwrap().parent().unwrap());
    let module = Arc::new(Module::load(&path).expect("load"));
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .expect("an audio class");
    Plugin::new(module, &class).expect("create")
}

/// Runs `blocks` blocks of a 440 Hz sine at amplitude 0.5 and returns the RMS of the last
/// `measure` blocks, left channel.
fn run(audio: &mut AudioPart, blocks: usize, measure: usize) -> f64 {
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let (mut sum, mut n) = (0.0f64, 0usize);
    let mut phase = 0.0f64;
    for b in 0..blocks {
        for i in 0..BLOCK {
            let v = (0.5 * (phase * std::f64::consts::TAU).sin()) as f32;
            phase = (phase + 440.0 / RATE).fract();
            l[i] = v;
            r[i] = v;
        }
        audio.process(&mut l, &mut r, &[]);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        if b >= blocks - measure {
            sum += l.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>();
            n += BLOCK;
        }
    }
    (sum / n as f64).sqrt()
}

#[test]
fn the_test_gain_follows_its_parameter_and_restores_state() {
    let mut plugin = load();
    let gain = plugin
        .params()
        .into_iter()
        .find(|p| p.title == "Gain")
        .expect("a Gain parameter");
    let mut audio = plugin.audio().unwrap();
    audio.prepare(RATE, BLOCK, true).unwrap();

    let unity = run(&mut audio, 20, 10);
    // A 0.5 sine has an RMS of 0.5/sqrt(2).
    assert!(
        (unity - 0.5 / 2f64.sqrt()).abs() < 1e-3,
        "unity RMS {unity}"
    );

    let target = 0.3;
    assert!(plugin.set_param(gain.id, target));
    let quieter = run(&mut audio, 40, 10);
    let expected = unity * target;
    assert!(
        (quieter / expected - 1.0).abs() < 0.01,
        "RMS {quieter}, expected {expected}"
    );
    assert_eq!(audio.counts(), (0, 1));

    let state = plugin.get_state().unwrap();
    assert!(!state.component.is_empty());
    drop(audio);
    drop(plugin);

    let restored = load();
    restored.set_state(&state).unwrap();
    let v = restored.param(gain.id).unwrap();
    assert!((v - target).abs() < 1e-6, "restored {v}");
}
