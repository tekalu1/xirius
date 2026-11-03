//! The built-ins and their parameters (PLG-016, PLG-045).
//!
//! The order of a built-in's parameters is the order a list and a generated panel show them
//! in, and the index the engine reads them by. Ids are never renamed once a song can hold
//! them; a new parameter goes at the end of its group.

use crate::eq::{HIGH_TYPE_LABELS, LOW_TYPE_LABELS, MID_TYPE_LABELS};
use crate::{BuiltinDef, Curve, Kind, Off, ParamSpec, Unit};

const fn spec(
    id: &'static str,
    label: &'static str,
    unit: Unit,
    min: f64,
    max: f64,
    default: f64,
    curve: Curve,
) -> ParamSpec {
    ParamSpec {
        id,
        label,
        unit,
        min,
        max,
        default,
        curve,
        off: Off::Never,
        preset: true,
    }
}

/// A knob the built-in presets leave where it is.
const fn no_preset(p: ParamSpec) -> ParamSpec {
    ParamSpec { preset: false, ..p }
}

/// A knob added after the first factory presets were made (PLG-062). The set now names
/// each device's knobs one by one (PLG-064), so it is a knob like the others.
const fn later(p: ParamSpec) -> ParamSpec {
    p
}

/// A cut that is off at or below `at` (the bottom of its range).
const fn off_below(p: ParamSpec, at: f64) -> ParamSpec {
    ParamSpec {
        off: Off::AtOrBelow(at),
        ..p
    }
}

/// A cut that is off at or above `at` (the top of its range).
const fn off_above(p: ParamSpec, at: f64) -> ParamSpec {
    ParamSpec {
        off: Off::AtOrAbove(at),
        ..p
    }
}

const fn db(id: &'static str, label: &'static str, min: f64, max: f64, default: f64) -> ParamSpec {
    spec(id, label, Unit::Db, min, max, default, Curve::Linear)
}

const fn hz(id: &'static str, label: &'static str, min: f64, max: f64, default: f64) -> ParamSpec {
    spec(id, label, Unit::Hz, min, max, default, Curve::Log)
}

const fn ms(id: &'static str, label: &'static str, min: f64, max: f64, default: f64) -> ParamSpec {
    spec(id, label, Unit::Ms, min, max, default, Curve::Log)
}

const fn pct(id: &'static str, label: &'static str, max: f64, default: f64) -> ParamSpec {
    spec(id, label, Unit::Percent, 0.0, max, default, Curve::Linear)
}

const fn toggle(id: &'static str, label: &'static str, on: bool) -> ParamSpec {
    let default = if on { 1.0 } else { 0.0 };
    spec(id, label, Unit::None, 0.0, 1.0, default, Curve::Toggle)
}

const fn choice(
    id: &'static str,
    label: &'static str,
    labels: &'static [&'static str],
    default: usize,
) -> ParamSpec {
    let last = (labels.len() - 1) as f64;
    spec(
        id,
        label,
        Unit::None,
        0.0,
        last,
        default as f64,
        Curve::Choice(labels),
    )
}

const fn semis(id: &'static str, label: &'static str, range: f64) -> ParamSpec {
    spec(id, label, Unit::Semitones, -range, range, 0.0, Curve::Int)
}

/// The slopes of the EQ's cut filters.
pub const SLOPES: &[&str] = &["12 dB/oct", "24 dB/oct", "48 dB/oct"];

/// A band's Q. The default of a new one is 1/√2: a slope-1 shelf's, a Butterworth cut's.
const fn q(id: &'static str, label: &'static str, default: f64) -> ParamSpec {
    spec(id, label, Unit::None, 0.1, 18.0, default, Curve::Log)
}

const FLAT_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

