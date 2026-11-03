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
//! - チャンネルエディターとストリップの経路（UI-321）も同じ線で描く。直線は縦か横、曲がりは
//!   半径 [`StructureLine::RADIUS`] 以下の四分円だけ（[`LinePiece`]）。
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

/// 曲がる構造線の 1 区間（UI-321）。直線は縦か横だけ、曲がりは四分円だけ。
///
/// 座標は使う側の面の中の px（右・下が正）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinePiece {
    /// `from` から `to` への縦か横の直線。
    Straight { from: (f32, f32), to: (f32, f32) },
    /// 中心 `center`・半径 `radius` の四分円。`from` から `to` へ回る。
    Quarter {
        from: (f32, f32),
        to: (f32, f32),
        center: (f32, f32),
        radius: f32,
    },
}

impl LinePiece {
    pub fn from(self) -> (f32, f32) {
        match self {
            LinePiece::Straight { from, .. } | LinePiece::Quarter { from, .. } => from,
        }
    }

    pub fn to(self) -> (f32, f32) {
        match self {
            LinePiece::Straight { to, .. } | LinePiece::Quarter { to, .. } => to,
        }
    }

    /// 四分円の回る向きが、画面の上で時計回りか（y が下向きの座標で）。
    pub fn clockwise(self) -> bool {
        match self {
            LinePiece::Straight { .. } => false,
            LinePiece::Quarter {
                from, to, center, ..
            } => {
                let (a, b) = (
                    (from.0 - center.0, from.1 - center.1),
                    (to.0 - center.0, to.1 - center.1),
                );
                a.0 * b.1 - a.1 * b.0 > 0.0
            }
        }
    }

    /// 決まりの形か: 直線は縦か横、曲がりは軸に沿った両端を持つ 90° の円弧で、半径は
    /// [`StructureLine::RADIUS`] 以下。
    pub fn is_lawful(self) -> bool {
        const E: f32 = 0.01;
        match self {
            LinePiece::Straight { from, to } => {
                (from.0 - to.0).abs() < E || (from.1 - to.1).abs() < E
            }
            LinePiece::Quarter {
                from,
                to,
                center,
                radius,
            } => {
                let a = (from.0 - center.0, from.1 - center.1);
                let b = (to.0 - center.0, to.1 - center.1);
                let on_axis = |v: (f32, f32)| v.0.abs() < E || v.1.abs() < E;
                let len = |v: (f32, f32)| (v.0 * v.0 + v.1 * v.1).sqrt();
                radius > 0.0
                    && radius <= StructureLine::RADIUS.px() + E
                    && (len(a) - radius).abs() < E
                    && (len(b) - radius).abs() < E
                    && on_axis(a)
                    && on_axis(b)
                    && (a.0 * b.0 + a.1 * b.1).abs() < E
            }
        }
    }
}

impl StructureLine {
    /// 曲がりの半径。曲がる所はすべてこの四分円（間が詰まっていれば、四分円のまま小さくする）。
    pub const RADIUS: Radius = Radius::S;
    /// ストリップの行（22）の中で、線の端と部品の縁との距離（px）。行の間が詰まっているので、
    /// そこだけ [`StructureLine::END_INSET`] より近い。
    pub const TIGHT_INSET: f32 = Space::X1.px();

    /// 部品の縁 `edge` から、線の端を止める位置までの距離。`tight` はストリップの行の中。
    pub const fn inset(tight: bool) -> f32 {
        if tight {
            Self::TIGHT_INSET
        } else {
            Self::END_INSET
        }
    }

    /// `(x1, y1)` から `(x2, y2)` へ下る線: 縦 → 四分円 → 横（高さ `ym`）→ 四分円 → 縦。
    /// x が同じなら、まっすぐの縦の線だけ。分かれる所・合わさる所の「かっこ」の 1 本。
    pub fn elbow(from: (f32, f32), to: (f32, f32), ym: f32) -> Vec<LinePiece> {
        let ((x1, y1), (x2, y2)) = (from, to);
        if (x2 - x1).abs() < 1.0 {
            return vec![LinePiece::Straight { from, to: (x1, y2) }];
        }
        let dir = (x2 - x1).signum();
        let r = Self::RADIUS
            .px()
            .min((x2 - x1).abs() / 2.0)
            .min(ym - y1)
            .min(y2 - ym)
            .max(0.0);
        let mut out = Vec::new();
        let mut push = |p: LinePiece| {
            if p.from() != p.to() {
                out.push(p);
            }
        };
        push(LinePiece::Straight {
            from,
            to: (x1, ym - r),
        });
        if r > 0.0 {
            push(LinePiece::Quarter {
                from: (x1, ym - r),
                to: (x1 + dir * r, ym),
                center: (x1 + dir * r, ym - r),
                radius: r,
            });
        }
        push(LinePiece::Straight {
            from: (x1 + dir * r, ym),
            to: (x2 - dir * r, ym),
        });
        if r > 0.0 {
            push(LinePiece::Quarter {
                from: (x2 - dir * r, ym),
                to: (x2, ym + r),
                center: (x2 - dir * r, ym + r),
                radius: r,
            });
        }
        push(LinePiece::Straight {
            from: (x2, ym + r),
            to,
        });
        out
    }

