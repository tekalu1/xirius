//! VST3 plugins of a song in the plugin host process (OPEN-02, PLG-022, PLG-023, PLG-024): the
//! export matches the plugin in process, a crash leaves the dry signal, a restart brings the
//! state back, and what the user changes in the editor comes into the song as one undo step.
//! Each plugin runs where the song says: in process, on the shared host or on one of its own;
//! a host that goes down leaves the plugins on the others playing, and a plugin that moves
//! keeps its state. Nothing here opens an audio device: the engine is pulled by the test.
//!
//! The tests skip when kHs Gain is not installed or `xirius-plugin-host` has not been built
//! (`cargo test --workspace` builds it).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::model::{PluginIsolation, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::AudioEngine;
use xirius_engine::server::{DEFAULT_LATENCY, EditPhase, find_host};
use xirius_engine::wire::Fault;
use xirius_plugins::{Catalog, encode_state, probe, uid_hex};
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, Options, Progress, RangeSpec};
use xirius_session::graph::Select;
use xirius_session::host::{HostConfig, SessionHost, collect_plugin_states};
use xirius_session::plugins::Isolation;

const RATE: u32 = 48000;
const AMP: f32 = 0.5;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-isolation-test-{}-{n}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let Value::Object(raw) = args else {
        unreachable!()
    };
    commands::run(s, None, name, &raw, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn write_sine(path: &Path, secs: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(secs * RATE as f32) as usize {
        let x = (i as f32 / RATE as f32 * 440.0 * std::f32::consts::TAU).sin() * AMP;
        w.write_sample(x).unwrap();
    }
    w.finalize().unwrap();
}

fn rms(x: &[f32]) -> f64 {
    let sum: f64 = x.iter().map(|v| f64::from(*v).powi(2)).sum();
    (sum / x.len().max(1) as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// The level of the dry sine.
fn unity() -> f64 {
    f64::from(AMP) / 2f64.sqrt()
}

/// kHs Gain, the host, a state made by the plugin itself with the gain turned down, and how
/// many dB down that is. `None` (the test skips) when either is missing.
struct Gain {
    host: PathBuf,
    catalog: Arc<Catalog>,
    uid: String,
    quiet_state: String,
    quiet_db: f64,
    /// The gain's own value (0 dB).
    unity_value: f64,
}

fn gain() -> Option<Gain> {
    let path = xirius_vst3::default_vst3_dir().join("Kilohearts/kHs Gain.vst3");
    if !path.exists() {
        eprintln!("skipped: kHs Gain is not installed");
        return None;
    }
    let Some(host) = find_host() else {
        eprintln!("skipped: xirius-plugin-host is not built");
        return None;
    };
    let entries = probe(&path).expect("probe");
    let uid = entries[0].uid.clone();
    let module = Arc::new(xirius_vst3::Module::load(&path).unwrap());
    let class = module
        .classes()
        .into_iter()
        .find(|c| uid_hex(&c.cid) == uid)
        .unwrap();
    let plugin = xirius_vst3::Plugin::new(module, &class).unwrap();
    let p = plugin
        .params()
        .into_iter()
        .find(|p| p.title == "Gain")
        .unwrap();
    assert!(plugin.set_param(p.id, 0.3));
    let text = plugin.param_text(p.id, 0.3).unwrap();
    let quiet_db: f64 = text
        .trim()
        .replace('\u{2212}', "-")
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let quiet_state = encode_state(&plugin.get_state().unwrap());
    let unity_value = p.default;
    Some(Gain {
        host,
        catalog: Arc::new(Catalog {
            entries,
            failures: vec![],
        }),
        uid,
        quiet_state,
        quiet_db,
        unity_value,
    })
}

/// A saved song with an audio track playing a sine for `secs` and kHs Gain on it.
fn song(dir: &Dir, g: &Gain, secs: f32) -> (Session, PathBuf, String) {
    let song_dir = dir.path().join("song");
    let mut s = Session::with_registry(Project::new("isolation"), shared_registry());
    s.save_as(&song_dir).unwrap();
    write_sine(&song_dir.join("sine.wav"), secs);
    let t = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Sine" })));
    // 120 BPM: a bar is 2 s.
    run(
        &mut s,
        "clip.add.audio",
        json!({ "track": t, "at": 0, "bars": f64::from(secs) / 2.0, "file": "sine.wav" }),
    );
    let pid = id_of(&run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "kHs Gain", "format": "vst3", "uid": g.uid }),
    ));
    (s, song_dir, pid)
}

fn options(g: &Gain, song_dir: &Path, isolation: Isolation) -> Options {
    Options {
        range: RangeSpec::Song,
        bit_depth: Some(32),
        dir: Some(song_dir.to_path_buf()),
        catalog: Some(g.catalog.clone()),
        isolation: Some(isolation),
        ..Options::default()
    }
}

