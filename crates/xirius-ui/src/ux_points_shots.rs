//! The mixer's insert flow, the track head and the channel editor in one set of scenes
//! (UI-338, UI-339, UI-340): a chain with a bypassed, a switched-off and a selected effect,
//! a splitter with an empty branch, the same chain in an opened track head and in the channel
//! editor, and the head and strips of every kind of track (the colour of a track's icon).
use crate::{
    mixer::{
        MixerHit,
        flow::{Pick, Spot},
    },
    shot::{Act, Part, Play, Shot, Target},
    studio::{DrawerTab, Geometry, Studio},
};
use serde_json::json;
use xirius_core::{
    id::{PluginId, TrackId},
    rack::RackNode,
};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Laid(setup), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "ux-channel",
        acts: &[Act::Reset, stage(2)],
    },
    Shot {
        name: "ux-head",
        acts: &[Act::Reset, stage(1)],
    },
    Shot {
        name: "ux-mixer",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "ux-mixer-hover",
        acts: &[
            Act::Reset,
            stage(0),
            Act::Hover(Target::Strip(Part::Insert, "Vocal")),
        ],
    },
    Shot {
        name: "ux-mixer-kinds",
        acts: &[Act::Reset, stage(3)],
    },
    Shot {
        name: "ux-seam-bottom",
        acts: &[Act::Reset, stage(6)],
    },
    Shot {
        name: "ux-seam-branch-bottom",
        acts: &[Act::Reset, stage(7)],
    },
    Shot {
        name: "ux-seam-drag",
        acts: &[Act::Reset, stage(10)],
    },
    Shot {
        name: "ux-seam-empty",
        acts: &[Act::Reset, stage(8)],
    },
    Shot {
        name: "ux-seam-many",
        acts: &[Act::Reset, stage(9)],
    },
    Shot {
        name: "ux-seam-middle",
        acts: &[Act::Reset, stage(5)],
    },
    Shot {
        name: "ux-seam-top",
        acts: &[Act::Reset, stage(4)],
    },
];

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}

fn add(s: &mut Studio, t: TrackId, name: &str) -> PluginId {
    s.run_command("plugin.add", json!({"track":t,"name":name}));
    s.project().tracks[&t]
        .inserts
        .iter()
        .rev()
        .find(|x| x.name == name)
        .unwrap()
        .id
}

/// EQ, a bypassed Compressor, Reverb, a switched-off Delay, then a splitter of two branches
/// (Chorus in the first, the second empty).
fn chain(s: &mut Studio, name: &str) -> TrackId {
    let t = track(s, name);
    add(s, t, "EQ");
    let comp = add(s, t, "Compressor");
    s.run_command("plugin.bypass", json!({"ids":[comp]}));
    add(s, t, "Reverb");
    let delay = add(s, t, "Delay");
    s.run_command("plugin.deactivate", json!({"ids":[delay]}));
    let chorus = add(s, t, "Chorus");
    s.run_command("rack.split.add", json!({"track":t}));
    let split = s.project().tracks[&t]
        .rack
        .as_ref()
        .unwrap()
        .fx
        .iter()
        .find_map(|n| match n {
            RackNode::Split(x) => Some((x.id, x.branches.iter().map(|b| b.id).collect::<Vec<_>>())),
            RackNode::Plugin(_) => None,
        })
        .unwrap();
    s.run_command("rack.node.move", json!({"id":chorus,"to":split.1[0]}));
    t
}

/// The seam under the last row of a strip: the bottom of its flow.
fn bottom(s: &Studio, size: (f32, f32), t: TrackId) -> Spot {
    let g = &s.geometry(size.0, size.1);
    let strip = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .unwrap();
    s.strip_flow(&strip, &s.project().tracks[&t])
        .seams()
        .pop()
        .unwrap()
        .spot
}

/// The scenes of the insert point (UI-362): the line shown at the top, in the middle and at the
/// bottom of a chain, at the bottom of a branch, on a track with none, on a long chain, and the
/// line of a row held and moved.
fn seam_scene(s: &mut Studio, g: &Geometry, n: u8) {
    s.instruments.open.clear();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    match n {
        4 | 5 | 7 | 10 => {
            let vocal = chain(s, "Vocal");
            let first = s.project().tracks[&vocal].inserts[0].id;
            let second = s.project().tracks[&vocal].inserts[1].id;
            match n {
                4 => s.seam_show(vocal, Spot::Before(Pick::Plugin(first)), true),
                5 => s.seam_show(vocal, Spot::After(Pick::Plugin(second)), true),
                7 => {
                    let spot = bottom(s, g.size, vocal);
                    s.seam_show(vocal, spot, true);
                }
                _ => {
                    let g = &s.geometry(g.size.0, g.size.1);
                    let (x, y) = s.mixer_center(g, MixerHit::Open(vocal, first)).unwrap();
                    s.press_keys(g, x, y);
                    assert!(s.chain_down(g, x, y));
                    let (_, y2) = s.mixer_center(g, MixerHit::Open(vocal, second)).unwrap();
                    assert!(s.fx_move(x, y2 + 6.0));
                    s.seam_moved(g, x, y2 + 6.0, std::time::Instant::now());
                }
            }
        }
        6 | 9 => {
            let guitar = track(s, "Guitar");
            let names = [
                "EQ",
                "Compressor",
                "Reverb",
                "Delay",
                "Chorus",
                "EQ",
                "Compressor",
                "Reverb",
                "Delay",
                "Chorus",
                "EQ",
                "Compressor",
            ];
            for name in names.iter().take(if n == 6 { 3 } else { names.len() }) {
                add(s, guitar, name);
            }
            // a long chain is scrolled to its end, where its last seam is
            s.mixer.set_flow_offset(guitar, usize::MAX / 64);
            let spot = bottom(s, g.size, guitar);
            s.seam_show(guitar, spot, true);
        }
        _ => {}
    }
}

fn setup(s: &mut Studio, g: &Geometry, n: u8) {
    if n >= 4 {
        seam_scene(s, g, n);
        assert!(s.error.is_none(), "{:?}", s.error);
        return;
    }
    s.instruments.open.clear();
    let vocal = chain(s, if n == 1 { "Drums" } else { "Vocal" });
    match n {
        0 | 3 => {
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            s.drawer_size.mixer = Some(600.0);
        }
        1 => {
            s.press(g, crate::studio::Hit::Widen(vocal));
        }
        2 => {
            s.open_channel(vocal);
        }
        _ => {}
    }
    assert!(s.error.is_none(), "{:?}", s.error);
}
