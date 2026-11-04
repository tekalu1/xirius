//! The channel editor's face (MIX-042): a floating panel with the plain bar, the toolbar and
//! the flow, cut to its body.

use xirius_core::model::{Plugin, Track};
use xirius_design::{FontSize, LineState, Radius, Weight};

use super::flow::{CardKind, Column, Flow, Gap, Key};
use super::keys;
use super::{
    ChannelWin, Grab, Part, Tool, View, card_parts, col_label, head_parts, head_shown, mode_text,
};
use crate::draw::{Align, Pen};
use crate::mixer::flow::{Pick, level_t, level_text};
use crate::studio::{Geometry, Rect, Studio};
use crate::title_bar;

/// The level's rail and its cap, as the mixer's.
const RAIL: f32 = 3.0;
const CAP_W: f32 = 4.0;
const CAP_H: f32 = 10.0;
/// The ring round a bypassed device's dot.
const RING: f32 = 1.5;
/// A crossover's line.
const CROSS_W: f32 = 2.0;
const CROSS_H: f32 = 14.0;

/// Paints the open channel editor: over the device windows when it was the last pressed,
/// under them when not (`over` says which pass this is).
pub fn paint(pen: &mut Pen, s: &Studio, g: &Geometry, over: bool) {
    let Some(w) = &s.channel else {
        return;
    };
    if w.top != over {
        return;
    }
    let Some(v) = s.channel_view(g) else {
        return;
    };
    let p = s.project();
    let Some(t) = p.tracks.get(&w.track) else {
        return;
    };
    let c = pen.c;
    pen.floating(v.r, c.paper, Radius::M);
    let (preset, modified) = s.chain_presets.tag(t);
    title_bar::paint(
        pen,
        v.bar_rect(),
        &title_bar::View {
            track: &v.name,
            device: "",
            icon: "route",
            powered: true,
            preset: &preset,
            modified,
            steps: s.chain_presets.steps(t),
            pinned: w.pinned,
            popout: false,
            keyed: false,
            seats: title_bar::Seats::default(),
        },
    );
    paint_tools(pen, w, &v);
    if let Some(m) = &v.macros {
        paint_macros(pen, s, w, t, m);
    }
    pen.mask(v.body, |pen| paint_flow(pen, s, w, t, &v.flow));
    if let Some(k) = &v.keys {
        paint_keys(pen, w, t, k);
    }
}

