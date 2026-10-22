//! Headless engine demo (ARC-004): runs a small mix on the default output device and prints
//! what the device and the engine did.
//!
//! ```text
//! xirius-engine devices
//! xirius-engine play [OPTIONS]
//! xirius-engine vst3 --plugin BUNDLE [--plugin BUNDLE...] [--editor DIR] [--param ID=VALUE]
//!                    [--sandbox [--dedicated] [--sandbox-latency FRAMES] [--instrument]
//!                     [--crash-after-blocks N [--restart-after S]]] [OPTIONS]
//! OPTIONS: [--seconds S] [--buffer FRAMES] [--max-block FRAMES] [--exclusive] [--wav PATH]
//!          [--gain G] [--audible]
//! ```
//!
//! The device gets zeros unless `--audible` is given: everything is rendered, metered and
//! written to the WAV as usual, so the measurements hold, but nothing is heard.
//!
//! `vst3`: an instrument becomes a track that gets the notes; an effect becomes an insert on a
//! sine-synth track that gets them. `--editor DIR` opens each editor (without activating it)
//! while playing and saves a capture to `DIR`. `--param` sets a parameter of the first plugin
//! halfway through, through the same queue the editor uses. `--sandbox` runs the plugins in a
//! `xirius-plugin-host` process instead (effects, or instruments with `--instrument`; no editor
//! or `--param` then) and reports the host's CPU time; `--dedicated` gives each plugin a host
//! of its own instead of one shared by all. `--crash-after-blocks` makes the host crash, to
//! hear that the tracks go on dry, and `--restart-after` restarts it that many seconds into
//! the run. Every run reports this process's CPU time, which holds the plugins run in it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_engine::builtin::{Delay, Sine, SineSynth};
use xirius_engine::device::{self, CpalOutput};
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::exclusive::ExclusiveOutput;
use xirius_engine::mixer::{MeterReading, NodeId};
use xirius_engine::plugin::Vst3Processor;
use xirius_engine::server::{PluginLink, PluginServer, RemotePlugin, RemoteSpec};
use xirius_engine::stats::StatsSnapshot;
use xirius_engine::tap::WavTap;
use xirius_engine::wire::Fault;
use xirius_engine::{
    BusSpec, Command, EngineEvent, MixSpec, NoteKind, Plan, ProcessSetup, RenderMode, SendSpec,
    TrackSlot, TrackSpec,
};
use xirius_vst3::{Editor, Module, Plugin};

type BoxError = Box<dyn std::error::Error>;

enum Out {
    Shared(CpalOutput),
    Exclusive(ExclusiveOutput),
}

struct Args {
    command: String,
    seconds: f64,
    buffer: Option<u32>,
    max_block: usize,
    exclusive: bool,
    wav: Option<String>,
    gain: f32,
    audible: bool,
    plugins: Vec<PathBuf>,
    editor: Option<PathBuf>,
    param: Option<(u32, f64)>,
    sandbox: bool,
    dedicated: bool,
    sandbox_latency: u32,
    crash_after: Option<u64>,
    restart_after: Option<f64>,
    instrument: bool,
}

fn parse_args() -> Result<Args, BoxError> {
    let mut it = std::env::args().skip(1);
    let mut a = Args {
        command: it.next().unwrap_or_else(|| "play".into()),
        seconds: 3.0,
        buffer: None,
        max_block: 512,
        exclusive: false,
        wav: None,
        gain: 0.3,
        audible: false,
        plugins: Vec::new(),
        editor: None,
        param: None,
        sandbox: false,
        dedicated: false,
        sandbox_latency: 2048,
        crash_after: None,
        restart_after: None,
        instrument: false,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--seconds" => a.seconds = value()?.parse()?,
            "--buffer" => a.buffer = Some(value()?.parse()?),
            "--max-block" => a.max_block = value()?.parse()?,
            "--exclusive" => a.exclusive = true,
            "--wav" => a.wav = Some(value()?),
            "--gain" => a.gain = value()?.parse()?,
            "--audible" => a.audible = true,
            "--plugin" => a.plugins.push(value()?.into()),
            "--editor" => a.editor = Some(value()?.into()),
            "--param" => {
                let v = value()?;
                let (id, x) = v.split_once('=').ok_or("--param takes ID=VALUE")?;
                a.param = Some((id.parse()?, x.parse()?));
            }
            "--sandbox" => a.sandbox = true,
            "--dedicated" => a.dedicated = true,
            "--sandbox-latency" => a.sandbox_latency = value()?.parse()?,
            "--crash-after-blocks" => a.crash_after = Some(value()?.parse()?),
            "--restart-after" => a.restart_after = Some(value()?.parse()?),
            "--instrument" => a.instrument = true,
            _ => return Err(format!("unknown flag {flag}").into()),
        }
    }
    Ok(a)
}

