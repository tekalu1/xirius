//! The AI's work on the song (AI-003, AI-005, AI-006, AI-012, AI-014 to AI-017).
//!
//! - A **request** groups what the AI does for one ask into one history step
//!   (`ai.request.begin` ... `ai.request.end`, or `ai.batch` in one go).
//! - A request can run on a **work copy** (`draft`): the AI edits a private copy
//!   of the song and the song itself does not move until a person accepts the
//!   copy (one step, undoable) or throws it away.
//! - The AI's deleting and saving can be held for a person's **answer**: the
//!   run returns a `Decision` with a one-time token that is good for 120 s; the
//!   answer is a command only a person may run (`ai.decide`).
//! - `AiActivity` says which tracks, clips and notes the AI is editing now and
//!   which it touched last, in a form the UI reads to make them glow.
//!
//! All of it lives in the session, so the UI, the CLI and MCP see the same
//! state. None of it is saved to the song folder.

use super::*;

#[cfg(test)]
#[path = "work/opening_tests.rs"]
mod opening_tests;

/// How long a one-time answer token lives (AI-015).
pub const DECISION_TTL_MS: u64 = 120_000;
/// How long the AI's edit counts as "now" for the glow when no request is open.
pub const LIVE_MS: u64 = 1_500;
/// Open questions kept at most.
const MAX_PENDING: usize = 32;
/// Marks kept at most (the oldest go first).
const MAX_MARKS: usize = 5_000;

/// What the AI may not do without a person's answer (OPEN-04: "edit only";
/// deleting and saving ask first).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Confirm {
    /// Removing something (`Risk::Destructive`).
    pub destructive: bool,
    /// Saving, when the AI's permission stops short of `save` (`Risk::Guarded`).
    pub save: bool,
}

impl Confirm {
    /// The recommended default: ask for both.
    pub const OPEN_04: Confirm = Confirm {
        destructive: true,
        save: true,
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestMode {
    /// Edits the song as it goes; the steps are folded into one at the end.
    Direct,
    /// Edits a work copy; a person brings it into the song or drops it.
    Draft,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    Open,
    /// The AI is done with the work copy; it waits for a person.
    Review,
}

/// What to do when the song changed after the work copy was made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnConflict {
    /// Refuse; the person decides what to do.
    Reject,
    /// Put the work copy in anyway as one step (undoing it brings back what
    /// was there).
    Replace,
}

impl OnConflict {
    pub fn parse(s: &str) -> CmdResult<OnConflict> {
        match s {
            "reject" => Ok(OnConflict::Reject),
            "replace" => Ok(OnConflict::Replace),
            _ => Err(CmdError::invalid(
                "on_conflict には reject か replace を指定してください",
            )),
        }
    }
}

/// A track, clip, note, ... by kind and id.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct Target {
    pub kind: String,
    pub id: String,
}

/// What the UI reads to make the AI's work visible.
#[derive(Clone, Debug, Default, Serialize)]
pub struct AiActivity {
    /// A request is open: the AI is at work.
    pub active: bool,
    /// What the AI's latest edits touched (the glow runs here while live).
    pub editing: Vec<Target>,
    /// What the AI touched and a person has not touched since (the purple edge).
    pub marks: Vec<Target>,
    pub updated_ms: u64,
}

impl AiActivity {
    /// Should the glow run now?
    pub fn is_live(&self, now_ms: u64) -> bool {
        !self.editing.is_empty()
            && (self.active || now_ms.saturating_sub(self.updated_ms) < LIVE_MS)
    }

    fn add(list: &mut Vec<Target>, t: Target) {
        if !list.contains(&t) {
            list.push(t);
        }
    }