/// The key range figure: the chosen layer's header, the boxes over the map and the keyboard.
fn paint_keys(pen: &mut Pen, w: &ChannelWin, t: &Track, k: &keys::Keys) {
    let c = pen.c;
    pen.fill(Rect::new(k.panel.x, k.panel.y, k.panel.w, 1.0), c.line[0]);
    // The header: the chosen layer's chip, name, transpose and ranges.
    if let Some(b) = k.boxes.get(k.chosen) {
        let h = k.head;
        let cy = h.y + h.h / 2.0;
        let mut x = h.x;
        let chip = Rect::new(x, cy - super::CHIP / 2.0, super::CHIP, super::CHIP);
        // design-lint: allow(exception-family) 層の色の印（層はトラックの色の族で見分ける。v2 s04 の鍵域の図）
        pen.round(chip, c.tracks[b.index % c.tracks.len()], Radius::Clip);
        x += super::CHIP + 6.0;
        let name = keys::layer_name(t, b.index);
        let name_w = pen.text(
            &name,
            FontSize::Body,
            Weight::Semibold,
            c.ink_strong,
            Rect::new(x, h.y, 200.0, h.h),
            Align::Left,
        );
        // Transpose: its label, then the field it is dragged up and down in.
        if let Some((f, v)) = k.transpose {
            pen.text(
                "Transpose",
                FontSize::Small,
                Weight::Regular,
                c.ink,
                Rect::new(f.x - keys::LABEL_GAP - 120.0, h.y, 120.0, h.h),
                Align::Right,
            );
            pen.round(f, c.well, Radius::Clip);
            let inner = Rect::new(f.x + keys::FIELD_PAD, f.y, f.w - 2.0 * keys::FIELD_PAD, f.h);
            pen.text(
                &keys::transpose_text(v),
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                inner,
                Align::Right,
            );
            x = f.right() + keys::ITEM_GAP;
        } else {
            x += name_w + keys::ITEM_GAP;
        }
        for (label, value) in [
            ("Range", keys::keys_text(b.ranges.0)),
            ("Velocity", keys::velocity_text(b.ranges.1)),
        ] {
            x += pen.text(
                label,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                Rect::new(x, h.y, 120.0, h.h),
                Align::Left,
            ) + 6.0;
            x += pen.text(
                &value,
                FontSize::Small,
                Weight::Semibold,
                c.ink_strong,
                Rect::new(x, h.y, 120.0, h.h),
                Align::Left,
            ) + 16.0;
        }
    }
    // The map: a line at each C, then the boxes, the chosen one over the rest.
    let m = k.map;
    pen.round(m, c.well, Radius::Xs);
    let (first, n) = k.span;
    for key in (first as u16 + 1..(first + n) as u16).filter(|key| key % 12 == 0) {
        let x = keys::key_x(m, k.span, f32::from(key));
        pen.fill(Rect::new(x, m.y, 1.0, m.h), c.line[0]);
    }
    let mut order: Vec<&keys::LayerBox> = k.boxes.iter().collect();
    order.sort_by_key(|b| b.index == k.chosen);
    let mut labels: Vec<Rect> = Vec::new();
    for b in order.iter().rev() {
        // Each box's label under the ones above it that it would cover.
        let mut at = Rect::new(b.rect.x, b.rect.y, b.rect.w, 18.0);
        while labels
            .iter()
            .any(|l| l.y == at.y && l.x < at.right() && at.x < l.right())
        {
            at.y += 18.0;
        }
        labels.push(at);
    }
    labels.reverse();
    // A box past the map's ends while its side is dragged there is cut at them.
    let near = Rect::new(
        m.x - keys::GRIP_T,
        m.y - keys::GRIP_T,
        m.w + 2.0 * keys::GRIP_T,
        m.h + 2.0 * keys::GRIP_T,
    );
    pen.mask(near, |pen| {
        for (b, label) in order.iter().zip(&labels) {
            let chosen = b.index == k.chosen;
            let hot = matches!(w.hot, Some(Part::Range(id) | Part::Side(id, _)) if id == b.id);
            let (line, width) = if chosen {
                (c.ring, 1.5)
            } else if hot {
                (c.line[1], 1.0)
            } else {
                (c.line[0], 1.0)
            };
            pen.outline(b.rect, line, Radius::Xs, width);
            if label.bottom() <= b.rect.bottom() {
                pen.mask(b.rect, |pen| {
                    let cy = label.y + label.h / 2.0;
                    let mut x = label.x + 6.0;
                    let chip = Rect::new(x, cy - 4.0, 8.0, 8.0);
                    // design-lint: allow(exception-family) 層の色の印（層はトラックの色の族で見分ける。v2 s04 の鍵域の図）
                    pen.round(chip, c.tracks[b.index % c.tracks.len()], Radius::Clip);
                    x += 12.0;
                    x += pen.text(
                        &keys::layer_name(t, b.index),
                        FontSize::Small,
                        Weight::Semibold,
                        c.ink_strong,
                        Rect::new(x, label.y, (label.right() - x).max(0.0), label.h),
                        Align::Left,
                    ) + 6.0;
                    let ranges = format!(
                        "{} · {}",
                        keys::keys_text(b.ranges.0).replace(' ', ""),
                        keys::velocity_text(b.ranges.1).replace(' ', "")
                    );
                    pen.text(
                        &ranges,
                        FontSize::Small,
                        Weight::Regular,
                        c.ink,
                        Rect::new(x, label.y, (label.right() - x).max(0.0), label.h),
                        Align::Left,
                    );
                });
            }
            if chosen {
                for e in keys::Edge::ALL {
                    pen.round(keys::grip(b, e), c.ring, Radius::Pill);
                }
            }
        }
    });
    // The keyboard, with each C named.
    let (whites, blacks) = keys::key_rects(k.keyboard, k.span);
    pen.fill(k.keyboard, c.line[0]);
    for (_, r) in &whites {
        pen.round(
            Rect::new(r.x + 0.5, r.y, (r.w - 1.0).max(0.0), r.h - 1.0),
            c.key_white,
            Radius::Clip,
        );
    }
    for (key, r) in &whites {
        if key % 12 == 0 {
            pen.text(
                &xirius_params::pitch::name(*key),
                FontSize::Small,
                Weight::Regular,
                c.key_label,
                Rect::new(r.x + 2.0, r.bottom() - 16.0, 40.0, 16.0),
                Align::Left,
            );
        }
    }
    for (_, r) in &blacks {
        pen.round(*r, c.key_black, Radius::Clip);
    }
}

