//! Track automation: lanes of points that move a channel's volume, pan, mute,
//! a send's level or a plug-in parameter while the song plays (AUT-001).
//!
//! Points hold the value in the target's own unit (dB, -1..1 pan, 0/1 mute,
//! the parameter's number), so the song file reads plainly. The lane's drawn
//! height goes through `to_norm` / `from_norm`.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

pub use xirius_params::{MAX_LIST, clap_steps, vst3_steps};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::controls::ControlId;
use crate::id::{NodeId, PluginId, SendId};
use crate::model::{MAX_DB, MIN_DB, Track};
use crate::time::Tick;

/// What a lane moves. Text form: `volume`, `pan`, `mute`, `send:<send id>`,
/// `param:<plugin id>:<parameter name>`, `branch:<node id>:gain|pan|mute`,
/// `control:k1`..`k8`, `b1`..`b8`, `x1`, `y1`, `x2`, `y2`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AutoTarget {
    Volume,
    Pan,
    Mute,
    Send(SendId),
    Param(PluginId, String),
    /// The gain, pan or mute of a splitter's branch or a multi-instrument's
    /// layer (PRJ-026, MIX-039).
    Branch(NodeId, BranchPart),
    /// A macro control of the track, 0..1 (AUT-018). Its lane moves the
    /// control and through it every target the control is mapped to.
    Control(ControlId),
}

/// Which level of a branch (or layer) a [`AutoTarget::Branch`] moves.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BranchPart {
    Gain,
    Pan,
    Mute,
}

impl BranchPart {
    pub const ALL: [BranchPart; 3] = [BranchPart::Gain, BranchPart::Pan, BranchPart::Mute];

    pub fn name(self) -> &'static str {
        match self {
            BranchPart::Gain => "gain",
            BranchPart::Pan => "pan",
            BranchPart::Mute => "mute",
        }
    }

    pub fn parse(s: &str) -> Option<BranchPart> {
        BranchPart::ALL.into_iter().find(|p| p.name() == s)
    }
}

impl fmt::Display for AutoTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutoTarget::Volume => f.write_str("volume"),
            AutoTarget::Pan => f.write_str("pan"),
            AutoTarget::Mute => f.write_str("mute"),
            AutoTarget::Send(s) => write!(f, "send:{s}"),
            AutoTarget::Param(p, name) => write!(f, "param:{p}:{name}"),
            AutoTarget::Branch(n, part) => write!(f, "branch:{n}:{}", part.name()),
            AutoTarget::Control(c) => write!(f, "control:{c}"),
        }
    }
}

impl std::str::FromStr for AutoTarget {
    type Err = String;
    fn from_str(s: &str) -> Result<AutoTarget, String> {
        match s {
            "volume" => return Ok(AutoTarget::Volume),
            "pan" => return Ok(AutoTarget::Pan),
            "mute" => return Ok(AutoTarget::Mute),
            _ => {}
        }
        if let Some(id) = s.strip_prefix("send:") {
            return Ok(AutoTarget::Send(id.parse()?));
        }
        if let Some(rest) = s.strip_prefix("param:")
            && let Some((id, name)) = rest.split_once(':')
            && !name.is_empty()
        {
            return Ok(AutoTarget::Param(id.parse()?, name.to_string()));
        }
        if let Some(rest) = s.strip_prefix("branch:")
            && let Some((id, part)) = rest.split_once(':')
            && let Some(part) = BranchPart::parse(part)
        {
            return Ok(AutoTarget::Branch(id.parse()?, part));
        }
        if let Some(c) = s.strip_prefix("control:") {
            return Ok(AutoTarget::Control(c.parse()?));
        }
        Err(format!(
            "`{s}` は volume・pan・mute・send:<ID>・param:<ID>:<名前>・branch:<ID>:gain|pan|mute・control:<k1〜k8|b1〜b8|x1|y1|x2|y2> のいずれでもありません"
        ))
    }
}

impl Serialize for AutoTarget {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AutoTarget {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl AutoTarget {
    /// The lowest and highest value a point may hold.
    pub fn range(&self) -> (f64, f64) {
        match self {
            AutoTarget::Volume | AutoTarget::Send(_) | AutoTarget::Branch(_, BranchPart::Gain) => {
                (MIN_DB, MAX_DB)
            }
            AutoTarget::Pan | AutoTarget::Branch(_, BranchPart::Pan) => (-1.0, 1.0),
            AutoTarget::Mute
            | AutoTarget::Param(..)
            | AutoTarget::Branch(_, BranchPart::Mute)
            | AutoTarget::Control(_) => (0.0, 1.0),
        }
    }

