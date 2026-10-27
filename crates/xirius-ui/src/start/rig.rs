//! The device panel under the entries (UI-260, start.html § 01): the output device with its
//! rate and latency, the test tone and the output's meter; a chip with a meter for each input;
//! a chip with a light for the PC keys and each MIDI port; and how many plugins there are, or
//! how far their first scan is. It shows each time a song with no tracks is open.

use xirius_design::{FontSize, Radius, Weight};
use xirius_session::audio_settings::PC_KEYBOARD;

use crate::browser::text_width;
use crate::draw::{Align, Pen};
use crate::studio::{Hit, Rect, Studio};

use super::Part;

mod meter;
pub use meter::level_width;
use meter::{meter, midi};

/// The panel's padding up and down, and at the sides.
const PAD_Y: f32 = 10.0;
const PAD_X: f32 = 12.0;
/// The column of the rows' icons, and the gap after it.
const ICON_COL: f32 = 20.0;
const ICON_GAP: f32 = 10.0;
/// A row's height at least, and the gap between rows.
const ROW_H: f32 = 28.0;
const ROW_GAP: f32 = 6.0;
/// The gap between the parts of a row (and between lines when its chips wrap).
const GAP: f32 = 10.0;
/// An icon.
const ICON: f32 = 16.0;
/// A chip: its height, its padding, the gap inside it, and the input's meter in it.
const CHIP_H: f32 = 26.0;
const CHIP_PAD: f32 = 8.0;
const CHIP_GAP: f32 = 6.0;
const LEVEL_W: f32 = 54.0;
const LEVEL_H: f32 = 8.0;
/// MIDI's light.
const DOT: f32 = 8.0;
/// The device's button: its height, its padding, and the gap before its arrow.
const LINK_H: f32 = 26.0;
const LINK_PAD: f32 = 4.0;
const LINK_GAP: f32 = 4.0;
/// The test tone's button: its height and padding.
const BUTTON_H: f32 = 28.0;
const BUTTON_PAD: f32 = 10.0;
/// The output's meter: its width, each side's bar and the gap between them.
const STEREO_W: f32 = 90.0;
const THIN: f32 = 4.0;
const THIN_GAP: f32 = 3.0;
/// The scan's bar.
const BAR_H: f32 = 6.0;
/// How much of the output's level is left the next frame.
const FALL: f32 = 0.85;

pub const NO_DEVICE: &str = "オーディオデバイスなし";
pub const NO_INPUT: &str = "入力なし";
pub const TONE: &str = "テスト音";

/// What the panel shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rig {
    /// The device in use, if there is one.
    pub device: Option<String>,
    /// 「48 kHz · 6.8 ms」 once the device is open.
    pub info: String,
    /// Each input of the device and its level (0..1).
    pub inputs: Vec<(String, f32)>,
    /// The PC keys and the MIDI ports, each lit while it is played.
    pub midi: Vec<(String, bool)>,
    /// Plugins found, and those that could not be read.
    pub found: usize,
    pub failed: usize,
    /// While the scan runs: how many of how many.
    pub scanning: Option<(u64, u64)>,
    /// The output's level, left and right.
    pub out: [f32; 2],
    /// The test tone sounds.
    pub tone: bool,
}

impl Rig {
    pub fn plugins_text(&self) -> String {
        match self.scanning {
            Some((done, total)) => format!("プラグイン {} / {}", done.min(total), total),
            None => format!("プラグイン {}", self.found),
        }
    }
}

/// Where the panel's parts are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RigLayout {
    pub panel: Rect,
    /// Each row's icon: the output, the inputs, MIDI, the plugins.
    pub icons: [Rect; 4],
    /// The device's button (its name and the arrow, or 「オーディオデバイスなし」).
    pub device: Rect,
    pub info: Rect,
    /// The test tone's button; empty with no device.
    pub tone: Rect,
    pub meter: Rect,
    /// A chip per input, or the one 「入力なし」.
    pub inputs: Vec<Rect>,
    pub midi: Vec<Rect>,
    pub plugins: Rect,
    /// The scan's bar, while it runs.
    pub bar: Option<Rect>,
    /// The plugins that could not be read.
    pub bad: Option<Rect>,
}

fn width(s: &str) -> f32 {
    text_width(s, FontSize::Small)
}

