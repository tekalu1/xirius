//! 文字ではない色。図形・印・重ねる面。
//!
//! `Accent` は、フォーカスの輪・AI の線・書き出し後の印の点。中立の面の上で 3:1 以上
//! （`tests/contrast.rs`）。文字の色には使わない（文字は [`crate::Ink`]）。
//!
//! `Overlay` は、スクロールのつまみと、画面を覆う幕。コントラストの対象外（操作の目印ではなく
//! 面の一部。つまみは hover で濃くなる）。

use crate::color::Color;
use crate::palette::Palette;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Accent {
    /// キーボードのフォーカスの輪。
    FocusRing,
    /// AI が書いた・動かしている印の線。
    AiLine,
    /// 書き出しの後に曲が変わった印の点。
    StaleDot,
}

impl Accent {
    pub const ALL: [Accent; 3] = [Accent::FocusRing, Accent::AiLine, Accent::StaleDot];

    pub fn color(self, theme: Theme) -> Color {
        let p = Palette::of(theme);
        match self {
            Accent::FocusRing => p.line_focus,
            Accent::AiLine => p.line_ai,
            Accent::StaleDot => p.alert_stale_dot,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overlay {
    ScrollThumb,
    ScrollThumbHover,
    /// ダイアログの下を覆う幕。
    Veil,
}

impl Overlay {
    pub const ALL: [Overlay; 3] = [
        Overlay::ScrollThumb,
        Overlay::ScrollThumbHover,
        Overlay::Veil,
    ];

    pub fn color(self, theme: Theme) -> Color {
        let p = Palette::of(theme);
        match self {
            Overlay::ScrollThumb => p.surface_thumb,
            Overlay::ScrollThumbHover => p.surface_thumb_hover,
            Overlay::Veil => p.surface_veil,
        }
    }
}

/// ピアノロールの鍵盤の面。白鍵は白、黒鍵は暗い青灰（どちらのテーマでも同じ）。
/// 鍵盤は実物の見た目なので、面の段（`Surface`）とは別に持つ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PianoKey {
    White,
    Black,
}

impl PianoKey {
    /// The note name's colour in `theme`, on the white key or, `lit`, on the key that is
    /// sounding or under the pointer.
    pub fn label_on(theme: Theme, lit: bool) -> Color {
        let p = Palette::of(theme);
        if lit { p.ink_on_key_lit } else { p.ink_on_key }
    }

    pub const ALL: [PianoKey; 2] = [PianoKey::White, PianoKey::Black];

    pub fn color(self, theme: Theme) -> Color {
        let p = Palette::of(theme);
        match self {
            PianoKey::White => p.surface_key_white,
            PianoKey::Black => p.surface_key_black,
        }
    }
}
