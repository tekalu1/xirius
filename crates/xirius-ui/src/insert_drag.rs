//! A strip's insert heading dragged (MIX-045). Held and moved 4 px (10 with a pen or a
//! finger), a copy of the insert rows follows the pointer; let go on another strip or on a
//! track's head in the arrangement, that channel's inserts become the same plug-ins and
//! splitters in the same state, in one step
//! (`channel.inserts.paste --from`). With Shift they go after the ones it has. The source and a
//! frozen track take nothing: the copy dims and the pointer says so.

use serde_json::json;
use xirius_core::id::TrackId;
use xirius_design::{FontSize, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::mixer::flow::{self, Area, Kind};
use crate::mixer::{MixerHit, ROW_H, WIDE_W};
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tree::RowKind;

/// How far the pointer goes before a press on the heading is a drag: further for a pen or a
/// finger, which tremble as they lift (UI-326).
const SLOP: f32 = 4.0;
const SLOP_COARSE: f32 = 10.0;
/// The copy that follows the pointer: its width (the name drag's), its room inside and the
/// rows it shows.
const GHOST_W: f32 = 120.0;
const GHOST_PAD: f32 = 3.0;
const GHOST_ROWS: usize = 6;
/// The ring round where the inserts go, and the line where Shift puts them.
const RING_W: f32 = 2.0;
const LINE_H: f32 = 2.0;
/// The line keeps this far inside the ring, so the two read apart.
const LINE_GAP: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub from: TrackId,
    start: (f32, f32),
    pub at: (f32, f32),
    pub moved: bool,
    /// Shift is held: the inserts go after the target's own.
    pub append: bool,
}

/// Where a release lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drop {
    pub to: TrackId,
    /// On a strip (else a track's head in the arrangement).
    pub strip: bool,
    /// Whether the channel takes the inserts: not the source, not frozen.
    pub takes: bool,
}

impl Studio {
    /// A press on a strip's insert heading starts the drag; the press goes on selecting the
    /// channel and putting the keys on the heading.
    pub fn insert_drag_down(&mut self, g: &Geometry, x: f32, y: f32) {
        self.mixer.insert_drag = None;
        if self.popup.is_some() {
            return;
        }
        if let Hit::Mixer(MixerHit::Heading(t, Area::Insert)) = self.hit(g, x, y) {
            self.mixer.insert_drag = Some(Drag {
                from: t,
                start: (x, y),
                at: (x, y),
                moved: false,
                append: false,
            });
        }
    }

    pub fn insert_drag_move(&mut self, x: f32, y: f32, shift: bool) -> bool {
        let Some(d) = &mut self.mixer.insert_drag else {
            return false;
        };
        d.at = (x, y);
        d.append = shift;
        let slop = if self.coarse_press { SLOP_COARSE } else { SLOP };
        let began = !d.moved && (x - d.start.0).hypot(y - d.start.1) >= slop;
        d.moved |= began;
        let moved = d.moved;
        if began {
            // a new drag: what the last one said goes, and nothing else
            self.untell("channel.inserts.paste");
        }
        moved
    }

    /// The release: on a channel that takes them, the inserts go there. True when it was a
    /// drag (so the press is not a click as well).
    pub fn insert_drag_up(&mut self, g: &Geometry, x: f32, y: f32, shift: bool) -> bool {
        let Some(d) = self.mixer.insert_drag.take() else {
            return false;
        };
        if !d.moved {
            return false;
        }
        match self.insert_drop(g, d.from, x, y) {
            Some(Drop {
                to, takes: true, ..
            }) => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                // a refusal says why, as any failed command does
                self.arrange_run(
                    "channel.inserts.paste",
                    json!({ "ids": [to.to_string()], "from": d.from.to_string(), "append": shift }),
                );
                self.rebuild(g);
            }
            _ => self.untell("channel.inserts.paste"),
        }
        true
    }

    /// Where a release of `from`'s inserts at a point lands: a strip, or a track's head in
    /// the arrangement.
    pub fn insert_drop(&self, g: &Geometry, from: TrackId, x: f32, y: f32) -> Option<Drop> {
        let p = self.project();
        let strip = (self.mixer_shown() && g.drawer_body.contains(x, y))
            .then(|| {
                self.mixer_strips(g)
                    .into_iter()
                    .find(|s| self.strip_holds(g, s, x, y))
            })
            .flatten();
        let (to, on_strip) = match strip {
            Some(s) => (s.track, true),
            None if g.column.contains(x, y) => {
                let RowKind::Track(t) = self.tree().row_at(y - g.column.y + self.scroll.1)?.kind
                else {
                    return None;
                };
                (t, false)
            }
            None => return None,
        };
        let track = p.tracks.get(&to)?;
        Some(Drop {
            to,
            strip: on_strip,
            takes: to != from && track.frozen.is_none(),
        })
    }

    /// The drag is over a channel that does not take the inserts (the pointer says no).
    pub fn insert_drag_refused(&self, g: &Geometry) -> bool {
        self.mixer
            .insert_drag
            .filter(|d| d.moved)
            .and_then(|d| self.insert_drop(g, d.from, d.at.0, d.at.1))
            .is_some_and(|d| !d.takes)
    }

    /// The insert area of `t`'s strip on screen, with where inserts after the last go.
    fn insert_panel(&self, g: &Geometry, t: TrackId) -> Option<(Rect, Rect, f32)> {
        let s = self.mixer_strips(g).into_iter().find(|s| s.track == t)?;
        let p = self.project();
        let flow = self.strip_flow(&s, p.tracks.get(&t)?);
        let panel = flow
            .panels
            .iter()
            .find(|(a, _)| *a == Area::Insert)
            .map(|(_, r)| *r)?;
        Some((s.inserts, panel, flow.insert_end()?))
    }
}

