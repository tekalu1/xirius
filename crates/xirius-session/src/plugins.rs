//! Loading the VST3 plugins a song names (PLG-002, PLG-052, OPEN-02), and the CLAP ones
//! ([`crate::clap_plugins`], PLG-028).
//!
//! A song keeps a plugin's class id (`uid`), its parameters by name and its saved state. The
//! catalogue from the last scan ([`xirius_plugins::Catalog`]) says which bundle offers the
//! class.
//!
//! External plugins run in plugin host processes. The song selects the default vendor group
//! or a dedicated process for each plugin. Audio crosses shared memory without added latency;
//! the engine waits for the host within the current block. A crashed or hung host is bypassed
//! and can be restarted while other hosts continue playing. Editor changes return as
//! [`RemoteEdit`]s and song parameter changes reach a host without rebuilding the mix.
//!
//! The built-ins with a processor of the engine's ([`xirius_engine::stock`], PLG-016) are
//! opened here too: they always run in this process, and their parameters move without a new
//! mix like those of a plugin in a host (PLG-045).
//!
//! A plugin that uses ARA ([`xirius_plugins::Entry::ara`], PLG-078) runs in a plugin host process;
//! its document and audio access use the separate ARA worker protocol.
//!
//! A song's plugins can be opened before its mix is built ([`PluginLoader::preload`], ENG-064),
//! one at a time while a window keeps drawing; the build then takes them as they are. A plugin
//! in a host is asked for there and the answer is taken when the mix is built, so the plugins
//! of different hosts open at once. How long each one took is kept for
//! [`PluginLoader::open_times`].
//!
//! A missing host executable makes the external plugin unavailable. The environment can
//! request a single shared host for tests.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use xirius_engine::plugin_updates::{Field, Origin, Stamp, UpdateGate};

use xirius_core::id::PluginId;
use xirius_core::model::{Plugin, PluginIsolation, PluginKind};
use xirius_engine::server::{
    DEFAULT_LATENCY, ParamEdit, PluginLink, PluginServer, PluginStatus, RemotePlugin, RemoteSpec,
    find_host,
};
use xirius_engine::stock::{self, StockHandle};
use xirius_engine::{ProcessSetup, Processor, Readied};
use xirius_plugins::{Catalog, decode_state};
use xirius_vst3::PluginState;

use crate::clap_plugins::{self, is_clap};

/// Whether the song's plugin is one this loader opens.
pub fn is_vst3(x: &Plugin) -> bool {
    x.format.eq_ignore_ascii_case("vst3")
}

/// Where plugins run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Isolation {
    /// Every plugin in one process of the given `xirius-plugin-host` executable, whatever the
    /// song says.
    Separate(PathBuf),
    /// Where each plugin's [`PluginIsolation`] says, with the given `xirius-plugin-host`
    /// executable (`None`: a separate placement is refused before loading).
    PerPlugin(Option<PathBuf>),
}

static HOST_OVERRIDE: Mutex<Option<Isolation>> = Mutex::new(None);

impl Isolation {
    /// What [`Default`] gives from now on in this process, in place of the environment and the
    /// host next to the program. `None` goes back to those.
    pub fn set_default(isolation: Option<Isolation>) {
        *HOST_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()) = isolation;
    }
}

impl Default for Isolation {
    /// [`Isolation::set_default`]'s, else a test override for one shared host, else the
    /// song's default grouping or dedicated placement with [`find_host`].
    fn default() -> Isolation {
        if let Some(i) = HOST_OVERRIDE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return i;
        }
        let forced = std::env::var("XIRIUS_PLUGIN_ISOLATION").unwrap_or_default();
        if forced.eq_ignore_ascii_case("shared") || forced.eq_ignore_ascii_case("separate") {
            return Isolation::Separate(find_host().unwrap_or_default());
        }
        Isolation::PerPlugin(find_host())
    }
}

/// What a plugin in a host process did in its editor since the last
/// [`PluginLoader::take_edits`].
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteEdit {
    pub origin: Origin,
    pub stamp: xirius_engine::plugin_updates::Stamp,
    pub plugin: PluginId,
    /// The plugin's state after the last edit settled (base64, as the song keeps it).
    pub state: Option<String>,
    /// The parameter moves, for automation (Touch / Latch / Write).
    pub params: Vec<ParamEdit>,
}

/// The single precedence rule for passive edits, saves and live graph transfers:
/// current explicit class/state/reset wins over any observation of an older
/// intent; explicitly changed parameter keys win over snapshot values.
#[derive(Clone)]
struct StateIntent(Plugin);
impl StateIntent {
    fn protected_params(&self, current: &Plugin) -> Vec<String> {
        self.0
            .params
            .keys()
            .chain(current.params.keys())
            .filter(|key| self.0.params.get(*key) != current.params.get(*key))
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }

    fn values(
        &self,
        current: &Plugin,
        mut runtime: Vec<(String, f64)>,
        reports: &[ParamEdit],
        titles: &[(u32, String)],
    ) -> Vec<(String, f64)> {
        let param_id = |key: &str| {
            titles
                .iter()
                .find(|(id, title)| {
                    key.parse::<u32>().ok() == Some(*id) || title.eq_ignore_ascii_case(key)
                })
                .map(|(id, _)| *id)
        };
        for (key, value) in &mut runtime {
            if let Some(now) = reports
                .iter()
                .rev()
                .filter(|p| names(key, p))
                .find_map(|p| p.value)
            {
                *value = now;
            }
        }
        let changed = self.protected_params(current);
        runtime.retain(|(old, _)| {
            !changed.iter().any(|key| {
                old.eq_ignore_ascii_case(key)
                    || param_id(old)
                        .zip(param_id(key))
                        .is_some_and(|(a, b)| a == b)
            })
        });
        for (key, value) in &current.params {
            if !runtime.iter().any(|(name, _)| {
                name.eq_ignore_ascii_case(key)
                    || param_id(name)
                        .zip(param_id(key))
                        .is_some_and(|(a, b)| a == b)
            }) {
                let changed = changed.iter().any(|changed| {
                    changed.eq_ignore_ascii_case(key)
                        || param_id(changed)
                            .zip(param_id(key))
                            .is_some_and(|(a, b)| a == b)
                });
                let value = if changed {
                    *value
                } else {
                    reports
                        .iter()
                        .rev()
                        .filter(|p| names(key, p))
                        .find_map(|p| p.value)
                        .unwrap_or(*value)
                };
                runtime.push((key.clone(), value));
            }
        }
        runtime
    }
    fn accepts(&self, current: &Plugin) -> bool {
        let old = &self.0;
        old.uid == current.uid
            && old.format == current.format
            && old.kind == current.kind
            && old.state == current.state
    }
    fn reconcile(&self, current: &Plugin, edit: &mut RemoteEdit) -> bool {
        let mut gate = UpdateGate::new(edit.stamp);
        let keys = self.protected_params(current);
        if !self.accepts(current) || !keys.is_empty() {
            gate.explicit(gate.next_generation(), !self.accepts(current), &keys);
        }
        if !gate.accepts(edit.stamp, Field::State) {
            return false;
        }
        edit.params.retain(|p| {
            gate.accepts(
                edit.stamp,
                Field::Parameter {
                    id: p.id,
                    title: &p.title,
                },
            )
        });
        true
    }
}

/// A plugin opened in a host process, and the song's values it was last given.
#[derive(Clone)]
struct Remote {
    link: Arc<PluginLink>,
    server: PluginServer,
    /// [`PluginIsolation::Shared`] or [`PluginIsolation::Dedicated`].
    placement: PluginIsolation,
    owner: String,
    applied: Plugin,
}

/// The instance that plays a plugin while the build in progress opens it in another host
/// (ENG-071). Until the new mix plays
/// ([`PluginLoader::promote_opened`]) its window, its reports and what is moved in it are this
/// one's; when the new mix is let go ([`PluginLoader::drop_ready`]) it is the plugin's again.
enum Leaving {
    Remote(Remote),
}

/// A window that moved with a plugin to another place when the new mix played (ENG-071).
/// Until the host of the instance that played has let go of it, that host may still say the
/// person shut the old window as the mix changed: then the moved window shuts too.
struct Following {
    /// The instance that played and its host, kept until the host has let go of it.
    old_link: Arc<PluginLink>,
    old_server: PluginServer,
    /// How many windows the old host had said were shut when the new mix played.
    shut: u64,
    /// The instance in a host that plays now, with the window that opened on it.
    to: (PluginServer, Arc<PluginLink>),
    /// A host that has not let go by then is not waited for: its window moves as it is.
    until: Instant,
}

/// How long the moved window waits for a host that does not answer (ENG-071). A host that
/// answers lets go of the old instance within a frame or two of the new mix playing.
const HAND_OVER: Duration = Duration::from_secs(2);

/// An asynchronous state request. Polling never waits for another process.
pub struct StatePull {
    cancelled: std::cell::Cell<bool>,
    asked: Vec<(PluginId, Arc<PluginLink>, u64)>,
    errors: Vec<(PluginId, String)>,
    basis: HashMap<PluginId, StateIntent>,
    ignored: std::cell::RefCell<HashSet<PluginId>>,
}
impl StatePull {
    pub fn reconcile(&self, target: &xirius_core::model::Project) {
        let current: HashMap<_, _> = crate::starting::hosted(target)
            .into_iter()
            .map(|(_, p)| (p.id, p))
            .collect();
        let mut ignored = self.ignored.borrow_mut();
        for (id, old) in &self.basis {
            if current.get(id).is_none_or(|now| !old.accepts(now)) && ignored.insert(*id) {
                for (_, link, request) in self.asked.iter().filter(|(asked, _, _)| asked == id) {
                    link.cancel_capture(*request);
                }
            }
        }
    }
    pub fn quarantine(&self) {
        self.cancel();
    }
    /// Leaves out each plugin that failed or has not answered yet, so the ones that answered
    /// are read on their own (MIX-045): one host gone down does not throw away the others'
    /// states. A late answer is quarantined, not put in the song afterwards.
    pub fn settle(&self) {
        let mut ignored = self.ignored.borrow_mut();
        for (id, _) in &self.errors {
            ignored.insert(*id);
        }
        for (id, link, request) in &self.asked {
            if !ignored.contains(id)
                && (!self.answered(link.capture_finished(*request))
                    || link.capture_error(*request).is_some())
            {
                link.cancel_capture(*request);
                ignored.insert(*id);
            }
        }
    }

    /// Whether a plugin's answer is in, `finished` as its host or capture says. A test can hold
    /// the answers back on its own thread (`capture_answer`), as from a host slower than any
    /// wait, whichever way the processes race.
    fn answered(&self, finished: bool) -> bool {
        #[cfg(feature = "test-io")]
        if finished
            && xirius_core::io_test::before("capture_answer", std::path::Path::new("")).is_err()
        {
            return false;
        }
        finished
    }

    /// Whether the pull asked plugin `id` for its state.
    pub(crate) fn asks(&self, id: PluginId) -> bool {
        self.basis.contains_key(&id)
    }

    pub fn ready(&self) -> bool {
        self.cancelled.get()
            || self.asked.iter().all(|(id, link, request)| {
                self.ignored.borrow().contains(id) || self.answered(link.capture_finished(*request))
            })
    }

    pub fn error(&self) -> Option<String> {
        if self.cancelled.get() {
            return Some("プラグインの状態要求が取り消されました".into());
        }
        self.errors
            .iter()
            .find(|(id, _)| !self.ignored.borrow().contains(id))
            .map(|(_, e)| e.clone())
            .or_else(|| {
                self.asked
                    .iter()
                    .filter(|(id, _, _)| !self.ignored.borrow().contains(id))
                    .find_map(|(_, link, request)| link.capture_error(*request))
            })
    }

    /// Abandon host replies that arrive after the pull was cancelled.
    /// This does not claim the plugin has stopped or its owner has retired.
    pub fn cancel(&self) {
        self.cancelled.set(true);
        for (_, link, request) in &self.asked {
            link.cancel_capture(*request);
        }
    }
}

impl Drop for StatePull {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// How a plugin is, for `plugin.status`.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginReport {
    pub id: PluginId,
    pub name: String,
    /// Where it runs now.
    pub placement: PluginIsolation,
    /// Where the song asks it to run.
    pub requested: PluginIsolation,
    pub status: PluginStatus,
}

impl PluginReport {
    /// The process to suggest instead: a plugin that took the shared host down is better off
    /// in one of its own. Only a suggestion; the song decides (PLG-022).
    pub fn suggest(&self) -> Option<PluginIsolation> {
        (self.placement == PluginIsolation::Shared
            && self.requested != PluginIsolation::Dedicated
            && self.status.faults > 0)
            .then_some(PluginIsolation::Dedicated)
    }
}

/// What a plugin is opened with.
struct Values {
    state: Option<PluginState>,
    params: Vec<(String, f64)>,
}

fn apply_remote(r: &mut Remote, x: &Plugin) -> bool {
    if r.applied.isolation != x.isolation {
        return false;
    }
    if r.applied.state == x.state && r.applied.params == x.params {
        r.link.set_bypassed(x.bypass);
        r.applied = x.clone();
        return true;
    }
    if r.applied.params.keys().any(|k| !x.params.contains_key(k)) {
        return false;
    }
    let state_changed = r.applied.state != x.state;
    if state_changed {
        match x.state.as_deref().filter(|s| !s.is_empty()) {
            Some(text) => match decode_state(text) {
                Ok(state) => r.server.set_state(&r.link, state),
                Err(_) => return false,
            },
            None => return false,
        }
    }
    for (key, value) in &x.params {
        if state_changed || r.applied.params.get(key) != Some(value) {
            r.server.set_param(&r.link, key, *value);
        }
    }
    r.applied = x.clone();
    r.link.set_bypassed(x.bypass);
    true
}

/// A running built-in and the song's plugin it was last given.
struct Stock {
    handle: StockHandle,
    applied: Plugin,
    /// The plugin whose files a full ring turned away: applied once
    /// [`PluginLoader::resend_files`] gets them through.
    waiting: Option<Plugin>,
}

