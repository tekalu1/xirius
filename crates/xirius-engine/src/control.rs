//! The controller values an instrument has been told about (EDT-004).
//!
//! Controller lanes of a MIDI clip reach every instrument as [`NoteKind::Control`] events next to
//! the notes. This is the port an instrument keeps them in: it calls [`ControlState::apply`] for
//! each event it is handed and reads the current values where its sound depends on them. What a
//! controller does to the sound is the instrument's own business.

use crate::processor::{CTRL_AFTERTOUCH, CTRL_PITCH_BEND, NoteKind};

/// The number of distinct controller numbers: 128 CCs, aftertouch and pitch bend.
pub const CONTROL_NUMBERS: usize = 130;

/// The value a controller has when no point sets it: bend centred, expression (CC 11) open,
/// everything else zero.
pub fn rest_value(number: u8) -> i32 {
    match number {
        11 => 127,
        _ => 0,
    }
}

/// The range a controller's value takes.
pub fn value_range(number: u8) -> (i32, i32) {
    if number == CTRL_PITCH_BEND {
        (-8192, 8191)
    } else {
        (0, 127)
    }
}

/// Where a controller number sits in a table of [`CONTROL_NUMBERS`].
pub fn index(number: u8) -> usize {
    usize::from(number).min(CONTROL_NUMBERS - 1)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlState {
    values: [i32; CONTROL_NUMBERS],
}

impl Default for ControlState {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlState {
    pub fn new() -> ControlState {
        let mut values = [0; CONTROL_NUMBERS];
        for (n, v) in values.iter_mut().enumerate() {
            *v = rest_value(n as u8);
        }
        ControlState { values }
    }

    /// Takes in one event; false (and no change) for a note event.
    pub fn apply(&mut self, kind: NoteKind) -> bool {
        let NoteKind::Control { number, value } = kind else {
            return false;
        };
        let (lo, hi) = value_range(number);
        self.values[index(number)] = value.clamp(lo, hi);
        true
    }

    /// Back to the rest values (a new song, or the transport jumped).
    pub fn reset(&mut self) {
        *self = ControlState::new();
    }

    pub fn cc(&self, number: u8) -> i32 {
        self.values[index(number.min(127))]
    }

    pub fn aftertouch(&self) -> i32 {
        self.values[index(CTRL_AFTERTOUCH)]
    }

    pub fn pitch_bend(&self) -> i32 {
        self.values[index(CTRL_PITCH_BEND)]
    }

    /// Pitch bend as -1..=1.
    pub fn bend_unit(&self) -> f32 {
        let v = self.pitch_bend();
        if v < 0 {
            v as f32 / 8192.0
        } else {
            v as f32 / 8191.0
        }
    }

    /// A CC (or aftertouch for 128) as 0..=1.
    pub fn unit(&self, number: u8) -> f32 {
        self.values[index(number)] as f32 / 127.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_rest_and_follows_events() {
        let mut s = ControlState::new();
        assert_eq!(s.pitch_bend(), 0);
        assert_eq!(s.cc(11), 127);
        assert_eq!(s.cc(64), 0);
        assert!(s.apply(NoteKind::control(64, 127)));
        assert!(s.apply(NoteKind::control(CTRL_PITCH_BEND, -8192)));
        assert!(s.apply(NoteKind::control(CTRL_AFTERTOUCH, 90)));
        assert_eq!(s.cc(64), 127);
        assert_eq!(s.bend_unit(), -1.0);
        assert_eq!(s.aftertouch(), 90);
        assert!(!s.apply(NoteKind::Off { pitch: 60 }));
        s.reset();
        assert_eq!(s, ControlState::new());
    }

    #[test]
    fn values_are_kept_in_range() {
        let mut s = ControlState::new();
        s.apply(NoteKind::control(1, 500));
        s.apply(NoteKind::control(CTRL_PITCH_BEND, 100_000));
        assert_eq!(s.cc(1), 127);
        assert_eq!(s.pitch_bend(), 8191);
        assert_eq!(s.bend_unit(), 1.0);
    }
}
