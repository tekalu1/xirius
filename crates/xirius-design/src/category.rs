//! 操作の分類の 5 色（SET-007）。設定のショートカットの、キーボードの点と凡例だけに使う。
//!
//! トラックの色（[`crate::TrackColor`]）とは別の族にしてある（曲の中のトラックと取り違えない）。
//! 中立の面の上で 3:1 以上（`tests/contrast.rs`）。文字の色には使わない。

use crate::color::Color;
use crate::palette::Palette;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    /// ツールの切り替え。
    Tool,
    /// 再生・録音。
    Play,
    /// ノートやクリップの編集。
    Edit,
    /// トラック。
    Track,
    /// 表示・画面。
    View,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Tool,
        Category::Play,
        Category::Edit,
        Category::Track,
        Category::View,
    ];

    pub fn color(self, theme: Theme) -> Color {
        let p = Palette::of(theme);
        match self {
            Category::Tool => p.line_category_tool,
            Category::Play => p.line_category_play,
            Category::Edit => p.line_category_edit,
            Category::Track => p.line_category_track,
            Category::View => p.line_category_view,
        }
    }
}