/// A plugin opened before the build that takes it (ENG-064).
struct Ready {
    applied: Plugin,
    owner: String,
    result: Result<(Box<dyn Processor>, Vec<String>), String>,
    /// The list of the build in progress the opening put it on.
    list: Fresh,
}

#[derive(Clone, Copy)]
enum Fresh {
    Remote,
    None,
}

/// How long one plugin of the last build took to open (ENG-064).
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTime {
    pub plugin: PluginId,
    /// The track or bus it is on, and its name.
    pub owner: String,
    pub name: String,
    pub took: Duration,
    /// It runs in a host process.
    pub apart: bool,
}

/// Exactly one snapshot per instrument/track in a selected graph, without owning its audio.
#[derive(Clone, Default)]
struct PressedBindings {
    instruments: HashMap<PluginId, std::sync::Weak<xirius_engine::performance::Pressed>>,
    multis: HashMap<xirius_core::id::TrackId, std::sync::Weak<xirius_engine::performance::Pressed>>,
}

/// Opens plugins for a build. Bundles stay loaded between builds, so a rebuild after an edit
/// does not load the DLL again; a plugin in a host process keeps its link across builds.
#[derive(Default)]
pub struct PluginLoader {
    pressed: PressedBindings,
    building_pressed: Option<PressedBindings>,
    editor_contexts: Arc<Mutex<HashMap<PluginId, EditorContext>>>,
    intents: HashMap<PluginId, Option<Plugin>>,
    process_setup: Option<ProcessSetup>,
    catalog: Option<Arc<Catalog>>,
    process_prefs: Option<xirius_prefs::Prefs>,
    isolation: Isolation,
    /// Hosts shared by plugins from the same vendor (or bundle parent folder).
    groups: HashMap<String, PluginServer>,
    /// The hosts of plugins that run in one of their own.
    dedicated: HashMap<PluginId, PluginServer>,
    remotes: HashMap<PluginId, Remote>,
    /// Edits a plugin made in a process it has since left, for the next
    /// [`PluginLoader::take_edits`].
    carried: Vec<RemoteEdit>,
    /// Plugins of the build in progress, for [`PluginLoader::opening_warnings`].
    fresh: Vec<PluginId>,
    /// The built-ins of the last build, and of the build in progress.
    stock: HashMap<PluginId, Stock>,
    fresh_stock: Vec<PluginId>,
    /// The tempo the built-ins that follow it play at (beats per minute; 0 before it is set).
    tempo: f64,
    /// The song's folder (relative sample names resolve in it) and the mix's rate (ENG-077).
    dir: Option<PathBuf>,
    rate: u32,
    /// Read files by path and rate (a file read once is not decoded again until it changes).
    wavs: crate::wav::WavCache,
    /// Files that could not be read while `apply` moved a running built-in's samples.
    file_warnings: Vec<String>,
    /// The plugins opened ahead of the next build ([`PluginLoader::preload`]).
    ready: HashMap<PluginId, Ready>,
    /// The plugins whose last opening for a build failed, for [`PluginLoader::answered`].
    failed: HashSet<PluginId>,
    /// How long the plugins opened since the last build took here, with their owner and name.
    took: HashMap<PluginId, (String, String, Duration)>,
    /// Plugins in a host process the build took before the host answered (ENG-065), with how
    /// many answers their link had then: what the host says about them comes with
    /// [`PluginLoader::take_late_warnings`].
    late: HashMap<PluginId, u64>,
    /// The instances that play the plugins the build in progress opened again elsewhere.
    leaving: HashMap<PluginId, Leaving>,
    /// Windows that moved with a plugin when the last mix played, whose old host has not let
    /// go yet (ENG-071, [`PluginLoader::settle_following`]).
    following: Vec<Following>,
    /// The last value moved in each plugin's window since the last state it sent, for
    /// [`PluginLoader::moves_until`].
    unsettled: HashMap<PluginId, Vec<(Stamp, ParamEdit)>>,
}

#[derive(Clone, Default)]
struct EditorContext {
    area: Option<xirius_vst3::EditorArea>,
    bar: Option<xirius_vst3::EditorBar>,
    caption: Option<xirius_vst3::Caption>,
}
/// The startup log's one line for an open that failed: what it was, why, and how long the
/// open took. The plugin's name and kind only; a reason naming a file keeps none of its path.
fn note_open_failed(x: &Plugin, took: Duration, reason: &str) {
    xirius_engine::crash::note(&format!(
        "plugin_open_failed name={} kind={:?} format={} s={:.3} reason={}",
        x.name,
        x.kind,
        x.format,
        took.as_secs_f64(),
        without_paths(reason)
    ));
}

