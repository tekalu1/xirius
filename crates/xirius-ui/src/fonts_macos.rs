//! CoreText shaping for window-free callers; no NSApplication or display is needed.
use core_foundation::{
    attributed_string::CFMutableAttributedString,
    base::{CFRange, TCFType},
    string::CFString,
};
use core_text::{font::CTFont, line::CTLine, string_attributes::kCTFontAttributeName};
use xirius_design::{FONT_FILES, FontSize, Weight};

thread_local! {
    static FONTS: Vec<CTFont> = super::EMBEDDED.iter()
        .map(|(_, bytes)| core_text::font::new_from_buffer(bytes).expect("bundled font"))
        .collect();
}

pub(super) fn measure(text: &str, size: FontSize, weight: Weight) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    FONTS.with(|fonts| {
        let mut string = CFMutableAttributedString::new();
        string.replace_str(&CFString::new(text), CFRange::init(0, 0));
        let (mut byte, mut utf16) = (0, 0);
        for (len, face) in crate::glyphs::runs(text) {
            let n = text[byte..byte + len].encode_utf16().count();
            let index = FONT_FILES
                .iter()
                .position(|f| f.font == face && f.weight == weight)
                .expect("bundled face and weight");
            let font = fonts[index].clone_with_font_size(f64::from(size.px()));
            // SAFETY: CoreText's ranges are UTF-16; each run is within this live string.
            unsafe {
                string.set_attribute(
                    CFRange::init(utf16 as isize, n as isize),
                    kCTFontAttributeName,
                    &font,
                );
            }
            byte += len;
            utf16 += n;
        }
        CTLine::new_with_attributed_string(string.as_concrete_TypeRef())
            .get_typographic_bounds()
            .width as f32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_faces_shape_on_a_worker_without_an_application() {
        std::thread::spawn(|| {
            let width = |s| measure(s, FontSize::Body, Weight::Regular);
            assert_eq!(width(""), 0.0);
            assert!(width("WWWW") > width("iiii") * 3.0);
            for text in ["日本語", "A日本語⌘", "𝄞 日本語"] {
                assert!(width(text).is_finite() && width(text) > 0.0);
            }
        })
        .join()
        .unwrap();
    }
}
