//! The instruments of the browser: the double click, and dragging one onto the
//! tracks. Dropped on an instrument track it becomes that track's instrument;
//! anywhere else on the tracks it starts a new track.

use serde_json::json;
use xirius_core::id::TrackId;
use xirius_core::model::TrackKind;
use xirius_design::{FontSize, Radius, Space, Weight};

use crate::browser::{BRow, BrowserHit, LibItem};
use crate::draw::{Align, Pen};
use crate::instrument::{Pick, resolve};
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tree::RowKind;

/// How far the pointer goes before a press on an instrument is a drag.
const SLOP: f32 = 4.0;

/// An instrument being dragged out of the browser.
#[derive(Clone, Debug, PartialEq)]
pub struct LibDrag {
    pub name: &'static str,
    pub icon: &'static str,
    pub from: (f32, f32),
    pub at: (f32, f32),
    /// The pointer went past [`SLOP`]: letting go drops it.
    pub moved: bool,
}

/// What letting go of a dragged instrument does there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    /// It becomes the instrument of this track.
    Replace(TrackId),
    /// A new track plays it.
    New,
    Nothing,
}

/// What a double click and a drop off the tracks do (the first row of S38).
const NEW_LABEL: &str = "新しいトラックで使う";

impl Studio {
    /// The plugin a browser item names; says so when this PC does not have it.
    fn lib_resolve(&mut self, name: &str) -> Option<Pick> {
        let pick = resolve(&self.plugin_catalog(), name);
        if pick.is_none() {
            self.error = Some(format!("「{name}」はまだ見つかっていません"));
        }
        pick
    }

    /// A new instrument track playing `name` (one step); it is selected.
    pub fn use_on_new_track(&mut self, name: &str) -> bool {
        let Some(pick) = self.lib_resolve(name) else {
            return false;
        };
        let Some(data) = self.arrange_run(
            "track.add.with-instrument",
            json!({ "instrument": pick.name, "format": pick.format, "uid": pick.uid }),
        ) else {
            return false;
        };
        if let Some(t) = data["id"].as_str().and_then(|s| s.parse().ok()) {
            self.select_row(Some(RowKind::Track(t)));
            self.begin_loading_instrument(t, name, &pick.format);
        }
        true
    }

    /// The instrument of track `t` is on its way in: the list's item shows the spinner.
    fn begin_loading_instrument(&mut self, t: TrackId, name: &str, format: &str) {
        let id = self
            .project()
            .tracks
            .get(&t)
            .and_then(|x| crate::instrument::current(x))
            .map(|x| x.id);
        if let Some(id) = id {
            self.begin_loading(name, format, id);
        }
    }

    /// `name` in place of the instrument of track `t` (one step).
    pub fn use_on_track(&mut self, t: TrackId, name: &str) -> bool {
        let Some(pick) = self.lib_resolve(name) else {
            return false;
        };
        if !self.set_instrument(t, &pick) {
            return false;
        }
        self.select_row(Some(RowKind::Track(t)));
        self.begin_loading_instrument(t, name, &pick.format);
        true
    }

    /// The instrument item at `i` of the browser's list.
    fn lib_item(&self, g: &Geometry, i: usize) -> Option<(LibItem, Rect)> {
        let l = self.browser.layout(g.panel);
        let placed = l.rows.get(i)?;
        match &placed.row {
            BRow::Item { item, .. } if !item.effect => {
                Some((*item, self.browser.row_rect(&l, placed)))
            }
            _ => None,
        }
    }

    /// After a press on the browser: an instrument item starts a drag, and
    /// a double click puts it on a new track.
    pub fn lib_grab(&mut self, g: &Geometry, hit: Hit, x: f32, y: f32, clicks: usize) -> bool {
        let Hit::Browser(BrowserHit::Item(i)) = hit else {
            return self.file_grab(g, hit, x, y, clicks);
        };
        let Some((item, _)) = self.lib_item(g, i) else {
            return false;
        };
        if clicks >= 2 {
            self.browser.drag = None;
            return self.use_on_new_track(item.name);
        }
        self.browser.drag = Some(LibDrag {
            name: item.name,
            icon: item.icon,
            from: (x, y),
            at: (x, y),
            moved: false,
        });
        true
    }

    /// The pointer moved while an instrument is held.
    pub fn lib_move(&mut self, x: f32, y: f32) -> bool {
        if self.file_move(x, y) {
            return true;
        }
        let Some(d) = &mut self.browser.drag else {
            return false;
        };
        d.at = (x, y);
        if (x - d.from.0).hypot(y - d.from.1) > SLOP {
            d.moved = true;
        }
        true
    }

