//! The sidechain on the mixer (MIX-048): the key sends after a strip's sends with their bars,
//! the receiving compressor's mark and meter, the partner lit under the pointer, and the send
//! list's group サイドチェーン.
use crate::{
    mixer::MixerHit,
    shot::{Act, Part, Play, Shot, Target},
    studio::{DrawerTab, Geometry, Hit, Studio},
};
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "mixer-sc01",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 0)],
    },
    Shot {
        name: "mixer-sc02",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 1)],
    },
    Shot {
        name: "mixer-sc03",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 2)],
    },
    Shot {
        name: "mixer-sc04",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(setup), 3),
            Act::Press(Target::Strip(Part::AddSend, "Shaker")),
        ],
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

fn setup(s: &mut Studio, _: &Geometry, n: u8) {
    let drums = track(s, "Drums");
    let bass = track(s, "Bass");
    let keys = track(s, "Keys");
    builtin(s, bass, "eq");
    let comp = builtin(s, bass, "compressor");
    let pad = builtin(s, keys, "compressor");
    s.run_command(
        "plugin.sidechain.add",
        json!({ "id": comp.to_string(), "from": drums.to_string() }),
    );
    s.run_command(
        "plugin.sidechain.add",
        json!({ "id": pad.to_string(), "from": drums.to_string(), "level": -9.0 }),
    );
    s.instruments.open.clear();
    s.drawer_size.mixer = Some(600.0);
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.mixer.gr.insert(comp, 6.0);
    s.mixer.gr.insert(pad, 2.5);
    let key = crate::mixer::keys::key_sends(&s.project(), drums)[0].id;
    match n {
        0 => {}
        1 => s.hover = Hit::Mixer(MixerHit::KeySend(drums, key)),
        2 => s.hover = Hit::Mixer(MixerHit::Open(bass, comp)),
        // the send list of a strip that keys nothing yet
        _ => {}
    }
    assert!(s.error.is_none(), "{:?}", s.error);
}