fn demo_mix(gain: f32) -> MixSpec {
    let mut drone = TrackSpec::new("sine 220 Hz", Box::new(Sine::new(220.0, 0.2)));
    drone.pan = -0.4;
    drone.sends = vec![SendSpec {
        bus: 0,
        level: 0.3,
        pre_fader: false,
    }];
    let mut synth = TrackSpec::new("sine synth", Box::new(SineSynth::new()));
    synth.pan = 0.4;
    synth.sends = vec![SendSpec {
        bus: 0,
        level: 0.4,
        pre_fader: false,
    }];
    let mut echo = BusSpec::new("echo 150 ms");
    echo.inserts = vec![Box::new(Delay::new(7200))];
    echo.gain = 0.6;
    MixSpec {
        tracks: vec![TrackSlot::New(drone), TrackSlot::New(synth)],
        buses: vec![echo],
        master_inserts: Vec::new(),
        master_gain: gain,
    }
}

/// The plugins of `--sandbox`, each with the server that runs it, and the servers.
struct Sandbox {
    servers: Vec<PluginServer>,
    links: Vec<(String, Arc<PluginLink>, PluginServer)>,
}

impl Sandbox {
    /// The host processes' CPU time so far, summed.
    fn cpu(&self) -> Option<Duration> {
        self.servers
            .iter()
            .map(|s| s.pid().and_then(process_cpu))
            .sum()
    }
}

/// With `--sandbox`: each bundle in a host process (one for all, or one each with
/// `--dedicated`), as an instrument track or as an effect on a sine-synth track. The bundle is
/// not loaded in this process.
fn sandbox_mix(a: &Args) -> Result<(MixSpec, Sandbox), BoxError> {
    let host = std::env::current_exe()?.with_file_name("xirius-plugin-host.exe");
    let mut servers = vec![PluginServer::new(Some(host.clone()))];
    let mut tracks = Vec::new();
    let mut links = Vec::new();
    for (i, path) in a.plugins.iter().enumerate() {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if a.dedicated && i > 0 {
            servers.push(PluginServer::new(Some(host.clone())));
        }
        let server = servers.last().expect("one server at least").clone();
        let link = PluginLink::new();
        let plugin = RemotePlugin::new(
            server.clone(),
            link.clone(),
            RemoteSpec {
                name: format!("{name} (separate process)"),
                bundle: path.clone(),
                instrument: a.instrument,
                latency: a.sandbox_latency,
                ..RemoteSpec::default()
            },
        );
        let t = if a.instrument {
            TrackSpec::new(name.clone(), Box::new(plugin))
        } else {
            let mut t = TrackSpec::new(format!("sine synth -> {name}"), Box::new(SineSynth::new()));
            t.inserts.push(Box::new(plugin));
            t
        };
        println!(
            "plugin: {name} in the plugin host process{}",
            if a.dedicated { " of its own" } else { "" }
        );
        tracks.push(TrackSlot::New(t));
        links.push((name, link, server));
    }
    let mix = MixSpec {
        tracks,
        buses: Vec::new(),
        master_inserts: Vec::new(),
        master_gain: a.gain,
    };
    Ok((mix, Sandbox { servers, links }))
}

/// CPU time (user + kernel) of a process so far.
fn process_cpu(pid: u32) -> Option<Duration> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain queries on a handle we open and close here.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut c, mut e, mut k, mut u) = (zero, zero, zero, zero);
        let ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0;
        CloseHandle(h);
        let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        ok.then(|| Duration::from_nanos((t(k) + t(u)) * 100))
    }
}

