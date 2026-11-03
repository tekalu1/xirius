//! The screen of a song with no tracks yet (mock § 01): the entries to start
//! from in the middle of the body. 「MIDI を演奏」 makes an instrument track,
//! arms it and turns the PC keys into a keyboard (UI-074); 「オーディオを録音」
//! makes an armed audio track; 「ファイルをドロップ」 says what a drop does.
//!
//! Only the three entries, centred (UI-350): the recent songs and the sample are on the start
//! screen ([`crate::home`]), the test tone in 設定 › オーディオ. With no audio device, one line
//! 「オーディオデバイスなし」 under the entries opens [`crate::popup::PopupKind::NoDevice`].

use serde_json::json;
use xirius_design::{FontSize, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::instrument::DEFAULT;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tree::RowKind;

pub mod rig;

/// An entry of the empty screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Audio,
    Midi,
    /// Only a label: files are dropped anywhere on the screen.
    Drop,
}

impl Entry {
    pub const ALL: [Entry; 3] = [Entry::Audio, Entry::Midi, Entry::Drop];

    pub fn label(self) -> &'static str {
        match self {
            Entry::Audio => "オーディオを録音",
            Entry::Midi => "MIDI を演奏",
            Entry::Drop => "ファイルをドロップ",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Entry::Audio => "mic",
            Entry::Midi => "piano",
            Entry::Drop => "drop",
        }
    }

    /// A button (the drop entry only says what to do).
    pub fn pressable(self) -> bool {
        self != Entry::Drop
    }
}

/// A part of the screen under the entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// 「オーディオデバイスなし」, only with no audio device: its menu (UI-350).
    NoDevice,
}

/// The mock's sizes: the zone's margin, an entry's width, its circle, the
/// gap between the circle and the label, and between entries.
const MARGIN: f32 = 12.0;
const WIDTH: f32 = 110.0;
const CIRCLE: f32 = 46.0;
const GAP: f32 = 8.0;
const LABEL: f32 = 16.0;
const BETWEEN: f32 = 30.0;
/// The entries in a window too narrow for them: each narrower, closer together (UI-350).
const NARROW_WIDTH: f32 = 96.0;
const NARROW_BETWEEN: f32 = 8.0;
/// Between the entries and the line under them.
const STACK: f32 = 20.0;
/// The mark on 「オーディオを録音」 when the device has no inputs: its size and place.
const BADGE: f32 = 16.0;
const BADGE_X: f32 = 68.0;
const BADGE_Y: f32 = -3.0;

/// The area the entries sit in the middle of: the rows' column and lanes.
pub fn zone(g: &Geometry) -> Rect {
    let x = g.column.x.min(g.lanes.x);
    let r = Rect::new(x, g.lanes.y, g.lanes.right() - x, g.lanes.h);
    Rect::new(
        r.x + MARGIN,
        r.y + MARGIN,
        (r.w - 2.0 * MARGIN).max(0.0),
        (r.h - 2.0 * MARGIN).max(0.0),
    )
}

/// Where the screen's parts are: the entries in a row, centred in [`zone`].
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Each entry's whole rect (circle and label) and its circle, in [`Entry::ALL`]'s order.
    pub entries: [(Rect, Rect); 3],
    /// 「オーディオデバイスなし」 under the entries, with no audio device.
    pub no_device: Option<Rect>,
}

impl Layout {
    /// The entries, and the line under them when `no_device`.
    pub fn of(g: &Geometry, no_device: bool) -> Layout {
        let z = zone(g);
        let n = Entry::ALL.len() as f32;
        let (mut width, mut between) = (WIDTH, BETWEEN);
        if n * width + (n - 1.0) * between > z.w {
            (width, between) = (NARROW_WIDTH, NARROW_BETWEEN);
        }
        let entries_w = n * width + (n - 1.0) * between;
        let entries_h = CIRCLE + GAP + LABEL;
        let line_h = if no_device {
            STACK + no_device::HEIGHT
        } else {
            0.0
        };
        let y = z.y + ((z.h - entries_h - line_h) / 2.0).max(0.0);
        let x0 = z.x + (z.w - entries_w) / 2.0;
        let entries = std::array::from_fn(|i| {
            let x = x0 + i as f32 * (width + between);
            let circle = Rect::new(x + (width - CIRCLE) / 2.0, y, CIRCLE, CIRCLE);
            (Rect::new(x, y, width, entries_h), circle)
        });
        let no_device = no_device.then(|| {
            let w = no_device::width();
            Rect::new(
                z.x + (z.w - w) / 2.0,
                y + entries_h + STACK,
                w,
                no_device::HEIGHT,
            )
        });
        Layout { entries, no_device }
    }

