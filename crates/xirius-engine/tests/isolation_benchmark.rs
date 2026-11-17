//! Device-free, paced comparison of fixture instances in the engine and plugin hosts.
//! The short shared-host case runs in CI. The ten-minute matrix is opt-in.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use xirius_clap::{Bundle, Plugin};
use xirius_engine::builtin::Sine;
use xirius_engine::clap_plugin::ClapProcessor;
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::plugin::Vst3Processor;
use xirius_engine::rt::ProAudioThread;
use xirius_engine::server::{PluginLink, PluginServer, RemotePlugin, RemoteSpec};
use xirius_engine::wire::Fault;
use xirius_engine::{
    AudioIo, Command, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode, TrackSlot,
    TrackSpec,
};
use xirius_vst3::{Module, Plugin as Vst3Plugin};

mod crash_records;
use crash_records::CrashRecords;

#[derive(Clone, Copy, Debug)]
enum Placement {
    InProcess,
    Shared,
    Dedicated,
}

static COPY: AtomicU64 = AtomicU64::new(0);

struct FixtureFile {
    path: std::path::PathBuf,
}

impl FixtureFile {
    fn cleanup(&mut self) {
        std::fs::remove_file(&self.path).expect("fixture copy remained");
        self.path.clear();
    }
}

impl Drop for FixtureFile {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn fixture() -> FixtureFile {
    xirius_test_clap::fixture_marker();
    let bundle = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "isolation-{}-{}.clap",
        std::process::id(),
        COPY.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::copy(fixture_library(), &bundle).unwrap();
    FixtureFile { path: bundle }
}

fn fixture_library() -> std::path::PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!(
            "{}xirius_test_clap{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ))
}

fn local(bundle: &Path, clap: bool) -> Box<dyn Processor> {
    if clap {
        let bundle = Bundle::load(bundle).unwrap();
        Box::new(ClapProcessor::new(
            Plugin::new(&bundle, xirius_test_clap::ID)
                .unwrap()
                .audio()
                .unwrap(),
        ))
    } else {
        let module = std::sync::Arc::new(Module::load(bundle).unwrap());
        let class = module
            .classes()
            .into_iter()
            .find(|c| c.is_audio_processor())
            .unwrap();
        Box::new(Vst3Processor::new(
            Vst3Plugin::new(module, &class).unwrap().audio().unwrap(),
        ))
    }
}

struct Impulse(bool);

impl Processor for Impulse {
    fn name(&self) -> &str {
        "impulse"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {
        self.0 = false;
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        io.left.fill(0.0);
        io.right.fill(0.0);
        if !self.0 {
            io.left[0] = 1.0;
            io.right[0] = 1.0;
            self.0 = true;
        }
    }
}

#[test]
fn remote_impulse_has_no_added_latency() {
    let _records = CrashRecords::new();
    let mut file = fixture();
    let bundle = file.path.clone();
    let server = PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()));
    let link = PluginLink::new();
    let mut track = TrackSpec::new("impulse", Box::new(Impulse(false)));
    track.inserts.push(Box::new(RemotePlugin::new(
        server.clone(),
        link.clone(),
        RemoteSpec {
            name: "fixture".into(),
            bundle,
            class: Some(xirius_clap::class_key(xirius_test_clap::ID)),
            ..RemoteSpec::default()
        },
    )));
    let plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(track)],
            ..MixSpec::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 128,
            mode: RenderMode::Realtime,
        },
        None,
    )
    .unwrap();
    #[cfg(windows)]
    assert_eq!(plan.latency(), 0);
    let (mut audio, control) = engine::create(plan, EngineConfig::default());
    let mut out = [0.0f32; 256];
    audio.render(&mut out, 2);
    #[cfg(windows)]
    assert_eq!(out[0], 1.0, "the impulse moved after the first sample");
    assert_eq!(server.status(&link).misses, 0);
    drop(audio);
    drop(control);
    server.kill();
    assert!(server.wait_down(Duration::from_secs(30)));
    file.cleanup();
}

