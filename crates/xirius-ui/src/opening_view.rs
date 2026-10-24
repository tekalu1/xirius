//! The approved 2026-10-24 loading card (proposal B), painted through Pen.
use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};
use gpui::{accesskit, div, prelude::*, px};
use std::{cell::RefCell, rc::Rc};
use unicode_segmentation::UnicodeSegmentation;
use xirius_design::{FontSize, Layout, Radius, Weight};

pub fn card(g: &Geometry) -> Rect {
    let w = 360.0_f32.min((g.size.0 - 24.0).max(0.0));
    Rect::new((g.size.0 - w) / 2.0, (g.size.1 - 160.0) / 2.0, w, 160.0)
}
pub fn cancel_rect(g: &Geometry) -> Rect {
    let r = card(g);
    Rect::new(r.right() - 104.0, r.bottom() - 44.0, 88.0, 28.0)
}
pub fn confirm_card(g: &Geometry) -> Rect {
    let w = 330.0_f32.min((g.size.0 - 24.0).max(0.0));
    Rect::new((g.size.0 - w) / 2.0, (g.size.1 - 116.0) / 2.0, w, 116.0)
}
pub fn close_buttons(g: &Geometry) -> (Rect, Rect) {
    let r = confirm_card(g);
    (
        Rect::new(r.right() - 184.0, r.bottom() - 44.0, 48.0, 28.0),
        Rect::new(r.right() - 128.0, r.bottom() - 44.0, 112.0, 28.0),
    )
}
pub fn name_rect(g: &Geometry) -> Rect {
    let r = card(g);
    Rect::new(r.x + 16.0, r.y + 72.0, (r.w - 32.0).max(0.0), 20.0)
}
/// Remove parent components first, then the middle of the filename, preserving
/// extended grapheme clusters. Width always comes from the actual text shaper.
pub fn shorten(full: &str, width: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if width <= 0.0 {
        return String::new();
    }
    if measure(full) <= width {
        return full.into();
    }
    let normalized = full.replace('\\', "/");
    let parts: Vec<_> = normalized.split('/').collect();
    for i in 1..parts.len() {
        let candidate = format!("…/{}", parts[i..].join("/"));
        if measure(&candidate) <= width {
            return candidate;
        }
    }
    let name = parts.last().copied().unwrap_or(full);
    let gs: Vec<_> = name.graphemes(true).collect();
    for keep in (1..gs.len()).rev() {
        let left = keep.div_ceil(2);
        let candidate = format!(
            "{}…{}",
            gs[..left].concat(),
            gs[gs.len() - (keep / 2)..].concat()
        );
        if measure(&candidate) <= width {
            return candidate;
        }
    }
    if measure("…") <= width {
        "…".into()
    } else {
        String::new()
    }
}
fn text(pen: &mut Pen, s: &str, r: Rect, title: bool) {
    pen.text(
        s,
        if title {
            FontSize::Lead
        } else {
            FontSize::Small
        },
        if title {
            Weight::Medium
        } else {
            Weight::Regular
        },
        if title { pen.c.ink_strong } else { pen.c.ink },
        r,
        Align::Left,
    );
}
pub fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) -> bool {
    let Some(open) = &s.opening else {
        return false;
    };
    if !open.visible(crate::wait::now()) {
        return true;
    }
    #[cfg(feature = "loading-probe")]
    font_tests::verify_widths(pen);
    crate::cleanup_dialog::paint_veil(pen, g);
    let r = if open.confirm_close {
        confirm_card(g)
    } else {
        card(g)
    };
    if open.confirm_close {
        // The same confirmation surface, cells and two-button composition as cleanup.
        use crate::popup::{Cell, Look, PopupLayout};
        let (back, close) = close_buttons(g);
        let cells = vec![
            Cell::new(
                Rect::new(r.x + 16.0, r.y + 16.0, r.w - 32.0, 28.0),
                Look::Clean(crate::cleanup_dialog::CleanLook::Title),
            )
            .text(crate::opening::CLOSE),
            Cell::new(back, Look::Button).text("戻る"),
            Cell::new(close, Look::Danger).text("中止して閉じる"),
        ];
        crate::popup::paint(pen, &PopupLayout { rect: r, cells }, None);
        pen.outline(
            if open.close_focus { close } else { back },
            pen.c.ring,
            Radius::S,
            2.0,
        );
        return true;
    }
    pen.floating(r, pen.c.surface_2, Radius::M);
    pen.icon_turned(
        "spinner",
        Rect::new(r.x + 16.0, r.y + 23.0, 14.0, 14.0),
        pen.c.ink_strong,
        s.spin(),
    );
    text(
        pen,
        open.title(),
        Rect::new(r.x + 38.0, r.y + 16.0, r.w - 54.0, 28.0),
        true,
    );
    pen.text(
        &open.detail(),
        FontSize::Body,
        Weight::Regular,
        pen.c.ink,
        Rect::new(r.x + 16.0, r.y + 48.0, r.w - 32.0, 24.0),
        Align::Left,
    );
    let full = open.status.current.to_string_lossy();
    let nr = name_rect(g);
    let fitted = shorten(&full, nr.w, |t| {
        pen.measure(t, FontSize::Small, Weight::Regular)
    });
    text(pen, &fitted, nr, false);
    if (open.full_name || open.name_focus) && fitted != full {
        let lines = wrap(&full, nr.w, |t| {
            pen.measure(t, FontSize::Small, Weight::Regular)
        });
        let tip = Rect::new(
            nr.x,
            nr.bottom() + 4.0,
            nr.w,
            lines.len() as f32 * 20.0 + 16.0,
        );
        pen.floating(tip, pen.c.surface_3, Radius::S);
        for (i, line) in lines.iter().enumerate() {
            text(
                pen,
                line,
                Rect::new(
                    tip.x + 8.0,
                    tip.y + 8.0 + i as f32 * 20.0,
                    tip.w - 16.0,
                    20.0,
                ),
                false,
            );
        }
    }
    if !open.cancelled {
        let b = cancel_rect(g);
        if open.cancel_hover {
            pen.round(b, pen.c.surface_3, Radius::S);
        }
        pen.text(
            "中止  Esc",
            FontSize::Small,
            Weight::Regular,
            pen.c.ink,
            Rect::new(b.x + 8.0, b.y, b.w - 16.0, b.h),
            Align::Right,
        );
    }
    true
}
fn wrap(full: &str, w: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for g in full.graphemes(true) {
        let last = lines.last_mut().unwrap();
        if !last.is_empty() && measure(&format!("{last}{g}")) > (w - 16.0).max(0.0) {
            lines.push(g.into());
        } else {
            last.push_str(g);
        }
    }
    lines
}

