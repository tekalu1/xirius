//! The built-ins and their parameters (PLG-016, PLG-045).
//!
//! The order of a built-in's parameters is the order a list and a generated panel show them
//! in, and the index the engine reads them by. Ids are never renamed once a song can hold
//! them; a new parameter goes at the end of its group.

use crate::{BuiltinDef, Curve, Kind, ParamSpec, Unit};

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

pub static EQ: BuiltinDef = BuiltinDef {
    uid: "eq",
    name: "EQ",
    kind: Kind::Effect,
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
    ],
};

pub static COMPRESSOR: BuiltinDef = BuiltinDef {
    uid: "compressor",
    name: "Compressor",
    kind: Kind::Effect,
    params: &[
        db("threshold", "スレッショルド", -60.0, 0.0, -18.0),
        spec("ratio", "レシオ", Unit::Ratio, 1.0, 20.0, 4.0, Curve::Log),
        ms("attack", "アタック", 0.1, 200.0, 10.0),
        ms("release", "リリース", 10.0, 2000.0, 120.0),
        db("knee", "ニー", 0.0, 24.0, 6.0),
        db("makeup", "メイクアップ", 0.0, 24.0, 0.0),
        pct("mix", "ミックス", 100.0, 100.0),
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
    params: &[
        db("input", "入力ゲイン", 0.0, 24.0, 0.0),
        db("ceiling", "上限", -24.0, 0.0, -0.3),
        ms("release", "リリース", 1.0, 1000.0, 100.0),
    ],
};

/// The sounds the Sampler has (made when it is prepared; see the engine's `stock::samples`).
pub const SAMPLER_SOUNDS: &[&str] = &[
    "ピアノ",
    "エレピ",
    "ストリングス",
    "ベース",
    "プラック",
    "オルガン",
];

pub static SAMPLER: BuiltinDef = BuiltinDef {
    uid: "sampler",
    name: "Sampler",
    kind: Kind::Instrument,
    params: &[
        choice("sound", "音色", SAMPLER_SOUNDS, 0),
        ms("attack", "アタック", 1.0, 5000.0, 2.0),
        ms("decay", "ディケイ", 1.0, 10_000.0, 1000.0),
        pct("sustain", "サステイン", 100.0, 100.0),
        ms("release", "リリース", 1.0, 10_000.0, 300.0),
        hz("cutoff", "カットオフ", 20.0, 20_000.0, 20_000.0),
        pct("resonance", "レゾナンス", 100.0, 0.0),
        pct("velocity", "ベロシティの効き", 100.0, 80.0),
        semis("tune", "ピッチ", 24.0),
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
    ],
};

/// The Drum Kit's kits.
pub const KITS: &[&str] = &["スタンダード", "エレクトロ"];

pub static DRUM_KIT: BuiltinDef = BuiltinDef {
    uid: "drum-kit",
    name: "Drum Kit",
    kind: Kind::Instrument,
    params: &[
        choice("kit", "キット", KITS, 0),
        db("volume", "ボリューム", -48.0, 12.0, 0.0),
        semis("tune", "ピッチ", 12.0),
        spec(
            "decay",
            "余韻",
            Unit::Percent,
            25.0,
            400.0,
            100.0,
            Curve::Log,
        ),
        pct("velocity", "ベロシティの効き", 100.0, 100.0),
        db("kick", "キック", -24.0, 12.0, 0.0),
        db("snare", "スネア", -24.0, 12.0, 0.0),
        db("hats", "ハイハット", -24.0, 12.0, 0.0),
        db("toms", "タム", -24.0, 12.0, 0.0),
        db("cymbals", "シンバル", -24.0, 12.0, 0.0),
        db("percussion", "パーカッション", -24.0, 12.0, 0.0),
    ],
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