/// Exports a song through the host and in process: the bytes are the same, and with `speed`
/// the export through the host (its start included) is faster than real time. That holds on a
/// quiet machine only, so it runs with `--ignored`.
fn export_through_the_host(speed: bool) {
    let Some(g) = gain() else { return };
    let dir = Dir::new("match");
    let (mut s, song_dir, pid) = song(&dir, &g, 10.0);
    run(
        &mut s,
        "plugin.state.set",
        json!({ "id": pid, "state": g.quiet_state }),
    );
    let (inproc, remote) = (dir.path().join("in.wav"), dir.path().join("remote.wav"));
    export::mixdown(
        s.project(),
        &inproc,
        &options(&g, &song_dir, Isolation::InProcess),
    )
    .unwrap();
    let begun = Instant::now();
    let r = export::mixdown(
        s.project(),
        &remote,
        &options(&g, &song_dir, Isolation::Separate(g.host.clone())),
    )
    .unwrap();
    let took = begun.elapsed();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(
        std::fs::read(&inproc).unwrap(),
        std::fs::read(&remote).unwrap(),
        "the host process renders the same bytes as the plugin in process"
    );
    let secs = r.frames as f64 / f64::from(RATE);
    let level = db(f64::from(r.rms) / unity());
    eprintln!(
        "export through the host: {secs:.1} s of song in {} ms ({:.0}x real time, host start included), {level:.2} dB, bytes equal to in process",
        took.as_millis(),
        secs / took.as_secs_f64()
    );
    assert!(
        (level - g.quiet_db).abs() < 0.1,
        "{level} vs {}",
        g.quiet_db
    );
    if speed {
        assert!(took.as_secs_f64() < secs, "{took:?} for {secs} s");
    }
}

#[test]
fn the_export_through_the_host_matches_the_plugin_in_process() {
    export_through_the_host(false);
}

#[test]
#[ignore = "real time: a busy machine cannot keep up; run on a quiet one with --ignored"]
fn the_export_through_the_host_is_faster_than_real_time() {
    export_through_the_host(true);
}

#[test]
fn a_host_that_goes_down_during_an_export_leaves_the_dry_signal() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("crash");
    let secs = 60.0;
    let (mut s, song_dir, pid) = song(&dir, &g, secs);
    run(
        &mut s,
        "plugin.state.set",
        json!({ "id": pid, "state": g.quiet_state }),
    );
    let p = s.project().clone();
    let mut builder = Builder::new(RATE, Some(song_dir));
    builder.set_catalog(Some(g.catalog.clone()));
    builder.set_isolation(Isolation::Separate(g.host.clone()));
    // A short render first opens the server, so this thread can kill its host.
    export::render(&mut builder, &p, Select::All, 0, 4800, 0).unwrap();
    let server = builder.plugins().server().unwrap().clone();
    let progress = Arc::new(Progress::default());
    let total = (secs * RATE as f32) as u64;
    let render = {
        let progress = progress.clone();
        thread::spawn(move || {
            let r =
                export::render_with(&mut builder, &p, Select::All, 0, total, 0, Some(&progress));
            (r, builder)
        })
    };
    let begun = Instant::now();
    while progress.done() < u64::from(RATE) * 10 && begun.elapsed() < GUARD {
        thread::sleep(Duration::from_micros(200));
    }
    server.kill();
    let (r, builder) = render.join().unwrap();
    let out = r.expect("the export finishes");
    assert_eq!(out.left.len() as u64, total);
    let window = RATE as usize / 4;
    let levels: Vec<f64> = out
        .left
        .chunks(window)
        .map(|c| db(rms(c) / unity()))
        .collect();
    let quiet = levels
        .iter()
        .filter(|l| (*l - g.quiet_db).abs() < 0.1)
        .count();
    let dry = levels.iter().filter(|l| l.abs() < 0.1).count();
    eprintln!(
        "export with the host killed after 10 s of song: {quiet} windows at {:.2} dB, {dry} dry at 0 dB, of {}; first {:.2} dB, last {:.2} dB",
        g.quiet_db,
        levels.len(),
        levels[0],
        levels[levels.len() - 1]
    );
    assert!(quiet >= 4, "{levels:?}");
    assert!(dry >= 4, "{levels:?}");
    // Every window is the plugin or the dry sine, except the one it went down in: never silence.
    assert!(quiet + dry + 1 >= levels.len(), "{levels:?}");
    assert!(levels.last().unwrap().abs() < 0.1);
    let st = builder.plugins().statuses();
    assert!(st.iter().all(|(_, _, s)| s.crashed), "{st:?}");
}

/// Pulls the engine at the pace of a device, into memory. Like a device, it does not make up
/// for time it lost (a busy machine let the thread wait): a burst of blocks would leave the
/// host processes no time to answer and look to the engine like a stalled host.
struct Pull {
    stop: Arc<AtomicBool>,
    left: Arc<Mutex<Vec<f32>>>,
    thread: Option<JoinHandle<()>>,
}

