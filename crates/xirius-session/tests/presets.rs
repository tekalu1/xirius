use serde_json::{Value, json};
use xirius_core::{
    Session,
    id::{PluginId, TrackId},
    model::Project,
    session::RunOptions,
};
use xirius_session::{
    commands::{self, shared_registry},
    ext::ExtConfig,
    output::OutputMode,
};
fn run(s: &mut Session, n: &str, a: Value) -> Value {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"))
        .data
}
fn session() -> Session {
    let mut s = Session::with_registry(Project::new("preset"), shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            ..ExtConfig::default()
        },
    );
    s
}

#[test]
fn builtin_and_vst3_files_restore_all_values_and_state_in_one_undo() {
    for (format, name, uid) in [
        ("builtin", "EQ", "EQ"),
        ("vst3", "Test", "0123456789ABCDEF0123456789ABCDEF"),
    ] {
        let mut s = session();
        let t = run(&mut s, "track.add.audio", json!({}))["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let id: PluginId = run(
            &mut s,
            "plugin.add",
            json!({"track":t,"name":name,"format":format,"uid":uid}),
        )["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let dir = tempfile::Builder::new()
            .prefix("xirius-preset-")
            .tempdir()
            .unwrap();
        let path = dir.path().join(format!("{format}.xirius-preset"));
        run(
            &mut s,
            "plugin.state.set",
            json!({"id":id.to_string(),"state":if format=="vst3" {xirius_plugins::encode_state(&xirius_vst3::PluginState {component:vec![1,2,3],controller:vec![4,5]})}else{"b3BhcXVl".into()}}),
        );
        let before = s.project().tracks[&t.parse::<TrackId>().unwrap()].inserts[0].clone();
        run(
            &mut s,
            "plugin.preset.save",
            json!({"id":id.to_string(),"file":path}),
        );
        run(
            &mut s,
            "plugin.preset.apply",
            json!({"id":id.to_string(),"values":[{"param":"custom","value":0.2}]}),
        );
        let edited = s.project().tracks[&t.parse::<TrackId>().unwrap()].inserts[0].clone();
        run(
            &mut s,
            "plugin.preset.open",
            json!({"id":id.to_string(),"file":path}),
        );
        assert_eq!(
            s.project().tracks[&t.parse::<TrackId>().unwrap()].inserts[0],
            before
        );
        run(&mut s, "history.undo", json!({}));
        assert_eq!(
            s.project().tracks[&t.parse::<TrackId>().unwrap()].inserts[0],
            edited
        );
        run(&mut s, "history.redo", json!({}));
        assert_eq!(
            s.project().tracks[&t.parse::<TrackId>().unwrap()].inserts[0],
            before
        );
        if format == "builtin" {
            let canonical = run(
                &mut s,
                "plugin.add",
                json!({"track":t,"name":"EQ","uid":"eq"}),
            )["id"]
                .as_str()
                .unwrap()
                .to_owned();
            run(
                &mut s,
                "plugin.preset.open",
                json!({"id":canonical,"file":path}),
            );
            let restored = s.project().tracks[&t.parse::<TrackId>().unwrap()]
                .inserts
                .iter()
                .find(|p| p.id.to_string() == canonical)
                .unwrap();
            assert_eq!(restored.params, before.params);
            assert_eq!(restored.state, before.state);
        }
        let other = run(&mut s, "plugin.add", json!({"track":t,"name":"Delay"}))["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let before = s.project().clone();
        assert!(
            s.run_value(
                "plugin.preset.open",
                json!({"id":other,"file":path}),
                &RunOptions::human()
            )
            .is_err()
        );
        assert_eq!(*s.project(), before);
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn malformed_file_and_dry_run_leave_song_and_disk_untouched() {
    let mut s = session();
    let id = run(&mut s, "plugin.add", json!({"track":"master","name":"EQ"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let dir = tempfile::Builder::new()
        .prefix("xirius-preset-dry-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("preset.json");
    let opts = RunOptions {
        dry_run: true,
        ..RunOptions::human()
    };
    s.run_value("plugin.preset.save", json!({"id":id,"file":path}), &opts)
        .unwrap();
    assert!(!path.exists());
    std::fs::write(&path, b"broken").unwrap();
    let before = s.project().clone();
    assert!(
        s.run_value(
            "plugin.preset.open",
            json!({"id":id,"file":path}),
            &RunOptions::human()
        )
        .is_err()
    );
    assert_eq!(*s.project(), before);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn installed_vst3_restores_its_own_stream_from_a_preset_file() {
    use std::sync::Arc;
    use xirius_plugins::{decode_state, encode_state, probe, uid_hex};
    let path = xirius_vst3::default_vst3_dir().join("Kilohearts/kHs Gain.vst3");
    if !path.exists() {
        eprintln!("kHs Gain not installed; native preset test skipped");
        return;
    }
    let entry = probe(&path).unwrap().remove(0);
    let module = Arc::new(xirius_vst3::Module::load(&path).unwrap());
    let class = module
        .classes()
        .into_iter()
        .find(|c| uid_hex(&c.cid) == entry.uid)
        .unwrap();
    let plugin = xirius_vst3::Plugin::new(module.clone(), &class).unwrap();
    let gain = plugin
        .params()
        .into_iter()
        .find(|p| p.title == "Gain")
        .unwrap();
    assert!(plugin.set_param(gain.id, 0.3));
    let state = encode_state(&plugin.get_state().unwrap());
    let mut s = session();
    let id = run(
        &mut s,
        "plugin.add",
        json!({"track":"master","name":entry.name,"format":"vst3","uid":entry.uid}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    run(&mut s, "plugin.state.set", json!({"id":id,"state":state}));
    let dir = tempfile::Builder::new()
        .prefix("xirius-native-preset-")
        .tempdir()
        .unwrap();
    let file = dir.path().join("preset.json");
    run(&mut s, "plugin.preset.save", json!({"id":id,"file":file}));
    run(&mut s, "plugin.state.set", json!({"id":id,"state":""}));
    run(&mut s, "plugin.preset.open", json!({"id":id,"file":file}));
    let restored = s.project().tracks[&TrackId::MASTER].inserts[0]
        .state
        .as_deref()
        .unwrap();
    let other = xirius_vst3::Plugin::new(module, &class).unwrap();
    other.set_state(&decode_state(restored).unwrap()).unwrap();
    assert!((other.param(gain.id).unwrap() - 0.3).abs() < 1e-6);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn invalid_state_is_rejected_even_in_a_dry_run() {
    let mut s = session();
    let id = run(&mut s, "plugin.add", json!({"track":"master","name":"EQ"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let dir = tempfile::Builder::new()
        .prefix("xirius-invalid-preset-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("preset.json");
    run(&mut s, "plugin.preset.save", json!({"id":id,"file":path}));
    let mut data: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    data["state"] = json!("broken!");
    std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    let before = s.project().clone();
    for dry_run in [false, true] {
        assert!(
            s.run_value(
                "plugin.preset.open",
                json!({"id":id,"file":path}),
                &RunOptions {
                    dry_run,
                    ..RunOptions::human()
                }
            )
            .is_err()
        );
        assert_eq!(*s.project(), before);
    }
    assert!(
        s.run_value(
            "plugin.preset.apply",
            json!({"id":id,"values":[],"state":"broken!"}),
            &RunOptions::human()
        )
        .is_err()
    );
    assert_eq!(*s.project(), before);
    std::fs::remove_file(path).unwrap();
}
