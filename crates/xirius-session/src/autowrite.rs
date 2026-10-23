//! Writing automation while the song plays (AUT-002).
//!
//! A [`Pass`] lives from play to stop. The controls a person moves (a fader,
//! a pan or send knob, a plug-in's own editor) are fed to it as touches; at
//! stop [`Pass::finish`] gives the arguments of one `auto.write`, so all that
//! one pass wrote is undone in one step. The rules follow each track's mode
//! as it was when play started:
//!
//! - Touch: writes while the control is held, then glides back to what the
//!   lane had, over [`RETURN`] (a plug-in parameter with no lane and no
//!   value in the song stays where it was let go: what it had is not known).
//! - Latch: writes from the first touch to stop, the last value held.
//! - Write: writes the whole pass, from where play started to stop; lanes
//!   not touched get the channel's own value.
//!
//! While a control writes, its lane must not move it, so [`Pass::heard`]
//! gives the song as the player should hear it. Points are thinned before
//! they reach the song ([`thin`]).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::auto::{AutoLane, AutoMode, AutoPoint, AutoTarget};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Project, Track};
use xirius_core::time::Tick;
use xirius_engine::server::EditPhase;

use crate::host::ParamEvent;

/// How long a Touch takes to glide back to the lane after the control is let
/// go: a sixteenth at 960 ticks a quarter.
pub const RETURN: Tick = 240;

/// The largest step, in the lane's own height (0..1), a thinned line may be
/// away from the points it replaces.
pub const TOLERANCE: f64 = 0.004;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// The control was taken hold of.
    Begin,
    Change,
    /// It was let go.
    End,
}

impl From<EditPhase> for Phase {
    fn from(p: EditPhase) -> Phase {
        match p {
            EditPhase::Begin => Phase::Begin,
            EditPhase::Change => Phase::Change,
            EditPhase::End => Phase::End,
        }
    }
}

type Key = (TrackId, AutoTarget);

/// One stretch of a lane being written.
#[derive(Clone, Debug)]
struct Stroke {
    mode: AutoMode,
    from: Tick,
    points: Vec<(Tick, f64)>,
    /// The control is held now.
    held: bool,
}

impl Stroke {
    fn last(&self) -> Option<(Tick, f64)> {
        self.points.last().copied()
    }
}

/// What one pass of playing writes.
#[derive(Clone, Debug)]
pub struct Pass {
    /// Where play started.
    start: Tick,
    /// The song when play started: the modes and the lanes Touch returns to.
    song: Project,
    open: HashMap<Key, Stroke>,
    done: Vec<(Key, Stroke)>,
    completed: Vec<Value>,
}

impl Pass {
    pub fn begin(p: &Project, at: Tick) -> Pass {
        Pass {
            start: at,
            song: p.clone(),
            open: HashMap::new(),
            done: Vec::new(),
            completed: Vec::new(),
        }
    }

    /// Finish the visited segment at a loop boundary or seek, keeping held controls.
    pub fn restart_segment(&mut self, next_start: Tick, end: Tick) {
        let carry: Vec<_> = self
            .open
            .iter()
            .filter_map(|(key, s)| {
                s.last()
                    .map(|(_, value)| (key.clone(), s.mode, s.held, value))
            })
            .collect();
        let next = Pass::begin(&self.song, next_start);
        let old = std::mem::replace(self, next);
        self.completed = old.finish(end);
        for (key, mode, held, value) in carry {
            self.open.insert(
                key,
                Stroke {
                    mode,
                    from: next_start,
                    points: vec![(next_start, value)],
                    held,
                },
            );
        }
    }

    /// Where play started.
    pub fn start(&self) -> Tick {
        self.start
    }

    /// The mode the track writes in this pass, if it writes.
    pub fn mode(&self, track: TrackId) -> Option<AutoMode> {
        let m = self.song.tracks.get(&track)?.auto_mode;
        matches!(m, AutoMode::Touch | AutoMode::Latch | AutoMode::Write).then_some(m)
    }

    /// Does any track write in this pass?
    pub fn writes_any(&self) -> bool {
        self.song.tracks.keys().any(|t| self.mode(*t).is_some())
    }

