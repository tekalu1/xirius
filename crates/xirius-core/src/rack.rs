//! The rack (PRJ-026): how a track's inserts are really wired. A track with
//! no `rack` plays its `inserts` in order, as before. A rack appears the
//! first time a split or a multi-instrument is placed, and goes away when
//! everything is put back into a plain line.
//!
//! The rack is only an ordering overlay: every plug-in still lives in
//! `Track::inserts`, so the parts of the app that do not know about nesting
//! (CPU lists, crash recovery, freezing, process isolation, DAWproject,
//! track presets) keep working unchanged. Every id of `inserts` appears
//! exactly once in the rack; [`Track::fix_rack`] makes that true for a song
//! another version or a hand edit left inconsistent, reporting every change
//! it makes the way PRJ-007 reads presets.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::id::{NodeId, PluginId};
use crate::model::{PluginKind, Track, TrackKind};
use crate::track_preset::{Adjust, AdjustKind};

/// A split holds no more than this many branches.
pub const MAX_BRANCHES: usize = 5;
/// A split holds no less than this many branches; fewer is folded into a line.
pub const MIN_BRANCHES: usize = 2;
/// How deep splits may nest inside each chain (the track's fx and each
/// layer's fx each count from zero).
pub const MAX_SPLIT_DEPTH: usize = 4;

/// The default crossover frequencies for a split of `branches` branches
/// (ENG-076): two branches split at 1k, three at 200 and 2k, and so on.
pub fn default_crossovers(branches: usize) -> &'static [f64] {
    match branches {
        2 => &[1000.0],
        3 => &[200.0, 2000.0],
        4 => &[150.0, 800.0, 4000.0],
        _ => &[100.0, 400.0, 1600.0, 6000.0],
    }
}

/// How a split feeds its branches.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitMode {
    /// The same sound to every branch.
    #[default]
    Normal,
    /// Left/right or mid/side; always two branches (ENG-076).
    Channel,
    /// One band per branch, split at `crossovers` (ENG-076).
    Frequency,
}

/// One branch of a split: its own insert chain, then gain and pan at the merge.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Branch {
    pub id: NodeId,
    #[serde(default)]
    pub fx: Vec<RackNode>,
    /// Gain just before the merge, in dB.
    #[serde(default)]
    pub gain_db: f64,
    /// -1 (left) ..= 1 (right), applied at the merge.
    #[serde(default)]
    pub pan: f64,
    /// The whole branch is silent (automatable).
    #[serde(default)]
    pub mute: bool,
}

/// A point the sound splits at and merges back after.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Split {
    pub id: NodeId,
    #[serde(default)]
    pub mode: SplitMode,
    /// The band boundaries in Hz for [`SplitMode::Frequency`]
    /// (`branches.len() - 1` of them, ascending); empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crossovers: Vec<f64>,
    /// 2..=5 branches.
    #[serde(default)]
    pub branches: Vec<Branch>,
}

/// One element of an fx chain: a plug-in or a split.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RackNode {
    Plugin(PluginId),
    Split(Split),
}

/// One layer of a multi-instrument: an instrument, its own fx chain and the
/// notes it plays.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub id: NodeId,
    #[serde(default)]
    pub name: String,
    pub instrument: PluginId,
    /// The layer's own fx chain; splits are allowed here too.
    #[serde(default)]
    pub fx: Vec<RackNode>,
    /// The key range, `low..=high` (0..=127).
    #[serde(default = "full_keys")]
    pub keys: (u8, u8),
    /// The velocity range, `low..=high` (1..=127).
    #[serde(default = "full_velocity")]
    pub velocity: (u8, u8),
    #[serde(default)]
    pub transpose: i8,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub mute: bool,
}

fn full_keys() -> (u8, u8) {
    (0, 127)
}

fn full_velocity() -> (u8, u8) {
    (1, 127)
}

/// Where an instrument track's sound comes from.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// One instrument, as a plain instrument track has.
    Single(PluginId),
    /// A multi-instrument: every layer's instrument plays the notes its
    /// ranges take. A multi never holds another multi.
    Multi { id: NodeId, layers: Vec<Layer> },
}

/// The ordering overlay over `Track::inserts` (PRJ-026).
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Rack {
    /// The sound source; only an instrument track has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// The chain behind the source (or behind the input of an audio track).
    #[serde(default)]
    pub fx: Vec<RackNode>,
}

