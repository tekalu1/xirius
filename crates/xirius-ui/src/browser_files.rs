//! The file tab of the browser, on the disk (BRW-002, BRW-005, BRW-006, BRW-008, BRW-015).
//!
//! The first tab is the song's folder (「この曲」), then the places the user added; what the
//! browser keeps (the places, favourite and hidden instruments) is the user's file
//! [`xirius_session::places`], so the commands `location.*` and `browser.*` change the same
//! tabs. A folder lists its folders, then the audio and MIDI files the song takes, each by
//! name; dot files and the song's own folders (history, peaks) are left out. A listing is read
//! off this thread at most once a second; the length of each file is also read there and shown when it
//! is known. A search looks through the folder shown and the folders in it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(test)]
use xirius_session::places;
use xirius_session::places::{Place, Prefs, SONG};

use crate::browser::{BRow, Browser};

/// How long a listing is used before the folder is read again.
const FRESH: Duration = Duration::from_secs(1);
/// How deep a search goes below the folder shown.
const SEARCH_DEPTH: usize = 8;
/// How many entries a search looks at, and how many it lists.
const SEARCH_SEEN: usize = 20_000;
const SEARCH_FOUND: usize = 500;
/// Folders of the song folder that are the song's own.
const SONG_OWN: [&str; 2] = ["history", xirius_session::peaks::FOLDER];

/// What the song tab says before the song has a folder.
pub const NO_FOLDER: &str = "曲のフォルダーはまだありません";
/// What a place whose folder is gone says.
pub const GONE: &str = "フォルダーが見つかりません";
/// What a folder with nothing the song takes says.
pub const EMPTY: &str = "音声ファイルや MIDI ファイルはありません";

/// A tab of the file tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub name: String,
    /// `None` for the song's tab of a song not saved yet.
    pub path: Option<PathBuf>,
    /// The song's folder: it cannot be taken away.
    pub song: bool,
}

/// A folder or a file of a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
}

pub fn is_midi(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mid" | "midi"))
}

/// Files the song takes: audio files and MIDI files.
pub fn takes(path: &Path) -> bool {
    crate::drop_audio::is_audio(path) || is_midi(path)
}

fn read(dir: &Path) -> Option<Vec<Entry>> {
    #[cfg(test)]
    xirius_core::io_test::before("browser_top", dir).ok()?;
    if !dir.is_dir() {
        return None;
    }
    #[cfg(test)]
    let listing =
        xirius_core::io_test::before("browser_read_dir", dir).and_then(|()| std::fs::read_dir(dir));
    #[cfg(not(test))]
    let listing = std::fs::read_dir(dir);
    let Ok(rd) = listing else {
        return Some(Vec::new());
    };
    let mut out: Vec<Entry> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            #[cfg(test)]
            let kind = xirius_core::io_test::before("browser_file_type", &path)
                .and_then(|()| e.file_type());
            #[cfg(not(test))]
            let kind = e.file_type();
            let dir = match kind {
                Ok(kind) if !kind.is_symlink() => kind.is_dir(),
                Ok(_) => {
                    #[cfg(test)]
                    if xirius_core::io_test::before("browser_link", &path).is_err() {
                        return takes(&path).then_some(Entry {
                            name,
                            path,
                            dir: false,
                        });
                    }
                    path.is_dir()
                }
                Err(_) => {
                    #[cfg(test)]
                    let _ = xirius_core::io_test::before("browser_type_fallback", &path);
                    path.is_dir()
                }
            };
            (dir || takes(&path)).then_some(Entry { name, path, dir })
        })
        .collect();
    out.sort_by_cached_key(|e| (!e.dir, e.name.to_lowercase(), e.name.clone()));
    Some(out)
}

struct Listed {
    at: Instant,
    entries: Arc<Vec<Entry>>,
    gone: bool,
    pending: Option<u64>,
}

#[derive(Default)]
struct Listings {
    root: Option<PathBuf>,
    dirs: HashMap<PathBuf, Listed>,
    jobs: xirius_session::read_jobs::ReadJobs<PathBuf, Option<Vec<Entry>>>,
    seen: HashSet<PathBuf>,
}

