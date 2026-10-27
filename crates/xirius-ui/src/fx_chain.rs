//! Shared insert rows for the mixer and expanded track headers.
use crate::browser::{BRow, BrowserHit};
use crate::draw::{Align, Pen};
use crate::instrument::Pick;
use crate::mixer::{List, MixerHit, ROW_H, bypass_part, list_row};
use crate::popup::{Action, Choice, PopupKind};
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tree::RowKind;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{PluginKind, Track};
use xirius_design::{FontSize, Radius, Weight};

pub const HEADER_H: f32 = 72.0;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub enum Source {
    Effect(Pick),
    Insert(TrackId, PluginId),
}
#[derive(Clone, Debug)]
pub struct Drag {
    pub source: Source,
    pub from: (f32, f32),
    pub at: (f32, f32),
    pub moved: bool,
    pub click: Option<(PopupKind, String)>,
}

/// Where an insert's name is written: from the bypass dot to the end of the row, whether or not
/// the pointer is on the row. The remove mark shows only under the pointer and lies over the tail
/// of a name that runs under it, so a strip's width is spent on the name.
pub fn name_rect(row: Rect) -> Rect {
    let dot = bypass_part(row);
    Rect::new(
        dot.right(),
        row.y,
        (row.right() - dot.right() - 2.0).max(0.0),
        row.h,
    )
}

/// Whether the preset mark shows (and can be pressed) under the pointer on a row of the insert
/// `name`: only when the name leaves it room, so a long name is not covered to be read.
pub fn preset_mark_fits(row: Rect, name: &str) -> bool {
    let room = remove_part(row).x - name_rect(row).x - 2.0;
    crate::browser::text_width(name, FontSize::Caption) + 16.0 <= room
}

pub fn remove_part(row: Rect) -> Rect {
    Rect::new(row.right() - 16.0, row.y, 16.0, row.h)
}

pub fn hit(s: &Studio, t: &Track, list: Rect, rows: usize, x: f32, y: f32) -> Option<MixerHit> {
    if !list.contains(x, y) {
        return None;
    }
    let i = ((y - list.y) / (ROW_H + 2.0)).floor() as usize;
    if i >= rows || !list_row(list, i).contains(x, y) {
        return Some(MixerHit::Strip(t.id));
    }
    let Some(k) = (s.mixer.offset(t.id, List::Inserts) + i).checked_sub(crate::instrument::gap(t))
    else {
        return Some(MixerHit::AddInstrument(t.id));
    };
    Some(match t.inserts.get(k) {
        Some(p)
            if (p.kind == PluginKind::Effect || p.deactivated)
                && !s.health.crashed(p.id)
                && bypass_part(list_row(list, i)).contains(x, y) =>
        {
            MixerHit::Bypass(t.id, p.id)
        }
        Some(p) => MixerHit::Open(t.id, p.id),
        None if k == t.inserts.len() && t.frozen.is_none() => MixerHit::AddInsert(t.id),
        None => MixerHit::Strip(t.id),
    })
}