    /// `(x1, y1)` から下りて四分円で横へ曲がり、`(x2, y2)` で止まる線（ストリップの分岐）。
    pub fn hook(from: (f32, f32), to: (f32, f32)) -> Vec<LinePiece> {
        let ((x1, y1), (x2, y2)) = (from, to);
        let dir = if x2 >= x1 { 1.0 } else { -1.0 };
        let r = Self::RADIUS.px().min((x2 - x1).abs()).min(y2 - y1).max(0.0);
        let mut out = vec![LinePiece::Straight {
            from,
            to: (x1, y2 - r),
        }];
        if r > 0.0 {
            out.push(LinePiece::Quarter {
                from: (x1, y2 - r),
                to: (x1 + dir * r, y2),
                center: (x1 + dir * r, y2 - r),
                radius: r,
            });
        }
        out.push(LinePiece::Straight {
            from: (x1 + dir * r, y2),
            to,
        });
        out.retain(|p| p.from() != p.to());
        out
    }

    /// 中央の縦の線 `x` から、分岐の列 `columns` へ開く（`split`）か、列から `x` へ閉じる
    /// かっこ。開くときは `y_from`（上の部品の下）から `y_to`（分岐の頭の上）へ、閉じるときは
    /// 分岐の下から下の部品の上へ。曲がる高さは真ん中。分岐が 1 本なら、まっすぐの線だけ。
    pub fn bracket(x: f32, columns: &[f32], y_from: f32, y_to: f32, split: bool) -> Vec<LinePiece> {
        let ym = (y_from + y_to) / 2.0;
        if let [only] = columns {
            let (a, b) = if split { (x, *only) } else { (*only, x) };
            return Self::elbow((a, y_from), (b, y_to), ym);
        }
        columns
            .iter()
            .flat_map(|c| {
                let (a, b) = if split { (x, *c) } else { (*c, x) };
                Self::elbow((a, y_from), (b, y_to), ym)
            })
            .collect()
    }

    /// 線の区間がすべて決まりの形で、続いているか。
    pub fn lawful(pieces: &[LinePiece]) -> bool {
        pieces.iter().all(|p| p.is_lawful())
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

    #[test]
    fn elbows_turn_only_by_quarter_circles() {
        for (from, to, ym) in [
            ((10.0, 0.0), (60.0, 40.0), 20.0),
            ((60.0, 0.0), (10.0, 40.0), 20.0),
            ((10.0, 0.0), (14.0, 40.0), 20.0),
            ((10.0, 0.0), (60.0, 6.0), 3.0),
        ] {
            let pieces = StructureLine::elbow(from, to, ym);
            assert!(StructureLine::lawful(&pieces), "{from:?} {to:?}");
            assert_eq!(pieces.first().unwrap().from(), from);
            assert_eq!(pieces.last().unwrap().to(), to);
            for w in pieces.windows(2) {
                assert_eq!(w[0].to(), w[1].from(), "the line is unbroken");
            }
        }
        // The same column: one straight line.
        let one = StructureLine::elbow((10.0, 0.0), (10.0, 40.0), 20.0);
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn hooks_and_brackets_keep_the_shape() {
        let h = StructureLine::hook((10.0, 0.0), (40.0, 11.0));
        assert!(StructureLine::lawful(&h));
        assert_eq!(h.last().unwrap().to(), (40.0, 11.0));
        for split in [true, false] {
            let b = StructureLine::bracket(50.0, &[20.0, 50.0, 80.0], 0.0, 40.0, split);
            assert!(StructureLine::lawful(&b));
            let quarters = b
                .iter()
                .filter(|p| matches!(p, LinePiece::Quarter { .. }))
                .count();
            assert_eq!(
                quarters, 4,
                "the outer columns turn twice, the middle not at all"
            );
        }
        let single = StructureLine::bracket(50.0, &[50.0], 0.0, 40.0, true);
        assert_eq!(single.len(), 1);
    }

    #[test]
    fn slanted_lines_and_other_curves_are_refused() {
        let slant = LinePiece::Straight {
            from: (0.0, 0.0),
            to: (10.0, 10.0),
        };
        assert!(!slant.is_lawful());
        let wide = LinePiece::Quarter {
            from: (0.0, 0.0),
            to: (20.0, 20.0),
            center: (20.0, 0.0),
            radius: 20.0,
        };
        assert!(!wide.is_lawful(), "a radius over RADIUS");
        let half = LinePiece::Quarter {
            from: (0.0, 0.0),
            to: (8.0, 0.0),
            center: (4.0, 0.0),
            radius: 4.0,
        };
        assert!(!half.is_lawful(), "a half circle");
        let off_axis = LinePiece::Quarter {
            from: (5.66, 5.66),
            to: (5.66, -5.66),
            center: (0.0, 0.0),
            radius: 8.0,
        };
        assert!(!off_axis.is_lawful(), "ends off the axes");
    }

    #[test]
    fn ends_stop_short_of_the_edges() {
        assert_eq!(StructureLine::inset(false), StructureLine::END_INSET);
        assert_eq!(StructureLine::inset(true), StructureLine::TIGHT_INSET);
        const {
            assert!(StructureLine::TIGHT_INSET < StructureLine::END_INSET);
            assert!(StructureLine::TIGHT_INSET >= StructureLine::THICKNESS);
            assert!(StructureLine::RADIUS.px() > StructureLine::THICKNESS);
        }
    }
}
