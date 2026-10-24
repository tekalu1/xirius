//! Connects the settings' observations to the running studio.
use crate::studio::Studio;
use std::time::Instant;

impl Studio {
    pub fn poll_io(&mut self) {
        let project = self.project();
        let dir = self.song.dir();
        let settings = self
            .song
            .settings()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = io.sync(&settings.prefs) {
            if let Some(ui) = &mut self.settings {
                ui.tell(e, None);
            }
            return;
        }
        let detecting = io.detecting();
        if self.transport.midimap != settings.midimap {
            self.transport.midimap = settings.midimap.clone();
        }
        if self.transport.recording {
            if let Some(control) = &self.transport.midi_control {
                control.update(&settings.prefs, |name| io.receiving(name, Instant::now()));
                control.assign(&settings.midimap);
                self.live_midi
                    .assign
                    .pending
                    .extend(control.take().into_iter().map(|t| (t.port, t.message)));
            }
        } else {
            self.transport.midi_control = None;
        }
        if self.transport.mode == xirius_session::output::OutputMode::Device {
            let prefs = settings.prefs.clone();
            self.transport.observe_inputs = detecting
                || project.root.is_empty()
                || self
                    .settings
                    .as_ref()
                    .is_some_and(|ui| ui.section == crate::settings::Section::Audio);
            if let Err(e) = self.transport.configure_io(&project, dir, prefs) {
                self.error = Some(e);
            }
            let (levels, latency) = self.transport.io_observation();
            io.observe(levels, latency);
        } else {
            self.transport.io_prefs = settings.prefs.clone();
        }
        if !detecting && let Err(e) = io.load_routes(settings.dir.as_deref()) {
            self.error = Some(e);
        }
        io.poll(Instant::now());
        if detecting
            && !io.detecting()
            && let Err(e) = io.save_routes(settings.dir.as_deref())
        {
            self.error = Some(e);
        }
        self.transport.route_output(io.view.output);
        // A device listed without inputs says so in the input row. A real device's channels are
        // read as the window opens (`read_input_channels`).
        if self.transport.mode != xirius_session::output::OutputMode::Device
            && let Some(d) = io.view.devices.iter().find(|d| d.name == io.view.device)
        {
            if d.inputs.is_empty() {
                crate::input_row::note_channels(0);
            } else if crate::input_row::noted_channels() == 0 {
                crate::input_row::note_channels(2);
            }
        }
        if !self.midi_ports.known().is_empty() {
            io.view.ports = self.midi_ports.known().to_vec();
        }
        if let Some(ui) = &mut self.settings {
            ui.io = io.view.clone();
            if !io.detecting() {
                ui.detect = None;
            }
        }
    }

    pub fn io_tooltip(&self) -> String {
        self.song
            .io
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .view
            .latency
            .map_or_else(
                || "設定".into(),
                |l| format!("入出力 {:.1} ms", l.total_ms()),
            )
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::Section;
    use crate::studio::Studio;
    use serde_json::json;

    /// Two simulated devices; nothing is opened.
    fn simulated_devices() -> Box<dyn xirius_engine::devices::Backend> {
        use xirius_engine::devices::{Device, Simulated};
        struct Quiet;
        impl xirius_engine::record::AudioInput for Quiet {
            fn channels(&self) -> usize {
                2
            }
            fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
                left.fill(0.0);
                right.fill(0.0);
            }
        }
        let device = |name: &str| Device {
            name: name.into(),
            inputs: vec!["In 1".into(), "In 2".into()],
            outputs: vec!["Out 1".into(), "Out 2".into()],
        };
        Box::new(Simulated::new(
            vec![device("Interface A"), device("Interface B")],
            Default::default(),
            Box::new(Quiet),
        ))
    }

    #[test]
    fn a_new_song_keeps_the_list_of_audio_devices_and_the_chosen_one() {
        let dir_tmp = tempfile::Builder::new()
            .prefix("xirius-ui-devices-")
            .tempdir()
            .unwrap();
        let dir = dir_tmp.path().join("ui-devices");
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Studio::sample();
        s.song.load_settings(&dir);
        s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
        s.song
            .io
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(simulated_devices());
        s.song
            .set(
                "audio.set",
                json!({"key": "device", "value": "Interface B"}),
            )
            .unwrap();
        s.poll_io();
        let names = |s: &Studio| -> Vec<String> {
            let io = s.song.io.lock().unwrap_or_else(|e| e.into_inner());
            io.view.devices.iter().map(|d| d.name.clone()).collect()
        };
        assert_eq!(names(&s), ["Interface A", "Interface B"]);

        s.new_song_sync();
        s.poll_io();
        assert_eq!(
            names(&s),
            ["Interface A", "Interface B"],
            "the list is kept"
        );
        s.open_settings(Section::Audio);
        s.poll_io();
        let ui = s.settings.as_ref().unwrap();
        assert_eq!(ui.io.device, "Interface B", "the chosen device stays");
        assert_eq!(ui.io.devices.len(), 2);
        // What the next song sets is written to the same folder.
        s.song
            .set(
                "audio.set",
                json!({"key": "device", "value": "Interface A"}),
            )
            .unwrap();
        let saved = std::fs::read_to_string(dir.join("prefs.toml")).unwrap();
        assert!(saved.contains("Interface A"), "{saved}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