impl SplitMode {
    fn name(self) -> &'static str {
        match self {
            SplitMode::Normal => "normal",
            SplitMode::Channel => "channel",
            SplitMode::Frequency => "frequency",
        }
    }
}

impl RackNode {
    /// `v` as this node's JSON for `rack.show`: plug-ins by id and name,
    /// splits as their whole tree.
    fn to_json(&self, t: &Track) -> Value {
        match self {
            RackNode::Plugin(id) => json!({
                "plugin": id.to_string(),
                "name": t.inserts.iter().find(|x| x.id == *id).map(|x| x.name.as_str()),
            }),
            RackNode::Split(s) => json!({ "split": s.to_json(t) }),
        }
    }
}

impl Split {
    fn to_json(&self, t: &Track) -> Value {
        json!({
            "id": self.id.to_string(),
            "mode": self.mode.name(),
            "crossovers": self.crossovers,
            "branches": self.branches.iter().map(|b| json!({
                "id": b.id.to_string(),
                "gain_db": b.gain_db,
                "pan": b.pan,
                "mute": b.mute,
                "fx": b.fx.iter().map(|n| n.to_json(t)).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

impl Layer {
    fn to_json(&self, t: &Track) -> Value {
        json!({
            "id": self.id.to_string(),
            "name": self.name,
            "instrument": self.instrument.to_string(),
            "instrument_name": t.inserts.iter().find(|x| x.id == self.instrument).map(|x| x.name.as_str()),
            "keys": [self.keys.0, self.keys.1],
            "velocity": [self.velocity.0, self.velocity.1],
            "transpose": self.transpose,
            "gain_db": self.gain_db,
            "pan": self.pan,
            "mute": self.mute,
            "fx": self.fx.iter().map(|n| n.to_json(t)).collect::<Vec<_>>(),
        })
    }
}

impl Source {
    fn to_json(&self, t: &Track) -> Value {
        match self {
            Source::Single(id) => json!({
                "single": id.to_string(),
                "name": t.inserts.iter().find(|x| x.id == *id).map(|x| x.name.as_str()),
            }),
            Source::Multi { id, layers } => json!({
                "multi": {
                    "id": id.to_string(),
                    "layers": layers.iter().map(|l| l.to_json(t)).collect::<Vec<_>>(),
                },
            }),
        }
    }
}

impl Rack {
    /// A rack that plays exactly like the serial `inserts` it is made from.
    /// For an instrument track the (first) instrument becomes the source.
    pub fn serial(t: &Track) -> Rack {
        let instrument = t
            .inserts
            .iter()
            .find(|x| x.kind == PluginKind::Instrument)
            .map(|x| x.id);
        Rack {
            source: instrument
                .filter(|_| t.kind == TrackKind::Instrument)
                .map(Source::Single),
            fx: t
                .inserts
                .iter()
                .filter(|x| Some(x.id) != instrument)
                .map(|x| RackNode::Plugin(x.id))
                .collect(),
        }
    }

    /// True when the rack holds no split and no multi-instrument: it then
    /// says nothing a plain order of `inserts` would not.
    pub fn is_serial(&self) -> bool {
        !matches!(self.source, Some(Source::Multi { .. }))
            && self.fx.iter().all(|n| matches!(n, RackNode::Plugin(_)))
    }

    /// Every plug-in the rack references, in the order a plain line would
    /// play them: the source's instruments first, then every effect,
    /// depth-first (a split's branches in order).
    pub fn flatten_order(&self) -> Vec<PluginId> {
        let mut out = Vec::new();
        match &self.source {
            Some(Source::Single(id)) => out.push(*id),
            Some(Source::Multi { layers, .. }) => {
                for l in layers {
                    out.push(l.instrument);
                }
                for l in layers {
                    flatten_nodes(&l.fx, &mut out);
                }
            }
            None => {}
        }
        flatten_nodes(&self.fx, &mut out);
        out
    }

    /// Sorts `t.inserts` into [`Rack::flatten_order`]: plug-ins the rack does
    /// not name keep their relative order at the end.
    fn flatten_into(&self, t: &mut Track) {
        let pos: HashMap<PluginId, usize> = self
            .flatten_order()
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
        t.inserts
            .sort_by_key(|x| pos.get(&x.id).copied().unwrap_or(usize::MAX));
    }

    /// Whether any part of the rack references `id`.
    pub fn uses_plugin(&self, id: PluginId) -> bool {
        self.flatten_order().contains(&id)
    }

    /// Removes `id` wherever it appears: an fx node, a source or a layer's
    /// instrument. An emptied layer or multi goes with it.
    pub fn remove_plugin(&mut self, id: PluginId) -> bool {
        let mut gone = remove_from_nodes(&mut self.fx, id);
        if let Some(src) = self.source.take() {
            self.source = match src {
                Source::Single(x) if x == id => {
                    gone = true;
                    None
                }
                Source::Single(x) => Some(Source::Single(x)),
                Source::Multi {
                    id: multi,
                    mut layers,
                } => {
                    let before = layers.len();
                    layers.retain_mut(|l| {
                        gone |= remove_from_nodes(&mut l.fx, id);
                        l.instrument != id
                    });
                    gone |= layers.len() != before;
                    if layers.is_empty() {
                        None
                    } else {
                        Some(Source::Multi { id: multi, layers })
                    }
                }
            };
        }
        gone
    }

    /// Points every appearance of `from` at `to` (the plug-in a swap or a
    /// duplicate put in its place). Returns whether anything changed.
    pub fn replace_plugin(&mut self, from: PluginId, to: PluginId) -> bool {
        let mut hit = replace_in_nodes(&mut self.fx, from, to);
        if let Some(src) = &mut self.source {
            match src {
                Source::Single(x) if *x == from => {
                    *x = to;
                    hit = true;
                }
                Source::Single(_) => {}
                Source::Multi { layers, .. } => {
                    for l in layers {
                        if l.instrument == from {
                            l.instrument = to;
                            hit = true;
                        }
                        hit |= replace_in_nodes(&mut l.fx, from, to);
                    }
                }
            }
        }
        hit
    }

    /// `plugins` old → new, and fresh node ids from `alloc` (a duplicated
    /// track's rack gets ids of its own).
    pub fn remap_ids(
        &mut self,
        plugins: &HashMap<PluginId, PluginId>,
        alloc: &mut dyn FnMut() -> u64,
    ) {
        let mut nodes = |id: &mut NodeId| *id = NodeId(alloc());
        match &mut self.source {
            Some(Source::Single(x)) => {
                if let Some(n) = plugins.get(x) {
                    *x = *n;
                }
            }
            Some(Source::Multi { id, layers }) => {
                nodes(id);
                for l in layers {
                    nodes(&mut l.id);
                    if let Some(n) = plugins.get(&l.instrument) {
                        l.instrument = *n;
                    }
                    remap_nodes(&mut l.fx, plugins, &mut nodes);
                }
            }
            None => {}
        }
        remap_nodes(&mut self.fx, plugins, &mut nodes);
    }

    /// Runs `f` on every node id of the rack (multi, layers, splits,
    /// branches), for renumbering.
    pub fn each_node_id_mut(&mut self, f: &mut dyn FnMut(&mut NodeId)) {
        if let Some(Source::Multi { id, layers }) = &mut self.source {
            f(id);
            for l in layers {
                f(&mut l.id);
                each_node_id_in(&mut l.fx, f);
            }
        }
        each_node_id_in(&mut self.fx, f);
    }

    /// Every node id of the rack (multi, layers, splits, branches).
    pub fn node_ids(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        if let Some(Source::Multi { id, layers }) = &self.source {
            out.push(*id);
            for l in layers {
                out.push(l.id);
                node_ids_of(&l.fx, &mut out);
            }
        }
        node_ids_of(&self.fx, &mut out);
        out
    }

    /// The tree `rack.show` answers.
    pub fn to_json(&self, t: &Track, implicit: bool) -> Value {
        json!({
            "track": t.id.to_string(),
            "implicit": implicit,
            "source": self.source.as_ref().map(|s| s.to_json(t)),
            "fx": self.fx.iter().map(|n| n.to_json(t)).collect::<Vec<_>>(),
        })
    }
}

fn flatten_nodes(nodes: &[RackNode], out: &mut Vec<PluginId>) {
    for n in nodes {
        match n {
            RackNode::Plugin(id) => out.push(*id),
            RackNode::Split(s) => {
                for b in &s.branches {
                    flatten_nodes(&b.fx, out);
                }
            }
        }
    }
}

fn each_node_id_in(nodes: &mut [RackNode], f: &mut dyn FnMut(&mut NodeId)) {
    for n in nodes {
        if let RackNode::Split(s) = n {
            f(&mut s.id);
            for b in &mut s.branches {
                f(&mut b.id);
                each_node_id_in(&mut b.fx, f);
            }
        }
    }
}

fn node_ids_of(nodes: &[RackNode], out: &mut Vec<NodeId>) {
    for n in nodes {
        if let RackNode::Split(s) = n {
            out.push(s.id);
            for b in &s.branches {
                out.push(b.id);
                node_ids_of(&b.fx, out);
            }
        }
    }
}

fn remove_from_nodes(nodes: &mut Vec<RackNode>, id: PluginId) -> bool {
    let before = nodes.len();
    nodes.retain_mut(|n| match n {
        RackNode::Plugin(x) => *x != id,
        RackNode::Split(s) => {
            for b in &mut s.branches {
                remove_from_nodes(&mut b.fx, id);
            }
            true
        }
    });
    nodes.len() != before
}

fn replace_in_nodes(nodes: &mut [RackNode], from: PluginId, to: PluginId) -> bool {
    let mut hit = false;
    for n in nodes {
        match n {
            RackNode::Plugin(x) if *x == from => {
                *x = to;
                hit = true;
            }
            RackNode::Plugin(_) => {}
            RackNode::Split(s) => {
                for b in &mut s.branches {
                    hit |= replace_in_nodes(&mut b.fx, from, to);
                }
            }
        }
    }
    hit
}

fn remap_nodes(
    nodes: &mut [RackNode],
    plugins: &HashMap<PluginId, PluginId>,
    alloc: &mut dyn FnMut(&mut NodeId),
) {
    for n in nodes {
        match n {
            RackNode::Plugin(x) => {
                if let Some(new) = plugins.get(x) {
                    *x = *new;
                }
            }
            RackNode::Split(s) => {
                alloc(&mut s.id);
                for b in &mut s.branches {
                    alloc(&mut b.id);
                    remap_nodes(&mut b.fx, plugins, alloc);
                }
            }
        }
    }
}

impl Track {
    /// Flattens the rack back into `inserts` and drops it: the track saves
    /// and plays exactly as a serial track again.
    pub fn flatten_rack(&mut self) {
        let Some(rack) = self.rack.take() else {
            return;
        };
        rack.flatten_into(self);
    }

    /// What `rack.show` answers: the rack itself, or the serial order it
    /// would have (`implicit` then is true).
    pub fn rack_json(&self) -> Value {
        match &self.rack {
            Some(r) => r.to_json(self, false),
            None => Rack::serial(self).to_json(self, true),
        }
    }

    /// Makes the rack agree with `inserts`, the way a song is read
    /// (PRJ-007): a plug-in the rack does not name is appended at the end of
    /// the top chain (`Filled`), a rack entry pointing nowhere is left out
    /// (`Dropped`), a plug-in appearing twice keeps its first place
    /// (`Dropped`), an effect named as a source or an instrument inside an
    /// fx chain is turned down (`Rejected`), and a track that is not an
    /// instrument track loses its source and its instruments (`Rejected`).
    /// `inserts` is then kept in the flattened order, so the plain serial
    /// playback the engine still does is the rack's projection. A rack left
    /// with no split and no multi is folded away entirely.
    ///
    /// The rows say what was changed; the file never silently loses a
    /// plug-in.
    ///
    /// `alloc` hands out fresh ids (from the project's counter) for nodes the
    /// fix creates, so a filled layer never gets a placeholder id.
    pub fn fix_rack(&mut self, alloc: &mut dyn FnMut() -> u64) -> Vec<Adjust> {
        let Some(mut rack) = self.rack.take() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        let mut seen: HashSet<PluginId> = HashSet::new();
        let plugin = |id: PluginId| self.inserts.iter().find(|x| x.id == id);
        let instrument_track = self.kind == TrackKind::Instrument;

        if !instrument_track && rack.source.take().is_some() {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                "ラックの音源",
                "インストゥルメントトラックではないため、外しました",
            ));
        }
        if let Some(src) = rack.source.take() {
            rack.source = match src {
                Source::Single(id) => match plugin(id) {
                    Some(x) if x.kind == PluginKind::Instrument => {
                        seen.insert(id);
                        Some(Source::Single(id))
                    }
                    Some(x) => {
                        rows.push(Adjust::new(
                            AdjustKind::Rejected,
                            x.name.clone(),
                            "インストゥルメントではないため、音源から外しました",
                        ));
                        None
                    }
                    None => {
                        rows.push(Adjust::new(
                            AdjustKind::Dropped,
                            format!("音源 {id}"),
                            "インサートに無いため、外しました",
                        ));
                        None
                    }
                },
                Source::Multi { id, layers } => {
                    let mut kept = Vec::new();
                    for mut l in layers {
                        match plugin(l.instrument) {
                            Some(x) if x.kind == PluginKind::Instrument => {
                                if !seen.insert(l.instrument) {
                                    rows.push(Adjust::new(
                                        AdjustKind::Dropped,
                                        format!("レイヤー「{}」", x.name),
                                        "同じインストゥルメントが 2 回出るため、外しました",
                                    ));
                                    continue;
                                }
                                if l.name.trim().is_empty() {
                                    l.name = x.name.clone();
                                }
                                l.keys = (l.keys.0.min(127), l.keys.1.min(127));
                                if l.keys.0 > l.keys.1 {
                                    rows.push(Adjust::new(
                                        AdjustKind::Rounded,
                                        format!("{} の鍵盤の範囲", l.name),
                                        "下限が上限を越えるため、全音域にしました",
                                    ));
                                    l.keys = full_keys();
                                }
                                l.velocity =
                                    (l.velocity.0.clamp(1, 127), l.velocity.1.clamp(1, 127));
                                if l.velocity.0 > l.velocity.1 {
                                    rows.push(Adjust::new(
                                        AdjustKind::Rounded,
                                        format!("{} のベロシティの範囲", l.name),
                                        "下限が上限を越えるため、全域にしました",
                                    ));
                                    l.velocity = full_velocity();
                                }
                                l.fx = fix_nodes(
                                    std::mem::take(&mut l.fx),
                                    &self.inserts,
                                    &mut seen,
                                    &mut rows,
                                    &format!("レイヤー「{}」", l.name),
                                    0,
                                );
                                kept.push(l);
                            }
                            Some(x) => {
                                rows.push(Adjust::new(
                                    AdjustKind::Rejected,
                                    x.name.clone(),
                                    "インストゥルメントではないため、レイヤーから外しました",
                                ));
                            }
                            None => {
                                let label = if l.name.trim().is_empty() {
                                    "レイヤー".to_string()
                                } else {
                                    format!("レイヤー「{}」", l.name)
                                };
                                rows.push(Adjust::new(
                                    AdjustKind::Dropped,
                                    label,
                                    "インサートに無いため、レイヤーを外しました",
                                ));
                            }
                        }
                    }
                    if kept.is_empty() {
                        rows.push(Adjust::new(
                            AdjustKind::Dropped,
                            "マルチインストゥルメント",
                            "使えるレイヤーが無いため、外しました",
                        ));
                        None
                    } else {
                        Some(Source::Multi { id, layers: kept })
                    }
                }
            };
        }
        rack.fx = fix_nodes(
            std::mem::take(&mut rack.fx),
            &self.inserts,
            &mut seen,
            &mut rows,
            "FX",
            0,
        );

        // What the rack does not name goes back in: effects at the end of
        // the top chain, instruments into a free source. An instrument with
        // no free seat is turned down like a second one on a plain track.
        let mut inserts = Vec::with_capacity(self.inserts.len());
        let mut filled = Vec::new();
        for x in std::mem::take(&mut self.inserts) {
            if seen.contains(&x.id) {
                inserts.push(x);
                continue;
            }
            match x.kind {
                PluginKind::Effect => {
                    seen.insert(x.id);
                    rows.push(Adjust::new(
                        AdjustKind::Filled,
                        x.name.clone(),
                        "ラックに無いため、FX の末尾に足しました",
                    ));
                    filled.push(RackNode::Plugin(x.id));
                    inserts.push(x);
                }
                PluginKind::Instrument => {
                    let name = x.name.clone();
                    match (instrument_track, &mut rack.source) {
                        (true, None) => {
                            seen.insert(x.id);
                            rows.push(Adjust::new(
                                AdjustKind::Filled,
                                name,
                                "ラックに無いため、音源にしました",
                            ));
                            rack.source = Some(Source::Single(x.id));
                            inserts.push(x);
                        }
                        (true, Some(Source::Multi { layers, .. })) => {
                            seen.insert(x.id);
                            rows.push(Adjust::new(
                                AdjustKind::Filled,
                                name.clone(),
                                "ラックに無いため、レイヤーに足しました",
                            ));
                            layers.push(Layer {
                                id: NodeId(alloc()),
                                name,
                                instrument: x.id,
                                fx: Vec::new(),
                                keys: full_keys(),
                                velocity: full_velocity(),
                                transpose: 0,
                                gain_db: 0.0,
                                pan: 0.0,
                                mute: false,
                            });
                            inserts.push(x);
                        }
                        _ => {
                            rows.push(Adjust::new(
                                AdjustKind::Rejected,
                                name,
                                "音源の席が無いため、外しました",
                            ));
                        }
                    }
                }
            }
        }
        rack.fx.extend(filled);
        self.inserts = inserts;

        // Nothing but a line left folds back, so the file writes as a plain
        // track again.
        rack.flatten_into(self);
        self.rack = if rack.is_serial() { None } else { Some(rack) };
        rows
    }
}

/// One chain of the rack, made consistent: unknown or duplicate plug-ins,
/// instruments misplaced in fx seats, splits too deep or malformed, and
/// ranges out of their bounds.
fn fix_nodes(
    nodes: Vec<RackNode>,
    inserts: &[crate::model::Plugin],
    seen: &mut HashSet<PluginId>,
    rows: &mut Vec<Adjust>,
    at: &str,
    depth: usize,
) -> Vec<RackNode> {
    let plugin = |id: PluginId| inserts.iter().find(|x| x.id == id);
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        match n {
            RackNode::Plugin(id) => match plugin(id) {
                Some(x) if x.kind != PluginKind::Instrument => {
                    if !seen.insert(id) {
                        rows.push(Adjust::new(
                            AdjustKind::Dropped,
                            x.name.clone(),
                            "2 回出るため、後ろを外しました",
                        ));
                    } else {
                        out.push(RackNode::Plugin(id));
                    }
                }
                Some(x) => {
                    rows.push(Adjust::new(
                        AdjustKind::Rejected,
                        x.name.clone(),
                        format!("インストゥルメントは {at} に置けないため、外しました"),
                    ));
                }
                None => {
                    rows.push(Adjust::new(
                        AdjustKind::Dropped,
                        format!("{at} のプラグイン {id}"),
                        "インサートに無いため、外しました",
                    ));
                }
            },
            RackNode::Split(mut s) => {
                if depth >= MAX_SPLIT_DEPTH {
                    rows.push(Adjust::new(
                        AdjustKind::Dropped,
                        format!("{at} のスプリッター"),
                        format!("入れ子は {MAX_SPLIT_DEPTH} 段までのため、外しました"),
                    ));
                    continue;
                }
                // Branches are cut before their fx is walked: whatever they
                // held is appended to this chain, so it stays in the file.
                let mut extra: Vec<RackNode> = Vec::new();
                if s.branches.len() > MAX_BRANCHES {
                    rows.push(Adjust::new(
                        AdjustKind::Dropped,
                        format!("{at} のスプリッターの分岐"),
                        format!(
                            "{MAX_BRANCHES} 本までのため、{} 本を外しました",
                            s.branches.len() - MAX_BRANCHES
                        ),
                    ));
                    for b in s.branches.split_off(MAX_BRANCHES) {
                        extra.extend(b.fx);
                    }
                }
                if s.mode == SplitMode::Channel && s.branches.len() > 2 {
                    rows.push(Adjust::new(
                        AdjustKind::Dropped,
                        format!("{at} のチャンネル分割の分岐"),
                        "2 本までのため、余分を外しました",
                    ));
                    for b in s.branches.split_off(2) {
                        extra.extend(b.fx);
                    }
                }
                for b in &mut s.branches {
                    b.fx = fix_nodes(
                        std::mem::take(&mut b.fx),
                        inserts,
                        seen,
                        rows,
                        "分岐",
                        depth + 1,
                    );
                }
                match s.branches.len() {
                    0 => {
                        rows.push(Adjust::new(
                            AdjustKind::Dropped,
                            format!("{at} のスプリッター"),
                            "分岐が無いため、外しました",
                        ));
                    }
                    1 => {
                        rows.push(Adjust::new(
                            AdjustKind::Filled,
                            format!("{at} のスプリッター"),
                            "分岐が 1 本のため、直列に展開しました",
                        ));
                        out.extend(s.branches.pop().expect("one branch").fx);
                    }
                    _ => {
                        fix_crossovers(&mut s, rows, at);
                        out.push(RackNode::Split(s));
                    }
                }
                out.extend(fix_nodes(extra, inserts, seen, rows, at, depth));
            }
        }
    }
    out
}

/// A kept split's `crossovers` made to match its mode: Frequency gets
/// `branches - 1` ascending boundaries, the other modes get none.
fn fix_crossovers(s: &mut Split, rows: &mut Vec<Adjust>, at: &str) {
    if s.mode != SplitMode::Frequency {
        if !s.crossovers.is_empty() {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                format!("{at} のスプリッターのクロスオーバー"),
                "この分割では使わないため、外しました",
            ));
            s.crossovers.clear();
        }
        return;
    }
    let need = s.branches.len() - 1;
    s.crossovers.retain(|f| f.is_finite() && *f > 0.0);
    if s.crossovers.len() > need {
        rows.push(Adjust::new(
            AdjustKind::Dropped,
            format!("{at} のクロスオーバー"),
            format!("{need} 個までのため、余分を外しました"),
        ));
        s.crossovers.truncate(need);
    }
    let defaults = default_crossovers(s.branches.len());
    let mut last = 0.0_f64;
    let mut fixed = false;
    for (i, f) in s.crossovers.iter_mut().enumerate() {
        let d = defaults[i.min(defaults.len() - 1)];
        if *f <= last {
            *f = (last * 2.0).max(d);
            fixed = true;
        }
        last = *f;
    }
    while s.crossovers.len() < need {
        s.crossovers
            .push(defaults[s.crossovers.len().min(defaults.len() - 1)].max(last * 2.0));
        last = *s.crossovers.last().expect("just pushed");
        fixed = true;
    }
    if fixed {
        rows.push(Adjust::new(
            AdjustKind::Filled,
            format!("{at} のクロスオーバー"),
            "個数または順序が合わないため、直しました",
        ));
    }
}

impl crate::model::Project {
    /// [`Track::fix_rack`] on every track; the rows name the track. Node ids
    /// that the file repeats are re-allocated, and every id the fix hands out
    /// comes from `next_id`, so nothing shadows another node's name.
    pub fn fix_racks(&mut self) -> Vec<Adjust> {
        // `next` never goes under any node id the file already holds, so a
        // renumbered or filled node cannot collide with a kept one.
        let mut next = self.next_id;
        for t in self.tracks.values() {
            if let Some(r) = &t.rack {
                for n in r.node_ids() {
                    next = next.max(n.0 + 1);
                }
            }
        }
        let mut seen_ids: HashSet<u64> = HashSet::new();
        let mut rows = Vec::new();
        for t in self.tracks.values_mut() {
            if t.rack.is_none() {
                continue;
            }
            let name = t.name.clone();
            let t = std::sync::Arc::make_mut(t);
            let mut reused = 0usize;
            if let Some(r) = &mut t.rack {
                r.each_node_id_mut(&mut |id| {
                    if !seen_ids.insert(id.0) {
                        *id = NodeId(next);
                        seen_ids.insert(next);
                        next += 1;
                        reused += 1;
                    }
                });
            }
            if reused > 0 {
                rows.push(Adjust::new(
                    AdjustKind::Rounded,
                    format!("{name} のラック"),
                    format!("ノードの ID が重複したため、{reused} 件採り直しました"),
                ));
            }
            for a in t.fix_rack(&mut || {
                let n = next;
                next += 1;
                n
            }) {
                rows.push(Adjust::new(a.kind, format!("{name}: {}", a.what), a.reason));
            }
        }
        self.next_id = self.next_id.max(next);
        rows
    }

    /// [`Track::fix_rack`] on the one track a command just edited. Node ids
    /// the fix needs are allocated from the project's id counter.
    pub fn fix_track_rack(&mut self, tid: crate::id::TrackId) -> Vec<Adjust> {
        let mut next = self.next_id;
        let rows = match self.tracks.get_mut(&tid) {
            Some(t) => std::sync::Arc::make_mut(t).fix_rack(&mut || {
                let n = next;
                next += 1;
                n
            }),
            None => Vec::new(),
        };
        self.next_id = self.next_id.max(next);
        rows
    }

    /// One line for the session's notices, or none.
    pub fn rack_notice(rows: &[Adjust]) -> Option<String> {
        if rows.is_empty() {
            return None;
        }
        let mut parts: Vec<String> = rows
            .iter()
            .take(4)
            .map(|a| format!("{} — {}", a.what, a.reason))
            .collect();
        if rows.len() > 4 {
            parts.push(format!("ほか {} 件", rows.len() - 4));
        }
        Some(format!(
            "ラックの読み込みを {} 件調整しました（{}）",
            rows.len(),
            parts.join("・")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Plugin, PluginKind, TrackKind};

    fn fresh_id() -> impl FnMut() -> u64 {
        let mut n = 9_000u64;
        move || {
            n += 1;
            n
        }
    }

    fn plugin(id: u64, kind: PluginKind) -> Plugin {
        Plugin {
            id: PluginId(id),
            name: format!("p{id}"),
            kind,
            format: "builtin".into(),
            uid: format!("u{id}"),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            samples: Vec::new(),
            isolation: Default::default(),
            sidechain: None,
        }
    }

    #[test]
    fn serial_rack_is_dropped_and_order_is_kept() {
        let mut t = Track::new(crate::id::TrackId(1), TrackKind::Instrument, "t");
        t.inserts = vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Effect),
            plugin(3, PluginKind::Effect),
        ];
        t.rack = Some(Rack {
            source: Some(Source::Single(PluginId(1))),
            fx: vec![RackNode::Plugin(PluginId(3)), RackNode::Plugin(PluginId(2))],
        });
        assert!(t.fix_rack(&mut fresh_id()).is_empty());
        assert!(t.rack.is_none());
        assert_eq!(
            t.inserts.iter().map(|x| x.id.0).collect::<Vec<_>>(),
            vec![1, 3, 2]
        );
    }

    #[test]
    fn missing_and_duplicate_plugins_are_reported() {
        let mut t = Track::new(crate::id::TrackId(1), TrackKind::Audio, "t");
        t.inserts = vec![plugin(1, PluginKind::Effect), plugin(2, PluginKind::Effect)];
        t.rack = Some(Rack {
            source: None,
            fx: vec![
                RackNode::Plugin(PluginId(9)),
                RackNode::Plugin(PluginId(1)),
                RackNode::Plugin(PluginId(1)),
            ],
        });
        let rows = t.fix_rack(&mut fresh_id());
        assert!(t.rack.is_none());
        assert_eq!(
            rows.iter()
                .filter(|r| r.kind == AdjustKind::Dropped)
                .count(),
            2
        );
        assert_eq!(
            t.inserts.iter().map(|x| x.id.0).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn multi_fills_a_new_layer() {
        let mut t = Track::new(crate::id::TrackId(1), TrackKind::Instrument, "t");
        t.inserts = vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Instrument),
        ];
        t.rack = Some(Rack {
            source: Some(Source::Multi {
                id: NodeId(10),
                layers: vec![Layer {
                    id: NodeId(11),
                    name: "a".into(),
                    instrument: PluginId(1),
                    fx: Vec::new(),
                    keys: (0, 127),
                    velocity: (1, 127),
                    transpose: 0,
                    gain_db: 0.0,
                    pan: 0.0,
                    mute: false,
                }],
            }),
            fx: Vec::new(),
        });
        let rows = t.fix_rack(&mut fresh_id());
        assert!(
            matches!(&t.rack.as_ref().unwrap().source, Some(Source::Multi { layers, .. }) if layers.len() == 2)
        );
        assert_eq!(
            rows.iter().filter(|r| r.kind == AdjustKind::Filled).count(),
            1
        );
    }
}
