//! The browser's file: kept between runs, the song's tab never in it, the commands on it.

use serde_json::json;

use super::*;

/// A folder of the test's own, removed when dropped (also when the test fails).
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-places-{name}-"))
        .tempdir()
        .unwrap()
}

fn args(v: serde_json::Value) -> Args {
    Args::from_json(v)
}

#[test]
fn independent_processes_serialize_the_same_preference_file() {
    const WORKER: &str = "XIRIUS_PLACES_LOCK_TEST";
    if let Some(root) = std::env::var_os(WORKER) {
        let root = PathBuf::from(root);
        std::fs::write(root.join("started"), "ready").unwrap();
        run(
            &root.join("browser.json"),
            None,
            "browser.favorite",
            &args(json!({"item":"Child","on":true})),
            false,
        )
        .unwrap();
        std::fs::write(root.join("finished"), "done").unwrap();
        return;
    }
    let root = folder("process-lock");
    let file = root.path().join("browser.json");
    let mut child = None;
    transaction(&file, || {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "places::tests::independent_processes_serialize_the_same_preference_file",
            ])
            .env(WORKER, root.path());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        child = Some(command.spawn().unwrap());
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !root.path().join("started").exists() {
            assert!(std::time::Instant::now() < end, "child startup watchdog");
            std::thread::yield_now();
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(
            !root.path().join("finished").exists(),
            "child bypassed transaction lock"
        );
        let mut prefs = load(&file)?;
        prefs.favourites.insert("Parent".into());
        save(&file, &prefs)
    })
    .unwrap();
    let mut child = child.unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(std::time::Instant::now() < end, "child completion watchdog");
        std::thread::yield_now();
    }
    let prefs = load(&file).unwrap();
    assert!(prefs.favourites.contains("Parent"));
    assert!(prefs.favourites.contains("Child"));
}

