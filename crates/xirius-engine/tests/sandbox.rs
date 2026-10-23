//! Plugins in the host process (OPEN-02, PLG-022/023/024/052). Offline they match the plugin
//! in-process exactly, delayed by one block that delay compensation lines up; in real time a
//! host that crashes, hangs or cannot load leaves the dry signal playing, and a restart brings
//! the plugin back with the state it had. Nothing goes to a device. Tests that need an
//! installed plugin skip when it is missing.
//!
//! Waits are on what happened (the host is down, the state came, the host served every block),
//! not on a time: a busy machine (a parallel build) makes everything slower. The tests whose
//! point is keeping up with real time are `#[ignore]`d, as a busy machine cannot keep up; run
//! them on a quiet machine with `cargo test -p xirius-engine --test sandbox -- --ignored`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_engine::builtin::Sine;
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::plugin::Vst3Processor;
use xirius_engine::server::{
    EditPhase, PluginLink, PluginServer, PluginStatus, RemotePlugin, RemoteSpec,
};
use xirius_engine::wire::Fault;
use xirius_engine::{
    AudioEngine, Command, MixSpec, NoteKind, Plan, ProcessSetup, Processor, RenderMode, TimedNote,
    Timeline, TrackSlot, TrackSpec,
};
use xirius_vst3::{Module, Plugin, default_vst3_dir};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 256;
/// `L` in real time.
const RT_LATENCY: usize = 1024;
/// kHs Gain's "Gain" parameter at 0.3 is -12 dB.
const GAIN_PARAM: (u32, f64) = (1, 0.3);
const SINE_RMS: f64 = 0.353_553;

fn setup(mode: RenderMode) -> ProcessSetup {
    ProcessSetup {
        sample_rate: RATE,
        max_block: BLOCK,
        mode,
    }
}

fn installed(name: &str) -> Option<PathBuf> {
    let p = default_vst3_dir().join(name);
    if p.exists() {
        Some(p)
    } else {
        eprintln!("skipped: {} is not installed", p.display());
        None
    }
}

fn server() -> PluginServer {
    PluginServer::new(Some(env!("CARGO_BIN_EXE_xirius-plugin-host").into()))
}

fn remote(
    server: &PluginServer,
    bundle: PathBuf,
    params: &[(u32, f64)],
) -> (RemotePlugin, Arc<PluginLink>) {
    let link = PluginLink::new();
    let plugin = RemotePlugin::new(
        server.clone(),
        link.clone(),
        RemoteSpec {
            name: "remote".into(),
            bundle,
            params: params.iter().map(|&(id, v)| (id.to_string(), v)).collect(),
            latency: RT_LATENCY as u32,
            ..RemoteSpec::default()
        },
    );
    (plugin, link)
}

fn local(bundle: &Path, params: &[(u32, f64)]) -> Vst3Processor {
    let module = Arc::new(Module::load(bundle).unwrap());
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .unwrap();
    let mut plugin = Plugin::new(module, &class).unwrap();
    for &(id, v) in params {
        plugin.set_param(id, v);
    }
    Vst3Processor::new(plugin.audio().unwrap())
}

