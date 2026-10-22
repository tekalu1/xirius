use serde_json::{Map, Value, json};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use xirius_core::{
    Args, CmdResult, ExtContext, Extension, Output, Project, Session,
    command::{ArgDef, CommandDef, Handler, Registry, Risk},
    session::{Confirm, RunOptions, RunResult},
};
use xirius_mcp::{Gateway, Host, Scope};

struct Blocked {
    registry: Registry,
    entered: mpsc::Sender<Value>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl Host for Blocked {
    fn with_registry(&self, use_registry: &mut dyn FnMut(&Registry)) {
        use_registry(&self.registry);
    }
    fn run(
        &self,
        command: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
        _: bool,
    ) -> CmdResult<RunResult> {
        assert!(opts.author.is_ai());
        self.entered.send(json!(raw)).unwrap();
        let _ = self
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(15));
        let mut result =
            Session::new("blocked").run_value("track.add.audio", json!({"name": "Done"}), opts)?;
        result.command = command.into();
        Ok(result)
    }
}

/// B-5: `background: false` means "stay until it is done", even when the operation is long.
#[test]
fn background_false_waits_for_the_end() {
    let (entered, observed) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let registry = Registry::from_defs(vec![CommandDef {
        name: "slow.render",
        summary: "render",
        args: vec![ArgDef::flag("background", "background")],
        history: false,
        risk: Risk::Write,
        handler: Handler::Session,
        screen_only: false,
    }]);
    let gateway = Gateway::hosted(
        Arc::new(Blocked {
            registry,
            entered,
            release: Mutex::new(wait),
        }),
        "test",
        Scope::Edit,
    )
    .with_save(false);
    let caller = gateway.clone();
    let (sent, received) = mpsc::channel();
    let call = std::thread::spawn(move || {
        sent.send(
            caller.call(
                "slow_render",
                json!({"background": false, "request_key": "blocked"})
                    .as_object()
                    .unwrap(),
            ),
        )
        .unwrap()
    });
    let raw = observed.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(raw["background"], false);
    // It does not answer while the operation still runs.
    assert!(
        received.recv_timeout(Duration::from_millis(300)).is_err(),
        "background: false answered before the end"
    );
    release.send(()).unwrap();
    let (value, error) = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the result after the end")
        .unwrap();
    assert!(!error, "{value}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["receipt"]["state"], "completed");
    call.join().unwrap();
    gateway.shutdown();
}

/// A command with `background` and a handler, so a session can be asked to run it.
struct Slow;
impl Extension for Slow {
    fn answers(&self, name: &str) -> bool {
        name == "slow.render"
    }
    fn run(&mut self, _: &ExtContext<'_>, _: &CommandDef, _: &Args, _: bool) -> CmdResult<Output> {
        Ok(Output::msg("done"))
    }
}

struct Owned {
    session: Arc<Mutex<Session>>,
    registry: Registry,
}
impl Host for Owned {
    fn with_registry(&self, use_registry: &mut dyn FnMut(&Registry)) {
        use_registry(&self.registry);
    }
    fn run(
        &self,
        command: &str,
        raw: &Map<String, Value>,
        opts: &RunOptions,
        _: bool,
    ) -> CmdResult<RunResult> {
        xirius_mcp::gateway::lock(&self.session).run(command, raw, opts)
    }
}

/// B-5: an operation that waits for a person says so (and carries the decision) instead of
/// answering `ok: true, pending: true`, and a refusal comes back as a failure.
#[test]
fn a_background_command_waiting_for_a_person_says_so() {
    let mut defs = xirius_core::commands::all();
    defs.push(CommandDef {
        name: "slow.render",
        summary: "render",
        args: vec![ArgDef::flag("background", "background")],
        history: false,
        risk: Risk::Guarded,
        handler: Handler::Session,
        screen_only: false,
    });
    let registry = Registry::from_defs(defs);
    let mut session = Session::with_registry(Project::new("ask"), Arc::new(registry.clone()));
    session.set_confirm(Confirm::OPEN_04);
    session.add_extension(Box::new(Slow));
    let song = Arc::new(Mutex::new(session));
    let gateway = Gateway::hosted(
        Arc::new(Owned {
            session: song,
            registry,
        }),
        "test",
        Scope::Edit,
    );
    let (value, error) = gateway
        .call(
            "slow_render",
            json!({"request_key": "ask"}).as_object().unwrap(),
        )
        .unwrap();
    assert!(!error, "{value}");
    assert_eq!(value["confirmation_required"], true, "{value}");
    assert!(value["decision"]["question"].is_string(), "{value}");
    let token = value["decision"]["token"]
        .as_str()
        .expect("a decision token")
        .to_string();
    assert!(value.get("pending").is_none(), "{value}");

    let (answer, answer_error) = gateway
        .admin(
            "ai.decide",
            json!({"token": token, "answer": "no"}).as_object().unwrap(),
        )
        .unwrap();
    assert!(!answer_error, "{answer}");

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let receipt = gateway.receipt("ask").unwrap();
        if receipt["receipt"]["state"] == "completed" {
            assert_eq!(receipt["ok"], false, "{receipt}");
            assert_eq!(receipt["error"]["code"], "denied", "{receipt}");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{receipt}");
        std::thread::sleep(Duration::from_millis(10));
    }
    gateway.shutdown();
}
