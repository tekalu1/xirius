//! 寸法のトークン（余白・角丸・文字の大きさ・行間）。値は `tokens.json` が元で、`generated.rs` に
//! 書き出される（手で書かない）。値は Pleiad の刻み（UI-049）に、
//! DAW 用の小さい段（`Radius::Xs`・`Radius::Clip`、`FontSize::Micro`、`LineHeight::Dense`）を足したもの。
//!
//! 線の太さの型は無い。面を線で区切らない（UI-051）。面の上辺の光も無い（UI-007）。

/// 余白。4 の倍数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    X1,
    X2,
    X3,
    X4,
    X5,
    X6,
}

impl Space {
    pub const ALL: [Space; 6] = [
        Space::X1,
        Space::X2,
        Space::X3,
        Space::X4,
        Space::X5,
        Space::X6,
    ];
}

/// 角丸。全部が曲線で、1 辺だけ角を変える型は無い。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Radius {
    /// ノブの目盛り・小さな点。
    Xs,
    /// クリップ・ノート。
    Clip,
    S,
    M,
    L,
    Pill,
}

impl Radius {
    pub const ALL: [Radius; 6] = [
        Radius::Xs,
        Radius::Clip,
        Radius::S,
        Radius::M,
        Radius::L,
        Radius::Pill,
    ];
}

/// 文字の大きさ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontSize {
    Micro,
    Caption,
    Small,
    Body,
    Lead,
    Title,
    /// Numeric readout in the approved audio settings pane.
    Readout,
}

impl FontSize {
    pub const ALL: [FontSize; 7] = [
        FontSize::Micro,
        FontSize::Caption,
        FontSize::Small,
        FontSize::Body,
        FontSize::Lead,
        FontSize::Title,
        FontSize::Readout,
    ];
}

/// 行間（文字の大きさに対する倍率）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineHeight {
    /// 文章。
    Body,
    /// 詰めた表示（トラックの頭・ミキサー・数値）。
    Dense,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_are_multiples_of_four() {
        for s in Space::ALL {
            assert_eq!(s.px() % 4.0, 0.0, "{s:?}");
        }
    }

    #[test]
    fn scales_are_increasing() {
        assert!(Space::ALL.windows(2).all(|w| w[0].px() < w[1].px()));
        assert!(Radius::ALL.windows(2).all(|w| w[0].px() < w[1].px()));
        assert!(FontSize::ALL.windows(2).all(|w| w[0].px() < w[1].px()));
    }
}