impl Listings {
    fn poll(&mut self) -> bool {
        let mut changed = false;
        for (path, listed) in &mut self.dirs {
            if let Some(id) = listed.pending
                && let Some(entries) = self.jobs.take(path, id).map(|r| r.unwrap_or(None))
            {
                listed.gone = entries.is_none();
                listed.entries = Arc::new(entries.unwrap_or_default());
                listed.pending = None;
                listed.at = Instant::now();
                changed = true;
            }
        }
        self.jobs.retain(|p| self.dirs.contains_key(p));
        changed
            || self
                .dirs
                .values()
                .any(|l| l.pending.is_none() && l.at.elapsed() >= FRESH)
    }

    fn list(&mut self, dir: &Path) -> Arc<Vec<Entry>> {
        self.seen.insert(dir.to_path_buf());
        let listed = self
            .dirs
            .entry(dir.to_path_buf())
            .or_insert_with(|| Listed {
                at: Instant::now() - FRESH,
                entries: Arc::new(Vec::new()),
                gone: false,
                pending: None,
            });
        if !crate::opening::cached_paint()
            && listed.pending.is_none()
            && listed.at.elapsed() >= FRESH
        {
            let path = dir.to_path_buf();
            listed.pending = self.jobs.request(path.clone(), move || read(&path));
            if listed.pending.is_some() {
                listed.at = Instant::now();
            }
        }
        listed.entries.clone()
    }
}

thread_local! {
    static LISTS: RefCell<Listings> = RefCell::new(Listings::default());
    static LENGTHS: RefCell<Option<Lengths>> = const { RefCell::new(None) };
}

/// What `dir` holds, or an empty listing until the worker returns. A stale listing
/// stays visible while it is refreshed; this function never waits for disk I/O.
pub fn list(dir: &Path) -> Arc<Vec<Entry>> {
    LISTS.with(|l| l.borrow_mut().list(dir))
}

/// The listings are read again on the next look.
pub fn forget() {
    LISTS.with(|l| {
        let mut l = l.borrow_mut();
        l.root = None;
        l.dirs.clear();
        l.jobs.retain(|_| false);
    });
}

/// Waits for all listings the test browser currently needs, including search descendants.
#[cfg(test)]
pub(crate) fn settle_rows(browser: &Browser) {
    if browser.tab != crate::browser::RailTab::Files {
        return;
    }
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        poll();
        browser.rows();
        if LISTS.with(|l| {
            l.borrow()
                .dirs
                .values()
                .all(|l| l.pending.is_none() && l.at.elapsed() < FRESH)
        }) {
            return;
        }
        assert!(Instant::now() < until, "browser listing never returned");
        std::thread::yield_now();
    }
}

/// `dir`'s listing as the tab shows it: at the top of the song folder, without its own.
fn shown(dir: &Path, song_top: bool) -> Vec<Entry> {
    list(dir)
        .iter()
        .filter(|e| !(song_top && e.dir && SONG_OWN.contains(&e.name.as_str())))
        .cloned()
        .collect()
}

/// The files under `dir` whose name has `q` in it (`q` in lower case).
fn search(
    dir: &Path,
    q: &str,
    depth: usize,
    song_top: bool,
    seen: &mut usize,
    out: &mut Vec<Entry>,
) {
    for e in shown(dir, song_top) {
        if *seen >= SEARCH_SEEN || out.len() >= SEARCH_FOUND {
            return;
        }
        *seen += 1;
        if e.dir {
            if depth < SEARCH_DEPTH {
                search(&e.path, q, depth + 1, false, seen, out);
            }
        } else if e.name.to_lowercase().contains(q) {
            out.push(e);
        }
    }
}

// --------------------------------------------------------------------- lengths

/// A file whose length took longer than this to read makes its folder a slow one: the rest of
/// its files are guessed from their size until their rows are on screen (BRW-022). One read
/// quicker than this makes it a quick one again.
pub const SLOW: Duration = Duration::from_millis(300);
/// Bytes a second of an uncompressed file of a kind nothing was measured of in its folder yet:
/// 44.1 kHz, 16 bit, stereo. A compressed kind is not guessed without a measure.
const GUESS_RATE: f64 = 176_400.0;
/// The kinds of file whose bytes a second follow from the sound alone.
const UNCOMPRESSED: [&str; 5] = ["wav", "wave", "aif", "aiff", "aifc"];

