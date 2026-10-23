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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MIN_GRAPHIC_CONTRAST, contrast_ratio};

    #[test]
    fn an_off_switch_has_a_visible_track_and_knob_in_both_themes() {
        for theme in [Theme::Dark, Theme::Light] {
            for hovered in [false, true] {
                let face = SwitchFace::off(theme, hovered);
                assert!(face.track.is_opaque() && face.knob.is_opaque());
                for background in [
                    Surface::Paper,
                    Surface::S0,
                    Surface::S1,
                    Surface::S2,
                    Surface::S3,
                    Surface::Hover,
                    Surface::Sel,
                ] {
                    let ratio = contrast_ratio(face.track, background.color(theme));
                    assert!(
                        ratio >= MIN_GRAPHIC_CONTRAST,
                        "{theme:?} {hovered}: {background:?} {ratio}"
                    );
                }
                let knob = contrast_ratio(face.track, face.knob);
                assert!(
                    knob >= MIN_GRAPHIC_CONTRAST,
                    "{theme:?} {hovered}: knob {knob}"
                );
                let background = if theme == Theme::Dark {
                    Surface::S1
                } else {
                    Surface::Paper
                };
                println!(
                    "{theme:?}, hover={hovered}: track/background {:.4}:1, knob/track {knob:.4}:1",
                    contrast_ratio(face.track, background.color(theme))
                );
            }
        }
    }
}
