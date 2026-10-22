//! `--script drum-names`: the drum rows named by the track's instrument (EDT-134). The Drums
//! track plays the test VST3, which names the kick and the snare; the hi-hat keeps its GM name,
//! and a name the user gives wins over the instrument's. Silent; no OS input.

use serde_json::json;

use crate::drum_view_shots::play as part;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};

pub const SHOTS: &[Shot] = &[
    // The kick and the snare named by the instrument, the hi-hat by GM.
    Shot {
        name: "dn01-instrument-names",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(play), 0),
        ],
    },
    // The kick renamed by the user: that name shows; the snare keeps the instrument's.
    Shot {
        name: "dn02-renamed",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 1),
        ],
    },
];

/// The test VST3 built next to this executable.
fn fixture() -> std::path::PathBuf {
    let base = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    [
        base.join("xirius_test_vst3.dll"),
        base.join("deps/xirius_test_vst3.dll"),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .expect("build xirius-test-vst3 first")
}

fn drums(s: &Studio) -> xirius_core::id::TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Drums")
        .unwrap()
        .id
}

/// The Drums track's instrument becomes the test VST3, and the player reads its names.
pub(crate) fn instrument(s: &mut Studio) {
    #[cfg(test)]
    crate::plugin_reload_shots::require_plugin_host();
    let entries = xirius_plugins::probe(&fixture()).unwrap();
    let entry = &entries[0];
    let track = drums(s);
    let prior: Vec<String> = s.project().tracks[&track]
        .inserts
        .iter()
        .filter(|x| x.kind == xirius_core::model::PluginKind::Instrument)
        .map(|x| x.id.to_string())
        .collect();
    if !prior.is_empty() {
        assert!(s.run_command("plugin.remove", json!({ "ids": prior })));
    }
    assert!(s.run_command(
        "plugin.add",
        json!({
            "track": track.to_string(), "name": entry.name, "kind": "instrument",
            "format": "vst3", "uid": entry.uid
        })
    ));
    s.use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    crate::shot::run_for(s, 15.0, |s| !s.roll_drum_rows().plugin.is_empty());
    assert!(s.error.is_none(), "{:?}", s.error);
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => instrument(s),
        _ => {
            let clip = s.selected_clip.unwrap().1.to_string();
            assert!(s.run_command(
                "roll.drum.rename",
                json!({ "clip": clip, "pitch": 36, "name": "808 Kick" })
            ));
        }
    }
    let g = &s.geometry(g.size.0, g.size.1);
    s.resized(g);
    if let Some(c) = s.roll_ctx(g) {
        s.roll.sync(&c);
        s.roll.center(&c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edt_134_the_drum_rows_take_the_instruments_names_after_the_users() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 900.0);
        part(&mut s, &g, 0);
        assert_eq!(s.roll_drum().as_deref(), Some("gm"));
        // Before the instrument is a plugin, the rows have the GM names.
        assert_eq!(s.roll_drum_rows().name(36), "キック");
        // The player was not running: showing the rows starts it so the names can be read.
        assert!(s.transport.host().is_none());
        play(&mut s, &g, 0);
        assert!(s.transport.host().is_some());
        assert!(!s.transport.playing, "it plays nothing");
        let rows = s.roll_drum_rows();
        for (pitch, name) in xirius_test_vst3::NOTE_NAMES {
            assert_eq!(rows.name(pitch), name);
        }
        assert_eq!(rows.name(42), "ハイハット（閉）", "a key it does not name");
        assert_eq!(rows.name(90), "ノート 90");
        assert!(rows.is_empty(), "the instrument's names are not the user's");
        play(&mut s, &g, 1);
        let rows = s.roll_drum_rows();
        assert_eq!(rows.name(36), "808 Kick");
        assert_eq!(rows.name(38), xirius_test_vst3::NOTE_NAMES[1].1);
        // 既定に戻す takes back only the user's name.
        let clip = s.selected_clip.unwrap().1.to_string();
        assert!(s.run_command("roll.drum.reset", json!({ "clip": clip })));
        assert_eq!(
            s.roll_drum_rows().name(36),
            xirius_test_vst3::NOTE_NAMES[0].1
        );
        // The names are not kept with the song.
        let saved = serde_json::to_string(&s.song.session().state.view.roll).unwrap();
        assert!(!saved.contains("Fixture"), "{saved}");
    }
}
