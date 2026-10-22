//! 「文字 × 面」の組。許された組だけが作れる。
//!
//! - 面は [`Surface`]、文字（アイコンを含む）は [`Ink`]。色の値は、組を通してだけ取り出せる。
//! - 許された組の一覧は、下の `allowed_pairs!` の 1 か所だけ。型（`ReadableOn`）と
//!   実行時の表（[`ALLOWED`]）と、`tests/contrast.rs` の検査が、全部この一覧から出る。
//! - 薄い文字（`ink-weak` の類い）は、`Ink` に作らない。
//!
//! 型で縛る（作れない組は、コンパイルが通らない）:
//!
//! ```
//! use xirius_design::{ink, surface, Pair, Theme};
//! let pair = Pair::<surface::S1, ink::Body>::new();
//! let _ = pair.resolve(Theme::Dark);
//! ```
//!
//! ```compile_fail
//! use xirius_design::{ink, surface, Pair};
//! // 塗りの上に本文の色は置けない（塗りの上は `OnFill` だけ）。
//! let _ = Pair::<surface::Fill, ink::Body>::new();
//! ```
//!
//! ```compile_fail
//! use xirius_design::{ink, surface, Pair};
//! // 4.5:1 に届かない組は、一覧に無い。
//! let _ = Pair::<surface::Clip, ink::Mark>::new();
//! ```

use std::marker::PhantomData;

use crate::color::Color;
use crate::palette::Palette;
use crate::theme::Theme;

/// 文字を置いてよい面。色は [`Surface::color`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Paper,
    S0,
    S1,
    S2,
    S3,
    Hover,
    Sel,
    /// クリップの面（中立）。
    Clip,
    /// 選んだクリップの面。枠の代わりに明るさの差で選択を示す。
    ClipSel,
    /// 主要操作のボタンの塗り。
    Fill,
    FillHover,
    /// 閉じる・録音待ちなど、取り消せない・危険な操作の塗り。上の文字は `OnFill` だけ。
    Danger,
    DangerHover,
}

/// 文字・アイコンの色の段。`Weak` は無い。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ink {
    Strong,
    Body,
    Blue,
    Mark,
    Alert,
    Ai,
    OnFill,
}

impl Surface {
    pub const ALL: [Surface; 13] = [
        Surface::Paper,
        Surface::S0,
        Surface::S1,
        Surface::S2,
        Surface::S3,
        Surface::Hover,
        Surface::Sel,
        Surface::Clip,
        Surface::ClipSel,
        Surface::Fill,
        Surface::FillHover,
        Surface::Danger,
        Surface::DangerHover,
    ];

    /// 面の色。
    pub fn color(self, theme: Theme) -> Color {
        let p = Palette::of(theme);
        match self {
            Surface::Paper => p.surface_paper,
            Surface::S0 => p.surface_0,
            Surface::S1 => p.surface_1,
            Surface::S2 => p.surface_2,
            Surface::S3 => p.surface_3,
            Surface::Hover => p.surface_hover,
            Surface::Sel => p.surface_sel,
            Surface::Clip => p.surface_clip,
            Surface::ClipSel => p.surface_clip_sel,
            Surface::Fill => p.fill_primary,
            Surface::FillHover => p.fill_primary_hover,
            Surface::Danger => p.fill_danger,
            Surface::DangerHover => p.fill_danger_hover,
        }
    }

    /// この面の上の既定の文字（本文）。塗りの上は `OnFill`。どの面でも必ず作れる。
    pub const fn text(self) -> Readable {
        let ink = match self {
            Surface::Fill | Surface::FillHover | Surface::Danger | Surface::DangerHover => {
                Ink::OnFill
            }
            _ => Ink::Body,
        };
        Readable { surface: self, ink }
    }

    /// 強い文字（名前・見出し）。塗りの上は `OnFill`。どの面でも必ず作れる。
    pub const fn strong_text(self) -> Readable {
        let ink = match self {
            Surface::Fill | Surface::FillHover | Surface::Danger | Surface::DangerHover => {
                Ink::OnFill
            }
            _ => Ink::Strong,
        };
        Readable { surface: self, ink }
    }

    /// この面の上に置ける文字の一覧。
    pub fn readable_inks(self) -> Vec<Ink> {
        ALLOWED
            .iter()
            .filter(|(s, _)| *s == self)
            .map(|(_, i)| *i)
            .collect()
    }
}

impl Ink {
    pub const ALL: [Ink; 7] = [
        Ink::Strong,
        Ink::Body,
        Ink::Blue,
        Ink::Mark,
        Ink::Alert,
        Ink::Ai,
        Ink::OnFill,
    ];

    /// 文字の色。外へは出さず、[`Readable`] を通して取る。
    fn color(self, theme: Theme) -> Color {
        let p = Palette::of(theme);
        match self {
            Ink::Strong => p.ink_strong,
            Ink::Body => p.ink_body,
            Ink::Blue => p.ink_blue,
            Ink::Mark => p.ink_mark,
            Ink::Alert => p.ink_alert,
            Ink::Ai => p.ink_ai,
            Ink::OnFill => p.ink_on_fill,
        }
    }

