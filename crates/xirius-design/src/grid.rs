//! 時間のグリッドの線（UI-339）。アレンジとピアノロールの、小節・拍・分割・細かい線。
//!
//! 「線で区切らない」規則の、構造線（[`crate::StructureLine`]）に次ぐ 2 つ目の例外。
//! 区切りではなく、時間の目盛り。段（[`GridTier`]）・太さ・色・出し始めのしきい値をここで固定し、
//! 使う側は変えられない。
//!
//! - 太さは小節だけ [`GridLine::BAR_THICKNESS`]（2 px）、ほかは [`GridLine::THICKNESS`]（1 px）。
//!   置く位置は [`GridLine::left`]（2 px の線は時刻の位置を真ん中にする）。
//! - 色は文字の色を薄く重ねた `line_grid_*`。面（地・hover・選択・黒鍵の行）が変わっても段の順が崩れない。
//! - 線の間が [`GridTier::min_px`] より詰まる段は描かない。出始めは [`GridLine::fade`] で薄く入れる。
//!
//! `xirius-design-lint` は、`line_grid_*` の色（画面の `grid`）を、幅と位置を `GridLine` から取らずに
//! 塗る物を `line` として止める（`docs/design-rules.md`）。

use crate::color::Color;
use crate::palette::Palette;
use crate::theme::Theme;

/// 線の段。強い順。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GridTier {
    /// 小節の頭。
    Bar,
    /// 拍子の拍（6/8・9/8・12/8 は付点 4 分）。
    Beat,
    /// 拍の分割（1/2。複合拍子か 3 連のグリッドでは 1/3）。
    Sub,
    /// 分割より細かいグリッドの値と、フレーム。
    Fine,
}

impl GridTier {
    pub const ALL: [GridTier; 4] = [GridTier::Bar, GridTier::Beat, GridTier::Sub, GridTier::Fine];

    /// 強い順の番号（小節が 0）。
    pub const fn index(self) -> usize {
        self as usize
    }

    /// この段の線を出す、線の間の最小（px）。小節の線はいつも出す。
    pub const fn min_px(self) -> f32 {
        match self {
            GridTier::Bar => 0.0,
            GridTier::Beat => GridLine::MIN_BEAT_PX,
            GridTier::Sub | GridTier::Fine => GridLine::MIN_FINE_PX,
        }
    }
}

/// 時間のグリッドの線。型は値を持たない（定数と色の取り出し口だけ）。
#[derive(Clone, Copy, Debug)]
pub struct GridLine;

impl GridLine {
    /// 小節の線の太さ（px）。
    pub const BAR_THICKNESS: f32 = 2.0;
    /// 小節のほかの線の太さ（px）。
    pub const THICKNESS: f32 = 1.0;
    /// 拍の線を出す、拍の間の最小（px）。足りなければ拍の線は全部消す（間引かない）。
    pub const MIN_BEAT_PX: f32 = 6.0;
    /// 分割と細かい線を出す、線の間の最小（px）。自動のスナップの最小も同じ。
    pub const MIN_FINE_PX: f32 = 8.0;
    /// 出始めの濃さ（本来の濃さに対する割合）。
    pub const FADE_FROM: f32 = 0.4;

    /// 段の太さ（px）。
    pub const fn width(tier: GridTier) -> f32 {
        match tier {
            GridTier::Bar => Self::BAR_THICKNESS,
            _ => Self::THICKNESS,
        }
    }

    /// 時刻の位置 `x` に置く線の左端（px、画素に揃える）。2 px の線は `x` を真ん中にする。
    pub fn left(x: f32, tier: GridTier) -> f32 {
        x.round() - (Self::width(tier) / 2.0).floor()
    }

    /// 線の間が `px` のときの濃さの割合（0〜1）。しきい値未満は 0（描かない）、
    /// しきい値で [`Self::FADE_FROM`]、しきい値の 2 倍で 1。
    pub fn fade(tier: GridTier, px: f32) -> f32 {
        let min = tier.min_px();
        if min <= 0.0 {
            return 1.0;
        }
        if px.is_nan() || px < min {
            return 0.0;
        }
        (Self::FADE_FROM + (1.0 - Self::FADE_FROM) * (px - min) / min).min(1.0)
    }

    /// 段の色（透過のある色。面の上に重ねる）。
    pub fn color(tier: GridTier, theme: Theme) -> Color {
        let p = Palette::of(theme);
        match tier {
            GridTier::Bar => p.line_grid_bar,
            GridTier::Beat => p.line_grid_beat,
            GridTier::Sub => p.line_grid_sub,
            GridTier::Fine => p.line_grid_fine,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bar_line_is_two_pixels() {
        assert_eq!(GridLine::width(GridTier::Bar), 2.0);
        for tier in [GridTier::Beat, GridTier::Sub, GridTier::Fine] {
            assert_eq!(GridLine::width(tier), 1.0);
        }
    }

    #[test]
    fn a_two_pixel_line_is_centred_on_its_time() {
        assert_eq!(GridLine::left(100.4, GridTier::Bar), 99.0);
        assert_eq!(GridLine::left(100.4, GridTier::Beat), 100.0);
    }

    #[test]
    fn lines_fade_in_from_their_threshold() {
        assert_eq!(GridLine::fade(GridTier::Bar, 0.5), 1.0);
        assert_eq!(GridLine::fade(GridTier::Beat, 5.9), 0.0);
        assert!((GridLine::fade(GridTier::Beat, 6.0) - 0.4).abs() < 1e-6);
        assert!((GridLine::fade(GridTier::Beat, 9.0) - 0.7).abs() < 1e-6);
        assert_eq!(GridLine::fade(GridTier::Beat, 12.0), 1.0);
        assert_eq!(GridLine::fade(GridTier::Fine, 7.9), 0.0);
        assert!((GridLine::fade(GridTier::Fine, 8.0) - 0.4).abs() < 1e-6);
        assert_eq!(GridLine::fade(GridTier::Sub, 100.0), 1.0);
    }

    #[test]
    fn tiers_get_lighter_in_order() {
        for theme in Theme::ALL {
            let alpha = GridTier::ALL.map(|t| GridLine::color(t, theme).to_f32()[3]);
            assert!(alpha.windows(2).all(|p| p[0] > p[1]), "{theme:?} {alpha:?}");
        }
    }
}