#[test]
fn automation_sent_before_a_block_matches_the_same_process() {
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let server = server();
    let (mut remote, link) = remote(&server, bundle.clone(), &[GAIN_PARAM]);
    remote.prepare(&setup(RenderMode::Offline));
    assert_eq!(remote.error(), None);
    let module = Arc::new(Module::load(&bundle).unwrap());
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .unwrap();
    let mut plugin = Plugin::new(module, &class).unwrap();
    plugin.set_param(GAIN_PARAM.0, GAIN_PARAM.1);
    let sender = plugin.param_sender();
    let mut local = plugin.audio().unwrap();
    local.prepare(RATE, BLOCK, true).unwrap();
    assert!(!server.automate(&link, "absent", 0.5, 0));
    let mut expected = Vec::new();
    let mut actual = Vec::new();
    for block in 0..64 {
        let value = if block % 4 < 2 { 0.3 } else { 0.5 };
        assert!(sender.set("Gain", value));
        assert!(server.automate(&link, "Gain", value, 0));
        let mut l = [0.25; BLOCK];
        let mut r = l;
        local.process(&mut l, &mut r, &[]);
        expected.extend_from_slice(&l);
        let mut l = [0.25; BLOCK];
        let mut r = l;
        remote.process(
            &mut xirius_engine::AudioIo {
                left: &mut l,
                right: &mut r,
                key: None,
            },
            &[],
        );
        actual.extend_from_slice(&l);
    }
    let delay = remote.latency() as usize;
    let error = actual[delay..]
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(error < 1e-6, "block automation differs by {error}");
    assert_eq!(server.status(&link).dry_blocks, 0);
}

#[test]
fn automation_keeps_its_offsets_inside_the_input_block() {
    use xirius_vst3::ParamChange;
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let server = server();
    let (mut remote, link) = remote(&server, bundle.clone(), &[GAIN_PARAM]);
    remote.prepare(&setup(RenderMode::Offline));
    assert_eq!(remote.error(), None);
    let module = Arc::new(Module::load(&bundle).unwrap());
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .unwrap();
    let mut plugin = Plugin::new(module, &class).unwrap();
    plugin.set_param(GAIN_PARAM.0, GAIN_PARAM.1);
    let mut local = plugin.audio().unwrap();
    local.prepare(RATE, BLOCK, true).unwrap();
    let mut expected = Vec::new();
    let mut actual = Vec::new();
    for block in 0..32 {
        let params = [
            ParamChange {
                offset: 17,
                id: GAIN_PARAM.0,
                value: if block % 2 == 0 { 0.3 } else { 0.5 },
            },
            ParamChange {
                offset: 149,
                id: GAIN_PARAM.0,
                value: if block % 2 == 0 { 0.5 } else { 0.3 },
            },
        ];
        for p in &params {
            assert!(server.automate(&link, "Gain", p.value, p.offset));
        }
        let mut l = [0.25; BLOCK];
        let mut r = l;
        local.process_with_params(&mut l, &mut r, &[], &[], &params);
        expected.extend_from_slice(&l);
        let mut l = [0.25; BLOCK];
        let mut r = l;
        remote.process(
            &mut xirius_engine::AudioIo {
                left: &mut l,
                right: &mut r,
                key: None,
            },
            &[],
        );
        actual.extend_from_slice(&l);
    }
    let delay = remote.latency() as usize;
    let error = actual[delay..]
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(error < 1e-6, "sample offsets differ by {error}");
}

/// A centred 440 Hz sine at `amp` through `insert`.
fn sine_track(amp: f32, insert: Option<Box<dyn Processor>>) -> TrackSlot {
    let mut t = TrackSpec::new("sine", Box::new(Sine::new(440.0, amp)));
    t.inserts.extend(insert);
    TrackSlot::New(t)
}

fn engine_for(tracks: Vec<TrackSlot>, mode: RenderMode) -> (AudioEngine, u32) {
    let spec = MixSpec {
        tracks,
        ..MixSpec::default()
    };
    let plan = Plan::build(spec, setup(mode), None).unwrap();
    let latency = plan.latency();
    let (audio, control) = engine::create(plan, EngineConfig::default());
    std::mem::forget(control);
    (audio, latency)
}

