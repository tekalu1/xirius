//! The song's audio clips played through their ARA plugins (PLG-032 to PLG-037, PLG-047,
//! ENG-025, PLG-058, EDT-023).
//!
//! The song's table of ARA clips ([`xirius_core::ara`]) says which clips go through which
//! plugin. Each plugin has one document here ([`Doc`], a [`Service`] on its own thread), shared
//! by every builder of the same song folder, and is told the song's clips of it as they change.
//! The clip's sound through the plugin comes back rendered, and the builder plays it in place
//! of the file; until it is there, after a crash or when the plugin is missing, the clip plays
//! its file.
//!
//! - Each clip goes to its plugin and comes back at its file's own rate, and is converted to the
//!   song's rate to play (PLG-047), so changing the song's rate keeps the plugin's edits. The
//!   files are read on a thread of their own for the player ([`Doc::hand_over`]).
//! - The player's builder holds the live document ([`Held::live`]). Another builder (an export,
//!   a bounce) uses the live document as it is when it has the same song, at any rate, and
//!   otherwise a document of its own, started from the live one's archive. It waits for the
//!   analysis, at most [`EXPORT_WAIT`] (ENG-025).
//! - A song keeps each plugin's archive ([`take_archives`], ARC-025, PLG-037). The archive is
//!   asked for only after the plugin said its document changed, so opening a song and playing it
//!   leaves it as it was. The window's thread never waits for it: the plugin's thread writes it
//!   and a later frame puts it in; writing or leaving the song waits over the frames for the
//!   one asked, at most [`STORE_WAIT`], and otherwise writes the one it has.
//! - A plugin that faults or hangs ends its own thread only ([`xirius_ara::contain`]); its
//!   clips play their files and `plugin.status` lists it until `plugin.restart` (PLG-058).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak, mpsc};
use std::time::{Duration, Instant, SystemTime};

pub use xirius_ara::Phase;
pub use xirius_ara::service::Loader;
use xirius_ara::service::{Binary, Caption, Placement, Rendered, Stored, source_id};
use xirius_ara::{ClipSpec, Service, Song, SourceAudio};
use xirius_core::model::{Clip, ClipBody, Project, TrackKind};
use xirius_engine::AudioData;
use xirius_plugins::Catalog;

use crate::tempo::TempoMap;

/// How long a builder that is not the player's waits for the plugins' analysis (ENG-025).
pub const EXPORT_WAIT: Duration = Duration::from_secs(10);
/// How long a plugin may take to hand its archive over.
pub const STORE_WAIT: Duration = Duration::from_secs(5);

/// Makes the loader of a plugin registered with [`register`].
pub type MakeLoader = Arc<dyn Fn() -> Loader + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn registered() -> &'static Mutex<HashMap<String, (String, MakeLoader)>> {
    static MAP: OnceLock<Mutex<HashMap<String, (String, MakeLoader)>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// Makes `uid` an ARA plugin named `name`, loaded by `make`, ahead of the catalogue (the
/// tests' stand-in plugin).
pub fn register(uid: &str, name: &str, make: MakeLoader) {
    lock(registered()).insert(uid.to_string(), (name.to_string(), make));
}

/// The ARA plugins there are, as (id, name): the registered ones and the catalogue's.
pub fn plugins(catalog: &Catalog) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = lock(registered())
        .iter()
        .map(|(uid, (name, _))| (uid.clone(), name.clone()))
        .collect();
    out.sort();
    out.extend(
        catalog
            .entries
            .iter()
            .filter(|e| e.ara)
            .map(|e| (e.uid.clone(), e.name.clone())),
    );
    out
}

/// The ARA plugin `uid` names, as (id, name).
pub fn find(catalog: &Catalog, uid: &str) -> Option<(String, String)> {
    plugins(catalog)
        .into_iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(uid))
}

fn loader(catalog: &Catalog, uid: &str) -> Option<Loader> {
    if let Some((_, make)) = lock(registered()).get(uid) {
        return Some(make());
    }
    let e = catalog.find(uid).filter(|e| e.ara)?;
    let (path, name, id) = (e.path.clone(), e.name.clone(), e.uid.clone());
    match e.format.as_str() {
        "vst3" => Some(Box::new(move || {
            let b = xirius_ara::vst3::Vst3Binary::load(&path, Some(&name))?;
            Ok(Box::new(b) as Box<dyn Binary>)
        })),
        "clap" => Some(Box::new(move || {
            let b = xirius_ara::clap::ClapBinary::load(&path, &id)?;
            Ok(Box::new(b) as Box<dyn Binary>)
        })),
        _ => None,
    }
}

