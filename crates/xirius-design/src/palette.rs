//! 色のトークンの実値（ダーク・ライト）。値は `tokens.json` が元で、`generated.rs` に書き出される
//! （手で書かない。直すのは `tokens.json`）。
//!
//! 値の出どころ:
//! - 面・文字・塗り・フォーカスの線: Pleiad の `web/tokens.css` を元にした。
//! - `surface-clip`・メーター・AI・トラックの色: モック `daw-main.html`（3 版）の DAW 用の族。
//!
//! Pleiad から値を変えたもの（コントラスト比 4.5:1 を満たすため）:
//! - ダークの `fill_primary_hover`: `#6573d6` → `#4f5ec8`（`ink_on_fill` の上で 3.89 → 5.14）。
//!   ダークでは押すと「明るく」ではなく「沈む」。
//! - 文字に使う赤（`ink_alert`）: ダーク `#ef5a50` → `#ff7a70`、ライト `#d9443a` → `#a92a22`。
//! - 文字に使う AI の紫（`ink_ai`）: ライトの `#7d4fe0` → `#6a3cc8`。
//! - ライトのメーター（溝の上で 3:1 以上）: 緑 `#2fa56d` → `#1f8a56`、黄 `#c9a21e` → `#947500`。
//!
//! 外す・足さないと決めたもの:
//! - `ink-weak` / `line-faint` / `ink-code-*`: 薄い文字は使わない（rev3、UI-092）。
//! - `alert-tint` / `ai-tint`: 薄い色の面の上に文字を置かない（rev3・rev4、UI-108）。
//!
//! フィールドは外へ出さない。使う側は [`crate::Surface`]・[`crate::Pair`]・[`crate::Accent`]・
//! [`crate::TrackColor`] を通してだけ色を取る（文字 × 面の組を型で絞るため）。

use crate::color::Color;
use crate::theme::Theme;

macro_rules! palette {
    ($( $(#[$m:meta])* $name:ident : $dark:expr, $light:expr; )*) => {
        /// 1 つのテーマの全トークン。
        #[derive(Clone, Copy, Debug)]
        pub(crate) struct Palette {
            $( $(#[$m])* pub(crate) $name: Color, )*
        }

        impl Palette {
            const DARK: Palette = Palette { $( $name: $dark, )* };
            const LIGHT: Palette = Palette { $( $name: $light, )* };

            /// 検査用: （トークン名, 色）の並び。
            pub(crate) fn entries(&self) -> Vec<(&'static str, Color)> {
                vec![ $( (stringify!($name), self.$name), )* ]
            }
        }
    };
}

const fn c(rgb: u32) -> Color {
    Color::hex(rgb)
}
const fn ca(rgb: u32, a: u8) -> Color {
    Color::hex_alpha(rgb, a)
}

// 色の一覧（値）は `tokens.json` から作った `generated.rs` にある。
palette_tokens!(palette);
impl Palette {
    pub(crate) const fn of(theme: Theme) -> &'static Palette {
        match theme {
            Theme::Dark => &Self::DARK,
            Theme::Light => &Self::LIGHT,
        }
    }

    pub(crate) fn track(&self, index: usize) -> Color {
        [
            self.track_0,
            self.track_1,
            self.track_2,
            self.track_3,
            self.track_4,
            self.track_5,
            self.track_6,
            self.track_7,
            self.track_8,
            self.track_9,
            self.track_10,
            self.track_11,
            self.track_12,
            self.track_13,
            self.track_14,
            self.track_15,
        ][index]
    }
}

/// トークン名の検査用（外から読める最小の口）。
pub fn token_names() -> Vec<&'static str> {
    Palette::DARK
        .entries()
        .into_iter()
        .map(|(n, _)| n)
        .collect()
}
