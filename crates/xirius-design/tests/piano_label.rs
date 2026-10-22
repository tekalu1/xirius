//! The roll's note names (C4, …) read on the key they sit on, in both themes (UI-013).

use xirius_design::{Accent, MIN_TEXT_CONTRAST, PianoKey, Theme, contrast_ratio};

#[test]
fn the_note_name_reads_on_a_white_key_and_on_a_lit_key_in_both_themes() {
    for theme in Theme::ALL {
        for (face, lit) in [
            (PianoKey::White.color(theme), false),
            (Accent::FocusRing.color(theme), true),
        ] {
            let ratio = contrast_ratio(PianoKey::label_on(theme, lit), face);
            assert!(ratio >= MIN_TEXT_CONTRAST, "{theme:?}: {ratio:.2}");
        }
    }
}
