//! A control an automation lane plays is not moved by hand (UI-063): on a channel in Read, a
//! fader, a pan or send knob or a built-in plug-in's knob whose lane has points follows the lane,
//! so a drag, a controller's knob, "0 dB に戻す" and "値を入力…" leave it be. The pointer shows
//! the "not allowed" sign over it and the tooltip says why. Off plays no lane, and Touch, Latch
//! and Write take the control to write it, so those move as before.

use xirius_core::auto::{AutoMode, AutoTarget};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Track;

use crate::context::Item;
use crate::mixer::MixerHit;
use crate::studio::{Geometry, Hit, Rect, Studio};

/// Why the control does not move.
pub const REASON: &str = "オートメーション（Read）のため変更できません";

/// Does the lane of `target` on `track` hold its control?
pub fn locked(track: &Track, target: &AutoTarget) -> bool {
    track.auto_mode == AutoMode::Read && crate::auto_mark::present(track, target)
}

/// `item` greyed out with the reason when `locked`.
pub fn gate(item: Item, locked: bool) -> Item {
    if !locked {
        return item;
    }
    Item {
        reason: REASON.into(),
        ..item.enabled(false)
    }
}

impl Studio {
    /// Is the mixer's fader, pan or send knob `hit` held by its lane?
    pub fn mixer_locked(&self, hit: MixerHit) -> bool {
        let Some(target) = crate::arrange::write::target_of(hit) else {
            return false;
        };
        self.project()
            .tracks
            .get(&hit.track())
            .is_some_and(|t| locked(t, &target))
    }

    /// Is the knob `param` of the plug-in `plugin` on `track` held by its lane?
    pub fn param_locked(&self, track: TrackId, plugin: PluginId, param: &str) -> bool {
        let target = AutoTarget::Param(plugin, param.to_string());
        self.project()
            .tracks
            .get(&track)
            .is_some_and(|t| locked(t, &target))
    }