/// Places chips of `widths` from `x` on lines of `w` from `y`; gives them and the height used.
fn flow(widths: &[f32], x: f32, y: f32, w: f32) -> (Vec<Rect>, f32) {
    let (mut cx, mut line) = (x, 0usize);
    let mut rects = Vec::with_capacity(widths.len());
    for &cw in widths {
        if cx > x && cx + cw > x + w {
            cx = x;
            line += 1;
        }
        let top = y + line as f32 * (CHIP_H + GAP);
        rects.push(Rect::new(cx, top, cw, CHIP_H));
        cx += cw + GAP;
    }
    let lines = line as f32 + 1.0;
    let h = (lines * CHIP_H + (lines - 1.0) * GAP).max(ROW_H);
    // One line sits in the middle of the row.
    if line == 0 {
        for r in &mut rects {
            r.y += (ROW_H - CHIP_H) / 2.0;
        }
    }
    (rects, h)
}

/// A part `h` tall in the middle of a row from `y`.
fn mid(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y + (ROW_H - h) / 2.0, w, h)
}

impl RigLayout {
    /// The panel `w` wide from (`x`, `y`).
    pub fn of(rig: &Rig, x: f32, y: f32, w: f32) -> RigLayout {
        let x0 = x + PAD_X + ICON_COL + ICON_GAP;
        let inner = (x + w - PAD_X - x0).max(0.0);
        let mut l = RigLayout::default();
        let mut top = y + PAD_Y;
        let mut heights = [ROW_H; 4];

        // The output.
        match &rig.device {
            Some(name) => {
                let dw = LINK_PAD + width(name) + LINK_GAP + ICON + LINK_PAD;
                l.device = mid(x0, top, dw, LINK_H);
                let mut cx = l.device.right() + GAP;
                if !rig.info.is_empty() {
                    l.info = mid(cx, top, width(&rig.info), ROW_H);
                    cx = l.info.right() + GAP;
                }
                let tw = BUTTON_PAD + ICON + CHIP_GAP + width(TONE) + BUTTON_PAD;
                l.tone = mid(cx, top, tw, BUTTON_H);
                l.meter = mid(l.tone.right() + GAP, top, STEREO_W, 2.0 * THIN + THIN_GAP);
            }
            None => {
                let dw = LINK_PAD + ICON + LINK_GAP + width(NO_DEVICE) + LINK_PAD;
                l.device = mid(x0, top, dw, LINK_H);
            }
        }
        l.icons[0] = Rect::new(x + PAD_X, top, ICON_COL, ROW_H);
        top += heights[0] + ROW_GAP;

        // The inputs.
        if rig.inputs.is_empty() {
            l.inputs = vec![mid(x0, top, ICON + CHIP_GAP + width(NO_INPUT), CHIP_H)];
        } else {
            let widths: Vec<f32> = rig
                .inputs
                .iter()
                .map(|(n, _)| CHIP_PAD + width(n) + CHIP_GAP + LEVEL_W + CHIP_PAD)
                .collect();
            (l.inputs, heights[1]) = flow(&widths, x0, top, inner);
        }
        l.icons[1] = Rect::new(x + PAD_X, top, ICON_COL, heights[1]);
        top += heights[1] + ROW_GAP;

        // MIDI.
        let widths: Vec<f32> = rig
            .midi
            .iter()
            .map(|(n, _)| CHIP_PAD + DOT + CHIP_GAP + width(n) + CHIP_PAD)
            .collect();
        (l.midi, heights[2]) = flow(&widths, x0, top, inner);
        l.icons[2] = Rect::new(x + PAD_X, top, ICON_COL, heights[2]);
        top += heights[2] + ROW_GAP;

        // The plugins.
        let text = rig.plugins_text();
        l.plugins = mid(x0, top, width(&text), ROW_H);
        let after = l.plugins.right() + GAP;
        if rig.scanning.is_some() {
            l.bar = Some(mid(after, top, (x0 + inner - after).max(0.0), BAR_H));
        } else if rig.failed > 0 {
            let w = ICON + CHIP_GAP + width(&rig.failed.to_string());
            l.bad = Some(mid(after, top, w, CHIP_H));
        }
        l.icons[3] = Rect::new(x + PAD_X, top, ICON_COL, heights[3]);
        top += heights[3] + PAD_Y;

        l.panel = Rect::new(x, y, w, top - y);
        l
    }

