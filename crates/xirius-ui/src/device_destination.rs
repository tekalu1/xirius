//! Moving an editor moves its view, never the device in the song (UI-337).
use crate::popup::Choice;
use crate::studio::Studio;
use xirius_core::id::PluginId;

fn destinations(plugin: &xirius_core::model::Plugin) -> Vec<Choice> {
    let rows: &[(&str, &str)] = if plugin.format == "vst3" || plugin.builtin().is_some() {
        &[("external", "独立ウィンドウ")]
    } else {
        &[]
    };
    rows.iter()
        .map(|(to, label)| Choice {
            to: (*to).into(),
            label: (*label).into(),
        })
        .collect()
}

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
    pub(crate) fn device_destinations(&self, id: PluginId) -> (Vec<Choice>, String) {
        let p = self.project();
        let Some(plugin) = p
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|p| p.id == id)
        else {
            return (Vec::new(), String::new());
        };
        if plugin.format == "vst3" {
            return (destinations(plugin), "external".into());
        }
        if plugin.builtin().is_none() {
            return (Vec::new(), String::new());
        }
        (destinations(plugin), "external".into())
    }

    pub(crate) fn move_device_view(&mut self, id: PluginId, destination: &str) {
        if !matches!(destination, "mixer" | "internal" | "external") {
            return;
        }
        if !self.instruments.is_open(id) {
            return;
        }
        // Older callers may still ask for a docked or internal view. The device and
        // its editing state stay intact; only the presentation is migrated.
        self.instrument_up();
        self.device_sweep_end();
        self.instruments.detached.insert(id);
        if let Some(w) = self.instruments.open.iter_mut().find(|w| w.plugin == id) {
            w.docked = false;
        }
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
            assert!(s.mixer_split(&g).is_none());
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
    fn vst_destinations_offer_only_an_independent_window() {
        let mut s = Studio::sample();
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(s.set_instrument(t, &Pick::built_in(crate::instrument::DEFAULT)));
        let mut plugin = current(&s.project().tracks[&t]).unwrap().clone();
        plugin.format = "vst3".into();
        let menu = destinations(&plugin);
        assert_eq!(menu.len(), 1);
        assert_eq!(menu[0].to, "external");
        assert_eq!(menu[0].label, "独立ウィンドウ");
    }

    #[test]
    fn a_view_moves_without_changing_the_instance_values_or_pin_and_close_keeps_it() {
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
        for place in ["external", "external", "mixer", "internal", "external"] {
            s.move_device_view(id, place);
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
            assert_eq!(s.device_destinations(id).1, "external");
            assert!(!s.instruments.open[0].docked);
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
        s.move_device_view(ids[0], "external");
        s.show_instrument(tracks[1], ids[1]);
        assert!(!s.instruments.is_open(ids[0]));
        assert!(!s.instruments.detached.contains(&ids[0]));
        assert_eq!(current(&s.project().tracks[&tracks[0]]).unwrap().id, ids[0]);
        s.move_device_view(ids[1], "external");
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
