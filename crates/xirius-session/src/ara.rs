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
//! - The player's builder holds the live document ([`Held::live`]); it follows the song's
//!   rate (PLG-047). Another builder (an export, a bounce) uses the live document as it is when
//!   it has the same song at the same rate, and otherwise a document of its own, started from
//!   the live one's archive. It waits for the analysis, at most [`EXPORT_WAIT`] (ENG-025).
//! - A song keeps each plugin's archive ([`take_archives`], ARC-025, PLG-037). The archive is
//!   asked for only after the plugin said its document changed, so opening a song and playing it
//!   leaves it as it was.
//! - A plugin that faults or hangs ends its own thread only ([`xirius_ara::contain`]); its
//!   clips play their files and `plugin.status` lists it until `plugin.restart` (PLG-058).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use xirius_ara::service::{Binary, Loader, Rendered, source_id};
use xirius_ara::{ClipSpec, Phase, Service, Song, SourceAudio};
use xirius_core::model::{Clip, ClipBody, Project, TrackKind};
use xirius_engine::AudioData;
use xirius_plugins::Catalog;

use crate::tempo::TempoMap;
use crate::wav::WavCache;

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

/// A clip's render and its samples as the engine plays them.
type Sound = (Arc<Rendered>, Arc<AudioData>);

/// One plugin's document of one song.
pub struct Doc {
    pub plugin: String,
    pub name: String,
    key: Key,
    service: Service,
    /// The song last handed over.
    song: Mutex<Option<Song>>,
    /// The files' samples at the song's rate, by file and rate.
    sources: Mutex<HashMap<(PathBuf, u32), Arc<SourceAudio>>>,
    /// Each clip's render as the engine plays it.
    sounds: Mutex<HashMap<u64, Sound>>,
    /// The archive last taken from the plugin.
    stored: Mutex<Option<xirius_core::ara::Archive>>,
}

impl Doc {
    fn start(key: Key, name: String, load: Loader, archive: Option<(Vec<u8>, String)>) -> Arc<Doc> {
        contain();
        // Every document of the plugin on its one ARA thread (Melodyne keeps to its first).
        let service =
            Service::start_shared(&key.1, load, archive, Arc::new(crate::processed::bump));
        let doc = Arc::new(Doc {
            plugin: key.1.clone(),
            name,
            key: key.clone(),
            service,
            song: Mutex::new(None),
            sources: Mutex::new(HashMap::new()),
            sounds: Mutex::new(HashMap::new()),
            stored: Mutex::new(None),
        });
        lock(docs()).insert(key, Arc::downgrade(&doc));
        doc
    }

    pub fn service(&self) -> &Service {
        &self.service
    }

    fn rate(&self) -> Option<f64> {
        lock(&self.song).as_ref().map(|s| s.rate)
    }

    fn source(
        &self,
        path: &Path,
        rate: u32,
        cache: &mut WavCache,
    ) -> Result<Arc<SourceAudio>, String> {
        let key = (path.to_path_buf(), rate);
        if let Some(s) = lock(&self.sources).get(&key) {
            return Ok(s.clone());
        }
        let data = cache.load(path, rate)?;
        let s = Arc::new(SourceAudio::stereo(
            f64::from(rate),
            data.left.clone(),
            data.right.clone(),
        ));
        lock(&self.sources).insert(key, s.clone());
        Ok(s)
    }