    /// Mute and a macro button jump from point to point; the rest glide.
    pub fn stepped(&self) -> bool {
        match self {
            AutoTarget::Mute | AutoTarget::Branch(_, BranchPart::Mute) => true,
            AutoTarget::Control(c) => c.is_button(),
            _ => false,
        }
    }

    /// How many values a point may hold on this channel, when the values between have no
    /// meaning: a switch (on or off) or a list of choices. The points of such a lane sit on
    /// the steps and hold up to the next one (a stair). A whole number (a transpose in
    /// semitones) is not one: its lane glides through the numbers between, and the
    /// parameter takes the nearest as it plays. A VST3 or CLAP parameter has steps when the
    /// running plug-in says so (`ext`, [`ParamSteps`]).
    pub fn steps_with(&self, t: &Track, ext: &ParamSteps) -> Option<u32> {
        match self {
            AutoTarget::Param(p, name) => {
                let plugin = t.inserts.iter().find(|x| x.id == *p)?;
                match plugin.builtin() {
                    Some(def) => match def.resolve(name)?.spec.curve {
                        xirius_params::Curve::Toggle => Some(2),
                        xirius_params::Curve::Choice(labels) if labels.len() >= 2 => {
                            Some(labels.len() as u32)
                        }
                        _ => None,
                    },
                    None => ext.of(*p, name),
                }
            }
            t if t.stepped() => Some(2),
            _ => None,
        }
    }

    /// [`AutoTarget::steps_with`] for the built-in devices and Mute alone.
    pub fn steps_on(&self, t: &Track) -> Option<u32> {
        self.steps_with(t, &ParamSteps::default())
    }

    /// `v` on the nearest step of this channel's target; as it is when the target glides.
    pub fn snap_with(&self, t: &Track, ext: &ParamSteps, v: f64) -> f64 {
        match self.steps_with(t, ext) {
            Some(n) => snap(self.range(), n, v),
            None => v,
        }
    }

    /// [`AutoTarget::snap_with`] for the built-in devices and Mute alone.
    pub fn snap_on(&self, t: &Track, v: f64) -> f64 {
        self.snap_with(t, &ParamSteps::default(), v)
    }

    /// A level in dB that moves along the fader's feel.
    pub fn is_level(&self) -> bool {
        matches!(
            self,
            AutoTarget::Volume | AutoTarget::Send(_) | AutoTarget::Branch(_, BranchPart::Gain)
        )
    }

    /// The value as a height of the lane, 0 (bottom) ..= 1 (top). Levels use
    /// the fader's feel: 0 dB sits a little above the middle.
    pub fn to_norm(&self, v: f64) -> f64 {
        let (lo, hi) = self.range();
        let n = match self {
            t if t.is_level() => {
                if v <= MIN_DB {
                    0.0
                } else {
                    (10f64.powf((v - MAX_DB) / 60.0)).clamp(0.0, 1.0)
                }
            }
            _ => (v - lo) / (hi - lo),
        };
        n.clamp(0.0, 1.0)
    }

    pub fn from_norm(&self, n: f64) -> f64 {
        let n = n.clamp(0.0, 1.0);
        let (lo, hi) = self.range();
        match self {
            t if t.is_level() => {
                if n <= 0.0 {
                    MIN_DB
                } else {
                    (MAX_DB + 60.0 * n.log10()).clamp(MIN_DB, MAX_DB)
                }
            }
            t if t.stepped() => n.round(),
            _ => lo + (hi - lo) * n,
        }
    }

