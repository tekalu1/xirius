//! Trying real plugins one by one for the table in `docs/plugin-test-matrix.md` (PLG-041):
//! whether each one loads, plays, gives its state back in a new instance and opens its editor.
//!
//! Each plugin runs in a process of its own ([`trial_main`]) on the scan's hidden desktop and
//! in a job that is killed at the time limit, so a crash, a hang or a licence dialog only ends
//! that plugin's row, and no window comes up in front of the user. Nothing opens an audio
//! device: the audio is rendered into memory. The process writes its record after every stage,
//! so a crash shows which stage it was in.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::child::{end_now, quiet_crashes};
use crate::spawn::{self, Desktop, Exit};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 512;
/// Two seconds of audio; an instrument gets a note for the first.
const BLOCKS: usize = 188;
const NOTE_OFF_BLOCK: usize = 94;
/// How long an editor runs before it is captured and closed.
const EDITOR_TIME: Duration = Duration::from_secs(2);
/// How many parameters the state stage moves before saving.
const TRIED_PARAMS: usize = 3;
/// How near a value read back must be: plugins round to their own steps (FM8 keeps 0.8 as
/// 0.799).
const CLOSE: f64 = 5e-3;
/// How long messages run before values are read back.
const SETTLE: Duration = Duration::from_millis(100);
/// How long a new instance may take to show a loaded state.
const LOAD_WAIT: Duration = Duration::from_secs(3);

/// One plugin to try: a bundle and, for bundles with several plugins, the plugin's name.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub bundle: PathBuf,
    pub name: Option<String>,
}

impl Pick {
    /// `path` or `path<TAB>name`, one per line; empty lines and `#` lines are skipped.
    pub fn parse_list(text: &str) -> Vec<Pick> {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| match l.split_once('\t') {
                Some((p, n)) => Pick {
                    bundle: p.trim().into(),
                    name: Some(n.trim().to_string()),
                },
                None => Pick {
                    bundle: l.into(),
                    name: None,
                },
            })
            .collect()
    }
}

/// How a stage went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Ok,
    Failed,
    /// Not tried: our host does not offer it for this format.
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub outcome: Outcome,
    pub note: String,
    pub ms: u64,
}

/// What one plugin did. A stage that is `None` was not reached.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Trial {
    pub bundle: String,
    pub format: String,
    pub name: String,
    pub vendor: String,
    pub instrument: bool,
    pub load: Option<Stage>,
    pub play: Option<Stage>,
    pub state: Option<Stage>,
    pub editor: Option<Stage>,
    /// The stage the process was in when it ended without finishing, and how it ended.
    pub ended: Option<String>,
    /// The stage that is running; cleared when it is written.
    pub running: Option<String>,
    /// The call within the running stage, so a crash says where it was.
    #[serde(default)]
    pub step: Option<String>,
}

impl Trial {
    fn stages(&mut self) -> [(&'static str, &mut Option<Stage>); 4] {
        [
            ("load", &mut self.load),
            ("play", &mut self.play),
            ("state", &mut self.state),
            ("editor", &mut self.editor),
        ]
    }
}

/// Tries each pick in its own process: `program [args] --bundle <p> [--name <n>] --out <f>
/// [--shots <dir>]`. A process that does not finish within `timeout` is killed.
pub fn run_trials(
    program: &Path,
    args: &[String],
    picks: &[Pick],
    work: &Path,
    shots: Option<&Path>,
    timeout: Duration,
    progress: &mut dyn FnMut(&Trial),
) -> std::io::Result<Vec<Trial>> {
    std::fs::create_dir_all(work)?;
    let desktop = Desktop::hidden();
    let mut out = Vec::new();
    for (i, pick) in picks.iter().enumerate() {
        let file = work.join(format!("trial-{i}.json"));
        let _ = std::fs::remove_file(&file);
        let mut a = args.to_vec();
        a.extend([
            "--bundle".into(),
            pick.bundle.to_string_lossy().into_owned(),
        ]);
        if let Some(n) = &pick.name {
            a.extend(["--name".into(), n.clone()]);
        }
        a.extend(["--out".into(), file.to_string_lossy().into_owned()]);
        if let Some(s) = shots {
            a.extend(["--shots".into(), s.to_string_lossy().into_owned()]);
        }
        let exit = spawn::run(program, &a, timeout, desktop.as_ref(), &|| false)?;
        let mut t: Trial = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| Trial {
                bundle: pick.bundle.to_string_lossy().into_owned(),
                name: pick.name.clone().unwrap_or_default(),
                running: Some("load".into()),
                ..Trial::default()
            });
        let how = match exit {
            Exit::Code(0) => None,
            Exit::Code(c) => Some(format!("終了コード 0x{c:08X}")),
            Exit::TimedOut => Some(format!("{} 秒で時間切れ", timeout.as_secs())),
            Exit::Stopped => Some("止めた".into()),
        };
        let how = how.map(|h| match t.step.take() {
            Some(step) => format!("{h}（{step}）"),
            None => h,
        });
        if let (Some(stage), Some(how)) = (t.running.take(), how) {
            for (name, slot) in t.stages() {
                if name == stage {
                    *slot = Some(Stage {
                        outcome: Outcome::Failed,
                        note: how.clone(),
                        ms: 0,
                    });
                }
            }
            t.ended = Some(format!("{stage}: {how}"));
        }
        progress(&t);
        out.push(t);
    }
    Ok(out)
}