/// `reason` on one line, with anything shaped like a file path replaced by <path>.
pub(crate) fn without_paths(reason: &str) -> String {
    reason
        .replace('\n', " ")
        .split(' ')
        .map(|w| {
            if w.contains('\\') || w.contains('/') {
                "<path>"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The class id a song keeps as 32 hex digits.
fn parse_uid(uid: &str) -> Option<[u8; 16]> {
    if uid.len() != 32 || !uid.is_ascii() {
        return None;
    }
    let mut cid = [0u8; 16];
    for (i, b) in cid.iter_mut().enumerate() {
        *b = u8::from_str_radix(&uid[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(cid)
}

/// Whether a parameter edit is of the parameter a song names by `key` (numeric id or title).
fn names(key: &str, p: &ParamEdit) -> bool {
    key.parse::<u32>().ok() == Some(p.id) || p.title.eq_ignore_ascii_case(key)
}

impl PluginLoader {
    pub(crate) fn advance_intents(
        &mut self,
        before: &xirius_core::model::Project,
        after: &xirius_core::model::Project,
    ) {
        let before: HashMap<_, _> = crate::starting::hosted(before)
            .into_iter()
            .map(|(_, p)| (p.id, p))
            .collect();
        let after: HashMap<_, _> = crate::starting::hosted(after)
            .into_iter()
            .map(|(_, p)| (p.id, p))
            .collect();
        let ids: HashSet<_> = before.keys().chain(after.keys()).copied().collect();
        for id in ids {
            let old = self
                .intents
                .get(&id)
                .cloned()
                .unwrap_or_else(|| before.get(&id).map(|p| (*p).clone()));
            let now = after.get(&id).map(|p| (*p).clone());
            let replace = old
                .as_ref()
                .zip(now.as_ref())
                .is_none_or(|(old, now)| !StateIntent(old.clone()).accepts(now));
            let keys = old
                .as_ref()
                .zip(now.as_ref())
                .map(|(old, now)| StateIntent(old.clone()).protected_params(now))
                .unwrap_or_default();
            if old != now && (replace || !keys.is_empty()) {
                if let Some(r) = self.remotes.get(&id) {
                    r.server.invalidate(&r.link, replace, &keys);
                }
                if let Some(leaving) = self.leaving.get_mut(&id) {
                    let Leaving::Remote(r) = leaving;
                    r.server.invalidate(&r.link, replace, &keys);
                }
            }
            self.intents.insert(id, now);
        }
    }
    fn update_gate(&self, id: PluginId) -> Option<UpdateGate> {
        self.remotes
            .get(&id)
            .map(|r| r.link.update_gate())
            .or_else(|| {
                self.leaving.get(&id).map(|leaving| {
                    let Leaving::Remote(r) = leaving;
                    r.link.update_gate()
                })
            })
    }

    pub(crate) fn live_copy(&self) -> Result<impl FnOnce() -> Self + Send + 'static, String> {
        let catalog = self.catalog.clone();
        let isolation = self.isolation.clone();
        let process_prefs = self.process_prefs.clone();
        let groups = self.groups.clone();
        let dedicated = self.dedicated.clone();
        let remotes = self.remotes.clone();
        let editor_contexts = self.editor_contexts.clone();
        let intents = self.intents.clone();
        let stock = self
            .stock
            .iter()
            .map(|(id, value)| {
                (
                    *id,
                    Stock {
                        handle: value.handle.clone(),
                        applied: value.applied.clone(),
                        waiting: value.waiting.clone(),
                    },
                )
            })
            .collect();
        let (dir, rate, tempo, process_setup) =
            (self.dir.clone(), self.rate, self.tempo, self.process_setup);
        let pressed = self.pressed.clone();
        Ok(move || Self {
            pressed,
            editor_contexts,
            intents,
            catalog,
            isolation,
            process_prefs,
            groups,
            dedicated,
            remotes,
            stock,
            dir,
            rate,
            tempo,
            process_setup,
            ..Self::default()
        })
    }

    pub(crate) fn capture_for_live_build(
        &mut self,
        target: &xirius_core::model::Project,
        kept: &std::collections::BTreeSet<PluginId>,
    ) -> Result<(), String> {
        let basis = self.state_basis();
        let wanted = crate::starting::hosted(target)
            .into_iter()
            .filter(|(_, next)| {
                !kept.contains(&next.id) && basis.get(&next.id).is_some_and(|old| old.accepts(next))
            })
            .map(|(_, next)| next.id)
            .collect();
        let receipt = self.begin_pull_states_matching(Some(&wanted), false);
        let receipt = Self::wait_for_pull(receipt, crate::host::PULL_WAIT)?;
        // A retained processor also retains its editor mailbox. Do not drain it
        // into a candidate that may be abandoned, even when wanted is empty.
        let mut edits = self.current_edits_matching(target, Some(&receipt), Some(&wanted))?;
        for edit in &mut edits {
            edit.origin = Origin::Transfer;
        }
        self.carried.extend(edits);
        Ok(())
    }

    pub(crate) fn set_setup(&mut self, setup: ProcessSetup) {
        self.process_setup = Some(setup);
    }
    pub fn new(catalog: Option<Arc<Catalog>>) -> PluginLoader {
        PluginLoader {
            catalog,
            ..PluginLoader::default()
        }
    }

    pub fn pressed_keys(&self, id: PluginId) -> Vec<u8> {
        self.pressed
            .instruments
            .get(&id)
            .and_then(std::sync::Weak::upgrade)
            .map_or_else(Vec::new, |s| s.notes())
    }

    pub fn multi_pressed_keys(&self, track: xirius_core::id::TrackId) -> Vec<u8> {
        self.pressed
            .multis
            .get(&track)
            .and_then(std::sync::Weak::upgrade)
            .map_or_else(Vec::new, |s| s.notes())
    }

    pub(crate) fn register_pressed(
        &mut self,
        id: PluginId,
        state: &Arc<xirius_engine::performance::Pressed>,
    ) {
        self.building_pressed
            .get_or_insert_with(PressedBindings::default)
            .instruments
            .insert(id, Arc::downgrade(state));
    }

    pub(crate) fn register_multi_pressed(
        &mut self,
        track: xirius_core::id::TrackId,
        state: &Arc<xirius_engine::performance::Pressed>,
    ) {
        self.building_pressed
            .get_or_insert_with(PressedBindings::default)
            .multis
            .insert(track, Arc::downgrade(state));
    }

    pub(crate) fn keep_pressed_track(&mut self, track: xirius_core::id::TrackId) {
        if let Some(state) = self.pressed.multis.get(&track) {
            self.building_pressed
                .get_or_insert_with(PressedBindings::default)
                .multis
                .insert(track, state.clone());
        }
    }

    fn promote_pressed(&mut self) {
        if let Some(pressed) = self.building_pressed.take() {
            self.pressed = pressed;
        }
    }

    /// The catalogue to look plugins up in. Without one, the standard cache is read when a
    /// plugin is first needed.
    pub fn set_catalog(&mut self, catalog: Option<Arc<Catalog>>) {
        self.catalog = catalog;
    }

    /// Where plugins opened from now on run.
    pub fn set_isolation(&mut self, isolation: Isolation) {
        if isolation != self.isolation {
            self.groups.clear();
            self.dedicated.clear();
            self.remotes.clear();
            self.following.clear();
        }
        self.isolation = isolation;
    }

    pub fn isolation(&self) -> &Isolation {
        &self.isolation
    }

    fn catalog(&mut self) -> &Catalog {
        self.catalog
            .get_or_insert_with(|| Arc::new(Catalog::load_standard()))
    }

    /// The catalogue, shared ([`crate::ara`]).
    pub(crate) fn shared_catalog(&mut self) -> Arc<Catalog> {
        self.catalog
            .get_or_insert_with(|| Arc::new(Catalog::load_standard()))
            .clone()
    }

    /// Called when a build starts.
    pub fn begin_build(&mut self) {
        self.building_pressed = Some(PressedBindings::default());
        self.fresh.clear();
        self.fresh_stock.clear();
    }

    /// The processor is carried by the next plan, so retain its controller and
    /// meters without opening a replacement or taking another audio lease.
    pub(crate) fn keep_built(&mut self, ids: impl IntoIterator<Item = PluginId>) {
        for id in ids {
            if let Some(state) = self.pressed.instruments.get(&id) {
                self.building_pressed
                    .get_or_insert_with(PressedBindings::default)
                    .instruments
                    .insert(id, state.clone());
            }
            if self.remotes.contains_key(&id) {
                self.fresh.push(id);
            }
            if self.stock.contains_key(&id) {
                self.fresh_stock.push(id);
            }
        }
    }

    /// A caller with an in-memory settings store can supply the same policy without file I/O.
    pub fn set_process_prefs(&mut self, prefs: xirius_prefs::Prefs) {
        self.process_prefs = Some(prefs);
    }

    fn requested(&self, x: &Plugin) -> PluginIsolation {
        let disk;
        let prefs = if let Some(prefs) = &self.process_prefs {
            prefs
        } else {
            disk = xirius_prefs::Settings::open(xirius_prefs::config_path());
            &disk.prefs
        };
        let legacy = crate::plugin_policy::legacy_override(prefs, self.catalog.as_deref(), x);
        let fallback = legacy.unwrap_or(x.isolation);
        xirius_prefs::isolation::resolve(prefs, &x.format, &x.uid, fallback)
    }

    /// Where `x` runs and the host executable. `ara` marks the catalogue's ARA entry.
    fn placement(
        &self,
        x: &Plugin,
        _ara: bool,
        _warnings: &mut Vec<String>,
    ) -> (PluginIsolation, PathBuf) {
        let requested = self.requested(x);
        let placement = if requested == PluginIsolation::Dedicated {
            PluginIsolation::Dedicated
        } else {
            PluginIsolation::Shared
        };
        let host = match &self.isolation {
            Isolation::Separate(host) | Isolation::PerPlugin(Some(host)) => host.clone(),
            Isolation::PerPlugin(None) => PathBuf::new(),
        };
        (placement, host)
    }

    /// The single policy point for choosing a shared host.
    fn host_group(entry: &xirius_plugins::Entry, catalog: Option<&Catalog>) -> String {
        // Every class in one bundle must use the same host, even if its catalogue
        // records disagree about the vendor. Pick a stable name for that bundle.
        let vendor = catalog
            .into_iter()
            .flat_map(|catalog| catalog.entries.iter())
            .filter(|other| other.path == entry.path)
            .map(|other| other.vendor.trim())
            .filter(|vendor| !vendor.is_empty())
            .chain((!entry.vendor.trim().is_empty()).then_some(entry.vendor.trim()))
            .min()
            .unwrap_or("");
        if !vendor.is_empty() {
            format!("vendor:{}", vendor.to_lowercase())
        } else {
            format!(
                "folder:{}",
                entry
                    .path
                    .parent()
                    .unwrap_or(&entry.path)
                    .to_string_lossy()
                    .to_lowercase()
            )
        }
    }

    /// The host for a plugin placed `placement`.
    fn server_for(
        &mut self,
        id: PluginId,
        placement: PluginIsolation,
        host: PathBuf,
        entry: &xirius_plugins::Entry,
    ) -> PluginServer {
        match placement {
            PluginIsolation::Dedicated => self
                .dedicated
                .entry(id)
                .or_insert_with(|| PluginServer::automatic(Some(host)))
                .clone(),
            _ => self
                .groups
                .entry(Self::host_group(entry, self.catalog.as_deref()))
                .or_insert_with(|| PluginServer::automatic(Some(host)))
                .clone(),
        }
    }

    /// What `x` is opened with: the song's values, or, when it moves out of a host process,
    /// what it has there now (with the edits in its editor that the song has not taken yet;
    /// those are kept for [`PluginLoader::take_edits`]).
    fn values(&mut self, x: &Plugin, moving: Option<Remote>) -> Result<Values, String> {
        if let Some(r) = moving.filter(|old| StateIntent(old.applied.clone()).accepts(x))
            && let Some(now) = r.server.current(&r.link)
        {
            let edits = r.link.take_edits();
            let snapshot = self.carried.iter().rev().find(|e| e.plugin == x.id);
            let state = edits
                .state
                .clone()
                .or_else(|| {
                    snapshot
                        .and_then(|e| e.state.as_deref())
                        .and_then(|s| decode_state(s).ok())
                })
                .or(now.state);
            let reports: Vec<_> = snapshot
                .into_iter()
                .flat_map(|e| e.params.iter().cloned())
                .chain(edits.params.iter().cloned())
                .collect();
            let titles = r.link.opened().map(|o| o.params).unwrap_or_default();
            let params = StateIntent(r.applied.clone()).values(x, now.params, &reports, &titles);
            if edits.state.is_some() || !edits.params.is_empty() {
                self.carried.push(RemoteEdit {
                    origin: edits.origin,
                    stamp: edits.stamp,
                    plugin: x.id,
                    state: edits.state.as_ref().map(xirius_plugins::encode_state),
                    params: edits.params,
                });
            }
            return Ok(Values { state, params });
        }
        let state = match x.state.as_deref().filter(|s| !s.is_empty()) {
            Some(text) => Some(decode_state(text).map_err(|e| format!("{}: {e}", x.name))?),
            None => None,
        };
        Ok(Values {
            state,
            params: x.params.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        })
    }

    /// The plugin as a processor with its state and parameters applied, and warnings about
    /// parameters it does not have and where it runs. `owner` names the track or bus in
    /// messages. A plugin [`PluginLoader::preload`] opened as it is now is taken as it is.
    pub fn instantiate(
        &mut self,
        owner: &str,
        x: &Plugin,
    ) -> Result<(Box<dyn Processor>, Vec<String>), String> {
        let out = self.instantiate_once(owner, x);
        if out.is_err() {
            self.failed.insert(x.id);
        } else {
            self.failed.remove(&x.id);
        }
        out
    }

    fn instantiate_once(
        &mut self,
        owner: &str,
        x: &Plugin,
    ) -> Result<(Box<dyn Processor>, Vec<String>), String> {
        if let Some(r) = self.ready.remove(&x.id)
            && r.applied == *x
            && r.owner == owner
        {
            match r.list {
                Fresh::Remote => self.fresh.push(x.id),
                Fresh::None => {}
            }
            return r.result;
        }
        let at = Instant::now();
        let out = self.open(owner, x);
        let took = at.elapsed();
        if let Err(e) = &out {
            note_open_failed(x, took, e);
        }
        self.took
            .insert(x.id, (owner.to_string(), x.name.clone(), took));
        out
    }

    /// Opens `x` for the next build and begins getting it ready for `setup` (ENG-064): a
    /// plugin here is prepared now; one in a host is asked for there, and
    /// [`PluginLoader::preparing`] is true until the answer comes. The build takes it from
    /// [`PluginLoader::instantiate`], with the same warnings and errors.
    pub fn preload(&mut self, owner: &str, x: &Plugin, setup: &ProcessSetup) {
        let at = Instant::now();
        let before = self.fresh.len();
        let result = self
            .open(owner, x)
            .map(|(p, w)| (Box::new(Readied::new(p, setup)) as Box<dyn Processor>, w))
            .inspect_err(|e| note_open_failed(x, at.elapsed(), e));
        let list = if self.fresh.len() > before {
            Fresh::Remote
        } else {
            Fresh::None
        };
        self.took
            .insert(x.id, (owner.to_string(), x.name.clone(), at.elapsed()));
        self.ready.insert(
            x.id,
            Ready {
                applied: x.clone(),
                owner: owner.to_string(),
                result,
                list,
            },
        );
    }

    pub fn drop_ready(&mut self) {
        self.ready.clear();
        self.following.clear();
        for (id, leaving) in std::mem::take(&mut self.leaving) {
            let Leaving::Remote(r) = leaving;
            if r.placement == PluginIsolation::Dedicated {
                self.dedicated.insert(id, r.server.clone());
            } else {
                self.dedicated.remove(&id);
            }
            self.remotes.insert(id, r);
        }
    }

    pub fn preloaded(&self, id: PluginId) -> bool {
        self.ready.contains_key(&id)
    }

    /// Whether the opening [`PluginLoader::preload`] began for `id` in a host process is still
    /// going on.
    pub fn preparing(&self, id: PluginId) -> bool {
        self.ready
            .get(&id)
            .and_then(|r| r.result.as_ref().ok())
            .is_some_and(|(p, _)| p.preparing())
    }

    /// External plugins run in a host process; built-ins stay in the engine.
    pub fn runs_apart(&mut self, x: &Plugin) -> bool {
        is_vst3(x) || is_clap(x)
    }

    /// How long each plugin the last build opened took: to be made and prepared here, or, for
    /// one in a host process, for the host to open it (known once the mix is prepared).
    pub fn open_times(&self) -> Vec<OpenTime> {
        let mut out: Vec<OpenTime> = self
            .took
            .iter()
            .map(|(id, (owner, name, took))| {
                let link = self.remotes.get(id).map(|r| &r.link);
                OpenTime {
                    plugin: *id,
                    owner: owner.clone(),
                    name: name.clone(),
                    took: link.and_then(|l| l.open_took()).unwrap_or(*took),
                    apart: link.is_some(),
                }
            })
            .collect();
        out.sort_by_key(|t| t.plugin);
        out
    }

    fn open(
        &mut self,
        owner: &str,
        x: &Plugin,
    ) -> Result<(Box<dyn Processor>, Vec<String>), String> {
        self.open_raw(owner, x)
    }

    fn open_raw(
        &mut self,
        owner: &str,
        x: &Plugin,
    ) -> Result<(Box<dyn Processor>, Vec<String>), String> {
        if crate::host::on_window_thread() {
            return Err("音の準備は裏で行う必要があります".into());
        }
        let _filter = xirius_engine::crash::guard_exception_filter();
        let entry = crate::plugin_policy::find(self.catalog(), x)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "{}（{} {}）が見つかりません（plugin.rescan で再スキャンしてください）",
                    x.name, x.format, x.uid
                )
            })?;
        let mut warnings = Vec::new();
        let (placement, host) = self.placement(x, entry.ara, &mut warnings);
        if !host.is_file() {
            return Err(format!(
                "{}: 別プロセスのホストが見つからないため、読み込めません",
                x.name
            ));
        }
        // A plugin that stays in its host keeps its link; one that moves is opened anew with
        // what it has now.
        let stays = self
            .remotes
            .get(&x.id)
            .is_some_and(|r| r.placement == placement);
        let moving = if stays {
            // Rebuilding PDC must keep an editor's current state even before it is collected
            // into the song. Changed song values still override that state as usual.
            self.remotes
                .get(&x.id)
                .filter(|r| StateIntent(r.applied.clone()).accepts(x))
                .cloned()
        } else {
            self.remotes.remove(&x.id)
        };
        // It plays on there until the new mix does. One the build in progress opened already
        // and moves again has never played.
        if !stays
            && let Some(r) = &moving
            && !self.leaving.contains_key(&x.id)
        {
            self.leaving.insert(x.id, Leaving::Remote(r.clone()));
        }
        // The window open on the instance in the host it leaves opens on the new one when
        // that one plays.
        let window = moving
            .as_ref()
            .filter(|_| !stays)
            .and_then(|r| r.link.editor());
        let transfer = moving
            .as_ref()
            .filter(|old| StateIntent(old.applied.clone()).accepts(x))
            .map(|old| {
                (
                    old.link.live_revision(),
                    StateIntent(old.applied.clone()).protected_params(x),
                    (!stays).then(|| old.link.update_gate()),
                )
            });
        let editor_context = moving.as_ref().map(|previous| previous.link.clone());
        let values = self.values(x, moving)?;
        let server = self.server_for(x.id, placement, host, &entry);
        let link = match self.remotes.get(&x.id) {
            Some(r) if stays => r.link.clone(),
            _ => PluginLink::new(),
        };
        link.set_bypassed(x.bypass);
        let spec = RemoteSpec {
            name: x.name.clone(),
            bundle: entry.path.clone(),
            class: if is_clap(x) {
                Some(clap_plugins::class_key(x))
            } else {
                parse_uid(&x.uid)
            },
            state: values.state,
            params: values.params,
            instrument: x.kind == PluginKind::Instrument,
            latency: DEFAULT_LATENCY,
        };
        self.remotes.insert(
            x.id,
            Remote {
                link: link.clone(),
                server: server.clone(),
                placement,
                owner: owner.to_string(),
                applied: x.clone(),
            },
        );
        self.fresh.push(x.id);
        if let Some(previous) = &editor_context {
            link.copy_editor_context_from(previous);
        }
        // Background rebuilds retain the same view context.
        // Cache it on the link before its deferred OpenEditor is sent.
        let context = self
            .editor_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&x.id)
            .cloned();
        if let Some(context) = context {
            if let Some(area) = context.area {
                server.editor_area(&link, area);
            }
            if let Some(bar) = context.bar {
                server.editor_bar(&link, bar);
            }
        }
        if let Some((title, caption)) = window {
            server.open_editor_later(&link, &title, caption);
        }
        let mut processor = RemotePlugin::new(server, link, spec);
        if let Some((revision, protected, basis)) = transfer {
            if let Some(basis) = basis {
                processor.preserve_updates_from(basis, revision, protected);
            } else {
                processor.preserve_live_edits_since(revision, protected);
            }
        } else {
            processor.discard_previous_editor_changes();
        }
        Ok((Box::new(processor), warnings))
    }

    pub fn opened(&self, id: PluginId) -> bool {
        self.remotes.contains_key(&id)
    }

    /// The instance in a host process that plays `id`: the one the build in progress moves
    /// away from, else the last build's.
    fn playing_remote(&self, id: PluginId) -> Option<&Remote> {
        match self.leaving.get(&id) {
            Some(Leaving::Remote(r)) => Some(r),
            None => self.remotes.get(&id),
        }
    }

    /// Warnings reported while host instances are opening.
    pub fn opening_warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for id in &self.fresh {
            let Some(r) = self.remotes.get(id) else {
                continue;
            };
            if self.late.contains_key(id) {
                continue;
            }
            out.extend(Self::remote_warnings(r));
        }
        out
    }

    pub(crate) fn opening_warnings_for(
        &self,
        ids: impl IntoIterator<Item = PluginId>,
    ) -> Vec<String> {
        let mut seen = HashSet::new();
        ids.into_iter()
            .filter(|id| seen.insert(*id))
            .filter_map(|id| self.remotes.get(&id))
            .flat_map(Self::remote_warnings)
            .collect()
    }

    /// What the last opening of a remote plugin said: the instance a new mix opened, before it
    /// plays too (ENG-067).
    fn remote_warnings(r: &Remote) -> Vec<String> {
        let mut out = Vec::new();
        let name = &r.applied.name;
        if let Some(e) = r.link.opening_error() {
            out.push(format!(
                "{}: {name} を読み込めません（バイパスして再生します）: {e}",
                r.owner
            ));
        }
        if let Some(o) = r.link.opening() {
            out.extend(
                o.warnings
                    .iter()
                    .map(|w| format!("{}: {name}: {w}", r.owner)),
            );
        }
        out
    }

    /// Lets the plugins [`PluginLoader::preload`] began in a host process join the next build
    /// before the host has answered (ENG-065): they play dry until they are in, and what the
    /// host says about them comes later from [`PluginLoader::take_late_warnings`].
    pub fn allow_late(&mut self) {
        for (id, r) in self.ready.iter_mut() {
            let Ok((p, _)) = r.result.as_mut() else {
                continue;
            };
            // Read before asking, so an answer that comes in between is not missed.
            let seen = self.remotes.get(id).map(|remote| remote.link.answers());
            if !p.preparing() {
                continue;
            }
            p.allow_late();
            if let Some(seen) = seen {
                self.late.insert(*id, seen);
            }
        }
    }

    /// Whether some plugin the build took before its host answered has no answer yet.
    pub fn late_pending(&self) -> bool {
        self.late.iter().any(|(id, seen)| {
            self.remotes
                .get(id)
                .is_some_and(|r| r.link.answers() <= *seen)
        })
    }

    /// What the host said about the plugins the build took before it answered, once it has
    /// (the text [`PluginLoader::opening_warnings`] gives for the others).
    pub fn take_late_warnings(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let remotes = &self.remotes;
        self.late.retain(|id, seen| {
            let Some(r) = remotes.get(id) else {
                return false;
            };
            if r.link.answers() <= *seen {
                return true;
            }
            out.extend(Self::remote_warnings(r));
            false
        });
        out
    }

    /// Some plugins run in a host process, so their state and parameters can change without
    /// a new mix, and their editors' edits can be taken into the song.
    pub fn live(&self) -> bool {
        !self.remotes.is_empty()
            || !self.leaving.is_empty()
            || !self.carried.is_empty()
            || !self.stock.is_empty()
    }

    /// Where the built-ins' files are read from and the rate they are read at (ENG-077). The
    /// builder sets both when it is made and when the song's folder moves.
    pub fn set_sample_home(&mut self, dir: Option<PathBuf>, rate: u32) {
        self.dir = dir;
        self.rate = rate;
    }

    /// `name` on the disk: relative names are the song folder's.
    fn resolve(&self, name: &str) -> PathBuf {
        let path = std::path::Path::new(name);
        match (&self.dir, path.is_absolute()) {
            (Some(dir), false) => dir.join(path),
            _ => path.to_path_buf(),
        }
    }

    /// Reads the files `x` lists into slots at the mix's rate, off the audio thread; the
    /// handle lays them out with the device's values. A file that cannot be read is an empty
    /// slot and is named in `warnings`.
    fn load_set(
        &mut self,
        x: &Plugin,
        warnings: &mut Vec<String>,
    ) -> Vec<Option<xirius_engine::stock::files::FileSlot>> {
        use xirius_engine::stock::files::FileSlot;
        let mut slots = Vec::with_capacity(x.samples.len());
        for name in &x.samples {
            let path = self.resolve(name);
            let leaf = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.clone());
            match self.wavs.load(&path, self.rate) {
                Ok(data) => slots.push(Some(FileSlot::new(leaf, data))),
                Err(e) => {
                    warnings.push(e);
                    slots.push(None);
                }
            }
        }
        slots
    }

    /// Warnings `apply` left while it moved a built-in's files (the missing ones).
    pub fn take_file_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.file_warnings)
    }

    /// Makes the processor of a built-in the engine has (PLG-016) with the song's values for
    /// it, and keeps its handle so they can move without a new mix. `None` when the plugin is
    /// not such a built-in. The warnings name values the built-in has no parameter for, and
    /// files it could not read (ENG-077).
    pub fn stock(&mut self, x: &Plugin) -> Option<(Box<dyn Processor>, Vec<String>)> {
        let def = x.builtin()?;
        let (processor, handle) = stock::create(def.uid)?;
        let mut warnings: Vec<String> = handle
            .set_all(&x.params)
            .into_iter()
            .map(|k| format!("{} に「{k}」というパラメーターはありません", x.name))
            .collect();
        if !x.samples.is_empty() && handle.takes_files() {
            let set = self.load_set(x, &mut warnings);
            handle.set_files(set);
        }
        if self.tempo > 0.0 {
            handle.set_tempo(self.tempo);
        }
        self.stock.insert(
            x.id,
            Stock {
                handle,
                applied: x.clone(),
                waiting: None,
            },
        );
        self.fresh_stock.push(x.id);
        Some((processor, warnings))
    }

    /// Sends again the files and values a full ring turned away, once the devices have
    /// polled (L2-smp-4): called with each poll of the engine, so the last change does not
    /// wait for the next one.
    pub fn resend_files(&mut self) {
        for s in self.stock.values_mut() {
            if s.handle.resend_files()
                && let Some(x) = s.waiting.take()
            {
                s.applied = x;
            }
        }
    }

    /// Whether the plugin runs as a built-in of the engine's.
    pub fn is_stock(&self, id: PluginId) -> bool {
        self.stock.contains_key(&id)
    }

    /// What the built-in shows of its sound to its window (ENG-072): the window turns it on
    /// with [`stock::Probe::watch`] while it shows and reads it on its own clock. `None` when
    /// the plugin is not a built-in.
    pub fn stock_probe(&self, id: PluginId) -> Option<Arc<stock::Probe>> {
        self.stock.get(&id).map(|s| s.handle.probe())
    }

    /// The 0..1 value a running built-in plays parameter `key` at now (what its lanes and
    /// edits last gave it). `None` when the plugin is not a running built-in.
    pub fn stock_value(&self, id: PluginId, key: &str) -> Option<f64> {
        self.stock.get(&id)?.handle.get(key)
    }

    /// The tempo the built-ins play at from now on (the Delay's note lengths).
    pub fn set_tempo(&mut self, bpm: f64) {
        self.tempo = bpm;
        for s in self.stock.values() {
            s.handle.set_tempo(bpm);
        }
    }

    /// Gives the running plugin the song's state and parameters for it. `false` when that
    /// takes a new mix: the plugin is not in a host process, or a value was removed and only
    /// opening it again brings back its default.
    pub fn apply(&mut self, x: &Plugin) -> bool {
        if let Some(s) = self.stock.get_mut(&x.id) {
            // What the handle holds: the waiting set's when one waits. Reading other files
            // is not for the window's thread (the fifth review's low 1).
            let held = s.waiting.as_ref().unwrap_or(&s.applied);
            if crate::host::on_window_thread() && held.samples != x.samples {
                return false;
            }
            // The last file gone, the stand-in plays again: a new mix.
            if x.samples.is_empty() && !s.applied.samples.is_empty() {
                return false;
            }
            // Only the values and the files move; another built-in or format takes a new
            // processor.
            let mut same = s.applied.clone();
            same.params.clone_from(&x.params);
            same.samples.clone_from(&x.samples);
            if same != *x {
                return false;
            }
            let params = s.waiting.as_ref().unwrap_or(&s.applied).params != x.params;
            // The files the handle holds: only the values move. A set a full ring turned
            // away takes them and still waits, sent again with them (the sixth review's
            // low 4); other files go in its place (the fourth review's low 9).
            if s.waiting.as_ref().unwrap_or(&s.applied).samples == x.samples {
                if params {
                    s.handle.set_all(&x.params);
                }
                match &mut s.waiting {
                    Some(w) => *w = x.clone(),
                    None => s.applied = x.clone(),
                }
                return true;
            }
            // A new set of files reaches the running device without a new mix (ENG-077).
            let handle = s.handle.clone();
            let mut w = Vec::new();
            let set = self.load_set(x, &mut w);
            self.file_warnings
                .extend(w.into_iter().map(|e| format!("{}: {e}", x.name)));
            // The ring being full (the device is not polling) keeps `applied` as it was, so
            // the next `apply` or `resend_files` sends the set again.
            // The values with the files, in one place of the ring: the new files never
            // sound with the last ones' values (the fifth review's low 1, the sixth's low 4).
            let sent = if params {
                handle.set_files_with(set, &x.params).0
            } else {
                handle.set_files(set)
            };
            if let Some(s) = self.stock.get_mut(&x.id) {
                if sent {
                    s.applied = x.clone();
                    s.waiting = None;
                } else {
                    s.waiting = Some(x.clone());
                }
            }
            return true;
        }
        let applied = self
            .remotes
            .get_mut(&x.id)
            .is_some_and(|r| apply_remote(r, x));
        // The instance that plays until the new mix does hears them too (ENG-071).
        if applied && let Some(l) = self.leaving.get_mut(&x.id) {
            let mut there = x.clone();
            let Leaving::Remote(r) = l;
            there.isolation = r.applied.isolation;
            apply_remote(r, &there);
        }
        applied
    }

    /// Moves one parameter of a running plugin to `value` (0..1) without a new mix and
    /// without the song holding it: a lane playing (AUT-002). False when the plugin is not
    /// running or has no such parameter.
    pub fn set_live_param(&self, id: PluginId, key: &str, value: f64) -> bool {
        if let Some(s) = self.stock.get(&id) {
            return s.handle.set(key, value);
        }
        if let Some(Leaving::Remote(r)) = self.leaving.get(&id) {
            return r.server.automate(&r.link, key, value, 0);
        }
        self.remotes
            .get(&id)
            .is_some_and(|r| r.server.automate(&r.link, key, value, 0))
    }

    /// The values moved in the plugin's window up to `e`, when `e` brings its state: the
    /// song's values for the parameters it names follow them. The plugin sends its state a
    /// while after the moves (once it has played them), often in a later frame of the screen
    /// than the moves were taken in (ENG-071), so the moves of an edit without a state are
    /// kept for the one that brings it.
    pub fn moves_until(&mut self, e: &RemoteEdit) -> Option<Vec<ParamEdit>> {
        let gate = self
            .update_gate(e.plugin)
            .unwrap_or_else(|| UpdateGate::new(e.stamp));
        let moves = self.unsettled.entry(e.plugin).or_default();
        moves.retain(|(stamp, p)| {
            gate.accepts(
                *stamp,
                Field::Parameter {
                    id: p.id,
                    title: &p.title,
                },
            )
        });
        for p in e.params.iter().filter(|p| p.value.is_some()) {
            if !gate.accepts(
                e.stamp,
                Field::Parameter {
                    id: p.id,
                    title: &p.title,
                },
            ) {
                continue;
            }
            moves.retain(|(_, m)| m.id != p.id);
            moves.push((e.stamp, p.clone()));
        }
        e.state.as_ref()?;
        self.unsettled
            .remove(&e.plugin)
            .map(|moves| moves.into_iter().map(|(_, p)| p).collect())
    }

    /// Records that the song now holds `state` and `params` for a plugin because they were
    /// taken from the plugin itself, so they are not sent back to it.
    pub fn taken(
        &mut self,
        id: PluginId,
        state: Option<String>,
        params: &[(String, f64)],
        stamp: Stamp,
        origin: Origin,
    ) {
        let playing = self.playing_remote(id).map(|r| r.link.current_gate());
        let accepted = match origin {
            Origin::Capture => playing.is_some_and(|gate| gate.accepts(stamp, Field::Receipt)),
            Origin::Editor | Origin::Transfer => self
                .update_gate(id)
                .is_some_and(|gate| gate.accepts(stamp, Field::State)),
        };
        if !accepted {
            return;
        }

        if let Some(r) = self.remotes.get_mut(&id) {
            if state.is_some() {
                r.applied.state.clone_from(&state);
                if origin == Origin::Capture
                    && let Some(decoded) = state.as_deref().and_then(|s| decode_state(s).ok())
                {
                    r.server.remember_snapshot(&r.link, stamp, decoded, params);
                }
            }
            for (k, v) in params {
                r.applied.params.insert(k.clone(), *v);
            }
        }
        if let Some(applied) = self.remotes.get(&id).map(|r| &r.applied) {
            self.intents.insert(id, Some(applied.clone()));
        }
        let applied = match self.leaving.get_mut(&id) {
            Some(Leaving::Remote(r)) => &mut r.applied,
            None => return,
        };
        if state.is_some() {
            applied.state = state;
        }
        for (k, v) in params {
            applied.params.insert(k.clone(), *v);
        }
    }

    /// Reads every plugin's state now, so the next [`PluginLoader::take_edits`] has it
    /// whether or not the plugin said anything had changed (PLG-057): a plugin that changes
    /// its patch in its own window may send no edit, and the song would be
    /// saved with the state from when it was opened. The ones in host processes are asked
    /// and waited for, up to `wait` (the ones whose host went down or never opened are not).
    pub fn begin_pull_states(&mut self) -> StatePull {
        self.begin_pull_states_matching(None, false)
    }

    /// [`PluginLoader::begin_pull_states`] for the plugins `ids` names only: a channel's
    /// inserts copied or replaced read what those hold now (MIX-045).
    pub fn begin_pull_states_of(&mut self, ids: &HashSet<PluginId>) -> StatePull {
        self.begin_pull_states_matching(Some(ids), false)
    }

    /// [`PluginLoader::begin_pull_states_of`] for a read that leaves the song as it is (a
    /// copy, MIX-045): the requested state is not taken into the song meanwhile. Read the answers
    /// with [`PluginLoader::peek_pulled_states`].
    pub fn begin_peek_states_of(&mut self, ids: &HashSet<PluginId>) -> StatePull {
        self.begin_pull_states_matching(Some(ids), true)
    }

    fn begin_pull_states_matching(
        &mut self,
        wanted: Option<&HashSet<PluginId>>,
        _peek: bool,
    ) -> StatePull {
        let selected = |id: &PluginId| wanted.is_none_or(|ids| ids.contains(id));
        let basis = self
            .state_basis()
            .into_iter()
            .filter(|(id, _)| selected(id))
            .collect();
        let mut errors = Vec::new();
        let mut remotes: Vec<(PluginId, &Remote)> = self
            .remotes
            .iter()
            .filter(|(id, _)| selected(id) && !self.leaving.contains_key(id))
            .map(|(id, r)| (*id, r))
            .collect();
        for (id, Leaving::Remote(r)) in &self.leaving {
            if selected(id) {
                remotes.push((*id, r));
            }
        }
        let asked: Vec<_> = remotes
            .into_iter()
            .filter(|(id, r)| {
                let s = r.server.status(&r.link);
                // A rejected opening never created an editable instance: keep
                // the song's existing opaque state, including missing plugins.
                // A running host lost to a crash cannot attest its latest state.
                if s.crashed {
                    errors.push((
                        *id,
                        format!("{}: プラグインの状態を取得できません", r.applied.name),
                    ));
                }
                !s.crashed && s.error.is_none()
            })
            .map(|(id, r)| {
                let request = r.server.request_state(&r.link);
                #[cfg(feature = "test-io")]
                let _ = xirius_core::io_test::before(
                    "capture_requested",
                    std::path::Path::new(&id.to_string()),
                );
                (id, r.link.clone(), request)
            })
            .collect();
        StatePull {
            cancelled: std::cell::Cell::new(false),
            asked,
            errors,
            basis,
            ignored: Default::default(),
        }
    }

    pub fn pull_states(&mut self, wait: Duration) -> Result<StatePull, String> {
        let asked = self.begin_pull_states();
        Self::wait_for_pull(asked, wait)
    }

    fn wait_for_pull(asked: StatePull, wait: Duration) -> Result<StatePull, String> {
        let end = Instant::now() + wait;
        while Instant::now() < end && !asked.ready() {
            std::thread::sleep(Duration::from_millis(2));
        }
        if !asked.ready() {
            asked.quarantine();
            return Err("プラグインの状態を取得できません。未保存の変更が残っています".into());
        }
        if let Some(error) = asked.error() {
            asked.cancel();
            return Err(error);
        }
        Ok(asked)
    }

    /// Edits made in the plugins' editors since the last call.
    pub fn take_edits(&mut self) -> Vec<RemoteEdit> {
        self.take_edits_with_pull(None, None)
            .expect("ordinary edit reports have no capture receipt")
    }

    /// What the plugins a pull asked have answered, each on its own: one that failed or did
    /// not answer is left out and the others are still read (MIX-045). Nothing the song
    /// collects from the plugins is taken (their edits stay for the next collection), so a
    /// copy can carry the states without the song changing. The pull should come from
    /// [`PluginLoader::begin_peek_states_of`].
    pub fn peek_pulled_states(&mut self, pull: &StatePull) -> Vec<RemoteEdit> {
        let ignored = pull.ignored.borrow();
        let mut out = Vec::new();
        for (id, link, request) in pull.asked.iter().filter(|(id, _, _)| !ignored.contains(id)) {
            if let Ok(e) = link.take_capture_edits(*request)
                && let Some(state) = e.state.as_ref()
            {
                out.push(RemoteEdit {
                    origin: e.origin,
                    stamp: e.stamp,
                    plugin: *id,
                    state: Some(xirius_plugins::encode_state(state)),
                    params: e.params,
                });
            }
        }
        out
    }

    /// Edits taken and not put in the song, for the next collection to take again.
    pub(crate) fn carry(&mut self, edits: Vec<RemoteEdit>) {
        self.carried.extend(edits);
    }

    /// Move retained edits only when the candidate is published. Cloning these
    /// at worker launch would replay edits collected while the worker ran.
    pub(crate) fn take_carried_for(
        &mut self,
        ids: &std::collections::BTreeSet<PluginId>,
    ) -> Vec<RemoteEdit> {
        let (take, keep) = std::mem::take(&mut self.carried)
            .into_iter()
            .partition(|e| ids.contains(&e.plugin));
        self.carried = keep;
        take
    }

    pub fn take_pulled_edits(&mut self, pull: &StatePull) -> Result<Vec<RemoteEdit>, String> {
        if !pull.ready() {
            return Err("プラグインの状態を取得中です".into());
        }
        if let Some(error) = pull.error() {
            return Err(error);
        }
        self.take_edits_with_pull(Some(pull), None)
    }

    fn state_basis(&self) -> HashMap<PluginId, StateIntent> {
        let mut basis: HashMap<_, _> = self
            .remotes
            .iter()
            .map(|(id, r)| (*id, StateIntent(r.applied.clone())))
            .collect();
        for (id, leaving) in &self.leaving {
            let Leaving::Remote(r) = leaving;
            let applied = &r.applied;
            basis.insert(*id, StateIntent(applied.clone()));
        }
        basis
    }

    pub(crate) fn current_edits(
        &mut self,
        target: &xirius_core::model::Project,
        pull: Option<&StatePull>,
    ) -> Result<Vec<RemoteEdit>, String> {
        self.current_edits_matching(target, pull, None)
    }

    fn current_edits_matching(
        &mut self,
        target: &xirius_core::model::Project,
        pull: Option<&StatePull>,
        only: Option<&HashSet<PluginId>>,
    ) -> Result<Vec<RemoteEdit>, String> {
        if let Some(pull) = pull {
            pull.reconcile(target);
        }
        let passive = self.state_basis();
        if let Some(pull) = pull {
            if !pull.ready() {
                return Err("プラグインの状態を取得中です".into());
            }
            if let Some(error) = pull.error() {
                return Err(error);
            }
        }
        let mut edits = self.take_edits_with_pull(pull, only)?;
        let current: HashMap<_, _> = crate::starting::hosted(target)
            .into_iter()
            .map(|(_, p)| (p.id, p))
            .collect();
        let mut stale_receipt = false;
        edits.retain_mut(|edit| {
            if edit.origin == Origin::Capture {
                let playing = self
                    .playing_remote(edit.plugin)
                    .map(|r| r.link.current_gate());
                if playing.is_none_or(|gate| !gate.accepts(edit.stamp, Field::Receipt)) {
                    stale_receipt = true;
                    return false;
                }
            }
            if let Some(gate) = self.update_gate(edit.plugin) {
                if edit.state.is_some() && !gate.accepts(edit.stamp, Field::State) {
                    return false;
                }
                edit.params.retain(|p| {
                    gate.accepts(
                        edit.stamp,
                        Field::Parameter {
                            id: p.id,
                            title: &p.title,
                        },
                    )
                });
            }
            // A capture needs this request's basis. Passive editor reports
            // outside a selective pull still use their live instance's basis.
            let basis = pull.and_then(|p| p.basis.get(&edit.plugin)).or_else(|| {
                (edit.origin != Origin::Capture)
                    .then(|| passive.get(&edit.plugin))
                    .flatten()
            });
            basis
                .zip(current.get(&edit.plugin))
                .is_some_and(|(old, now)| old.reconcile(now, edit))
        });
        if stale_receipt {
            return Err(xirius_engine::plugin_updates::STALE_UPDATE.into());
        }
        Ok(edits)
    }

    fn take_edits_with_pull(
        &mut self,
        pull: Option<&StatePull>,
        only: Option<&HashSet<PluginId>>,
    ) -> Result<Vec<RemoteEdit>, String> {
        let selected = |id: &PluginId| only.is_none_or(|ids| ids.contains(id));
        let (mut out, retained) = std::mem::take(&mut self.carried)
            .into_iter()
            .partition(|e| selected(&e.plugin));
        self.carried = retained;
        let mut remotes: Vec<(&PluginId, &Remote)> = self
            .remotes
            .iter()
            .filter(|(id, _)| selected(id) && !self.leaving.contains_key(id))
            .collect();
        for (id, Leaving::Remote(r)) in &self.leaving {
            if selected(id) {
                remotes.push((id, r));
            }
        }
        for (id, r) in remotes {
            let e = if let Some((_, _, request)) = pull.and_then(|p| {
                p.asked.iter().find(|(asked, link, _)| {
                    !p.ignored.borrow().contains(asked) && asked == id && Arc::ptr_eq(link, &r.link)
                })
            }) {
                r.link.take_capture_edits(*request)?
            } else {
                r.link.take_edits()
            };
            if e.state.is_none() && e.params.is_empty() {
                continue;
            }
            out.push(RemoteEdit {
                origin: e.origin,
                stamp: e.stamp,
                plugin: *id,
                state: e.state.as_ref().map(xirius_plugins::encode_state),
                params: e.params,
            });
        }
        out.sort_by_key(|e| e.params.first().map(|p| p.at));
        Ok(out)
    }

    /// Polls both VST3 controller threads and host links for latency changes (ENG-011).
    /// Other restart flags stay available to state/edit collection.
    pub fn take_latency_changes(&mut self) -> bool {
        let mut changed = false;
        for remote in self.remotes.values() {
            changed |= remote.link.take_latency_change().is_some();
        }
        for Leaving::Remote(r) in self.leaving.values() {
            changed |= r.link.take_latency_change().is_some();
        }
        changed
    }

    pub fn param_titles(&self, id: PluginId) -> Vec<(u32, String)> {
        self.playing_remote(id)
            .and_then(|r| r.link.opened())
            .map(|o| o.params)
            .unwrap_or_default()
    }

    pub fn param_steps(&self, id: PluginId) -> Vec<(u32, u32)> {
        self.playing_remote(id)
            .map(|r| r.link.opened_steps())
            .unwrap_or_default()
    }

    pub fn note_names(&self, id: PluginId) -> Vec<(u8, String)> {
        self.playing_remote(id)
            .map(|r| r.link.note_names())
            .unwrap_or_default()
    }

    pub fn statuses(&self) -> Vec<(PluginId, String, PluginStatus)> {
        self.reports()
            .into_iter()
            .map(|r| (r.id, r.name, r.status))
            .collect()
    }

    /// The plugins in host processes, how they are and where they run: while a new mix that
    /// moves a plugin is on its way, the instance that plays (ENG-071).
    pub fn reports(&self) -> Vec<PluginReport> {
        let ids = self.remotes.keys().chain(self.leaving.keys());
        let mut ids: Vec<PluginId> = ids.copied().collect();
        ids.sort();
        ids.dedup();
        let mut out: Vec<_> = ids
            .into_iter()
            .filter_map(|id| Some((id, self.playing_remote(id)?)))
            .map(|(id, r)| PluginReport {
                id,
                name: r.applied.name.clone(),
                placement: r.placement,
                requested: r.applied.isolation,
                status: r.server.status(&r.link),
            })
            .collect();
        out.sort_by_key(|r| r.id);
        out
    }

    /// The hosts in use: vendor groups first, then dedicated ones by plugin.
    fn servers(&self) -> Vec<&PluginServer> {
        let mut groups: Vec<_> = self.groups.iter().collect();
        groups.sort_by_key(|(group, _)| *group);
        let mut dedicated: Vec<_> = self.dedicated.iter().collect();
        dedicated.sort_by_key(|(id, _)| **id);
        groups
            .into_iter()
            .map(|(_, s)| s)
            .chain(dedicated.into_iter().map(|(_, s)| s))
            .collect()
    }

    /// Whether a host process went down.
    pub fn crashed(&self) -> bool {
        self.servers().into_iter().any(PluginServer::crashed)
    }

    /// Starts the host processes that went down again (all of them when none did) and opens
    /// their plugins with the state they had.
    pub fn restart(&self) -> Result<(), String> {
        let servers = self.servers();
        if servers.is_empty() {
            return Err("別のプロセスで実行中のプラグインはありません".into());
        }
        let down: Vec<_> = servers.iter().filter(|s| s.crashed()).collect();
        let pick: Vec<_> = if down.is_empty() {
            servers.iter().collect()
        } else {
            down
        };
        let mut errors = Vec::new();
        for s in pick {
            if let Err(e) = s.restart() {
                errors.push(e);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Starts the host process `id` runs in again, and no other. A plugin that runs in the
    /// engine has none.
    pub fn restart_of(&self, id: PluginId) -> Result<(), String> {
        let Some(server) = self.server_of(id) else {
            return Err("そのプラグインは別のプロセスで実行されていません".into());
        };
        server.restart()
    }

    /// As [`PluginLoader::restart`], without waiting for the plugins to open again (ENG-065):
    /// they play dry until they are back.
    pub fn restart_later(&self) -> Result<(), String> {
        let servers = self.servers();
        if servers.is_empty() {
            return Err("別のプロセスで実行中のプラグインはありません".into());
        }
        let down: Vec<_> = servers.iter().filter(|s| s.crashed()).collect();
        let pick: Vec<_> = if down.is_empty() {
            servers.iter().collect()
        } else {
            down
        };
        let mut errors = Vec::new();
        for s in pick {
            if let Err(e) = s.restart_later() {
                errors.push(e);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// As [`PluginLoader::restart_of`], without waiting (ENG-065).
    pub fn restart_of_later(&self, id: PluginId) -> Result<(), String> {
        let Some(server) = self.server_of(id) else {
            return Err("そのプラグインは別のプロセスで実行されていません".into());
        };
        server.restart_later()
    }

    /// Opens (`true`) or closes a plugin's editor in its host process.
    pub fn editor(
        &self,
        id: PluginId,
        open: bool,
        caption: Option<xirius_vst3::Caption>,
    ) -> Result<(), String> {
        let caption = {
            let mut contexts = self
                .editor_contexts
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let context = contexts.entry(id).or_default();
            if caption.is_some() {
                context.caption = caption;
            }
            context.caption
        };
        // While a new mix that moves it is on its way, the window is the playing instance's,
        // and the new one's opens when it plays (ENG-071).
        let moving = self.leaving.contains_key(&id);
        let (Some(r), new) = (self.playing_remote(id), self.remotes.get(&id)) else {
            return Err("そのプラグインは別のプロセスで実行されていません".into());
        };
        if open {
            r.server
                .open_editor(&r.link, &Self::editor_title(r), caption);
        } else {
            r.server.close_editor(&r.link);
        }
        if let Some(new) = new.filter(|_| moving) {
            if open {
                new.server
                    .open_editor_later(&new.link, &Self::editor_title(new), caption);
            } else {
                new.server.close_editor(&new.link);
            }
        }
        Ok(())
    }

    fn editor_title(remote: &Remote) -> String {
        format!("{} · {}", remote.applied.name, remote.owner)
    }

    /// The title of the playing instance's window in its plugin host.
    pub fn editor_window_title(&self, id: PluginId) -> Option<String> {
        self.playing_remote(id).map(Self::editor_title)
    }

    /// Queues a physical coordinate snapshot before opening the view.
    pub fn editor_bar(&self, id: PluginId, bar: xirius_vst3::EditorBar) {
        self.editor_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id)
            .or_default()
            .bar = Some(bar.clone());
        if let Some(r) = self.playing_remote(id) {
            r.server.editor_bar(&r.link, bar.clone());
        }
        if self.leaving.contains_key(&id)
            && let Some(new) = self.remotes.get(&id)
        {
            new.server.editor_bar(&new.link, bar);
        }
    }

    pub fn take_editor_actions(&self, id: PluginId) -> Vec<xirius_vst3::EditorAction> {
        self.playing_remote(id)
            .map_or_else(Vec::new, |r| r.link.take_editor_actions())
    }

    /// The host's opening, success, or failure reports for this editor.
    pub fn take_editor_open_events(
        &self,
        id: PluginId,
    ) -> Vec<xirius_engine::server::EditorOpenEvent> {
        self.playing_remote(id)
            .map_or_else(Vec::new, |r| r.link.take_editor_open_events())
    }

    pub fn editor_origin(&self, id: PluginId) -> Option<(i32, i32)> {
        let origin = self.playing_remote(id).and_then(|r| r.link.editor_origin());
        if let Some(origin) = origin
            && let Some(area) = self
                .editor_contexts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&id)
                .and_then(|context| context.area.as_mut())
        {
            area.restore = Some(origin);
        }
        origin
    }

    /// Queues a physical coordinate snapshot before opening the view.
    pub fn editor_area(&self, id: PluginId, area: xirius_vst3::EditorArea) {
        self.editor_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id)
            .or_default()
            .area = Some(area);
        if let Some(r) = self.playing_remote(id) {
            r.server.editor_area(&r.link, area);
        }
        if self.leaving.contains_key(&id)
            && let Some(new) = self.remotes.get(&id)
        {
            new.server.editor_area(&new.link, area);
        }
    }

    /// Queues a raise without changing the view's open/closed state.
    pub fn raise_editor(&self, id: PluginId, foreground_from: u64) {
        if let Some(r) = self.playing_remote(id) {
            r.server.raise_editor_from(&r.link, foreground_from);
        }
    }

    /// Gives the open window of `id` new caption colours.
    pub fn set_caption(&self, id: PluginId, caption: xirius_vst3::Caption) {
        self.editor_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id)
            .or_default()
            .caption = Some(caption);
        if let Some(r) = self.playing_remote(id) {
            r.server.set_caption(&r.link, caption);
        }
        // The window that opens when the new mix plays.
        if self.leaving.contains_key(&id)
            && let Some(new) = self.remotes.get(&id)
            && let Some((title, _)) = new.link.editor()
        {
            new.server
                .open_editor_later(&new.link, &title, Some(caption));
        }
    }

    pub fn server(&self) -> Option<&PluginServer> {
        self.groups.values().next()
    }

    /// The host a plugin runs in.
    pub fn server_of(&self, id: PluginId) -> Option<&PluginServer> {
        self.remotes.get(&id).map(|r| &r.server)
    }

    /// Where a plugin of the last build runs, when that is a host process.
    pub fn placement_of(&self, id: PluginId) -> Option<PluginIsolation> {
        self.remotes.get(&id).map(|r| r.placement)
    }

    /// The link of a plugin in a host process.
    pub fn link(&self, id: PluginId) -> Option<Arc<PluginLink>> {
        self.remotes.get(&id).map(|r| r.link.clone())
    }

    /// Whether the last build's instance of `id` has been answered for (PLG-050): its host
    /// opened it, failed to, or went down. Until then its window cannot be asked for, and what
    /// went wrong is not known yet.
    pub fn answered(&self, id: PluginId) -> bool {
        match self.remotes.get(&id) {
            Some(r) => r.link.answers() > 0 || r.server.crashed(),
            None => self.failed.contains(&id),
        }
    }

    /// The link of the instance in a host process that plays `id`: while a new mix that moves
    /// it is on its way, the one it moves away from (ENG-071).
    pub fn playing_link(&self, id: PluginId) -> Option<Arc<PluginLink>> {
        self.playing_remote(id).map(|r| r.link.clone())
    }

    /// The mix of the last build has been swapped in: the instances it opened again in their
    /// hosts are the ones that play, with the windows and what they report (ENG-066).
    /// A plugin moved to another host has its window there once the new mix plays, if its
    /// window was still open on the instance that played until then (ENG-071).
    pub fn promote_opened(&mut self) {
        self.promote_pressed();
        let until = Instant::now() + HAND_OVER;
        let mut following = Vec::new();
        for (id, r) in &self.remotes {
            if let Some(Leaving::Remote(old)) = self.leaving.get(id) {
                // Before the window is looked at: a word from the host after this is caught.
                let shut = old.link.windows_shut();
                if old.link.editor().is_none() && r.link.editor().is_some() {
                    // Closed while the new mix was on its way.
                    r.server.close_editor(&r.link);
                } else if r.link.editor().is_some() {
                    following.push(Following {
                        old_link: old.link.clone(),
                        old_server: old.server.clone(),
                        shut,
                        to: (r.server.clone(), r.link.clone()),
                        until,
                    });
                }
            }
            r.server.promote(&r.link);
        }
        for (id, l) in std::mem::take(&mut self.leaving) {
            let Leaving::Remote(old) = l;
            // Before the window is looked at, as above.
            if !self.remotes.contains_key(&id) && old.link.editor().is_some() {
                old.server.close_editor(&old.link);
            }
        }
        self.following = following;
    }

    /// The mix of the last build is rendered offline: it plays as soon as it is built, so the
    /// instances it took over from are let go of now (ENG-071). A builder used for several
    /// renders, as for the stems, would keep the first render's instances otherwise.
    pub fn built_offline(&mut self) {
        self.promote_pressed();
        self.leaving.clear();
    }

    pub fn settle_following(&mut self) {
        let now = Instant::now();
        self.following.retain(|f| {
            // Whether the host let go first: what it said before that is in by then.
            let let_go = f.old_server.has_let_go(&f.old_link) || now >= f.until;
            if f.old_link.windows_shut() != f.shut {
                let (server, link) = &f.to;
                if link.editor().is_some() {
                    server.close_editor(link);
                }
                return false;
            }
            !let_go
        });
    }

    /// Called when a build ends: forgets the plugins it did not open, and the hosts of their
    /// own that nothing runs in any more (they end once the old mix lets go of them).
    pub fn end_build(&mut self) {
        let fresh = std::mem::take(&mut self.fresh);
        self.remotes.retain(|id, _| fresh.contains(id));
        let fresh_stock = &self.fresh_stock;
        self.stock.retain(|id, _| fresh_stock.contains(id));
        // Free the file sets the audio thread handed back (ENG-077).
        for s in self.stock.values() {
            s.handle.reclaim_files();
        }
        self.ready.clear();
        self.took.retain(|id, _| fresh.contains(id));
        let remotes = &self.remotes;
        self.dedicated.retain(|id, _| {
            remotes
                .get(id)
                .is_some_and(|r| r.placement == PluginIsolation::Dedicated)
        });
        self.fresh = fresh;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_engine::server::EditPhase;

    #[test]
    fn host_group_keeps_a_vendors_shell_classes_together_and_falls_back_to_parent() {
        let mut first = xirius_plugins::Entry {
            uid: "a".into(),
            name: "First".into(),
            vendor: "Fixture Vendor".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            category: String::new(),
            version: String::new(),
            path: PathBuf::from("fixture/one.vst3"),
            ara: false,
            requires_process_main: false,
        };
        let mut second = first.clone();
        second.uid = "b".into();
        second.path = PathBuf::from("elsewhere/two.vst3");
        assert_eq!(
            PluginLoader::host_group(&first, None),
            PluginLoader::host_group(&second, None)
        );
        second.path = first.path.clone();
        second.vendor = "Different Vendor".into();
        let catalog = Catalog {
            entries: vec![first.clone(), second.clone()],
            ..Catalog::default()
        };
        assert_eq!(
            PluginLoader::host_group(&first, Some(&catalog)),
            PluginLoader::host_group(&second, Some(&catalog))
        );
        second.vendor.clear();
        first.vendor.clear();
        second.path = PathBuf::from("fixture/two.vst3");
        assert_eq!(
            PluginLoader::host_group(&first, None),
            PluginLoader::host_group(&second, None)
        );
        second.path = PathBuf::from("elsewhere/two.vst3");
        assert_ne!(
            PluginLoader::host_group(&first, None),
            PluginLoader::host_group(&second, None)
        );
    }

    #[test]
    fn pressed_bindings_follow_kept_tracks_without_unioning_candidates_or_retired_graphs() {
        use xirius_core::id::TrackId;
        use xirius_engine::performance::Pressed;
        let id = PluginId(7);
        let track = TrackId(8);
        let old = Arc::new(Pressed::default());
        old.publish([60].into_iter());
        let old_multi = Arc::new(Pressed::default());
        old_multi.publish([48].into_iter());
        let mut active = PluginLoader::default();
        active.begin_build();
        active.register_pressed(id, &old);
        active.register_multi_pressed(track, &old_multi);
        active.promote_opened();

        let mut kept = active.live_copy().unwrap()();
        kept.begin_build();
        kept.keep_built([id]);
        kept.keep_pressed_track(track);
        assert_eq!(kept.pressed_keys(id), vec![60]);
        old.publish([60, 62].into_iter());
        assert_eq!(active.pressed_keys(id), vec![60, 62]);
        kept.promote_opened();
        assert_eq!(kept.pressed_keys(id), vec![60, 62]);
        assert_eq!(kept.multi_pressed_keys(track), vec![48]);

        let replacement = Arc::new(Pressed::default());
        replacement.publish([72].into_iter());
        let replacement_multi = Arc::new(Pressed::default());
        replacement_multi.publish([55].into_iter());
        let mut discarded = kept.live_copy().unwrap()();
        discarded.begin_build();
        discarded.register_pressed(id, &replacement);
        discarded.register_multi_pressed(track, &replacement_multi);
        assert_eq!(
            discarded.pressed_keys(id),
            vec![60, 62],
            "candidate is not active yet"
        );
        drop(discarded);
        assert_eq!(kept.pressed_keys(id), vec![60, 62]);

        kept.begin_build();
        kept.register_pressed(id, &replacement);
        kept.register_multi_pressed(track, &replacement_multi);
        kept.promote_opened();
        assert_eq!(kept.pressed_keys(id), vec![72]);
        assert_eq!(kept.multi_pressed_keys(track), vec![55]);
        old.publish([63].into_iter());
        old_multi.publish([49].into_iter());
        assert_eq!(
            kept.pressed_keys(id),
            vec![72],
            "retired graph is still alive but excluded"
        );
        assert_eq!(kept.multi_pressed_keys(track), vec![55]);
    }

    #[test]
    fn editor_context_follows_background_rebuilds_without_opening_a_view() {
        let loader = PluginLoader::default();
        let id = PluginId(7);
        let bar = xirius_vst3::EditorBar {
            track: "Vocal".into(),
            device: "Effect".into(),
            preset: "Room".into(),
            presets: vec!["Room".into(), "Hall".into()],
            powered: true,
            pinned: true,
            ..Default::default()
        };
        let area = xirius_vst3::EditorArea {
            replacing: false,
            main_window: 123,
            main: [2000, 0, 3000, 800],
            work: [1920, 0, 3840, 1040],
            restore: Some((2200, 100)),
        };
        loader.editor_bar(id, bar.clone());
        loader.editor_area(id, area);
        let caption = xirius_vst3::Caption {
            dark: false,
            fill: 0xf4f4fa,
            text: 0x141830,
        };
        loader.set_caption(id, caption);
        let building = loader.live_copy().unwrap()();
        let snapshot = building
            .editor_contexts
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .unwrap();
        assert_eq!(snapshot.area, Some(area));
        assert_eq!(snapshot.bar, Some(bar.clone()));
        assert_eq!(snapshot.caption, Some(caption));
        let mut changed = bar;
        changed.pinned = false;
        loader.editor_bar(id, changed.clone());
        let changed_caption = xirius_vst3::Caption {
            dark: true,
            ..caption
        };
        loader.set_caption(id, changed_caption);
        assert_eq!(
            building.editor_contexts.lock().unwrap()[&id].caption,
            Some(changed_caption)
        );
        assert_eq!(
            building.editor_contexts.lock().unwrap()[&id].bar,
            Some(changed)
        );
        assert!(building.remotes.is_empty());
    }

    #[test]
    fn process_policy_precedes_first_load_for_new_song_and_preset_and_preload() {
        let base = Plugin {
            pad_outputs: Default::default(),
            id: PluginId(9),
            name: "Renamed fixture".into(),
            kind: PluginKind::Instrument,
            format: "vst3".into(),
            uid: "ABCDEF".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            sidechain: None,
            note_source: None,
            preset: None,
            samples: vec![],
            isolation: PluginIsolation::Shared,
        };
        let catalog = Catalog {
            entries: vec![xirius_plugins::Entry {
                uid: base.uid.clone(),
                name: "Original fixture".into(),
                vendor: "Fixture".into(),
                kind: base.kind,
                format: base.format.clone(),
                category: "Instrument".into(),
                version: "1".into(),
                path: PathBuf::from("missing-plugin.vst3"),
                ara: false,
                requires_process_main: false,
            }],
            failures: vec![],
        };
        let mut prefs = xirius_prefs::Prefs::default();
        prefs
            .set(
                &xirius_prefs::isolation::key(&base.format, &base.uid),
                &serde_json::json!("dedicated"),
            )
            .unwrap();
        let song: Plugin = serde_json::from_value(serde_json::to_value(&base).unwrap()).unwrap();
        let mut preset = base.clone();
        preset.state = Some("invalid state: must never be decoded locally".into());
        for plugin in [base, song, preset] {
            let mut loader = PluginLoader::new(Some(Arc::new(catalog.clone())));
            loader.set_process_prefs(prefs.clone());
            loader.set_isolation(Isolation::PerPlugin(None));
            assert!(loader.runs_apart(&plugin));
            let error = loader.instantiate("Track", &plugin).err().unwrap();
            assert!(error.contains("ホストが見つからない"), "{error}");
            let setup = crate::build::setup(48000, xirius_engine::RenderMode::Realtime);
            loader.preload("Track", &plugin, &setup);
            assert!(loader.preloaded(plugin.id), "preload recorded a result");
            let error = loader.instantiate("Track", &plugin).err().unwrap();
            assert!(error.contains("ホストが見つからない"), "{error}");
            assert!(loader.link(plugin.id).is_none());
        }
    }

    /// PLG-050: a plugin whose host could not be started is answered for, so
    /// its window says it could not be loaded rather than giving up in silence.
    #[test]
    fn a_plugin_whose_host_could_not_start_is_answered_for() {
        let id = PluginId(8);
        let plugin = Plugin {
            id,
            name: "Fixture".into(),
            pad_outputs: Default::default(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            uid: "not-installed".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            sidechain: None,
            note_source: None,
            preset: None,
            samples: Vec::new(),
            isolation: PluginIsolation::Shared,
        };
        let mut loader = PluginLoader::new(Some(Arc::new(Catalog::default())));
        assert!(!loader.answered(id), "not built");
        assert!(loader.instantiate("Track 1", &plugin).is_err());
        assert!(loader.answered(id), "its opening failed: answered");
        assert!(loader.statuses().iter().all(|(at, ..)| *at != id));
    }

    /// PLG-050: a plugin built for a host process is not answered for until the host says how
    /// its opening went, here that the host could not be started.
    #[test]
    fn a_plugin_in_a_host_is_answered_for_once_its_opening_is() {
        let id = PluginId(7);
        let missing = std::env::temp_dir()
            .join("xirius-no-such-host")
            .join("host.exe");
        let server = PluginServer::new(Some(missing));
        let link = PluginLink::new();
        let mut loader = PluginLoader::new(None);
        assert!(!loader.answered(id), "not built");
        loader.remotes.insert(
            id,
            Remote {
                link: link.clone(),
                server: server.clone(),
                placement: PluginIsolation::Shared,
                owner: String::new(),
                applied: Plugin {
                    id,
                    name: "Fixture".into(),
                    pad_outputs: Default::default(),
                    kind: PluginKind::Effect,
                    format: "vst3".into(),
                    uid: "fixture".into(),
                    bypass: false,
                    deactivated: false,
                    params: Default::default(),
                    state: None,
                    sidechain: None,
                    note_source: None,
                    preset: None,
                    samples: Vec::new(),
                    isolation: PluginIsolation::Shared,
                },
            },
        );
        assert!(!loader.answered(id), "built, not opened yet");
        let mut processor = RemotePlugin::new(server, link.clone(), RemoteSpec::default());
        processor.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: xirius_engine::RenderMode::Realtime,
        });
        assert!(loader.answered(id), "its opening failed: answered");
        // What the window then says it from: the plugin could not be loaded.
        let statuses = loader.statuses();
        let (.., status) = statuses.iter().find(|(at, ..)| *at == id).unwrap();
        assert!(status.error.is_some(), "{status:?}");
    }

    #[test]
    fn model_commands_reset_undo_and_observations_use_one_monotonic_intent_clock() {
        use xirius_core::{
            id::{Node, TrackId},
            model::{Project, Track, TrackKind},
        };
        let plugin = Plugin {
            pad_outputs: Default::default(),
            id: PluginId(42),
            name: "Fixture".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            uid: "fixture".into(),
            bypass: false,
            deactivated: false,
            params: [("1".into(), 0.25)].into_iter().collect(),
            state: Some("initial".into()),
            sidechain: None,
            note_source: None,
            preset: None,
            samples: Vec::new(),
            isolation: PluginIsolation::Shared,
        };
        let mut initial = Project::new("Fixture");
        let track = TrackId(1);
        let mut row = Track::new(track, TrackKind::Audio, "A");
        row.inserts.push(plugin.clone());
        initial.root.push(Node::Track(track));
        initial.tracks.insert(track, Arc::new(row));
        let link = PluginLink::new();
        let server = PluginServer::new(None);
        let mut loader = PluginLoader::new(None);
        loader.remotes.insert(
            plugin.id,
            Remote {
                link: link.clone(),
                server,
                placement: PluginIsolation::Shared,
                owner: String::new(),
                applied: plugin.clone(),
            },
        );
        loader.advance_intents(&initial, &initial);
        let old = link.update_gate().stamp();
        let mut changed = initial.clone();
        Arc::make_mut(changed.tracks.get_mut(&track).unwrap()).inserts[0]
            .params
            .insert("1".into(), 0.75);
        loader.advance_intents(&initial, &changed);
        let first = link.update_gate().stamp();
        assert!(first.generation > old.generation);
        let mut reset = changed.clone();
        Arc::make_mut(reset.tracks.get_mut(&track).unwrap()).inserts[0].state = None;
        loader.advance_intents(&changed, &reset);
        let second = link.update_gate().stamp();
        assert!(second.generation > first.generation);
        loader.advance_intents(&reset, &initial); // undo restores the same values/bytes
        let restored = link.update_gate().stamp();
        assert!(restored.generation > second.generation);
        assert!(!link.update_gate().accepts(old, Field::State));
        loader.taken(
            plugin.id,
            Some("observed".into()),
            &[("1".into(), 0.5)],
            restored,
            Origin::Capture,
        );
        let mut observed = initial.clone();
        let row = Arc::make_mut(observed.tracks.get_mut(&track).unwrap());
        row.inserts[0].state = Some("observed".into());
        row.inserts[0].params.insert("1".into(), 0.5);
        loader.advance_intents(&initial, &observed);
        assert_eq!(
            link.update_gate().stamp(),
            restored,
            "accepted state is an observation, not another explicit change"
        );
    }

    #[test]
    fn selective_transfer_retains_passive_edits_and_does_not_invent_capture_receipts() {
        use xirius_core::{
            id::{Node, TrackId},
            model::{Project, Track, TrackKind},
        };
        let plugin = Plugin {
            pad_outputs: Default::default(),
            id: PluginId(42),
            name: "Fixture".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            uid: "fixture".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: Some("old".into()),
            sidechain: None,
            note_source: None,
            preset: None,
            samples: Vec::new(),
            isolation: PluginIsolation::Shared,
        };
        let mut p = Project::new("retained editor");
        let track = TrackId(3);
        let mut row = Track::new(track, TrackKind::Audio, "fixture");
        row.inserts.push(plugin.clone());
        p.root.push(Node::Track(track));
        p.tracks.insert(track, Arc::new(row));
        let mut loader = PluginLoader::new(None);
        let link = PluginLink::new();
        loader.remotes.insert(
            plugin.id,
            Remote {
                link: link.clone(),
                server: PluginServer::new(None),
                placement: PluginIsolation::Shared,
                owner: String::new(),
                applied: plugin.clone(),
            },
        );
        let edit = |origin| RemoteEdit {
            origin,
            stamp: link.update_gate().stamp(),
            plugin: plugin.id,
            state: Some("editor patch".into()),
            params: vec![],
        };
        loader.carry(vec![edit(Origin::Editor)]);
        let kept = [plugin.id].into_iter().collect();
        loader.capture_for_live_build(&p, &kept).unwrap();
        assert_eq!(
            loader.carried.len(),
            1,
            "empty wanted must not drain retained edits"
        );
        assert_eq!(loader.carried[0].origin, Origin::Editor);
        let moved = loader.take_carried_for(&kept);
        assert_eq!(
            moved.len(),
            1,
            "publication transfers the retained carry exactly once"
        );
        assert!(loader.take_carried_for(&kept).is_empty());
        loader.carry(moved);
        let empty = HashSet::new();
        let pull = loader.begin_pull_states_matching(Some(&empty), false);
        let edits = loader.current_edits(&p, Some(&pull)).unwrap();
        assert_eq!(
            edits.len(),
            1,
            "passive uses live basis outside selective pull"
        );
        assert_eq!(edits[0].state.as_deref(), Some("editor patch"));
        loader.carry(vec![edit(Origin::Capture)]);
        assert!(
            loader.current_edits(&p, Some(&pull)).unwrap().is_empty(),
            "a capture outside this request has no receipt basis"
        );
        loader.carry(vec![edit(Origin::Editor)]);
        let mut reset = p.clone();
        Arc::make_mut(reset.tracks.get_mut(&track).unwrap()).inserts[0].state = None;
        loader.advance_intents(&p, &reset);
        assert!(
            loader
                .current_edits(&reset, Some(&pull))
                .unwrap()
                .is_empty(),
            "an explicit reset still wins over the retained editor"
        );
    }

    #[test]
    fn one_intent_rule_preserves_reset_replacement_and_changed_parameter_aliases() {
        let old = Plugin {
            pad_outputs: Default::default(),
            id: PluginId(42),
            name: "Fixture".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            uid: "fixture".into(),
            bypass: false,
            deactivated: false,
            params: [("1".into(), 0.25)].into_iter().collect(),
            state: Some("old".into()),
            sidechain: None,
            note_source: None,
            preset: None,
            samples: Vec::new(),
            isolation: PluginIsolation::default(),
        };
        let intent = StateIntent(old.clone());
        for change in [0, 1, 2, 3] {
            let mut current = old.clone();
            match change {
                0 => current.state = None,
                1 => current.state = Some("new".into()),
                2 => current.uid = "replacement".into(),
                _ => current.kind = PluginKind::Instrument,
            }
            assert!(!intent.accepts(&current));
        }
        let mut current = old.clone();
        current.params.clear();
        current.params.insert("Latency".into(), 0.75);
        let mut edit = RemoteEdit {
            origin: Origin::Editor,
            stamp: Stamp::default(),
            plugin: old.id,
            state: Some("observed".into()),
            params: vec![ParamEdit {
                id: 1,
                title: "Latency".into(),
                value: Some(0.5),
                phase: EditPhase::Snapshot,
                at: Instant::now(),
            }],
        };
        let values = intent.values(
            &current,
            vec![("1".into(), 0.25), ("Latency".into(), 0.5)],
            &edit.params,
            &[(1, "Latency".into())],
        );
        assert_eq!(values, vec![("Latency".into(), 0.75)]);
        let mut alias = old.clone();
        alias.params = [("Latency".into(), 0.25)].into_iter().collect();
        let values = StateIntent(alias.clone()).values(
            &alias,
            vec![("1".into(), 0.5)],
            &[],
            &[(1, "Latency".into())],
        );
        assert_eq!(
            values,
            vec![("1".into(), 0.5)],
            "an unchanged title alias cannot restore the old model value"
        );
        assert!(intent.reconcile(&current, &mut edit));
        assert!(edit.params.is_empty());
    }

    #[test]
    fn a_restart_for_a_plugin_that_runs_here_says_it_has_no_process() {
        let loader = PluginLoader::new(None);
        let e = loader.restart_of(PluginId(7)).unwrap_err();
        assert!(e.contains("別のプロセス"), "{e}");
    }

    /// A sampler whose song lists a file reads it off the audio thread and its voices play
    /// it; a file that cannot be read is a warning and the slot stays silent (ENG-077).
    #[test]
    fn a_builtins_files_are_read_off_the_audio_thread_and_play() {
        use std::f64::consts::TAU;
        use xirius_engine::{AudioIo, NoteEvent, NoteKind, RenderMode};
        let tmp = tempfile::Builder::new()
            .prefix("xirius-plugin-samples-")
            .tempdir()
            .unwrap();
        let path = tmp.path().join("hit.wav");
        let mut w = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for i in 0..9_600i32 {
            let v = ((i as f64 * 440.0 * TAU / 48_000.0).sin() * 0.4) as f32;
            w.write_sample(v).unwrap();
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();

        let mk = |samples: Vec<String>| Plugin {
            pad_outputs: Default::default(),
            id: PluginId(9),
            name: "Sampler".into(),
            kind: PluginKind::Instrument,
            format: "builtin".into(),
            uid: "sampler".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            sidechain: None,
            note_source: None,
            preset: None,
            samples,
            isolation: PluginIsolation::default(),
        };
        let mut loader = PluginLoader::new(None);
        loader.set_sample_home(Some(tmp.path().to_path_buf()), 48_000);

        // A file that is there reaches the voices.
        let x = mk(vec!["hit.wav".into()]);
        let (mut p, warnings) = loader.stock(&x).unwrap();
        assert_eq!(warnings, Vec::<String>::new());
        p.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: RenderMode::Realtime,
        });
        let mut left = [0.0f32; 512];
        let mut right = [0.0f32; 512];
        let mut io = AudioIo {
            left: &mut left,
            right: &mut right,
            key: None,
            pad_routes: None,
            route_owner: 0,
        };
        p.process(
            &mut io,
            &[NoteEvent {
                offset: 0,
                kind: NoteKind::On {
                    pitch: 60,
                    velocity: 1.0,
                },
            }],
        );
        assert!(left.iter().any(|v| *v != 0.0), "no sound");

        // A file that is not there is a warning, and the device still opens.
        let x = mk(vec!["gone.wav".into()]);
        let (_, warnings) = loader.stock(&x).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("gone.wav"), "{}", warnings[0]);
    }

    /// A set that cannot go into a full ring is not marked applied, and the next poll after
    /// the device took the ring sends it again without another change (ENG-077, L2-smp-4).
    #[test]
    fn a_full_ring_leaves_the_new_files_pending_until_the_device_polls() {
        use xirius_engine::{AudioIo, RenderMode};
        let tmp = tempfile::Builder::new()
            .prefix("xirius-plugin-samples-")
            .tempdir()
            .unwrap();
        let mk = |samples: Vec<String>| Plugin {
            pad_outputs: Default::default(),
            id: PluginId(9),
            name: "Sampler".into(),
            kind: PluginKind::Instrument,
            format: "builtin".into(),
            uid: "sampler".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            sidechain: None,
            note_source: None,
            preset: None,
            samples,
            isolation: PluginIsolation::default(),
        };
        let mut loader = PluginLoader::new(None);
        loader.set_sample_home(Some(tmp.path().to_path_buf()), 48_000);
        let x = mk(vec!["a.wav".into()]);
        let (mut p, _) = loader.stock(&x).unwrap();
        // Fill the inbox ring (QUEUE is 8; one set is already in it from `stock`).
        let h = loader.stock.get(&x.id).unwrap().handle.clone();
        for _ in 0..8 {
            if !h.set_files(Vec::new()) {
                break;
            }
        }
        // The next set does not fit: the change is not marked applied.
        let x2 = mk(vec!["b.wav".into()]);
        assert!(loader.apply(&x2));
        assert_eq!(loader.stock[&x.id].applied.samples, vec!["a.wav"]);
        loader.resend_files();
        assert_eq!(
            loader.stock[&x.id].applied.samples,
            vec!["a.wav"],
            "still full"
        );
        // The device polls once, the ring empties, and the change lands on a retry.
        p.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: RenderMode::Realtime,
        });
        let mut left = [0.0f32; 16];
        let mut right = [0.0f32; 16];
        p.process(
            &mut AudioIo {
                left: &mut left,
                right: &mut right,
                key: None,
                pad_routes: None,
                route_owner: 0,
            },
            &[],
        );
        loader.resend_files();
        assert_eq!(loader.stock[&x.id].applied.samples, vec!["b.wav"]);
        assert!(loader.stock[&x.id].waiting.is_none());
        assert!(loader.apply(&x2));
        assert_eq!(loader.stock[&x.id].applied.samples, vec!["b.wav"]);
    }

    /// The fourth review's low 9: a set turned away by a full ring, then undone back to the
    /// files applied, sends those files with the next poll, not the set turned away.
    #[test]
    fn undone_back_to_the_applied_files_a_set_turned_away_does_not_go() {
        use xirius_engine::{AudioIo, RenderMode};
        let tmp = tempfile::Builder::new()
            .prefix("xirius-plugin-samples-")
            .tempdir()
            .unwrap();
        let mk = |samples: Vec<String>| Plugin {
            pad_outputs: Default::default(),
            id: PluginId(9),
            name: "Sampler".into(),
            kind: PluginKind::Instrument,
            format: "builtin".into(),
            uid: "sampler".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            sidechain: None,
            note_source: None,
            preset: None,
            samples,
            isolation: PluginIsolation::default(),
        };
        let mut loader = PluginLoader::new(None);
        loader.set_sample_home(Some(tmp.path().to_path_buf()), 48_000);
        let x = mk(vec!["a.wav".into()]);
        let (mut p, _) = loader.stock(&x).unwrap();
        let h = loader.stock.get(&x.id).unwrap().handle.clone();
        for _ in 0..8 {
            if !h.set_files(Vec::new()) {
                break;
            }
        }
        let x2 = mk(vec!["b.wav".into()]);
        assert!(loader.apply(&x2));
        assert_eq!(
            loader.stock[&x.id].waiting.as_ref().map(|w| &w.samples),
            Some(&x2.samples)
        );
        // Undone: A waits to go in B's place, though A is what was applied.
        assert!(loader.apply(&x));
        assert_eq!(
            loader.stock[&x.id].waiting.as_ref().map(|w| &w.samples),
            Some(&x.samples),
            "B would go with the next poll"
        );
        p.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: RenderMode::Realtime,
        });
        let (mut left, mut right) = ([0.0f32; 16], [0.0f32; 16]);
        p.process(
            &mut AudioIo {
                left: &mut left,
                right: &mut right,
                key: None,
                pad_routes: None,
                route_owner: 0,
            },
            &[],
        );
        loader.resend_files();
        assert_eq!(loader.stock[&x.id].applied.samples, vec!["a.wav"]);
        assert!(loader.stock[&x.id].waiting.is_none());
        // Applied again with nothing waiting, it returns early.
        assert!(loader.apply(&x));
        assert!(loader.stock[&x.id].waiting.is_none());
    }

    /// The fifth review's low 1: undone to the files applied with other values while a set
    /// waits, the device ends with the files undone to, though the ring has one place left
    /// (the values' layout used to take it with the files turned away). The two files differ
    /// in length, so the sound tells which the device holds.
    #[test]
    fn undone_with_other_values_the_device_ends_with_the_files_undone_to() {
        use std::f64::consts::TAU;
        use xirius_engine::{AudioIo, NoteEvent, NoteKind, RenderMode};
        let tmp = tempfile::Builder::new()
            .prefix("xirius-plugin-samples-")
            .tempdir()
            .unwrap();
        let wav = |name: &str, frames: i32| {
            let mut w = hound::WavWriter::create(
                tmp.path().join(name),
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 48_000,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .unwrap();
            for i in 0..frames {
                let v = ((i as f64 * 440.0 * TAU / 48_000.0).sin() * 0.4) as f32;
                w.write_sample(v).unwrap();
                w.write_sample(v).unwrap();
            }
            w.finalize().unwrap();
        };
        wav("short.wav", 480);
        wav("long.wav", 48_000);
        let mk = |samples: &str, normalize: Option<f64>| Plugin {
            pad_outputs: Default::default(),
            id: PluginId(9),
            name: "Sampler".into(),
            kind: PluginKind::Instrument,
            format: "builtin".into(),
            uid: "sampler".into(),
            bypass: false,
            deactivated: false,
            params: normalize
                .map(|v| ("file1.normalize".to_string(), v))
                .into_iter()
                .collect(),
            state: None,
            sidechain: None,
            note_source: None,
            preset: None,
            samples: vec![samples.into()],
            isolation: PluginIsolation::default(),
        };
        let mut loader = PluginLoader::new(None);
        loader.set_sample_home(Some(tmp.path().to_path_buf()), 48_000);
        let first = mk("short.wav", None);
        let (mut p, warnings) = loader.stock(&first).unwrap();
        assert_eq!(warnings, Vec::<String>::new());
        p.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: RenderMode::Realtime,
        });
        let block = |p: &mut Box<dyn xirius_engine::Processor>, notes: &[NoteEvent]| {
            let (mut left, mut right) = ([0.0f32; 512], [0.0f32; 512]);
            p.process(
                &mut AudioIo {
                    left: &mut left,
                    right: &mut right,
                    key: None,
                    pad_routes: None,
                    route_owner: 0,
                },
                notes,
            );
            left.iter().any(|v| *v != 0.0)
        };
        // The values move with the files kept: applied.
        let valued = mk("short.wav", Some(1.0));
        assert!(loader.apply(&valued));
        assert_eq!(loader.stock[&first.id].applied, valued);
        // The ring full, the long file waits.
        let h = loader.stock.get(&first.id).unwrap().handle.clone();
        while h.set_files(Vec::new()) {}
        let long = mk("long.wav", Some(1.0));
        assert!(loader.apply(&long));
        assert_eq!(loader.stock[&first.id].waiting.as_ref(), Some(&long));
        // The device takes the ring; seven sets of the long file fill it again but one place.
        block(&mut p, &[]);
        let mut w = Vec::new();
        for _ in 0..7 {
            assert!(h.set_files(loader.load_set(&long, &mut w)));
        }
        // Undone two steps at once: the short file and the first values.
        assert!(loader.apply(&first));
        let s = &loader.stock[&first.id];
        assert_eq!((&s.applied, &s.waiting), (&first, &None));
        // A note held past the short file's end is silent by then: the short file sounds.
        let on = NoteEvent {
            offset: 0,
            kind: NoteKind::On {
                pitch: 60,
                velocity: 1.0,
            },
        };
        assert!(block(&mut p, &[on]), "no sound");
        let tail: Vec<bool> = (0..4).map(|_| block(&mut p, &[])).collect();
        assert_eq!(tail, vec![false; 4], "the long file sounds");
        // The first values follow with the next poll, and the files stay.
        loader.resend_files();
        block(&mut p, &[]);
        assert!(block(&mut p, &[on]), "no sound");
        let tail: Vec<bool> = (0..4).map(|_| block(&mut p, &[])).collect();
        assert_eq!(tail, vec![false; 4], "the long file sounds");
    }

    /// The fifth review's low 1: on the window's thread, a set waiting to go is not read and
    /// sent from there; the mix that comes instead takes the song's files.
    #[test]
    fn on_the_windows_thread_a_waiting_set_takes_a_new_mix() {
        let tmp = tempfile::Builder::new()
            .prefix("xirius-plugin-samples-")
            .tempdir()
            .unwrap();
        let mk = |samples: Vec<String>| Plugin {
            pad_outputs: Default::default(),
            id: PluginId(9),
            name: "Sampler".into(),
            kind: PluginKind::Instrument,
            format: "builtin".into(),
            uid: "sampler".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            sidechain: None,
            note_source: None,
            preset: None,
            samples,
            isolation: PluginIsolation::default(),
        };
        let mut loader = PluginLoader::new(None);
        loader.set_sample_home(Some(tmp.path().to_path_buf()), 48_000);
        let x = mk(vec!["a.wav".into()]);
        let _p = loader.stock(&x).unwrap();
        let h = loader.stock.get(&x.id).unwrap().handle.clone();
        while h.set_files(Vec::new()) {}
        assert!(loader.apply(&mk(vec!["b.wav".into()])));
        std::thread::scope(|t| {
            t.spawn(|| {
                crate::host::mark_window_thread();
                assert!(!loader.apply(&x), "read on the window's thread");
            });
        });
    }

    /// The sixth review's low 4: new files with new values take one place of the ring, so
    /// with one place left nothing waits to be sent again; and new values for a set that
    /// waits go with it, also from the window's thread, which reads no files for them.
    #[test]
    fn new_files_go_with_their_values_in_one_place_of_the_ring() {
        use xirius_engine::{AudioIo, RenderMode};
        let tmp = tempfile::Builder::new()
            .prefix("xirius-plugin-samples-")
            .tempdir()
            .unwrap();
        let mk = |samples: &str, normalize: Option<f64>| Plugin {
            pad_outputs: Default::default(),
            id: PluginId(9),
            name: "Sampler".into(),
            kind: PluginKind::Instrument,
            format: "builtin".into(),
            uid: "sampler".into(),
            bypass: false,
            deactivated: false,
            params: normalize
                .map(|v| ("file1.normalize".to_string(), v))
                .into_iter()
                .collect(),
            state: None,
            sidechain: None,
            note_source: None,
            preset: None,
            samples: vec![samples.into()],
            isolation: PluginIsolation::default(),
        };
        let mut loader = PluginLoader::new(None);
        loader.set_sample_home(Some(tmp.path().to_path_buf()), 48_000);
        let first = mk("a.wav", None);
        let (mut p, _) = loader.stock(&first).unwrap();
        p.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: RenderMode::Realtime,
        });
        let mut poll = || {
            let (mut left, mut right) = ([0.0f32; 16], [0.0f32; 16]);
            p.process(
                &mut AudioIo {
                    left: &mut left,
                    right: &mut right,
                    key: None,
                    pad_routes: None,
                    route_owner: 0,
                },
                &[],
            );
        };
        // The device takes the ring; seven sets fill it again but one place.
        poll();
        let h = loader.stock.get(&first.id).unwrap().handle.clone();
        for _ in 0..7 {
            assert!(h.set_files(Vec::new()));
        }
        let second = mk("b.wav", Some(1.0));
        assert!(loader.apply(&second));
        let s = &loader.stock[&first.id];
        assert_eq!((&s.applied, &s.waiting), (&second, &None));
        assert!(h.resend_files(), "the values wait to go again");
        // The ring full, the next files wait; new values for them, from the window's
        // thread, wait with them.
        while h.set_files(Vec::new()) {}
        let third = mk("c.wav", Some(1.0));
        assert!(loader.apply(&third));
        let valued = mk("c.wav", Some(0.0));
        std::thread::scope(|t| {
            t.spawn(|| {
                crate::host::mark_window_thread();
                assert!(loader.apply(&valued));
            });
        });
        let s = &loader.stock[&first.id];
        assert_eq!((&s.applied, s.waiting.as_ref()), (&second, Some(&valued)));
        poll();
        loader.resend_files();
        let s = &loader.stock[&first.id];
        assert_eq!((&s.applied, &s.waiting), (&valued, &None));
    }

    fn edit(id: u32, phase: EditPhase, value: Option<f64>) -> ParamEdit {
        ParamEdit {
            id,
            title: String::new(),
            phase,
            value,
            at: Instant::now(),
        }
    }

    #[test]
    fn the_moves_wait_for_the_state_that_comes_in_a_later_frame() {
        let mut loader = PluginLoader::new(None);
        let plugin = PluginId(3);
        // The moves are taken in one frame; the plugin's state comes in the next.
        let moved = RemoteEdit {
            origin: Origin::Editor,
            stamp: Stamp::default(),
            plugin,
            state: None,
            params: vec![
                edit(1, EditPhase::Begin, None),
                edit(1, EditPhase::Change, Some(0.4)),
                edit(2, EditPhase::Change, Some(0.9)),
                edit(1, EditPhase::Change, Some(0.5)),
            ],
        };
        assert_eq!(loader.moves_until(&moved), None);
        let settled = RemoteEdit {
            origin: Origin::Editor,
            stamp: Stamp::default(),
            plugin,
            state: Some("c3RhdGU=".into()),
            params: vec![edit(1, EditPhase::End, None)],
        };
        let moves = loader.moves_until(&settled).expect("the state came");
        let last: Vec<(u32, Option<f64>)> = moves.iter().map(|p| (p.id, p.value)).collect();
        assert_eq!(last, [(2, Some(0.9)), (1, Some(0.5))]);
        // Taken with that state: the next one has none of them.
        let again = loader.moves_until(&settled).unwrap();
        assert!(again.is_empty());
    }
}
