//! `--script eng3`: what the engine's output side looks like on the screen — an instrument
//! track's output list with this PC's MIDI ports (ENG-028), and the right-click menu of a
//! selection that mixes a clip and automation points (CTX-007).

use std::collections::BTreeSet;

use xirius_core::auto::AutoTarget;

use crate::arrange::probe::{point_xy, track, two_lanes};
use crate::popup::Action;
use crate::shot::{Act, Part, Play, Shot, Target};
use crate::studio::{DrawerTab, Geometry, Studio};

/// Stage `n` of this script, played by [`stage`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

pub const ENG3: &[Shot] = &[
    // The output list of an instrument track: the buses, then the MIDI ports.
    Shot {
        name: "e01-midi-out-list",
        acts: &[staged(0), Act::Press(Target::Strip(Part::Output, "Keys"))],
    },
    // One port picked: the strip's output field names it.
    Shot {
        name: "e02-midi-out-set",
        acts: &[staged(1)],
    },
    // The same list when the picked port is no longer on this PC: it stays, lit.
    Shot {
        name: "e03-midi-out-gone",
        acts: &[staged(2), Act::Press(Target::Strip(Part::Output, "Keys"))],
    },
    // A clip and a volume point selected together, the point pressed: only what both have.
    Shot {
        name: "e04-mixed-menu",
        acts: &[Act::Reset, staged(3)],
    },
];

/// Plays stage `n` of [`ENG3`].
pub fn stage(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            crate::midi_out_row::set_test_ports(Some(vec![
                "loopMIDI Port".into(),
                "Microsoft GS Wavetable Synth".into(),
            ]));
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
        }
        1 => {
            s.popup_action(g, Action::Output("midi:loopMIDI Port".into()));
        }
        2 => {
            s.popup = None;
            crate::midi_out_row::set_test_ports(Some(vec!["Microsoft GS Wavetable Synth".into()]));
        }
        3 => {
            let lane = two_lanes(s, g, "Bass");
            let keys = track(s, "Keys");
            let clip = s.project().tracks[&keys].clips[0].id;
            let vol = AutoTarget::Volume;
            let tick = s.project().tracks[&lane]
                .automation
                .iter()
                .find(|l| l.target == vol)
                .map(|l| l.points[1].tick)
                .expect("the volume lane");
            s.arrange.clips = BTreeSet::from([(keys, clip)]);
            s.selected_clip = Some((keys, clip));
            s.arrange.points.select(lane, &vol, tick, false);
            let (x, y) = point_xy(s, g, lane, 0, 1);
            s.scene_context(g, x, y);
        }
        _ => {}
    }
}