pub fn paint(pen: &mut Pen, s: &Studio, t: &Track, list: Rect, rows: usize) {
    let c = pen.c;
    // A frozen track's instrument and inserts are locked (EXP-004): dim, with the lock where
    // the close was, and nothing lights under the pointer.
    let frozen = t.frozen.is_some();
    let hover = match s.hover {
        Hit::Mixer(h) if !frozen => Some(h),
        _ => None,
    };
    for i in 0..rows {
        let row = list_row(list, i);
        let Some(k) =
            (s.mixer.offset(t.id, List::Inserts) + i).checked_sub(crate::instrument::gap(t))
        else {
            crate::instrument::paint_empty(pen, row, hover == Some(MixerHit::AddInstrument(t.id)));
            continue;
        };
        let Some(p) = t.inserts.get(k) else {
            if k == t.inserts.len() && !frozen {
                pen.round(
                    row,
                    if hover == Some(MixerHit::AddInsert(t.id)) {
                        c.surface_3
                    } else {
                        c.surface_1
                    },
                    Radius::S,
                );
                pen.icon(
                    "plus",
                    Rect::new(row.x + (row.w - 12.0) / 2.0, row.y + 5.0, 12.0, 12.0),
                    c.ink,
                );
            }
            continue;
        };
        let hot = matches!(hover,Some(MixerHit::Open(_,id)|MixerHit::Bypass(_,id)) if id==p.id);
        let pinged = s.perf.pinged(p.id);
        let face = if hot {
            c.surface_3
        } else if frozen {
            c.surface_1
        } else if p.kind == PluginKind::Instrument {
            c.well
        } else {
            c.surface_2
        };
        pen.round(row, face, Radius::S);
        if pinged {
            pen.round(row, c.fill.opacity(0.35), Radius::S);
        }
        let dot = bypass_part(row);
        if frozen {
        } else if s.health.crashed(p.id) {
            pen.icon(
                "alert",
                Rect::new(dot.x + 4.0, dot.y + 5.0, 12.0, 12.0),
                c.alert,
            );
        } else if p.deactivated {
            pen.icon(
                "power",
                Rect::new(dot.x + 4.0, dot.y + 5.0, 12.0, 12.0),
                c.ink.opacity(LOCKED),
            );
        } else if p.kind == PluginKind::Instrument {
            pen.icon(
                "keys",
                Rect::new(dot.x + 4.0, dot.y + 5.0, 12.0, 12.0),
                c.ink,
            );
        } else {
            pen.round(
                Rect::new(dot.x + 6.0, dot.y + 6.5, 9.0, 9.0),
                if p.bypass { c.thumb } else { c.solo_on.0 },
                Radius::Pill,
            );
        }
        let remove = remove_part(row);
        let preset = crate::fx_preset::part(row);
        let heavy = s.perf.heavy.is_heavy(p.id) && !frozen && !p.deactivated;
        let mut name = name_rect(row);
        if heavy {
            // The gauge takes the place before the close; the name is cut short of it.
            name.w = (remove_part(row).x - HEAVY_W - 2.0 - name.x).max(0.0);
        }
        // The blue of an open plug-in falls under 4.5:1 on the ping's tint (light
        // theme): while the row is pinged its name is strong (UI-309).
        let ink = if p.deactivated || frozen {
            c.ink
        } else if !pinged && (s.mixer.is_open(p.id) || s.instruments.is_open(p.id)) {
            c.ink_blue
        } else {
            c.ink_strong
        };
        let mut shown = 0.0;
        pen.mask(name, |pen| {
            shown = pen.text(
                &p.name,
                FontSize::Caption,
                Weight::Regular,
                ink,
                name,
                Align::Left,
            );
        });
        if heavy {
            pen.icon(
                "gauge",
                Rect::new(remove.x - GAUGE, remove.y + 5.0, GAUGE, GAUGE),
                c.alert,
            );
        }
        if hot {
            // The mark sits on the tail of a name that runs under it, so the name keeps the room
            // it has without the pointer on it.
            if name.x + shown > remove.x {
                pen.round(remove, face, Radius::S);
            }
            if preset_mark_fits(row, &p.name) {
                pen.icon(
                    "file",
                    Rect::new(preset.x + 3.0, preset.y + 6.0, 10.0, 10.0),
                    c.ink,
                );
            }
            pen.icon(
                "close",
                Rect::new(remove.x + 3.0, remove.y + 6.0, 10.0, 10.0),
                c.ink,
            );
        }
        if frozen {
            pen.icon(
                "lock",
                Rect::new(remove.x + 3.0, remove.y + 6.0, 10.0, 10.0),
                c.ink.opacity(LOCKED),
            );
        }
    }
}

/// The room of the heavy plugin's gauge before the close.
const HEAVY_W: f32 = 14.0;
/// The gauge's size.
const GAUGE: f32 = 12.0;

/// How strongly a locked row's lock shows.
const LOCKED: f32 = 0.5;

