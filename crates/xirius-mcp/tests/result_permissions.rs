use serde_json::json;
use std::sync::{Arc, Mutex};
use xirius_core::{Session, session::AiPermission};
use xirius_mcp::{Gateway, Scope};

#[test]
fn revoking_permission_also_blocks_receipts_and_stored_result_pages() {
    let session = Arc::new(Mutex::new(Session::new("revoked")));
    let gateway = Gateway::new(session.clone(), "test", Scope::Edit).with_save(false);
    let result = gateway
        .call("ops_schema", json!({}).as_object().unwrap())
        .unwrap()
        .0;
    let pages = json!({"id": result["result_id"], "page": 0});
    assert!(pages["id"].is_string());
    let receipt = gateway
        .call(
            "track_add_audio",
            json!({"name": "Private", "request_key": "private"})
                .as_object()
                .unwrap(),
        )
        .unwrap()
        .0;
    assert_eq!(receipt["receipt"]["state"], "completed");
    assert!(
        !gateway
            .call("ops_get-result", pages.as_object().unwrap())
            .unwrap()
            .1
    );
    session
        .lock()
        .unwrap()
        .set_ai_permission(AiPermission::Off)
        .unwrap();
    for (tool, args) in [
        ("ops_get-result", pages),
        ("ops_receipt", json!({"id": "private"})),
    ] {
        let (result, error) = gateway.call(tool, args.as_object().unwrap()).unwrap();
        assert!(error);
        assert_eq!(result["error"]["code"], "denied");
        assert!(result.get("text").is_none() && result.get("receipt").is_none());
    }
    gateway.shutdown();
}