    /// The value the channel has now, without automation (the flat line a
    /// new lane draws, AUT-016). `None` when the send or parameter is gone.
    pub fn current(&self, t: &Track) -> Option<f64> {
        match self {
            AutoTarget::Volume => Some(t.volume_db),
            AutoTarget::Pan => Some(t.pan),
            AutoTarget::Mute => Some(if t.mute { 1.0 } else { 0.0 }),
            // A muted send's level is the one kept aside, where `set_on` writes it.
            AutoTarget::Send(s) => t
                .sends
                .iter()
                .find(|x| x.id == *s)
                .map(|x| x.muted_level.unwrap_or(x.level_db)),
            AutoTarget::Param(p, name) => t.inserts.iter().find(|x| x.id == *p).map(|x| {
                x.params.get(name).copied().unwrap_or_else(|| {
                    // an untouched builtin parameter sits at its default
                    x.builtin()
                        .and_then(|d| d.param(name))
                        .map_or(0.0, |(_, p)| p.default_norm())
                })
            }),
            AutoTarget::Branch(n, part) => {
                t.rack
                    .as_ref()
                    .and_then(|r| r.levels(*n))
                    .map(|(gain, pan, mute)| match part {
                        BranchPart::Gain => gain,
                        BranchPart::Pan => pan,
                        BranchPart::Mute => f64::from(u8::from(mute)),
                    })
            }
            AutoTarget::Control(c) => Some(t.controls.get(*c).value),
        }
    }

    /// Writes `v` where this target lives on the channel; `false` when it is gone.
    pub fn set_on(&self, t: &mut Track, v: f64) -> bool {
        match self {
            AutoTarget::Volume => t.volume_db = v,
            AutoTarget::Pan => t.pan = v,
            AutoTarget::Mute => t.mute = v >= 0.5,
            // A muted send keeps its level aside (`muted_level`) and stays silent: a macro or a
            // lane moves the kept level, as `stored_value` reads it.
            AutoTarget::Send(s) => match t.sends.iter_mut().find(|x| x.id == *s) {
                Some(x) => match x.muted_level.as_mut() {
                    Some(kept) => *kept = v,
                    None => x.level_db = v,
                },
                None => return false,
            },
            AutoTarget::Param(pid, name) => match t.inserts.iter_mut().find(|x| x.id == *pid) {
                Some(x) => {
                    x.params.insert(name.clone(), v);
                }
                None => return false,
            },
            AutoTarget::Branch(n, part) => match t.rack.as_mut().and_then(|r| r.levels_mut(*n)) {
                Some(l) => match part {
                    BranchPart::Gain => *l.gain_db = v,
                    BranchPart::Pan => *l.pan = v,
                    BranchPart::Mute => *l.mute = v >= 0.5,
                },
                None => return false,
            },
            // the control moves what it is mapped to as well
            AutoTarget::Control(c) => {
                let v = if c.is_button() { v.round() } else { v }.clamp(0.0, 1.0);
                t.controls.get_mut(*c).value = v;
                t.apply_control_at(*c, v);
            }
        }
        true
    }

    /// The saved value, without inventing a value for an unknown plug-in parameter.
    /// A muted send keeps the level it returns to when unmuted.
    pub fn stored_value(&self, t: &Track) -> Option<f64> {
        match self {
            AutoTarget::Param(p, name) => t
                .inserts
                .iter()
                .find(|x| x.id == *p)
                .and_then(|x| x.params.get(name).copied()),
            _ => self.current(t),
        }
    }

    /// Does the channel still have what this lane points at?
    pub fn exists_on(&self, t: &Track) -> bool {
        self.current(t).is_some()
    }
}

/// How a channel's automation is played and written (AUT-002, AUT-011).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMode {
    /// Lanes are kept but not played.
    Off,
    /// Lanes play; moving a control does not write.
    #[default]
    Read,
    /// Writes while a control is held, then returns to the lane.
    Touch,
    /// Writes from the first touch until playback stops.
    Latch,
    /// Writes the whole time it plays.
    Write,
}

impl AutoMode {
    pub const ALL: [AutoMode; 5] = [
        AutoMode::Off,
        AutoMode::Read,
        AutoMode::Touch,
        AutoMode::Latch,
        AutoMode::Write,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AutoMode::Off => "off",
            AutoMode::Read => "read",
            AutoMode::Touch => "touch",
            AutoMode::Latch => "latch",
            AutoMode::Write => "write",
        }
    }

    pub fn parse(s: &str) -> Option<AutoMode> {
        AutoMode::ALL.into_iter().find(|m| m.name() == s)
    }

    /// Do the lanes drive the sound in this mode? (Write replaces them.)
    pub fn plays(self) -> bool {
        matches!(self, AutoMode::Read | AutoMode::Touch | AutoMode::Latch)
    }

