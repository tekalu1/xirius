//! Which of the two bundled faces draws each character of a string (UI-005).
//!
//! GPUI hands a character the chosen face lacks to the OS fonts (DirectWrite's fallback), which
//! is the one way a screen could end up in a face that is not bundled. The bundled Noto Sans JP is
//! a subset (JIS X 0208 and Latin) that lacks some symbols Inter has, the Mac's key symbols
//! (`⌘⌥⌃⌫↩⇥`) among them, so a string is drawn in one face and the characters only the other
//! face has are drawn in that one. The cmaps are read from the embedded files; the three weights
//! of a family share one (`tests/glyph_cover.rs` holds them to it).

use std::sync::OnceLock;

use skrifa::charmap::Charmap;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use xirius_design::Font;

use crate::fonts::EMBEDDED;

fn charmap(file: &str) -> Charmap<'static> {
    let (_, bytes) = EMBEDDED
        .iter()
        .find(|(name, _)| *name == file)
        .expect("the face is embedded");
    FontRef::new(bytes)
        .expect("the embedded face reads")
        .charmap()
}

fn inter() -> &'static Charmap<'static> {
    static MAP: OnceLock<Charmap<'static>> = OnceLock::new();
    MAP.get_or_init(|| charmap("Inter-Regular.ttf"))
}

fn noto() -> &'static Charmap<'static> {
    static MAP: OnceLock<Charmap<'static>> = OnceLock::new();
    MAP.get_or_init(|| charmap("NotoSansJP-Regular.ttf"))
}

/// Characters that are not drawn as a glyph (line breaks, tabs) and need no face.
fn drawn(ch: char) -> bool {
    !ch.is_control()
}

/// The bundled face `font` has a glyph for `ch`.
pub fn has(font: Font, ch: char) -> bool {
    let map = match font {
        Font::Inter => inter(),
        Font::NotoSansJp => noto(),
    };
    map.map(ch).is_some_and(|g| g != GlyphId::NOTDEF)
}

fn other(font: Font) -> Font {
    match font {
        Font::Inter => Font::NotoSansJp,
        Font::NotoSansJp => Font::Inter,
    }
}

/// The face a string is drawn in: Inter for ASCII, otherwise Noto Sans JP (which has Latin too,
/// so a Japanese string keeps one face), or Inter when only Inter has all its characters.
pub fn pick(text: &str) -> Font {
    let all = |font| text.chars().filter(|c| drawn(*c)).all(|c| has(font, c));
    if text.is_ascii() || (!all(Font::NotoSansJp) && all(Font::Inter)) {
        Font::Inter
    } else {
        Font::NotoSansJp
    }
}

/// The face of each character: the string's face, or the other one where only it has the glyph.
fn face_of(base: Font, ch: char) -> Font {
    if drawn(ch) && !has(base, ch) && has(other(base), ch) {
        other(base)
    } else {
        base
    }
}

/// The string cut into `(byte length, face)` runs, in order, covering all of it.
pub fn runs(text: &str) -> Vec<(usize, Font)> {
    let base = pick(text);
    let mut out: Vec<(usize, Font)> = Vec::new();
    for ch in text.chars() {
        let font = face_of(base, ch);
        match out.last_mut() {
            Some((len, last)) if *last == font => *len += ch.len_utf8(),
            _ => out.push((ch.len_utf8(), font)),
        }
    }
    out
}

/// The characters of `text` neither bundled face has: the ones the OS would draw. Empty for
/// every string the screen itself writes.
pub fn missing(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| drawn(*c) && !has(Font::Inter, *c) && !has(Font::NotoSansJp, *c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_takes_inter_and_japanese_takes_noto() {
        assert_eq!(pick("Drums 120"), Font::Inter);
        assert_eq!(pick("夜の散歩"), Font::NotoSansJp);
        assert_eq!(runs("Ctrl+→"), vec![("Ctrl+→".len(), Font::NotoSansJp)]);
    }

    #[test]
    fn a_symbol_only_inter_has_takes_inter() {
        for s in ["⌫", "⇧⌘Z", "⌃⌥↩", "⇥"] {
            assert_eq!(runs(s), vec![(s.len(), Font::Inter)], "{s}");
        }
    }

    #[test]
    fn a_mac_key_in_a_japanese_line_is_drawn_in_inter() {
        let s = "⌘K は割り当てられていません";
        assert_eq!(
            runs(s),
            vec![
                ("⌘".len(), Font::Inter),
                (s.len() - "⌘".len(), Font::NotoSansJp)
            ]
        );
        assert!(missing(s).is_empty());
    }

    #[test]
    fn a_character_neither_face_has_is_reported() {
        assert_eq!(missing("a𝄞b"), vec!['𝄞']);
        assert!(missing("一行目\n二行目").is_empty());
    }
}
