//! Hosts installed plugins, offline: the VST3 effect `XIRIUS_REAL_VST3_EFFECT` names and the
//! VST3 instrument `XIRIUS_REAL_VST3_INSTRUMENT` names (each a bundle's path). They are
//! `#[ignore]`d and run with `python scripts/ci_tests.py --plugins`; each fails when its
//! variable is not set. The same calls on the test VST3 run everywhere (`gain.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use xirius_vst3::{AudioPart, CTRL_PITCH_BEND, Control, Module, Note, Plugin};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 512;

/// `ParameterInfo::kIsReadOnly`, `kIsProgramChange` and `kIsBypass`.
const NOT_A_SETTING: i32 = (1 << 1) | (1 << 15) | (1 << 16);

fn load(variable: &str) -> Plugin {
    let path = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{variable} names an installed VST3 bundle"));
    let module = Arc::new(Module::load(&path).expect("load"));
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .expect("an audio class");
    Plugin::new(module, &class).expect("create")
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

#[test]
#[ignore = "needs XIRIUS_REAL_VST3_EFFECT (python scripts/ci_tests.py --plugins)"]
fn an_installed_effect_processes_a_sine_and_restores_its_state() {
    let mut plugin = load("XIRIUS_REAL_VST3_EFFECT");
    let mut audio = plugin.audio().unwrap();
    audio.prepare(RATE, BLOCK, true).unwrap();
    let rms = run(&mut audio, 100, 50, &[], true);
    eprintln!("installed effect: RMS {rms:.6}");
    assert!(rms > 0.01, "RMS {rms}");
    assert_eq!(audio.counts().0, 0);

    // One of its settings moved away from its default comes back in a new instance.
    let p = plugin
        .params()
        .into_iter()
        .find(|p| p.flags & NOT_A_SETTING == 0)
        .expect("a parameter to set");
    let target = if p.default < 0.5 { 0.7 } else { 0.2 };
    assert!(plugin.set_param(p.id, target));
    let set = plugin.param(p.id).unwrap();
    let state = plugin.get_state().unwrap();
    assert!(!state.component.is_empty());
    drop(audio);
    drop(plugin);

    let restored = load("XIRIUS_REAL_VST3_EFFECT");
    restored.set_state(&state).unwrap();
    let v = restored.param(p.id).unwrap();
    assert!(
        (v - set).abs() < 1e-6,
        "{}: restored {v}, set {set}",
        p.title
    );
}

#[test]
#[ignore = "needs XIRIUS_REAL_VST3_INSTRUMENT (python scripts/ci_tests.py --plugins)"]
fn an_installed_instrument_plays_a_note() {
    let mut plugin = load("XIRIUS_REAL_VST3_INSTRUMENT");
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
    eprintln!("installed instrument: note RMS {playing:.6}, after release {released:.2e}");
    assert!(released < playing / 100.0, "RMS after release {released}");
    assert_eq!(audio.dropped_events(), 0);
}

#[test]
#[ignore = "needs XIRIUS_REAL_VST3_INSTRUMENT (python scripts/ci_tests.py --plugins)"]
fn an_installed_instrument_receives_controller_changes_as_parameters_or_events() {
    let mut plugin = load("XIRIUS_REAL_VST3_INSTRUMENT");
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
        "installed instrument: {c:?}, maps CC1 {}, bend {}",
        audio.maps_control(1),
        audio.maps_control(CTRL_PITCH_BEND)
    );
    // Every change went one way or the other; none was lost.
    assert_eq!(c.as_params + c.as_events + c.dropped, 3);
    assert_eq!(c.dropped, 0);
}
