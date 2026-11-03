//! The audio devices and the test tone (UI-260・UI-350): the list of the devices, a device
//! chosen from it, and 「テスト音」 with the output's meters on 設定 › オーディオ › 出力.

use crate::studio::Studio;

mod meter;
pub use meter::level_width;

/// How much of the output's level is left the next frame.
const FALL: f32 = 0.85;

pub const NO_DEVICE: &str = "オーディオデバイスなし";
pub const TONE: &str = "テスト音";

impl Studio {
    /// Is there an audio device to play on? Without one, the start screen and the new song's
    /// screen say 「オーディオデバイスなし」 (UI-350).
    pub fn has_audio_device(&self) -> bool {
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
        let v = &io.view;
        v.devices.iter().any(|d| d.name == v.device)
    }

    /// Once a frame while the settings' audio page shows: the output's level and the tone.
    /// True while the meters move.
    pub(crate) fn rig_tick(&mut self) -> bool {
        let settings = self
            .settings
            .as_ref()
            .is_some_and(|ui| ui.section == crate::settings::Section::Audio);
        if !settings {
            self.rig_out = [0.0; 2];
            self.rig_tone = false;
            return false;
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
        changed || tone || self.rig_out.iter().any(|a| *a > 0.0)
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
        // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meter_spans_sixty_decibels() {
        assert_eq!(level_width(0.0), 0.0);
        assert!((level_width(1.0) - 1.0).abs() < 1e-6);
        // −18 dBFS shows at seven tenths.
        let tone = 10f32.powf(-18.0 / 20.0);
        assert!((level_width(tone) - 0.7).abs() < 1e-3);
    }
}
