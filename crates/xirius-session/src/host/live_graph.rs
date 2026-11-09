//! A live plan is prepared beside the running graph. SDK ownership and the
//! output are retained; only a ready plan crosses the command queue.
use super::*;
use crate::background::Job;

pub(super) struct Update {
    source: Project,
    target: Project,
    latency: bool,
    work: Job<Outcome>,
    ready: Option<Candidate>,
}
#[cfg(test)]
impl Update {
    pub(super) fn work_ready(&self) -> bool {
        self.work.ready()
    }
}
enum Outcome {
    Ready(Box<Candidate>),
    Failed { error: String, state: bool },
}
struct Candidate {
    builder: Builder,
    built: crate::build::Built,
    plan: Option<Box<Plan>>,
    kept_plugins: std::collections::BTreeSet<PluginId>,
}

impl SessionHost {
    pub(super) fn step_live_graph(&mut self) -> Option<SyncReport> {
        if self.graph_work.is_none() {
            let pending = self.pending.take()?;
            let mut warnings = pending.warnings;
            if self.failed_graph_retry().is_some() {
                self.graph_latency_pending |= pending.latency;
                return (!warnings.is_empty()).then_some(SyncReport {
                    warnings,
                    ..Default::default()
                });
            }
            let source = self.song.clone();
            let latency = std::mem::take(&mut self.graph_latency_pending) || pending.latency;
            let target = pending.target;
            let make = match self.builder.live_copy() {
                Ok(make) => make,
                Err(error) => {
                    self.graph_latency_pending |= latency;
                    self.graph_retry = None;
                    self.graph_failed = Some(GraphRetry(Arc::new(GraphFailure {
                        target: source,
                        error: error.clone(),
                    })));
                    warnings.push(error);
                    return Some(SyncReport {
                        warnings,
                        ..Default::default()
                    });
                }
            };
            let shape = self.controller.shape();
            let old_graph = self.graph.clone();
            let old_structure = self.structure.clone();
            let old_timelines = self.tracks.clone();
            let snapshot = target.clone();
            let work = Job::spawn("live-graph", move || {
                let mut builder = make();
                let graph = Graph::of(&snapshot);
                let scope = Scope::new(&snapshot, Select::All);
                let structure = Structure::of(&snapshot, &graph, &scope);
                // Existing unkeyed tracks keep their processors while bus indices
                // remain compatible. Keys use node numbers and require rebuilding.
                let keep: BTreeMap<_, _> = structure
                    .tracks
                    .iter()
                    .filter_map(|node| {
                        let old = old_structure.tracks.iter().position(|old| old == node)?;
                        (!latency
                            && old_graph.buses == graph.buses
                            && old_timelines.get(&node.id).is_some_and(|old| {
                                old.matches(&snapshot, &snapshot.tracks[&node.id])
                            })
                            && node.inserts.iter().all(|x| x.sidechain.is_none()))
                        .then_some((node.id, old))
                    })
                    .collect();
                let kept_plugins: std::collections::BTreeSet<_> = snapshot
                    .tracks
                    .iter()
                    .filter(|(id, _)| keep.contains_key(id))
                    .flat_map(|(_, t)| t.inserts.iter().map(|x| x.id))
                    .collect();
                if let Err(error) = builder
                    .plugins_mut()
                    .capture_for_live_build(&snapshot, &kept_plugins)
                {
                    return Ok(Outcome::Failed { error, state: true });
                }
                let setup = shape.setup;
                for (owner, plugin) in crate::starting::hosted(&snapshot) {
                    if !kept_plugins.contains(&plugin.id) {
                        builder.plugins_mut().preload(&owner, &plugin, &setup);
                    }
                }
                loop {
                    builder.plugins_mut().poll_preloads();
                    if !crate::starting::hosted(&snapshot)
                        .iter()
                        .any(|(_, plugin)| {
                            !kept_plugins.contains(&plugin.id)
                                && builder.plugins().preparing(plugin.id)
                        })
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                builder.keep_live_tracks(keep);
                let mut built = builder.build(&snapshot, Select::All);
                built.warnings.extend(builder.plugins().opening_warnings());
                if !built.warnings.is_empty() {
                    return Ok(Outcome::Failed {
                        error: built.warnings.join("。"),
                        state: false,
                    });
                }
                // The spec is consumed on the worker. No prepare/prime/SDK or
                // buffer construction can run during publication on the window.
                let spec = std::mem::take(&mut built.spec);
                match Plan::build(spec, setup, Some(shape)) {
                    Ok(plan) => Ok(Outcome::Ready(Box::new(Candidate {
                        builder,
                        built,
                        plan: Some(Box::new(plan)),
                        kept_plugins,
                    }))),
                    Err(error) => Ok(Outcome::Failed {
                        error: error.to_string(),
                        state: false,
                    }),
                }
            });
            match work {
                Ok(work) => {
                    self.graph_work = Some(Update {
                        source,
                        target,
                        latency,
                        work,
                        ready: None,
                    })
                }
                Err(error) => {
                    self.graph_latency_pending |= latency;
                    self.graph_retry = None;
                    self.graph_failed = Some(GraphRetry(Arc::new(GraphFailure {
                        target: self.song.clone(),
                        error: error.clone(),
                    })));
                    warnings.push(error);
                    return Some(SyncReport {
                        warnings,
                        ..Default::default()
                    });
                }
            }
            // Publication is a later poll, even for an empty/instant plan.
            // This gives queued latency/model changes a defined handoff point.
            return (!warnings.is_empty()).then_some(SyncReport {
                warnings,
                ..Default::default()
            });
        }
        let mut update = self.graph_work.take().unwrap();
        if update.source != self.song {
            if !update.work.ready() && update.ready.is_none() {
                self.graph_work = Some(update);
                return None;
            }
            let latency = update.latency;
            // A claimed candidate (queue pressure) is also destroyed off UI.
            let _ = Job::spawn("stale-graph", move || {
                drop(update);
                Ok(())
            });
            if self.pending.is_none() {
                self.begin_swap(self.song.clone(), SwapKind::Sync, Vec::new());
            }
            // Retargeting an editor edit must not turn a latency rebuild into
            // a normal update that retains the old processors and compensation.
            if let Some(pending) = &mut self.pending {
                pending.latency |= latency;
            }
            return None;
        }
        if update.ready.is_none() {
            let Some(result) = update.work.poll() else {
                self.graph_work = Some(update);
                return None;
            };
            match result {
                Ok(Outcome::Ready(candidate)) => update.ready = Some(*candidate),
                Ok(Outcome::Failed { error, state }) => {
                    self.graph_latency_pending |=
                        update.latency || self.pending.as_ref().is_some_and(|p| p.latency);
                    self.state_transfer_failed |= state;
                    self.graph_retry = None;
                    self.graph_failed = Some(GraphRetry(Arc::new(GraphFailure {
                        target: update.source,
                        error: error.clone(),
                    })));
                    self.pending = None;
                    return Some(SyncReport {
                        warnings: vec![error],
                        ..Default::default()
                    });
                }
                Err(error) => {
                    self.graph_latency_pending |=
                        update.latency || self.pending.as_ref().is_some_and(|p| p.latency);
                    self.graph_retry = None;
                    self.graph_failed = Some(GraphRetry(Arc::new(GraphFailure {
                        target: update.source,
                        error: error.clone(),
                    })));
                    self.pending = None;
                    return Some(SyncReport {
                        warnings: vec![error],
                        ..Default::default()
                    });
                }
            }
        }
        // Never replace the engine's recording graph or an unfinished capture.
        if self.recording.is_some() {
            self.graph_work = Some(update);
            return None;
        }
        let candidate = update.ready.as_mut().unwrap();
        let plan = candidate.plan.take().unwrap();
        if let Err(Command::SwapPlan(plan)) = self.controller.send(Command::SwapPlan(plan)) {
            candidate.plan = Some(plan);
            self.graph_work = Some(update);
            return None;
        }
        let Candidate {
            builder,
            built,
            kept_plugins,
            ..
        } = update.ready.take().unwrap();
        let visible: Vec<_> = self
            .builder
            .plugins()
            .statuses()
            .into_iter()
            .filter_map(|(id, _, status)| status.editor_open.then_some(id))
            .collect();
        let mut old = std::mem::replace(&mut self.builder, builder);
        let carried = old.plugins_mut().take_carried_for(&kept_plugins);
        self.builder.plugins_mut().carry(carried);
        self.builder.plugins_mut().promote_opened();
        for id in visible {
            if !self
                .builder
                .plugins()
                .statuses()
                .iter()
                .any(|(pid, _, s)| *pid == id && s.editor_open)
            {
                let _ = self.builder.plugins().editor(id, true, None);
            }
        }
        let old_material = self.material.take();
        let _ = Job::spawn("live-plan-retire", move || {
            drop(old);
            drop(old_material);
            Ok(())
        });
        self.graph = built.graph;
        self.structure = built.structure;
        self.mix = built.mix;
        self.tracks = TimelineState::all(&update.target);
        self.tempo = (update.target.ppq, update.target.tempo.clone());
        self.tmap = TempoMap::from_project(&update.target);
        self.end_frame = built.end_frame;
        self.live = update
            .target
            .tracks
            .iter()
            .map(|(id, t)| (*id, automation::realtime(t)))
            .collect();
        self.swapping = Some(Instant::now());
        self.levels.clear();
        let late_latency = self.pending.take().is_some_and(|pending| pending.latency);
        self.latency_dirty |= late_latency;
        self.graph_failed = None;
        if let Some(retry) = &mut self.graph_retry {
            retry.applied = retry.target == update.source;
        }
        self.params.clear();
        let p = self.song.clone();
        self.send_transport(&p);
        self.follow_song(&p);
        self.sync_thru(&p, &mut SyncReport::default());
        self.sync_midi_out(&mut SyncReport::default());
        Some(SyncReport {
            swapped: true,
            ..Default::default()
        })
    }
}
