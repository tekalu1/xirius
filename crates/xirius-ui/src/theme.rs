//! The bridge from `xirius-design` tokens to GPUI types.
//!
//! This file holds no colour, size or font of its own: every value comes from
//! `xirius-design` (UI-049). Colours are converted here and nowhere else; the
//! rest of the crate asks for a token (`Surface`, `Ink`, `TrackColor`, …) and
//! turns it into an `Hsla` with [`paint`].
//!
//! Rules this crate follows (docs/plan.toml, UI-007 / UI-008): no border lines,
//! no gradients, clips are a neutral surface and only their contents use the
//! track colour. Surfaces are told apart by their lightness only. The one
//! line-like mark is the folder range line (`StructureLine`, MIX-038).

use std::sync::Arc;

use gpui::{FontFeatures, FontWeight, Hsla, Rgba, SharedString};
use xirius_design::{Color, FAMILY_STACK, Font, FontSize, Layout, TABULAR_NUMS, Theme, Weight};

/// The theme the app runs in. Light is not wired to a setting yet.
pub const THEME: Theme = Theme::Dark;

/// A design colour as a GPUI colour.
pub fn paint(color: Color) -> Hsla {
    let [r, g, b, a] = color.to_f32();
    // design-lint: allow(raw-color) the one place a design token becomes a GPUI colour
    Rgba { r, g, b, a }.into()
}

/// The family name GPUI knows a bundled face by.
pub fn family(font: Font) -> SharedString {
    SharedString::from(font.family())
}

/// The face of a family at a weight. `tabular` turns on `tnum` so digits keep
/// their width.
fn face(font: Font, weight: Weight, tabular: bool) -> gpui::Font {
    let mut face = gpui::font(family(font));
    face.weight = FontWeight(f32::from(weight.value()));
    if tabular {
        face.features = FontFeatures(Arc::new(vec![(TABULAR_NUMS.to_string(), 1)]));
    }
    face
}

/// The text face for Latin text: Inter.
pub fn ui_font(weight: Weight, tabular: bool) -> gpui::Font {
    face(FAMILY_STACK[0], weight, tabular)
}

/// The face for a given string: Inter when it is ASCII, otherwise Noto Sans JP
/// (which has Latin too) for the whole string, or Inter when only Inter has all
/// its characters (the Mac's key symbols; see [`crate::glyphs::pick`]).
///
/// GPUI's DirectWrite fallback only searches the OS fonts, never the bundled
/// ones, and draws the fallback face's glyph ids from a different face of the
/// same family (the screenshots showed wrong kanji). Choosing the family
/// ourselves keeps every glyph in a bundled face.
pub fn text_font(text: &str, weight: Weight, tabular: bool) -> gpui::Font {
    face(crate::glyphs::pick(text), weight, tabular)
}

/// The text runs of a string in [`text_font`]'s face, with the characters only the other bundled
/// face has (a Mac key symbol in a Japanese line) in that face.
pub fn text_runs(text: &str, weight: Weight, tabular: bool, color: Hsla) -> Vec<gpui::TextRun> {
    crate::glyphs::runs(text)
        .into_iter()
        .map(|(len, font)| gpui::TextRun {
            len,
            font: face(font, weight, tabular),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect()
}

/// Sizes in logical pixels, all taken from `xirius-design`.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub top_bar_h: f32,
    /// Width of a Windows minimise / maximise / close button.
    pub caption_button_w: f32,
    /// macOS: the traffic lights sit this far from the left and the top.
    pub traffic_light_x: f32,
    pub traffic_light_y: f32,
    /// macOS: room the traffic lights take at the left of the top bar.
    pub traffic_light_room: f32,
    pub ruler_h: f32,
    pub header_w: f32,
    pub track_h: f32,
    pub track_gap: f32,
    pub piano_keys_w: f32,
    pub note_row_h: f32,
    pub meter_strip_h: f32,
    pub meter_gap: f32,
    pub meter_hold_h: f32,
    pub playhead_w: f32,
    pub text_size: f32,
    pub small_text_size: f32,
}

pub const METRICS: Metrics = Metrics {
    top_bar_h: Layout::TOP_BAR_H,
    caption_button_w: Layout::CAPTION_BUTTON_W,
    traffic_light_x: Layout::TRAFFIC_LIGHT_X,
    traffic_light_y: Layout::TRAFFIC_LIGHT_Y,
    traffic_light_room: Layout::TRAFFIC_LIGHT_ROOM,
    ruler_h: Layout::RULER_H,
    header_w: Layout::BENCH_HEADER_W,
    track_h: Layout::BENCH_TRACK_H,
    track_gap: Layout::BENCH_TRACK_GAP,
    piano_keys_w: Layout::PIANO_KEYS_W,
    note_row_h: Layout::NOTE_ROW_H,
    meter_strip_h: Layout::METER_STRIP_H,
    meter_gap: Layout::METER_GAP,
    meter_hold_h: Layout::METER_HOLD_H,
    playhead_w: Layout::PLAYHEAD_W,
    text_size: FontSize::Small.px(),
    small_text_size: FontSize::Caption.px(),
};

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_design::{Ink, Surface, TrackColor};

    #[test]
    fn paint_keeps_a_design_colour() {
        let white = paint(xirius_design::PianoKey::White.color(THEME));
        assert!((white.l - 1.0).abs() < 1e-6 && white.a == 1.0);
        let dark = paint(Surface::S0.color(THEME));
        assert!(dark.l < 0.1);
    }

    #[test]
    fn paint_keeps_alpha() {
        let veil = paint(xirius_design::Overlay::Veil.color(THEME));
        assert!(veil.a < 1.0 && veil.a > 0.0);
    }

    #[test]
    fn tokens_turn_into_distinct_colours() {
        let s1 = paint(Surface::S1.color(THEME));
        let s3 = paint(Surface::S3.color(THEME));
        assert_ne!(s1, s3);
        let ink = paint(Surface::S1.text().ink_color(THEME));
        assert_ne!(ink, s1);
        assert_eq!(Surface::S1.text().ink(), Ink::Body);
        assert_ne!(
            paint(TrackColor::new(0).content(THEME)),
            paint(TrackColor::new(1).content(THEME))
        );
    }

    #[test]
    fn japanese_text_takes_the_japanese_face_and_ascii_takes_inter() {
        let latin = text_font("Drums 120", Weight::Medium, false);
        let japanese = text_font("夜の散歩", Weight::Medium, false);
        assert_eq!(latin.family.as_ref(), "Inter");
        assert_eq!(japanese.family.as_ref(), "Noto Sans JP");
        assert_eq!(latin.weight, japanese.weight);
        assert!(latin.fallbacks.is_none() && japanese.fallbacks.is_none());
    }

    #[test]
    fn family_names_are_the_bundled_ones() {
        assert_eq!(family(Font::Inter).as_ref(), "Inter");
        assert_eq!(family(Font::NotoSansJp).as_ref(), "Noto Sans JP");
    }
}
