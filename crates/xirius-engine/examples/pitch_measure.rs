//! Run with an output directory outside the repository to measure PLG-076 and
//! write short synthesized before/after WAVs for listening.

use std::f64::consts::TAU;
use std::fs::{File, create_dir_all};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use xirius_engine::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, RenderMode};
use xirius_engine::stock;

const RATE: f64 = 48_000.0;

fn voice(seconds: f64, kind: usize) -> Vec<f32> {
    let n = (seconds * RATE) as usize;
    let mut phase = 0.0;
    (0..n)
        .map(|i| {
            let t = i as f64 / RATE;
            let hz = match kind {
                1 => 245.0 + 65.0 * t / seconds,
                2 => 270.0 + 5.0 * (TAU * 5.0 * t).sin(),
                _ => 270.0,
            };
            phase += TAU * hz / RATE;
            let vowel = (1..=12)
                .map(|h| {
                    let f = hz * h as f64;
                    let shape = (-((f - 800.0) / 300.0).powi(2)).exp()
                        + 0.8 * (-((f - 1600.0) / 400.0).powi(2)).exp();
                    shape * (phase * h as f64).sin()
                })
                .sum::<f64>();
            let envelope = (t * 12.0).min(1.0) * ((seconds - t) * 12.0).min(1.0);
            (vowel * envelope * 0.12) as f32
        })
        .collect()
}

fn render(input: &[f32], range: f64, amount: f64) -> (Vec<f32>, usize, (f32, f32, i32, f32)) {
    render_with(input, &[("range", range), ("amount", amount)], &[])
}

fn render_with(
    input: &[f32],
    settings: &[(&str, f64)],
    notes: &[NoteEvent],
) -> (Vec<f32>, usize, (f32, f32, i32, f32)) {
    let (mut p, h) = stock::create("pitch-correction").unwrap();
    for &(id, value) in settings {
        let spec = h.def().param(id).unwrap().1;
        assert!(h.set(id, spec.to_norm(value)));
    }
    let probe = h.probe();
    probe.watch(true);
    p.prepare(&ProcessSetup {
        sample_rate: RATE,
        max_block: 512,
        mode: RenderMode::Offline,
    });
    let latency = p.latency() as usize;
    let mut left = input.to_vec();
    let mut right = left.clone();
    for at in (0..left.len()).step_by(512) {
        let end = (at + 512).min(left.len());
        p.process(
            &mut AudioIo::new(&mut left[at..end], &mut right[at..end]),
            if at == 0 { notes } else { &[] },
        );
    }
    (left, latency, probe.pitch_view())
}

fn wav(path: &Path, samples: &[f32]) -> io::Result<()> {
    let mut f = BufWriter::new(File::create(path)?);
    let bytes = (samples.len() * 4) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + bytes).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&(RATE as u32).to_le_bytes())?;
    f.write_all(&((RATE as u32) * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&bytes.to_le_bytes())?;
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        f.write_all(&value.to_le_bytes())?;
        f.write_all(&value.to_le_bytes())?;
    }
    f.flush()?;
    Ok(())
}

fn cpu(range: f64) -> f64 {
    let (mut p, h) = stock::create("pitch-correction").unwrap();
    let spec = h.def().param("range").unwrap().1;
    assert!(h.set("range", spec.to_norm(range)));
    p.prepare(&ProcessSetup {
        sample_rate: RATE,
        max_block: 512,
        mode: RenderMode::Realtime,
    });
    let source = voice(2.0, 0);
    let mut left = [0.0f32; 512];
    let mut right = [0.0f32; 512];
    let blocks = 180;
    let start = Instant::now();
    for block in 0..blocks {
        for i in 0..512 {
            let value = source[(block * 512 + i) % source.len()];
            left[i] = value;
            right[i] = value;
        }
        p.process(&mut AudioIo::new(&mut left, &mut right), &[]);
    }
    let real_time = blocks as f64 * 512.0 / RATE;
    100.0 * start.elapsed().as_secs_f64() / real_time
}

fn main() -> io::Result<()> {
    let directory = std::env::args().nth(1).expect("output directory argument");
    let directory = Path::new(&directory);
    create_dir_all(directory)?;
    for (name, kind, seconds) in [("vowel", 0, 3.0), ("glide", 1, 2.5), ("vibrato", 2, 2.5)] {
        let source = voice(seconds, kind);
        let (after, delay, view) = render(&source, 1.0, 100.0);
        let mut before = vec![0.0; source.len()];
        before[delay..].copy_from_slice(&source[..source.len() - delay]);
        wav(&directory.join(format!("{name}-before.wav")), &before)?;
        wav(&directory.join(format!("{name}-after.wav")), &after)?;
        println!(
            "{name}: detector {:.3} Hz, target {}, output {:.3} Hz",
            view.0, view.2, view.1
        );
    }
    for range in 0..3 {
        let source = voice(0.3, 0);
        let (_, delay, _) = render(&source, range as f64, 0.0);
        println!(
            "range {range}: {delay} frames, {:.3} ms, release CPU {:.3}%",
            delay as f64 / RATE * 1000.0,
            cpu(range as f64)
        );
    }
    let mut consonants = vec![0.0f32; 96_000];
    let mut state = 0x1234_5678u32;
    for sample in &mut consonants[9_600..19_200] {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *sample = (state as f64 / u32::MAX as f64 * 0.2 - 0.1) as f32;
    }
    for (i, sample) in consonants.iter_mut().enumerate().take(76_800).skip(19_200) {
        *sample = (0.4 * (TAU * 270.0 * i as f64 / RATE).sin()) as f32;
    }
    for x in &mut consonants[76_800..86_400] {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *x = (state as f64 / u32::MAX as f64 * 0.2 - 0.1) as f32;
    }
    let (after, delay, _) = render(&consonants, 1.0, 100.0);
    let mut before = vec![0.0; consonants.len()];
    before[delay..].copy_from_slice(&consonants[..consonants.len() - delay]);
    wav(&directory.join("consonants-before.wav"), &before)?;
    wav(&directory.join("consonants-after.wav"), &after)?;

    let f0 = 110.0;
    let formant_voice: Vec<f32> = (0..96_000)
        .map(|i| {
            let phase = TAU * f0 * i as f64 / RATE;
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
    let note = [NoteEvent {
        offset: 0,
        kind: NoteKind::On {
            pitch: 48,
            velocity: 1.0,
        },
    }];
    wav(
        &directory.join("three-semitones-before.wav"),
        &formant_voice,
    )?;
    for (label, formant) in [("on", 1.0), ("off", 0.0)] {
        let (audio, _, _) = render_with(
            &formant_voice,
            &[("speed", 1.0), ("formant", formant)],
            &note,
        );
        wav(
            &directory.join(format!("three-semitones-formant-{label}.wav")),
            &audio,
        )?;
    }
    let mut phase = 0.0;
    let vibrato_voice: Vec<f32> = (0..96_000)
        .map(|i| {
            let cents = 30.0 + 40.0 * (TAU * 5.5 * i as f64 / RATE).sin();
            phase += TAU * 261.625_565 * 2f64.powf(cents / 1200.0) / RATE;
            (0.4 * phase.sin()) as f32
        })
        .collect();
    wav(&directory.join("speed-vibrato-before.wav"), &vibrato_voice)?;
    for speed in [1.0, 20.0, 200.0] {
        let (audio, _, _) = render_with(&vibrato_voice, &[("speed", speed), ("scale", 1.0)], &[]);
        wav(
            &directory.join(format!("speed-vibrato-{speed:.0}ms-after.wav")),
            &audio,
        )?;
    }
    Ok(())
}