impl Pull {
    fn start(mut audio: AudioEngine) -> Pull {
        let stop = Arc::new(AtomicBool::new(false));
        let left = Arc::new(Mutex::new(Vec::new()));
        let thread = {
            let (stop, left) = (stop.clone(), left.clone());
            thread::spawn(move || {
                const N: usize = 480;
                let period = Duration::from_secs_f64(N as f64 / f64::from(RATE));
                let mut buf = vec![0.0f32; N * 2];
                let mut due = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    audio.render(&mut buf, 2);
                    left.lock().unwrap().extend(buf.iter().step_by(2).copied());
                    due += period;
                    let now = Instant::now();
                    if due > now {
                        thread::sleep(due - now);
                    } else {
                        due = now;
                    }
                }
            })
        };
        Pull {
            stop,
            left,
            thread: Some(thread),
        }
    }

    fn frames(&self) -> usize {
        self.left.lock().unwrap().len()
    }

    /// The first frame that is not silent.
    fn onset(&self) -> usize {
        let since = Instant::now();
        loop {
            if let Some(i) = self
                .left
                .lock()
                .unwrap()
                .iter()
                .position(|x| x.abs() > 1e-6)
            {
                return i;
            }
            assert!(since.elapsed() < GUARD, "no sound came");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits for `secs` more of sound and gives its level against the dry sine, in dB.
    fn level(&self, secs: f64) -> f64 {
        let from = self.frames();
        let n = (secs * f64::from(RATE)) as usize;
        let since = Instant::now();
        while self.frames() < from + n {
            assert!(
                since.elapsed() < GUARD + Duration::from_secs_f64(secs),
                "the engine rendered {} of {n} frames",
                self.frames() - from
            );
            thread::sleep(Duration::from_millis(5));
        }
        let left = self.left.lock().unwrap();
        db(rms(&left[from..from + n]) / unity())
    }
}

impl Drop for Pull {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Only a guard against waiting forever (a host that never goes down, a state that never
/// comes); how soon it happens is not what is tested.
const GUARD: Duration = Duration::from_secs(60);

/// For every plugin in a host process: its wet and dry blocks, and whether its host is down.
fn blocks(host: &SessionHost) -> Vec<(u64, u64, bool)> {
    host.plugin_statuses()
        .into_iter()
        .map(|(_, _, s)| (s.wet_blocks, s.dry_blocks, s.crashed))
        .collect()
}

/// Every plugin played as its host says between `a` and `b`: through the host on every block
/// while it is up, dry on every block while it is down.
fn as_the_hosts_say(a: &[(u64, u64, bool)], b: &[(u64, u64, bool)]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(&(w0, d0, down), &(w1, d1, down1))| {
            down == down1 && if down { w1 == w0 } else { d1 == d0 && w1 > w0 }
        })
}

/// The level of `secs` of sound once it has settled, against the dry sine, in dB. Under load a
/// host can deliver a block late, and that block plays dry: the level is then that of a busy
/// machine, not of the plugin. So the stretch is taken again until every plugin played as its
/// host says over it and the 0.3 s before it (a plugin back from a restart keeps the dry signal
/// over its latency and fades over within that).
fn steady(host: &SessionHost, pull: &Pull, secs: f64) -> f64 {
    for _ in 0..10 {
        let a = blocks(host);
        pull.level(0.3);
        let level = pull.level(secs);
        let b = blocks(host);
        if as_the_hosts_say(&a, &b) {
            return level;
        }
        eprintln!("a block played otherwise ({a:?} -> {b:?}), measuring again");
    }
    panic!("no {secs} s played as the hosts say");
}

fn start(s: &Session, song_dir: &Path, g: &Gain) -> (SessionHost, Pull) {
    start_with(s, song_dir, g, Isolation::Separate(g.host.clone()))
}

fn start_with(s: &Session, song_dir: &Path, g: &Gain, isolation: Isolation) -> (SessionHost, Pull) {
    let config = HostConfig {
        rate: Some(RATE),
        dir: Some(song_dir.to_path_buf()),
        catalog: Some(g.catalog.clone()),
        isolation: Some(isolation),
        ..HostConfig::default()
    };
    let (mut host, audio, warnings) = SessionHost::start(s.project(), config).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(host.play());
    (host, Pull::start(audio))
}

/// Waits until `f` holds, polling the host.
fn wait_for(host: &mut SessionHost, what: &str, f: impl Fn(&mut SessionHost) -> bool) {
    let end = Instant::now() + GUARD;
    while !f(host) {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        host.poll();
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn while_playing_a_killed_host_keeps_the_sound_and_a_restart_brings_the_state_back() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("kill");
    let (mut s, song_dir, pid) = song(&dir, &g, 30.0);
    run(
        &mut s,
        "plugin.state.set",
        json!({ "id": pid, "state": g.quiet_state }),
    );
    let (mut host, pull) = start(&s, &song_dir, &g);
    let before = steady(&host, &pull, 0.5);

    let server = host.plugins().server().unwrap().clone();
    server.kill();
    assert!(server.wait_down(GUARD));
    let killed = steady(&host, &pull, 0.5);
    let st = host.plugin_statuses();
    assert!(st[0].2.crashed, "{st:?}");
    let exit = st[0].2.exit_code;

    let begun = Instant::now();
    host.restart_plugins().unwrap();
    wait_for(&mut host, "the restarted plugin", |h| {
        let st = h.plugin_statuses();
        !st[0].2.crashed && h.plugins().link(st[0].0).unwrap().opened().is_some()
    });
    let restart_ms = begun.elapsed().as_millis();
    let after = steady(&host, &pull, 0.5);
    let st = host.plugin_statuses();
    eprintln!(
        "playing: {before:.2} dB; host killed (exit {exit:?}): {killed:.2} dB; restarted in {restart_ms} ms: {after:.2} dB; latency {} frames, wet {} dry {} late {} crashes {}",
        st[0].2.latency, st[0].2.wet_blocks, st[0].2.dry_blocks, st[0].2.misses, st[0].2.crashes
    );
    assert!((before - g.quiet_db).abs() < 0.1, "{before}");
    assert!(killed.abs() < 0.1, "the dry sine plays on: {killed}");
    assert!(
        (after - g.quiet_db).abs() < 0.1,
        "the state came back: {after}"
    );
    assert_eq!(st[0].2.crashes, 1);
    drop(pull);
}

#[test]
fn a_parameter_set_in_the_song_reaches_the_running_plugin_without_a_new_mix() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("param");
    let (mut s, song_dir, pid) = song(&dir, &g, 30.0);
    let (mut host, pull) = start(&s, &song_dir, &g);
    let unity_level = steady(&host, &pull, 0.3);
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": pid, "param": "gain", "value": 0.3 }),
    );
    let report = host.sync(s.project());
    assert!(!report.swapped, "{report:?}");
    assert_eq!(report.commands, 1, "{report:?}");
    let turned = steady(&host, &pull, 0.3);
    eprintln!(
        "a parameter set by the song while playing: {unity_level:.2} dB -> {turned:.2} dB, no new mix"
    );
    assert!(unity_level.abs() < 0.1, "{unity_level}");
    assert!((turned - g.quiet_db).abs() < 0.1, "{turned}");
}

#[test]
fn an_edit_in_the_plugin_comes_into_the_song_as_one_undo_step_and_feeds_automation() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("collect");
    let (mut s, song_dir, _) = song(&dir, &g, 30.0);
    let (mut host, pull) = start(&s, &song_dir, &g);
    let id = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .next()
        .unwrap()
        .id;
    wait_for(&mut host, "the plugin to open", |h| {
        h.plugins().link(id).unwrap().opened().is_some()
    });
    let unity_level = steady(&host, &pull, 0.3);
    let titles = host.plugins().param_titles(id);
    let gain_id = titles.iter().find(|(_, t)| t == "Gain").unwrap().0;
    let link = host.plugins().link(id).unwrap();
    let steps_before = s.cursor();

    // The user drags the gain down in the editor.
    host.plugins()
        .server()
        .unwrap()
        .simulate_edit(&link, gain_id, &[0.45, 0.4, 0.35, 0.3]);
    let begun = Instant::now();
    let mut params = Vec::new();
    let collected = loop {
        host.poll();
        let c = collect_plugin_states(&mut s, &mut host, &RunOptions::human()).unwrap();
        params.extend(c.params);
        if let Some(r) = c.result {
            break r;
        }
        assert!(begun.elapsed() < GUARD, "no state came");
        thread::sleep(Duration::from_millis(20));
    };
    let took = begun.elapsed().as_millis();
    let x = |s: &Session| {
        s.project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .next()
            .unwrap()
            .clone()
    };
    assert!(x(&s).state.is_some());
    assert!(collected.step.is_some());
    assert_eq!(s.cursor(), steps_before + 1, "one step");
    let edited = steady(&host, &pull, 0.3);

    let phases: Vec<EditPhase> = params.iter().map(|p| p.phase).collect();
    assert_eq!(
        phases,
        [
            EditPhase::Begin,
            EditPhase::Change,
            EditPhase::Change,
            EditPhase::Change,
            EditPhase::Change,
            EditPhase::End
        ]
    );
    assert!(
        params
            .iter()
            .all(|p| p.title == "Gain" && p.param == gain_id)
    );
    assert_eq!(params[4].value, Some(0.3));
    assert!(
        params.iter().all(|p| p.frame.is_some()),
        "placed on the song"
    );

    // One undo takes the whole edit back, and the plugin follows the song.
    run(&mut s, "history.undo", json!({}));
    assert_eq!(x(&s).state, None);
    host.sync(s.project());
    let undone = steady(&host, &pull, 0.3);
    run(&mut s, "history.redo", json!({}));
    let report = host.sync(s.project());
    let redone = steady(&host, &pull, 0.3);
    eprintln!(
        "editor edit: {unity_level:.2} dB -> {edited:.2} dB, collected after {took} ms as one step ({} param events at frames {:?}..{:?}); undo {undone:.2} dB; redo {redone:.2} dB (new mix: {})",
        params.len(),
        params[0].frame,
        params[params.len() - 1].frame,
        report.swapped
    );
    assert!(unity_level.abs() < 0.1, "{unity_level}");
    assert!((edited - g.quiet_db).abs() < 0.1, "{edited}");
    assert!(undone.abs() < 0.1, "{undone}");
    assert!((redone - g.quiet_db).abs() < 0.1, "{redone}");
    assert!(!report.swapped, "the redone state is set in place");

    // Nothing left: the state the song now has is not reported back as an edit.
    thread::sleep(Duration::from_millis(400));
    let again = collect_plugin_states(&mut s, &mut host, &RunOptions::human()).unwrap();
    assert!(again.result.is_none());
}

