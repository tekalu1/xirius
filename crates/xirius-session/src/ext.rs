//! The session layer's answers to the commands core leaves to an extension (ARC-016).
//!
//! - `export.mixdown`, `export.stems`: at once, or with `--background` as a job that
//!   `export.status` reports on and `export.cancel` stops (EXP-001, EXP-002, AI-002). Either
//!   way the export of a saved song leaves a record in the song folder ([`crate::jobs`]), so
//!   another process lists it, follows it and stops it, and one cut off reads as interrupted.
//! - `export.update`, `export.open`, `export.reveal_os`, `export.delete`: what the list of
//!   exports does with one of them (EXP-024, EXP-025).
//! - `transport.*`: a player of its own, started on the first transport command, that follows
//!   the song as commands change it (ARR-001, ENG-009). It plays into the device or into a
//!   silent output, as [`ExtConfig::output`] says; with [`OutputMode::None`] there is no
//!   player and the transport commands are refused.
//! - `plugin.rescan`, `browser.plugin.list`: the installed plugins (PLG-001, PLG-003, BRW-009).
//! - `plugin.scan.status`: the scan that runs or ran last, and the list it keeps (PLG-059).
//!   With `--background` the scan is a job like an export, and the catalogue it makes is used
//!   from the next command on (PLG-021).
//! - `plugin.status`, `plugin.restart`: the plugins of the player (OPEN-02). `plugin.collect`
//!   writes into the song, which an extension cannot, so it needs `xirius play`, whose loop
//!   keeps the engine itself.
//! - `record.start`, `record.stop`, `record.status`: recording on the player (REC-001,
//!   REC-016). With the device as the output the input is the device's; with a silent output
//!   an armed audio track records silence. MIDI comes from [`ExtExtras::midi`] (ENG-028). `record.stop` leaves the takes as a
//!   `record.take.add` ([`Extension::take_edits`]), so the session places them as one step.
//! - `browser.favorite`, `browser.hide`, `view.browser.hidden`, `location.*`: what the browser
//!   keeps between runs, in the user's file ([`crate::places`]; BRW-001, BRW-012, BRW-015).
//! - `project.cleanup`: the recordings no state of the song names go to the recycle bin
//!   ([`crate::cleanup`]; [`ExtExtras::trash`] says where they go instead).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::command::{Args, CommandDef, Output};
use xirius_core::id::TrackId;
use xirius_core::model::{Plugin, PluginKind, Project};
use xirius_core::store::now_ms;
use xirius_core::{CmdError, CmdResult, ExtContext, Extension};
use xirius_engine::record::MidiInput;
use xirius_plugins::{Catalog, ScanOptions, ScanReport, scan, scan_seeing};

use crate::cleanup::{self, Trash};
use crate::commands::{self, CLEANUP, NAMES, RECORD};
use crate::export::{self, CANCELLED, Progress};
use crate::host::{HostConfig, SessionHost};
use crate::import;
use crate::jobs::Store;
use crate::midi_out::MidiOutMaker;
use crate::os;
use crate::output::{Output as Sink, OutputMode};
use crate::places;
use crate::reveal;
mod disk_cache;

/// Finished jobs kept in memory for `export.status`.
const KEEP_FINISHED: usize = 32;
/// How often a running export looks for a cancel from another process.
const WATCH: Duration = Duration::from_millis(100);
/// How often a running export writes how far it is.
const SAVE_EVERY: u32 = 5;
/// How long closing waits for exports to stop before it marks them interrupted.
const CLOSE_WAIT: Duration = Duration::from_secs(3);
/// What an export stopped by closing says.
const CLOSED: &str =
    "アプリまたはサーバーが終了したため中止しました。ファイルは書き出されていません";
/// The message of a plugin scan stopped by `export.cancel` (PLG-021).
const SCAN_STOPPED: &str = "プラグインのスキャンを中止しました";

pub struct ExtConfig {
    pub output: OutputMode,
    /// Where `plugin.rescan` looks and which cache it keeps.
    pub scan: ScanOptions,
    /// The catalogue to use; `None` reads `scan.cache` when a plugin is first needed.
    pub catalog: Option<Arc<Catalog>>,
}

impl Default for ExtConfig {
    fn default() -> Self {
        ExtConfig {
            output: OutputMode::None,
            scan: ScanOptions::default(),
            catalog: None,
        }
    }
}

/// Makes a new MIDI input for each recording (a device's ports, or a stand-in in tests).
pub type MidiMaker = Box<dyn FnMut() -> Box<dyn MidiInput> + Send>;

/// What a session extension is given besides [`ExtConfig`] ([`SessionExt::with`]).
#[derive(Default)]
pub struct ExtExtras {
    pub io_settings: Option<xirius_prefs::Shared>,
    /// Where `record.start` takes MIDI from; `None`: armed instrument tracks are not recorded.
    pub midi: Option<MidiMaker>,
    /// Where the MIDI output ports tracks send to are opened; `None`: those tracks are silent.
    pub midi_out: Option<MidiOutMaker>,
    /// Where `project.cleanup` and an export that replaces files move them; `None`: the
    /// recycle bin.
    pub trash: Option<Trash>,
    /// The folder of the track presets; `None`: [`crate::track_preset::dir`].
    pub track_presets: Option<PathBuf>,
    /// The folder of the device presets; `None`: [`crate::xpreset::dir`].
    pub presets: Option<PathBuf>,
}

/// The engine that plays, and where its sound goes.
struct Player {
    host: SessionHost,
    sink: Sink,
    dir: Option<PathBuf>,
}

#[derive(Clone)]
enum JobState {
    Running,
    Done(Output),
    Failed(String),
    /// Stopped by closing the process it ran in.
    Interrupted(String),
}

/// An export or a plugin scan of this process.
struct Job {
    /// Only file-writing workers take this gate. UI reports never take it.
    disk_gate: Mutex<()>,
    id: String,
    command: String,
    /// The arguments that make the same export again (`Null` for a scan).
    args: Value,
    /// The file of a mix, the folder of stems.
    path: String,
    /// The exports whose files this one writes again (`export.update`, or an export of the
    /// same names): their lines leave the list when it is done.
    replaces: Vec<String>,
    /// The files it writes, so another export does not write over them while it runs.
    writes: Vec<PathBuf>,
    /// What the song was when the export started, to tell later which files are stale
    /// ([`crate::export_stale::base`]); `Null` for a scan.
    base: Value,
    /// The tracks told edited since, by file (`export.touch`, EXP-023).
    touched: Mutex<Map<String, Value>>,
    progress: Arc<Progress>,
    started: Instant,
    started_ms: u64,
    /// How long it took and when it ended.
    ended: Mutex<Option<(Duration, u64)>>,
    state: Mutex<JobState>,
    /// Set when the process closes, so a cancel then reads as interrupted.
    closing: AtomicBool,
    /// Where the record goes; `None` for a song not saved yet and for a scan.
    disk: Option<Store>,
    lock: Mutex<Option<File>>,
}

impl Job {
    fn running(&self) -> bool {
        matches!(*self.state.lock().unwrap(), JobState::Running)
    }

    fn is_export(&self) -> bool {
        self.command.starts_with("export.")
    }

    fn to_json(&self) -> Value {
        let st = self.state.lock().unwrap();
        self.json_of(&st)
    }

    fn json_of(&self, state: &JobState) -> Value {
        let ended = *self.ended.lock().unwrap();
        let elapsed = ended.map_or_else(|| self.started.elapsed(), |(d, _)| d);
        let secs = elapsed.as_secs_f64();
        let mut v = json!({
            "job": self.id,
            "command": self.command,
            "pid": std::process::id(),
            "started_ms": self.started_ms,
            "ended_ms": ended.map(|(_, ms)| ms),
            "done": self.progress.done(),
            "total": self.progress.total(),
            "fraction": self.progress.fraction(),
            "elapsed": (secs * 1000.0).round() / 1000.0,
        });
        if self.command == "plugin.rescan" {
            v["file"] = json!(self.progress.seeing());
        }
        if self.is_export() {
            let files: Vec<Value> = self
                .progress
                .files()
                .into_iter()
                .map(|f| json!({ "name": f.name, "done": f.done, "total": f.total }))
                .collect();
            let sound = self.progress.seconds_done();
            v["args"] = self.args.clone();
            v["path"] = json!(self.path);
            v["files"] = json!(files);
            // How many seconds of sound a second of work makes (EXP-017).
            v["speed"] = if sound > 0.0 && secs > 0.0 {
                json!((sound / secs * 10.0).round() / 10.0)
            } else {
                Value::Null
            };
            match self.replaces.as_slice() {
                [] => {}
                [one] => v["replaces"] = json!(one),
                many => v["replaces"] = json!(many),
            }
            if !self.base.is_null() {
                v["base"] = self.base.clone();
            }
            let touched = self.touched.lock().unwrap();
            if !touched.is_empty() {
                v["touched"] = Value::Object(touched.clone());
            }
        }
        let state = match state {
            JobState::Running => "running",
            JobState::Done(out) => {
                v["message"] = json!(out.message);
                v["result"] = out.data.clone();
                "done"
            }
            JobState::Failed(e) if e == CANCELLED || e == SCAN_STOPPED => {
                v["error"] = json!(e);
                "cancelled"
            }
            JobState::Failed(e) => {
                v["error"] = json!(e);
                "failed"
            }
            JobState::Interrupted(e) => {
                v["error"] = json!(e);
                "interrupted"
            }
        };
        v["state"] = json!(state);
        v
    }