    fn forget(&mut self, ids: &[String]) {
        self.marks.retain(|t| !ids.contains(&t.id));
        self.editing.retain(|t| !ids.contains(&t.id));
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RequestInfo {
    pub id: u64,
    pub title: String,
    pub author: Author,
    pub mode: RequestMode,
    pub state: RequestState,
    /// Commands run so far.
    pub commands: usize,
    pub started_ms: u64,
    /// For a work copy: what accepting it would change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<ChangeSet>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RequestOutcome {
    pub title: String,
    pub mode: RequestMode,
    /// `done`, `review`, `accepted` or `reverted`.
    pub result: &'static str,
    /// The history step the request made (none when nothing changed).
    pub step: Option<u64>,
    pub commands: usize,
    pub changes: ChangeSet,
}

/// A question for a person (AI-017).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
    /// One-time token for `ai.decide`.
    pub token: String,
    pub command: String,
    pub args: Value,
    pub author: Author,
    pub question: String,
    /// What happens when nobody answers.
    pub default_answer: String,
    pub created_ms: u64,
    pub expires_ms: u64,
}

struct Request {
    id: u64,
    title: String,
    author: Author,
    mode: RequestMode,
    state: RequestState,
    started_ms: u64,
    /// History state the request started at.
    base_n: u64,
    base: Arc<Project>,
    commands: Vec<Value>,
    touched: Vec<Target>,
    /// The work copy, for a draft.
    scratch: Option<Box<Session>>,
}

#[derive(Default)]
pub(super) struct AiWork {
    pub(super) confirm: Confirm,
    req: Option<Request>,
    activity: AiActivity,
    pending: Vec<Decision>,
    completed_decisions: Vec<(String, CmdResult<RunResult>)>,
    next_req: u64,
    token_seq: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Gate {
    Allow,
    /// Look only: the run is made as a dry run.
    DryOnly,
    /// Ask a person first.
    Confirm,
}

fn author_name(a: &Author) -> String {
    match a {
        Author::Human => "ユーザー".into(),
        Author::Ai(n) => n.clone(),
    }
}

fn targets_of(changes: &ChangeSet) -> (Vec<Target>, Vec<String>) {
    let mut live = Vec::new();
    let mut gone = Vec::new();
    for c in &changes.items {
        if c.change == crate::diff::ChangeKind::Removed {
            gone.push(c.id.clone());
        } else if c.kind != "project" {
            live.push(Target {
                kind: c.kind.clone(),
                id: c.id.clone(),
            });
        }
    }
    (live, gone)
}

impl Session {
    // ---- reading -----------------------------------------------------------------

    pub fn confirm(&self) -> Confirm {
        self.ai_work.confirm
    }

    /// Chooses what the AI's runs must ask a person about.
    pub fn set_confirm(&mut self, c: Confirm) {
        self.ai_work.confirm = c;
    }

    pub fn ai_activity(&self) -> &AiActivity {
        &self.ai_work.activity
    }

    pub fn request_info(&self) -> Option<RequestInfo> {
        self.ai_work.req.as_ref().map(|r| self.info_of(r, false))
    }

    /// As `request_info`, with what accepting a work copy would change.
    pub fn request_status(&self) -> Option<RequestInfo> {
        self.ai_work.req.as_ref().map(|r| self.info_of(r, true))
    }

    fn info_of(&self, r: &Request, with_changes: bool) -> RequestInfo {
        let changes = match (&r.scratch, with_changes) {
            (Some(s), true) => Some(diff(&self.current, &s.current)),
            _ => None,
        };
        RequestInfo {
            id: r.id,
            title: r.title.clone(),
            author: r.author.clone(),
            mode: r.mode,
            state: r.state,
            commands: r.commands.len(),
            started_ms: r.started_ms,
            changes,
        }
    }

    /// The work copy, to draw next to (or instead of) the song.
    pub fn draft_project(&self) -> Option<Arc<Project>> {
        self.ai_work
            .req
            .as_ref()
            .and_then(|r| r.scratch.as_ref())
            .map(|s| s.current.clone())
    }

    /// Questions waiting for a person (the ones that ran out of time are dropped).
    pub fn pending_decisions(&mut self) -> Vec<Decision> {
        self.purge_decisions(now_ms());
        self.ai_work.pending.clone()
    }

    /// A person touched these ids: the AI's marks on them go (AI-006).
    pub fn human_touched(&mut self, ids: &[String]) {
        self.ai_work.activity.marks.retain(|t| !ids.contains(&t.id));
    }

    // ---- requests ----------------------------------------------------------------

    pub fn begin_request(
        &mut self,
        author: &Author,
        title: &str,
        mode: RequestMode,
    ) -> CmdResult<RequestInfo> {
        let title = title.trim();
        if title.is_empty() {
            return Err(CmdError::invalid("依頼のタイトルを入力してください"));
        }
        if let Some(r) = &self.ai_work.req {
            let state = match r.state {
                RequestState::Open => "進行中",
                RequestState::Review => "確認待ち",
            };
            return Err(CmdError::conflict(format!(
                "「{}」（{}）が{state}です。AI の依頼は同時に 1 件しか実行できません",
                r.title,
                author_name(&r.author)
            )));
        }
        let scratch = (mode == RequestMode::Draft).then(|| Box::new(self.make_scratch()));
        self.ai_work.next_req += 1;
        let req = Request {
            id: self.ai_work.next_req,
            title: title.to_string(),
            author: author.clone(),
            mode,
            state: RequestState::Open,
            started_ms: now_ms(),
            base_n: self.hist.cursor,
            base: self.current.clone(),
            commands: Vec::new(),
            touched: Vec::new(),
            scratch,
        };
        let info = self.info_of(&req, false);
        self.ai_work.req = Some(req);
        let a = &mut self.ai_work.activity;
        a.active = true;
        a.editing.clear();
        a.updated_ms = now_ms();
        Ok(info)
    }