/// Puts the containment in front of the crash report, once (PLG-058).
fn contain() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        #[cfg(windows)]
        {
            xirius_engine::crash::set_first_filter(xirius_ara::contain::last_chance);
            if !xirius_engine::crash::exception_filter_installed() {
                xirius_ara::contain::install_own_filter();
            }
        }
    });
}

/// Which document: the song's folder, the plugin, and the rate (0 for the live one).
type Key = (Option<PathBuf>, String, u32);

fn docs() -> &'static Mutex<HashMap<Key, Weak<Doc>>> {
    static MAP: OnceLock<Mutex<HashMap<Key, Weak<Doc>>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn existing(key: &Key) -> Option<Arc<Doc>> {
    let mut map = lock(docs());
    map.retain(|_, d| d.strong_count() > 0);
    map.get(key).and_then(Weak::upgrade)
}

/// A file's samples at its own rate (PLG-047), shared by every document and read again when
/// the file changes. Nothing here keeps them: the documents' songs do.
enum File {
    Loading,
    Ready(Weak<SourceAudio>),
    Failed(String),
}

/// A file's length and time of change, to see that it changed.
type Stamp = (u64, Option<SystemTime>);

fn files() -> &'static Mutex<HashMap<PathBuf, (Stamp, File)>> {
    static MAP: OnceLock<Mutex<HashMap<PathBuf, (Stamp, File)>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn stamp(path: &Path) -> Result<Stamp, String> {
    std::fs::metadata(path)
        .map(|m| (m.len(), m.modified().ok()))
        .map_err(|e| format!("{} を開けません: {e}", path.display()))
}

fn read_source(path: &Path) -> Result<Arc<SourceAudio>, String> {
    let d = crate::decode::decode(path)?;
    Ok(Arc::new(SourceAudio::stereo(
        f64::from(d.rate),
        d.left,
        d.right,
    )))
}

fn store(path: PathBuf, stamp: Stamp, read: &Result<Arc<SourceAudio>, String>) {
    let file = match read {
        Ok(a) => File::Ready(Arc::downgrade(a)),
        Err(e) => File::Failed(e.clone()),
    };
    lock(files()).insert(path, (stamp, file));
}

/// The samples of `path`, once read. With `wait` the file is read here; otherwise on a thread
/// of its own, and `None` until it is: then every document is handed its song again
/// ([`Doc::hand_over`]), so the UI thread never decodes a file for a plugin.
fn source(path: &Path, wait: bool) -> Option<Result<Arc<SourceAudio>, String>> {
    let stamp = match stamp(path) {
        Ok(s) => s,
        Err(e) => return Some(Err(e)),
    };
    {
        let mut map = lock(files());
        map.retain(|_, (_, f)| !matches!(f, File::Ready(w) if w.strong_count() == 0));
        match map.get(path) {
            Some((s, File::Ready(w))) if *s == stamp => {
                if let Some(a) = w.upgrade() {
                    return Some(Ok(a));
                }
            }
            Some((s, File::Failed(e))) if *s == stamp => return Some(Err(e.clone())),
            Some((s, File::Loading)) if *s == stamp && !wait => return None,
            _ => {}
        }
        if !wait {
            map.insert(path.to_path_buf(), (stamp, File::Loading));
        }
    }
    if wait {
        let read = read_source(path);
        store(path.to_path_buf(), stamp, &read);
        return Some(read);
    }
    let owned = path.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("ara-source".into())
        .spawn(move || {
            let read = read_source(&owned);
            store(owned, stamp, &read);
            let docs: Vec<Arc<Doc>> = lock(docs()).values().filter_map(Weak::upgrade).collect();
            for d in docs {
                d.hand_over();
            }
            // Kept until the documents took it.
            drop(read);
            crate::processed::bump();
        });
    if let Err(e) = spawned {
        lock(files()).remove(path);
        return Some(Err(format!("{} を読み込めません: {e}", path.display())));
    }
    None
}

/// The samples of a clip whose file is still being read.
fn unread() -> Arc<SourceAudio> {
    static UNREAD: OnceLock<Arc<SourceAudio>> = OnceLock::new();
    UNREAD
        .get_or_init(|| Arc::new(SourceAudio::stereo(0.0, Vec::new(), Vec::new())))
        .clone()
}

