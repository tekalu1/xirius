//! Live timeline replacement. The initial song preparation card owns no part of
//! this path: existing processors, editors, playback and recording stay alive.
use super::*;
use crate::background::Job;
use std::collections::VecDeque;
use xirius_engine::Timeline;

pub(super) struct Update {
    target: Project,
    generation: u64,
    work: Job<Prepared>,
    ready: Option<Prepared>,
}

struct Prepared {
    timelines: VecDeque<(usize, TrackId, Timeline)>,
    warnings: Vec<String>,
}

fn matches(a: &Project, b: &Project) -> bool {
    a.ppq == b.ppq
        && a.tempo == b.tempo
        && Graph::of(a) == Graph::of(b)
        && a.tracks.iter().all(|(id, t)| {
            b.tracks
                .get(id)
                .is_some_and(|now| TimelineState::new(a, t).matches(b, now))
        })
}

impl SessionHost {
    pub(super) fn begin_material(&mut self, p: &Project, report: &mut SyncReport) {
        if self.material.is_some() || Graph::of(p) != self.graph {
            return; // Coalesce edits while a single worker owns the old request.
        }
        let tempo = self.tempo.0 != p.ppq || self.tempo.1 != p.tempo;
        let changed: Vec<_> = self
            .graph
            .tracks
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                (self.material_dirty
                    || tempo
                    || !self
                        .tracks
                        .get(id)
                        .is_some_and(|old| old.matches(p, &p.tracks[id])))
                .then_some((i, *id))
            })
            .collect();
        if changed.is_empty() && !tempo {
            return;
        }
        let target = p.clone();
        let snapshot = target.clone();
        let make = self.builder.material_copy();
        let work = Job::spawn("live-material", move || {
            let mut builder = make();
            let map = TempoMap::from_project(&snapshot);
            let mut warnings = Vec::new();
            let timelines = changed
                .into_iter()
                .map(|(i, id)| {
                    let timeline =
                        builder.timeline(&snapshot, &snapshot.tracks[&id], &map, &mut warnings);
                    (i, id, timeline)
                })
                .collect();
            Ok(Prepared {
                timelines,
                warnings,
            })
        });
        match work {
            Ok(work) => {
                self.material = Some(Update {
                    target,
                    generation: crate::processed::generation(),
                    work,
                    ready: None,
                })
            }
            Err(error) => report.warnings.push(error),
        }
    }

    pub(super) fn step_material(&mut self) -> Option<SyncReport> {
        let mut update = self.material.take()?;
        // Discard stale results while the worker still owns them. A newer edit
        // never publishes material for an earlier timeline or device generation.
        if !matches(&update.target, &self.song)
            || update.generation != crate::processed::generation()
            || Graph::of(&self.song) != self.graph
        {
            if !update.work.ready() && update.ready.is_none() {
                self.material = Some(update);
                return None;
            }
            if let Some(ready) = update.ready.take() {
                let _ = Job::spawn("stale-material", move || {
                    drop(ready);
                    Ok(())
                });
            }
            drop(update);
            let p = self.song.clone();
            let mut report = SyncReport::default();
            if Graph::of(&p) == self.graph {
                self.begin_material(&p, &mut report);
            }
            return Some(report);
        }
        if update.ready.is_none() {
            let Some(result) = update.work.poll() else {
                self.material = Some(update);
                return None;
            };
            match result {
                Ok(ready) => update.ready = Some(ready),
                Err(error) => {
                    return Some(SyncReport {
                        warnings: vec![error],
                        ..Default::default()
                    });
                }
            }
        }
        let ready = update.ready.as_mut().unwrap();
        let mut report = SyncReport {
            warnings: std::mem::take(&mut ready.warnings),
            ..Default::default()
        };
        while let Some((track, id, timeline)) = ready.timelines.pop_front() {
            match self.controller.send(Command::SetTimeline {
                track,
                timeline: Box::new(timeline),
            }) {
                Ok(()) => {
                    self.tracks.insert(
                        id,
                        TimelineState::new(&update.target, &update.target.tracks[&id]),
                    );
                    report.commands += 1;
                }
                Err(Command::SetTimeline { timeline, .. }) => {
                    ready.timelines.push_front((track, id, *timeline));
                    report.deferred += 1;
                    break;
                }
                Err(_) => unreachable!(),
            }
        }
        if !ready.timelines.is_empty() {
            self.material = Some(update);
            return Some(report);
        }
        self.material_dirty = false;
        self.tempo = (update.target.ppq, update.target.tempo.clone());
        self.tmap = TempoMap::from_project(&update.target);
        self.builder.plugins_mut().set_tempo(self.tmap.bpm_at(0));
        self.end_frame = self.tmap.frame_at(
            crate::graph::song_end_tick(&update.target),
            f64::from(self.rate()),
        );
        self.send_transport(&update.target);
        Some(report)
    }

    /// While topology is pending, map only safe strip values onto the running
    /// node IDs. Keep sends/keys tied to their existing engine slots.
    pub(super) fn send_pending_mix(&mut self, p: &Project, report: &mut SyncReport) {
        let mut heard = p.clone();
        for (id, old) in &self.tracks {
            heard.tracks.entry(*id).or_insert_with(|| old.track.clone());
        }
        self.automated = automation::has_lanes(&heard).then(|| heard.clone());
        let timing = crate::automation_timing::Timing::new(
            &heard,
            &self.graph,
            &self.controller.shape(),
            self.builder.plugins(),
        )
        .with_start(self.automation_start);
        let heard = timing.song_at(
            &heard,
            self.status.frame,
            &self.tmap,
            f64::from(self.rate()),
        );
        let mut mix = Mix::of(&heard, &self.graph, &Scope::new(&heard, Select::All));
        for (new, old) in mix
            .tracks
            .iter_mut()
            .zip(&self.mix.tracks)
            .chain(mix.buses.iter_mut().zip(&self.mix.buses))
        {
            new.sends.clone_from(&old.sends);
            new.keys.clone_from(&old.keys);
        }
        mix.master_keys.clone_from(&self.mix.master_keys);
        self.send_mix(&mix, report);
    }
}