    /// The AI is done. A direct request is folded into one step; a work copy
    /// waits for a person.
    pub fn end_request(&mut self) -> CmdResult<RequestOutcome> {
        let Some(mut req) = self.ai_work.req.take() else {
            return Err(CmdError::new(
                ErrorCode::NothingToDo,
                "終了する依頼がありません",
            ));
        };
        match req.mode {
            RequestMode::Direct => self.finish_direct(req, "done"),
            RequestMode::Draft => {
                if req.state == RequestState::Review {
                    self.ai_work.req = Some(req);
                    return Err(CmdError::new(
                        ErrorCode::NothingToDo,
                        "作業コピーは確認待ちです（ユーザーが取り込むか破棄するまでお待ちください）",
                    ));
                }
                req.state = RequestState::Review;
                let changes = req
                    .scratch
                    .as_ref()
                    .map_or_else(ChangeSet::default, |s| diff(&self.current, &s.current));
                let out = RequestOutcome {
                    title: req.title.clone(),
                    mode: req.mode,
                    result: "review",
                    step: None,
                    commands: req.commands.len(),
                    changes,
                };
                self.ai_work.req = Some(req);
                self.settle();
                Ok(out)
            }
        }
    }

    /// A person takes the AI's work into the song: a work copy becomes one
    /// step; a direct request just ends.
    pub fn accept_request(&mut self, on_conflict: OnConflict) -> CmdResult<RequestOutcome> {
        let Some(req) = self.ai_work.req.take() else {
            return Err(CmdError::new(
                ErrorCode::NothingToDo,
                "取り込む依頼がありません",
            ));
        };
        if req.mode == RequestMode::Direct {
            return self.finish_direct(req, "accepted");
        }
        let Some(scratch) = req.scratch.as_ref() else {
            return Err(CmdError::new(ErrorCode::Corrupt, "作業コピーがありません"));
        };
        let moved = !(Arc::ptr_eq(&req.base, &self.current) || *req.base == *self.current);
        if moved && on_conflict == OnConflict::Reject {
            let msg = format!(
                "作業コピーの作成後に曲が変更されています。作業コピーで置き換える（on_conflict=replace。\
                 取り消せます）か、破棄して依頼し直してください（「{}」）",
                req.title
            );
            self.ai_work.req = Some(req);
            return Err(CmdError::conflict(msg));
        }
        let mut fin = (*scratch.current).clone();
        fin.next_id = fin.next_id.max(self.hist.id_floor);
        let changes = diff(&self.current, &fin);
        let n_commands = req.commands.len();
        let mut step = None;
        if !changes.is_empty() {
            let summary = format!("{}（作業コピー）{}", req.title, changes.summary());
            let args = json!({"title": req.title, "draft": true, "commands": req.commands});
            let (n, _) = self.push_step(fin, "ai.request", args, &req.author, summary);
            step = Some(n);
            self.prune_selection();
        }
        self.settle();
        Ok(RequestOutcome {
            title: req.title,
            mode: req.mode,
            result: "accepted",
            step,
            commands: n_commands,
            changes,
        })
    }

    /// A person throws the AI's work away: the work copy is dropped; a direct
    /// request is rewound to where it began.
    pub fn revert_request(&mut self) -> CmdResult<RequestOutcome> {
        let Some(req) = self.ai_work.req.take() else {
            return Err(CmdError::new(
                ErrorCode::NothingToDo,
                "破棄する依頼がありません",
            ));
        };
        let changes = match (&req.scratch, req.mode) {
            (Some(s), _) => diff(&self.current, &s.current),
            (None, _) => diff(&req.base, &self.current),
        };
        if req.mode == RequestMode::Direct && self.hist.cursor != req.base_n {
            if !self.only_by(req.base_n, &req.author) {
                let msg =
                    "途中にユーザーの操作があるため、自動では戻せません（履歴から戻してください）";
                self.ai_work.req = Some(req);
                return Err(CmdError::conflict(msg));
            }
            if let Err(e) = self.rewind_to(req.base_n) {
                self.ai_work.req = Some(req);
                return Err(e);
            }
        }
        let a = &mut self.ai_work.activity;
        a.marks.retain(|t| !req.touched.contains(t));
        self.settle();
        Ok(RequestOutcome {
            title: req.title,
            mode: req.mode,
            result: "reverted",
            step: None,
            commands: req.commands.len(),
            changes,
        })
    }