/// Real actionable accessibility controls over the canvas, plus a polite status
/// whose contents are throttled independently from the painted filename.
pub(crate) fn accessible(s: Rc<RefCell<Studio>>, g: &Geometry) -> impl IntoElement {
    let mut root = div().id("opening-accessible").absolute();
    let borrowed = s.borrow();
    let Some(open) = &borrowed.opening else {
        return root;
    };
    if !open.visible(crate::wait::now()) {
        return root;
    }
    let r = if open.confirm_close {
        confirm_card(g)
    } else {
        card(g)
    };
    let title = if open.confirm_close {
        crate::opening::CLOSE
    } else {
        open.title()
    };
    let full = open.status.current.to_string_lossy().into_owned();
    let announce = open.announcement.clone();
    let confirmed = open.confirm_close;
    let cancelled = open.cancelled;
    root = root
        .left(px(r.x))
        .top(px(r.y + Layout::TOP_BAR_H))
        .w(px(r.w))
        .h(px(r.h))
        .role(gpui::Role::Dialog)
        .aria_label(title)
        .a11y_synthetic_children(move |tree| {
            let mut status = accesskit::Node::new(accesskit::Role::Status);
            status.set_label(announce.clone());
            status.set_live(accesskit::Live::Polite);
            tree.push_child(tree.synthetic_node_id("open-status"), status);
        });
    drop(borrowed);
    let mut buttons = Vec::new();
    if confirmed {
        let (a, b) = close_buttons(g);
        buttons.push(("戻る", a, 0));
        buttons.push(("中止して閉じる", b, 1));
    } else if !cancelled {
        buttons.push(("中止", cancel_rect(g), 2));
    }
    for (label, b, action) in buttons {
        let studio = s.clone();
        root = root.child(
            div()
                .id(label)
                .absolute()
                .left(px(b.x - r.x))
                .top(px(b.y - r.y))
                .w(px(b.w))
                .h(px(b.h))
                .role(gpui::Role::Button)
                .aria_label(label)
                .on_a11y_action(accesskit::Action::Click, move |_, window, _| {
                    let mut s = studio.borrow_mut();
                    match action {
                        1 => s.accept_open_close(),
                        2 => s.cancel_opening(),
                        _ => {
                            if let Some(o) = &mut s.opening {
                                o.confirm_close = false;
                            }
                        }
                    }
                    window.refresh();
                }),
        );
    }
    if !confirmed {
        let studio = s.clone();
        root = root.child(
            div()
                .id("opening-current-file")
                .absolute()
                .left(px(16.0))
                .top(px(72.0))
                .w(px(r.w - 32.0))
                .h(px(20.0))
                .role(gpui::Role::Label)
                .aria_label(full)
                .on_a11y_action(accesskit::Action::Focus, move |_, window, _| {
                    if let Some(open) = &mut studio.borrow_mut().opening {
                        open.name_focus = true;
                    }
                    window.refresh();
                }),
        );
    }
    root
}

#[cfg(feature = "loading-probe")]
#[path = "opening_font_tests.rs"]
mod font_tests;