    /// The panel's height for `rig`, `w` wide.
    pub fn height(rig: &Rig, w: f32) -> f32 {
        RigLayout::of(rig, 0.0, 0.0, w).panel.h
    }

    /// Where a part shows, if it does.
    pub fn part(&self, part: Part) -> Option<Rect> {
        match part {
            Part::Device => Some(self.device),
            Part::Tone => (self.tone.w > 0.0).then_some(self.tone),
            _ => None,
        }
    }

    /// The button under a point.
    pub fn part_at(&self, x: f32, y: f32) -> Option<Part> {
        [Part::Device, Part::Tone]
            .into_iter()
            .find(|p| self.part(*p).is_some_and(|r| r.contains(x, y)))
    }
}

fn icon_in(r: Rect) -> Rect {
    Rect::new(
        r.x + (r.w - ICON) / 2.0,
        r.y + (r.h - ICON) / 2.0,
        ICON,
        ICON,
    )
}

/// An icon at the left of a part, in the middle of its height.
fn icon_at(r: Rect, x: f32) -> Rect {
    Rect::new(x, r.y + (r.h - ICON) / 2.0, ICON, ICON)
}

pub fn paint(pen: &mut Pen, studio: &Studio, rig: &Rig, l: &RigLayout) {
    let c = pen.c;
    pen.round(l.panel, c.surface_1, Radius::S);
    for (r, name) in l.icons.iter().zip(["volume", "mic", "midi", "plug"]) {
        pen.icon(name, icon_in(Rect::new(r.x, r.y, r.w, ROW_H)), c.ink);
    }
    let small = |pen: &mut Pen, s: &str, ink, r: Rect, align| {
        pen.text(s, FontSize::Small, Weight::Regular, ink, r, align);
    };

    // The output.
    if studio.hover == Hit::Entrance(Part::Device) {
        pen.round(l.device, c.hover, Radius::Clip);
    }
    let d = l.device;
    match &rig.device {
        Some(name) => {
            let text = Rect::new(d.x + LINK_PAD, d.y, width(name), d.h);
            small(pen, name, c.ink_strong, text, Align::Left);
            let chevron = icon_at(d, d.right() - LINK_PAD - ICON);
            pen.icon("chevron-down", chevron, c.ink_strong);
            pen.digits(
                &rig.info,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                l.info,
                Align::Left,
            );
            let t = l.tone;
            let (face, ink) = if rig.tone {
                (c.fill, c.on_fill)
            } else if studio.hover == Hit::Entrance(Part::Tone) {
                (c.thumb, c.ink_strong)
            } else {
                (c.surface_3, c.ink_strong)
            };
            pen.round(t, face, Radius::Clip);
            pen.icon("tone", icon_at(t, t.x + BUTTON_PAD), ink);
            let x = t.x + BUTTON_PAD + ICON + CHIP_GAP;
            pen.text(
                TONE,
                FontSize::Small,
                Weight::Medium,
                ink,
                Rect::new(x, t.y, t.right() - x, t.h),
                Align::Left,
            );
            let m = l.meter;
            for (i, a) in rig.out.iter().enumerate() {
                let y = m.y + i as f32 * (THIN + THIN_GAP);
                meter(pen, Rect::new(m.x, y, m.w, THIN), *a, Radius::Xs);
            }
        }
        None => {
            pen.icon("alert", icon_at(d, d.x + LINK_PAD), c.alert);
            let x = d.x + LINK_PAD + ICON + LINK_GAP;
            small(
                pen,
                NO_DEVICE,
                c.ink_alert,
                Rect::new(x, d.y, d.right() - x, d.h),
                Align::Left,
            );
        }
    }

    // The inputs.
    if rig.inputs.is_empty()
        && let Some(r) = l.inputs.first()
    {
        pen.icon("alert", icon_at(*r, r.x), c.alert);
        let x = r.x + ICON + CHIP_GAP;
        small(
            pen,
            NO_INPUT,
            c.ink_alert,
            Rect::new(x, r.y, r.right() - x, r.h),
            Align::Left,
        );
    }
    for ((name, level), r) in rig.inputs.iter().zip(&l.inputs) {
        pen.round(*r, c.surface_2, Radius::Clip);
        let text = Rect::new(r.x + CHIP_PAD, r.y, width(name), r.h);
        small(pen, name, c.ink_strong, text, Align::Left);
        let x = r.right() - CHIP_PAD - LEVEL_W;
        let bar = Rect::new(x, r.y + (r.h - LEVEL_H) / 2.0, LEVEL_W, LEVEL_H);
        meter(pen, bar, *level, Radius::Clip);
    }

    // MIDI.
    for ((name, lit), r) in rig.midi.iter().zip(&l.midi) {
        pen.round(*r, c.surface_2, Radius::Clip);
        let dot = Rect::new(r.x + CHIP_PAD, r.y + (r.h - DOT) / 2.0, DOT, DOT);
        midi(pen, dot, *lit);
        let x = dot.right() + CHIP_GAP;
        small(
            pen,
            name,
            c.ink_strong,
            Rect::new(x, r.y, r.right() - x, r.h),
            Align::Left,
        );
    }

    // The plugins.
    pen.digits(
        &rig.plugins_text(),
        FontSize::Small,
        Weight::Regular,
        c.ink,
        l.plugins,
        Align::Left,
    );
    if let (Some(bar), Some((done, total))) = (l.bar, rig.scanning) {
        pen.round(bar, c.well, Radius::Clip);
        let part = if total == 0 {
            0.0
        } else {
            (done.min(total) as f32 / total as f32).clamp(0.0, 1.0)
        };
        if part > 0.0 {
            pen.round(
                Rect::new(bar.x, bar.y, bar.w * part, bar.h),
                c.fill,
                Radius::Clip,
            );
        }
    }
    if let Some(r) = l.bad {
        pen.icon("alert", icon_at(r, r.x), c.alert);
        let x = r.x + ICON + CHIP_GAP;
        pen.digits(
            &rig.failed.to_string(),
            FontSize::Small,
            Weight::Regular,
            c.ink_alert,
            Rect::new(x, r.y, r.right() - x, r.h),
            Align::Left,
        );
    }
}

