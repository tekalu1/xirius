//! 浮く面の影。
//!
//! 面の区切りは、面の明るさの段の差（[`crate::Surface`]）と、浮く面の外側の影
//! （[`FloatShadow`]）だけで作る（UI-007）。線も、面の上辺の光も無い。
//! 影は全周に広がる柔らかい外側の影で、片側にだけ出る影・光は作れない
//! （`xirius-design-lint` の `one-sided-edge` が止める）。

use crate::color::Color;
use crate::palette::Palette;
use crate::theme::Theme;

/// 浮く面（ポップオーバー・メニュー・ダイアログ）の影。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatShadow {
    pub color: Color,
    pub offset_y: f32,
    pub blur: f32,
}

impl FloatShadow {
    /// 近い影と遠い影の 2 枚（遠い方から重ねる）。
    pub fn layers(theme: Theme) -> [FloatShadow; 2] {
        let p = Palette::of(theme);
        [
            FloatShadow {
                color: p.surface_shadow,
                offset_y: 12.0,
                blur: 32.0,
            },
            FloatShadow {
                color: p.surface_shadow_near,
                offset_y: 2.0,
                blur: 6.0,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_shadow_is_far_first_then_near_and_translucent() {
        for theme in Theme::ALL {
            let [far, near] = FloatShadow::layers(theme);
            assert!(far.blur > near.blur && far.offset_y > near.offset_y);
            assert!(far.color.a > 0 && far.color.a < 255);
            assert!(near.color.a > 0 && near.color.a < 255);
        }
    }
}