impl View {
    /// The bar's band.
    pub fn bar_rect(&self) -> Rect {
        Rect::new(self.r.x, self.r.y, self.r.w, title_bar::BAR_H)
    }
}

fn paint_tools(pen: &mut Pen, w: &ChannelWin, v: &View) {
    let c = pen.c;
    pen.fill(v.toolbar, c.surface_1);
    for (tool, r) in &v.tools {
        let hot = w.hot == Some(Part::Tool(*tool));
        let on = *tool == Tool::Macros && w.macros;
        let (face, ink) = if on {
            (Some(c.sel), c.ink_strong)
        } else if hot {
            (Some(c.surface_3), c.ink_strong)
        } else {
            (None, c.ink_strong)
        };
        if let Some(face) = face {
            pen.round(*r, face, Radius::Clip);
        }
        let icon = super::TBTN_ICON;
        if *tool == Tool::Popout {
            pen.icon(
                tool.icon(),
                Rect::new(
                    r.x + (r.w - icon) / 2.0,
                    r.y + (r.h - icon) / 2.0,
                    icon,
                    icon,
                ),
                ink,
            );
            continue;
        }
        let ix = r.x + super::TBTN_PAD;
        pen.icon(
            tool.icon(),
            Rect::new(ix, r.y + (r.h - icon) / 2.0, icon, icon),
            ink,
        );
        let tx = ix + icon + super::TBTN_GAP;
        pen.text(
            tool.label(),
            FontSize::Body,
            Weight::Regular,
            ink,
            Rect::new(tx, r.y, (r.right() - tx).max(0.0), r.h),
            Align::Left,
        );
    }
}

/// The macro row: the band under the toolbar, Macro 1–8 and the button to the macro window.
fn paint_macros(pen: &mut Pen, s: &Studio, w: &ChannelWin, t: &Track, m: &super::MacroRow) {
    let c = pen.c;
    pen.fill(m.band, c.surface_1);
    for (i, cell) in m.cells.iter().enumerate() {
        let k = i as u8;
        crate::macro_pad::paint::paint_knob(
            pen,
            *cell,
            crate::macro_pad::knob_value(s, t, k),
            &t.control_label(xirius_core::controls::ControlId::Knob(k)),
            w.hot == Some(Part::Macro(k)),
        );
    }
    if w.hot == Some(Part::MacroOut) {
        pen.round(m.popout, c.surface_3, Radius::Clip);
    }
    let icon = super::TBTN_ICON;
    let r = m.popout;
    pen.icon(
        "popout",
        Rect::new(
            r.x + (r.w - icon) / 2.0,
            r.y + (r.h - icon) / 2.0,
            icon,
            icon,
        ),
        c.ink_strong,
    );
}