fn mode_of(s: &Session, id: &str) -> PluginIsolation {
    s.project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id.to_string() == id)
        .unwrap()
        .isolation
}

fn set_mode(s: &mut Session, id: &str, mode: &str) {
    run(
        s,
        "plugin.isolation.set",
        json!({ "ids": [id], "mode": mode }),
    );
}

/// Each plugin where the song says, with the host of this build.
fn per_plugin(g: &Gain) -> Isolation {
    Isolation::PerPlugin(Some(g.host.clone()))
}

/// Waits until every plugin in a host process has opened.
fn wait_open(host: &mut SessionHost) {
    wait_for(host, "the plugins to open", |h| {
        h.plugin_statuses()
            .iter()
            .all(|(id, ..)| h.plugins().link(*id).unwrap().opened().is_some())
    });
}

#[test]
fn every_process_mode_plays_and_only_another_process_adds_its_delay() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("modes");
    let (mut s, song_dir, pid) = song(&dir, &g, 30.0);
    run(
        &mut s,
        "plugin.state.set",
        json!({ "id": pid, "state": g.quiet_state }),
    );
    let id = pid.parse().unwrap();
    let mut pids = Vec::new();
    for mode in ["inprocess", "shared", "dedicated"] {
        set_mode(&mut s, &pid, mode);
        let (mut host, pull) = start_with(&s, &song_dir, &g, per_plugin(&g));
        wait_open(&mut host);
        let delay = pull.onset() - 1;
        let level = steady(&host, &pull, 0.5);
        let placement = host.plugins().placement_of(id);
        let server_pid = host.plugins().server_of(id).and_then(|sv| sv.pid());
        let shared_pid = host.plugins().server().and_then(|sv| sv.pid());
        eprintln!(
            "{mode}: {level:.2} dB, output delayed {delay} frames, host pid {server_pid:?} (shared {shared_pid:?})"
        );
        assert!((level - g.quiet_db).abs() < 0.1, "{mode}: {level}");
        match mode {
            "inprocess" => {
                assert_eq!(delay, 0, "no delay in process");
                assert_eq!(placement, None);
                assert!(host.plugin_statuses().is_empty());
            }
            "shared" => {
                assert_eq!(delay, DEFAULT_LATENCY as usize);
                assert_eq!(placement, Some(PluginIsolation::Shared));
                assert_eq!(server_pid, shared_pid);
            }
            _ => {
                assert_eq!(delay, DEFAULT_LATENCY as usize);
                assert_eq!(placement, Some(PluginIsolation::Dedicated));
                assert!(shared_pid.is_none(), "nothing on the shared host");
            }
        }
        pids.push(server_pid);
        drop(pull);
    }
    assert!(pids[1].is_some() && pids[2].is_some() && pids[1] != pids[2]);

    // Without the host, a plugin that asks for another process runs in this one, and says so.
    let config = HostConfig {
        rate: Some(RATE),
        dir: Some(song_dir.clone()),
        catalog: Some(g.catalog.clone()),
        isolation: Some(Isolation::PerPlugin(None)),
        ..HostConfig::default()
    };
    let (host, _audio, warnings) = SessionHost::start(s.project(), config).unwrap();
    assert!(host.plugin_statuses().is_empty());
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("xirius-plugin-host が見つからないため")),
        "{warnings:?}"
    );
}

