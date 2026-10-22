//! The screen of a song with no tracks yet (mock § 01): the entries to start
//! from in the middle of the body. 「MIDI を演奏」 makes an instrument track,
//! arms it and turns the PC keys into a keyboard (UI-074); 「オーディオを録音」
//! makes an armed audio track; 「ファイルをドロップ」 says what a drop does.
//!
//! Under the entries, the panel of the devices (UI-260, [`rig`]), then the songs opened last
//! (up to [`SHOWN`]) and the sample song after them (UI-261, start.html § 01): a press opens
//! one. The column is centred in the body.

use serde_json::json;
use xirius_design::{FontSize, Radius, Weight};

use crate::draw::{Align, Pen};
use crate::instrument::DEFAULT;
use crate::keep::RecentSong;
use crate::popup::ago;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tree::RowKind;

pub mod rig;
pub use rig::{Rig, RigLayout};

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

/// A part of the entrance under the entries (UI-261).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// A row of the recent songs, newest first.
    Song(usize),
    /// The sample song's row, after the songs.
    Sample,
    /// The device's button on the panel: the list of the devices (UI-260).
    Device,
    /// 「テスト音」 on the panel.
    Tone,
}

/// The mock's sizes: the zone's margin, an entry's width, its circle, the
/// gap between the circle and the label, and between entries.
const MARGIN: f32 = 12.0;
const WIDTH: f32 = 110.0;
const CIRCLE: f32 = 46.0;
const GAP: f32 = 8.0;
const LABEL: f32 = 16.0;
const BETWEEN: f32 = 30.0;
/// Between the entries and the list under them.
const STACK: f32 = 20.0;
/// The list's width at most.
pub const PANEL: f32 = 560.0;
/// The list's heading, and the gap under it.
const HEADING: f32 = 16.0;
const HEADING_GAP: f32 = 4.0;
/// A row of the list.
pub const ROW: f32 = 30.0;
/// Songs the list shows; the sample comes after them.
pub const SHOWN: usize = 5;
/// A row's padding, its icon, and the gap between its parts.
const ROW_PAD: f32 = 8.0;
const ROW_ICON: f32 = 16.0;
const ROW_GAP: f32 = 10.0;
/// The sample's tag: its height and the padding at its sides.
const TAG_H: f32 = 16.0;
const TAG_PAD: f32 = 6.0;
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

/// Where the entrance's parts are: one column, centred in [`zone`].
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Each entry's whole rect (circle and label) and its circle, in [`Entry::ALL`]'s order.
    pub entries: [(Rect, Rect); 3],
    /// The heading 「最近の曲」.
    pub heading: Rect,
    /// The panel of the devices, between the entries and the list.
    pub rig: RigLayout,
    /// The rows: the songs, then the sample.
    pub rows: Vec<Rect>,
}

impl Layout {
    /// The column with the panel showing `rig` and `rows` rows in the list.
    pub fn of(g: &Geometry, rows: usize, rig: &Rig) -> Layout {
        let z = zone(g);
        let n = Entry::ALL.len() as f32;
        let entries_w = n * WIDTH + (n - 1.0) * BETWEEN;
        let entries_h = CIRCLE + GAP + LABEL;
        let w = PANEL.min(z.w);
        let rig_h = RigLayout::height(rig, w);
        let list_h = HEADING + HEADING_GAP + rows as f32 * ROW;
        let h = entries_h + STACK + rig_h + STACK + list_h;
        let mut y = z.y + ((z.h - h) / 2.0).max(0.0);
        let x0 = z.x + (z.w - entries_w) / 2.0;
        let entries = std::array::from_fn(|i| {
            let x = x0 + i as f32 * (WIDTH + BETWEEN);
            let circle = Rect::new(x + (WIDTH - CIRCLE) / 2.0, y, CIRCLE, CIRCLE);
            (Rect::new(x, y, WIDTH, entries_h), circle)
        });
        y += entries_h + STACK;
        let x = z.x + (z.w - w) / 2.0;
        let rig = RigLayout::of(rig, x, y, w);
        y += rig_h + STACK;
        let heading = Rect::new(x + 4.0, y, w - 4.0, HEADING);
        y += HEADING + HEADING_GAP;
        let rows = (0..rows)
            .map(|i| Rect::new(x, y + i as f32 * ROW, w, ROW))
            .collect();
        Layout {
            entries,
            heading,
            rig,
            rows,
        }
    }

    pub fn entry(&self, e: Entry) -> (Rect, Rect) {
        self.entries[Entry::ALL.iter().position(|x| *x == e).unwrap_or(0)]
    }

    /// The row of a part, if it is shown.
    pub fn row(&self, part: Part) -> Option<Rect> {
        match part {
            Part::Song(i) => (i + 1 < self.rows.len()).then(|| self.rows[i]),
            Part::Sample => self.rows.last().copied(),
            Part::Device | Part::Tone => self.rig.part(part),
        }
    }

    /// The part under a point.
    pub fn part_at(&self, x: f32, y: f32) -> Option<Part> {
        if let Some(part) = self.rig.part_at(x, y) {
            return Some(part);
        }
        let i = self.rows.iter().position(|r| r.contains(x, y))?;
        Some(if i + 1 == self.rows.len() {
            Part::Sample
        } else {
            Part::Song(i)
        })
    }
}

