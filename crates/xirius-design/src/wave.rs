//! 波形の識別色。トラックの波形とブラウザーのプレビューにだけ使う。
use crate::palette::Palette;
use crate::{Color, Theme, TrackColor};

pub struct WaveColor;
impl WaveColor {
    /// Preview dots retain graphic contrast throughout their pulse, in both themes.
    pub const DOT_MIN: f32 = 0.60;
    pub const DOT_MAX: f32 = 0.90;
    pub const DOT_REST: f32 = 0.60;

    pub fn for_track(track: TrackColor, theme: Theme) -> Color {
        track.content(theme)
    }
    pub fn preview(theme: Theme) -> Color {
        Palette::of(theme).ink_blue
    }
    pub fn rest(theme: Theme) -> Color {
        Palette::of(theme).wave_rest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wave_colors_remain_visible_on_neutral_surfaces() {
        for theme in Theme::ALL {
            assert!(
                crate::contrast_ratio(WaveColor::preview(theme), crate::Surface::S1.color(theme))
                    >= 3.0
            );
            for track in TrackColor::all() {
                assert_eq!(WaveColor::for_track(track, theme), track.content(theme));
            }
        }
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    #[test]
    fn preview_dots_keep_graphic_contrast_at_every_phase() {
        for theme in Theme::ALL {
            let surface = crate::Surface::Sel.color(theme);
            for i in 0..=100 {
                let alpha = WaveColor::DOT_MIN
                    + (WaveColor::DOT_MAX - WaveColor::DOT_MIN) * i as f32 / 100.0;
                let mut dot = WaveColor::rest(theme);
                dot.a = (alpha * 255.0).round() as u8;
                let ratio = crate::contrast_ratio(dot.over(surface), surface);
                assert!(ratio >= 3.0, "{theme:?}, alpha={alpha}, contrast={ratio}");
            }
            let mut rest = WaveColor::rest(theme);
            rest.a = (WaveColor::DOT_REST * 255.0).round() as u8;
            assert!(crate::contrast_ratio(rest.over(surface), surface) >= 3.0);
        }
    }
}
