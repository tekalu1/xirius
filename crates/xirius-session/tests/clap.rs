//! A CLAP plugin a song names (PLG-028): it plays with its state in this process and in the
//! plugin host process, an edit in it comes into the song as its state, and that state plays
//! again when the song is opened anew. The plugin is `xirius-test-clap`, copied as a `.clap`
//! file and listed by the catalog's probe. Nothing opens an audio device: the test pulls the
//! engine.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_core::id::{ClipId, Node, PluginId, TrackId};
use xirius_core::model::{
    Clip, ClipBody, Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind,
};
use xirius_core::session::{RunOptions, Session};
use xirius_engine::AudioEngine;
use xirius_engine::server::find_host;
use xirius_plugins::{Catalog, decode_state, encode_state, probe};
use xirius_session::commands::shared_registry;
use xirius_session::host::{HostConfig, SessionHost, collect_plugin_states};
use xirius_session::plugins::Isolation;
use xirius_vst3::PluginState;

const GUARD: Duration = Duration::from_secs(60);
const PLUGIN: PluginId = PluginId(500);
const TRACK: TrackId = TrackId(100);

/// The test plugin as a `.clap` file, and a steady signal of 1.0 to play through it. The
/// signal's folder goes when dropped; the plugin stays loaded until the process ends, so its
/// copy lives under the target folder (written over by the next run) and not the temp folder,
/// in a folder of the test's own (`dir`: a copy loaded by another test cannot be written).
fn files(dir: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    xirius_test_clap::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let plugins = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(dir);
    std::fs::create_dir_all(&plugins).unwrap();
    let clap = plugins.join("Xirius Test Gain.clap");
    std::fs::copy(deps.join("xirius_test_clap.dll"), &clap).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("xirius-clap-session-")
        .tempdir()
        .unwrap();
    let wav = dir.path().join("ones.wav");
    let mut w = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for _ in 0..48_000 * 30 {
        w.write_sample(1.0f32).unwrap();
        w.write_sample(1.0f32).unwrap();
    }
    w.finalize().unwrap();
    (dir, clap, wav)
}

/// The plugin's state for a gain.
fn state(gain: f32) -> String {
    let mut component = b"XTCL".to_vec();
    component.extend(gain.to_le_bytes());
    component.extend(0u32.to_le_bytes());
    encode_state(&PluginState {
        component,
        controller: Vec::new(),
    })
}