#[test]
fn prefetched_remote_track_runs_on_its_worker() {
    let _records = CrashRecords::new();
    let mut file = fixture();
    let server = PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()));
    let link = PluginLink::new();
    let mut track = TrackSpec::new("ahead", Box::new(Sine::new(440.0, 0.1)));
    track.prefetch = true;
    track.inserts.push(Box::new(RemotePlugin::new(
        server.clone(),
        link.clone(),
        RemoteSpec {
            name: "fixture".into(),
            bundle: file.path.clone(),
            class: Some(xirius_clap::class_key(xirius_test_clap::ID)),
            ..RemoteSpec::default()
        },
    )));
    let plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(track)],
            ..MixSpec::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 128,
            mode: RenderMode::Realtime,
        },
        None,
    )
    .unwrap();
    let (mut audio, mut control) = engine::create(plan, EngineConfig::default());
    control.send(Command::Play).ok().unwrap();
    let mut output = [0.0f32; 256];
    let deadline = Instant::now() + Duration::from_secs(5);
    while audio.prefetch(0).is_none_or(|s| s.rendered_frames < 128) {
        audio.render(&mut output, 2);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(audio.prefetch(0).unwrap().lent);
    assert!(server.status(&link).roundtrip.count > 0);
    drop(audio);
    drop(control);
    server.kill();
    assert!(server.wait_down(Duration::from_secs(30)));
    file.cleanup();
}

