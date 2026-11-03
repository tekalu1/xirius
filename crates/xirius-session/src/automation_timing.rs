use crate::{
    graph::Graph,
    plugins::PluginLoader,
    rack_row::{Slot, layout},
    tempo::TempoMap,
};
use std::collections::HashMap;
use xirius_core::{
    auto::AutoTarget,
    id::{PluginId, TrackId},
    model::{PluginKind, Project},
};
use xirius_engine::automation_time::Chain;
use xirius_engine::mixer::PlanShape;
use xirius_engine::split::Inner;

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
    pub fn new<'a>(
        p: &'a Project,
        graph: &'a Graph,
        shape: &'a PlanShape,
        loader: &PluginLoader,
    ) -> Self {
        let mut timing = Self::default();
        let nodes = |ids: &'a [TrackId], chains: &'a [Chain], nested: &'a [Vec<Inner>]| {
            ids.iter()
                .zip(chains)
                .enumerate()
                .map(move |(i, (id, chain))| {
                    (id, chain, nested.get(i).map_or(&[][..], Vec::as_slice))
                })
        };
        // The master is as late as the mix reaching it (AUT-019).
        let master = (
            &TrackId::MASTER,
            &shape.automation.master,
            shape.nested.master.as_slice(),
        );
        let all = nodes(
            &graph.tracks,
            &shape.automation.tracks,
            &shape.nested.tracks,
        )
        .chain(nodes(
            &graph.buses,
            &shape.automation.buses,
            &shape.nested.buses,
        ))
        .chain(std::iter::once(master));
        for (id, chain, nested) in all {
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
            let Some(rack) = &t.rack else {
                for (index, plugin) in retained.enumerate() {
                    timing.plugins.insert(plugin.id, chain.insert(index));
                }
                continue;
            };
            // A splitter is one insert of the row; the plug-ins in its branches are as late
            // as the engine says (ENG-074).
            let built: Vec<u64> = retained.map(|p| p.id.0).collect();
            for (index, slot) in layout(rack, &built).iter().enumerate() {
                if let Slot::Plugin(id) = slot {
                    timing.plugins.insert(PluginId(*id), chain.insert(index));
                }
            }
            for inner in nested {
                timing.plugins.insert(
                    PluginId(inner.id),
                    chain.insert(inner.insert) + inner.offset,
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use xirius_core::Session;
    use xirius_core::controls::ControlId;
    use xirius_core::session::RunOptions;

    /// A macro control's lane moves a plug-in parameter at the plug-in's time, not the
    /// fader's: the parameter sits before the delay the fader waits for (AUT-019, MIX-039).
    #[test]
    fn a_macro_lane_moves_a_parameter_at_the_plug_ins_time() {
        let mut s = Session::new("timing");
        let run = |s: &mut Session, name: &str, args: serde_json::Value| {
            s.run_value(name, args.clone(), &RunOptions::human())
                .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
        };
        let t = run(&mut s, "track.add.audio", json!({ "name": "Vocal" })).data["id"]
            .as_str()
            .unwrap()
            .to_string();
        let eq = run(&mut s, "plugin.add", json!({ "track": t, "name": "EQ" })).data["id"]
            .as_str()
            .unwrap()
            .to_string();
        run(
            &mut s,
            "control.assign",
            json!({ "track": t, "control": "k1", "target": format!("param:{eq}:gain") }),
        );
        for (at, value) in [(0, 0.0), (7680, 1.0)] {
            run(
                &mut s,
                "auto.point.add",
                json!({ "track": t, "target": "control:k1", "at": at, "value": value }),
            );
        }
        let p = s.project();
        let tid: TrackId = t.parse().unwrap();
        let pid: PluginId = eq.parse().unwrap();
        let rate = 48_000.0;
        let map = TempoMap::from_project(p);
        // the fader waits a second for a look-ahead after the EQ; the EQ itself does not
        let timing = Timing {
            start: 0,
            channels: HashMap::from([(tid, 48_000)]),
            plugins: HashMap::from([(pid, 0)]),
        };
        let frame = 96_000;
        let lane = |tick: f64| {
            p.tracks[&tid]
                .automation
                .iter()
                .find(|l| l.target == AutoTarget::Control(ControlId::Knob(0)))
                .unwrap()
                .value_at(tick)
                .unwrap()
        };
        let now = lane(map.tick_of_frame(frame, rate) as f64);
        let late = lane(map.tick_of_frame(frame - 48_000, rate) as f64);
        assert!((now - late).abs() > 0.05, "{now} {late}");
        let k1 = p.tracks[&tid].controls.get(ControlId::Knob(0));
        let want = k1.maps[0].value(now);

        let song = timing.song_at(p, frame, &map, rate);
        let t = &song.tracks[&tid];
        let gain = t.inserts[0].params.get("gain").copied().unwrap();
        assert!((gain - want).abs() < 1e-9, "{gain} vs {want}");
        assert!((t.controls.get(ControlId::Knob(0)).value - late).abs() < 1e-9);

        let mut live = Vec::new();
        crate::automation::lane_params(
            &p.tracks[&tid],
            |target| timing.tick(tid, target, frame, &map, rate),
            &mut live,
        );
        assert_eq!(live.len(), 1, "{live:?}");
        assert!((live[0].2 - want).abs() < 1e-9, "{live:?} vs {want}");
    }
}
