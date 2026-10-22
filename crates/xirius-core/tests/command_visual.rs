use xirius_core::command::Registry;

#[test]
fn schemas_share_command_labels_icons_and_bindings() {
    let registry = Registry::standard();
    for def in registry.all() {
        let metadata = &def.json_schema()["annotations"]["xirius"];
        assert_eq!(metadata["label"].as_str(), def.label());
        assert_eq!(metadata["icon"].as_str(), Some(def.icon()));
        assert_eq!(
            metadata["keys"].as_array().unwrap().len(),
            def.bindings().count()
        );
    }
    let undo = registry.get("history.undo").unwrap();
    assert_eq!(undo.icon(), "undo");
    assert!(undo.bindings().any(|e| e.keys.contains(&"Mod+Z")));
}
