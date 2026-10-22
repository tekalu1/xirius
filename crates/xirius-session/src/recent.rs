//! The songs the person had open (PRJ-014) and the one the app opens with (UI-261), kept in
//! `recent.json` under the app's own folder ([`root`]): the screen's lists and `song.recent`
//! read the same file.
//!
//! The file is `{"songs": [{dir, name, at_ms, draft}], "open": …}`, newest first. `open` is
//! what the window had when it closed: a song's folder, `"sample"`, or `null` for an empty
//! song. A file written before `open` existed opens its newest song.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Songs the list remembers.
pub const RECENT_MAX: usize = 12;

/// A song the person had open, newest first in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentSong {
    pub dir: PathBuf,
    pub name: String,
    pub at_ms: u64,
    /// Kept by the app (never saved by the person).
    pub draft: bool,
}

/// What the app opens with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LastOpen {
    /// The first start, or a window closed on a song that was never kept.
    Empty,
    /// The sample song, built by the app.
    Sample,
    Song(PathBuf),
}

/// Where the app keeps what it keeps for itself: `XIRIUS_KEEP_DIR`, otherwise `keep` next to
/// the plugin cache.
pub fn root() -> PathBuf {
    std::env::var_os("XIRIUS_KEEP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| xirius_plugins::default_cache_path().with_file_name("keep"))
}

/// `recent.json` in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentList {
    path: PathBuf,
}

impl RecentList {
    pub fn in_dir(root: &Path) -> RecentList {
        RecentList {
            path: root.join("recent.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Option<Value> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object)
    }

    fn write(&self, v: &Value) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.path, v.to_string());
    }

    /// The songs remembered, newest first. A folder that is gone is left out.
    pub fn songs(&self) -> Vec<RecentSong> {
        let Some(v) = self.read() else {
            return Vec::new();
        };
        v["songs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                Some(RecentSong {
                    dir: PathBuf::from(s["dir"].as_str()?),
                    name: s["name"].as_str()?.to_string(),
                    at_ms: s["at_ms"].as_u64().unwrap_or(0),
                    draft: s["draft"].as_bool().unwrap_or(false),
                })
            })
            .filter(|s| s.dir.join("project.json").exists())
            .collect()
    }

    /// Writes `songs`, keeping what else the file holds.
    fn write_songs(&self, songs: &[RecentSong]) {
        let mut v = self.read().unwrap_or_else(|| json!({}));
        v["songs"] = json!(
            songs
                .iter()
                .map(|s| json!({
                    "dir": s.dir.to_string_lossy(),
                    "name": s.name,
                    "at_ms": s.at_ms,
                    "draft": s.draft,
                }))
                .collect::<Vec<_>>()
        );
        self.write(&v);
    }

    /// Puts the song in `dir` first in the list.
    pub fn remember(&self, dir: &Path, name: &str, draft: bool, now: u64) {
        let mut all = self.songs();
        all.retain(|s| s.dir != dir);
        all.insert(
            0,
            RecentSong {
                dir: dir.to_path_buf(),
                name: name.to_string(),
                at_ms: now,
                draft,
            },
        );
        all.truncate(RECENT_MAX);
        self.write_songs(&all);
    }

    /// Takes the song in `dir` out of the list.
    pub fn forget(&self, dir: &Path) {
        let mut all = self.songs();
        let before = all.len();
        all.retain(|s| s.dir != dir);
        if all.len() != before {
            self.write_songs(&all);
        }
    }

    /// The window closed with `open`: the next start opens it.
    pub fn closed_with(&self, open: &LastOpen) {
        let mut v = self.read().unwrap_or_else(|| json!({ "songs": [] }));
        v["open"] = match open {
            LastOpen::Empty => Value::Null,
            LastOpen::Sample => json!("sample"),
            LastOpen::Song(dir) => json!(dir.to_string_lossy()),
        };
        self.write(&v);
    }

    /// What the app opens with: nothing written yet is the first start (an empty song);
    /// otherwise what the last window closed with, a song whose folder is gone being an empty
    /// one. A file from before `open` opens its newest song.
    pub fn last_open(&self) -> LastOpen {
        let Some(v) = self.read() else {
            return LastOpen::Empty;
        };
        let song = |dir: PathBuf| {
            if dir.join("project.json").exists() {
                LastOpen::Song(dir)
            } else {
                LastOpen::Empty
            }
        };
        match v.get("open") {
            None => self
                .songs()
                .into_iter()
                .next()
                .map_or(LastOpen::Empty, |s| LastOpen::Song(s.dir)),
            Some(Value::String(s)) if s == "sample" => LastOpen::Sample,
            Some(Value::String(dir)) => song(PathBuf::from(dir)),
            Some(_) => LastOpen::Empty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("xirius-recent-{tag}-"))
            .tempdir()
            .unwrap()
    }

    fn song_dir(root: &Path, name: &str) -> PathBuf {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("project.json"), "{}").unwrap();
        d
    }

    #[test]
    fn the_first_start_is_an_empty_song_and_the_next_opens_what_the_window_closed_with() {
        let tmp = scratch("open");
        let list = RecentList::in_dir(tmp.path());
        assert_eq!(list.last_open(), LastOpen::Empty, "nothing written yet");
        let a = song_dir(tmp.path(), "a");
        list.remember(&a, "a", false, 100);
        list.closed_with(&LastOpen::Song(a.clone()));
        assert_eq!(list.last_open(), LastOpen::Song(a.clone()));
        // Remembering another keeps what the window closed with.
        let b = song_dir(tmp.path(), "b");
        list.remember(&b, "b", false, 200);
        assert_eq!(list.last_open(), LastOpen::Song(a.clone()));
        list.closed_with(&LastOpen::Sample);
        assert_eq!(list.last_open(), LastOpen::Sample);
        list.closed_with(&LastOpen::Empty);
        assert_eq!(list.last_open(), LastOpen::Empty);
        // A song whose folder is gone opens as an empty one.
        list.closed_with(&LastOpen::Song(a.clone()));
        std::fs::remove_dir_all(&a).unwrap();
        assert_eq!(list.last_open(), LastOpen::Empty);
        let names: Vec<String> = list.songs().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["b"]);
    }

    #[test]
    fn a_list_written_before_open_existed_opens_its_newest_song() {
        let tmp = scratch("old");
        let a = song_dir(tmp.path(), "a");
        let b = song_dir(tmp.path(), "b");
        let old = json!({ "songs": [
            { "dir": b.to_string_lossy(), "name": "b", "at_ms": 200, "draft": false },
            { "dir": a.to_string_lossy(), "name": "a", "at_ms": 100, "draft": true },
        ] });
        std::fs::write(tmp.path().join("recent.json"), old.to_string()).unwrap();
        let list = RecentList::in_dir(tmp.path());
        assert_eq!(list.last_open(), LastOpen::Song(b.clone()));
        let songs = list.songs();
        assert_eq!(songs.len(), 2);
        assert!(songs[1].draft);
        // Written again, the songs stay and `open` is added.
        list.closed_with(&LastOpen::Song(a.clone()));
        let again = RecentList::in_dir(tmp.path());
        assert_eq!(again.songs(), songs);
        assert_eq!(again.last_open(), LastOpen::Song(a));
    }

    #[test]
    fn a_broken_file_is_an_empty_list_and_a_first_start() {
        let tmp = scratch("broken");
        std::fs::write(tmp.path().join("recent.json"), "[1, 2").unwrap();
        let list = RecentList::in_dir(tmp.path());
        assert!(list.songs().is_empty());
        assert_eq!(list.last_open(), LastOpen::Empty);
    }
}
