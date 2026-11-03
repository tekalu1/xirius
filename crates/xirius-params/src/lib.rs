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
pub mod eq;
pub mod pitch;

pub use catalog::{
    BUILTINS, CHOKES, COMPRESSOR, DELAY, DIVISIONS, DRUM_KIT, EQ, FEELS, FILTER_TYPES, LAYER_MODES,
    LIMITER, PAD_NAMES, PADS, REVERB, SAMPLER, SLOPES, TRIGGERS,
};
pub use eq::{BandType, EQ_BANDS, EqBand};

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
    /// Left to right, -100..100, shown as `L50`, `C`, `R50`.
    Pan,
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
            Unit::Pan => "",
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

/// Where a cut's knob is off, letting everything through, and reads `Off`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Off {
    /// Every value does something.
    Never,
    /// At or below this plain value (a low cut at the bottom of its range).
    AtOrBelow(f64),
    /// At or above it (a high cut at the top of its range).
    AtOrAbove(f64),
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
    pub off: Off,
    /// Whether the built-in presets set it and are told apart by it. The sidechain's knobs
    /// are not (PLG-071): a preset leaves them where they are, and a song from before them
    /// still reads the preset it was at.
    pub preset: bool,
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

    /// Whether a plain value is where the knob is off ([`Off`]).
    pub fn is_off(&self, plain: f64) -> bool {
        match self.off {
            Off::Never => false,
            Off::AtOrBelow(at) => plain <= at,
            Off::AtOrAbove(at) => plain >= at,
        }
    }

    /// A plain value as the UI and the CLI show it (`-12.0 dB`, `1.20 kHz`, `付点`, `Off`).
    pub fn text(&self, plain: f64) -> String {
        let (lo, hi) = self.range();
        let v = plain.clamp(lo, hi);
        if self.is_off(v) {
            return "Off".to_string();
        }
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
            Unit::None if self.curve == Curve::Int => format!("{v:.0}"),
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
            Unit::Pan => pan_text(v),
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
        if self.unit == Unit::Pan {
            return pan_parse(t).filter(|v| *v >= lo && *v <= hi);
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
            _ => match (self.off, t.to_ascii_lowercase().as_str()) {
                (Off::AtOrBelow(_), "off" | "オフ") => Some(lo),
                (Off::AtOrAbove(_), "off" | "オフ") => Some(hi),
                _ => None,
            },
        }
    }
}

impl ParamSpec {
    /// Reads what is typed into a device window's value (UI-324): a number with or without its
    /// unit ("2k", "2.5 kHz", "-3", "-3 dB", "120ms", "0.5 s"), a share of the sweep ("50%" on
    /// a knob that is not in percent), a fraction ("1/8"), a choice's label, on/off or "off"
    /// for a cut. The value is taken to the nearest end of the range. `None` when the text is
    /// none of these or its unit is not the parameter's.
    pub fn typed(&self, text: &str) -> Option<f64> {
        let t: String = text.trim().chars().map(halfwidth).collect();
        let (lo, hi) = self.range();
        let clamp = |v: f64| v.is_finite().then(|| v.clamp(lo, hi));
        match self.curve {
            Curve::Choice(labels) => {
                if let Some(i) = labels.iter().position(|l| l.eq_ignore_ascii_case(&t)) {
                    return Some(i as f64);
                }
                return t.parse::<f64>().ok().and_then(|v| clamp(v.round()));
            }
            Curve::Toggle => {
                return match t.to_lowercase().as_str() {
                    "on" | "true" | "1" | "オン" => Some(1.0),
                    "off" | "false" | "0" | "オフ" => Some(0.0),
                    _ => None,
                };
            }
            _ => {}
        }
        if self.unit == Unit::Pan {
            return pan_parse(&t).and_then(clamp);
        }
        let lower = t.to_lowercase();
        if matches!(lower.as_str(), "off" | "オフ") {
            return match self.off {
                Off::AtOrBelow(_) => Some(lo),
                Off::AtOrAbove(_) => Some(hi),
                Off::Never => None,
            };
        }
        if let Some((a, b)) = lower.split_once('/') {
            let (a, b) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?);
            return (b != 0.0).then(|| a / b).and_then(clamp);
        }
        let split = lower
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
            .unwrap_or(lower.len());
        let (number, unit) = lower.split_at(split);
        let v: f64 = number.trim().parse().ok()?;
        let plain = match (unit.trim(), self.unit) {
            ("", _) => v,
            ("k" | "khz", Unit::Hz) => v * 1000.0,
            ("hz", Unit::Hz)
            | ("db", Unit::Db)
            | ("ms", Unit::Ms)
            | ("s", Unit::Seconds)
            | ("%", Unit::Percent)
            | (":1", Unit::Ratio)
            | ("st", Unit::Semitones)
            | ("cent", Unit::Cents) => v,
            ("s", Unit::Ms) => v * 1000.0,
            ("ms", Unit::Seconds) => v / 1000.0,
            // A share of the sweep on a knob of another unit.
            ("%", _) => self.to_plain(v / 100.0),
            _ => return None,
        };
        clamp(plain)
    }
}