    /// `surface` の上に置けるなら、その組。
    pub fn on(self, surface: Surface) -> Option<Readable> {
        Readable::new(surface, self)
    }
}

/// 許された組（実行時の形）。`new` は表を引くので、許されない組は `None`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Readable {
    surface: Surface,
    ink: Ink,
}

/// 文字と面の色（描画に渡す形）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub surface: Color,
    pub ink: Color,
}

impl Readable {
    pub fn new(surface: Surface, ink: Ink) -> Option<Readable> {
        ALLOWED
            .contains(&(surface, ink))
            .then_some(Readable { surface, ink })
    }

    pub const fn surface(self) -> Surface {
        self.surface
    }

    pub const fn ink(self) -> Ink {
        self.ink
    }

    pub fn resolve(self, theme: Theme) -> Resolved {
        Resolved {
            surface: self.surface.color(theme),
            ink: self.ink.color(theme),
        }
    }

    /// 文字・アイコンの色だけ（面は親が持っているとき）。
    pub fn ink_color(self, theme: Theme) -> Color {
        self.ink.color(theme)
    }
}

mod sealed {
    pub trait Sealed {}
}

/// 面の型。
pub trait SurfaceKind: sealed::Sealed {
    const SURFACE: Surface;
}

/// 文字の型。
pub trait InkKind: sealed::Sealed {
    const INK: Ink;
}

/// `Self` の文字を `S` の面の上に置いてよい（`allowed_pairs!` が実装する）。
pub trait ReadableOn<S: SurfaceKind>: InkKind {}

/// 型で縛った組。`Pair::<surface::S1, ink::Body>::new()` は通り、許されない組は作れない。
#[derive(Clone, Copy, Debug, Default)]
pub struct Pair<S: SurfaceKind, I: ReadableOn<S>>(PhantomData<(S, I)>);

impl<S: SurfaceKind, I: ReadableOn<S>> Pair<S, I> {
    pub const fn new() -> Self {
        Pair(PhantomData)
    }

    pub const fn readable(self) -> Readable {
        Readable {
            surface: S::SURFACE,
            ink: I::INK,
        }
    }

    pub fn resolve(self, theme: Theme) -> Resolved {
        self.readable().resolve(theme)
    }
}

impl<S: SurfaceKind, I: ReadableOn<S>> From<Pair<S, I>> for Readable {
    fn from(pair: Pair<S, I>) -> Readable {
        pair.readable()
    }
}

macro_rules! kinds {
    (surface: $($s:ident),*; ink: $($i:ident),*) => {
        /// 面の型。
        pub mod surface {
            use super::{sealed::Sealed, Surface, SurfaceKind};
            $(
                #[derive(Clone, Copy, Debug)]
                pub struct $s;
                impl Sealed for $s {}
                impl SurfaceKind for $s { const SURFACE: Surface = Surface::$s; }
            )*
        }
        /// 文字の型。
        pub mod ink {
            use super::{sealed::Sealed, Ink, InkKind};
            $(
                #[derive(Clone, Copy, Debug)]
                pub struct $i;
                impl Sealed for $i {}
                impl InkKind for $i { const INK: Ink = Ink::$i; }
            )*
        }
    };
}

kinds! {
    surface: Paper, S0, S1, S2, S3, Hover, Sel, Clip, ClipSel, Fill, FillHover, Danger, DangerHover;
    ink: Strong, Body, Blue, Mark, Alert, Ai, OnFill
}

/// 許された組の一覧。ここが唯一の定義で、型・表・検査が全部ここから出る。
/// 組を足すと `tests/contrast.rs` が両テーマで 4.5:1 を確かめる。
macro_rules! allowed_pairs {
    ($( $s:ident : [ $($i:ident),* $(,)? ] ; )*) => {
        $( $( impl ReadableOn<surface::$s> for ink::$i {} )* )*

        /// 許された（面, 文字）の組。
        pub const ALLOWED: &[(Surface, Ink)] = &[
            $( $( (Surface::$s, Ink::$i), )* )*
        ];
    };
}

allowed_pairs! {
    Paper: [Strong, Body, Blue, Mark, Alert, Ai];
    S0: [Strong, Body, Blue, Mark, Alert, Ai];
    S1: [Strong, Body, Blue, Mark, Alert, Ai];
    S2: [Strong, Body, Blue, Mark, Alert, Ai];
    S3: [Strong, Body, Blue, Mark, Alert, Ai];
    Hover: [Strong, Body, Blue, Mark, Alert, Ai];
    Sel: [Strong, Body, Blue, Mark, Alert, Ai];
    // ダークの Mark は、クリップの面の上で 4.32:1（足りない）。
    Clip: [Strong, Body, Blue, Alert, Ai];
    // 明るくした分、Blue・Alert・Ai はダークで 4.5:1 に届かない。クリップの名前は Body。
    ClipSel: [Strong, Body];
    Fill: [OnFill];
    FillHover: [OnFill];
    Danger: [OnFill];
    DangerHover: [OnFill];
}
