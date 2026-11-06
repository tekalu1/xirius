//! The splitter's branches in the strips (MIX-045, `splitter-branches.html`): an empty branch
//! just added and picked, its level row under it at 116, then with its pan at 252; then a
//! channel split's empty L and R, L picked.
use crate::{
    mixer::flow::Pick,
    shot::{Act, Play, Shot},
    studio::{DrawerTab, Studio},
};
use serde_json::json;
use xirius_core::{
    id::{NodeId, TrackId},
    rack::RackNode,
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "splitter-branches01",
        acts: &[Act::Reset, Act::Stage(Play::Studio(setup), 0)],
    },
    Shot {
        name: "splitter-branches02",
        acts: &[Act::Reset, Act::Stage(Play::Studio(setup), 1)],
    },
    Shot {
        name: "splitter-branches03",
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

/// The track's splitter and its branches.
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
    let vocal = track(s, "Vocal");
    s.run_command("plugin.add", json!({"track":vocal,"name":"EQ"}));
    s.run_command("plugin.add", json!({"track":vocal,"name":"Comp"}));
    let comp = s.project().tracks[&vocal].inserts.last().unwrap().id;
    s.run_command("rack.split.add", json!({"track":vocal}));
    let (sp, b) = split(s, vocal);
    s.run_command("rack.node.move", json!({"id":comp,"to":b[0]}));
    s.run_command("rack.branch.add", json!({"split":sp}));
    let added = *split(s, vocal).1.last().unwrap();
    s.mixer.chosen = Some((vocal, Pick::Branch(added)));
    if n == 1 {
        s.mixer.wide.insert(vocal);
    }
    if n == 2 {
        let bass = track(s, "Bass");
        s.run_command("rack.split.add", json!({"track":bass,"mode":"channel"}));
        let lr = split(s, bass).1;
        s.mixer.chosen = Some((bass, Pick::Branch(lr[0])));
    }
    s.drawer_size.mixer = Some(600.0);
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    assert!(s.error.is_none(), "{:?}", s.error);
}