/// A pan as it reads: `C` in the middle, else the side and how far (`L50`, `R100`).
fn pan_text(v: f64) -> String {
    let n = v.abs().round();
    if n < 1.0 {
        "C".to_string()
    } else if v < 0.0 {
        format!("L{n:.0}")
    } else {
        format!("R{n:.0}")
    }
}

/// `C`, `L50`, `r 20` or a plain number (left below 0).
fn pan_parse(t: &str) -> Option<f64> {
    let t = t.trim().to_ascii_uppercase();
    if t == "C" {
        return Some(0.0);
    }
    if let Some(rest) = t.strip_prefix('L') {
        return rest.trim().parse::<f64>().ok().map(|v| -v.abs());
    }
    if let Some(rest) = t.strip_prefix('R') {
        return rest.trim().parse::<f64>().ok().map(f64::abs);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A full-width digit, sign or mark as the half-width one (what an IME types).
fn halfwidth(c: char) -> char {
    match c {
        '\u{2212}' | '\u{FF0D}' => '-',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{3000}' => ' ',
        c => c,
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
    /// Ids a song from before may still keep: they are dropped without a warning.
    pub retired: &'static [&'static str],
    /// What each of the device's sample files has (PLG-067): kept as `file<n>.<id>`, `n`
    /// counting the plugin's `samples` from 1.
    pub per_file: &'static [ParamSpec],
    /// What each pad has (PLG-068): kept as `<pad>.<id>`, the pad named as [`PAD_NAMES`].
    pub per_pad: &'static [ParamSpec],
}

/// Whose value a key is ([`BuiltinDef::resolve`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The device's own: one of `params`.
    Device,
    /// A sample file's, by its index (from 0) in the plugin's `samples`.
    File(usize),
    /// A pad's, 0..[`PADS`].
    Pad(usize),
}

/// A key of a built-in's values, read ([`BuiltinDef::resolve`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// The key as a song keeps it (`attack`, `file2.start`, `A1.decay`).
    pub key: String,
    pub scope: Scope,
    pub spec: &'static ParamSpec,
}

/// The key a sample file's value is kept under: `file<n>.<id>` (`n` from 1).
pub fn file_key(index: usize, id: &str) -> String {
    format!("file{}.{id}", index + 1)
}

/// The key a pad's value is kept under: `A1.decay`.
pub fn pad_key(pad: usize, id: &str) -> String {
    format!("{}.{id}", PAD_NAMES[pad % PADS])
}

/// The pad a name like `A1` or `h16` means.
pub fn pad_of(name: &str) -> Option<usize> {
    PAD_NAMES
        .iter()
        .position(|p| p.eq_ignore_ascii_case(name.trim()))
}

