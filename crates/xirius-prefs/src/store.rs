//! The files in the settings folder, read and written as one (06 §3.5).
//!
//! The UI, the CLI and the MCP server each hold a [`Settings`]; a change one makes is written
//! to the file at once, and the others see it the next time they look ([`Settings::reload_if_changed`]).
//! A file that does not read as TOML is left alone until the next change, and copied to
//! `<name>.bak` before that change overwrites it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::chord::Os;
use crate::keymap::Keymap;
use crate::macros::Macros;
use crate::midimap::Midimap;
use crate::mousemap::Mousemap;
use crate::prefs::Prefs;

/// Which file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum File {
    Prefs,
    Keymap,
    Mousemap,
    Midimap,
    Macros,
}

impl File {
    pub const ALL: [File; 5] = [
        File::Prefs,
        File::Keymap,
        File::Mousemap,
        File::Midimap,
        File::Macros,
    ];

    pub fn name(self) -> &'static str {
        match self {
            File::Prefs => "prefs.toml",
            File::Keymap => "keymap.toml",
            File::Mousemap => "mousemap.toml",
            File::Midimap => "midimap.toml",
            File::Macros => "macros.toml",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    reads: Option<Arc<Reads>>,
    /// The folder, `None` for settings kept in memory only.
    pub dir: Option<PathBuf>,
    pub prefs: Prefs,
    pub keymap: Keymap,
    pub mousemap: Mousemap,
    pub midimap: Midimap,
    pub macros: Macros,
    /// What could not be read, per line.
    pub problems: Vec<String>,
    /// Goes up on every change, read or made.
    pub rev: u64,
    pub os: Os,
    stamps: [Option<Stamp>; File::ALL.len()],
    /// The file did not read as TOML (copied to `.bak` before it is written).
    broken: [bool; File::ALL.len()],
}

#[derive(Debug)]
struct Reads {
    ui: std::thread::ThreadId,
    busy: std::sync::atomic::AtomicBool,
    reply: Mutex<Option<(u64, Settings)>>,
    due: Mutex<std::time::Instant>,
}

pub type Shared = Arc<Mutex<Settings>>;

/// Where the settings live: `XIRIUS_CONFIG_DIR`, otherwise the folder that travels with the
/// user: `%APPDATA%\Xirius` (Roaming) on Windows, `~/Library/Application Support/Xirius` on a
/// Mac and `$XDG_CONFIG_HOME/xirius` (`~/.config/xirius`) elsewhere. What belongs to the PC
/// (the drawer's height, the plugin cache, the peaks) stays in the cache folder
/// (`%LOCALAPPDATA%\Xirius` on Windows).
///
/// Settings an earlier build saved in `%LOCALAPPDATA%\Xirius` are moved here the first time.
pub fn config_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("XIRIUS_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    let dir = user_dir();
    if let Some(old) = old_dir().filter(|old| *old != dir) {
        move_settings(&old, &dir);
    }
    dir
}

/// Resolve the location without migration/file I/O on a window thread.
pub fn config_path() -> PathBuf {
    std::env::var_os("XIRIUS_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(user_dir)
}

fn user_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("Xirius"))
        .unwrap_or_else(|| std::env::temp_dir().join("Xirius"));
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Application Support/Xirius"))
        .unwrap_or_else(|| std::env::temp_dir().join("Xirius"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|p| p.join("xirius"))
        .unwrap_or_else(|| std::env::temp_dir().join("xirius"));
    base
}

/// Where an earlier build kept the settings: `%LOCALAPPDATA%\Xirius` on Windows. On a Mac and
/// elsewhere they were in the user's folder already.
fn old_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Xirius"))
    } else {
        None
    }
}