/// The key of what the pointer is on, for the links that light with it.
fn hot_key(w: &ChannelWin) -> Option<Key> {
    match w.hot? {
        Part::Card(pick) | Part::Dot(pick) | Part::More(pick) => Some(Key::of(pick)),
        Part::Mode(id) => Some(Key::Node(id)),
        Part::Level(id) | Part::Mute(id) | Part::Remove(id) => Some(Key::Node(id)),
        _ => None,
    }
}

fn paint_flow(pen: &mut Pen, s: &Studio, w: &ChannelWin, t: &Track, f: &Flow) {
    let c = pen.c;
    let hot = hot_key(w);
    let chosen = w.chosen.map(Key::of);
    for state in [LineState::Rest, LineState::Hover, LineState::Selected] {
        for link in &f.links {
            let lit = match state {
                LineState::Rest => true,
                LineState::Hover => hot.is_some_and(|k| link.keys.contains(&k)),
                LineState::Selected => chosen.is_some_and(|k| link.keys.contains(&k)),
            };
            if lit {
                pen.structure(&link.pieces, state);
            }
        }
    }
    for io in &f.ios {
        pen.round(io.rect, c.surface_2, Radius::Pill);
        let ix = io.rect.x + super::flow::IO_PAD;
        let icon = super::flow::IO_ICON;
        pen.icon(
            io.icon,
            Rect::new(ix, io.rect.y + (io.rect.h - icon) / 2.0, icon, icon),
            c.ink,
        );
        let tx = ix + icon + super::flow::IO_GAP;
        pen.text(
            &io.text,
            FontSize::Small,
            Weight::Regular,
            c.ink_strong,
            Rect::new(tx, io.rect.y, (io.rect.right() - tx).max(0.0), io.rect.h),
            Align::Left,
        );
    }
    let held = s
        .mixer
        .level_drag
        .filter(|d| d.track == t.id)
        .map(|d| d.node);
    for col in &f.columns {
        paint_head(pen, s, w, t, f, col, held);
    }
    for cross in &f.crosses {
        let grabbed = matches!(
            &w.grab,
            Some(Grab::Cross { split, i, .. }) if *split == cross.split && *i == cross.i
        );
        let lit = grabbed || w.hot == Some(Part::Cross(cross.split, cross.i));
        let r = cross.rect;
        pen.round(
            Rect::new(
                r.x + (r.w - CROSS_W) / 2.0,
                r.y + (r.h - CROSS_H) / 2.0,
                CROSS_W,
                CROSS_H,
            ),
            if lit { c.ring } else { c.line[2] },
            Radius::Pill,
        );
        if let Some(Grab::Cross { now, .. }) = &w.grab
            && grabbed
            && let Some(hz) = now.get(cross.i)
        {
            let text = if *hz >= 1000.0 {
                format!("{:.1}k", hz / 1000.0)
            } else {
                format!("{hz:.0}")
            };
            pen.digits(
                &text,
                FontSize::Micro,
                Weight::Regular,
                c.ink_strong,
                Rect::new(r.x - 40.0, r.y - r.h, r.w + 80.0, r.h),
                Align::Center,
            );
        }
    }
    let dragged = match &w.grab {
        Some(Grab::Card {
            pick, moved: true, ..
        }) => Some(*pick),
        _ => None,
    };
    for card in &f.cards {
        paint_card(pen, s, w, t, card, dragged == Some(card.pick));
    }
    for (i, gap) in f.gaps.iter().enumerate() {
        if !gap_shown(s, w, t, f, gap) {
            continue;
        }
        let lit = w.hot == Some(Part::Gap(i));
        let (face, ink) = if lit {
            c.primary
        } else {
            (c.surface_3, c.ink_strong)
        };
        let r = gap.button;
        pen.floating(r, face, Radius::Pill);
        let i = (r.w * 0.625).round();
        pen.icon(
            "plus",
            Rect::new(r.x + (r.w - i) / 2.0, r.y + (r.h - i) / 2.0, i, i),
            ink,
        );
    }
    if let Some(Grab::Card {
        moved: true,
        to: Some(slot),
        ..
    }) = &w.grab
    {
        pen.round(slot.mark, c.ring, Radius::Pill);
    }
}

