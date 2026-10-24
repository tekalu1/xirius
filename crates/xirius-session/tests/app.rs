//! `song.recent` and `app.update.check` (UI-261, UI-262) from the command table, with a fetch of
//! the test's own: no test reaches the network.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;
use xirius_core::session::RunOptions;
use xirius_core::{Project, Session};
use xirius_session::app::AppExt;
use xirius_session::recent::{LastOpen, RecentList};
use xirius_session::update::{CURRENT, Fetch};

fn session(root: &std::path::Path, settings: xirius_prefs::Shared, fetch: Fetch) -> Session {
    let mut s = Session::with_registry(
        Project::new("test"),
        xirius_session::commands::shared_registry(),
    );
    s.add_extension(Box::new(AppExt {
        root: root.to_path_buf(),
        settings,
        fetch,
    }));
    s
}

fn counting(body: Result<String, String>) -> (Fetch, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let a = asked.clone();
    let fetch: Fetch = Arc::new(move |_: &str| {
        a.fetch_add(1, Ordering::SeqCst);
        body.clone()
    });
    (fetch, asked)
}

/// A version after this program's.
fn later() -> String {
    let major = env!("CARGO_PKG_VERSION_MAJOR").parse::<u64>().unwrap() + 1;
    format!("{major}.0.0")
}

#[test]
fn song_recent_lists_the_songs_and_what_the_next_start_opens() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-app-recent-")
        .tempdir()
        .unwrap();
    let (fetch, asked) = counting(Err("unused".into()));
    let mut s = session(tmp.path(), xirius_prefs::Settings::memory().shared(), fetch);
    let run = |s: &mut Session| {
        s.run_value("song.recent", json!({}), &RunOptions::human())
            .unwrap()
    };
    let first = run(&mut s);
    assert_eq!(first.data["songs"], json!([]));
    assert_eq!(first.data["open"]["kind"], "empty");
    assert_eq!(first.message, "最近の曲なし");

    let dir = tmp.path().join("夜の散歩 2");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("project.json"), "{}").unwrap();
    let list = RecentList::in_dir(tmp.path());
    list.remember(&dir, "夜の散歩 2", false, 1_700_000_000_000)
        .unwrap();
    list.closed_with(&LastOpen::Song(dir.clone())).unwrap();
    let next = run(&mut s);
    assert_eq!(next.data["songs"][0]["name"], "夜の散歩 2");
    assert_eq!(next.data["songs"][0]["at_ms"], 1_700_000_000_000u64);
    assert_eq!(next.data["songs"][0]["draft"], false);
    assert_eq!(next.data["open"]["kind"], "song");
    assert_eq!(next.data["open"]["dir"], dir.to_string_lossy().as_ref());
    assert!(next.changes.is_empty(), "the song is not changed");
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

#[test]
fn app_update_check_asks_now_and_says_when_a_newer_version_is_out() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-app-update-")
        .tempdir()
        .unwrap();
    let v = later();
    let body = json!({
        "tag_name": format!("v{v}"),
        "html_url": format!("https://github.com/tekalu1/xirius/releases/tag/v{v}"),
        "body": "- 録音の開始が速くなりました",
    })
    .to_string();
    let (fetch, asked) = counting(Ok(body));
    let settings = xirius_prefs::Settings::memory().shared();
    let mut s = session(tmp.path(), settings.clone(), fetch);
    let r = s
        .run_value("app.update.check", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(r.data["newer"], true);
    assert_eq!(r.data["latest"], v.as_str());
    assert_eq!(r.data["current"], CURRENT);
    assert_eq!(r.data["notes"], json!(["録音の開始が速くなりました"]));
    assert_eq!(r.message, format!("新しいバージョン {v} があります"));
    // Run again within the day, it still asks: the limit is the launch's.
    s.run_value("app.update.check", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(asked.load(Ordering::SeqCst), 2);

    // UI-344: only the launch is off. The CLI keeps its sentence and asks manually.
    settings
        .lock()
        .unwrap()
        .prefs
        .set("app.update.check", &json!(false))
        .unwrap();
    let off = s
        .run_value("app.update.check", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(off.data["enabled"], true);
    assert_eq!(off.message, format!("新しいバージョン {v} があります"));
    assert_eq!(asked.load(Ordering::SeqCst), 3);
}

#[test]
fn app_update_check_fails_with_a_reason_when_it_cannot_ask() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-app-update-fail-")
        .tempdir()
        .unwrap();
    let (fetch, _) = counting(Err("Could not resolve host: api.github.com".into()));
    let mut s = session(tmp.path(), xirius_prefs::Settings::memory().shared(), fetch);
    let e = s
        .run_value("app.update.check", json!({}), &RunOptions::human())
        .unwrap_err();
    assert!(e.to_string().contains("更新を確認できません"), "{e}");
    assert!(!tmp.path().join("update.json").exists());
}

#[test]
fn the_update_pref_is_on_until_turned_off_and_survives_a_reopen() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-app-pref-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().join("prefs");
    // A settings file from before the pref existed: on.
    std::fs::create_dir_all(&dir).unwrap();
    let old = xirius_prefs::Settings::open(&dir);
    assert!(old.prefs.bool("app.update.check"));
    let mut s = Session::with_registry(
        Project::new("test"),
        xirius_session::commands::shared_registry(),
    );
    let settings = xirius_prefs::Settings::open(&dir).shared();
    xirius_prefs::attach(&mut s, settings);
    s.run_value(
        "prefs.set",
        json!({ "key": "app.update.check", "value": "false" }),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(
        !xirius_prefs::Settings::open(&dir)
            .prefs
            .bool("app.update.check")
    );
}