    pub fn is_read(&self) -> bool {
        *self == AutoMode::Read
    }
}

/// One point of a lane.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct AutoPoint {
    /// Song position.
    pub tick: Tick,
    pub value: f64,
    /// Bend of the line to the next point, -1 (sags) ..= 1 (bulges); 0 is straight.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub curve: f64,
    /// Holds the value up to the next point, then jumps (a stair); `curve` is not used.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub step: bool,
}

impl AutoPoint {
    /// A point with a straight line to the next one.
    pub fn new(tick: Tick, value: f64) -> AutoPoint {
        AutoPoint {
            tick,
            value,
            curve: 0.0,
            step: false,
        }
    }
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// The points of one target, in time order.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct AutoLane {
    /// Playback is suspended while the owning send is muted; points remain editable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suspended: bool,
    pub target: AutoTarget,
    /// Folded away with × (the points keep playing).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(default)]
    pub points: Arc<Vec<AutoPoint>>,
}

impl AutoLane {
    pub fn on_track(target: AutoTarget, track: &Track) -> AutoLane {
        let suspended = matches!(target, AutoTarget::Send(id) if track.sends.iter().any(|s|s.id==id && s.muted_level.is_some()));
        AutoLane {
            suspended,
            ..AutoLane::new(target)
        }
    }
    pub fn new(target: AutoTarget) -> AutoLane {
        AutoLane {
            suspended: false,
            target,
            hidden: false,
            points: Arc::new(Vec::new()),
        }
    }

    /// The value at a song position; `None` when the lane has no points.
    pub fn value_at(&self, tick: f64) -> Option<f64> {
        if self.suspended {
            return None;
        }
        self.curve_at(tick)
    }

    /// The stored curve, including while its send is muted.
    pub fn curve_at(&self, tick: f64) -> Option<f64> {
        let pts = &*self.points;
        let first = pts.first()?;
        if tick <= first.tick as f64 {
            return Some(first.value);
        }
        let i = pts.partition_point(|p| (p.tick as f64) <= tick);
        let a = pts[i - 1];
        let Some(b) = pts.get(i) else {
            return Some(a.value);
        };
        if self.target.stepped() || a.step {
            return Some(a.value);
        }
        let f = (tick - a.tick as f64) / (b.tick - a.tick) as f64;
        let (na, nb) = (self.target.to_norm(a.value), self.target.to_norm(b.value));
        let n = na + (nb - na) * bend(f, a.curve);
        // levels glide along the lane's height, as drawn
        Some(if self.target.is_level() {
            self.target.from_norm(n)
        } else {
            a.value + (b.value - a.value) * bend(f, a.curve)
        })
    }

    pub(crate) fn point_at(&self, tick: Tick) -> Option<AutoPoint> {
        if let Some(x) = self.points.iter().find(|x| x.tick == tick) {
            return Some(*x);
        }
        // inside a stair the new point holds on, so the line after it stays the same
        let step = self
            .points
            .iter()
            .rev()
            .find(|x| x.tick < tick)
            .is_some_and(|x| x.step);
        self.value_at(tick as f64).map(|value| AutoPoint {
            step,
            ..AutoPoint::new(tick, value)
        })
    }

    /// Lifts the stretch `[from, to]` out to be laid down elsewhere (AUT-012):
    /// its points with the values in force at both ends. The lane keeps those
    /// two values, so the line outside the stretch does not change.
    pub fn take(&mut self, from: Tick, to: Tick) -> Vec<AutoPoint> {
        let (Some(a), Some(b)) = (self.point_at(from), self.point_at(to)) else {
            return Vec::new();
        };
        let mut out = vec![a];
        out.extend(
            self.points
                .iter()
                .filter(|x| x.tick > from && x.tick < to)
                .copied(),
        );
        out.push(b);
        self.splice(
            from,
            to,
            [
                AutoPoint {
                    curve: 0.0,
                    step: false,
                    ..a
                },
                AutoPoint { curve: 0.0, ..b },
            ],
        );
        out
    }

