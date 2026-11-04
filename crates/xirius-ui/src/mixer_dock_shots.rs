//! The mixer opened out (MIX-046): the face of the device picked on a strip at the right of
//! the strips, and the strips folded to bands beside the strip in front when the face is wide.
use crate::{
    mixer::flow::Pick,
    shot::{Act, Play, Shot},
    studio::{DrawerTab, Geometry, Studio},
};
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "mixer-dock01",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 0)],
    },
    Shot {
        name: "mixer-dock02",
        acts: &[Act::Reset, Act::Stage(Play::Laid(setup), 1)],
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

fn setup(s: &mut Studio, g: &Geometry, n: u8) {
    let bass = track(s, "Bass");
    add(s, bass, "EQ");
    let vocal = track(s, "Vocal");
    let comp = add(s, vocal, "Compressor");
    let eq = add(s, vocal, "EQ");
    add(s, vocal, "Reverb");
    s.instruments.open.clear();
    s.drawer_size.mixer = Some(600.0);
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(g.size.0, g.size.1);
    let pick = if n == 0 { comp } else { eq };
    s.mixer.chosen = Some((vocal, Pick::Plugin(pick)));
    s.dock_follow(&g);
    if n == 1 {
        // the face pulled wide, past where the strips fold
        let i = s.instruments.dock_slot().unwrap();
        s.instruments.open[i].body.0 = crate::mixer::dock::widest(g.drawer_body);
        s.mixer.lead = Some(vocal);
    }
    s.reveal_pick(&g, vocal, Pick::Plugin(pick));
    assert!(s.error.is_none(), "{:?}", s.error);
}
