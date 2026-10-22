use serde_json::json;
use std::sync::{Arc, Mutex};
use xirius_core::{Project, Session, command::Risk};
use xirius_mcp::{Gateway, Scope};

#[test]
fn complete_registry_keys_and_mcp_metadata_share_the_command_names() {
    let registry = xirius_session::commands::registry();
    for entry in xirius_core::commands::keys::ENTRIES {
        let name = entry.command.split_whitespace().next().unwrap();
        assert!(
            registry.get(name).is_some(),
            "unregistered key command: {name}"
        );
    }
    let session = Session::with_registry(Project::new("metadata"), Arc::new(registry.clone()));
    let gateway = Gateway::new(Arc::new(Mutex::new(session)), "test", Scope::Edit).with_save(false);
    let tools = gateway.tools();
    for def in registry
        .all()
        .iter()
        .filter(|def| def.risk != Risk::HumanOnly && !def.screen_only)
    {
        for arg in &def.args {
            let label = xirius_core::commands::labels::argument(arg.name);
            assert_ne!(
                label, arg.name,
                "{} --{} needs a field label",
                def.name, arg.name
            );
            assert!(label.chars().count() <= 24);
        }
        let tool = tools
            .iter()
            .find(|tool| tool["_meta"]["xirius"]["command"] == def.name)
            .unwrap();
        assert_eq!(tool["name"], def.name.replace('.', "_"));
        assert_eq!(tool["_meta"]["xirius"]["label"], json!(def.label()));
        assert_eq!(tool["_meta"]["xirius"]["icon"], def.icon());
        assert_eq!(
            tool["_meta"]["xirius"]["keys"],
            def.json_schema()["annotations"]["xirius"]["keys"]
        );
    }
}
