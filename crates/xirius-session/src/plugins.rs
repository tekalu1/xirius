//! Loading the VST3 plugins a song names (PLG-002, PLG-052, OPEN-02), and the CLAP ones
//! ([`crate::clap_plugins`], PLG-028).
//!
//! A song keeps a plugin's class id (`uid`), its parameters by name and its saved state. The
//! catalogue from the last scan ([`xirius_plugins::Catalog`]) says which bundle offers the
//! class.
//!
//! Each plugin says in the song where it runs ([`PluginIsolation`], PLG-022): in this process
//! (the default), in the `xirius-plugin-host` process the other plugins share, or in a host
//! process of its own. A plugin in a host exchanges audio with the mix through shared memory,
//! one fixed delay `L` that delay compensation lines up (PLG-023). A plugin that crashes or
//! hangs takes only its host down: the plugins on that host go on with the dry signal until
//! [`PluginLoader::restart`], the ones elsewhere keep playing. A plugin that took the shared
//! host down is counted ([`PluginStatus::faults`]), so `plugin.status` can suggest giving it a
//! host of its own; that is never done by itself. Edits in the plugin's editor come back as
//! [`RemoteEdit`]s, for the song ([`crate::host::collect_plugin_states`]) and for writing
//! automation. Parameter and state changes from the song reach a plugin in a host without a
//! new mix. Moved to another process, a plugin is opened there with the state it had.
//!
//! A plugin in this process keeps its UI-thread side here ([`PluginLoader::local`]), so its
//! editor is the instance that plays: its edits are heard, come back as [`RemoteEdit`]s like
//! those of a plugin in a host, and the song's changes to its values reach it without a new
//! mix. A new mix opens it again with what its editor changed.
//!
//! The built-ins with a processor of the engine's ([`xirius_engine::stock`], PLG-016) are
//! opened here too: they always run in this process, and their parameters move without a new
//! mix like those of a plugin in a host (PLG-045).
//!
//! A plugin that uses ARA ([`xirius_plugins::Entry::ara`], PLG-025) always runs in this process,
//! with a warning when the song asked for another: ARA reads the song's audio here.
//!
//! A song's plugins can be opened before its mix is built ([`PluginLoader::preload`], ENG-064),
//! one at a time while a window keeps drawing; the build then takes them as they are. A plugin
//! in a host is asked for there and the answer is taken when the mix is built, so the plugins
//! of different hosts open at once. How long each one took is kept for
//! [`PluginLoader::open_times`].
//!
//! Without the host the plugins run in this process, with a warning for the ones that asked
//! for another. `XIRIUS_PLUGIN_ISOLATION` (`inprocess`, `shared`) puts every plugin in one
//! place whatever the song says, for tests.

use std::collections::{HashMap, HashSet};
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use xirius_core::id::PluginId;
use xirius_core::model::{Plugin, PluginIsolation, PluginKind};
use xirius_engine::plugin::Vst3Processor;
use xirius_engine::server::{
    DEFAULT_LATENCY, EditPhase, ParamEdit, PluginLink, PluginServer, PluginStatus, RemotePlugin,
    RemoteSpec, find_host,
};
use xirius_engine::stock::{self, StockHandle};
use xirius_engine::{ProcessSetup, Processor, Readied};
use xirius_plugins::{Catalog, decode_state, uid_hex};
use xirius_vst3::{Gesture, Module, PluginState};

use crate::clap_plugins::{self, is_clap};

/// Whether the song's plugin is one this loader opens.
pub fn is_vst3(x: &Plugin) -> bool {
    x.format.eq_ignore_ascii_case("vst3")
}

/// Where plugins run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Isolation {
    /// Every plugin in this process, whatever the song says: a plugin that crashes takes the
    /// program with it.
    InProcess,
    /// Every plugin in one process of the given `xirius-plugin-host` executable, whatever the
    /// song says.
    Separate(PathBuf),
    /// Where each plugin's [`PluginIsolation`] says, with the given `xirius-plugin-host`
    /// executable (`None`: there is none, and every plugin runs in this process).
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
    /// [`Isolation::set_default`]'s, else what `XIRIUS_PLUGIN_ISOLATION` says (`inprocess`, or
    /// `shared` with the host [`find_host`] finds), else each plugin where the song says with
    /// that host.
    fn default() -> Isolation {
        if let Some(i) = HOST_OVERRIDE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return i;
        }
        let forced = std::env::var("XIRIUS_PLUGIN_ISOLATION").unwrap_or_default();
        if forced.eq_ignore_ascii_case("inprocess") {
            return Isolation::InProcess;
        }
        if forced.eq_ignore_ascii_case("shared") || forced.eq_ignore_ascii_case("separate") {
            return find_host().map_or(Isolation::InProcess, Isolation::Separate);
        }
        Isolation::PerPlugin(find_host())
    }
}

