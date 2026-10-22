//! トラックの 16 色。
//!
//! 使い方の規則（rev3・rev4）:
//! - クリップ（イベント）の面は中立（[`crate::Surface::Clip`]）のまま。トラックの色は、
//!   中身（MIDI のノート・波形・ドラムの打点）と、アイコン・小さな点にだけ使う（[`TrackColor::content`]）。
//! - アレンジの区間は、物ごとトラックの色で塗る（[`TrackColor::section`]）。その上の文字は
//!   [`SectionFace`] が持つ色だけ（4.5:1 以上は検査で確かめる）。
//! - 1 辺だけに色を付けない。

use crate::color::Color;
use crate::palette::Palette;
use crate::theme::Theme;

/// トラックの色の番号（0〜15）。範囲外の番号は 16 で割った余りにする。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackColor(u8);

/// 色で塗った区間の面と、その上の文字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionFace {
    pub fill: Color,
    pub label: Color,
}

impl TrackColor {
    pub const COUNT: usize = 16;

    pub const fn new(index: usize) -> Self {
        TrackColor((index % Self::COUNT) as u8)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub fn all() -> impl Iterator<Item = TrackColor> {
        (0..Self::COUNT).map(TrackColor::new)
    }

    /// 中身（ノート・波形・打点）・アイコン・点の色。中立の面の上で 3:1 以上。
    pub fn content(self, theme: Theme) -> Color {
        Palette::of(theme).track(self.index())
    }

    /// アレンジの区間のように、物ごと塗るときの面と文字。
    pub fn section(self, theme: Theme) -> SectionFace {
        SectionFace {
            fill: self.content(theme),
            label: Palette::of(theme).ink_on_track,
        }
    }
}
