//! User text for the loading-probe window tests. It deliberately includes glyphs
//! outside the bundled faces, unlike the product's fixed UI labels.

pub(super) fn long_track_name() -> String {
    "長い同名トラック🎹・ストリングスとピアノを重ねた最後のテイク".repeat(20)
}