#[test]
fn legacy_favourites_import_once_and_removal_survives_restart() {
    let root = folder("legacy");
    let file = root.path().join("browser.json");
    let legacy = root.path().join(".browser-favourites.json");
    let mut prefs = Prefs::first_run(None);
    prefs.favourites.insert("device:delay".into());
    save(&file, &prefs).unwrap();
    std::fs::write(&legacy, r#"["device:sampler","preset:track"]"#).unwrap();
    run_with_legacy(
        &file,
        Some(&legacy),
        None,
        "browser.favorite",
        &args(json!({"item":"device:sampler","on":false})),
        false,
    )
    .unwrap();
    let first = load(&file).unwrap();
    assert!(first.legacy_favourites_imported);
    assert!(first.favourites.contains("device:delay"));
    assert!(first.favourites.contains("preset:track"));
    assert!(!first.favourites.contains("device:sampler"));
    assert!(!first.favourites.contains("Sampler"));
    let restarted = transaction(&file, || load_migrated(&file, Some(&legacy), true)).unwrap();
    assert_eq!(first, restarted);
}

#[test]
fn failed_legacy_read_is_not_marked_imported_and_can_be_retried() {
    let root = folder("legacy-error");
    let file = root.path().join("browser.json");
    let legacy = root.path().join(".browser-favourites.json");
    save(&file, &Prefs::first_run(None)).unwrap();
    std::fs::write(&legacy, "broken").unwrap();
    assert!(transaction(&file, || load_migrated(&file, Some(&legacy), true)).is_err());
    assert!(!load(&file).unwrap().legacy_favourites_imported);
    std::fs::write(&legacy, r#"["device:delay"]"#).unwrap();
    assert!(
        transaction(&file, || load_migrated(&file, Some(&legacy), true))
            .unwrap()
            .favourites
            .contains("device:delay")
    );
}

#[test]
fn a_dry_run_does_not_commit_the_legacy_import() {
    let root = folder("legacy-dry");
    let file = root.path().join("browser.json");
    let legacy = root.path().join(".browser-favourites.json");
    std::fs::write(&legacy, r#"["device:delay"]"#).unwrap();
    run_with_legacy(
        &file,
        Some(&legacy),
        None,
        "browser.favorite",
        &args(json!({"item":"device:delay","on":false})),
        true,
    )
    .unwrap();
    assert!(!file.exists());
    assert!(!file.with_extension("json.lock").exists());
}

#[test]
fn a_first_run_has_the_downloads_and_music_there_are_and_three_favourites() {
    let home_tmp = folder("home");
    let home = home_tmp.path().to_path_buf();
    std::fs::create_dir_all(home.join("Music")).unwrap();
    let p = Prefs::first_run(Some(&home));
    assert_eq!(
        p.places,
        [Place {
            name: "Music".into(),
            path: home.join("Music"),
        }]
    );
    assert!(p.favourites.contains("Sampler"));
    assert!(Prefs::first_run(None).places.is_empty());

    // A file that is not JSON says so.
    let file = home.join("browser.json");
    std::fs::write(&file, "{").unwrap();
    assert!(load(&file).unwrap_err().contains("読み取れません"));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn what_is_saved_is_what_is_loaded_and_the_song_tab_is_never_in_the_file() {
    let dir_tmp = folder("save");
    let dir = dir_tmp.path().to_path_buf();
    let samples = dir.join("Samples");
    std::fs::create_dir_all(&samples).unwrap();
    let file = dir.join("prefs").join("browser.json");
    let mut p = Prefs::first_run(None);
    p.favourites.insert("Delay".into());
    p.hidden.insert("Chorus".into());
    p.show_hidden = true;
    assert_eq!(p.add_place(&samples, None).unwrap(), "Samples");
    save(&file, &p).unwrap();
    assert_eq!(load(&file).unwrap(), p);
    assert!(!file.with_extension("json.part").exists());

    // A tab named like the song's in the file is not read.
    std::fs::write(
        &file,
        json!({ "places": [{ "name": SONG, "path": "x" }] }).to_string(),
    )
    .unwrap();
    assert!(load(&file).unwrap().places.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn places_are_added_once_named_apart_renamed_and_removed_but_the_song_stays() {
    let dir_tmp = folder("tabs");
    let dir = dir_tmp.path().to_path_buf();
    let a = dir.join("Drums");
    let b = dir.join("more").join("Drums");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut p = Prefs::first_run(None);
    assert_eq!(p.add_place(&a, None).unwrap(), "Drums");
    // The same folder again (written another way): it has its tab.
    let again = dir.join("more").join("..").join("Drums");
    assert!(p.add_place(&again, None).unwrap_err().contains("もう"));
    // Another folder of the same name gets a name of its own.
    assert_eq!(p.add_place(&b, None).unwrap(), "Drums 2");
    assert_eq!(p.add_place(&dir, Some(SONG)).unwrap(), format!("{SONG} 2"));
    assert!(p.add_place(&dir.join("none"), None).is_err());

    p.rename_place("Drums 2", "Kits").unwrap();
    assert_eq!(p.places[1].name, "Kits");
    assert!(p.rename_place("Kits", "Drums").is_err());
    assert!(p.rename_place("Kits", " ").is_err());
    // By its folder too.
    assert_eq!(p.remove_place(&a.to_string_lossy()).unwrap().name, "Drums");
    assert!(p.remove_place(SONG).unwrap_err().contains("外したり"));
    assert!(p.rename_place(SONG, "x").is_err());
    assert!(
        p.remove_place("none")
            .unwrap_err()
            .contains("見つかりません")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_commands_change_the_file_and_a_dry_run_does_not() {
    let dir_tmp = folder("run");
    let dir = dir_tmp.path().to_path_buf();
    let file = dir.join("browser.json");
    std::fs::write(&file, "{}").unwrap();
    let song = dir.join("Song");

    let out = run(
        &file,
        Some(&song),
        "browser.favorite",
        &args(json!({ "item": "Delay" })),
        false,
    )
    .unwrap();
    assert_eq!(out.message, "「Delay」をお気に入りにしました");
    assert!(load(&file).unwrap().favourites.contains("device:delay"));
    // Without --on/--off it flips; --on keeps it on.
    run(
        &file,
        None,
        "browser.favorite",
        &args(json!({ "item": "Delay" })),
        false,
    )
    .unwrap();
    assert!(load(&file).unwrap().favourites.is_empty());
    let on = json!({ "item": "Delay", "on": true });
    run(&file, None, "browser.favorite", &args(on.clone()), false).unwrap();
    run(&file, None, "browser.favorite", &args(on), false).unwrap();
    assert!(load(&file).unwrap().favourites.contains("device:delay"));

    run(
        &file,
        None,
        "browser.hide",
        &args(json!({ "item": "Chorus" })),
        false,
    )
    .unwrap();
    let out = run(
        &file,
        None,
        "view.browser.hidden",
        &args(json!({ "on": true })),
        false,
    )
    .unwrap();
    assert_eq!(out.message, "隠したもの（1 個）も一覧に表示します");
    let p = load(&file).unwrap();
    assert!(p.hidden.contains("Chorus") && p.show_hidden);

    // A dry run says what it would do and keeps the file.
    let before = std::fs::read(&file).unwrap();
    let out = run(
        &file,
        None,
        "location.add",
        &args(json!({ "path": dir.to_string_lossy(), "name": "Work" })),
        true,
    )
    .unwrap();
    assert_eq!(out.message, "（試行）場所のタブ「Work」を足しました");
    assert_eq!(std::fs::read(&file).unwrap(), before);

    let add = json!({ "path": dir.to_string_lossy(), "name": "Work" });
    run(&file, None, "location.add", &args(add), false).unwrap();
    run(
        &file,
        None,
        "location.rename",
        &args(json!({ "location": "Work", "name": "Desk" })),
        false,
    )
    .unwrap();
    let list = run(&file, Some(&song), "location.list", &Args::default(), false).unwrap();
    let tabs = list.data["locations"].as_array().unwrap();
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0]["name"], SONG);
    assert_eq!(tabs[0]["path"], song.to_string_lossy().as_ref());
    assert_eq!(tabs[1]["name"], "Desk");

    let err = run(
        &file,
        None,
        "location.remove",
        &args(json!({ "location": SONG })),
        false,
    )
    .unwrap_err();
    assert!(err.message.contains("外したり"), "{}", err.message);
    let out = run(
        &file,
        None,
        "location.remove",
        &args(json!({ "location": "Desk" })),
        false,
    )
    .unwrap();
    assert_eq!(
        out.message,
        "場所のタブ「Desk」を外しました（フォルダーはそのままです）"
    );
    assert!(load(&file).unwrap().places.is_empty());
    assert!(dir.is_dir());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ambiguous_legacy_device_names_survive_explicit_id_edits() {
    let root = folder("duplicate-device-names");
    let file = root.path().join("browser.json");
    let legacy = root.path().join(".browser-favourites.json");
    std::fs::write(&legacy, r#"["Shared","Sampler"]"#).unwrap();
    let entries = [("a", "Shared"), ("b", "Shared"), ("c", "Sampler")].into_iter().map(|(uid, name)| {
        serde_json::from_value(json!({"uid":uid,"name":name,"vendor":"","kind":"instrument","format":"vst3","category":"","version":"","path":"fixture.vst3"})).unwrap()
    }).collect();
    let catalog = xirius_plugins::Catalog {
        entries,
        failures: vec![],
    };
    for id in ["device:a", "device:c", "device:sampler"] {
        for on in [true, false] {
            let resolved = device_args(&args(json!({"item":id,"on":on})), &catalog);
            run_with_legacy(
                &file,
                Some(&legacy),
                None,
                "browser.favorite",
                &resolved,
                false,
            )
            .unwrap();
            let saved = load(&file).unwrap();
            assert!(saved.favourites.contains("Shared"));
            assert!(saved.favourites.contains("Sampler"));
            assert_eq!(saved.favourites.contains(id), on);
        }
    }
}
