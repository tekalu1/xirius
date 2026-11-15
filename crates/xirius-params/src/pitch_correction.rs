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
    if norm >= CUSTOM_BASE - 0.000_001 {
        (((norm.clamp(CUSTOM_BASE, 1.0) - CUSTOM_BASE) * f64::from(CHROMATIC) / (1.0 - CUSTOM_BASE))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keyboard_mask_survives_the_song_value() {
        for mask in 0..=CHROMATIC {
            let stored = custom_norm(mask) as f32;
            assert_eq!(scale_mask(f64::from(stored)), mask);
            assert_eq!(
                crate::PITCH_CORRECTION.params[1].to_plain(f64::from(stored)),
                3.0
            );
        }
        assert_eq!(scale_mask(0.0), CHROMATIC);
        assert_eq!(scale_mask(1.0 / 3.0), MAJOR);
        assert_eq!(scale_mask(2.0 / 3.0), MINOR);
    }
}