#[test]
fn a_host_that_goes_down_leaves_the_plugins_on_the_other_hosts_playing() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("hosts");
    let (mut s, song_dir, a) = song(&dir, &g, 60.0);
    // A second track with the same sine and its own kHs Gain.
    let t = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Sine 2" })));
    run(
        &mut s,
        "clip.add.audio",
        json!({ "track": t, "at": 0, "bars": 30, "file": "sine.wav" }),
    );
    let b = id_of(&run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "kHs Gain", "format": "vst3", "uid": g.uid }),
    ));
    for id in [&a, &b] {
        run(
            &mut s,
            "plugin.state.set",
            json!({ "id": id, "state": g.quiet_state }),
        );
    }
    set_mode(&mut s, &a, "dedicated");
    set_mode(&mut s, &b, "shared");
    let (mut host, pull) = start_with(&s, &song_dir, &g, per_plugin(&g));
    wait_open(&mut host);
    let (ia, ib) = (a.parse().unwrap(), b.parse().unwrap());
    let dedicated = host.plugins().server_of(ia).unwrap().clone();
    let shared = host.plugins().server_of(ib).unwrap().clone();
    assert!(dedicated.pid().is_some() && dedicated.pid() != shared.pid());
    let status = |h: &SessionHost, id| {
        h.plugin_statuses()
            .into_iter()
            .find(|(x, ..)| *x == id)
            .unwrap()
            .2
    };
    let crashed = |h: &SessionHost| (status(h, ia).crashed, status(h, ib).crashed);

    // The two sines add up in phase: both quiet, or one dry and one quiet.
    let q = 10f64.powf(g.quiet_db / 20.0);
    let (both, one_dry) = (db(2.0 * q), db(1.0 + q));
    let playing = steady(&host, &pull, 0.5);

    // The dedicated host goes down: only its plugin plays dry.
    dedicated.kill();
    assert!(dedicated.wait_down(GUARD));
    let wet_before = status(&host, ib).wet_blocks;
    let dedicated_down = steady(&host, &pull, 0.5);
    assert!(
        status(&host, ib).wet_blocks > wet_before,
        "the shared host plays on"
    );
    assert_eq!(crashed(&host), (true, false));
    assert!(!shared.crashed());

    // A restart brings back the host that went down, and only that one.
    let shared_pid = shared.pid();
    host.restart_plugins().unwrap();
    wait_for(&mut host, "the dedicated host back", |h| {
        crashed(h) == (false, false)
    });
    assert_eq!(shared.pid(), shared_pid, "the shared host was left alone");
    let restarted = steady(&host, &pull, 0.5);

    // And the reverse: the shared host goes down, the dedicated one plays on.
    shared.kill();
    assert!(shared.wait_down(GUARD));
    let wet_before = status(&host, ia).wet_blocks;
    let shared_down = steady(&host, &pull, 0.5);
    assert!(
        status(&host, ia).wet_blocks > wet_before,
        "the dedicated host plays on"
    );
    assert_eq!(crashed(&host), (false, true));
    assert!(!dedicated.crashed());
    eprintln!(
        "two hosts: both playing {playing:.2} dB (expected {both:.2}); dedicated killed {dedicated_down:.2} dB, restarted {restarted:.2} dB, shared killed {shared_down:.2} dB (expected {one_dry:.2})"
    );
    assert!((playing - both).abs() < 0.1, "{playing}");
    assert!((dedicated_down - one_dry).abs() < 0.1, "{dedicated_down}");
    assert!((restarted - both).abs() < 0.1, "{restarted}");
    assert!((shared_down - one_dry).abs() < 0.1, "{shared_down}");
    // Killed from outside, neither plugin is to blame.
    assert!(
        host.plugins()
            .reports()
            .iter()
            .all(|r| r.suggest().is_none())
    );
    drop(pull);
}

