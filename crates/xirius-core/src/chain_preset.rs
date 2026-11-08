//! The chain presets (PLG-065): an FX chain, an instrument with the effects behind it, or a
//! multi-instrument with its layers, kept as one `.xpreset` with the rack and the macros.
//!
//! The file names its devices and the rack's nodes by `ref` (`p1`, `n1`, `b1`, `m1`, `l1`)
//! rather than by the song's ids, and a map of a macro by those refs
//! (`param:p3:mix`, `branch:b2:gain`), so a chain reads into any song: the ids are given out
//! again on the way in ([`Chain::renumber`]). A file is read the way a track preset is: what
//! is missing is filled, what is not understood is left out, and every change comes back as
//! an [`Adjust`] row; the rack goes through [`Track::fix_rack`], which keeps the limits a
//! song has (a splitter nests 4 deep at most, a multi holds no multi).

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::auto::{AutoTarget, BranchPart};
use crate::controls::{ControlId, MacroControls};
use crate::error::{CmdError, CmdResult};
use crate::id::{AnyId, NodeId, PluginId, TrackId};
use crate::model::{Plugin, PluginKind, Track, TrackKind};
use crate::rack::{Branch, Layer, Rack, RackNode, Source, Split, SplitMode};
use crate::track_preset::{Adjust, AdjustKind, parse_plugin, plugin_value};
use crate::xpreset::FORMAT;

/// The version this writes.
pub const VERSION: u32 = 1;

const KEYS: [&str; 9] = [
    "format",
    "version",
    "kind",
    "name",
    "saved_at",
    "devices",
    "rack",
    "controls",
    "browser_folder",
];

/// The kinds of chain a preset may hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainKind {
    /// Effects only: a splitter, a branch or a lane's whole FX.
    Fx,
    /// One instrument and the effects behind it.
    Instrument,
    /// Every layer of a multi-instrument and the effects behind it.
    Multi,
}

impl ChainKind {
    pub const ALL: [ChainKind; 3] = [ChainKind::Fx, ChainKind::Instrument, ChainKind::Multi];

    pub fn name(self) -> &'static str {
        match self {
            ChainKind::Fx => "fx-chain",
            ChainKind::Instrument => "instrument-fx",
            ChainKind::Multi => "multi-instrument",
        }
    }

    pub fn of(s: &str) -> Option<ChainKind> {
        ChainKind::ALL.into_iter().find(|k| k.name() == s)
    }

    /// The kind of track the chain is read for.
    fn track_kind(self) -> TrackKind {
        match self {
            ChainKind::Fx => TrackKind::Audio,
            _ => TrackKind::Instrument,
        }
    }
}

/// The kind of chain `v` holds, when it is a chain preset at all.
pub fn kind_of_value(v: &Value) -> Option<ChainKind> {
    if v.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return None;
    }
    v.get("kind")
        .and_then(Value::as_str)
        .and_then(ChainKind::of)
}

/// Devices, the rack over them and the macros that move them. Ids are local to the chain
/// (plug-ins from 1) until [`Chain::renumber`] gives it the song's.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Chain {
    pub devices: Vec<Plugin>,
    pub rack: Rack,
    pub controls: MacroControls,
}

/// A node of a rack found by its id.
enum Found<'a> {
    Multi,
    Layer(&'a Layer),
    Split(&'a Split),
    Branch(&'a Branch),
}

fn find(r: &Rack, id: NodeId) -> Option<Found<'_>> {
    fn within(nodes: &[RackNode], id: NodeId) -> Option<Found<'_>> {
        nodes.iter().find_map(|n| match n {
            RackNode::Split(s) if s.id == id => Some(Found::Split(s)),
            RackNode::Split(s) => s.branches.iter().find_map(|b| {
                if b.id == id {
                    Some(Found::Branch(b))
                } else {
                    within(&b.fx, id)
                }
            }),
            RackNode::Plugin(_) => None,
        })
    }
    if let Some(Source::Multi { id: m, layers }) = &r.source {
        if *m == id {
            return Some(Found::Multi);
        }
        for l in layers {
            if l.id == id {
                return Some(Found::Layer(l));
            }
            if let Some(f) = within(&l.fx, id) {
                return Some(f);
            }
        }
    }
    within(&r.fx, id)
}