/// Moves the files (and their `.bak`) from `from` to `to`, each only when `to` has none
/// of that name yet; everything else in `from` stays. A file that cannot be moved is left where
/// it was. Returns the names moved.
pub fn move_settings(from: &Path, to: &Path) -> Vec<String> {
    let mut moved = Vec::new();
    for f in File::ALL {
        for name in [f.name().to_string(), format!("{}.bak", f.name())] {
            let (src, dst) = (from.join(&name), to.join(&name));
            if !src.is_file() || dst.exists() || std::fs::create_dir_all(to).is_err() {
                continue;
            }
            let done = std::fs::rename(&src, &dst).is_ok()
                || (std::fs::copy(&src, &dst).is_ok() && std::fs::remove_file(&src).is_ok());
            if done {
                moved.push(name);
            }
        }
    }
    moved
}

/// How far a file's modified time may lag the change that set it. The time only moves on
/// the clock's tick (0.5 to 15.6 ms on Windows, 2 s on FAT), so a file written again within
/// the same tick keeps its time, and maybe its length.
const TICK: Duration = Duration::from_secs(2);

/// A file as it was last read or written.
#[derive(Clone, Copy, Debug)]
struct Stamp {
    modified: SystemTime,
    len: u64,
    hash: u64,
    /// When the file was looked at; while that is within [`TICK`] of `modified`, the time
    /// and the length do not tell a later change, and the contents are compared too.
    taken: SystemTime,
}

impl Stamp {
    /// The file at `path`, holding `bytes`, as it was at `taken` (or before).
    fn of(path: &Path, bytes: &[u8], taken: SystemTime) -> Option<Stamp> {
        let m = std::fs::metadata(path).ok()?;
        Some(Stamp {
            modified: m.modified().ok()?,
            len: m.len(),
            hash: hash(bytes),
            taken,
        })
    }

    /// Whether the file at `path` still is the one stamped.
    fn holds(&mut self, path: &Path) -> bool {
        let Ok(m) = std::fs::metadata(path) else {
            return false;
        };
        if m.modified().ok() != Some(self.modified) || m.len() != self.len {
            return false;
        }
        if self
            .taken
            .duration_since(self.modified)
            .is_ok_and(|d| d >= TICK)
        {
            return true;
        }
        let now = SystemTime::now();
        if std::fs::read(path).map(|b| hash(&b)).ok() != Some(self.hash) {
            return false;
        }
        self.taken = now;
        true
    }
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{DefaultHasher, Hasher};
    let mut h = DefaultHasher::new();
    h.write(bytes);
    h.finish()
}

impl Settings {
    pub fn on_ui_thread(&self) -> bool {
        self.reads
            .as_ref()
            .is_some_and(|reads| reads.ui == std::thread::current().id())
    }
    pub fn command_snapshot(shared: &Shared) -> Self {
        let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
        if guard.on_ui_thread() {
            guard.reload_if_changed();
            return guard.clone();
        }
        let mut next = guard.worker_copy();
        drop(guard);
        next.reload_if_changed();
        next
    }
    pub fn defer_window_reads(&mut self) {
        self.reads = Some(Arc::new(Reads {
            ui: std::thread::current().id(),
            busy: Default::default(),
            reply: Default::default(),
            due: Mutex::new(std::time::Instant::now()),
        }));
    }
    pub fn worker_copy(&self) -> Self {
        let mut copy = self.clone();
        copy.reads = None;
        copy
    }
    pub fn adopt_worker(&mut self, mut next: Self) {
        next.reads = self.reads.clone();
        *self = next;
    }
    /// Settings in memory only (tests, scripted runs).
    pub fn memory() -> Settings {
        Settings {
            reads: None,
            dir: None,
            prefs: Prefs::default(),
            keymap: Keymap::default(),
            mousemap: Mousemap::default(),
            midimap: Midimap::default(),
            macros: Macros::default(),
            problems: Vec::new(),
            rev: 0,
            os: Os::current(),
            stamps: [None; File::ALL.len()],
            broken: [false; File::ALL.len()],
        }
    }

    /// Reads the folder (a missing file is the defaults).
    pub fn open(dir: impl Into<PathBuf>) -> Settings {
        let mut s = Settings::memory();
        s.dir = Some(dir.into());
        s.read_all();
        s
    }

    pub fn shared(self) -> Shared {
        Arc::new(Mutex::new(self))
    }

