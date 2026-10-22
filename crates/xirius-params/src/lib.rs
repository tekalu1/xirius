//! The parameter spec every built-in instrument and effect is described with (PLG-045), and
//! the catalogue of the built-ins (PLG-016).
//!
//! - A [`ParamSpec`] gives a parameter's id, its label, its unit, its range and default in the
//!   unit, and the [`Curve`] that maps the range onto 0..1.
//! - A song keeps a built-in's parameters as that 0..1 value, the same as a VST3 plugin's, so
//!   an automation lane (whose range is 0..1) moves them the same way. [`ParamSpec::to_plain`]
//!   and [`ParamSpec::to_norm`] go between the two, and [`ParamSpec::text`] shows a value.
//! - [`BUILTINS`] lists the built-ins: the Sampler and the Drum Kit, and EQ, Compressor,
//!   Reverb, Delay and Limiter. [`find`] looks one up by its uid or name.
//!
//! The crate has no dependencies, so the project model, the engine and the UI can all read it.

mod catalog;

pub use catalog::{
    BUILTINS, COMPRESSOR, DELAY, DIVISIONS, DRUM_KIT, EQ, FEELS, KITS, LIMITER, REVERB, SAMPLER,
    SAMPLER_SOUNDS, SLOPES,
};

/// What a parameter's plain value is measured in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    None,
    Db,
    Hz,
    Ms,
    Seconds,
    Percent,
    /// A compression ratio, shown as `4.0:1`.
    Ratio,
    Semitones,
    Cents,
}

impl Unit {
    /// The symbol a list shows next to the value (empty for none).
    pub fn symbol(self) -> &'static str {
        match self {
            Unit::None => "",
            Unit::Db => "dB",
            Unit::Hz => "Hz",
            Unit::Ms => "ms",
            Unit::Seconds => "s",
            Unit::Percent => "%",
            Unit::Ratio => ":1",
            Unit::Semitones => "st",
            Unit::Cents => "cent",
        }
    }
}

/// How the plain range is laid out over 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Linear,
    /// Equal ratios take equal steps (frequencies, times). `min` is above 0.
    Log,
    /// Whole numbers from `min` to `max`, evenly spaced.
    Int,
    /// One of the labels; the plain value is the index.
    Choice(&'static [&'static str]),
    /// Off (0) or on (1).
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamSpec {
    /// The key a song keeps the value under (`plugin.param.set --param`).
    pub id: &'static str,
    pub label: &'static str,
    pub unit: Unit,
    pub min: f64,
    pub max: f64,
    /// In the unit.
    pub default: f64,
    pub curve: Curve,
}

