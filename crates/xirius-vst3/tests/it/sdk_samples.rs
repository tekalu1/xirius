//! Hosts the VST3 SDK's own sample plugins (PLG-042): `host-checker`, which scores the host on
//! the calls it sees, plus `again`, `adelay` and `note-expression-synth`, whose behaviour is
//! known from their sources. Build them from the pinned SDK (docs/plugin-hosting.md) and point
//! `XIRIUS_VST3_SDK_SAMPLES` at the folder holding the `.vst3` bundles; without it the tests skip.
//! Everything runs offline: no audio device, no window.

use std::path::PathBuf;
use std::sync::Arc;

use xirius_vst3::{Module, Note, Plugin};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 512;

fn load(bundle: &str) -> Option<Plugin> {
    let Some(dir) = std::env::var_os("XIRIUS_VST3_SDK_SAMPLES") else {
        eprintln!("skipped: XIRIUS_VST3_SDK_SAMPLES is not set");
        return None;
    };
    let path = PathBuf::from(dir).join(bundle);
    assert!(path.exists(), "{} is not built", path.display());
    let module = Arc::new(Module::load(&path).expect("load"));
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .expect("an audio class");
    Some(Plugin::new(module, &class).expect("create"))
}

fn param(plugin: &Plugin, title: &str) -> u32 {
    plugin
        .params()
        .into_iter()
        .find(|p| p.title == title)
        .unwrap_or_else(|| panic!("no {title} parameter"))
        .id
}

fn note(pitch: u8, velocity: f32, on: bool) -> Note {
    Note {
        offset: 0,
        pitch,
        velocity,
        on,
    }
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

fn sine(phase: &mut f64, out: &mut [f32]) {
    for v in out {
        *v = (0.5 * (*phase * std::f64::consts::TAU).sin()) as f32;
        *phase = (*phase + 440.0 / RATE).fract();
    }
}

#[test]
fn host_checker_scores_the_host_and_takes_its_state_back() {
    let Some(mut plugin) = load("host-checker.vst3") else {
        return;
    };
    let score = param(&plugin, "Scoring");
    let before = plugin.param(score).unwrap();
    let mut audio = plugin.audio().expect("audio");
    audio.prepare(RATE, BLOCK, false).expect("prepare");
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let mut phase = 0.0;
    for b in 0..64 {
        sine(&mut phase, &mut l);
        r.copy_from_slice(&l);
        let notes = if b == 1 {
            vec![note(60, 0.8, true)]
        } else if b == 9 {
            vec![note(60, 0.0, false)]
        } else {
            Vec::new()
        };
        audio.process(&mut l, &mut r, &notes);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    }
    // An offline pass too: host-checker records the process mode it is given.
    audio.prepare(RATE, BLOCK, true).expect("prepare offline");
    for _ in 0..8 {
        audio.process(&mut l, &mut r, &[]);
    }
    drop(audio);
    let after = plugin.param(score).unwrap();
    eprintln!(
        "host-checker: Scoring {:.0} -> {:.0} ({})",
        before * 100.0,
        after * 100.0,
        plugin.param_text(score, after).unwrap_or_default()
    );
    assert!(after > 0.0, "host-checker saw none of the host's features");

    let state = plugin.get_state().expect("get state");
    assert!(!state.component.is_empty());
    plugin.set_state(&state).expect("set state");
}

#[test]
fn again_scales_by_its_gain() {
    let Some(mut plugin) = load("again.vst3") else {
        return;
    };
    let gain = param(&plugin, "Gain");
    assert!(plugin.set_param(gain, 0.25));
    let mut audio = plugin.audio().expect("audio");
    audio.prepare(RATE, BLOCK, true).expect("prepare");
    let mut phase = 0.0;
    let (mut ins, mut outs) = (0.0, 0.0);
    for b in 0..16 {
        let mut l = vec![0.0f32; BLOCK];
        sine(&mut phase, &mut l);
        let mut r = l.clone();
        let input = rms(&l);
        audio.process(&mut l, &mut r, &[]);
        if b >= 4 {
            ins += input;
            outs += rms(&l);
        }
    }
    assert!((outs / ins - 0.25).abs() < 0.01, "ratio {}", outs / ins);
    drop(audio);
    let state = plugin.get_state().expect("state");
    plugin.set_param(gain, 1.0);
    plugin.set_state(&state).expect("restore");
    assert!((plugin.param(gain).unwrap() - 0.25).abs() < 1e-6);
}

#[test]
fn adelay_delays_an_impulse_by_its_time() {
    let Some(mut plugin) = load("adelay.vst3") else {
        return;
    };
    let delay = param(&plugin, "Delay");
    // 0.01 s at 48 kHz.
    assert!(plugin.set_param(delay, 0.01));
    let mut audio = plugin.audio().expect("audio");
    audio.prepare(RATE, BLOCK, true).expect("prepare");
    // One silent block lets the change land before the impulse.
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    audio.process(&mut l, &mut r, &[]);
    let mut out = Vec::new();
    for b in 0..4 {
        let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        if b == 0 {
            l[0] = 1.0;
            r[0] = 1.0;
        }
        audio.process(&mut l, &mut r, &[]);
        out.extend_from_slice(&l);
    }
    let at = out
        .iter()
        .position(|&x| x.abs() > 0.5)
        .expect("the impulse comes out");
    assert_eq!(at, (0.01 * RATE) as usize);
}

#[test]
fn note_expression_synth_sounds_on_a_note() {
    let Some(mut plugin) = load("note-expression-synth.vst3") else {
        return;
    };
    let mut audio = plugin.audio().expect("audio");
    audio.prepare(RATE, BLOCK, true).expect("prepare");
    assert!(audio.has_event_input());
    let mut level = 0.0;
    for b in 0..16 {
        let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        let notes = if b == 0 {
            vec![note(69, 0.9, true)]
        } else {
            Vec::new()
        };
        audio.process(&mut l, &mut r, &notes);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        level = rms(&l).max(level);
    }
    assert!(level > 0.01, "silent: {level}");
}