pub static EQ: BuiltinDef = BuiltinDef {
    uid: "eq",
    name: "EQ",
    kind: Kind::Effect,
    retired: &[],
    per_file: &[],
    per_pad: &[],
    params: &[
        toggle("low_cut_on", "ローカット", false),
        hz("low_cut_freq", "ローカットの周波数", 20.0, 1000.0, 80.0),
        choice("low_cut_slope", "ローカットの傾き", SLOPES, 1),
        hz("low_freq", "低域の周波数", 20.0, 1000.0, 100.0),
        db("low_gain", "低域のゲイン", -18.0, 18.0, 0.0),
        hz("mid1_freq", "中域 1 の周波数", 20.0, 20_000.0, 400.0),
        db("mid1_gain", "中域 1 のゲイン", -18.0, 18.0, 0.0),
        spec(
            "mid1_q",
            "中域 1 の Q",
            Unit::None,
            0.1,
            18.0,
            1.0,
            Curve::Log,
        ),
        hz("mid2_freq", "中域 2 の周波数", 20.0, 20_000.0, 2500.0),
        db("mid2_gain", "中域 2 のゲイン", -18.0, 18.0, 0.0),
        spec(
            "mid2_q",
            "中域 2 の Q",
            Unit::None,
            0.1,
            18.0,
            1.0,
            Curve::Log,
        ),
        hz("high_freq", "高域の周波数", 1000.0, 20_000.0, 8000.0),
        db("high_gain", "高域のゲイン", -18.0, 18.0, 0.0),
        toggle("high_cut_on", "ハイカット", false),
        hz(
            "high_cut_freq",
            "ハイカットの周波数",
            1000.0,
            20_000.0,
            12_000.0,
        ),
        choice("high_cut_slope", "ハイカットの傾き", SLOPES, 1),
        db("output", "出力", -24.0, 24.0, 0.0),
        // Each band's type, on, Q and slope (PLG-062; `xirius_params::EQ_BANDS`). Their
        // defaults are the bands above as they were, so a song or a preset from before them
        // sounds the same.
        later(choice("b1_type", "ローカットの型", LOW_TYPE_LABELS, 0)),
        later(db("b1_gain", "ローカットのゲイン", -18.0, 18.0, 0.0)),
        later(q("b1_q", "ローカットの Q", FLAT_Q)),
        later(toggle("b2_on", "低域", true)),
        later(choice("b2_type", "低域の型", LOW_TYPE_LABELS, 1)),
        later(q("b2_q", "低域の Q", FLAT_Q)),
        later(choice("b2_slope", "低域の傾き", SLOPES, 1)),
        later(toggle("b3_on", "中域 1", true)),
        later(choice("b3_type", "中域 1 の型", MID_TYPE_LABELS, 2)),
        later(choice("b3_slope", "中域 1 の傾き", SLOPES, 1)),
        later(toggle("b4_on", "中域 2", true)),
        later(choice("b4_type", "中域 2 の型", MID_TYPE_LABELS, 2)),
        later(choice("b4_slope", "中域 2 の傾き", SLOPES, 1)),
        later(toggle("b5_on", "高域", true)),
        later(choice("b5_type", "高域の型", HIGH_TYPE_LABELS, 2)),
        later(q("b5_q", "高域の Q", FLAT_Q)),
        later(choice("b5_slope", "高域の傾き", SLOPES, 1)),
        later(choice("b6_type", "ハイカットの型", HIGH_TYPE_LABELS, 3)),
        later(db("b6_gain", "ハイカットのゲイン", -18.0, 18.0, 0.0)),
        later(q("b6_q", "ハイカットの Q", FLAT_Q)),
    ],
};

pub static COMPRESSOR: BuiltinDef = BuiltinDef {
    uid: "compressor",
    name: "Compressor",
    kind: Kind::Effect,
    retired: &[],
    per_file: &[],
    per_pad: &[],
    params: &[
        db("threshold", "スレッショルド", -60.0, 0.0, -18.0),
        spec("ratio", "レシオ", Unit::Ratio, 1.0, 20.0, 4.0, Curve::Log),
        ms("attack", "アタック", 0.1, 200.0, 10.0),
        ms("release", "リリース", 10.0, 2000.0, 120.0),
        db("knee", "ニー", 0.0, 24.0, 6.0),
        db("makeup", "メイクアップ", 0.0, 24.0, 0.0),
        pct("mix", "ミックス", 100.0, 100.0),
        // The sidechain (PLG-071), in Studio Pro's words. Int or Ext is the plug-in's
        // sidechain switch (the title bar's), not a knob: the key comes from the sources
        // whenever it is on. One Gain: the Input Gain at Int, the Side Gain (on the key
        // alone) at Ext. The presets leave all of them where they are.
        no_preset(db("gain", "Gain", -12.0, 24.0, 0.0)),
        no_preset(toggle("filter", "Filter", false)),
        // At the bottom of its range the low cut is off, at the top the high cut (Studio
        // Pro's Off past 16 kHz is the top's 20 kHz here).
        no_preset(off_below(
            hz("low_cut", "Low Cut", 20.0, 16_000.0, 20.0),
            20.5,
        )),
        no_preset(off_above(
            hz("high_cut", "High Cut", 20.0, 20_000.0, 20_000.0),
            19_999.0,
        )),
        no_preset(toggle("listen", "Listen", false)),
    ],
};

