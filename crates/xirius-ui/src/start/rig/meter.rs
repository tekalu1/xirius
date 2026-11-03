//! How wide the output's meters are for a level.

/// How wide a meter's bar is for a level: 60 dB across, as the mock's meters.
pub fn level_width(a: f32) -> f32 {
    if a <= 0.0005 {
        0.0
    } else {
        (1.0 + a.max(1e-3).log10() / 3.0).clamp(0.02, 1.0)
    }
}
