//! Starting a song's player a little at a time (ENG-064), so that its window keeps drawing and
//! shows how far the opening of its plugins has come.
//!
//! [`Starting::step`] first asks every plugin that runs in a host process to open there; those
//! open at once, each in a host of its own or one after another in a shared one, and are counted
//! as their answers come. Then each step opens one plugin that runs in this process. The
//! plugins are opened as [`Builder::build`] would open them, so [`Starting::finish`] builds the
//! mix with them as they are and opens what is left on the spot. [`SessionHost::start`] is the
//! same, all at once.
//!
//! A plugin that cannot be opened says so in the warnings, as when it is opened on the spot,
//! and the song plays with what stands in for it.

use std::time::{Duration, Instant};

use xirius_core::model::{Plugin, PluginKind, Project, TrackKind};
use xirius_engine::mixer::PlanError;
use xirius_engine::{AudioEngine, EngineConfig, ProcessSetup, RenderMode};

use crate::build::{Builder, setup};
use crate::clap_plugins::is_hosted;
use crate::graph::{DISCARD, Graph, Scope, Select, Structure};
use crate::host::{HostConfig, SessionHost};

/// A track, a bus or the master, and the plugins on it that are opened.
struct Unit {
    name: String,
    plugins: Vec<Plugin>,
}

/// How far a track's plugins are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitState {
    Done,
    Working,
    Waiting,
}

/// What the screen shows of a start under way: each track (and bus) with plugins to open,
/// in the order of the song, and how many are done.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub units: Vec<(String, UnitState)>,
    pub done: usize,
}

pub struct Starting {
    builder: Builder,
    engine: EngineConfig,
    setup: ProcessSetup,
    units: Vec<Unit>,
    /// The plugins that run in a host process were asked for.
    asked: bool,
    processed_seen: u64,
    began: Instant,
}

impl Starting {
    /// Gets ready to start `p`'s player; nothing is opened yet.
    pub fn new(p: &Project, config: HostConfig) -> Starting {
        let rate = config.rate.unwrap_or(p.settings.sample_rate);
        let processed_seen = crate::processed::generation();
        let mut builder = Builder::new(rate, config.dir);
        builder.set_background(true);
        builder.set_threaded_local(config.threaded_local);
        builder.set_catalog(config.catalog);
        if let Some(i) = config.isolation {
            builder.set_isolation(i);
        }
        Starting {
            builder,
            engine: config.engine,
            setup: setup(rate, RenderMode::Realtime),
            units: units(p),
            asked: false,
            processed_seen,
            began: Instant::now(),
        }
    }

    /// Opens a little more: the first step asks for every plugin that runs in a host
    /// process, each later one opens one plugin here. Whether everything is open.
    pub fn step(&mut self) -> bool {
        self.builder.plugins_mut().poll_preloads();
        if !self.asked {
            self.asked = true;
            self.ask_apart();
            return self.ready();
        }
        let next = self
            .units
            .iter()
            .flat_map(|u| u.plugins.iter().map(move |x| (u.name.as_str(), x)))
            .find(|(_, x)| !self.builder.plugins().preloaded(x.id));
        if let Some((owner, x)) = next {
            let (owner, x) = (owner.to_string(), x.clone());
            self.builder.plugins_mut().preload(&owner, &x, &self.setup);
        }
        self.ready()
    }

    fn ask_apart(&mut self) {
        let setup = self.setup;
        for u in &self.units {
            for x in &u.plugins {
                let loader = self.builder.plugins_mut();
                if loader.runs_apart(x) {
                    loader.preload(&u.name, x, &setup);
                }
            }
        }
    }

    /// Every plugin is open, and every answer from a host process has come.
    pub fn ready(&self) -> bool {
        self.asked && self.units.iter().all(|u| self.unit(u) == UnitState::Done)
    }

    fn unit(&self, u: &Unit) -> UnitState {
        let loader = self.builder.plugins();
        let opened = u.plugins.iter().filter(|x| loader.preloaded(x.id)).count();
        let busy = u.plugins.iter().any(|x| loader.preparing(x.id));
        if opened == u.plugins.len() && !busy {
            UnitState::Done
        } else if opened > 0 || busy {
            UnitState::Working
        } else {
            UnitState::Waiting
        }
    }

    pub fn progress(&self) -> Progress {
        let mut units: Vec<(String, UnitState)> = self
            .units
            .iter()
            .map(|u| (u.name.clone(), self.unit(u)))
            .collect();
        // The one the next step opens is under way too.
        if self.asked
            && let Some(next) = units.iter_mut().find(|(_, s)| *s != UnitState::Done)
            && next.1 == UnitState::Waiting
        {
            next.1 = UnitState::Working;
        }
        let done = units.iter().filter(|(_, s)| *s == UnitState::Done).count();
        Progress { units, done }
    }

    /// No plugin to open: the player starts at once.
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// How long since [`Starting::new`].
    pub fn elapsed(&self) -> Duration {
        self.began.elapsed()
    }