/// `target` with the ids of `plugins` and `nodes`; `None` when it points outside them. The
/// track's volume and pan go along only with a whole track (`track`).
fn moved(
    target: &AutoTarget,
    plugins: &HashMap<PluginId, PluginId>,
    nodes: &HashMap<NodeId, NodeId>,
    track: bool,
) -> Option<AutoTarget> {
    match target {
        AutoTarget::Param(id, name) => plugins.get(id).map(|n| AutoTarget::Param(*n, name.clone())),
        AutoTarget::Branch(id, part) => nodes.get(id).map(|n| AutoTarget::Branch(*n, *part)),
        AutoTarget::Volume | AutoTarget::Pan if track => Some(target.clone()),
        _ => None,
    }
}

fn no_instrument() -> CmdError {
    CmdError::not_found("このトラックにインストゥルメントがありません")
}

impl Chain {
    /// What `of` names on `t` as a chain of `kind`, and how many maps of the macros were left
    /// out because they point outside it. `of` is a plug-in, a splitter, a branch or a layer;
    /// none (or the track) is the track's whole FX, its instrument, or its multi.
    pub fn capture(t: &Track, kind: ChainKind, of: Option<AnyId>) -> CmdResult<(Chain, usize)> {
        let rack = t.rack.clone().unwrap_or_else(|| Rack::serial(t));
        let plugin = |id: PluginId| {
            t.inserts.iter().find(|x| x.id == id).ok_or_else(|| {
                CmdError::not_found(format!("このトラックにプラグイン {id} がありません"))
            })
        };
        let multi =
            || CmdError::conflict("マルチインストゥルメントは multi-instrument で保存してください");
        if kind != ChainKind::Fx && t.kind != TrackKind::Instrument {
            return Err(no_instrument());
        }
        let whole = matches!(of, None | Some(AnyId::Track(_)));
        let (source, fx) = match kind {
            ChainKind::Fx => match of {
                _ if whole => (None, rack.fx.clone()),
                Some(AnyId::Plugin(id)) => {
                    let x = plugin(id)?;
                    if x.kind != PluginKind::Effect {
                        return Err(CmdError::conflict(format!(
                            "「{}」はエフェクトではありません",
                            x.name
                        )));
                    }
                    (None, vec![RackNode::Plugin(id)])
                }
                Some(AnyId::RackNode(n)) => match find(&rack, n) {
                    Some(Found::Split(s)) => (None, vec![RackNode::Split(s.clone())]),
                    Some(Found::Branch(b)) => (None, b.fx.clone()),
                    Some(Found::Layer(l)) => (None, l.fx.clone()),
                    Some(Found::Multi) => return Err(multi()),
                    None => {
                        return Err(CmdError::not_found(format!(
                            "このトラックのラックに {n} がありません"
                        )));
                    }
                },
                None | Some(_) => {
                    return Err(CmdError::invalid(
                        "--of にはエフェクト・スプリッター・分岐・レイヤーを指定してください",
                    ));
                }
            },
            ChainKind::Instrument => match (of, &rack.source) {
                (_, Some(Source::Single(x))) if whole => {
                    (Some(Source::Single(*x)), rack.fx.clone())
                }
                (_, Some(Source::Multi { .. })) if whole => return Err(multi()),
                (_, None) if whole => return Err(no_instrument()),
                (Some(AnyId::Plugin(id)), Some(Source::Single(x))) if *x == id => {
                    (Some(Source::Single(id)), rack.fx.clone())
                }
                (Some(AnyId::Plugin(id)), Some(Source::Multi { layers, .. })) => {
                    match layers.iter().find(|l| l.instrument == id) {
                        Some(l) => (Some(Source::Single(id)), l.fx.clone()),
                        None => {
                            return Err(CmdError::conflict(format!(
                                "「{}」はこのトラックの音源ではありません",
                                plugin(id)?.name
                            )));
                        }
                    }
                }
                (Some(AnyId::RackNode(n)), _) => match find(&rack, n) {
                    Some(Found::Layer(l)) => (Some(Source::Single(l.instrument)), l.fx.clone()),
                    _ => {
                        return Err(CmdError::invalid(
                            "--of にはインストゥルメントかレイヤーを指定してください",
                        ));
                    }
                },
                _ => {
                    return Err(CmdError::invalid(
                        "--of にはインストゥルメントかレイヤーを指定してください",
                    ));
                }
            },
            ChainKind::Multi => match &rack.source {
                Some(s @ Source::Multi { .. }) => (Some(s.clone()), rack.fx.clone()),
                _ => {
                    return Err(CmdError::conflict(
                        "このトラックはマルチインストゥルメントではありません",
                    ));
                }
            },
        };
        if source.is_none() && fx.is_empty() {
            return Err(CmdError::conflict("保存するエフェクトがありません"));
        }
        Ok(Chain::local(t, Rack { source, fx }, false))
    }

