use std::f64::consts::TAU;

use xirius_engine::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, RenderMode};
use xirius_engine::stock;

#[global_allocator]
static ALLOCATOR: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

fn tone(hz: f64, rate: f64, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| (0.4 * (TAU * hz * i as f64 / rate).sin()) as f32)
        .collect()
}

fn run(
    rate: f64,
    block: usize,
    input: &[f32],
    amount: f64,
    notes: &[(usize, NoteKind)],
) -> (Vec<f32>, u32) {
    let (mut device, handle) = stock::create("pitch-correction").expect("Pitch Correction exists");
    let probe = handle.probe();
    probe.watch(true);
    let spec = handle.def().param("amount").expect("Amount parameter").1;
    assert!(handle.set("amount", spec.to_norm(amount)));
    device.prepare(&ProcessSetup {
        sample_rate: rate,
        max_block: block,
        mode: RenderMode::Offline,
    });
    let latency = device.latency();
    let mut left = input.to_vec();
    let mut right = input.to_vec();
    let mut at = 0;
    while at < input.len() {
        let end = (at + block).min(input.len());
        let events: Vec<_> = notes
            .iter()
            .filter(|(i, _)| (at..end).contains(i))
            .map(|(i, kind)| NoteEvent {
                offset: (i - at) as u32,
                kind: *kind,
            })
            .collect();
        device.process(
            &mut AudioIo::new(&mut left[at..end], &mut right[at..end]),
            &events,
        );
        at = end;
    }
    assert_eq!(left, right);
    let _view = probe.pitch_view();
    (left, latency)
}

fn configured(
    rate: f64,
    block: usize,
    input: &[f32],
    settings: &[(&str, f64)],
    notes: &[(usize, NoteKind)],
) -> (Vec<f32>, u32, (f32, f32, i32, f32)) {
    let (mut device, handle) = stock::create("pitch-correction").unwrap();
    for (id, plain) in settings {
        let spec = handle.def().param(id).unwrap().1;
        assert!(handle.set(id, spec.to_norm(*plain)));
    }
    let probe = handle.probe();
    probe.watch(true);
    device.prepare(&ProcessSetup {
        sample_rate: rate,
        max_block: block,
        mode: RenderMode::Offline,
    });
    let latency = device.latency();
    let mut left = input.to_vec();
    let mut right = input.to_vec();
    let mut at = 0;
    let mut ordinal = 0;
    while at < input.len() {
        let width = if block == 513 {
            [1, 64, 512, 37][ordinal % 4]
        } else {
            block
        };
        let end = (at + width).min(input.len());
        let events: Vec<_> = notes
            .iter()
            .filter(|(i, _)| (at..end).contains(i))
            .map(|(i, kind)| NoteEvent {
                offset: (i - at) as u32,
                kind: *kind,
            })
            .collect();
        device.process(
            &mut AudioIo::new(&mut left[at..end], &mut right[at..end]),
            &events,
        );
        at = end;
        ordinal += 1;
    }
    assert_eq!(left, right);
    (left, latency, probe.pitch_view())
}

#[test]
fn catalog_has_the_seven_automation_destinations_and_relative_keys() {
    let def = xirius_params::find("pitch-correction").expect("catalog entry");
    for (id, label) in [
        ("key", "キー"),
        ("scale", "スケール"),
        ("speed", "スピード"),
        ("transition", "ノートトランジション"),
        ("amount", "補正の量"),
        ("formant", "フォルマント"),
        ("range", "音域"),
    ] {
        assert_eq!(def.param(id).unwrap().1.label, label);
    }
    assert_eq!(def.params.len(), 7);
    let custom = xirius_params::pitch_correction::toggle_relative(1.0 / 3.0, 4);
    let mask = xirius_params::pitch_correction::scale_mask(custom);
    assert_eq!(mask & (1 << 4), 0);
}

#[test]
fn zero_amount_is_an_exact_latency_only_pass() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut input = tone(223.0, rate, rate as usize / 5);
        input[0] = 1.0;
        let (out, latency) = run(rate, 64, &input, 0.0, &[]);
        let delay = latency as usize;
        assert!(delay > 0);
        assert!(out[..delay].iter().all(|x| *x == 0.0));
        assert_eq!(&out[delay..], &input[..input.len() - delay]);
    }
}