/// The rows of the table: plugin, format, kind and the four stages.
pub fn markdown_rows(trials: &[Trial]) -> String {
    let cell = |s: &Option<Stage>| match s {
        None => "（未到達）".to_string(),
        Some(s) => {
            let mark = match s.outcome {
                Outcome::Ok => "○",
                Outcome::Failed => "×",
                Outcome::Unsupported => "—",
            };
            format!("{mark} {}", s.note.replace('|', "/"))
        }
    };
    let mut out = String::new();
    for t in trials {
        let kind = if t.instrument {
            "インストゥルメント"
        } else {
            "エフェクト"
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            t.name,
            t.vendor,
            t.format.to_uppercase(),
            kind,
            cell(&t.load),
            cell(&t.play),
            cell(&t.state),
            cell(&t.editor),
        ));
    }
    out
}

/// The trial process: `--bundle <path> [--name <name>] --out <file> [--shots <dir>]`.
/// Ends the process itself once the record is written.
pub fn trial_main(args: &[String]) -> i32 {
    quiet_crashes();
    let (mut bundle, mut name, mut out, mut shots) = (None, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().cloned();
        match (a.as_str(), v) {
            ("--bundle", Some(v)) => bundle = Some(PathBuf::from(v)),
            ("--name", Some(v)) => name = Some(v),
            ("--out", Some(v)) => out = Some(PathBuf::from(v)),
            ("--shots", Some(v)) => shots = Some(PathBuf::from(v)),
            _ => return usage(),
        }
    }
    let (Some(bundle), Some(out)) = (bundle, out) else {
        return usage();
    };
    let mut rec = Recorder {
        out,
        trial: Trial {
            bundle: bundle.to_string_lossy().into_owned(),
            name: name.clone().unwrap_or_default(),
            ..Trial::default()
        },
    };
    if xirius_clap::is_clap(&bundle) {
        rec.trial.format = "clap".into();
        clap_trial(&bundle, name.as_deref(), &mut rec);
    } else {
        rec.trial.format = "vst3".into();
        vst3_trial(&bundle, name.as_deref(), shots.as_deref(), &mut rec);
    }
    rec.save();
    end_now(0)
}

fn usage() -> i32 {
    eprintln!("usage: --bundle <path> [--name <name>] --out <file> [--shots <dir>]");
    2
}

/// Keeps the record on disk as it grows.
struct Recorder {
    out: PathBuf,
    trial: Trial,
}

impl Recorder {
    fn save(&self) {
        if let Ok(body) = serde_json::to_vec(&self.trial) {
            let _ = std::fs::write(&self.out, body);
        }
    }

    fn start(&mut self, stage: &str) -> Instant {
        self.trial.running = Some(stage.into());
        self.trial.step = None;
        self.save();
        Instant::now()
    }

    /// Notes the call about to be made in the running stage.
    fn step(&mut self, call: &str) {
        self.trial.step = Some(call.into());
        self.save();
    }

    /// Writes a stage's result; `Err` is a failure with its message.
    fn finish(&mut self, stage: &str, began: Instant, result: Result<(Outcome, String), String>) {
        let (outcome, note) = result.unwrap_or_else(|e| (Outcome::Failed, e));
        let s = Some(Stage {
            outcome,
            note,
            ms: began.elapsed().as_millis() as u64,
        });
        for (name, slot) in self.trial.stages() {
            if name == stage {
                *slot = s.clone();
            }
        }
        self.trial.running = None;
        self.trial.step = None;
        self.save();
    }
}

