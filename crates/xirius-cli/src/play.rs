//! `xirius play`: plays the song and takes commands from standard input while it plays.
//!
//! Edits made on the way (`note.add`, `track.volume`, `undo`, ...) reach the sound through the
//! session layer's host, which sends only what changed. `--no-device` plays into a silent output
//! instead of the sound device, which is how the tests run.
//!
//! What the user changes in a plugin's editor is taken into the song before each command and
//! when the input ends (`plugin.collect`), so an `undo` sees it as one step and the song saved
//! keeps it.
//!
//! `record.start` / `record.stop` record the armed tracks from the device's input (with
//! `--no-device`, silence of the right length), and the instrument tracks from the MIDI input
//! `--midi-in` / `--midi-script` choose ([`crate::midi`]). `wait 5.1.0` waits until the transport gets
//! there, which is how a script records for a while; a recording still running when the input
//! ends is stopped and placed before the song is saved. On quitting, the recordings nothing uses
//! any more go to the recycle bin (`project.cleanup`).

use std::io::{self, BufRead};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Map, json};
use xirius_core::{CmdError, Registry, Session};
use xirius_engine::AudioEngine;
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::record::{Source, Sources};

use crate::app::{
    EXIT_COMMAND, EXIT_OK, close, no_song, open, options, print_result, report_error,
    run_and_persist, usage_error,
};
use crate::args::{self, Global, PlayOptions, Usage};
use crate::midi;

/// Callbacks after which the engine has taken every command sent before a `wait` began and
/// reported its transport since: the first may have begun before the last command was sent.
const SETTLED: u64 = 2;

/// How long `wait` waits for an engine that renders nothing at all (an output that stopped).
const STALLED: Duration = Duration::from_secs(10);

/// Frames per pull of the silent output.
const CHUNK: usize = 480;

/// Where the sound goes.
enum Output {
    Device(xirius_session::output::Output),
    Silent(SilentOutput),
}