    pub fn entry(&self, e: Entry) -> (Rect, Rect) {
        self.entries[Entry::ALL.iter().position(|x| *x == e).unwrap_or(0)]
    }

    /// Where a part shows, if it does.
    pub fn row(&self, part: Part) -> Option<Rect> {
        match part {
            Part::NoDevice => self.no_device,
        }
    }

    /// The part under a point.
    pub fn part_at(&self, x: f32, y: f32) -> Option<Part> {
        self.no_device
            .is_some_and(|r| r.contains(x, y))
            .then_some(Part::NoDevice)
    }
}

/// 「オーディオデバイスなし」: the line on the start screen and the new song's screen that
/// opens the menu with 「再スキャン」 and 「オーディオの設定…」 (UI-350).
pub(crate) mod no_device {
    use super::*;
    use crate::browser::text_width;
    use crate::start::rig::NO_DEVICE;

    pub const HEIGHT: f32 = xirius_design::Layout::MENU_ROW_H;
    const ICON: f32 = xirius_design::Layout::ICON_SIZE;

    /// The line's width: its padding, the mark, and the words.
    pub fn width() -> f32 {
        Space::X2.px()
            + ICON
            + Space::X2.px()
            + text_width(NO_DEVICE, FontSize::Small)
            + Space::X2.px()
    }

    pub fn paint(pen: &mut Pen, r: Rect, hot: bool) {
        if hot {
            pen.round(r, pen.c.hover, Radius::S);
        }
        let pad = Space::X2.px();
        let icon = Rect::new(r.x + pad, r.y + (r.h - ICON) / 2.0, ICON, ICON);
        pen.icon("alert", icon, pen.c.alert);
        let x = icon.right() + pad;
        pen.text(
            NO_DEVICE,
            FontSize::Small,
            Weight::Medium,
            pen.c.ink_alert,
            Rect::new(x, r.y, (r.right() - x).max(0.0), r.h),
            Align::Left,
        );
    }
}

impl Studio {
    /// The song has no tracks: the body shows the entries.
    pub fn start_shown(&self) -> bool {
        self.project().root.is_empty()
    }

    /// Where the screen's parts are.
    pub fn start_layout(&self, g: &Geometry) -> Layout {
        Layout::of(g, !self.has_audio_device())
    }

    /// An entry's whole rect (circle and label) and its circle.
    pub fn entry_rects(&self, g: &Geometry, e: Entry) -> (Rect, Rect) {
        self.start_layout(g).entry(e)
    }

    /// The entry button under a point, on the empty screen.
    pub fn start_at(&self, g: &Geometry, x: f32, y: f32) -> Option<Entry> {
        if !self.start_shown() {
            return None;
        }
        let l = self.start_layout(g);
        Entry::ALL
            .into_iter()
            .filter(|e| e.pressable())
            .find(|e| l.entry(*e).0.contains(x, y))
    }

    /// The part under the entries under a point, on the empty screen.
    pub fn entrance_at(&self, g: &Geometry, x: f32, y: f32) -> Option<Part> {
        if !self.start_shown() {
            return None;
        }
        self.start_layout(g).part_at(x, y)
    }

    /// Where a part shows.
    pub fn entrance_rect(&self, g: &Geometry, part: Part) -> Option<Rect> {
        if !self.start_shown() {
            return None;
        }
        self.start_layout(g).row(part)
    }

    /// 「オーディオデバイスなし」: its menu under it.
    pub fn entrance_press(&mut self, g: &Geometry, part: Part) -> bool {
        match part {
            Part::NoDevice => {
                if let Some(r) = self.entrance_rect(g, part) {
                    self.open_popup(crate::popup::PopupKind::NoDevice, r);
                }
                true
            }
        }
    }