    /// The held control under the pointer, where its tooltip hangs.
    pub fn lock_hovered(&self, g: &Geometry) -> Option<Rect> {
        if self.mixer.drag.is_some() {
            return None;
        }
        let (x, y) = self.input_map.pointer;
        if let Some((track, plugin, param, r)) = self.knob_under(g, x, y) {
            return self.param_locked(track, plugin, param).then_some(r);
        }
        match self.hover {
            Hit::Mixer(m) if self.mixer_locked(m) => self.mixer_rect(g, m),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::CursorStyle;
    use serde_json::json;
    use xirius_core::id::TrackId;

    use super::REASON;
    use crate::instrument::window::Part;
    use crate::instrument::{DEFAULT, Pick, current};
    use crate::mixer::MixerHit;
    use crate::pointer::of_studio;
    use crate::studio::{DrawerTab, Geometry, Hit, Studio};

    fn track(s: &Studio, name: &str) -> TrackId {
        s.project()
            .tracks
            .values()
            .find(|t| t.name == name)
            .map(|t| t.id)
            .unwrap_or_else(|| panic!("no track {name}"))
    }

    /// The pointer moved to (x, y), as the window takes it.
    fn hover(s: &mut Studio, g: &Geometry, x: f32, y: f32) {
        s.input_map.pointer = (x, y);
        s.hover = s.hit(g, x, y);
    }

    fn tip(s: &Studio, g: &Geometry) -> Option<&'static str> {
        crate::hover_tip::pointer_tip(s, g).map(|(t, _)| t)
    }

    fn blocked(s: &Studio, g: &Geometry) -> bool {
        of_studio(s, g).style == CursorStyle::OperationNotAllowed
    }

    #[test]
    fn a_fader_or_knob_its_lane_plays_in_read_does_not_move_and_says_why() {
        let mut s = Studio::sample();
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        let g = s.geometry(1600.0, 1000.0);
        let t = track(&s, "Bass");
        let id = t.to_string();
        assert!(s.run_command("track.add.bus", json!({ "name": "Fx", "into": "root" })));
        let fx = track(&s, "Fx");
        assert!(s.run_command(
            "send.add",
            json!({ "ids": [id], "to": fx.to_string(), "level": -9 })
        ));
        let send = s.project().tracks[&t].sends[0].id;
        let bar = i64::from(s.project().ppq) * 4;
        for (target, v) in [
            ("volume".to_string(), -12.0),
            ("pan".to_string(), -0.5),
            (format!("send:{send}"), -20.0),
        ] {
            for at in [0, 4 * bar] {
                assert!(s.run_command(
                    "auto.point.add",
                    json!({ "track": id, "target": target, "at": at, "value": v })
                ));
            }
        }
        let before = s.project();
        let steps = s.song.steps();
        for (hit, reset) in [
            (MixerHit::Fader(t), "0 dB に戻す"),
            (MixerHit::Pan(t), "既定に戻す"),
            (MixerHit::SendLevel(t, send), "0 dB に戻す"),
        ] {
            let (x, y) = s.mixer_center(&g, hit).expect("on screen");
            hover(&mut s, &g, x, y);
            assert_eq!(s.hover, Hit::Mixer(hit));
            // The tooltip says why, and the pointer says no.
            assert_eq!(tip(&s, &g), Some(REASON), "{hit:?}");
            assert!(blocked(&s, &g), "{hit:?}");
            // A press and a drag, as the window sends them, move nothing.
            s.press(&g, Hit::Mixer(hit));
            assert!(s.drag_begin(&g, Hit::Mixer(hit), y));
            assert!(s.mixer.drag.is_none());
            assert!(!s.drag_move(y + 40.0, false));
            assert!(!s.drag_end());
            // Nor does a controller's knob.
            if !matches!(hit, MixerHit::SendLevel(..)) {
                assert!(!s.knob_move(hit, 10));
            }
            // The menu keeps its rows, greyed out with the reason.
            assert!(s.scene_context(&g, x, y));
            for label in [reset, "値を入力…"] {
                let row = s
                    .context_rows()
                    .into_iter()
                    .find(|r| r.label == label)
                    .unwrap_or_else(|| panic!("{hit:?}: no {label}"));
                assert!(!row.enabled, "{hit:?}: {label}");
                assert_eq!(row.reason, REASON);
            }
            s.popup = None;
        }
        assert_eq!(s.song.steps(), steps);
        let (was, now) = (&before.tracks[&t], &s.project().tracks[&t]);
        assert_eq!(
            (was.volume_db, was.pan, was.sends[0].level_db),
            (now.volume_db, now.pan, now.sends[0].level_db)
        );

        // Touch takes hold of the fader to write: it moves, without the reason.
        for mode in ["touch", "off"] {
            assert!(s.run_command("auto.mode", json!({ "ids": [id], "mode": mode })));
            let hit = MixerHit::Fader(t);
            let (x, y) = s.mixer_center(&g, hit).unwrap();
            hover(&mut s, &g, x, y);
            assert_ne!(tip(&s, &g), Some(REASON), "{mode}");
            assert!(!blocked(&s, &g), "{mode}");
            assert!(s.drag_begin(&g, Hit::Mixer(hit), y));
            assert!(s.drag_move(y + 40.0, false), "{mode}");
            assert!(s.drag_end());
        }
        assert!(s.project().tracks[&t].volume_db < before.tracks[&t].volume_db);
    }

    #[test]
    fn a_plugin_knob_its_lane_plays_does_not_turn_and_says_why() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        let t = track(&s, "Keys");
        assert!(s.set_instrument(t, &Pick::built_in(DEFAULT)));
        let plugin = current(&s.project().tracks[&t]).cloned().unwrap();
        assert!(s.press(&g, Hit::Keys(t)));
        assert!(s.instruments.is_open(plugin.id));
        let def = plugin.builtin().expect("built in");
        let held = def.params[0].id;
        assert!(s.run_command(
            "auto.point.add",
            json!({
                "track": t.to_string(),
                "target": format!("param:{}:{held}", plugin.id),
                "at": 0,
                "value": 0.25
            })
        ));
        let params = |s: &Studio| current(&s.project().tracks[&t]).unwrap().params.clone();
        let (before, steps) = (params(&s), s.song.steps());

        let (x, y) = s.instrument_center(&g, plugin.id, Part::Knob(0)).unwrap();
        hover(&mut s, &g, x, y);
        assert_eq!(tip(&s, &g), Some(REASON));
        assert!(blocked(&s, &g));
        assert!(s.instrument_down(&g, x, y));
        assert!(!s.instrument_moved(&g, x, y - 40.0, false));
        assert!(!s.instrument_up());
        assert_eq!((params(&s), s.song.steps()), (before, steps));

        // The knob next to it has no lane: it turns, in one step.
        let (x, y) = s.instrument_center(&g, plugin.id, Part::Knob(1)).unwrap();
        hover(&mut s, &g, x, y);
        assert_ne!(tip(&s, &g), Some(REASON));
        assert!(!blocked(&s, &g));
        assert!(s.instrument_down(&g, x, y));
        assert!(s.instrument_moved(&g, x, y - 40.0, false));
        assert!(s.instrument_up());
        assert_eq!(s.song.steps(), steps + 1);
    }
}