    /// Puts `pts` in place of every point in `[from, to]`.
    pub fn splice(&mut self, from: Tick, to: Tick, pts: impl IntoIterator<Item = AutoPoint>) {
        let v = Arc::make_mut(&mut self.points);
        v.retain(|x| x.tick < from || x.tick > to);
        v.extend(pts);
        v.sort_by_key(|x| x.tick);
        v.dedup_by_key(|x| x.tick);
    }

    /// Are the points on the steps of the target on `t`, as stairs (or does it glide)?
    pub fn snapped_with(&self, t: &Track, ext: &ParamSteps) -> bool {
        let Some(n) = self.target.steps_with(t, ext) else {
            return true;
        };
        let range = self.target.range();
        // Mute and the like hold by their kind already; a parameter holds by its points
        let hold = !self.target.stepped();
        self.points
            .iter()
            .all(|x| (!hold || (x.step && x.curve == 0.0)) && snap(range, n, x.value) == x.value)
    }

    /// Makes the lane of a target with steps a stair on `t`: each point on a step, holding up
    /// to the next. Where a line between two points crossed from one step to another, a point
    /// goes at the first tick past the crossing, so the lane plays as it did (the parameter
    /// took the nearest step all along): a song saved before, or a lane written as a line,
    /// sounds the same to the tick. `false` when the lane was a stair already (it is not
    /// touched then).
    pub fn snap_with(&mut self, t: &Track, ext: &ParamSteps) -> bool {
        self.snap_as(t, ext, true)
    }

    /// [`AutoLane::snap_with`] for a lane that was a stair before an edit wrote it: each point
    /// goes on its step and holds, with no point at a crossing (a point added on a stair is a
    /// step of it, not the end of a line).
    pub fn snap_points_with(&mut self, t: &Track, ext: &ParamSteps) -> bool {
        self.snap_as(t, ext, false)
    }