    /// Builds the player for `p` with what is open, opening the rest now. The plugins that run
    /// in a host process are asked for together first.
    pub fn finish(self, p: &Project) -> Result<(SessionHost, AudioEngine, Vec<String>), PlanError> {
        self.finish_with(p, false)
    }

    /// The initial worker waits for decode/conversion/prime and builds a complete
    /// realtime plan. Later edits may render transforms in the background again.
    pub fn finish_prepared(
        mut self,
        p: &Project,
        track: impl FnMut(xirius_core::id::TrackId, bool),
    ) -> Result<
        (
            SessionHost,
            AudioEngine,
            Vec<crate::build::PreparationFailure>,
        ),
        String,
    > {
        self.builder.prepare_strictly();
        self.builder.set_background(false);
        self.builder.plugins_mut().set_setup(self.setup);
        if !self.asked {
            self.asked = true;
            self.ask_apart();
        }
        let built = self.builder.build_observed(p, Select::All, track);
        let failures = built.failures.clone();
        if let Some(failure) = failures.iter().find(|f| f.track.is_none()) {
            return Err(format!("{}: {}", failure.name, failure.reason));
        }
        self.builder.set_background(true);
        #[cfg(feature = "test-io")]
        xirius_core::io_test::before(
            "prepare_graph",
            self.builder
                .dir()
                .unwrap_or_else(|| std::path::Path::new("")),
        )
        .map_err(|e| e.to_string())?;
        SessionHost::assemble(
            p,
            self.builder,
            built,
            self.setup,
            self.engine,
            self.processed_seen,
        )
        .map(|(host, audio, _)| (host, audio, failures))
        .map_err(|e| e.to_string())
    }

    pub fn owner_retirements(&self) -> Vec<xirius_vst3::owner::Retirement> {
        self.builder.plugins().owner_retirements()
    }

    /// As [`Starting::finish`], without waiting for the plugins that run in a host process and
    /// have not answered yet (ENG-065): the mix is built at once, each of them plays dry until
    /// it is open and sounds from that moment. What a host says about them comes later from
    /// [`SessionHost::take_late_warnings`]. For a window; offline work waits.
    pub fn finish_late(
        self,
        p: &Project,
    ) -> Result<(SessionHost, AudioEngine, Vec<String>), PlanError> {
        self.finish_with(p, true)
    }

    fn finish_with(
        mut self,
        p: &Project,
        late: bool,
    ) -> Result<(SessionHost, AudioEngine, Vec<String>), PlanError> {
        if !self.asked {
            self.asked = true;
            self.ask_apart();
        }
        if late {
            self.builder.plugins_mut().allow_late();
        }
        let built = self.builder.build(p, Select::All);
        SessionHost::assemble(
            p,
            self.builder,
            built,
            self.setup,
            self.engine,
            self.processed_seen,
        )
    }
}

/// The plugins [`Builder::build`] opens for `p` and the track, bus or master each is on, in its
/// order.
pub(crate) fn hosted(p: &Project) -> Vec<(String, Plugin)> {
    units(p)
        .into_iter()
        .flat_map(|u| {
            let name = u.name;
            u.plugins.into_iter().map(move |x| (name.clone(), x))
        })
        .collect()
}

/// The tracks, buses and master whose plugins [`Builder::build`] opens, in its order.
fn units(p: &Project) -> Vec<Unit> {
    let graph = Graph::of(p);
    let scope = Scope::new(p, Select::All);
    let structure = Structure::of(p, &graph, &scope);
    let hosted = |x: &&Plugin| x.runs() && is_hosted(x);
    let effects = |inserts: &[Plugin]| -> Vec<Plugin> {
        inserts
            .iter()
            .filter(|x| x.kind == PluginKind::Effect)
            .filter(hosted)
            .cloned()
            .collect()
    };
    let mut out = Vec::new();
    for (i, id) in graph.tracks.iter().enumerate() {
        let t = &p.tracks[id];
        let shape = &structure.tracks[i];
        let mut plugins = Vec::new();
        if t.kind == TrackKind::Instrument && t.frozen.is_none() && shape.midi.is_none() {
            plugins.extend(
                t.inserts
                    .iter()
                    .find(|x| x.kind == PluginKind::Instrument)
                    .filter(hosted)
                    .cloned(),
            );
        }
        plugins.extend(effects(&shape.inserts));
        out.push(Unit {
            name: t.name.clone(),
            plugins,
        });
    }
    for shape in &structure.buses {
        let name = if shape.id == DISCARD {
            "discard".to_string()
        } else {
            p.tracks[&shape.id].name.clone()
        };
        out.push(Unit {
            name,
            plugins: effects(&shape.inserts),
        });
    }
    out.push(Unit {
        name: "master".to_string(),
        plugins: effects(&structure.master_inserts),
    });
    out.retain(|u| !u.plugins.is_empty());
    out
}
