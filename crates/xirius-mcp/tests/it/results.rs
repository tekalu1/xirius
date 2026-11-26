use serde_json::json;
use xirius_mcp::results::{PAGE_CHARS, Results};

#[test]
fn gateway_pages_can_be_fetched_through_the_generated_tool() {
    let gateway = xirius_mcp::Gateway::new(
        std::sync::Arc::new(std::sync::Mutex::new(xirius_core::Session::new("results"))),
        "pages",
        xirius_mcp::Scope::Edit,
    )
    .with_save(false);
    let (summary, error) = gateway
        .call("ops_list", json!({}).as_object().unwrap())
        .unwrap();
    assert!(!error);
    let id = summary["result_id"].as_str().unwrap();
    let mut text = String::new();
    for page in 0..summary["pages"].as_u64().unwrap() {
        let (result, error) = gateway
            .call(
                "ops_get-result",
                json!({"id": id, "page": page}).as_object().unwrap(),
            )
            .unwrap();
        assert!(!error);
        text.push_str(result["text"].as_str().unwrap());
    }
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["ok"],
        true
    );
}

#[test]
fn large_unicode_result_is_summarized_and_reconstructed_by_pages() {
    let store = Results::default();
    let value = json!({"ok": true, "command": "notes.list", "data": "ノート🎵".repeat(PAGE_CHARS)});
    let summary = store.store("receipt-1", value.clone());
    assert!(summary.get("data").is_none());
    let pages = summary["pages"].as_u64().unwrap() as usize;
    let text: String = (0..pages)
        .map(|page| {
            store.get_result("receipt-1", page).unwrap()["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        value
    );
    assert!(store.get_result("receipt-1", pages).is_err());
    assert!(store.get_result("missing", 0).is_err());
}

#[test]
fn summary_keeps_decision_error_and_preview_flags() {
    let store = Results::default();
    let value = json!({"ok": false, "dry_run": true, "confirmation_required": true,
        "decision": {"token": "approval", "args": "x".repeat(PAGE_CHARS * 2)},
        "error": {"code": "denied", "message": "Denied"}, "data": {"state": "pending", "job": "job-1"}});
    let summary = store.store("large-decision", value);
    assert_eq!(summary["confirmation_required"], true);
    assert_eq!(summary["decision"]["token"], "approval");
    assert!(summary["decision"].get("args").is_none());
    assert_eq!(summary["dry_run"], true);
    assert_eq!(summary["error"]["code"], "denied");
    assert_eq!(summary["data"]["state"], "pending");
}