/// Renders `frames` more frames (left channel) into `left` in device buffers of 480 frames.
/// `paced` renders them no faster than real time, as a device would ask for them; like a
/// device, it does not make up for time it lost (a busy machine let the thread wait), as a
/// burst of buffers would leave the host no time to answer and look like a stalled host.
/// `each` runs between buffers with the frames rendered so far.
fn render(
    audio: &mut AudioEngine,
    left: &mut Vec<f32>,
    frames: usize,
    paced: bool,
    mut each: impl FnMut(usize),
) {
    let mut out = vec![0.0f32; 480 * 2];
    let end = left.len() + frames;
    let period = Duration::from_secs_f64(480.0 / RATE);
    let mut due = Instant::now();
    while left.len() < end {
        if paced {
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            } else {
                due = now;
            }
            due += period;
        }
        audio.render(&mut out, 2);
        left.extend(out.iter().step_by(2));
        each(left.len());
    }
    left.truncate(end);
    assert!(left.iter().all(|x| x.is_finite()));
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// Only a guard against waiting forever (a host that never goes down, a state that never
/// comes); how soon it happens is not what is tested.
const GUARD: Duration = Duration::from_secs(60);

/// Renders 9600 frames at a time (at the pace of a device if `paced`) until two stretches in a
/// row played as the host says (through it on every block while `wet`, dry on every block
/// otherwise), and gives the level of the last one. Under load a block can come back late and
/// play dry: such a stretch is the level of a busy machine, not of the plugin. The stretch
/// before covers a plugin back from a restart keeping the dry signal over its latency and
/// fading over.
fn settled(
    audio: &mut AudioEngine,
    out: &mut Vec<f32>,
    server: &PluginServer,
    link: &PluginLink,
    wet: bool,
    paced: bool,
) -> f64 {
    let mut before = false;
    let end = Instant::now() + GUARD;
    while Instant::now() < end {
        let a = server.status(link);
        render(audio, out, 9600, paced, |_| {});
        let b = server.status(link);
        let clean = if wet {
            b.dry_blocks == a.dry_blocks && b.wet_blocks > a.wet_blocks
        } else {
            b.wet_blocks == a.wet_blocks
        };
        if clean && before {
            return rms(&out[out.len() - 9600..]);
        }
        before = clean;
    }
    panic!("the plugin never played as its host says for 0.4 s (wet: {wet})");
}

/// Renders offline until the host is down, then `after` more frames. A fault is a message the
/// host's main thread reads when it gets to run; on a busy machine the plugin's audio thread
/// (in MMCSS) can play every block of a fixed length before that, so the fault never comes.
fn render_until_down(
    audio: &mut AudioEngine,
    out: &mut Vec<f32>,
    server: &PluginServer,
    after: usize,
) {
    let end = Instant::now() + GUARD;
    while !server.wait_down(Duration::ZERO) {
        assert!(Instant::now() < end, "the host never went down");
        render(audio, out, 4800, false, |_| {});
    }
    render(audio, out, after, false, |_| {});
}

/// Builds the mix again until the host served every block of a run (`run` gives the status
/// after it): offline a block the host does not answer within 2 s plays dry by design, and a
/// busy machine can open a plugin past its time limit. A run that went that way is a busy
/// machine, not the plugin; how often it took is printed.
fn served<T>(what: &str, mut run: impl FnMut() -> (T, PluginStatus)) -> (T, PluginStatus) {
    let end = Instant::now() + GUARD;
    for tries in 1.. {
        let (r, st) = run();
        let clean = st.error.is_none() && st.dry_blocks == 0 && st.misses == 0;
        if clean || Instant::now() >= end {
            if tries > 1 {
                eprintln!("{what}: the host served every block on try {tries}");
            }
            return (r, st);
        }
        eprintln!(
            "{what}: try {tries} not served (error {:?}, dry {}, misses {}), again",
            st.error, st.dry_blocks, st.misses
        );
    }
    unreachable!()
}

/// Every 480-frame stretch from `from` on has signal: the sound never stopped.
fn assert_continuous(x: &[f32], from: usize) {
    for (i, c) in x[from..].chunks(480).enumerate() {
        let peak = c.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.05, "silent stretch at frame {}", from + i * 480);
    }
}

