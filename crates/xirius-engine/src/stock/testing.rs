//! What the built-ins' tests share: rendering offline in uneven blocks and measuring.

use std::f64::consts::TAU;

use super::{StockHandle, create};
use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor, RenderMode};

pub const RATE: f64 = 48_000.0;

/// Blocks of these sizes in turn, so what happens once every few frames meets every edge.
const BLOCKS: [usize; 4] = [512, 37, 128, 301];

pub fn setup() -> ProcessSetup {
    ProcessSetup {
        sample_rate: RATE,
        max_block: 512,
        mode: RenderMode::Offline,
    }
}

/// A prepared built-in and its handle.
pub fn make(key: &str) -> (Box<dyn Processor>, StockHandle) {
    let (mut p, h) = create(key).unwrap_or_else(|| panic!("no built-in {key}"));
    p.prepare(&setup());
    (p, h)
}

/// Sets a parameter in its unit.
pub fn set_plain(h: &StockHandle, id: &str, plain: f64) {
    let (_, spec) = h.def().param(id).unwrap_or_else(|| panic!("no {id}"));
    assert!(h.set(id, spec.to_norm(plain)));
}

/// Runs the input through the processor; `notes` are (frame, event) in frame order.
pub fn render(
    p: &mut dyn Processor,
    left: &[f32],
    right: &[f32],
    notes: &[(usize, NoteKind)],
) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (left.to_vec(), right.to_vec());
    let mut at = 0;
    let mut k = 0;
    let mut events = Vec::new();
    while at < l.len() {
        let n = BLOCKS[k % BLOCKS.len()].min(l.len() - at);
        k += 1;
        events.clear();
        events.extend(
            notes
                .iter()
                .filter(|(f, _)| (at..at + n).contains(f))
                .map(|(f, kind)| NoteEvent {
                    offset: (f - at) as u32,
                    kind: *kind,
                }),
        );
        let mut io = AudioIo {
            left: &mut l[at..at + n],
            right: &mut r[at..at + n],
        };
        p.process(&mut io, &events);
        at += n;
    }
    (l, r)
}

pub fn sine(freq: f64, amp: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (amp * (TAU * freq * i as f64 / RATE).sin()) as f32)
        .collect()
}

pub fn rms(x: &[f32]) -> f64 {
    let sum: f64 = x.iter().map(|v| f64::from(*v).powi(2)).sum();
    (sum / x.len().max(1) as f64).sqrt()
}

pub fn peak(x: &[f32]) -> f64 {
    x.iter().fold(0.0, |m, v| m.max(f64::from(v.abs())))
}

pub fn db(ratio: f64) -> f64 {
    20.0 * ratio.max(1e-30).log10()
}

/// Two seconds of silence come out as silence.
pub fn assert_silent(p: &mut dyn Processor) {
    let zero = vec![0.0; RATE as usize * 2];
    let (l, r) = render(p, &zero, &zero, &[]);
    assert_eq!(peak(&l), 0.0, "{}: left", p.name());
    assert_eq!(peak(&r), 0.0, "{}: right", p.name());
}

/// A unit impulse on both channels, then silence.
pub fn impulse(frames: usize) -> Vec<f32> {
    let mut x = vec![0.0; frames];
    x[0] = 1.0;
    x
}

pub fn on(pitch: u8, velocity: f32) -> NoteKind {
    NoteKind::On { pitch, velocity }
}

pub fn off(pitch: u8) -> NoteKind {
    NoteKind::Off { pitch }
}
