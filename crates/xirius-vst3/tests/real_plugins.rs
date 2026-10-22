//! Hosts plugins installed on this machine, offline. Each test skips when its plugin is not
//! installed, so the suite also passes on machines without them.

use std::path::PathBuf;
use std::sync::Arc;

use xirius_vst3::{AudioPart, CTRL_PITCH_BEND, Control, Module, Note, Plugin, default_vst3_dir};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 512;

fn load(relative: &str) -> Option<Plugin> {
    let path: PathBuf = default_vst3_dir().join(relative);
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
    Some(Plugin::new(module, &class).expect("create"))
}

/// Runs `blocks` blocks of a 440 Hz sine at amplitude 0.5 (silence for instruments) and
/// returns the RMS of the last `measure` blocks, left channel.
fn run(audio: &mut AudioPart, blocks: usize, measure: usize, notes: &[Note], sine: bool) -> f64 {
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let (mut sum, mut n) = (0.0f64, 0usize);
    let mut phase = 0.0f64;
    for b in 0..blocks {
        for i in 0..BLOCK {
            let v = if sine {
                (0.5 * (phase * std::f64::consts::TAU).sin()) as f32
            } else {
                0.0
            };
            phase = (phase + 440.0 / RATE).fract();
            l[i] = v;
            r[i] = v;
        }
        let block_notes: &[Note] = if b == 0 { notes } else { &[] };
        audio.process(&mut l, &mut r, block_notes);
        assert!(
            l.iter().chain(&r).all(|x| x.is_finite()),
            "non-finite output"
        );
        if b >= blocks - measure {
            sum += l.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>();
            n += BLOCK;
        }
    }
    (sum / n as f64).sqrt()
}

/// The leading number of a plugin's display text, e.g. "-6.02 dB" -> -6.02.
fn leading_number(text: &str) -> f64 {
    let t: String = text
        .trim()
        .chars()
        .map(|c| if c == '\u{2212}' { '-' } else { c })
        .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'))
        .collect();
    t.parse()
        .unwrap_or_else(|_| panic!("no number in {text:?}"))
}

#[test]
fn khs_gain_follows_its_parameter_and_restores_state() {
    let Some(mut plugin) = load("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let gain = plugin
        .params()
        .into_iter()
        .find(|p| p.title == "Gain")
        .expect("a Gain parameter");
    let mut audio = plugin.audio().unwrap();
    audio.prepare(RATE, BLOCK, true).unwrap();

    let unity = run(&mut audio, 20, 10, &[], true);
    // A 0.5 sine has an RMS of 0.5/sqrt(2).
    assert!(
        (unity - 0.5 / 2f64.sqrt()).abs() < 1e-3,
        "unity RMS {unity}"
    );

    let target = 0.3;
    assert!(plugin.set_param(gain.id, target));
    let db = leading_number(&plugin.param_text(gain.id, target).unwrap());
    let quieter = run(&mut audio, 40, 10, &[], true);
    let expected = unity * 10f64.powf(db / 20.0);
    assert!(
        (quieter / expected - 1.0).abs() < 0.01,
        "{db} dB: RMS {quieter}, expected {expected}"
    );
    eprintln!(
        "kHs Gain: unity RMS {unity:.6}, {db} dB -> RMS {quieter:.6} (expected {expected:.6})"
    );
    assert_eq!(audio.counts(), (0, 1));

    let state = plugin.get_state().unwrap();
    assert!(!state.component.is_empty());
    drop(audio);
    drop(plugin);

    let restored = load("Kilohearts/kHs Gain.vst3").unwrap();
    restored.set_state(&state).unwrap();
    let v = restored.param(gain.id).unwrap();
    assert!((v - target).abs() < 1e-6, "restored {v}");
}

#[test]
fn supermassive_processes_a_sine() {
    let Some(mut plugin) = load("ValhallaSupermassive.vst3") else {
        return;
    };
    let mut audio = plugin.audio().unwrap();
    audio.prepare(RATE, BLOCK, true).unwrap();
    let rms = run(&mut audio, 100, 50, &[], true);
    eprintln!("Supermassive: RMS {rms:.6}");
    assert!(rms > 0.01, "RMS {rms}");
    assert_eq!(audio.counts().0, 0);
}

#[test]
fn vital_plays_a_note() {
    let Some(mut plugin) = load("Vital.vst3") else {
        return;
    };
    let mut audio = plugin.audio().unwrap();
    audio.prepare(RATE, BLOCK, true).unwrap();
    assert!(audio.has_event_input());
    assert!(audio.input_channels().is_empty());
    let silent = run(&mut audio, 10, 10, &[], false);
    assert_eq!(silent, 0.0);
    let on = Note {
        offset: 0,
        pitch: 60,
        velocity: 0.8,
        on: true,
    };
    let playing = run(&mut audio, 40, 20, &[on], false);
    assert!(playing > 1e-3, "RMS {playing}");
    let off = Note { on: false, ..on };
    // Long enough for the default patch's release to finish.
    let released = run(&mut audio, 400, 20, &[off], false);
    eprintln!("Vital: note RMS {playing:.6}, after release {released:.2e}");
    assert!(released < playing / 100.0, "RMS after release {released}");
    assert_eq!(audio.dropped_events(), 0);
}

#[test]
fn vital_receives_controller_changes_as_parameters_or_events() {
    let Some(mut plugin) = load("Vital.vst3") else {
        return;
    };
    let mut audio = plugin.audio().unwrap();
    audio.prepare(RATE, BLOCK, true).unwrap();
    let on = Note {
        offset: 0,
        pitch: 60,
        velocity: 0.8,
        on: true,
    };
    let controls = [
        Control {
            offset: 10,
            number: 64,
            value: 127,
        },
        Control {
            offset: 20,
            number: CTRL_PITCH_BEND,
            value: 4000,
        },
        Control {
            offset: 30,
            number: 1,
            value: 64,
        },
    ];
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    audio.process_with(&mut l, &mut r, &[on], &controls);
    assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    let c = audio.control_counts();
    eprintln!(
        "Vital: {c:?}, maps CC1 {}, bend {}",
        audio.maps_control(1),
        audio.maps_control(CTRL_PITCH_BEND)
    );
    // Every change went one way or the other; none was lost.
    assert_eq!(c.as_params + c.as_events + c.dropped, 3);
    assert_eq!(c.dropped, 0);
}