/// Does `gap` show its ＋: the pointer is in its room or the chosen card is right before or
/// after it, and no card or head's level is held (review 3 低12).
pub(crate) fn gap_shown(s: &Studio, w: &ChannelWin, t: &Track, f: &Flow, gap: &Gap) -> bool {
    let near = w.pointer.is_some_and(|(x, y)| gap.rect.contains(x, y))
        || w.chosen.is_some_and(|pick| f.beside(pick, gap));
    let held = s.mixer.level_drag.is_some_and(|d| d.track == t.id);
    near && w.grab.is_none() && !held
}

#[allow(clippy::too_many_arguments)]
fn paint_head(
    pen: &mut Pen,
    s: &Studio,
    w: &ChannelWin,
    t: &Track,
    f: &Flow,
    col: &Column,
    held: Option<xirius_core::id::NodeId>,
) {
    let c = pen.c;
    let Some(target) = s.row_target(t.id, col.id) else {
        return;
    };
    let db = s
        .mixer
        .level_drag
        .filter(|d| d.track == t.id && d.node == col.id)
        .map_or(target.db, |d| d.value);
    let hp = head_parts(col);
    if !head_shown(w, col, f, held) {
        // At rest a head keeps only what is away from the default: M, a level, a band.
        let keep = if target.mute || db != 0.0 {
            Some(level_text((db, target.mute)))
        } else {
            col.band.clone()
        };
        if let Some(text) = keep {
            pen.digits(
                &text,
                FontSize::Micro,
                Weight::Regular,
                c.ink,
                hp.keep,
                Align::Right,
            );
        }
        return;
    }
    pen.text(
        &col_label(col),
        FontSize::Caption,
        Weight::Semibold,
        c.ink_strong,
        hp.num,
        Align::Center,
    );
    let bar = hp.level;
    let mid = bar.y + bar.h / 2.0;
    let rail = Rect::new(bar.x, mid - RAIL / 2.0, bar.w, RAIL);
    pen.round(rail, c.well, Radius::Pill);
    let x = bar.x + bar.w * level_t(db);
    pen.round(
        Rect::new(bar.x, rail.y, x - bar.x, RAIL),
        c.ring,
        Radius::Pill,
    );
    pen.round(
        Rect::new(x - CAP_W / 2.0, mid - CAP_H / 2.0, CAP_W, CAP_H),
        c.ring,
        Radius::Pill,
    );
    pen.digits(
        &level_text((db, false)),
        FontSize::Micro,
        Weight::Regular,
        c.ink_strong,
        hp.value,
        Align::Right,
    );
    let (face, ink) = if target.mute {
        c.mute_on
    } else {
        (c.surface_2, c.ink)
    };
    pen.round(hp.mute, face, Radius::Clip);
    pen.text(
        "M",
        FontSize::Micro,
        Weight::Semibold,
        ink,
        hp.mute,
        Align::Center,
    );
    if let Some(r) = hp.remove {
        if w.hot == Some(Part::Remove(col.id)) {
            pen.round(r, c.surface_3, Radius::Clip);
        }
        let i = 12.0;
        pen.icon(
            "close",
            Rect::new(r.x + (r.w - i) / 2.0, r.y + (r.h - i) / 2.0, i, i),
            c.ink,
        );
    }
}

fn plugin_of(t: &Track, pick: Pick) -> Option<&Plugin> {
    match pick {
        Pick::Plugin(id) => t.inserts.iter().find(|p| p.id == id),
        Pick::Split(_) => None,
    }
}

