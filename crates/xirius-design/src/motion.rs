//! 動きのトークン。時間は 120 / 240 / 560 ms（UI-053）。AI が動いている印の 1 周は約 1 秒。
//! 値は `tokens.json` が元で、`generated.rs` に書き出される（手で書かない）。
//! 「視差・動きを減らす」の設定が入っているときは、全部 0（動かさず、終わった状態を出す）。

use crate::AI_LOOP_MS;

/// 動きの長さ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Duration {
    /// ホバー・フォーカス。
    Fast,
    /// 開閉・出現。
    Base,
    /// 大きな面の最小化・最大化。
    Slow,
}

impl Duration {
    pub const ALL: [Duration; 3] = [Duration::Fast, Duration::Base, Duration::Slow];
}

/// 動きの曲線（CSS の `cubic-bezier(x1, y1, x2, y2)` と同じ並び）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Easing(pub [f32; 4]);

/// 動きの設定。`reduced` は OS の「動きを減らす」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Motion {
    pub reduced: bool,
}

impl Motion {
    pub const fn new(reduced: bool) -> Self {
        Motion { reduced }
    }

    /// 実際に使う長さ（ミリ秒）。減らす設定のときは 0。
    pub const fn duration_ms(self, d: Duration) -> u32 {
        if self.reduced { 0 } else { d.ms() }
    }

    /// AI の印の 1 周（ミリ秒）。減らす設定のときは 0（回さず、静止した印を出す）。
    pub const fn ai_loop_ms(self) -> u32 {
        if self.reduced { 0 } else { AI_LOOP_MS }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        let ms: Vec<u32> = Duration::ALL.iter().map(|d| d.ms()).collect();
        assert_eq!(ms, [120, 240, 560]);
    }

    #[test]
    fn reduced_motion_is_zero_everywhere() {
        let m = Motion::new(true);
        for d in Duration::ALL {
            assert_eq!(m.duration_ms(d), 0);
        }
        assert_eq!(m.ai_loop_ms(), 0);
    }

    #[test]
    fn normal_motion_keeps_values() {
        let m = Motion::default();
        assert_eq!(m.duration_ms(Duration::Base), 240);
        assert_eq!(m.ai_loop_ms(), 1000);
    }
}