#[test]
fn empty_custom_keyboard_has_no_target_and_passes_the_voice_unchanged() {
    let rate = 48_000.0;
    let input = tone(270.0, rate, 14_000);
    let (mut device, handle) = stock::create("pitch-correction").unwrap();
    handle.probe().watch(true);
    assert!(handle.set("scale", xirius_params::pitch_correction::custom_norm(0)));
    device.prepare(&ProcessSetup {
        sample_rate: rate,
        max_block: 512,
        mode: RenderMode::Offline,
    });
    let delay = device.latency() as usize;
    let mut left = input.clone();
    let mut right = input.clone();
    for at in (0..input.len()).step_by(512) {
        let end = (at + 512).min(input.len());
        device.process(
            &mut AudioIo::new(&mut left[at..end], &mut right[at..end]),
            &[],
        );
    }
    assert_eq!(handle.probe().pitch_view().2, -1);
    assert_eq!(&left[delay..], &input[..input.len() - delay]);
}

fn estimate_hz(signal: &[f32], rate: f64, low: f64, high: f64) -> f64 {
    let mut best = (0.0, f64::NEG_INFINITY);
    let mut hz = low;
    while hz <= high {
        let mut re = 0.0;
        let mut im = 0.0;
        for (i, sample) in signal.iter().enumerate() {
            let window = 0.5 - 0.5 * (TAU * i as f64 / signal.len() as f64).cos();
            let phase = TAU * hz * i as f64 / rate;
            re += f64::from(*sample) * window * phase.cos();
            im += f64::from(*sample) * window * phase.sin();
        }
        let power = re * re + im * im;
        if power > best.1 {
            best = (hz, power);
        }
        hz += 0.5;
    }
    best.0
}

#[test]
fn a_detuned_sine_moves_toward_the_nearest_chromatic_note() {
    let rate = 48_000.0;
    let input_hz = 270.0;
    let input = tone(input_hz, rate, rate as usize);
    let (full, delay) = run(rate, 512, &input, 100.0, &[]);
    let (half, _) = run(rate, 512, &input, 50.0, &[]);
    let start = delay as usize + 20_000;
    let a = estimate_hz(&full[start..start + 12_000], rate, 240.0, 280.0);
    let b = estimate_hz(&half[start..start + 12_000], rate, 240.0, 280.0);
    let target = 440.0 * 2f64.powf((61.0 - 69.0) / 12.0);
    assert!((1200.0 * (a / target).log2()).abs() < 25.0);
    let halfway = (input_hz * target).sqrt();
    assert!((1200.0 * (b / halfway).log2()).abs() < 30.0);
}

#[test]
fn speed_and_note_transition_set_distinct_response_times() {
    let rate = 48_000.0;
    let (mut device, handle) = stock::create("pitch-correction").unwrap();
    handle.probe().watch(true);
    device.prepare(&ProcessSetup {
        sample_rate: rate,
        max_block: 64,
        mode: RenderMode::Offline,
    });
    let mut first_voiced = None;
    let mut initial = 0.0;
    let mut at_speed = 0.0;
    let mut at_transition_early = 0.0;
    let mut at_transition_late = 0.0;
    for at in (0..36_000).step_by(64) {
        let mut left: Vec<f32> = (at..at + 64)
            .map(|i| (0.4 * (TAU * 270.0 * i as f64 / rate).sin()) as f32)
            .collect();
        let mut right = left.clone();
        let events = if at == 19_200 {
            vec![NoteEvent {
                offset: 0,
                kind: NoteKind::On {
                    pitch: 60,
                    velocity: 1.0,
                },
            }]
        } else {
            vec![]
        };
        device.process(&mut AudioIo::new(&mut left, &mut right), &events);
        let output_hz = f64::from(handle.probe().pitch_view().1);
        if output_hz > 0.0 && first_voiced.is_none() {
            first_voiced = Some(at);
            initial = output_hz;
        }
        if first_voiced.is_some_and(|first| at == first + 960) {
            at_speed = output_hz;
        }
        if at == 19_200 + 448 {
            at_transition_early = output_hz;
        }
        if at == 19_200 + 4_736 {
            at_transition_late = output_hz;
        }
    }
    let c_sharp = 440.0 * 2f64.powf((61.0 - 69.0) / 12.0);
    let c = 440.0 * 2f64.powf((60.0 - 69.0) / 12.0);
    let progress = (at_speed - initial) / (c_sharp - initial);
    assert!((0.45..0.8).contains(&progress), "20 ms progress {progress}");
    assert!(at_transition_early > c + 10.0);
    assert!((at_transition_late - c).abs() < (at_transition_early - c).abs() * 0.55);
}