    /// The request is over: nothing is "being edited" any more.
    fn settle(&mut self) {
        let a = &mut self.ai_work.activity;
        a.active = false;
        a.editing.clear();
        a.updated_ms = now_ms();
    }

    fn make_scratch(&self) -> Session {
        let mut s = Session::with_registry((*self.current).clone(), self.registry.clone());
        s.hist.id_floor = s.hist.id_floor.max(self.hist.id_floor);
        s.state = self.state.clone();
        s.ai = self.ai;
        s
    }

    /// Were all steps after `n` made by `who`, up to the end of the history?
    fn only_by(&self, n: u64, who: &Author) -> bool {
        n >= self.hist.first
            && self.hist.cursor == self.hist.last()
            && self.hist.steps[(n - self.hist.first) as usize..]
                .iter()
                .all(|s| s.author == *who)
    }

    /// Ends a direct request, folding its steps into one when they are all the
    /// author's.
    fn finish_direct(&mut self, req: Request, result: &'static str) -> CmdResult<RequestOutcome> {
        let changes = diff(&req.base, &self.current);
        let mut step = None;
        if self.hist.cursor > req.base_n {
            step = Some(self.hist.cursor);
            let n_steps = self.hist.cursor - req.base_n;
            if n_steps > 1 && self.only_by(req.base_n, &req.author) {
                let summary = format!(
                    "{}（{} 操作）{}",
                    req.title,
                    req.commands.len(),
                    changes.summary()
                );
                let fin = (*self.current).clone();
                self.hist.cursor = req.base_n;
                let args = json!({"title": req.title, "commands": req.commands});
                let (n, _) = self.push_step(fin, "ai.request", args, &req.author, summary);
                step = Some(n);
            }
        }
        self.settle();
        Ok(RequestOutcome {
            title: req.title,
            mode: req.mode,
            result,
            step,
            commands: req.commands.len(),
            changes,
        })
    }

    /// Makes `n` the end of the history, dropping the steps after it.
    fn rewind_to(&mut self, n: u64) -> CmdResult<()> {
        let target = self.state_at(n)?;
        let old_last = self.hist.last();
        let rel = self.hist.rel(n);
        self.hist.states.truncate(rel + 1);
        self.hist.steps.truncate(rel);
        self.hist.cursor = n;
        self.current = target;
        self.note_uses();
        self.persist_index((n + 1..=old_last).collect());
        self.prune_selection();
        Ok(())
    }

    // ---- who may run -------------------------------------------------------------------

    /// Only one AI works at a time (AI-016): another AI's runs are refused
    /// while a request is open.
    pub(super) fn check_operator(&self, author: &Author) -> CmdResult<()> {
        if let Some(r) = &self.ai_work.req
            && r.author.is_ai()
            && r.author != *author
        {
            return Err(CmdError::conflict(format!(
                "AI「{}」の依頼「{}」が進行中です。終了するまでお待ちください",
                author_name(&r.author),
                r.title
            )));
        }
        Ok(())
    }

    /// While a work copy is open, the AI's runs go to the copy. `None` for
    /// everything else.
    pub(super) fn run_in_work_copy(
        &mut self,
        name: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
    ) -> Option<CmdResult<RunResult>> {
        let req = self.ai_work.req.as_ref()?;
        if req.mode != RequestMode::Draft || !opts.author.is_ai() {
            return None;
        }
        let registry = self.registry.clone();
        let def = registry.get(name)?;
        if def.name.starts_with("ai.") && def.name != "ai.batch" {
            return None;
        }
        Some(self.draft_run(def, raw, opts))
    }

    fn draft_run(
        &mut self,
        def: &CommandDef,
        raw: &Map<String, Value>,
        opts: &RunOptions,
    ) -> CmdResult<RunResult> {
        self.check_operator(&opts.author)?;
        // Off and human-only are refused here; the copy applies "look only".
        self.gate(def, &opts.author)?;
        if def.risk == Risk::Guarded
            || (matches!(def.handler, Handler::Session)
                && !def.name.starts_with("history.")
                && def.name != "ai.batch")
        {
            return Err(CmdError::new(
                ErrorCode::Denied,
                format!("{} は作業コピーの中では使用できません", def.name),
            ));
        }
        let ai = self.ai;
        let req = self.ai_work.req.as_mut().expect("a request");
        if req.state == RequestState::Review {
            return Err(CmdError::conflict(
                "作業コピーは確認待ちです（ユーザーが取り込むか破棄するまで AI は編集できません）",
            ));
        }
        let mut scratch = req.scratch.take().expect("a work copy");
        scratch.ai = ai;
        let r = scratch.run(def.name, raw, opts);
        if let Some(req) = self.ai_work.req.as_mut() {
            req.scratch = Some(scratch);
        }
        let mut r = r?;
        r.message.push_str("（作業コピー）");
        if r.step.take().is_some() {
            self.note_ai_edit(def.name, raw, &r);
        }
        Ok(r)
    }

