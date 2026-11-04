//! The list of a receiver's key sources (UI-336), dropped from the «name ▾» seat of its window:
//! every channel that could key it, by トラック and バス, each with its send box, the send's
//! position (pre/post) and the output box at the right, and while it sends, the send's amount.
//! A channel the key would loop back from is shown with the loop mark and takes nothing.
use xirius_core::id::{PluginId, SendId, TrackId};
use xirius_core::model::{KeyMode, Project, TrackKind};
use xirius_design::{FontSize, Layout, Radius, Weight};

use crate::draw::{Align, Pen};
use crate::pick_list::{PickGroup, PickList, PickRow};
use crate::popup::{Action, Cell, Look};
use crate::studio::Rect;

/// What a part of a source's row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPart {
    /// The send box: the channel's sound through a send of its own.
    Send,
    /// Where the send leaves the channel: before or after its fader.
    Pre,
    /// The output box: the channel's output as it is.
    Output,
    /// The send's amount, dragged.
    Amount,
}

/// A source's row as the list shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyRow {
    pub plugin: PluginId,
    pub track: TrackId,
    /// The channel's colour.
    pub mark: usize,
    /// Sending: before the fader, and the amount in dB.
    pub send: Option<(bool, f64)>,
    pub output: bool,
    /// Not a source, and a key from it would make the routing loop.
    pub looped: bool,
}

/// How a cell of the list paints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KLook {
    /// A column's name over the boxes.
    Column,
    /// The row: the channel's colour and name, up to `right` before the row's right end.
    Row {
        mark: usize,
        right: f32,
    },
    Check {
        on: bool,
    },
    Pre {
        on: bool,
    },
    /// The send's amount, `t` along.
    Bar {
        t: f32,
    },
    Value,
    Loop,
}

/// The width of the list.
pub const WIDTH: f32 = 330.0;
const END: f32 = 6.0;
const BOX_COL: f32 = 44.0;
const PRE_COL: f32 = 28.0;
const GAP: f32 = 8.0;
const CHECK: f32 = 16.0;
const PRE: f32 = 22.0;
const BAR_W: f32 = 48.0;
const BAR_H: f32 = 6.0;
const VALUE_W: f32 = 36.0;
const VALUE_GAP: f32 = 5.0;
const CHIP: f32 = 10.0;
const LOOP: f32 = 12.0;

/// The columns of a row `r`: the send box's, the position's and the output box's.
fn columns(r: Rect) -> (Rect, Rect, Rect) {
    let out = Rect::new(r.right() - END - BOX_COL, r.y, BOX_COL, r.h);
    let pre = Rect::new(out.x - GAP - PRE_COL, r.y, PRE_COL, r.h);
    let send = Rect::new(pre.x - GAP - BOX_COL, r.y, BOX_COL, r.h);
    (send, pre, out)
}

fn centered(r: Rect, s: f32) -> Rect {
    Rect::new(r.x + (r.w - s) / 2.0, r.y + (r.h - s) / 2.0, s, s)
}

/// A key send's amount as the list shows it: "+1.5 dB", "0.0 dB", "-12.0 dB".
pub fn amount_words(db: f64) -> String {
    let db = (db * 10.0).round() / 10.0;
    if db > 0.0 {
        format!("+{db:.1} dB")
    } else {
        format!("{:.1} dB", db + 0.0)
    }
}

/// The source list of `plugin`: the channels before it that could key it, by トラック and
/// バス, in the mixer's order, without its own channel and the master. `level` is a send's
/// amount while its bar is held. None when the plug-in is gone.
pub fn list(
    p: &Project,
    plugin: PluginId,
    level: impl Fn(TrackId, SendId) -> Option<f64>,
) -> Option<PickList> {
    let (owner, x) = p
        .tracks
        .values()
        .find_map(|t| Some((t.id, t.inserts.iter().find(|x| x.id == plugin)?)))?;
    let sources = x
        .sidechain
        .as_ref()
        .map(|sc| sc.sources.as_slice())
        .unwrap_or_default();
    let mut tracks = Vec::new();
    let mut buses = Vec::new();
    for id in crate::mixer::strip_order(p) {
        let Some(t) = p.tracks.get(&id) else {
            continue;
        };
        if id == owner || t.kind == TrackKind::Master {
            continue;
        }
        let source = sources.iter().find(|s| s.track == id);
        let send = source.and_then(|s| {
            let (pre, db, _) = s.mode.send()?;
            Some((pre, level(id, s.id).unwrap_or(db)))
        });
        let output = source.is_some_and(|s| matches!(s.mode, KeyMode::Output));
        let looped = source.is_none() && !p.sidechain_ok(plugin, id);
        let key = KeyRow {
            plugin,
            track: id,
            mark: crate::draw::track_color(t),
            send,
            output,
            looped,
        };
        let mut row =
            PickRow::new(t.shown_name()).act(Action::KeySource(plugin, id, KeyPart::Send));
        if looped {
            row = row.disabled();
        }
        row.key = Some(key);
        if t.kind == TrackKind::Bus {
            buses.push(row);
        } else {
            tracks.push(row);
        }
    }
    let mut out = PickList::of(Vec::new()).named("一致するチャンネルはありません");
    out.groups.clear();
    if !tracks.is_empty() {
        out.groups.push(PickGroup::new("トラック", tracks));
    }
    if !buses.is_empty() {
        out.groups.push(PickGroup::new("バス", buses));
    }
    out.keyed = true;
    Some(out)
}