/// The song as the builder last saw it: every clip, with the ones whose files are still being
/// read on samples of none ([`unread`]).
struct Wanted {
    song: Song,
    /// Each clip's file, in the song's order.
    files: Vec<PathBuf>,
}

/// A clip's render as the engine plays it: for the clip's start in its file and its length,
/// at the song's rate.
struct Sound {
    rendered: Arc<Rendered>,
    at: f64,
    length: f64,
    rate: u32,
    data: Arc<AudioData>,
}

/// One plugin's document of one song.
pub struct Doc {
    pub plugin: String,
    pub name: String,
    key: Key,
    service: Service,
    /// The song last handed over: the clips whose files are read.
    song: Mutex<Option<Song>>,
    wanted: Mutex<Option<Wanted>>,
    /// Each clip's render as the engine plays it.
    sounds: Mutex<HashMap<u64, Sound>>,
    /// The archive last taken from the plugin.
    stored: Mutex<Option<xirius_core::ara::Archive>>,
    /// The archive asked for and not come yet.
    asked: Mutex<Option<Stored>>,
    /// The newest archive was asked for while [`Doc::asked`] was on its way: asked again when
    /// it comes.
    again: AtomicBool,
}

impl Doc {
    fn start(key: Key, name: String, load: Loader, archive: Option<(Vec<u8>, String)>) -> Arc<Doc> {
        contain();
        // Every document of the plugin on its one ARA thread (some keep to their first).
        let service =
            Service::start_shared(&key.1, load, archive, Arc::new(crate::processed::bump));
        let doc = Arc::new(Doc {
            plugin: key.1.clone(),
            name,
            key: key.clone(),
            service,
            song: Mutex::new(None),
            wanted: Mutex::new(None),
            sounds: Mutex::new(HashMap::new()),
            stored: Mutex::new(None),
            asked: Mutex::new(None),
            again: AtomicBool::new(false),
        });
        lock(docs()).insert(key, Arc::downgrade(&doc));
        doc
    }

    pub fn service(&self) -> &Service {
        &self.service
    }

    /// The song of `p` as this plugin sees it: its clips that play their files as they are,
    /// each at its file's own rate (PLG-047). Without `wait`, a file not read yet is read on a
    /// thread of its own and its clip waits ([`unread`]).
    fn song_of(
        &self,
        p: &Project,
        tmap: &TempoMap,
        dir: Option<&Path>,
        wait: bool,
        warnings: &mut Vec<String>,
    ) -> (Song, Vec<PathBuf>) {
        let mut clips = Vec::new();
        let mut files = Vec::new();
        for t in p.tracks.values().filter(|t| t.kind == TrackKind::Audio) {
            for c in &t.clips {
                let Some(row) = p.ara.clips.get(&c.id).filter(|r| r.plugin == self.plugin) else {
                    continue;
                };
                let ClipBody::Audio {
                    file,
                    offset,
                    process,
                    ..
                } = &c.body
                else {
                    continue;
                };
                if !process.is_plain() {
                    continue;
                }
                let path = resolve(dir, file);
                let audio = match source(&path, wait) {
                    Some(Ok(a)) => a,
                    Some(Err(e)) => {
                        warnings.push(format!("{} / {}: {e}", t.name, c.name));
                        continue;
                    }
                    None => unread(),
                };
                let (start, offset, duration) = times(c, *offset, tmap);
                clips.push(ClipSpec {
                    key: c.id.0,
                    clone_of: row.clone_of.map(|c| c.0),
                    // The file as the song names it, so the plugin's archive still fits when the
                    // song's folder moves.
                    source: source_id(file),
                    name: if c.name.is_empty() {
                        file.clone()
                    } else {
                        c.name.clone()
                    },
                    audio,
                    start,
                    offset,
                    duration,
                });
                files.push(path);
            }
        }
        let (numerator, denominator) = p
            .timesig
            .first()
            .map_or((4, 4), |s| (s.num as i32, s.den as i32));
        let song = Song {
            bpm: tmap.bpm_at(0),
            numerator,
            denominator,
            clips,
        };
        (song, files)
    }

    fn sync(&self, song: Song, files: Vec<PathBuf>) {
        *lock(&self.wanted) = Some(Wanted { song, files });
        self.hand_over();
    }

