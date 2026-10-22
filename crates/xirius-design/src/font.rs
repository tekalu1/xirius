//! 書体。使うのは Inter と Noto Sans JP だけ（どちらも SIL OFL 1.1）。
//!
//! Mac と Windows で同じ字形にするため、書体は同梱して読み込む（UI-002）。ただしこの crate は
//! フォントのファイルを持たない（`fonts/README.md` に、取得の手順とハッシュを書く）。
//! アプリ側は [`FONT_FILES`] の表を見て、`fonts/` から読み込む。
//!
//! 書体名の文字列は、この crate の外に書かない（`xirius-design-lint` の `raw-font`）。

/// 使える書体。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    /// 欧文・数字。
    Inter,
    /// 日本語。
    NotoSansJp,
}

impl Font {
    pub const ALL: [Font; 2] = [Font::Inter, Font::NotoSansJp];
}

/// 文字の太さ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
}

/// 数字を等幅にする OpenType の機能（`tnum`）。数値の表示は全部これを付ける。
pub const TABULAR_NUMS: &str = "tnum";

/// 書体のファイル 1 つ。
#[derive(Clone, Copy, Debug)]
pub struct FontFile {
    pub font: Font,
    pub weight: Weight,
    /// `fonts/` の下のファイル名。
    pub file_name: &'static str,
}

/// 同梱する書体のファイルの一覧（`fonts/README.md` の手順で置く）。
pub const FONT_FILES: &[FontFile] = &[
    FontFile {
        font: Font::Inter,
        weight: Weight::Regular,
        file_name: "Inter-Regular.ttf",
    },
    FontFile {
        font: Font::Inter,
        weight: Weight::Medium,
        file_name: "Inter-Medium.ttf",
    },
    FontFile {
        font: Font::Inter,
        weight: Weight::Semibold,
        file_name: "Inter-SemiBold.ttf",
    },
    FontFile {
        font: Font::NotoSansJp,
        weight: Weight::Regular,
        file_name: "NotoSansJP-Regular.ttf",
    },
    FontFile {
        font: Font::NotoSansJp,
        weight: Weight::Medium,
        file_name: "NotoSansJP-Medium.ttf",
    },
    FontFile {
        font: Font::NotoSansJp,
        weight: Weight::Semibold,
        file_name: "NotoSansJP-SemiBold.ttf",
    },
];

/// 描画に渡す family の並び（欧文が先、日本語が後）。
pub const FAMILY_STACK: [Font; 2] = [Font::Inter, Font::NotoSansJp];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_font_has_a_file_for_every_weight() {
        for font in Font::ALL {
            let n = FONT_FILES.iter().filter(|f| f.font == font).count();
            assert_eq!(n, 3, "{font:?}");
        }
    }
}