    // ---- what the AI touched --------------------------------------------------------

    pub(super) fn after_run(
        &mut self,
        name: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
        r: &RunResult,
    ) {
        // The ai.* commands' own results are bookkeeping; what they changed was
        // noted when the edits were made (or is made by `decide`, below).
        if r.dry_run || r.decision.is_some() || name.starts_with("ai.") {
            return;
        }
        if opts.author.is_ai() {
            if r.step.is_some() {
                self.note_ai_edit(name, raw, r);
            }
            return;
        }
        let (live, gone) = targets_of(&r.changes);
        let mut ids: Vec<String> = live.into_iter().map(|t| t.id).collect();
        ids.extend(gone);
        if name.starts_with("select.") {
            let sel: Vec<String> = self
                .state
                .selection
                .iter()
                .map(ToString::to_string)
                .collect();
            ids.extend(sel);
        }
        if !ids.is_empty() {
            self.ai_work.activity.forget(&ids);
        }
    }

    fn note_ai_edit(&mut self, name: &str, raw: &Map<String, Value>, r: &RunResult) {
        let (live, gone) = targets_of(&r.changes);
        let w = &mut self.ai_work;
        let mut args = raw.clone();
        args.remove("dry_run");
        if let Some(req) = w.req.as_mut() {
            req.commands.push(json!({"command": name, "args": args}));
            for t in &live {
                AiActivity::add(&mut req.touched, t.clone());
            }
        }
        let a = &mut w.activity;
        a.forget(&gone);
        if !a.active || w.req.is_none() {
            a.editing.clear();
        }
        for t in live {
            AiActivity::add(&mut a.editing, t.clone());
            AiActivity::add(&mut a.marks, t);
        }
        if a.marks.len() > MAX_MARKS {
            let extra = a.marks.len() - MAX_MARKS;
            a.marks.drain(..extra);
        }
        a.active = w.req.is_some();
        a.updated_ms = now_ms();
    }

    // ---- questions for a person ------------------------------------------------------

    fn purge_decisions(&mut self, now: u64) {
        self.ai_work.pending.retain(|d| now <= d.expires_ms);
    }

    fn new_token(&mut self) -> String {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(now_ms());
        h.write_u64(self.ai_work.token_seq);
        h.write_u32(std::process::id());
        self.ai_work.token_seq += 1;
        format!("dec_{:016x}", h.finish())
    }

    /// The AI's run needs a person's answer: say what it would do and wait.
    pub(super) fn hold(
        &mut self,
        def: &CommandDef,
        raw: &Map<String, Value>,
        opts: &RunOptions,
    ) -> CmdResult<RunResult> {
        // A run that would fail anyway is not worth a question.
        let preview = self.run_inner(def.name, raw, &opts.clone().dry(), true)?;
        let now = now_ms();
        self.purge_decisions(now);
        if self.ai_work.pending.len() >= MAX_PENDING {
            self.ai_work.pending.remove(0);
        }
        let mut args = raw.clone();
        args.remove("dry_run");
        let what = if preview.changes.is_empty() {
            preview.message.clone()
        } else {
            preview.changes.summary()
        };
        let question = format!(
            "{} が {} を実行しようとしています（{what}）。実行を許可しますか？ 回答が無い場合は実行しません",
            author_name(&opts.author),
            def.name
        );
        let d = Decision {
            token: self.new_token(),
            command: def.name.to_string(),
            args: Value::Object(args),
            author: opts.author.clone(),
            question: question.clone(),
            default_answer: "no".into(),
            created_ms: now,
            expires_ms: now + DECISION_TTL_MS,
        };
        self.ai_work.pending.push(d.clone());
        let mut r = RunResult::new(def.name, Output::msg(question), preview.changes);
        r.decision = Some(d);
        Ok(r)
    }

    /// A person's answer to a question. The token is used up first, so it
    /// cannot be used twice; a late answer is refused (AI-015).
    pub fn decide(&mut self, token: &str, yes: bool, now: u64) -> CmdResult<RunResult> {
        let _using = crate::in_use::using();
        if self
            .ai_work
            .completed_decisions
            .iter()
            .any(|(key, _)| key == token)
        {
            return self.decide_inner(token, yes, now);
        }
        let before = self.current.clone();
        let mut result = self.decide_inner(token, yes, now);
        if let Ok(result) = &mut result {
            self.stamp_run_result("ai.decide", &before, &RunOptions::human(), true, result);
        }
        if self.ai_work.completed_decisions.len() >= MAX_PENDING {
            let _ = self.ai_work.completed_decisions.remove(0);
        }
        self.ai_work
            .completed_decisions
            .push((token.into(), result.clone()));
        result
    }