    /// Hands the plugin the wanted song's clips whose files are read.
    fn hand_over(&self) {
        let mut wanted = lock(&self.wanted);
        let Some(w) = wanted.as_mut() else {
            return;
        };
        let none = unread();
        for (c, path) in w.song.clips.iter_mut().zip(&w.files) {
            if Arc::ptr_eq(&c.audio, &none)
                && let Some(Ok(a)) = source(path, false)
            {
                c.audio = a;
            }
        }
        let song = Song {
            bpm: w.song.bpm,
            numerator: w.song.numerator,
            denominator: w.song.denominator,
            clips: w
                .song
                .clips
                .iter()
                .filter(|c| !Arc::ptr_eq(&c.audio, &none))
                .cloned()
                .collect(),
        };
        self.service.sync(&song);
        let keys: BTreeSet<u64> = song.clips.iter().map(|c| c.key).collect();
        lock(&self.sounds).retain(|k, _| keys.contains(k));
        *lock(&self.song) = Some(song);
    }

    /// The samples of the clip's file, once read.
    fn source_of(&self, key: u64) -> Option<Arc<SourceAudio>> {
        let song = lock(&self.song);
        let c = song.as_ref()?.clips.iter().find(|c| c.key == key)?;
        Some(c.audio.clone())
    }

    /// The clip's sound through the plugin at the song's `rate`. A clip trimmed or slipped in
    /// its file since its render plays the render where it covers the clip and its file
    /// elsewhere, until the plugin renders it again (PLG-036).
    fn sound(&self, c: &Clip, offset: i64, tmap: &TempoMap, rate: u32) -> Option<Arc<AudioData>> {
        let state = self.service.state(c.id.0)?;
        if state.phase == Phase::Crashed {
            return None;
        }
        let r = state.rendered?;
        let (_, at, length) = times(c, offset, tmap);
        if let Some(s) = lock(&self.sounds).get(&c.id.0)
            && Arc::ptr_eq(&s.rendered, &r)
            && (s.rate, s.at, s.length) == (rate, at, length)
        {
            return Some(s.data.clone());
        }
        let fits = (r.offset - at).abs() <= 1e-6 && (r.duration - length).abs() <= 1e-6;
        let (left, right) = if fits {
            (r.left.clone(), r.right.clone())
        } else {
            splice(&r, &*self.source_of(c.id.0)?, at, length)?
        };
        // Rendered at the file's rate; played at the song's (ENG-022).
        let (left, right) =
            crate::decode::convert(left, right, false, r.rate.round() as u32, rate).ok()?;
        let data = Arc::new(AudioData { left, right });
        lock(&self.sounds).insert(
            c.id.0,
            Sound {
                rendered: r,
                at,
                length,
                rate,
                data: data.clone(),
            },
        );
        Some(data)
    }

    /// Asks the plugin for its archive, without waiting. Not `fresh`, only when it said it
    /// changed its document since it was last asked and no answer is on its way. `fresh` (the
    /// song is written or the clips leave it), when it changed it at all, counting an edit it
    /// has not told of yet (that comes a turn later): asked again after an answer on its way.
    fn ask(&self, asked: &mut Option<Stored>, fresh: bool) {
        if asked.is_some() {
            if fresh {
                self.again.store(true, Ordering::Release);
            }
        } else if fresh {
            *asked = self.service.store_changed_later();
        } else if self.service.take_dirty() {
            *asked = self.service.store_later();
        }
    }

    /// Takes the archive asked for when it has come (with `wait`, waiting at most
    /// [`STORE_WAIT`]). Whether it is still on its way.
    fn receive(&self, asked: &mut Option<Stored>, wait: bool) -> bool {
        while let Some(rx) = asked.as_ref() {
            let got = if wait {
                rx.recv_timeout(STORE_WAIT)
                    .map_err(|e| e == mpsc::RecvTimeoutError::Timeout)
            } else {
                rx.try_recv().map_err(|e| e == mpsc::TryRecvError::Empty)
            };
            match got {
                // Still being written: a later call takes it.
                Err(true) => return true,
                // The plugin failed or went down, or it did not change: nothing comes.
                Err(false) | Ok(None) => {}
                Ok(Some((bytes, id))) => {
                    let a = xirius_ara::Archive::new(&self.plugin, &id, &bytes);
                    *lock(&self.stored) = Some(xirius_core::ara::Archive {
                        version: a.version,
                        plugin: a.plugin,
                        archive_id: a.archive_id,
                        data: a.data,
                    });
                }
            }
            *asked = None;
            if self.again.swap(false, Ordering::AcqRel) {
                *asked = self.service.store_changed_later();
            }
        }
        false
    }