/// The kind of `path`, as its extension in lower case.
fn kind(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// What the length thread says of a file.
enum Told {
    /// Read from the file.
    Exact(String),
    /// Guessed from its size, in a slow folder; read when its row is on screen.
    Guess(String),
}

/// How the length thread reads a file: its length as the row shows it and in seconds, and
/// its size. The system's in the app; a test puts in a slow one.
pub struct Measure {
    pub length: LengthOf,
    pub size: SizeOf,
}

/// A file's length as the row shows it, and in seconds when it is audio.
pub type LengthOf = Box<dyn Fn(&Path) -> (String, Option<f64>) + Send>;
/// A file's size in bytes.
pub type SizeOf = Box<dyn Fn(&Path) -> Option<u64> + Send>;

impl Measure {
    fn system() -> Measure {
        Measure {
            length: Box::new(|p| {
                if is_midi(p) {
                    return (length_text(p), None);
                }
                match xirius_session::decode::probe(p) {
                    Ok(i) => (clock(i.seconds()), Some(i.seconds())),
                    Err(_) => (String::new(), None),
                }
            }),
            size: Box::new(|p| std::fs::metadata(p).map(|m| m.len()).ok()),
        }
    }
}

/// The lengths of files, read by a thread of their own.
struct Lengths {
    known: HashMap<PathBuf, String>,
    /// Those of `known` that are guesses.
    guessed: HashSet<PathBuf>,
    /// Those of `guessed` asked for again (their rows came on screen), until the answer is back.
    again: HashSet<PathBuf>,
    asked: HashSet<PathBuf>,
    /// The file rows on screen, which the thread reads even in a slow folder.
    seen: Arc<Mutex<HashSet<PathBuf>>>,
    ask: Sender<PathBuf>,
    told: Receiver<(PathBuf, Told)>,
}

impl Lengths {
    fn new(m: Measure) -> Lengths {
        let (ask, asks) = channel::<PathBuf>();
        let (tell, told) = channel();
        let seen = Arc::new(Mutex::new(HashSet::new()));
        let on_screen = seen.clone();
        std::thread::Builder::new()
            .name("browser-lengths".into())
            .spawn(move || {
                // Bytes a second of the files of each kind measured in a folder, and the slow
                // folders.
                let mut rates: HashMap<(PathBuf, String), f64> = HashMap::new();
                let mut slow: HashSet<PathBuf> = HashSet::new();
                for path in asks {
                    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                    let shown = on_screen
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .contains(&path);
                    let key = (dir, kind(&path));
                    let told = if slow.contains(&key.0) && !shown {
                        let rate = rates.get(&key).copied().or_else(|| {
                            UNCOMPRESSED.contains(&key.1.as_str()).then_some(GUESS_RATE)
                        });
                        Told::Guess(guess(&path, (m.size)(&path), rate))
                    } else {
                        let start = Instant::now();
                        let (text, secs) = (m.length)(&path);
                        if start.elapsed() > SLOW {
                            slow.insert(key.0.clone());
                        } else {
                            slow.remove(&key.0);
                        }
                        if let (Some(s), Some(b)) = (secs.filter(|s| *s > 0.0), (m.size)(&path)) {
                            rates.insert(key, b as f64 / s);
                        }
                        Told::Exact(text)
                    };
                    if tell.send((path, told)).is_err() {
                        break;
                    }
                }
            })
            .expect("start the thread that reads file lengths");
        Lengths {
            known: HashMap::new(),
            guessed: HashSet::new(),
            again: HashSet::new(),
            asked: HashSet::new(),
            seen,
            ask,
            told,
        }
    }
}

/// `約 1:23` for a file of `size` bytes at `rate` bytes a second; nothing for a MIDI file, one
/// whose size cannot be had or one with no rate to go by.
fn guess(path: &Path, size: Option<u64>, rate: Option<f64>) -> String {
    match (size, rate) {
        (Some(b), Some(r)) if !is_midi(path) && r > 0.0 => format!("約 {}", clock(b as f64 / r)),
        _ => String::new(),
    }
}

fn lengths<R>(f: impl FnOnce(&mut Lengths) -> R) -> R {
    LENGTHS.with(|l| {
        let mut l = l.borrow_mut();
        f(l.get_or_insert_with(|| Lengths::new(Measure::system())))
    })
}

/// Reads the lengths of this thread's rows with `m` from now on (a test's slow place).
#[cfg(test)]
pub(crate) fn measure_with(m: Measure) {
    LENGTHS.with(|l| *l.borrow_mut() = Some(Lengths::new(m)));
}

/// How long `path` is, as the row shows it; empty until it is known, a guess in a slow folder
/// until its row is on screen.
pub fn length(path: &Path) -> String {
    lengths(|l| {
        if let Some(t) = l.known.get(path) {
            return t.clone();
        }
        if l.asked.insert(path.to_path_buf()) {
            let _ = l.ask.send(path.to_path_buf());
        }
        String::new()
    })
}

/// The file rows on screen now: those only guessed are read.
pub fn seen<'a>(paths: impl Iterator<Item = &'a Path>) {
    lengths(|l| {
        let now: HashSet<PathBuf> = paths.map(Path::to_path_buf).collect();
        // The thread is told what is on screen before it is asked, or it guesses again.
        {
            let mut seen = l.seen.lock().unwrap_or_else(|e| e.into_inner());
            if *seen != now {
                seen.clone_from(&now);
            }
        }
        for p in &now {
            // Asked again once, until the answer is back.
            if l.guessed.contains(p) && l.again.insert(p.clone()) {
                let _ = l.ask.send(p.clone());
            }
        }
    })
}

