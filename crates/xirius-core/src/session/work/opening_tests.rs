use super::*;

#[test]
fn save_request_recognizes_accepted_tokens_without_consuming_or_executing_them() {
    let mut session = Session::new("kept");
    session.set_confirm(Confirm::OPEN_04);
    let raw = json!({"path":"unused-save-copy"});
    assert_eq!(
        session.save_request("project.save-as", raw.as_object().unwrap()),
        Some(Some(PathBuf::from("unused-save-copy")))
    );
    let held = session
        .run(
            "project.save-as",
            raw.as_object().unwrap(),
            &RunOptions::ai("AI"),
        )
        .unwrap();
    let token = held.decision.unwrap().token;
    let yes = json!({"token":token,"answer":"yes"});
    let no = json!({"token":token,"answer":"no"});
    assert_eq!(
        session.save_request("ai.decide", yes.as_object().unwrap()),
        Some(Some(PathBuf::from("unused-save-copy")))
    );
    assert_eq!(
        session.save_request("ai.decide", no.as_object().unwrap()),
        None
    );
    assert_eq!(session.pending_decisions().len(), 1);
    session
        .run("ai.decide", no.as_object().unwrap(), &RunOptions::human())
        .unwrap();
    assert_eq!(
        session.save_request("ai.decide", yes.as_object().unwrap()),
        None
    );
    assert!(session.dir().is_none());
}

struct Events(Vec<Value>);
impl crate::Extension for Events {
    fn answers(&self, _: &str) -> bool {
        false
    }
    fn run(
        &mut self,
        _: &crate::ExtContext<'_>,
        _: &CommandDef,
        _: &Args,
        _: bool,
    ) -> CmdResult<Output> {
        unreachable!()
    }
    fn song_changed(&mut self, cx: &crate::ExtContext<'_>) {
        self.0.push(json!({"event":"song", "name":cx.project.name}));
    }
    fn take_events(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.0)
    }
}

fn session_with_events(project: Project) -> Session {
    let mut session = Session::from_project(project);
    session.set_confirm(Confirm::OPEN_04);
    session.add_extension(Box::new(Events(vec![json!({"event":"pending"})])));
    session
}

#[test]
fn deferred_prompts_and_dry_runs_keep_the_regular_command_hashes() {
    let raw = json!({"id":"v1", "path":"unused-copy"});
    for opts in [RunOptions::ai("AI"), RunOptions::ai("AI").dry()] {
        let project = Project::new("kept");
        let mut ordinary_session = session_with_events(project.clone());
        let mut deferred_session = session_with_events(project);
        let ordinary = ordinary_session
            .run("version.open", raw.as_object().unwrap(), &opts)
            .unwrap();
        let deferred = match deferred_session
            .prepare_open("version.open", raw.as_object().unwrap(), &opts)
            .unwrap()
        {
            Err(reply) => reply,
            Ok(_) => panic!("expected a prompt or a dry run"),
        };
        assert_eq!(deferred.data, ordinary.data);
        assert_eq!(deferred.events, ordinary.events);
        assert_eq!(deferred.events.len(), 1);
        assert_eq!(deferred.dry_run, ordinary.dry_run);
        assert_eq!(deferred.forced_dry_run, ordinary.forced_dry_run);
        assert_eq!(deferred.decision.is_some(), ordinary.decision.is_some());
    }
}

#[test]
fn deferred_completion_preserves_the_regular_response_events() {
    use crate::session::opening::{OpenId, OpenJob};
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("target");
    drop(Session::create(&path, Project::new("after")).unwrap());
    let project = Project::new("before");
    let mut ordinary = session_with_events(project.clone());
    let mut deferred = session_with_events(project);
    let raw = json!({"path":path});
    let expected = ordinary
        .run(
            "project.open",
            raw.as_object().unwrap(),
            &RunOptions::human(),
        )
        .unwrap();
    let prepared = match deferred
        .prepare_open(
            "project.open",
            raw.as_object().unwrap(),
            &RunOptions::human(),
        )
        .unwrap()
    {
        Ok(prepared) => prepared,
        Err(_) => panic!("expected a request"),
    };
    let job = OpenJob::start_prepared(
        deferred,
        prepared,
        OpenId {
            job: 1,
            generation: 1,
        },
    )
    .unwrap_or_else(|_| panic!("spawn"));
    let outcome = job.finish_blocking().unwrap();
    let actual = outcome.command_result.unwrap().unwrap();
    assert_eq!(actual.events.len(), 2);
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn deferred_approval_expiry_consumes_the_token_and_keeps_the_result() {
    let mut session = Session::new("kept");
    session.set_confirm(Confirm::OPEN_04);
    let raw = json!({"id":"v1", "path":"unused-copy"});
    let held = match session
        .prepare_open(
            "version.open",
            raw.as_object().unwrap(),
            &RunOptions::ai("AI"),
        )
        .unwrap()
    {
        Err(reply) => reply,
        Ok(_) => panic!("expected confirmation"),
    };
    let token = held.decision.unwrap().token;
    let raw = json!({"token":token,"answer":"yes"});
    let error = match session.prepare_open_decision_at(
        raw.as_object().unwrap(),
        &RunOptions::human(),
        u64::MAX,
    ) {
        Err(error) => error,
        Ok(_) => panic!("expired approval executed"),
    };
    assert_eq!(error.code, ErrorCode::Expired);
    assert!(!session.is_open_decision(raw.as_object().unwrap()));
    assert!(session.pending_decisions().is_empty());
    let cached = session
        .run(
            "ai.decision.get",
            json!({"token":token}).as_object().unwrap(),
            &RunOptions::human(),
        )
        .unwrap();
    assert_eq!(cached.data["state"], "completed");
    assert_eq!(
        cached.data["result"]["error"]["code"],
        json!(ErrorCode::Expired)
    );
    let again = session.decide(&token, true, u64::MAX).unwrap_err();
    assert_eq!(again.code, ErrorCode::NotFound);
}
