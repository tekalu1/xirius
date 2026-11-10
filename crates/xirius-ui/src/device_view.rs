//! Opening and closing the view of a device never changes the device in the song (UI-337).
use crate::studio::Studio;
use xirius_core::id::PluginId;

impl Studio {
    pub(crate) fn close_device_view(&mut self, id: PluginId) {
        if self.instruments.note.is_some_and(|n| n.plugin == id) {
            self.window_note_off();
        }
        if self
            .instruments
            .grab
            .as_ref()
            .is_some_and(|g| g.plugin() == id)
        {
            let target = self.instruments.detached_target.replace(id);
            self.instrument_up();
            self.instruments.detached_target = target;
        }
        if self
            .instruments
            .keyboard_drag
            .is_some_and(|(plugin, _)| plugin == id)
        {
            self.instruments.keyboard_drag = None;
        }
        if self.instruments.sweep.is_some_and(|s| s.plugin == id) {
            self.device_sweep_end();
        }
        self.instruments.open.retain(|w| w.plugin != id);
        self.instruments.detached.remove(&id);
        crate::instrument::window::forget_turns(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::{Pick, current};

    #[test]
    fn every_builtin_opens_independently_from_plugin_and_mixer_entry_points() {
        use crate::mixer::{MixerHit, flow::Pick as Row};
        use crate::studio::DrawerTab;
        for name in [
            "Sampler",
            "Drum Kit",
            "EQ",
            "Compressor",
            "Limiter",
            "Reverb",
            "Delay",
        ] {
            let mut s = Studio::sample();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            let track = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Keys")
                .unwrap()
                .id;
            assert!(
                s.run_command(
                    "plugin.add",
                    serde_json::json!({"track": track, "name": name})
                ),
                "{name}: {:?}",
                s.error
            );
            let id = s.project().tracks[&track].inserts.last().unwrap().id;
            let before = serde_json::to_value(&s.project().tracks[&track].inserts).unwrap();
            s.instruments.open.clear();
            assert!(s.open_plugin(track, id));
            assert!(s.instruments.detached.contains(&id), "{name}");
            let g = s.geometry(1600.0, 1000.0);
            assert_eq!(s.instruments.frame(0, &g).w, 0.0);
            s.close_device_view(id);
            // The actual row press/release path also opens an independent view.
            let (x, y) = s.mixer_center(&g, MixerHit::Open(track, id)).unwrap();
            s.chosen_press(&g, x, y);
            assert!(s.chain_down(&g, x, y));
            assert!(s.fx_up(&g, x, y, false));
            assert_eq!(s.mixer.chosen, Some((track, Row::Plugin(id))));
            assert!(s.instruments.detached.contains(&id), "{name}");
            s.show_instrument(track, id);
            assert_eq!(
                s.instruments.open.iter().filter(|w| w.plugin == id).count(),
                1
            );
            s.instruments.detached_target = Some(id);
            let local = s.settled_geometry(700.0, 500.0);
            let (x, y) = s
                .instrument_center(&local, id, crate::instrument::window::Part::Close)
                .unwrap();
            assert!(s.instrument_down(&local, x, y));
            s.instruments.detached_target = None;
            s.resized(&g);
            s.tick();
            assert!(
                !s.instruments.is_open(id),
                "closing {name} must not reopen it"
            );
            assert_eq!(
                serde_json::to_value(&s.project().tracks[&track].inserts).unwrap(),
                before
            );
        }
    }

    #[test]
    fn opening_a_view_again_keeps_the_instance_values_and_pin_and_close_keeps_the_device() {
        let mut s = Studio::sample();
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(s.set_instrument(t, &Pick::built_in(crate::instrument::DEFAULT)));
        let id = current(&s.project().tracks[&t]).unwrap().id;
        s.show_instrument(t, id);
        s.instruments.open[0].pinned = true;
        s.run_command(
            "plugin.param.set",
            serde_json::json!({"id": id, "param": "cutoff", "value": 0.37}),
        );
        s.run_command("plugin.bypass", serde_json::json!({"ids": [id]}));
        assert!(s.error.is_none(), "{:?}", s.error);
        let before = serde_json::to_value(&s.project().tracks[&t].inserts).unwrap();
        for _ in 0..4 {
            s.show_instrument(t, id);
            assert_eq!(
                s.instruments.open.iter().filter(|w| w.plugin == id).count(),
                1
            );
            assert!(
                s.instruments
                    .open
                    .iter()
                    .find(|w| w.plugin == id)
                    .unwrap()
                    .pinned
            );
            assert!(s.instruments.detached.contains(&id));
            assert_eq!(
                serde_json::to_value(&s.project().tracks[&t].inserts).unwrap(),
                before
            );
        }
        s.instruments.detached_target = Some(id);
        let g = s.geometry(800.0, 600.0);
        let (x, y) = s
            .instrument_center(&g, id, crate::instrument::window::Part::Close)
            .unwrap();
        s.instrument_down(&g, x, y);
        assert!(!s.instruments.is_open(id));
        assert_eq!(
            serde_json::to_value(&s.project().tracks[&t].inserts).unwrap(),
            before
        );
    }

    #[test]
    fn replacing_an_unpinned_external_view_clears_its_surface_but_keeps_the_device() {
        let mut s = Studio::sample();
        let tracks: Vec<_> = s
            .project()
            .tracks
            .values()
            .filter(|t| t.kind == xirius_core::model::TrackKind::Instrument)
            .map(|t| t.id)
            .take(2)
            .collect();
        assert_eq!(tracks.len(), 2);
        let mut ids = Vec::new();
        for &track in &tracks {
            assert!(s.set_instrument(track, &Pick::built_in(crate::instrument::DEFAULT)));
            ids.push(current(&s.project().tracks[&track]).unwrap().id);
        }
        s.show_instrument(tracks[0], ids[0]);
        s.show_instrument(tracks[1], ids[1]);
        assert!(!s.instruments.is_open(ids[0]));
        assert!(!s.instruments.detached.contains(&ids[0]));
        assert_eq!(current(&s.project().tracks[&tracks[0]]).unwrap().id, ids[0]);
        s.instruments
            .open
            .iter_mut()
            .find(|w| w.plugin == ids[1])
            .unwrap()
            .pinned = true;
        s.show_instrument(tracks[0], ids[0]);
        assert!(s.instruments.is_open(ids[1]));
        assert!(s.instruments.detached.contains(&ids[1]));
        assert_eq!(s.instruments.open.len(), 2);
    }
}
