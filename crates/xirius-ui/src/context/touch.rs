//! Touch recognition independent of a window or OS input injection.
use std::time::{Duration, Instant};

pub const HOLD: Duration = Duration::from_millis(500);
const SLOP: f32 = 8.0;

pub struct Touch<T> {
    pending: Option<(T, (f32, f32), Instant)>,
    consumed: bool,
}

impl<T> Default for Touch<T> {
    fn default() -> Self {
        Self {
            pending: None,
            consumed: false,
        }
    }
}

impl<T> Touch<T> {
    pub fn press(&mut self, event: T, position: (f32, f32), now: Instant) {
        self.pending = Some((event, position, now));
        self.consumed = false;
    }

    pub fn waiting(&self) -> bool {
        self.pending.is_some()
    }

    /// A drag starts with the original press; a small tremor still counts as a hold.
    pub fn moved(&mut self, position: (f32, f32)) -> Option<T> {
        let (_, start, _) = self.pending.as_ref()?;
        if (position.0 - start.0).hypot(position.1 - start.1) > SLOP {
            self.pending.take().map(|p| p.0)
        } else {
            None
        }
    }

    pub fn poll(&mut self, now: Instant) -> Option<T> {
        let (_, _, start) = self.pending.as_ref()?;
        if now.saturating_duration_since(*start) < HOLD {
            return None;
        }
        self.consumed = true;
        self.pending.take().map(|p| p.0)
    }

    /// (short tap to replay, suppress this release after opening a menu).
    pub fn release(&mut self) -> (Option<T>, bool) {
        (
            self.pending.take().map(|p| p.0),
            std::mem::take(&mut self.consumed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hold_tap_and_drag_are_mutually_exclusive() {
        let now = Instant::now();
        let mut touch = Touch::default();
        touch.press(7, (10.0, 10.0), now);
        assert_eq!(touch.poll(now + HOLD - Duration::from_millis(1)), None);
        assert_eq!(touch.moved((12.0, 13.0)), None);
        assert_eq!(touch.poll(now + HOLD), Some(7));
        assert_eq!(touch.poll(now + HOLD), None);
        assert_eq!(touch.release(), (None, true));
        touch.press(8, (10.0, 10.0), now);
        assert_eq!(touch.release(), (Some(8), false));
        touch.press(9, (10.0, 10.0), now);
        assert_eq!(touch.moved((30.0, 10.0)), Some(9));
        assert_eq!(touch.poll(now + HOLD), None);
        assert_eq!(touch.release(), (None, false));
    }
}
