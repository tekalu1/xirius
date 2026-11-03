//! The sidechain's seat on a compressor's window and its source list (UI-336): no source, one
//! listened to, two («Drums +1»), turned off, and the list open with a send, an output and a
//! channel the key would loop back from.
use crate::{
    instrument::window::Part,
    popup::PopupKind,
    shot::{Act, Play, Shot},
    studio::{Geometry, Studio},
};
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "key-sources01",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 0)],
    },
    Shot {
        name: "key-sources02",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 1)],
    },
    Shot {
        name: "key-sources03",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 2)],
    },
    Shot {
        name: "key-sources04",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 3)],
    },
    Shot {
        name: "key-sources05",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 4)],
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

fn builtin(s: &mut Studio, t: TrackId, uid: &str) -> PluginId {
    s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": uid, "kind": "effect", "format": "builtin", "uid": uid }),
    );
    s.project().tracks[&t].inserts.last().unwrap().id
}

fn key(s: &mut Studio, comp: PluginId, from: TrackId, more: serde_json::Value) {
    let mut args = json!({ "id": comp.to_string(), "from": from.to_string() });
    if let (Some(a), Some(m)) = (args.as_object_mut(), more.as_object()) {
        a.extend(m.clone());
    }
    s.run_command("plugin.sidechain.add", args);
}

fn setup(s: &mut Studio, g: &Geometry, n: u8) {
    let drums = track(s, "Drums");
    let bass = track(s, "Bass");
    let keys = track(s, "Keys");
    let shaker = track(s, "Shaker");
    let comp = builtin(s, bass, "compressor");
    match n {
        0 => {}
        1 => key(s, comp, drums, json!({})),
        _ => {
            key(s, comp, drums, json!({ "level": -6.0, "pre": true }));
            key(s, comp, keys, json!({ "output": true }));
        }
    }
    if n == 3 {
        s.run_command(
            "plugin.sidechain.enable",
            json!({ "id": comp.to_string(), "state": "off" }),
        );
    }
    if n == 4 {
        // Shaker waits on Bass: its key would loop
        let other = builtin(s, shaker, "compressor");
        key(s, other, bass, json!({}));
        s.run_command("track.add.bus", json!({}));
    }
    s.instruments.open.clear();
    s.show_instrument(bass, comp);
    if n == 4 {
        let p = s.project();
        let r = s.instruments.rect(0, g.size);
        let t = &p.tracks[&bass];
        let x = t.inserts.iter().find(|x| x.id == comp).unwrap();
        let anchor = s
            .instruments
            .part_rect(r, &p, t, x, x.builtin().unwrap(), Part::Sources);
        s.open_popup(PopupKind::KeySources(comp), anchor);
    }
    assert!(s.error.is_none(), "{:?}", s.error);
}
