//! 画面の寸法（px）。値は `tokens.json` が元で、`generated.rs` に書き出される（手で書かない）。
//! モック `daw-main.html` の値に、DAW の画面の部品の寸法を足したもの。
//!
//! 余白（[`crate::Space`]）・角丸（[`crate::Radius`]）・文字の大きさ（[`crate::FontSize`]）は別の型。
//! ここは、部品そのものの大きさ（行の高さ・列の幅・ボタンの大きさ）。
//! 線の太さは無い（面を線で区切らない）。構造線の太さだけは [`crate::StructureLine`] が持つ。

#[derive(Clone, Copy, Debug)]
pub struct Layout;

impl Layout {
    /// いちばん深い行が `deepest` の曲での、トラックの頭の幅。
    pub fn track_head_w(deepest: usize) -> f32 {
        let extra = deepest
            .min(Layout::INDENT_MAX_DEPTH)
            .saturating_sub(Layout::HEAD_ROOM_DEPTH);
        Layout::TRACK_HEAD_W + extra as f32 * Layout::INDENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Radius, StructureLine};

    // Checked when the tests compile: 札 2 つ分の高さの行に、名前の行と札の行が入り、
    // 字下げには構造線が入る。
    const _: () = {
        assert!(Layout::TRACK_ROW_H >= Layout::ROW_ICON + Layout::TOGGLE);
        assert!(Layout::FOLDER_ROW_H >= Layout::ROW_ICON);
        assert!(Layout::INDENT >= StructureLine::THICKNESS * 4.0);
    };

    #[test]
    fn the_head_widens_only_for_indents_past_its_room() {
        assert_eq!(Layout::track_head_w(0), Layout::TRACK_HEAD_W);
        assert_eq!(
            Layout::track_head_w(Layout::HEAD_ROOM_DEPTH),
            Layout::TRACK_HEAD_W
        );
        let widest = Layout::track_head_w(Layout::INDENT_MAX_DEPTH);
        assert_eq!(widest, Layout::TRACK_HEAD_W + Layout::INDENT);
        assert_eq!(Layout::track_head_w(Layout::INDENT_MAX_DEPTH + 4), widest);
    }

    #[test]
    fn block_gap_is_smaller_than_the_block_corner() {
        assert!(Layout::BLOCK_GAP < Radius::M.px());
    }
}