    /// The whole of `t` (a track preset, MIX-030 v2), with every macro.
    pub fn of_track(t: &Track) -> (Chain, usize) {
        let rack = t.rack.clone().unwrap_or_else(|| Rack::serial(t));
        Chain::local(t, rack, true)
    }

    /// The plug-ins `sub` names, off `t` and numbered from 1, and the maps of `t`'s macros
    /// that point inside; the count of the maps left out.
    fn local(t: &Track, mut sub: Rack, track: bool) -> (Chain, usize) {
        let mut plugins = HashMap::new();
        let mut devices: Vec<Plugin> = Vec::new();
        for id in sub.flatten_order() {
            if plugins.contains_key(&id) {
                continue;
            }
            let Some(x) = t.inserts.iter().find(|x| x.id == id) else {
                continue;
            };
            let mut x = x.clone();
            x.id = PluginId(devices.len() as u64 + 1);
            x.sidechain = None;
            plugins.insert(id, x.id);
            devices.push(x);
        }
        let before = sub.node_ids();
        let mut n = 0;
        sub.remap_ids(&plugins, &mut || {
            n += 1;
            n
        });
        let nodes: HashMap<NodeId, NodeId> = before.into_iter().zip(sub.node_ids()).collect();
        let mut controls = t.controls.clone();
        controls.page = None;
        if !track {
            controls.xy = MacroControls::default().xy;
        }
        let dropped = controls.retarget(|x| moved(x, &plugins, &nodes, track));
        (
            Chain {
                devices,
                rack: sub,
                controls,
            },
            dropped,
        )
    }

    /// Gives the chain ids from `alloc` (the song's counter); the old → new of its plug-ins.
    pub fn renumber(&mut self, alloc: &mut dyn FnMut() -> u64) -> HashMap<PluginId, PluginId> {
        let mut plugins = HashMap::new();
        for x in &mut self.devices {
            let n = PluginId(alloc());
            plugins.insert(x.id, n);
            x.id = n;
        }
        let before = self.rack.node_ids();
        self.rack.remap_ids(&plugins, alloc);
        let nodes: HashMap<NodeId, NodeId> = before.into_iter().zip(self.rack.node_ids()).collect();
        self.controls.retarget(|x| moved(x, &plugins, &nodes, true));
        plugins
    }

    /// The devices, the rack and the macros as a file keeps them, by ref. The macros are
    /// `None` when no control has a map (a whole track keeps its names and values too).
    pub fn to_values(&self, track: bool) -> (Vec<Value>, Value, Option<Value>) {
        let mut refs = Refs::default();
        for (i, x) in self.devices.iter().enumerate() {
            refs.plugins.insert(x.id, format!("p{}", i + 1));
        }
        let rack = refs.rack(&self.rack);
        let devices = self
            .devices
            .iter()
            .map(|x| {
                let mut v = plugin_value(x);
                if let Some(o) = v.as_object_mut() {
                    o.insert("ref".into(), json!(refs.plugins[&x.id]));
                }
                v
            })
            .collect();
        let keep = if track {
            !self.controls.is_empty()
        } else {
            self.controls.has_maps()
        };
        let controls = keep.then(|| refs.controls(&self.controls, track));
        (devices, rack, controls)
    }

