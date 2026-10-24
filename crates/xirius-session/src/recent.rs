//! The songs the person had open (PRJ-014) and the one the app opens with (UI-261), kept in
//! `recent.json` under the app's own folder ([`root`]): the screen's lists and `song.recent`
//! read the same file.
//!
//! The file is `{"songs": [{dir, name, at_ms, draft}], "open": …}`, newest first. `open` is
//! what the window had when it closed: a song's folder, `"sample"`, or `null` for an empty
//! song. A file written before `open` existed opens its newest song.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

// One transaction boundary for every RecentList, including independently created
// handles in open/close workers. The stable OS lock extends this boundary across
// processes; recent.json itself is replaced and must never be the lock inode.
static UPDATES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn update_guard() -> std::sync::MutexGuard<'static, ()> {
    match UPDATES.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            #[cfg(test)]
            ON_WAIT.with(|h| {
                if let Some(h) = h.borrow_mut().take() {
                    h();
                }
            });
            UPDATES.lock().unwrap_or_else(|error| error.into_inner())
        }
    }
}

#[cfg(test)]
thread_local! {
    static AFTER_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static ON_WAIT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

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
    fn update_guard(
        &self,
    ) -> xirius_core::CmdResult<(std::sync::MutexGuard<'static, ()>, std::fs::File)> {
        use std::fs::{OpenOptions, TryLockError};
        let process = update_guard();
        let root = self.path.parent().expect("recent list parent");
        std::fs::create_dir_all(root)?;
        // Never truncate, replace or remove this file, even after releasing it.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("recent.lock"))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                #[cfg(test)]
                ON_WAIT.with(|h| {
                    if let Some(h) = h.borrow_mut().take() {
                        h();
                    }
                });
                file.lock()?;
            }
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        Ok((process, file))
    }
    pub fn in_dir(root: &Path) -> RecentList {
        RecentList {
            path: root.join("recent.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Option<Value> {
        self.read_checked().ok().flatten()
    }

    fn read_checked(&self) -> xirius_core::CmdResult<Option<Value>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let value = serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object);
        #[cfg(test)]
        AFTER_READ.with(|h| {
            if let Some(h) = h.borrow_mut().take() {
                h();
            }
        });
        Ok(value)
    }

    fn write(&self, v: &Value) -> xirius_core::CmdResult<()> {
        xirius_core::store::atomic_write(&self.path, v.to_string().as_bytes())
    }

    /// The songs remembered, newest first. A folder that is gone is left out.
    pub fn songs(&self) -> Vec<RecentSong> {
        let Some(v) = self.read() else {
            return Vec::new();
        };
        Self::songs_in(&v)
    }

    fn songs_in(v: &Value) -> Vec<RecentSong> {
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
    fn write_songs(&self, mut v: Value, songs: &[RecentSong]) -> xirius_core::CmdResult<()> {
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
        self.write(&v)
    }

    /// Puts the song in `dir` first in the list.
    pub fn remember(
        &self,
        dir: &Path,
        name: &str,
        draft: bool,
        now: u64,
    ) -> xirius_core::CmdResult<()> {
        let _transaction = self.update_guard()?;
        let v = self.read_checked()?.unwrap_or_else(|| json!({}));
        let mut all = Self::songs_in(&v);
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
        self.write_songs(v, &all)
    }

    /// Takes the song in `dir` out of the list.
    pub fn forget(&self, dir: &Path) -> xirius_core::CmdResult<()> {
        let _transaction = self.update_guard()?;
        let v = self.read_checked()?.unwrap_or_else(|| json!({}));
        let mut all = Self::songs_in(&v);
        let before = all.len();
        all.retain(|s| s.dir != dir);
        if all.len() != before {
            self.write_songs(v, &all)?;
        }
        Ok(())
    }

    /// The window closed with `open`: the next start opens it.
    pub fn closed_with(&self, open: &LastOpen) -> xirius_core::CmdResult<()> {
        let _transaction = self.update_guard()?;
        let mut v = self
            .read_checked()?
            .unwrap_or_else(|| json!({ "songs": [] }));
        v["open"] = match open {
            LastOpen::Empty => Value::Null,
            LastOpen::Sample => json!("sample"),
            LastOpen::Song(dir) => json!(dir.to_string_lossy()),
        };
        self.write(&v)
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

    #[test]
    fn recent_process_child() {
        use std::io::{BufRead, Write};
        let Some(root) = std::env::var_os("XIRIUS_RECENT_TEST_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let list = RecentList::in_dir(&root);
        let signal = |text: &str| {
            println!("RECENT_SYNC:{text}");
            std::io::stdout().flush().unwrap();
        };
        match std::env::var("XIRIUS_RECENT_TEST_ROLE").unwrap().as_str() {
            "remember" | "close" => {
                AFTER_READ.with(|h| {
                    *h.borrow_mut() = Some(Box::new(move || {
                        signal("read");
                        let mut line = String::new();
                        std::io::stdin().lock().read_line(&mut line).unwrap();
                        assert_eq!(line.trim(), "release");
                    }))
                });
                if std::env::var("XIRIUS_RECENT_TEST_ROLE").unwrap() == "close" {
                    list.closed_with(&LastOpen::Sample).unwrap();
                } else {
                    list.remember(&root.join("a"), "a", false, 2).unwrap();
                }
            }
            "newer" => {
                ON_WAIT.with(|h| *h.borrow_mut() = Some(Box::new(move || signal("waiting"))));
                list.remember(&root.join("draft"), "draft", true, 3)
                    .unwrap();
                list.forget(&root.join("a")).unwrap();
                list.closed_with(&LastOpen::Song(root.join("draft")))
                    .unwrap();
            }
            _ => panic!("unexpected test role"),
        }
        signal("done");
    }

    #[test]
    fn another_process_cannot_erase_a_draft_or_the_close_record() {
        use std::io::{BufRead, BufReader, Write};
        use std::process::{Child, Command, Stdio};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        struct Process(Child, mpsc::Receiver<String>);
        impl Process {
            fn start(root: &Path, role: &str) -> Self {
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "recent::tests::recent_process_child",
                        "--nocapture",
                    ])
                    .env("XIRIUS_RECENT_TEST_ROOT", root)
                    .env("XIRIUS_RECENT_TEST_ROLE", role)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
                let output = child.stdout.take().unwrap();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    for line in BufReader::new(output).lines() {
                        let line = line.unwrap();
                        if let Some((_, marker)) = line.split_once("RECENT_SYNC:") {
                            let _ = tx.send(marker.to_owned());
                        }
                    }
                });
                Self(child, rx)
            }
            fn next(&self) -> String {
                self.1.recv_timeout(Duration::from_secs(15)).unwrap()
            }
            fn finish(&mut self) {
                let until = Instant::now() + Duration::from_secs(15);
                loop {
                    if let Some(status) = self.0.try_wait().unwrap() {
                        assert!(status.success());
                        break;
                    }
                    assert!(Instant::now() < until, "child did not exit");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        impl Drop for Process {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        for role in ["remember", "close"] {
            let tmp = scratch("processes");
            let a = song_dir(tmp.path(), "a");
            let b = song_dir(tmp.path(), "draft");
            let list = RecentList::in_dir(tmp.path());
            list.remember(&a, "a", false, 1).unwrap();
            let mut p = Process::start(tmp.path(), role);
            assert_eq!(p.next(), "read");
            let mut q = Process::start(tmp.path(), "newer");
            // The old implementation completes Q while P holds stale JSON.
            // With OS exclusion, Q signals contention before blocking instead.
            assert!(matches!(q.next().as_str(), "waiting" | "done"));
            writeln!(p.0.stdin.as_mut().unwrap(), "release").unwrap();
            p.finish();
            q.finish();
            assert!(list.songs().iter().any(|s| s.dir == b && s.draft));
            assert_eq!(list.last_open(), LastOpen::Song(b));
        }
    }

    #[test]
    fn an_old_update_cannot_erase_a_draft_or_the_close_record() {
        use std::sync::{Arc, Condvar, Mutex, mpsc};
        for close in [false, true] {
            let tmp = scratch("serialized");
            let a = song_dir(tmp.path(), "a");
            let b = song_dir(tmp.path(), "draft");
            let list = RecentList::in_dir(tmp.path());
            list.remember(&a, "a", false, 1).unwrap();
            let gate = Arc::new((Mutex::new(false), Condvar::new()));
            let (tx, read) = mpsc::channel();
            let old = list.clone();
            let held = gate.clone();
            let old_a = a.clone();
            let stale = std::thread::spawn(move || {
                AFTER_READ.with(|h| {
                    *h.borrow_mut() = Some(Box::new(move || {
                        tx.send(()).unwrap();
                        let lock = held.0.lock().unwrap();
                        drop(held.1.wait_while(lock, |released| !*released).unwrap());
                    }))
                });
                if close {
                    old.closed_with(&LastOpen::Sample).unwrap();
                } else {
                    old.remember(&old_a, "a", false, 2).unwrap();
                }
            });
            read.recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            let (tx, attempted) = mpsc::channel();
            let new = list.clone();
            let draft = b.clone();
            let newer = std::thread::spawn(move || {
                let waiting = tx.clone();
                ON_WAIT.with(|h| {
                    *h.borrow_mut() = Some(Box::new(move || {
                        let _ = waiting.send(());
                    }))
                });
                new.remember(&draft, "draft", true, 3).unwrap();
                new.forget(&old_a_path(&new, &draft)).unwrap();
                new.closed_with(&LastOpen::Song(draft)).unwrap();
                let _ = tx.send(());
            });
            attempted
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
            stale.join().unwrap();
            newer.join().unwrap();
            assert!(list.songs().iter().any(|s| s.dir == b && s.draft));
            assert_eq!(list.last_open(), LastOpen::Song(b));
        }
    }

    fn old_a_path(list: &RecentList, draft: &Path) -> PathBuf {
        // Exercise forget in the same sequence without removing the recovery row.
        list.songs()
            .into_iter()
            .find(|s| s.dir != draft)
            .unwrap()
            .dir
    }

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
        list.remember(&a, "a", false, 100).unwrap();
        list.closed_with(&LastOpen::Song(a.clone())).unwrap();
        assert_eq!(list.last_open(), LastOpen::Song(a.clone()));
        // Remembering another keeps what the window closed with.
        let b = song_dir(tmp.path(), "b");
        list.remember(&b, "b", false, 200).unwrap();
        assert_eq!(list.last_open(), LastOpen::Song(a.clone()));
        list.closed_with(&LastOpen::Sample).unwrap();
        assert_eq!(list.last_open(), LastOpen::Sample);
        list.closed_with(&LastOpen::Empty).unwrap();
        assert_eq!(list.last_open(), LastOpen::Empty);
        // A song whose folder is gone opens as an empty one.
        list.closed_with(&LastOpen::Song(a.clone())).unwrap();
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
        list.closed_with(&LastOpen::Song(a.clone())).unwrap();
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
