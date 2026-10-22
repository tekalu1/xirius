//! メーター（値を表す物）。**色のグラデーションを作れるのは、このモジュールだけ。**
//!
//! 面・ヘッダー・波形・クリップのグラデーションは、型に無い（UI-090・UI-105）。
//! `tests/api_surface.rs` が、`gradient` という語がこのファイルの外に出ないことを確かめる。

use crate::color::Color;
use crate::palette::Palette;
use crate::theme::Theme;

/// グラデーションの 1 点。`at` は 0.0（下端）〜 1.0（上端）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub at: f32,
    pub color: Color,
}

/// 値を表す帯の色の変化。メーターの型からしか作れない。
#[derive(Clone, Debug, PartialEq)]
pub struct ValueGradient {
    stops: [GradientStop; 3],
}

impl ValueGradient {
    pub fn stops(&self) -> &[GradientStop] {
        &self.stops
    }

    /// 0.0〜1.0 の位置の色（段の間は線形に混ぜる）。
    pub fn color_at(&self, at: f32) -> Color {
        let at = at.clamp(0.0, 1.0);
        let s = &self.stops;
        let (a, b) = if at <= s[1].at {
            (s[0], s[1])
        } else {
            (s[1], s[2])
        };
        let t = if b.at > a.at {
            (at - a.at) / (b.at - a.at)
        } else {
            0.0
        };
        let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
        Color {
            r: mix(a.color.r, b.color.r),
            g: mix(a.color.g, b.color.g),
            b: mix(a.color.b, b.color.b),
            a: 255,
        }
    }
}

/// レベルメーターの色づけ。緑 → 黄 → 赤。
#[derive(Clone, Copy, Debug)]
pub struct LevelMeter {
    /// 黄色が始まる位置（0.0〜1.0）。
    pub hot_at: f32,
}

impl Default for LevelMeter {
    /// 0 dBFS を 1.0 とした位置で、-18 dB 付近から黄に向かう値。
    fn default() -> Self {
        LevelMeter { hot_at: 0.70 }
    }
}

impl LevelMeter {
    /// 区切りの無い、値を表す帯。
    pub fn gradient(self, theme: Theme) -> ValueGradient {
        let p = Palette::of(theme);
        ValueGradient {
            stops: [
                GradientStop {
                    at: 0.0,
                    color: p.meter_ok,
                },
                GradientStop {
                    at: self.hot_at,
                    color: p.meter_hot,
                },
                GradientStop {
                    at: 1.0,
                    color: p.meter_over,
                },
            ],
        }
    }

    /// 溝（帯の下地）。
    pub fn well(theme: Theme) -> Color {
        Palette::of(theme).meter_well
    }

    /// 振り切れたとき（0 dB を超えたとき）に小さく光る点の色。
    pub fn clip_light(theme: Theme) -> Color {
        Palette::of(theme).meter_over
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_the_meter_colors() {
        for theme in Theme::ALL {
            let g = LevelMeter::default().gradient(theme);
            assert_eq!(g.color_at(0.0), g.stops()[0].color);
            assert_eq!(g.color_at(1.0), g.stops()[2].color);
            assert_eq!(g.color_at(0.7), g.stops()[1].color);
        }
    }
}