fn song(wav: &Path, uid: &str, state: Option<String>) -> Project {
    let mut p = Project::new("clap");
    let mut track = Track::new(TRACK, TrackKind::Audio, "ones".to_string());
    track.clips.push(Clip {
        id: ClipId(300),
        name: "ones".into(),
        color: None,
        start: 0,
        length: i64::from(p.ppq) * 40,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: wav.to_string_lossy().into_owned(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    });
    track.inserts.push(Plugin {
        id: PLUGIN,
        name: xirius_test_clap::NAME.into(),
        kind: PluginKind::Effect,
        format: "clap".into(),
        uid: uid.into(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state,
        isolation: PluginIsolation::InProcess,
        sidechain: None,
    });
    p.tracks.insert(TRACK, Arc::new(track));
    p.root.push(Node::Track(TRACK));
    p
}

fn start(p: &Project, catalog: &Arc<Catalog>, isolation: Isolation) -> (SessionHost, AudioEngine) {
    let (mut host, audio, warnings) = SessionHost::start(
        p,
        HostConfig {
            catalog: Some(catalog.clone()),
            isolation: Some(isolation),
            ..HostConfig::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(host.play());
    (host, audio)
}

/// Waits until 40 blocks in a row come out at `level`. A plugin in its host plays dry until
/// it is open and when a block comes back late; that is not the plugin's level.
fn settles_at(host: &mut SessionHost, audio: &mut AudioEngine, level: f32) {
    let mut buffer = [0.0f32; 256 * 2];
    let mut run = 0;
    let end = Instant::now() + GUARD;
    while run < 40 {
        assert!(
            Instant::now() < end,
            "the output never settled at {level}: {:?}",
            &buffer[..4]
        );
        host.poll();
        audio.render(&mut buffer, 2);
        if buffer.iter().all(|v| (v - level).abs() < 1e-4) {
            run += 1;
        } else {
            run = 0;
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn a_clap_plugin_plays_with_its_state_and_an_edit_comes_into_the_song() {
    let (_dir, clap, wav) = files("session_clap");
    let entries = probe(&clap).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].format, "clap");
    let uid = entries[0].uid.clone();
    assert_eq!(uid, xirius_test_clap::ID);
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });

    // In this process, with the song's state.
    let p = song(&wav, &uid, Some(state(0.25)));
    let (mut host, mut audio) = start(&p, &catalog, Isolation::InProcess);
    settles_at(&mut host, &mut audio, 0.25);
    drop(audio);
    drop(host);

    let Some(exe) = find_host() else {
        eprintln!("the plugin host binary is not built; the separate process is not checked");
        return;
    };
    // In the host process, with the same state; the user turns the gain up in the plugin.
    let mut s = Session::with_registry(p, shared_registry());
    let (mut host, mut audio) = start(s.project(), &catalog, Isolation::Separate(exe));
    settles_at(&mut host, &mut audio, 0.25);
    let link = host.plugins().link(PLUGIN).unwrap();
    host.plugins()
        .server_of(PLUGIN)
        .unwrap()
        .simulate_edit(&link, xirius_test_clap::GAIN, &[0.5]);
    let end = Instant::now() + GUARD;
    let collected = loop {
        let mut buffer = [0.0f32; 256 * 2];
        audio.render(&mut buffer, 2);
        host.poll();
        let c = collect_plugin_states(&mut s, &mut host, &RunOptions::human()).unwrap();
        if let Some(r) = c.result {
            break r;
        }
        assert!(Instant::now() < end, "the state after the edit never came");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(collected.step.is_some(), "the edit is one undo step");
    let x = s.project().tracks[&TRACK].inserts[0].clone();
    let saved = decode_state(x.state.as_deref().unwrap()).unwrap();
    assert_eq!(&saved.component[..4], b"XTCL");
    assert_eq!(
        f32::from_le_bytes(saved.component[4..8].try_into().unwrap()),
        0.5
    );
    settles_at(&mut host, &mut audio, 0.5);
    drop(audio);
    drop(host);

    // The song as saved, opened anew in this process: the edit is in it.
    let (mut host, mut audio) = start(s.project(), &catalog, Isolation::InProcess);
    settles_at(&mut host, &mut audio, 0.5);
}

/// The kick's name as the plugin gives it for the song's state.
fn kick(host: &SessionHost) -> Option<String> {
    host.plugins()
        .note_names(PLUGIN)
        .into_iter()
        .find(|(k, _)| *k == xirius_test_clap::NOTE_NAMES[0].0)
        .map(|(_, n)| n)
}

#[test]
fn edt_134_the_names_a_clap_plugin_gives_its_keys_reach_the_loader_and_follow_its_state() {
    let (_dir, clap, wav) = files("session_clap_names");
    let catalog = Arc::new(Catalog {
        entries: probe(&clap).unwrap(),
        failures: Vec::new(),
    });
    let p = song(&wav, xirius_test_clap::ID, Some(state(0.25)));
    let want: Vec<(u8, String)> = xirius_test_clap::NOTE_NAMES
        .iter()
        .map(|&(k, n)| (k, n.to_owned()))
        .collect();

    // In this process: the names of the song's state, read when it opened.
    let (host, audio) = start(&p, &catalog, Isolation::InProcess);
    assert_eq!(kick(&host).as_deref(), Some(xirius_test_clap::SOFT_KICK));
    assert_eq!(host.plugins().note_names(PLUGIN)[1..], want[1..]);
    drop(audio);
    drop(host);

    let Some(exe) = find_host() else {
        eprintln!("the plugin host binary is not built; the separate process is not checked");
        return;
    };
    // In the host process: the names come with the opening, and again when the user's edit
    // changes them.
    let (mut host, mut audio) = start(&p, &catalog, Isolation::Separate(exe));
    settles_at(&mut host, &mut audio, 0.25);
    assert_eq!(kick(&host).as_deref(), Some(xirius_test_clap::SOFT_KICK));
    let link = host.plugins().link(PLUGIN).unwrap();
    host.plugins()
        .server_of(PLUGIN)
        .unwrap()
        .simulate_edit(&link, xirius_test_clap::GAIN, &[0.75]);
    let end = Instant::now() + GUARD;
    while host.plugins().note_names(PLUGIN) != want {
        assert!(
            Instant::now() < end,
            "the names never followed the edit: {:?}",
            host.plugins().note_names(PLUGIN)
        );
        let mut buffer = [0.0f32; 256 * 2];
        audio.render(&mut buffer, 2);
        host.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}
