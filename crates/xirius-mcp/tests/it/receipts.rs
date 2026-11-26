use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use xirius_core::{Session, protocol::ReceiptState};
use xirius_mcp::{Gateway, Scope, receipts::Receipts};

#[test]
fn timeout_is_unknown_and_late_completion_keeps_the_id() {
    let store = Receipts::default();
    let mut events = store.subscribe();
    store.accept("one", "track.mute", "args".into()).unwrap();
    store.update("one", ReceiptState::Running, None);
    assert_eq!(
        store.wait("one", Duration::from_millis(1)).unwrap()["receipt"]["state"],
        "unknown"
    );
    store.update("one", ReceiptState::Completed, Some(json!({"ok": true})));
    let states: Vec<Value> = (0..3)
        .map(|_| events.try_recv().unwrap()["receipt"]["state"].clone())
        .collect();
    assert_eq!(
        states,
        [json!("accepted"), json!("running"), json!("completed")]
    );
    assert_eq!(store.get("one").unwrap()["receipt"]["id"], "one");
    assert!(!store.accept("one", "track.mute", "args".into()).unwrap());
    assert!(store.accept("one", "track.mute", "other".into()).is_err());
}

#[test]
fn gateway_retry_edits_once_and_conflicting_key_is_typed() {
    let session = Arc::new(Mutex::new(Session::new("receipts")));
    let gateway = Gateway::new(session.clone(), "test", Scope::Edit).with_save(false);
    let args = json!({"name": "One", "request_key": "same"});
    for _ in 0..2 {
        let (result, error) = gateway
            .call("track_add_audio", args.as_object().unwrap())
            .unwrap();
        assert!(!error);
        assert_eq!(result["receipt"]["state"], "completed");
        assert_eq!(result["receipt"]["id"], "same");
    }
    assert_eq!(session.lock().unwrap().project().tracks.len(), 2);
    let (result, error) = gateway
        .call(
            "track_add_audio",
            json!({"name": "Other", "request_key": "same"})
                .as_object()
                .unwrap(),
        )
        .unwrap();
    assert!(error);
    assert_eq!(result["error"]["code"], "conflict");
}

#[test]
fn approval_finishes_the_original_receipt_and_retry_does_not_repeat_the_edit() {
    let session = Arc::new(Mutex::new(Session::new("decisions")));
    let gateway = Gateway::new(session.clone(), "test", Scope::Edit).with_save(false);
    let track = gateway
        .call(
            "track_add_audio",
            json!({"name": "Delete"}).as_object().unwrap(),
        )
        .unwrap()
        .0["data"]["id"]
        .clone();
    let args = json!({"ids": [track], "request_key": "delete-once"});
    let first = gateway
        .call("track_delete", args.as_object().unwrap())
        .unwrap()
        .0;
    assert_eq!(first["receipt"]["state"], "accepted");
    assert_eq!(first["confirmation_required"], true);
    gateway
        .admin(
            "ai.decide",
            json!({"token": first["decision"]["token"], "answer": "yes"})
                .as_object()
                .unwrap(),
        )
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        let result = gateway.receipt("delete-once").unwrap();
        if result["receipt"]["state"] == "completed" {
            assert_eq!(result["changes"]["counts"]["track"]["removed"], 1);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    let retry = gateway
        .call("track_delete", args.as_object().unwrap())
        .unwrap()
        .0;
    assert_eq!(retry["receipt"]["state"], "completed");
    assert_eq!(session.lock().unwrap().project().tracks.len(), 1);
}

#[test]
fn field_order_does_not_change_the_idempotency_key() {
    let gateway = Gateway::new(
        Arc::new(Mutex::new(Session::new("order"))),
        "test",
        Scope::Edit,
    )
    .with_save(false);
    let first: Value =
        serde_json::from_str(r#"{"name":"One","into":"root","request_key":"ordered"}"#).unwrap();
    let second: Value =
        serde_json::from_str(r#"{"request_key":"ordered","into":"root","name":"One"}"#).unwrap();
    assert!(
        !gateway
            .call("track_add_audio", first.as_object().unwrap())
            .unwrap()
            .1
    );
    assert!(
        !gateway
            .call("track_add_audio", second.as_object().unwrap())
            .unwrap()
            .1
    );
}