    fn decide_inner(&mut self, token: &str, yes: bool, now: u64) -> CmdResult<RunResult> {
        let Some(i) = self.ai_work.pending.iter().position(|d| d.token == token) else {
            return Err(CmdError::not_found(
                "その確認は見つかりません（回答済みか、期限切れです）",
            ));
        };
        let d = self.ai_work.pending.remove(i);
        if now > d.expires_ms {
            return Err(CmdError::new(
                ErrorCode::Expired,
                format!(
                    "{} の確認は期限切れです。AI からもう一度依頼してください",
                    d.command
                ),
            ));
        }
        if !yes {
            let msg = format!("{} は実行しませんでした", d.command);
            return Ok(RunResult::new(
                "ai.decide",
                Output::with(msg, json!({"accepted": false})),
                ChangeSet::default(),
            ));
        }
        let Value::Object(raw) = d.args else {
            return Err(CmdError::new(
                ErrorCode::Corrupt,
                "確認の引数を読み取れません",
            ));
        };
        let opts = RunOptions {
            author: d.author,
            dry_run: false,
        };
        let r = self.run_inner(&d.command, &raw, &opts, true)?;
        self.after_run(&d.command, &raw, &opts, &r);
        Ok(r)
    }

    // ---- the commands --------------------------------------------------------------

    /// Only a pending open approval is intercepted. Answered tokens retain the
    /// ordinary one-use refusal; ai.decision.get retrieves their final result.
    pub fn is_open_decision(&self, raw: &Map<String, Value>) -> bool {
        let Some(def) = self.registry.get("ai.decide") else {
            return false;
        };
        let Ok(raw) = Registry::spelled(def, raw) else {
            return false;
        };
        let Some(token) = raw.get("token").and_then(Value::as_str) else {
            return false;
        };
        let is_open =
            |command: &str| matches!(command, "project.open" | "version.open" | "project.new");
        self.ai_work
            .pending
            .iter()
            .any(|d| d.token == token && is_open(&d.command))
    }

    pub(super) fn record_open_decision(&mut self, token: String, result: CmdResult<RunResult>) {
        if self.ai_work.completed_decisions.len() >= MAX_PENDING {
            let _ = self.ai_work.completed_decisions.remove(0);
        }
        self.ai_work.completed_decisions.push((token, result));
    }

    pub(super) fn prepare_open_decision(
        &mut self,
        raw: &Map<String, Value>,
        opts: &RunOptions,
    ) -> CmdResult<Result<opening::PreparedOpen, RunResult>> {
        self.prepare_open_decision_at(raw, opts, now_ms())
    }

    fn prepare_open_decision_at(
        &mut self,
        raw: &Map<String, Value>,
        opts: &RunOptions,
        now: u64,
    ) -> CmdResult<Result<opening::PreparedOpen, RunResult>> {
        let registry = self.registry.clone();
        let def = registry.get("ai.decide").expect("decision command");
        self.gate(def, &opts.author)?;
        let mut spelled = Registry::spelled(def, raw)?;
        spelled.remove("dry_run");
        let args = registry.normalize(def, &spelled)?;
        let token = args.str("token")?.to_string();
        if opts.dry_run
            || raw
                .get("dry_run")
                .is_some_and(|v| v == &Value::Bool(true) || v == "true")
        {
            return self.run_inner("ai.decide", raw, opts, false).map(Err);
        }
        if self
            .ai_work
            .completed_decisions
            .iter()
            .any(|(key, _)| key == &token)
        {
            return Err(CmdError::not_found(
                "その確認は見つかりません（回答済みか、期限切れです）",
            ));
        }
        if args.opt_str("answer") != Some("yes") {
            return self.decide(&token, false, now).map(Err);
        }
        let i = self
            .ai_work
            .pending
            .iter()
            .position(|d| d.token == token)
            .ok_or_else(|| {
                CmdError::not_found("その確認は見つかりません（回答済みか、期限切れです）")
            })?;
        let decision = self.ai_work.pending.remove(i);
        let prepared = (|| {
            if now > decision.expires_ms {
                return Err(CmdError::new(
                    ErrorCode::Expired,
                    format!(
                        "{} の確認は期限切れです。AI からもう一度依頼してください",
                        decision.command
                    ),
                ));
            }
            let Value::Object(raw) = decision.args else {
                return Err(CmdError::new(
                    ErrorCode::Corrupt,
                    "確認の引数を読み取れません",
                ));
            };
            let author = RunOptions {
                author: decision.author,
                dry_run: false,
            };
            self.prepare_open_inner(&decision.command, &raw, &author, true)
        })();
        match prepared {
            Ok(Ok(mut prepared)) => {
                prepared.decision = Some(token);
                Ok(Ok(prepared))
            }
            Ok(Err(mut reply)) => {
                let before = self.current.clone();
                let command = reply.command.clone();
                self.stamp_run_result(&command, &before, opts, true, &mut reply);
                self.record_open_decision(token, Ok(reply.clone()));
                Ok(Err(reply))
            }
            Err(error) => {
                self.record_open_decision(token, Err(error.clone()));
                Err(error)
            }
        }
    }