#[test]
fn a_plugin_moved_to_another_process_keeps_its_state_and_the_move_is_one_undo_step() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("move");
    let (mut s, song_dir, pid) = song(&dir, &g, 60.0);
    set_mode(&mut s, &pid, "shared");
    let (mut host, pull) = start_with(&s, &song_dir, &g, per_plugin(&g));
    wait_open(&mut host);
    let id = pid.parse().unwrap();
    let unity_level = steady(&host, &pull, 0.3);

    // The user turns the gain down in the editor, and the song has not taken it yet.
    let titles = host.plugins().param_titles(id);
    let gain_id = titles.iter().find(|(_, t)| t == "Gain").unwrap().0;
    let link = host.plugins().link(id).unwrap();
    host.plugins()
        .server_of(id)
        .unwrap()
        .simulate_edit(&link, gain_id, &[0.45, 0.3]);
    wait_for(&mut host, "the edited state", |_| link.has_pending_state());
    let edited = steady(&host, &pull, 0.3);
    let shared_pid = host.plugins().server().unwrap().pid();

    // To a host of its own: one step, a new mix, and the plugin keeps the edit.
    let before = s.cursor();
    set_mode(&mut s, &pid, "dedicated");
    assert_eq!(s.cursor(), before + 1, "one step");
    let report = host.sync(s.project());
    assert!(report.swapped, "{report:?}");
    wait_open(&mut host);
    assert_eq!(
        host.plugins().placement_of(id),
        Some(PluginIsolation::Dedicated)
    );
    let dedicated_pid = host.plugins().server_of(id).unwrap().pid();
    assert!(dedicated_pid.is_some() && dedicated_pid != shared_pid);
    let dedicated = steady(&host, &pull, 0.3);

    // Into this process: it still has it.
    set_mode(&mut s, &pid, "inprocess");
    let report = host.sync(s.project());
    assert!(report.swapped && report.warnings.is_empty(), "{report:?}");
    assert_eq!(host.plugins().placement_of(id), None);
    let in_process = steady(&host, &pull, 0.3);

    // The edit made before the moves still comes into the song.
    let c = collect_plugin_states(&mut s, &mut host, &RunOptions::human()).unwrap();
    assert!(c.result.is_some(), "the carried edit is collected");
    let x = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .next()
        .unwrap()
        .clone();
    assert!(x.state.is_some());
    eprintln!(
        "move with an edit the song had not taken: shared {unity_level:.2} -> edited {edited:.2} dB; dedicated (pid {dedicated_pid:?}, shared {shared_pid:?}) {dedicated:.2} dB; in process {in_process:.2} dB"
    );
    assert!(unity_level.abs() < 0.1, "{unity_level}");
    for level in [edited, dedicated, in_process] {
        assert!((level - g.quiet_db).abs() < 0.1, "{level}");
    }
    drop(pull);
    drop(host);

    // One undo takes back the collection, the next the move into this process; the song
    // saved and opened again keeps the mode and its history.
    run(&mut s, "history.undo", json!({}));
    run(&mut s, "history.undo", json!({}));
    assert_eq!(mode_of(&s, &pid), PluginIsolation::Dedicated);
    s.save().unwrap();
    let text = std::fs::read_to_string(song_dir.join("project.json")).unwrap();
    assert!(text.contains("\"isolation\": \"dedicated\""), "saved");
    drop(s);
    let mut s = Session::open_with(&song_dir, shared_registry()).unwrap();
    assert_eq!(mode_of(&s, &pid), PluginIsolation::Dedicated);
    run(&mut s, "history.undo", json!({}));
    assert_eq!(mode_of(&s, &pid), PluginIsolation::Shared);
}

