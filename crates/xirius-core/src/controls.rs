//! A track's macro controls (MIX-039): eight knobs, eight buttons and two
//! X/Y pads. Each control holds a value 0..=1 and moves any number of the
//! track's targets (plug-in parameters, branch levels, volume, pan, sends)
//! along its own range and curve. One target answers to one control only.
//!
//! The control's value is part of the song. Setting it writes every mapped
//! target into the song too, so the mix and the running plug-ins follow by
//! the paths they already take; playback re-applies it over the lanes
//! (`at_each`), so a mapped target cannot drift from its control.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::auto::{AutoTarget, BranchPart, bend};
use crate::model::Track;

pub const KNOBS: usize = 8;
pub const BUTTONS: usize = 8;
pub const PADS: usize = 2;
/// The knobs the screen's X/Y pad moves (MIX-041) before others are chosen: Macro 1 and 2.
pub const XY: [u8; 2] = [0, 1];

/// One control of the set. Text form: `k1`..`k8`, `b1`..`b8`, `x1`, `y1`,
/// `x2`, `y2`. The number inside is zero-based.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ControlId {
    Knob(u8),
    Button(u8),
    X(u8),
    Y(u8),
}

impl ControlId {
    /// Every control, knobs first.
    pub fn all() -> impl Iterator<Item = ControlId> {
        (0..KNOBS as u8)
            .map(ControlId::Knob)
            .chain((0..BUTTONS as u8).map(ControlId::Button))
            .chain((0..PADS as u8).flat_map(|i| [ControlId::X(i), ControlId::Y(i)]))
    }

    /// A button is off (0) or on (1), nothing between.
    pub fn is_button(self) -> bool {
        matches!(self, ControlId::Button(_))
    }

    /// The name a control has before it is named or mapped.
    pub fn default_name(self) -> String {
        match self {
            ControlId::Knob(i) => format!("Macro {}", i + 1),
            ControlId::Button(i) => format!("Button {}", i + 1),
            ControlId::X(i) => format!("Pad {} X", i + 1),
            ControlId::Y(i) => format!("Pad {} Y", i + 1),
        }
    }
}

impl fmt::Display for ControlId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlId::Knob(i) => write!(f, "k{}", i + 1),
            ControlId::Button(i) => write!(f, "b{}", i + 1),
            ControlId::X(i) => write!(f, "x{}", i + 1),
            ControlId::Y(i) => write!(f, "y{}", i + 1),
        }
    }
}

impl std::str::FromStr for ControlId {
    type Err = String;
    fn from_str(s: &str) -> Result<ControlId, String> {
        let bad = || format!("`{s}` は k1〜k8・b1〜b8・x1・y1・x2・y2 のいずれでもありません");
        let mut chars = s.chars();
        let kind = chars.next().ok_or_else(bad)?;
        let n: usize = chars.as_str().parse().map_err(|_| bad())?;
        let (max, make): (usize, fn(u8) -> ControlId) = match kind {
            'k' => (KNOBS, ControlId::Knob),
            'b' => (BUTTONS, ControlId::Button),
            'x' => (PADS, ControlId::X),
            'y' => (PADS, ControlId::Y),
            _ => return Err(bad()),
        };
        if n == 0 || n > max {
            return Err(bad());
        }
        Ok(make((n - 1) as u8))
    }
}

/// One mapped target: the control's 0..1 moves it from `from` to `to`
/// (heights of the target, 0..1; `from > to` runs backwards) along `curve`
/// (-1..1, as an automation segment bends).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Mapping {
    pub target: AutoTarget,
    #[serde(default)]
    pub from: f64,
    #[serde(default = "one")]
    pub to: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub curve: f64,
}

