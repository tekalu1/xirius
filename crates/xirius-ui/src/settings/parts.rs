//! The parts the settings are made of: a heading, a row, a switch, a segmented control,
//! a button, a number field. Sizes follow the approved settings layout (`docs/ui-approved.md`).

use gpui::{Bounds, Corners, point, px, size};
use xirius_design::{FontSize, Layout, Radius, Weight};

use super::scene::{self, B};
use super::{Do, FieldId, SettingsUi};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// A row's least height, and the gap under a heading.
pub const ROW_H: f32 = 40.0;
/// The width of the label column on オーディオ (ドライバー, オーディオデバイス).
pub const LABEL_W: f32 = 150.0;

/// A 16px icon at `x`, in the middle of `row` from top to bottom.
pub fn icon_at(x: f32, row: Rect) -> Rect {
    let s = Layout::ICON_SIZE;
    Rect::new(x, row.y + (row.h - s) / 2.0, s, s)
}

/// A 14px icon at `x`, in the middle of `row`.
pub fn small_icon_at(x: f32, row: Rect) -> Rect {
    Rect::new(x, row.y + (row.h - 14.0) / 2.0, 14.0, 14.0)
}

/// The heading of a group (外観, AI ...); gives the y under it.
pub fn gh(b: &mut B, text: &str, x: f32, y: f32) -> f32 {
    let r = Rect::new(x, y + 18.0, 400.0, 16.0);
    b.text(
        text,
        FontSize::Caption,
        Weight::Semibold,
        b.c.ink,
        r,
        Align::Left,
    );
    r.bottom() + 4.0
}

/// A row's label: an icon (when given) and the text. `tip` shows over the text.
pub fn label(b: &mut B, r: Rect, icon: Option<&'static str>, text: &str, tip: Option<&str>) {
    let c = b.c;
    let mut x = r.x;
    if let Some(i) = icon {
        b.icon(i, icon_at(x, r), c.ink);
        x += Layout::ICON_SIZE + 10.0;
    }
    let w = b.text(
        text,
        FontSize::Small,
        Weight::Regular,
        c.ink_strong,
        Rect::new(x, r.y, r.right() - x, r.h),
        Align::Left,
    );
    if let Some(t) = tip {
        b.tip(Rect::new(x, r.y + 8.0, w, r.h - 16.0), t);
    }
}

/// The on/off switch, its right end at `right`, in the middle of `row`.
pub fn toggle(b: &mut B, right: f32, row: Rect, on: bool, act: Do, tip: Option<&str>) -> Rect {
    let c = b.c;
    let r = Rect::new(right - 34.0, row.y + (row.h - 20.0) / 2.0, 34.0, 20.0);
    let hot = b.hot(r);
    let (face, knob) = if on {
        (if hot { c.fill_hover } else { c.fill }, c.on_fill)
    } else {
        c.switch_off[usize::from(hot)]
    };
    b.round(r, face, Radius::Pill);
    let kx = if on {
        r.right() - 3.0 - 14.0
    } else {
        r.x + 3.0
    };
    b.round(Rect::new(kx, r.y + 3.0, 14.0, 14.0), knob, Radius::Pill);
    b.item_pick(
        r,
        act,
        tip.map(str::to_string),
        Some(super::scene::Pick::Switch(on)),
    );
    r
}

/// One choice of a segmented control.
pub struct Seg<'a> {
    pub label: &'a str,
    pub icon: Option<&'static str>,
    pub act: Do,
    pub on: bool,
    pub tip: Option<&'a str>,
}

/// The segmented control: a tray, the choice that holds raised. Gives its box.
pub fn seg(b: &mut B, x: f32, y: f32, items: &[(&str, Do, bool)], tips: Option<&[&str]>) -> Rect {
    let segs: Vec<Seg> = items
        .iter()
        .enumerate()
        .map(|(i, (label, act, on))| Seg {
            label,
            icon: None,
            act: act.clone(),
            on: *on,
            tip: tips
                .and_then(|t| t.get(i))
                .copied()
                .filter(|t| !t.is_empty()),
        })
        .collect();
    seg_with(b, x, y, &segs, 24.0, 2.0, 30.0, 8.0)
}