/// A key's own id, without the file or pad it is of (`file2.start` is `start`).
pub fn bare_id(key: &str) -> &str {
    key.rsplit_once('.').map_or(key, |(_, id)| id)
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

    /// What a key names: one of the device's parameters (`attack`), a sample file's value
    /// (`file2.start`) or a pad's (`A1.decay`). Letter case is ignored; the key comes back as
    /// the song keeps it.
    pub fn resolve(&self, key: &str) -> Option<Resolved> {
        if let Some((_, spec)) = self.param(key) {
            return Some(Resolved {
                key: spec.id.to_string(),
                scope: Scope::Device,
                spec,
            });
        }
        let (head, id) = key.trim().split_once('.')?;
        let find =
            |specs: &'static [ParamSpec]| specs.iter().find(|p| p.id.eq_ignore_ascii_case(id));
        let file = head
            .get(..4)
            .filter(|h| h.eq_ignore_ascii_case("file"))
            .and_then(|_| head[4..].parse::<usize>().ok())
            .filter(|n| *n >= 1);
        if let Some(n) = file {
            let spec = find(self.per_file)?;
            return Some(Resolved {
                key: file_key(n - 1, spec.id),
                scope: Scope::File(n - 1),
                spec,
            });
        }
        let pad = pad_of(head)?;
        let spec = find(self.per_pad)?;
        Some(Resolved {
            key: pad_key(pad, spec.id),
            scope: Scope::Pad(pad),
            spec,
        })
    }

    /// The pad the Drum Kit's file `i` (from 0) is a layer of: what `file<n>.pad` says, else
    /// the `i`th pad, so files given without one fill the pads in order. 0 on a device
    /// without pads.
    pub fn file_pad(&self, values: &std::collections::BTreeMap<String, f64>, i: usize) -> usize {
        let Some(spec) = self.per_file.iter().find(|p| p.id == "pad") else {
            return 0;
        };
        values
            .get(&file_key(i, "pad"))
            .map_or(i, |v| spec.to_plain(*v).round().max(0.0) as usize)
            .min(PADS - 1)
    }

    /// Whether a key is one the device no longer has ([`Self::retired`]).
    pub fn is_retired(&self, key: &str) -> bool {
        self.retired.iter().any(|r| r.eq_ignore_ascii_case(key))
    }

    /// The device's parameters followed by a file's or a pad's, under the keys the song keeps
    /// them by (`file2.start`): what a window shows while that zone or pad is chosen. Made
    /// once for each and kept, so it lives as long as the program.
    pub fn view(&'static self, scope: Scope) -> &'static BuiltinDef {
        type Views = Vec<(&'static str, Scope, &'static BuiltinDef)>;
        static VIEWS: std::sync::Mutex<Views> = std::sync::Mutex::new(Vec::new());
        let extra = match scope {
            Scope::Device => return self,
            Scope::File(_) => self.per_file,
            Scope::Pad(_) => self.per_pad,
        };
        if extra.is_empty() {
            return self;
        }
        let mut views = VIEWS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, _, d)) = views.iter().find(|(u, s, _)| *u == self.uid && *s == scope) {
            return d;
        }
        let key = |id: &str| match scope {
            Scope::File(i) => file_key(i, id),
            Scope::Pad(p) => pad_key(p, id),
            Scope::Device => id.to_string(),
        };
        let params: Vec<ParamSpec> = self
            .params
            .iter()
            .copied()
            .chain(extra.iter().map(|p| ParamSpec {
                id: Box::leak(key(p.id).into_boxed_str()),
                ..*p
            }))
            .collect();
        let d: &'static BuiltinDef = Box::leak(Box::new(BuiltinDef {
            params: Box::leak(params.into_boxed_slice()),
            ..*self
        }));
        views.push((self.uid, scope, d));
        d
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

/// The knobs a factory preset sets: each knob's id and its plain value.
pub type FactorySet = &'static [(&'static str, f64)];