#[test]
fn offline_the_remote_plugin_matches_the_plugin_in_process_faster_than_real_time() {
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let frames = 10 * RATE as usize;
    let ((wet, latency, opened, took), st) = served("offline kHs Gain", || {
        let server = server();
        let started = Instant::now();
        let (plugin, link) = remote(&server, bundle.clone(), &[GAIN_PARAM]);
        let (mut audio, latency) = engine_for(
            vec![sine_track(0.5, Some(Box::new(plugin)))],
            RenderMode::Offline,
        );
        let opened = started.elapsed();
        let mut wet = Vec::new();
        let t = Instant::now();
        render(&mut audio, &mut wet, frames, false, |_| {});
        let took = t.elapsed();
        ((wet, latency, opened, took), server.status(&link))
    });
    assert_eq!(latency as usize, BLOCK, "one block of delay offline");

    let (mut audio, _) = engine_for(
        vec![sine_track(
            0.5,
            Some(Box::new(local(&bundle, &[GAIN_PARAM]))),
        )],
        RenderMode::Offline,
    );
    let mut dry = Vec::new();
    render(&mut audio, &mut dry, frames, false, |_| {});

    assert!(wet[..BLOCK].iter().all(|&x| x == 0.0));
    let max_diff = wet[BLOCK..]
        .iter()
        .zip(&dry)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    let speed = frames as f64 / RATE / took.as_secs_f64();
    eprintln!(
        "offline kHs Gain: opened in {:.0} ms, 10 s rendered in {:.0} ms ({speed:.0}x real \
         time), max difference from in-process {max_diff:e}, wet blocks {}, dry {}",
        opened.as_secs_f64() * 1e3,
        took.as_secs_f64() * 1e3,
        st.wet_blocks,
        st.dry_blocks
    );
    assert_eq!(st.error, None);
    assert_eq!((st.dry_blocks, st.misses), (0, 0));
    assert_eq!(max_diff, 0.0);
    assert!(speed > 1.0, "{speed}x real time");
}

/// Delay compensation (ENG-010) holds the other tracks back by the remote plugin's `L`, to the
/// sample: the same sine with the opposite sign through the plugin in-process cancels it.
#[test]
fn delay_compensation_lines_the_remote_plugin_up_to_the_sample() {
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    for (mode, l) in [
        (RenderMode::Offline, BLOCK),
        (RenderMode::Realtime, RT_LATENCY),
    ] {
        let run = || {
            let server = server();
            let (plugin, link) = remote(&server, bundle.clone(), &[GAIN_PARAM]);
            let (mut audio, latency) = engine_for(
                vec![
                    sine_track(0.5, Some(Box::new(plugin))),
                    sine_track(-0.5, Some(Box::new(local(&bundle, &[GAIN_PARAM])))),
                ],
                mode,
            );
            let mut sum = Vec::new();
            render(
                &mut audio,
                &mut sum,
                RATE as usize,
                mode == RenderMode::Realtime,
                |_| {},
            );
            ((sum, latency), server.status(&link))
        };
        // In real time a block the host is late with plays dry by design; a run with one says
        // nothing about the alignment, so it is only checked when the machine kept up.
        let ((sum, latency), st) = if mode == RenderMode::Offline {
            served("offline delay compensation", run)
        } else {
            run()
        };
        assert_eq!(latency as usize, l);
        let max = sum.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        eprintln!(
            "{mode:?}: plan latency {latency}, |remote - in-process| max {max:e}, dry {}, misses {}",
            st.dry_blocks, st.misses
        );
        if mode == RenderMode::Offline || st.misses == 0 {
            assert_eq!((st.dry_blocks, st.misses), (0, 0), "{mode:?}");
            assert_eq!(max, 0.0, "{mode:?}");
        }
    }
}

