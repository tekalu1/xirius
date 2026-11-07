//! The splitter's branches in the strips (MIX-045, `splitter-branches.html`): an empty branch
//! just added and picked, its level row under it at 116, then with its pan at 252; then a
//! channel split's empty L and R, L picked. Then the same two at 252 with the channel editor
//! open over them, the pointer on the empty branch's head and on L's: the head's level and pan
//! (review 2, H2).
use crate::{
    mixer::flow::Pick,
    shot::{Act, Play, Shot},
    studio::{DrawerTab, Geometry, Studio},
};
use serde_json::json;
use xirius_core::{
    id::{NodeId, TrackId},
    rack::RackNode,
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "mute-switch-empty-116",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_switch), 0)],
    },
    Shot {
        name: "mute-switch-empty-252",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_switch), 1)],
    },
    Shot {
        name: "mute-switch-nested-116",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_switch), 2)],
    },
    Shot {
        name: "mute-switch-nested-252",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_switch), 3)],
    },
    Shot {
        name: "mute-switch-restored-empty-116",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_switch), 4)],
    },
    Shot {
        name: "mute-switch-restored-empty-252",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_switch), 5)],
    },
    Shot {
        name: "mute-switch-restored-nested-116",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_switch), 6)],
    },
    Shot {
        name: "mute-switch-restored-nested-252",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_switch), 7)],
    },
    Shot {
        name: "mute-switch-strip-empty-116",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_strip), 0)],
    },
    Shot {
        name: "mute-switch-strip-empty-252",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_strip), 1)],
    },
    Shot {
        name: "mute-switch-strip-nested-116",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_strip), 2)],
    },
    Shot {
        name: "mute-switch-strip-nested-252",
        acts: &[Act::Reset, Act::Stage(Play::Laid(mute_strip), 3)],
    },
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
    Shot {
        name: "splitter-branches04",
        acts: &[Act::Reset, Act::Stage(Play::Laid(editor), 1)],
    },
    Shot {
        name: "splitter-branches05",
        acts: &[Act::Reset, Act::Stage(Play::Laid(editor), 2)],
    },
];

fn mute_strip(s: &mut Studio, g: &Geometry, n: u8) {
    mute_switch(s, g, n);
    s.channel = None;
}

/// The approved open switches and the three empty branch names, with no pointer over them.
fn mute_switch(s: &mut Studio, _g: &Geometry, n: u8) {
    let t = track(s, "Vocal");
    assert!(s.run_command("rack.split.add", json!({"track":t,"branches":3})));
    let (sp, b) = split(s, t);
    let mut targets = vec![b[1]];
    if matches!(n, 2 | 3 | 6 | 7) {
        assert!(s.run_command(
            "rack.split.add",
            json!({"track":t,"at":format!("branch:{}",b[0])})
        ));
        let nested = crate::channel::split_of(&s.project(), t, sp)
            .unwrap()
            .branches[0]
            .fx
            .iter()
            .find_map(|node| {
                if let RackNode::Split(x) = node {
                    Some(x.clone())
                } else {
                    None
                }
            })
            .unwrap();
        for wide in [false, true] {
            s.mixer.folds.set(nested.id, true, wide, true);
        }
        targets.push(nested.branches[1].id);
    }
    for id in &targets {
        assert!(s.run_command("rack.branch.set", json!({"id":id,"mute":"on"})));
    }
    if n >= 4 {
        for id in targets {
            assert!(s.run_command("rack.branch.set", json!({"id":id,"mute":"off"})));
        }
    }
    for wide in [false, true] {
        s.mixer.folds.set(sp, false, wide, true);
    }
    s.mixer.chosen = None;
    if n % 2 == 1 {
        s.mixer.wide.insert(t);
    }
    s.drawer_size.mixer = Some(600.0);
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.open_channel(t);
    s.channel.as_mut().unwrap().pointer = None;
    s.channel.as_mut().unwrap().chosen = None;
    assert!(s.error.is_none(), "{:?}", s.error);
}

#[test]
fn mix_045_nested_switches_and_empty_names_survive_without_hover() {
    use crate::channel::{flow::Key, paint::resting_branch_name};
    for n in 0..8 {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        mute_switch(&mut s, &g, n);
        let t = track(&s, "Vocal");
        let v = s.channel_view(&g).unwrap();
        let count = if n >= 4 {
            0
        } else if n >= 2 {
            2
        } else {
            1
        };
        assert_eq!(v.flow.switches.len(), count);
        let strip = s
            .mixer_strips(&g)
            .into_iter()
            .find(|x| x.track == t)
            .unwrap();
        let f = s.strip_flow(&strip, &s.project().tracks[&t]);
        assert_eq!(crate::mixer::flow::switches(&f).len(), count);
        for col in &v.flow.columns {
            let (name, r) = resting_branch_name(col).unwrap();
            assert_eq!(name, format!("分岐 {}", col.n));
            assert!(r.w > 60.0);
        }
        for (_, keys) in &v.flow.switches {
            assert!(keys.iter().any(|k| matches!(k, Key::Node(_))));
        }
        let before = v.flow.switches.clone();
        let mut shifted = v.flow;
        shifted.shift(17.0, -9.0);
        for ((after, _), (before, _)) in shifted.switches.iter().zip(before) {
            assert_eq!(*after, before.translated(17.0, -9.0));
        }
        let muted: Vec<_> = shifted
            .columns
            .iter()
            .filter(|c| c.target.mute)
            .map(|c| c.id)
            .collect();
        for id in muted {
            assert!(s.run_command("rack.branch.set", json!({"id":id,"mute":"off"})));
        }
        assert!(s.channel_view(&g).unwrap().flow.switches.is_empty());
        let strip = s
            .mixer_strips(&g)
            .into_iter()
            .find(|x| x.track == t)
            .unwrap();
        assert!(
            crate::mixer::flow::switches(&s.strip_flow(&strip, &s.project().tracks[&t])).is_empty()
        );
    }
}

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

/// Stage `n` of [`setup`] at 252 with the picked one's channel editor open, the pointer on the
/// picked branch's head.
fn editor(s: &mut Studio, g: &Geometry, n: u8) {
    setup(s, n);
    let (t, b) = match s.mixer.chosen {
        Some((t, Pick::Branch(b))) => (t, b),
        other => panic!("{other:?}"),
    };
    s.mixer.wide.insert(t);
    s.open_channel(t);
    let v = s.channel_view(g).unwrap();
    let head = v.flow.columns.iter().find(|c| c.id == b).unwrap().head;
    s.channel_hover(g, head.x + head.w / 2.0, head.y + head.h / 2.0);
}