    pub(super) fn run_ai_command(
        &mut self,
        def: &CommandDef,
        args: &Args,
        opts: &RunOptions,
        dry: bool,
    ) -> CmdResult<RunResult> {
        let name = def.name;
        let plain = |msg: String| RunResult::new(name, Output::msg(msg), ChangeSet::default());
        let outcome = |name: &str, msg: &str, o: RequestOutcome| {
            let mut r = RunResult::new(
                name,
                Output::with(format!("{msg}（{}）", o.title), json!(o)),
                o.changes.clone(),
            );
            r.step = o.step;
            r
        };
        match name {
            "ai.request.begin" => {
                let title = args.str("title")?.to_string();
                if dry {
                    return Ok(plain(format!("依頼「{title}」を開始します")));
                }
                let mode = if args.flag("draft") {
                    RequestMode::Draft
                } else {
                    RequestMode::Direct
                };
                let info = self.begin_request(&opts.author, &title, mode)?;
                let how = match mode {
                    RequestMode::Direct => "ここからの操作を 1 つの操作にまとめます",
                    RequestMode::Draft => {
                        "作業コピーで進めます。ユーザーが取り込むまで曲は変わりません"
                    }
                };
                Ok(RunResult::new(
                    name,
                    Output::with(
                        format!("依頼「{title}」を開始しました（{how}）"),
                        json!(info),
                    ),
                    ChangeSet::default(),
                ))
            }
            "ai.request.end" | "ai.request.accept" | "ai.request.revert" => {
                if dry {
                    return Ok(plain(format!("{name} を実行します")));
                }
                match name {
                    "ai.request.end" => {
                        let o = self.end_request()?;
                        let msg = if o.result == "review" {
                            "作業コピーを確認待ちにしました"
                        } else {
                            "依頼を終了しました"
                        };
                        Ok(outcome(name, msg, o))
                    }
                    "ai.request.accept" => {
                        let c = OnConflict::parse(args.opt_str("on_conflict").unwrap_or("reject"))?;
                        Ok(outcome(name, "取り込みました", self.accept_request(c)?))
                    }
                    _ => Ok(outcome(name, "破棄しました", self.revert_request()?)),
                }
            }
            "ai.request.status" => {
                let info = self.request_status();
                let msg = match &info {
                    Some(i) => format!("依頼「{}」（{} 操作）", i.title, i.commands),
                    None => "進行中の依頼はありません".to_string(),
                };
                Ok(RunResult::new(
                    name,
                    Output::with(msg, json!({ "request": info })),
                    ChangeSet::default(),
                ))
            }
            "ai.activity" => {
                let now = now_ms();
                let a = &self.ai_work.activity;
                let data = json!({
                    "live": a.is_live(now),
                    "activity": a,
                    "request": self.request_info(),
                    "pending": self.ai_work.pending.len(),
                });
                Ok(RunResult::new(
                    name,
                    Output::with(
                        format!("編集中 {} 件・マーク {} 件", a.editing.len(), a.marks.len()),
                        data,
                    ),
                    ChangeSet::default(),
                ))
            }
            "ai.pending" => {
                let list = self.pending_decisions();
                Ok(RunResult::new(
                    name,
                    Output::with(format!("{} 件", list.len()), json!(list)),
                    ChangeSet::default(),
                ))
            }
            "ai.decision.get" => {
                let token = args.str("token")?;
                if let Some((_, result)) = self
                    .ai_work
                    .completed_decisions
                    .iter()
                    .find(|(key, _)| key == token)
                {
                    let result = match result {
                        Ok(result) => {
                            let mut value = serde_json::to_value(result)
                                .map_err(|e| CmdError::io(e.to_string()))?;
                            value["ok"] = json!(true);
                            value
                        }
                        Err(error) => json!({"ok": false, "error": error}),
                    };
                    Ok(RunResult::new(
                        name,
                        Output::with(
                            "完了しました",
                            json!({"state": "completed", "result": result}),
                        ),
                        ChangeSet::default(),
                    ))
                } else if let Some(decision) = self
                    .ai_work
                    .pending
                    .iter()
                    .find(|decision| decision.token == token)
                {
                    if now_ms() > decision.expires_ms {
                        return Err(CmdError::new(ErrorCode::Expired, "承認の期限が切れました"));
                    }
                    Ok(RunResult::new(
                        name,
                        Output::with("承認待ち", json!({"state": "pending"})),
                        ChangeSet::default(),
                    ))
                } else {
                    Err(CmdError::not_found("承認の結果が見つかりません"))
                }
            }
            "ai.decide" => {
                let token = args.str("token")?.to_string();
                let yes = args.opt_str("answer") == Some("yes");
                if dry {
                    return Ok(plain("確認に回答します".into()));
                }
                self.decide(&token, yes, now_ms())
            }
            "ai.batch" => self.run_batch(args, opts, dry),
            other => Err(CmdError::new(
                ErrorCode::UnknownCommand,
                format!("{other} は実装されていません"),
            )),
        }
    }

