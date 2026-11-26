//! トラックの小さな札（ミュート・ソロ・録音待ち・モニター）。
//!
//! 切れているときは中立の面（[`Toggle::off`]）。入っているときだけ色の面（[`Toggle::on`]）で、
//! 札の文字は面ごとに決めた色だけ（どれも 4.5:1 以上。`tests/it/contrast.rs`）。

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

/// An off switch's track and knob: each stays distinguishable at 3:1 (UI-317).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchFace {
    pub track: Color,
    pub knob: Color,
}

impl SwitchFace {
    pub fn off(theme: Theme, hovered: bool) -> Self {
        let p = Palette::of(theme);
        Self {
            track: if hovered {
                p.surface_switch_off_hover
            } else {
                p.surface_switch_off
            },
            knob: p.surface_key_white,
        }
    }
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

    /// ほかのトラックのソロで黙っているトラックの M の文字。面は切れているときの面（S3）のまま
    /// 塗らない。文字 1 つの状態の印なので、面との差は図形の 3:1 で見る（UI-376）。
    pub fn implied_mute(theme: Theme) -> Color {
        Palette::of(theme).ink_mute_implied
    }
}
