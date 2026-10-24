//! The selection of tracks (UI-340): one set shared by the track heads and the mixer, with the
//! focus (`Studio::selected_row`, the row the inspector shows) and the anchor a range is
//! measured from (`Arrange::anchor`).
//!
//! A plain press chooses one row, Ctrl adds or takes away one, Shift chooses the range from the
//! anchor and Ctrl+Shift adds that range. With the heads in focus, Shift+↑↓ widens and narrows
//! the range and Ctrl+A chooses every track.

use serde_json::{Map, Value};
use xirius_core::id::TrackId;

use crate::studio::{Geometry, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

impl Studio {
    /// The rows of the column, top to bottom (the ones inside a closed folder are not there).
    fn row_order(&self) -> Vec<RowKind> {
        self.tree().rows.iter().map(|r| r.kind).collect()
    }

    /// The selected rows that are still in the song, the focus first.
    pub fn picked_rows(&self) -> Vec<RowKind> {
        let p = self.project();
        let live = |r: &RowKind| match r {
            RowKind::Track(t) => p.tracks.contains_key(t),
            RowKind::Folder(f) => p.folders.contains_key(f),
        };
        let mut rows: Vec<RowKind> = self.selected_row.into_iter().filter(live).collect();
        for r in &self.arrange.tracks {
            if live(r) && !rows.contains(r) {
                rows.push(*r);
            }
        }
        rows
    }

    /// The row is lit as selected: in the set, or the focus.
    pub fn is_picked(&self, kind: RowKind) -> bool {
        self.selected_row == Some(kind) || self.arrange.tracks.contains(&kind)
    }

    /// The selected tracks, the focus first: a selected folder stands for the tracks in it.
    pub fn picked_tracks(&self) -> Vec<TrackId> {
        let p = self.project();
        let mut ids = Vec::new();
        for row in self.picked_rows() {
            match row {
                RowKind::Track(t) if !t.is_master() => ids.push(t),
                RowKind::Folder(f) => ids.extend(p.tracks_in(f)),
                RowKind::Track(_) => {}
            }
        }
        let mut seen = std::collections::HashSet::new();
        ids.retain(|t| p.tracks.contains_key(t) && seen.insert(*t));
        ids
    }

    /// `rows` become the selection, in the column's order, with `focus` the focus.
    fn set_picked(&mut self, rows: Vec<RowKind>, focus: RowKind) {
        let order = self.row_order();
        let place = |r: &RowKind| order.iter().position(|o| o == r).unwrap_or(usize::MAX);
        let mut rows = rows;
        rows.sort_by_key(place);
        rows.dedup();
        self.arrange.tracks = rows;
        self.select_row(Some(focus));
        self.arrange.touched();
    }

    /// The rows from `a` to `b` in the column, both ends in (just `b` when either is not shown).
    fn rows_between(&self, a: RowKind, b: RowKind) -> Vec<RowKind> {
        let order = self.row_order();
        match (
            order.iter().position(|r| *r == a),
            order.iter().position(|r| *r == b),
        ) {
            (Some(i), Some(j)) => order[i.min(j)..=i.max(j)].to_vec(),
            _ => vec![b],
        }
    }

    /// A press on a row's head or a strip's name with these modifiers: plain chooses it alone,
    /// Ctrl adds it or takes it away, Shift chooses the range from the anchor, Ctrl+Shift adds the
    /// range to the selection.
    pub fn pick_row(&mut self, kind: RowKind, mods: Mods) {
        let picked = self.picked_rows();
        if mods.shift {
            let anchor = self
                .arrange
                .anchor
                .filter(|a| self.tree().row_of(*a).is_some())
                .or(self.selected_row)
                .unwrap_or(kind);
            let mut rows = self.rows_between(anchor, kind);
            if mods.ctrl {
                rows.extend(picked);
            }
            self.set_picked(rows, kind);
            // The range is measured from the same row again next time.
            self.arrange.anchor = Some(anchor);
        } else if mods.ctrl {
            if picked.contains(&kind) {
                // The last selected row stays: the inspector always has a track to show.
                if picked.len() == 1 {
                    return;
                }
                let rest: Vec<RowKind> = picked.into_iter().filter(|r| *r != kind).collect();
                let focus = match self.selected_row {
                    Some(f) if f != kind && rest.contains(&f) => f,
                    _ => self.nearest(&rest, kind),
                };
                self.set_picked(rest, focus);
            } else {
                let mut rows = picked;
                rows.push(kind);
                self.set_picked(rows, kind);
            }
            self.arrange.anchor = Some(kind);
        } else {
            self.set_picked(vec![kind], kind);
            self.arrange.anchor = Some(kind);
        }
    }

    /// A press on a row's head, with the modifiers it was made with (`Arrange::mods`): with Ctrl or
    /// Shift it chooses as [`Studio::pick_row`] does; a plain press on a selected row moves the
    /// focus there and keeps the others, so a drag takes them all along (the click that ends
    /// without a drag chooses the row alone).
    pub fn press_row(&mut self, kind: RowKind) {
        let mods = self.arrange.mods;
        if !mods.ctrl && !mods.shift && self.is_picked(kind) {
            self.select_row(Some(kind));
        } else {
            self.pick_row(kind, mods);
        }
    }

    /// The row of `rows` nearest below `gone` in the column, else the nearest above it.
    fn nearest(&self, rows: &[RowKind], gone: RowKind) -> RowKind {
        let order = self.row_order();
        let at = order.iter().position(|r| *r == gone).unwrap_or(0);
        let place = |r: &RowKind| order.iter().position(|o| o == r);
        rows.iter()
            .copied()
            .filter_map(|r| place(&r).map(|i| (r, i)))
            .min_by_key(|(_, i)| if *i > at { (0, *i - at) } else { (1, at - *i) })
            .map(|(r, _)| r)
            .unwrap_or(rows[0])
    }

    /// ↑ or ↓ on the heads: the next row alone, or with `extend` (Shift) the range from the
    /// anchor to it, which widens and narrows as the focus moves.
    pub fn step_rows(&mut self, g: &Geometry, down: bool, extend: bool) -> bool {
        let order = self.row_order();
        if order.is_empty() {
            return false;
        }
        let at = self
            .selected_row
            .and_then(|f| order.iter().position(|r| *r == f));
        let next = match at {
            Some(i) if down => (i + 1).min(order.len() - 1),
            Some(i) => i.saturating_sub(1),
            None if down => 0,
            None => order.len() - 1,
        };
        let to = order[next];
        if extend && at.is_some() {
            self.pick_row(
                to,
                Mods {
                    shift: true,
                    ..Mods::NONE
                },
            );
        } else {
            self.pick_row(to, Mods::NONE);
        }
        self.row_into_view(g, to);
        true
    }

    /// The heads have the keys: the last press in the arrange was on a head.
    pub fn heads_focused(&self) -> bool {
        self.arrange.focus && self.arrange.heads
    }

    /// `select.step` up or down and `select.all` with the heads in focus: they choose rows. None
    /// leaves the key to the clips.
    pub(crate) fn heads_key(
        &mut self,
        g: &Geometry,
        name: &str,
        args: &Map<String, Value>,
    ) -> Option<bool> {
        if !self.heads_focused() {
            return None;
        }
        match name {
            "select.all" => Some(self.pick_all_rows()),
            "select.step" => {
                let down = match args.get("dir").and_then(Value::as_str)? {
                    "down" => true,
                    "up" => false,
                    _ => return None,
                };
                let extend = args.get("extend").and_then(Value::as_bool).unwrap_or(false);
                Some(self.step_rows(g, down, extend))
            }
            _ => None,
        }
    }

    /// Ctrl+A on the heads: every track of the column (a closed folder stands for its tracks).
    pub fn pick_all_rows(&mut self) -> bool {
        let rows: Vec<RowKind> = self
            .tree()
            .rows
            .iter()
            .filter(|r| match r.kind {
                RowKind::Track(_) => true,
                RowKind::Folder(f) => self.is_collapsed(f),
            })
            .map(|r| r.kind)
            .collect();
        let Some(first) = rows.first().copied() else {
            return false;
        };
        let focus = self
            .selected_row
            .filter(|f| rows.contains(f))
            .unwrap_or(first);
        self.set_picked(rows, focus);
        self.arrange.anchor = Some(first);
        true
    }

    /// Scrolls the column so the row shows.
    fn row_into_view(&mut self, g: &Geometry, kind: RowKind) {
        let Some(row) = self.tree().row_of(kind).cloned() else {
            return;
        };
        let bottom = row.y + row.full_h();
        if row.y < self.scroll.1 {
            self.scroll.1 = row.y;
        } else if bottom > self.scroll.1 + g.column.h {
            self.scroll.1 = bottom - g.column.h;
        }
        self.clamp_scroll(g);
    }
}

#[cfg(test)]
#[path = "track_select_tests.rs"]
mod tests;