    /// The plugin's archive: the one taken last, or the newer one it was asked for when it has
    /// come. With `wait` (`plugin.collect`, the headless pull), the newest is asked for as when
    /// the song is written, and waited for at most [`STORE_WAIT`]; without, an edit asks for it
    /// and a later call takes it.
    fn archive(&self, wait: bool) -> Option<xirius_core::ara::Archive> {
        let mut asked = lock(&self.asked);
        // An edit made while the plugin wrote the archive asks again; a few rounds at most.
        for round in 0..3 {
            self.ask(&mut asked, wait && round == 0);
            if asked.is_none() || self.receive(&mut asked, wait) {
                break;
            }
        }
        drop(asked);
        lock(&self.stored).clone()
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        let mut map = lock(docs());
        if map.get(&self.key).is_some_and(|w| w.strong_count() == 0) {
            map.remove(&self.key);
        }
    }
}

fn resolve(dir: Option<&Path>, file: &str) -> PathBuf {
    let path = Path::new(file);
    match dir {
        Some(d) if !path.is_absolute() => d.join(path),
        _ => path.to_path_buf(),
    }
}

/// A clip's start on the timeline, its start in the file and its length, in seconds.
fn times(c: &Clip, offset: i64, tmap: &TempoMap) -> (f64, f64, f64) {
    let start = tmap.seconds_at(c.start);
    (
        start,
        tmap.seconds_at(c.start + offset) - start,
        tmap.seconds_at(c.end()) - start,
    )
}

/// The clip from `at` in its file for `length` seconds, at the render's rate: the render where it
/// covers the clip, the file's samples elsewhere (PLG-036). `None` when the file is at another
/// rate than the render.
fn splice(
    r: &Rendered,
    source: &SourceAudio,
    at: f64,
    length: f64,
) -> Option<(Vec<f32>, Vec<f32>)> {
    if source.rate != r.rate || source.channels.len() < 2 {
        return None;
    }
    let frames = |s: f64| (s * r.rate).round() as i64;
    let n = frames(length).max(0) as usize;
    let shift = frames(at - r.offset);
    let rendered = frames(r.duration)
        .max(0)
        .min(r.left.len().min(r.right.len()) as i64);
    let from = frames(at);
    let dry = |c: &[f32], i: i64| usize::try_from(i).ok().and_then(|i| c.get(i)).copied();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for k in 0..n as i64 {
        let j = k + shift;
        let (l, rr) = if (0..rendered).contains(&j) {
            (r.left[j as usize], r.right[j as usize])
        } else {
            (
                dry(&source.channels[0], from + k).unwrap_or(0.0),
                dry(&source.channels[1], from + k).unwrap_or(0.0),
            )
        };
        left.push(l);
        right.push(rr);
    }
    Some((left, right))
}

/// The song's archive of a plugin, as the plugin reads it: `None` when there is none, it is
/// another plugin's or it is of a later Xirius. The document checks its archive id against the
/// factory's before restoring it.
fn archive_of(p: &Project, plugin: &str) -> Option<(Vec<u8>, String)> {
    let a = p.ara.archives.get(plugin).filter(|a| a.plugin == plugin)?;
    let a = xirius_ara::Archive {
        version: a.version,
        plugin: a.plugin.clone(),
        archive_id: a.archive_id.clone(),
        data: a.data.clone(),
    };
    Some((a.bytes()?, a.archive_id))
}

/// One plugin's row in `plugin.status`.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub plugin: String,
    pub name: String,
    pub loaded: bool,
    pub crashed: Option<String>,
    pub failed: Option<String>,
    pub clips: usize,
    /// Clips still analysed or rendered.
    pub working: usize,
    /// The analysis's progress over those clips, 0 to 1.
    pub progress: f32,
    /// Each clip's phase, by its key (the clip id), in order.
    pub phases: Vec<(u64, Phase)>,
    /// The plugin's editor window is open.
    pub editing: bool,
}