/// What the rendered audio looked like.
#[derive(Default)]
struct Level {
    sum: f64,
    frames: usize,
    peak: f32,
    finite: bool,
}

impl Level {
    fn new() -> Level {
        Level {
            finite: true,
            ..Level::default()
        }
    }

    fn add(&mut self, l: &[f32], r: &[f32]) {
        for (&a, &b) in l.iter().zip(r) {
            self.finite &= a.is_finite() && b.is_finite();
            self.peak = self.peak.max(a.abs()).max(b.abs());
            self.sum += f64::from(a * a + b * b) / 2.0;
            self.frames += 1;
        }
    }

    fn result(&self, latency: u32) -> Result<(Outcome, String), String> {
        if !self.finite {
            return Err("NaN か無限大が出た".into());
        }
        let rms = (self.sum / self.frames.max(1) as f64).sqrt();
        // Below -120 dB is a plugin with nothing loaded, not a sound.
        let level = if self.peak < 1e-6 {
            "無音".to_string()
        } else {
            format!("RMS {:.1} dB", 20.0 * rms.max(1e-12).log10())
        };
        let lat = if latency > 0 {
            format!("・遅延 {latency}")
        } else {
            String::new()
        };
        Ok((Outcome::Ok, format!("{level}{lat}")))
    }
}

/// A 440 Hz sine at -12 dB on both channels, for effects.
fn sine(block: usize, l: &mut [f32], r: &mut [f32]) {
    for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
        let t = (block * BLOCK + i) as f64 / RATE;
        let v = (0.25 * (std::f64::consts::TAU * 440.0 * t).sin()) as f32;
        *a = v;
        *b = v;
    }
}

/// A value for `current` that differs from it, on the parameter's steps when it has them.
fn other_value(current: f64, steps: i32) -> f64 {
    let v = if current < 0.5 { 0.8 } else { 0.2 };
    if steps > 0 {
        let n = f64::from(steps);
        let s = (v * n).round() / n;
        if (s - current).abs() < 1e-9 {
            return if current < 0.5 { 1.0 } else { 0.0 };
        }
        return s;
    }
    v
}

/// The state stage's note from each tried parameter's title and whether its value came back
/// in the new instance. A failure when none did.
fn state_note(
    back: &[(String, bool)],
    same: bool,
    bytes: usize,
) -> Result<(Outcome, String), String> {
    let missing: String = back
        .iter()
        .filter(|(_, b)| !b)
        .map(|(t, _)| format!("「{t}」"))
        .collect();
    let n = back.len();
    let ok = back.iter().filter(|(_, b)| *b).count();
    if ok == 0 {
        return Err(format!("{n} 個とも戻らない（{missing}）"));
    }
    let mut note = format!("{n} 個中 {ok} 個が戻る");
    if ok < n {
        note.push_str(&format!("（{missing}は戻らない）"));
    }
    let same = if same {
        "同じバイト列"
    } else {
        "バイト列は違う"
    };
    note.push_str(&format!("・{same}（{bytes} B）"));
    Ok((Outcome::Ok, note))
}

