//! The CPU of the song and of each plugin (ENG-008).
//!
//! The top bar's 「CPU N%」 button opens a list of the song's plugins, the heaviest first:
//! whether it works (the check at the left turns it off and on), its name, its track and its
//! share of the time its audio lasts. A plugin over 15% gets the gauge mark on its insert row
//! until it falls under 12%; the mark only says so, it never stops a sound. A double press on
//! a row makes that plugin's insert row light up in the mixer; a right press opens its menu
//! (画面を開く、無効にする、トラックをフリーズ). Every way ends in the commands
//! (`plugin.deactivate`, `plugin.activate`, `track.freeze`, `track.unfreeze`), which the CLI and
//! MCP run under the same names.
//!
//! The studio asks the host for the readings a couple of times a second ([`POLL`]) and keeps
//! them in [`Perf`].

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_design::{FontSize, Layout, Radius, Weight};
use xirius_engine::timing::Timings;
use xirius_session::perf::{Heavy, PluginLoad, Run, loads};

use crate::draw::{Align, Pen};
use crate::popup::{Action, Cell, Look, PopupKind};
use crate::studio::{Geometry, Rect, Studio};

/// How often the host is asked.
pub const POLL: Duration = Duration::from_millis(500);
/// How long a row that was pinged stays lit in the mixer.
pub const PING: Duration = Duration::from_millis(1600);

pub const W: f32 = 452.0;
/// The most rows shown at once; the wheel moves on.
pub const ROWS: usize = 10;
const PAD: f32 = 12.0;
const ROW_H: f32 = 26.0;
const CHECK_W: f32 = 32.0;
const TRACK_W: f32 = 108.0;
const LOAD_W: f32 = 96.0;
const BAR_W: f32 = 44.0;

/// What the host said last, and what the screen shows of it.
#[derive(Default)]
pub struct Perf {
    /// The whole callback's load: 0.4 is 40%.
    pub load: f64,
    /// Blocks that missed their time, and device underruns.
    pub xruns: u64,
    /// The song's plugins, the heaviest first.
    pub rows: Vec<PluginLoad>,
    pub heavy: Heavy,
    /// Readings in place of the host's, for tests and screenshots.
    pub fake: Option<Fake>,
    /// The plugin a double press lit in the mixer, and when.
    pub ping: Option<(PluginId, Instant)>,
    /// The row picked in the list.
    pub picked: Option<PluginId>,
    polled: Option<Instant>,
    signature: Vec<(PluginId, u32, Run)>,
}

/// Readings in place of the host's.
#[derive(Clone, Debug, Default)]
pub struct Fake {
    pub load: f64,
    pub xruns: u64,
    /// Each plugin's share of its time.
    pub loads: BTreeMap<PluginId, f64>,
}

impl Perf {
    /// The percent the top bar says.
    pub fn percent(&self) -> u32 {
        percent(self.load)
    }

    /// Is the plugin pinged and still lit?
    pub fn pinged(&self, id: PluginId) -> bool {
        self.ping
            .is_some_and(|(p, at)| p == id && at.elapsed() < PING)
    }
}

pub fn percent(load: f64) -> u32 {
    (load * 100.0).round().clamp(0.0, 999.0) as u32
}

// ---------------------------------------------------------------- what the list shows

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: PluginId,
    pub track: TrackId,
    pub name: String,
    pub track_name: String,
    pub run: Run,
    pub percent: u32,
    pub heavy: bool,
    pub picked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub percent: u32,
    pub xruns: u64,
    pub rows: Vec<Row>,
}