/// The columns' names over a group's boxes, at the heading `r`.
pub fn head_cells(r: Rect) -> Vec<Cell> {
    let (send, _, out) = columns(r);
    vec![
        Cell::new(send, Look::Key(KLook::Column)).text("センド"),
        Cell::new(out, Look::Key(KLook::Column)).text("出力"),
    ]
}

/// The cells of a source's row `r`: the row, then its parts. `cur`: the arrow keys' row.
pub fn row_cells(r: Rect, name: &str, key: &KeyRow, cur: bool) -> Vec<Cell> {
    let (send_col, pre_col, out_col) = columns(r);
    let act = |part| Action::KeySource(key.plugin, key.track, part);
    let mut extra = send_col.x - GAP;
    let mut parts = Vec::new();
    if let Some((_, db)) = key.send {
        let value = Rect::new(extra - VALUE_W, r.y, VALUE_W, r.h);
        let bar = Rect::new(value.x - VALUE_GAP - BAR_W, r.y, BAR_W, r.h);
        extra = bar.x;
        parts.push(
            Cell::new(
                bar,
                Look::Key(KLook::Bar {
                    t: crate::mixer::keys::bar_t(db),
                }),
            )
            .act(act(KeyPart::Amount))
            .text(amount_words(db))
            .spoken(format!("{name} センドの量")),
        );
        parts.push(Cell::new(value, Look::Key(KLook::Value)).text(amount_words(db)));
    } else if key.looped {
        let mark = Rect::new(extra - LOOP, r.y, LOOP, r.h);
        extra = mark.x;
        parts.push(Cell::new(mark, Look::Key(KLook::Loop)).spoken("ループ"));
    }
    let check = |col: Rect, on: bool, part: KeyPart, what: &str| {
        let cell = Cell::new(centered(col, CHECK), Look::Key(KLook::Check { on }))
            .on(on)
            .act(act(part))
            .spoken(format!("{name} {what}"));
        if key.looped { cell.off() } else { cell }
    };
    parts.push(check(send_col, key.send.is_some(), KeyPart::Send, "センド"));
    let pre = key.send.is_some_and(|(pre, _)| pre);
    let pre_cell = Cell::new(centered(pre_col, PRE), Look::Key(KLook::Pre { on: pre }))
        .on(pre)
        .act(act(KeyPart::Pre))
        .spoken(format!("{name} プリフェーダー"));
    parts.push(if key.send.is_some() {
        pre_cell
    } else {
        pre_cell.off()
    });
    parts.push(check(out_col, key.output, KeyPart::Output, "出力"));
    let right = r.right() - (extra - GAP);
    let row = Cell::new(
        r,
        Look::Key(KLook::Row {
            mark: key.mark,
            right,
        }),
    )
    .text(name)
    .cur(cur);
    // A row the key would loop back from is fainter and said to be off (sidechain.html).
    let mut out = vec![if key.looped { row.off() } else { row }];
    out.extend(parts);
    out
}

pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: &KLook, hot: bool) {
    let c = pen.c;
    let r = cell.rect;
    match *look {
        KLook::Column => {
            pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                r,
                Align::Center,
            );
        }
        KLook::Row { mark, right } => {
            let end = r.right() - right;
            if cell.cursor {
                pen.round(r, c.sel, Radius::S);
            }
            let x = r.x + GAP;
            let chip = Rect::new(x, r.y + (r.h - CHIP) / 2.0, CHIP, CHIP);
            // design-lint: allow(exception-family) 送り元のチャンネルの色の印（トラックの表示、モックの sidechain.html）
            pen.round(chip, c.tracks[mark % c.tracks.len()], Radius::Xs);
            let x = chip.right() + GAP;
            let room = Rect::new(x, r.y, (end - x).max(0.0), r.h);
            let name = pen.fit(&cell.text, FontSize::Small, Weight::Medium, room.w);
            pen.text(
                &name,
                FontSize::Small,
                Weight::Medium,
                if cell.enabled { c.ink_strong } else { c.ink },
                room,
                Align::Left,
            );
        }
        KLook::Check { on } => {
            if on {
                pen.round(r, c.fill, Radius::Xs);
                pen.icon("check", centered(r, Layout::ICON_SIZE - 4.0), c.on_fill);
            } else {
                let ring = if hot && cell.live() { c.ink } else { c.line[0] };
                pen.outline(r, ring, Radius::Xs, 1.5);
            }
        }
        KLook::Pre { on } => {
            let ink = if !cell.enabled {
                c.line[0]
            } else if on {
                pen.round(r, c.surface_3, Radius::Xs);
                c.ink_strong
            } else {
                if hot {
                    pen.round(r, c.hover, Radius::Xs);
                }
                c.ink
            };
            let icon = if on { "pre" } else { "post" };
            pen.icon(icon, centered(r, LOOP), ink);
        }
        KLook::Bar { t } => {
            let well = Rect::new(r.x, r.y + (r.h - BAR_H) / 2.0, r.w, BAR_H);
            pen.round(well, c.well, Radius::Pill);
            let w = well.w * t.clamp(0.0, 1.0);
            if w > 0.0 {
                pen.round(
                    Rect::new(well.x, well.y, w, BAR_H),
                    c.ink_mark,
                    Radius::Pill,
                );
            }
        }
        KLook::Value => {
            pen.digits(
                &cell.text,
                FontSize::Micro,
                Weight::Regular,
                c.ink,
                r,
                Align::Right,
            );
        }
        KLook::Loop => {
            pen.icon("loop", centered(r, LOOP), c.ink);
        }
    }
}

#[cfg(test)]
#[path = "key_sources_tests.rs"]
mod tests;