/// Takes the lengths read since the last call; true when a row should be drawn again.
pub fn poll() -> bool {
    let listed = LISTS.with(|l| l.borrow_mut().poll());
    lengths(|l| {
        let mut got = false;
        while let Ok((path, told)) = l.told.try_recv() {
            let text = match told {
                Told::Exact(t) => {
                    l.guessed.remove(&path);
                    l.again.remove(&path);
                    t
                }
                Told::Guess(t) => {
                    // A reading is not undone by a guess.
                    if l.known.contains_key(&path) && !l.guessed.contains(&path) {
                        continue;
                    }
                    // Guessed again (the row went off screen first): asked again when it is
                    // back on screen.
                    l.again.remove(&path);
                    l.guessed.insert(path.clone());
                    t
                }
            };
            l.known.insert(path, text);
            got = true;
        }
        got || listed
    })
}

/// Waits (up to `limit`) until every length asked for is known, read or guessed.
pub fn settle(limit: Duration) {
    let start = Instant::now();
    loop {
        poll();
        if lengths(|l| l.asked.iter().all(|p| l.known.contains_key(p))) || start.elapsed() > limit {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// `0.4 秒` under a second (a drum hit is not `0:00`), then `0:24`, `12:05`, `1:02:03`.
pub fn clock(secs: f64) -> String {
    let secs = secs.max(0.0);
    if secs < 0.95 {
        return format!("{:.1} 秒", secs.max(0.1));
    }
    let s = secs.round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The length of a file as the row shows it: minutes and seconds, bars for a MIDI file
/// (four beats a bar); empty when it cannot be read.
pub fn length_text(path: &Path) -> String {
    if is_midi(path) {
        const PPQ: u32 = 960;
        let Ok(notes) = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| crate::smf::read(&b, PPQ))
        else {
            return String::new();
        };
        let end = notes.iter().map(|n| n.at + n.length).max().unwrap_or(0);
        let bar = i64::from(PPQ) * 4;
        return format!("{} 小節", ((end + bar - 1) / bar).max(1));
    }
    xirius_session::decode::probe(path)
        .map(|i| clock(i.seconds()))
        .unwrap_or_default()
}

// ------------------------------------------------------------------- the state

impl Browser {
    /// What the browser keeps, as it is now.
    pub fn prefs(&self) -> Prefs {
        Prefs {
            favourites: self.favourites.clone(),
            hidden: self.hidden.clone(),
            show_hidden: self.show_hidden,
            places: self.places.clone(),
        }
    }

    pub(crate) fn apply(&mut self, p: Prefs) {
        self.prefs_io.base = Some(p.clone());
        let here = self.location().name;
        self.favourites = p.favourites;
        self.hidden = p.hidden;
        self.show_hidden = p.show_hidden;
        self.places = p.places;
        // The tab shown stays when it is still there.
        let i = self.locations().iter().position(|l| l.name == here);
        if i != Some(self.loc) {
            self.set_location(i.unwrap_or(0));
        }
    }

    /// The browser keeps what it keeps in `file` from now on, starting with what is there.
    #[cfg(test)]
    pub(crate) fn use_prefs_fixture(&mut self, file: PathBuf) -> Result<(), String> {
        let p = places::load(&file);
        self.prefs_file = Some(file);
        self.apply(p?);
        Ok(())
    }

    /// Reads the file again when another program (the CLI, MCP, a command) changed it.
    pub fn reload_prefs(&mut self) -> bool {
        self.poll_prefs_io()
    }

    /// Coalesces writes and serializes them with preference reads and place validation.
    fn save_prefs(&mut self) {
        self.save_prefs_later();
    }

    pub fn toggle_favourite(&mut self, name: &str) {
        if !self.favourites.remove(name) {
            self.favourites.insert(name.to_string());
        }
        self.save_prefs();
    }

    /// Leaves an instrument or an effect out of the lists, or puts it back.
    pub fn set_hidden(&mut self, name: &str, on: bool) {
        if on {
            self.hidden.insert(name.to_string());
        } else {
            self.hidden.remove(name);
        }
        self.save_prefs();
    }

    /// Shows the items left out of the lists, or leaves them out again.
    pub fn set_show_hidden(&mut self, on: bool) {
        self.show_hidden = on;
        self.save_prefs();
    }

    // ------------------------------------------------------------- places

    /// Gives the tab `i` another name (not the song's).
    pub fn rename_place(&mut self, i: usize, name: &str) -> Result<String, String> {
        let Some(old) = i.checked_sub(1).and_then(|k| self.places.get(k)) else {
            return Err("曲のフォルダー（この曲）の名前は変えられません".into());
        };
        let mut p = self.prefs();
        let old = old.name.clone();
        p.rename_place(&old, name)?;
        self.places = p.places;
        self.save_prefs();
        Ok(name.trim().to_string())
    }

    /// The song's folder (`None` before it is saved). The song tab starts again at its top
    /// when it changes.
    pub fn set_song_dir(&mut self, dir: Option<PathBuf>) {
        if self.song_dir == dir {
            return;
        }
        self.song_dir = dir;
        if self.loc == 0 {
            self.set_location(0);
        }
    }

    /// The tabs: the song's folder, then the places.
    pub fn locations(&self) -> Vec<Location> {
        let mut out = vec![Location {
            name: SONG.to_string(),
            path: self.song_dir.clone(),
            song: true,
        }];
        out.extend(self.places.iter().map(|p| Location {
            name: p.name.clone(),
            path: Some(p.path.clone()),
            song: false,
        }));
        out
    }

    pub fn location(&self) -> Location {
        let mut all = self.locations();
        all.swap_remove(self.loc.min(all.len() - 1))
    }

    /// Script/test fixture setup only. Interactive places use the bounded
    /// asynchronous validation in `add_place_later`.
    pub(crate) fn add_place_fixture(&mut self, dir: &Path) -> Result<String, String> {
        let mut p = self.prefs();
        let name = p.add_place(dir, None)?;
        self.places = p.places;
        self.save_prefs();
        self.set_location(self.places.len());
        Ok(name)
    }

    /// Takes the tab `i` away (not the song's); the folder stays as it is.
    pub fn remove_place(&mut self, i: usize) -> Result<Place, String> {
        if i == 0 {
            return Err("曲のフォルダー（この曲）は外せません".into());
        }
        if i > self.places.len() {
            return Err("その場所のタブはありません".into());
        }
        let gone = self.places.remove(i - 1);
        self.save_prefs();
        if self.loc >= i {
            self.set_location(self.loc.saturating_sub(1).min(self.places.len()));
        }
        Ok(gone)
    }

    /// The folder the list shows.
    pub fn here_dir(&self) -> Option<PathBuf> {
        let mut dir = self.location().path?;
        dir.extend(&self.path);
        Some(dir)
    }

    pub fn set_location(&mut self, i: usize) {
        self.pool_heading = None;
        if i <= self.places.len() {
            forget();
            self.loc = i;
            self.path.clear();
            self.selected_file = None;
            self.playing = None;
            self.scroll = 0.0;
        }
    }

    /// Back to the first `n` parts of the breadcrumb (0 is the location itself).
    pub fn go_up_to(&mut self, n: usize) {
        self.pool_heading = None;
        forget();
        self.path.truncate(n);
        self.scroll = 0.0;
    }

    pub fn toggle_dir(&mut self, key: &str) {
        if !self.open_dirs.remove(key) {
            self.open_dirs.insert(key.to_string());
        }
    }

    /// Selecting an audio file starts its audition (BRW-007); another file stops the one
    /// that played.
    pub fn select_file(&mut self, key: &str) {
        self.pool_heading = None;
        self.selected_file = Some(key.to_string());
        self.playing = crate::drop_audio::is_audio(Path::new(key)).then(|| key.to_string());
    }

    pub fn toggle_audition(&mut self) {
        let Some(key) = self.selected_file.clone() else {
            return;
        };
        self.playing = if self.playing.as_deref() == Some(&key) {
            None
        } else {
            Some(key)
        };
    }

    /// The folder `key` (a folder row's) becomes the one the list shows.
    pub fn enter_dir(&mut self, key: &str) {
        self.pool_heading = None;
        let Some(here) = self.here_dir() else {
            return;
        };
        if let Ok(rest) = Path::new(key).strip_prefix(&here) {
            forget();
            self.path.extend(
                rest.components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned()),
            );
            self.scroll = 0.0;
        }
    }

    // --------------------------------------------------------------- rows

    pub(crate) fn file_rows(&self) -> Vec<BRow> {
        let loc = self.location();
        let Some(top) = loc.path.clone() else {
            forget();
            return vec![BRow::Note(NO_FOLDER)];
        };
        let here = self.here_dir().unwrap_or_else(|| top.clone());
        LISTS.with(|l| {
            let mut l = l.borrow_mut();
            l.seen.clear();
            if l.root.as_deref() != Some(&here) {
                l.dirs.clear();
                l.jobs.retain(|_| false);
                l.root = Some(here.clone());
            }
        });
        // main checked the location's top, not the current child folder. A
        // missing/unreadable child is empty while a missing top says GONE.
        if here != top {
            list(&top);
        }
        let song_top = loc.song && self.path.is_empty();
        let q = self.search.trim().to_lowercase();
        let mut rows = Vec::new();
        if q.is_empty() {
            self.tree_rows(&here, 0, song_top, &mut rows);
            if rows.is_empty() {
                rows.push(BRow::Note(EMPTY));
            }
        } else {
            let mut found = Vec::new();
            search(&here, &q, 0, song_top, &mut 0, &mut found);
            for e in found {
                self.push_file(&mut rows, &e, 0);
            }
            if rows.is_empty() {
                rows.push(BRow::Note("見つかりません"));
            }
        }
        if LISTS.with(|l| l.borrow().dirs.get(&top).is_some_and(|l| l.gone)) {
            return vec![BRow::Note(GONE)];
        }
        LISTS.with(|l| {
            let mut l = l.borrow_mut();
            let seen = std::mem::take(&mut l.seen);
            l.dirs.retain(|p, _| seen.contains(p));
            l.jobs.retain(|p| seen.contains(p));
            l.seen = seen;
        });
        rows
    }

    fn push_file(&self, rows: &mut Vec<BRow>, e: &Entry, depth: usize) {
        let key = e.path.to_string_lossy().into_owned();
        let selected = self.selected_file.as_deref() == Some(&key);
        rows.push(BRow::File {
            key: key.clone(),
            name: e.name.clone(),
            len: length(&e.path),
            depth,
            selected,
        });
        if selected && crate::drop_audio::is_audio(&e.path) {
            rows.push(BRow::Audition {
                playing: self.playing.as_deref() == Some(&key),
                key,
                depth,
            });
        }
    }

    fn tree_rows(&self, dir: &Path, depth: usize, song_top: bool, rows: &mut Vec<BRow>) {
        for e in shown(dir, song_top) {
            if !e.dir {
                self.push_file(rows, &e, depth);
                continue;
            }
            let key = e.path.to_string_lossy().into_owned();
            let open = self.open_dirs.contains(&key);
            rows.push(BRow::Dir {
                key,
                name: e.name.clone(),
                depth,
                open,
            });
            if open && depth < SEARCH_DEPTH {
                self.tree_rows(&e.path, depth + 1, false, rows);
            }
        }
    }
}

impl crate::studio::Studio {
    /// The browser follows the song's folder and what another program changed in its file.
    /// True when the list should be drawn again.
    pub fn follow_browser(&mut self) -> bool {
        let dir = self.song().dir();
        let moved = self.browser.song_dir != dir;
        self.browser.set_song_dir(dir);
        let reloaded = self.browser.reload_prefs();
        if let Some(e) = self.browser.prefs_error.take() {
            self.error = Some(e);
        }
        if let Some(notice) = self.browser.prefs_notice.take() {
            self.tell(notice);
        }
        let pool = self.follow_pool();
        let clip = self.follow_pool_clip();
        moved || reloaded || pool || clip
    }
}

#[cfg(test)]
mod tests;