    /// Writes how far it is, while it runs. Holding the state, so it cannot land after the
    /// record of how it ended.
    fn save(&self) {
        let _write = self.disk_gate.lock().unwrap();
        let st = self.state.lock().unwrap();
        if let (JobState::Running, Some(d)) = (&*st, &self.disk) {
            let record = self.json_of(&st);
            drop(st);
            let _ = d.write(&record);
        }
    }

    /// Ends the job with `r`, writes how it ended and lets go of its lock. Only the first end
    /// counts.
    fn finish(&self, r: &CmdResult<(String, Value)>) {
        let _write = self.disk_gate.lock().unwrap();
        let mut st = self.state.lock().unwrap();
        if !matches!(*st, JobState::Running) {
            return;
        }
        *self.ended.lock().unwrap() = Some((self.started.elapsed(), now_ms()));
        *st = match r {
            Ok((message, data)) => JobState::Done(Output::with(message.clone(), data.clone())),
            Err(e)
                if (e.message == CANCELLED || e.message == SCAN_STOPPED)
                    && self.closing.load(Ordering::Relaxed) =>
            {
                JobState::Interrupted(CLOSED.to_string())
            }
            Err(e) => JobState::Failed(e.message.clone()),
        };
        let ended = st.clone();
        drop(st);
        self.end(&ended);
    }

    /// Ends a job its process could not wait for.
    fn interrupt(&self) {
        let _write = self.disk_gate.lock().unwrap();
        let mut st = self.state.lock().unwrap();
        if !matches!(*st, JobState::Running) {
            return;
        }
        *self.ended.lock().unwrap() = Some((self.started.elapsed(), now_ms()));
        *st = JobState::Interrupted(CLOSED.to_string());
        let ended = st.clone();
        drop(st);
        self.end(&ended);
    }