/// [`seg`] with its sizes: a button's height, the tray's padding, a button's least width
/// and its sides.
#[allow(clippy::too_many_arguments)]
pub fn seg_with(
    b: &mut B,
    x: f32,
    y: f32,
    items: &[Seg],
    h: f32,
    pad: f32,
    min_w: f32,
    side: f32,
) -> Rect {
    let c = b.c;
    let widths: Vec<f32> = items
        .iter()
        .map(|s| {
            let text = if s.label.is_empty() {
                0.0
            } else {
                scene::width(s.label, FontSize::Caption)
            };
            let icon = if s.icon.is_some() { 14.0 } else { 0.0 };
            let gap = if icon > 0.0 && text > 0.0 { 4.0 } else { 0.0 };
            (text + icon + gap + side * 2.0).max(min_w)
        })
        .collect();
    let w = widths.iter().sum::<f32>() + 2.0 * (items.len().saturating_sub(1)) as f32 + pad * 2.0;
    let tray = Rect::new(x, y, w, h + pad * 2.0);
    b.round(tray, c.surface_2, Radius::S);
    let mut bx = x + pad;
    for (s, bw) in items.iter().zip(widths) {
        let r = Rect::new(bx, y + pad, bw, h);
        let hot = b.hot(r);
        if s.on {
            b.round(r, c.paper, Radius::S);
        } else if hot {
            b.round(r, c.hover, Radius::S);
        }
        let ink = if s.on || hot { c.ink_strong } else { c.ink };
        match (s.icon, s.label.is_empty()) {
            (Some(i), true) => b.icon(
                i,
                Rect::new(r.x + (bw - 14.0) / 2.0, r.y + (h - 14.0) / 2.0, 14.0, 14.0),
                ink,
            ),
            (icon, _) => {
                let tw = scene::width(s.label, FontSize::Caption);
                let iw = if icon.is_some() { 18.0 } else { 0.0 };
                let tx = r.x + (bw - tw - iw) / 2.0;
                if let Some(i) = icon {
                    b.icon(i, small_icon_at(tx, r), ink);
                }
                b.text(
                    s.label,
                    FontSize::Caption,
                    if s.on {
                        Weight::Medium
                    } else {
                        Weight::Regular
                    },
                    ink,
                    Rect::new(tx + iw, r.y, tw + 2.0, r.h),
                    Align::Left,
                );
            }
        }
        b.item_pick(
            r,
            s.act.clone(),
            s.tip.map(str::to_string),
            Some(super::scene::Pick::Choice(s.on)),
        );
        bx += bw + 2.0;
    }
    tray
}

/// A small button with an icon and a word (再スキャン, フォルダーを追加, 取り消す).
pub fn button(b: &mut B, r: Rect, icon: &'static str, text: &str, act: Do) {
    let c = b.c;
    let hot = b.hot(r);
    b.round(r, if hot { c.thumb } else { c.surface_3 }, Radius::S);
    b.icon(icon, small_icon_at(r.x + 10.0, r), c.ink_strong);
    b.text(
        text,
        FontSize::Small,
        Weight::Regular,
        c.ink_strong,
        Rect::new(r.x + 10.0 + 14.0 + 6.0, r.y, r.w - 30.0, r.h),
        Align::Left,
    );
    b.item(r, act, None);
}

/// The width of a [`button`] with `text`.
pub fn button_w(text: &str) -> f32 {
    10.0 + 14.0 + 6.0 + scene::width(text, FontSize::Small) + 10.0
}

/// A square button with an icon only.
pub fn icon_button(b: &mut B, r: Rect, icon: &'static str, act: Do, tip: Option<&str>) {
    let c = b.c;
    if b.hot(r) {
        b.round(r, c.hover, Radius::S);
    }
    let s = if r.w < 26.0 { 14.0 } else { Layout::ICON_SIZE };
    b.icon(
        icon,
        Rect::new(r.x + (r.w - s) / 2.0, r.y + (r.h - s) / 2.0, s, s),
        if b.hot(r) { c.ink_strong } else { c.ink },
    );
    b.item(r, act, tip.map(str::to_string));
}