impl Studio {
    pub fn fx_header_rect(&self, g: &Geometry, t: TrackId) -> Option<Rect> {
        let row = self.tree().row_of(RowKind::Track(t))?;
        if !row.tall || row.h < crate::studio::RowParts::LINE_3_Y + 24.0 + HEADER_H {
            return None;
        }
        Some(Rect::new(
            g.column.x + row.content_x() + 24.0,
            self.row_y(g, row) + crate::studio::RowParts::LINE_3_Y + 26.0,
            g.column.w - row.content_x() - 32.0,
            HEADER_H,
        ))
    }
    pub fn fx_header_hit(&self, g: &Geometry, x: f32, y: f32) -> Option<MixerHit> {
        if !g.column.contains(x, y) {
            return None;
        }
        let row = self.tree().row_at(y - g.column.y + self.scroll.1)?;
        let RowKind::Track(t) = row.kind else {
            return None;
        };
        hit(
            self,
            self.project().tracks.get(&t)?,
            self.fx_header_rect(g, t)?,
            3,
            x,
            y,
        )
    }
    pub fn fx_target(&self, g: &Geometry, x: f32, y: f32) -> Option<(TrackId, usize)> {
        let p = self.project();
        let strips = if self.mixer_shown() && g.drawer_body.contains(x, y) {
            self.mixer_strips(g)
                .into_iter()
                .filter(|s| self.strip_holds(g, s, x, y))
                .collect()
        } else {
            Vec::new()
        };
        for (t, list, rows) in strips
            .into_iter()
            .map(|s| (s.track, s.inserts, s.rows.inserts))
            .chain(p.tracks.keys().filter_map(|t| {
                g.column
                    .contains(x, y)
                    .then(|| self.fx_header_rect(g, *t))
                    .flatten()
                    .map(|r| (*t, r, 3))
            }))
        {
            if list.contains(x, y) {
                let track = p.tracks.get(&t)?;
                let i =
                    (((y - list.y) / (ROW_H + 2.0)).floor() as usize).min(rows.saturating_sub(1));
                let after = usize::from(y > list_row(list, i).y + ROW_H / 2.0);
                return Some((
                    t,
                    (self.mixer.offset(t, List::Inserts) + i + after)
                        .saturating_sub(crate::instrument::gap(track))
                        .min(track.inserts.len()),
                ));
            }
        }
        if let Some(h) = self.mixer_hit(g, x, y) {
            let t = h.track();
            return Some((t, p.tracks[&t].inserts.len()));
        }
        if g.column.contains(x, y) || g.lanes.contains(x, y) {
            let RowKind::Track(t) = self.tree().row_at(y - g.column.y + self.scroll.1)?.kind else {
                return None;
            };
            return Some((t, p.tracks.get(&t)?.inserts.len()));
        }
        None
    }
    pub fn effect_pick(&mut self, to: &str) -> Option<Pick> {
        let catalog = self.plugin_catalog();
        if let Some(e) = catalog.find(to).filter(|e| e.kind == PluginKind::Effect) {
            return Some(Pick {
                name: e.name.clone(),
                format: e.format.clone(),
                uid: e.uid.clone(),
            });
        }
        let name = to.strip_prefix("builtin:").unwrap_or(to);
        if let Some(d) = xirius_params::find(name).filter(|d| d.kind == xirius_params::Kind::Effect)
        {
            return Some(Pick {
                name: d.name.into(),
                format: "builtin".into(),
                uid: d.uid.into(),
            });
        }
        catalog
            .entries
            .iter()
            .find(|e| e.kind == PluginKind::Effect && e.name == to)
            .map(|e| Pick {
                name: e.name.clone(),
                format: e.format.clone(),
                uid: e.uid.clone(),
            })
    }
    pub fn add_effect(&mut self, t: TrackId, index: usize, pick: &Pick) -> bool {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        let Some(data)=self.arrange_run("plugin.add",json!({"track":t.to_string(),"name":pick.name,"format":pick.format,"uid":pick.uid,"index":index})) else{return false;};
        self.mixer
            .recent
            .retain(|p| p.format != pick.format || p.uid != pick.uid);
        self.mixer.recent.insert(0, pick.clone());
        self.mixer.recent.truncate(8);
        self.reveal_last(t, List::Inserts);
        if let Some(id) = data["id"].as_str().and_then(|s| s.parse().ok()) {
            self.begin_loading(&pick.name, &pick.format, id);
            self.open_plugin(t, id);
        }
        true
    }
    pub fn recent_effects(&self) -> Vec<Choice> {
        self.mixer
            .recent
            .iter()
            .map(|p| Choice {
                to: if p.format == "builtin" {
                    format!("builtin:{}", p.uid)
                } else {
                    p.uid.clone()
                },
                label: p.name.clone(),
            })
            .collect()
    }
    pub fn chain_down(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.mixer.browser_fx_focus = false;
        if self.preset_panel_down(x, y) {
            return true;
        }
        let h = self.hit(g, x, y);
        if let Hit::Mixer(m) = h
            && self.refuse_frozen(m)
        {
            return true;
        }
        let source = match h {
            Hit::Browser(BrowserHit::Item(i)) if self.popup.is_none() => {
                let l = self.browser.layout(g.panel);
                let Some(BRow::Item { item, .. }) = l.rows.get(i).map(|r| &r.row) else {
                    return false;
                };
                if !item.effect {
                    return false;
                }
                let name = item.name;
                self.browser.selected_item = Some(name);
                self.mixer.browser_fx_focus = true;
                let Some(pick) = self.effect_pick(name) else {
                    return false;
                };
                Source::Effect(pick)
            }
            Hit::Item(i) => {
                let Some(p) = &self.popup else {
                    return false;
                };
                let kind = p.kind;
                if !matches!(kind, PopupKind::Insert(_)) {
                    return false;
                }
                let Some(Action::Pick(to)) =
                    p.layout(g.size).cells.get(i).and_then(|c| c.action.clone())
                else {
                    return false;
                };
                let Some(pick) = self.effect_pick(&to) else {
                    return false;
                };
                self.mixer.fx_drag = Some(Drag {
                    source: Source::Effect(pick),
                    from: (x, y),
                    at: (x, y),
                    moved: false,
                    click: Some((kind, to)),
                });
                return true;
            }
            Hit::Mixer(MixerHit::Open(t, id)) => {
                let list = self
                    .fx_header_rect(g, t)
                    .filter(|r| r.contains(x, y))
                    .or_else(|| {
                        self.mixer_strips(g)
                            .into_iter()
                            .find(|s| s.track == t)
                            .map(|s| s.inserts)
                    });
                if let Some(list) = list {
                    let i = ((y - list.y) / (ROW_H + 2.0)).floor().max(0.0) as usize;
                    let row = list_row(list, i);
                    let named = self
                        .project()
                        .tracks
                        .get(&t)
                        .and_then(|k| k.inserts.iter().find(|p| p.id == id))
                        .is_some_and(|p| preset_mark_fits(row, &p.name));
                    if named && crate::fx_preset::part(row).contains(x, y) {
                        self.mixer.preset_panel = Some(crate::fx_preset::Panel {
                            id,
                            rect: Rect::new(
                                x.min(g.size.0 - 216.0),
                                y.min(g.size.1 - 52.0),
                                208.0,
                                52.0,
                            ),
                        });
                        return true;
                    }
                    if remove_part(row).contains(x, y) {
                        self.close_editor(id);
                        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                        self.run_command("plugin.remove", json!({"ids":[id.to_string()]}));
                        return true;
                    }
                }
                Source::Insert(t, id)
            }
            Hit::Mixer(MixerHit::AddInsert(t))
                if self.fx_header_rect(g, t).is_some_and(|r| r.contains(x, y)) =>
            {
                self.open_popup(PopupKind::Insert(t), Rect::new(x, y, 1.0, ROW_H));
                return true;
            }
            Hit::Mixer(MixerHit::AddInstrument(t))
                if self.fx_header_rect(g, t).is_some_and(|r| r.contains(x, y)) =>
            {
                self.open_popup(PopupKind::Instrument(t), Rect::new(x, y, 1.0, ROW_H));
                return true;
            }
            _ => return false,
        };
        self.mixer.fx_drag = Some(Drag {
            source,
            from: (x, y),
            at: (x, y),
            moved: false,
            click: None,
        });
        true
    }
    pub fn fx_move(&mut self, x: f32, y: f32) -> bool {
        let Some(d) = &mut self.mixer.fx_drag else {
            return false;
        };
        d.at = (x, y);
        d.moved |= (x - d.from.0).hypot(y - d.from.1) > 4.0;
        if d.moved {
            self.popup = None;
        }
        true
    }
    pub fn chain_wheel(&mut self, g: &Geometry, x: f32, y: f32, dy: f32) -> bool {
        let p = self.project();
        let Some(t) = p
            .tracks
            .keys()
            .find(|t| {
                self.fx_header_rect(g, **t)
                    .is_some_and(|r| r.contains(x, y))
            })
            .copied()
        else {
            return false;
        };
        let max = (p.tracks[&t].inserts.len() + crate::instrument::gap(&p.tracks[&t]) + 1)
            .saturating_sub(3);
        let off = self.mixer.offset(t, List::Inserts);
        let next = if dy > 0.0 {
            off.saturating_sub(1)
        } else if dy < 0.0 {
            (off + 1).min(max)
        } else {
            off
        };
        self.mixer.set_insert_offset(t, next);
        true
    }
    pub fn fx_up(&mut self, g: &Geometry, x: f32, y: f32, copy: bool) -> bool {
        let Some(d) = self.mixer.fx_drag.take() else {
            return false;
        };
        if !d.moved {
            if let Some((kind, to)) = d.click {
                self.popup = None;
                self.mixer_pick(kind, &to);
            } else {
                match d.source {
                    Source::Insert(t, id) => {
                        if self.health.crashed(id) {
                            self.open_popup(
                                crate::popup::PopupKind::CrashedPlugin(t, id),
                                Rect::new(d.from.0, d.from.1, 1.0, ROW_H),
                            );
                        } else {
                            self.open_plugin(t, id);
                        }
                    }
                    Source::Effect(p) => {
                        let t = match self.selected_row {
                            Some(RowKind::Track(t)) => t,
                            _ => TrackId::MASTER,
                        };
                        let n = self.project().tracks[&t].inserts.len();
                        self.add_effect(t, n, &p);
                    }
                }
            }
            return true;
        }
        let Some((t, mut index)) = self.fx_target(g, x, y) else {
            return true;
        };
        match d.source {
            Source::Effect(p) => {
                self.add_effect(t, index, &p);
            }
            Source::Insert(from, id) => {
                if !copy
                    && from == t
                    && let Some(pos) = self.project().tracks[&t]
                        .inserts
                        .iter()
                        .position(|p| p.id == id)
                    && index > pos
                {
                    index -= 1;
                }
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    if copy {
                        "plugin.copy_to"
                    } else {
                        "plugin.move"
                    },
                    json!({"id":id.to_string(),"track":t.to_string(),"index":index}),
                );
            }
        }
        true
    }
}

pub fn paint_headers(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let p = s.project();
    pen.mask(g.column, |pen| {
        for t in p.tracks.values() {
            if let Some(r) = s.fx_header_rect(g, t.id) {
                paint(pen, s, t, r, 3);
            }
        }
    });
}
pub fn paint_drag(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let Some(d) = s.mixer.fx_drag.as_ref().filter(|d| d.moved) else {
        return;
    };
    let name = match &d.source {
        Source::Effect(p) => p.name.clone(),
        Source::Insert(t, id) => s.project().tracks[t]
            .inserts
            .iter()
            .find(|p| p.id == *id)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
    };
    let c = pen.c;
    let r = Rect::new(
        (d.at.0 + 12.0).min(g.size.0 - 132.0),
        (d.at.1 + 12.0).min(g.size.1 - 28.0),
        120.0,
        24.0,
    );
    pen.round(r, c.surface_3, Radius::S);
    pen.text(
        &name,
        FontSize::Caption,
        Weight::Medium,
        c.ink_strong,
        r,
        Align::Center,
    );
}
