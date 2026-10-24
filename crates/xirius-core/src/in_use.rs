//! The audio files the songs open in this process name (REC-026), for the cleanup of a song
//! that was closed: it runs on a thread of its own while the next song is open, and a recording
//! it found unused may be given to that song meanwhile (from the browser, by its full path).
//! It moves a file only while no song open here has named it.
//!
//! Each session notes the files of each state as it gets to it ([`Uses::note`]): the song as it
//! is and every state it made or went back to, so what an undo or a redo can bring back too. The
//! notes go with the session. The states of a history written by an earlier run are noted only
//! once gone back to; the cleanup of another song never read them either.
//!
//! A command runs while it holds the files [`using`], and the cleanup checks a file while it
//! holds them [`alone`], twice: before it puts the file aside (a rename in its folder) and after,
//! before it moves it to the recycle bin. A command that reads a file and names it in the song
//! (an import) is then over before the first check, and the file is kept, or over before the
//! second, and the file is put back, or starts after the file was put aside, and does not find
//! it. Nothing is read from the disk while the files are held alone, so commands do not wait for
//! a slow drive: one waits for a check in memory, and for the commands already running only
//! [`BEHIND_CLEANUP`] at most.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use crate::model::{ClipBody, Project};

struct Gate {
    /// Holds of [`using`], of all threads.
    using: usize,
    /// The thread that holds [`alone`].
    alone: Option<ThreadId>,
    /// Threads waiting in [`alone`].
    pending: usize,
    /// The holds of [`using`] of the threads waiting in [`alone`]: a cleanup run from a command
    /// names nothing while it waits, so another's does not wait for it.
    parked: usize,
    /// Threads waiting in [`using`].
    waiting: usize,
    /// How many of the threads that waited in [`using`] go before the next [`alone`].
    first: usize,
}

static GATE: Mutex<Gate> = Mutex::new(Gate {
    using: 0,
    alone: None,
    pending: 0,
    parked: 0,
    waiting: 0,
    first: 0,
});
static FREED: Condvar = Condvar::new();

thread_local! {
    /// Holds of [`using`] on this thread.
    static MINE: Cell<usize> = const { Cell::new(0) };
}

fn gate() -> MutexGuard<'static, Gate> {
    GATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Held while a command runs: the cleanup does not check or move a file meanwhile. A thread may
/// hold it more than once (a command that runs others); it is let go on the thread that took it.
#[must_use]
pub struct Using(std::marker::PhantomData<*const ()>);

/// How long a command waits behind a cleanup that waits in [`alone`] for the commands running
/// then: one of them may run long (an import of a large file), and commands do not wait for each
/// other.
pub const BEHIND_CLEANUP: Duration = Duration::from_millis(50);

