//! A session: the song being edited, its history, the selection and the song
//! folder it lives in. Every client (UI, CLI, MCP) drives the same `run`.
//!
//! History is a list of whole-song snapshots, one per step. A `Project` is a
//! handful of `Arc`s, so a step costs only what the command touched, and
//! undo / redo / jump to any step are just "make that snapshot current", which
//! is right for every command by construction. A step records the command, its
//! arguments, who ran it (human or AI) and when (ARC-003, ARC-007).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::clipboard::Clipboard;
use crate::command::{ArgType, Args, Author, CommandDef, EditCtx, Handler, Output, Registry, Risk};
use crate::diff::{ChangeSet, diff};
use crate::error::{CmdError, CmdResult, ErrorCode};
use crate::extension::{ExtContext, Extension};
use crate::id::AnyId;
use crate::model::Project;
use crate::recovery::Recovery;
use crate::store::{
    FORMAT_VERSION, HISTORY_FORMAT_VERSION, HistoryIndex, Manifest, Overwrite, Stamp, Store,
    VersionInfo, VersionKind, now_ms,
};
use crate::time::Tick;

/// Steps kept (older ones fall off the back).
pub const MAX_STEPS: usize = 1000;
/// Snapshots kept in memory when a song folder backs the history.
pub const KEEP_LOADED: usize = 200;
/// Automatic save points kept.
pub const AUTO_KEEP: usize = 20;
/// The gap between automatic save points: 30 seconds (PRJ-009).
pub const AUTO_INTERVAL_MS: u64 = 30 * 1000;
/// How long after a file was written its size and time can be trusted to tell the next
/// write (PRJ-025): the coarsest clock a song folder may be on (FAT) keeps times in steps of
/// 2 seconds, and a write within the same step leaves the same time.
pub const STAMP_SETTLES: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) const LOCK: &str = "session.lock";

mod collect;
pub use collect::{Collected, copy_into, is_external, song_name};
mod renames;
mod work;
pub use work::*;
mod writer;
use writer::{Job, Writer};
pub(crate) mod saving;
pub use saving::SaveProgress;
mod lane;
use lane::Lane;
pub use lane::{MAX_LANES, others_open};

/// What the AI may do (AI-012). Each level includes the ones before it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiPermission {
    /// Nothing at all.
    Off,
    /// Look at the song; edits run as a dry run only.
    Read,
    /// Edit the song (every step undoable).
    #[default]
    Edit,
    /// Edit and save.
    Save,
}

