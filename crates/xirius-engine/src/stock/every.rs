//! What every built-in in [`xirius_params::BUILTINS`] must do, checked for each (ARC-054):
//! silence in is silence out, at its defaults and with every value at its lowest and its
//! highest, with and without a file; notes without its files make no sound and nothing
//! panics; once its sound stops it lets go, with no voice left sounding (PLG-066) and its
//! tail under -120 dB within eight seconds, whether its file plays out or a note is let go
//! before the file ends. A built-in added to the list is checked here as it is; one
//! without a processor fails.

use std::panic::{AssertUnwindSafe, catch_unwind};

use xirius_params::{BUILTINS, BuiltinDef, pad_key};

use super::testing::*;
use super::{StockHandle, create};
use crate::processor::Processor;

/// The values a built-in is checked at: its defaults, then every value at its lowest, then at
/// its highest (the loudest gains, the longest feedback, the sharpest resonance).
const VALUES: [(&str, Option<f64>); 3] = [
    ("at its defaults", None),
    ("every value at 0", Some(0.0)),
    ("every value at 1", Some(1.0)),
];

/// A prepared built-in, or why there is none.
fn device(uid: &str) -> Result<(Box<dyn Processor>, StockHandle), String> {
    let (mut p, h) = create(uid).ok_or("no processor")?;
    p.prepare(&setup());
    Ok((p, h))
}

fn set_every(h: &StockHandle, norm: f64) -> Result<(), String> {
    for p in h.def().params {
        if !h.set(p.id, norm) {
            return Err(format!("{} was not set", p.id));
        }
    }
    Ok(())
}

/// The first frame that is not zero (a NaN is not zero).
fn silent(l: &[f32], r: &[f32]) -> Result<(), String> {
    for (side, x) in [("left", l), ("right", r)] {
        if let Some(i) = x.iter().position(|v| *v != 0.0) {
            return Err(format!("{side} frame {i} is {}", x[i]));
        }
    }
    Ok(())
}

/// The rows a check has run and the ones that failed or panicked, each named.
#[derive(Default)]
struct Rows {
    run: usize,
    failed: Vec<String>,
}

impl Rows {
    fn check(&mut self, row: String, f: impl FnOnce() -> Result<(), String>) {
        self.run += 1;
        let failure = match catch_unwind(AssertUnwindSafe(f)) {
            Ok(Ok(())) => return,
            Ok(Err(e)) => e,
            Err(panic) => {
                let text = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("?");
                format!("panicked: {text}")
            }
        };
        self.failed.push(format!("{row}: {failure}"));
    }

    fn assert(self, what: &str) {
        assert!(
            self.failed.is_empty(),
            "{} of {} rows {what}:\n{}",
            self.failed.len(),
            self.run,
            self.failed.join("\n")
        );
    }
}

fn file(seconds: f64) -> super::files::FileSlot {
    sine_file("tone_C3.wav", 261.6, 0.4, (RATE * seconds) as usize)
}

/// The values that make a note sound for as long as it is held: a pad's trigger at Normal
/// where the device has one (the Drum Kit's pads play One Shot by default).
fn held(def: &BuiltinDef) -> Vec<(String, f64)> {
    def.per_pad
        .iter()
        .filter(|p| p.id == "trigger")
        .filter_map(|p| {
            let normal = p.choices().iter().position(|c| *c == "Normal")?;
            Some((pad_key(0, p.id), p.to_norm(normal as f64)))
        })
        .collect()
}

#[test]
fn every_builtin_gives_silence_for_silence() {
    let mut rows = Rows::default();
    let zero = vec![0.0; RATE as usize];
    for def in BUILTINS {
        let files = create(def.uid).is_some_and(|(_, h)| h.takes_files());
        for with_file in [false, true] {
            if with_file && !files {
                continue;
            }
            for (values, norm) in VALUES {
                let file_label = if with_file { "a file" } else { "no file" };
                rows.check(format!("{}, {values}, {file_label}", def.uid), || {
                    let (mut p, h) = device(def.uid)?;
                    if let Some(norm) = norm {
                        set_every(&h, norm)?;
                    }
                    if with_file {
                        give(&h, vec![file(1.0)]);
                    }
                    let (l, r) = render(p.as_mut(), &zero, &zero, &[]);
                    silent(&l, &r)
                });
            }
        }
    }
    rows.assert("did not give silence for silence");
}

#[test]
fn every_builtin_makes_no_sound_without_its_files() {
    let mut rows = Rows::default();
    let zero = vec![0.0; RATE as usize / 2];
    let notes = [
        (0, on(36, 1.0)),
        (100, on(60, 0.5)),
        (9_600, off(36)),
        (12_000, off(60)),
    ];
    for def in BUILTINS {
        let files = create(def.uid).is_some_and(|(_, h)| h.takes_files());
        for empty_slot in [false, true] {
            if empty_slot && !files {
                continue;
            }
            let how = if empty_slot {
                "a file that was not found"
            } else {
                "no files"
            };
            rows.check(format!("{}, notes with {how}", def.uid), || {
                let (mut p, h) = device(def.uid)?;
                if empty_slot && !h.set_files(vec![None]) {
                    return Err("the empty slot was not taken".into());
                }
                let (l, r) = render(p.as_mut(), &zero, &zero, &notes);
                silent(&l, &r)
            });
        }
    }
    rows.assert("made a sound without files");
}

/// A tenth of a second of a tone in and a note held (on a device with files, of a file
/// `seconds` long, set as `values`), then eight seconds after the note is let go: no voice may
/// still sound and the last second must be under -120 dB.
fn lets_go(uid: &str, seconds: f64, values: &[(String, f64)]) -> Result<(), String> {
    let (mut p, h) = device(uid)?;
    if h.takes_files() {
        let values = values.iter().map(|(k, v)| (k, v));
        let (sent, unknown) = h.set_files_with(vec![Some(file(seconds))], values);
        if !sent || !unknown.is_empty() {
            return Err(format!("the file was not taken ({unknown:?})"));
        }
    }
    let x = sine(440.0, 0.5, 4_800);
    let (l, r) = render(p.as_mut(), &x, &x, &[(0, on(36, 0.8))]);
    if silent(&l, &r).is_ok() {
        return Err("it made no sound to let go of".into());
    }
    let second = vec![0.0; RATE as usize];
    let mut last = 0.0;
    for k in 0..8 {
        let notes = if k == 0 { vec![(0, off(36))] } else { vec![] };
        let (l, r) = render(p.as_mut(), &second, &second, &notes);
        if l.iter().chain(&r).any(|v| v.is_nan()) {
            return Err(format!("second {} has a NaN", k + 1));
        }
        last = peak(&l).max(peak(&r));
    }
    let voices = h.probe().sounding();
    if voices > 0 {
        return Err(format!("{voices} voices still sound after 8 s"));
    }
    if db(last) > -120.0 {
        return Err(format!("its eighth second peaks at {:.1} dB", db(last)));
    }
    Ok(())
}

#[test]
fn every_builtin_lets_its_sound_go() {
    let mut rows = Rows::default();
    for def in BUILTINS {
        rows.check(format!("{}, a sound that plays out", def.uid), || {
            lets_go(def.uid, 0.2, &[])
        });
        if create(def.uid).is_some_and(|(_, h)| h.takes_files()) {
            // Longer than the eight seconds, so only letting go of the note stops it.
            rows.check(
                format!("{}, a note let go before its file ends", def.uid),
                || lets_go(def.uid, 10.0, &held(def)),
            );
        }
    }
    rows.assert("did not let their sound go");
}