    pub fn start_press(&mut self, g: &Geometry, e: Entry) -> bool {
        match e {
            Entry::Midi => self.start_midi(),
            Entry::Audio => self.start_audio(g),
            Entry::Drop => false,
        }
    }

    /// 「MIDI を演奏」: an armed track playing the built-in instrument, and
    /// the PC keys play it.
    pub fn start_midi(&mut self) -> bool {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
        let made = self.song.run(
            "track.add.with-instrument",
            json!({ "instrument": DEFAULT, "arm": true }),
        );
        let Some(id) = self.made(made) else {
            return true;
        };
        self.select_row(Some(RowKind::Track(id)));
        self.pc_keys.on = true;
        self.tell(format!("{DEFAULT} を選択しました"));
        true
    }

    /// Does the device in use have inputs?
    pub fn device_has_inputs(&self) -> bool {
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
        let v = &io.view;
        v.devices
            .iter()
            .find(|d| d.name == v.device)
            .is_some_and(|d| !d.inputs.is_empty())
    }

    /// 「オーディオを録音」: an armed audio track on the loudest input. A device without
    /// inputs has nothing to record from: no track, and the list of the devices with inputs
    /// opens under the entry instead (UI-260); choosing one makes the track.
    pub(crate) fn start_audio(&mut self, g: &Geometry) -> bool {
        self.poll_io();
        if !self.device_has_inputs() {
            let (_, circle) = self.entry_rects(g, Entry::Audio);
            self.open_popup(crate::popup::PopupKind::Device(true), circle);
            return true;
        }
        let input = {
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            let mut io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
            io.view.selected =
                xirius_session::audio_settings::loudest(&io.view.levels).or(io.view.selected);
            io.input_name()
        };
        let made = self
            .song
            // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
            .run("track.add.audio", json!({ "name": "Audio" }))
            .and_then(|data| {
                let id = data["id"].as_str().unwrap_or_default().to_string();
                if let Some(input) = &input {
                    self.song
                        // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
                        .run("track.input.set", json!({"ids": [id], "input": input}))?;
                }
                self.song
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
                    .run("track.arm", json!({ "ids": [id] }))
                    .map(|_| data)
            });
        if let Some(id) = self.made(made) {
            self.select_row(Some(RowKind::Track(id)));
            self.drawer_open = true;
            self.tab = crate::studio::DrawerTab::Mixer;
        }
        true
    }

    /// The new track of a command's data, after the screen followed it.
    fn made(
        &mut self,
        made: Result<serde_json::Value, String>,
    ) -> Option<xirius_core::id::TrackId> {
        match made {
            Ok(data) => {
                self.error = None;
                self.after_edit();
                data["id"].as_str()?.parse().ok()
            }
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }
}

/// The entries in the middle of the empty body, and the line under them.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    if !studio.start_shown() {
        return;
    }
    let z = zone(g);
    if studio.drop_over {
        pen.round(z, pen.c.sel, Radius::S);
    }
    let l = studio.start_layout(g);
    let no_inputs = !studio.device_has_inputs();
    for e in Entry::ALL {
        let (r, circle) = l.entry(e);
        let hot = e.pressable() && studio.hover == Hit::Start(e);
        let fill = if hot { pen.c.thumb } else { pen.c.surface_3 };
        pen.round(circle, fill, Radius::Pill);
        let icon = Rect::new(
            circle.x + (CIRCLE - 20.0) / 2.0,
            circle.y + (CIRCLE - 20.0) / 2.0,
            20.0,
            20.0,
        );
        pen.icon(e.icon(), icon, pen.c.ink);
        let label = Rect::new(r.x, circle.bottom() + GAP, r.w, LABEL);
        pen.text(
            e.label(),
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink,
            label,
            Align::Center,
        );
        if e == Entry::Audio && no_inputs {
            // Nothing to record from: a mark on the entry.
            let badge = Rect::new(
                circle.x + BADGE_X - (WIDTH - CIRCLE) / 2.0,
                r.y + BADGE_Y,
                BADGE,
                BADGE,
            );
            pen.icon("alert", badge, pen.c.alert);
        }
    }
    if let Some(r) = l.no_device {
        no_device::paint(pen, r, studio.hover == Hit::Entrance(Part::NoDevice));
    }
}

#[cfg(test)]
mod tests;