#[test]
fn correction_converges_for_small_and_large_detuning_on_both_sides() {
    let rate = 48_000.0;
    for cents in [-80.0_f64, -40.0, -10.0, 10.0, 40.0, 80.0] {
        let hz = 220.0 * 2f64.powf(cents / 1200.0);
        let target_midi = (69.0 + 12.0 * (hz / 440.0).log2()).round();
        let target_hz = 440.0 * 2f64.powf((target_midi - 69.0) / 12.0);
        let signal = tone(hz, rate, 36_000);
        let (output, latency, view) = configured(rate, 512, &signal, &[], &[]);
        assert_eq!(view.2 as f64, target_midi);
        let start = latency as usize + 16_000;
        let measured = estimate_hz(
            &output[start..start + 10_000],
            rate,
            target_hz - 18.0,
            target_hz + 18.0,
        );
        let error = (1200.0 * (measured / target_hz).log2()).abs();
        assert!(
            error < 30.0,
            "{cents} cents: {error} cents after correction"
        );
    }
}

#[test]
fn ranges_report_the_exact_delay_and_unvoiced_sound_passes_unchanged() {
    for (range, low) in [(0.0, 60.0), (1.0, 100.0), (2.0, 200.0)] {
        let rate = 48_000.0;
        let mut noise = vec![0.0f32; 7000];
        noise[3] = 1.0;
        let mut state = 12345u32;
        for x in &mut noise[200..] {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *x = (state as f64 / u32::MAX as f64 * 0.2 - 0.1) as f32;
        }
        let (out, latency, _) = configured(rate, 512, &noise, &[("range", range)], &[]);
        let period = (rate / low).ceil() as u32;
        assert_eq!(latency, (5 * period).div_ceil(2) + 1);
        let delay = latency as usize;
        assert_eq!(&out[..delay], &vec![0.0; delay]);
        assert_eq!(&out[delay..], &noise[..noise.len() - delay]);
    }
}

fn consonant_voice() -> (Vec<f32>, usize, usize, usize) {
    let rate = 48_000.0;
    let first_noise = 4_800;
    let voice_start = 9_600;
    let voice_end = 33_600;
    let second_noise_end = 38_400;
    let mut signal = vec![0.0; 48_000];
    let mut state = 0x1234_5678u32;
    for sample in &mut signal[first_noise..voice_start] {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *sample = (state as f64 / u32::MAX as f64 * 0.2 - 0.1) as f32;
    }
    for (n, x) in signal[voice_start..voice_end].iter_mut().enumerate() {
        let envelope = (n as f64 / 240.0)
            .min(1.0)
            .min((voice_end - voice_start - n) as f64 / 240.0);
        *x = (0.4 * envelope * (TAU * 270.0 * n as f64 / rate).sin()) as f32;
    }
    for x in &mut signal[voice_end..second_noise_end] {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *x = (state as f64 / u32::MAX as f64 * 0.2 - 0.1) as f32;
    }
    (signal, voice_start, voice_end, second_noise_end)
}

