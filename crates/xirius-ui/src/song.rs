//! The song the screen shows, and the one place that talks to `xirius-core`.
//!
//! The view never edits a `Project`. A press on M or S becomes a core command
//! (`track.mute` / `track.solo`) run through a `Session`, so it lands in the
//! history and Ctrl+Z / Ctrl+Shift+Z walk it back. The view then reads
//! `Session::project_arc()` again.
//!
//! The sample song is built with core commands too (no model structs are
//! filled in by hand), then given a fresh history so that undo starts at the
//! first thing a person does, not at the setup.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::Project;
use xirius_core::session::RunOptions;
use xirius_core::store::VersionKind;
use xirius_core::time::Tick;
use xirius_core::{Args, CmdError, CmdResult, CommandDef, ExtContext, Extension, Output, Session};
use xirius_design::{Theme, TrackColor};
use xirius_plugins::ScanOptions;
use xirius_session::cleanup::{self, Trash};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::host::SessionHost;
use xirius_session::output::OutputMode;

use crate::theme::THEME;

pub const SAMPLE_NAME: &str = "夜の散歩";

/// The name of a new song with no tracks.
pub const EMPTY_NAME: &str = "新しい曲";
/// How a history row the AI made begins (AI-030), as core writes it.
pub const AI_ROW: &str = "AI · ";
const BAR: Tick = 4 * 960;
const BEAT: Tick = 960;

/// `#rrggbb` of a palette colour, the way core stores a track colour.
pub fn hex(color: TrackColor) -> String {
    format!("#{:06x}", color.content(THEME).to_rgb_u32())
}

/// The palette colour a core `#rrggbb` stands for (either theme), if any.
pub fn palette_color(hex: &str) -> Option<TrackColor> {
    let value = u32::from_str_radix(hex.strip_prefix('#')?, 16).ok()?;
    TrackColor::all().find(|c| {
        [Theme::Dark, Theme::Light]
            .iter()
            .any(|t| c.content(*t).to_rgb_u32() == value)
    })
}

/// One state in the history, as the history popover lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryItem {
    /// The state's number: `history.jump` takes it.
    pub n: u64,
    pub label: String,
    /// The core command that made it (empty for the opening state).
    pub command: String,
    /// Made by the AI rather than by the person.
    pub ai: bool,
    /// The state the song is at now.
    pub current: bool,
    /// Undone; redo would bring it back.
    pub undone: bool,
    /// What the command was run with: the `xirius …` line of the operation.
    pub args: Value,
    /// What it changed, in words (`ノート 12 件`).
    pub summary: String,
    pub at_ms: u64,
    /// A command that left no step (a change of the AI's permission): it cannot be jumped to.
    pub note: bool,
}

/// The `n` of the first line that is no step (`HistoryItem::note`); the lines count up from it.
pub const NOTE_N: u64 = 1 << 48;

/// A saved version of the song, as the version list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionItem {
    pub id: String,
    pub name: String,
    /// Named by the person (a save point made on a timer is not).
    pub named: bool,
    pub at_ms: u64,
}

/// What a version holds, against the song as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionDetail {
    pub tracks: usize,
    pub length_bars: u32,
    /// Tracks that differ from now (or exist in only one of the two).
    pub changed: Vec<TrackId>,
    /// Colour (`#rrggbb`) of each track in the version.
    pub colors: Vec<(TrackId, Option<String>)>,
}

