//! 面の段の差の検査（UI-007）。
//!
//! 線も上辺の光も使わないので、面どうしの区切りは「明るさの差」だけになる。
//! 差が小さすぎると面の境が見えなくなるため、明るさの順に並べた隣り合う段の差に下限を置く。
//!
//! 差は CIE L*（知覚的な明るさ、0〜100）で測る。コントラスト比（WCAG）は暗い側で差を
//! 過小に見せるので、暗い・明るいの両方を同じ物差しで測れる L* を使う。
//!
//! 下限 [`MIN_STEP_DELTA_L`] = 1.25:
//! - 平らな 2 色が隣り合うとき、L* の差がおよそ 1 で、ようやく境が分かる（最小可知差）。
//!   1.25 はその上に少し余裕を取った値で、これを割ると面の境が見えなくなる。
//! - いまのパレットの最小は、暗い S0→S2 の 1.42 と、明るい S2→S0 の 1.37（S0 と S2 は
//!   いちばん近い対）。下限はそれより低くしてある。パレットの値を直すときは、
//!   この下限を下げずに、段を広げる。
//! - S0↔S2 以外の隣り合う段は、暗い側が 3.0 以上、明るい側が 1.7 以上（選択の面と S2 の間が最小）。
//!
//! 対象は、塗り（`Fill`・`FillHover`）を除く中立の面。同じ色の面（ホバーは S1、
//! 暗い側の選択は S3 と同じ色）は 1 段として数える。

use xirius_design::{Color, Surface, Theme};

const MIN_STEP_DELTA_L: f64 = 1.25;

const NEUTRAL: [Surface; 9] = [
    Surface::Paper,
    Surface::S0,
    Surface::S1,
    Surface::S2,
    Surface::S3,
    Surface::Hover,
    Surface::Sel,
    Surface::Clip,
    Surface::ClipSel,
];

/// CIE L*（0〜100）。
fn lightness(c: Color) -> f64 {
    let y = c.relative_luminance();
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        24389.0 / 27.0 * y
    }
}

/// 明るさの順に並べた、色が違う段（面の名前つき）。
fn ladder(theme: Theme) -> Vec<(Surface, f64)> {
    let mut steps: Vec<(Surface, Color)> = Vec::new();
    for surface in NEUTRAL {
        let color = surface.color(theme);
        if !steps.iter().any(|(_, c)| *c == color) {
            steps.push((surface, color));
        }
    }
    let mut steps: Vec<(Surface, f64)> =
        steps.into_iter().map(|(s, c)| (s, lightness(c))).collect();
    steps.sort_by(|a, b| a.1.total_cmp(&b.1));
    steps
}

#[test]
fn adjacent_surface_steps_are_far_enough_apart_to_tell_in_both_themes() {
    for theme in Theme::ALL {
        let steps = ladder(theme);
        assert!(steps.len() >= 6, "{theme:?}: 段が足りない: {steps:?}");
        let mut worst = f64::MAX;
        for pair in steps.windows(2) {
            let delta = pair[1].1 - pair[0].1;
            worst = worst.min(delta);
            assert!(
                delta >= MIN_STEP_DELTA_L,
                "{theme:?}: {:?} と {:?} の L* の差 {delta:.2} が下限 {MIN_STEP_DELTA_L} より小さい",
                pair[0].0,
                pair[1].0
            );
        }
        println!(
            "{theme:?}: 隣り合う面の段の差 最小 ΔL* = {worst:.2}（{} 段）",
            steps.len()
        );
    }
}