/// The note lengths the Delay can follow the tempo in.
pub const DIVISIONS: &[&str] = &["1/32", "1/16", "1/8", "1/4", "1/2", "1/1"];
/// Straight, dotted or triplet.
pub const FEELS: &[&str] = &["ストレート", "付点", "3連"];

pub static DELAY: BuiltinDef = BuiltinDef {
    uid: "delay",
    name: "Delay",
    kind: Kind::Effect,
    retired: &[],
    per_file: &[],
    per_pad: &[],
    params: &[
        toggle("sync", "テンポに合わせる", true),
        ms("time", "時間", 1.0, 2000.0, 375.0),
        choice("division", "音符", DIVISIONS, 2),
        choice("feel", "音符の種類", FEELS, 1),
        pct("feedback", "フィードバック", 95.0, 35.0),
        pct("mix", "ミックス", 100.0, 30.0),
        toggle("ping_pong", "ピンポン", false),
        hz("low_cut", "ローカット", 20.0, 2000.0, 80.0),
        hz("high_cut", "ハイカット", 1000.0, 20_000.0, 8000.0),
    ],
};

pub static REVERB: BuiltinDef = BuiltinDef {
    uid: "reverb",
    name: "Reverb",
    kind: Kind::Effect,
    retired: &[],
    per_file: &[],
    per_pad: &[],
    params: &[
        pct("size", "広さ", 100.0, 50.0),
        spec(
            "decay",
            "残響の長さ",
            Unit::Seconds,
            0.2,
            20.0,
            2.0,
            Curve::Log,
        ),
        spec(
            "predelay",
            "プリディレイ",
            Unit::Ms,
            0.0,
            200.0,
            10.0,
            Curve::Linear,
        ),
        hz("damping", "高域の減衰", 1000.0, 20_000.0, 7000.0),
        hz("low_cut", "ローカット", 20.0, 1000.0, 120.0),
        pct("early", "初期反射", 100.0, 40.0),
        pct("width", "広がり", 100.0, 100.0),
        pct("mix", "ミックス", 100.0, 25.0),
    ],
};

pub static LIMITER: BuiltinDef = BuiltinDef {
    uid: "limiter",
    name: "Limiter",
    kind: Kind::Effect,
    retired: &[],
    per_file: &[],
    per_pad: &[],
    params: &[
        db("input", "入力ゲイン", 0.0, 24.0, 0.0),
        db("ceiling", "上限", -24.0, 0.0, -0.3),
        ms("release", "リリース", 1.0, 1000.0, 100.0),
    ],
};

/// The Sampler's and the Drum Kit's sound, the same knobs in both (PLG-067, PLG-068): the
/// envelope, the filter and the amp. The Sampler has one set for the device, the Drum Kit one
/// per pad.
macro_rules! voice {
    () => {
        [
            ms("attack", "アタック", 1.0, 5000.0, 2.0),
            ms("decay", "ディケイ", 1.0, 10_000.0, 1000.0),
            pct("sustain", "サステイン", 100.0, 100.0),
            ms("release", "リリース", 1.0, 10_000.0, 300.0),
            choice("ftype", "フィルターの型", FILTER_TYPES, 0),
            hz("cutoff", "カットオフ", 20.0, 20_000.0, 20_000.0),
            pct("resonance", "レゾナンス", 100.0, 0.0),
            pct("drive", "ドライブ", 100.0, 0.0),
            semis("transpose", "トランスポーズ", 48.0),
            spec(
                "fine",
                "微調整",
                Unit::Cents,
                -100.0,
                100.0,
                0.0,
                Curve::Linear,
            ),
            db("volume", "ボリューム", -48.0, 12.0, 0.0),
            spec("pan", "パン", Unit::Pan, -100.0, 100.0, 0.0, Curve::Linear),
            pct("velocity", "ベロシティの効き", 100.0, 80.0),
        ]
    };
}

/// Where a sample plays from and how (PLG-067): a Sampler zone's own, a Drum Kit pad's.
/// Start, End and the loop are shares of the file's length.
macro_rules! sample {
    () => {
        [
            pct("start", "開始", 100.0, 0.0),
            pct("end", "終わり", 100.0, 100.0),
            toggle("loop", "ループ", false),
            pct("loop_start", "ループの開始", 100.0, 35.0),
            pct("loop_end", "ループの終わり", 100.0, 85.0),
            pct("xfade", "クロスフェード", 100.0, 10.0),
            toggle("reverse", "逆再生", false),
            toggle("normalize", "ノーマライズ", false),
        ]
    };
}

