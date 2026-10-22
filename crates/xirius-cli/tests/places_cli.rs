//! `xirius browser.*` and `location.*`, run as a user would, on a browser file and a home
//! folder made for the test (BRW-001, BRW-005, BRW-012, BRW-015): what they change is in the
//! file the screen reads, and none of it is in the song's history.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

struct Home {
    dir: PathBuf,
    song: PathBuf,
}

impl Home {
    fn new(tag: &str) -> Home {
        let dir =
            std::env::temp_dir().join(format!("xirius-cli-places-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home/Music")).unwrap();
        std::fs::create_dir_all(dir.join("Samples/Drums")).unwrap();
        let h = Home {
            song: dir.join("Song"),
            dir,
        };
        let out = h
            .command()
            .args(["init"])
            .arg(&h.song)
            .args(["--name", "Places"])
            .output()
            .expect("run xirius init");
        assert!(out.status.success(), "{}", text(&out.stderr));
        h
    }

    fn file(&self) -> PathBuf {
        self.dir.join("browser.json")
    }

    /// The program, with its browser file and home folder in the test's folder.
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_xirius"));
        c.env_remove("XIRIUS_SONG")
            .env("XIRIUS_BROWSER_FILE", self.file())
            .env("USERPROFILE", self.dir.join("home"))
            .env("HOME", self.dir.join("home"))
            .current_dir(std::env::temp_dir());
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command()
            .arg("--song")
            .arg(&self.song)
            .args(args)
            .arg("--json")
            .output()
            .expect("run xirius")
    }

    fn ok(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}{}",
            text(&out.stdout),
            text(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("json output")
    }

    fn kept(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.file()).unwrap()).unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn names(v: &Value) -> Vec<String> {
    v["data"]["locations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_string())
        .collect()
}

fn path(v: &Value, i: usize) -> PathBuf {
    PathBuf::from(v["data"]["locations"][i]["path"].as_str().unwrap())
}

#[test]
fn places_are_added_renamed_and_taken_away_in_the_users_file() {
    let h = Home::new("places");
    let before = song(&h);
    // A first run: the song's tab and the home folder's music.
    let first = h.ok(&["location.list"]);
    assert_eq!(names(&first), ["この曲", "Music"]);
    assert_eq!(
        std::fs::canonicalize(path(&first, 0)).unwrap(),
        std::fs::canonicalize(&h.song).unwrap()
    );

    let samples = h.dir.join("Samples");
    let samples_arg = samples.to_string_lossy().into_owned();
    let dry = h.ok(&["location.add", "--path", &samples_arg, "--dry-run"]);
    assert_eq!(dry["data"]["dry_run"], true);
    assert!(!h.file().exists(), "a dry run writes nothing");

    let added = h.ok(&["location.add", "--path", &samples_arg]);
    assert_eq!(added["message"], "場所のタブ「Samples」を足しました");
    assert_eq!(h.kept()["places"].as_array().unwrap().len(), 2);
    let again = h.run(&["location.add", "--path", &samples_arg]);
    assert!(!again.status.success(), "the same folder twice");

    h.ok(&["location.rename", "--location", "Samples", "--name", "素材"]);
    assert_eq!(names(&h.ok(&["location.list"]))[2], "素材");
    let missing = h.dir.join("nowhere").to_string_lossy().into_owned();
    assert!(
        !h.run(&["location.add", "--path", &missing])
            .status
            .success()
    );

    let gone = h.ok(&["location.remove", "--location", &samples_arg]);
    assert_eq!(
        gone["message"],
        "場所のタブ「素材」を外しました（フォルダーはそのままです）"
    );
    assert!(samples.join("Drums").is_dir());
    assert!(
        !h.run(&["location.remove", "--location", "この曲"])
            .status
            .success(),
        "the song's tab stays"
    );
    assert_eq!(names(&h.ok(&["location.list"])), ["この曲", "Music"]);
    assert_eq!(song(&h), before, "the song is not changed");
}

#[test]
fn favourites_and_hidden_things_are_kept_for_the_screen() {
    let h = Home::new("favourites");
    let before = song(&h);
    h.ok(&["browser.favorite", "--item", "Dexed"]);
    h.ok(&["browser.hide", "--item", "Surge XT"]);
    let shown = h.ok(&["view.browser.hidden", "--on"]);
    assert_eq!(shown["message"], "隠したもの（1 個）も一覧に表示します");
    let kept = h.kept();
    let favourites: Vec<&str> = kept["favourites"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(favourites.contains(&"Dexed"));
    assert_eq!(kept["hidden"], serde_json::json!(["Surge XT"]));
    assert_eq!(kept["show_hidden"], true);

    // Pressed again without --on/--off: it goes back.
    let off = h.ok(&["browser.favorite", "--item", "Dexed"]);
    assert_eq!(off["message"], "「Dexed」をお気に入りから外しました");
    assert!(!h.run(&["browser.hide", "--item", " "]).status.success());
    assert_eq!(song(&h), before, "the song is not changed");
}

/// The song's file as it is now.
fn song(h: &Home) -> Vec<u8> {
    std::fs::read(h.song.join("project.json")).unwrap()
}