#[test]
fn consonants_are_exactly_delayed_and_the_voice_keeps_its_last_thirty_ms() {
    let (input, voice_start, voice_end, second_noise_end) = consonant_voice();
    let (output, latency, _) = configured(48_000.0, 512, &input, &[], &[]);
    let shift = latency as usize;
    let join = 240; // 5 ms on each side of the voice/noise junction.
    assert_eq!(
        &output[shift + 4_800 + join..shift + voice_start - join],
        &input[4_800 + join..voice_start - join]
    );
    assert_eq!(
        &output[shift + voice_end + join..shift + second_noise_end - join],
        &input[voice_end + join..second_noise_end - join]
    );
    let end = shift + voice_end - 300;
    let pitch = estimate_hz(&output[end - 1_440..end], 48_000.0, 255.0, 285.0);
    let target = 440.0 * 2f64.powf((61.0 - 69.0) / 12.0);
    assert!((1200.0 * (pitch / target).log2()).abs() < 30.0);
    let input_max = input
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    for edge in [voice_start, voice_end] {
        let at = edge + shift;
        let jump = output[at - 480..at + 480]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            jump < input_max * 1.5,
            "edge {edge}: jump {jump}, input {input_max}"
        );
    }
}

#[test]
fn a_step_in_pitch_reaches_the_output_at_the_same_source_time() {
    let rate = 48_000.0;
    let step = 19_200;
    let mut phase = 0.0;
    let input: Vec<f32> = (0..43_200)
        .map(|i| {
            phase += TAU * if i < step { 270.0 } else { 330.0 } / rate;
            (0.4 * phase.sin()) as f32
        })
        .collect();
    let (output, delay, _) = configured(rate, 512, &input, &[], &[]);
    let mut first_high = None;
    for center in (step - 2_400..step + 2_400).step_by(240) {
        let at = center + delay as usize;
        let hz = estimate_hz(&output[at - 480..at + 480], rate, 260.0, 345.0);
        if hz > 300.0 {
            first_high = Some(center);
            break;
        }
    }
    assert!(
        first_high.is_some_and(|at| at.abs_diff(step) <= 480),
        "step at {first_high:?}"
    );
}

#[test]
fn speed_preserves_fast_vibrato_but_fast_setting_flattens_it() {
    let rate = 48_000.0;
    let mut phase = 0.0;
    let input: Vec<f32> = (0..72_000)
        .map(|i| {
            let cents = 30.0 + 40.0 * (TAU * 5.5 * i as f64 / rate).sin();
            let hz = 261.625_565 * 2f64.powf(cents / 1200.0);
            phase += TAU * hz / rate;
            (0.4 * phase.sin()) as f32
        })
        .collect();
    let mut depths = Vec::new();
    for speed in [1.0, 20.0, 200.0] {
        let (mut device, handle) = stock::create("pitch-correction").unwrap();
        for (id, plain) in [("scale", 1.0), ("speed", speed)] {
            let spec = handle.def().param(id).unwrap().1;
            assert!(handle.set(id, spec.to_norm(plain)));
        }
        handle.probe().watch(true);
        device.prepare(&ProcessSetup {
            sample_rate: rate,
            max_block: 128,
            mode: RenderMode::Offline,
        });
        let mut cents = Vec::new();
        for at in (0..input.len()).step_by(128) {
            let end = (at + 128).min(input.len());
            let mut left = input[at..end].to_vec();
            let mut right = left.clone();
            device.process(&mut AudioIo::new(&mut left, &mut right), &[]);
            if at > 48_000 {
                let hz = f64::from(handle.probe().pitch_view().1);
                if hz > 0.0 {
                    cents.push(1200.0 * (hz / 261.625_565).log2());
                }
            }
        }
        let mean = cents.iter().sum::<f64>() / cents.len() as f64;
        let depth = (cents.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - cents.iter().copied().fold(f64::INFINITY, f64::min))
            / 2.0;
        eprintln!("speed {speed}: mean {mean:.3} cents, depth {depth:.3} cents");
        assert!(mean.abs() < 8.0, "speed {speed}: mean {mean}");
        depths.push(depth);
    }
    assert!(depths[2] >= 28.0, "slow depth {}", depths[2]);
    assert!(depths[0] <= 8.0, "fast depth {}", depths[0]);
    assert!(depths[0] < depths[1] && depths[1] < depths[2]);

    // Check the rendered sound as well as the processor's published pitch.
    let mut audio_depths = Vec::new();
    for speed in [1.0, 200.0] {
        let (audio, delay, _) =
            configured(rate, 512, &input, &[("scale", 1.0), ("speed", speed)], &[]);
        let cents: Vec<f64> = (48_000..68_000)
            .step_by(960)
            .map(|q| {
                let at = q + delay as usize;
                let hz = estimate_hz(&audio[at..at + 2_400], rate, 255.0, 275.0);
                1200.0 * (hz / 261.625_565).log2()
            })
            .collect();
        let depth = (cents.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - cents.iter().copied().fold(f64::INFINITY, f64::min))
            / 2.0;
        eprintln!("rendered speed {speed}: depth {depth:.3} cents");
        audio_depths.push(depth);
    }
    assert!(audio_depths[0] <= 10.0 && audio_depths[1] >= 28.0);
}

