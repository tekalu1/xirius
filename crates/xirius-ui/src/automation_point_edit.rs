use crate::{
    arrange::Spot,
    studio::{Geometry, Studio},
};
use serde_json::json;
use xirius_core::auto::AutoTarget;

impl Studio {
    pub(crate) fn automation_point_wheel(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        steps: f32,
    ) -> bool {
        let Spot::Point {
            track,
            target,
            index,
        } = self.arrange_spot(g, x, y)
        else {
            return false;
        };
        let p = self.project();
        let Some(lane) = p.tracks[&track]
            .automation
            .iter()
            .find(|l| l.target == target)
        else {
            return false;
        };
        let Some(point) = lane.points.get(index) else {
            return false;
        };
        let increment = match target {
            AutoTarget::Volume | AutoTarget::Send(_) => 0.1,
            AutoTarget::Mute => 1.0,
            _ => 0.01,
        };
        let tick = point.tick;
        if self.arrange_run("auto.points.move", json!({"track":track, "target":target.to_string(), "ticks":tick.to_string(), "value-by":f64::from(steps) * increment})).is_some() {
            self.arrange.points.select(track, &target, tick, false);
            self.arrange.focus = true;
            self.input_map.browser_focus = false;
            self.choose_editor();
            self.arrange.touched();
        }
        true
    }

    pub(crate) fn automation_history(&mut self, redo: bool) -> bool {
        self.drag_drop();
        // A bounce or freeze not yet in the song is taken back first (ENG-033).
        if !redo && self.job_undo() {
            self.error = None;
            return true;
        }
        let selection = self
            .arrange
            .points
            .lane()
            .cloned()
            .and_then(|(track, target)| {
                let project = self.project();
                let lane = project
                    .tracks
                    .get(&track)?
                    .automation
                    .iter()
                    .find(|l| l.target == target)?;
                let indices = lane
                    .points
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| self.arrange.points.has(track, &target, p.tick))
                    .map(|(i, _)| i)
                    .collect::<std::collections::BTreeSet<_>>();
                Some((
                    track,
                    target,
                    lane.points.len(),
                    indices,
                    self.arrange.points.ticks().clone(),
                ))
            });
        // A step the disk cannot follow (a file renamed back, BRW-013) stays, and says why.
        let changed = match self.song.step_history(redo) {
            Ok(Some(data)) => {
                self.error = None;
                let command = if redo { "history.redo" } else { "history.undo" };
                self.after_file_command(command, &data);
                true
            }
            Ok(None) => false,
            Err(e) => {
                self.error = Some(e);
                return true;
            }
        };
        if changed && let Some((track, target, count, indices, ticks)) = selection {
            self.arrange.points.clear();
            let project = self.project();
            if let Some(lane) = project
                .tracks
                .get(&track)
                .and_then(|t| t.automation.iter().find(|l| l.target == target))
            {
                for (i, p) in lane.points.iter().enumerate() {
                    if if lane.points.len() == count {
                        indices.contains(&i)
                    } else {
                        ticks.contains(&p.tick)
                    } {
                        self.arrange.points.select(track, &target, p.tick, true);
                    }
                }
            }
        }
        changed
    }

    pub(crate) fn automation_point_nudge(&mut self, right: bool) -> bool {
        let Some((track, target)) = self.arrange.points.lane().cloned() else {
            return false;
        };
        let ticks = self.arrange.points.ticks().clone();
        let step = self
            .tools()
            .snap
            .active(&self.meter(), false)
            .unwrap_or_else(|| {
                let tick = ticks.first().copied().unwrap_or(0);
                let tm = xirius_session::tempo::TempoMap::from_project(&self.project());
                (tm.tick_at(tm.seconds_at(tick) + 0.001) - tick).max(1)
            });
        let by = if right { step } else { -step };
        if let Some(out) = self.arrange_run("auto.points.move", json!({"track":track,"target":target.to_string(),"ticks":ticks.iter().map(ToString::to_string).collect::<Vec<_>>().join(","),"by":by})) {
            let by = out["by"].as_i64().unwrap_or(0);
            self.arrange.points.clear();
            for tick in ticks { self.arrange.points.select(track, &target, tick+by, true); }
            self.arrange.touched();
        }
        true
    }
}