/// The named factory presets of the built-ins (PLG-064): the device's uid, the preset's name
/// and the plain values it sets. The knobs a preset leaves out sit at their defaults. The
/// session writes them into the user's preset folder on the first run; after that they are
/// plain preset files (`preset.factory.restore` adds back the ones still missing). `Default`
/// is not among them: it is the device's own defaults, not a file. The Sampler and the Drum
/// Kit have only their Default.
pub const FACTORY_PRESETS: &[(&str, &str, FactorySet)] = &[
    (
        "eq",
        "ボーカルを明るく",
        &[
            ("low_cut_on", 1.0),
            ("low_cut_freq", 80.0),
            ("low_cut_slope", 1.0),
            ("low_freq", 120.0),
            ("low_gain", 2.0),
            ("mid1_freq", 350.0),
            ("mid1_gain", -3.5),
            ("mid1_q", 1.4),
            ("mid2_freq", 3200.0),
            ("mid2_gain", 2.5),
            ("mid2_q", 0.9),
            ("high_freq", 10_000.0),
            ("high_gain", 1.5),
        ],
    ),
    (
        "eq",
        "低域を締める",
        &[
            ("low_cut_on", 1.0),
            ("low_cut_freq", 90.0),
            ("low_cut_slope", 2.0),
            ("low_freq", 180.0),
            ("low_gain", -2.5),
            ("mid1_freq", 320.0),
            ("mid1_gain", -2.0),
            ("mid1_q", 1.2),
        ],
    ),
    (
        "eq",
        "電話の声",
        &[
            ("low_cut_on", 1.0),
            ("low_cut_freq", 400.0),
            ("low_cut_slope", 2.0),
            ("mid1_freq", 1500.0),
            ("mid1_gain", 4.0),
            ("high_cut_on", 1.0),
            ("high_cut_freq", 3200.0),
            ("high_cut_slope", 2.0),
        ],
    ),
    (
        "compressor",
        "ボーカルを整える",
        &[
            ("threshold", -24.0),
            ("ratio", 3.0),
            ("attack", 8.0),
            ("release", 140.0),
            ("knee", 6.0),
            ("makeup", 5.0),
        ],
    ),
    (
        "compressor",
        "ドラムをまとめる",
        &[
            ("threshold", -16.0),
            ("ratio", 4.0),
            ("attack", 25.0),
            ("release", 90.0),
            ("knee", 3.0),
            ("makeup", 3.0),
        ],
    ),
    (
        "compressor",
        "並列で太く",
        &[
            ("threshold", -34.0),
            ("ratio", 8.0),
            ("attack", 1.5),
            ("release", 70.0),
            ("knee", 0.0),
            ("makeup", 12.0),
            ("mix", 45.0),
        ],
    ),
    (
        "limiter",
        "マスターの仕上げ",
        &[("input", 4.0), ("ceiling", -1.0), ("release", 120.0)],
    ),
    (
        "limiter",
        "配信の音量",
        &[("input", 8.0), ("ceiling", -1.0), ("release", 200.0)],
    ),
    (
        "delay",
        "4 分のピンポン",
        &[
            ("division", 3.0),
            ("feel", 0.0),
            ("ping_pong", 1.0),
            ("feedback", 40.0),
            ("mix", 25.0),
        ],
    ),
    (
        "delay",
        "スラップバック",
        &[
            ("sync", 0.0),
            ("time", 90.0),
            ("feedback", 5.0),
            ("mix", 25.0),
            ("low_cut", 120.0),
            ("high_cut", 5000.0),
        ],
    ),
    (
        "delay",
        "3 連の広がり",
        &[
            ("division", 2.0),
            ("feel", 2.0),
            ("ping_pong", 1.0),
            ("feedback", 45.0),
        ],
    ),
    (
        "reverb",
        "小さい部屋",
        &[
            ("size", 25.0),
            ("decay", 0.6),
            ("predelay", 2.0),
            ("damping", 9000.0),
            ("early", 60.0),
            ("mix", 20.0),
        ],
    ),
    (
        "reverb",
        "ホール",
        &[
            ("size", 85.0),
            ("decay", 3.2),
            ("predelay", 24.0),
            ("damping", 6000.0),
            ("early", 30.0),
            ("mix", 25.0),
        ],
    ),
    (
        "reverb",
        "ボーカルのプレート",
        &[
            ("size", 55.0),
            ("decay", 1.8),
            ("predelay", 40.0),
            ("damping", 10_000.0),
            ("low_cut", 250.0),
            ("early", 15.0),
            ("mix", 20.0),
        ],
    ),
];

/// The version of the factory set the session copies to the user's folder. Raise it when
/// the set changes: a folder copied from an older set then gets the new presets, and loses
/// the old ones it copied that the user left untouched. 2: each device's own presets
/// (PLG-064) in place of the three shared ones.
pub const FACTORY_VERSION: u32 = 2;

/// The names of a built-in's factory presets, in the set's order.
pub fn factory_names(def: &BuiltinDef) -> Vec<&'static str> {
    FACTORY_PRESETS
        .iter()
        .filter(|(uid, _, _)| *uid == def.uid)
        .map(|(_, name, _)| *name)
        .collect()
}

/// The factory presets of a built-in: a name and the knob values (0..1) in `def.params`
/// order. A knob a preset leaves out keeps its default.
pub fn factory_presets(def: &BuiltinDef) -> Vec<(&'static str, Vec<f64>)> {
    FACTORY_PRESETS
        .iter()
        .filter(|(uid, _, _)| *uid == def.uid)
        .map(|(_, name, set)| {
            let values = def
                .params
                .iter()
                .map(|p| match set.iter().find(|(id, _)| *id == p.id) {
                    Some((_, v)) => p.to_norm(*v),
                    None => p.default_norm(),
                })
                .collect();
            (*name, values)
        })
        .collect()
}

