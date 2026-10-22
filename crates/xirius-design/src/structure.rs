//! 構造線（MIX-038）。フォルダーの範囲を、入れ子の段ごとに示す縦の線。
//!
//! 「線で区切らない」規則の、**ただ 1 つの例外**。メールの引用の線のように、フォルダーの中の行が
//! どの段まで入れ子になっているかを示す。区切りの線ではなく、範囲を示す印。
//!
//! 形の決まり（ここで固定し、使う側は変えられない）:
//! - 太さ [`StructureLine::THICKNESS`]（2 px）、両端は丸い（[`StructureLine::CAP`]）。
//! - 行から離し、字下げの溝（隣り合う段の間）の真ん中に置く（[`StructureLine::gutter_x`]）。
//! - 両端は、塊の角から [`StructureLine::END_INSET`] 内側で止める。角に触れない。
//! - 色は、通常は中立。hover と選択のときだけアクセント（[`LineState`]）。畳んだフォルダーでは描かない。
//!
//! `xirius-design-lint` は、細い棒（太さが小さい固定値の面）を「線」として止める。
//! 太さと位置を `StructureLine` の定数から取った物だけが通る（`docs/design-rules.md`）。

use crate::color::Color;
use crate::layout::Layout;
use crate::metrics::{Radius, Space};
use crate::palette::Palette;
use crate::theme::Theme;

/// 線の状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineState {
    /// 通常。中立の色。
    Rest,
    /// 範囲の上にポインターがあるとき。アクセント。
    Hover,
    /// そのフォルダーが選ばれているとき。アクセント（hover より強い）。
    Selected,
}

impl LineState {
    pub const ALL: [LineState; 3] = [LineState::Rest, LineState::Hover, LineState::Selected];
}

/// フォルダーの範囲を示す線。型は値を持たない（定数と色の取り出し口だけ）。
#[derive(Clone, Copy, Debug)]
pub struct StructureLine;

impl StructureLine {
    /// 太さ（px）。
    pub const THICKNESS: f32 = 2.0;
    /// 両端の丸め。太さの半分より大きいので、両端は完全な丸になる。
    pub const CAP: Radius = Radius::Xs;
    /// 線の端と、塊の上端・下端との距離（px）。
    pub const END_INSET: f32 = Space::X2.px();

    /// 入れ子の `depth` 段目のフォルダーの線の、塊の左端からの位置（px）。
    ///
    /// 行の中身は `Space::X2 + 段 × INDENT` から始まる。`depth` 段目のフォルダーの線は、
    /// その中身（`depth + 1` 段）の左の溝の真ん中に置く。行の中身とは重ならない。
    pub const fn gutter_x(depth: usize) -> f32 {
        Space::X2.px() + depth as f32 * Layout::INDENT + (Layout::INDENT - Self::THICKNESS) / 2.0
    }

    /// 状態に応じた色。
    pub fn color(state: LineState, theme: Theme) -> Color {
        let p = Palette::of(theme);
        match state {
            LineState::Rest => p.line_structure,
            LineState::Hover => p.line_structure_hover,
            LineState::Selected => p.line_structure_selected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_are_fully_round() {
        assert!(StructureLine::CAP.px() >= StructureLine::THICKNESS / 2.0);
    }

    #[test]
    fn gutter_lines_never_touch_row_content() {
        for depth in 0..6 {
            let line_left = StructureLine::gutter_x(depth);
            let line_right = line_left + StructureLine::THICKNESS;
            // この段のフォルダー自身の中身（左端 = Space::X2 + depth × INDENT）より右、
            // 中の行の中身（Space::X2 + (depth + 1) × INDENT）より左。
            let own = Space::X2.px() + depth as f32 * Layout::INDENT;
            let inner = own + Layout::INDENT;
            assert!(line_left > own && line_right < inner, "depth {depth}");
        }
    }

    #[test]
    fn neighbouring_depths_do_not_overlap() {
        let a = StructureLine::gutter_x(0) + StructureLine::THICKNESS;
        let b = StructureLine::gutter_x(1);
        assert!(b - a >= Space::X1.px());
    }

    #[test]
    fn active_states_differ_from_rest() {
        for theme in Theme::ALL {
            let rest = StructureLine::color(LineState::Rest, theme);
            assert_ne!(rest, StructureLine::color(LineState::Hover, theme));
            assert_ne!(rest, StructureLine::color(LineState::Selected, theme));
        }
    }
}