impl Studio {
    /// What the panel shows now.
    pub fn rig(&self) -> Rig {
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
        let v = &io.view;
        let device = v.devices.iter().find(|d| d.name == v.device);
        let info = v
            .latency
            .filter(|_| device.is_some())
            .map(|l| format!("{} · {:.1} ms", khz(l.rate), l.total_ms()))
            .unwrap_or_default();
        let inputs = device.map_or_else(Vec::new, |d| {
            d.inputs
                .iter()
                .enumerate()
                .map(|(i, n)| (n.clone(), v.levels.get(i).copied().unwrap_or(0.0)))
                .collect()
        });
        let midi = std::iter::once(PC_KEYBOARD.to_string())
            .chain(v.ports.iter().filter(|p| *p != PC_KEYBOARD).cloned())
            .map(|n| {
                let lit = v.received.contains(&n);
                (n, lit)
            })
            .collect();
        let catalog = self.mixer.catalog.as_deref();
        let scan = &self.mixer.scan;
        Rig {
            device: device.map(|d| d.name.clone()),
            info,
            inputs,
            midi,
            found: catalog.map_or(0, |c| c.entries.len()),
            failed: catalog.map_or(0, |c| c.failures.len()),
            scanning: scan.running().then_some((scan.done, scan.total)),
            out: self.rig_out,
            tone: self.rig_tone,
        }
    }

    /// Once a frame while the entrance or the settings' audio page shows: the output's level
    /// and the tone, and the plugin list read. True while something on the panel moves.
    pub(crate) fn rig_tick(&mut self) -> bool {
        let entrance = self.start_shown();
        let settings = self
            .settings
            .as_ref()
            .is_some_and(|ui| ui.section == crate::settings::Section::Audio);
        if !entrance && !settings {
            self.rig_out = [0.0; 2];
            self.rig_tone = false;
            return false;
        }
        if entrance && self.mixer.catalog.is_none() {
            self.plugin_catalog();
        }
        let peak = self.transport.take_output_peak();
        for (held, p) in self.rig_out.iter_mut().zip(peak) {
            *held = p.max(*held * FALL);
            if *held < 1e-4 {
                *held = 0.0;
            }
        }
        let tone = self.transport.tone_playing();
        let changed = tone != self.rig_tone;
        self.rig_tone = tone;
        let heard = {
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            let io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
            io.view.levels.iter().any(|l| *l > 0.0) || !io.view.received.is_empty()
        };
        changed || tone || heard || self.rig_out.iter().any(|a| *a > 0.0)
    }

