//! The mixer's insert flow, the track head and the channel editor in one set of scenes
//! (UI-338, UI-339, UI-340): a chain with a bypassed, a switched-off and a selected effect,
//! a splitter with an empty branch, the same chain in an opened track head and in the channel
//! editor, and the head and strips of every kind of track (the colour of a track's icon).
use crate::{
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
        name: "ux-head",
        acts: &[Act::Reset, stage(1)],
    },
    Shot {
        name: "ux-channel",
        acts: &[Act::Reset, stage(2)],
    },
    Shot {
        name: "ux-mixer-kinds",
        acts: &[Act::Reset, stage(3)],
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

fn setup(s: &mut Studio, g: &Geometry, n: u8) {
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