    /// Letting go of a held instrument: dropped where the pointer is.
    pub fn lib_up(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.file_up(g, x, y) {
            return true;
        }
        let Some(d) = self.browser.drag.take() else {
            return false;
        };
        if d.moved {
            match self.lib_drop_at(g, x, y) {
                Drop::Replace(t) => {
                    self.use_on_track(t, d.name);
                }
                Drop::New => {
                    self.use_on_new_track(d.name);
                }
                Drop::Nothing => {}
            }
        }
        true
    }

    /// What dropping an instrument at a point does.
    pub fn lib_drop_at(&self, g: &Geometry, x: f32, y: f32) -> Drop {
        if !g.column.contains(x, y) && !g.lanes.contains(x, y) {
            return Drop::Nothing;
        }
        let tree = self.tree();
        let Some(RowKind::Track(t)) = tree.row_at(y - g.column.y + self.scroll.1).map(|r| r.kind)
        else {
            return Drop::New;
        };
        let p = self.project();
        match p.tracks.get(&t) {
            Some(track) if track.kind == TrackKind::Instrument => Drop::Replace(t),
            _ => Drop::New,
        }
    }
}

/// The dragged instrument at the pointer, with what letting go does, and the
/// track it would go to lit.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    crate::browser_pick::paint(pen, studio, g);
    let Some(d) = studio.browser.drag.as_ref().filter(|d| d.moved) else {
        return;
    };
    let c = pen.c;
    let drop = studio.lib_drop_at(g, d.at.0, d.at.1);
    let tracks = Rect::new(
        g.column.x,
        g.column.y,
        g.lanes.right() - g.column.x,
        g.column.h,
    );
    let tree = studio.tree();
    let label = match drop {
        Drop::Replace(t) => {
            if let Some(row) = tree.row_of(RowKind::Track(t)) {
                let r = Rect::new(
                    tracks.x,
                    g.column.y + row.y - studio.scroll.1,
                    tracks.w,
                    row.h,
                );
                // See-through, so the row stays readable, and ringed.
                pen.mask(tracks, |pen| {
                    pen.round(r, c.sel.opacity(0.35), Radius::S);
                    pen.fill(Rect::new(r.x, r.y, r.w, 2.0), c.ring);
                    pen.fill(Rect::new(r.x, r.bottom() - 2.0, r.w, 2.0), c.ring);
                    pen.fill(Rect::new(r.x, r.y, 2.0, r.h), c.ring);
                    pen.fill(Rect::new(r.right() - 2.0, r.y, 2.0, r.h), c.ring);
                });
            }
            let name = &studio.project().tracks[&t].name;
            format!("{name} のインストゥルメントにする")
        }
        Drop::New => {
            // Under the last row, where the new track goes.
            if let Some(last) = tree.rows.last() {
                let y = g.column.y + last.bottom() - studio.scroll.1;
                pen.mask(tracks, |pen| {
                    pen.fill(Rect::new(tracks.x, y, tracks.w, 2.0), c.ring);
                });
            }
            NEW_LABEL.to_string()
        }
        Drop::Nothing => String::new(),
    };
    chip(pen, g, d, &label);
}

/// The chip that follows the pointer: the icon and name, and under them
/// what letting go does.
fn chip(pen: &mut Pen, g: &Geometry, d: &LibDrag, label: &str) {
    let c = pen.c;
    let pad = Space::X2.px();
    let icon = 16.0;
    let name_w = pen.measure(d.name, FontSize::Small, Weight::Medium);
    let label_w = pen.measure(label, FontSize::Caption, Weight::Regular);
    let w = pad * 2.0 + icon + Space::X1.px() * 2.0 + name_w.max(label_w);
    let h = if label.is_empty() { 28.0 } else { 44.0 };
    let x = (d.at.0 + 14.0).min(g.size.0 - w - Space::X1.px());
    let y = (d.at.1 + 16.0).min(g.size.1 - h - Space::X1.px());
    let r = Rect::new(x, y, w, h);
    pen.round(r, c.surface_3, Radius::S);
    pen.icon(
        d.icon,
        Rect::new(r.x + pad, r.y + 6.0, icon, icon),
        c.ink_strong,
    );
    let tx = r.x + pad + icon + Space::X1.px() * 2.0;
    let tw = r.right() - tx - pad;
    pen.text(
        d.name,
        FontSize::Small,
        Weight::Medium,
        c.ink_strong,
        Rect::new(tx, r.y + 4.0, tw, 20.0),
        Align::Left,
    );
    if !label.is_empty() {
        pen.text(
            label,
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(tx, r.y + 22.0, tw, 18.0),
            Align::Left,
        );
    }
}

#[cfg(test)]
mod tests;