#[test]
fn a_plugin_that_takes_the_shared_host_down_is_suggested_a_host_of_its_own() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("suggest");
    let (mut s, song_dir, pid) = song(&dir, &g, 60.0);
    set_mode(&mut s, &pid, "shared");
    let (mut host, pull) = start_with(&s, &song_dir, &g, per_plugin(&g));
    wait_open(&mut host);
    let id = pid.parse().unwrap();
    let link = host.plugins().link(id).unwrap();
    let server = host.plugins().server().unwrap().clone();
    server.fault(&link, Fault::Crash, 5);
    assert!(server.wait_down(GUARD));
    let r = commands::run(
        &mut s,
        Some(&mut host),
        "plugin.status",
        &Map::new(),
        &RunOptions::human(),
    )
    .unwrap();
    let row = &r.data["plugins"][0];
    eprintln!("plugin.status after a crash in the plugin: {}", r.message);
    assert_eq!(row["mode"], "shared");
    assert_eq!(row["faults"], 1);
    assert_eq!(row["suggest"], "dedicated");
    assert!(r.message.contains("--mode dedicated"), "{}", r.message);
    // Only suggested: the song still says shared, and the plugin stays where it was.
    assert_eq!(mode_of(&s, &pid), PluginIsolation::Shared);
    assert_eq!(
        host.plugins().placement_of(id),
        Some(PluginIsolation::Shared)
    );
    drop(pull);
}

/// The level of the output between `from` and `to` seconds, against the dry sine, polling the
/// host (which sends the lanes) while it waits.
fn level_between(host: &mut SessionHost, pull: &Pull, from: f64, to: f64) -> f64 {
    let (a, b) = (
        (from * f64::from(RATE)) as usize,
        (to * f64::from(RATE)) as usize,
    );
    let since = Instant::now();
    while pull.frames() < b {
        assert!(
            since.elapsed() < GUARD + Duration::from_secs_f64(to),
            "the engine rendered {} of {b} frames",
            pull.frames()
        );
        host.poll();
        thread::sleep(Duration::from_millis(5));
    }
    let left = pull.left.lock().unwrap();
    db(rms(&left[a..b]) / unity())
}