/// Pulls the engine on a thread of its own and throws the sound away.
struct SilentOutput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SilentOutput {
    /// `paced` follows the clock like a device; otherwise it runs as fast as it can while the
    /// transport plays (and rests while it does not).
    fn start(mut audio: AudioEngine, paced: bool) -> SilentOutput {
        audio.set_silent(true);
        let period = Duration::from_secs_f64(CHUNK as f64 / audio.setup().sample_rate);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::spawn(move || {
            let mut buf = vec![0.0f32; CHUNK * 2];
            let mut due = Instant::now();
            while !flag.load(Ordering::Relaxed) {
                // timed, so the callbacks are counted for `wait`
                audio.render_timed(&mut buf, 2);
                if paced {
                    due += period;
                    let now = Instant::now();
                    if due > now {
                        thread::sleep(due - now);
                    } else if now - due > period * 20 {
                        due = now;
                    }
                } else if !audio.is_playing() {
                    thread::sleep(Duration::from_millis(1));
                }
            }
        });
        SilentOutput {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for SilentOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn play(g: &Global, registry: &Arc<Registry>, opts: PlayOptions) -> u8 {
    let mut session = match open(g, registry) {
        Ok(Some(s)) => s,
        Ok(None) => return report_error(g, &no_song()),
        Err(e) => return report_error(g, &e),
    };
    let from = match opts
        .from
        .as_deref()
        .map(|s| session.project().time_map().parse(s))
    {
        Some(Ok(t)) => Some(t),
        Some(Err(e)) => return report_error(g, &e),
        None => None,
    };
    let config = HostConfig {
        dir: session.dir().map(std::path::Path::to_path_buf),
        ..HostConfig::default()
    };
    let (mut host, audio, warnings) = match SessionHost::start(session.project(), config) {
        Ok(v) => v,
        Err(e) => {
            return report_error(
                g,
                &CmdError::conflict(format!("オーディオエンジンを起動できませんでした: {e:?}")),
            );
        }
    };
    for w in &warnings {
        eprintln!("注意: {w}");
    }
    host.io_settings = Some(xirius_prefs::Settings::open(xirius_prefs::config_dir()).shared());
    if let Err(e) = xirius_session::io_store::apply(&mut host) {
        return report_error(g, &CmdError::conflict(e));
    }
    let midi = match midi::source(
        opts.midi_in.as_deref(),
        opts.midi_script.as_deref(),
        !opts.no_device,
    ) {
        Ok(m) => m,
        Err(e) => return report_error(g, &e),
    };
    let output = if opts.no_device {
        eprintln!("出力: 無音（--no-device）");
        host.set_record_sources(Sources {
            audio: Source::None,
            midi,
        });
        Output::Silent(SilentOutput::start(audio, !opts.unpaced))
    } else {
        let prefs = host
            .io_settings
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .prefs
            .clone();
        match xirius_session::output::Output::configured(
            xirius_session::output::OutputMode::Device,
            audio,
            &prefs,
        ) {
            Ok(Some(o)) => {
                eprintln!("出力: {}", o.describe());
                for w in host.set_midi_out(Some(xirius_session::midi_out::system_maker())) {
                    eprintln!("注意: {w}");
                }
                host.set_record_sources(Sources {
                    audio: Source::Device,
                    midi,
                });
                Output::Device(o)
            }
            Ok(None) => unreachable!("device output"),
            Err(e) => {
                return report_error(
                    g,
                    &CmdError::conflict(format!(
                        "オーディオ出力を開始できませんでした: {e}（音を出さずに試すには --no-device を指定してください）"
                    )),
                );
            }
        }
    };

    if opts.looping {
        host.set_looping(session.project(), true);
    }
    if let Some(t) = from {
        host.seek_tick(t);
    }
    host.play();

    let code = serve(g, registry, &mut session, &mut host);

    host.stop();
    match output {
        Output::Device(o) => drop(o),
        Output::Silent(s) => drop(s),
    }
    close(g, &mut session);
    code
}

/// Reads lines until `quit` or the end of input. A line that fails is reported and the loop goes
/// on (a person typing); the exit code says whether any did (a script).
fn serve(
    g: &Global,
    registry: &Arc<Registry>,
    session: &mut Session,
    host: &mut SessionHost,
) -> u8 {
    let mut code = EXIT_OK;
    let stdin = io::stdin();
    for (i, line) in stdin.lock().lines().enumerate() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = i + 1;
        let words = match args::split_line(line) {
            Ok(w) => w,
            Err(Usage(m)) => {
                code = usage_error(&format!("{at} 行目: {m}"));
                continue;
            }
        };
        match words[0].as_str() {
            "quit" | "exit" => return code,
            "wait" => {
                match words.get(1) {
                    Some(pos) => match session.project().time_map().parse(pos) {
                        Ok(t) => wait_until(host, t),
                        Err(e) => {
                            eprintln!("{at} 行目:");
                            report_error(g, &e);
                            code = EXIT_COMMAND;
                        }
                    },
                    None if host.is_recording() => {
                        code = usage_error(&format!(
                            "{at} 行目: 録音中は位置を指定しない wait を使用できません（wait 5.1.0 のように位置を指定するか、record.stop で停止してください）"
                        ));
                    }
                    None => wait_until_stopped(host),
                }
                continue;
            }
            "status" => {
                print_status(g, session, host);
                continue;
            }
            _ => {}
        }
        collect(g, session, host);
        let words = alias(registry, &words);
        let (name, used) = args::resolve_name(registry, &words);
        let Some(def) = registry.get(&name) else {
            eprintln!("{at} 行目:");
            let e = session
                .run(&name, &Map::new(), &options(g))
                .err()
                .unwrap_or_else(|| {
                    CmdError::invalid(format!("コマンド `{name}` が見つかりません"))
                });
            report_error(g, &e);
            code = EXIT_COMMAND;
            continue;
        };
        let raw = match args::parse_args(def, &words[used..]) {
            Ok(m) => m,
            Err(Usage(m)) => {
                code = usage_error(&format!("{at} 行目: {m}"));
                continue;
            }
        };
        match run_and_persist(session, Some(host), &name, &raw, &options(g), !g.no_save) {
            Ok(r) => print_result(g, &r),
            Err(e) => {
                eprintln!("{at} 行目で停止しました:");
                report_error(g, &e);
                code = EXIT_COMMAND;
            }
        }
    }
    // The input ended: a recording is placed, and the song plays out unless it repeats for ever.
    if host.is_recording() {
        let none = Map::new();
        match run_and_persist(
            session,
            Some(host),
            "record.stop",
            &none,
            &options(g),
            !g.no_save,
        ) {
            Ok(r) => print_result(g, &r),
            Err(e) => {
                report_error(g, &e);
                code = EXIT_COMMAND;
            }
        }
    }
    if !(host.looping() && session.project().loop_range.is_some()) {
        wait_until_stopped(host);
    }
    collect(g, session, host);
    code
}

/// Takes the edits made in the plugins' editors into the song, and saves it.
fn collect(g: &Global, session: &mut Session, host: &mut SessionHost) {
    if !host.plugins().live() {
        return;
    }
    let none = Map::new();
    match run_and_persist(
        session,
        Some(host),
        "plugin.collect",
        &none,
        &options(g),
        !g.no_save,
    ) {
        Ok(r) if r.step.is_some() => print_result(g, &r),
        Ok(_) => {}
        Err(e) => {
            report_error(g, &e);
        }
    }
}

/// `play`, `stop` and `seek` are short for the transport commands.
fn alias(registry: &Registry, words: &[String]) -> Vec<String> {
    let first = words[0].as_str();
    if matches!(first, "play" | "stop" | "seek") && registry.get(first).is_none() {
        let mut v = vec![format!("transport.{first}")];
        v.extend(words[1..].iter().cloned());
        return v;
    }
    words.to_vec()
}

/// Waits for the song to play out.
fn wait_until_stopped(host: &mut SessionHost) {
    wait_for(host, |_| false);
}

/// Waits until the transport reaches `tick` (or stops).
fn wait_until(host: &mut SessionHost, tick: i64) {
    let frame = host.tempo_map().frame_at(tick, f64::from(host.rate()));
    wait_for(host, |at| at >= frame);
}

/// Waits until the transport stops or plays at a frame `there` accepts. The engine reports its
/// transport at the end of a callback, so what the host has heard may be from before the last
/// command (a `play` or `seek` sent a moment ago, or a song that played out before a
/// recording started). It is taken as it is only once the engine has rendered [`SETTLED`]
/// callbacks since the wait began, however long a busy machine makes them.
fn wait_for(host: &mut SessionHost, there: impl Fn(u64) -> bool) {
    let stats = host.controller().stats();
    let from = stats.callbacks();
    let begun = Instant::now();
    loop {
        let settled = stats.callbacks() >= from + SETTLED;
        let st = host.poll();
        if settled && (!st.playing || there(st.frame)) {
            return;
        }
        if !settled && begun.elapsed() > STALLED {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn print_status(g: &Global, session: &Session, host: &mut SessionHost) {
    // The engine reports at the end of a callback; a command typed just before has been
    // applied by the next one or two.
    let stats = host.controller().stats();
    let (from, begun) = (stats.callbacks(), Instant::now());
    while stats.callbacks() < from + SETTLED && begun.elapsed() < STALLED {
        thread::sleep(Duration::from_millis(5));
    }
    let st = host.poll();
    let rate = f64::from(host.rate());
    let tick = host.tempo_map().tick_of_frame(st.frame, rate);
    let position = session.project().time_map().format(tick);
    if g.json {
        println!(
            "{}",
            json!({
                "ok": true,
                "playing": st.playing,
                "frame": st.frame,
                "tick": tick,
                "position": position,
                "looping": host.looping(),
            })
        );
    } else {
        println!(
            "{} {position}（{} フレーム）{}",
            if st.playing { "再生中" } else { "停止中" },
            st.frame,
            if host.looping() { " ループ" } else { "" }
        );
    }
}
