//! The channel editor (MIX-042): a vocal's chain with a splitter of three branches, the keys'
//! three layers, a branch under the pointer and a chosen card, as the mock's s03 shows them.
use crate::{
    mixer::flow::Pick,
    shot::{Act, Play, Shot},
    studio::{Geometry, Studio},
};
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::rack::{RackNode, Source};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "channel01",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 0)],
    },
    Shot {
        name: "channel02",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 1)],
    },
    Shot {
        name: "channel03",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 2)],
    },
    Shot {
        name: "channel04",
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
    s.run_command("plugin.add", json!({ "track": t, "name": name }));
    s.project().tracks[&t].inserts.last().unwrap().id
}

fn moved(s: &mut Studio, id: PluginId, to: impl ToString) {
    s.run_command("rack.node.move", json!({ "id": id, "to": to.to_string() }));
}

/// Compressor, a splitter of EQ, Reverb and Delay at -6 dB, then Limiter.
fn vocal(s: &mut Studio) -> (TrackId, PluginId) {
    let t = track(s, "Vocal");
    add(s, t, "Compressor");
    let eq = add(s, t, "EQ");
    let verb = add(s, t, "Reverb");
    let delay = add(s, t, "Delay");
    s.run_command("rack.split.add", json!({ "track": t }));
    let split = match &s.project().tracks[&t].rack.as_ref().unwrap().fx[..] {
        [.., RackNode::Split(x)] => x.id,
        other => panic!("{other:?}"),
    };
    s.run_command("rack.branch.add", json!({ "split": split }));
    let branches: Vec<_> = crate::channel::split_of(&s.project(), t, split)
        .unwrap()
        .branches
        .iter()
        .map(|b| b.id)
        .collect();
    moved(s, eq, branches[0]);
    moved(s, verb, branches[1]);
    moved(s, delay, branches[2]);
    s.run_command(
        "rack.branch.set",
        json!({ "id": branches[2], "gain": -6.0 }),
    );
    add(s, t, "Limiter");
    (t, verb)
}

/// Piano with EQ, Strings with Reverb and Bass, then Limiter.
fn keys(s: &mut Studio) -> TrackId {
    let t = track(s, "Keys");
    for name in ["Piano", "Strings", "Bass"] {
        s.run_command("rack.layer.add", json!({ "track": t, "name": name }));
    }
    let layers = match &s.project().tracks[&t].rack.as_ref().unwrap().source {
        Some(Source::Multi { layers, .. }) => layers.iter().map(|l| l.id).collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };
    let eq = add(s, t, "EQ");
    moved(s, eq, layers[0]);
    let verb = add(s, t, "Reverb");
    moved(s, verb, layers[1]);
    add(s, t, "Limiter");
    t
}

fn setup(s: &mut Studio, g: &Geometry, n: u8) {
    s.instruments.open.clear();
    let (vocal, verb) = vocal(s);
    let keys = keys(s);
    s.open_channel(if n == 1 { keys } else { vocal });
    match n {
        // the pointer over the second branch: its head shows its parts
        2 => {
            let v = s.channel_view(g).unwrap();
            let col = &v.flow.columns[1];
            s.channel_hover(
                g,
                col.head.x + col.head.w / 2.0,
                col.head.y + col.head.h / 2.0,
            );
        }
        // Reverb chosen: its path lit, its branch's head shown
        3 => {
            if let Some(w) = s.channel.as_mut() {
                w.chosen = Some(Pick::Plugin(verb));
            }
        }
        _ => {}
    }
    assert!(s.error.is_none(), "{:?}", s.error);
}