fn harmonic_envelope_peak(signal: &[f32], fundamental: f64, low: f64, high: f64) -> f64 {
    let mut points = Vec::new();
    for harmonic in 1..=(high / fundamental).ceil() as usize + 1 {
        let hz = harmonic as f64 * fundamental;
        if hz < low - fundamental || hz > high + fundamental {
            continue;
        }
        let mut re = 0.0;
        let mut im = 0.0;
        for (i, sample) in signal.iter().enumerate() {
            let window = 0.5 - 0.5 * (TAU * i as f64 / signal.len() as f64).cos();
            let angle = TAU * hz * i as f64 / 48_000.0;
            re += f64::from(*sample) * window * angle.cos();
            im += f64::from(*sample) * window * angle.sin();
        }
        points.push((hz, (re * re + im * im).max(1e-20).ln()));
    }
    let best = points
        .iter()
        .enumerate()
        .filter(|(_, (hz, _))| *hz >= low && *hz <= high)
        .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
        .unwrap()
        .0;
    if best == 0 || best + 1 == points.len() {
        return points[best].0;
    }
    let a = points[best - 1].1;
    let b = points[best].1;
    let c = points[best + 1].1;
    let denominator = a - 2.0 * b + c;
    let delta = if denominator.abs() > 1e-9 {
        0.5 * (a - c) / denominator
    } else {
        0.0
    };
    points[best].0 + delta.clamp(-0.5, 0.5) * fundamental
}

#[test]
fn three_semitone_shift_preserves_or_moves_the_formant_envelope() {
    let rate = 48_000.0;
    let f0 = 110.0;
    let target = f0 * 2f64.powf(3.0 / 12.0);
    let input: Vec<f32> = (0..96_000)
        .map(|i| {
            let phase = TAU * f0 * i as f64 / rate;
            (1..=30)
                .map(|h| {
                    let hz = f0 * h as f64;
                    let envelope = (-((hz - 800.0) / 220.0).powi(2)).exp()
                        + 0.9 * (-((hz - 1600.0) / 280.0).powi(2)).exp();
                    envelope * (phase * h as f64).sin()
                })
                .sum::<f64>() as f32
                * 0.1
        })
        .collect();
    let notes = [(
        0,
        NoteKind::On {
            pitch: 48,
            velocity: 1.0,
        },
    )];
    let (on, delay, _) = configured(rate, 512, &input, &[("speed", 1.0)], &notes);
    let (off, _, _) = configured(
        rate,
        512,
        &input,
        &[("speed", 1.0), ("formant", 0.0)],
        &notes,
    );
    let at = 48_000 + delay as usize;
    for (low, high) in [(650.0, 1000.0), (1400.0, 1850.0)] {
        let original = harmonic_envelope_peak(&input[48_000..72_000], f0, low, high);
        let kept = harmonic_envelope_peak(&on[at..at + 24_000], target, low, high);
        let moved = harmonic_envelope_peak(&off[at..at + 24_000], target, low * 1.15, high * 1.25);
        eprintln!("envelope {original:.1} {kept:.1} {moved:.1}");
        assert!((kept / original - 1.0).abs() < 0.05);
        assert!((moved / original - target / f0).abs() < 0.06);
    }
}

