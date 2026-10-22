//! A second run of shots (`xirius-ui --shot <dir> --script more`): the automation
//! point's menu and value box, the quantize panel, a folder's bus and the mixer's input row.
//!
//! Kept apart from [`crate::shot::SCRIPT`] so the two lists grow without touching
//! each other; each stage is an [`Act::Stage`] played here.

use serde_json::json;

use crate::arrange::probe::{point_xy, two_lanes};
use crate::mixer::MixerHit;
use crate::quantize_panel::QAct;
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Hit, Studio};
use crate::topbar::BarButton;

/// Stage `n` of this script, played by [`stage`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

pub const MORE: &[Shot] = &[
    // CTX-027: the right-click menu of a volume point.
    Shot {
        name: "m01-auto-point-menu",
        acts: &[Act::Reset, staged(0)],
    },
    // AUT-008: its value typed into the box.
    Shot {
        name: "m02-auto-value",
        acts: &[staged(1)],
    },
    // EDT-073: the quantize panel from the grid pop-over, on the Keys clip's notes.
    Shot {
        name: "m03-quantize-grid",
        acts: &[Act::Reset, staged(2)],
    },
    // EDT-073: グルーヴ, taking another clip's starts.
    Shot {
        name: "m04-quantize-groove",
        acts: &[staged(3)],
    },
    // EDT-073: a folder connected to the bus Groove, and its bus menu (the mock's folder head).
    Shot {
        name: "m05-folder-bus",
        acts: &[Act::Reset, staged(4)],
    },
    // MIX-021: an instrument strip's input lists the MIDI ports by name (ports of the shot,
    // not this PC's); the audio strips name their default pair.
    Shot {
        name: "m06-mixer-midi-in",
        acts: &[Act::Reset, staged(5)],
    },
    // MIX-021: an audio strip's list, the default lit.
    Shot {
        name: "m07-mixer-audio-in",
        acts: &[staged(6)],
    },
];

/// Plays stage `stage` of [`MORE`].
fn track_named(s: &Studio, name: &str) -> Option<xirius_core::id::TrackId> {
    let p = s.project();
    p.tracks.values().find(|t| t.name == name).map(|t| t.id)
}

pub fn stage(s: &mut Studio, g: &Geometry, stage: u8) {
    match stage {
        0 => {
            s.drawer_open = false;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            let t = two_lanes(s, g, "Bass");
            let (x, y) = point_xy(s, g, t, 0, 1);
            s.scene_context(g, x, y);
        }
        1 => {
            let g = &s.geometry(g.size.0, g.size.1);
            s.context_choose(g, "値を入力…");
            for c in "-6.5".chars() {
                let c = c.to_string();
                s.typed(&c, Some(&c));
            }
        }
        2 => {
            s.select_clip_of("Keys");
            s.drawer_open = true;
            s.tab = crate::studio::DrawerTab::Roll;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            if let Some(c) = s.roll_ctx(g) {
                s.roll.sync(&c);
                s.roll.center(&c);
                s.roll.selected = c.notes().iter().map(|n| n.id).collect();
            }
            s.press(g, Hit::Bar(BarButton::Grid));
            s.quant_action(g, QAct::Open);
        }
        3 => {
            s.quant_action(g, QAct::Groove(true));
            let first = s
                .popup
                .as_ref()
                .and_then(|p| p.quant.as_ref())
                .and_then(|v| v.clips.first().map(|(id, _)| *id));
            if let Some(c) = first {
                s.quant_action(g, QAct::Clip(c));
            }
        }
        4 => {
            s.drawer_open = false;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            let p = s.project();
            let rhythm = p
                .folders
                .iter()
                .find(|(_, f)| f.name == "Rhythm")
                .map(|(id, _)| *id);
            drop(p);
            let Some(f) = rhythm else { return };
            let bus = s.song.run("track.add.bus", json!({ "name": "Groove" }));
            if let Ok(bus) = bus {
                s.run_command(
                    "folder.bus.set",
                    json!({ "id": f.to_string(), "bus": bus["id"] }),
                );
            }
            s.rebuild(g);
            s.press(g, Hit::FolderBus(f));
        }
        5 => {
            crate::input_row::set_test_ports(Some(vec![
                "KeyStep 37".into(),
                "loopMIDI Port".into(),
            ]));
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            if let Some(keys) = track_named(s, "Keys") {
                s.press(g, Hit::Mixer(MixerHit::Input(keys)));
            }
        }
        6 => {
            s.popup = None;
            if let Some(vocal) = track_named(s, "Vocal") {
                s.press(g, Hit::Mixer(MixerHit::Input(vocal)));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::PopupKind;

    #[test]
    fn the_automation_stages_leave_the_menu_then_the_typed_box_open() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        stage(&mut s, &g, 0);
        assert!(s.context_open());
        stage(&mut s, &g, 1);
        let p = s.popup.as_ref().expect("the value box");
        assert!(matches!(p.kind, PopupKind::AutoValue(_)));
        assert_eq!(p.naming.as_deref(), Some("-6.5"));
    }

    #[test]
    fn the_quantize_stages_open_the_panel_on_selected_notes_then_a_groove() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        stage(&mut s, &g, 2);
        let p = s.popup.as_ref().expect("the panel");
        assert_eq!(p.kind, PopupKind::Quantize);
        assert!(p.quant.as_ref().unwrap().can_apply);
        stage(&mut s, &g, 3);
        let v = s.popup.as_ref().unwrap().quant.as_ref().unwrap();
        assert!(v.set.groove_on && v.set.groove.is_some());
    }

    #[test]
    fn the_input_stages_open_the_port_list_then_the_channel_list() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        stage(&mut s, &g, 5);
        let p = s.popup.as_ref().expect("the port list");
        let labels: Vec<_> = p.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["すべての入力", "KeyStep 37", "loopMIDI Port"]);
        stage(&mut s, &g, 6);
        let p = s.popup.as_ref().expect("the channel list");
        assert_eq!(p.choices[0].label, "既定（In 1+2）");
        assert_eq!(p.current, "none");
        crate::input_row::set_test_ports(None);
    }

    #[test]
    fn the_folder_bus_stage_connects_groove_and_opens_the_menu() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        stage(&mut s, &g, 4);
        let p = s.popup.as_ref().expect("the menu");
        assert!(matches!(p.kind, PopupKind::FolderBus(_)));
        let l = p.layout(g.size);
        assert!(l.cells.iter().any(|c| c.text == "Groove" && c.on));
    }
}