    /// The chain as a file keeps it, by ref: the same for two chains whose ids alone differ,
    /// so a track's chain can be told to be a preset's. A device counts by what it is and
    /// what it plays ([`SHAPE_KEYS`]): not its name, the process it runs in, the device preset
    /// it was named after, or a plug-in's own blob, which a plug-in may write over by itself
    /// once loaded (a device's window, too, tells its presets by the knobs). The macros count
    /// by what they move and where they stand, not by their knob or name: a load lands one on
    /// a free knob and names it after the preset.
    pub fn shape(&self) -> Value {
        let (devices, rack, controls) = self.to_values(false);
        let devices: Vec<Value> = devices
            .into_iter()
            .map(|mut v| {
                if let Some(o) = v.as_object_mut() {
                    o.retain(|k, _| SHAPE_KEYS.contains(&k.as_str()));
                }
                v
            })
            .collect();
        let macros = controls.as_ref().map(mapped).unwrap_or_default();
        json!({ "devices": devices, "rack": rack, "macros": macros })
    }

    /// Reads the devices (from `devices_key`), the rack and the macros of a file for a track
    /// of `kind`; `track` lets the macros move the track's volume and pan.
    pub(crate) fn parse(
        o: &Map<String, Value>,
        devices_key: &str,
        kind: TrackKind,
        track: bool,
        rows: &mut Vec<Adjust>,
    ) -> Chain {
        let mut refs: HashMap<String, PluginId> = HashMap::new();
        let mut devices = Vec::new();
        match o.get(devices_key) {
            Some(Value::Array(xs)) => {
                for (i, v) in xs.iter().enumerate() {
                    if let Some(mut x) = parse_plugin(i + 1, v, rows) {
                        x.id = PluginId(devices.len() as u64 + 1);
                        if let Some(r) = v.get("ref").and_then(Value::as_str) {
                            refs.entry(r.to_string()).or_insert(x.id);
                        }
                        devices.push(x);
                    }
                }
            }
            _ => rows.push(Adjust::new(
                AdjustKind::Filled,
                "機器",
                "無いため、空にしました",
            )),
        }
        let mut temp = Track::new(TrackId(0), kind, "");
        temp.inserts = devices;
        let mut reader = Reader {
            plugins: &refs,
            nodes: HashMap::new(),
            next: 0,
            rows,
        };
        let rack = match o.get("rack") {
            None | Some(Value::Null) => Rack::serial(&temp),
            Some(v) => reader.rack(v),
        };
        let Reader {
            nodes,
            mut next,
            rows,
            ..
        } = reader;
        temp.rack = Some(rack);
        rows.extend(temp.fix_rack(&mut || {
            next += 1;
            next
        }));
        let rack = temp.rack.clone().unwrap_or_else(|| Rack::serial(&temp));
        let controls = match o.get("controls") {
            None | Some(Value::Null) => MacroControls::default(),
            Some(v) => controls_in(v, &refs, &nodes, track, &temp, rows),
        };
        Chain {
            devices: temp.inserts,
            rack,
            controls,
        }
    }
}

/// The refs a file names devices and nodes by.
#[derive(Default)]
struct Refs {
    plugins: HashMap<PluginId, String>,
    nodes: HashMap<NodeId, String>,
    counts: HashMap<char, usize>,
}

impl Refs {
    fn node(&mut self, id: NodeId, letter: char) -> String {
        let n = self.counts.entry(letter).or_insert(0);
        *n += 1;
        let r = format!("{letter}{n}");
        self.nodes.insert(id, r.clone());
        r
    }

    fn plugin(&self, id: PluginId) -> Value {
        json!(self.plugins.get(&id).cloned().unwrap_or_default())
    }

    fn rack(&mut self, r: &Rack) -> Value {
        let mut m = Map::new();
        match &r.source {
            Some(Source::Single(x)) => {
                m.insert("source".into(), json!({ "single": self.plugin(*x) }));
            }
            Some(Source::Multi { id, layers }) => {
                let me = self.node(*id, 'm');
                let layers: Vec<Value> = layers
                    .iter()
                    .map(|l| {
                        json!({
                            "ref": self.node(l.id, 'l'),
                            "name": l.name,
                            "instrument": self.plugin(l.instrument),
                            "fx": self.nodes(&l.fx),
                            "keys": [l.keys.0, l.keys.1],
                            "velocity": [l.velocity.0, l.velocity.1],
                            "transpose": l.transpose,
                            "gain_db": l.gain_db,
                            "pan": l.pan,
                            "mute": l.mute,
                        })
                    })
                    .collect();
                m.insert(
                    "source".into(),
                    json!({ "multi": { "ref": me, "layers": layers } }),
                );
            }
            None => {}
        }
        m.insert("fx".into(), self.nodes(&r.fx));
        Value::Object(m)
    }

