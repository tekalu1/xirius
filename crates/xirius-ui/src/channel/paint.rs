//! The channel editor's face (MIX-042): a floating panel with the plain bar, the toolbar and
//! the flow, cut to its body.

use xirius_core::model::{Plugin, Track};
use xirius_design::{FontSize, LineState, Radius, Weight};

use super::flow::{CardKind, Column, Flow, Key};
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
    title_bar::paint_plain(pen, v.bar_rect(), &v.name, w.pinned);
    paint_tools(pen, w, &v);
    pen.mask(v.body, |pen| paint_flow(pen, s, w, t, &v.flow));
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
    let pointer = w.pointer;
    for (i, gap) in f.gaps.iter().enumerate() {
        if w.grab.is_some() || !pointer.is_some_and(|(x, y)| gap.rect.contains(x, y)) {
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