/// The most parameters a built-in has, for fixed-size tables on the audio thread.
pub const MAX_PARAMS: usize = 48;

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
    fn each_factory_preset_names_its_devices_knobs_inside_their_ranges() {
        // PLG-064: every id is the device's, every value inside its range, and one no preset
        // sets (the Compressor's sidechain) is never among them.
        for (uid, name, set) in FACTORY_PRESETS {
            let def = find(uid).unwrap_or_else(|| panic!("{uid}"));
            assert_eq!(def.kind, Kind::Effect, "{name}");
            for (id, v) in *set {
                let (_, p) = def.param(id).unwrap_or_else(|| panic!("{name}: {id}"));
                assert!(p.preset, "{name}: {id}");
                let (lo, hi) = p.range();
                assert!((lo..=hi).contains(v), "{name}: {id} = {v}");
                assert!((p.to_plain(p.to_norm(*v)) - v).abs() < 1e-6 * v.abs().max(1.0));
            }
        }
        assert_eq!(
            factory_names(&EQ),
            ["ボーカルを明るく", "低域を締める", "電話の声"]
        );
        assert_eq!(factory_names(&LIMITER).len(), 2);
        assert!(factory_names(&SAMPLER).is_empty());
        assert!(factory_names(&DRUM_KIT).is_empty());
        let phone = &factory_presets(&EQ)[2].1;
        let (k, p) = EQ.param("high_cut_freq").unwrap();
        assert!((p.to_plain(phone[k]) - 3200.0).abs() < 1e-6);
        let (k, p) = EQ.param("b2_type").unwrap();
        assert_eq!(phone[k], p.default_norm());
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
    fn the_compressors_cuts_read_off_at_their_ends() {
        // PLG-071: the Low Cut is off at the bottom of its range, the High Cut at the top,
        // as Studio Pro's knobs read; a hair off the end still is (the engine keeps a value
        // in an f32).
        let (_, low) = COMPRESSOR.param("low_cut").unwrap();
        let (_, high) = COMPRESSOR.param("high_cut").unwrap();
        assert_eq!(low.text(20.0), "Off");
        assert_eq!(low.text(20.3), "Off");
        assert_eq!(low.text(21.0), "21 Hz");
        assert_eq!(high.text(20_000.0), "Off");
        assert_eq!(high.text(19_999.5), "Off");
        assert_eq!(high.text(19_000.0), "19.0 kHz");
        assert_eq!(low.parse("Off"), Some(20.0));
        assert_eq!(high.parse("off"), Some(20_000.0));
        // A cut with no Off reads its frequency at its end.
        let (_, delay) = DELAY.param("high_cut").unwrap();
        assert_eq!(delay.text(20_000.0), "20.0 kHz");
        assert_eq!(delay.parse("off"), None);
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

    #[test]
    fn ui_324_typed_values_read_units_shares_fractions_and_labels() {
        let freq = EQ.param("mid1_freq").unwrap().1;
        assert_eq!(freq.typed("2k"), Some(2000.0));
        assert_eq!(freq.typed("2.5 kHz"), Some(2500.0));
        assert_eq!(freq.typed("440hz"), Some(440.0));
        assert_eq!(
            freq.typed("３００"),
            Some(300.0),
            "full-width digits from an IME"
        );
        assert_eq!(
            freq.typed("99k"),
            Some(20_000.0),
            "past the end goes to the end"
        );
        assert_eq!(freq.typed("3 dB"), None, "another unit");
        let gain = EQ.param("mid1_gain").unwrap().1;
        assert_eq!(gain.typed("-3"), Some(-3.0));
        assert_eq!(gain.typed("\u{2212}3 dB"), Some(-3.0));
        assert_eq!(gain.typed("+4.5db"), Some(4.5));
        assert_eq!(gain.typed("50%"), Some(0.0), "half the sweep of -18..18");
        let mix = DELAY.param("mix").unwrap().1;
        assert_eq!(mix.typed("50%"), Some(50.0));
        assert_eq!(mix.typed("50"), Some(50.0));
        let time = DELAY.param("time").unwrap().1;
        assert_eq!(time.typed("0.5 s"), Some(500.0));
        assert_eq!(time.typed("120ms"), Some(120.0));
        let division = DELAY.param("division").unwrap().1;
        assert_eq!(division.typed("1/8"), Some(2.0), "a label");
        assert_eq!(division.typed("1/2"), Some(4.0));
        let ratio = COMPRESSOR.param("ratio").unwrap().1;
        assert_eq!(ratio.typed("4:1"), Some(4.0));
        assert_eq!(ratio.typed("1/2"), Some(1.0), "a fraction, to the range");
        let low_cut = COMPRESSOR.param("low_cut").unwrap().1;
        assert_eq!(low_cut.typed("off"), Some(20.0));
        let on = EQ.param("low_cut_on").unwrap().1;
        assert_eq!(on.typed("On"), Some(1.0));
        assert_eq!(on.typed("オフ"), Some(0.0));
        assert_eq!(gain.typed("loud"), None);
        assert_eq!(gain.typed(""), None);
    }

    #[test]
    fn a_files_and_a_pads_values_resolve_to_the_keys_a_song_keeps() {
        let r = SAMPLER.resolve("FILE2.Start").unwrap();
        assert_eq!((r.key.as_str(), r.scope), ("file2.start", Scope::File(1)));
        assert_eq!(r.spec.id, "start");
        let r = DRUM_KIT.resolve("h16.decay").unwrap();
        assert_eq!((r.key.as_str(), r.scope), ("H16.decay", Scope::Pad(127)));
        let r = DRUM_KIT.resolve("file1.pad").unwrap();
        assert_eq!(r.scope, Scope::File(0));
        assert_eq!(SAMPLER.resolve("attack").unwrap().scope, Scope::Device);
        // No pads on a Sampler, no file 0, no such id.
        assert!(SAMPLER.resolve("A1.decay").is_none());
        assert!(SAMPLER.resolve("file0.start").is_none());
        assert!(DRUM_KIT.resolve("A1.nothing").is_none());
        assert!(DRUM_KIT.resolve("I1.decay").is_none());
        assert!(EQ.resolve("file1.start").is_none());
        assert!(SAMPLER.is_retired("sound") && DRUM_KIT.is_retired("kick"));
        assert_eq!(bare_id("A1.decay"), "decay");
    }

    #[test]
    fn a_view_puts_a_files_or_a_pads_values_after_the_devices() {
        let v = SAMPLER.view(Scope::File(2));
        assert_eq!(
            v.params.len(),
            SAMPLER.params.len() + SAMPLER.per_file.len()
        );
        assert_eq!(v.params[SAMPLER.params.len()].id, "file3.start");
        assert!(std::ptr::eq(v, SAMPLER.view(Scope::File(2))));
        let v = DRUM_KIT.view(Scope::Pad(17));
        assert_eq!(v.param("B2.trigger").unwrap().1.choices(), TRIGGERS);
        assert!(v.params.len() <= MAX_PARAMS);
        assert!(std::ptr::eq(EQ.view(Scope::Pad(0)), &EQ));
    }

    #[test]
    fn the_scoped_values_are_well_formed() {
        for d in BUILTINS {
            for p in d.per_file.iter().chain(d.per_pad) {
                let (lo, hi) = p.range();
                assert!(lo < hi && (lo..=hi).contains(&p.default), "{}", p.id);
                assert!(d.param(p.id).is_none() || p.id == "volume", "{}", p.id);
            }
            assert!(d.params.len() + d.per_file.len().max(d.per_pad.len()) <= MAX_PARAMS);
        }
        assert_eq!(PAD_NAMES[0], "A1");
        assert_eq!(PAD_NAMES[127], "H16");
        assert_eq!(pad_of("b3"), Some(18));
        assert_eq!(pad_key(18, "decay"), "B3.decay");
        assert_eq!(file_key(0, "pad"), "file1.pad");
    }

    #[test]
    fn a_pan_reads_as_a_side_and_how_far() {
        let pan = SAMPLER.param("pan").unwrap().1;
        assert_eq!(pan.text(0.0), "C");
        assert_eq!(pan.text(-50.0), "L50");
        assert_eq!(pan.text(100.0), "R100");
        assert_eq!(pan.typed("L50"), Some(-50.0));
        assert_eq!(pan.typed("r 20"), Some(20.0));
        assert_eq!(pan.typed("c"), Some(0.0));
        assert_eq!(pan.typed("-30"), Some(-30.0));
        assert_eq!(pan.parse("R10"), Some(10.0));
        let top = DRUM_KIT.resolve("file1.top").unwrap().spec;
        assert_eq!(top.text(64.0), "64");
    }
}