impl ParamSpec {
    /// The plain value of a 0..1 value. Values outside 0..1 are taken as the nearest end.
    pub fn to_plain(&self, norm: f64) -> f64 {
        let n = if norm.is_nan() {
            0.0
        } else {
            norm.clamp(0.0, 1.0)
        };
        match self.curve {
            Curve::Linear => self.min + n * (self.max - self.min),
            Curve::Log => self.min * (self.max / self.min).powf(n),
            Curve::Int => (self.min + n * (self.max - self.min)).round(),
            Curve::Choice(labels) => {
                let last = labels.len().saturating_sub(1) as f64;
                (n * last).round()
            }
            Curve::Toggle => {
                if n >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// The 0..1 value of a plain value. Values outside the range are taken as the nearest end.
    pub fn to_norm(&self, plain: f64) -> f64 {
        let (lo, hi) = self.range();
        let v = if plain.is_nan() {
            lo
        } else {
            plain.clamp(lo, hi)
        };
        if hi <= lo {
            return 0.0;
        }
        match self.curve {
            Curve::Linear => (v - lo) / (hi - lo),
            Curve::Log => (v / lo).ln() / (hi / lo).ln(),
            Curve::Int | Curve::Choice(_) | Curve::Toggle => (v.round() - lo) / (hi - lo),
        }
    }

    /// The plain range. A choice runs from 0 to the last index, a toggle from 0 to 1.
    pub fn range(&self) -> (f64, f64) {
        match self.curve {
            Curve::Choice(labels) => (0.0, labels.len().saturating_sub(1) as f64),
            Curve::Toggle => (0.0, 1.0),
            _ => (self.min, self.max),
        }
    }

    /// The default as a 0..1 value.
    pub fn default_norm(&self) -> f64 {
        self.to_norm(self.default)
    }

    /// How many values the parameter can take, when it has steps.
    pub fn steps(&self) -> Option<u32> {
        match self.curve {
            Curve::Linear | Curve::Log => None,
            Curve::Int => Some((self.max - self.min).round() as u32 + 1),
            Curve::Choice(labels) => Some(labels.len() as u32),
            Curve::Toggle => Some(2),
        }
    }

    /// The labels of a choice (empty otherwise).
    pub fn choices(&self) -> &'static [&'static str] {
        match self.curve {
            Curve::Choice(labels) => labels,
            _ => &[],
        }
    }

    /// A plain value as the UI and the CLI show it (`-12.0 dB`, `1.20 kHz`, `付点`).
    pub fn text(&self, plain: f64) -> String {
        let (lo, hi) = self.range();
        let v = plain.clamp(lo, hi);
        match self.curve {
            Curve::Choice(labels) => {
                return labels
                    .get(v.round() as usize)
                    .copied()
                    .unwrap_or("")
                    .to_string();
            }
            Curve::Toggle => return if v >= 0.5 { "オン" } else { "オフ" }.to_string(),
            _ => {}
        }
        match self.unit {
            Unit::None => format!("{v:.2}"),
            Unit::Db => format!("{v:.1} dB"),
            Unit::Hz if v >= 10_000.0 => format!("{:.1} kHz", v / 1000.0),
            Unit::Hz if v >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
            Unit::Hz => format!("{v:.0} Hz"),
            Unit::Ms if v >= 1000.0 => format!("{:.2} s", v / 1000.0),
            Unit::Ms if v >= 100.0 => format!("{v:.0} ms"),
            Unit::Ms if v >= 10.0 => format!("{v:.1} ms"),
            Unit::Ms => format!("{v:.2} ms"),
            Unit::Seconds => format!("{v:.2} s"),
            Unit::Percent => format!("{v:.0} %"),
            Unit::Ratio => format!("{v:.1}:1"),
            Unit::Semitones => format!("{v:+.0} st"),
            Unit::Cents => format!("{v:+.0} cent"),
        }
    }

    /// Reads a plain value written as a number, a choice label or on/off. `None` when the text
    /// is none of these or the number is outside the range.
    pub fn parse(&self, text: &str) -> Option<f64> {
        let t = text.trim();
        let (lo, hi) = self.range();
        if let Ok(v) = t.parse::<f64>() {
            return (v.is_finite() && v >= lo && v <= hi).then_some(v);
        }
        match self.curve {
            Curve::Choice(labels) => labels
                .iter()
                .position(|l| l.eq_ignore_ascii_case(t))
                .map(|i| i as f64),
            Curve::Toggle => match t.to_ascii_lowercase().as_str() {
                "on" | "true" | "オン" => Some(1.0),
                "off" | "false" | "オフ" => Some(0.0),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Whether a built-in makes sound from notes or changes the sound it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Instrument,
    Effect,
}

/// One built-in instrument or effect.
#[derive(Debug, PartialEq)]
pub struct BuiltinDef {
    /// What a song keeps in `uid` (`plugin.add --uid`).
    pub uid: &'static str,
    /// The name the browser and the insert row show.
    pub name: &'static str,
    pub kind: Kind,
    pub params: &'static [ParamSpec],
}

impl BuiltinDef {
    /// A parameter and its index, by id (letter case ignored).
    pub fn param(&self, id: &str) -> Option<(usize, &'static ParamSpec)> {
        self.params
            .iter()
            .enumerate()
            .find(|(_, p)| p.id.eq_ignore_ascii_case(id))
    }

    /// Every parameter's id and default as a 0..1 value, in order.
    pub fn defaults(&self) -> impl Iterator<Item = (&'static str, f64)> + '_ {
        self.params.iter().map(|p| (p.id, p.default_norm()))
    }
}

/// The built-in with this uid or name. Letter case, spaces, `-` and `_` are ignored, so
/// `Drum Kit`, `drum-kit` and `drumkit` are the same.
pub fn find(key: &str) -> Option<&'static BuiltinDef> {
    let key = squash(key);
    BUILTINS
        .iter()
        .copied()
        .find(|d| squash(d.uid) == key || squash(d.name) == key)
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The most parameters a built-in has, for fixed-size tables on the audio thread.
pub const MAX_PARAMS: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_the_two_instruments_and_five_effects() {
        let names: Vec<_> = BUILTINS.iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            [
                "Sampler",
                "Drum Kit",
                "EQ",
                "Compressor",
                "Reverb",
                "Delay",
                "Limiter"
            ]
        );
        let instruments = BUILTINS
            .iter()
            .filter(|d| d.kind == Kind::Instrument)
            .count();
        assert_eq!(instruments, 2);
    }

    #[test]
    fn every_spec_is_well_formed() {
        for d in BUILTINS {
            assert!(d.params.len() <= MAX_PARAMS, "{}", d.name);
            for (i, p) in d.params.iter().enumerate() {
                assert!(
                    p.id.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "{}: {}",
                    d.name,
                    p.id
                );
                assert_eq!(d.param(p.id).map(|x| x.0), Some(i), "{} の id が重複", p.id);
                let (lo, hi) = p.range();
                assert!(lo < hi, "{}.{}", d.name, p.id);
                assert!(
                    (lo..=hi).contains(&p.default),
                    "{}.{} の既定が範囲の外",
                    d.name,
                    p.id
                );
                if p.curve == Curve::Log {
                    assert!(p.min > 0.0, "{}.{}", d.name, p.id);
                }
                assert!(!p.label.is_empty());
            }
        }
    }

    #[test]
    fn plain_and_norm_round_trip() {
        for d in BUILTINS {
            for p in d.params {
                let back = p.to_plain(p.default_norm());
                assert!(
                    (back - p.default).abs() < 1e-9 * p.default.abs().max(1.0),
                    "{}.{}: {} -> {back}",
                    d.name,
                    p.id,
                    p.default
                );
                let (lo, hi) = p.range();
                assert_eq!(p.to_plain(0.0), lo);
                assert!((p.to_plain(1.0) - hi).abs() < 1e-9 * hi.abs().max(1.0));
                assert_eq!(p.to_norm(lo - 1000.0), 0.0);
                assert_eq!(p.to_norm(hi + 1e9), 1.0);
            }
        }
    }

    #[test]
    fn log_curve_puts_the_geometric_middle_at_half() {
        let (_, p) = EQ.param("mid1_freq").unwrap();
        let mid = (p.min * p.max).sqrt();
        assert!((p.to_norm(mid) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn text_shows_units_and_labels() {
        let (_, freq) = EQ.param("mid1_freq").unwrap();
        assert_eq!(freq.text(1200.0), "1.20 kHz");
        assert_eq!(freq.text(350.0), "350 Hz");
        let (_, gain) = EQ.param("mid1_gain").unwrap();
        assert_eq!(gain.text(-12.0), "-12.0 dB");
        let (_, feel) = DELAY.param("feel").unwrap();
        assert_eq!(feel.text(1.0), "付点");
        let (_, sync) = DELAY.param("sync").unwrap();
        assert_eq!(sync.text(1.0), "オン");
        let (_, ratio) = COMPRESSOR.param("ratio").unwrap();
        assert_eq!(ratio.text(4.0), "4.0:1");
        let (_, time) = DELAY.param("time").unwrap();
        assert_eq!(time.text(1500.0), "1.50 s");
    }

    #[test]
    fn parse_reads_numbers_labels_and_switches() {
        let (_, feel) = DELAY.param("feel").unwrap();
        assert_eq!(feel.parse("3連"), Some(2.0));
        assert_eq!(feel.parse("1"), Some(1.0));
        assert_eq!(feel.parse("9"), None);
        let (_, sync) = DELAY.param("sync").unwrap();
        assert_eq!(sync.parse("off"), Some(0.0));
        let (_, gain) = EQ.param("mid1_gain").unwrap();
        assert_eq!(gain.parse("-6.5"), Some(-6.5));
        assert_eq!(gain.parse("-60"), None);
        assert_eq!(gain.parse("loud"), None);
    }

    #[test]
    fn find_ignores_case_and_separators() {
        assert_eq!(find("Drum Kit").map(|d| d.uid), Some("drum-kit"));
        assert_eq!(find("drumkit").map(|d| d.uid), Some("drum-kit"));
        assert_eq!(find("EQ").map(|d| d.uid), Some("eq"));
        assert_eq!(find("LIMITER").map(|d| d.name), Some("Limiter"));
        assert_eq!(find("gain"), None);
        assert_eq!(find("Vital"), None);
    }

    #[test]
    fn choice_steps_land_on_indices() {
        let (_, div) = DELAY.param("division").unwrap();
        let n = div.choices().len();
        for i in 0..n {
            let norm = div.to_norm(i as f64);
            assert_eq!(div.to_plain(norm), i as f64);
            // A value a little off the step still lands on it.
            assert_eq!(div.to_plain((norm + 0.01).min(1.0)), i as f64);
        }
        assert_eq!(div.steps(), Some(n as u32));
    }
}