#[test]
fn offline_a_crash_leaves_the_dry_signal_playing() {
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let server = server();
    let (plugin, link) = remote(&server, bundle, &[GAIN_PARAM]);
    let (mut audio, _) = engine_for(
        vec![sine_track(0.5, Some(Box::new(plugin)))],
        RenderMode::Offline,
    );
    server.fault(&link, Fault::Crash, 40);
    let mut out = Vec::new();
    render_until_down(&mut audio, &mut out, &server, RATE as usize);
    let frames = out.len();
    let st = server.status(&link);
    assert!(st.crashed);
    // STATUS_ACCESS_VIOLATION
    assert_eq!(st.exit_code, Some(0xC000_0005));
    assert_continuous(&out, BLOCK);
    // Wet (-12 dB) before the crash, dry (unity) after it.
    let early = rms(&out[BLOCK + 2048..BLOCK + 4096]);
    let tail = rms(&out[frames - RATE as usize / 2..]);
    eprintln!(
        "offline crash: {:.2} dB before, {:.2} dB after, wet blocks {}, dry {}",
        db(early / SINE_RMS),
        db(tail / SINE_RMS),
        st.wet_blocks,
        st.dry_blocks
    );
    assert!((db(early / SINE_RMS) + 12.0).abs() < 0.1);
    assert!((tail / SINE_RMS - 1.0).abs() < 1e-3);
}

/// An edit in the editor is taken as the plugin's state, a killed host leaves the sound going
/// dry, and a restart brings the edited state back. In real time it is played at the pace of a
/// device, which a busy machine cannot keep up with: that one runs with `--ignored`.
fn a_killed_host_keeps_the_sound_and_a_restart_brings_the_state_back(mode: RenderMode) {
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let paced = mode == RenderMode::Realtime;
    let server = server();
    let (plugin, link) = remote(&server, bundle, &[]);
    let (mut audio, latency) = engine_for(vec![sine_track(0.5, Some(Box::new(plugin)))], mode);
    let l = if paced { RT_LATENCY } else { BLOCK };
    assert_eq!(latency as usize, l);
    let mut out = Vec::new();
    render(&mut audio, &mut out, RATE as usize / 2, paced, |_| {});
    let before = rms(&out[out.len() - 9600..]);

    // The user turns the gain down in the editor; the host sends the state after it.
    server.simulate_edit(&link, GAIN_PARAM.0, &[0.5, 0.4, GAIN_PARAM.1]);
    let edited_at = out.len();
    let mut state_at = None;
    let end = Instant::now() + GUARD;
    while state_at.is_none() && Instant::now() < end {
        render(&mut audio, &mut out, 4800, paced, |n| {
            if state_at.is_none() && link.has_pending_state() {
                state_at = Some(n);
            }
        });
    }
    let edits = link.take_edits();
    let edited = settled(&mut audio, &mut out, &server, &link, true, paced);
    assert!(edits.state.is_some(), "the state after the edit");
    let phases: Vec<EditPhase> = edits.params.iter().map(|e| e.phase).collect();
    assert_eq!(
        phases,
        [
            EditPhase::Begin,
            EditPhase::Change,
            EditPhase::Change,
            EditPhase::Change,
            EditPhase::End
        ]
    );
    assert!(edits.params.iter().all(|e| e.id == GAIN_PARAM.0));
    assert_eq!(edits.params[3].value, Some(GAIN_PARAM.1));
    assert!(!edits.params[0].title.is_empty());

    // The host dies while playing.
    let killed_at = out.len();
    server.kill();
    let end = Instant::now() + GUARD;
    while !server.wait_down(Duration::ZERO) {
        assert!(Instant::now() < end, "the host never went down");
        render(&mut audio, &mut out, 4800, paced, |_| {});
    }
    let st = server.status(&link);
    assert!(st.crashed);
    let dry = settled(&mut audio, &mut out, &server, &link, false, paced);

    // Restart: the same state comes back.
    let restarted_at = out.len();
    let t = Instant::now();
    server.restart().unwrap();
    let restart_took = t.elapsed();
    render(&mut audio, &mut out, RATE as usize, paced, |_| {});
    let back = settled(&mut audio, &mut out, &server, &link, true, paced);
    let st = server.status(&link);
    eprintln!(
        "{mode:?}: {:.2} dB, after the edit {:.2} dB (state after {} frames), killed: {:.2} dB, \
         restarted in {:.0} ms: {:.2} dB; wet blocks {}, dry {}, late {}, crashes {}",
        db(before / SINE_RMS),
        db(edited / SINE_RMS),
        state_at.map_or(-1, |n| (n - edited_at) as i64),
        db(dry / SINE_RMS),
        restart_took.as_secs_f64() * 1e3,
        db(back / SINE_RMS),
        st.wet_blocks,
        st.dry_blocks,
        st.misses,
        st.crashes
    );
    assert_continuous(&out, l);
    assert!((db(edited / SINE_RMS) + 12.0).abs() < 0.1);
    assert!((dry / SINE_RMS - 1.0).abs() < 1e-3);
    assert!((db(back / SINE_RMS) + 12.0).abs() < 0.1);
    assert!(!st.crashed);
    assert_eq!(st.crashes, 1);
    assert!(killed_at < restarted_at);
}