impl View {
    /// How far the wheel can move the list.
    pub fn max_offset(&self) -> usize {
        self.rows.len().saturating_sub(ROWS)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CAct {
    /// The check: turns the plugin off or on.
    Toggle(PluginId),
    /// The rest of the row: picks it.
    Pick(PluginId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum CLook {
    Head,
    /// The right end of the header.
    Xruns,
    Check {
        on: bool,
        frozen: bool,
    },
    Row {
        track: String,
        percent: u32,
        run: Run,
        heavy: bool,
        picked: bool,
    },
}

/// The list from (0, 0) = the pop-over's top-left corner.
pub fn cells(v: Option<&View>, offset: usize) -> Vec<Cell> {
    let Some(v) = v else {
        return Vec::new();
    };
    let inner = W - PAD * 2.0;
    let name_w = inner - CHECK_W - TRACK_W - LOAD_W;
    let mut cells = vec![
        Cell::new(
            Rect::new(PAD, PAD, inner / 2.0, Layout::MENU_ROW_H),
            Look::Label,
        )
        .text(format!("CPU {}%", v.percent)),
        Cell::new(
            Rect::new(PAD + inner / 2.0, PAD, inner / 2.0, Layout::MENU_ROW_H),
            Look::Cpu(CLook::Xruns),
        )
        .text(format!("音切れ {}", v.xruns)),
    ];
    let mut y = PAD + Layout::MENU_ROW_H;
    for (x, w, text) in [
        (PAD, CHECK_W, "有効"),
        (PAD + CHECK_W, name_w, "プラグイン"),
        (PAD + CHECK_W + name_w, TRACK_W, "トラック"),
        (PAD + CHECK_W + name_w + TRACK_W, LOAD_W, "CPU"),
    ] {
        cells.push(Cell::new(Rect::new(x, y, w, ROW_H - 4.0), Look::Cpu(CLook::Head)).text(text));
    }
    y += ROW_H - 4.0;
    if v.rows.is_empty() {
        cells.push(
            Cell::new(Rect::new(PAD, y, inner, ROW_H), Look::Note).text("プラグインはありません"),
        );
        return cells;
    }
    for r in v.rows.iter().skip(offset).take(ROWS) {
        let frozen = r.run == Run::Frozen;
        let mut check = Cell::new(
            Rect::new(PAD, y, CHECK_W, ROW_H),
            Look::Cpu(CLook::Check {
                on: r.run != Run::Deactivated,
                frozen,
            }),
        )
        .act(Action::Cpu(CAct::Toggle(r.id)));
        if frozen {
            check = check.off();
        }
        cells.push(check);
        cells.push(
            Cell::new(
                Rect::new(PAD + CHECK_W, y, inner - CHECK_W, ROW_H),
                Look::Cpu(CLook::Row {
                    track: r.track_name.clone(),
                    percent: r.percent,
                    run: r.run,
                    heavy: r.heavy,
                    picked: r.picked,
                }),
            )
            .text(r.name.clone())
            .act(Action::Cpu(CAct::Pick(r.id))),
        );
        y += ROW_H;
    }
    cells
}

pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: &CLook, hot: bool) {
    let c = pen.c;
    let r = cell.rect;
    match look {
        CLook::Head => {
            pen.text(
                &cell.text,
                FontSize::Micro,
                Weight::Medium,
                c.ink,
                Rect::new(r.x + 4.0, r.y, r.w - 4.0, r.h),
                Align::Left,
            );
        }
        CLook::Xruns => {
            pen.digits(
                &cell.text,
                FontSize::Caption,
                Weight::Medium,
                c.ink,
                r,
                Align::Right,
            );
        }
        CLook::Check { on, frozen } => {
            let m = 14.0;
            let box_r = Rect::new(r.x + (r.w - m) / 2.0, r.y + (r.h - m) / 2.0, m, m);
            if *frozen {
                pen.icon("lock", box_r, c.ink);
            } else if *on {
                pen.round(box_r, c.fill, Radius::Xs);
                pen.icon("check", box_r, c.on_fill);
            } else {
                pen.round(box_r, c.surface_3, Radius::Xs);
            }
        }
        CLook::Row {
            track,
            percent,
            run,
            heavy,
            picked,
        } => {
            if *picked {
                pen.round(r, c.sel, Radius::S);
            } else if hot {
                pen.round(r, c.hover, Radius::S);
            }
            let inner = W - PAD * 2.0;
            let name_w = inner - CHECK_W - TRACK_W - LOAD_W;
            let ink = if matches!(run, Run::Running | Run::Bypassed) {
                c.ink_strong
            } else {
                c.ink
            };
            let name = Rect::new(r.x + 4.0, r.y, name_w - 8.0, r.h);
            pen.mask(name, |pen| {
                pen.text(
                    &cell.text,
                    FontSize::Caption,
                    Weight::Regular,
                    ink,
                    name,
                    Align::Left,
                );
            });
            let mut x = r.x + name_w;
            if *run == Run::Frozen {
                pen.icon("freeze", Rect::new(x, r.y + 6.0, 14.0, 14.0), c.ink);
                x += 18.0;
            }
            let t = Rect::new(x, r.y, TRACK_W - (x - r.x - name_w) - 6.0, r.h);
            pen.mask(t, |pen| {
                pen.text(
                    track,
                    FontSize::Caption,
                    Weight::Regular,
                    c.ink,
                    t,
                    Align::Left,
                );
            });
            let lx = r.x + name_w + TRACK_W;
            if *run == Run::Running {
                let bar = Rect::new(lx, r.y + (r.h - 6.0) / 2.0, BAR_W, 6.0);
                pen.round(bar, c.well, Radius::Pill);
                let w = (BAR_W * (*percent as f32 / 100.0)).clamp(0.0, BAR_W);
                if w >= 1.0 {
                    // design-lint: allow(exception-family) 重いプラグインの印（警告の状態）
                    pen.round(
                        Rect::new(bar.x, bar.y, w, bar.h),
                        if *heavy { c.alert } else { c.fill },
                        Radius::Pill,
                    );
                }
                let n = Rect::new(lx + BAR_W + 6.0, r.y, LOAD_W - BAR_W - 6.0, r.h);
                pen.digits(
                    &format!("{percent}%"),
                    FontSize::Caption,
                    Weight::Medium,
                    if *heavy { c.ink_alert } else { c.ink_strong },
                    n,
                    Align::Right,
                );
            } else {
                let n = Rect::new(lx, r.y, LOAD_W, r.h);
                pen.text(
                    "–",
                    FontSize::Caption,
                    Weight::Regular,
                    c.ink,
                    n,
                    Align::Right,
                );
            }
        }
    }
}

// ---------------------------------------------------------------- the readings

fn order(rows: &mut [PluginLoad]) {
    rows.sort_by(|a, b| {
        b.reading
            .load
            .partial_cmp(&a.reading.load)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.track_name.cmp(&b.track_name))
    });
}

impl Studio {
    /// Asks the host for the readings, at most every [`POLL`]; follows the tracks being
    /// frozen. True when something on screen changed.
    pub fn poll_perf(&mut self) -> bool {
        let mut changed = false;
        if let Some((_, at)) = self.perf.ping
            && at.elapsed() >= PING
        {
            self.perf.ping = None;
            changed = true;
        }
        if self.perf.polled.is_some_and(|t| t.elapsed() < POLL) {
            return changed;
        }
        self.perf.polled = Some(Instant::now());
        changed |= self.read_perf();
        changed
    }

    /// Asks the host now.
    pub fn poll_perf_now(&mut self) -> bool {
        self.perf.polled = None;
        self.poll_perf()
    }

    fn read_perf(&mut self) -> bool {
        let project = self.project();
        let (load, xruns, mut rows) = match self.perf.fake.clone() {
            Some(f) => {
                let mut rows = loads(&project, &Timings::new(), None);
                for r in &mut rows {
                    if r.run == Run::Running
                        && let Some(l) = f.loads.get(&r.id)
                    {
                        r.reading.load = *l;
                        r.reading.blocks = 100;
                    }
                }
                order(&mut rows);
                (f.load, f.xruns, rows)
            }
            None => match self.transport.host_mut() {
                Some(h) => {
                    let cpu = h.cpu();
                    let timings = h.plugin_timings();
                    (cpu.load, cpu.xruns, loads(&project, &timings, None))
                }
                None => (0.0, 0, loads(&project, &Timings::new(), None)),
            },
        };
        let heavy_before = self.perf.heavy.len();
        let lit_before: Vec<bool> = rows
            .iter()
            .map(|r| self.perf.heavy.is_heavy(r.id))
            .collect();
        self.perf.heavy.update(&rows);
        let lit_after: Vec<bool> = rows
            .iter()
            .map(|r| self.perf.heavy.is_heavy(r.id))
            .collect();
        let signature: Vec<(PluginId, u32, Run)> = rows
            .iter()
            .map(|r| (r.id, percent(r.reading.load), r.run))
            .collect();
        // The rows show on screen only in the open list; the bar and the marks show the rest.
        let listed = signature != self.perf.signature
            && self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Perf);
        let changed = listed
            || percent(load) != percent(self.perf.load)
            || xruns != self.perf.xruns
            || heavy_before != self.perf.heavy.len()
            || lit_before != lit_after;
        self.perf.load = load;
        self.perf.xruns = xruns;
        self.perf.rows = std::mem::take(&mut rows);
        self.perf.signature = signature;
        if changed && self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Perf) {
            self.refresh_popup();
        }
        changed
    }