fn one() -> f64 {
    1.0
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Mapping {
    pub fn new(target: AutoTarget) -> Mapping {
        Mapping {
            target,
            from: 0.0,
            to: 1.0,
            curve: 0.0,
        }
    }

    /// The target's value (in its own unit) for the control at `v`.
    pub fn value(&self, v: f64) -> f64 {
        let n = self.from + (self.to - self.from) * bend(v, self.curve);
        self.target.from_norm(n)
    }

    /// The control value that gives the target's height `n` (straight curve
    /// only; a bent map is solved by halving).
    pub fn inverse(&self, n: f64) -> f64 {
        if self.from == self.to {
            return 0.0;
        }
        let f = ((n - self.from) / (self.to - self.from)).clamp(0.0, 1.0);
        if self.curve == 0.0 {
            return f;
        }
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if bend(mid, self.curve) < f {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }
}

/// A knob, a button or one axis of a pad.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Control {
    /// Set by `control.rename`; without it the control shows its first target's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 0..=1 (a button: 0 or 1).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub value: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<Mapping>,
}

impl Control {
    fn is_default(&self) -> bool {
        self.name.is_none() && self.value == 0.0 && self.maps.is_empty()
    }
}

/// An X/Y pad: two controls moved together.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Pad {
    #[serde(default)]
    pub x: Control,
    #[serde(default)]
    pub y: Control,
}

impl Pad {
    fn is_default(&self) -> bool {
        self.x.is_default() && self.y.is_default()
    }
}

/// The track's set of macro controls. Saved as lists without the unused
/// controls at their ends, and nothing at all while every control is unused.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(from = "Saved", into = "Saved")]
pub struct MacroControls {
    pub knobs: [Control; KNOBS],
    pub buttons: [Control; BUTTONS],
    pub pads: [Pad; PADS],
    /// The page the screen last showed (knobs, buttons or pad).
    pub page: Option<String>,
    /// The knobs (zero-based) the screen's X/Y pad moves across and up; never the same one.
    pub xy: [u8; 2],
}

