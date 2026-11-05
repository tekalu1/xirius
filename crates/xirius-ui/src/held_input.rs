//! The input a short save holds back (PRJ-031): before the save needs its veil, the window's
//! presses, keys and typed text wait, as they did with a synchronous save, and come in order
//! once it is done; after the veil they go. A key's characters come as text of their own,
//! not with the key's event let through again, so the text waits beside the keys (UI-326,
//! review 3 M1: a value typed while a double press's undo runs on the worker).

use std::ops::Range;

use gpui::PlatformInput;

use crate::saving::Saving;

/// One piece of input held back: an event, or text typed into a field.
pub(crate) enum Held {
    Event(PlatformInput),
    Text(Option<Range<usize>>, String),
}

/// What waits for the save, in the order it came.
#[derive(Default)]
pub(crate) struct HeldInput(Vec<Held>);

impl HeldInput {
    /// Whether a save holds the window: `held` waits for it then (before its veil, or a
    /// press's release whose press waits), or goes.
    pub(crate) fn hold(&mut self, saving: Option<&Saving>, held: Held) -> bool {
        let Some(save) = saving else {
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
        if save.defers_input() || release {
            self.0.push(held);
        }
        true
    }

    /// What waited, once no save holds the window.
    pub(crate) fn release(&mut self, saving: Option<&Saving>) -> Vec<Held> {
        if saving.is_some() {
            return Vec::new();
        }
        std::mem::take(&mut self.0)
    }
}