/// Loads each bundle's first audio class. Instruments become tracks; effects become inserts on
/// a sine-synth track. Every track gets the demo's notes.
fn plugin_mix(a: &Args) -> Result<(MixSpec, Vec<Plugin>), BoxError> {
    let mut plugins = Vec::new();
    let mut tracks = Vec::new();
    for path in &a.plugins {
        let module = Arc::new(Module::load(path)?);
        let class = module
            .classes()
            .into_iter()
            .find(|c| c.is_audio_processor())
            .ok_or_else(|| format!("{}: no audio class", path.display()))?;
        let mut plugin = Plugin::new(module, &class)?;
        let processor = Box::new(Vst3Processor::new(plugin.audio()?));
        let track = if class.is_instrument() {
            TrackSpec::new(class.name.clone(), processor)
        } else {
            let mut t = TrackSpec::new(
                format!("sine synth -> {}", class.name),
                Box::new(SineSynth::new()),
            );
            t.inserts.push(processor);
            t
        };
        println!(
            "plugin: {} ({}) [{}] as {}",
            class.name,
            class.vendor,
            class.sub_categories,
            if class.is_instrument() {
                "an instrument track"
            } else {
                "an insert"
            }
        );
        tracks.push(TrackSlot::New(track));
        plugins.push(plugin);
    }
    let mix = MixSpec {
        tracks,
        buses: Vec::new(),
        master_inserts: Vec::new(),
        master_gain: a.gain,
    };
    Ok((mix, plugins))
}

fn open_editors(plugins: &[Plugin], dir: &Path) -> Vec<(String, Editor)> {
    let _ = std::fs::create_dir_all(dir);
    let mut out = Vec::new();
    for p in plugins {
        let name = p.class().name.clone();
        match p.open_editor(&name) {
            // It should not take the foreground; if it does, give the desktop back at once.
            Ok(e) if e.took_foreground() => {
                println!("editor {name}: took the foreground; closed at once");
            }
            Ok(e) => {
                println!("editor {name}: open, {:?} px", e.size());
                out.push((name, e));
            }
            Err(e) => println!("editor {name}: {e}"),
        }
    }
    out
}

#[derive(Default)]
struct MeterSummary {
    reports: u64,
    rms_sum: [f64; 2],
    peak: [f32; 2],
    silent_reports: u64,
}

impl MeterSummary {
    fn add(&mut self, m: MeterReading) {
        self.reports += 1;
        for ch in 0..2 {
            self.rms_sum[ch] += m.rms[ch] as f64;
            self.peak[ch] = self.peak[ch].max(m.peak[ch]);
        }
        if m.peak[0] == 0.0 && m.peak[1] == 0.0 {
            self.silent_reports += 1;
        }
    }
}

fn print_stats(s: &StatsSnapshot, sample_rate: u32) {
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "callbacks: {}  frames: {} ({:.2} s)",
        s.callbacks,
        s.frames,
        s.frames as f64 / sample_rate as f64
    );
    println!(
        "frames per callback: min {}  max {}  mean {:.1}",
        s.min_frames,
        s.max_frames,
        s.frames as f64 / s.callbacks.max(1) as f64
    );
    println!(
        "callback busy time (ms): mean {:.4}  p50 <= {:.3}  p99 <= {:.3}  p99.9 <= {:.3}  max {:.4}",
        ms(s.busy_mean),
        ms(s.busy_p50),
        ms(s.busy_p99),
        ms(s.busy_p999),
        ms(s.busy_max)
    );
    print!("busy histogram (10 µs buckets, lower edge µs:count):");
    for (us, c) in s.buckets() {
        print!(" {us}:{c}");
    }
    println!();
    println!(
        "deadline misses: {}  late callbacks: {}  starved (device ran dry): {}  device xruns: {}  device errors: {}",
        s.deadline_misses, s.late_callbacks, s.starved, s.device_xruns, s.device_errors
    );
    println!(
        "output latency reported by the device: {:.2} ms",
        ms(s.output_latency)
    );
    println!("WAV tap dropped samples: {}", s.tap_dropped);
}

