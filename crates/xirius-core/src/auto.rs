//! Track automation: lanes of points that move a channel's volume, pan, mute,
//! a send's level or a plug-in parameter while the song plays (AUT-001).
//!
//! Points hold the value in the target's own unit (dB, -1..1 pan, 0/1 mute,
//! the parameter's number), so the song file reads plainly. The lane's drawn
//! height goes through `to_norm` / `from_norm`.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::id::{PluginId, SendId};
use crate::model::{MAX_DB, MIN_DB, Track};
use crate::time::Tick;

/// What a lane moves. Text form: `volume`, `pan`, `mute`, `send:<send id>`,
/// `param:<plugin id>:<parameter name>`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AutoTarget {
    Volume,
    Pan,
    Mute,
    Send(SendId),
    Param(PluginId, String),
}

impl fmt::Display for AutoTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutoTarget::Volume => f.write_str("volume"),
            AutoTarget::Pan => f.write_str("pan"),
            AutoTarget::Mute => f.write_str("mute"),
            AutoTarget::Send(s) => write!(f, "send:{s}"),
            AutoTarget::Param(p, name) => write!(f, "param:{p}:{name}"),
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
        Err(format!(
            "`{s}` は volume・pan・mute・send:<ID>・param:<ID>:<名前> のいずれでもありません"
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
            AutoTarget::Volume | AutoTarget::Send(_) => (MIN_DB, MAX_DB),
            AutoTarget::Pan => (-1.0, 1.0),
            AutoTarget::Mute | AutoTarget::Param(..) => (0.0, 1.0),
        }
    }

    /// Mute jumps from point to point; the rest glide.
    pub fn stepped(&self) -> bool {
        matches!(self, AutoTarget::Mute)
    }

    /// The value as a height of the lane, 0 (bottom) ..= 1 (top). Levels use
    /// the fader's feel: 0 dB sits a little above the middle.
    pub fn to_norm(&self, v: f64) -> f64 {
        let (lo, hi) = self.range();
        let n = match self {
            AutoTarget::Volume | AutoTarget::Send(_) => {
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
            AutoTarget::Volume | AutoTarget::Send(_) => {
                if n <= 0.0 {
                    MIN_DB
                } else {
                    (MAX_DB + 60.0 * n.log10()).clamp(MIN_DB, MAX_DB)
                }
            }
            AutoTarget::Mute => n.round(),
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
            AutoTarget::Send(s) => t.sends.iter().find(|x| x.id == *s).map(|x| x.level_db),
            AutoTarget::Param(p, name) => t
                .inserts
                .iter()
                .find(|x| x.id == *p)
                .map(|x| x.params.get(name).copied().unwrap_or(0.0)),
        }
    }

    /// The saved value, without inventing a value for an unknown plug-in parameter.
    /// A muted send keeps the level it returns to when unmuted.
    pub fn stored_value(&self, t: &Track) -> Option<f64> {
        match self {
            AutoTarget::Send(s) => t
                .sends
                .iter()
                .find(|x| x.id == *s)
                .map(|x| x.muted_level.unwrap_or(x.level_db)),
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
        Some(match self.target {
            AutoTarget::Volume | AutoTarget::Send(_) => self.target.from_norm(n),
            _ => a.value + (b.value - a.value) * bend(f, a.curve),
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
        for s in ["volume", "pan", "mute", "send:snd_4", "param:plg_9:cutoff"] {
            let t: AutoTarget = s.parse().unwrap();
            assert_eq!(t.to_string(), s);
        }
        assert!("param:plg_9:".parse::<AutoTarget>().is_err());
        assert!("gain".parse::<AutoTarget>().is_err());
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
