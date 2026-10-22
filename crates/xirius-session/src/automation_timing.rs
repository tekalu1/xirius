use crate::{graph::Graph, plugins::PluginLoader, tempo::TempoMap};
use std::collections::HashMap;
use xirius_core::{
    auto::AutoTarget,
    id::{PluginId, TrackId},
    model::{PluginKind, Project},
};
use xirius_engine::mixer::PlanShape;

/// Every stage evaluates the same song clock, less the audio delay before that stage.
#[derive(Default)]
pub struct Timing {
    start: u64,
    channels: HashMap<TrackId, u32>,
    plugins: HashMap<PluginId, u32>,
}
impl Timing {
    pub fn with_start(mut self, start: u64) -> Self {
        self.start = start;
        self
    }
    pub fn new(p: &Project, graph: &Graph, shape: &PlanShape, loader: &PluginLoader) -> Self {
        let mut timing = Self::default();
        for (ids, chains) in [
            (&graph.tracks, &shape.automation.tracks),
            (&graph.buses, &shape.automation.buses),
        ] {
            for (id, chain) in ids.iter().zip(chains) {
                timing.channels.insert(*id, chain.fader());
                let Some(t) = p.tracks.get(id) else { continue };
                for source in t
                    .inserts
                    .iter()
                    .filter(|p| p.kind == PluginKind::Instrument)
                {
                    timing.plugins.insert(source.id, 0);
                }
                let retained = t.inserts.iter().filter(|p| {
                    p.kind == PluginKind::Effect
                        && p.runs()
                        && ((p.format == "builtin" && p.uid.eq_ignore_ascii_case("gain"))
                            || loader.is_stock(p.id)
                            || loader.opened(p.id))
                });
                for (index, plugin) in retained.enumerate() {
                    timing.plugins.insert(plugin.id, chain.insert(index));
                }
            }
        }
        timing
    }
    pub fn tick(
        &self,
        id: TrackId,
        target: &AutoTarget,
        frame: u64,
        map: &TempoMap,
        rate: f64,
    ) -> f64 {
        let delay = match target {
            AutoTarget::Param(pid, _) => self.plugins.get(pid),
            _ => self.channels.get(&id),
        }
        .copied()
        .unwrap_or(0);
        map.tick_of_frame(frame.saturating_sub(u64::from(delay)).max(self.start), rate) as f64
    }
    pub fn song_at(&self, p: &Project, frame: u64, map: &TempoMap, rate: f64) -> Project {
        crate::automation::at_each(p, |id, target| self.tick(id, target, frame, map, rate))
    }
}