    /// Runs several commands as one request: all of them, or none (one step).
    fn run_batch(&mut self, args: &Args, opts: &RunOptions, dry: bool) -> CmdResult<RunResult> {
        let title = args
            .opt_str("title")
            .unwrap_or("まとめて実行します")
            .to_string();
        let list = match args.0.get("commands") {
            Some(Value::Array(a)) if !a.is_empty() => a.clone(),
            _ => {
                return Err(CmdError::invalid(
                    "commands に 1 つ以上のコマンドを指定してください",
                ));
            }
        };
        let mut steps: Vec<(String, Map<String, Value>)> = Vec::new();
        for (i, c) in list.iter().enumerate() {
            let cmd = c
                .get("command")
                .or_else(|| c.get("name"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    CmdError::invalid(format!("{} 番目に command がありません", i + 1))
                })?;
            if cmd.starts_with("ai.") {
                return Err(CmdError::invalid(format!(
                    "{} 番目: ai.* は入れ子にできません",
                    i + 1
                )));
            }
            let a = match c.get("args") {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(m)) => m.clone(),
                Some(_) => {
                    return Err(CmdError::invalid(format!(
                        "{} 番目の args は {{名前: 値}} の形式で指定してください",
                        i + 1
                    )));
                }
            };
            steps.push((cmd.to_string(), a));
        }
        let wrap = |i: usize, cmd: &str, e: CmdError, tail: &str| {
            CmdError::new(
                e.code,
                format!("{} 番目の `{cmd}`: {}（{tail}）", i + 1, e.message),
            )
        };
        if dry {
            let mut sc = self.make_scratch();
            let live = RunOptions {
                dry_run: false,
                ..opts.clone()
            };
            for (i, (cmd, a)) in steps.iter().enumerate() {
                sc.run(cmd, a, &live)
                    .map_err(|e| wrap(i, cmd, e, "ドライランは途中で停止しました"))?;
            }
            let changes = diff(&self.current, &sc.current);
            let msg = format!("{} 件の操作を実行した場合の変更内容", steps.len());
            return Ok(RunResult::new("ai.batch", Output::msg(msg), changes));
        }
        self.begin_request(&opts.author, &title, RequestMode::Direct)?;
        for (i, (cmd, a)) in steps.iter().enumerate() {
            match self.run(cmd, a, opts) {
                Err(e) => {
                    let _ = self.revert_request();
                    return Err(wrap(i, cmd, e, "すべて取り消しました"));
                }
                Ok(r) => {
                    if let Some(d) = r.decision {
                        self.ai_work.pending.retain(|p| p.token != d.token);
                        let _ = self.revert_request();
                        return Err(wrap(
                            i,
                            cmd,
                            CmdError::new(
                                ErrorCode::Denied,
                                "ユーザーの確認が必要な操作は、まとめて実行できません",
                            ),
                            "すべて取り消しました",
                        ));
                    }
                }
            }
        }
        let req = self.ai_work.req.take().expect("the batch's request");
        let o = self.finish_direct(req, "done")?;
        let msg = format!("{} 件の操作を 1 つの操作にまとめました", o.commands);
        let mut r = RunResult::new(
            "ai.batch",
            Output::with(msg, json!({ "title": o.title, "commands": o.commands })),
            o.changes,
        );
        r.step = o.step;
        Ok(r)
    }
}
