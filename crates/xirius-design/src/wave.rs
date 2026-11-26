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