/// The filter's types: low pass, band pass, high pass.
pub const FILTER_TYPES: &[&str] = &["LP", "BP", "HP"];

const VOICE: [ParamSpec; 13] = voice!();
const SAMPLE: [ParamSpec; 8] = sample!();

pub static SAMPLER: BuiltinDef = BuiltinDef {
    uid: "sampler",
    name: "Sampler",
    kind: Kind::Instrument,
    params: &VOICE,
    // The synthesized sounds and the pitch in semitones of before PLG-067: a song that kept
    // them opens without a word (`tune` is `transpose` now, over a wider range).
    retired: &["sound", "tune"],
    per_file: &SAMPLE,
    per_pad: &[],
};

/// The pads' names, bank by bank: A1..A16, B1..B16, .. H16. Pad `i` plays note 36 + i.
pub const PAD_NAMES: [&str; PADS] = [
    "A1", "A2", "A3", "A4", "A5", "A6", "A7", "A8", "A9", "A10", "A11", "A12", "A13", "A14", "A15",
    "A16", "B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9", "B10", "B11", "B12", "B13", "B14",
    "B15", "B16", "C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10", "C11", "C12", "C13",
    "C14", "C15", "C16", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "D8", "D9", "D10", "D11", "D12",
    "D13", "D14", "D15", "D16", "E1", "E2", "E3", "E4", "E5", "E6", "E7", "E8", "E9", "E10", "E11",
    "E12", "E13", "E14", "E15", "E16", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10",
    "F11", "F12", "F13", "F14", "F15", "F16", "G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9",
    "G10", "G11", "G12", "G13", "G14", "G15", "G16", "H1", "H2", "H3", "H4", "H5", "H6", "H7",
    "H8", "H9", "H10", "H11", "H12", "H13", "H14", "H15", "H16",
];

/// How many pads a Drum Kit has: 8 banks of 16.
pub const PADS: usize = 128;

/// How a pad picks among its layers.
pub const LAYER_MODES: &[&str] = &["Velocity", "Round Robin", "Random", "Stack"];
/// How a pad plays a note: One Shot plays the whole envelope whatever the note-off, Loop
/// loops the sample while the note is held, Normal is a Sampler's key, Toggle starts on one
/// note and stops on the next.
pub const TRIGGERS: &[&str] = &["One Shot", "Loop", "Normal", "Toggle"];
/// The choke groups: a pad stops the others of its group (Self: only itself again).
pub const CHOKES: [&str; 34] = [
    "None", "Self", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14",
    "15", "16", "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30",
    "31", "32",
];

const PAD: [ParamSpec; 26] = {
    let v: [ParamSpec; 13] = voice!();
    let s: [ParamSpec; 8] = sample!();
    [
        v[0],
        v[1],
        v[2],
        v[3],
        v[4],
        v[5],
        v[6],
        v[7],
        v[8],
        v[9],
        v[10],
        v[11],
        v[12],
        s[0],
        s[1],
        s[2],
        s[3],
        s[4],
        s[5],
        s[6],
        s[7],
        choice("layer_mode", "レイヤーの選び方", LAYER_MODES, 0),
        choice("trigger", "トリガー", TRIGGERS, 0),
        choice("choke", "チョーク", &CHOKES, 0),
        toggle("mute", "ミュート", false),
        toggle("solo", "ソロ", false),
    ]
};

/// A Drum Kit's file: the pad it is on, and the top velocity of its layer there.
const KIT_FILE: [ParamSpec; 2] = [
    choice("pad", "パッド", &PAD_NAMES, 0),
    spec(
        "top",
        "レイヤーの上端",
        Unit::None,
        1.0,
        127.0,
        127.0,
        Curve::Int,
    ),
];

pub static DRUM_KIT: BuiltinDef = BuiltinDef {
    uid: "drum-kit",
    name: "Drum Kit",
    kind: Kind::Instrument,
    params: &[db("volume", "ボリューム", -48.0, 12.0, 0.0)],
    // The synthesized kits and their knobs of before PLG-068.
    retired: &[
        "kit",
        "tune",
        "decay",
        "velocity",
        "kick",
        "snare",
        "hats",
        "toms",
        "cymbals",
        "percussion",
    ],
    per_file: &KIT_FILE,
    per_pad: &PAD,
};

/// Instruments first, then effects, in the order the browser lists them.
pub static BUILTINS: &[&BuiltinDef] = &[
    &SAMPLER,
    &DRUM_KIT,
    &EQ,
    &COMPRESSOR,
    &REVERB,
    &DELAY,
    &LIMITER,
];
