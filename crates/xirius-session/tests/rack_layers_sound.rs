//! A multi-instrument in a song (ENG-075): its layers are summed, a note outside a layer's key
//! range leaves that layer silent, and a layer's gain and mute change the sound offline and in
//! the running engine without a new mix. Nothing here opens an audio device.

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
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// An instrument track playing the built-in Sampler, with one E3 (64) through a bar.
fn voice(s: &mut Session) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": "A" })));
    run(
        s,
        "plugin.add",
        json!({ "track": t, "name": "Sampler", "kind": "instrument" }),
    );
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

/// A voice whose Sampler is the first layer of a multi-instrument, and a second Sampler layer.
fn layered(name: &str) -> (Session, String, String) {
    let mut s = song(name);
    let t = voice(&mut s);
    let second = id_of(&run(
        &mut s,
        "rack.layer.add",
        json!({ "track": t, "name": "Sampler" }),
    ));
    (s, t, second)
}

fn first_layer(s: &Session, t: &str) -> String {
    let v = s.project().tracks[&t.parse().unwrap()].rack_json();
    v["source"]["multi"]["layers"][0]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no layer in {v}"))
        .to_string()
}

fn end_frame(p: &Project) -> u64 {
    let rate = f64::from(p.settings.sample_rate);
    TempoMap::from_project(p).frame_at(song_end_tick(p), rate)
}

fn level(s: &Session) -> f64 {
    let p = s.project();
    let mut b = Builder::new(p.settings.sample_rate, None);
    let r = export::render(&mut b, p, Select::All, 0, end_frame(p), 0).unwrap();
    rms(&r.left, &r.right)
}

fn rms(l: &[f32], r: &[f32]) -> f64 {
    let n = (l.len() + r.len()).max(1) as f64;
    let sum: f64 = l.iter().chain(r).map(|x| f64::from(*x).powi(2)).sum();
    (sum / n).sqrt()
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

#[test]
fn layers_are_summed_and_their_ranges_gain_and_mute_change_the_sound() {
    let base = {
        let mut plain = song("plain");
        voice(&mut plain);
        level(&plain)
    };
    assert!(base > 0.0);
    let (mut s, t, second) = layered("layers");
    // Two layers of the same instrument take the note: twice the line.
    assert!(near(level(&s), 2.0 * base, 1e-3), "{} vs {base}", level(&s));

    // E3 (64) is above C1-B2: the second layer stays silent.
    run(
        &mut s,
        "rack.layer.set",
        json!({ "id": second, "keys": "C1-B2" }),
    );
    assert!(near(level(&s), base, 1e-3), "{} vs {base}", level(&s));
    // Nor does a soft range take a velocity of 100.
    run(
        &mut s,
        "rack.layer.set",
        json!({ "id": second, "keys": "0-127", "velocity": "1-63" }),
    );
    assert!(near(level(&s), base, 1e-3), "{} vs {base}", level(&s));

    run(
        &mut s,
        "rack.layer.set",
        json!({ "id": second, "velocity": "1-127", "gain": -6.0 }),
    );
    assert!(near(level(&s) / base, 1.501, 0.01), "{}", level(&s) / base);
    let first = first_layer(&s, &t);
    run(
        &mut s,
        "rack.layer.set",
        json!({ "id": first, "mute": "on" }),
    );
    assert!(near(level(&s) / base, 0.501, 0.01), "{}", level(&s) / base);

    // Taking the layer away gives the line back.
    run(&mut s, "rack.layer.remove", json!({ "id": second }));
    run(
        &mut s,
        "rack.layer.set",
        json!({ "id": first, "mute": "off" }),
    );
    assert!(near(level(&s), base, 1e-3), "{} vs {base}", level(&s));
}

#[test]
fn a_layer_value_moves_in_the_running_engine_without_a_new_mix() {
    let (mut s, _, second) = layered("live");
    let mut rig = Rig::new(&s);
    let both = rig.level(&s);
    assert!(near(both, level(&s), 1e-3), "{both} vs {}", level(&s));

    run(
        &mut s,
        "rack.layer.set",
        json!({ "id": second, "mute": "on" }),
    );
    let rep = rig.host.sync(s.project());
    assert!(!rep.swapped, "{rep:?}");
    assert!(rep.commands > 0, "{rep:?}");
    let one = rig.level(&s);
    assert!(near(one / both, 0.5, 0.01), "{one} vs {both}");
    assert!(near(one, level(&s), 1e-3));

    // A range is the structure: the engine takes a new mix for it.
    run(
        &mut s,
        "rack.layer.set",
        json!({ "id": second, "mute": "off", "keys": "C1-B2" }),
    );
    rig.host.sync(s.project());
    let narrowed = rig.level(&s);
    assert!(near(narrowed, one, 0.01), "{narrowed} vs {one}");
}
