//! The one search field: a sunken well, the magnifier at the left, the text, and a clear mark
//! (✖) at the right end whose place is kept even while it is hidden, so neither the text nor
//! the field moves when it shows (ui-approved 2026-11-10).
//!
//! The track browser, the home search, the list menus, the context menu, the About crates
//! search and the command palette all paint and hit-test through this part.

use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::draw::Pen;
use crate::settings::field::{Field, Look as FieldLook};
use crate::studio::Rect;

/// The search field's height (the command palette's is 48).
pub const HEIGHT: f32 = 28.0;
/// The focus ring's width.
pub const RING: f32 = 2.0;
/// The ✖'s hit box.
pub const CLEAR: f32 = 20.0;
/// The screen-reader name of the ✖.
pub const CLEAR_NAME: &str = "クリア";
/// What an empty result says, whatever the list.
pub const NO_MATCH: &str = "見つかりません";
/// The placeholder's text.
pub const PLACEHOLDER: &str = "検索";

/// The well's corner.
pub const RADIUS: Radius = Radius::S;

fn gap() -> f32 {
    Space::X2.px()
}

/// Where the text goes in a field `r` whose sides are `pad` in: after the magnifier, and short
/// of the ✖'s place (always kept).
pub fn text_rect(r: Rect, pad: f32) -> Rect {
    let x = r.x + pad + Layout::ICON_SIZE + gap();
    let right = r.right() - pad - CLEAR - gap() + gap().min(4.0);
    Rect::new(x, r.y, (right - x).max(0.0), r.h)
}

/// The ✖'s box, at the right end.
pub fn clear_rect(r: Rect, pad: f32) -> Rect {
    Rect::new(
        r.right() - pad - CLEAR + gap().min(4.0),
        r.y + (r.h - CLEAR) / 2.0,
        CLEAR,
        CLEAR,
    )
}

/// Whether a press at `(x, y)` is on the ✖ (only while there is text to clear).
pub fn clear_hit(r: Rect, pad: f32, field: &Field, x: f32, y: f32) -> bool {
    !field.is_empty() && clear_rect(r, pad).contains(x, y)
}

/// How one search field looks.
pub struct Spec<'a> {
    pub pad: f32,
    pub size: FontSize,
    pub placeholder: &'a str,
    pub focused: bool,
    /// The well (the palette's row is the surface itself).
    pub boxed: bool,
    pub icon: &'a str,
}

impl<'a> Spec<'a> {
    pub fn new(focused: bool) -> Spec<'a> {
        Spec {
            pad: Space::X2.px(),
            size: FontSize::Small,
            placeholder: PLACEHOLDER,
            focused,
            boxed: true,
            icon: "search",
        }
    }
}

/// Paints the field: well, 2px ring while focused, magnifier, text, and the ✖ when there is
/// text. Returns the ✖'s box when shown.
pub fn paint(pen: &mut Pen, r: Rect, field: &Field, spec: &Spec) -> Option<Rect> {
    let c = pen.c;
    if spec.boxed {
        pen.round(r, c.field, RADIUS);
        if spec.focused {
            pen.outline(r, c.ring, RADIUS, RING);
        }
    }
    let lens = Rect::new(
        r.x + spec.pad,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    );
    pen.icon(spec.icon, lens, c.ink);
    field.paint(
        pen,
        text_rect(r, spec.pad),
        FieldLook {
            size: spec.size,
            weight: Weight::Regular,
            ink: c.ink_strong,
            placeholder: spec.placeholder,
            placeholder_ink: c.ink_on_toggle,
            focused: spec.focused,
        },
    );
    if field.is_empty() {
        return None;
    }
    let x = clear_rect(r, spec.pad);
    let mark = Layout::ICON_SIZE.min(CLEAR - 4.0);
    pen.icon(
        "close",
        Rect::new(
            x.x + (CLEAR - mark) / 2.0,
            x.y + (CLEAR - mark) / 2.0,
            mark,
            mark,
        ),
        c.ink,
    );
    Some(x)
}

/// Draws `text` with the part `query` matches in bold (600), in the same colour: the match is
/// told by weight alone. Returns the text's width.
pub fn paint_matched(
    pen: &mut Pen,
    text: &str,
    query: &str,
    size: FontSize,
    weight: Weight,
    ink: gpui::Hsla,
    r: Rect,
) -> f32 {
    use crate::draw::Align;
    let Some((a, b)) = matched(text, query) else {
        return pen.text(text, size, weight, ink, r, Align::Left);
    };
    let (pre, mid, post) = (&text[..a], &text[a..b], &text[b..]);
    let mut x = r.x;
    for (part, w) in [(pre, weight), (mid, Weight::Semibold), (post, weight)] {
        if part.is_empty() {
            continue;
        }
        let width = pen.measure(part, size, w);
        pen.text(part, size, w, ink, Rect::new(x, r.y, r.w, r.h), Align::Left);
        x += width;
    }
    x - r.x
}

/// The byte range of `text` that `query` matches without case, when it does.
pub fn matched(text: &str, query: &str) -> Option<(usize, usize)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    // A case change that moves the bytes (rare letters) is not mapped back: no bold.
    if lower.len() != text.len() {
        return None;
    }
    let at = lower.find(&q)?;
    let end = at + q.len();
    (text.is_char_boundary(at) && text.is_char_boundary(end)).then_some((at, end))
}

/// What the screen reader is told about a search: the match count (never drawn).
pub fn count_info(matches: usize) -> String {
    if matches == 0 {
        NO_MATCH.to_string()
    } else {
        format!("{matches} 件")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::FieldId;
    use crate::settings::field::Field;

    fn field(s: &str) -> Field {
        Field::new(FieldId::Search, s.to_string())
    }

    #[test]
    fn the_text_room_does_not_move_when_the_clear_shows() {
        let r = Rect::new(10.0, 4.0, 200.0, HEIGHT);
        let a = text_rect(r, Space::X2.px());
        // The room is a function of the box alone, not of the text.
        assert_eq!(a, text_rect(r, Space::X2.px()));
        assert!(clear_rect(r, Space::X2.px()).x >= a.right());
        assert!(clear_rect(r, Space::X2.px()).right() <= r.right());
    }

    #[test]
    fn the_match_is_found_without_case() {
        assert_eq!(matched("Compressor", "PRE"), Some((3, 6)));
        assert_eq!(matched("Compressor", "xyz"), None);
        assert_eq!(matched("Compressor", "press"), Some((3, 8)));
        assert_eq!(matched("Reverb", ""), None);
        assert_eq!(matched("Reverb", "  "), None);
    }

    #[test]
    fn the_clear_hits_only_with_text() {
        let r = Rect::new(0.0, 0.0, 200.0, HEIGHT);
        let x = clear_rect(r, Space::X2.px());
        let (cx, cy) = (x.x + 2.0, x.y + 2.0);
        assert!(!clear_hit(r, Space::X2.px(), &field(""), cx, cy));
        assert!(clear_hit(r, Space::X2.px(), &field("a"), cx, cy));
        assert!(!clear_hit(r, Space::X2.px(), &field("a"), 5.0, 5.0));
    }

    #[test]
    fn the_numbers_are_the_approved_ones() {
        assert_eq!(HEIGHT, 28.0);
        assert_eq!(RING, 2.0);
        assert_eq!(NO_MATCH, "見つかりません");
        assert_eq!(CLEAR_NAME, "クリア");
        assert_eq!(count_info(0), "見つかりません");
        assert_eq!(count_info(3), "3 件");
    }
}
