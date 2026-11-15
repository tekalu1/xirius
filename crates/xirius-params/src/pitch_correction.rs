//! The normalized Scale value also keeps a 12-key mask relative to Key.
//!
//! 0, 1/3 and 2/3 are the three presets. Custom masks occupy [0.84, 1]. This
//! leaves seven public automation parameters, while a song's existing map of
//! normalized values can save every keyboard state without a model change.

pub const CHROMATIC: u16 = 0x0fff;
pub const MAJOR: u16 = 0b1010_1011_0101;
pub const MINOR: u16 = 0b0101_1010_1101;
const CUSTOM_BASE: f64 = 0.84;

pub fn custom_norm(mask: u16) -> f64 {
    CUSTOM_BASE + f64::from(mask & CHROMATIC) * (1.0 - CUSTOM_BASE) / f64::from(CHROMATIC)
}

pub fn scale_mask(norm: f64) -> u16 {
    if norm >= CUSTOM_BASE {
        (((norm.clamp(CUSTOM_BASE, 1.0) - CUSTOM_BASE) * f64::from(CHROMATIC)
            / (1.0 - CUSTOM_BASE))
            .round() as u16)
            & CHROMATIC
    } else if norm >= 0.5 {
        MINOR
    } else if norm >= 1.0 / 6.0 {
        MAJOR
    } else {
        CHROMATIC
    }
}

/// Toggling a key always enters Custom. Bit zero is the selected Key.
pub fn toggle_relative(norm: f64, relative: u8) -> f64 {
    custom_norm(scale_mask(norm) ^ (1 << (relative % 12)))
}
