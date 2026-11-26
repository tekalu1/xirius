//! Loads, creates and tears down plugins installed on this machine from many threads at once,
//! offline. Each test skips when its plugin is not installed.
//!
//! A watchdog ends the run when no thread gets a step further for [`stall`], printing where
//! each thread is and who holds the plugin turn, so a deadlock fails instead of waiting.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use xirius_vst3::{Module, Plugin, default_vst3_dir, turn_report};

const THREADS: usize = 8;
const BLOCK: usize = 256;

/// Rounds per thread. `XIRIUS_VST3_ROUNDS` raises it for a longer run by hand.
fn rounds(default: usize) -> usize {
    std::env::var("XIRIUS_VST3_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// How long the watchdog lets every thread stand still. A busy machine makes the steps
/// slower, not absent. `XIRIUS_VST3_STALL_SECS` changes it, to keep a stuck run alive for a
/// debugger.
fn stall() -> Duration {
    Duration::from_secs(
        std::env::var("XIRIUS_VST3_STALL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120),
    )
}

/// Where each thread of a test is, for the watchdog.
struct Steps {
    name: &'static str,
    at: Vec<Mutex<&'static str>>,
    taken: AtomicUsize,
}

impl Steps {
    fn new(name: &'static str) -> Steps {
        Steps {
            name,
            at: (0..THREADS).map(|_| Mutex::new("not started")).collect(),
            taken: AtomicUsize::new(0),
        }
    }

    fn at(&self, thread: usize, step: &'static str) {
        *self.at[thread].lock().unwrap() = step;
        self.taken.fetch_add(1, Ordering::SeqCst);
    }

    /// Runs on the test's own thread until `done`; ends the process when nothing moves.
    fn watch(&self, done: impl Fn() -> bool) {
        let limit = stall();
        let (mut seen, mut since) = (self.taken.load(Ordering::SeqCst), Instant::now());
        while !done() {
            std::thread::sleep(Duration::from_millis(100));
            let now = self.taken.load(Ordering::SeqCst);
            if now != seen {
                (seen, since) = (now, Instant::now());
            } else if since.elapsed() >= limit {
                eprintln!("{}: no thread moved for {limit:?}", self.name);
                for (i, at) in self.at.iter().enumerate() {
                    eprintln!("  thread {i}: {}", at.lock().unwrap());
                }
                let turn = turn_report().unwrap_or_else(|| "free".into());
                eprintln!("  plugin turn: {turn}");
                // A panic here would wait for the stuck threads before it is reported.
                std::process::exit(1);
            }
        }
    }
}

/// One round: load the module, create the first audio class, process a few blocks, drop all.
fn round(path: &Path, steps: &Steps, t: usize) {
    steps.at(t, "loading");
    let module = Arc::new(Module::load(path).expect("load"));
    steps.at(t, "listing the classes");
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .expect("an audio class");
    steps.at(t, "creating");
    let mut plugin = Plugin::new(module, &class).expect("create");
    steps.at(t, "preparing");
    let mut audio = plugin.audio().expect("audio");
    audio.prepare(48_000.0, BLOCK, true).expect("prepare");
    steps.at(t, "processing");
    let mut l = vec![0.25f32; BLOCK];
    let mut r = vec![0.25f32; BLOCK];
    for _ in 0..4 {
        audio.process(&mut l, &mut r, &[]);
    }
    assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    steps.at(t, "dropping");
    drop(audio);
    drop(plugin);
    steps.at(t, "between rounds");
}

/// `THREADS` threads start together and each runs `n` rounds of `relative`.
fn hammer(name: &'static str, relative: &str, n: usize) {
    let path = default_vst3_dir().join(relative);
    if !path.exists() {
        eprintln!("skipped: {} is not installed", path.display());
        return;
    }
    let start = Barrier::new(THREADS);
    let steps = Steps::new(name);
    std::thread::scope(|s| {
        let threads: Vec<_> = (0..THREADS)
            .map(|t| {
                let (start, steps, path) = (&start, &steps, &path);
                s.spawn(move || {
                    steps.at(t, "at the start");
                    start.wait();
                    for _ in 0..n {
                        round(path, steps, t);
                    }
                    steps.at(t, "done");
                })
            })
            .collect();
        steps.watch(|| threads.iter().all(|t| t.is_finished()));
    });
}

#[test]
fn khs_gain_loads_and_unloads_from_many_threads_at_once() {
    hammer("khs_gain", "Kilohearts/kHs Gain.vst3", rounds(20));
}

#[test]
fn supermassive_loads_and_unloads_from_many_threads_at_once() {
    hammer("supermassive", "ValhallaSupermassive.vst3", rounds(5));
}

/// Two plugins of the same vendor framework at once, and every thread keeps one instance
/// alive while the others come and go, so the module is shared rather than reloaded.
#[test]
fn instances_come_and_go_while_others_keep_the_module() {
    let gain = default_vst3_dir().join("Kilohearts/kHs Gain.vst3");
    let delay = default_vst3_dir().join("Kilohearts/kHs Delay.vst3");
    if !gain.exists() || !delay.exists() {
        eprintln!("skipped: kHs Gain or kHs Delay is not installed");
        return;
    }
    let n = rounds(10);
    let start = Barrier::new(THREADS);
    let steps = Steps::new("instances_come_and_go");
    std::thread::scope(|s| {
        let threads: Vec<_> = (0..THREADS)
            .map(|t| {
                let (start, gain, delay, steps) = (&start, &gain, &delay, &steps);
                s.spawn(move || {
                    steps.at(t, "loading the kept instance");
                    let keep = Arc::new(Module::load(gain).expect("load"));
                    let class = keep
                        .classes()
                        .into_iter()
                        .find(|c| c.is_audio_processor())
                        .unwrap();
                    let held = Plugin::new(keep, &class).expect("create");
                    steps.at(t, "at the start");
                    start.wait();
                    for i in 0..n {
                        round(if (t + i) % 2 == 0 { gain } else { delay }, steps, t);
                    }
                    steps.at(t, "dropping the kept instance");
                    drop(held);
                    steps.at(t, "done");
                })
            })
            .collect();
        steps.watch(|| threads.iter().all(|t| t.is_finished()));
    });
}