    /// The song of `p` as this plugin sees it: its clips that play their files as they are.
    fn song_of(
        &self,
        p: &Project,
        tmap: &TempoMap,
        rate: u32,
        dir: Option<&Path>,
        cache: &mut WavCache,
        warnings: &mut Vec<String>,
    ) -> (Song, BTreeSet<(PathBuf, u32)>) {
        let mut clips = Vec::new();
        let mut used = BTreeSet::new();
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
                let audio = match self.source(&path, rate, cache) {
                    Ok(a) => a,
                    Err(e) => {
                        warnings.push(format!("{} / {}: {e}", t.name, c.name));
                        continue;
                    }
                };
                used.insert((path, rate));
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
            }
        }
        let (numerator, denominator) = p
            .timesig
            .first()
            .map_or((4, 4), |s| (s.num as i32, s.den as i32));
        let song = Song {
            rate: f64::from(rate),
            bpm: tmap.bpm_at(0),
            numerator,
            denominator,
            clips,
        };
        (song, used)
    }

    fn sync(&self, song: Song, used: BTreeSet<(PathBuf, u32)>) {
        lock(&self.sources).retain(|k, _| used.contains(k));
        self.service.sync(&song);
        let keys: BTreeSet<u64> = song.clips.iter().map(|c| c.key).collect();
        lock(&self.sounds).retain(|k, _| keys.contains(k));
        *lock(&self.song) = Some(song);
    }

    /// The clip's sound through the plugin, when it is rendered for the clip as it is now.
    fn sound(&self, c: &Clip, offset: i64, tmap: &TempoMap, rate: u32) -> Option<Arc<AudioData>> {
        let state = self.service.state(c.id.0)?;
        if state.phase == Phase::Crashed {
            return None;
        }
        let r = state.rendered?;
        let (_, at, length) = times(c, offset, tmap);
        if r.rate != f64::from(rate)
            || (r.offset - at).abs() > 1e-6
            || (r.duration - length).abs() > 1e-6
        {
            return None;
        }
        let mut sounds = lock(&self.sounds);
        if let Some((seen, data)) = sounds.get(&c.id.0)
            && Arc::ptr_eq(seen, &r)
        {
            return Some(data.clone());
        }
        let data = Arc::new(AudioData {
            left: r.left.clone(),
            right: r.right.clone(),
        });
        sounds.insert(c.id.0, (r, data.clone()));
        Some(data)
    }

    /// The plugin's archive when it changed its document since it was last asked (or the one
    /// taken then).
    fn archive(&self) -> Option<xirius_core::ara::Archive> {
        if self.service.take_dirty()
            && let Some((bytes, id)) = self.service.store(STORE_WAIT)
        {
            let a = xirius_ara::Archive::new(&self.plugin, &id, &bytes);
            *lock(&self.stored) = Some(xirius_core::ara::Archive {
                version: a.version,
                plugin: a.plugin,
                archive_id: a.archive_id,
                data: a.data,
            });
        }
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

/// The song's archive of a plugin, as the plugin reads it: `None` when there is none or it is
/// of a later Xirius.
fn archive_of(p: &Project, plugin: &str) -> Option<(Vec<u8>, String)> {
    let a = p.ara.archives.get(plugin)?;
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

    /// Lets go of the documents of plugins no clip of `p` uses.
    pub fn release(&self, p: &Project) {
        let used: BTreeSet<&str> = p.ara.clips.values().map(|r| r.plugin.as_str()).collect();
        lock(&self.docs).retain(|k, _| used.contains(k.as_str()));
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
        cache: &mut WavCache,
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
                && live.rate() == Some(f64::from(rate))
            {
                // The player's document, when it already has this song.
                let (song, _) = live.song_of(p, tmap, rate, dir, cache, &mut Vec::new());
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
                let (song, used) = doc.song_of(p, tmap, rate, dir, cache, warnings);
                doc.sync(song, used);
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
                Report {
                    plugin: d.plugin.clone(),
                    name: d.name.clone(),
                    loaded: s.loaded(),
                    crashed: s.crashed(),
                    failed: s.failed(),
                    clips: states.len(),
                    working: working.len(),
                    progress,
                }
            })
            .collect();
        out.sort_by(|a, b| a.plugin.cmp(&b.plugin));
        out
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

    /// The archives of the plugins whose documents changed and differ from the song's.
    pub fn archives(&self, p: &Project) -> Vec<xirius_core::ara::Archive> {
        let docs: Vec<Arc<Doc>> = lock(&self.docs).values().cloned().collect();
        let mut out: Vec<_> = docs
            .iter()
            .filter(|d| d.key.2 == 0 && p.ara.clips.values().any(|r| r.plugin == d.plugin))
            .filter_map(|d| d.archive())
            .filter(|a| p.ara.archives.get(&a.plugin) != Some(a))
            .collect();
        out.sort_by(|a, b| a.plugin.cmp(&b.plugin));
        out
    }
}

/// Puts into the song the archives of the plugins whose documents changed (ARC-025): in place,
/// without a step of the history (the plugin keeps its own), and only when they differ, so a
/// song only opened and played is not changed. Whether the song changed.
pub fn take_archives(
    session: &mut xirius_core::session::Session,
    held: &Held,
) -> xirius_core::CmdResult<bool> {
    let archives = held.archives(session.project());
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
