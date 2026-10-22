#[test]
fn prohibited_plugin_frameworks_are_banned() {
    let config: toml::Value = toml::from_str(include_str!("../../../deny.toml")).unwrap();
    let deny = config["bans"]["deny"].as_array().unwrap();
    for name in [
        "juce",
        "juce-sys",
        "tracktion",
        "tracktion-engine",
        "nih_plug",
        "nih_plug_derive",
    ] {
        assert!(
            deny.iter()
                .any(|entry| entry["name"].as_str() == Some(name)),
            "{name}"
        );
    }
}