#[test]
fn key_scale_and_midi_notes_choose_the_nearest_allowed_note() {
    let rate = 48_000.0;
    let input = tone(270.0, rate, 24_000);
    let (_, _, chromatic) = configured(rate, 512, &input, &[], &[]);
    assert_eq!(chromatic.2, 61);
    // C major excludes C#, so the closest allowed note is C4.
    let (_, _, major) = configured(rate, 512, &input, &[("scale", 1.0)], &[]);
    assert_eq!(major.2, 60);
    // D major admits C#.
    let (_, _, d_major) = configured(rate, 512, &input, &[("scale", 1.0), ("key", 2.0)], &[]);
    assert_eq!(d_major.2, 61);
    // A low MIDI C folds by octaves to C4; a B chord tone is farther away.
    let notes = [
        (
            0,
            NoteKind::On {
                pitch: 36,
                velocity: 1.0,
            },
        ),
        (
            0,
            NoteKind::On {
                pitch: 35,
                velocity: 1.0,
            },
        ),
    ];
    let (_, _, midi) = configured(rate, 512, &input, &[], &notes);
    assert_eq!(midi.2, 60);
    let notes_off = [
        notes[0],
        (12_000, NoteKind::Off { pitch: 36 }),
        (12_000, NoteKind::Off { pitch: 35 }),
    ];
    let (_, _, after) = configured(rate, 512, &input, &[], &notes_off);
    assert_eq!(after.2, 61);
}

#[test]
fn block_edges_do_not_change_the_sound() {
    let rate = 48_000.0;
    let input = tone(270.0, rate, 14_000);
    let (reference, _, _) = configured(rate, 512, &input, &[], &[]);
    for block in [1, 64, 513] {
        let (other, _, _) = configured(rate, block, &input, &[], &[]);
        assert_eq!(reference, other, "block {block}");
    }
}

#[test]
fn audio_processing_allocates_nothing_after_prepare() {
    let (mut device, handle) = stock::create("pitch-correction").unwrap();
    handle.probe().watch(true);
    device.prepare(&ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 512,
        mode: RenderMode::Realtime,
    });
    let mut left = [0.0f32; 512];
    let mut right = [0.0f32; 512];
    for block in 0..80 {
        for i in 0..512 {
            let x = (TAU * 270.0 * (block * 512 + i) as f64 / 48_000.0).sin() as f32 * 0.4;
            left[i] = x;
            right[i] = x;
        }
        let events = [NoteEvent {
            offset: 7,
            kind: NoteKind::On {
                pitch: 60,
                velocity: 1.0,
            },
        }];
        assert_no_alloc::assert_no_alloc(|| {
            device.process(&mut AudioIo::new(&mut left, &mut right), &events)
        });
    }
}

#[test]
fn yin_reads_sines_glides_vibrato_and_a_harmonic_voice() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let len = (rate * 0.45) as usize;
        let cases: Vec<(Vec<f32>, f64, f64)> = vec![
            (tone(220.0, rate, len), 220.0, 3.0),
            (
                (0..len)
                    .scan(0.0, |phase, i| {
                        let hz = 200.0 + 40.0 * i as f64 / len as f64;
                        *phase += TAU * hz / rate;
                        Some((0.35 * phase.sin()) as f32)
                    })
                    .collect(),
                240.0,
                18.0,
            ),
            (
                (0..len)
                    .scan(0.0, |phase, i| {
                        let hz = 220.0 + 5.0 * (TAU * 5.0 * i as f64 / rate).sin();
                        *phase += TAU * hz / rate;
                        Some((0.35 * phase.sin()) as f32)
                    })
                    .collect(),
                225.0,
                35.0,
            ),
            (
                (0..len)
                    .map(|i| {
                        let phase = TAU * 220.0 * i as f64 / rate;
                        (1..=12)
                            .map(|harmonic| {
                                let f = 220.0 * harmonic as f64;
                                let envelope = (-((f - 880.0) / 320.0).powi(2)).exp()
                                    + 0.7 * (-((f - 1540.0) / 400.0).powi(2)).exp();
                                envelope * (phase * harmonic as f64).sin()
                            })
                            .sum::<f64>() as f32
                            * 0.1
                    })
                    .collect(),
                220.0,
                10.0,
            ),
        ];
        for (signal, expected, tolerance) in cases {
            let (_, _, view) = configured(rate, 512, &signal, &[], &[]);
            let error = (1200.0 * (f64::from(view.0) / expected).log2()).abs();
            assert!(
                error < tolerance,
                "rate {rate}, got {} Hz, expected {expected} Hz, {error} cents",
                view.0
            );
        }
    }
}