#[test]
fn offline_a_killed_host_keeps_the_sound_and_a_restart_brings_the_state_back() {
    a_killed_host_keeps_the_sound_and_a_restart_brings_the_state_back(RenderMode::Offline);
}

#[test]
#[ignore = "real time: a busy machine cannot keep up; run on a quiet one with --ignored"]
fn in_real_time_a_killed_host_keeps_the_sound_and_a_restart_brings_the_state_back() {
    a_killed_host_keeps_the_sound_and_a_restart_brings_the_state_back(RenderMode::Realtime);
}

#[test]
fn a_hung_host_is_killed_and_bypassed() {
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let server = server();
    let (plugin, link) = remote(&server, bundle, &[GAIN_PARAM]);
    let (mut audio, _) = engine_for(
        vec![sine_track(0.5, Some(Box::new(plugin)))],
        RenderMode::Offline,
    );
    server.fault(&link, Fault::Hang, 20);
    let mut out = Vec::new();
    // Down means the watcher killed it.
    render_until_down(&mut audio, &mut out, &server, RATE as usize / 2);
    assert!(server.status(&link).crashed);
    assert_continuous(&out, BLOCK);
    assert!((rms(&out[out.len() - RATE as usize / 2..]) / SINE_RMS - 1.0).abs() < 1e-3);
}

/// Two plugins share a host; the one that crashes or hangs it is the one counted (PLG-022).
#[test]
fn a_host_that_goes_down_is_put_on_the_plugin_that_took_it_down() {
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let server = server();
    let (a, link_a) = remote(&server, bundle.clone(), &[GAIN_PARAM]);
    let (b, link_b) = remote(&server, bundle, &[GAIN_PARAM]);
    let (mut audio, _) = engine_for(
        vec![
            sine_track(0.5, Some(Box::new(a))),
            sine_track(0.5, Some(Box::new(b))),
        ],
        RenderMode::Offline,
    );
    let faults = |l: &PluginLink| server.status(l).faults;
    assert_eq!((faults(&link_a), faults(&link_b)), (0, 0));

    // A crash inside a's plugin.
    server.fault(&link_a, Fault::Crash, 20);
    let mut out = Vec::new();
    render_until_down(&mut audio, &mut out, &server, 0);
    assert_eq!((faults(&link_a), faults(&link_b)), (1, 0));

    // b hangs: the audio side finds it stalled and the watcher stops the host.
    server.restart().unwrap();
    render(&mut audio, &mut out, 4800, false, |_| {});
    server.fault(&link_b, Fault::Hang, 10);
    render_until_down(&mut audio, &mut out, &server, 0);
    let st = server.status(&link_b);
    assert_eq!((faults(&link_a), st.faults, st.crashes), (1, 1, 2));
}

