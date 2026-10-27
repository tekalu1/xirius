//! KEY-015: a macro run from the screen. Each step goes the way its key would, so a step that
//! works on the selection takes the one on the screen, and the steps are one step of the history.

use xirius_prefs::cmdline;

use crate::studio::{Geometry, Studio};

pub(crate) struct Run {
    steps: std::collections::VecDeque<String>,
    owner: u64,
    failed: Option<String>,
}

impl Studio {
    /// Runs the macro `name`. Stops at the first step that fails and says why.
    pub(crate) fn run_macro(&mut self, g: &Geometry, name: &str) -> bool {
        if self.macro_run.is_some() || self.macro_stepping || self.input_blocked() {
            self.error = Some("マクロの実行中か、曲が別の処理中です".into());
            return true;
        }
        let steps = {
            let shared = self.song.settings().clone();
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            let s = shared.lock().unwrap();
            s.macros.get(name).map(|m| m.steps.clone())
        };
        let Some(steps) = steps else {
            self.error = Some(format!("「{name}」というマクロはありません"));
            return true;
        };
        if steps.is_empty() {
            self.error = Some(format!("マクロ「{name}」に手順がありません"));
            return true;
        }
        self.error = None;
        // A wheel's step that is still open must not be closed with the macro's group (P3-7 of
        // review 15).
        self.close_wheel_group();
        self.viewport_size.set(g.size);
        self.song.session_mut().begin_group();
        self.macro_run = Some(Run {
            steps: steps.into(),
            owner: self.song.session().uses_id(),
            failed: None,
        });
        self.poll_macro();
        true
    }

    /// Continue only after the previous operation has returned the actual Session.
    pub(crate) fn poll_macro(&mut self) -> bool {
        if self.input_blocked()
            || self.job.is_some()
            || self.dawproject.is_some()
            || self.file_request.is_some()
        {
            return false;
        }
        let Some(mut run) = self.macro_run.take() else {
            return false;
        };
        self.macro_stepping = true;
        if let Some(reason) = run.failed.take() {
            self.error = Some(reason);
        }
        if self.song.session().uses_id() != run.owner {
            self.error = Some("曲が替わったためマクロを停止しました".into());
        }
        while self.error.is_none() {
            let Some(line) = run.steps.pop_front() else {
                break;
            };
            let (command, args) = match cmdline::parse(&line, Some(self.song.registry())) {
                Ok(v) => v,
                Err(e) => {
                    self.error = Some(e);
                    break;
                }
            };
            let command = self.song.registry().canonical(&command).to_string();
            let (w, h) = self.viewport_size.get();
            let g = self.geometry(w, h);
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.run_parsed(&g, command, args);
            if self.input_blocked()
                || self.job.is_some()
                || self.dawproject.is_some()
                || self.file_request.is_some()
            {
                self.macro_run = Some(run);
                self.macro_stepping = false;
                return true;
            }
            if self.error.is_some() {
                break;
            }
        }
        self.song.session_mut().end_group();
        self.macro_stepping = false;
        true
    }

    pub(crate) fn fail_macro(&mut self, reason: String) {
        if let Some(run) = &mut self.macro_run {
            run.failed = Some(reason);
        }
    }
}