#[test]
fn a_parameter_lane_moves_the_plugin_while_playing_in_and_out_of_process() {
    let Some(g) = gain() else { return };
    let dir = Dir::new("lane");
    let (mut s, song_dir, pid) = song(&dir, &g, 10.0);
    let track = s
        .project()
        .tracks
        .iter()
        .find(|(_, t)| !t.inserts.is_empty())
        .unwrap()
        .0
        .to_string();
    // 120 BPM at 960 a quarter: a second is 1920 ticks. Quiet for 4 s, then the gain's own.
    run(
        &mut s,
        "auto.write",
        json!({ "writes": [{
            "track": track,
            "target": format!("param:{pid}:Gain"),
            "from": 0,
            "to": 7681,
            "points": [
                { "tick": 0, "value": 0.3 },
                { "tick": 7680, "value": 0.3 },
                { "tick": 7681, "value": g.unity_value },
            ],
        }] }),
    );
    for mode in ["inprocess", "dedicated"] {
        set_mode(&mut s, &pid, mode);
        let (mut host, pull) = start_with(&s, &song_dir, &g, per_plugin(&g));
        let lane = level_between(&mut host, &pull, 1.0, 3.5);
        let after = level_between(&mut host, &pull, 5.0, 7.5);
        drop(pull);

        let out = dir.path().join(format!("{mode}.wav"));
        let r =
            export::mixdown(s.project(), &out, &options(&g, &song_dir, per_plugin(&g))).unwrap();
        let q = 10f64.powf(g.quiet_db / 10.0);
        let want = 10.0 * ((4.0 * q + 6.0) / 10.0).log10();
        let exported = db(f64::from(r.rms) / unity());
        eprintln!(
            "{mode}: a Gain lane while playing {lane:.2} dB then {after:.2} dB; export {exported:.2} dB (lane says {want:.2})"
        );
        assert!((lane - g.quiet_db).abs() < 0.2, "{mode}: {lane}");
        assert!(after.abs() < 0.2, "{mode}: {after}");
        assert!(
            (exported - want).abs() < 0.2,
            "{mode}: {exported} vs {want}"
        );
    }
}

#[test]
fn an_edit_in_the_plugin_while_playing_is_written_into_its_lane_in_one_step() {
    use xirius_core::auto::AutoTarget;
    use xirius_session::autowrite::Pass;
    let Some(g) = gain() else { return };
    let dir = Dir::new("write");
    let (mut s, song_dir, pid) = song(&dir, &g, 30.0);
    let track = s
        .project()
        .tracks
        .iter()
        .find(|(_, t)| !t.inserts.is_empty())
        .unwrap()
        .0
        .to_owned();
    run(
        &mut s,
        "auto.mode",
        json!({ "ids": [track.to_string()], "mode": "touch" }),
    );
    // The song holds the gain, so Touch knows what to go back to.
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": pid, "param": "Gain", "value": g.unity_value }),
    );
    let (mut host, pull) = start(&s, &song_dir, &g);
    let id = pid.parse().unwrap();
    wait_for(&mut host, "the plugin to open", |h| {
        h.plugins().link(id).unwrap().opened().is_some()
    });
    pull.level(0.5);
    let mut pass = Pass::begin(s.project(), 0);
    let titles = host.plugins().param_titles(id);
    let gain_id = titles.iter().find(|(_, t)| t == "Gain").unwrap().0;
    let link = host.plugins().link(id).unwrap();
    host.plugins()
        .server()
        .unwrap()
        .simulate_edit(&link, gain_id, &[0.45, 0.4, 0.35, 0.3]);
    let begun = Instant::now();
    let mut taken = 0;
    while taken < 6 {
        host.poll();
        let c = collect_plugin_states(&mut s, &mut host, &RunOptions::human()).unwrap();
        let rate = f64::from(host.rate());
        let map = host.tempo_map().clone();
        taken += pass.params(&c.params, |f| map.tick_of_frame(f, rate));
        assert!(begun.elapsed() < GUARD, "{taken} events");
        thread::sleep(Duration::from_millis(20));
    }
    let stop = host.tick_now().unwrap();
    drop(pull);
    let writes = pass.finish(stop);
    let before = s.cursor();
    run(&mut s, "auto.write", json!({ "writes": writes }));
    assert_eq!(s.cursor(), before + 1, "one step");
    let t = &s.project().tracks[&track];
    let lane = t
        .automation
        .iter()
        .find(|l| l.target == AutoTarget::Param(id, "Gain".into()))
        .expect("a Gain lane");
    let values: Vec<f64> = lane.points.iter().map(|p| p.value).collect();
    eprintln!(
        "an editor edit while playing in Touch: {values:?} at {:?}",
        lane.points.iter().map(|p| p.tick).collect::<Vec<_>>()
    );
    assert!(values.contains(&0.3), "{values:?}");
    assert!(
        (values.last().unwrap() - g.unity_value).abs() < 1e-9,
        "Touch goes back to the gain it had: {values:?}"
    );
    run(&mut s, "history.undo", json!({}));
    assert!(s.project().tracks[&track].automation.is_empty());
}