    fn nodes(&mut self, nodes: &[RackNode]) -> Value {
        Value::Array(
            nodes
                .iter()
                .map(|n| match n {
                    RackNode::Plugin(x) => json!({ "plugin": self.plugin(*x) }),
                    RackNode::Split(s) => {
                        let me = self.node(s.id, 'n');
                        let branches: Vec<Value> = s
                            .branches
                            .iter()
                            .map(|b| {
                                json!({
                                    "ref": self.node(b.id, 'b'),
                                    "fx": self.nodes(&b.fx),
                                    "gain_db": b.gain_db,
                                    "pan": b.pan,
                                    "mute": b.mute,
                                })
                            })
                            .collect();
                        let mut o = Map::new();
                        o.insert("ref".into(), json!(me));
                        o.insert("mode".into(), json!(s.mode));
                        if !s.crossovers.is_empty() {
                            o.insert("crossovers".into(), json!(s.crossovers));
                        }
                        o.insert("branches".into(), json!(branches));
                        json!({ "split": o })
                    }
                })
                .collect(),
        )
    }

    fn target(&self, t: &AutoTarget) -> Option<String> {
        match t {
            AutoTarget::Param(id, name) => Some(format!("param:{}:{name}", self.plugins.get(id)?)),
            AutoTarget::Branch(id, part) => {
                Some(format!("branch:{}:{}", self.nodes.get(id)?, part.name()))
            }
            AutoTarget::Volume => Some("volume".into()),
            AutoTarget::Pan => Some("pan".into()),
            _ => None,
        }
    }

    fn controls(&self, c: &MacroControls, track: bool) -> Value {
        let mut c = c.clone();
        c.page = None;
        if !track {
            c.xy = MacroControls::default().xy;
        }
        let mut v = serde_json::to_value(&c).unwrap_or(Value::Null);
        each_map(&mut v, |m| {
            let text = m
                .get("target")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<AutoTarget>().ok())
                .and_then(|t| self.target(&t));
            match text {
                Some(s) => {
                    m.insert("target".into(), json!(s));
                    true
                }
                None => false,
            }
        });
        v
    }
}

/// What of a device [`Chain::shape`] keeps: which device, where the rack holds it, whether it
/// plays, its knobs and its samples.
const SHAPE_KEYS: [&str; 8] = [
    "ref",
    "kind",
    "format",
    "uid",
    "bypass",
    "deactivated",
    "params",
    "samples",
];

/// The controls of a saved set that have maps (knobs, buttons, pads' axes), as their maps
/// and values, in one order.
fn mapped(v: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut controls: Vec<&Value> = Vec::new();
    for k in ["knobs", "buttons"] {
        controls.extend(v.get(k).and_then(Value::as_array).into_iter().flatten());
    }
    for pad in v
        .get("pads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        controls.extend(["x", "y"].iter().filter_map(|axis| pad.get(*axis)));
    }
    for c in controls {
        if c["maps"].as_array().is_some_and(|m| !m.is_empty()) {
            out.push(json!({ "maps": c["maps"], "value": c.get("value") }));
        }
    }
    out.sort_by_key(Value::to_string);
    out
}