    /// Something has been written that stop would keep.
    pub fn is_empty(&self) -> bool {
        self.completed.is_empty()
            && self.open.is_empty()
            && self.done.is_empty()
            && !self
                .song
                .tracks
                .values()
                .any(|t| t.auto_mode == AutoMode::Write)
    }

    /// A control moved. `value` is in the target's unit (dB, -1..1, 0/1, the
    /// parameter's 0..1); a press or a let-go may come without one. False when
    /// the track does not write, so the caller does what it does when
    /// nothing records.
    pub fn touch(
        &mut self,
        track: TrackId,
        target: AutoTarget,
        phase: Phase,
        tick: Tick,
        value: Option<f64>,
    ) -> bool {
        let Some(mode) = self.mode(track) else {
            return false;
        };
        let key = (track, target);
        if let Some(s) = self.open.get(&key)
            && s.last().is_some_and(|(t, _)| tick < t)
        {
            // The loop went round: what was written so far ends where it
            // got to, and a new stretch starts at the top.
            let s = self.open.remove(&key).expect("looked up above");
            self.done.push((key.clone(), s));
        }
        match phase {
            Phase::Begin | Phase::Change => {
                let s = self.open.entry(key).or_insert_with(|| Stroke {
                    mode,
                    from: tick,
                    points: Vec::new(),
                    held: true,
                });
                s.held = true;
                if let Some(v) = value {
                    s.points.push((tick, v));
                }
            }
            Phase::End => {
                let Some(mut s) = self.open.remove(&key) else {
                    return true;
                };
                s.held = false;
                if let Some(v) = value.or(s.last().map(|x| x.1)) {
                    s.points.push((tick, v));
                }
                if mode == AutoMode::Touch {
                    let back = tick + RETURN;
                    if let Some(v) = self.lane_value(&key, back) {
                        s.points.push((back, v));
                    }
                    self.done.push((key, s));
                } else {
                    // Latch and Write hold the last value until stop.
                    self.open.insert(key, s);
                }
            }
        }
        true
    }

    /// Plug-in parameter moves from the plug-ins' own editors (the
    /// `params` of [`crate::host::collect_plugin_states`]). `tick_of` turns
    /// the song frame of an event into a tick; events from while the
    /// transport stood still are skipped. Returns how many were taken.
    pub fn params(&mut self, events: &[ParamEvent], tick_of: impl Fn(u64) -> Tick) -> usize {
        let mut n = 0;
        for e in events {
            let Some(frame) = e.frame else {
                continue;
            };
            let Some((track, target)) = self.param_target(e.plugin, e.param, &e.title) else {
                continue;
            };
            if self.touch(track, target, e.phase.into(), tick_of(frame), e.value) {
                n += 1;
            }
        }
        n
    }

    /// The lane a plug-in's parameter writes to: one that names it already
    /// (by number or by title), or a new one by its title.
    fn param_target(&self, plugin: PluginId, param: u32, title: &str) -> Option<Key> {
        let (tid, t) = self
            .song
            .tracks
            .iter()
            .find(|(_, t)| t.inserts.iter().any(|x| x.id == plugin))?;
        let named = |k: &str| k.parse::<u32>().ok() == Some(param) || k.eq_ignore_ascii_case(title);
        let lane = t.automation.iter().find_map(|l| match &l.target {
            AutoTarget::Param(p, k) if *p == plugin && named(k) => Some(l.target.clone()),
            _ => None,
        });
        let key = t
            .inserts
            .iter()
            .find(|x| x.id == plugin)
            .and_then(|x| x.params.keys().find(|k| named(k)).cloned());
        let name = key.unwrap_or_else(|| {
            if title.is_empty() {
                param.to_string()
            } else {
                title.to_string()
            }
        });
        Some((*tid, lane.unwrap_or(AutoTarget::Param(plugin, name))))
    }

    /// What the lane had at `tick` when play started, or the channel's own
    /// value when it had no points.
    fn lane_value(&self, key: &Key, tick: Tick) -> Option<f64> {
        let t = self.song.tracks.get(&key.0)?;
        t.automation
            .iter()
            .find(|l| l.target == key.1)
            .and_then(|l| l.value_at(tick as f64))
            .or_else(|| self.own(key))
    }