/// A number kept in a preference, written as the field shows it.
pub fn number_text(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// A number field, its right end at `right`, and its unit after it. Gives its box.
#[allow(clippy::too_many_arguments)]
pub fn number(
    b: &mut B,
    ui: &SettingsUi,
    right: f32,
    row: Rect,
    key: &str,
    value: f64,
    unit: Option<&str>,
    tip: Option<&str>,
) -> Rect {
    let c = b.c;
    let uw = unit
        .map(|u| scene::width(u, FontSize::Caption) + 8.0)
        .unwrap_or(0.0);
    let r = Rect::new(right - uw - 72.0, row.y + (row.h - 28.0) / 2.0, 72.0, 28.0);
    let fc = super::faces(c);
    let id = FieldId::Pref(key.to_string());
    let typing = ui.field.as_ref().filter(|f| f.id == id);
    if typing.is_some() {
        b.ring(r, c.ink_blue, fc.well, Radius::S, 1.5);
    } else {
        b.round(r, fc.well, Radius::S);
    }
    let inner = Rect::new(r.x + 8.0, r.y, r.w - 16.0, r.h);
    match typing {
        Some(f) => {
            let text = f.text();
            let tw = scene::width(&text, FontSize::Small);
            b.digits(
                &text,
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                inner,
                Align::Right,
            );
            // The caret, after the text.
            b.fill(
                Rect::new(inner.right() + 1.0, r.y + 7.0, 1.5, r.h - 14.0),
                c.ink_strong,
            );
            b.scene.field = Some(Rect::new(inner.right() - tw, r.y, tw, r.h));
        }
        None => {
            b.digits(
                &number_text(value),
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                inner,
                Align::Right,
            );
        }
    }
    b.item(r, Do::Field(id), tip.map(str::to_string));
    if let Some(u) = unit {
        b.text(
            u,
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(r.right() + 8.0, row.y, uw, row.h),
            Align::Left,
        );
    }
    r
}

/// A small tag: a word on a pill (the tool on マウス, a plugin's format).
pub fn chip(b: &mut B, x: f32, row: Rect, text: &str) -> Rect {
    let c = b.c;
    let w = scene::width(text, FontSize::Micro) + 12.0;
    let r = Rect::new(x, row.y + (row.h - 16.0) / 2.0, w, 16.0);
    b.round(r, c.surface_3, Radius::Pill);
    b.text(
        text,
        FontSize::Micro,
        Weight::Semibold,
        c.ink,
        r,
        Align::Center,
    );
    r
}

/// Is `key` a modifier (not a key that gives a chord by itself)?
pub fn is_modifier(key: &str) -> bool {
    matches!(
        key,
        "shift"
            | "control"
            | "ctrl"
            | "alt"
            | "platform"
            | "cmd"
            | "super"
            | "win"
            | "fn"
            | "capslock"
    )
}

/// The tip: a small box under what the pointer is on, kept inside the window.
pub fn paint_tip(pen: &mut Pen, at: Rect, tip: &str, screen: (f32, f32)) {
    let c = pen.c;
    let tw = pen.measure(tip, FontSize::Small, Weight::Regular);
    let w = tw + 16.0;
    let h = 26.0;
    let mut y = at.bottom() + 6.0;
    if y + h > screen.1 - 4.0 {
        y = at.y - 6.0 - h;
    }
    let x = (at.x + at.w / 2.0 - w / 2.0).clamp(4.0, (screen.0 - 4.0 - w).max(4.0));
    let r = Rect::new(x, y, w, h);
    let bounds = Bounds::new(
        point(px(pen.origin.0 + r.x), px(pen.origin.1 + r.y)),
        size(px(r.w), px(r.h)),
    );
    pen.window
        .paint_drop_shadows(bounds, Corners::all(px(Radius::S.px())), &pen.c.float);
    pen.round(r, c.surface_3, Radius::S);
    pen.text(
        tip,
        FontSize::Small,
        Weight::Regular,
        c.ink_strong,
        Rect::new(r.x + 8.0, r.y, tw + 2.0, r.h),
        Align::Left,
    );
}
