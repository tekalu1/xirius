//! The input a short save holds back (PRJ-031): before the save needs its veil, the window's
//! presses, keys and typed text wait, as they did with a synchronous save, and come in order
//! once it is done; after the veil they go. A key's characters come as text of their own,
//! not with the key's event let through again, so the text waits beside the keys (UI-326,
//! review 3 M1: a value typed while a double press's undo runs on the worker). A copy or a
//! paste of inserts asking its plug-ins for their state holds the input the same way, all
//! the while it waits, as it has no veil (MIX-045).

use std::ops::Range;

use gpui::PlatformInput;

/// One piece of input held back: an event, or text typed into a field.
pub(crate) enum Held {
    Event(PlatformInput),
    Text(Option<Range<usize>>, String),
}

/// What waits for the save, in the order it came.
#[derive(Default)]
pub(crate) struct HeldInput(Vec<Held>);

impl HeldInput {
    /// Whether something short holds the window ([`Studio::input_hold`]): `held` waits for it
    /// then (while it defers input, or a press's release whose press waits), or goes.
    ///
    /// [`Studio::input_hold`]: crate::studio::Studio::input_hold
    pub(crate) fn hold(&mut self, holding: Option<bool>, held: Held) -> bool {
        let Some(defers) = holding else {
            return false;
        };
        let waits = |f: &dyn Fn(&PlatformInput) -> bool| {
            self.0.iter().any(|h| matches!(h, Held::Event(e) if f(e)))
        };
        let release = match &held {
            Held::Event(PlatformInput::MouseUp(up)) => {
                waits(&|e| matches!(e, PlatformInput::MouseDown(down) if down.button == up.button))
            }
            Held::Event(PlatformInput::KeyUp(up)) => waits(
                &|e| matches!(e, PlatformInput::KeyDown(down) if down.keystroke.key == up.keystroke.key),
            ),
            _ => false,
        };
        if defers || release {
            self.0.push(held);
        }
        true
    }

    /// What waited, once nothing holds the window.
    pub(crate) fn release(&mut self, holding: Option<bool>) -> Vec<Held> {
        if holding.is_some() {
            return Vec::new();
        }
        std::mem::take(&mut self.0)
    }
}
