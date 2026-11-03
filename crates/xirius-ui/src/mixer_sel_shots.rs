//! A row picked on a strip (MIX-045): the picked Reverb with its level row and the ＋ after
//! it, the M of a row under the pointer, the rows of a touch, and the widened strip.
use crate::{
    mixer::{MixerHit, flow::Pick},
    shot::{Act, Play, Shot},
    studio::{DrawerTab, Geometry, Hit, Studio},
};
use serde_json::json;
use xirius_core::{
    id::{NodeId, PluginId, TrackId},
    rack::RackNode,
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "mixer-sel01",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 0)],
    },
    Shot {
        name: "mixer-sel02",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 1)],
    },
    Shot {
        name: "mixer-sel03",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 2)],
    },
    Shot {
        name: "mixer-sel04",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 3)],
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

fn split(s: &Studio, t: TrackId) -> Vec<NodeId> {
    let rack = s.project().tracks[&t].rack.clone().unwrap();
    rack.fx
        .iter()
        .find_map(|n| match n {
            RackNode::Split(x) => Some(x.branches.iter().map(|b| b.id).collect()),
            RackNode::Plugin(_) => None,
        })
        .unwrap()
}

fn setup(s: &mut Studio, g: &Geometry, n: u8) {
    let bass = track(s, "Bass");
    add(s, bass, "EQ");
    add(s, bass, "Delay");

    let vocal = track(s, "Vocal");
    add(s, vocal, "EQ");
    let comp = add(s, vocal, "Comp");
    let verb = add(s, vocal, "Reverb");
    s.run_command("rack.split.add", json!({"track":vocal}));
    let branches = split(s, vocal);
    s.run_command("rack.node.move", json!({"id":comp,"to":branches[0]}));
    s.run_command("rack.node.move", json!({"id":verb,"to":branches[1]}));
    s.run_command("rack.branch.set", json!({"id":branches[1],"gain":-6.0}));
    s.instruments.open.clear();

    match n {
        0 => s.mixer.chosen = Some((vocal, Pick::Plugin(verb))),
        1 => s.hover = Hit::Mixer(MixerHit::Open(vocal, comp)),
        2 => {
            s.mixer.coarse = true;
            s.mixer.chosen = Some((vocal, Pick::Plugin(verb)));
        }
        _ => {
            s.mixer.wide.insert(vocal);
            s.mixer.chosen = Some((vocal, Pick::Plugin(verb)));
        }
    }
    s.drawer_size.mixer = Some(600.0);
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    // as a click on the row would, the level row is brought into the strip's flow
    if let Some((t, pick)) = s.mixer.chosen {
        let g = s.geometry(g.size.0, g.size.1);
        s.reveal_pick(&g, t, pick);
    }
    assert!(s.error.is_none(), "{:?}", s.error);
}
