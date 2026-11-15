//! Run with an output directory outside the repository to measure PLG-076 and
//! write short synthesized before/after WAVs for listening.

use std::f64::consts::TAU;
use std::fs::{File, create_dir_all};
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use xirius_engine::processor::{AudioIo, ProcessSetup, RenderMode};
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
    let (mut p, h) = stock::create("pitch-correction").unwrap();
    for (id, value) in [("range", range), ("amount", amount)] {
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
            &[],
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

fn band_peak(signal: &[f32], low: usize, high: usize) -> (usize, f64) {
    let mut best = (0, 0.0);
    for hz in (low..=high).step_by(5) {
        let mut re = 0.0;
        let mut im = 0.0;
        for (i, sample) in signal.iter().enumerate() {
            let window = 0.5 - 0.5 * (TAU * i as f64 / signal.len() as f64).cos();
            let angle = TAU * hz as f64 * i as f64 / RATE;
            re += f64::from(*sample) * window * angle.cos();
            im += f64::from(*sample) * window * angle.sin();
        }
        let power = re * re + im * im;
        if power > best.1 {
            best = (hz, power);
        }
    }
    best
}

fn cpu() -> f64 {
    let (mut p, _) = stock::create("pitch-correction").unwrap();
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
        if kind == 0 {
            let start = delay + 24_000;
            for (label, low, high) in [("F1", 550, 1100), ("F2", 1250, 1900)] {
                let input_peak = band_peak(&before[start..start + 12_000], low, high);
                let output_peak = band_peak(&after[start..start + 12_000], low, high);
                println!(
                    "{label}: before {} Hz, after {} Hz",
                    input_peak.0, output_peak.0
                );
            }
        }
    }
    println!(
        "48 kHz, 512 frames, one instance, release CPU: {:.2}% of real time",
        cpu()
    );
    for range in 0..3 {
        let source = voice(0.3, 0);
        let (_, delay, _) = render(&source, range as f64, 0.0);
        println!(
            "range {range}: {delay} frames, {:.3} ms",
            delay as f64 / RATE * 1000.0
        );
    }
    Ok(())
}