/// Holds the files. Waits while a cleanup checks a file (in memory), and while one waits to (it
/// waits for the commands running then, not for those that start after), [`BEHIND_CLEANUP`] at
/// most; the commands that waited go before its next check.
pub fn using() -> Using {
    let mut g = gate();
    if MINE.with(Cell::get) == 0 {
        let since = Instant::now();
        loop {
            let behind = g.pending > 0 && g.first == 0;
            if g.alone.is_none() && !behind {
                break;
            }
            g.waiting += 1;
            if g.alone.is_some() {
                g = FREED.wait(g).unwrap_or_else(|e| e.into_inner());
            } else {
                let left = BEHIND_CLEANUP.saturating_sub(since.elapsed());
                if left.is_zero() {
                    g.waiting -= 1;
                    break;
                }
                g = FREED
                    .wait_timeout(g, left)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            g.waiting -= 1;
        }
        g.first = g.first.saturating_sub(1);
    }
    g.using += 1;
    MINE.with(|m| m.set(m.get() + 1));
    Using(std::marker::PhantomData)
}

impl Drop for Using {
    fn drop(&mut self) {
        let mut g = gate();
        g.using -= 1;
        MINE.with(|m| m.set(m.get() - 1));
        drop(g);
        FREED.notify_all();
    }
}

/// Held while the cleanup checks one file in memory: no command runs meanwhile.
#[must_use]
pub struct Alone(());

/// How long a cleanup run from a command waits for the commands of other threads.
const IN_COMMAND: Duration = Duration::from_secs(5);

/// Waits until no command runs (but those of the threads waiting here, this one's too), then
/// holds the files. None when this thread is in a command and those of others do not end soon:
/// one of them may wait for something this thread holds.
pub fn alone() -> Option<Alone> {
    let me = std::thread::current().id();
    let mine = MINE.with(Cell::get);
    let since = Instant::now();
    let mut g = gate();
    g.pending += 1;
    g.parked += mine;
    let held = loop {
        if g.alone.is_none() && g.using <= g.parked && g.first == 0 {
            break true;
        }
        if mine == 0 {
            g = FREED.wait(g).unwrap_or_else(|e| e.into_inner());
            continue;
        }
        let left = IN_COMMAND.saturating_sub(since.elapsed());
        if left.is_zero() {
            break false;
        }
        g = FREED
            .wait_timeout(g, left)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    };
    g.pending -= 1;
    g.parked -= mine;
    if !held {
        drop(g);
        FREED.notify_all();
        return None;
    }
    g.alone = Some(me);
    Some(Alone(()))
}

/// How many threads wait in [`alone`] now (a test waits for a cleanup to get there).
pub fn waiting_alone() -> usize {
    gate().pending
}

impl Drop for Alone {
    fn drop(&mut self) {
        let mut g = gate();
        g.alone = None;
        g.first = g.waiting;
        drop(g);
        FREED.notify_all();
    }
}

#[derive(Default)]
struct Noted {
    /// The song folder the names were read against, and its [`key`].
    dir: Option<PathBuf>,
    dir_key: Option<String>,
    /// The names as the clips have them, so each is made a [`key`] once.
    names: HashSet<String>,
    keys: HashSet<String>,
    /// The full paths of the names by their file name in one case, and those of short names
    /// (`TAKE-1~1.WAV`) under [`SHORT`]: the same file written another way (a link to a folder,
    /// a drive letter for a share) has a [`key`] of its own, which only the disk tells apart.
    paths: HashMap<String, HashSet<PathBuf>>,
}

/// Where [`Noted::paths`] has the names that may be short ones (Windows' 8.3 names).
const SHORT: &str = "~";

fn file_name(file: &Path) -> String {
    file.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

static NOTES: Mutex<BTreeMap<u64, Noted>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

fn notes() -> MutexGuard<'static, BTreeMap<u64, Noted>> {
    NOTES.lock().unwrap_or_else(|e| e.into_inner())
}

/// The files a session named (see the module). They go when it does.
pub struct Uses {
    id: u64,
}

impl Default for Uses {
    fn default() -> Uses {
        Uses {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl Uses {
    /// What tells this session's notes from the others'.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The files `p`'s clips name; relative ones are in `dir`.
    pub fn note(&self, p: &Project, dir: Option<&Path>) {
        let mut all = notes();
        let n = all.entry(self.id).or_default();
        // The keys read against the folder before stay: a state of before may name them (a
        // relative name written before the song had its folder), and one kept too many only
        // keeps a file.
        if n.dir.as_deref() != dir {
            n.dir = dir.map(Path::to_path_buf);
            n.dir_key = dir.map(|d| key(None, d));
            n.names.clear();
        }
        for c in p.tracks.values().flat_map(|t| &t.clips) {
            if let ClipBody::Audio { file, .. } = &c.body
                && !n.names.contains(file)
            {
                let name = Path::new(file);
                n.keys.insert(key(dir, name));
                let full = match dir {
                    Some(dir) if !name.is_absolute() => dir.join(name),
                    _ => name.to_path_buf(),
                };
                let short = file_name(name).contains(SHORT);
                n.paths
                    .entry(if short { SHORT.into() } else { file_name(name) })
                    .or_default()
                    .insert(full);
                n.names.insert(file.clone());
            }
        }
    }
}

impl Drop for Uses {
    fn drop(&mut self) {
        notes().remove(&self.id);
    }
}

/// Whether a session but `except` named the file of [`key`] `key`.
pub fn named(key: &str, except: Option<u64>) -> bool {
    notes()
        .iter()
        .any(|(id, n)| Some(*id) != except && n.keys.contains(key))
}

/// The files a session but `except` named with the file name of `file` (or a short name) under
/// another [`key`] than `key_of_file` (`file`'s): maybe `file` written another way, which the
/// disk tells.
pub fn named_alike(key_of_file: &str, file: &Path, except: Option<u64>) -> Vec<PathBuf> {
    let name = file_name(file);
    let mut alike: Vec<PathBuf> = notes()
        .iter()
        .filter(|(id, _)| Some(**id) != except)
        .flat_map(|(_, n)| [n.paths.get(&name), n.paths.get(SHORT)])
        .flatten()
        .flatten()
        .filter(|p| key(None, p) != key_of_file)
        .cloned()
        .collect();
    alike.sort();
    alike.dedup();
    alike
}

/// Whether a session but `except` has the song in `dir` open: its history may name any file of
/// the song, and is not all in memory.
pub fn opened(dir: &Path, except: Option<u64>) -> bool {
    let k = key(None, dir);
    notes()
        .iter()
        .any(|(id, n)| Some(*id) != except && n.dir_key.as_deref() == Some(k.as_str()))
}

/// A path as a clip names it, made comparable: absolute under `dir`, `.` and `..` folded, a
/// drive or a share written one way (`\\?\C:` as `C:`), `/` between the parts, and in one case
/// where the file system ignores case. Nothing is read from the disk.
pub fn key(dir: Option<&Path>, file: &Path) -> String {
    let full = match dir {
        Some(dir) if !file.is_absolute() => dir.join(file),
        _ => file.to_path_buf(),
    };
    let mut parts: Vec<String> = Vec::new();
    for c in full.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::RootDir => {}
            Component::Prefix(p) => parts.push(match p.kind() {
                Prefix::Disk(d) | Prefix::VerbatimDisk(d) => format!("{}:", char::from(d)),
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    format!("/{}/{}", server.to_string_lossy(), share.to_string_lossy())
                }
                _ => p.as_os_str().to_string_lossy().into_owned(),
            }),
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    let joined = parts.join("/");
    if cfg!(any(windows, target_os = "macos")) {
        joined.to_lowercase()
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_waits_only_for_the_file_being_moved_and_goes_first() {
        let held = alone().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let t = std::thread::spawn(move || {
            let _u = using();
            // A command it runs holds them too.
            let _again = using();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
        drop(held);
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        t.join().unwrap();
        // Commands hold it together.
        let a = using();
        let b = std::thread::spawn(|| drop(using()));
        b.join().unwrap();
        drop(a);
    }

    #[test]
    fn a_command_waits_behind_a_cleanup_waiting_for_a_long_one_only_a_moment() {
        let (held, release) = std::sync::mpsc::channel::<()>();
        let (go, went) = std::sync::mpsc::channel();
        let long = std::thread::spawn(move || {
            let _u = using();
            go.send(()).unwrap();
            let _ = release.recv_timeout(Duration::from_secs(3));
        });
        went.recv().unwrap();
        let before = waiting_alone();
        let cleanup = std::thread::spawn(|| drop(alone()));
        let since = Instant::now();
        while waiting_alone() <= before && since.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let since = Instant::now();
        let other = std::thread::spawn(|| drop(using()));
        other.join().unwrap();
        let waited = since.elapsed();
        drop(held);
        long.join().unwrap();
        cleanup.join().unwrap();
        assert!(
            waited < Duration::from_secs(2),
            "it waited {waited:?} for the long command"
        );
    }

    #[test]
    fn cleanups_run_from_commands_on_two_threads_go_one_after_the_other() {
        let since = Instant::now();
        let both: Vec<_> = (0..2)
            .map(|_| {
                std::thread::spawn(|| {
                    let _u = using();
                    std::thread::sleep(Duration::from_millis(50));
                    alone().is_some()
                })
            })
            .collect();
        for t in both {
            assert!(t.join().unwrap(), "it gave up waiting for the other");
        }
        assert!(since.elapsed() < IN_COMMAND / 2);
    }

    #[test]
    fn a_session_names_what_any_of_its_states_named_until_it_goes() {
        let dir = Path::new("/songs/b");
        let mut p = Project::new("b");
        let file = "/songs/a/Media/Recordings/take-3.wav";
        let k = key(None, Path::new(file));
        let uses = Uses::default();
        uses.note(&p, Some(dir));
        assert!(!named(&k, None));
        use crate::id::{ClipId, TrackId};
        use crate::model::{Clip, Track, TrackKind};
        let t = TrackId(p.alloc());
        let mut track = Track::new(t, TrackKind::Audio, "Vox");
        track.clips.push(Clip {
            id: ClipId(p.alloc()),
            name: "take-3".into(),
            color: None,
            start: 0,
            length: 960,
            muted: false,
            take: None,
            controllers: Vec::new(),
            body: ClipBody::Audio {
                file: file.into(),
                offset: 0,
                gain_db: 0.0,
                process: Default::default(),
            },
        });
        p.tracks.insert(t, std::sync::Arc::new(track));
        uses.note(&p, Some(dir));
        assert!(named(&k, None));
        assert!(!named(&k, Some(uses.id())), "but by itself");
        assert!(opened(Path::new("/songs/./B"), None) == cfg!(any(windows, target_os = "macos")));
        assert!(opened(dir, None) && !opened(dir, Some(uses.id())));
        assert!(!opened(Path::new("/songs/a"), None));
        let k_of = |f: &str| key(None, Path::new(f));
        let other = "/links/a/Media/Recordings/TAKE-3.wav";
        assert_eq!(
            named_alike(&k_of(other), Path::new(other), None),
            [PathBuf::from(file)],
            "the same name in another folder"
        );
        assert!(
            named_alike(&k, Path::new(file), None).is_empty(),
            "but its own"
        );
        assert!(named_alike(&k_of(other), Path::new(other), Some(uses.id())).is_empty());
        let elsewhere = "/songs/a/Media/Recordings/take-4.wav";
        assert!(named_alike(&k_of(elsewhere), Path::new(elsewhere), None).is_empty());
        drop(uses);
        assert!(!named(&k, None));
        assert!(!opened(dir, None));
        assert!(named_alike(&k_of(other), Path::new(other), None).is_empty());
    }
}