/// Runs this thread's messages for `time`. Plugins that move their controller from a timer or
/// a posted message do it here.
fn settle(time: Duration) {
    let end = Instant::now() + time;
    loop {
        #[cfg(windows)]
        // SAFETY: a standard PeekMessage loop on a zeroed MSG.
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
            };
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if Instant::now() >= end {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn vst3_trial(bundle: &Path, name: Option<&str>, shots: Option<&Path>, rec: &mut Recorder) {
    use xirius_vst3::{Module, Note, Placement, Plugin};
    // VST3 parameter flags.
    const CAN_AUTOMATE: i32 = 1;
    const READ_ONLY: i32 = 1 << 1;
    const HIDDEN: i32 = 1 << 4;
    const PROGRAM_CHANGE: i32 = 1 << 15;
    const BYPASS: i32 = 1 << 16;

    let began = rec.start("load");
    let loaded = (|| {
        let module = Arc::new(Module::load(bundle).map_err(|e| e.to_string())?);
        let class = module
            .classes()
            .into_iter()
            .filter(|c| c.is_audio_processor())
            .find(|c| name.is_none_or(|n| c.name == n))
            .ok_or_else(|| "そのクラスが無い".to_string())?;
        let plugin = Plugin::new(module.clone(), &class).map_err(|e| e.to_string())?;
        Ok::<_, String>((module, class, plugin))
    })();
    let (module, class, mut plugin) = match loaded {
        Ok(x) => x,
        Err(e) => return rec.finish("load", began, Err(e)),
    };
    rec.trial.name = class.name.clone();
    rec.trial.vendor = if class.vendor.is_empty() {
        module.vendor.clone()
    } else {
        class.vendor.clone()
    };
    rec.trial.instrument = class.is_instrument();
    let params = plugin.params();
    let note = format!("パラメーター {}", params.len());
    rec.finish("load", began, Ok((Outcome::Ok, note)));

    let began = rec.start("play");
    let mut audio = match plugin
        .audio()
        .and_then(|mut a| a.prepare(RATE, BLOCK, false).map(|lat| (a, lat)))
    {
        Ok(x) => x,
        Err(e) => return rec.finish("play", began, Err(e.to_string())),
    };
    let latency = audio.1;
    let instrument = rec.trial.instrument || audio.0.input_channels().is_empty();
    let mut level = Level::new();
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for b in 0..BLOCKS {
        let mut notes = Vec::new();
        if instrument {
            l.fill(0.0);
            r.fill(0.0);
            let on = b == 0;
            if on || b == NOTE_OFF_BLOCK {
                notes.push(Note {
                    offset: 0,
                    pitch: 60,
                    velocity: 0.8,
                    on,
                });
            }
        } else {
            sine(b, &mut l, &mut r);
        }
        audio.0.process(&mut l, &mut r, &notes);
        level.add(&l, &r);
    }
    rec.finish("play", began, level.result(latency));

    let began = rec.start("state");
    let picks: Vec<_> = params
        .iter()
        .filter(|p| {
            p.flags & CAN_AUTOMATE != 0
                && p.flags & (READ_ONLY | HIDDEN | PROGRAM_CHANGE | BYPASS) == 0
        })
        .take(TRIED_PARAMS)
        .cloned()
        .collect();
    let result = (|| {
        if picks.is_empty() {
            rec.step("getState");
            let s = plugin.get_state().map_err(|e| e.to_string())?;
            rec.step("新しいインスタンス");
            let b = Plugin::new(module.clone(), &class).map_err(|e| e.to_string())?;
            rec.step("setState");
            b.set_state(&s).map_err(|e| e.to_string())?;
            return Ok((
                Outcome::Ok,
                format!(
                    "動かせるパラメーターが無い・読み戻せた（{} B）",
                    s.component.len()
                ),
            ));
        }
        rec.step("setParamNormalized");
        for p in &picks {
            plugin.set_param(
                p.id,
                other_value(plugin.param(p.id).unwrap_or(p.default), p.step_count),
            );
        }
        // The processor takes the values in its next blocks.
        rec.step("process");
        for _ in 0..4 {
            l.fill(0.0);
            r.fill(0.0);
            audio.0.process(&mut l, &mut r, &[]);
        }
        settle(SETTLE);
        plugin.follow_processor();
        let wants: Vec<_> = picks.iter().map(|p| plugin.param(p.id)).collect();
        rec.step("getState");
        let saved = plugin.get_state().map_err(|e| e.to_string())?;
        rec.step("新しいインスタンス");
        let mut b = Plugin::new(module.clone(), &class).map_err(|e| e.to_string())?;
        rec.step("setState");
        b.set_state(&saved).map_err(|e| e.to_string())?;
        // Some plugins load a state in the background and their controllers follow on a timer
        // or once the processor runs: read until every value is back or the wait is over.
        rec.step("process（新しいインスタンス）");
        let mut part = b.audio().ok();
        if part
            .as_mut()
            .is_some_and(|a| a.prepare(RATE, BLOCK, false).is_err())
        {
            part = None;
        }
        let until = Instant::now() + LOAD_WAIT;
        let back = loop {
            if let Some(a) = part.as_mut() {
                for _ in 0..4 {
                    l.fill(0.0);
                    r.fill(0.0);
                    a.process(&mut l, &mut r, &[]);
                }
            }
            settle(SETTLE);
            b.follow_processor();
            let back: Vec<_> = picks
                .iter()
                .zip(&wants)
                .map(|(p, want)| {
                    let ok = want
                        .zip(b.param(p.id))
                        .is_some_and(|(w, x)| (x - w).abs() < CLOSE);
                    (p.title.clone(), ok)
                })
                .collect();
            if back.iter().all(|(_, ok)| *ok) || Instant::now() >= until {
                break back;
            }
        };
        drop(part);
        rec.step("getState（新しいインスタンス）");
        let again = b.get_state().map_err(|e| e.to_string())?;
        let bytes = saved.component.len() + saved.controller.len();
        state_note(&back, again == saved, bytes)
    })();
    rec.finish("state", began, result);
    drop(audio);

    let began = rec.start("editor");
    let result = (|| {
        rec.step("createView");
        let mut editor = plugin
            .open_editor_at(&class.name, Placement::offscreen())
            .map_err(|e| e.to_string())?;
        editor.pump(EDITOR_TIME);
        let (w, h) = editor.size();
        let mut note = format!("{w}×{h}");
        if let Some(dir) = shots {
            rec.step("撮影");
            let _ = std::fs::create_dir_all(dir);
            let file = dir.join(format!("{}.bmp", file_name(&class.name)));
            match editor.capture_bmp(&file) {
                Ok(c) if c.distinct_colors > 1 => {
                    note.push_str(&format!("・{} 色", c.distinct_colors))
                }
                Ok(_) => note.push_str("・画は空"),
                Err(_) => note.push_str("・撮れない"),
            }
        }
        if editor.took_foreground() {
            note.push_str("・前面を取った");
        }
        Ok((Outcome::Ok, note))
    })();
    rec.finish("editor", began, result);
}

fn clap_trial(bundle: &Path, name: Option<&str>, rec: &mut Recorder) {
    use xirius_clap::{Bundle, Note, Plugin};

    let began = rec.start("load");
    let loaded = (|| {
        let b = Bundle::load(bundle).map_err(|e| e.to_string())?;
        let d = b
            .descriptors()
            .iter()
            .find(|d| name.is_none_or(|n| d.name == n))
            .cloned()
            .ok_or_else(|| "そのプラグインが無い".to_string())?;
        let p = Plugin::new(&b, &d.id).map_err(|e| e.to_string())?;
        Ok::<_, String>((b, d, p))
    })();
    let (b, d, mut plugin) = match loaded {
        Ok(x) => x,
        Err(e) => return rec.finish("load", began, Err(e)),
    };
    rec.trial.name = d.name.clone();
    rec.trial.vendor = d.vendor.clone();
    rec.trial.instrument = d.is_instrument();
    let note = format!("パラメーター {}", plugin.params().len());
    rec.finish("load", began, Ok((Outcome::Ok, note)));

    let began = rec.start("play");
    let mut audio = match plugin
        .audio()
        .and_then(|mut a| a.prepare(RATE, BLOCK, false).map(|lat| (a, lat)))
    {
        Ok(x) => x,
        Err(e) => return rec.finish("play", began, Err(e.to_string())),
    };
    let latency = audio.1;
    let instrument = rec.trial.instrument || audio.0.input_channels().is_empty();
    let mut level = Level::new();
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for blk in 0..BLOCKS {
        let mut notes = Vec::new();
        if instrument {
            l.fill(0.0);
            r.fill(0.0);
            let on = blk == 0;
            if on || blk == NOTE_OFF_BLOCK {
                notes.push(Note {
                    offset: 0,
                    pitch: 60,
                    velocity: 0.8,
                    on,
                });
            }
        } else {
            sine(blk, &mut l, &mut r);
        }
        audio.0.process(&mut l, &mut r, &notes);
        level.add(&l, &r);
    }
    rec.finish("play", began, level.result(latency));

    let began = rec.start("state");
    let picks: Vec<_> = plugin
        .params()
        .iter()
        .filter(|p| p.is_automatable() && !p.is_read_only() && !p.is_hidden())
        .take(TRIED_PARAMS)
        .cloned()
        .collect();
    let result = (|| {
        if picks.is_empty() {
            rec.step("state.save");
            let s = plugin.get_state().map_err(|e| e.to_string())?;
            rec.step("新しいインスタンス");
            let n = Plugin::new(&b, &d.id).map_err(|e| e.to_string())?;
            rec.step("state.load");
            n.set_state(&s).map_err(|e| e.to_string())?;
            return Ok((
                Outcome::Ok,
                format!("動かせるパラメーターが無い・読み戻せた（{} B）", s.len()),
            ));
        }
        rec.step("set_param");
        for p in &picks {
            let current = plugin.param(p.id).unwrap_or(p.default_normalized());
            let steps = if p.is_stepped() { p.step_count() } else { 0 };
            plugin.set_param(p.id, other_value(current, steps));
        }
        rec.step("process");
        for _ in 0..4 {
            l.fill(0.0);
            r.fill(0.0);
            audio.0.process(&mut l, &mut r, &[]);
        }
        settle(SETTLE);
        let wants: Vec<_> = picks.iter().map(|p| plugin.param(p.id)).collect();
        rec.step("state.save");
        let saved = plugin.get_state().map_err(|e| e.to_string())?;
        rec.step("新しいインスタンス");
        let n = Plugin::new(&b, &d.id).map_err(|e| e.to_string())?;
        rec.step("state.load");
        n.set_state(&saved).map_err(|e| e.to_string())?;
        settle(SETTLE);
        let back: Vec<_> = picks
            .iter()
            .zip(&wants)
            .map(|(p, want)| {
                let ok = want
                    .zip(n.param(p.id))
                    .is_some_and(|(w, x)| (x - w).abs() < CLOSE);
                (p.title.clone(), ok)
            })
            .collect();
        rec.step("state.save（新しいインスタンス）");
        let again = n.get_state().map_err(|e| e.to_string())?;
        state_note(&back, again == saved, saved.len())
    })();
    rec.finish("state", began, result);
    drop(audio);

    let began = rec.start("editor");
    rec.finish(
        "editor",
        began,
        Ok((
            Outcome::Unsupported,
            "CLAP のエディターはホストに未実装".into(),
        )),
    );
}

/// `name` with the characters a file name cannot have replaced.
fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_has_paths_and_names() {
        let picks = Pick::parse_list("# comment\n\nC:/a.vst3\nC:/w.vst3\tCLA-76 Stereo\n");
        assert_eq!(
            picks,
            vec![
                Pick {
                    bundle: "C:/a.vst3".into(),
                    name: None
                },
                Pick {
                    bundle: "C:/w.vst3".into(),
                    name: Some("CLA-76 Stereo".into())
                },
            ]
        );
    }

    #[test]
    fn another_value_differs_and_stays_on_the_steps() {
        assert_eq!(other_value(0.1, 0), 0.8);
        assert_eq!(other_value(0.9, 0), 0.2);
        assert_eq!(other_value(0.0, 1), 1.0);
        assert_eq!(other_value(1.0, 1), 0.0);
        assert_eq!(other_value(0.0, 4), 0.75);
    }

    #[test]
    fn the_state_note_counts_what_came_back() {
        let t = |s: &str, b: bool| (s.to_string(), b);
        assert_eq!(
            state_note(&[t("A", true), t("B", true)], true, 10),
            Ok((
                Outcome::Ok,
                "2 個中 2 個が戻る・同じバイト列（10 B）".into()
            ))
        );
        assert_eq!(
            state_note(&[t("A", true), t("BPM", false)], false, 10),
            Ok((
                Outcome::Ok,
                "2 個中 1 個が戻る（「BPM」は戻らない）・バイト列は違う（10 B）".into()
            ))
        );
        assert_eq!(
            state_note(&[t("A", false), t("B", false)], true, 10),
            Err("2 個とも戻らない（「A」「B」）".into())
        );
    }

    #[test]
    fn a_row_marks_each_stage() {
        let ok = |n: &str| {
            Some(Stage {
                outcome: Outcome::Ok,
                note: n.into(),
                ms: 1,
            })
        };
        let t = Trial {
            name: "Gain".into(),
            vendor: "V".into(),
            format: "vst3".into(),
            load: ok("パラメーター 3"),
            play: ok("RMS -15.1 dB"),
            state: Some(Stage {
                outcome: Outcome::Failed,
                note: "a|b".into(),
                ms: 0,
            }),
            editor: None,
            ..Trial::default()
        };
        assert_eq!(
            markdown_rows(&[t]),
            "| Gain | V | VST3 | エフェクト | ○ パラメーター 3 | ○ RMS -15.1 dB | × a/b | （未到達） |\n"
        );
    }
}
