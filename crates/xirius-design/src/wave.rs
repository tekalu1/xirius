//! 波形の識別色。トラックの波形とブラウザーのプレビューにだけ使う。
use crate::palette::Palette;
use crate::{Color, Theme, TrackColor};

pub struct WaveColor;
impl WaveColor {
    pub fn for_track(track: TrackColor, theme: Theme) -> Color {
        track.content(theme)
    }
    pub fn preview(theme: Theme) -> Color {
        Palette::of(theme).ink_blue
    }
    pub fn rest(theme: Theme) -> Color {
        Palette::of(theme).ink_body
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
