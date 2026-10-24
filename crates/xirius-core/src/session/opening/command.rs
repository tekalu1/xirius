//! Validated command context travels with the worker, including one-time approvals.
use super::*;
use crate::session::{RunOptions, RunResult};
use crate::{Output, diff::ChangeSet, model::Project};
use serde_json::{Map, Value, json};

#[derive(Clone)]
pub struct PreparedOpen {
    pub request: OpenRequest,
    command: String,
    raw: Map<String, Value>,
    opts: RunOptions,
    before: Arc<Project>,
    persist: bool,
    pub(crate) decision: Option<String>,
}
impl PreparedOpen {
    pub(crate) fn new(
        session: &Session,
        request: OpenRequest,
        command: &str,
        raw: Map<String, Value>,
        opts: &RunOptions,
    ) -> Self {
        Self {
            request,
            command: command.into(),
            raw,
            opts: opts.clone(),
            before: session.current.clone(),
            persist: false,
            decision: None,
        }
    }
    /// The window connection has the same optional persistence as CLI/MCP.
    pub fn persist_after(&mut self, on: bool) {
        self.persist = on;
    }
    /// Failure before worker ownership (e.g. cancellation during state collection).
    pub fn fail(self, session: &mut Session, error: CmdError) -> CmdResult<RunResult> {
        self.finish(session, &Err(error), None, Vec::new())
    }
    pub(super) fn finish(
        self,
        session: &mut Session,
        outcome: &CmdResult<()>,
        skipped: Option<&crate::dawproject::Skips>,
        events: Vec<Value>,
    ) -> CmdResult<RunResult> {
        let result = outcome
            .clone()
            .map(|()| {
                let path = self.raw.get("path").and_then(Value::as_str).unwrap_or("");
                let output = match &self.request {
                    OpenRequest::CopyVersion { id, .. } => version_output(id, path),
                    OpenRequest::Dawproject { media, .. } => daw_output(
                        path,
                        media,
                        &session.current.name,
                        skipped.expect("successful import"),
                    ),
                    _ if self.command == "project.new" => Output::msg("新しい曲を作成しました"),
                    _ => Output::msg(format!("{path} を開きました")),
                };
                let mut result = RunResult::new(&self.command, output, ChangeSet::default());
                session.after_run(&self.command, &self.raw, &self.opts, &result);
                session.stamp_run_result(
                    &self.command,
                    &self.before,
                    &self.opts,
                    self.decision.is_some(),
                    &mut result,
                );
                result.events = events;
                result.events.extend(session.take_events());
                result
            })
            .and_then(|result| {
                if self.persist
                    && (!self.opts.author.is_ai()
                        || session.ai_permission() == super::super::AiPermission::Save)
                    && session.dir().is_some()
                    && session.is_dirty()
                {
                    session.save()?;
                }
                Ok(result)
            });
        if let Some(token) = self.decision {
            session.record_open_decision(token, result.clone());
        }
        result
    }
}

pub(crate) fn version_output(id: &str, path: &str) -> Output {
    Output::with(
        format!("バージョン {id} を別の曲として {path} に作成しました"),
        json!({"path": path}),
    )
}
pub(crate) fn daw_output(
    path: &str,
    media: &Path,
    name: &str,
    skipped: &crate::dawproject::Skips,
) -> Output {
    let n = skipped.len();
    let more = if n > 0 {
        format!("（読み込めなかったもの {n}）")
    } else {
        String::new()
    };
    Output::with(
        format!("{path} を新しい曲として開きました{more}"),
        json!({"path":path,"name":name,"media":media.display().to_string(),"skipped":skipped.to_json()}),
    )
}
