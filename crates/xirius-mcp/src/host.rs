//! Where the [`Gateway`](crate::Gateway) runs its commands. The command line
//! owns its session behind a mutex; the app owns its song on the screen's
//! thread, so it takes the runs as jobs over a channel ([`bridge`]) and
//! answers each one there (AI-019: the AI is a client on the other end, the
//! app only runs what it asks).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use xirius_core::Registry;
use xirius_core::session::{RunOptions, RunResult};
use xirius_core::{CmdError, ErrorCode, Session};

use crate::gateway::lock;
use crate::persist::run_and_persist;

/// A song the gateway can reach: its command table, and a way to run one
/// command on it (with the folder kept up to date, as [`run_and_persist`]).
pub trait Host: Send + Sync {
    /// Calls `f` with the command table.
    fn with_registry(&self, f: &mut dyn FnMut(&Registry));

    fn run(
        &self,
        command: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
        save: bool,
    ) -> Result<RunResult, CmdError>;
}

impl Host for Mutex<Session> {
    fn with_registry(&self, f: &mut dyn FnMut(&Registry)) {
        f(lock(self).registry());
    }

    fn run(
        &self,
        command: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
        save: bool,
    ) -> Result<RunResult, CmdError> {
        let mut s = lock(self);
        let reloaded = catch_up(&mut s)?;
        let mut r = run_and_persist(&mut s, command, raw, opts, save)?;
        if let Some(event) = reloaded {
            r.events.insert(0, event);
        }
        Ok(r)
    }
}

/// The event a run tells with when it first took what another process saved.
pub const RELOAD_EVENT: &str = "project.reload";

/// The session stays open across commands, so what another process (the screen) saved to
/// `project.json` since the last one comes in first as one step, 「ほかのアプリの変更」
/// (PRJ-024), as a command-line run that opens the song takes it. The run then answers on that
/// song, and `history.undo` takes the step back. The event says so.
fn catch_up(s: &mut Session) -> Result<Option<Value>, CmdError> {
    Ok(s.catch_up_with_disk()?.map(|(step, changes)| {
        json!({
            "event": RELOAD_EVENT,
            "step": step,
            "message": format!(
                "ほかのアプリが保存した曲を、履歴の手順 {step}「ほかのアプリの変更」として取り込んでから実行しました"
            ),
            "changes": changes,
        })
    }))
}

/// One run the app is asked to do. Answer it with [`Job::answer`]; a job
/// dropped unanswered tells the caller the app has closed.
pub struct Job {
    pub command: String,
    pub raw: Map<String, Value>,
    pub opts: RunOptions,
    pub save: bool,
    reply: mpsc::Sender<Result<RunResult, CmdError>>,
}

impl Job {
    pub fn answer(self, result: Result<RunResult, CmdError>) {
        let _ = self.reply.send(result);
    }
}

/// Where the runs of a [`bridge`] arrive, one [`Job`] each.
pub type Jobs = UnboundedReceiver<Job>;

/// The gateway's end of [`bridge`]: each run becomes a [`Job`] and waits for
/// its answer. The calls come from blocking threads (the server runs them in
/// `spawn_blocking`), so waiting here holds no async worker.
pub struct Bridge {
    registry: Arc<Registry>,
    jobs: UnboundedSender<Job>,
}

/// A host whose runs arrive at the returned receiver. `registry` is the
/// command table of the song that answers them.
pub fn bridge(registry: Arc<Registry>) -> (Arc<Bridge>, Jobs) {
    let (jobs, rx) = unbounded_channel();
    (Arc::new(Bridge { registry, jobs }), rx)
}

impl Host for Bridge {
    fn with_registry(&self, f: &mut dyn FnMut(&Registry)) {
        f(&self.registry);
    }

    fn run(
        &self,
        command: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
        save: bool,
    ) -> Result<RunResult, CmdError> {
        let closed = || CmdError::new(ErrorCode::Io, "Xirius の画面が閉じられています");
        let (reply, answer) = mpsc::channel();
        let job = Job {
            command: command.to_string(),
            raw: raw.clone(),
            opts: opts.clone(),
            save,
            reply,
        };
        self.jobs.send(job).map_err(|_| closed())?;
        answer.recv().map_err(|_| closed())?
    }
}