    pub fn path(&self, f: File) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(f.name()))
    }

    fn read_all(&mut self) {
        self.problems.clear();
        for f in File::ALL {
            self.read(f);
        }
        self.rev += 1;
    }

    fn read(&mut self, f: File) {
        let Some(path) = self.path(f) else { return };
        self.broken[f.index()] = false;
        let taken = SystemTime::now();
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                self.stamps[f.index()] = Stamp::of(&path, &[], taken);
                self.problems.push(format!("{}: {e}", f.name()));
                return;
            }
        };
        self.stamps[f.index()] = Stamp::of(&path, &bytes, taken);
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(e) => {
                let e = std::io::Error::new(std::io::ErrorKind::InvalidData, e);
                self.problems.push(format!("{}: {e}", f.name()));
                return;
            }
        };
        let table: toml::Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                if f == File::Keymap {
                    self.keymap = crate::lines::keymap(&text, &mut self.problems);
                    self.broken[f.index()] = true;
                    return;
                }
                let first = e.to_string().lines().next().unwrap_or("").to_string();
                self.problems.push(format!(
                    "{} を読めません（既定の値を使います）: {first}",
                    f.name()
                ));
                self.broken[f.index()] = true;
                return;
            }
        };
        let p = &mut self.problems;
        match f {
            File::Prefs => self.prefs = Prefs::from_toml(&table, p),
            File::Keymap => self.keymap = Keymap::from_toml(&table, p),
            File::Mousemap => self.mousemap = Mousemap::from_toml(&table, p),
            File::Midimap => self.midimap = Midimap::from_toml(&table, p),
            File::Macros => self.macros = Macros::from_toml(&table, p),
        }
    }

    /// Reads again the files another program changed. True when something was read.
    pub fn reload_if_changed(&mut self) -> bool {
        if let Some(reads) = self.reads.clone()
            && std::thread::current().id() == reads.ui
        {
            let mut changed = false;
            let reply = reads.reply.try_lock().ok().and_then(|mut r| r.take());
            if let Some((rev, next)) = reply
                && self.rev == rev
                && next.rev != rev
            {
                self.adopt_worker(next);
                changed = true;
            }
            use std::sync::atomic::Ordering;
            let due = reads
                .due
                .try_lock()
                .is_ok_and(|due| std::time::Instant::now() >= *due);
            if self.dir.is_some() && due && !reads.busy.swap(true, Ordering::AcqRel) {
                let mut next = self.worker_copy();
                let rev = self.rev;
                let worker = reads.clone();
                let spawned = std::thread::Builder::new()
                    .name("settings-read".into())
                    .spawn(move || {
                        if next.dir.as_deref() == Some(config_path().as_path()) {
                            let _ = config_dir();
                        }
                        next.reload_if_changed();
                        #[cfg(feature = "test-io")]
                        if let Some(dir) = &next.dir {
                            let _ = xirius_core::io_test::before("settings_read_ready", dir);
                        }
                        *worker.reply.lock().unwrap_or_else(|e| e.into_inner()) = Some((rev, next));
                        *worker.due.lock().unwrap_or_else(|e| e.into_inner()) =
                            std::time::Instant::now() + Duration::from_millis(250);
                        worker.busy.store(false, Ordering::Release);
                    });
                if spawned.is_err() {
                    self.problems.push("設定の読み込みを開始できません".into());
                    // No synchronous fallback or repeated thread-exhaustion loop.
                }
            }
            return changed;
        }
        let Some(dir) = self.dir.clone() else {
            return false;
        };
        #[cfg(feature = "test-io")]
        let _ = xirius_core::io_test::before("settings_read", &dir);
        let changed = File::ALL.iter().any(|f| {
            let path = dir.join(f.name());
            match &mut self.stamps[f.index()] {
                Some(s) => !s.holds(&path),
                None => path.exists(),
            }
        });
        if changed {
            self.read_all();
        }
        changed
    }

    /// Writes `f` (a new file, then renamed over the old one).
    pub fn save(&mut self, f: File) -> Result<(), String> {
        if self
            .reads
            .as_ref()
            .is_some_and(|reads| reads.ui == std::thread::current().id())
        {
            return Err("設定の保存は裏で行う必要があります".into());
        }
        self.rev += 1;
        let Some(path) = self.path(f) else {
            return Ok(());
        };
        let text = match f {
            File::Prefs => self.prefs.to_toml(),
            File::Keymap => self.keymap.to_toml(),
            File::Mousemap => self.mousemap.to_toml(),
            File::Midimap => self.midimap.to_toml(),
            File::Macros => self.macros.to_toml(),
        };
        let err = |e: std::io::Error| format!("{} を保存できません: {e}", path.display());
        #[cfg(feature = "test-io")]
        xirius_core::io_test::before("settings_write", &path).map_err(err)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        if self.broken[f.index()] {
            let bak = path.with_extension("toml.bak");
            std::fs::copy(&path, &bak).map_err(err)?;
            self.broken[f.index()] = false;
            self.problems.retain(|p| !p.starts_with(f.name()));
        }
        let tmp = path.with_extension("toml.tmp");
        let taken = SystemTime::now();
        std::fs::write(&tmp, &text).map_err(err)?;
        std::fs::rename(&tmp, &path).map_err(err)?;
        self.stamps[f.index()] = Stamp::of(&path, text.as_bytes(), taken);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "test-io")]
    #[test]
    fn completed_old_read_is_rejected_after_explicit_write() {
        use std::sync::{Condvar, mpsc};
        let root = tempfile::tempdir().unwrap();
        let mut settings = Settings::open(root.path());
        settings.defer_window_reads();
        // The held read must contain a real external change, rather than an
        // unchanged snapshot which would be ignored even without revision checks.
        std::fs::write(root.path().join("prefs.toml"), "[ui]\ntheme = 'dark'\n").unwrap();
        let reads = settings.reads.clone().unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(gate.clone());
        let path = root.path().to_owned();
        let (tx, entered) = mpsc::channel();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, file| {
            if op == "settings_read_ready" && file == path {
                let _ = tx.send(());
                drop(
                    gate.1
                        .wait_while(gate.0.lock().unwrap(), |done| !*done)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        settings.reload_if_changed();
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        let mut explicit = settings.worker_copy();
        explicit
            .prefs
            .set("ui.theme", &serde_json::json!("light"))
            .unwrap();
        std::thread::spawn(move || {
            explicit.save(File::Prefs).unwrap();
            explicit
        })
        .join()
        .map(|next| settings.adopt_worker(next))
        .unwrap();
        drop(release);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while reads.busy.load(std::sync::atomic::Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        {
            let reply = reads.reply.lock().unwrap();
            let (base, old) = reply.as_ref().unwrap();
            assert_ne!(old.rev, *base);
            assert_eq!(old.prefs.text("ui.theme"), "dark");
        }
        settings.reload_if_changed();
        assert_eq!(settings.prefs.text("ui.theme"), "light");
    }
    use crate::chord::Chord;
    use crate::keymap::Others;
    use serde_json::json;

    /// A folder of the test's own, removed when dropped (also when the test fails).
    fn temp(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("xirius-prefs-{name}-"))
            .tempdir()
            .unwrap()
    }

    #[test]
    fn settings_in_the_old_folder_move_to_the_new_one() {
        let (old_tmp, new_tmp) = (temp("old"), temp("new"));
        let (old, new) = (old_tmp.path(), new_tmp.path());
        std::fs::write(old.join("prefs.toml"), "a = 1\n").unwrap();
        std::fs::write(old.join("prefs.toml.bak"), "{\n").unwrap();
        std::fs::write(old.join("keymap.toml"), "old\n").unwrap();
        std::fs::write(new.join("keymap.toml"), "new\n").unwrap();
        // What belongs to the PC stays where it is.
        std::fs::write(old.join("view.json"), "{}").unwrap();
        std::fs::write(old.join("plugins.json"), "{}").unwrap();

        let moved = move_settings(old, new);
        assert_eq!(moved, ["prefs.toml", "prefs.toml.bak"]);
        assert_eq!(
            std::fs::read_to_string(new.join("prefs.toml")).unwrap(),
            "a = 1\n"
        );
        assert!(!old.join("prefs.toml").exists());
        // A file already in the new folder is not overwritten; the old one is left.
        assert_eq!(
            std::fs::read_to_string(new.join("keymap.toml")).unwrap(),
            "new\n"
        );
        assert!(old.join("keymap.toml").exists());
        assert!(old.join("view.json").exists() && old.join("plugins.json").exists());
        assert!(!new.join("view.json").exists());
        // The second time there is nothing left to move.
        assert!(move_settings(old, new).is_empty());
    }

    #[test]
    fn a_missing_new_folder_is_made() {
        let (old_tmp, new_tmp) = (temp("old2"), temp("new2"));
        let old = old_tmp.path();
        // The new folder is not there yet.
        let new = new_tmp.path().join("new");
        std::fs::write(old.join("mousemap.toml"), "").unwrap();
        assert_eq!(move_settings(old, &new), ["mousemap.toml"]);
        assert!(new.join("mousemap.toml").is_file());
        assert!(move_settings(&new.join("none"), &old.join("x")).is_empty());
        assert!(!old.join("x").exists());
    }

    #[test]
    fn a_change_is_there_the_next_time() {
        let dir_tmp = temp("next");
        let dir = dir_tmp.path().to_path_buf();
        let mut a = Settings::open(&dir);
        a.prefs
            .set("edit.automation.follows-clips", &json!(false))
            .unwrap();
        a.save(File::Prefs).unwrap();
        let q = Chord::parse("Q", Os::Win).unwrap();
        a.keymap.set("roll", &q, "note.mute", None, Others::Remove);
        a.save(File::Keymap).unwrap();
        a.mousemap
            .set("roll.select.hold", "Alt", "tool:erase")
            .unwrap();
        a.save(File::Mousemap).unwrap();

        let b = Settings::open(&dir);
        assert!(b.problems.is_empty(), "{:?}", b.problems);
        assert!(!b.prefs.bool("edit.automation.follows-clips"));
        assert_eq!(b.keymap, a.keymap);
        assert_eq!(b.mousemap, a.mousemap);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_programs_change_is_read() {
        let dir_tmp = temp("reload");
        let dir = dir_tmp.path().to_path_buf();
        let mut ui = Settings::open(&dir);
        assert!(!ui.reload_if_changed());
        let mut cli = Settings::open(&dir);
        cli.prefs.set("ui.theme", &json!("light")).unwrap();
        cli.save(File::Prefs).unwrap();
        assert!(ui.reload_if_changed());
        assert_eq!(ui.prefs.text("ui.theme"), "light");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_change_that_keeps_the_time_and_the_length_is_read() {
        let dir_tmp = temp("same-tick");
        let dir = dir_tmp.path().to_path_buf();
        let path = dir.join(File::Keymap.name());
        std::fs::write(&path, "[roll]\n'F9' = 'tool.erase'\n").unwrap();
        let mut ui = Settings::open(&dir);
        let f8 = Chord::parse("F8", Os::Win).unwrap();
        assert!(
            ui.keymap
                .lookup("roll", &f8)
                .is_none_or(|x| x.1 != "tool.erase")
        );
        // Written again within the clock's tick: the same time, the same length.
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "[roll]\n'F8' = 'tool.erase'\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(ui.reload_if_changed());
        assert_eq!(ui.keymap.lookup("roll", &f8).unwrap().1, "tool.erase");
        assert!(!ui.reload_if_changed());
    }

    #[test]
    fn a_file_that_does_not_read_is_kept_as_bak() {
        let dir_tmp = temp("bak");
        let dir = dir_tmp.path().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prefs.toml"), "[ui\ntheme = ").unwrap();
        let mut s = Settings::open(&dir);
        assert_eq!(s.problems.len(), 1, "{:?}", s.problems);
        assert_eq!(s.prefs.text("ui.theme"), "dark");
        s.prefs.set("ui.theme", &json!("light")).unwrap();
        s.save(File::Prefs).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("prefs.toml.bak")).unwrap(),
            "[ui\ntheme = "
        );
        assert!(s.problems.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
