//! The mixer strips' flow (MIX-044): a multi-instrument with an effect in a layer, a
//! bypassed effect, a splitter with a quieter branch, then the same splitter folded, then
//! all of it in the drawer's first height, where the flow scrolls.
use crate::{
    shot::{Act, Play, Shot},
    studio::{DrawerTab, Studio},
};
use serde_json::json;
use xirius_core::{
    id::{NodeId, PluginId, TrackId},
    rack::{RackNode, Source},
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "mixer-flow01",
        acts: &[Act::Reset, Act::Stage(Play::Studio(setup), 0)],
    },
    Shot {
        name: "mixer-flow02",
        acts: &[Act::Reset, Act::Stage(Play::Studio(setup), 1)],
    },
    Shot {
        name: "mixer-flow03",
        acts: &[Act::Reset, Act::Stage(Play::Studio(setup), 2)],
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

fn split(s: &Studio, t: TrackId) -> (NodeId, Vec<NodeId>) {
    let rack = s.project().tracks[&t].rack.clone().unwrap();
    rack.fx
        .iter()
        .find_map(|n| match n {
            RackNode::Split(x) => Some((x.id, x.branches.iter().map(|b| b.id).collect())),
            RackNode::Plugin(_) => None,
        })
        .unwrap()
}

fn setup(s: &mut Studio, n: u8) {
    let keys = track(s, "Keys");
    s.run_command("rack.layer.add", json!({"track":keys,"name":"Piano"}));
    s.run_command("rack.layer.add", json!({"track":keys,"name":"Pad"}));
    let layer = match &s.project().tracks[&keys].rack.as_ref().unwrap().source {
        Some(Source::Multi { layers, .. }) => layers[1].id,
        _ => unreachable!(),
    };
    let chorus = add(s, keys, "Chorus");
    s.run_command("rack.node.move", json!({"id":chorus,"to":layer}));
    add(s, keys, "Reverb");

    let bass = track(s, "Bass");
    add(s, bass, "EQ");
    let delay = add(s, bass, "Delay");
    s.run_command("plugin.bypass", json!({"ids":[delay]}));

    let vocal = track(s, "Vocal");
    add(s, vocal, "EQ");
    let comp = add(s, vocal, "Comp");
    let verb = add(s, vocal, "Reverb");
    s.run_command("rack.split.add", json!({"track":vocal}));
    let (sp, branches) = split(s, vocal);
    s.run_command("rack.node.move", json!({"id":comp,"to":branches[0]}));
    s.run_command("rack.node.move", json!({"id":verb,"to":branches[1]}));
    s.run_command("rack.branch.set", json!({"id":branches[1],"gain":-6.0}));
    if n == 1 {
        s.mixer.folded.insert(sp);
    }

    // the mock's strips are as tall as the flow; the third keeps the height the drawer opens with
    if n < 2 {
        s.drawer_size.mixer = Some(600.0);
    }
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    assert!(s.error.is_none(), "{:?}", s.error);
}
