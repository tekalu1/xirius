//! The parts every built-in device's window shares (UI-324…): its controls,
//! where they sit, and how a press, the wheel and the keys move them. The
//! window itself, its title bar and its place on the screen are
//! `instrument::window`'s.

pub mod compressor;
pub mod control;
pub mod delay;
pub mod dynamics;
pub mod eq;
pub mod graph;
pub mod input;
pub mod layout;
pub mod limiter;
pub mod play;
pub mod reverb;

#[cfg(test)]
mod compressor_tests;
#[cfg(test)]
mod delay_tests;
#[cfg(test)]
mod eq_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod limiter_tests;
#[cfg(test)]
mod reverb_tests;
