//! A controller's knob moving a fader or a pan knob (OPS-007).
//!
//! The knob takes the same path as the pointer: it starts a drag, every value moves it, and one
//! command (or the automation being written) is made when the knob has been still for [`QUIET`].
//! A sweep of a knob is one step of the history, not one per message.

use std::time::{Duration, Instant};

use super::meters::{FLOOR_DB, t_to_db};
use super::{Drag, MixerHit};
use crate::studio::Studio;
use xirius_core::model::MIN_DB;

/// How long a knob stays still before its sweep is let go.
pub const QUIET: Duration = Duration::from_millis(300);

/// The value a controller position (0..=127) gives the fader or the pan knob: the knob's own
/// position, as on the screen.
pub fn value_of(hit: MixerHit, cc: u8) -> f64 {
    let n = f32::from(cc.min(127)) / 127.0;
    match hit {
        MixerHit::Pan(_) => {
            let v = n * 2.0 - 1.0;
            let v = if v.abs() < 0.03 { 0.0 } else { v };
            f64::from((v * 100.0).round() / 100.0)
        }
        _ => {
            let db = t_to_db(n);
            if db <= FLOOR_DB {
                MIN_DB
            } else {
                f64::from(db)
            }
        }
    }
}

impl Studio {
    /// Moves the fader or the pan knob to the controller's position `cc`. False when the
    /// pointer holds a control (it keeps it) or the value is not one that moves.
    pub fn knob_move(&mut self, hit: MixerHit, cc: u8) -> bool {
        if self.mixer.drag.is_some() && self.mixer.knob_at.is_none() {
            return false;
        }
        if self.mixer.drag.is_some_and(|d| d.hit != hit) {
            self.knob_end();
        }
        if self.mixer.drag.is_none() {
            if self.mixer_locked(hit) {
                return false;
            }
            let Some(start) = self.mixer_value(hit) else {
                return false;
            };
            self.mixer.drag = Some(Drag {
                hit,
                y0: 0.0,
                travel: 1.0,
                start,
                value: start,
            });
            self.auto_touch(hit, xirius_session::autowrite::Phase::Begin, Some(start));
        }
        self.mixer.knob_at = Some(Instant::now());
        self.drag_to(value_of(hit, cc))
    }

    /// Lets go of the sweep of a knob: the value is made one step.
    pub fn knob_end(&mut self) -> bool {
        if self.mixer.knob_at.take().is_none() {
            return false;
        }
        self.drag_end()
    }

    /// Lets go of a sweep whose knob has been still. True when one went.
    pub fn knob_quiet(&mut self) -> bool {
        match self.mixer.knob_at {
            Some(at) if at.elapsed() >= QUIET => self.knob_end(),
            _ => false,
        }
    }
}
