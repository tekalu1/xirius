//! Xirius のデザイントークン（色・寸法・動き・書体）。GPUI に依存しない、値だけの crate。
//!
//! 決めたデザインの規則を、使う側が破れない形にしてある。
//!
//! | 規則 | この crate での守り方 |
//! |---|---|
//! | 面は単色。グラデーションは値を表す物（メーター）だけ | グラデーションの型は [`meter`] にしか無い |
//! | 線で区切らない | 線の太さの型が無い。面の上辺の光も無い。区切りは面の明るさの段と、浮く面の外側の影（[`FloatShadow`]）だけ。例外はフォルダーの範囲を示す構造線（[`StructureLine`]）の 1 つだけ |
//! | 文字は 4.5:1 以上。薄い文字は使わない | 面と文字の組は [`Pair`] / [`Readable`]。許された組だけ作れ、薄い文字の段が無い |
//! | 塗りの上は専用の文字色 | [`Surface::Fill`] の上は [`Ink::OnFill`] だけ |
//! | クリップは中立の面。トラックの色は中身に | [`Surface::Clip`] と [`TrackColor::content`] |
//! | 書体は Inter と Noto Sans JP | [`Font`] |
//! | 動きは 120 / 240 / 560 ms。減らす設定で 0 | [`Duration`]・[`Motion`] |
//!
//! 色の値は、`Surface`・`Ink`・`Accent`・`TrackColor` などを通してだけ取り出せる
//! （パレットの欄は外から見えない）。コード側に生の色を書くことは、`xirius-design-lint` が止める。

#[macro_use]
#[rustfmt::skip]
mod generated;

mod accent;
mod category;
mod color;
mod elevation;
mod font;
mod layout;
pub mod meter;
mod metrics;
mod motion;
mod pair;
mod palette;
mod structure;
mod theme;
mod toggle;
mod track;
mod wave;

pub use accent::{Accent, Overlay, PianoKey};
pub use category::Category;
pub use color::{Color, MIN_GRAPHIC_CONTRAST, MIN_TEXT_CONTRAST, contrast_ratio};
pub use elevation::FloatShadow;
pub use font::{FAMILY_STACK, FONT_FILES, Font, FontFile, TABULAR_NUMS, Weight};
pub use generated::AI_LOOP_MS;
pub use layout::Layout;
pub use meter::{GradientStop, LevelMeter, ValueGradient};
pub use metrics::{FontSize, LineHeight, Radius, Space};
pub use motion::{Duration, Easing, Motion};
pub use pair::{
    ALLOWED, Ink, InkKind, Pair, Readable, ReadableOn, Resolved, Surface, SurfaceKind, ink, surface,
};
pub use palette::token_names;
pub use structure::{LineState, StructureLine};
pub use theme::Theme;
pub use toggle::{SwitchFace, Toggle, ToggleFace};
pub use track::{SectionFace, TrackColor};
pub use wave::WaveColor;