/// The documents a builder holds, shared with its copies on the workers
/// ([`crate::build::Builder::material_copy`]), so a document opened there stays open.
#[derive(Clone)]
pub struct Held {
    docs: Arc<Mutex<HashMap<String, Arc<Doc>>>>,
    /// The player's: its documents follow the song (PLG-047).
    live: bool,
    /// How long to wait for the analysis, when not live (ENG-025).
    wait: Duration,
}

impl Default for Held {
    fn default() -> Held {
        Held {
            docs: Arc::default(),
            live: false,
            wait: EXPORT_WAIT,
        }
    }
}

impl Held {
    pub fn set_live(&mut self, live: bool) {
        self.live = live;
    }

    pub fn set_wait(&mut self, wait: Duration) {
        self.wait = wait;
    }

    pub fn is_empty(&self) -> bool {
        lock(&self.docs).is_empty()
    }

    /// The plugin's document, when open.
    pub fn doc(&self, plugin: &str) -> Option<Arc<Doc>> {
        lock(&self.docs).get(plugin).cloned()
    }

    /// Lets go of the documents of plugins no clip of `p` uses. One whose archive is still on
    /// its way is kept until it has come, so that bringing the clips back (an undo) finds the
    /// plugin's edits in it.
    pub fn release(&self, p: &Project) {
        let used: BTreeSet<&str> = p.ara.clips.values().map(|r| r.plugin.as_str()).collect();
        lock(&self.docs)
            .retain(|k, d| used.contains(k.as_str()) || d.receive(&mut lock(&d.asked), false));
    }

    /// Hands each plugin the song's clips of it, opening the documents not open yet and letting
    /// go of the ones no clip uses. Not live: waits for the analysis.
    #[allow(clippy::too_many_arguments)]
    pub fn sync(
        &self,
        p: &Project,
        tmap: &TempoMap,
        rate: u32,
        dir: Option<&Path>,
        catalog: &mut dyn FnMut() -> Arc<Catalog>,
        warnings: &mut Vec<String>,
    ) {
        self.release(p);
        let used: BTreeSet<&str> = p.ara.clips.values().map(|r| r.plugin.as_str()).collect();
        let deadline = Instant::now() + self.wait;
        let mut docs = lock(&self.docs);
        for plugin in used {
            let scope = dir.map(Path::to_path_buf);
            if !self.live
                && !docs.contains_key(plugin)
                && let Some(live) = existing(&(scope.clone(), plugin.to_string(), 0))
            {
                // The player's document, when it already has this song, at any rate: each clip
                // renders at its file's (PLG-047).
                let (song, _) = live.song_of(p, tmap, dir, true, &mut Vec::new());
                if lock(&live.song).as_ref() == Some(&song) {
                    docs.insert(plugin.to_string(), live);
                }
            }
            let doc = match docs.get(plugin) {
                Some(d) => d.clone(),
                None => match self.open(p, plugin, rate, scope, catalog) {
                    Some(d) => {
                        docs.insert(plugin.to_string(), d.clone());
                        d
                    }
                    None => {
                        let name = p
                            .ara
                            .clips
                            .values()
                            .find(|r| r.plugin == plugin)
                            .map_or(plugin, |r| r.name.as_str());
                        warnings.push(format!(
                            "ARA のプラグイン {name} が見つかりません。元の音で鳴らします"
                        ));
                        continue;
                    }
                },
            };
            // Another builder never hands the player's document a song of its own.
            if self.live || doc.key.2 != 0 {
                // The player's builder never waits for a file to be read (it is read aside).
                let (song, files) = doc.song_of(p, tmap, dir, !self.live, warnings);
                doc.sync(song, files);
            }
            if !self.live {
                doc.service.wait_idle(deadline);
            }
        }
    }

    fn open(
        &self,
        p: &Project,
        plugin: &str,
        rate: u32,
        scope: Option<PathBuf>,
        catalog: &mut dyn FnMut() -> Arc<Catalog>,
    ) -> Option<Arc<Doc>> {
        let key = (
            scope.clone(),
            plugin.to_string(),
            if self.live { 0 } else { rate },
        );
        if let Some(d) = existing(&key) {
            return Some(d);
        }
        let catalog = catalog();
        let load = loader(&catalog, plugin)?;
        let name = find(&catalog, plugin).map(|(_, n)| n).unwrap_or_else(|| {
            p.ara
                .clips
                .values()
                .find(|r| r.plugin == plugin)
                .map_or(plugin.to_string(), |r| r.name.clone())
        });
        // Not live: from the player's edits as they are now, else from the song's.
        let live = (!self.live)
            .then(|| existing(&(scope, plugin.to_string(), 0)))
            .flatten();
        let archive = match live {
            Some(l) => l.service.store(STORE_WAIT),
            None => archive_of(p, plugin),
        };
        Some(Doc::start(key, name, load, archive))
    }