    /// The channel's own value when play started. A plug-in parameter the
    /// song does not hold is not known: the plug-in has whatever it has.
    fn own(&self, (track, target): &Key) -> Option<f64> {
        let t = self.song.tracks.get(track)?;
        target.stored_value(t)
    }

    /// The controls that write now and their values: their lanes must not
    /// move them.
    fn held(&self) -> BTreeMap<TrackId, Vec<(AutoTarget, f64)>> {
        let mut out: BTreeMap<TrackId, Vec<(AutoTarget, f64)>> = BTreeMap::new();
        for ((t, target), s) in &self.open {
            if let Some((_, v)) = s.last() {
                out.entry(*t).or_default().push((target.clone(), v));
            }
        }
        out
    }

    /// The song as the player should hear it while writing: `p` with every
    /// writing control's lane taken away and the control at its value, and
    /// no lanes at all on a track in Write (it is being written over).
    /// Released Touch controls follow the same return line as the saved pass.
    /// `None` when nothing writes now (`p` is right as it is).
    pub fn heard(&self, p: &Project) -> Option<Project> {
        let held = self.held();
        let over: Vec<TrackId> = self
            .song
            .tracks
            .iter()
            .filter(|(_, t)| t.auto_mode == AutoMode::Write)
            .map(|(id, _)| *id)
            .collect();
        let returns: Vec<_> = self
            .done
            .iter()
            .filter(|(_, s)| s.mode == AutoMode::Touch)
            .filter_map(|(key, s)| {
                let (a, b) = (s.points.first()?, s.points.last()?);
                entry(key, a.0.min(s.from), b.0, s.points.clone(), self.own(key))
            })
            .collect();
        if held.is_empty() && over.is_empty() && returns.is_empty() && self.completed.is_empty() {
            return None;
        }
        let mut out = p.clone();
        for write in self.completed.iter().chain(&returns) {
            apply_heard(&mut out, write, &self.song);
        }
        for tid in over {
            if let Some(slot) = out.tracks.get_mut(&tid) {
                Arc::make_mut(slot).automation.clear();
            }
        }
        for (tid, list) in held {
            let Some(slot) = out.tracks.get_mut(&tid) else {
                continue;
            };
            let t = Arc::make_mut(slot);
            for (target, v) in list {
                t.automation.retain(|l| l.target != target);
                set(t, &target, v);
            }
        }
        Some(out)
    }

    /// Stop: the `writes` of one `auto.write`, empty when nothing was written.
    pub fn finish(mut self, stop: Tick) -> Vec<Value> {
        let open: Vec<(Key, Stroke)> = self.open.drain().collect();
        let mut strokes = std::mem::take(&mut self.done);
        for (key, mut s) in open {
            if let Some((t, v)) = s.last() {
                s.points.push((stop.max(t), v));
            }
            strokes.push((key, s));
        }
        // Write: the whole pass, touched or not.
        let mut whole: Vec<Key> = Vec::new();
        for (tid, t) in &self.song.tracks {
            if t.auto_mode != AutoMode::Write {
                continue;
            }
            for l in &t.automation {
                whole.push((*tid, l.target.clone()));
            }
        }
        for (key, s) in &strokes {
            if s.mode == AutoMode::Write && !whole.contains(key) {
                whole.push(key.clone());
            }
        }
        let mut out = std::mem::take(&mut self.completed);
        for key in whole {
            let end = stop.max(self.start);
            let mut mine: Vec<&Stroke> = strokes
                .iter()
                .filter(|(k, _)| *k == key)
                .map(|(_, s)| s)
                .collect();
            mine.sort_by_key(|s| s.from);
            let first = mine.iter().find_map(|s| s.points.first()).map(|x| x.1);
            let Some(own) = self.own(&key).or(first) else {
                continue;
            };
            let mut pts = vec![(self.start, own)];
            let mut last = own;
            for s in mine {
                pts.extend(
                    s.points
                        .iter()
                        .filter(|x| (self.start..=end).contains(&x.0)),
                );
                last = s.last().map_or(last, |x| x.1);
            }
            pts.push((end, last));
            out.extend(entry(&key, self.start, end, pts, self.own(&key)));
        }
        for (key, s) in strokes {
            if s.mode == AutoMode::Write {
                continue;
            }
            let (Some(a), Some(b)) = (s.points.first(), s.points.last()) else {
                continue;
            };
            let (from, to) = (a.0.min(s.from), b.0);
            let own = self.own(&key);
            out.extend(entry(&key, from, to, s.points, own));
        }
        out
    }
}

