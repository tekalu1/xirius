//! ENG-077: the files a device reads (a Sampler's zones, a Drum Kit's pads) are the song's
//! files: saving gathers them as it does the clips', `clip.relink` points them elsewhere, and
//! a missing one is reported.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn text(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// A project with one instrument track whose Sampler lists `samples`.
fn project_with_samples(samples: &[String]) -> Project {
    let mut p = Project::new("s");
    let mut t = Track::new(TrackId(1), TrackKind::Instrument, "Keys");
    t.inserts.push(Plugin {
        id: PluginId(2),
        name: "Sampler".into(),
        kind: PluginKind::Instrument,
        format: "builtin".into(),
        uid: "sampler".into(),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        sidechain: None,
        samples: samples.to_vec(),
        isolation: PluginIsolation::default(),
    });
    p.tracks.insert(TrackId(1), std::sync::Arc::new(t));
    p.root.push(xirius_core::id::Node::Track(TrackId(1)));
    p
}

fn samples(s: &Session) -> Vec<String> {
    s.project().tracks[&TrackId(1)].inserts[0].samples.clone()
}

#[test]
fn saving_gathers_a_devices_files_into_the_song_folder() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let kick = outside.join("Kick.wav");
    fs::write(&kick, b"kick").unwrap();
    let mut s = Session::from_project(project_with_samples(&[text(&kick)]));
    let r = s
        .run_value(
            "project.save-as",
            json!({ "path": text(&song) }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(r.message.contains("1 個"), "{}", r.message);
    assert_eq!(
        fs::read(song.join("Media/Imports/Kick.wav")).unwrap(),
        b"kick"
    );
    assert_eq!(samples(&s), ["Media/Imports/Kick.wav"]);
}

#[test]
fn relink_points_a_devices_file_elsewhere() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let mut s = Session::from_project(project_with_samples(&["Media/Imports/Kick.wav".into()]));
    run(
        &mut s,
        "project.save-as",
        json!({ "path": text(&tmp.path().join("song")) }),
    );
    run(
        &mut s,
        "clip.relink",
        json!({ "files": [{ "from": "Media/Imports/Kick.wav", "to": "Media/Imports/Snare.wav" }] }),
    );
    assert_eq!(samples(&s), ["Media/Imports/Snare.wav"]);
}

#[test]
fn a_file_the_device_cannot_find_is_reported_and_stays() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let mut s = Session::from_project(project_with_samples(&["gone/kick.wav".into()]));
    let r = s
        .run_value(
            "project.save-as",
            json!({ "path": text(&tmp.path().join("song")) }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(r.message.contains("見つからない"), "{}", r.message);
    // The reference stays so it can be found later.
    assert_eq!(samples(&s), ["gone/kick.wav"]);
}
