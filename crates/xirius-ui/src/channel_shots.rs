//! The channel editor (MIX-042): a vocal's chain with a splitter of three branches, the keys'
//! three layers, a branch under the pointer and a chosen card, as the mock's s03 shows them; the
//! keys' ranges under them (MIX-043), as v2's s04 does, with the first layer and then the third
//! chosen. Then the macro row and the macro window beside the editor (MIX-041), as v3's s06.
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
    Shot {
        name: "channel05",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 4)],
    },
    Shot {
        name: "channel06",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 5)],
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

/// Bass low with Compressor, Piano above it with EQ and Strings over Piano's loud notes with
/// Reverb, then Limiter. The third layer's instrument too.
fn keys(s: &mut Studio) -> (TrackId, PluginId) {
    let t = track(s, "Keys");
    for name in ["Bass", "Piano", "Strings"] {
        s.run_command("rack.layer.add", json!({ "track": t, "name": name }));
    }
    let layers = match &s.project().tracks[&t].rack.as_ref().unwrap().source {
        Some(Source::Multi { layers, .. }) => layers.clone(),
        other => panic!("{other:?}"),
    };
    for (l, (keys, velocity)) in
        layers
            .iter()
            .zip([("E0-B1", "1-127"), ("C2-C6", "1-100"), ("C2-C6", "80-127")])
    {
        let args = json!({ "id": l.id.to_string(), "keys": keys, "velocity": velocity });
        s.run_command("rack.layer.set", args);
    }
    for (l, name) in layers.iter().zip(["Compressor", "EQ", "Reverb"]) {
        let id = add(s, t, name);
        moved(s, id, l.id);
    }
    add(s, t, "Limiter");
    (t, layers[2].instrument)
}

fn setup(s: &mut Studio, g: &Geometry, n: u8) {
    s.instruments.open.clear();
    let (vocal, verb) = vocal(s);
    let (keys, strings) = keys(s);
    s.open_channel(if matches!(n, 1 | 4) { keys } else { vocal });
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
        // Strings chosen: the header names it and its box has the handles
        4 => {
            if let Some(w) = s.channel.as_mut() {
                w.chosen = Some(Pick::Plugin(strings));
            }
        }
        // the macro row on, and the macro window to the editor's right with its point at 40 % 70 %
        5 => {
            for (k, v) in [0.4, 0.7, 0.25, 0.5, 0.1, 0.8, 0.3, 0.6]
                .into_iter()
                .enumerate()
            {
                let args = json!({ "track": vocal, "control": format!("k{}", k + 1), "value": v });
                s.run_command("control.set", args);
            }
            if let Some(w) = s.channel.as_mut() {
                w.macros = true;
                w.at = Some((12.0, 56.0));
            }
            // beside the editor when the screen has the room, else over its lower part
            let right = s.channel_view(g).map_or(0.0, |v| v.r.right());
            let beside = right + 16.0 + crate::macro_pad::W <= g.size.0 - 12.0;
            let at = if beside {
                (right + 16.0, 56.0)
            } else {
                ((g.size.0 - crate::macro_pad::W - 12.0).max(0.0), 300.0)
            };
            s.open_macros(vocal);
            if let Some(w) = s.macro_pad.as_mut() {
                w.at = Some(at);
            }
        }
        _ => {}
    }
    assert!(s.error.is_none(), "{:?}", s.error);
}
