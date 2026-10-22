//! ショートカット, キーボード: the keyboard drawn with the keys in use, the categories, and
//! the list of commands with their keys (KEY-009〜024).

use std::collections::BTreeMap;

use xirius_design::{FontSize, Layout, Radius, Weight};
use xirius_prefs::Settings;
use xirius_prefs::catalog::{self, Cat};
use xirius_prefs::chord::{ALT, MOD, Os, SHIFT};
use xirius_prefs::keymap::{Issue, Row};
use xirius_prefs::{Chord, ext};

use super::macros::{self, MacroRow};
use super::parts::{self, Seg};
use super::scene::{self, B};
use super::{Do, FieldId, MenuKind, SettingsUi};
use crate::draw::Align;
use crate::studio::Rect;

/// A key's unit width and the gap between keys.
const U: f32 = 32.0;
const G: f32 = 4.0;
const KEY_H: f32 = 30.0;

#[derive(Clone, Copy)]
enum K {
    /// Room, in units.
    Gap(f32),
    /// A key: its name (as [`Chord`] keeps it), what is printed on it, its width in units.
    Key(&'static str, &'static str, f32),
    /// A modifier: its bit, its label, its width.
    Mod(u8, &'static str, f32),
    /// A key that gives no shortcut (Caps, Win).
    Inert(&'static str, f32),
}

const fn k(name: &'static str) -> K {
    K::Key(name, name, 1.0)
}

const ROWS: [&[K]; 6] = [
    &[
        K::Key("Escape", "Esc", 1.0),
        K::Gap(0.5),
        k("F1"),
        k("F2"),
        k("F3"),
        k("F4"),
        K::Gap(0.5),
        k("F5"),
        k("F6"),
        k("F7"),
        k("F8"),
        K::Gap(0.5),
        k("F9"),
        k("F10"),
        k("F11"),
        k("F12"),
        K::Gap(0.5),
        K::Key("Delete", "Del", 1.0),
    ],
    &[
        k("`"),
        k("1"),
        k("2"),
        k("3"),
        k("4"),
        k("5"),
        k("6"),
        k("7"),
        k("8"),
        k("9"),
        k("0"),
        k("-"),
        k("="),
        K::Key("Backspace", "⌫", 2.0),
    ],
    &[
        K::Key("Tab", "Tab", 1.5),
        k("Q"),
        k("W"),
        k("E"),
        k("R"),
        k("T"),
        k("Y"),
        k("U"),
        k("I"),
        k("O"),
        k("P"),
        k("["),
        k("]"),
        K::Key("\\", "\\", 1.5),
    ],
    &[
        K::Inert("Caps", 1.75),
        k("A"),
        k("S"),
        k("D"),
        k("F"),
        k("G"),
        k("H"),
        k("J"),
        k("K"),
        k("L"),
        k(";"),
        k("'"),
        K::Key("Enter", "Enter", 2.25),
    ],
    &[
        K::Mod(SHIFT, "Shift", 2.25),
        k("Z"),
        k("X"),
        k("C"),
        k("V"),
        k("B"),
        k("N"),
        k("M"),
        k(","),
        k("."),
        k("/"),
        K::Mod(SHIFT, "Shift", 2.75),
    ],
    &[
        K::Mod(MOD, "Ctrl", 1.5),
        K::Inert("Win", 1.25),
        K::Mod(ALT, "Alt", 1.5),
        K::Key("Space", "", 6.0),
        K::Mod(ALT, "Alt", 1.5),
        K::Mod(MOD, "Ctrl", 1.5),
        K::Gap(0.5),
        K::Key("Left", "←", 1.0),
        K::Key("Up", "↑", 1.0),
        K::Key("Down", "↓", 1.0),
        K::Key("Right", "→", 1.0),
    ],
];

fn key_w(w: f32) -> f32 {
    w * U + (w - 1.0) * G
}

/// How wide the drawn keyboard is.
pub fn keyboard_w() -> f32 {
    ROWS.iter()
        .map(|row| {
            row.iter()
                .map(|k| match k {
                    K::Gap(w) => w * U,
                    K::Key(_, _, w) | K::Mod(_, _, w) | K::Inert(_, w) => key_w(*w),
                })
                .sum::<f32>()
                + G * (row.len() - 1) as f32
        })
        .fold(0.0, f32::max)
}

/// The colour of a category.
fn cat_color(b: &B, cat: Cat) -> gpui::Hsla {
    let i = Cat::ALL.iter().position(|c| *c == cat).unwrap_or(0);
    b.c.cat[i]
}

fn ctx_label(name: &str) -> &str {
    catalog::context(name).map(|c| c.label).unwrap_or(name)
}

/// Does the row show under the search and the category picked?
fn matches(ui: &SettingsUi, r: &Row, label: &str) -> bool {
    if ui.legend.is_some_and(|c| c != r.cat) {
        return false;
    }
    if ui.key_mode {
        return ui.q_key.as_ref().is_none_or(|q| r.keys.contains(q));
    }
    let q = ui.query.trim().to_lowercase();
    q.is_empty() || label.to_lowercase().contains(&q) || r.command.to_lowercase().contains(&q)
}

/// The modifiers the drawn keyboard shows: held, or pressed on it.
fn mods(ui: &SettingsUi) -> u8 {
    ui.held | ui.clicked_mods
}

pub fn keys(
    b: &mut B,
    ui: &SettingsUi,
    s: &Settings,
    registry: &xirius_core::command::Registry,
    x: f32,
    top: f32,
    w: f32,
) -> f32 {
    let c = b.c;
    let os = Os::current();
    let rows = s.keymap.rows();
    let labels: Vec<String> = rows
        .iter()
        .map(|r| match xirius_prefs::macros::name_of_line(&r.command) {
            Some(name) => name,
            None => ext::command_label(&r.command, registry),
        })
        .collect();
    // The keys in use with the modifiers held: key → (name, context, category).
    let held = mods(ui);
    let mut bound: BTreeMap<&str, Vec<(String, Cat)>> = BTreeMap::new();
    for (r, label) in rows.iter().zip(&labels) {
        if ui.legend.is_some_and(|c| c != r.cat) {
            continue;
        }
        for kc in &r.keys {
            if kc.mods == held {
                bound
                    .entry(kc.key.as_str())
                    .or_default()
                    .push((format!("{label}（{}）", ctx_label(&r.context)), r.cat));
            }
        }
    }
    let hit = ui.q_key.as_ref().map(|q| q.key.as_str());
    let mut y = top + 6.0;
    for (ri, row) in ROWS.iter().enumerate() {
        let mut kx = x;
        for key in row.iter() {
            match *key {
                K::Gap(gw) => kx += gw * U,
                K::Inert(label, kw) => {
                    let r = Rect::new(kx, y, key_w(kw), KEY_H);
                    b.round(r, super::faces(c).well, Radius::S);
                    b.text(
                        label,
                        FontSize::Micro,
                        Weight::Regular,
                        c.ink,
                        r,
                        Align::Center,
                    );
                    kx = r.right();
                }
                K::Mod(bit, label, kw) => {
                    let r = Rect::new(kx, y, key_w(kw), KEY_H);
                    let on = held & bit != 0;
                    let face = if on {
                        c.fill
                    } else if b.hot(r) {
                        c.thumb
                    } else {
                        super::faces(c).well
                    };
                    b.round(r, face, Radius::S);
                    b.text(
                        label,
                        FontSize::Micro,
                        Weight::Regular,
                        if on { c.on_fill } else { c.ink },
                        r,
                        Align::Center,
                    );
                    b.item(r, Do::KbMod(bit), None);
                    kx = r.right();
                }
                K::Key(name, label, kw) => {
                    let r = Rect::new(kx, y, key_w(kw), KEY_H);
                    let on = bound.get(name);
                    let face = if b.hot(r) {
                        c.thumb
                    } else if on.is_some() {
                        c.surface_3
                    } else {
                        super::faces(c).well
                    };
                    if hit == Some(name) {
                        b.ring(r, c.ink_blue, face, Radius::S, 1.5);
                    } else {
                        b.round(r, face, Radius::S);
                    }
                    let (ink, weight) = match on {
                        Some(_) => (c.ink_strong, Weight::Semibold),
                        None => (c.ink, Weight::Regular),
                    };
                    b.text(label, FontSize::Micro, weight, ink, r, Align::Center);
                    let tip = on.map(|v| {
                        let dot = cat_color(b, v[0].1);
                        b.round(Rect::new(r.x + 4.0, r.y + 4.0, 5.0, 5.0), dot, Radius::Pill);
                        v.iter()
                            .map(|(t, _)| t.as_str())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    });
                    b.item(r, Do::KbKey(name.to_string()), tip);
                    kx = r.right();
                }
            }
            kx += G;
        }
        y += KEY_H + G + if ri == 0 { 4.0 } else { 0.0 };
    }
    let kb_bottom = y - G;
    // The categories, beside the keyboard (under it when there is no room).
    let lw = Cat::ALL
        .iter()
        .map(|c| scene::width(c.label(), FontSize::Caption))
        .fold(0.0, f32::max)
        + 6.0
        + 8.0
        + 8.0
        + 8.0;
    let (mut lx, mut ly) = (x + keyboard_w() + 16.0, top + 6.0 + 2.0);
    if lx + lw > x + w {
        lx = x;
        ly = kb_bottom + 8.0;
    }
    for cat in Cat::ALL {
        let r = Rect::new(lx, ly, lw, 22.0);
        let on = ui.legend == Some(cat);
        if on {
            b.round(r, c.sel, Radius::S);
        } else if b.hot(r) {
            b.round(r, c.hover, Radius::S);
        }
        let dot = cat_color(b, cat);
        b.round(Rect::new(r.x + 6.0, r.y + 7.0, 8.0, 8.0), dot, Radius::Pill);
        b.text(
            cat.label(),
            FontSize::Caption,
            Weight::Regular,
            if on { c.ink_strong } else { c.ink },
            Rect::new(r.x + 22.0, r.y, r.w - 26.0, r.h),
            Align::Left,
        );
        b.item(r, Do::Legend(cat), None);
        ly += 22.0 + 6.0;
    }
    y = kb_bottom.max(ly - 6.0) + 4.0;
    // The list, by context.
    let issues = s.keymap.conflicts(os);
    let (group_bottom, mut shown) =
        macros::group(b, ui, s, registry, &rows, &labels, &issues, os, x, y, w);
    y = group_bottom;
    for cx in catalog::CONTEXTS {
        let here: Vec<(&Row, &String)> = rows
            .iter()
            .zip(&labels)
            .filter(|(r, l)| {
                r.context == cx.name && !macros::is_group_row(s, r) && matches(ui, r, l)
            })
            .collect();
        if here.is_empty() {
            continue;
        }
        shown += here.len();
        y += 18.0 - 4.0;
        let hw = b.text(
            cx.label,
            FontSize::Caption,
            Weight::Semibold,
            c.ink,
            Rect::new(x + 6.0, y, w, 18.0),
            Align::Left,
        );
        b.digits(
            &here.len().to_string(),
            FontSize::Micro,
            Weight::Regular,
            c.ink,
            Rect::new(x + 6.0 + hw + 8.0, y + 1.0, 40.0, 18.0),
            Align::Left,
        );
        y += 18.0 + 4.0;
        for (r, label) in here {
            y = row(b, ui, r, label, &rows, &labels, &issues, os, x, y, w, None);
        }
    }
    if shown == 0 {
        let text = match (&ui.q_key, ui.key_mode) {
            (Some(q), true) => format!("{} は割り当てられていません", q.display(os)),
            _ => "見つかりません".to_string(),
        };
        b.text(
            &text,
            FontSize::Small,
            Weight::Regular,
            c.ink,
            Rect::new(x + 6.0, y + 12.0, w, 24.0),
            Align::Left,
        );
        y += 48.0;
    }
    y
}

/// One command: its name, its keys and what is wrong with them.
#[allow(clippy::too_many_arguments)]
pub(super) fn row(
    b: &mut B,
    ui: &SettingsUi,
    r: &Row,
    label: &str,
    rows: &[Row],
    labels: &[String],
    issues: &[Issue],
    os: Os,
    x: f32,
    y: f32,
    w: f32,
    mac: Option<&MacroRow>,
) -> f32 {
    let c = b.c;
    let fc = super::faces(c);
    let line = Rect::new(x, y, w, 36.0);
    if b.hot(line) {
        b.round(line, c.hover, Radius::S);
    }
    // A macro's row has no preset to go back to: what it can do is let go of its key.
    let changed = match mac {
        Some(_) => !r.keys.is_empty(),
        None => r.changed(),
    };
    let mk = Rect::new(line.x + 6.0 + 2.0, line.y + 15.0, 6.0, 6.0);
    if changed && mac.is_none() {
        b.round(mk, c.ink_blue, Radius::Pill);
        b.tip(Rect::new(line.x, line.y, 18.0, line.h), "変更済み");
    }
    // From the end: the restore button, the keys, then what is wrong.
    let rs = Rect::new(line.right() - 4.0 - 26.0 + 2.0, line.y + 7.0, 22.0, 22.0);
    if changed {
        parts::icon_button(
            b,
            rs,
            "restore",
            Do::Restore {
                ctx: r.context.clone(),
                cmd: r.command.clone(),
            },
            Some(if mac.is_some() {
                "キーを外す"
            } else {
                "プリセットに戻す"
            }),
        );
    }
    let mut ex = rs.x - 2.0 - 8.0;
    let capturing = ui
        .capture
        .as_ref()
        .filter(|(cx, cm, _)| *cx == r.context && *cm == r.command);
    let conflict_of = |k: &Chord| {
        issues.iter().find_map(|i| match i {
            Issue::Conflict {
                context,
                chord,
                commands,
            } if *context == r.context && chord == k && commands.contains(&r.command) => {
                commands.iter().find(|x| **x != r.command).cloned()
            }
            _ => None,
        })
    };
    let other_label = |cmd: &str| {
        rows.iter()
            .zip(labels)
            .find(|(o, _)| o.command == cmd && o.context == r.context)
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| cmd.to_string())
    };
    let mut chips: Vec<Option<Chord>> = r.keys.iter().cloned().map(Some).collect();
    if chips.is_empty() {
        chips.push(None);
    }
    let mut first_conflict: Option<(Chord, String)> = None;
    for key in chips.iter().rev() {
        let act = Do::Capture {
            ctx: r.context.clone(),
            cmd: r.command.clone(),
            from: key.clone(),
        };
        let waiting = capturing.is_some_and(|(_, _, f)| f == key);
        if waiting {
            let text = "キーを入力";
            let cw = (8.0 + 14.0 + 4.0 + scene::width(text, FontSize::Caption) + 8.0).max(96.0);
            let cr = Rect::new(ex - cw, line.y + 6.0, cw, 24.0);
            b.ring(cr, c.ink_blue, fc.well, Radius::S, 1.5);
            let pulse = (ui.t() * 2.0).fract() < 0.5;
            let tx = cr.x + (cw - 18.0 - scene::width(text, FontSize::Caption)) / 2.0;
            b.icon(
                "kbd",
                parts::small_icon_at(tx, cr),
                if pulse { c.ink_blue } else { c.ink },
            );
            b.text(
                text,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(tx + 18.0, cr.y, cw, cr.h),
                Align::Left,
            );
            b.item(cr, act, None);
            ex = cr.x - 6.0;
            continue;
        }
        match key {
            None => {
                let cr = Rect::new(ex - 34.0, line.y + 6.0, 34.0, 24.0);
                if b.hot(cr) {
                    b.round(cr, c.surface_3, Radius::S);
                }
                b.icon(
                    "plus",
                    Rect::new(cr.x + 10.0, cr.y + 5.0, 14.0, 14.0),
                    c.ink,
                );
                b.item(cr, act, Some("キーを割り当て".into()));
                ex = cr.x - 6.0;
            }
            Some(k) => {
                let text = k.display(os);
                let other = conflict_of(k);
                let tw = scene::width(&text, FontSize::Caption);
                let iw = if other.is_some() { 14.0 + 4.0 } else { 0.0 };
                let cw = (8.0 + iw + tw + 8.0).max(34.0);
                let cr = Rect::new(ex - cw, line.y + 6.0, cw, 24.0);
                let hot = b.hot(cr);
                b.round(cr, if hot { c.thumb } else { c.surface_3 }, Radius::S);
                let tx = cr.x + (cw - iw - tw) / 2.0;
                if other.is_some() {
                    b.icon("alert", parts::small_icon_at(tx, cr), c.alert);
                }
                b.text(
                    &text,
                    FontSize::Caption,
                    Weight::Semibold,
                    if other.is_some() {
                        c.ink_alert
                    } else {
                        c.ink_strong
                    },
                    Rect::new(tx + iw, cr.y, tw + 2.0, cr.h),
                    Align::Left,
                );
                b.item(cr, act, None);
                ex = cr.x - 6.0;
                if let Some(o) = other {
                    first_conflict = Some((k.clone(), o));
                }
            }
        }
    }
    // What is wrong, before the keys.
    if capturing.is_none() {
        if let Some((key, other)) = first_conflict {
            let name = other_label(&other);
            let ub = Rect::new(ex - 22.0, line.y + 7.0, 22.0, 22.0);
            parts::icon_button(
                b,
                ub,
                "unlink",
                Do::Unlink {
                    ctx: r.context.clone(),
                    cmd: r.command.clone(),
                    key: key.clone(),
                },
                Some(&format!("「{name}」のキーを外す")),
            );
            let sb = Rect::new(ub.x - 2.0 - 22.0, ub.y, 22.0, 22.0);
            parts::icon_button(
                b,
                sb,
                "swap",
                Do::Swap {
                    ctx: r.context.clone(),
                    cmd: r.command.clone(),
                    key,
                },
                Some("入れ替え"),
            );
            let t = format!("「{name}」と重複");
            let tw = scene::width(&t, FontSize::Caption);
            b.text(
                &t,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(sb.x - 6.0 - tw, line.y, tw + 2.0, line.h),
                Align::Left,
            );
            ex = sb.x - 6.0 - tw - 6.0;
        } else {
            for i in issues {
                match i {
                    Issue::Reserved {
                        context, command, ..
                    } if *context == r.context && *command == r.command => {
                        let wr = Rect::new(ex - 18.0, line.y + 11.0, 14.0, 14.0);
                        b.icon("lock", wr, c.ink);
                        b.tip(wr, "OS が使用するキーです");
                        ex = wr.x - 6.0;
                    }
                    Issue::Shadow {
                        context,
                        command,
                        hidden_context,
                        hidden,
                        ..
                    } if *context == r.context && *command == r.command => {
                        let hl = rows
                            .iter()
                            .zip(labels)
                            .find(|(o, _)| o.command == *hidden && o.context == *hidden_context)
                            .map(|(_, l)| l.clone())
                            .unwrap_or_else(|| hidden.clone());
                        let wr = Rect::new(ex - 18.0, line.y + 11.0, 14.0, 14.0);
                        b.icon("layers", wr, c.ink);
                        b.tip(
                            wr,
                            &format!(
                                "「{}」の「{hl}」より優先されます",
                                ctx_label(hidden_context)
                            ),
                        );
                        ex = wr.x - 6.0;
                    }
                    _ => {}
                }
            }
        }
    }
    // The name, and for a macro its steps after it; the command's own name is the tooltip.
    let nx = line.x + 6.0 + 10.0 + 8.0;
    let room = (ex - 8.0 - nx).max(0.0);
    if let Some(m) = mac {
        let open = ui.macro_open.as_deref() == Some(m.name);
        b.icon(
            if open {
                "chevron-down"
            } else {
                "chevron-right"
            },
            parts::small_icon_at(line.x + 6.0, line),
            c.ink,
        );
        b.item(
            Rect::new(line.x, line.y, (ex - 8.0 - line.x).max(0.0), line.h),
            Do::MacroOpen(m.name.to_string()),
            None,
        );
    }
    b.push_mask(Rect::new(nx, line.y, room, line.h));
    let nw = b.text(
        label,
        FontSize::Small,
        Weight::Medium,
        c.ink_strong,
        Rect::new(nx, line.y, room, line.h),
        Align::Left,
    );
    match mac {
        Some(m) => {
            b.text(
                &format!("{} 手順", m.count),
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(
                    nx + nw + 10.0,
                    line.y + 1.0,
                    (room - nw - 10.0).max(0.0),
                    line.h,
                ),
                Align::Left,
            );
        }
        None => b.tip(Rect::new(nx, line.y, nw.min(room), line.h), &r.command),
    }
    b.pop_mask();
    line.bottom()
}

/// The header's end on キーボード: the preset, the search and その他, up to `right`.
pub fn header(b: &mut B, ui: &SettingsUi, s: &Settings, left: f32, mid: f32, right: f32) {
    let c = b.c;
    let fc = super::faces(c);
    let os = Os::current();
    let more = Rect::new(
        right - Layout::ICON_BUTTON,
        mid - Layout::ICON_BUTTON / 2.0,
        Layout::ICON_BUTTON,
        Layout::ICON_BUTTON,
    );
    parts::icon_button(b, more, "more", Do::Menu(MenuKind::More), Some("その他"));
    let preset = catalog::preset(&s.keymap.preset)
        .map(|p| p.label)
        .unwrap_or(&s.keymap.preset);
    let pw = scene::width(preset, FontSize::Small) + 32.0;
    let room = more.x - 6.0 - left;
    let qw = 260f32.min(room - pw - 6.0).max(120.0);
    let q = Rect::new(more.x - 6.0 - qw, mid - 15.0, qw, 30.0);
    let pr = Rect::new(q.x - 6.0 - pw, mid - 14.0, pw, 28.0);
    b.field(pr, preset, true);
    let n = s.keymap.changed_rows();
    let tip = if n > 0 {
        format!("プリセット（変更 {n} 件）")
    } else {
        "プリセット".to_string()
    };
    b.item(pr, Do::Menu(MenuKind::Preset), Some(tip));
    // The search: by the key pressed, or by name.
    let typing = ui.field.as_ref().filter(|f| f.id == FieldId::Search);
    if typing.is_some() {
        b.ring(q, c.ink_blue, fc.well, Radius::S, 1.5);
    } else {
        b.round(q, fc.well, Radius::S);
    }
    b.icon(
        if ui.key_mode { "kbd" } else { "search" },
        parts::small_icon_at(q.x + 10.0, q),
        c.ink,
    );
    let segs = [
        Seg {
            label: "",
            icon: Some("kbd"),
            act: Do::KeyMode(true),
            on: ui.key_mode,
            tip: Some("キーで検索"),
        },
        Seg {
            label: "",
            icon: Some("search"),
            act: Do::KeyMode(false),
            on: !ui.key_mode,
            tip: Some("名前で検索"),
        },
    ];
    let sw = 24.0 * 2.0 + 2.0 + 2.0;
    let sr = Rect::new(q.right() - 3.0 - sw, q.y + 3.0, sw, 24.0);
    let tx = q.x + 10.0 + 14.0 + 6.0;
    // A press on the box: the name search takes the keyboard.
    b.item(
        Rect::new(q.x, q.y, sr.x - q.x, q.h),
        if ui.key_mode {
            Do::Nothing
        } else {
            Do::Field(FieldId::Search)
        },
        None,
    );
    let filled = if ui.key_mode {
        ui.q_key.is_some()
    } else {
        !ui.query.is_empty() || typing.is_some_and(|f| !f.text().is_empty())
    };
    let mut end = sr.x - 6.0;
    if filled {
        let cr = Rect::new(end - 22.0, q.y + 4.0, 22.0, 22.0);
        parts::icon_button(b, cr, "close", Do::ClearQuery, Some("クリア"));
        end = cr.x - 4.0;
    }
    let tr = Rect::new(tx, q.y, (end - tx).max(0.0), q.h);
    b.push_mask(tr);
    match (ui.key_mode, typing) {
        (true, _) => match &ui.q_key {
            Some(k) => {
                b.text(
                    &k.display(os),
                    FontSize::Small,
                    Weight::Regular,
                    c.ink_strong,
                    tr,
                    Align::Left,
                );
            }
            None => {
                let held = mods(ui);
                let shown = if held == 0 {
                    "キーを押す".to_string()
                } else {
                    format!(
                        "{}+",
                        Chord::new(held, "A")
                            .display(os)
                            .trim_end_matches('A')
                            .trim_end_matches('+')
                    )
                };
                b.text(
                    &shown,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    tr,
                    Align::Left,
                );
            }
        },
        (false, Some(f)) => {
            let text = f.text();
            let tw = if text.is_empty() {
                b.text(
                    "名前で検索",
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    tr,
                    Align::Left,
                );
                0.0
            } else {
                b.text(
                    &text,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink_strong,
                    tr,
                    Align::Left,
                )
            };
            b.fill(
                Rect::new(tr.x + tw + 1.0, q.y + 8.0, 1.5, q.h - 16.0),
                c.ink_strong,
            );
            b.scene.field = Some(Rect::new(tr.x, q.y, tw.max(1.0), q.h));
        }
        (false, None) => {
            if ui.query.is_empty() {
                b.text(
                    "名前で検索",
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    tr,
                    Align::Left,
                );
            } else {
                b.text(
                    &ui.query,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink_strong,
                    tr,
                    Align::Left,
                );
            }
        }
    }
    b.pop_mask();
    parts::seg_with(b, sr.x - 1.0, sr.y - 1.0, &segs, 22.0, 1.0, 24.0, 4.0);
}