    fn end(&self, st: &JobState) {
        let Some(d) = &self.disk else {
            return;
        };
        let record = self.json_of(st);
        for _ in 0..3 {
            if d.write(&record).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if let JobState::Done(_) = st {
            for old in &self.replaces {
                d.remove(old);
            }
        }
        d.release(&self.id, self.lock.lock().unwrap().take());
    }
}

/// Looks for a cancel from another process and writes the progress while `job` runs.
fn watch(job: Arc<Job>) {
    let _ = std::thread::Builder::new()
        .name(format!("{}-watch", job.id))
        .spawn(move || {
            let mut n = 0u32;
            while job.running() {
                std::thread::sleep(WATCH);
                if let Some(d) = &job.disk
                    && d.cancel_asked(&job.id)
                {
                    job.progress.cancel();
                }
                n += 1;
                if n.is_multiple_of(SAVE_EVERY) {
                    job.save();
                }
            }
        });
}

pub struct SessionExt {
    disk_cache: disk_cache::Cache,
    preset_cache: disk_cache::Presets,
    io_settings: Option<xirius_prefs::Shared>,
    output: OutputMode,
    preview: Option<crate::export_preview::Preview>,
    scan: ScanOptions,
    catalog: Option<Arc<Catalog>>,
    player: Option<Player>,
    jobs: Vec<Arc<Job>>,
    /// The file name patterns used last, for a song not saved yet (a saved song keeps them
    /// in its folder).
    names: Map<String, Value>,
    /// The catalogue a background scan made, not taken up yet.
    scanned: Arc<Mutex<Option<Arc<Catalog>>>>,
    /// The edits the last `record.stop` left for the session.
    pending: Vec<(String, Map<String, Value>)>,
    /// Where the player records MIDI from.
    midi: Option<Arc<Mutex<MidiMaker>>>,
    /// Where the player opens MIDI output ports.
    midi_out: Option<Arc<Mutex<MidiOutMaker>>>,
    trash: Trash,
    /// Where the track presets are kept.
    track_presets: Option<PathBuf>,
    /// Where the device presets are kept.
    presets: Option<PathBuf>,
    /// The `export.stale` events (EXP-023).
    watch: crate::export_watch::Watch,
}

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// An id no other process makes: the time, the process and a count.
fn new_id(kind: &str) -> String {
    format!(
        "{kind}-{:x}-{}-{}",
        now_ms(),
        std::process::id(),
        NEXT_JOB.fetch_add(1, Ordering::Relaxed)
    )
}

/// What `export.status` says of one record.
fn status_message(v: &Value) -> String {
    let id = v["job"].as_str().unwrap_or("");
    match v["state"].as_str() {
        Some("running") => format!(
            "{id}: {} {:.0}%",
            if v["command"] == "plugin.rescan" {
                "スキャン中"
            } else {
                "書き出し中"
            },
            v["fraction"].as_f64().unwrap_or(0.0) * 100.0
        ),
        Some("done") => format!("{id}: 完了\n{}", v["message"].as_str().unwrap_or("")),
        Some("cancelled") => format!("{id}: キャンセル済み"),
        Some("interrupted") => format!("{id}: 中断（{}）", v["error"].as_str().unwrap_or("")),
        _ => format!("{id}: 失敗: {}", v["error"].as_str().unwrap_or("")),
    }
}

/// The jobs a record says it replaced: one id, or a list of them.
fn replaced_by(v: &Value) -> Vec<Value> {
    match &v["replaces"] {
        Value::Array(ids) => ids.clone(),
        Value::Null => Vec::new(),
        one => vec![one.clone()],
    }
}

/// Whether two paths name the same file (Windows does not tell the case of names apart).
fn same_file(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// The files an export record wrote.
fn written_files(v: &Value) -> Vec<PathBuf> {
    match v["result"]["files"].as_array() {
        Some(files) => files
            .iter()
            .filter_map(|f| f["path"].as_str())
            .map(PathBuf::from)
            .collect(),
        None => v["result"]["path"]
            .as_str()
            .or(v["path"].as_str())
            .filter(|_| v["command"] == "export.mixdown")
            .map(PathBuf::from)
            .into_iter()
            .collect(),
    }
}

/// The files of a record `file` names (a written file's name or path), or all of them.
fn picked_files(v: &Value, file: Option<&str>) -> CmdResult<Vec<PathBuf>> {
    let all = written_files(v);
    let Some(f) = file else {
        return Ok(all);
    };
    let wanted = Path::new(f);
    let found: Vec<PathBuf> = all
        .into_iter()
        .filter(|p| {
            same_file(p, wanted)
                || (wanted.components().count() == 1
                    && p.file_name()
                        .is_some_and(|n| same_file(Path::new(n), wanted)))
        })
        .collect();
    if found.is_empty() {
        return Err(CmdError::invalid(format!(
            "{} は {f} を書き出していません",
            v["job"].as_str().unwrap_or_default()
        )));
    }
    Ok(found)
}

/// What a partial `export.update` of stems keeps: the record before, the strips written again
/// and the files of the others.
struct Part {
    record: Value,
    only: Vec<TrackId>,
}

impl Part {
    /// The record's files, in its order: strip and entry.
    fn files(&self) -> Vec<(String, Value)> {
        self.record["result"]["files"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| Some((f["strip"].as_str()?.to_string(), f.clone())))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The strips kept as they were.
    fn kept(&self) -> Vec<String> {
        self.files()
            .into_iter()
            .map(|(k, _)| k)
            .filter(|k| !self.only.iter().any(|t| t.to_string() == *k))
            .collect()
    }

    /// The result of rendering `only`, with the kept files in their places.
    fn merge(&self, r: CmdResult<(String, Value)>) -> CmdResult<(String, Value)> {
        let (message, mut data) = r?;
        let new: Vec<Value> = data["files"].as_array().cloned().unwrap_or_default();
        let mut files = Vec::new();
        let mut kept = 0;
        for (strip, old) in self.files() {
            match new.iter().find(|f| f["strip"] == strip.as_str()) {
                Some(f) => files.push(f.clone()),
                None => {
                    kept += 1;
                    files.push(old);
                }
            }
        }
        data["files"] = json!(files);
        data["kept"] = json!(kept);
        Ok((
            format!("{message}\n変わっていない {kept} 本のファイルはそのまま残しました"),
            data,
        ))
    }
}

impl SessionExt {
    pub fn new(config: ExtConfig) -> SessionExt {
        SessionExt::with(config, ExtExtras::default())
    }

    /// Like [`SessionExt::new`], with a MIDI input to record from.
    pub fn with(config: ExtConfig, extras: ExtExtras) -> SessionExt {
        SessionExt {
            disk_cache: Default::default(),
            preset_cache: Default::default(),
            io_settings: extras.io_settings,
            output: config.output,
            preview: None,
            scan: config.scan,
            catalog: config.catalog,
            player: None,
            jobs: Vec::new(),
            names: Map::new(),
            scanned: Arc::default(),
            pending: Vec::new(),
            midi: extras.midi.map(|m| Arc::new(Mutex::new(m))),
            midi_out: extras.midi_out.map(|m| Arc::new(Mutex::new(m))),
            trash: extras.trash.unwrap_or_else(|| Arc::new(os::trash)),
            track_presets: extras.track_presets,
            presets: extras.presets,
            watch: Default::default(),
        }
    }

    fn catalog(&mut self) -> Arc<Catalog> {
        self.adopt();
        let cache = &self.scan.cache;
        self.catalog
            .get_or_insert_with(|| Arc::new(Catalog::load(cache)))
            .clone()
    }

    /// The patterns used last for the song in `dir`.
    fn remembered(&self, dir: Option<&Path>) -> Map<String, Value> {
        match dir {
            Some(d) => Store::of_song(d).settings(),
            None => self.names.clone(),
        }
    }

    fn remember(&mut self, dir: Option<&Path>, key: &str, pattern: &str) {
        match dir {
            Some(d) => {
                let _ = Store::of_song(d).remember(key, json!(pattern));
            }
            None => {
                self.names.insert(key.to_string(), json!(pattern));
            }
        }
    }

    fn export_named(
        &mut self,
        cx: &ExtContext<'_>,
        name: &str,
        args: &Args,
        dry: bool,
        update: Option<String>,
        part: Option<Part>,
    ) -> CmdResult<Output> {
        let stems = name == "export.stems";
        let busy = self.busy();
        let remembered = self.remembered(cx.dir);
        let plan = commands::plan(cx.project, cx.dir, stems, args, &remembered, &busy)?;
        if dry {
            let (message, data) = commands::preview(cx.project, &plan)?;
            return Ok(Output::with(message, data));
        }
        // Refused before it starts, so a mistake leaves no record.
        let span = commands::check(cx.project, &plan)?;
        let mut base = crate::export_stale::base(
            cx.project,
            span,
            plan.stems.then_some(plan.strips.as_slice()),
        );
        let writes = commands::files_of(cx.project, &plan)?;
        // Only part of the stems again (EXP-020), when each file goes where it went before.
        let mut touched = Map::new();
        let part = match part {
            Some(part) if stems && Self::same_stems(cx.project, &plan, &part)? => {
                crate::export_stale::keep(&mut base, &part.record["base"], &part.kept());
                touched = crate::export_stale::keep_touched(&part.record, &part.kept());
                Some(part)
            }
            _ => None,
        };
        let rewritten: Vec<PathBuf> = match &part {
            Some(part) => {
                let paths =
                    export::stem_paths(cx.project, &plan.target, &plan.strips, &plan.options)
                        .map_err(|e| CmdError::conflict(e.to_string()))?;
                plan.strips
                    .iter()
                    .zip(paths)
                    .filter(|(id, _)| part.only.contains(id))
                    .filter_map(|(_, p)| p)
                    .collect()
            }
            None => writes.clone(),
        };
        // An export of the same names takes the place of the ones that wrote them (EXP-019):
        // their files go to the recycle bin, unless written over in place (--overwrite), and
        // their lines leave the list when this one is done.
        let replaces = self.replacing(cx, args, update.as_deref(), &writes, &rewritten)?;
        if update.is_none()
            && let Some(n) = args.opt_str("name")
        {
            self.remember(cx.dir, commands::name_key(stems), n);
        }
        if update.is_none() {
            Self::remember_range(cx, args);
        }
        let commands::Plan {
            target,
            strips,
            options: mut o,
            stored,
            ..
        } = plan;
        let progress = Arc::new(Progress::default());
        o.progress = Some(progress.clone());
        o.catalog = Some(self.catalog());
        let id = new_id("export");
        let disk = cx.dir.map(Store::of_song);
        let lock = disk.as_ref().and_then(|d| d.lock(&id));
        let job = Arc::new(Job {
            disk_gate: Mutex::new(()),
            id: id.clone(),
            command: name.to_string(),
            args: Value::Object(stored),
            path: target.display().to_string(),
            replaces,
            writes,
            base,
            touched: Mutex::new(touched),
            progress,
            started: Instant::now(),
            started_ms: now_ms(),
            ended: Mutex::new(None),
            state: Mutex::new(JobState::Running),
            closing: AtomicBool::new(false),
            disk: disk.clone(),
            lock: Mutex::new(lock),
        });
        job.save();
        if let Some(d) = &disk {
            d.prune();
            watch(job.clone());
        }
        self.jobs.push(job.clone());
        self.forget_old_jobs();
        let song = cx.project.clone();
        let strips = match &part {
            Some(part) => part.only.clone(),
            None => strips,
        };
        let finish = move |r: CmdResult<(String, Value)>| match &part {
            Some(part) => part.merge(r),
            None => r,
        };
        if !args.flag("background") {
            let r = finish(commands::render(&song, stems, &target, &strips, &o));
            job.finish(&r);
            let (message, mut data) = r?;
            if let Some(m) = data.as_object_mut() {
                m.insert("record".into(), json!(id));
            }
            return Ok(Output::with(message, data));
        }
        // A job: the song as it is now, rendered on a thread of its own.
        let (worker, path) = (job.clone(), target.clone());
        std::thread::Builder::new()
            .name(id.clone())
            .spawn(move || {
                let r = finish(commands::render(&song, stems, &path, &strips, &o));
                worker.finish(&r);
            })
            .map_err(|e| {
                let e = CmdError::conflict(format!("書き出しのスレッドを作成できません: {e}"));
                job.finish(&Err(e.clone()));
                e
            })?;
        Ok(Output::with(
            format!(
                "{} の書き出しを開始しました（{id}。進行状況は export.status --job {id}）",
                target.display()
            ),
            json!({
                "job": id,
                "state": "running",
                "path": target.display().to_string(),
                "status_command": "export.status",
            }),
        ))
    }

    /// The files the exports of this process that are running write.
    fn busy(&self) -> Vec<PathBuf> {
        self.jobs
            .iter()
            .filter(|j| j.is_export() && j.running())
            .flat_map(|j| {
                let mut v = j.writes.clone();
                v.push(PathBuf::from(&j.path));
                v
            })
            .collect()
    }

    /// The exports an export writing `writes` takes the place of (EXP-019): the one updated,
    /// and those whose files it all writes again. The files of `rewritten` there now go to the
    /// recycle bin, unless written over in place (--overwrite); with --numbered nothing is
    /// replaced.
    fn replacing(
        &self,
        cx: &ExtContext<'_>,
        args: &Args,
        update: Option<&str>,
        writes: &[PathBuf],
        rewritten: &[PathBuf],
    ) -> CmdResult<Vec<String>> {
        let mut replaces: Vec<String> = update.iter().map(|s| s.to_string()).collect();
        if !args.flag("numbered") {
            for v in self.records(cx) {
                let old = written_files(&v);
                let id = v["job"].as_str().unwrap_or_default().to_string();
                if v["state"] == "done"
                    && !old.is_empty()
                    && old.iter().all(|f| writes.iter().any(|w| same_file(f, w)))
                    && !replaces.contains(&id)
                {
                    replaces.push(id);
                }
            }
            if !args.flag("overwrite") {
                for f in rewritten.iter().filter(|f| f.is_file()) {
                    (self.trash)(f).map_err(CmdError::conflict)?;
                }
            }
        }
        Ok(replaces)
    }

    /// Keeps the range of an export in the song folder, for the panel to start from next time.
    fn remember_range(cx: &ExtContext<'_>, args: &Args) {
        if let Some(d) = cx.dir {
            let (range, sections) = commands::range_to_remember(args);
            let disk = Store::of_song(d);
            let _ = disk.remember(commands::RANGE_KEY, json!(range));
            let _ = disk.remember(commands::SECTIONS_KEY, json!(sections));
        }
    }

    /// `export.midi` (EXP-026): the notes of instrument tracks as MIDI files, written at once,
    /// with a record like the mix's and the stems'.
    fn export_midi(
        &mut self,
        cx: &ExtContext<'_>,
        args: &Args,
        dry: bool,
        update: Option<String>,
    ) -> CmdResult<Output> {
        let busy = self.busy();
        let remembered = self.remembered(cx.dir);
        let plan = crate::midi_export::plan(cx.project, cx.dir, args, &remembered, &busy)?;
        if dry {
            return Ok(crate::midi_export::preview(&plan));
        }
        let writes = plan.paths();
        let replaces = self.replacing(cx, args, update.as_deref(), &writes, &writes)?;
        if update.is_none() {
            if let Some(n) = args.opt_str("name") {
                self.remember(cx.dir, crate::midi_export::name_key(plan.each), n);
            }
            Self::remember_range(cx, args);
        }
        let id = new_id("export");
        let disk = cx.dir.map(Store::of_song);
        let lock = disk.as_ref().and_then(|d| d.lock(&id));
        let job = Arc::new(Job {
            disk_gate: Mutex::new(()),
            id: id.clone(),
            command: "export.midi".to_string(),
            args: Value::Object(plan.stored.clone()),
            path: plan.target.display().to_string(),
            replaces,
            writes,
            base: crate::midi_export::base(cx.project, plan.span, &plan.keyed()),
            touched: Mutex::new(Map::new()),
            progress: Arc::new(Progress::default()),
            started: Instant::now(),
            started_ms: now_ms(),
            ended: Mutex::new(None),
            state: Mutex::new(JobState::Running),
            closing: AtomicBool::new(false),
            disk: disk.clone(),
            lock: Mutex::new(lock),
        });
        job.save();
        if let Some(d) = &disk {
            d.prune();
        }
        self.jobs.push(job.clone());
        self.forget_old_jobs();
        let r = crate::midi_export::write(cx.project, &plan);
        job.finish(&r);
        let (message, mut data) = r?;
        if let Some(m) = data.as_object_mut() {
            m.insert("record".into(), json!(id));
        }
        Ok(Output::with(message, data))
    }

    /// Whether the stems `plan` writes are the ones `part`'s record wrote, each to the same file.
    fn same_stems(p: &Project, plan: &commands::Plan, part: &Part) -> CmdResult<bool> {
        let paths = export::stem_paths(p, &plan.target, &plan.strips, &plan.options)
            .map_err(|e| CmdError::conflict(e.to_string()))?;
        let now: Vec<(String, PathBuf)> = plan
            .strips
            .iter()
            .zip(paths)
            .filter_map(|(id, path)| Some((id.to_string(), path?)))
            .collect();
        let then: Vec<(String, PathBuf)> = part
            .files()
            .into_iter()
            .filter_map(|(k, f)| Some((k, PathBuf::from(f["path"].as_str()?))))
            .collect();
        Ok(now.len() == then.len()
            && now
                .iter()
                .zip(&then)
                .all(|((a, pa), (b, pb))| a == b && same_file(pa, pb)))
    }

    /// The strips of a stems record to write again: the file named, else the stale ones;
    /// `None` to write every file again (a mix, a record without fingerprints, a change of
    /// tempo, metre, settings or range, or every file stale).
    fn part_of(
        &self,
        cx: &ExtContext<'_>,
        v: &Value,
        file: Option<&str>,
    ) -> CmdResult<Option<Part>> {
        let picked = match file {
            Some(f) => Some(picked_files(v, Some(f))?),
            None => None,
        };
        if v["command"] != "export.stems" {
            return Ok(None);
        }
        let pr = crate::export_stale::prints(cx.project);
        let Some(st) = crate::export_stale::check(cx.project, &pr, v) else {
            return Ok(None);
        };
        if st.song_changed || st.range_changed {
            return Ok(None);
        }
        let only: Vec<TrackId> = st
            .files
            .iter()
            .filter(|f| match &picked {
                Some(p) => p.iter().any(|p| same_file(p, Path::new(&f.path))),
                None => f.stale,
            })
            .filter_map(|f| f.strip.as_deref()?.parse().ok())
            .collect();
        if only.is_empty() || only.len() == st.files.len() {
            return Ok(None);
        }
        Ok(Some(Part {
            record: v.clone(),
            only,
        }))
    }

    fn forget_old_jobs(&mut self) {
        // An export written again by a finished `export.update` leaves the list.
        let replaced: Vec<String> = self
            .jobs
            .iter()
            .filter(|j| matches!(*j.state.lock().unwrap(), JobState::Done(_)))
            .flat_map(|j| j.replaces.clone())
            .collect();
        self.jobs.retain(|j| !replaced.contains(&j.id));
        let finished = self.jobs.iter().filter(|j| !j.running()).count();
        let mut extra = finished.saturating_sub(KEEP_FINISHED);
        self.jobs.retain(|j| {
            if extra > 0 && !j.running() {
                extra -= 1;
                false
            } else {
                true
            }
        });
    }

    fn job(&self, id: &str) -> Option<&Arc<Job>> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// A job's record: this process's own, else the one in the song folder.
    fn record(&self, cx: &ExtContext<'_>, id: &str) -> CmdResult<Value> {
        if let Some(j) = self.job(id) {
            return Ok(j.to_json());
        }
        if crate::host::on_window_thread() {
            return self
                .disk_cache
                .read(cx.project, cx.dir)
                .records
                .into_iter()
                .find(|v| v["job"] == id)
                .ok_or_else(|| CmdError::conflict("書き出しの記録を読んでいます"));
        }
        cx.dir
            .and_then(|d| Store::of_song(d).read(id))
            .ok_or_else(|| CmdError::invalid(format!("ジョブ {id} が見つかりません")))
    }

    /// An export's record, which must have ended.
    fn finished_export(&self, cx: &ExtContext<'_>, id: &str) -> CmdResult<Value> {
        let v = self.record(cx, id)?;
        if !v["command"]
            .as_str()
            .is_some_and(|c| c.starts_with("export."))
        {
            return Err(CmdError::invalid(format!(
                "{id} は書き出しジョブではありません"
            )));
        }
        if v["state"] == "running" {
            return Err(CmdError::conflict(format!(
                "{id} はまだ書き出し中です（export.cancel でキャンセルするか、完了までお待ちください）"
            )));
        }
        Ok(v)
    }

    /// Every job: this process's and the song folder's, oldest first.
    fn records(&self, cx: &ExtContext<'_>) -> Vec<Value> {
        let mut all: Vec<Value> = self.jobs.iter().map(|j| j.to_json()).collect();
        if let Some(d) = cx.dir {
            let records = if crate::host::on_window_thread() {
                self.disk_cache.read(cx.project, cx.dir).records
            } else {
                Store::of_song(d).list()
            };
            for v in records {
                if !self.jobs.iter().any(|j| v["job"] == j.id.as_str()) {
                    all.push(v);
                }
            }
        }
        let replaced: Vec<Value> = all
            .iter()
            .filter(|v| v["state"] == "done")
            .flat_map(replaced_by)
            .collect();
        all.retain(|v| !replaced.contains(&v["job"]));
        all.sort_by_key(|v| v["started_ms"].as_u64().unwrap_or(0));
        all
    }

    fn status(&mut self, cx: &ExtContext<'_>, args: &Args) -> CmdResult<Output> {
        self.forget_old_jobs();
        if let Some(id) = args.opt_str("job") {
            let mut v = self.record(cx, id)?;
            crate::export_stale::annotate(cx.project, std::slice::from_mut(&mut v));
            return Ok(Output::with(status_message(&v), v));
        }
        let mut all = self.records(cx);
        crate::export_stale::annotate(cx.project, &mut all);
        self.watch.look(&all);
        let count = |s: &str| all.iter().filter(|v| v["state"] == s).count();
        let (running, interrupted) = (count("running"), count("interrupted"));
        let mut msg = format!("ジョブ {} 件（うち実行中 {running} 件", all.len());
        if interrupted > 0 {
            msg.push_str(&format!("・中断 {interrupted} 件"));
        }
        msg.push('）');
        Ok(Output::with(msg, json!({ "jobs": all })))
    }

    fn cancel(&self, cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Output> {
        let id = args.str("job")?;
        let Some(j) = self.job(id) else {
            // Another process's export: it looks for the cancel file while it renders.
            let v = self.record(cx, id)?;
            if v["state"] != "running" {
                return Ok(Output::with(format!("{id} はすでに終了しています"), v));
            }
            if dry {
                return Ok(Output::msg(format!("{id} をキャンセルします")));
            }
            let d = Store::of_song(cx.dir.expect("a record on disk has a song folder"));
            d.ask_cancel(id)?;
            return Ok(Output::with(
                format!(
                    "{id} にキャンセルを要求しました（別のプロセスの書き出し。まもなく停止します）"
                ),
                json!({ "job": id }),
            ));
        };
        if !j.running() {
            return Ok(Output::with(
                format!("{id} はすでに終了しています"),
                j.to_json(),
            ));
        }
        if dry {
            return Ok(Output::msg(format!("{id} をキャンセルします")));
        }
        j.progress.cancel();
        let how = if j.command == "plugin.rescan" {
            "読み込み中のプラグインのプロセスを終了し、読み込み済みの分を一覧に残します"
        } else {
            "次のブロックで停止します"
        };
        Ok(Output::with(
            format!("{id} にキャンセルを要求しました（{how}）"),
            json!({ "job": id }),
        ))
    }

    /// `export.update`: the same export again, of the song as it is now, over the same file.
    fn update(&mut self, cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Output> {
        let id = args.str("job")?;
        let v = self.finished_export(cx, id)?;
        let Value::Object(mut stored) = v["args"].clone() else {
            return Err(CmdError::conflict(format!(
                "{id} の記録に設定がありません（書き出し直せません）"
            )));
        };
        // The same files again: the ones there now go to the recycle bin.
        stored.remove("overwrite");
        stored.remove("numbered");
        if args.flag("background") {
            stored.insert("background".into(), json!(true));
        }
        let command = v["command"]
            .as_str()
            .unwrap_or("export.mixdown")
            .to_string();
        if command == "export.midi" {
            stored.remove("background");
            return self.export_midi(cx, &Args(stored), dry, Some(id.to_string()));
        }
        let part = self.part_of(cx, &v, args.opt_str("file"))?;
        self.export_named(cx, &command, &Args(stored), dry, Some(id.to_string()), part)
    }

    /// `export.open`: the settings of an export, or the ones to start from.
    fn open(&self, cx: &ExtContext<'_>, args: &Args) -> CmdResult<Output> {
        let Some(id) = args.opt_str("job") else {
            if crate::host::on_window_thread() {
                let view = self.disk_cache.read(cx.project, cx.dir);
                return Ok(Output::with(
                    "書き出しの設定",
                    if view.defaults.is_null() {
                        json!({"_loading":true})
                    } else {
                        view.defaults
                    },
                ));
            }
            let v = commands::default_settings(cx.project, cx.dir, &self.remembered(cx.dir));
            return Ok(Output::with("書き出しの既定の設定", v));
        };
        let v = self.record(cx, id)?;
        if !v["args"].is_object() {
            return Err(CmdError::invalid(format!(
                "{id} は書き出しジョブではありません"
            )));
        }
        Ok(Output::with(
            format!(
                "{id} の設定（{} に同じ引数を渡すと、同じ内容を書き出せます）",
                v["command"].as_str().unwrap_or("")
            ),
            json!({ "job": id, "command": v["command"], "args": v["args"], "path": v["path"] }),
        ))
    }

    /// `export.reveal_os`: the written file in the OS's file window.
    fn reveal(&self, cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Output> {
        let id = args.str("job")?;
        let v = self.finished_export(cx, id)?;
        let path = picked_files(&v, args.opt_str("file"))?
            .into_iter()
            .find(|p| p.exists())
            .or_else(|| v["path"].as_str().map(PathBuf::from).filter(|p| p.exists()))
            .ok_or_else(|| CmdError::conflict(format!("{id} のファイルはすでにありません")))?;
        if !dry {
            os::reveal(&path).map_err(CmdError::conflict)?;
        }
        Ok(Output::with(
            format!("{} を表示します", path.display()),
            json!({ "path": path.display().to_string() }),
        ))
    }

    /// `export.touch` (EXP-023): the files written before that the tracks named play into are
    /// stale from now on, until they are written again.
    fn touch(&mut self, cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Output> {
        let ids = args.ids::<TrackId>("tracks")?;
        if ids.is_empty() {
            return Err(CmdError::invalid("--tracks を指定してください"));
        }
        for id in &ids {
            cx.project.track(*id)?;
        }
        let touched: Vec<(Value, Map<String, Value>)> = self
            .records(cx)
            .into_iter()
            .filter_map(|v| {
                let t = crate::export_stale::touch(cx.project, &v, &ids)?;
                Some((v, t))
            })
            .collect();
        let jobs: Vec<Value> = touched.iter().map(|(v, _)| v["job"].clone()).collect();
        let message = |done: &str| match jobs.len() {
            0 => "古くなる書き出しはありません".to_string(),
            n => format!("書き出し {n} 件のファイルを古いものとして{done}"),
        };
        if dry {
            return Ok(Output::with(message("扱います"), json!({ "jobs": jobs })));
        }
        for (mut v, t) in touched {
            let id = v["job"].as_str().unwrap_or_default().to_string();
            if let Some(j) = self.job(&id) {
                *j.touched.lock().unwrap() = t;
                if let Some(d) = &j.disk {
                    d.write(&j.to_json())?;
                }
            } else if let Some(d) = cx.dir {
                v["touched"] = Value::Object(t);
                Store::of_song(d).write(&v)?;
            }
        }
        self.look(cx);
        Ok(Output::with(message("扱いました"), json!({ "jobs": jobs })))
    }

    /// `export.watch` (EXP-023): from now on, or no more, an `export.stale` event each time
    /// the files of an export turn stale or current again.
    fn watch_exports(&mut self, cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Output> {
        let on = !args.flag("stop");
        let mut all = self.records(cx);
        crate::export_stale::annotate(cx.project, &mut all);
        let exports: Vec<Value> = all
            .iter()
            .filter_map(crate::export_watch::summary)
            .collect();
        let message = match (on, dry) {
            (true, true) => "書き出しが古くなったら知らせます",
            (true, false) => "書き出しが古くなったら知らせます（export.stale）",
            (false, _) => "書き出しの古さの通知を止めました",
        };
        if !dry {
            self.watch.set(on, &all);
        }
        Ok(Output::with(
            message,
            json!({ "watching": on, "exports": exports }),
        ))
    }

    /// Looks how the exports stand, while `export.watch` is on.
    fn look(&mut self, cx: &ExtContext<'_>) {
        if !self.watch.on() {
            return;
        }
        let mut all = self.records(cx);
        crate::export_stale::annotate(cx.project, &mut all);
        self.watch.look(&all);
    }

    /// `export.delete`: the written files to the recycle bin, and the record away.
    fn delete(&mut self, cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Output> {
        let id = args.str("job")?;
        let v = self.finished_export(cx, id)?;
        let files: Vec<PathBuf> = written_files(&v)
            .into_iter()
            .filter(|p| p.exists())
            .collect();
        let shown: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
        if dry {
            return Ok(Output::with(
                format!(
                    "{id} の {} 個のファイルをごみ箱に移動し、一覧から削除します",
                    files.len()
                ),
                json!({ "job": id, "files": shown }),
            ));
        }
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.job == id)
        {
            // UI dispatches one command to its worker. Close and acknowledge
            // the preview output there before trashing its open files.
            self.preview = None;
        }
        for f in &files {
            (self.trash)(f).map_err(CmdError::conflict)?;
        }
        if let Some(d) = cx.dir {
            Store::of_song(d).remove(id);
        }
        self.jobs.retain(|j| j.id != id);
        self.disk_cache.removed(id);
        Ok(Output::with(
            format!(
                "{id} の {} 個のファイルをごみ箱に移動し、一覧から削除しました",
                files.len()
            ),
            json!({ "job": id, "files": shown }),
        ))
    }

    fn preview_export(&mut self, cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Output> {
        let job = args.str("job")?;
        if dry {
            return Ok(Output::with("試聴します", json!({"job":job})));
        }
        if args.flag("stop") {
            self.preview = None;
            return Ok(Output::with("試聴を停止しました", json!({"playing":false})));
        }
        if args.flag("status") {
            let data = self
                .preview
                .as_mut()
                .map(|p| p.poll())
                .unwrap_or(json!({"playing":false}));
            if data["playing"] == false {
                self.preview = None;
            }
            return Ok(Output::with("試聴", data));
        }
        let record = self.successful_export(cx, job)?;
        if record["command"] == "export.midi" {
            return Err(CmdError::invalid("MIDI ファイルは試聴できません"));
        }
        let files = picked_files(&record, args.opt_str("file"))?;
        let output = match args.opt_str("output") {
            Some("silent") => OutputMode::Silent { paced: true },
            Some("device") => OutputMode::Device,
            Some("none") => OutputMode::None,
            None => self.output,
            _ => return Err(CmdError::invalid("出力は device または silent です")),
        };
        self.preview = None;
        self.preview = Some(
            crate::export_preview::Preview::start(job, &files, output, self.io_settings.clone())
                .map_err(CmdError::conflict)?,
        );
        let shown: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
        Ok(Output::with(
            "試聴を開始しました",
            json!({"job":job,"playing":true,"files":shown}),
        ))
    }

    fn successful_export(&self, cx: &ExtContext<'_>, id: &str) -> CmdResult<Value> {
        let record = self.finished_export(cx, id)?;
        if record["state"] != "done" || record["result"].is_null() {
            return Err(CmdError::conflict("書き出しが完了していません"));
        }
        Ok(record)
    }

    fn import_export(&mut self, cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Output> {
        let record = self.successful_export(cx, args.str("job")?)?;
        let files = picked_files(&record, args.opt_str("file"))?;
        if files.is_empty() {
            return Err(CmdError::conflict("書き出したファイルがありません"));
        }
        let at = match args.0.get("at") {
            Some(at) if !at.is_null() => at.clone(),
            _ => json!(crate::export_stale::head(cx.project, &record)),
        };
        if record["command"] == "export.midi" {
            return self.import_midi(cx, &files, at, dry);
        }
        let mut plans = Vec::new();
        for file in &files {
            let data = json!({"file": file.display().to_string(), "tempo":"keep", "at": at});
            plans.push(import::run(
                cx,
                &Args(data.as_object().unwrap().clone()),
                dry,
            )?);
        }
        let placements: Vec<_> = plans
            .into_iter()
            .flat_map(|p| p.edits)
            .map(|(_, args)| args)
            .collect();
        if !dry {
            self.pending.push((
                "clip.import.batch".into(),
                json!({"files":placements}).as_object().unwrap().clone(),
            ));
        }
        Ok(Output::with(
            "書き出した音を取り込みました",
            json!({"files":files}),
        ))
    }

    /// The MIDI files of an export as new tracks, in one step (EXP-026): the parts of every
    /// file are placed together, keeping the song's tempo.
    fn import_midi(
        &mut self,
        cx: &ExtContext<'_>,
        files: &[PathBuf],
        at: Value,
        dry: bool,
    ) -> CmdResult<Output> {
        let mut place: Option<Map<String, Value>> = None;
        for file in files {
            let data = json!({"file": file.display().to_string(), "tempo": "keep", "at": at});
            let plan = crate::midi_import::run(cx, &Args(data.as_object().unwrap().clone()), dry)?;
            for (_, args) in plan.edits {
                match &mut place {
                    None => place = Some(args),
                    Some(all) => {
                        let parts = args["parts"].as_array().cloned().unwrap_or_default();
                        if let Some(Value::Array(v)) = all.get_mut("parts") {
                            v.extend(parts);
                        }
                    }
                }
            }
        }
        let tracks = place
            .as_ref()
            .and_then(|p| p["parts"].as_array())
            .map_or(0, Vec::len);
        if let Some(place) = place
            && !dry
        {
            self.pending.push(("midi.import.place".into(), place));
        }
        Ok(Output::with(
            format!("書き出した MIDI を取り込みました（{tracks} トラック）"),
            json!({"files": files, "tracks": tracks}),
        ))
    }

    /// The player, started now if it is not running.
    fn player(&mut self, cx: &ExtContext<'_>) -> CmdResult<&mut Player> {
        if self.output == OutputMode::None {
            return Err(CmdError::conflict(
                "オーディオ出力が開始されていません（再生は `xirius play` か `xirius mcp` の中で使用してください）",
            ));
        }
        if self
            .player
            .as_ref()
            .is_some_and(|p| p.dir.as_deref() != cx.dir)
        {
            self.player = None;
        }
        if self.player.is_none() {
            let dir = cx.dir.map(Path::to_path_buf);
            let config = HostConfig {
                dir: dir.clone(),
                catalog: Some(self.catalog()),
                ..HostConfig::default()
            };
            let (mut host, audio, _warnings) =
                SessionHost::start(cx.project, config).map_err(|e| {
                    CmdError::conflict(format!("オーディオエンジンを起動できませんでした: {e:?}"))
                })?;
            let midi = self
                .midi
                .clone()
                .map(|m| Box::new(move || (m.lock().unwrap())()) as Box<dyn FnMut() -> _ + Send>);
            host.set_record_sources(crate::record::Sources {
                audio: match self.output {
                    OutputMode::Device => crate::record::Source::Device,
                    _ => crate::record::Source::None,
                },
                midi,
            });
            host.io_settings = self.io_settings.clone();
            if let Some(m) = self.midi_out.clone() {
                host.set_midi_out(Some(Box::new(move |ports, rate| {
                    (m.lock().unwrap())(ports, rate)
                })));
            }
            crate::io_store::apply(&mut host).map_err(CmdError::conflict)?;
            let prefs = self
                .io_settings
                .as_ref()
                .map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).prefs.clone());
            let sink = match prefs {
                Some(p) => Sink::configured(self.output, audio, &p),
                None => Sink::start(self.output, audio),
            }
            .map_err(|e| CmdError::conflict(format!("オーディオ出力を開始できませんでした: {e}")))?
            .expect("an output mode other than None");
            self.player = Some(Player { host, sink, dir });
        }
        Ok(self.player.as_mut().expect("started above"))
    }

    fn transport(
        &mut self,
        cx: &ExtContext<'_>,
        name: &str,
        args: &Args,
        dry: bool,
    ) -> CmdResult<Output> {
        if dry {
            return Ok(Output::msg(format!(
                "{name} をドライランで実行しました（変更はありません）"
            )));
        }
        let p: &Project = cx.project;
        let player = self.player(cx)?;
        let output = player.sink.describe();
        let (message, mut data) = commands::transport_on(&mut player.host, p, name, args)?;
        data["output"] = json!(output);
        Ok(Output::with(message, data))
    }

    fn recording(
        &mut self,
        cx: &ExtContext<'_>,
        name: &str,
        args: &Args,
        dry: bool,
    ) -> CmdResult<Output> {
        if dry {
            return Ok(Output::msg(format!(
                "{name} をドライランで実行しました（変更はありません）"
            )));
        }
        if name != "record.start" && self.player.is_none() {
            return Err(CmdError::conflict("録音していません"));
        }
        let player = self.player(cx)?;
        if name != "record.stop" {
            let (message, data) =
                crate::record::record_on(&mut player.host, cx.project, cx.dir, name, args)?;
            return Ok(Output::with(message, data));
        }
        let f = player.host.record_takes(crate::record::STOP_WAIT)?;
        let mut message = format!("録音を停止しました（{}）", f.summary());
        if f.takes.is_empty() {
            message.push_str("。クリップにできるテイクはありません");
        } else {
            self.pending.push(("record.take.add".into(), f.args()));
        }
        for w in &f.warnings {
            message.push_str(&format!("\n{w}"));
        }
        Ok(Output::with(message, f.to_json()))
    }

    /// `project.cleanup`, unless a recording or an AI's work copy may still need the files.
    fn cleanup(&self, cx: &ExtContext<'_>, dry: bool) -> CmdResult<Output> {
        let Some(dir) = cx.dir else {
            return Ok(Output::with(
                "曲が保存されていないため、録音のファイルはありません",
                json!({ "files": [], "bytes": 0, "kept": 0, "moved": [] }),
            ));
        };
        if self.player.as_ref().is_some_and(|p| p.host.is_recording()) {
            return Err(CmdError::conflict(
                "録音中は片付けできません（record.stop で止めてから実行してください）",
            ));
        }
        if cx.work_copy {
            return Err(CmdError::conflict(cleanup::WORK_COPY));
        }
        // A recording another process has open, or a state of its history not yet on the
        // disk, may name a file no state here does (PRJ-023).
        if !dry && xirius_core::session::others_open(dir, cx.lane) {
            return Err(CmdError::conflict(cleanup::OTHERS_OPEN));
        }
        let own = cleanup::Own {
            uses: Some(cx.uses),
            lane: cx.lane,
        };
        cleanup::run(cx.project, dir, dry, &self.trash, own)
    }

    fn plugins(&mut self, name: &str, args: &Args, dry: bool) -> CmdResult<Output> {
        let Some(player) = &mut self.player else {
            return Ok(Output::with(
                "再生していないため、別のプロセスで実行中のプラグインはありません",
                json!({ "plugins": [] }),
            ));
        };
        if dry {
            return Ok(Output::msg(format!(
                "{name} をドライランで実行しました（変更はありません）"
            )));
        }
        let (message, data) = commands::plugins_on(&mut player.host, name, args)?;
        Ok(Output::with(message, data))
    }

    fn stats(
        &mut self,
        cx: &ExtContext<'_>,
        name: &str,
        args: &Args,
        dry: bool,
    ) -> CmdResult<Output> {
        if dry {
            return Ok(Output::msg(format!(
                "{name} をドライランで実行しました（変更はありません）"
            )));
        }
        let host = self.player.as_mut().map(|p| &mut p.host);
        let (message, data) = commands::stats_on(host, cx.project, name, args)?;
        Ok(Output::with(message, data))
    }

    fn rescan(&mut self, args: &Args, dry: bool) -> CmdResult<Output> {
        let mut opts = self.scan.clone();
        // The folders added in the settings (plugins.vst3.paths, plugins.clap.paths) are
        // looked in too; each folder is searched for both kinds.
        if let Some(settings) = &self.io_settings {
            let prefs = settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .prefs
                .clone();
            for key in ["plugins.vst3.paths", "plugins.clap.paths"] {
                for dir in prefs.list(key).into_iter().map(PathBuf::from) {
                    if !opts.dirs.contains(&dir) {
                        opts.dirs.push(dir);
                    }
                }
            }
        }
        opts.full = args.flag("full");
        if let Some(path) = args.opt_str("path") {
            opts.retry.push(PathBuf::from(path));
        }
        if dry {
            return Ok(Output::msg(format!(
                "{} 個のフォルダーのプラグインを{}スキャンします",
                opts.dirs.len(),
                if opts.full {
                    "すべて"
                } else if !opts.retry.is_empty() {
                    "指定した 1 つを含めて"
                } else {
                    "新規と変更されたもののみ"
                }
            )));
        }
        if !args.flag("background") {
            let r = scan(&opts).map_err(CmdError::conflict)?;
            let out = scan_output(&r, &opts);
            self.use_catalog(Arc::new(r.catalog));
            return Ok(out);
        }
        if let Some(j) = self
            .jobs
            .iter()
            .find(|j| j.command == "plugin.rescan" && j.running())
        {
            return Ok(Output::with(
                format!(
                    "すでにスキャン中です（{}。進行状況は export.status --job {}）",
                    j.id, j.id
                ),
                json!({ "job": j.id, "state": "running", "status_command": "export.status" }),
            ));
        }
        // A job: each bundle is still opened in a process of its own, so one that crashes
        // is a failure in the list and the scan goes on.
        let id = new_id("scan");
        let job = Arc::new(Job {
            disk_gate: Mutex::new(()),
            id: id.clone(),
            command: "plugin.rescan".to_string(),
            args: Value::Null,
            path: String::new(),
            replaces: Vec::new(),
            writes: Vec::new(),
            base: Value::Null,
            touched: Mutex::default(),
            progress: Arc::new(Progress::default()),
            started: Instant::now(),
            started_ms: now_ms(),
            ended: Mutex::new(None),
            state: Mutex::new(JobState::Running),
            closing: AtomicBool::new(false),
            disk: None,
            lock: Mutex::new(None),
        });
        let (worker, slot) = (job.clone(), self.scanned.clone());
        std::thread::Builder::new()
            .name(id.clone())
            .spawn(move || {
                let pr = worker.progress.clone();
                let r = scan_seeing(
                    &opts,
                    &|done, total| pr.set(done as u64, total as u64),
                    &|b| pr.see(b.display().to_string()),
                    &|| pr.cancelled(),
                );
                let r = match r {
                    // What it opened before the stop is in the list all the same.
                    Ok(r) if r.stopped => {
                        *slot.lock().unwrap() = Some(Arc::new(r.catalog));
                        Err(CmdError::conflict(SCAN_STOPPED))
                    }
                    Ok(r) => {
                        let out = scan_output(&r, &opts);
                        // Before the state says done, so whoever sees it done gets the new list.
                        *slot.lock().unwrap() = Some(Arc::new(r.catalog));
                        Ok((out.message, out.data))
                    }
                    Err(e) => Err(CmdError::conflict(e)),
                };
                worker.finish(&r);
            })
            .map_err(|e| CmdError::conflict(format!("スキャンのスレッドを作成できません: {e}")))?;
        self.jobs.push(job);
        self.forget_old_jobs();
        Ok(Output::with(
            format!(
                "プラグインのスキャンを開始しました（{id}。進行状況は export.status --job {id}）"
            ),
            json!({
                "job": id,
                "state": "running",
                "cache": self.scan.cache,
                "status_command": "export.status",
            }),
        ))
    }

    /// `plugin.scan.status` (PLG-059): the scan that runs, or the last of this process, and the
    /// plugins and failures of the list as it stands.
    fn scan_status(&mut self) -> Output {
        let catalog = self.catalog();
        let job = self
            .jobs
            .iter()
            .rev()
            .find(|j| j.command == "plugin.rescan")
            .map(|j| j.to_json());
        let state = job
            .as_ref()
            .and_then(|j| j["state"].as_str())
            .unwrap_or("idle")
            .to_string();
        let (done, total) = job.as_ref().map_or((0, 0), |j| {
            (
                j["done"].as_u64().unwrap_or(0),
                j["total"].as_u64().unwrap_or(0),
            )
        });
        // The bundle it opens now; none once it ended.
        let file = job
            .as_ref()
            .filter(|_| state == "running")
            .and_then(|j| j["file"].as_str())
            .filter(|f| !f.is_empty())
            .map(str::to_string);
        let found = catalog.entries.len();
        let failed = catalog.failures.len();
        let message = if state == "running" {
            if total == 0 {
                "スキャン中（プラグインを検索しています）".to_string()
            } else {
                format!("スキャン中 {} / {total}", done.min(total))
            }
        } else if failed > 0 {
            format!("プラグイン {found}（読み込めない {failed}）")
        } else {
            format!("プラグイン {found}")
        };
        Output::with(
            message,
            json!({
                "state": state,
                "job": job.as_ref().map(|j| j["job"].clone()),
                "done": done,
                "total": total,
                "file": file,
                "plugins": found,
                "failed": failed,
                "failures": catalog
                    .failures
                    .iter()
                    .map(|f| json!({ "path": f.path, "kind": f.kind, "error": f.error }))
                    .collect::<Vec<_>>(),
                "cache": self.scan.cache,
            }),
        )
    }

    /// Takes up what a background scan finished with.
    fn adopt(&mut self) {
        let fresh = self.scanned.lock().unwrap().take();
        if let Some(c) = fresh {
            self.use_catalog(c);
        }
    }

    /// The list a scan made, here and in the player.
    fn use_catalog(&mut self, catalog: Arc<Catalog>) {
        if let Some(pl) = &mut self.player {
            pl.host.set_catalog(Some(catalog.clone()));
        }
        self.catalog = Some(catalog);
    }

    fn browse(&mut self, args: &Args) -> CmdResult<Output> {
        let catalog = self.catalog();
        let kind = match args.opt_str("kind") {
            Some("instrument") => Some(PluginKind::Instrument),
            Some("effect") => Some(PluginKind::Effect),
            _ => None,
        };
        let query = args.opt_str("query").unwrap_or("");
        let found = catalog.search(query, kind);
        let limit = args.opt_i64("limit").unwrap_or(100).max(0) as usize;
        let entries: Vec<Value> = found
            .iter()
            .take(limit)
            .map(|e| {
                let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                v["add"] = json!({
                    "name": e.name,
                    "kind": e.kind,
                    "format": e.format,
                    "uid": e.uid,
                });
                v
            })
            .collect();
        let mut data = json!({
            "count": found.len(),
            "entries": entries,
            "cache": self.scan.cache,
        });
        if args.flag("failed") {
            data["failures"] = json!(
                catalog
                    .failures
                    .iter()
                    .map(|f| json!({ "path": f.path, "kind": f.kind, "error": f.error }))
                    .collect::<Vec<_>>()
            );
        }
        let msg = if catalog.entries.is_empty() && catalog.failures.is_empty() {
            "プラグインの一覧がありません（未スキャンです。plugin.rescan でスキャンしてください）"
                .to_string()
        } else {
            let mut lines = vec![format!(
                "{} 件（全 {} 件）",
                found.len(),
                catalog.entries.len()
            )];
            lines.extend(found.iter().take(limit).map(|e| {
                let k = match e.kind {
                    PluginKind::Instrument => "インストゥルメント",
                    PluginKind::Effect => "エフェクト",
                };
                format!("{} / {}（{k}、{} {}）", e.name, e.vendor, e.format, e.uid)
            }));
            lines.join("\n")
        };
        Ok(Output::with(msg, data))
    }
}

impl Extension for SessionExt {
    fn answers(&self, name: &str) -> bool {
        NAMES.contains(&name)
            || RECORD.contains(&name)
            || name == CLEANUP
            || import::NAMES.contains(&name)
            || crate::midi_import::NAMES.contains(&name)
            || crate::clip_audio::NAMES.contains(&name)
            || crate::bounce::NAMES.contains(&name)
            || places::NAMES.contains(&name)
            || reveal::NAMES.contains(&name)
            || crate::preset::NAMES.contains(&name)
            || crate::track_preset::NAMES.contains(&name)
            || crate::xpreset::NAMES.contains(&name)
            || crate::export_watch::NAMES.contains(&name)
            || crate::midi_export::NAMES.contains(&name)
            || crate::dawproject::NAMES.contains(&name)
            || crate::file_ops::NAMES.contains(&name)
            || crate::listen::NAMES.contains(&name)
            || crate::pool::NAMES.contains(&name)
    }

    fn run(
        &mut self,
        cx: &ExtContext<'_>,
        def: &CommandDef,
        args: &Args,
        dry: bool,
    ) -> CmdResult<Output> {
        self.adopt();
        match def.name {
            n if crate::preset::NAMES.contains(&n) => {
                let (output, edits) = crate::preset::run(cx, args, n, dry)?;
                self.pending.extend(edits);
                Ok(output)
            }
            n if crate::track_preset::NAMES.contains(&n) => {
                let dir = self
                    .track_presets
                    .clone()
                    .unwrap_or_else(crate::track_preset::dir);
                if n == "track.preset.list" && crate::host::on_window_thread() {
                    return Ok(Output::with(
                        "",
                        self.preset_cache.read(dir, cx.project.clone()),
                    ));
                }
                let catalog = self.catalog();
                let (output, edits) =
                    crate::track_preset::run(&dir, cx.project, &catalog, n, args, dry)?;
                self.pending.extend(edits);
                Ok(output)
            }
            n if crate::xpreset::NAMES.contains(&n) => {
                let catalog = self.catalog();
                let dir = self.presets.clone().unwrap_or_else(crate::xpreset::dir);
                let (output, edits) =
                    crate::xpreset::run(&dir, cx.dir, cx.project, &catalog, n, args, dry)?;
                self.pending.extend(edits);
                Ok(output)
            }
            "export.selection" => {
                let ranged = crate::export_selection::ranged(cx.project, args)?;
                self.export_named(cx, "export.mixdown", &ranged, dry, None, None)
            }
            "export.mixdown" | "export.stems" => {
                self.export_named(cx, def.name, args, dry, None, None)
            }
            "export.status" => self.status(cx, args),
            "export.cancel" => self.cancel(cx, args, dry),
            "export.update" => self.update(cx, args, dry),
            "export.open" => self.open(cx, args),
            "export.preview" => self.preview_export(cx, args, dry),
            "export.import" => self.import_export(cx, args, dry),
            "export.reveal_os" => self.reveal(cx, args, dry),
            "export.delete" => self.delete(cx, args, dry),
            "export.touch" => self.touch(cx, args, dry),
            "export.watch" => self.watch_exports(cx, args, dry),
            "export.midi" => self.export_midi(cx, args, dry, None),
            "export.dawproject" => crate::dawproject::run(cx, args, dry, &self.trash),
            "plugin.rescan" => self.rescan(args, dry),
            "plugin.scan.status" => Ok(self.scan_status()),
            "browser.plugin.list" => self.browse(args),
            "plugin.status" | "plugin.restart" | "plugin.reload" => {
                self.plugins(def.name, args, dry)
            }
            "plugin.stats" | "engine.stats" => self.stats(cx, def.name, args, dry),
            "audio.test-tone" => {
                if dry {
                    return Ok(Output::msg(
                        "audio.test-tone をドライランで実行しました（変更はありません）",
                    ));
                }
                let player = self.player(cx)?;
                let (message, data) = commands::tone_on(&mut player.host, args)?;
                Ok(Output::with(message, data))
            }
            "plugin.collect" => Err(CmdError::conflict(
                "プラグインの編集の反映は `xirius play` の中で使用してください",
            )),
            n if RECORD.contains(&n) => self.recording(cx, n, args, dry),
            CLEANUP => self.cleanup(cx, dry),
            n if import::NAMES.contains(&n) => {
                let plan = import::run(cx, args, dry)?;
                self.pending.extend(plan.edits);
                Ok(plan.output)
            }
            n if crate::midi_import::NAMES.contains(&n) => {
                let plan = crate::midi_import::run(cx, args, dry)?;
                self.pending.extend(plan.edits);
                Ok(plan.output)
            }
            n if crate::clip_audio::NAMES.contains(&n) => {
                let plan = crate::clip_audio::run(cx, n, args, dry)?;
                self.pending.extend(plan.edits);
                Ok(plan.output)
            }
            n if crate::bounce::NAMES.contains(&n) => {
                let catalog = Some(self.catalog());
                let plan = crate::bounce::run(cx, catalog, n, args, dry)?;
                self.pending.extend(plan.edits);
                Ok(plan.output)
            }
            n if places::NAMES.contains(&n) => places::run(&places::path(), cx.dir, n, args, dry),
            n if reveal::NAMES.contains(&n) => reveal::run(cx.dir, args, dry),
            n if crate::file_ops::NAMES.contains(&n) => {
                let plan = crate::file_ops::run(cx, n, args, dry, &self.trash)?;
                self.pending.extend(plan.edits);
                Ok(plan.output)
            }
            n if crate::listen::NAMES.contains(&n) => {
                crate::listen::run(cx, Some(self.catalog()), args)
            }
            n if crate::pool::NAMES.contains(&n) => {
                let plan = crate::pool::run(cx, n, args, dry)?;
                self.pending.extend(plan.edits);
                Ok(plan.output)
            }
            n => self.transport(cx, n, args, dry),
        }
    }

    fn take_edits(&mut self) -> Vec<(String, Map<String, Value>)> {
        std::mem::take(&mut self.pending)
    }

    fn song_changed(&mut self, cx: &ExtContext<'_>) {
        if let Some(pl) = &mut self.player {
            if pl.dir.as_deref() != cx.dir {
                // Another song: its audio paths are relative to another folder.
                self.player = None;
            } else {
                pl.host.sync(cx.project);
            }
        }
        self.look(cx);
    }

    fn take_events(&mut self) -> Vec<Value> {
        self.watch.take()
    }

    /// By the last scan; with none to go by, it cannot tell.
    fn has_plugin(&mut self, plugin: &Plugin) -> Option<bool> {
        if crate::host::on_window_thread() && self.catalog.is_none() {
            return None;
        }
        let catalog = self.catalog();
        (!catalog.entries.is_empty()).then(|| catalog.find(&plugin.uid).is_some())
    }
}

impl Drop for SessionExt {
    /// Closing stops the exports still running; each says it was interrupted by the close.
    fn drop(&mut self) {
        let running: Vec<&Arc<Job>> = self.jobs.iter().filter(|j| j.running()).collect();
        for j in &running {
            j.closing.store(true, Ordering::Relaxed);
            j.progress.cancel();
        }
        let start = Instant::now();
        while running.iter().any(|j| j.is_export() && j.running()) && start.elapsed() < CLOSE_WAIT {
            std::thread::sleep(Duration::from_millis(20));
        }
        for j in running.iter().filter(|j| j.is_export()) {
            j.interrupt();
        }
    }
}

/// What `plugin.rescan` says about a finished scan.
fn scan_output(r: &ScanReport, opts: &ScanOptions) -> Output {
    let catalog = &r.catalog;
    let failures: Vec<Value> = catalog
        .failures
        .iter()
        .map(|f| json!({ "path": f.path, "kind": f.kind, "error": f.error }))
        .collect();
    Output::with(
        format!(
            "{} 個のバンドルから {} 個のプラグインを検出しました（スキャン {}・キャッシュ {}・失敗 {}、{:.2} 秒）",
            r.bundles,
            catalog.entries.len(),
            r.probed,
            r.cached,
            catalog.failures.len(),
            r.elapsed.as_secs_f64()
        ),
        json!({
            "bundles": r.bundles,
            "plugins": catalog.entries.len(),
            "probed": r.probed,
            "cached": r.cached,
            "failed": r.failed,
            "elapsed": r.elapsed.as_secs_f64(),
            "failures": failures,
            "cache": opts.cache,
        }),
    )
}

#[cfg(all(test, feature = "test-io"))]
mod io_tests {
    use super::*;
    use std::sync::{Condvar, mpsc};

    #[test]
    fn export_reports_do_not_wait_for_metadata_persistence() {
        let root = tempfile::tempdir().unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(gate.clone());
        let path = root.path().to_owned();
        let ui = std::thread::current().id();
        let (tx, entered) = mpsc::channel();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, file| {
            if op == "export_record" && file.starts_with(&path) {
                assert_ne!(std::thread::current().id(), ui);
                let _ = tx.send(());
                drop(
                    gate.1
                        .wait_while(gate.0.lock().unwrap(), |done| !*done)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        let job = Arc::new(Job {
            disk_gate: Mutex::new(()),
            id: "export-fixture".into(),
            command: "export.mix".into(),
            args: Value::Null,
            path: String::new(),
            replaces: Vec::new(),
            writes: Vec::new(),
            base: Value::Null,
            touched: Mutex::default(),
            progress: Arc::new(Progress::default()),
            started: Instant::now(),
            started_ms: now_ms(),
            ended: Mutex::new(None),
            state: Mutex::new(JobState::Running),
            closing: AtomicBool::new(false),
            disk: Some(Store::of_song(root.path())),
            lock: Mutex::new(None),
        });
        let writer = job.clone();
        let worker = std::thread::spawn(move || writer.finish(&Ok((String::new(), Value::Null))));
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(job.state.try_lock().is_ok());
        for _ in 0..32 {
            assert_eq!(job.to_json()["state"], "done");
        }
        drop(release);
        worker.join().unwrap();
    }
}