    /// The clip's sound through its plugin, when it is ready for the clip as it is now.
    pub fn sound(
        &self,
        p: &Project,
        c: &Clip,
        offset: i64,
        tmap: &TempoMap,
        rate: u32,
    ) -> Option<Arc<AudioData>> {
        let row = p.ara.clips.get(&c.id)?;
        let doc = lock(&self.docs).get(&row.plugin)?.clone();
        doc.sound(c, offset, tmap, rate)
    }

    /// The documents, for `plugin.status`.
    pub fn reports(&self) -> Vec<Report> {
        let docs: Vec<Arc<Doc>> = lock(&self.docs).values().cloned().collect();
        let mut out: Vec<Report> = docs
            .iter()
            .map(|d| {
                let s = &d.service;
                let states = s.states();
                let working: Vec<_> = states
                    .values()
                    .filter(|c| {
                        matches!(
                            c.phase,
                            Phase::Waiting | Phase::Analysing | Phase::Rendering
                        )
                    })
                    .collect();
                let progress = if working.is_empty() {
                    1.0
                } else {
                    working.iter().map(|c| c.progress).sum::<f32>() / working.len() as f32
                };
                let mut phases: Vec<(u64, Phase)> =
                    states.iter().map(|(k, c)| (*k, c.phase)).collect();
                phases.sort_by_key(|(k, _)| *k);
                Report {
                    plugin: d.plugin.clone(),
                    name: d.name.clone(),
                    loaded: s.loaded(),
                    crashed: s.crashed(),
                    failed: s.failed(),
                    clips: states.len(),
                    working: working.len(),
                    progress,
                    phases,
                    editing: s.editing(),
                }
            })
            .collect();
        out.sort_by(|a, b| a.plugin.cmp(&b.plugin));
        out
    }

    /// Opens `plugin`'s editor showing `clips` (by key), or raises it (PLG-036), without
    /// waiting: one window per document, on its ARA thread; a failure comes in
    /// [`Held::take_editor_errors`]. Its edits come into the song with [`take_archives`].
    pub fn open_editor(
        &self,
        plugin: &str,
        title: &str,
        place: Placement,
        clips: &[u64],
    ) -> Result<(), String> {
        let doc = self
            .doc(plugin)
            .ok_or_else(|| "the plugin's document is not open".to_string())?;
        doc.service.request_editor(title, place, clips)
    }

    /// Why editors asked for did not open, by plugin, once.
    pub fn take_editor_errors(&self) -> Vec<(String, String)> {
        let docs = lock(&self.docs);
        let mut out: Vec<(String, String)> = docs
            .iter()
            .filter_map(|(k, d)| d.service.take_editor_error().map(|e| (k.clone(), e)))
            .collect();
        out.sort();
        out
    }

    /// The plugins whose editor window is open.
    pub fn editing(&self) -> Vec<String> {
        let docs = lock(&self.docs);
        let mut out: Vec<String> = docs
            .iter()
            .filter(|(_, d)| d.service.editing())
            .map(|(k, _)| k.clone())
            .collect();
        out.sort();
        out
    }

    /// The editors' caption colours, as the app's theme changed.
    pub fn set_editor_caption(&self, caption: Caption) {
        for d in lock(&self.docs).values() {
            if d.service.editing() {
                d.service.set_editor_caption(caption);
            }
        }
    }

    /// Lets go of the documents that crashed or failed (or of `plugin`'s), so the next build
    /// opens them again from the song's archive. How many.
    pub fn restart(&self, plugin: Option<&str>) -> usize {
        let mut docs = lock(&self.docs);
        let before = docs.len();
        docs.retain(|k, d| match plugin {
            Some(p) => !k.eq_ignore_ascii_case(p),
            None => d.service.crashed().is_none() && d.service.failed().is_none(),
        });
        before - docs.len()
    }

