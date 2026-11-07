//! Insertion, a song on disk, and a saved chain all resolve process preferences before loading.
use serde_json::{Value, json};
use std::sync::Arc;
use xirius_core::{
    Session,
    id::TrackId,
    model::{Plugin, PluginIsolation, PluginKind, Project},
    session::RunOptions,
};
use xirius_plugins::{Catalog, Entry};
use xirius_session::{
    commands::{self, shared_registry},
    ext::{ExtConfig, ExtExtras},
    output::OutputMode,
    plugins::{Isolation, PluginLoader},
};
fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}
fn inserted(s: &Session, track: &str) -> Plugin {
    s.project().tracks[&track.parse::<TrackId>().unwrap()]
        .inserts
        .iter()
        .find(|x| x.format == "vst3")
        .unwrap()
        .clone()
}
#[test]
fn settings_apply_to_insertion_song_open_and_chain_preset_before_module_or_state_restore() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::with_registry(Project::new("Isolation"), shared_registry());
    commands::attach_with(
        &mut session,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            ..ExtConfig::default()
        },
        ExtExtras {
            presets: Some(root.path().join("presets")),
            ..ExtExtras::default()
        },
    );
    let track = run(&mut session, "track.add.audio", json!({"name":"Source"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    run(
        &mut session,
        "plugin.add",
        json!({"track":track,"name":"Fixture","format":"vst3","uid":"00112233445566778899AABBCCDDEEFF","kind":"effect"}),
    );
    let new = inserted(&session, &track);
    run(
        &mut session,
        "preset.save",
        json!({"track":track,"kind":"fx-chain","name":"Isolated"}),
    );
    session.save_as(root.path().join("song")).unwrap();
    let opened = Session::open_with(root.path().join("song"), shared_registry()).unwrap();
    let song = inserted(&opened, &track);
    let target = run(&mut session, "track.add.audio", json!({"name":"Target"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    run(
        &mut session,
        "preset.load",
        json!({"track":target,"preset":"Isolated"}),
    );
    let preset = inserted(&session, &target);
    assert_ne!(new.id, preset.id);
    let catalog = Catalog {
        entries: vec![Entry {
            uid: new.uid.clone(),
            name: new.name.clone(),
            vendor: "Fixture".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            category: "Fx".into(),
            version: "1".into(),
            path: root.path().join("never-load.vst3"),
            ara: false,
            requires_process_main: false,
        }],
        failures: vec![],
    };
    for mode in [PluginIsolation::Shared, PluginIsolation::Dedicated] {
        let mut prefs = xirius_prefs::Prefs::default();
        prefs
            .set(
                &xirius_prefs::isolation::key("vst3", &new.uid),
                &json!(mode.name()),
            )
            .unwrap();
        for x in [&new, &song, &preset] {
            let mut loader = PluginLoader::new(Some(Arc::new(catalog.clone())));
            loader.set_process_prefs(prefs.clone());
            loader.set_isolation(Isolation::PerPlugin(None));
            assert!(loader.runs_apart(x));
            let error = loader.instantiate("Track", x).err().unwrap();
            assert!(
                error.contains("ホストが見つからない"),
                "must reject isolation, before trying a nonexistent module: {error}"
            );
            assert!(loader.local(x.id).is_none());
        }
    }
    // An existing but unlaunchable host also fails closed, with an opening warning.
    let host = root.path().join("invalid-host.exe");
    std::fs::write(&host, b"not an executable").unwrap();
    let mut loader = PluginLoader::new(Some(Arc::new(catalog)));
    let mut prefs = xirius_prefs::Prefs::default();
    prefs
        .set(
            &xirius_prefs::isolation::key("vst3", &new.uid),
            &json!("dedicated"),
        )
        .unwrap();
    loader.set_process_prefs(prefs);
    loader.set_isolation(Isolation::PerPlugin(Some(host)));
    let (mut processor, _) = loader.instantiate("Track", &new).unwrap();
    processor.prepare(&xirius_engine::ProcessSetup {
        sample_rate: 48000.0,
        max_block: 128,
        mode: xirius_engine::RenderMode::Realtime,
    });
    assert_eq!(
        loader.placement_of(new.id),
        Some(PluginIsolation::Dedicated)
    );
    assert!(loader.local(new.id).is_none());
    assert!(
        !loader.opening_warnings().is_empty(),
        "a host launch failure must reach the normal notification path"
    );
}
