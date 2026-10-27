//! A run of shots for the small fixes (`xirius-ui --shot <dir> --script small`): a folder's
//! new bus in one step, a stair in an automation lane, and a mono device's input read as the
//! window opens.
//!
//! Kept apart from the other scripts so the lists grow without touching each other; each
//! stage is an [`Act::Stage`] played here.

use crate::arrange::probe::{point_xy, two_lanes};
use crate::input_row::{note_channels, set_test_channels};
use crate::mixer::MixerHit;
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Hit, Studio};
use crate::topbar::BarButton;

/// Stage `n` of this script, played by [`stage`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

pub const SMALL: &[Shot] = &[
    // A folder's 新しいバス: the bus is made and connected in one step, and the history has
    // one row for it.
    Shot {
        name: "s01-folder-new-bus",
        acts: &[Act::Reset, staged(0)],
    },
    // CTX-027: 階段 in the point's menu, lit, and the line holding flat up to the next point.
    Shot {
        name: "s02-auto-stair",
        acts: &[Act::Reset, staged(1)],
    },
    // A mono device read as the window opens: the Vocal well says In 1 before any list
    // is opened.
    Shot {
        name: "s03-mixer-mono-in-1",
        acts: &[Act::Reset, staged(2)],
    },
    // Its list: the default is In 1, and there is no In 2.
    Shot {
        name: "s04-mixer-mono-list",
        acts: &[staged(3)],
    },
];

fn folder_named(s: &Studio, name: &str) -> Option<xirius_core::id::FolderId> {
    let p = s.project();
    p.folders
        .iter()
        .find(|(_, f)| f.name == name)
        .map(|(id, _)| *id)
}

/// The mixer open, with the device's channels read as the window does it, as one.
fn mono_mixer(s: &mut Studio, g: &Geometry) {
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = &s.geometry(g.size.0, g.size.1);
    s.resized(g);
    s.read_input_channels_with(|| 1);
    let begun = std::time::Instant::now();
    while s.input_channels.is_some() && begun.elapsed() < std::time::Duration::from_secs(5) {
        s.idle();
        // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn vocal(s: &Studio) -> Option<xirius_core::id::TrackId> {
    let p = s.project();
    p.tracks.values().find(|t| t.name == "Vocal").map(|t| t.id)
}

/// Plays stage `stage` of [`SMALL`].
pub fn stage(s: &mut Studio, g: &Geometry, stage: u8) {
    // Each stage starts from the two channels of the test tone.
    if stage != 3 {
        note_channels(2);
        set_test_channels(None);
    }
    match stage {
        0 => {
            s.drawer_open = false;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            let Some(f) = folder_named(s, "Rhythm") else {
                return;
            };
            s.press(g, Hit::FolderBus(f));
            s.folder_bus_action(g, crate::folder_bus::NEW.into());
            s.press(g, Hit::Bar(BarButton::History));
        }
        1 => {
            s.drawer_open = false;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            let t = two_lanes(s, g, "Bass");
            let (x, y) = point_xy(s, g, t, 0, 0);
            // Make the stair, then open the menu again on its shape.
            s.scene_context(g, x, y);
            s.context_choose(g, "次のポイントまでの形");
            s.context_choose(g, "階段");
            s.scene_context(g, x, y);
            s.context_choose(g, "次のポイントまでの形");
        }
        2 => mono_mixer(s, g),
        3 => {
            let g = &s.geometry(g.size.0, g.size.1);
            let Some(t) = vocal(s) else {
                return;
            };
            // The list reads the device again as it opens: one channel still.
            set_test_channels(Some(1));
            s.press(g, Hit::Mixer(MixerHit::Input(t)));
            set_test_channels(None);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::PopupKind;

    #[test]
    fn the_new_bus_stage_connects_a_new_bus_and_opens_the_history() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        let steps = s.song.steps();
        stage(&mut s, &g, 0);
        let f = folder_named(&s, "Rhythm").unwrap();
        assert!(s.project().folders[&f].bus.is_some());
        assert_eq!(s.song.steps(), steps + 1);
        assert!(matches!(
            s.popup.as_ref().map(|p| p.kind),
            Some(PopupKind::History)
        ));
    }

    #[test]
    fn the_stair_stage_leaves_the_menu_open_with_the_stair_lit() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        stage(&mut s, &g, 1);
        assert!(s.context_open());
        let lit: Vec<String> = s
            .context_rows()
            .into_iter()
            .filter(|r| r.on)
            .map(|r| r.label)
            .collect();
        assert_eq!(lit, ["階段"]);
        let p = s.project();
        let bass = p.tracks.values().find(|t| t.name == "Bass").unwrap();
        assert!(bass.automation[0].points[0].step);
    }

    #[test]
    fn the_mono_stages_name_in_1_in_the_well_and_the_list() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        stage(&mut s, &g, 2);
        let t = vocal(&s).unwrap();
        assert!(s.input_channels.is_none());
        assert_eq!(crate::input_row::label(&s.project().tracks[&t]), "In 1");
        stage(&mut s, &g, 3);
        let p = s.popup.as_ref().unwrap();
        assert!(matches!(p.kind, PopupKind::Input(_)));
        let cs = p.choices();
        let labels: Vec<&str> = cs.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["既定（In 1）", "In 1"]);
        // The next stage starts from two channels again.
        stage(&mut s, &g, 0);
        assert_eq!(crate::input_row::default_name(), "In 1+2");
    }
}