    /// The list the pop-over shows.
    pub(crate) fn perf_view(&self) -> View {
        View {
            percent: self.perf.percent(),
            xruns: self.perf.xruns,
            rows: self
                .perf
                .rows
                .iter()
                .map(|r| Row {
                    id: r.id,
                    track: r.track,
                    name: r.name.clone(),
                    track_name: r.track_name.clone(),
                    run: r.run,
                    percent: percent(r.reading.load),
                    heavy: self.perf.heavy.is_heavy(r.id),
                    picked: self.perf.picked == Some(r.id),
                })
                .collect(),
        }
    }

    /// The plugin of the row under a point of the open list.
    pub fn cpu_row_at(&self, g: &Geometry, x: f32, y: f32) -> Option<(TrackId, PluginId)> {
        let p = self.popup.as_ref().filter(|p| p.kind == PopupKind::Perf)?;
        let layout = p.layout(g.size);
        let cell = layout.cells.iter().find(|c| c.rect.contains(x, y))?;
        let Some(Action::Cpu(CAct::Toggle(id) | CAct::Pick(id))) = &cell.action else {
            return None;
        };
        let row = self.perf.rows.iter().find(|r| r.id == *id)?;
        Some((row.track, *id))
    }

    fn cpu_check_at(&self, g: &Geometry, x: f32, y: f32) -> bool {
        self.popup
            .as_ref()
            .filter(|p| p.kind == PopupKind::Perf)
            .is_some_and(|p| {
                p.layout(g.size).cells.iter().any(|c| {
                    c.rect.contains(x, y) && matches!(c.action, Some(Action::Cpu(CAct::Toggle(_))))
                })
            })
    }

