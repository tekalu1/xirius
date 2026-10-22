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
    assert!(load(&file).unwrap().favourites.contains("Delay"));
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
    assert!(load(&file).unwrap().favourites.contains("Delay"));

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