/// What a plugin in a host process did in its editor since the last
/// [`PluginLoader::take_edits`].
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteEdit {
    pub plugin: PluginId,
    /// The plugin's state after the last edit settled (base64, as the song keeps it).
    pub state: Option<String>,
    /// The parameter moves, for automation (Touch / Latch / Write).
    pub params: Vec<ParamEdit>,
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

/// The instance that plays a plugin while the build in progress has opened it again in
/// another place, or again in this process (ENG-071). Until the new mix plays
/// ([`PluginLoader::promote_opened`]) its window, its reports and what is moved in it are this
/// one's; when the new mix is let go ([`PluginLoader::drop_ready`]) it is the plugin's again.
enum Leaving {
    Remote(Remote),
    /// With the parameters for automation of the plugin while it plays.
    Local(Local, Option<xirius_vst3::ParamSender>),
}

/// A window that moved with a plugin to another place when the new mix played (ENG-071).
/// Until the host of the instance that played has let go of it, that host may still say the
/// person shut the old window as the mix changed: then the moved window shuts too, or does not
/// open in this process.
struct Following {
    id: PluginId,
    /// The instance that played and its host, kept until the host has let go of it.
    old_link: Arc<PluginLink>,
    old_server: PluginServer,
    /// How many windows the old host had said were shut when the new mix played.
    shut: u64,
    /// The instance in a host that plays now, with the window that opened on it; `None` for
    /// one in this process, whose window the screen opens ([`PluginLoader::window_moved_in`]).
    to: Option<(PluginServer, Arc<PluginLink>)>,
    /// A host that has not let go by then is not waited for: its window moves as it is.
    until: Instant,
}

/// How long the moved window waits for a host that does not answer (ENG-071). A host that
/// answers lets go of the old instance within a frame or two of the new mix playing.
const HAND_OVER: Duration = Duration::from_secs(2);

/// An asynchronous state request. Polling never waits for another process.
pub struct StatePull {
    cancelled: std::cell::Cell<bool>,
    asked: Vec<(Arc<PluginLink>, u64)>,
    owned: Vec<crate::owned_local::Capture>,
    not_before: Instant,
}
impl StatePull {
    pub fn ready(&self) -> bool {
        self.cancelled.get()
            || (Instant::now() >= self.not_before
                && self.asked.iter().all(|(l, seen)| l.states_seen() > *seen)
                && self.owned.iter().all(|capture| capture.ready()))
    }

    pub fn error(&self) -> Option<String> {
        if self.cancelled.get() {
            return Some("プラグインの状態要求が取り消されました".into());
        }
        self.owned.iter().find_map(|capture| capture.error())
    }

    /// Abandon local owner replies even when an SDK call cannot be interrupted.
    /// This does not claim the plugin has stopped or its owner has retired.
    pub fn cancel(&self) {
        self.cancelled.set(true);
        for capture in &self.owned {
            capture.cancel();
        }
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

/// `kParamValuesChanged`: the plugin changed many values at once (a preset in its editor).
const PARAM_VALUES_CHANGED: i32 = 4;
const LATENCY_CHANGED: i32 = 8;
/// How long after an edit settles the plugin's state is read: by then the processor has had
/// the edit (as the host process waits).
pub(crate) const CAPTURE_WAIT: Duration = Duration::from_millis(300);
/// Parameter moves kept until [`PluginLoader::take_edits`] collects them.
const MAX_LOCAL_EDITS: usize = 65_536;

/// A value kept on the thread that made it. A VST3 plugin's UI-thread side may only be used
/// there (the SDK's rule), while the loader moves with the session; elsewhere it is out of
/// reach, and dropped there it is leaked rather than torn down on the wrong thread.
struct OnThread<T> {
    value: ManuallyDrop<T>,
    thread: ThreadId,
}

// SAFETY: the value is only reached and dropped on the thread that made it (see `get`, Drop).
unsafe impl<T> Send for OnThread<T> {}

impl<T> OnThread<T> {
    fn new(value: T) -> OnThread<T> {
        OnThread {
            value: ManuallyDrop::new(value),
            thread: std::thread::current().id(),
        }
    }

    fn get(&self) -> Option<&T> {
        (std::thread::current().id() == self.thread).then_some(&*self.value)
    }
}

impl<T> Drop for OnThread<T> {
    fn drop(&mut self) {
        if std::thread::current().id() == self.thread {
            // SAFETY: dropped once, on its own thread.
            unsafe { ManuallyDrop::drop(&mut self.value) };
        }
    }
}

/// A plugin running in this process: its UI-thread side, kept so that its editor is the
/// instance that plays and what the editor moves is heard and reaches the song.
struct Local {
    restart_pending: i32,
    plugin: Option<OnThread<xirius_vst3::Plugin>>,
    owned: Option<Box<crate::owned_local::OwnedLocal>>,
    applied: Plugin,
    /// Which instance this is ([`PluginLoader::local`]): an editor of an older one belongs to
    /// a mix that is gone.
    generation: u64,
    /// Parameter id → title.
    titles: Vec<(u32, String)>,
    /// The names the plugin gives its keys, read again with each state it takes or reports.
    note_names: Vec<(u8, String)>,
    /// When to read the state after an edit settled.
    capture: Option<Instant>,
}

impl Local {
    fn direct(&self) -> Option<&xirius_vst3::Plugin> {
        self.plugin.as_ref()?.get()
    }
    /// The parameter moves in the editor since the last call, and the state once the last
    /// edit has settled (or now, with `now`, when there were edits not read yet).
    fn take(&mut self, id: PluginId, now: bool) -> Option<RemoteEdit> {
        if let Some(owned) = &self.owned {
            let edits = owned.take(id);
            self.note_names = owned.names();
            return edits;
        }
        let plugin = self.plugin.as_ref()?.get()?;
        // What the processor changed on its own shows in the editor (not an edit of the user's).
        plugin.follow_processor();
        let handler = plugin.handler();
        let mut params = Vec::new();
        for g in handler.take_gestures() {
            let (pid, phase, value) = match g {
                Gesture::Begin(pid) => (pid, EditPhase::Begin, None),
                Gesture::Perform(pid, v) => (pid, EditPhase::Change, Some(v)),
                Gesture::End(pid) => (pid, EditPhase::End, None),
            };
            if phase == EditPhase::End {
                self.capture = Some(Instant::now() + CAPTURE_WAIT);
            }
            if params.len() >= MAX_LOCAL_EDITS {
                params.remove(0);
            }
            let title = self
                .titles
                .iter()
                .find(|(i, _)| *i == pid)
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            params.push(ParamEdit {
                id: pid,
                title,
                phase,
                value,
                at: Instant::now(),
            });
        }
        let flags = handler.take_restart_flags() | std::mem::take(&mut self.restart_pending);
        // State collection must not consume a pending latency notification.
        self.restart_pending |= flags & LATENCY_CHANGED;
        if flags & PARAM_VALUES_CHANGED != 0 && self.capture.is_none() {
            self.capture = Some(Instant::now() + CAPTURE_WAIT);
        }
        let due = match self.capture {
            Some(at) => now || Instant::now() >= at,
            None => now && !params.is_empty(),
        };
        let state = if due {
            self.capture = None;
            // A preset chosen in the editor may name its keys anew.
            self.note_names = plugin.note_names();
            plugin.get_state().ok()
        } else {
            None
        };
        if state.is_none() && params.is_empty() {
            return None;
        }
        Some(RemoteEdit {
            plugin: id,
            state: state.as_ref().map(xirius_plugins::encode_state),
            params,
        })
    }

    /// Gives the playing instance the song's state and parameters for it, as a plugin in a
    /// host process is given them. `false` when that takes a new mix.
    fn apply(&mut self, x: &Plugin) -> bool {
        let mut same = self.applied.clone();
        same.state.clone_from(&x.state);
        same.params.clone_from(&x.params);
        if same != *x {
            return false;
        }
        if self.applied.state == x.state && self.applied.params == x.params {
            return true;
        }
        if self
            .applied
            .params
            .keys()
            .any(|k| !x.params.contains_key(k))
        {
            return false;
        }
        if let Some(owned) = &self.owned {
            if !owned.apply(x, &self.applied) {
                return false;
            }
            self.applied = x.clone();
            return true;
        }
        let Some(plugin) = self.plugin.as_ref().and_then(OnThread::get) else {
            return false;
        };
        let state_changed = self.applied.state != x.state;
        if state_changed {
            let Some(Ok(state)) = x
                .state
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(decode_state)
            else {
                return false;
            };
            if plugin.set_state(&state).is_err() {
                return false;
            }
            // What the plugin reports about its own new state is not the user's.
            self.capture = None;
            plugin.handler().take_gestures();
            self.restart_pending |= plugin.handler().take_restart_flags() & LATENCY_CHANGED;
            self.note_names = plugin.note_names();
        }
        for (key, value) in &x.params {
            if (state_changed || self.applied.params.get(key) != Some(value))
                && let Some(id) = param_id(&plugin.params(), key)
            {
                plugin.set_param(id, value.clamp(0.0, 1.0));
            }
        }
        self.applied = x.clone();
        true
    }
}

/// Gives a plugin in a host process the song's state and parameters for it, as
/// [`PluginLoader::apply`]. `false` when that takes a new mix.
fn apply_remote(r: &mut Remote, x: &Plugin) -> bool {
    if r.applied.isolation != x.isolation {
        return false;
    }
    if r.applied.state == x.state && r.applied.params == x.params {
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
    true
}

/// A parameter by numeric id, else by title (case-insensitive), as a song names it.
fn param_id(params: &[xirius_vst3::ParamInfo], key: &str) -> Option<u32> {
    key.parse::<u32>()
        .ok()
        .filter(|id| params.iter().any(|p| p.id == *id))
        .or_else(|| {
            params
                .iter()
                .find(|p| p.title.eq_ignore_ascii_case(key))
                .map(|p| p.id)
        })
}

/// A running built-in and the song's plugin it was last given.
struct Stock {
    handle: StockHandle,
    applied: Plugin,
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
    Local,
    Clap,
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

/// Opens plugins for a build. Bundles stay loaded between builds, so a rebuild after an edit
/// does not load the DLL again; a plugin in a host process keeps its link across builds.
#[derive(Default)]
pub struct PluginLoader {
    threaded_local: bool,
    process_setup: Option<ProcessSetup>,
    catalog: Option<Arc<Catalog>>,
    modules: HashMap<PathBuf, Arc<Module>>,
    isolation: Isolation,
    /// The host the plugins share (OPEN-02: one for the VST3 plugins).
    shared: Option<PluginServer>,
    /// The hosts of plugins that run in one of their own.
    dedicated: HashMap<PluginId, PluginServer>,
    remotes: HashMap<PluginId, Remote>,
    /// Edits a plugin made in a process it has since left, for the next
    /// [`PluginLoader::take_edits`].
    carried: Vec<RemoteEdit>,
    /// Plugins of the build in progress, for [`PluginLoader::opening_warnings`].
    fresh: Vec<PluginId>,
    /// The parameters of plugins that run in this process, for automation while playing.
    senders: HashMap<PluginId, xirius_vst3::ParamSender>,
    /// The plugins of the last build that run in this process, and those of the build in
    /// progress.
    locals: HashMap<PluginId, Local>,
    fresh_locals: Vec<PluginId>,
    /// The last [`Local::generation`] given.
    generation: u64,
    /// The built-ins of the last build, and of the build in progress.
    stock: HashMap<PluginId, Stock>,
    fresh_stock: Vec<PluginId>,
    /// The names the CLAP plugins of the last build that run in this process give their keys
    /// (their values change only with a new mix), and those of the build in progress.
    clap_names: HashMap<PluginId, Vec<(u8, String)>>,
    fresh_clap: Vec<PluginId>,
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
    /// How long the plugins opened since the last build took here, with their owner and name.
    took: HashMap<PluginId, (String, String, Duration)>,
    /// Plugins in a host process the build took before the host answered (ENG-065), with how
    /// many answers their link had then: what the host says about them comes with
    /// [`PluginLoader::take_late_warnings`].
    late: HashMap<PluginId, u64>,
    /// The instances that play the plugins the build in progress opened again elsewhere.
    leaving: HashMap<PluginId, Leaving>,
    /// The plugins the last mix moved into this process with their host's window open, for
    /// [`PluginLoader::window_moved_in`].
    moved_in: HashSet<PluginId>,
    /// Windows that moved with a plugin when the last mix played, whose old host has not let
    /// go yet (ENG-071, [`PluginLoader::settle_following`]).
    following: Vec<Following>,
    /// The last value moved in each plugin's window since the last state it sent, for
    /// [`PluginLoader::moves_until`].
    unsettled: HashMap<PluginId, Vec<ParamEdit>>,
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
    pub(crate) fn set_threaded_local(&mut self, on: bool) {
        self.threaded_local = on;
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

    /// The catalogue to look plugins up in. Without one, the standard cache is read when a
    /// plugin is first needed.
    pub fn set_catalog(&mut self, catalog: Option<Arc<Catalog>>) {
        self.catalog = catalog;
    }

    /// Where plugins opened from now on run.
    pub fn set_isolation(&mut self, isolation: Isolation) {
        if isolation != self.isolation {
            self.shared = None;
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

    /// Called when a build starts.
    pub fn begin_build(&mut self) {
        self.fresh.clear();
        self.fresh_stock.clear();
        self.fresh_locals.clear();
        self.fresh_clap.clear();
    }

    /// Where `x` runs, and the host executable when that is not this process. `ara`: the
    /// catalogue says it uses ARA.
    fn placement(
        &self,
        x: &Plugin,
        ara: bool,
        warnings: &mut Vec<String>,
    ) -> (PluginIsolation, PathBuf) {
        if ara {
            if x.isolation.is_separate() {
                warnings.push(format!(
                    "{}: ARA のプラグインのため、このプロセスで実行します（{} の指定）",
                    x.name,
                    x.isolation.name()
                ));
            }
            return (PluginIsolation::InProcess, PathBuf::new());
        }
        match &self.isolation {
            Isolation::InProcess => (PluginIsolation::InProcess, PathBuf::new()),
            Isolation::Separate(host) => (PluginIsolation::Shared, host.clone()),
            Isolation::PerPlugin(Some(host)) => (x.isolation, host.clone()),
            Isolation::PerPlugin(None) => {
                if x.isolation.is_separate() {
                    warnings.push(format!(
                        "{}: xirius-plugin-host が見つからないため、このプロセスで実行します（{} の指定）",
                        x.name,
                        x.isolation.name()
                    ));
                }
                (PluginIsolation::InProcess, PathBuf::new())
            }
        }
    }

    /// The host for a plugin placed `placement`.
    fn server_for(
        &mut self,
        id: PluginId,
        placement: PluginIsolation,
        host: PathBuf,
    ) -> PluginServer {
        match placement {
            PluginIsolation::Dedicated => self
                .dedicated
                .entry(id)
                .or_insert_with(|| PluginServer::new(Some(host)))
                .clone(),
            _ => self
                .shared
                .get_or_insert_with(|| PluginServer::new(Some(host)))
                .clone(),
        }
    }

    /// What `x` is opened with: the song's values, or, when it moves out of a host process,
    /// what it has there now (with the edits in its editor that the song has not taken yet;
    /// those are kept for [`PluginLoader::take_edits`]).
    fn values(&mut self, x: &Plugin, moving: Option<Remote>) -> Result<Values, String> {
        if let Some(r) = moving
            && let Some(now) = r.server.current(&r.link)
        {
            let edits = r.link.take_edits();
            let mut params = now.params;
            for (key, v) in &mut params {
                if let Some(value) = edits
                    .params
                    .iter()
                    .rev()
                    .filter(|p| names(key, p))
                    .find_map(|p| p.value)
                {
                    *v = value;
                }
            }
            if edits.state.is_some() || !edits.params.is_empty() {
                self.carried.push(RemoteEdit {
                    plugin: x.id,
                    state: edits.state.as_ref().map(xirius_plugins::encode_state),
                    params: edits.params,
                });
            }
            return Ok(Values {
                state: now.state,
                params,
            });
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
        if let Some(r) = self.ready.remove(&x.id)
            && r.applied == *x
            && r.owner == owner
        {
            match r.list {
                Fresh::Remote => self.fresh.push(x.id),
                Fresh::Local => self.fresh_locals.push(x.id),
                Fresh::Clap => self.fresh_clap.push(x.id),
                Fresh::None => {}
            }
            return r.result;
        }
        let at = Instant::now();
        let out = self.open(owner, x);
        self.took
            .insert(x.id, (owner.to_string(), x.name.clone(), at.elapsed()));
        out
    }

    /// Opens `x` for the next build and begins getting it ready for `setup` (ENG-064): a
    /// plugin here is prepared now; one in a host is asked for there, and
    /// [`PluginLoader::preparing`] is true until the answer comes. The build takes it from
    /// [`PluginLoader::instantiate`], with the same warnings and errors.
    pub fn preload(&mut self, owner: &str, x: &Plugin, setup: &ProcessSetup) {
        let at = Instant::now();
        let before = (
            self.fresh.len(),
            self.fresh_locals.len(),
            self.fresh_clap.len(),
        );
        let result = self.open(owner, x).map(|(p, w)| {
            let p: Box<dyn Processor> = Box::new(Readied::new(p, setup));
            (p, w)
        });
        let list = if self.fresh.len() > before.0 {
            Fresh::Remote
        } else if self.fresh_locals.len() > before.1 {
            Fresh::Local
        } else if self.fresh_clap.len() > before.2 {
            Fresh::Clap
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

    /// Lets go of the plugins [`PluginLoader::preload`] opened and no build took (ENG-065).
    /// The ones they were to take over from are the plugins' again (ENG-071).
    pub fn drop_ready(&mut self) {
        self.ready.clear();
        // The new mix was let go: a window that was to follow it has nothing to follow.
        self.following.clear();
        for (id, l) in std::mem::take(&mut self.leaving) {
            match l {
                Leaving::Remote(r) => {
                    self.locals.remove(&id);
                    match r.placement {
                        PluginIsolation::Dedicated => {
                            self.dedicated.insert(id, r.server.clone());
                        }
                        _ => {
                            self.dedicated.remove(&id);
                        }
                    }
                    self.remotes.insert(id, r);
                }
                Leaving::Local(l, sender) => {
                    self.remotes.remove(&id);
                    self.dedicated.remove(&id);
                    self.locals.insert(id, l);
                    match sender {
                        Some(s) => self.senders.insert(id, s),
                        None => self.senders.remove(&id),
                    };
                }
            }
        }
    }

    /// Whether [`PluginLoader::preload`] has opened `id`.
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

    /// Whether `x` would run in a host process.
    pub fn runs_apart(&mut self, x: &Plugin) -> bool {
        let Some(ara) = self.catalog().find(&x.uid).map(|e| e.ara) else {
            return false;
        };
        self.placement(x, ara, &mut Vec::new()).0.is_separate()
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
        let _filter = xirius_engine::crash::guard_exception_filter();
        let entry = self.catalog().find(&x.uid).cloned().ok_or_else(|| {
            format!(
                "{}（{} {}）が見つかりません（plugin.rescan で再スキャンしてください）",
                x.name, x.format, x.uid
            )
        })?;
        let mut warnings = Vec::new();
        let (placement, host) = self.placement(x, entry.ara, &mut warnings);
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
                .filter(|r| {
                    r.applied.uid == x.uid
                        && r.applied.format == x.format
                        && r.applied.kind == x.kind
                        && r.applied.state == x.state
                })
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
        let replace_params = moving
            .as_ref()
            .is_some_and(|r| r.applied.params != x.params);
        // The window open on the instance in the host it leaves opens on the new one when
        // that one plays.
        let window = moving
            .as_ref()
            .filter(|_| !stays)
            .and_then(|r| r.link.editor());
        let mut values = self.values(x, moving)?;
        if replace_params {
            values.params = x.params.iter().map(|(k, v)| (k.clone(), *v)).collect();
        }
        // The instance that played in this process until now: what its editor changed that
        // the song has not taken yet goes to the new one, and to the song later.
        if let Some(mut old) = self.locals.remove(&x.id) {
            let preserve = old.applied.uid == x.uid
                && old.applied.format == x.format
                && old.applied.state == x.state;
            // Latency-only notifications need not include gestures or a parameter edit.
            if preserve {
                values.state = old
                    .direct()
                    .and_then(|p| p.get_state().ok())
                    .or(values.state);
            }
            if let Some(edit) = old.take(x.id, true) {
                if preserve && let Some(text) = &edit.state {
                    values.state = decode_state(text).ok().or(values.state);
                }
                if old.applied.params == x.params {
                    for (key, v) in &mut values.params {
                        if let Some(value) = edit
                            .params
                            .iter()
                            .rev()
                            .filter(|p| names(key, p))
                            .find_map(|p| p.value)
                        {
                            *v = value;
                        }
                    }
                }
                self.carried.push(edit);
            }
            if !self.leaving.contains_key(&x.id) {
                let sender = self.senders.remove(&x.id);
                self.leaving.insert(x.id, Leaving::Local(old, sender));
            }
        }

        if placement.is_separate() {
            let server = self.server_for(x.id, placement, host);
            let link = match self.remotes.get(&x.id) {
                Some(r) if stays => r.link.clone(),
                _ => PluginLink::new(),
            };
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
            if let Some((title, caption)) = window {
                server.open_editor_later(&link, &title, caption);
            }
            return Ok((Box::new(RemotePlugin::new(server, link, spec)), warnings));
        }

        if is_clap(x) {
            let (p, names) = clap_plugins::open_in_process(
                &entry.path,
                x,
                values.state.as_ref(),
                &values.params,
                &mut warnings,
            )?;
            self.clap_names.insert(x.id, names);
            self.fresh_clap.push(x.id);
            return Ok((p, warnings));
        }
        if self.threaded_local {
            let setup = self.process_setup.unwrap_or_else(|| {
                crate::build::setup(self.rate, xirius_engine::RenderMode::Realtime)
            });
            let ticket = xirius_vst3::owner::Controller::open(xirius_vst3::owner::Open {
                bundle: entry.path.clone(),
                class: parse_uid(&x.uid)
                    .ok_or_else(|| "プラグインの ID が正しくありません".to_string())?,
                state: values.state,
                params: values.params,
                requires_process_main: false,
                sample_rate: setup.sample_rate,
                max_block: setup.max_block,
                offline: setup.mode == xirius_engine::RenderMode::Offline,
            })?;
            // This synchronous builder entry is for non-UI callers. UI preloading
            // and the preparation coordinator use the asynchronous entry below.
            let opened = loop {
                if let Some(result) = ticket.poll() {
                    break result?;
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            let latency = opened
                .audio
                .prepared(
                    setup.sample_rate,
                    setup.max_block,
                    setup.mode == xirius_engine::RenderMode::Offline,
                )
                .map_err(|e| e.to_string())?;
            let titles: Vec<_> = opened.params.into_iter().map(|p| (p.id, p.title)).collect();
            self.senders.insert(x.id, opened.sender);
            self.generation += 1;
            self.locals.insert(
                x.id,
                Local {
                    plugin: None,
                    owned: Some(Box::new(crate::owned_local::OwnedLocal::new(
                        opened.controller,
                        titles.clone(),
                        opened.note_names.clone(),
                        latency,
                    ))),
                    restart_pending: 0,
                    applied: x.clone(),
                    generation: self.generation,
                    titles,
                    note_names: opened.note_names,
                    capture: None,
                },
            );
            self.fresh_locals.push(x.id);
            return Ok((Box::new(Vst3Processor::owned(opened.audio)), warnings));
        }
        let module = match self.modules.get(&entry.path) {
            Some(m) => m.clone(),
            None => {
                let m = Arc::new(
                    Module::load(&entry.path)
                        .map_err(|e| format!("{}: {e}", entry.path.display()))?,
                );
                self.modules.insert(entry.path.clone(), m.clone());
                m
            }
        };
        let class = module
            .classes()
            .into_iter()
            .find(|c| uid_hex(&c.cid).eq_ignore_ascii_case(&x.uid))
            .ok_or_else(|| format!("{} に {} が見つかりません", entry.path.display(), x.uid))?;
        let mut plugin = xirius_vst3::Plugin::new(module, &class)
            .map_err(|e| format!("{} を読み込めません: {e}", x.name))?;
        if let Some(state) = &values.state {
            plugin
                .set_state(state)
                .map_err(|e| format!("{} の状態を復元できません: {e}", x.name))?;
        }
        if !values.params.is_empty() {
            let params = plugin.params();
            for (key, value) in &values.params {
                match param_id(&params, key) {
                    Some(id) => {
                        plugin.set_param(id, value.clamp(0.0, 1.0));
                    }
                    None => {
                        warnings.push(format!("{}: パラメーター {key} が見つかりません", x.name))
                    }
                }
            }
        }
        self.senders.insert(x.id, plugin.param_sender());
        let part = plugin
            .audio()
            .map_err(|e| format!("{} を再生できません: {e}", x.name))?;
        // What the plugin said while it was being set up is not the user's.
        plugin.handler().take_gestures();
        plugin.handler().take_restart_flags();
        self.generation += 1;
        let titles = plugin
            .params()
            .into_iter()
            .map(|p| (p.id, p.title))
            .collect();
        let note_names = plugin.note_names();
        self.locals.insert(
            x.id,
            Local {
                restart_pending: 0,
                plugin: Some(OnThread::new(plugin)),
                owned: None,
                applied: x.clone(),
                generation: self.generation,
                titles,
                note_names,
                capture: None,
            },
        );
        self.fresh_locals.push(x.id);
        Ok((Box::new(Vst3Processor::new(part)), warnings))
    }

    /// The UI-thread side of a plugin that plays in this process, and which instance it is
    /// (it changes when a new mix opens the plugin again): its editor is the one whose edits
    /// are heard. `None` for a plugin elsewhere, or asked from another thread than the one
    /// that opened it.
    /// While a new mix that opened it again is on its way, the instance that plays is the old
    /// one (ENG-071).
    pub fn local(&self, id: PluginId) -> Option<(&xirius_vst3::Plugin, u64)> {
        let l = self.playing_local(id)?;
        Some((l.direct()?, l.generation))
    }

    /// Whether the build in progress, else the last, opened `id` as a VST3 plugin, here or in
    /// a host process.
    pub fn opened(&self, id: PluginId) -> bool {
        self.locals.contains_key(&id) || self.remotes.contains_key(&id)
    }

    /// The instance in a host process that plays `id`: the one the build in progress moves
    /// away from, else the last build's.
    fn playing_remote(&self, id: PluginId) -> Option<&Remote> {
        match self.leaving.get(&id) {
            Some(Leaving::Remote(r)) => Some(r),
            Some(Leaving::Local(..)) => None,
            None => self.remotes.get(&id),
        }
    }

    /// The instance in this process that plays `id`, as [`PluginLoader::playing_remote`].
    fn playing_local(&self, id: PluginId) -> Option<&Local> {
        match self.leaving.get(&id) {
            Some(Leaving::Local(l, _)) => Some(l),
            Some(Leaving::Remote(_)) => None,
            None => self.locals.get(&id),
        }
    }

    /// What the host processes said about the plugins of the last build once the mix was
    /// prepared: plugins they could not open (they play dry) and parameters they do not have.
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
            || !self.locals.is_empty()
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

    /// Reads the files `x` lists into a [`FileSet`] at the mix's rate, off the audio thread.
    /// A file that cannot be read is an empty slot and is named in `warnings`.
    fn load_set(
        &mut self,
        x: &Plugin,
        warnings: &mut Vec<String>,
    ) -> xirius_engine::stock::files::FileSet {
        use xirius_engine::stock::files::{FileSet, FileSlot};
        let mut slots = Vec::with_capacity(x.samples.len());
        for name in &x.samples {
            let path = self.resolve(name);
            let leaf = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.clone());
            match self.wavs.load(&path, self.rate) {
                Ok(data) => slots.push(Some(FileSlot { name: leaf, data })),
                Err(e) => {
                    warnings.push(e);
                    slots.push(None);
                }
            }
        }
        FileSet { slots }
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
            },
        );
        self.fresh_stock.push(x.id);
        Some((processor, warnings))
    }

    /// Whether the plugin runs as a built-in of the engine's.
    pub fn is_stock(&self, id: PluginId) -> bool {
        self.stock.contains_key(&id)
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
            // Only the values and the files move; another built-in or format takes a new
            // processor.
            let mut same = s.applied.clone();
            same.params.clone_from(&x.params);
            same.samples.clone_from(&x.samples);
            if same != *x {
                return false;
            }
            if s.applied.params != x.params {
                s.handle.set_all(&x.params);
            }
            if s.applied.samples == x.samples {
                s.applied = x.clone();
                return true;
            }
            // A new set of files reaches the running device without a new mix (ENG-077).
            let handle = s.handle.clone();
            let mut w = Vec::new();
            let set = self.load_set(x, &mut w);
            self.file_warnings
                .extend(w.into_iter().map(|e| format!("{}: {e}", x.name)));
            // The ring being full (the device is not polling) keeps `applied` as it was, so
            // the next `apply` sends the set again.
            if let Some(s) = self.stock.get_mut(&x.id)
                && handle.set_files(set)
            {
                s.applied = x.clone();
            }
            return true;
        }
        let applied = if let Some(l) = self.locals.get_mut(&x.id) {
            l.apply(x)
        } else if let Some(r) = self.remotes.get_mut(&x.id) {
            apply_remote(r, x)
        } else {
            false
        };
        // The instance that plays until the new mix does hears them too (ENG-071).
        if applied && let Some(l) = self.leaving.get_mut(&x.id) {
            let mut there = x.clone();
            match l {
                Leaving::Remote(r) => {
                    there.isolation = r.applied.isolation;
                    apply_remote(r, &there);
                }
                Leaving::Local(l, _) => {
                    there.isolation = l.applied.isolation;
                    l.apply(&there);
                }
            }
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
        match self.leaving.get(&id) {
            Some(Leaving::Remote(r)) => return r.server.automate(&r.link, key, value, 0),
            Some(Leaving::Local(_, sender)) => {
                return sender.as_ref().is_some_and(|s| s.set(key, value));
            }
            None => {}
        }
        if let Some(r) = self.remotes.get(&id) {
            return r.server.automate(&r.link, key, value, 0);
        }
        self.senders.get(&id).is_some_and(|s| s.set(key, value))
    }

    /// The values moved in the plugin's window up to `e`, when `e` brings its state: the
    /// song's values for the parameters it names follow them. The plugin sends its state a
    /// while after the moves (once it has played them), often in a later frame of the screen
    /// than the moves were taken in (ENG-071), so the moves of an edit without a state are
    /// kept for the one that brings it.
    pub fn moves_until(&mut self, e: &RemoteEdit) -> Option<Vec<ParamEdit>> {
        let moves = self.unsettled.entry(e.plugin).or_default();
        for p in e.params.iter().filter(|p| p.value.is_some()) {
            moves.retain(|m| m.id != p.id);
            moves.push(p.clone());
        }
        e.state.as_ref()?;
        self.unsettled.remove(&e.plugin)
    }

    /// Records that the song now holds `state` and `params` for a plugin because they were
    /// taken from the plugin itself, so they are not sent back to it.
    pub fn taken(&mut self, id: PluginId, state: Option<String>, params: &[(String, f64)]) {
        if let Some(r) = self.remotes.get_mut(&id) {
            if state.is_some() {
                r.applied.state.clone_from(&state);
            }
            for (k, v) in params {
                r.applied.params.insert(k.clone(), *v);
            }
        }
        if let Some(l) = self.locals.get_mut(&id) {
            if state.is_some() {
                l.applied.state.clone_from(&state);
            }
            for (k, v) in params {
                l.applied.params.insert(k.clone(), *v);
            }
        }
        let applied = match self.leaving.get_mut(&id) {
            Some(Leaving::Remote(r)) => &mut r.applied,
            Some(Leaving::Local(l, _)) => &mut l.applied,
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
    /// its patch in its own window, as Keyscape does, sends no edit, and the song would be
    /// saved with the state from when it was opened. The ones in host processes are asked
    /// and waited for, up to `wait` (the ones whose host went down or never opened are not).
    pub fn begin_pull_states(&mut self) -> StatePull {
        let mut owned = Vec::new();
        for l in self.locals.values_mut() {
            if let Some(local) = &l.owned {
                owned.push(local.capture(CAPTURE_WAIT));
            } else {
                l.capture = Some(Instant::now());
            }
        }
        let mut remotes: Vec<&Remote> = self.remotes.values().collect();
        for l in self.leaving.values_mut() {
            match l {
                Leaving::Remote(r) => remotes.push(r),
                Leaving::Local(l, _) => {
                    if let Some(local) = &l.owned {
                        owned.push(local.capture(CAPTURE_WAIT));
                    } else {
                        l.capture = Some(Instant::now());
                    }
                }
            }
        }
        let asked: Vec<_> = remotes
            .into_iter()
            .filter(|r| {
                let s = r.server.status(&r.link);
                !s.crashed && s.error.is_none()
            })
            .map(|r| {
                let seen = r.link.states_seen();
                r.server.request_state(&r.link);
                (r.link.clone(), seen)
            })
            .collect();
        let local = !self.locals.is_empty()
            || self
                .leaving
                .values()
                .any(|l| matches!(l, Leaving::Local(_, _)));
        StatePull {
            cancelled: std::cell::Cell::new(false),
            asked,
            owned,
            not_before: Instant::now() + if local { CAPTURE_WAIT } else { Duration::ZERO },
        }
    }

    pub fn pull_states(&mut self, wait: Duration) -> Result<(), String> {
        let asked = self.begin_pull_states();
        let end = Instant::now() + wait;
        while Instant::now() < end && !asked.ready() {
            std::thread::sleep(Duration::from_millis(2));
        }
        if !asked.ready() {
            asked.cancel();
            return Err("プラグインの状態を取得できません。未保存の変更が残っています".into());
        }
        if let Some(error) = asked.error() {
            asked.cancel();
            return Err(error);
        }
        Ok(())
    }

    /// Edits made in the plugins' editors since the last call.
    pub fn take_edits(&mut self) -> Vec<RemoteEdit> {
        let mut out = std::mem::take(&mut self.carried);
        for (id, l) in &mut self.locals {
            out.extend(l.take(*id, false));
        }
        let mut remotes: Vec<(&PluginId, &Remote)> = self.remotes.iter().collect();
        for (id, l) in &mut self.leaving {
            match l {
                Leaving::Remote(r) => remotes.push((id, r)),
                Leaving::Local(l, _) => out.extend(l.take(*id, false)),
            }
        }
        for (id, r) in remotes {
            let e = r.link.take_edits();
            if e.state.is_none() && e.params.is_empty() {
                continue;
            }
            out.push(RemoteEdit {
                plugin: *id,
                state: e.state.as_ref().map(xirius_plugins::encode_state),
                params: e.params,
            });
        }
        out.sort_by_key(|e| e.params.first().map(|p| p.at));
        out
    }

    /// Polls both VST3 controller threads and host links for latency changes (ENG-011).
    /// Other restart flags stay available to state/edit collection.
    pub fn take_latency_changes(&mut self) -> bool {
        let mut changed = false;
        for local in self.locals.values_mut() {
            if let Some(owned) = &local.owned {
                changed |= owned.take_latency();
            }
            if let Some(plugin) = local.plugin.as_ref().and_then(OnThread::get) {
                local.restart_pending |= plugin.handler().take_restart_flags();
                if local.restart_pending & LATENCY_CHANGED != 0 {
                    local.restart_pending &= !LATENCY_CHANGED;
                    changed = true;
                }
            }
        }
        for remote in self.remotes.values() {
            changed |= remote.link.take_latency_change().is_some();
        }
        for l in self.leaving.values_mut() {
            match l {
                Leaving::Remote(r) => changed |= r.link.take_latency_change().is_some(),
                Leaving::Local(local, _) => {
                    if let Some(owned) = &local.owned {
                        changed |= owned.take_latency();
                    }
                    if let Some(plugin) = local.plugin.as_ref().and_then(OnThread::get) {
                        local.restart_pending |= plugin.handler().take_restart_flags();
                        if local.restart_pending & LATENCY_CHANGED != 0 {
                            local.restart_pending &= !LATENCY_CHANGED;
                            changed = true;
                        }
                    }
                }
            }
        }
        changed
    }

    /// A plugin's parameter id → title, as the host reported when it opened.
    pub fn param_titles(&self, id: PluginId) -> Vec<(u32, String)> {
        if let Some(l) = self.playing_local(id) {
            return l.titles.clone();
        }
        self.playing_remote(id)
            .and_then(|r| r.link.opened())
            .map(|o| o.params)
            .unwrap_or_default()
    }

    /// The names a plugin gives its keys, by key (EDT-134): empty when it names none, or is
    /// not a VST3 or CLAP plugin of the current mix.
    pub fn note_names(&self, id: PluginId) -> Vec<(u8, String)> {
        if let Some(l) = self.playing_local(id) {
            return l.note_names.clone();
        }
        if let Some(names) = self.clap_names.get(&id) {
            return names.clone();
        }
        self.playing_remote(id)
            .map(|r| r.link.note_names())
            .unwrap_or_default()
    }

    /// The plugins in host processes and how they are.
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
        let mut locals: Vec<_> = self
            .locals
            .keys()
            .chain(self.leaving.keys())
            .copied()
            .collect();
        locals.sort();
        locals.dedup();
        for id in locals {
            if let Some(local) = self.playing_local(id)
                && let Some(owned) = &local.owned
            {
                out.push(PluginReport {
                    id,
                    name: local.applied.name.clone(),
                    placement: PluginIsolation::InProcess,
                    requested: local.applied.isolation,
                    status: PluginStatus {
                        error: owned.error(),
                        editor_open: owned.visible(),
                        latency: owned.latency.get(),
                        ..Default::default()
                    },
                });
            }
        }
        out.sort_by_key(|r| r.id);
        out
    }

    /// The hosts in use: the shared one first, then the dedicated ones by plugin.
    fn servers(&self) -> Vec<&PluginServer> {
        let mut dedicated: Vec<_> = self.dedicated.iter().collect();
        dedicated.sort_by_key(|(id, _)| **id);
        self.shared
            .iter()
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
        if let Some(local) = self.playing_local(id)
            && let Some(owned) = &local.owned
        {
            return if owned.editor(local.applied.name.clone(), caption, open) {
                Ok(())
            } else {
                Err(owned
                    .error()
                    .unwrap_or_else(|| "プラグインが応答しません".into()))
            };
        }
        // While a new mix that moves it is on its way, the window is the playing instance's,
        // and the new one's opens when it plays (ENG-071).
        let moving = self.leaving.contains_key(&id);
        let (Some(r), new) = (self.playing_remote(id), self.remotes.get(&id)) else {
            return Err("そのプラグインは別のプロセスで実行されていません".into());
        };
        let title = |r: &Remote| format!("{} · {}", r.applied.name, r.owner);
        if open {
            r.server.open_editor(&r.link, &title(r), caption);
        } else {
            r.server.close_editor(&r.link);
        }
        if let Some(new) = new.filter(|_| moving) {
            if open {
                new.server
                    .open_editor_later(&new.link, &title(new), caption);
            } else {
                new.server.close_editor(&new.link);
            }
        }
        Ok(())
    }

    /// Gives the open window of `id` new caption colours.
    pub fn set_caption(&self, id: PluginId, caption: xirius_vst3::Caption) {
        if let Some(local) = self.playing_local(id)
            && let Some(owned) = &local.owned
        {
            owned.caption(caption);
        }
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

    /// Space forwarded from editors owned by local controller threads.
    pub fn take_local_play_requests(&self) -> u32 {
        self.locals
            .keys()
            .chain(self.leaving.keys())
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .filter_map(|id| self.playing_local(id)?.owned.as_ref())
            .fold(0u32, |count, local| {
                count.saturating_add(local.take_play_requests())
            })
    }

    /// The host the plugins share, for tests that make it fail.
    pub fn server(&self) -> Option<&PluginServer> {
        self.shared.as_ref()
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

    /// The link of the instance in a host process that plays `id`: while a new mix that moves
    /// it is on its way, the one it moves away from (ENG-071).
    pub fn playing_link(&self, id: PluginId) -> Option<Arc<PluginLink>> {
        self.playing_remote(id).map(|r| r.link.clone())
    }

    /// The mix of the last build has been swapped in: the instances it opened again in their
    /// hosts are the ones that play, with the windows and what they report (ENG-066).
    /// A plugin moved to another place has its window there once the new mix plays, if its
    /// window was still open on the instance that played until then (ENG-071): one moved into
    /// this process has it opened by the screen ([`PluginLoader::window_moved_in`]) once the
    /// old host has let go ([`PluginLoader::settle_following`]).
    pub fn promote_opened(&mut self) {
        self.moved_in.clear();
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
                        id: *id,
                        old_link: old.link.clone(),
                        old_server: old.server.clone(),
                        shut,
                        to: Some((r.server.clone(), r.link.clone())),
                        until,
                    });
                }
            }
            r.server.promote(&r.link);
        }
        for (id, l) in std::mem::take(&mut self.leaving) {
            let Leaving::Remote(old) = l else { continue };
            // Before the window is looked at, as above.
            let shut = old.link.windows_shut();
            if !self.remotes.contains_key(&id) && old.link.editor().is_some() {
                old.server.close_editor(&old.link);
                following.push(Following {
                    id,
                    old_link: old.link,
                    old_server: old.server,
                    shut,
                    to: None,
                    until,
                });
            }
        }
        self.following = following;
    }

    /// The mix of the last build is rendered offline: it plays as soon as it is built, so the
    /// instances it took over from are let go of now (ENG-071). A builder used for several
    /// renders, as for the stems, would keep the first render's instances otherwise.
    pub fn built_offline(&mut self) {
        self.leaving.clear();
    }

    /// Whether the mix that plays moved `id` into this process while its window was open in
    /// its host (ENG-071): the screen opens the window here. A window closed while the new mix
    /// was on its way does not open.
    pub fn window_moved_in(&self, id: PluginId) -> bool {
        self.moved_in.contains(&id) && self.locals.contains_key(&id)
    }

    /// Whether the mix that plays moved `id` into this process with its host's window open,
    /// and that host has not let go of it yet (ENG-071): until it has, the window may yet have
    /// been shut there, and [`PluginLoader::window_moved_in`] waits.
    pub fn window_moving_in(&self, id: PluginId) -> bool {
        self.locals.contains_key(&id) && self.following.iter().any(|f| f.id == id && f.to.is_none())
    }

    /// Settles the windows that moved with a plugin when the last mix played, as the hosts of
    /// the instances that played let go of them (ENG-071). One whose old window the host says
    /// the person shut is shut on the instance that plays now, or does not open in this
    /// process: a window closed while the new mix was on its way stays closed.
    pub fn settle_following(&mut self) {
        let now = Instant::now();
        let mut moved_in = Vec::new();
        self.following.retain(|f| {
            // Whether the host let go first: what it said before that is in by then.
            let let_go = f.old_server.has_let_go(&f.old_link) || now >= f.until;
            if f.old_link.windows_shut() != f.shut {
                if let Some((server, link)) = &f.to
                    && link.editor().is_some()
                {
                    server.close_editor(link);
                }
                return false;
            }
            if let_go && f.to.is_none() {
                moved_in.push(f.id);
            }
            !let_go
        });
        self.moved_in.extend(moved_in);
    }

    /// Called when a build ends: forgets the plugins it did not open, and the hosts of their
    /// own that nothing runs in any more (they end once the old mix lets go of them).
    pub fn end_build(&mut self) {
        let fresh = std::mem::take(&mut self.fresh);
        self.remotes.retain(|id, _| fresh.contains(id));
        let fresh_stock = &self.fresh_stock;
        self.stock.retain(|id, _| fresh_stock.contains(id));
        let fresh_locals = &self.fresh_locals;
        self.locals.retain(|id, _| fresh_locals.contains(id));
        let fresh_clap = &self.fresh_clap;
        self.clap_names.retain(|id, _| fresh_clap.contains(id));
        // Free the file sets the audio thread handed back (ENG-077).
        for s in self.stock.values() {
            s.handle.reclaim_files();
        }
        self.ready.clear();
        let fresh_locals = &self.fresh_locals;
        self.took.retain(|id, _| {
            fresh.contains(id) || fresh_locals.contains(id) || fresh_clap.contains(id)
        });
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

    /// A set that cannot go into a full ring is not marked applied, so a later `apply` sends
    /// it again once the device polls (ENG-077).
    #[test]
    fn a_full_ring_leaves_the_new_files_pending_until_the_device_polls() {
        use xirius_engine::stock::files::FileSet;
        use xirius_engine::{AudioIo, RenderMode};
        let tmp = tempfile::Builder::new()
            .prefix("xirius-plugin-samples-")
            .tempdir()
            .unwrap();
        let mk = |samples: Vec<String>| Plugin {
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
            if !h.set_files(FileSet::default()) {
                break;
            }
        }
        // The next set does not fit: the change is not marked applied.
        let x2 = mk(vec!["b.wav".into()]);
        assert!(loader.apply(&x2));
        assert_eq!(loader.stock[&x.id].applied.samples, vec!["a.wav"]);
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
            },
            &[],
        );
        assert!(loader.apply(&x2));
        assert_eq!(loader.stock[&x.id].applied.samples, vec!["b.wav"]);
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