impl Studio {
    /// The song has no tracks: the body shows the entries.
    pub fn start_shown(&self) -> bool {
        self.project().root.is_empty()
    }

    /// The songs the entrance lists: the last opened, newest first, but not this one.
    pub fn start_recent(&self) -> Vec<RecentSong> {
        let here = self.song.record_dir();
        self.keep
            .listed()
            .iter()
            .filter(|r| Some(&r.dir) != here.as_ref())
            .take(SHOWN)
            .cloned()
            .collect()
    }

    /// Where the entrance's parts are.
    pub fn start_layout(&self, g: &Geometry) -> Layout {
        Layout::of(g, self.start_recent().len() + 1, &self.rig())
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

    /// The part of the list under a point, on the empty screen.
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

    /// A row of the list: its song (or the sample) in place of this one.
    pub fn entrance_press(&mut self, g: &Geometry, part: Part) -> bool {
        match part {
            Part::Song(i) => {
                let Some(song) = self.start_recent().into_iter().nth(i) else {
                    return false;
                };
                self.start_file_request(crate::studio::FileRequest::Open, &song.dir);
            }
            Part::Sample => self.start_leaving(crate::saving::Leave::Sample),
            Part::Device => {
                if let Some(r) = self.entrance_rect(g, part) {
                    self.open_popup(crate::popup::PopupKind::Device(false), r);
                }
                return true;
            }
            Part::Tone => return self.play_test_tone(),
        }
        self.rebuild(g);
        true
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
            let mut io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
            io.view.selected =
                xirius_session::audio_settings::loudest(&io.view.levels).or(io.view.selected);
            io.input_name()
        };
        let made = self
            .song
            .run("track.add.audio", json!({ "name": "Audio" }))
            .and_then(|data| {
                let id = data["id"].as_str().unwrap_or_default().to_string();
                if let Some(input) = &input {
                    self.song
                        .run("track.input.set", json!({"ids": [id], "input": input}))?;
                }
                self.song
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

/// The entries in the middle of the empty body, and the list under them.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    if !studio.start_shown() {
        return;
    }
    let z = zone(g);
    if studio.drop_over {
        pen.round(z, pen.c.sel, Radius::S);
    }
    let songs = studio.start_recent();
    let rig = studio.rig();
    let l = Layout::of(g, songs.len() + 1, &rig);
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
        if e == Entry::Audio && rig.inputs.is_empty() {
            // Nothing to record from: a mark on the entry.
            let badge = Rect::new(r.x + BADGE_X, r.y + BADGE_Y, BADGE, BADGE);
            pen.icon("alert", badge, pen.c.alert);
        }
    }
    rig::paint(pen, studio, &rig, &l.rig);
    pen.text(
        "最近の曲",
        FontSize::Caption,
        Weight::Semibold,
        pen.c.ink_strong,
        l.heading,
        Align::Left,
    );
    let now = xirius_core::store::now_ms();
    for (i, r) in l.rows.iter().enumerate() {
        let (part, name, when) = match songs.get(i) {
            Some(s) => (Part::Song(i), s.name.as_str(), Some(ago(now, s.at_ms))),
            None => (Part::Sample, crate::song::SAMPLE_NAME, None),
        };
        let hot = studio.hover == Hit::Entrance(part);
        paint_row(pen, *r, name, when.as_deref(), hot);
    }
}

/// A row of the list: the file's icon, the name, the sample's tag, and when it was opened.
fn paint_row(pen: &mut Pen, r: Rect, name: &str, when: Option<&str>, hot: bool) {
    if hot {
        pen.round(r, pen.c.hover, Radius::S);
    }
    let icon = Rect::new(
        r.x + ROW_PAD,
        r.y + (r.h - ROW_ICON) / 2.0,
        ROW_ICON,
        ROW_ICON,
    );
    pen.icon("file", icon, pen.c.ink);
    let mut right = r.right() - ROW_PAD;
    if let Some(when) = when {
        let w = pen.measure(when, FontSize::Caption, Weight::Regular);
        pen.digits(
            when,
            FontSize::Caption,
            Weight::Regular,
            pen.c.ink,
            Rect::new(right - w, r.y, w, r.h),
            Align::Right,
        );
        right -= w + ROW_GAP;
    }
    let x = icon.right() + ROW_GAP;
    let place = Rect::new(x, r.y, (right - x).max(0.0), r.h);
    let name_w = pen
        .measure(name, FontSize::Small, Weight::Regular)
        .min(place.w);
    let ink = pen.c.ink_strong;
    pen.mask(place, |pen| {
        pen.text(
            name,
            FontSize::Small,
            Weight::Regular,
            ink,
            place,
            Align::Left,
        );
    });
    if when.is_none() {
        // The sample's tag after its name.
        let tag = "サンプル";
        let w = pen.measure(tag, FontSize::Micro, Weight::Regular) + 2.0 * TAG_PAD;
        let t = Rect::new(x + name_w + ROW_GAP, r.y + (r.h - TAG_H) / 2.0, w, TAG_H);
        pen.round(t, pen.c.surface_3, Radius::Clip);
        pen.text(
            tag,
            FontSize::Micro,
            Weight::Regular,
            pen.c.ink,
            t,
            Align::Center,
        );
    }
}

#[cfg(test)]
mod tests;