impl Default for MacroControls {
    fn default() -> MacroControls {
        MacroControls {
            knobs: Default::default(),
            buttons: Default::default(),
            pads: Default::default(),
            page: None,
            xy: XY,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    knobs: Vec<Control>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    buttons: Vec<Control>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pads: Vec<Pad>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    xy: Option<[u8; 2]>,
}

fn trimmed<T: Clone>(xs: &[T], unused: impl Fn(&T) -> bool) -> Vec<T> {
    let n = xs.iter().rposition(|x| !unused(x)).map_or(0, |i| i + 1);
    xs[..n].to_vec()
}

fn filled<T: Default, const N: usize>(xs: Vec<T>) -> [T; N] {
    let mut it = xs.into_iter();
    std::array::from_fn(|_| it.next().unwrap_or_default())
}

impl From<MacroControls> for Saved {
    fn from(m: MacroControls) -> Saved {
        Saved {
            knobs: trimmed(&m.knobs, Control::is_default),
            buttons: trimmed(&m.buttons, Control::is_default),
            pads: trimmed(&m.pads, Pad::is_default),
            page: m.page,
            xy: (m.xy != XY).then_some(m.xy),
        }
    }
}

impl From<Saved> for MacroControls {
    fn from(s: Saved) -> MacroControls {
        let mut m = MacroControls {
            knobs: filled(s.knobs),
            buttons: filled(s.buttons),
            pads: filled(s.pads),
            page: s.page,
            xy: s
                .xy
                .filter(|[x, y]| x != y && usize::from(*x.max(y)) < KNOBS)
                .unwrap_or(XY),
        };
        for id in ControlId::all() {
            let c = m.get_mut(id);
            c.value = if c.value.is_finite() {
                c.value.clamp(0.0, 1.0)
            } else {
                0.0
            };
            if id.is_button() {
                c.value = c.value.round();
            }
            // a hand-edited or older file: a number that is no number takes its default, the
            // rest is held to its range
            for map in &mut c.maps {
                map.from = sane(map.from, 0.0, 0.0, 1.0);
                map.to = sane(map.to, 1.0, 0.0, 1.0);
                map.curve = sane(map.curve, 0.0, -1.0, 1.0);
            }
        }
        // one target, one control: a target two controls name stays the first one's
        let mut owned = Vec::new();
        for id in ControlId::all() {
            m.get_mut(id).maps.retain(|map| {
                let first = !owned.contains(&map.target);
                if first {
                    owned.push(map.target.clone());
                }
                first
            });
        }
        m
    }
}

fn sane(v: f64, or: f64, lo: f64, hi: f64) -> f64 {
    if v.is_finite() { v.clamp(lo, hi) } else { or }
}

impl MacroControls {
    /// Nothing named, set or mapped (the set is not saved).
    pub fn is_empty(&self) -> bool {
        self.page.is_none() && self.xy == XY && ControlId::all().all(|id| self.get(id).is_default())
    }

    pub fn get(&self, id: ControlId) -> &Control {
        match id {
            ControlId::Knob(i) => &self.knobs[i as usize],
            ControlId::Button(i) => &self.buttons[i as usize],
            ControlId::X(i) => &self.pads[i as usize].x,
            ControlId::Y(i) => &self.pads[i as usize].y,
        }
    }

    pub fn get_mut(&mut self, id: ControlId) -> &mut Control {
        match id {
            ControlId::Knob(i) => &mut self.knobs[i as usize],
            ControlId::Button(i) => &mut self.buttons[i as usize],
            ControlId::X(i) => &mut self.pads[i as usize].x,
            ControlId::Y(i) => &mut self.pads[i as usize].y,
        }
    }

    /// Every control with its id, knobs first.
    pub fn iter(&self) -> impl Iterator<Item = (ControlId, &Control)> {
        ControlId::all().map(move |id| (id, self.get(id)))
    }

    /// The knobs the X/Y pad moves: across, then up.
    pub fn xy_knobs(&self) -> [ControlId; 2] {
        self.xy.map(ControlId::Knob)
    }

    /// Puts knob `k` on the pad's axis (`0` across, `1` up); the knob the other axis had
    /// already goes to this axis's old one, so the two stay apart.
    pub fn set_xy(&mut self, axis: usize, k: u8) {
        let other = 1 - axis.min(1);
        if self.xy[other] == k {
            self.xy[other] = self.xy[axis.min(1)];
        }
        self.xy[axis.min(1)] = k;
    }

    /// The control that moves `target`, if one does.
    pub fn owner_of(&self, target: &AutoTarget) -> Option<ControlId> {
        self.iter()
            .find(|(_, c)| c.maps.iter().any(|m| m.target == *target))
            .map(|(id, _)| id)
    }

    /// Does any control move anything?
    pub fn has_maps(&self) -> bool {
        self.iter().any(|(_, c)| !c.maps.is_empty())
    }

    /// Changes every target `f` returns a new one for and drops those it
    /// returns `None` for. Returns how many maps were dropped.
    pub fn retarget(&mut self, mut f: impl FnMut(&AutoTarget) -> Option<AutoTarget>) -> usize {
        let mut dropped = 0;
        for id in ControlId::all() {
            let c = self.get_mut(id);
            let before = c.maps.len();
            c.maps.retain_mut(|m| match f(&m.target) {
                Some(t) => {
                    m.target = t;
                    true
                }
                None => false,
            });
            dropped += before - c.maps.len();
        }
        dropped
    }
}

/// May a control move `target`? The track's volume, pan and sends, plug-in
/// parameters and branch levels; never another macro control.
pub fn can_map(target: &AutoTarget) -> bool {
    matches!(
        target,
        AutoTarget::Volume
            | AutoTarget::Pan
            | AutoTarget::Send(_)
            | AutoTarget::Param(..)
            | AutoTarget::Branch(..)
    )
}

/// The short name of what `target` moves, as a control shows it once mapped.
pub fn target_name(t: &Track, target: &AutoTarget) -> String {
    match target {
        AutoTarget::Volume => "ボリューム".into(),
        AutoTarget::Pan => "パン".into(),
        AutoTarget::Mute => "ミュート".into(),
        AutoTarget::Send(_) => "センド".into(),
        AutoTarget::Param(pid, name) => t
            .inserts
            .iter()
            .find(|x| x.id == *pid)
            .and_then(|x| x.builtin())
            .and_then(|d| d.params.iter().find(|p| p.id == name))
            .map_or_else(|| name.clone(), |p| p.label.to_string()),
        AutoTarget::Branch(n, part) => {
            let part = match part {
                BranchPart::Gain => "レベル",
                BranchPart::Pan => "パン",
                BranchPart::Mute => "ミュート",
            };
            match t.rack.as_ref().and_then(|r| r.node_label(*n)) {
                Some((_, name)) => format!("{name} · {part}"),
                None => format!("分岐 · {part}"),
            }
        }
        AutoTarget::Control(c) => t.control_label(*c),
    }
}

impl Track {
    /// The name the control shows: its own, else its first target's, else `Macro n`.
    pub fn control_label(&self, id: ControlId) -> String {
        let c = self.controls.get(id);
        if let Some(n) = &c.name {
            return n.clone();
        }
        match c.maps.first() {
            Some(m) => target_name(self, &m.target),
            None => id.default_name(),
        }
    }

    /// What the control at `v` sets: every mapped target that still exists, with its value.
    pub fn control_values(&self, id: ControlId, v: f64) -> Vec<(AutoTarget, f64)> {
        self.controls
            .get(id)
            .maps
            .iter()
            .filter(|m| m.target.exists_on(self))
            .map(|m| (m.target.clone(), m.value(v)))
            .collect()
    }

    /// Writes the control's value at `v` into every target it moves.
    pub fn apply_control_at(&mut self, id: ControlId, v: f64) {
        for (target, value) in self.control_values(id, v) {
            target.set_on(self, value);
        }
    }

    /// Writes every control's stored value into its targets.
    pub fn apply_controls(&mut self) {
        for id in ControlId::all() {
            let v = self.controls.get(id).value;
            self.apply_control_at(id, v);
        }
    }

    /// Drops maps whose target the track no longer has. Returns how many.
    pub fn prune_controls(&mut self) -> usize {
        if !self.controls.has_maps() {
            return 0;
        }
        let mut controls = std::mem::take(&mut self.controls);
        let n = controls.retarget(|t| t.exists_on(self).then(|| t.clone()));
        self.controls = controls;
        n
    }
}

impl crate::model::Project {
    /// Drops every map whose target its track no longer has (a plug-in,
    /// send or branch went away). Tracks without such a map are not touched.
    pub fn prune_controls(&mut self) -> usize {
        let mut n = 0;
        for t in self.tracks.values_mut() {
            let dead = t
                .controls
                .iter()
                .any(|(_, c)| c.maps.iter().any(|m| !m.target.exists_on(t)));
            if dead {
                n += std::sync::Arc::make_mut(t).prune_controls();
            }
        }
        n
    }

    /// Drops every lane of a branch or layer its track no longer has (MIX-039): a splitter,
    /// branch or layer that went takes its lanes with it, as its macro maps go.
    pub fn prune_branch_lanes(&mut self) -> usize {
        let mut n = 0;
        for t in self.tracks.values_mut() {
            let dead: Vec<AutoTarget> = t
                .automation
                .iter()
                .filter(|l| matches!(l.target, AutoTarget::Branch(..)) && !l.target.exists_on(t))
                .map(|l| l.target.clone())
                .collect();
            if dead.is_empty() {
                continue;
            }
            n += dead.len();
            std::sync::Arc::make_mut(t)
                .automation
                .retain(|l| !dead.contains(&l.target));
        }
        n
    }

    /// Writes each control's value into what it moves, as a song is read: a song saved with a
    /// target apart from its control plays the control's value, so it keeps that one too
    /// (MIX-039). Tracks already so are not touched.
    pub fn settle_controls(&mut self) {
        for t in self.tracks.values_mut() {
            if !t.controls.has_maps() {
                continue;
            }
            let mut heard = (**t).clone();
            heard.apply_controls();
            if heard != **t {
                *t = std::sync::Arc::new(heard);
            }
        }
    }

    /// Puts back what an edit moved of a target a macro control moves (MIX-039): what the
    /// song keeps and what plays would part, and the next lane or reopen would jump back. An
    /// edit that changed a control itself (`control.*`, a preset) sets both, so that control's
    /// targets stand; so does a target that was already apart and was not touched. What is put
    /// back is the value the control gives it. Returns what was put back, as
    /// "Vocal のボリューム（K1）".
    pub fn hold_owned(&mut self, before: &crate::model::Project) -> Vec<String> {
        let mut held = Vec::new();
        for (id, t) in self.tracks.iter_mut() {
            if !t.controls.has_maps() {
                continue;
            }
            let Some(b) = before.tracks.get(id) else {
                continue;
            };
            // what a changed control moves stands, even where another control moves it too
            let set: Vec<&AutoTarget> = t
                .controls
                .iter()
                .filter(|(k, c)| b.controls.get(*k) != *c)
                .flat_map(|(_, c)| c.maps.iter().map(|m| &m.target))
                .collect();
            let mut put = Vec::new();
            for (k, c) in t.controls.iter() {
                if b.controls.get(k) != c {
                    continue;
                }
                for m in c.maps.iter().filter(|m| !set.contains(&&m.target)) {
                    let now = m.target.stored_value(t);
                    if now == m.target.stored_value(b) {
                        continue;
                    }
                    let mut heard = (**t).clone();
                    heard.apply_control_at(k, c.value);
                    if let Some(v) = m.target.stored_value(&heard)
                        && Some(v) != now
                    {
                        put.push((k, m.target.clone(), v));
                    }
                }
            }
            if put.is_empty() {
                continue;
            }
            let t = std::sync::Arc::make_mut(t);
            for (k, target, v) in put {
                held.push(format!(
                    "{} の{}（{}）",
                    t.name,
                    target_name(t, &target),
                    k.to_string().to_uppercase()
                ));
                target.set_on(t, v);
            }
        }
        held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_read_back_from_their_text() {
        let all: Vec<_> = ControlId::all().collect();
        assert_eq!(all.len(), KNOBS + BUTTONS + 2 * PADS);
        for id in all {
            assert_eq!(id.to_string().parse::<ControlId>(), Ok(id));
        }
        for bad in ["k0", "k9", "b9", "x3", "y0", "z1", "", "k"] {
            assert!(bad.parse::<ControlId>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_map_runs_from_from_to_to_along_its_curve() {
        let mut m = Mapping::new(AutoTarget::Pan);
        m.from = 0.25;
        m.to = 0.75;
        assert!((m.value(0.0) - -0.5).abs() < 1e-9);
        assert!((m.value(1.0) - 0.5).abs() < 1e-9);
        assert!((m.value(0.5) - 0.0).abs() < 1e-9);
        // backwards
        m.from = 1.0;
        m.to = 0.0;
        assert!((m.value(0.0) - 1.0).abs() < 1e-9);
        // a bulging curve runs ahead of the straight line
        m.from = 0.0;
        m.to = 1.0;
        m.curve = 1.0;
        assert!(m.value(0.5) > 0.0);
        assert!((m.inverse(AutoTarget::Pan.to_norm(m.value(0.3))) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn unused_controls_are_not_saved() {
        let mut m = MacroControls::default();
        assert!(m.is_empty());
        assert_eq!(serde_json::to_value(&m).unwrap(), serde_json::json!({}));
        m.get_mut(ControlId::Knob(2)).value = 0.5;
        m.get_mut(ControlId::Y(0)).name = Some("Air".into());
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["knobs"].as_array().unwrap().len(), 3);
        assert!(v.get("buttons").is_none());
        assert_eq!(v["pads"].as_array().unwrap().len(), 1);
        let back: MacroControls = serde_json::from_value(v).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn the_pads_knobs_stay_apart_and_are_saved_once_chosen() {
        let mut m = MacroControls::default();
        assert_eq!(m.xy_knobs(), [ControlId::Knob(0), ControlId::Knob(1)]);
        m.set_xy(0, 4);
        assert_eq!(m.xy, [4, 1]);
        // the other axis's knob: the two trade places
        m.set_xy(1, 4);
        assert_eq!(m.xy, [1, 4]);
        assert!(!m.is_empty());
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, serde_json::json!({"xy": [1, 4]}));
        assert_eq!(serde_json::from_value::<MacroControls>(v).unwrap(), m);
        for bad in [
            serde_json::json!({"xy": [2, 2]}),
            serde_json::json!({"xy": [0, 8]}),
        ] {
            assert_eq!(serde_json::from_value::<MacroControls>(bad).unwrap().xy, XY);
        }
    }

    #[test]
    fn a_long_or_bad_list_reads_into_the_fixed_set() {
        let v = serde_json::json!({
            "knobs": [{"value": 4.0}, {"value": -1}, {}, {}, {}, {}, {}, {}, {"value": 0.5}],
            "buttons": [{"value": 0.7}],
        });
        let m: MacroControls = serde_json::from_value(v).unwrap();
        assert_eq!(m.knobs[0].value, 1.0);
        assert_eq!(m.knobs[1].value, 0.0);
        assert_eq!(m.buttons[0].value, 1.0, "a button is off or on");
    }

    #[test]
    fn only_track_targets_may_be_mapped() {
        assert!(can_map(&AutoTarget::Volume));
        assert!(can_map(&"branch:nod_2:gain".parse().unwrap()));
        assert!(!can_map(&AutoTarget::Mute));
    }

    #[test]
    fn a_control_lane_reads_back_and_moves_what_the_control_moves() {
        let k1: AutoTarget = "control:k1".parse().unwrap();
        assert_eq!(k1, AutoTarget::Control(ControlId::Knob(0)));
        assert_eq!(k1.to_string(), "control:k1");
        assert_eq!(k1.range(), (0.0, 1.0));
        assert!(!k1.stepped());
        let b2: AutoTarget = "control:b2".parse().unwrap();
        assert!(b2.stepped());
        assert!("control:k9".parse::<AutoTarget>().is_err());

        let mut t =
            crate::model::Track::new(crate::id::TrackId(1), crate::model::TrackKind::Audio, "A");
        let mut m = Mapping::new(AutoTarget::Pan);
        (m.from, m.to) = (0.5, 1.0);
        t.controls.get_mut(ControlId::Knob(0)).maps.push(m);
        assert!(k1.exists_on(&t), "every control is there to write");
        k1.set_on(&mut t, 1.0);
        assert_eq!(t.controls.get(ControlId::Knob(0)).value, 1.0);
        assert!((t.pan - 1.0).abs() < 1e-9, "{}", t.pan);
        b2.set_on(&mut t, 0.6);
        assert_eq!(t.controls.get(ControlId::Button(1)).value, 1.0);
        // the lane is saved with the track and named by the control
        let mut lane = crate::auto::AutoLane::new(k1.clone());
        lane.points = std::sync::Arc::new(vec![crate::auto::AutoPoint::new(0, 0.25)]);
        t.automation.push(lane);
        let back: crate::model::Track =
            serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.automation[0].target, k1);
        assert_eq!(back.automation[0].points[0].value, 0.25);
        assert_eq!(target_name(&back, &k1), "パン");
    }
}