    /// The documents of the song's plugins whose archives the song keeps.
    fn kept(&self, p: &Project) -> Vec<Arc<Doc>> {
        lock(&self.docs)
            .values()
            .filter(|d| d.key.2 == 0 && p.ara.clips.values().any(|r| r.plugin == d.plugin))
            .cloned()
            .collect()
    }

    /// Asks, without waiting, the plugins whose documents changed for their archives, so that
    /// they are ready when the song is written or left: an edit the plugin has not told of yet
    /// counts too.
    pub fn ask_archives(&self, p: &Project) {
        for d in self.kept(p) {
            d.ask(&mut lock(&d.asked), true);
        }
    }

    /// Takes in the archives asked for that have come, without waiting. Whether one is still
    /// on its way: writing or leaving the song waits for it over the window's frames.
    pub fn archives_pending(&self) -> bool {
        lock(&self.docs)
            .values()
            .fold(false, |on, d| d.receive(&mut lock(&d.asked), false) | on)
    }

    /// The archives of the plugins whose documents changed and differ from the song's: those
    /// that have come, or with `wait` also those on their way.
    pub fn archives(&self, p: &Project, wait: bool) -> Vec<xirius_core::ara::Archive> {
        let mut out: Vec<_> = self
            .kept(p)
            .iter()
            .filter_map(|d| d.archive(wait))
            .filter(|a| p.ara.archives.get(&a.plugin) != Some(a))
            .collect();
        out.sort_by(|a, b| a.plugin.cmp(&b.plugin));
        out
    }
}

/// Puts into the song the archives of the plugins whose documents changed (ARC-025): in place,
/// without a step of the history (the plugin keeps its own), and only when they differ, so a
/// song only opened and played is not changed. Whether the song changed.
///
/// Without `wait` (each frame of the window, and writing or leaving the song there) nothing is
/// waited for: an edit asks the plugin for its archive, which its thread writes, and a later
/// call puts it in, even after the plugin's window closed. The window writes the song once
/// [`Held::archives_pending`] is false or [`STORE_WAIT`] has passed, with the archives it has.
/// With `wait` (`plugin.collect`, the headless pull) the archives asked are waited for, each at
/// most [`STORE_WAIT`].
pub fn take_archives(
    session: &mut xirius_core::session::Session,
    held: &Held,
    wait: bool,
) -> xirius_core::CmdResult<bool> {
    let archives = held.archives(session.project(), wait);
    if archives.is_empty() {
        return Ok(false);
    }
    let changes = session.amend(|p| {
        for a in archives {
            p.ara.archives.insert(a.plugin.clone(), a);
        }
    })?;
    Ok(!changes.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(offset: f64, duration: f64, value: f32) -> Rendered {
        let n = (duration * 10.0) as usize;
        Rendered {
            rate: 10.0,
            duration,
            offset,
            left: vec![value; n],
            right: vec![-value; n],
        }
    }

    fn file() -> SourceAudio {
        let s: Vec<f32> = (0..40).map(|i| i as f32 / 100.0).collect();
        SourceAudio::stereo(10.0, s.clone(), s)
    }

    #[test]
    fn a_trimmed_clip_keeps_its_render_where_it_still_covers_it() {
        // Rendered from 1 s for 2 s; the clip now starts at 1.5 s and goes on to 3.5 s.
        let (l, r) = splice(&rendered(1.0, 2.0, 1.0), &file(), 1.5, 2.0).unwrap();
        assert_eq!(l.len(), 20);
        assert!(l[..15].iter().all(|&v| v == 1.0));
        assert!(r[..15].iter().all(|&v| v == -1.0));
        // Past the render, the file as it is: frames 30 to 34.
        assert_eq!(l[15..], file().channels[0][30..35]);
        assert_eq!(r[15..], l[15..]);
    }

    #[test]
    fn a_clip_slipped_earlier_plays_its_file_before_the_render() {
        let (l, _) = splice(&rendered(1.0, 2.0, 1.0), &file(), 0.5, 1.0).unwrap();
        assert_eq!(l[..5], file().channels[0][5..10]);
        assert!(l[5..].iter().all(|&v| v == 1.0));
    }

    #[test]
    fn a_file_at_another_rate_is_not_spliced() {
        let s = SourceAudio::stereo(20.0, vec![0.0; 80], vec![0.0; 80]);
        assert!(splice(&rendered(0.0, 2.0, 1.0), &s, 0.5, 1.0).is_none());
    }
}
