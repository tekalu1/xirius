//! A macro control's lane, the lane of a parameter it moves (broken, named
//! by the control) and a splitter branch's lane (AUT-018).
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    tree::RowKind,
};
use serde_json::json;
use xirius_core::id::TrackId;

pub const SHOTS: &[Shot] = &[Shot {
    name: "macro-lane01",
    acts: &[
        Act::Reset,
        Act::Stage(Play::Studio(setup), 0),
        Act::Press(Target::Track("Keys")),
        Act::Key("a"),
    ],
}];

fn track(s: &Studio) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id
}

fn setup(s: &mut Studio, _: u8) {
    s.drawer_open = false;
    let t = track(s);
    s.run_command("plugin.add", json!({"track":t,"name":"Reverb"}));
    let reverb = s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "Reverb")
        .unwrap()
        .id;
    let decay = format!("param:{reverb}:decay");
    s.run_command("rack.split.add", json!({"track":t}));
    let branch = s.project().tracks[&t]
        .rack
        .as_ref()
        .unwrap()
        .node_ids()
        .into_iter()
        .filter(|n| {
            s.project().tracks[&t]
                .rack
                .as_ref()
                .unwrap()
                .levels(*n)
                .is_some()
        })
        .nth(1)
        .unwrap();
    s.run_command(
        "control.assign",
        json!({"track":t,"control":"k1","target":decay,"min":0.1,"max":0.8}),
    );
    s.run_command(
        "control.rename",
        json!({"track":t,"control":"k1","name":"Space"}),
    );
    for (at, v) in [(0, 0.2), (7680, 0.9), (15360, 0.35), (23040, 0.6)] {
        s.run_command(
            "auto.point.add",
            json!({"track":t,"target":"control:k1","at":at,"value":v}),
        );
    }
    s.run_command("auto.lane.add", json!({"track":t,"target":decay}));
    for (at, v) in [(0, -6.0), (11520, -18.0), (19200, 0.0)] {
        s.run_command(
            "auto.point.add",
            json!({"track":t,"target":format!("branch:{branch}:gain"),"at":at,"value":v}),
        );
    }
    s.scroll.1 = s.tree().row_of(RowKind::Track(t)).unwrap().y;
    assert!(s.error.is_none(), "{:?}", s.error);
}
