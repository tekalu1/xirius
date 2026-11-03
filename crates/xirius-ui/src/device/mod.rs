//! The parts every built-in device's window shares (UI-324…): its controls,
//! where they sit, and how a press, the wheel and the keys move them. The
//! window itself, its title bar and its place on the screen are
//! `instrument::window`'s.

pub mod control;
pub mod graph;
pub mod input;
pub mod layout;

#[cfg(test)]
mod input_tests;