fn paint_card(
    pen: &mut Pen,
    s: &Studio,
    w: &ChannelWin,
    t: &Track,
    card: &super::flow::Card,
    dragged: bool,
) {
    let c = pen.c;
    let r = card.rect;
    let chosen = w.chosen == Some(card.pick);
    let hot = match w.hot {
        Some(Part::Card(p) | Part::Dot(p) | Part::More(p)) => p == card.pick,
        Some(Part::Mode(id)) => card.pick == Pick::Split(id),
        _ => false,
    };
    let (face, ink, soft) = if chosen {
        (c.primary.0, c.primary.1, c.primary.1)
    } else if hot {
        (c.surface_3, c.ink_strong, c.ink)
    } else {
        (c.surface_1, c.ink_strong, c.ink)
    };
    let face = if dragged { face.opacity(0.5) } else { face };
    pen.round(r, face, Radius::S);
    let cp = card_parts(card);
    match card.kind {
        CardKind::Split { mode } => {
            let name = pen.fit("Splitter", FontSize::Small, Weight::Regular, cp.name.w);
            pen.text(
                &name,
                FontSize::Small,
                Weight::Regular,
                ink,
                cp.name,
                Align::Left,
            );
            if let Some(pill) = cp.mode {
                let lit = matches!(w.hot, Some(Part::Mode(id)) if Pick::Split(id) == card.pick);
                let pill_face = if chosen {
                    c.primary_hover.0
                } else if lit {
                    c.hover
                } else {
                    c.surface_2
                };
                pen.round(pill, pill_face, Radius::Pill);
                let tx = pill.x + 8.0;
                let tw = pen.text(
                    mode_text(mode),
                    FontSize::Small,
                    Weight::Regular,
                    ink,
                    Rect::new(tx, pill.y, (pill.right() - 18.0 - tx).max(0.0), pill.h),
                    Align::Left,
                );
                pen.icon(
                    "chevron-down",
                    Rect::new(tx + tw + 2.0, pill.y + (pill.h - 12.0) / 2.0, 12.0, 12.0),
                    soft,
                );
            }
        }
        _ => {
            let Some(p) = plugin_of(t, card.pick) else {
                return;
            };
            if let Some(d) = cp.dot {
                let glyph = Rect::new(d.x + 2.0, d.y + 2.0, 12.0, 12.0);
                let dot = Rect::new(d.x + 4.0, d.y + 4.0, 8.0, 8.0);
                if s.health.crashed(p.id) {
                    pen.icon("alert", glyph, if chosen { c.primary.1 } else { c.alert });
                } else if p.deactivated {
                    pen.icon("power", glyph, soft);
                } else if p.bypass {
                    pen.outline(dot, c.line[0], Radius::Pill, RING);
                } else {
                    pen.round(dot, c.solo_on.0, Radius::Pill);
                }
            }
            if let (Some(chip), CardKind::Instrument { layer: Some(i) }) = (cp.chip, card.kind) {
                pen.round(chip, c.tracks[i % c.tracks.len()], Radius::Clip);
            }
            let name = pen.fit(&p.name, FontSize::Small, Weight::Regular, cp.name.w);
            if p.bypass || p.deactivated {
                pen.struck(
                    &name,
                    FontSize::Small,
                    Weight::Regular,
                    soft,
                    cp.name,
                    Align::Left,
                );
            } else {
                pen.text(
                    &name,
                    FontSize::Small,
                    Weight::Regular,
                    ink,
                    cp.name,
                    Align::Left,
                );
            }
        }
    }
    if hot || chosen {
        if w.hot == Some(Part::More(card.pick)) && !chosen {
            pen.round(cp.more, c.hover, Radius::Clip);
        }
        let i = 12.0;
        let m = cp.more;
        pen.icon(
            "more",
            Rect::new(m.x + (m.w - i) / 2.0, m.y + (m.h - i) / 2.0, i, i),
            soft,
        );
    }
}