    pub(crate) fn cpu_action(&mut self, act: CAct) -> bool {
        match act {
            CAct::Pick(id) => self.perf.picked = Some(id),
            CAct::Toggle(id) => {
                let off = self
                    .perf
                    .rows
                    .iter()
                    .find(|r| r.id == id)
                    .is_some_and(|r| r.run == Run::Deactivated);
                let command = if off {
                    "plugin.activate"
                } else {
                    "plugin.deactivate"
                };
                self.arrange_run(command, json!({ "ids": [id.to_string()] }));
                self.poll_perf_now();
            }
        }
        self.refresh_popup();
        true
    }

    /// A double press on a row: the plugin's insert row lights up in the mixer.
    pub fn cpu_down(&mut self, g: &Geometry, x: f32, y: f32, clicks: u32) -> bool {
        if clicks < 2 {
            return false;
        }
        let Some((t, id)) = self.cpu_row_at(g, x, y) else {
            return false;
        };
        // The check is its own press: two of them are not a ping.
        if self.cpu_check_at(g, x, y) {
            return false;
        }
        // A press the performance list takes leaves the inspector's field (UI-312).
        self.field_blur();
        self.cpu_ping(g, t, id);
        true
    }

    pub fn cpu_ping(&mut self, g: &Geometry, t: TrackId, id: PluginId) {
        // The channel's strip comes into sight: a command of the screen, not of the song.
        let reveal = json!({ "id": t.to_string(), "in": "mixer" });
        if let Some(args) = reveal.as_object() {
            self.screen_command(g, "track.reveal", args);
        }
        self.rebuild(g);
        // The list of the strip scrolls so the row is in it, from above or from below
        // (UI-171).
        let p = self.project();
        let row = p.tracks.get(&t).and_then(|x| {
            let k = x.inserts.iter().position(|p| p.id == id)?;
            Some(k + crate::instrument::gap(x))
        });
        let shown = crate::mixer::Rows::of(&p).inserts;
        if let Some(row) = row {
            let off = self.mixer.offset(t, crate::mixer::List::Inserts);
            if row < off {
                self.mixer.set_insert_offset(t, row);
            } else if row >= off + shown {
                self.mixer.set_insert_offset(t, row + 1 - shown);
            }
        }
        self.perf.picked = Some(id);
        self.perf.ping = Some((id, Instant::now()));
        self.refresh_popup();
    }

    /// A right press on a row of the open list: its menu.
    pub fn cpu_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some((t, id)) = self.cpu_row_at(g, x, y) else {
            return false;
        };
        self.perf.picked = Some(id);
        let items = self.cpu_items(t, id);
        self.open_context(items, x, y);
        true
    }
}

#[cfg(test)]
mod tests;