/// Runs `f` on every map of a saved set of controls (knobs, buttons, pads' axes); a map `f`
/// says false to is left out.
fn each_map(v: &mut Value, mut f: impl FnMut(&mut Map<String, Value>) -> bool) {
    let mut controls: Vec<&mut Value> = Vec::new();
    let Some(o) = v.as_object_mut() else {
        return;
    };
    for (k, list) in o.iter_mut() {
        let Some(list) = list.as_array_mut() else {
            continue;
        };
        match k.as_str() {
            "knobs" | "buttons" => controls.extend(list.iter_mut()),
            "pads" => {
                for pad in list.iter_mut().filter_map(Value::as_object_mut) {
                    for (axis, c) in pad.iter_mut() {
                        if axis == "x" || axis == "y" {
                            controls.push(c);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for c in controls {
        if let Some(maps) = c.get_mut("maps").and_then(Value::as_array_mut) {
            maps.retain_mut(|m| m.as_object_mut().is_some_and(&mut f));
        }
    }
}

/// Reads a file's rack by its refs.
struct Reader<'a> {
    plugins: &'a HashMap<String, PluginId>,
    nodes: HashMap<String, NodeId>,
    next: u64,
    rows: &'a mut Vec<Adjust>,
}

fn number(o: &Map<String, Value>, k: &str, default: f64) -> f64 {
    o.get(k)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .unwrap_or(default)
}

fn range(o: &Map<String, Value>, k: &str, default: (u8, u8)) -> (u8, u8) {
    let byte = |v: &Value| v.as_u64().map(|n| n.min(127) as u8);
    match o.get(k).and_then(Value::as_array).map(Vec::as_slice) {
        Some([a, b]) => (byte(a).unwrap_or(default.0), byte(b).unwrap_or(default.1)),
        _ => default,
    }
}

impl Reader<'_> {
    fn id(&mut self, r: Option<&Value>) -> NodeId {
        self.next += 1;
        let id = NodeId(self.next);
        if let Some(s) = r.and_then(Value::as_str) {
            self.nodes.entry(s.to_string()).or_insert(id);
        }
        id
    }

    fn plugin(&mut self, r: Option<&Value>, what: &str) -> Option<PluginId> {
        let found = r
            .and_then(Value::as_str)
            .and_then(|s| self.plugins.get(s).copied());
        if found.is_none() {
            self.rows.push(Adjust::new(
                AdjustKind::Rejected,
                what,
                "指す機器が無いため、外しました",
            ));
        }
        found
    }

    fn rack(&mut self, v: &Value) -> Rack {
        let Some(o) = v.as_object() else {
            self.rows.push(Adjust::new(
                AdjustKind::Dropped,
                "ラック",
                "読み込めないため、機器を順に並べました",
            ));
            return Rack::default();
        };
        let source = match o.get("source") {
            None | Some(Value::Null) => None,
            Some(s) => self.source(s),
        };
        let fx = self.nodes(o.get("fx"), false);
        Rack { source, fx }
    }

    fn source(&mut self, v: &Value) -> Option<Source> {
        if let Some(r) = v.get("single") {
            return self.plugin(Some(r), "音源").map(Source::Single);
        }
        let Some(m) = v.get("multi").and_then(Value::as_object) else {
            self.rows.push(Adjust::new(
                AdjustKind::Dropped,
                "音源",
                "読み込めないため、外しました",
            ));
            return None;
        };
        let id = self.id(m.get("ref"));
        let mut layers = Vec::new();
        for (i, l) in m
            .get("layers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            if let Some(l) = self.layer(i + 1, l) {
                layers.push(l);
            }
        }
        Some(Source::Multi { id, layers })
    }

    fn layer(&mut self, n: usize, v: &Value) -> Option<Layer> {
        let what = format!("レイヤー {n}");
        let Some(o) = v.as_object() else {
            self.rows.push(Adjust::new(
                AdjustKind::Dropped,
                what,
                "読み込めないため、外しました",
            ));
            return None;
        };
        let instrument = self.plugin(o.get("instrument"), &what)?;
        let id = self.id(o.get("ref"));
        Some(Layer {
            id,
            name: o
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            instrument,
            fx: self.nodes(o.get("fx"), true),
            keys: range(o, "keys", (0, 127)),
            velocity: range(o, "velocity", (1, 127)),
            transpose: o
                .get("transpose")
                .and_then(Value::as_i64)
                .map_or(0, |t| t.clamp(i8::MIN.into(), i8::MAX.into()) as i8),
            gain_db: number(o, "gain_db", 0.0),
            pan: number(o, "pan", 0.0),
            mute: o.get("mute").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    fn nodes(&mut self, v: Option<&Value>, in_layer: bool) -> Vec<RackNode> {
        let mut out = Vec::new();
        for n in v.and_then(Value::as_array).into_iter().flatten() {
            if let Some(r) = n.get("plugin") {
                if let Some(id) = self.plugin(Some(r), "エフェクト") {
                    out.push(RackNode::Plugin(id));
                }
            } else if let Some(s) = n.get("split").and_then(Value::as_object) {
                out.push(RackNode::Split(self.split(s)));
            } else if n.get("multi").is_some() {
                self.rows.push(Adjust::new(
                    AdjustKind::Rejected,
                    "マルチインストゥルメント",
                    if in_layer {
                        "マルチの中にマルチは入れられないため、外しました"
                    } else {
                        "エフェクトの列にマルチは入れられないため、外しました"
                    },
                ));
            } else {
                self.rows.push(Adjust::new(
                    AdjustKind::Dropped,
                    "ラックの部品",
                    "読み込めないため、外しました",
                ));
            }
        }
        out
    }

    fn split(&mut self, o: &Map<String, Value>) -> Split {
        let id = self.id(o.get("ref"));
        let mode = match o.get("mode") {
            None | Some(Value::Null) => SplitMode::Normal,
            Some(m) => serde_json::from_value(m.clone()).unwrap_or_else(|_| {
                self.rows.push(Adjust::new(
                    AdjustKind::Filled,
                    "スプリッターの分け方",
                    "読み込めないため normal にしました",
                ));
                SplitMode::Normal
            }),
        };
        let crossovers = o
            .get("crossovers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect();
        let mut branches = Vec::new();
        for b in o
            .get("branches")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(b) = b.as_object() else {
                continue;
            };
            branches.push(Branch {
                id: self.id(b.get("ref")),
                fx: self.nodes(b.get("fx"), false),
                gain_db: number(b, "gain_db", 0.0),
                pan: number(b, "pan", 0.0),
                mute: b.get("mute").and_then(Value::as_bool).unwrap_or(false),
            });
        }
        Split {
            id,
            mode,
            crossovers,
            branches,
        }
    }
}

/// A map's target from its text by ref.
fn target_in(
    s: &str,
    plugins: &HashMap<String, PluginId>,
    nodes: &HashMap<String, NodeId>,
    track: bool,
) -> Option<AutoTarget> {
    match s {
        "volume" if track => return Some(AutoTarget::Volume),
        "pan" if track => return Some(AutoTarget::Pan),
        _ => {}
    }
    if let Some((r, name)) = s.strip_prefix("param:").and_then(|x| x.split_once(':'))
        && !name.is_empty()
    {
        return Some(AutoTarget::Param(*plugins.get(r)?, name.to_string()));
    }
    let (r, part) = s.strip_prefix("branch:")?.split_once(':')?;
    Some(AutoTarget::Branch(*nodes.get(r)?, BranchPart::parse(part)?))
}

/// Reads a file's macros: a map is kept when its target is on `temp` (the chain as read).
fn controls_in(
    v: &Value,
    plugins: &HashMap<String, PluginId>,
    nodes: &HashMap<String, NodeId>,
    track: bool,
    temp: &Track,
    rows: &mut Vec<Adjust>,
) -> MacroControls {
    let mut v = v.clone();
    let Some(o) = v.as_object_mut() else {
        rows.push(Adjust::new(
            AdjustKind::Dropped,
            "マクロ",
            "読み込めないため、外しました",
        ));
        return MacroControls::default();
    };
    o.remove("page");
    if !track {
        o.remove("xy");
    }
    for (k, max, what) in [
        ("knobs", crate::controls::KNOBS, "ノブ"),
        ("buttons", crate::controls::BUTTONS, "ボタン"),
        ("pads", crate::controls::PADS, "パッド"),
    ] {
        if let Some(list) = o.get_mut(k).and_then(Value::as_array_mut)
            && list.len() > max
        {
            list.truncate(max);
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                format!("マクロの{what}"),
                format!("{max} 個を超える分は、外しました"),
            ));
        }
    }
    let mut lost = 0;
    let mut rounded = false;
    each_map(&mut v, |m| {
        let t = m
            .get("target")
            .and_then(Value::as_str)
            .and_then(|s| target_in(s, plugins, nodes, track))
            .filter(|t| t.exists_on(temp));
        let Some(t) = t else {
            lost += 1;
            return false;
        };
        m.insert("target".into(), json!(t.to_string()));
        for (k, lo) in [("from", 0.0), ("to", 0.0), ("curve", -1.0)] {
            if let Some(n) = m.get(k).and_then(Value::as_f64) {
                let c = if n.is_finite() { n.clamp(lo, 1.0) } else { 0.0 };
                if c != n {
                    rounded = true;
                    m.insert(k.into(), json!(c));
                }
            }
        }
        true
    });
    if lost > 0 {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            "マクロの割り当て",
            format!("{lost} 件は、指す先が無いため外しました"),
        ));
    }
    if rounded {
        rows.push(Adjust::new(
            AdjustKind::Rounded,
            "マクロの割り当て",
            "範囲外の値を範囲の端にしました",
        ));
    }
    match serde_json::from_value::<MacroControls>(v) {
        Ok(c) => c,
        Err(_) => {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                "マクロ",
                "読み込めないため、外しました",
            ));
            MacroControls::default()
        }
    }
}