/// A path as the text a command takes (forward slashes, so it reads the same
/// everywhere).
fn path_text(p: &Path) -> String {
    p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

pub struct Song {
    session: Session,
    /// The list `plugin.rescan` keeps.
    plugin_cache: PathBuf,
    /// The user's settings, answered by the session's `prefs.*` / `keymap.*` /
    /// `mousemap.*` commands. In memory until [`Song::load_settings`].
    settings: xirius_prefs::Shared,
    pub io: xirius_session::audio_settings::Shared,
    /// Kept in a folder of the app's own (not one the person chose) so that its
    /// steps survive a crash until it is saved somewhere (PRJ-001).
    draft: bool,
    /// The sample song the app builds, not one from a folder (UI-261).
    sample: bool,
    /// Where `project.cleanup` and an export that replaces files move them (`None`: the
    /// session's own choice); a new song in this one's place moves them to the same place.
    trash: Option<Trash>,
    /// The screen's commands (`screen_ops`) a run over the session asked for (KEY-012): an AI's
    /// over MCP. The window runs them on its next frame ([`crate::studio::Studio::run_asked`]).
    asked: Arc<Mutex<Asked>>,
}

/// The screen's commands asked of the session, waiting for the window.
#[derive(Default)]
pub(crate) struct Asked {
    pub list: Vec<(String, Map<String, Value>)>,
    /// The window runs them now: one asked again from there is not the session's to queue.
    pub busy: bool,
}

/// Answers the screen's commands for the session: it keeps them for the window, which has the
/// screen they change (KEY-012). Without it, the session says they are not there. Which
/// commands those are is the table's mark (`screen_only`), not a list kept here.
struct ScreenExt {
    asked: Arc<Mutex<Asked>>,
    registry: Arc<xirius_core::Registry>,
}

impl Extension for ScreenExt {
    fn answers(&self, name: &str) -> bool {
        self.registry.get(name).is_some_and(|def| def.screen_only)
    }

    fn has_screen(&self) -> bool {
        true
    }

    fn run(
        &mut self,
        _cx: &ExtContext<'_>,
        def: &CommandDef,
        args: &Args,
        dry: bool,
    ) -> CmdResult<Output> {
        let mut asked = self.asked.lock().unwrap_or_else(|e| e.into_inner());
        if asked.busy {
            return Err(CmdError::invalid(format!(
                "{} は画面で実行できませんでした",
                def.name
            )));
        }
        if !dry {
            asked.list.push((def.name.to_string(), args.0.clone()));
        }
        Ok(Output::msg("画面で実行します"))
    }
}

impl Song {
    pub fn project(&self) -> Arc<Project> {
        self.session.project_arc()
    }

    /// Goes down as a killed process does (for the tests and the screenshots): the writes on
    /// the way are made, but nothing is closed, so the song's lock stays.
    #[doc(hidden)]
    pub fn simulate_crash(self) {
        let mut song = std::mem::ManuallyDrop::new(self);
        std::mem::replace(&mut song.session, Session::new("")).simulate_crash();
    }

    /// The session, for what reads its state directly (the AI's marks).
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// The session, for the runs the AI asks for over MCP (they carry the
    /// AI as their author, which [`Song::run`] does not).
    pub(crate) fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Changes the song in place, leaving no history step (`Session::amend`):
    /// the screen's own state asking, not the person's edit. Returns the change
    /// it made (empty when the song was already so).
    pub(crate) fn amend(
        &mut self,
        f: impl FnOnce(&mut Project),
    ) -> Result<xirius_core::diff::ChangeSet, String> {
        self.session.amend(f).map_err(|e| e.to_string())
    }

    pub(crate) fn take_session_for_save(&mut self) -> Session {
        let preview = self.session.save_preview();
        std::mem::replace(&mut self.session, preview)
    }

    pub(crate) fn finish_session_save(&mut self, session: Session, saved_as: bool) {
        self.session = session;
        if saved_as {
            self.draft = false;
        }
    }

    pub(crate) fn autosave_checked(&mut self, now: u64) -> xirius_core::error::CmdResult<bool> {
        self.session.autosave_if_due(now).map(|v| v.is_some())
    }

    pub fn toggle_mute(&mut self, id: TrackId) -> Result<(), String> {
        self.toggle("track.mute", id)
    }

    pub fn toggle_solo(&mut self, id: TrackId) -> Result<(), String> {
        self.toggle("track.solo", id)
    }

    fn toggle(&mut self, command: &str, id: TrackId) -> Result<(), String> {
        self.session
            .run_value(
                command,
                json!({ "ids": [id.to_string()] }),
                &RunOptions::human(),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Runs one core command as a person (one history step) and returns its data.
    pub fn run(&mut self, command: &str, args: Value) -> Result<Value, String> {
        self.session
            .run_value(command, args, &RunOptions::human())
            .map(|r| r.data)
            .map_err(|e| e.to_string())
    }

    /// Stops the recording on `host` and puts its takes into the song as one
    /// step of the history (REC-016). Returns the clips it made.
    pub fn finish_recording(&mut self, host: &mut SessionHost) -> Result<Value, String> {
        xirius_session::record::finish_recording(
            &mut self.session,
            host,
            &RunOptions::human(),
            xirius_session::record::STOP_WAIT,
        )
        .map(|r| r.data)
        .map_err(|e| e.to_string())
    }

    /// Takes what was turned in the plugins' own windows into the song, as one step of the
    /// history (PLG-027). True when the song changed.
    pub fn collect_plugin_edits(&mut self, host: &mut SessionHost) -> Result<bool, String> {
        xirius_session::host::collect_plugin_states(&mut self.session, host, &RunOptions::human())
            .map(|c| c.result.is_some())
            .map_err(|e| e.to_string())
    }

    /// [`Song::collect_plugin_edits`] after asking every plugin for its state: what a plugin
    /// changed in its window without saying so is in the song too (PLG-057). Before the song
    /// is written or left.
    pub fn pull_plugin_states(&mut self, host: &mut SessionHost) -> Result<bool, String> {
        xirius_session::host::pull_plugin_states(
            &mut self.session,
            host,
            &RunOptions::human(),
            xirius_session::host::PULL_WAIT,
        )
        .map(|c| c.result.is_some())
        .map_err(|e| e.to_string())
    }

    /// Steps the person has made (the setup is not counted).
    pub fn steps(&self) -> usize {
        self.session.steps().len()
    }

    pub fn can_undo(&self) -> bool {
        self.session.can_undo()
    }

    /// Every operation the song can run: the one list the menus, the keys,
    /// the CLI, MCP and the command palette share.
    pub fn registry(&self) -> &xirius_core::command::Registry {
        self.session.registry()
    }

    /// Whether a person could run the command with these arguments now,
    /// without running it (`dry_run`): `Ok(false)` when it would change
    /// nothing. The error's code tells a value that is missing
    /// (`InvalidArgs`) from the song's state.
    pub fn check(
        &mut self,
        command: &str,
        mut args: Value,
    ) -> Result<bool, xirius_core::error::CmdError> {
        args["dry_run"] = json!(true);
        self.session
            .run_value(command, args, &RunOptions::human())
            .map(|r| !r.changes.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        self.session.can_redo()
    }

    /// Returns whether anything was undone.
    pub fn undo(&mut self) -> bool {
        matches!(self.step_history(false), Ok(Some(_)))
    }

    /// Returns whether anything was redone.
    pub fn redo(&mut self) -> bool {
        matches!(self.step_history(true), Ok(Some(_)))
    }

    /// One step back or forward: `Ok(None)` when there is none, what the command returned
    /// (with the files it renamed, BRW-013) when it moved, the error when the history cannot
    /// move (a renamed file the disk cannot rename back).
    pub fn step_history(&mut self, redo: bool) -> Result<Option<Value>, String> {
        let (can, command) = if redo {
            (self.can_redo(), "history.redo")
        } else {
            (self.can_undo(), "history.undo")
        };
        if !can {
            return Ok(None);
        }
        self.run(command, json!({})).map(Some)
    }

    /// The label of the state the song is at (the newest step not undone).
    pub fn current_label(&self) -> Option<String> {
        self.session
            .history()
            .into_iter()
            .find(|row| row.current && !row.command.is_empty())
            .map(|row| row.label)
    }

    // ------------------------------------------------ history, versions, files

    /// Runs a core command as the person, whatever it returns.
    fn run_human(&mut self, command: &str, args: Value) -> Result<(), String> {
        self.session
            .run_value(command, args, &RunOptions::human())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Every state the song has been in, oldest first. Row 0 is how it was
    /// when it was opened.
    pub fn history(&self) -> Vec<HistoryItem> {
        self.session
            .history()
            .into_iter()
            .map(|row| HistoryItem {
                n: row.n,
                label: if row.n == 0 {
                    "開いた時点".to_string()
                } else if row.author.is_ai() {
                    // AI-030: the AI's rows say so, with the same friendly name.
                    let name = if row.label == row.author.row_label(&row.command) {
                        self.friendly(&row.command)
                    } else {
                        row.label.clone()
                    };
                    if name.starts_with(AI_ROW) {
                        name
                    } else {
                        format!("{AI_ROW}{name}")
                    }
                } else if row.label == row.author.row_label(&row.command) {
                    self.friendly(&row.command)
                } else {
                    row.label
                },
                command: row.command,
                ai: row.author.is_ai(),
                current: row.current,
                undone: row.undone,
                args: row.args,
                summary: row.summary,
                at_ms: row.at_ms,
                note: false,
            })
            .collect()
    }

    /// The commands that left no step, oldest first (a change of the AI's permission).
    pub fn history_notes(&self) -> Vec<HistoryItem> {
        self.session
            .op_notes()
            .iter()
            .enumerate()
            .map(|(i, note)| {
                let ai = note.author.is_ai();
                let name = self.friendly(&note.command);
                HistoryItem {
                    n: NOTE_N + i as u64,
                    label: if ai { format!("{AI_ROW}{name}") } else { name },
                    command: note.command.clone(),
                    ai,
                    current: false,
                    undone: false,
                    args: note.args.clone(),
                    summary: xirius_core::AiPermission::parse(&note.summary).map_or_else(
                        |_| note.summary.clone(),
                        |p| crate::ai::level(p).name.to_string(),
                    ),
                    at_ms: note.at_ms,
                    note: true,
                }
            })
            .collect()
    }

    /// A command's name in the history, as a person would say it.
    pub(crate) fn friendly(&self, command: &str) -> String {
        match command {
            "track.mute" => "ミュートを切り替え".to_string(),
            "track.solo" => "ソロを切り替え".to_string(),
            "track.arm" => "録音準備を切り替え".to_string(),
            "track.color.set" => "トラックの色を変更".to_string(),
            "track.output.set" => "出力先を変更".to_string(),
            "folder.bus.add" => "フォルダーを新しいバスに接続".to_string(),
            "version.restore" => "バージョンを復元".to_string(),
            "project.reload" => "ほかのアプリの変更".to_string(),
            "record.take.add" => "録音".to_string(),
            "track.input.set" => "入力を変更".to_string(),
            "plugin.isolation.set" => "プラグインの動かし方を変更".to_string(),
            "ai.permission.set" => "AI の権限を変更".to_string(),
            _ => self.session.registry().get(command).map_or_else(
                || "編集".to_string(),
                |def| {
                    def.summary
                        .split(['（', '('])
                        .next()
                        .unwrap_or("編集")
                        .trim()
                        .to_string()
                },
            ),
        }
    }

    /// `history.jump`: go to state `n`, forward or back.
    pub fn jump(&mut self, n: u64) -> Result<(), String> {
        self.run_human("history.jump", json!({ "step": n }))
    }

    pub fn grid(&self) -> &str {
        self.grid_for(&self.session.state.view.editor)
    }

    pub fn snap(&self) -> bool {
        self.snap_for(&self.session.state.view.editor)
    }

    pub fn set_grid(&mut self, grid: &str) -> Result<(), String> {
        self.run_human(
            "view.grid",
            json!({ "grid": grid, "editor":self.session.state.view.editor }),
        )
    }

    pub fn set_snap(&mut self, on: bool) -> Result<(), String> {
        let arg = if on {
            json!({ "on": true })
        } else {
            json!({ "off": true })
        };
        let mut arg = arg;
        arg["editor"] = json!(self.session.state.view.editor);
        self.run_human("view.snap", arg)
    }

    pub fn grid_for(&self, editor: &str) -> &str {
        if editor == "roll" {
            &self.session.state.view.roll.grid
        } else {
            &self.session.state.view.grid
        }
    }

    pub fn snap_for(&self, editor: &str) -> bool {
        if editor == "roll" {
            self.session.state.view.roll.snap
        } else {
            self.session.state.view.snap
        }
    }

    /// Where the song is kept, once it has been saved. A draft has no place of
    /// the person's choosing yet.
    pub fn dir(&self) -> Option<PathBuf> {
        if self.draft {
            return None;
        }
        self.session.dir().map(Path::to_path_buf)
    }

    /// Kept on disk and not changed since.
    pub fn is_saved(&self) -> bool {
        !self.draft && self.session.dir().is_some() && !self.session.is_dirty()
    }

    /// Where the song's record is, saved by the person or kept as a draft.
    pub fn record_dir(&self) -> Option<PathBuf> {
        self.session.dir().map(Path::to_path_buf)
    }

    pub fn is_draft(&self) -> bool {
        self.draft
    }

    /// The sample song the app builds: the next start builds it again (UI-261).
    pub fn is_sample(&self) -> bool {
        self.sample
    }

    /// Starts recording a song nobody has saved yet in `dir` (PRJ-001).
    pub fn keep_draft(&mut self, dir: &Path) -> Result<(), String> {
        self.keep_draft_checked(dir).map_err(|e| e.to_string())
    }

    pub(crate) fn keep_draft_checked(&mut self, dir: &Path) -> xirius_core::error::CmdResult<()> {
        self.session.keep_as_draft(dir)?;
        self.draft = true;
        Ok(())
    }

    /// Marks the song just opened from `dir` as a draft, when `dir` is one.
    pub fn mark_draft(&mut self, draft: bool) {
        self.draft = draft;
    }

    /// What came back when the song was opened after a crash (PRJ-012).
    pub fn recovery(&self) -> Option<&xirius_core::recovery::Recovery> {
        self.session.recovery()
    }

    /// Goes back to the state last saved, leaving the later steps to redo.
    pub fn discard_unsaved(&mut self) -> Result<(), String> {
        self.session.discard_unsaved().map_err(|e| e.to_string())
    }

    pub fn clear_recovery(&mut self) {
        self.session.clear_recovery();
    }

    /// Lets go of the song's folder (the window closes), so the lock it holds is removed.
    pub fn release(&mut self) {
        self.session = Session::new(EMPTY_NAME);
    }

    /// Are the edits still being written to the song's folder (PRJ-022)?
    pub fn writes_pending(&self) -> bool {
        self.session.writes_pending()
    }

    /// The writes to the song's folder that failed since the last call.
    pub fn take_write_errors(&self) -> Vec<String> {
        self.session.take_write_errors()
    }

    /// Waits until the edits are written to the song's folder.
    pub fn flush_writes(&self) {
        self.session.flush_writes();
    }

    /// Each write to the song's folder waits `delay` first, as a slow disk would.
    #[cfg(test)]
    pub fn simulate_slow_disk(&self, delay: std::time::Duration) {
        self.session.simulate_slow_disk(delay);
    }

    /// The automatic save point, when it is due (PRJ-009). Returns whether one was made.
    pub fn autosave(&mut self, now_ms: u64) -> Result<bool, String> {
        self.session
            .autosave_if_due(now_ms)
            .map(|v| v.is_some())
            .map_err(|e| e.to_string())
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.run_human("project.save", json!({}))
    }

    pub fn save_as(&mut self, dir: &Path) -> Result<(), String> {
        self.run_human("project.save-as", json!({ "path": path_text(dir) }))?;
        self.draft = false;
        Ok(())
    }

    /// `project.open`: this song is replaced by the one in `dir`.
    pub fn open(&mut self, dir: &Path) -> Result<(), String> {
        self.run_human("project.open", json!({ "path": path_text(dir) }))?;
        self.draft = false;
        self.sample = false;
        Ok(())
    }

    /// The versions kept beside the song, newest first. Empty when it has none
    /// (or has not been saved yet).
    pub fn versions(&self) -> Vec<VersionItem> {
        let Ok(mut all) = self.session.versions() else {
            return Vec::new();
        };
        all.sort_by_key(|v| std::cmp::Reverse((v.created_ms, v.id.clone())));
        all.into_iter()
            .map(|v| VersionItem {
                named: v.kind == VersionKind::Named,
                id: v.id,
                name: v.name,
                at_ms: v.created_ms,
            })
            .collect()
    }

    /// `version.save`: a named version of the song as it is now.
    pub fn save_version(&mut self, name: &str) -> Result<(), String> {
        self.run_human("version.save", json!({ "name": name }))
    }

    /// What a version holds, set against the song as it is now.
    pub fn version_detail(&self, id: &str) -> Option<VersionDetail> {
        let p = self.session.store()?.read_version(id).ok()?;
        let now = self.session.project_arc();
        let ordered = |p: &Project| crate::draw::ordered_tracks(p).len();
        let changed = p
            .tracks
            .values()
            .filter(|t| now.tracks.get(&t.id).is_none_or(|n| **n != ***t))
            .map(|t| t.id)
            .chain(
                now.tracks
                    .keys()
                    .filter(|k| !p.tracks.contains_key(k))
                    .copied(),
            )
            .collect();
        let length = p
            .tracks
            .values()
            .flat_map(|t| t.clips.iter().map(|c| c.start + c.length))
            .chain(p.sections.iter().map(|s| s.start + s.length))
            .max()
            .unwrap_or(0);
        Some(VersionDetail {
            tracks: ordered(&p),
            length_bars: p.time_map().to_bbt(length).0.saturating_sub(1),
            changed,
            colors: p.tracks.values().map(|t| (t.id, t.color.clone())).collect(),
        })
    }

    /// The SHA-256 of `project.json` when another process saved the song since the screen last
    /// read or wrote it (UI-289). A draft is in a folder of the app's own: no one else writes it.
    pub(crate) fn disk_changed(&mut self) -> Option<String> {
        if self.draft {
            return None;
        }
        self.session.disk_changed().ok().flatten()
    }

    /// What another process saved comes in as one step, `project.reload` (UI-289): the step,
    /// none when the file holds the song as it is.
    pub(crate) fn reload_from_disk(&mut self) -> Result<Option<u64>, String> {
        self.session
            .reload_from_disk(&xirius_core::command::Author::Human)
            .map(|step| step.map(|(n, _)| n))
            .map_err(|e| e.to_string())
    }

    /// `version.restore`: the version comes back as a new step (undoable).
    pub fn restore_version(&mut self, id: &str) -> Result<(), String> {
        self.run_human("version.restore", json!({ "id": id }))
    }

    /// `version.open` into `dir`, then open that folder: the version becomes
    /// its own song and this one is left as it was on disk.
    pub fn open_version_as_song(&mut self, id: &str, dir: &Path) -> Result<(), String> {
        self.run_human("version.open", json!({ "id": id, "path": path_text(dir) }))?;
        self.open(dir)
    }

    // ------------------------------------------------------------- tracks

    /// `track.color.set`; `None` takes the colour away.
    pub fn set_color(&mut self, id: TrackId, color: Option<TrackColor>) -> Result<(), String> {
        let color = color.map_or("none".to_string(), hex);
        self.run_human(
            "track.color.set",
            json!({ "ids": [id.to_string()], "color": color }),
        )
    }

    /// `track.output.set`: `default`, `master` or a bus id.
    /// A new track (or folder) of `kind` at the end of the song.
    pub fn add_track(&mut self, kind: &str) -> Result<(), String> {
        let command = match kind {
            "folder" => "folder.add".to_string(),
            _ => format!("track.add.{kind}"),
        };
        self.run_human(&command, json!({}))
    }

    pub fn set_output(&mut self, id: TrackId, to: &str) -> Result<(), String> {
        self.run_human(
            "track.output.set",
            json!({ "ids": [id.to_string()], "to": to }),
        )
    }

    pub fn toggle_arm(&mut self, id: TrackId) -> Result<(), String> {
        self.toggle("track.arm", id)
    }

    /// The input monitor of a track: what the input plays on the output while it is armed (REC-011).
    pub fn toggle_monitor(&mut self, id: TrackId) -> Result<(), String> {
        self.toggle("track.monitor", id)
    }

    pub fn sample() -> Song {
        Song::sample_with(crate::mixer::scan::options())
    }

    /// The sample song, with the session's commands (`plugin.rescan`, `export.status`, …)
    /// scanning as `scan` says.
    pub fn sample_with(scan: ScanOptions) -> Song {
        Song::sample_into(scan, default_trash(), None)
    }

    /// The sample song in the place of `old`, with its settings (see [`Song::empty_after`]).
    pub fn sample_after(old: &Song) -> Song {
        Song::sample_into(
            crate::mixer::scan::options(),
            default_trash(),
            Some((old.settings.clone(), old.io.clone())),
        )
    }

    /// A new song with no tracks: the screen shows where to start (UI-074).
    pub fn empty() -> Song {
        Song::start(
            Session::new(EMPTY_NAME),
            crate::mixer::scan::options(),
            default_trash(),
            None,
        )
    }

    /// A new song with no tracks, in the place of `old`: the settings (the theme, the
    /// driver and the device, the keys) and the list of audio devices belong to this PC and the
    /// person using it, not to a song, so they are carried over and not read again.
    pub fn empty_after(old: &Song) -> Song {
        Song::start(
            Session::new(EMPTY_NAME),
            crate::mixer::scan::options(),
            old.trash.clone(),
            Some((old.settings.clone(), old.io.clone())),
        )
    }

    /// `project` (a `.dawproject` read) as a new, unsaved song in the place of `old`, which
    /// carries its settings and device list over as [`Song::empty_after`] does.
    pub fn imported_after(old: &Song, project: Project) -> Song {
        Song::start(
            Session::with_registry(project, xirius_session::commands::shared_registry()),
            crate::mixer::scan::options(),
            old.trash.clone(),
            Some((old.settings.clone(), old.io.clone())),
        )
    }

    /// What moves a file to the recycle bin for this song: the one it was made with, else the
    /// OS's own.
    pub fn trash(&self) -> Trash {
        self.trash
            .clone()
            .unwrap_or_else(|| Arc::new(xirius_session::os::trash))
    }

    /// The sample song, whose `project.cleanup` moves files with `trash`.
    #[cfg(test)]
    pub fn sample_with_trash(trash: Trash) -> Song {
        Song::sample_into(crate::mixer::scan::options(), Some(trash), None)
    }

    fn sample_into(
        scan: ScanOptions,
        trash: Option<Trash>,
        carried: Option<(xirius_prefs::Shared, xirius_session::audio_settings::Shared)>,
    ) -> Song {
        let mut b = Builder {
            session: Session::new(SAMPLE_NAME),
        };
        b.build();
        let mut song = Song::start(b.session, scan, trash, carried);
        song.sample = true;
        song
    }

    /// The screen's song from `built`, with an empty history. `carried` is the settings and the
    /// device list of the song it takes the place of (none: this is the first, in memory).
    fn start(
        built: Session,
        scan: ScanOptions,
        trash: Option<Trash>,
        carried: Option<(xirius_prefs::Shared, xirius_session::audio_settings::Shared)>,
    ) -> Song {
        let registry = xirius_session::commands::shared_registry();
        let mut session = Session::with_registry(built.project().clone(), registry.clone());
        let plugin_cache = scan.cache.clone();
        let config = ExtConfig {
            output: OutputMode::None,
            scan,
            catalog: None,
        };
        // The record button records on the screen's own player, which opens
        // the MIDI input itself (`Transport::record`): this extension has no
        // output, so its `record.start` is refused and needs none.
        let (settings, io) = carried.unwrap_or_else(|| {
            (
                xirius_prefs::Settings::memory().shared(),
                std::sync::Arc::new(std::sync::Mutex::new(
                    xirius_session::audio_settings::Io::default(),
                )),
            )
        });
        let extras = ExtExtras {
            io_settings: Some(settings.clone()),
            midi: None,
            midi_out: None,
            trash: trash.clone(),
            track_presets: test_presets_dir(),
        };
        xirius_session::commands::attach_with(&mut session, config, extras);
        // An edit returns before its step reaches the disk, so a slow disk does not stop the
        // window (PRJ-022). Songs opened later take the same writer.
        session.write_in_background(true);
        xirius_prefs::attach(&mut session, settings.clone());
        session.add_extension(Box::new(xirius_session::audio_settings::IoExt {
            io: io.clone(),
            settings: settings.clone(),
        }));
        session.add_extension(Box::new(app_ext(settings.clone())));
        let asked = Arc::new(Mutex::new(Asked::default()));
        session.add_extension(Box::new(ScreenExt {
            asked: asked.clone(),
            registry,
        }));
        Song {
            session,
            plugin_cache,
            settings,
            io,
            draft: false,
            sample: false,
            trash,
            asked,
        }
    }

    /// The screen's commands asked of the session since the last call, and whether the window
    /// runs them now (while it does, the session refuses another).
    pub(crate) fn asked(&self) -> &Arc<Mutex<Asked>> {
        &self.asked
    }

    /// The settings the session's commands change.
    pub fn settings(&self) -> &xirius_prefs::Shared {
        &self.settings
    }

    /// Reads the settings from `dir` (the window does; a test or a script keeps
    /// them in memory). The commands write there from then on.
    pub fn load_settings(&mut self, dir: &Path) {
        let mut read = xirius_prefs::Settings::open(dir);
        if let Ok(mut s) = self.settings.lock() {
            // The screen repaints on a new revision.
            read.rev += s.rev;
            *s = read;
        }
    }

    /// Runs one settings command as a person. The settings are not steps of
    /// the song's history. Returns the command's message.
    pub fn set(&mut self, command: &str, args: Value) -> Result<String, String> {
        self.session
            .run_value(command, args, &RunOptions::human())
            .map(|r| r.message)
            .map_err(|e| e.to_string())
    }

    pub fn plugin_cache(&self) -> &Path {
        &self.plugin_cache
    }

    /// `project.cleanup`: the recordings in the song's folder that nothing
    /// names (the song, its history on disk, its versions) go to the recycle
    /// bin. Returns how many moved.
    pub fn cleanup(&mut self) -> Result<usize, String> {
        self.run(xirius_session::commands::CLEANUP, json!({}))
            .map(|v| v["moved"].as_array().map_or(0, Vec::len))
    }
}

/// Where `project.cleanup` moves files: the folder `XIRIUS_TRASH_DIR` names, else the recycle
/// bin (`None`). A test refuses instead, so it never fills the recycle bin of the PC it runs on.
fn default_trash() -> Option<Trash> {
    if cfg!(test) {
        Some(Arc::new(|p: &Path| {
            Err(format!("試験ではごみ箱に移動しません: {}", p.display()))
        }))
    } else {
        cleanup::trash_from_env()
    }
}

struct Builder {
    session: Session,
}

impl Builder {
    fn run(&mut self, command: &str, args: Value) -> Value {
        match self.session.run_value(command, args, &RunOptions::human()) {
            Ok(r) => r.data,
            Err(e) => panic!("sample song: {command} failed: {e}"),
        }
    }

    fn id(data: &Value) -> String {
        data["id"].as_str().expect("an id").to_string()
    }

    fn folder(&mut self, name: &str, into: &str) -> String {
        Self::id(&self.run("folder.add", json!({ "name": name, "into": into })))
    }

    fn track(&mut self, kind: &str, name: &str, into: &str, color: usize) -> String {
        let id = Self::id(&self.run(
            &format!("track.add.{kind}"),
            json!({ "name": name, "into": into }),
        ));
        self.run(
            "track.color.set",
            json!({ "ids": [id], "color": hex(TrackColor::new(color)) }),
        );
        let icon = match name {
            "Drums" | "Conga" | "Shaker" => "drums",
            "Keys" => "keys",
            "Vocal" => "mic",
            _ => "wave",
        };
        self.run("track.icon.set", json!({ "ids": [id], "icon": icon }));
        id
    }

    fn clip(&mut self, track: &str, bar: u32, bars: f64, name: &str) -> String {
        Self::id(&self.run(
            "clip.add.midi",
            json!({ "track": track, "at": format!("{bar}.1"), "bars": bars, "name": name }),
        ))
    }

    fn audio(&mut self, track: &str, bar: u32, bars: f64, file: &str) -> String {
        Self::id(&self.run(
            "clip.add.audio",
            json!({ "track": track, "at": format!("{bar}.1"), "bars": bars, "file": file }),
        ))
    }

    fn note(&mut self, clip: &str, at: Tick, pitch: u8, length: Tick, velocity: u8) {
        self.run(
            "note.add",
            json!({ "clip": clip, "at": at, "pitch": pitch, "length": length, "velocity": velocity }),
        );
    }

    fn build(&mut self) {
        for (name, at, bars, color) in [
            ("Intro", 1, 4.0, 2),
            ("Verse", 5, 8.0, 7),
            ("Chorus", 13, 8.0, 12),
        ] {
            self.run(
                "section.add",
                json!({ "name": name, "at": format!("{at}.1"), "bars": bars,
                        "color": hex(TrackColor::new(color)) }),
            );
        }

        // Rhythm > (Drums, Bass, Percussion > (Shaker, Latin > Conga))
        let rhythm = self.folder("Rhythm", "root");
        let drums = self.track("instrument", "Drums", &rhythm, 3);
        let bass = self.track("instrument", "Bass", &rhythm, 8);
        let perc = self.folder("Percussion", &rhythm);
        let shaker = self.track("instrument", "Shaker", &perc, 4);
        let latin = self.folder("Latin", &perc);
        let conga = self.track("instrument", "Conga", &latin, 5);
        // Three tracks in a row at the root.
        let keys = self.track("instrument", "Keys", "root", 10);
        let vocal = self.track("audio", "Vocal", "root", 13);
        let guitar = self.track("audio", "Guitar", "root", 1);

        // Drums: kick on 1 and 3, snare on 2 and 4, hats on the eighths.
        for (bar, bars) in [(1, 4.0), (5, 8.0)] {
            let c = self.clip(&drums, bar, bars, "Drums");
            for b in 0..(bars as i64) {
                let base = Tick::from(bar - 1) * BAR + b * BAR;
                for beat in 0..4 {
                    let at = base + beat * BEAT;
                    let (pitch, vel) = if beat % 2 == 0 { (36, 112) } else { (38, 104) };
                    self.note(&c, at, pitch, BEAT / 4, vel);
                    self.note(&c, at, 42, BEAT / 4, 78);
                    self.note(&c, at + BEAT / 2, 42, BEAT / 4, 64);
                }
            }
        }

        let c = self.clip(&bass, 5, 4.0, "Bass");
        let line = [36u8, 36, 43, 41, 38, 38, 45, 43];
        for b in 0..4i64 {
            for (i, pitch) in line.iter().enumerate() {
                let at = 4 * BAR + b * BAR + (i as i64) * BEAT / 2;
                self.note(&c, at, *pitch, BEAT / 2 - 60, 100);
            }
        }

        let c = self.clip(&shaker, 5, 4.0, "Shaker");
        for b in 0..4i64 {
            for i in 0..8i64 {
                self.note(
                    &c,
                    4 * BAR + b * BAR + i * BEAT / 2,
                    70,
                    BEAT / 4,
                    60 + (i % 2) as u8 * 20,
                );
            }
        }
        let c = self.clip(&conga, 9, 4.0, "Conga");
        for b in 0..4i64 {
            for (i, pitch) in [62u8, 63, 63, 62, 64, 63].iter().enumerate() {
                self.note(
                    &c,
                    8 * BAR + b * BAR + (i as i64) * 2 * BEAT / 3,
                    *pitch,
                    BEAT / 3,
                    90,
                );
            }
        }

        let c = self.clip(&keys, 5, 4.0, "Keys");
        let chords: [[u8; 3]; 4] = [[60, 64, 67], [57, 60, 64], [53, 57, 60], [55, 59, 62]];
        for (i, chord) in chords.iter().enumerate() {
            for pitch in chord {
                self.note(&c, 4 * BAR + (i as i64) * BAR, *pitch, BAR - 120, 92);
            }
        }

        self.audio(&vocal, 6, 6.0, "vocal_take1.wav");
        self.audio(&guitar, 9, 5.0, "guitar_di.wav");
    }
}

/// Where the tests keep track presets: a folder of their own for each song, so a test never
/// reads the presets of the person running it, nor those of another test.
#[cfg(test)]
fn test_presets_dir() -> Option<PathBuf> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    Some(std::env::temp_dir().join(format!(
        "xirius-ui-track-presets-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
}

#[cfg(not(test))]
fn test_presets_dir() -> Option<PathBuf> {
    None
}

/// `song.recent` and `app.update.check` (UI-261, UI-262): the app's folder and the system's
/// `curl`; a test's read a folder of its own and never reach the network.
#[cfg(not(test))]
fn app_ext(settings: xirius_prefs::Shared) -> xirius_session::app::AppExt {
    xirius_session::app::AppExt::standard(settings)
}

#[cfg(test)]
fn app_ext(settings: xirius_prefs::Shared) -> xirius_session::app::AppExt {
    let root = test_presets_dir()
        .expect("a test's folder")
        .with_extension("app");
    xirius_session::app::AppExt {
        root,
        settings,
        fetch: std::sync::Arc::new(|_: &str| Err("試験はネットワークに出ない".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::id::Node;
    use xirius_core::model::TrackKind;

    fn track_named(p: &Project, name: &str) -> TrackId {
        p.tracks
            .values()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no track {name}"))
            .id
    }

    #[test]
    fn the_sample_song_has_the_tracks_and_folders_of_the_mock() {
        let song = Song::sample();
        let p = song.project();
        assert_eq!(p.name, SAMPLE_NAME);
        for name in [
            "Drums", "Bass", "Shaker", "Conga", "Keys", "Vocal", "Guitar",
        ] {
            track_named(&p, name);
        }
        assert_eq!(p.folders.len(), 3);
        assert!(matches!(p.root[0], Node::Folder(_)));
        assert!(matches!(p.root[1], Node::Track(_)));
        assert_eq!(p.root.len(), 4);
        assert_eq!(p.sections.len(), 3);
        assert_eq!(p.tracks[&track_named(&p, "Vocal")].kind, TrackKind::Audio);
    }

    #[test]
    fn folders_nest_three_deep() {
        let p = Song::sample().project();
        let Node::Folder(rhythm) = p.root[0] else {
            panic!("a folder first")
        };
        let perc = p.folders[&rhythm]
            .children
            .iter()
            .find_map(|n| match n {
                Node::Folder(f) => Some(*f),
                Node::Track(_) => None,
            })
            .expect("Percussion in Rhythm");
        let latin = p.folders[&perc]
            .children
            .iter()
            .find_map(|n| match n {
                Node::Folder(f) => Some(*f),
                Node::Track(_) => None,
            })
            .expect("Latin in Percussion");
        assert_eq!(p.folders[&latin].children.len(), 1);
    }

    #[test]
    fn track_colours_come_from_the_palette() {
        let p = Song::sample().project();
        for t in p.tracks.values().filter(|t| t.kind != TrackKind::Master) {
            let hex = t.color.as_deref().expect("a colour");
            assert!(palette_color(hex).is_some(), "{} has {hex}", t.name);
        }
        assert_eq!(
            palette_color(&hex(TrackColor::new(5))),
            Some(TrackColor::new(5))
        );
        assert_eq!(palette_color("nonsense"), None);
        for s in &p.sections {
            assert!(palette_color(s.color.as_deref().expect("section colour")).is_some());
        }
    }

    #[test]
    fn clips_hold_notes_and_the_audio_ones_a_file() {
        let p = Song::sample().project();
        let drums = &p.tracks[&track_named(&p, "Drums")];
        assert_eq!(drums.clips.len(), 2);
        assert!(drums.clips[0].notes().len() > 20);
        let vocal = &p.tracks[&track_named(&p, "Vocal")];
        assert!(!vocal.clips[0].is_midi());
    }

    #[test]
    fn setup_is_not_in_the_history() {
        let song = Song::sample();
        assert_eq!(song.steps(), 0);
        assert!(!song.can_undo());
        assert!(song.current_label().is_none());
    }

    #[test]
    fn human_and_ai_edits_share_the_same_history_and_undo_cursor() {
        let mut song = Song::sample();
        let id = track_named(&song.project(), "Drums");
        song.toggle_mute(id).unwrap();
        song.session
            .run_value(
                "track.solo",
                json!({"ids": [id.to_string()]}),
                &RunOptions::ai("Assistant"),
            )
            .unwrap();
        let rows = song.history();
        assert_eq!(rows.len(), 3);
        assert!(!rows[1].ai);
        assert_ne!(rows[1].label, "track.mute");
        assert_ne!(rows[2].label, "track.solo");
        assert_eq!(rows[1].label, "ミュートを切り替え");
        assert_eq!(rows[2].label, "AI · ソロを切り替え");
        assert!(rows[2].ai && rows[2].current);
        assert!(song.undo());
        assert!(song.history()[1].current);
        assert!(song.history()[2].undone);
        assert!(song.project().tracks[&id].mute);
        assert!(!song.project().tracks[&id].solo);
    }

    #[test]
    fn mute_runs_a_core_command_lands_in_history_and_undoes() {
        let mut song = Song::sample();
        let id = track_named(&song.project(), "Bass");
        assert!(!song.project().tracks[&id].mute);

        song.toggle_mute(id).unwrap();
        assert!(song.project().tracks[&id].mute);
        assert_eq!(song.steps(), 1);
        assert!(song.can_undo());
        assert!(song.current_label().is_some());

        assert!(song.undo());
        assert!(!song.project().tracks[&id].mute);
        assert!(!song.can_undo() && song.can_redo());

        assert!(song.redo());
        assert!(song.project().tracks[&id].mute);
    }

    #[test]
    fn solo_is_its_own_command_and_leaves_mute_alone() {
        let mut song = Song::sample();
        let id = track_named(&song.project(), "Keys");
        song.toggle_solo(id).unwrap();
        let t = song.project().tracks[&id].clone();
        assert!(t.solo && !t.mute);
        song.toggle_solo(id).unwrap();
        assert!(!song.project().tracks[&id].solo);
        assert_eq!(song.steps(), 2);
    }

    #[test]
    fn undo_with_nothing_to_undo_does_nothing() {
        let mut song = Song::sample();
        assert!(!song.undo());
        assert!(!song.redo());
    }

    #[test]
    fn a_new_edit_after_undo_drops_the_redo() {
        let mut song = Song::sample();
        let a = track_named(&song.project(), "Drums");
        let b = track_named(&song.project(), "Vocal");
        song.toggle_mute(a).unwrap();
        song.undo();
        song.toggle_solo(b).unwrap();
        assert!(!song.can_redo());
        assert!(!song.project().tracks[&a].mute);
        assert!(song.project().tracks[&b].solo);
    }
}