fn apply_heard(p: &mut Project, write: &Value, original: &Project) {
    let Some(track) = write["track"]
        .as_str()
        .and_then(|s| s.parse::<TrackId>().ok())
    else {
        return;
    };
    let Some(target) = write["target"]
        .as_str()
        .and_then(|s| s.parse::<AutoTarget>().ok())
    else {
        return;
    };
    let Some(slot) = p.tracks.get_mut(&track) else {
        return;
    };
    let t = Arc::make_mut(slot);
    if !t.automation.iter().any(|l| l.target == target) {
        t.automation.push(AutoLane::new(target.clone()));
    }
    let fallback = original
        .tracks
        .get(&track)
        .and_then(|t| target.stored_value(t));
    let lane = t
        .automation
        .iter_mut()
        .find(|l| l.target == target)
        .unwrap();
    let points = write["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| AutoPoint::new(p["tick"].as_i64().unwrap(), p["value"].as_f64().unwrap()));
    lane.overwrite(
        write["from"].as_i64().unwrap(),
        write["to"].as_i64().unwrap(),
        points,
        fallback,
    );
}

/// One write of `auto.write`. `own` is the channel's value when play started:
/// a lane the pass makes holds it outside the written stretch, even when the
/// song's value has followed the control since (a plug-in's own editor).
fn entry(
    key: &Key,
    from: Tick,
    to: Tick,
    pts: Vec<(Tick, f64)>,
    own: Option<f64>,
) -> Option<Value> {
    if to < from {
        return None;
    }
    let pts = thin(&key.1, pts);
    if pts.is_empty() {
        return None;
    }
    let mut w = json!({
        "track": key.0.to_string(),
        "target": key.1.to_string(),
        "from": from,
        "to": to,
        "points": pts
            .iter()
            .map(|(t, v)| json!({ "tick": t, "value": v }))
            .collect::<Vec<_>>(),
    });
    if let Some(v) = own {
        w["fallback"] = json!(v);
    }
    Some(w)
}

fn set(t: &mut Track, target: &AutoTarget, v: f64) {
    match target {
        AutoTarget::Volume => t.volume_db = v,
        AutoTarget::Pan => t.pan = v,
        AutoTarget::Mute => t.mute = v >= 0.5,
        AutoTarget::Send(s) => {
            if let Some(x) = t.sends.iter_mut().find(|x| x.id == *s) {
                x.level_db = v;
            }
        }
        AutoTarget::Param(pid, name) => {
            if let Some(x) = t.inserts.iter_mut().find(|x| x.id == *pid) {
                x.params.insert(name.clone(), v);
            }
        }
    }
}

/// Fewer points for the same line: one point per tick (the last), and a
/// point is dropped when the line without it stays within [`TOLERANCE`] of
/// the lane's height (Ramer–Douglas–Peucker, measured up and down). A
/// stepped target (mute) keeps only its changes.
pub fn thin(target: &AutoTarget, mut pts: Vec<(Tick, f64)>) -> Vec<(Tick, f64)> {
    pts.sort_by_key(|x| x.0);
    let mut one: Vec<(Tick, f64)> = Vec::with_capacity(pts.len());
    for x in pts {
        match one.last_mut() {
            Some(l) if l.0 == x.0 => *l = x,
            _ => one.push(x),
        }
    }
    if target.stepped() {
        let mut out: Vec<(Tick, f64)> = Vec::new();
        for (i, x) in one.iter().enumerate() {
            let last = i + 1 == one.len();
            if out.last().is_none_or(|l| l.1 != x.1) || last {
                out.push(*x);
            }
        }
        return out;
    }
    if one.len() <= 2 {
        return one;
    }
    let norm: Vec<f64> = one.iter().map(|x| target.to_norm(x.1)).collect();
    let mut keep = vec![false; one.len()];
    keep[0] = true;
    keep[one.len() - 1] = true;
    let mut stack = vec![(0, one.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (ta, tb) = (one[a].0 as f64, one[b].0 as f64);
        let mut worst = (0.0, a);
        for i in a + 1..b {
            let f = (one[i].0 as f64 - ta) / (tb - ta);
            let d = (norm[i] - (norm[a] + (norm[b] - norm[a]) * f)).abs();
            if d > worst.0 {
                worst = (d, i);
            }
        }
        if worst.0 > TOLERANCE {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    one.into_iter()
        .zip(keep)
        .filter_map(|(x, k)| k.then_some(x))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::auto::{AutoLane, AutoPoint};
    use xirius_core::model::Track;

    fn song(mode: AutoMode) -> (Project, TrackId) {
        let mut p = Project::new("w");
        let id = TrackId(p.alloc());
        let mut t = Track::new(id, xirius_core::model::TrackKind::Audio, "A");
        t.auto_mode = mode;
        t.volume_db = -6.0;
        let mut l = AutoLane::new(AutoTarget::Volume);
        l.points = Arc::new(vec![AutoPoint::new(0, -20.0)]);
        t.automation.push(l);
        p.tracks.insert(id, Arc::new(t));
        (p, id)
    }

    /// A fader held from 1000 to 2000, moved to -3 dB at 1500.
    fn hold(pass: &mut Pass, t: TrackId) {
        let v = AutoTarget::Volume;
        assert!(pass.touch(t, v.clone(), Phase::Begin, 1000, Some(-10.0)));
        pass.touch(t, v.clone(), Phase::Change, 1500, Some(-3.0));
        pass.touch(t, v, Phase::End, 2000, Some(-3.0));
    }

    fn points(w: &Value) -> Vec<(i64, f64)> {
        w["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| (x["tick"].as_i64().unwrap(), x["value"].as_f64().unwrap()))
            .collect()
    }

    #[test]
    fn touch_writes_while_held_and_goes_back_to_the_lane() {
        let (p, t) = song(AutoMode::Touch);
        let mut pass = Pass::begin(&p, 0);
        hold(&mut pass, t);
        let w = pass.finish(9000);
        assert_eq!(w.len(), 1);
        assert_eq!(
            (w[0]["from"].as_i64(), w[0]["to"].as_i64()),
            (Some(1000), Some(2000 + RETURN))
        );
        assert_eq!(
            points(&w[0]),
            vec![
                (1000, -10.0),
                (1500, -3.0),
                (2000, -3.0),
                (2000 + RETURN, -20.0)
            ]
        );
    }

    #[test]
    fn latch_holds_the_last_value_until_stop() {
        let (p, t) = song(AutoMode::Latch);
        let mut pass = Pass::begin(&p, 0);
        hold(&mut pass, t);
        // still heard at -3 after the let-go, without the lane
        let heard = pass.heard(&p).unwrap();
        let h = &heard.tracks[&t];
        assert_eq!(h.volume_db, -3.0);
        assert!(h.automation.is_empty());
        let w = pass.finish(9000);
        assert_eq!(
            (w[0]["from"].as_i64(), w[0]["to"].as_i64()),
            (Some(1000), Some(9000))
        );
        assert_eq!(points(&w[0]).last(), Some(&(9000, -3.0)));
    }

    #[test]
    fn write_covers_the_whole_pass() {
        let (p, t) = song(AutoMode::Write);
        let mut pass = Pass::begin(&p, 480);
        assert!(!pass.is_empty(), "Write writes even untouched");
        let heard = pass.heard(&p).unwrap();
        assert!(
            heard.tracks[&t].automation.is_empty(),
            "written over, not played"
        );
        hold(&mut pass, t);
        let w = pass.finish(9000);
        assert_eq!(w.len(), 1);
        assert_eq!(
            (w[0]["from"].as_i64(), w[0]["to"].as_i64()),
            (Some(480), Some(9000))
        );
        let pts = points(&w[0]);
        assert_eq!(
            pts.first(),
            Some(&(480, -6.0)),
            "the channel's own value before the touch"
        );
        assert_eq!(pts.last(), Some(&(9000, -3.0)));
    }

    #[test]
    fn read_and_off_do_not_write() {
        for m in [AutoMode::Read, AutoMode::Off] {
            let (p, t) = song(m);
            let mut pass = Pass::begin(&p, 0);
            assert!(!pass.touch(t, AutoTarget::Volume, Phase::Begin, 0, Some(0.0)));
            assert!(pass.heard(&p).is_none());
            assert!(pass.finish(100).is_empty());
        }
    }

    #[test]
    fn the_loop_going_round_starts_a_new_stretch() {
        let (p, t) = song(AutoMode::Latch);
        let mut pass = Pass::begin(&p, 0);
        let v = AutoTarget::Volume;
        pass.touch(t, v.clone(), Phase::Begin, 3000, Some(-1.0));
        pass.touch(t, v.clone(), Phase::Change, 3800, Some(-2.0));
        pass.touch(t, v, Phase::Change, 100, Some(-4.0));
        let w = pass.finish(500);
        let spans: Vec<(i64, i64)> = w
            .iter()
            .map(|x| (x["from"].as_i64().unwrap(), x["to"].as_i64().unwrap()))
            .collect();
        assert_eq!(spans, vec![(3000, 3800), (100, 500)]);
    }

    #[test]
    fn a_new_lane_holds_the_value_play_started_with_not_the_one_the_edit_left() {
        use xirius_core::id::Node;
        use xirius_core::id::PluginId;
        use xirius_core::model::{Plugin, PluginKind};
        use xirius_core::session::{RunOptions, Session};
        let (mut p, t) = song(AutoMode::Touch);
        let pid = PluginId(p.alloc());
        let insert = Plugin {
            id: pid,
            name: "Gain".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            uid: "gain".into(),
            bypass: false,
            deactivated: false,
            params: [("Gain".to_string(), 0.5)].into(),
            state: None,
            isolation: Default::default(),
            sidechain: None,
        };
        Arc::make_mut(p.tracks.get_mut(&t).unwrap())
            .inserts
            .push(insert);
        let gain = AutoTarget::Param(pid, "Gain".into());
        let mut pass = Pass::begin(&p, 0);
        assert!(pass.touch(t, gain.clone(), Phase::Begin, 960, Some(0.45)));
        pass.touch(t, gain.clone(), Phase::End, 960, Some(0.3));
        let w = pass.finish(1500);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0]["fallback"].as_f64(), Some(0.5), "{w:?}");
        // The editor's state reached the song before stop: the song now says 0.3.
        let slot = p.tracks.get_mut(&t).unwrap();
        Arc::make_mut(slot).inserts[0]
            .params
            .insert("Gain".into(), 0.3);
        p.root.push(Node::Track(t));
        let mut s = Session::from_project(p);
        s.run_value("auto.write", json!({ "writes": w }), &RunOptions::human())
            .unwrap();
        let lane = s.project().tracks[&t]
            .automation
            .iter()
            .find(|l| l.target == gain)
            .unwrap()
            .clone();
        let ends: Vec<(i64, f64)> = lane.points.iter().map(|x| (x.tick, x.value)).collect();
        assert_eq!(lane.value_at(959.0), Some(0.5), "{ends:?}");
        assert_eq!(lane.value_at(5000.0), Some(0.5), "{ends:?}");
    }

    #[test]
    fn a_straight_run_thins_to_its_ends() {
        let pts: Vec<(Tick, f64)> = (0..=100)
            .map(|i| (i * 10, -1.0 + 0.02 * i as f64))
            .collect();
        let out = thin(&AutoTarget::Pan, pts);
        assert_eq!(out.len(), 2);
        // a corner stays
        let mut pts: Vec<(Tick, f64)> = (0..=50).map(|i| (i * 10, 0.0)).collect();
        pts.extend((51..=100).map(|i| (i * 10, (i - 50) as f64 * 0.02)));
        let out = thin(&AutoTarget::Pan, pts);
        assert_eq!(out.len(), 3, "{out:?}");
        // mute keeps only its changes
        let m = thin(
            &AutoTarget::Mute,
            vec![
                (0, 0.0),
                (10, 0.0),
                (20, 1.0),
                (30, 1.0),
                (40, 0.0),
                (50, 0.0),
            ],
        );
        assert_eq!(m, vec![(0, 0.0), (20, 1.0), (40, 0.0), (50, 0.0)]);
    }
}