    fn snap_as(&mut self, t: &Track, ext: &ParamSteps, cross: bool) -> bool {
        if self.snapped_with(t, ext) {
            return false;
        }
        let Some(n) = self.target.steps_with(t, ext) else {
            return false;
        };
        let range = self.target.range();
        let level = |v: f64| snap(range, n, v);
        if self.target.stepped() {
            for x in Arc::make_mut(&mut self.points) {
                x.value = level(x.value);
            }
            return true;
        }
        if !cross {
            for x in Arc::make_mut(&mut self.points) {
                x.value = level(x.value);
                x.step = true;
                x.curve = 0.0;
            }
            return true;
        }
        let pts = &*self.points;
        let mut out = Vec::with_capacity(pts.len());
        for (i, a) in pts.iter().enumerate() {
            out.push(AutoPoint {
                value: level(a.value),
                curve: 0.0,
                step: true,
                ..*a
            });
            let Some(b) = pts.get(i + 1).filter(|_| !a.step) else {
                continue;
            };
            let at = |tick: Tick| {
                let f = (tick - a.tick) as f64 / (b.tick - a.tick) as f64;
                level(a.value + (b.value - a.value) * bend(f, a.curve))
            };
            // the line runs one way, so each step it reaches starts at one tick
            let (mut from, mut now) = (a.tick, level(a.value));
            while level(b.value) != now {
                let (mut lo, mut hi) = (from, b.tick);
                while hi - lo > 1 {
                    let mid = lo + (hi - lo) / 2;
                    if at(mid) == now {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                if hi >= b.tick {
                    break;
                }
                now = at(hi);
                from = hi;
                out.push(AutoPoint {
                    step: true,
                    ..AutoPoint::new(hi, now)
                });
            }
        }
        self.points = Arc::new(out);
        true
    }

    /// [`AutoLane::snapped_with`] for the built-in devices and Mute alone.
    pub fn snapped_on(&self, t: &Track) -> bool {
        self.snapped_with(t, &ParamSteps::default())
    }

    /// [`AutoLane::snap_with`] for the built-in devices and Mute alone.
    pub fn snap_on(&mut self, t: &Track) -> bool {
        self.snap_with(t, &ParamSteps::default())
    }
}

impl crate::model::Project {
    /// Makes every lane of a target with steps a stair ([`AutoLane::snap_with`]) as a song
    /// comes in: read from a file (a song saved before had lines between the steps of a
    /// switch) or from a DAWproject. Only the built-in devices: what a VST3 or CLAP plug-in
    /// has is not known until it runs. Returns how many lanes it changed.
    pub fn snap_lanes(&mut self) -> usize {
        self.snap_lanes_where(&ParamSteps::default(), |_, _| Some(true))
    }

    /// [`Project::snap_lanes`] for the lanes an edit wrote (those whose points are not the
    /// ones in `before`), knowing the steps of the running plug-ins: an edit leaves the lanes
    /// it did not touch as they are, so its undo holds only its own change. A lane that was a
    /// stair (or new) keeps the points the edit put, each on its step
    /// ([`AutoLane::snap_points_with`]); one that was still lines (a plug-in's lane from before
    /// its steps were known) becomes the stair it played as.
    pub fn snap_edited_lanes(&mut self, before: &crate::model::Project, ext: &ParamSteps) -> usize {
        self.snap_lanes_where(ext, |t, l| {
            let was = before.tracks.get(&t.id).and_then(|b| {
                b.automation
                    .iter()
                    .find(|x| x.target == l.target)
                    .map(|x| (b, x))
            });
            match was {
                Some((_, x)) if Arc::ptr_eq(&x.points, &l.points) => None,
                Some((b, x)) => Some(!x.snapped_with(b, ext)),
                None => Some(false),
            }
        })
    }

    fn snap_lanes_where(
        &mut self,
        ext: &ParamSteps,
        mut pick: impl FnMut(&Track, &AutoLane) -> Option<bool>,
    ) -> usize {
        let mut n = 0;
        for t in self.tracks.values_mut() {
            if t.automation
                .iter()
                .all(|l| l.snapped_with(t, ext) || pick(t, l).is_none())
            {
                continue;
            }
            let t = Arc::make_mut(t);
            let owner = t.clone();
            for l in &mut t.automation {
                if let Some(cross) = pick(&owner, l) {
                    n += usize::from(l.snap_as(&owner, ext, cross));
                }
            }
        }
        n
    }
}

/// What only a running VST3 or CLAP plug-in can tell of its parameters: how many values each
/// switch or list of choices takes, by plug-in, parameter id and title. A parameter it does
/// not name glides (one the plug-in does not say has steps, or whose steps it tells in a way
/// that cannot be trusted).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamSteps(Arc<HashMap<PluginId, StepList>>);

/// One plugin's parameters with steps: (id, title, values).
pub type StepList = Vec<(u32, String, u32)>;

impl ParamSteps {
    pub fn new(map: HashMap<PluginId, StepList>) -> ParamSteps {
        ParamSteps(Arc::new(map))
    }

    pub fn is_empty(&self) -> bool {
        self.0.values().all(Vec::is_empty)
    }

    /// How many plugins it knows steps of.
    pub fn plugins(&self) -> usize {
        self.0.values().filter(|l| !l.is_empty()).count()
    }

    /// What is known of one plugin: (id, title, values) of each parameter with steps.
    pub fn list(&self, plugin: PluginId) -> &[(u32, String, u32)] {
        self.0.get(&plugin).map_or(&[], Vec::as_slice)
    }

    /// The values of a parameter named as a lane names it: by its id, else by its title.
    pub fn of(&self, plugin: PluginId, key: &str) -> Option<u32> {
        let list = self.0.get(&plugin)?;
        let by_id = key
            .parse::<u32>()
            .ok()
            .and_then(|id| list.iter().find(|(x, _, _)| *x == id));
        by_id
            .or_else(|| {
                list.iter()
                    .find(|(_, title, _)| title.eq_ignore_ascii_case(key))
            })
            .map(|(_, _, n)| *n)
            .filter(|n| *n >= 2)
    }
}

/// `v` on the nearest of `n` evenly spaced steps from `lo` to `hi`.
pub fn snap((lo, hi): (f64, f64), n: u32, v: f64) -> f64 {
    if n < 2 || hi <= lo {
        return v;
    }
    let last = f64::from(n - 1);
    let k = ((v.clamp(lo, hi) - lo) / (hi - lo) * last).round();
    lo + (hi - lo) * k / last
}

/// Shapes the 0..1 fraction of a segment by its curve (-1..1).
pub fn bend(f: f64, curve: f64) -> f64 {
    let f = f.clamp(0.0, 1.0);
    if curve == 0.0 {
        return f;
    }
    f.powf(4f64.powf(-curve.clamp(-1.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_read_back_from_their_text() {
        for s in [
            "volume",
            "pan",
            "mute",
            "send:snd_4",
            "param:plg_9:cutoff",
            "branch:nod_3:gain",
            "branch:nod_3:pan",
            "branch:nod_3:mute",
        ] {
            let t: AutoTarget = s.parse().unwrap();
            assert_eq!(t.to_string(), s);
        }
        assert!("param:plg_9:".parse::<AutoTarget>().is_err());
        assert!("gain".parse::<AutoTarget>().is_err());
        assert!("branch:nod_3:width".parse::<AutoTarget>().is_err());
    }

    #[test]
    fn levels_map_to_heights_and_back() {
        let t = AutoTarget::Volume;
        assert_eq!(t.to_norm(MIN_DB), 0.0);
        assert!((t.to_norm(MAX_DB) - 1.0).abs() < 1e-9);
        for db in [-60.0, -12.0, -6.0, 0.0, 6.0] {
            assert!((t.from_norm(t.to_norm(db)) - db).abs() < 1e-6);
        }
        assert!(t.to_norm(0.0) > 0.5, "0 dB sits above the middle");
    }

    #[test]
    fn a_lane_glides_between_points_and_holds_at_the_ends() {
        let mut l = AutoLane::new(AutoTarget::Pan);
        assert_eq!(l.value_at(0.0), None);
        l.points = Arc::new(vec![AutoPoint::new(100, -1.0), AutoPoint::new(300, 1.0)]);
        assert_eq!(l.value_at(0.0), Some(-1.0));
        assert_eq!(l.value_at(200.0), Some(0.0));
        assert_eq!(l.value_at(900.0), Some(1.0));
        // a bulging curve is ahead of the straight line halfway
        Arc::make_mut(&mut l.points)[0].curve = 1.0;
        assert!(l.value_at(200.0).unwrap() > 0.0);
        l.target = AutoTarget::Mute;
        assert_eq!(l.value_at(299.0), Some(-1.0), "mute steps");
    }

    #[test]
    fn a_stair_holds_its_value_up_to_the_next_point() {
        let mut l = AutoLane::new(AutoTarget::Volume);
        l.points = Arc::new(vec![
            AutoPoint {
                step: true,
                ..AutoPoint::new(100, -12.0)
            },
            AutoPoint::new(300, 0.0),
            AutoPoint::new(500, -6.0),
        ]);
        assert_eq!(l.value_at(100.0), Some(-12.0));
        assert_eq!(l.value_at(299.0), Some(-12.0));
        assert_eq!(l.value_at(300.0), Some(0.0));
        // the next segment is not a stair: it glides
        let v = l.value_at(400.0).unwrap();
        assert!(v < 0.0 && v > -6.0, "{v}");
    }

    #[test]
    fn points_without_a_shape_read_as_before_and_write_as_before() {
        // a song file from before the stair
        let old = r#"[{"tick":0,"value":-6.0},{"tick":960,"value":0.0,"curve":0.5}]"#;
        let pts: Vec<AutoPoint> = serde_json::from_str(old).unwrap();
        assert!(pts.iter().all(|x| !x.step));
        assert_eq!(pts[1].curve, 0.5);
        assert_eq!(serde_json::to_string(&pts).unwrap(), old);
        let stair = AutoPoint {
            step: true,
            ..AutoPoint::new(0, 1.0)
        };
        let text = serde_json::to_string(&stair).unwrap();
        assert_eq!(text, r#"{"tick":0,"value":1.0,"step":true}"#);
        assert_eq!(serde_json::from_str::<AutoPoint>(&text).unwrap(), stair);
    }

    #[test]
    fn lifting_a_stretch_out_of_a_stair_leaves_the_line_after_it() {
        let mut l = AutoLane::new(AutoTarget::Pan);
        l.points = Arc::new(vec![
            AutoPoint {
                step: true,
                ..AutoPoint::new(0, -1.0)
            },
            AutoPoint::new(1000, 1.0),
        ]);
        let before: Vec<_> = (600..1000)
            .step_by(50)
            .map(|t| l.value_at(t as f64))
            .collect();
        l.take(200, 500);
        let after: Vec<_> = (600..1000)
            .step_by(50)
            .map(|t| l.value_at(t as f64))
            .collect();
        assert_eq!(before, after);
    }
}