    /// The list of the devices: each one (only those with inputs when `only_inputs`), its
    /// inputs and outputs, and the one in use.
    pub(crate) fn device_view(
        &mut self,
        only_inputs: bool,
    ) -> (Vec<crate::popup::Choice>, Vec<String>, String) {
        self.poll_io();
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
        let v = &io.view;
        let (choices, hints) = v
            .devices
            .iter()
            .filter(|d| !only_inputs || !d.inputs.is_empty())
            .map(|d| {
                let choice = crate::popup::Choice {
                    to: d.name.clone(),
                    label: d.name.clone(),
                };
                let hint = format!("入力 {} · 出力 {}", d.inputs.len(), d.outputs.len());
                (choice, hint)
            })
            .unzip();
        (choices, hints, v.device.clone())
    }

    /// A device of the list: it is used from now on. Chosen from 「オーディオを録音」, the
    /// armed track is made on it.
    pub(crate) fn device_action(&mut self, g: &crate::studio::Geometry, name: String) -> bool {
        let from_entry = self
            .popup
            .as_ref()
            .is_some_and(|p| p.kind == crate::popup::PopupKind::Device(true));
        self.popup = None;
        if self.transport.is_windowed() {
            if self.input_blocked() {
                return true;
            }
            self.io_command_on_worker(
                "audio.set",
                serde_json::json!({"key":"device","value":name})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            if let Some(save) = &mut self.saving {
                save.device_entry = from_entry.then_some(*g);
            }
            return true;
        }
        let set = self.song.set(
            "audio.set",
            serde_json::json!({ "key": "device", "value": name }),
        );
        if let Err(e) = set {
            self.error = Some(e);
            return true;
        }
        self.error = None;
        self.poll_io();
        if from_entry && self.device_has_inputs() {
            self.start_audio(g);
        }
        self.rebuild(g);
        true
    }

    /// 「テスト音」: the tone once, left then right, on the output in use.
    pub(crate) fn play_test_tone(&mut self) -> bool {
        let p = self.project();
        match self.transport.test_tone(&p, self.song.dir()) {
            Ok(_) => {
                self.error = None;
                self.rig_tone = true;
            }
            Err(e) => self.error = Some(e),
        }
        true
    }
}

/// `48000` as 「48 kHz」, `44100` as 「44.1 kHz」.
fn khz(rate: u32) -> String {
    let k = f64::from(rate) / 1000.0;
    if (k - k.round()).abs() < 1e-9 {
        format!("{} kHz", k.round())
    } else {
        format!("{k:.1} kHz")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_read_as_a_person_says_them() {
        assert_eq!(khz(48_000), "48 kHz");
        assert_eq!(khz(44_100), "44.1 kHz");
        assert_eq!(khz(96_000), "96 kHz");
    }

    #[test]
    fn a_meter_spans_sixty_decibels() {
        assert_eq!(level_width(0.0), 0.0);
        assert!((level_width(1.0) - 1.0).abs() < 1e-6);
        // −18 dBFS shows at seven tenths.
        let tone = 10f32.powf(-18.0 / 20.0);
        assert!((level_width(tone) - 0.7).abs() < 1e-3);
    }

    #[test]
    fn chips_wrap_onto_a_new_line_when_the_row_is_full() {
        let (rects, h) = flow(&[100.0, 100.0, 100.0], 0.0, 0.0, 250.0);
        assert_eq!(rects[1].y, rects[0].y);
        assert_eq!(rects[2].x, 0.0);
        assert_eq!(rects[2].y, CHIP_H + GAP);
        assert_eq!(h, 2.0 * CHIP_H + GAP);
        let (one, h) = flow(&[100.0], 0.0, 0.0, 250.0);
        assert_eq!((one[0].y, h), ((ROW_H - CHIP_H) / 2.0, ROW_H));
    }
}
