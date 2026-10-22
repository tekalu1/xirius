//! トラックの小さな札（ミュート・ソロ・録音待ち・モニター）。
//!
//! 切れているときは中立の面（[`Toggle::off`]）。入っているときだけ色の面（[`Toggle::on`]）で、
//! 札の文字は面ごとに決めた色だけ（どれも 4.5:1 以上。`tests/contrast.rs`）。

use crate::color::Color;
use crate::pair::{Readable, Surface};
use crate::palette::Palette;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Toggle {
    Mute,
    Solo,
    Arm,
    Monitor,
}

/// 札の面と、その上の文字・アイコン。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToggleFace {
    pub fill: Color,
    pub label: Color,
}

impl Toggle {
    pub const GLOW: (f32, f32) = (5.0, 0.08);
    pub const ALL: [Toggle; 4] = [Toggle::Mute, Toggle::Solo, Toggle::Arm, Toggle::Monitor];

    /// 入っているときの面と文字。
    pub fn on(self, theme: Theme) -> ToggleFace {
        let p = Palette::of(theme);
        match self {
            Toggle::Mute => ToggleFace {
                fill: p.fill_toggle_mute,
                label: p.ink_toggle_mute,
            },
            Toggle::Solo => ToggleFace {
                fill: p.fill_toggle_solo,
                label: p.ink_toggle_solo,
            },
            Toggle::Arm => ToggleFace {
                fill: p.fill_danger,
                label: p.ink_on_fill,
            },
            Toggle::Monitor => ToggleFace {
                fill: p.fill_primary,
                label: p.ink_on_fill,
            },
        }
    }

    /// 切れているときの面と文字（4 つとも同じ）。
    pub const fn off() -> Readable {
        Surface::S3.text()
    }
}