fn play(
    a: &Args,
    mix: MixSpec,
    plugins: &[Plugin],
    sandbox: Option<&Sandbox>,
) -> Result<(), BoxError> {
    let info = device::default_output()?;
    println!(
        "device: {}  mix format: {} Hz, {} ch, {}",
        info.name, info.sample_rate, info.channels, info.sample_format
    );
    let setup = ProcessSetup {
        sample_rate: info.sample_rate as f64,
        max_block: a.max_block,
        mode: RenderMode::Realtime,
    };
    xirius_engine::crash::note(&format!(
        "sample_rate={} max_block={} mode={:?}",
        setup.sample_rate, setup.max_block, setup.mode
    ));
    let plan = Plan::build(mix, setup, None)?;
    if let Some(sb) = sandbox {
        for (name, link, server) in &sb.links {
            let st = server.status(link);
            match link.error() {
                Some(e) => println!("plugin {name}: could not open: {e}"),
                None => println!(
                    "plugin {name}: L + plugin latency = {} frames ({:.1} ms)",
                    st.latency,
                    st.latency as f64 * 1000.0 / setup.sample_rate
                ),
            }
            if let Some(n) = a.crash_after {
                server.fault(link, Fault::Crash, n);
            }
        }
    }
    let host_cpu_start = sandbox.and_then(Sandbox::cpu);
    let own_cpu_start = process_cpu(std::process::id());
    println!(
        "plan latency (compensated across paths): {} frames",
        plan.latency()
    );
    let (mut audio, mut control) = engine::create(plan, EngineConfig::default());
    audio.set_silent(!a.audible);
    println!(
        "device output: {}",
        if a.audible {
            "audible"
        } else {
            "zeros (rendered, metered and tapped as usual; pass --audible to hear it)"
        }
    );
    let tap = match &a.wav {
        Some(path) => {
            let (tap, producer) = WavTap::start(path, info.sample_rate, 2.0)?;
            audio.set_tap(producer);
            Some(tap)
        }
        None => None,
    };

    let stats = audio.stats();
    let mut output = if a.exclusive {
        let o = ExclusiveOutput::start(audio, a.buffer)?;
        let i = &o.info;
        println!(
            "mode: WASAPI exclusive, {}  period {} frames ({:.2} ms; asked {}, device min {}, default {})  max block: {}",
            i.format,
            i.period_frames,
            i.period_frames as f64 * 1000.0 / i.sample_rate as f64,
            i.requested_period_frames,
            i.min_period_frames,
            i.default_period_frames,
            a.max_block
        );
        Some(Out::Exclusive(o))
    } else {
        let o = CpalOutput::start(audio, a.buffer)?;
        let mode = match a.buffer {
            Some(n) => format!("shared, asked for {n} frames"),
            None => "shared, device default buffer".into(),
        };
        println!("mode: {mode}  max block: {}", a.max_block);
        Some(Out::Shared(o))
    };
    let mut editors = match &a.editor {
        Some(dir) => open_editors(plugins, dir),
        None => Vec::new(),
    };

    let notes = [60u8, 64, 67, 72, 67, 64];
    let tracks = control.shape().tracks;
    // The demo mix plays its notes on the sine synth; the plugin mix on every track.
    let note_tracks = if plugins.is_empty() && sandbox.is_none() {
        1..2
    } else {
        0..tracks
    };
    let started = Instant::now();
    let mut next_note = 0usize;
    let mut held: Option<u8> = None;
    let mut param_sent = false;
    let mut master = MeterSummary::default();
    let mut last_position = 0;
    let mut restarted = false;
    let mut host_cpu_end = None;
    while started.elapsed().as_secs_f64() < a.seconds {
        if let (Some(sb), Some(at), false) = (sandbox, a.restart_after, restarted)
            && started.elapsed().as_secs_f64() >= at
        {
            host_cpu_end = sb.cpu();
            let t = Instant::now();
            let r: Result<Vec<()>, String> = sb.servers.iter().map(PluginServer::restart).collect();
            println!(
                "restart at {:.2} s: {:?} in {:.1} ms",
                started.elapsed().as_secs_f64(),
                r,
                t.elapsed().as_secs_f64() * 1000.0
            );
            restarted = true;
        }
        let step = (started.elapsed().as_millis() / 250) as usize;
        if step >= next_note {
            let pitch = notes[next_note % notes.len()];
            for track in note_tracks.clone() {
                if let Some(p) = held {
                    let _ = control.send(Command::Note {
                        track,
                        kind: NoteKind::Off { pitch: p },
                    });
                }
                let _ = control.send(Command::Note {
                    track,
                    kind: NoteKind::On {
                        pitch,
                        velocity: 0.7,
                    },
                });
            }
            held = Some(pitch);
            next_note = step + 1;
        }
        if let (Some((id, v)), false, Some(p)) = (a.param, param_sent, plugins.first())
            && started.elapsed().as_secs_f64() >= a.seconds / 2.0
        {
            let queued = p.set_param(id, v);
            println!(
                "param {id} of {} set to {v} ({}) at {:.2} s, queued {queued}",
                p.class().name,
                p.param_text(id, v).unwrap_or_default(),
                started.elapsed().as_secs_f64()
            );
            param_sent = true;
        }
        control.poll(|e| match e {
            EngineEvent::Meter {
                node: NodeId::Master,
                reading,
            } => master.add(reading),
            EngineEvent::Position(p) => last_position = p,
            _ => {}
        });
        if editors.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
        } else {
            let slice = Duration::from_millis(10) / editors.len() as u32;
            for (_, e) in &mut editors {
                e.pump(slice);
            }
        }
    }
    if let Some(dir) = &a.editor {
        for (name, e) in &editors {
            let path = dir.join(format!("{name}.bmp"));
            match e.capture_bmp(&path) {
                Ok(c) => println!(
                    "editor {name}: captured {}x{}, {} colours, resize requests {} -> {}",
                    c.width,
                    c.height,
                    c.distinct_colors,
                    e.resize_requests(),
                    path.display()
                ),
                Err(err) => println!("editor {name}: capture failed: {err}"),
            }
        }
    }
    drop(editors);
    let played = started.elapsed();
    let own_cpu_end = process_cpu(std::process::id());
    if host_cpu_end.is_none() {
        host_cpu_end = sandbox.and_then(Sandbox::cpu);
    }
    match output.take() {
        Some(Out::Shared(o)) => drop(o.stop()),
        Some(Out::Exclusive(o)) => o.stop()?,
        None => {}
    }
    control.poll(|e| {
        if let EngineEvent::Meter {
            node: NodeId::Master,
            reading,
        } = e
        {
            master.add(reading)
        }
    });

    let snap = stats.snapshot();
    print_stats(&snap, info.sample_rate);
    let n = master.reports.max(1) as f64;
    println!(
        "master meter: {} reports, mean RMS L {:.4} R {:.4}, peak L {:.4} R {:.4}, silent reports {}",
        master.reports,
        master.rms_sum[0] / n,
        master.rms_sum[1] / n,
        master.peak[0],
        master.peak[1],
        master.silent_reports
    );
    println!("engine position: {last_position} frames");
    for p in plugins {
        let (seen, dropped) = p.handler().edit_counts();
        println!(
            "plugin {}: parameter edits queued {seen}, dropped {dropped}",
            p.class().name
        );
    }
    if let (Some(a0), Some(a1)) = (own_cpu_start, own_cpu_end) {
        let cpu = a1.saturating_sub(a0);
        println!(
            "this process CPU: {:.1} ms over {:.2} s = {:.2} % of one core",
            cpu.as_secs_f64() * 1000.0,
            played.as_secs_f64(),
            cpu.as_secs_f64() * 100.0 / played.as_secs_f64()
        );
    }
    if let Some(sb) = sandbox {
        for (name, link, server) in &sb.links {
            let st = server.status(link);
            println!(
                "plugin {name}: wet blocks {}, dry blocks {}, late blocks {}, crashed {} (exit code {}), host crashes {}",
                st.wet_blocks,
                st.dry_blocks,
                st.misses,
                st.crashed,
                st.exit_code
                    .map(|c| format!("{c:#010x}"))
                    .unwrap_or_else(|| "-".into()),
                st.crashes
            );
        }
        if let (Some(a0), Some(a1)) = (host_cpu_start, host_cpu_end) {
            let cpu = a1.saturating_sub(a0);
            println!(
                "plugin host CPU ({} process{}): {:.1} ms over {:.2} s = {:.2} % of one core{}",
                sb.servers.len(),
                if sb.servers.len() == 1 { "" } else { "es" },
                cpu.as_secs_f64() * 1000.0,
                played.as_secs_f64(),
                cpu.as_secs_f64() * 100.0 / played.as_secs_f64(),
                if restarted {
                    " (until the restart)"
                } else {
                    ""
                }
            );
        }
    }
    if let (Some(tap), Some(path)) = (tap, &a.wav) {
        let written = tap.finish()?;
        println!(
            "WAV: {path} ({written} samples = {:.2} s)",
            written as f64 / 2.0 / info.sample_rate as f64
        );
    }
    Ok(())
}

fn main() -> Result<(), BoxError> {
    if let Err(e) = xirius_engine::crash::install("engine", true) {
        eprintln!("クラッシュの記録先を準備できません: {e}");
    }
    let a = parse_args()?;
    match a.command.as_str() {
        "devices" => {
            let info = device::default_output()?;
            println!("{info:?}");
            Ok(())
        }
        "play" => play(&a, demo_mix(a.gain), &[], None),
        "vst3" => {
            if a.plugins.is_empty() {
                return Err("vst3 needs --plugin BUNDLE".into());
            }
            if a.sandbox {
                let (mix, sandbox) = sandbox_mix(&a)?;
                return play(&a, mix, &[], Some(&sandbox));
            }
            let (mix, plugins) = plugin_mix(&a)?;
            play(&a, mix, &plugins, None)
        }
        other => Err(format!("unknown command {other}").into()),
    }
}
