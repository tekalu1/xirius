//! Running a command against a song folder and keeping the folder up to date:
//! the saved song, and the selection that lives next to it. The command line
//! and the MCP server share this, so an edit from either lands the same way.

use serde_json::{Map, Value};
use xirius_core::id::AnyId;
use xirius_core::session::{RunOptions, RunResult, exists};
use xirius_core::{AiPermission, CmdError, Risk, Session};

/// The selection lives only in a session, but each command-line run is a new
/// session. So `select.*` is kept in a small file next to the song, and the
/// next run starts with it (ids that no longer exist are dropped). It is not
/// part of the song: saving, versions and history never see it.
const SELECTION_FILE: &str = "cli-selection.json";

pub fn load_selection(s: &mut Session) {
    let Some(dir) = s.dir() else { return };
    let Ok(text) = std::fs::read_to_string(dir.join(SELECTION_FILE)) else {
        return;
    };
    let Ok(ids) = serde_json::from_str::<Vec<AnyId>>(&text) else {
        return;
    };
    let p = s.project_arc();
    s.state.selection = ids.into_iter().filter(|i| exists(&p, i)).collect();
}

pub fn store_selection(s: &Session) {
    let Some(dir) = s.dir() else { return };
    let file = dir.join(SELECTION_FILE);
    if s.state.selection.is_empty() {
        let _ = std::fs::remove_file(&file);
        return;
    }
    let Ok(text) = serde_json::to_string(&s.state.selection) else {
        return;
    };
    let tmp = dir.join(format!("{SELECTION_FILE}.tmp"));
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &file);
    }
}

/// Runs a command and, if the song changed, saves it. A person's edit always
/// saves; an AI's only when it holds the `save` permission (its steps are in
/// the history either way, so nothing is lost).
pub fn run_and_persist(
    s: &mut Session,
    name: &str,
    raw: &Map<String, Value>,
    opts: &RunOptions,
    save: bool,
) -> Result<RunResult, CmdError> {
    let risk = s.registry().get(name).map(|d| d.risk);
    let before = s.state.selection.clone();
    let r = s.run(name, raw, opts)?;
    if s.state.selection != before {
        store_selection(s);
    }
    let may_save = !opts.author.is_ai() || s.ai_permission() == AiPermission::Save;
    if save
        && may_save
        && !r.dry_run
        && r.decision.is_none()
        && risk.is_some_and(|k| k != Risk::Read)
        && s.dir().is_some()
        && s.is_dirty()
    {
        s.save()?;
    }
    Ok(r)
}

/// A run result as the JSON both front ends print.
pub fn result_json(r: &RunResult) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("ok".into(), Value::Bool(true));
        if r.decision.is_some() {
            // The command did not run. It waits for a person's answer.
            m.insert("confirmation_required".into(), Value::Bool(true));
        }
    }
    v
}
