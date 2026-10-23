//! A splitter in a song (ENG-074): its branches are summed where they meet, a branch's gain and
//! mute change the sound offline and in the running engine without a new mix, and flattening
//! the rack gives the line back. Nothing here opens an audio device.

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::AudioEngine;
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export;
use xirius_session::ext::ExtConfig;
use xirius_session::graph::{Select, song_end_tick};
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::tempo::TempoMap;

fn song(name: &str) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    s
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let args = match args {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    commands::run(s, None, name, &args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// An instrument track with one note through a bar.
fn voice(s: &mut Session) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": "A" })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 1.0 }),
    ));
    run(
        s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": 64, "length": 3840, "velocity": 100 }),
    );
    t
}

fn end_frame(p: &Project) -> u64 {
    let rate = f64::from(p.settings.sample_rate);
    TempoMap::from_project(p).frame_at(song_end_tick(p), rate)
}

fn render(s: &Session) -> (Vec<f32>, Vec<f32>) {
    let p = s.project();
    let mut b = Builder::new(p.settings.sample_rate, None);
    let r = export::render(&mut b, p, Select::All, 0, end_frame(p), 0).unwrap();
    (r.left, r.right)
}

fn rms(l: &[f32], r: &[f32]) -> f64 {
    let n = (l.len() + r.len()).max(1) as f64;
    let sum: f64 = l.iter().chain(r).map(|x| f64::from(*x).powi(2)).sum();
    (sum / n).sqrt()
}

fn level(s: &Session) -> f64 {
    let (l, r) = render(s);
    rms(&l, &r)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1e-9)
}

/// A running engine that is never given a device: the test pulls the audio itself.
struct Rig {
    host: SessionHost,
    audio: AudioEngine,
}

impl Rig {
    fn new(s: &Session) -> Rig {
        let (host, audio, warnings) =
            SessionHost::start(s.project(), HostConfig::default()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        Rig { host, audio }
    }

    fn pull(&mut self, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut out = vec![0.0f32; frames * 2];
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(480);
            self.audio.render(&mut out[done * 2..(done + n) * 2], 2);
            done += n;
        }
        let l = out.iter().step_by(2).copied().collect();
        let r = out.iter().skip(1).step_by(2).copied().collect();
        (l, r)
    }

    /// The song from start to end as the engine plays it now.
    fn level(&mut self, s: &Session) -> f64 {
        self.host.stop();
        self.pull(24000);
        let frames = end_frame(s.project()) as usize;
        self.host.seek_frame(0);
        self.host.play();
        let (l, r) = self.pull(frames);
        self.host.stop();
        self.pull(480);
        rms(&l, &r)
    }
}

/// A track with an EQ, and a splitter after it whose two branches are still empty.
fn split_song(name: &str) -> (Session, String, String, [String; 2]) {
    let mut s = song(name);
    let t = voice(&mut s);
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "EQ" }),
    ));
    let v = run(&mut s, "rack.split.add", json!({ "track": t })).data;
    let b = [0, 1].map(|i| v["branches"][i].as_str().unwrap().to_string());
    (s, t, eq, b)
}

#[test]
fn branches_are_summed_and_their_gain_and_mute_change_the_sound() {
    let base = {
        let mut plain = song("plain");
        voice(&mut plain);
        level(&plain)
    };
    assert!(base > 0.0);
    let (mut s, _, eq, b) = split_song("split");
    // Two empty branches are two copies of the line.
    assert!(near(level(&s), 2.0 * base, 1e-3), "{} vs {base}", level(&s));

    // The EQ at -6 dB in the first branch; the second still passes the line.
    run(&mut s, "rack.node.move", json!({ "id": eq, "to": b[0] }));
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": eq, "param": "output", "plain": "-6" }),
    );
    assert!(near(level(&s) / base, 1.501, 0.01), "{}", level(&s) / base);

    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": b[1], "mute": "on" }),
    );
    assert!(near(level(&s) / base, 0.501, 0.01), "{}", level(&s) / base);

    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": b[0], "gain": -6.0, "mute": "off" }),
    );
    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": b[1], "mute": "off" }),
    );
    assert!(near(level(&s) / base, 1.251, 0.01), "{}", level(&s) / base);
}

#[test]
fn a_branch_value_moves_in_the_running_engine_without_a_new_mix() {
    let (mut s, _, _, b) = split_song("live");
    let mut rig = Rig::new(&s);
    let both = rig.level(&s);
    assert!(near(both, level(&s), 1e-3), "{both} vs {}", level(&s));

    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": b[1], "mute": "on" }),
    );
    let rep = rig.host.sync(s.project());
    assert!(!rep.swapped, "{rep:?}");
    assert!(rep.commands > 0, "{rep:?}");
    let one = rig.level(&s);
    assert!(near(one / both, 0.5, 0.01), "{one} vs {both}");
    assert!(near(one, level(&s), 1e-3));

    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": b[0], "gain": -6.0 }),
    );
    let rep = rig.host.sync(s.project());
    assert!(!rep.swapped, "{rep:?}");
    let low = rig.level(&s);
    assert!(near(low / one, 0.501, 0.01), "{low} vs {one}");
    assert!(near(low, level(&s), 1e-3));
}

#[test]
fn flattening_gives_the_line_back_and_one_branch_sounds_like_the_line() {
    let (mut s, t, eq, b) = split_song("flat");
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": eq, "param": "output", "plain": "-6" }),
    );
    let (bl, br) = {
        let mut plain = song("plain");
        let pt = voice(&mut plain);
        let pe = id_of(&run(
            &mut plain,
            "plugin.add",
            json!({ "track": pt, "name": "EQ" }),
        ));
        run(
            &mut plain,
            "plugin.param.set",
            json!({ "id": pe, "param": "output", "plain": "-6" }),
        );
        render(&plain)
    };

    // Removing a branch leaves one, which is the line again: the same bits as without a rack.
    run(&mut s, "rack.node.move", json!({ "id": eq, "to": b[0] }));
    run(&mut s, "rack.branch.remove", json!({ "id": b[1] }));
    assert_eq!(render(&s), (bl.clone(), br.clone()));

    run(&mut s, "rack.split.add", json!({ "track": t }));
    assert!(s.project().tracks.values().any(|x| x.rack.is_some()));
    run(&mut s, "rack.flatten", json!({ "track": t }));
    assert!(s.project().tracks.values().all(|x| x.rack.is_none()));
    assert_eq!(render(&s), (bl, br));
}