/// The ring round the channel a release reaches, the line where Shift puts the inserts, and
/// the copy following the pointer (dim over a channel that does not take them).
pub fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let Some(d) = s.mixer.insert_drag.filter(|d| d.moved) else {
        return;
    };
    let c = pen.c;
    let drop = s.insert_drop(g, d.from, d.at.0, d.at.1);
    match drop {
        Some(Drop {
            to,
            strip: true,
            takes: true,
        }) => {
            if let Some((view, panel, end)) = s.insert_panel(g, to) {
                pen.mask(view, |pen| {
                    pen.outline(panel, c.ring, Radius::S, RING_W);
                    if d.append {
                        let x = panel.x + GHOST_PAD;
                        let y =
                            (end - LINE_H / 2.0).min(panel.bottom() - RING_W - LINE_GAP - LINE_H);
                        pen.round(
                            Rect::new(x, y, panel.right() - GHOST_PAD - x, LINE_H),
                            c.ring,
                            Radius::Pill,
                        );
                    }
                });
            }
        }
        Some(Drop {
            to,
            strip: false,
            takes: true,
        }) => {
            if let Some(row) = s.tree().row_of(RowKind::Track(to)) {
                let r = Rect::new(g.column.x, s.row_y(g, row), g.column.w, row.h);
                pen.mask(g.column, |pen| {
                    pen.outline(r, c.ring, Radius::S, RING_W);
                    if d.append {
                        let x = r.x + GHOST_PAD;
                        pen.round(
                            Rect::new(
                                x,
                                r.bottom() - RING_W - LINE_GAP - LINE_H,
                                r.w - GHOST_PAD * 2.0,
                                LINE_H,
                            ),
                            c.ring,
                            Radius::Pill,
                        );
                    }
                });
            }
        }
        _ => {}
    }
    ghost(pen, s, g, d, drop.is_some_and(|d| !d.takes));
}

/// The insert rows of the dragged channel, under the pointer: a heading when it has none.
fn ghost(pen: &mut Pen, s: &Studio, g: &Geometry, d: Drag, refused: bool) {
    let c = pen.c;
    let p = s.project();
    let Some(track) = p.tracks.get(&d.from) else {
        return;
    };
    // the rows as the source strip shows them, at the width it has now
    let wide = s.width_of(g, d.from) >= WIDE_W;
    let rows: Vec<_> = flow::lines(track, &s.mixer.folds, wide)
        .into_iter()
        .filter(|l| l.area == Area::Insert && !matches!(l.kind, Kind::Head { .. }))
        .collect();
    let shown = rows.len().clamp(1, GHOST_ROWS);
    let h = shown as f32 * ROW_H + GHOST_PAD * 2.0;
    let r = Rect::new(
        (d.at.0 + Space::X3.px()).min(g.size.0 - GHOST_W - Space::X3.px()),
        (d.at.1 + Space::X3.px()).min(g.size.1 - h - Space::X1.px()),
        GHOST_W,
        h,
    );
    // dim over a channel that does not take them: the lower face and the softer ink
    let (face, ink) = if refused {
        (c.surface_2, c.ink)
    } else {
        (c.surface_3, c.ink_strong)
    };
    pen.round(r, face, Radius::S);
    let row = |i: usize| {
        Rect::new(
            r.x + GHOST_PAD,
            r.y + GHOST_PAD + i as f32 * ROW_H,
            r.w - GHOST_PAD * 2.0,
            ROW_H,
        )
    };
    if rows.is_empty() {
        pen.text(
            "インサート",
            FontSize::Micro,
            Weight::Regular,
            ink,
            row(0),
            Align::Left,
        );
        return;
    }
    for (i, l) in rows.iter().take(GHOST_ROWS).enumerate() {
        let at = row(i);
        let x = at.x + f32::from(l.depth) * Space::X4.px();
        let name = match l.kind {
            Kind::Split { .. } => "Splitter".to_string(),
            _ => flow::row_name(track, l),
        };
        let w = (at.right() - x).max(0.0);
        let name = if i + 1 == GHOST_ROWS && rows.len() > GHOST_ROWS {
            "…".to_string()
        } else {
            pen.fit(&name, FontSize::Small, Weight::Regular, w)
        };
        pen.text(
            &name,
            FontSize::Small,
            Weight::Regular,
            ink,
            Rect::new(x, at.y, w, at.h),
            Align::Left,
        );
    }
}

#[cfg(test)]
#[path = "insert_drag_tests.rs"]
mod tests;