#[cfg(windows)]
fn fault_case(fault: Fault) -> (Duration, Duration, u64) {
    let _records = CrashRecords::new();
    let mut file = fixture();
    let server = PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()));
    let link = PluginLink::new();
    let mut remote = RemotePlugin::new(
        server.clone(),
        link.clone(),
        RemoteSpec {
            name: "fixture".into(),
            bundle: file.path.clone(),
            class: Some(xirius_clap::class_key(xirius_test_clap::ID)),
            ..RemoteSpec::default()
        },
    );
    remote.prepare(&ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 128,
        mode: RenderMode::Realtime,
    });
    assert_eq!(remote.error(), None);
    for _ in 0..2 {
        let mut left = [1.0f32; 128];
        let mut right = left;
        remote.process(&mut AudioIo::new(&mut left, &mut right), &[]);
        assert_eq!(left, [1.0; 128]);
    }
    server.fault(&link, fault, 3);
    let mut process = || {
        let mut left = [1.0f32; 128];
        let mut right = left;
        let start = Instant::now();
        remote.process(&mut AudioIo::new(&mut left, &mut right), &[]);
        assert_eq!(left, [1.0; 128]);
        start.elapsed()
    };
    let initial_dry = server.status(&link).dry_blocks;
    let mut failed = None;
    for _ in 0..100 {
        let elapsed = process();
        if server.status(&link).dry_blocks > initial_dry {
            failed = Some(elapsed);
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let failed = failed.expect("the host fault did not bypass any block");
    let following = process();
    let timeouts = server.status(&link).timeouts;
    drop(remote);
    server.kill();
    assert!(server.wait_down(Duration::from_secs(30)));
    file.cleanup();
    (failed, following, timeouts)
}

#[test]
#[cfg(windows)]
fn a_hung_host_times_out_once_then_audio_continues() {
    let (failed, following, timeouts) = fault_case(Fault::Hang);
    assert!(failed >= Duration::from_millis(200));
    assert!(failed < Duration::from_millis(500));
    assert!(following < Duration::from_millis(100));
    assert_eq!(timeouts, 1);
}

#[test]
#[cfg(windows)]
fn a_crashed_host_is_bypassed_without_waiting() {
    let (_failed, following, _) = fault_case(Fault::Crash);
    assert!(following < Duration::from_millis(100));
}

fn run(placement: Placement, block: usize, seconds: u32) {
    let _records = CrashRecords::new();
    let external = std::env::var_os("XIRIUS_BENCH_BUNDLE");
    let mut file = (external.is_none() && !matches!(placement, Placement::InProcess)).then(fixture);
    let bundle = external
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(|| file.as_ref().map(|f| f.path.clone()))
        .unwrap_or_else(fixture_library);
    let mut servers = Vec::new();
    let mut links = Vec::new();
    let shared = PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()));
    let mut tracks = Vec::new();
    for track in 0..10 {
        let mut t = TrackSpec::new(format!("track-{track}"), Box::new(Sine::new(440.0, 0.05)));
        for _ in 0..2 {
            match placement {
                Placement::InProcess => t.inserts.push(local(&bundle, external.is_none())),
                Placement::Shared | Placement::Dedicated => {
                    let server = if matches!(placement, Placement::Shared) {
                        shared.clone()
                    } else {
                        PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()))
                    };
                    let link = PluginLink::new();
                    t.inserts.push(Box::new(RemotePlugin::new(
                        server.clone(),
                        link.clone(),
                        RemoteSpec {
                            name: "fixture".into(),
                            bundle: bundle.clone(),
                            class: if external.is_none() {
                                Some(xirius_clap::class_key(xirius_test_clap::ID))
                            } else {
                                None
                            },
                            latency: 0,
                            ..RemoteSpec::default()
                        },
                    )));
                    servers.push(server);
                    links.push(link);
                }
            }
        }
        tracks.push(TrackSlot::New(t));
    }
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: block,
        mode: RenderMode::Realtime,
    };
    let plan = Plan::build(
        MixSpec {
            tracks,
            ..MixSpec::default()
        },
        setup,
        None,
    )
    .unwrap();
    let added_latency = plan.latency();
    let (mut audio, control) = engine::create(plan, EngineConfig::default());
    let mut output = vec![0.0f32; block * 2];
    let budget = Duration::from_secs_f64(block as f64 / 48_000.0);
    let calls = (48_000u64 * u64::from(seconds) / block as u64) as usize;
    let mut times = Vec::with_capacity(calls);
    let mut late = 0u64;
    let _rt = ProAudioThread::join();
    #[cfg(windows)]
    assert!(_rt.is_some(), "the callback thread needs audio scheduling");
    let start = Instant::now();
    for _ in 0..calls {
        let call = Instant::now();
        audio.render(&mut output, 2);
        let elapsed = call.elapsed();
        if elapsed > budget {
            late += 1;
        }
        times.push(elapsed.as_nanos() as u64);
        if elapsed < budget {
            std::thread::sleep(budget - elapsed);
        }
    }
    times.sort_unstable();
    let percentile = |p: usize| times[(times.len() * p / 100).min(times.len() - 1)] / 1_000;
    let max = times.last().copied().unwrap_or(0) / 1_000;
    let statuses: Vec<_> = servers
        .iter()
        .zip(&links)
        .map(|(s, l)| s.status(l))
        .collect();
    let timeout_count: u64 = statuses.iter().map(|s| s.timeouts).sum();
    let overruns: u64 = statuses.iter().map(|s| s.deadline_overruns).sum();
    let max_of = |f: fn(&xirius_engine::server::PluginStatus) -> u64| {
        statuses.iter().map(f).max().unwrap_or(0) / 1_000
    };
    let roundtrip = (
        max_of(|s| s.roundtrip.p50_ns),
        max_of(|s| s.roundtrip.p99_ns),
        max_of(|s| s.roundtrip.max_ns),
    );
    let host = (
        max_of(|s| s.host_time.p50_ns),
        max_of(|s| s.host_time.p99_ns),
        max_of(|s| s.host_time.max_ns),
    );
    let host_cpu = statuses
        .iter()
        .map(|s| s.host_time.total_ns as f64)
        .sum::<f64>()
        / start.elapsed().as_nanos() as f64
        * 100.0;
    let callback_cpu = times.iter().sum::<u64>() as f64 / start.elapsed().as_nanos() as f64 * 100.0;
    eprintln!(
        "isolation {placement:?} block {block} seconds {seconds}: callbacks {calls}, late {late}, latency {added_latency} samples, callback p50/p99/max {} / {} / {max} us, roundtrip p50/p99/max {:?} us, host p50/p99/max {:?} us, callback CPU {callback_cpu:.1}%, host DSP CPU {host_cpu:.1}%, overruns {overruns}, timeouts {timeout_count}, elapsed {:?}",
        percentile(50),
        percentile(99),
        roundtrip,
        host,
        start.elapsed()
    );
    assert!(output.iter().all(|x| x.is_finite()));
    assert_eq!(timeout_count, 0);
    #[cfg(windows)]
    if external.is_none() {
        assert_eq!(added_latency, 0);
    }
    drop(audio);
    drop(control);
    for server in &servers {
        server.kill();
    }
    for server in &servers {
        assert!(server.wait_down(Duration::from_secs(30)));
    }
    drop(servers);
    drop(shared);
    if let Some(file) = &mut file {
        file.cleanup();
    }
}

#[test]
fn short_shared_fixture_matrix() {
    run(Placement::Shared, 128, 1);
}

#[test]
#[ignore = "ten-minute paced performance comparison"]
fn ten_minute_matrix() {
    let only_block = std::env::var("XIRIUS_BENCH_BLOCK")
        .ok()
        .and_then(|s| s.parse::<usize>().ok());
    let only_placement = std::env::var("XIRIUS_BENCH_PLACEMENT").ok();
    let seconds = std::env::var("XIRIUS_BENCH_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(600);
    for placement in [
        Placement::InProcess,
        Placement::Shared,
        Placement::Dedicated,
    ] {
        if only_placement
            .as_deref()
            .is_some_and(|p| p != format!("{placement:?}"))
        {
            continue;
        }
        for block in [64, 128, 256] {
            if only_block.is_none_or(|b| b == block) {
                run(placement, block, seconds);
            }
        }
    }
}