impl AiPermission {
    pub const ALL: [&'static str; 4] = ["off", "read", "edit", "save"];

    pub fn parse(s: &str) -> CmdResult<AiPermission> {
        Ok(match s {
            "off" => AiPermission::Off,
            "read" => AiPermission::Read,
            "edit" => AiPermission::Edit,
            "save" => AiPermission::Save,
            _ => {
                return Err(CmdError::invalid(format!(
                    "AI の権限には {} のいずれかを指定してください",
                    Self::ALL.join("・")
                )));
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        Self::ALL[self as usize]
    }
}

/// One entry of the history.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Step {
    /// Number of the state this step made.
    pub n: u64,
    pub command: String,
    pub args: Value,
    pub author: Author,
    pub at_ms: u64,
    pub summary: String,
    /// Steps of one macro run carry the number of the first one; undo and redo take them
    /// together. 0 for a step on its own.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub group: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// A text longer than this in a step's arguments is kept as its length only. The state the
/// step made holds it in full; the arguments are read to show the step, and the index that
/// carries them is written whole at every edit (a plugin's state is hundreds of KB).
pub const ARG_KEPT_BYTES: usize = 4096;

/// `args` with every text longer than [`ARG_KEPT_BYTES`] put as its length.
fn slim_args(args: &mut Value) {
    match args {
        Value::String(s) if s.len() > ARG_KEPT_BYTES => *s = format!("…（{} バイト）", s.len()),
        Value::Array(a) => a.iter_mut().for_each(slim_args),
        Value::Object(o) => o.values_mut().for_each(slim_args),
        _ => {}
    }
}

/// A row of the history panel.
#[derive(Clone, Debug, Serialize)]
pub struct HistoryRow {
    pub n: u64,
    pub label: String,
    pub command: String,
    /// What the command was run with (the one-line `xirius …` of the operation).
    pub args: Value,
    pub author: Author,
    pub at_ms: u64,
    pub summary: String,
    /// The state the song is at now.
    pub current: bool,
    /// Undone: it would come back with redo.
    pub undone: bool,
}

/// A command that left no step, such as a change of the AI's permission. The history lists
/// it as a line with the steps (AI-025, AI-032).
#[derive(Clone, Debug, Serialize)]
pub struct OpNote {
    pub command: String,
    pub args: Value,
    pub author: Author,
    pub at_ms: u64,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    #[serde(default)]
    pub folders_closed: bool,
    #[serde(default)]
    pub ruler_seconds: bool,
    #[serde(default)]
    pub mixer_wide: bool,
    #[serde(default)]
    pub scale: Option<(u8, u16)>,
    #[serde(default)]
    pub roll: EditorView,
    #[serde(default)]
    pub quantize: serde_json::Value,
    /// The quantize programs A to G that were saved over the built-in ones (EDT-076).
    #[serde(default)]
    pub quantize_programs: std::collections::BTreeMap<String, serde_json::Value>,
    /// The program last chosen.
    #[serde(default)]
    pub quantize_program: Option<String>,
    #[serde(default = "arrange_editor")]
    pub editor: String,
    #[serde(default)]
    pub link: bool,
    /// What the pencil draws (UI-128).
    #[serde(default)]
    pub draw_shape: crate::shape::Shape,
    pub zoom_h: f64,
    pub zoom_v: f64,
    pub scroll_tick: Tick,
    pub grid: String,
    pub snap: bool,
}

impl Default for View {
    fn default() -> View {
        View {
            folders_closed: false,
            ruler_seconds: false,
            mixer_wide: false,
            scale: None,
            roll: EditorView::default(),
            quantize: serde_json::Value::Null,
            quantize_programs: Default::default(),
            quantize_program: None,
            editor: arrange_editor(),
            link: false,
            draw_shape: crate::shape::Shape::default(),
            zoom_h: 1.0,
            zoom_v: 1.0,
            scroll_tick: 0,
            grid: "1/16".into(),
            snap: true,
        }
    }
}

fn arrange_editor() -> String {
    "arrange".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditorView {
    #[serde(default)]
    pub external_groove: Option<crate::editing::Groove>,
    #[serde(default)]
    pub mode: Option<String>,
    pub grid: String,
    pub snap: bool,
    #[serde(default)]
    pub quantize: serde_json::Value,
    /// Whether a part opens in the drum view (by clip id); a part not listed follows its
    /// instrument (EDT-006).
    #[serde(default)]
    pub drum_clips: std::collections::BTreeMap<String, bool>,
    /// The drum map each track shows (by track id).
    #[serde(default)]
    pub drum_maps: std::collections::BTreeMap<String, String>,
    /// What was changed in each track's drum rows (by track id): their order, the hidden ones and
    /// their names.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub drum_rows: std::collections::BTreeMap<String, crate::drum::RowEdits>,
}

impl Default for EditorView {
    fn default() -> Self {
        Self {
            external_groove: None,
            mode: None,
            grid: "1/16".into(),
            snap: true,
            quantize: serde_json::Value::Null,
            drum_clips: Default::default(),
            drum_maps: Default::default(),
            drum_rows: Default::default(),
        }
    }
}

/// What a session remembers that is not the song: never in the history.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SessionState {
    pub selection: Vec<AnyId>,
    pub view: View,
    /// What the last copy or cut took (CTX-028); kept outside the history.
    #[serde(skip)]
    pub clipboard: Option<Clipboard>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunOptions {
    pub author: Author,
    pub dry_run: bool,
}

impl Default for RunOptions {
    fn default() -> RunOptions {
        RunOptions {
            author: Author::Human,
            dry_run: false,
        }
    }
}

impl RunOptions {
    pub fn human() -> RunOptions {
        RunOptions::default()
    }
    pub fn ai(name: impl Into<String>) -> RunOptions {
        RunOptions {
            author: Author::Ai(name.into()),
            dry_run: false,
        }
    }
    pub fn dry(mut self) -> RunOptions {
        self.dry_run = true;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunResult {
    pub command: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
    pub changes: ChangeSet,
    pub dry_run: bool,
    /// The AI may only look, so a dry run was made instead.
    pub forced_dry_run: bool,
    /// The history step the run made (none for a read, a dry run or a no-op).
    pub step: Option<u64>,
    /// The run was not made: a person has to answer first (AI-017).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
    /// What the extensions have to tell since the last run (`export.stale`, EXP-023), oldest
    /// first. Each has the field `event` naming it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Value>,
}

#[derive(Clone)]
struct History {
    /// Number of the oldest state kept.
    first: u64,
    cursor: u64,
    /// `steps[i]` made state `first + i + 1`.
    steps: Vec<Step>,
    /// `states[i]` is state `first + i`; `None` while only on disk.
    states: Vec<Option<Arc<Project>>>,
    /// Highest `next_id` ever seen, so an id is never reused after an undo.
    id_floor: u64,
}

impl History {
    fn new(base: Arc<Project>) -> History {
        History {
            first: 0,
            cursor: 0,
            steps: Vec::new(),
            id_floor: base.next_id,
            states: vec![Some(base)],
        }
    }

    fn last(&self) -> u64 {
        self.first + self.steps.len() as u64
    }

    fn rel(&self, abs: u64) -> usize {
        (abs - self.first) as usize
    }

    fn index(&self) -> HistoryIndex {
        HistoryIndex {
            first: self.first,
            cursor: self.cursor,
            steps: self.steps.clone(),
            id_floor: self.id_floor,
        }
    }

    fn get(&mut self, abs: u64, store: Option<&Store>) -> CmdResult<Arc<Project>> {
        if abs < self.first || abs > self.last() {
            return Err(CmdError::range(format!(
                "手順 {abs} はありません（{}〜{}）",
                self.first,
                self.last()
            )));
        }
        let rel = self.rel(abs);
        if let Some(p) = &self.states[rel] {
            return Ok(p.clone());
        }
        let store =
            store.ok_or_else(|| CmdError::new(ErrorCode::Corrupt, "履歴の状態を読み込めません"))?;
        let p = Arc::new(store.read_state(abs)?);
        self.states[rel] = Some(p.clone());
        Ok(p)
    }

    /// Is state `abs` in memory?
    fn loaded(&self, abs: u64) -> bool {
        abs >= self.first && abs <= self.last() && self.states[self.rel(abs)].is_some()
    }

    /// Drops far-away snapshots from memory; the files keep them.
    fn evict(&mut self) {
        let keep_from = self.states.len().saturating_sub(KEEP_LOADED);
        let cur = self.rel(self.cursor);
        for (i, s) in self.states.iter_mut().enumerate() {
            if i < keep_from && i != cur {
                *s = None;
            }
        }
    }
}

pub struct Session {
    registry: Arc<Registry>,
    hist: History,
    current: Arc<Project>,
    /// The state `project.json` holds (none until the song has been saved).
    saved: Option<Arc<Project>>,
    pub state: SessionState,
    store: Option<Store>,
    ai: AiPermission,
    next_version: u64,
    auto_base: Option<Arc<Project>>,
    last_auto_ms: u64,
    pub auto_interval_ms: u64,
    /// Things worth telling the person once (a crash was recovered, ...).
    pub notices: Vec<String>,
    /// The history lane this session holds in its song folder (PRJ-023).
    lane: Option<Lane>,
    /// The history step `project.json` was written at (a save, not an
    /// automatic save point).
    saved_step: u64,
    /// What came back when the folder was opened after a crash.
    recovery: Option<Recovery>,
    /// What the AI's work is waiting on, and what it is touching (AI-012).
    ai_work: AiWork,
    /// Answer the table's commands core does not run itself (ARC-016).
    extensions: Vec<Box<dyn Extension>>,
    /// Set while the edits of a macro run go in: `Some(0)` until the first step is made, then
    /// that step's number.
    group: Option<u64>,
    /// The group is a turn of one field (a wheel): each step takes the place of the turn's
    /// last one, so the turn is one step of the history (UI-319).
    turn: bool,
    /// Whether a person's edits answer with `state_hash`. The screen's session goes without:
    /// nobody there reads it, and it is the whole song written out twice for each edit (UI-319).
    hash_people: bool,
    /// Commands that left no step, for the history's lines (not saved with the song).
    notes: Vec<OpNote>,
    /// The SHA-256 of `project.json` as this session last read or wrote it: the AI writes over
    /// the file only while it is still that (AI-018).
    disk_hash: Option<String>,
    /// The size and time `project.json` had when it was last found to be the file of
    /// `disk_hash`: while it still has them, the file is not read to be hashed (PRJ-025).
    disk_stamp: Option<Stamp>,
    /// How many times [`Session::disk_changed`] read and hashed `project.json`.
    disk_hashes: u64,
    /// Where the history's writes go (PRJ-022).
    writer: Writer,
    /// States were let go of since the blobs were last pruned, so the next save prunes them
    /// (PRJ-021).
    prune_due: bool,
    /// The automatic save points handed to the writer, for the list while they are on the
    /// way (PRJ-022).
    autos_sent: Vec<VersionInfo>,
    /// The history was started afresh on open, from the saved song, and its first state is
    /// not on the disk yet: `history/<first>.json` may be another history's (PRJ-021).
    base_unwritten: bool,
}

impl Drop for Session {
    fn drop(&mut self) {
        // The lock goes only once the history is on the disk.
        self.writer.flush();
        if let (Some(lane), Some(s)) = (&self.lane, &self.store) {
            let _ = fs::remove_file(lane::marker(&s.dir, lane.k));
            // Another process's lane that holds no step has nothing to keep.
            if lane.k != 0 && !s.has_index() {
                let _ = s.remove_lane(lane.k);
            }
        }
    }
}

impl Session {
    /// Goes down as a process that crashes would (for the tests): the writes handed over are
    /// made and the lane is let go, but nothing is closed, so its marker stays.
    #[doc(hidden)]
    pub fn simulate_crash(mut self) {
        self.writer.flush();
        if let Some(lane) = &mut self.lane {
            lane.drop_lock();
        }
        std::mem::forget(self);
    }

    // ---- creating and opening -------------------------------------------

    /// An empty song that lives only in memory until `save-as`.
    pub fn new(name: &str) -> Session {
        Session::from_project(Project::new(name))
    }

    pub fn from_project(p: Project) -> Session {
        Session::with_registry(p, Arc::new(Registry::standard()))
    }

    pub fn with_registry(p: Project, registry: Arc<Registry>) -> Session {
        let cur = Arc::new(p);
        Session {
            registry,
            hist: History::new(cur.clone()),
            current: cur,
            saved: None,
            state: SessionState::default(),
            store: None,
            ai: AiPermission::default(),
            next_version: 1,
            auto_base: None,
            last_auto_ms: now_ms(),
            auto_interval_ms: AUTO_INTERVAL_MS,
            notices: Vec::new(),
            lane: None,
            saved_step: 0,
            recovery: None,
            ai_work: AiWork::default(),
            extensions: Vec::new(),
            group: None,
            turn: false,
            hash_people: true,
            notes: Vec::new(),
            disk_hash: None,
            disk_stamp: None,
            disk_hashes: 0,
            writer: Writer::default(),
            prune_due: false,
            autos_sent: Vec::new(),
            base_unwritten: false,
        }
    }

    /// Makes a new song folder holding `p`.
    pub fn create(dir: impl AsRef<Path>, p: Project) -> CmdResult<Session> {
        let mut s = Session::from_project(p);
        s.attach_new_store(dir.as_ref(), false)?;
        Ok(s)
    }

    pub fn open(dir: impl AsRef<Path>) -> CmdResult<Session> {
        Session::open_with(dir, Arc::new(Registry::standard()))
    }

    /// Opens a song folder. The state the history ends at is what you get, so
    /// edits made but not saved before a quit or a crash are still there.
    ///
    /// While another process has the song open, this one keeps a history of its own
    /// (PRJ-023): it goes on with the one its last run in that place left, or starts at where
    /// the first process's history is.
    pub fn open_with(dir: impl AsRef<Path>, registry: Arc<Registry>) -> CmdResult<Session> {
        Session::open_inner(dir.as_ref(), registry, &mut None)
    }

    /// Opens `dir` in the lane `lane` holds (the song this session has open, opened again), or
    /// in the first one free. The lane is the session's once the song is open; till then it
    /// stays in `lane`.
    fn open_inner(
        dir: &Path,
        registry: Arc<Registry>,
        lane: &mut Option<Lane>,
    ) -> CmdResult<Session> {
        if !Store::is_song(dir) {
            return Err(CmdError::not_found(format!(
                "{} は曲のフォルダーではありません",
                dir.display()
            )));
        }
        let reopened = lane.is_some();
        if lane.is_none() {
            *lane = Some(Lane::take(dir, false)?);
        }
        let k = lane.as_ref().map_or(0, |l| l.k);
        let store = Store::in_lane(dir, k);
        let view = store.read_view()?;
        let manifest = store.read_manifest()?;
        let saved = Arc::new(store.read_project()?);
        let was_locked = !reopened && lane::marker(dir, k).exists();
        let mut notices = Vec::new();
        let mut prune_due = false;
        let mut base_unwritten = false;

        let (hist, current) = match store.read_index()? {
            Some(mut idx) => {
                // A song written before the arguments were kept short: the next index written
                // is short too.
                for s in &mut idx.steps {
                    slim_args(&mut s.args);
                }
                let mut ok =
                    idx.cursor >= idx.first && idx.cursor <= idx.first + idx.steps.len() as u64;
                // An older Xirius that could not read this history started its own from the
                // saved song without writing that song as state 0, and wrote its steps (of
                // format 1) after it. State 0 is then this Xirius's (format 2), not the one
                // those steps were made on: it is not gone back to.
                if ok
                    && idx.first == 0
                    && !idx.steps.is_empty()
                    && store.state_format(0) == Some(HISTORY_FORMAT_VERSION)
                    && store.state_format(1) == Some(FORMAT_VERSION)
                {
                    if idx.cursor == 0 {
                        ok = false;
                    } else {
                        idx.first = 1;
                        idx.steps.remove(0);
                    }
                }
                match store.read_state(idx.cursor) {
                    Ok(cur) if ok => {
                        // A crash in the middle of a step, or an older Xirius that wrote its own
                        // history over this one, leaves states the index does not name: the
                        // next save clears them and the blobs only they pointed to.
                        prune_due =
                            store.has_states_outside(idx.first, idx.first + idx.steps.len() as u64);
                        let cur = Arc::new(cur);
                        let mut states = vec![None; idx.steps.len() + 1];
                        states[(idx.cursor - idx.first) as usize] = Some(cur.clone());
                        let id_floor = idx.id_floor.max(cur.next_id).max(saved.next_id);
                        (
                            History {
                                first: idx.first,
                                cursor: idx.cursor,
                                steps: idx.steps,
                                states,
                                id_floor,
                            },
                            cur,
                        )
                    }
                    _ => {
                        notices.push(
                            "履歴を読み込めなかったため、保存済みの曲から再開しました".to_string(),
                        );
                        prune_due = true;
                        base_unwritten = true;
                        (History::new(saved.clone()), saved.clone())
                    }
                }
            }
            None => {
                base_unwritten = true;
                // Another process's history starts at where the first process's is (which
                // may not be saved yet).
                let first = Store::new(dir);
                let base = (k != 0)
                    .then(|| first.read_index().ok().flatten())
                    .flatten()
                    .and_then(|idx| first.read_state(idx.cursor).ok())
                    .map_or_else(|| saved.clone(), Arc::new);
                (History::new(base.clone()), base)
            }
        };
        let saved_step = if k == 0 {
            manifest.saved_step
        } else {
            store
                .lane_saved_step()
                .filter(|n| (hist.first..=hist.last()).contains(n))
                .unwrap_or(hist.first)
        };

        let mut next_version = manifest.next_version.max(1);
        let versions = store.list_versions()?;
        for v in &versions {
            if let Some(n) = v.id.strip_prefix('v').and_then(|n| n.parse::<u64>().ok()) {
                next_version = next_version.max(n + 1);
            }
        }

        let mut s = Session {
            registry,
            hist,
            current,
            saved: Some(saved.clone()),
            state: SessionState {
                view,
                ..SessionState::default()
            },
            store: Some(store),
            ai: manifest.ai_permission,
            next_version,
            auto_base: None,
            last_auto_ms: now_ms(),
            auto_interval_ms: AUTO_INTERVAL_MS,
            notices,
            lane: None,
            saved_step,
            recovery: None,
            ai_work: AiWork::default(),
            extensions: Vec::new(),
            group: None,
            turn: false,
            hash_people: true,
            notes: Vec::new(),
            disk_hash: None,
            disk_stamp: None,
            disk_hashes: 0,
            writer: Writer::default(),
            prune_due,
            autos_sent: Vec::new(),
            base_unwritten,
        };

        if was_locked {
            let mut msg =
                "前回は正常に終了しませんでした。履歴の最後の状態から再開します".to_string();
            let mut version = None;
            if s.is_dirty() {
                let info = s.new_version(
                    "異常終了の前に保存されていた状態",
                    VersionKind::BeforeCrash,
                    &saved,
                )?;
                msg.push_str(&format!(
                    "（保存されていた状態はバージョン {} に残しました）",
                    info.id
                ));
                version = Some(info.id);
            }
            // The blob a step cut off by the crash wrote before its state goes with the next
            // save.
            s.prune_due = true;
            s.recovery = Some(Recovery::from_history(
                &s.hist.steps,
                s.hist.cursor,
                s.saved_step,
                version,
            ));
            s.notices.push(msg);
        }
        if k == 0 && !reopened {
            s.close_free_lanes()?;
        }
        // A lane its last run left with nothing unsaved, where another process has saved the
        // song since, goes on from that file (UI-289): what was saved there is not written over.
        // The lane's own steps stay under it.
        if k != 0
            && !reopened
            && !was_locked
            && !base_unwritten
            && s.saved_step == s.hist.cursor
            && s.is_dirty()
        {
            let mut base = (*saved).clone();
            base.next_id = base.next_id.max(s.hist.id_floor);
            let changes = diff(&s.current, &base);
            s.push_step(
                base,
                "project.reload",
                json!({}),
                &Author::Human,
                changes.summary(),
            );
            s.saved_step = s.hist.cursor;
            s.saved = Some(s.current.clone());
        }
        s.set_disk_hash(Store::new(dir).project_hash()?);
        let mark = lane::marker(dir, k);
        if let Some(parent) = mark.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(mark, std::process::id().to_string());
        s.lane = lane.take();
        Ok(s)
    }

    /// Closes the other processes' lanes no process holds any more (PRJ-023): where each got
    /// to is kept as a version when it differs from the song as it opens, and the lane goes.
    /// A lane that cannot be read is left as it is.
    fn close_free_lanes(&mut self) -> CmdResult<()> {
        let Some(store) = self.store.clone() else {
            return Ok(());
        };
        for k in store.lanes().into_iter().filter(|&k| k != 0) {
            let Ok(Some(_held)) = lane::try_hold(&store.dir, k) else {
                continue;
            };
            let other = Store::in_lane(&store.dir, k);
            let end = match other.read_index() {
                Ok(None) => None,
                Ok(Some(idx)) => match other.read_state(idx.cursor) {
                    Ok(p) => Some(p),
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            if let Some(p) = end
                && p != *self.current
            {
                let info = self.new_version(
                    "ほかのプロセスで編集していた状態",
                    VersionKind::OtherProcess,
                    &p,
                )?;
                self.notices.push(format!(
                    "ほかのプロセスがこの曲で編集していた状態をバージョン {} に残しました",
                    info.id
                ));
            }
            if other.remove_lane(k).is_ok() {
                self.prune_due = true;
            }
        }
        Ok(())
    }

    // ---- reading ---------------------------------------------------------

    pub fn project(&self) -> &Project {
        &self.current
    }

    pub fn project_arc(&self) -> Arc<Project> {
        self.current.clone()
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The song's folder, once the writes handed to the thread are done (so what it reads is
    /// up to date).
    pub fn store(&self) -> Option<&Store> {
        self.writer.flush();
        self.store.as_ref()
    }

    pub fn dir(&self) -> Option<&Path> {
        self.store.as_ref().map(|s| s.dir.as_path())
    }

    pub fn ai_permission(&self) -> AiPermission {
        self.ai
    }

    pub fn set_ai_permission(&mut self, p: AiPermission) -> CmdResult<()> {
        self.ai = p;
        if self.store.is_some() {
            self.write_manifest()?;
        }
        Ok(())
    }

    /// Does the song differ from what is saved?
    pub fn is_dirty(&self) -> bool {
        match &self.saved {
            None => true,
            Some(s) => !Arc::ptr_eq(s, &self.current) && **s != *self.current,
        }
    }

    pub fn cursor(&self) -> u64 {
        self.hist.cursor
    }

    pub fn first_step(&self) -> u64 {
        self.hist.first
    }

    pub fn last_step(&self) -> u64 {
        self.hist.last()
    }

    pub fn can_undo(&self) -> bool {
        self.hist.cursor > self.hist.first
    }

    /// Steps made until [`Session::end_group`] are taken back and done again together (a macro
    /// the screen runs step by step).
    pub fn begin_group(&mut self) {
        self.group = Some(0);
        self.turn = false;
    }

    /// A group of one field turned again and again (a wheel or a drag): each step takes the
    /// place of the last one the turn made, so the history keeps one step for the whole turn,
    /// and the disk one state (UI-319). [`Session::end_group`] ends it.
    pub fn begin_turn(&mut self) {
        self.group = Some(0);
        self.turn = true;
    }

    pub fn end_group(&mut self) {
        self.group = None;
        self.turn = false;
    }

    /// Whether a person's edits answer with `state_hash` (on by default). The screen turns it
    /// off; the AI's edits keep it, and so does a song opened later in this session.
    pub fn hash_people_edits(&mut self, on: bool) {
        self.hash_people = on;
    }

    pub fn can_redo(&self) -> bool {
        self.hist.cursor < self.hist.last()
    }

    pub fn steps(&self) -> &[Step] {
        &self.hist.steps
    }

    pub fn history(&self) -> Vec<HistoryRow> {
        let mut rows = vec![HistoryRow {
            n: self.hist.first,
            label: "初期状態".into(),
            command: String::new(),
            args: Value::Null,
            author: Author::Human,
            at_ms: 0,
            summary: String::new(),
            current: self.hist.cursor == self.hist.first,
            undone: false,
        }];
        for s in &self.hist.steps {
            rows.push(HistoryRow {
                n: s.n,
                label: s.author.row_label(&s.command),
                command: s.command.clone(),
                args: s.args.clone(),
                author: s.author.clone(),
                at_ms: s.at_ms,
                summary: s.summary.clone(),
                current: s.n == self.hist.cursor,
                undone: s.n > self.hist.cursor,
            });
        }
        rows
    }

    /// The commands run in this session that left no step (a change of the AI's permission),
    /// oldest first.
    pub fn op_notes(&self) -> &[OpNote] {
        &self.notes
    }

    // ---- running commands --------------------------------------------------

    pub fn run_value(
        &mut self,
        name: &str,
        args: Value,
        opts: &RunOptions,
    ) -> CmdResult<RunResult> {
        let raw = match args {
            Value::Object(m) => m,
            Value::Null => Map::new(),
            _ => {
                return Err(CmdError::invalid(
                    "引数は {名前: 値} の形式で指定してください",
                ));
            }
        };
        self.run(name, &raw, opts)
    }

    pub fn run(
        &mut self,
        name: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
    ) -> CmdResult<RunResult> {
        let before = self.current.clone();
        let mut result = self.run_unnotified(name, raw, opts);
        if let Ok(result) = &mut result
            && self
                .registry
                .get(name)
                .is_some_and(|def| def.risk != Risk::Read)
        {
            if !result.data.is_object() {
                result.data = json!({"value": result.data});
            }
            if self.hash_people || opts.author.is_ai() {
                result.data["state_hash"] =
                    json!({"before": before.content_hash(), "after": self.current.content_hash()});
            }
        }
        if !Arc::ptr_eq(&before, &self.current) {
            drop(before);
            self.tell_extensions(&opts.author);
        }
        if let Ok(result) = &mut result {
            result.events = self.take_events();
        }
        result
    }

    /// What the extensions have to tell, oldest first, taken from them. [`Session::run`]
    /// hands it with each result; a caller that changes the song by other means takes it here.
    pub fn take_events(&mut self) -> Vec<Value> {
        self.extensions
            .iter_mut()
            .flat_map(|e| e.take_events())
            .collect()
    }

    // ---- extensions ----------------------------------------------------------

    /// Adds an extension. It answers the `Handler::Session` commands of the table core has no
    /// answer for, and stays with the session when `project.open` opens another song.
    pub fn add_extension(&mut self, ext: Box<dyn Extension>) {
        self.extensions.push(ext);
    }

    /// Whether an extension has a window behind it: the commands marked `screen_only` run
    /// only then.
    pub fn has_screen(&self) -> bool {
        self.extensions.iter().any(|e| e.has_screen())
    }

    /// Whether `author` may run `name` at all: the check `run` makes first, for a caller that
    /// runs a table command by other means. A command that would wait for a person counts as
    /// refused.
    pub fn check_permission(&self, name: &str, author: &Author) -> CmdResult<()> {
        let def = self.registry.get(name).ok_or_else(|| {
            CmdError::new(
                ErrorCode::UnknownCommand,
                format!("コマンド `{name}` が見つかりません"),
            )
        })?;
        match self.gate(def, author)? {
            Gate::Allow => Ok(()),
            Gate::DryOnly | Gate::Confirm => Err(CmdError::new(
                ErrorCode::Denied,
                format!(
                    "{name} は現在の AI の権限（{}）では実行できません",
                    self.ai.as_str()
                ),
            )),
        }
    }

    fn tell_extensions(&mut self, author: &Author) {
        if self.extensions.is_empty() {
            return;
        }
        let mut exts = std::mem::take(&mut self.extensions);
        let cx = ExtContext {
            project: &self.current,
            dir: self.store.as_ref().map(|s| s.dir.as_path()),
            author,
            registry: &self.registry,
            work_copy: self.draft_project().is_some(),
            lane: self.lane.as_ref().map(|l| l.k),
        };
        for e in &mut exts {
            e.song_changed(&cx);
        }
        self.extensions = exts;
    }

    fn run_extension(
        &mut self,
        def: &CommandDef,
        args: &Args,
        opts: &RunOptions,
        dry: bool,
    ) -> CmdResult<RunResult> {
        let Some(i) = self.extensions.iter().position(|e| e.answers(def.name)) else {
            return Err(CmdError::new(
                ErrorCode::UnknownCommand,
                format!("{} は実装されていません", def.name),
            ));
        };
        let mut exts = std::mem::take(&mut self.extensions);
        let cx = ExtContext {
            project: &self.current,
            dir: self.store.as_ref().map(|s| s.dir.as_path()),
            author: &opts.author,
            registry: &self.registry,
            work_copy: self.draft_project().is_some(),
            lane: self.lane.as_ref().map(|l| l.k),
        };
        let out = exts[i].run(&cx, def, args, dry);
        let edits = if dry || out.is_err() {
            Vec::new()
        } else {
            exts[i].take_edits()
        };
        let grouped = edits.len() > 1 && exts[i].groups_edits(def.name);
        self.extensions = exts;
        if grouped {
            self.group = Some(0);
        }
        let mut result = RunResult::new(def.name, out?, ChangeSet::default());
        // What the run left for the song (the takes of a recording) goes in as edits of the
        // table, each a step of the history, reported under this command's name.
        for (name, raw) in edits {
            let r = self.run_inner(&name, &raw, opts, true);
            if r.is_err() {
                self.group = None;
            }
            let r = r?;
            if !r.message.is_empty() {
                result.message = format!("{}。{}", result.message, r.message);
            }
            if result.data.is_null() {
                result.data = r.data;
            } else if let (Value::Object(m), Value::Object(extra)) = (&mut result.data, r.data) {
                for (k, v) in extra {
                    m.entry(k).or_insert(v);
                }
            }
            result.changes = r.changes;
            result.step = r.step.or(result.step);
        }
        self.group = None;
        Ok(result)
    }

    fn run_unnotified(
        &mut self,
        name: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
    ) -> CmdResult<RunResult> {
        if let Some(r) = self.run_in_work_copy(name, raw, opts) {
            return r;
        }
        if opts.author.is_ai() {
            self.check_operator(&opts.author)?;
        }
        let result = self.run_inner(name, raw, opts, false)?;
        self.after_run(name, raw, opts, &result);
        Ok(result)
    }

    fn run_inner(
        &mut self,
        name: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
        confirmed: bool,
    ) -> CmdResult<RunResult> {
        let registry = self.registry.clone();
        let Some(def) = registry.get(name) else {
            let target = name.split('.').next().unwrap_or(name);
            let near: Vec<&str> = registry
                .all()
                .iter()
                .map(|d| d.name)
                .filter(|n| n.starts_with(target))
                .take(6)
                .collect();
            let hint = if near.is_empty() {
                String::new()
            } else {
                format!("（候補: {}）", near.join("・"))
            };
            return Err(CmdError::new(
                ErrorCode::UnknownCommand,
                format!("コマンド `{name}` が見つかりません{hint}"),
            ));
        };

        if def.screen_only && !self.has_screen() {
            return Err(CmdError::new(
                ErrorCode::Denied,
                format!(
                    "{} は画面でだけ使えます（画面のない CLI からは実行できません）",
                    def.name
                ),
            ));
        }
        let gate = self.gate(def, &opts.author)?;
        let asked_dry = opts.dry_run
            || raw
                .get("dry_run")
                .is_some_and(|v| v == &Value::Bool(true) || v == "true");
        // A trial run changes nothing, so it needs no answer.
        if gate == Gate::Confirm && !confirmed && !asked_dry {
            return self.hold(def, raw, opts);
        }
        let forced = gate == Gate::DryOnly;
        let mut dry = asked_dry || forced;

        let mut raw = Registry::spelled(def, raw)?;
        raw.remove("dry_run");
        if self.uses_selection(def) {
            self.fill_from_selection(def, &mut raw);
        }
        for ext in &self.extensions {
            ext.fill_args(def, &mut raw);
        }
        let args = registry.normalize(def, &raw)?;

        let mut result = match def.handler {
            Handler::Read(f) => {
                dry = false;
                let out = f(&self.current, &mut self.state, &registry, &args)?;
                RunResult::new(def.name, out, ChangeSet::default())
            }
            Handler::Edit(f) => self.run_edit(def, f, &args, opts, dry)?,
            Handler::Session => self.run_session(def, &args, opts, dry)?,
        };
        result.dry_run = dry;
        result.forced_dry_run = forced;
        if forced {
            result
                .message
                .push_str("（AI の権限が「読み取りのみ」のため、ドライランとして実行しました）");
        }
        Ok(result)
    }

    /// Whether the run may be made, must be a dry run, or waits for a person.
    fn gate(&self, def: &CommandDef, author: &Author) -> CmdResult<Gate> {
        let Author::Ai(who) = author else {
            return Ok(Gate::Allow);
        };
        let deny = |why: &str| {
            Err(CmdError::new(
                ErrorCode::Denied,
                format!(
                    "{} は {who} からは実行できません: {why}（AI の権限: {}）",
                    def.name,
                    self.ai.as_str()
                ),
            ))
        };
        let confirm = self.ai_work.confirm;
        if def.name == "ai.permission.set" {
            if self.ai == AiPermission::Off {
                return deny("AI の権限がオフです");
            }
            return if confirm.save {
                Ok(Gate::Confirm)
            } else {
                deny("権限の変更にはユーザーの承認が必要です")
            };
        }
        match (def.risk, self.ai) {
            (_, AiPermission::Off) => deny("AI の権限がオフです"),
            (Risk::HumanOnly, _) => deny("ユーザーのみ実行できます"),
            (Risk::Read, _) => Ok(Gate::Allow),
            (Risk::Write, AiPermission::Read) | (Risk::Destructive, AiPermission::Read) => {
                Ok(Gate::DryOnly)
            }
            (Risk::Write, _) => Ok(Gate::Allow),
            (Risk::Destructive, _) if confirm.destructive => Ok(Gate::Confirm),
            (Risk::Destructive, _) => Ok(Gate::Allow),
            (Risk::Guarded, AiPermission::Save) => Ok(Gate::Allow),
            (Risk::Guarded, AiPermission::Edit) if confirm.save => Ok(Gate::Confirm),
            (Risk::Guarded, _) => deny("保存には「編集と保存・書き出し」の権限が必要です"),
        }
    }

    fn uses_selection(&self, def: &CommandDef) -> bool {
        matches!(def.handler, Handler::Edit(_))
    }

    /// An `ids` argument left out means "the selection".
    fn fill_from_selection(&self, def: &CommandDef, raw: &mut Map<String, Value>) {
        for a in &def.args {
            if let ArgType::Ids(kind) = &a.ty
                && raw.get(a.name).is_none_or(Value::is_null)
            {
                let ids: Vec<String> = self
                    .state
                    .selection
                    .iter()
                    .filter(|i| kind.accepts(i))
                    .map(ToString::to_string)
                    .collect();
                if !ids.is_empty() {
                    raw.insert(a.name.to_string(), json!(ids));
                }
            }
        }
    }

    fn run_edit(
        &mut self,
        def: &CommandDef,
        f: crate::command::EditFn,
        args: &Args,
        opts: &RunOptions,
        dry: bool,
    ) -> CmdResult<RunResult> {
        let before = self.current.clone();
        let mut work = (*before).clone();
        work.next_id = work.next_id.max(self.hist.id_floor);
        let ctx = EditCtx {
            author: &opts.author,
            store: self.store.as_ref(),
            clipboard: self.state.clipboard.as_ref(),
        };
        let mut out = f(&mut work, &ctx, args)?;
        crate::commands::overlap::apply(&before, &mut work, args)?;
        if def.name != "clip.relink" {
            crate::freeze::check(&before, &work)?;
        }
        work.prune_clip_fx();
        crate::retime::retime(&before, &mut work);
        let taken = out.clipboard.take();
        work.validate()
            .map_err(|e| CmdError::new(e.code, format!("{}: {}", def.name, e.message)))?;
        let mut changes = diff(&before, &work);
        if changes.is_empty() {
            let mut a = work.clone();
            a.next_id = before.next_id;
            if a == *before {
                let mut r = RunResult::new(def.name, out, changes);
                if r.message.is_empty() {
                    r.message = "変更はありませんでした".into();
                }
                return Ok(r);
            }
            // A change the diff has no word for: keep it, as a plain step.
            changes.counts.entry("project".into()).or_default().changed += 1;
        }
        let mut result = RunResult::new(def.name, out, changes.clone());
        if dry {
            return Ok(result);
        }
        if taken.is_some() {
            self.state.clipboard = taken;
        }
        let (n, warnings) = self.push_step(
            work,
            def.name,
            Value::Object(args.0.clone()),
            &opts.author,
            changes.summary(),
        );
        result.step = Some(n);
        if !warnings.is_empty() {
            result
                .message
                .push_str(&format!("（{}）", warnings.join("・")));
        }
        Ok(result)
    }

    // ---- the history -------------------------------------------------------

    fn push_step(
        &mut self,
        work: Project,
        command: &str,
        mut args: Value,
        author: &Author,
        summary: String,
    ) -> (u64, Vec<String>) {
        slim_args(&mut args);
        let mut warnings = Vec::new();
        let new = Arc::new(work);
        // A turn of one field takes the place of its last step, so the turn is one step of the
        // history and one state on the disk (UI-319). A relink's step stays: it moves files.
        if self.turn
            && let Some(g) = self.group.filter(|g| *g != 0)
            && self.hist.cursor == self.hist.last()
            && self.hist.cursor > self.hist.first
            && command != "clip.relink"
        {
            let last = &self.hist.steps[self.hist.rel(self.hist.cursor) - 1];
            if last.group == g && last.command == command {
                if let (Some(a), Some(b)) = (
                    last.args.get("by").and_then(Value::as_i64),
                    args.get("by").and_then(Value::as_i64),
                ) {
                    args["by"] = Value::from(a.saturating_add(b));
                }
                self.hist.cursor -= 1;
            }
        }
        let base = self.unwritten_base();
        let h = &mut self.hist;
        h.id_floor = h.id_floor.max(new.next_id);
        let rel = h.rel(h.cursor);
        let old_last = h.last();
        let truncated = rel + 1 < h.states.len();
        h.states.truncate(rel + 1);
        h.steps.truncate(rel);

        let before = truncated.then(|| h.index());
        let n = h.cursor + 1;
        let group = match self.group {
            Some(0) => {
                self.group = Some(n);
                n
            }
            Some(g) => g,
            None => 0,
        };
        h.steps.push(Step {
            n,
            command: command.to_string(),
            args,
            author: author.clone(),
            at_ms: now_ms(),
            summary,
            group,
        });
        h.states.push(Some(new.clone()));
        h.cursor = n;
        let mut dropped = Vec::new();
        while h.steps.len() > MAX_STEPS {
            h.steps.remove(0);
            h.states.remove(0);
            dropped.push(h.first);
            h.first += 1;
        }
        if self.store.is_some() {
            h.evict();
        }
        self.current = new.clone();

        if let Some(store) = self.store.clone() {
            let mut remove: Vec<u64> = (n + 1..=old_last).collect();
            remove.extend(dropped);
            // State `n` is written over when the step replaces the ones ahead.
            self.prune_due |= before.is_some() || !remove.is_empty();
            if let Some((abs, state)) = base
                && let Err(e) = self.writer.submit(Job::State {
                    store: store.clone(),
                    n: abs,
                    state,
                })
            {
                warnings.push(writer::message(&e));
            }
            let job = Job::Step {
                store,
                before,
                n,
                state: new,
                index: self.hist.index(),
                remove,
            };
            if let Err(e) = self.writer.submit(job) {
                warnings.push(writer::message(&e));
            }
        }
        (n, warnings)
    }

    /// Writes the index as the history is now, then lets go of the states in `remove`. A
    /// failure is kept for [`Session::take_write_errors`]; the move itself stands.
    fn persist_index(&mut self, remove: Vec<u64>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        self.prune_due |= !remove.is_empty();
        if let Some((abs, state)) = self.unwritten_base()
            && let Err(e) = self.writer.submit(Job::State {
                store: store.clone(),
                n: abs,
                state,
            })
        {
            self.writer.keep_error(e);
        }
        let job = Job::Index {
            store,
            index: self.hist.index(),
            remove,
        };
        if let Err(e) = self.writer.submit(job) {
            self.writer.keep_error(e);
        }
    }

    /// The first state, once, when it is not on the disk yet (`base_unwritten`).
    fn unwritten_base(&mut self) -> Option<(u64, Arc<Project>)> {
        if self.store.is_none() || !self.base_unwritten {
            return None;
        }
        let first = self.hist.first;
        let state = self.hist.states.first().cloned().flatten()?;
        self.base_unwritten = false;
        Some((first, state))
    }

    fn persist_cursor(&mut self) {
        self.persist_index(Vec::new());
    }

    /// History state `abs`. One only on the disk is read once the writes in flight are done.
    fn state_at(&mut self, abs: u64) -> CmdResult<Arc<Project>> {
        if !self.hist.loaded(abs) {
            self.writer.flush();
        }
        self.hist.get(abs, self.store.as_ref())
    }

    /// The song at history state `abs`, without moving to it.
    pub fn peek(&mut self, abs: u64) -> CmdResult<Arc<Project>> {
        self.state_at(abs)
    }

    /// Goes to history state `abs`. A step on the way that renamed a file on the disk renames
    /// it back or again ([`renames`]); when the disk cannot follow, nothing moves.
    pub fn jump(&mut self, abs: u64) -> CmdResult<ChangeSet> {
        let target = self.state_at(abs)?;
        self.move_disk(abs)?;
        let changes = diff(&self.current, &target);
        self.hist.cursor = abs;
        self.current = target;
        self.persist_cursor();
        self.prune_selection();
        Ok(changes)
    }

    pub fn undo(&mut self, count: u64) -> CmdResult<ChangeSet> {
        let to = self.undo_target(count)?;
        self.jump(to)
    }

    pub fn redo(&mut self, count: u64) -> CmdResult<ChangeSet> {
        let to = self.redo_target(count)?;
        self.jump(to)
    }

    /// Changes the song at the cursor in place, without a step of the history:
    /// for the state that follows something else (an instrument track's record
    /// arm following the selection), not an edit the person made and would take
    /// back. The state the cursor names gets the change, so undo and redo walk
    /// across it as though it had always been there. Returns the change it made
    /// (empty when the song was already so).
    pub fn amend(&mut self, f: impl FnOnce(&mut Project)) -> CmdResult<ChangeSet> {
        let before = self.current.clone();
        let mut work = (*before).clone();
        f(&mut work);
        work.validate()
            .map_err(|e| CmdError::new(e.code, format!("amend: {}", e.message)))?;
        let changes = diff(&before, &work);
        if changes.is_empty() {
            return Ok(changes);
        }
        let new = Arc::new(work);
        self.current = new.clone();
        let rel = self.hist.rel(self.hist.cursor);
        if let Some(slot) = self.hist.states.get_mut(rel) {
            *slot = Some(new.clone());
        }
        if let Some(store) = self.store.clone()
            && let Err(e) = self.writer.submit(Job::State {
                store,
                n: self.hist.cursor,
                state: new,
            })
        {
            self.writer.keep_error(e);
        }
        self.tell_extensions(&Author::Human);
        Ok(changes)
    }

    fn undo_target(&self, count: u64) -> CmdResult<u64> {
        if !self.can_undo() {
            return Err(CmdError::new(
                ErrorCode::NothingToDo,
                "取り消せる操作がありません",
            ));
        }
        let mut at = self.hist.cursor;
        for _ in 0..count.max(1) {
            if at <= self.hist.first {
                break;
            }
            // The step that made state `at`, and the steps of its macro before it.
            let group = self.hist.steps[self.hist.rel(at) - 1].group;
            at -= 1;
            while group != 0
                && at > self.hist.first
                && self.hist.steps[self.hist.rel(at) - 1].group == group
            {
                at -= 1;
            }
        }
        Ok(at)
    }

    fn redo_target(&self, count: u64) -> CmdResult<u64> {
        if !self.can_redo() {
            return Err(CmdError::new(
                ErrorCode::NothingToDo,
                "やり直せる操作がありません",
            ));
        }
        let mut at = self.hist.cursor;
        for _ in 0..count.max(1) {
            if at >= self.hist.last() {
                break;
            }
            let group = self.hist.steps[self.hist.rel(at)].group;
            at += 1;
            while group != 0
                && at < self.hist.last()
                && self.hist.steps[self.hist.rel(at)].group == group
            {
                at += 1;
            }
        }
        Ok(at)
    }

    /// Drops selected ids that no longer exist.
    fn prune_selection(&mut self) {
        let p = &self.current;
        let notes: HashSet<_> = if self
            .state
            .selection
            .iter()
            .any(|i| matches!(i, AnyId::Note(_)))
        {
            p.tracks
                .values()
                .flat_map(|t| t.clips.iter().flat_map(|c| c.notes().iter().map(|n| n.id)))
                .collect()
        } else {
            HashSet::new()
        };
        self.state.selection.retain(|id| match id {
            AnyId::Note(n) => notes.contains(n),
            other => exists(p, other),
        });
    }

    // ---- files -------------------------------------------------------------

    fn manifest(&self) -> Manifest {
        let mut m: Manifest = Store::new_manifest(&self.current.name, self.saved_step, self.ai);
        m.next_version = self.next_version;
        m
    }

    /// Writes the manifest now, after the writes in flight (an automatic save point writes it
    /// too).
    fn write_manifest(&self) -> CmdResult<()> {
        let store = self.store.as_ref().ok_or_else(no_store)?;
        self.writer.flush();
        store.update_manifest(&self.manifest())?;
        if store.lane != 0 {
            store.write_lane_saved_step(self.saved_step)?;
        }
        Ok(())
    }

    /// Writes the song to a folder, with the whole history and every version,
    /// and makes that folder the session's own.
    ///
    /// A `draft` keeps the song as it began: `project.json` holds the first
    /// state of the history and the song stays unsaved, so a crash can bring
    /// back every step and "discard" has somewhere to go back to.
    fn attach_new_store(&mut self, dir: &Path, draft: bool) -> CmdResult<()> {
        if let Some(old) = &self.store
            && same_path(&old.dir, dir)
        {
            return Err(CmdError::conflict(
                "現在と同じ場所には名前を付けて保存できません",
            ));
        }
        if dir.exists() && (!dir.is_dir() || dir.read_dir()?.next().is_some()) {
            return Err(CmdError::conflict(format!(
                "{} は空のフォルダーではありません。空のフォルダーか、まだ無いフォルダーを指定してください",
                dir.display()
            )));
        }
        fs::create_dir_all(dir)?;
        let new_lane = Lane::take(dir, true)?;
        let new = Store::new(dir);
        let old = self.store.clone();
        // The copy takes the history as the disk has it.
        self.writer.flush();
        // The old folder goes back to the state it was last saved in, so a
        // "save as" forks the song instead of dragging the original along. The
        // unsaved steps stay in its history as redo.
        if let Some(o) = &old
            && self.lane.is_some()
            && self.is_dirty()
            && self.saved_step >= self.hist.first
            && self.saved_step <= self.hist.last()
        {
            let mut idx = self.hist.index();
            idx.cursor = self.saved_step;
            let _ = o.write_index(&idx);
        }
        for abs in self.hist.first..=self.hist.last() {
            let p = self.hist.get(abs, old.as_ref())?;
            new.write_state(abs, &p)?;
        }
        // Only the blobs the states point to were copied.
        self.prune_due = false;
        self.base_unwritten = false;
        new.write_index(&self.hist.index())?;
        if let Some(o) = &old {
            o.copy_versions_to(&new)?;
        }
        let kept = if draft {
            self.hist.get(self.hist.first, old.as_ref())?
        } else {
            self.current.clone()
        };
        self.hist.evict();
        self.saved_step = if draft {
            self.hist.first
        } else {
            self.hist.cursor
        };
        new.write_project(&kept)?;
        self.set_disk_hash(new.project_hash()?);
        new.write_view(&self.state.view)?;
        let old_lane = self.lane.replace(new_lane);
        self.store = Some(new);
        let _ = fs::write(lane::marker(dir, 0), std::process::id().to_string());
        if let (Some(o), Some(l)) = (&old, old_lane) {
            let _ = fs::remove_file(lane::marker(&o.dir, l.k));
        }
        self.write_manifest()?;
        self.saved = Some(kept);
        Ok(())
    }

    pub fn save(&mut self) -> CmdResult<()> {
        let store = self.store.clone().ok_or_else(no_store)?;
        self.writer.flush();
        store.write_project(&self.current)?;
        self.set_disk_hash(store.project_hash()?);
        store.write_view(&self.state.view)?;
        self.saved_step = self.hist.cursor;
        self.write_manifest()?;
        self.prune_blobs(store);
        self.saved = Some(self.current.clone());
        Ok(())
    }

    /// After a save: the blobs the history no longer points to go, on the writer's thread
    /// when it has one. Nothing is told when that fails; the next save tries again.
    fn prune_blobs(&mut self, store: Store) {
        if std::mem::take(&mut self.prune_due) {
            let _ = self.writer.submit(Job::Prune {
                store,
                first: self.hist.first,
                last: self.hist.last(),
            });
        }
    }

    /// [`Session::save`] as the AI may make it (AI-018): `project.json` is written over only
    /// while it is still the file this session last read or wrote, after a copy of it is kept
    /// in `backups/` and read back by its hash; the new file is read back as well.
    fn save_over(&mut self) -> CmdResult<Overwrite> {
        let store = self.store.clone().ok_or_else(no_store)?;
        self.writer.flush();
        let expected = self.expected_on_disk()?;
        let done = store.overwrite_project(&self.current, &expected)?;
        self.set_disk_hash(Some(done.after.clone()));
        store.write_view(&self.state.view)?;
        self.saved_step = self.hist.cursor;
        self.write_manifest()?;
        self.prune_blobs(store);
        self.saved = Some(self.current.clone());
        Ok(done)
    }

    /// The hash `project.json` has to have for the AI to write over it.
    fn expected_on_disk(&self) -> CmdResult<String> {
        self.disk_hash.clone().ok_or_else(|| {
            CmdError::conflict("project.json を読んでいないため、上書きを中止しました")
        })
    }

    pub fn save_as(&mut self, dir: impl AsRef<Path>) -> CmdResult<()> {
        self.attach_new_store(dir.as_ref(), false)
    }

    /// Keeps a song that has not been saved in a folder of its own, so its
    /// steps survive a crash. The song stays unsaved (it differs from what
    /// the folder calls saved) until `save-as` gives it a place.
    pub fn keep_as_draft(&mut self, dir: impl AsRef<Path>) -> CmdResult<()> {
        self.attach_new_store(dir.as_ref(), true)
    }

    /// What came back when this song was opened after a crash.
    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }

    /// The step the song was last saved at; "discard" goes back to it.
    pub fn saved_step(&self) -> u64 {
        self.saved_step
    }

    /// Goes back to the state last saved (the steps after it stay as redo).
    pub fn discard_unsaved(&mut self) -> CmdResult<()> {
        self.jump(self.saved_step)?;
        self.recovery = None;
        Ok(())
    }

    /// The recovery notice has been answered.
    pub fn clear_recovery(&mut self) {
        self.recovery = None;
    }

    fn new_version(
        &mut self,
        name: &str,
        kind: VersionKind,
        p: &Project,
    ) -> CmdResult<VersionInfo> {
        let store = self.store.clone().ok_or_else(no_store)?;
        let number = self.next_version;
        self.next_version += 1;
        if kind == VersionKind::Auto {
            // On the writer's thread when it has one, after the steps before it.
            let info = VersionInfo {
                id: format!("v{number}"),
                name: name.to_string(),
                kind,
                created_ms: now_ms(),
            };
            let job = Job::Version {
                store,
                info: info.clone(),
                state: Arc::new(p.clone()),
                manifest: self.manifest(),
            };
            if !self.writer.pending() {
                self.autos_sent.clear();
            }
            self.writer.submit(job)?;
            self.autos_sent.push(info.clone());
            return Ok(info);
        }
        self.writer.flush();
        let info = store.add_version(number, name, kind, p)?;
        self.write_manifest()?;
        Ok(info)
    }

    /// A named version of the song as it is now.
    pub fn save_version(&mut self, name: &str) -> CmdResult<VersionInfo> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CmdError::invalid("バージョン名を入力してください"));
        }
        let cur = self.current.clone();
        self.new_version(name, VersionKind::Named, &cur)
    }

    /// The versions, without waiting for the writer: the UI reads the list after each edit
    /// while a popup is open. Automatic points still on the way are added as the writer will
    /// add them, the oldest past [`AUTO_KEEP`] dropped.
    pub fn versions(&self) -> CmdResult<Vec<VersionInfo>> {
        // Asked before the list is read: none pending then means the list has them all.
        let on_the_way = self.writer.pending();
        let mut all = self.store.as_ref().ok_or_else(no_store)?.list_versions()?;
        if on_the_way {
            for info in &self.autos_sent {
                if !all.iter().any(|v| v.id == info.id) {
                    all.push(info.clone());
                }
            }
            let number = |v: &VersionInfo| v.id[1..].parse::<u64>().unwrap_or(0);
            let mut autos: Vec<u64> = all
                .iter()
                .filter(|v| v.kind == VersionKind::Auto)
                .map(number)
                .collect();
            autos.sort_unstable();
            let cut = autos.len().saturating_sub(AUTO_KEEP);
            let dropped = &autos[..cut];
            all.retain(|v| v.kind != VersionKind::Auto || !dropped.contains(&number(v)));
        }
        Ok(all)
    }

    /// Makes an automatic save point when the song changed since the last one
    /// and the interval has passed. The UI calls this on a timer.
    pub fn autosave_if_due(&mut self, now: u64) -> CmdResult<Option<VersionInfo>> {
        if self.store.is_none() || now < self.last_auto_ms + self.auto_interval_ms {
            return Ok(None);
        }
        let base = self.auto_base.clone().or_else(|| self.saved.clone());
        if base
            .as_ref()
            .is_some_and(|b| Arc::ptr_eq(b, &self.current) || **b == *self.current)
        {
            self.last_auto_ms = now;
            return Ok(None);
        }
        let cur = self.current.clone();
        let info = self.new_version("自動保存", VersionKind::Auto, &cur)?;
        self.auto_base = Some(cur);
        self.last_auto_ms = now;
        Ok(Some(info))
    }

    /// Brings a version back as a new history step, so it can be undone.
    pub fn restore_version(&mut self, id: &str, author: &Author) -> CmdResult<(u64, ChangeSet)> {
        let store = self.store().cloned().ok_or_else(no_store)?;
        let mut p = store.read_version(id)?;
        p.next_id = p.next_id.max(self.hist.id_floor);
        let changes = diff(&self.current, &p);
        if changes.is_empty() {
            return Err(CmdError::new(
                ErrorCode::NothingToDo,
                format!("バージョン {id} は現在の状態と同じです"),
            ));
        }
        let (n, _) = self.push_step(
            p,
            "version.restore",
            json!({ "id": id }),
            author,
            changes.summary(),
        );
        self.prune_selection();
        Ok((n, changes))
    }

    /// The SHA-256 of `project.json` when it is no longer the file this session last read or
    /// wrote, that is, when another process saved the song since (UI-289). `None` when it is
    /// the same file, or the song has no folder or no such file.
    ///
    /// The file is read and hashed only when its size or modification time is not what it
    /// was when it was last found to be that file (PRJ-025): a session held open checks it
    /// before every command, and a large song takes long to hash. A file whose time changed
    /// with the same bytes (touched, or written back as it was) is still the same file. The
    /// size and time are trusted only when they were read [`STAMP_SETTLES`] after the file was
    /// written, so a write in the same tick of a coarse clock, with the same size, is not
    /// missed.
    pub fn disk_changed(&mut self) -> CmdResult<Option<String>> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let looked = std::time::SystemTime::now();
        let stamp = store.project_stamp()?;
        if stamp.is_some() && stamp == self.disk_stamp {
            return Ok(None);
        }
        self.disk_hashes += 1;
        let hash = store.project_hash()?;
        if hash.is_some() && hash == self.disk_hash {
            self.disk_stamp = stamp.filter(|s| {
                s.modified
                    .and_then(|m| looked.duration_since(m).ok())
                    .is_some_and(|age| age >= STAMP_SETTLES)
            });
            return Ok(None);
        }
        Ok(hash.filter(|h| self.disk_hash.as_deref() != Some(h.as_str())))
    }

    /// How many times [`Session::disk_changed`] has read and hashed `project.json`, to tell
    /// that a check of a file that has not changed does not (PRJ-025).
    pub fn disk_hashes(&self) -> u64 {
        self.disk_hashes
    }

    /// `project.json` is now the file of `hash`; its size and time are read anew the next
    /// time it is checked.
    fn set_disk_hash(&mut self, hash: Option<String>) {
        self.disk_hash = hash;
        self.disk_stamp = None;
    }

    /// Takes `project.json` as another process left it as one history step, `project.reload`,
    /// so undo goes back to before it (UI-289). The song then counts as saved: it is the file.
    /// `None` when there was nothing to take: the file is still the one this session last read
    /// or wrote, or it holds the song as it is now (then only the file is remembered).
    pub fn reload_from_disk(&mut self, author: &Author) -> CmdResult<Option<(u64, ChangeSet)>> {
        let store = self.store().cloned().ok_or_else(no_store)?;
        let (mut p, hash) = store.read_project_hashed()?;
        if self.disk_hash.as_deref() == Some(hash.as_str()) {
            return Ok(None);
        }
        p.next_id = p.next_id.max(self.hist.id_floor);
        let changes = diff(&self.current, &p);
        self.set_disk_hash(Some(hash));
        if changes.is_empty() {
            self.saved_step = self.hist.cursor;
            self.saved = Some(self.current.clone());
            self.write_manifest()?;
            return Ok(None);
        }
        let (n, _) = self.push_step(p, "project.reload", json!({}), author, changes.summary());
        self.prune_selection();
        self.saved_step = n;
        self.saved = Some(self.current.clone());
        self.write_manifest()?;
        Ok(Some((n, changes)))
    }

    /// For a session held open across commands (the MCP server, PRJ-024): with nothing unsaved,
    /// what another process (the screen) saved to `project.json` since is taken as one step,
    /// [`Session::reload_from_disk`], before the next command, as a command-line run opening
    /// the song does. With unsaved steps the song stays as it is; the AI's save then refuses to
    /// write over the file (AI-018).
    pub fn catch_up_with_disk(&mut self) -> CmdResult<Option<(u64, ChangeSet)>> {
        if self.saved_step != self.hist.cursor || self.disk_changed()?.is_none() {
            return Ok(None);
        }
        self.reload_from_disk(&Author::Human)
    }

    /// Opens the `.dawproject` at `path` as a new song not saved yet, in place of this one
    /// (EXP-006); its audio goes to `media`. Returns what the song could not take, with the
    /// plugins an extension says this PC does not have.
    pub fn open_dawproject(
        &mut self,
        path: &Path,
        media: &Path,
    ) -> CmdResult<crate::dawproject::Imported> {
        let mut got = crate::dawproject::import(path, media).map_err(CmdError::invalid)?;
        let exts = &mut self.extensions;
        crate::dawproject::not_here(&got.project, &mut got.skipped, &mut |x| {
            exts.iter_mut().find_map(|e| e.has_plugin(x))
        });
        let mut other = Session::with_registry(got.project.clone(), self.registry.clone());
        other.extensions = std::mem::take(&mut self.extensions);
        self.hand_writer_to(&mut other);
        std::mem::swap(self, &mut other);
        Ok(got)
    }

    /// The song that takes this one's place writes the way this one did, and is handed the
    /// errors not yet taken.
    fn hand_writer_to(&mut self, other: &mut Session) {
        self.writer.flush();
        std::mem::swap(&mut self.writer, &mut other.writer);
        other.hash_people = self.hash_people;
    }

    /// Opens a version as a separate song in a new folder; this session stays
    /// as it is.
    pub fn open_version_as_song(&self, id: &str, dir: impl AsRef<Path>) -> CmdResult<Session> {
        let store = self.store().ok_or_else(no_store)?;
        let mut p = store.read_version(id)?;
        let info = store.list_versions()?.into_iter().find(|v| v.id == id);
        if let Some(i) = info {
            p.name = format!("{}（{}）", p.name, i.name);
        }
        let mut s = Session::with_registry(p, self.registry.clone());
        s.ai = self.ai;
        s.attach_new_store(dir.as_ref(), false)?;
        Ok(s)
    }

    // ---- commands the session runs itself ------------------------------------

    fn run_session(
        &mut self,
        def: &CommandDef,
        args: &Args,
        opts: &RunOptions,
        dry: bool,
    ) -> CmdResult<RunResult> {
        let name = def.name;
        let plain = |msg: String| RunResult::new(name, Output::msg(msg), ChangeSet::default());
        match name {
            "history.list" => {
                let mut rows = self.history();
                let skip = args
                    .opt_i64("limit")
                    .map_or(0, |l| rows.len().saturating_sub(l.max(1) as usize));
                rows.drain(..skip);
                let data = json!({ "cursor": self.hist.cursor, "rows": rows });
                Ok(RunResult::new(
                    name,
                    Output::with(
                        format!(
                            "{} 手順（現在は {}）",
                            self.hist.steps.len(),
                            self.hist.cursor
                        ),
                        data,
                    ),
                    ChangeSet::default(),
                ))
            }
            "history.undo" | "history.redo" | "history.jump" => {
                let to = match name {
                    "history.undo" => self.undo_target(args.opt_i64("count").unwrap_or(1) as u64)?,
                    "history.redo" => self.redo_target(args.opt_i64("count").unwrap_or(1) as u64)?,
                    _ => {
                        let n = args.i64("step")?;
                        if n < 0 {
                            return Err(CmdError::range("手順は 0 以上で指定してください"));
                        }
                        n as u64
                    }
                };
                // Renaming a file back or again needs what renaming it needs (BRW-013).
                if !dry
                    && self.moves_disk(to)
                    && let Some(rename) = self.registry.get("file.rename")
                    && self.gate(rename, &opts.author)? != Gate::Allow
                {
                    return Err(CmdError::new(
                        ErrorCode::Denied,
                        format!(
                            "ファイルの名前の変更を含むため、{name} は現在の AI の権限（{}）では実行できません",
                            self.ai.as_str()
                        ),
                    ));
                }
                let renamed = self.disk_renames(to);
                let changes = if dry {
                    let t = self.state_at(to)?;
                    diff(&self.current, &t)
                } else {
                    self.jump(to)?
                };
                let verb = match name {
                    "history.undo" => "戻しました",
                    "history.redo" => "進めました",
                    _ => "移動しました",
                };
                let mut data = json!({ "cursor": to });
                if !renamed.is_empty() {
                    data["renamed"] = json!(renamed);
                }
                let mut r = RunResult::new(
                    name,
                    Output::with(format!("手順 {to} に{verb}"), data),
                    changes,
                );
                if dry {
                    r.message = format!("手順 {to} に移動した場合の変更内容");
                }
                Ok(r)
            }
            "project.save" => {
                if dry {
                    return Ok(plain(format!(
                        "{} に保存します",
                        self.dir()
                            .map_or("（保存先なし）".into(), |d| d.display().to_string())
                    )));
                }
                if opts.author.is_ai() {
                    let (got, done) = self.save_over_collecting()?;
                    return Ok(RunResult::new(
                        name,
                        Output::with(
                            format!(
                                "保存しました{}（上書き前のファイルの控え: {}）",
                                got.say(),
                                done.backup
                            ),
                            json!({
                                "backup": done.backup,
                                "file_hash": {"before": done.before, "after": done.after},
                            }),
                        ),
                        ChangeSet::default(),
                    ));
                }
                let got = self.save_collecting()?;
                Ok(plain(format!("保存しました{}", got.say())))
            }
            "project.save-as" => {
                let path = args.str("path")?.to_string();
                if dry {
                    return Ok(plain(format!("{path} に名前を付けて保存します")));
                }
                let got = self.save_as_collecting(&path)?;
                Ok(RunResult::new(
                    name,
                    Output::with(
                        format!("{path} に保存しました{}", got.say()),
                        json!({ "path": path, "copied": got.copied, "missing": got.missing }),
                    ),
                    ChangeSet::default(),
                ))
            }
            "project.open" => {
                let path = args.str("path")?.to_string();
                if crate::dawproject::is_dawproject(Path::new(&path)) {
                    if dry {
                        return Ok(plain(format!("{path} を新しい曲として開きます")));
                    }
                    let media = args
                        .opt_str("media")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| media_dir_for(Path::new(&path)));
                    let got = self.open_dawproject(Path::new(&path), &media)?;
                    let n = got.skipped.len();
                    let more = if n > 0 {
                        format!("（読み込めなかったもの {n}）")
                    } else {
                        String::new()
                    };
                    return Ok(RunResult::new(
                        name,
                        Output::with(
                            format!("{path} を新しい曲として開きました{more}"),
                            json!({
                                "path": path,
                                "name": self.current.name,
                                "media": media.display().to_string(),
                                "skipped": got.skipped.to_json(),
                            }),
                        ),
                        ChangeSet::default(),
                    ));
                }
                if dry {
                    return Ok(plain(format!("{path} を開きます")));
                }
                // The song being left is on the disk before another (or the same) is read.
                self.writer.flush();
                // The same song opened again keeps its lane (PRJ-023).
                let same = self
                    .store
                    .as_ref()
                    .is_some_and(|s| same_path(&s.dir, Path::new(&path)));
                let mut lane = if same { self.lane.take() } else { None };
                let mut other =
                    match Session::open_inner(Path::new(&path), self.registry.clone(), &mut lane) {
                        Ok(other) => other,
                        Err(e) => {
                            if same {
                                self.lane = lane;
                            }
                            return Err(e);
                        }
                    };
                other.extensions = std::mem::take(&mut self.extensions);
                self.hand_writer_to(&mut other);
                std::mem::swap(self, &mut other);
                Ok(plain(format!("{path} を開きました")))
            }
            "version.save" => {
                let n = args.str("name")?.to_string();
                if dry {
                    return Ok(plain(format!("バージョン「{n}」を作成します")));
                }
                let v = self.save_version(&n)?;
                Ok(RunResult::new(
                    name,
                    Output::with(format!("バージョン {} を作成しました", v.id), json!(v)),
                    ChangeSet::default(),
                ))
            }
            "version.list" => {
                let vs = self.versions()?;
                Ok(RunResult::new(
                    name,
                    Output::with(format!("{} 件", vs.len()), json!(vs)),
                    ChangeSet::default(),
                ))
            }
            "version.restore" => {
                let id = args.str("id")?.to_string();
                if dry {
                    let store = self.store().cloned().ok_or_else(no_store)?;
                    let p = store.read_version(&id)?;
                    let changes = diff(&self.current, &p);
                    return Ok(RunResult::new(
                        name,
                        Output::msg(format!("バージョン {id} に戻した場合の変更内容")),
                        changes,
                    ));
                }
                let (n, changes) = self.restore_version(&id, &opts.author)?;
                let mut r = RunResult::new(
                    name,
                    Output::msg(format!(
                        "バージョン {id} に戻しました（手順 {n}。取り消せます）"
                    )),
                    changes,
                );
                r.step = Some(n);
                Ok(r)
            }
            "version.open" => {
                let id = args.str("id")?.to_string();
                let path = args.str("path")?.to_string();
                if dry {
                    return Ok(plain(format!(
                        "バージョン {id} を別の曲として {path} に開きます"
                    )));
                }
                let s = self.open_version_as_song(&id, &path)?;
                drop(s);
                Ok(RunResult::new(
                    name,
                    Output::with(
                        format!("バージョン {id} を別の曲として {path} に作成しました"),
                        json!({ "path": path }),
                    ),
                    ChangeSet::default(),
                ))
            }
            n if n.starts_with("ai.") && n != "ai.permission.get" && n != "ai.permission.set" => {
                self.run_ai_command(def, args, opts, dry)
            }
            "ai.permission.get" => Ok(RunResult::new(
                name,
                Output::with(
                    self.ai.as_str().to_string(),
                    json!({ "level": self.ai.as_str() }),
                ),
                ChangeSet::default(),
            )),
            "ai.permission.set" => {
                let level = AiPermission::parse(args.str("level")?)?;
                if dry {
                    return Ok(plain(format!(
                        "AI の権限を {} に変更します",
                        level.as_str()
                    )));
                }
                self.set_ai_permission(level)?;
                self.notes.push(OpNote {
                    command: name.to_string(),
                    args: json!({ "level": level.as_str() }),
                    author: opts.author.clone(),
                    at_ms: now_ms(),
                    summary: level.as_str().to_string(),
                });
                Ok(plain(format!(
                    "AI の権限を {} に変更しました",
                    level.as_str()
                )))
            }
            _ => self.run_extension(def, args, opts, dry),
        }
    }
}

impl RunResult {
    fn new(command: &str, out: Output, changes: ChangeSet) -> RunResult {
        RunResult {
            command: command.to_string(),
            message: out.message,
            data: out.data,
            changes,
            dry_run: false,
            forced_dry_run: false,
            step: None,
            decision: None,
            events: Vec::new(),
        }
    }
}

/// Where the audio of a `.dawproject` opened without a folder named goes: a folder of its own
/// in the temporary folder. The song names the files there, as it names any file brought in
/// from outside its folder.
pub fn media_dir_for(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map_or_else(|| "song".into(), |s| s.to_string_lossy().into_owned());
    std::env::temp_dir().join("xirius-dawproject").join(format!(
        "{stem}-{}-{}",
        now_ms(),
        std::process::id()
    ))
}

fn no_store() -> CmdError {
    CmdError::conflict("曲がまだ保存されていません（project.save-as で保存先を指定してください）")
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Does an id still point at something in the song?
pub fn exists(p: &Project, id: &AnyId) -> bool {
    match id {
        AnyId::Track(t) => p.tracks.contains_key(t),
        AnyId::Folder(f) => p.folders.contains_key(f),
        AnyId::Clip(c) => p.find_clip(*c).is_some(),
        AnyId::Note(n) => p
            .tracks
            .values()
            .any(|t| t.clips.iter().any(|c| c.notes().iter().any(|x| x.id == *n))),
        AnyId::Plugin(x) => p
            .tracks
            .values()
            .any(|t| t.inserts.iter().any(|i| i.id == *x)),
        AnyId::Send(x) => p
            .tracks
            .values()
            .any(|t| t.sends.iter().any(|i| i.id == *x)),
        AnyId::Section(x) => p.sections.iter().any(|s| s.id == *x),
        AnyId::Marker(x) => p.markers.iter().any(|s| s.id == *x),
        AnyId::Tempo(x) => p.tempo.iter().any(|s| s.id == *x),
        AnyId::TimeSig(x) => p.timesig.iter().any(|s| s.id == *x),
    }
}