#[test]
fn a_plugin_that_cannot_load_is_bypassed() {
    let server = server();
    let (plugin, link) = remote(&server, "C:/nowhere/missing.vst3".into(), &[]);
    let (mut audio, latency) = engine_for(
        vec![sine_track(0.5, Some(Box::new(plugin)))],
        RenderMode::Realtime,
    );
    assert_eq!(latency as usize, RT_LATENCY);
    let mut out = Vec::new();
    render(&mut audio, &mut out, 9600, false, |_| {});
    assert!(link.error().is_some());
    assert!(!server.status(&link).crashed);
    assert!(out[..RT_LATENCY].iter().all(|&x| x == 0.0));
    assert_continuous(&out, RT_LATENCY);
}

#[test]
fn a_state_set_by_the_song_is_not_reported_back() {
    let Some(bundle) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let server = server();
    let (plugin, link) = remote(&server, bundle, &[]);
    let (mut audio, _) = engine_for(
        vec![sine_track(0.5, Some(Box::new(plugin)))],
        RenderMode::Offline,
    );
    server.simulate_edit(&link, GAIN_PARAM.0, &[GAIN_PARAM.1]);
    let mut out = Vec::new();
    let end = Instant::now() + GUARD;
    while !link.has_pending_state() && Instant::now() < end {
        render(&mut audio, &mut out, 4800, false, |_| {});
    }
    let state = link.take_edits().state.expect("the edited state");
    server.set_state(&link, state);
    render(&mut audio, &mut out, 48_000, false, |_| {});
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(link.take_edits(), Default::default());
}

/// An instrument gets its notes on their frame: its first sound comes exactly `L` after the
/// same plugin's in-process one.
#[test]
fn a_remote_instrument_plays_its_notes_on_the_frame() {
    let Some(bundle) = installed("Vital.vst3") else {
        return;
    };
    let note_at = 1000u64;
    let track = |p: Box<dyn Processor>| {
        let mut t = TrackSpec::new("synth", p);
        t.timeline = Timeline::new(
            vec![
                TimedNote {
                    frame: note_at,
                    kind: NoteKind::On {
                        pitch: 60,
                        velocity: 0.8,
                    },
                },
                TimedNote {
                    frame: note_at + 24_000,
                    kind: NoteKind::Off { pitch: 60 },
                },
            ],
            Vec::new(),
        );
        TrackSlot::New(t)
    };
    let onset = |p: Box<dyn Processor>| {
        let spec = MixSpec {
            tracks: vec![track(p)],
            ..MixSpec::default()
        };
        let plan = Plan::build(spec, setup(RenderMode::Offline), None).unwrap();
        let latency = plan.latency();
        let (mut audio, mut control) = engine::create(plan, EngineConfig::default());
        control.send(Command::Play).ok().unwrap();
        let mut out = Vec::new();
        render(&mut audio, &mut out, RATE as usize, false, |_| {});
        let first = out.iter().position(|&x| x != 0.0);
        (first, rms(&out), latency)
    };
    let ((remote_first, remote_rms, remote_latency), st) = served("Vital", || {
        let server = server();
        let link = PluginLink::new();
        let plugin = RemotePlugin::new(
            server.clone(),
            link.clone(),
            RemoteSpec {
                name: "Vital".into(),
                bundle: bundle.clone(),
                instrument: true,
                ..RemoteSpec::default()
            },
        );
        let r = onset(Box::new(plugin));
        (r, server.status(&link))
    });
    let (local_first, local_rms, _) = onset(Box::new(local(&bundle, &[])));
    eprintln!(
        "Vital: first sound in-process at {local_first:?}, remote at {remote_first:?} (L \
         {remote_latency}), RMS {local_rms:.4} / {remote_rms:.4}"
    );
    assert_eq!(st.error, None);
    assert_eq!((st.dry_blocks, st.misses), (0, 0));
    assert_eq!(
        remote_first,
        local_first.map(|f| f + remote_latency as usize)
    );
    assert!(remote_rms > 1e-3);
}