/// A chain preset: its name, kind and chain.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainPreset {
    pub name: String,
    pub kind: ChainKind,
    pub saved_at: Option<u64>,
    pub chain: Chain,
}

impl ChainPreset {
    pub fn to_value(&self) -> Value {
        let (devices, rack, controls) = self.chain.to_values(false);
        let mut m = Map::new();
        m.insert("format".into(), json!(FORMAT));
        m.insert("version".into(), json!(VERSION));
        m.insert("kind".into(), json!(self.kind.name()));
        m.insert("name".into(), json!(self.name));
        if let Some(at) = self.saved_at {
            m.insert("saved_at".into(), json!(at));
        }
        m.insert("devices".into(), json!(devices));
        m.insert("rack".into(), rack);
        if let Some(c) = controls {
            m.insert("controls".into(), c);
        }
        Value::Object(m)
    }

    /// Reads a chain preset, and says what had to be adjusted; `fallback` names one whose
    /// file has no name.
    pub fn parse(v: &Value, fallback: Option<&str>) -> CmdResult<(ChainPreset, Vec<Adjust>)> {
        let kind = kind_of_value(v)
            .ok_or_else(|| CmdError::invalid("チェーンのプリセットではありません"))?;
        let o = v.as_object().expect("a chain preset is an object");
        let mut rows = Vec::new();
        for k in o.keys().filter(|k| !KEYS.contains(&k.as_str())) {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                k.as_str(),
                "この版では読み込めないため、外しました",
            ));
        }
        let name = match o
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            Some(n) => n.to_string(),
            None => {
                rows.push(Adjust::new(
                    AdjustKind::Filled,
                    "名前",
                    "無いため、ファイルの名前にしました",
                ));
                fallback.unwrap_or("プリセット").to_string()
            }
        };
        let chain = Chain::parse(o, "devices", kind.track_kind(), false, &mut rows);
        let ok = match kind {
            ChainKind::Fx => !chain.rack.fx.is_empty(),
            ChainKind::Instrument => matches!(chain.rack.source, Some(Source::Single(_))),
            ChainKind::Multi => chain.rack.source.is_some(),
        };
        if !ok {
            return Err(CmdError::invalid(format!(
                "プリセット「{name}」に読み込める機器がありません"
            )));
        }
        Ok((
            ChainPreset {
                name,
                kind,
                saved_at: o.get("saved_at").and_then(Value::as_u64),
                chain,
            },
            rows,
        ))
    }

    /// An effect alone as a chain of one, for a device preset put at a place in the rack.
    pub fn of_effect(name: &str, mut x: Plugin) -> ChainPreset {
        x.id = PluginId(1);
        ChainPreset {
            name: name.to_string(),
            kind: ChainKind::Fx,
            saved_at: None,
            chain: Chain {
                rack: Rack {
                    source: None,
                    fx: vec![RackNode::Plugin(x.id)],
                },
                devices: vec![x],
                controls: MacroControls::default(),
            },
        }
    }
}

/// The controls of `c` that a chain's own may land on: none named, none mapped.
pub fn free(c: &MacroControls, id: ControlId) -> bool {
    let x = c.get(id);
    x.name.is_none() && x.maps.is_empty()
}
