//! The song's routing seen as the engine sees it: which track and bus gets which index, what is
//! audible under mute and solo, and which parts of the mix are structure (a new plan is needed)
//! and which are values (a command to the running plan is enough).

use std::collections::{BTreeSet, HashMap, HashSet};

use xirius_core::id::{PluginId, SendId, TrackId};
use xirius_core::model::{KeyMode, MidiOut, Output, Plugin, PluginKind, Project, Track, TrackKind};
use xirius_engine::Output as EOut;

/// What a render takes: the whole mix, or one track or bus on its own (a stem).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select {
    All,
    Track(TrackId),
    Bus(TrackId),
}

/// The bus that catches whatever a stem would otherwise send somewhere else. It is muted.
pub const DISCARD: TrackId = TrackId(u64::MAX);

/// The engine's numbering: tracks in id order, buses in an order where every bus comes before
/// the buses it feeds (ENG-034).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    pub tracks: Vec<TrackId>,
    pub buses: Vec<TrackId>,
}

impl Graph {
    pub fn of(p: &Project) -> Graph {
        let tracks: Vec<TrackId> = p
            .tracks
            .values()
            .filter(|t| matches!(t.kind, TrackKind::Audio | TrackKind::Instrument))
            .map(|t| t.id)
            .collect();
        let bus_ids: BTreeSet<TrackId> = p
            .tracks
            .values()
            .filter(|t| t.kind == TrackKind::Bus)
            .map(|t| t.id)
            .collect();
        // Kahn's algorithm; among the buses that are ready the lowest id goes first, so the
        // numbering does not move when something unrelated changes.
        let mut feeds: HashMap<TrackId, BTreeSet<TrackId>> = HashMap::new();
        let mut waiting: HashMap<TrackId, usize> = bus_ids.iter().map(|b| (*b, 0)).collect();
        for b in &bus_ids {
            for to in bus_targets(p, *b) {
                if bus_ids.contains(&to) && feeds.entry(*b).or_default().insert(to) {
                    *waiting.get_mut(&to).expect("bus") += 1;
                }
            }
        }
        let mut ready: BTreeSet<TrackId> = waiting
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(b, _)| *b)
            .collect();
        let mut buses = Vec::with_capacity(bus_ids.len());
        while let Some(b) = ready.pop_first() {
            buses.push(b);
            for to in feeds.get(&b).into_iter().flatten() {
                let n = waiting.get_mut(to).expect("bus");
                *n -= 1;
                if *n == 0 {
                    ready.insert(*to);
                }
            }
        }
        // A loop cannot be in a valid song; if one is, keep the buses so the plan reports it.
        for b in &bus_ids {
            if !buses.contains(b) {
                buses.push(*b);
            }
        }
        Graph { tracks, buses }
    }

    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| *t == id)
    }

    pub fn bus_index(&self, id: TrackId) -> Option<usize> {
        self.buses.iter().position(|t| *t == id)
    }
}

/// Where a channel plays to or sends to: the buses it reaches in one step.
/// Sidechain keys count too (MIX-047): a channel that keys a plug-in feeds
/// the plug-in's channel, so the bus order, the stems' reach and the loop
/// check see it.
pub fn bus_targets(p: &Project, id: TrackId) -> Vec<TrackId> {
    let Some(t) = p.tracks.get(&id) else {
        return Vec::new();
    };
    let mut v = Vec::new();
    if let Some(Output::Bus(b)) = p.effective_output(id) {
        v.push(b);
    }
    v.extend(t.sends.iter().map(|s| s.to));
    v.extend(
        p.key_edges()
            .into_iter()
            .filter_map(|(from, to)| (from == id).then_some(to)),
    );
    v
}

/// Where a channel plays to or sends to, without the sidechain key edges
/// (MIX-047): solo walks these alone. A soloed key source keeps what it
/// plays to audible, but the plug-in it keys does not count as its sound,
/// and a soloed channel does not pull the sources it is keyed by. A channel
/// hidden by solo is silent — its sidechain keys stop too (ENG-078).
fn audio_targets(p: &Project, id: TrackId) -> Vec<TrackId> {
    let Some(t) = p.tracks.get(&id) else {
        return Vec::new();
    };
    let mut v = Vec::new();
    if let Some(Output::Bus(b)) = p.effective_output(id) {
        v.push(b);
    }
    v.extend(t.sends.iter().map(|s| s.to));
    v
}

/// `10^(db/20)`; the bottom of the fader range is silence.
pub fn db_to_gain(db: f64) -> f32 {
    if db <= xirius_core::model::MIN_DB {
        0.0
    } else {
        10f64.powf(db / 20.0) as f32
    }
}

/// The channels that cannot be heard because of mute and solo: `true` means silent.
///
/// Mute wins over solo. With any solo on, what stays audible is the soloed channels, what they
/// feed on the way to the master, and what feeds a soloed bus.
pub fn silenced(p: &Project) -> HashMap<TrackId, bool> {
    let soloed: Vec<TrackId> = p
        .tracks
        .values()
        .filter(|t| t.solo && !t.id.is_master())
        .map(|t| t.id)
        .collect();
    let mut audible: HashSet<TrackId> = HashSet::new();
    if !soloed.is_empty() {
        let mut down: Vec<TrackId> = soloed.clone();
        while let Some(n) = down.pop() {
            if audible.insert(n) {
                down.extend(audio_targets(p, n));
            }
        }
        let reverse = audio_reverse_edges(p);
        let mut up: Vec<TrackId> = soloed
            .iter()
            .copied()
            .filter(|id| p.tracks[id].kind == TrackKind::Bus)
            .collect();
        while let Some(n) = up.pop() {
            for from in reverse.get(&n).into_iter().flatten() {
                if audible.insert(*from) {
                    up.push(*from);
                }
            }
        }
    }
    p.tracks
        .values()
        .map(|t| {
            let solo_hides = !soloed.is_empty() && !t.id.is_master() && !audible.contains(&t.id);
            (t.id, t.mute || solo_hides)
        })
        .collect()
}

fn reverse_edges(p: &Project) -> HashMap<TrackId, Vec<TrackId>> {
    reverse_of(p, bus_targets)
}

fn audio_reverse_edges(p: &Project) -> HashMap<TrackId, Vec<TrackId>> {
    reverse_of(p, audio_targets)
}

fn reverse_of(
    p: &Project,
    targets: fn(&Project, TrackId) -> Vec<TrackId>,
) -> HashMap<TrackId, Vec<TrackId>> {
    let mut reverse: HashMap<TrackId, Vec<TrackId>> = HashMap::new();
    for t in p.tracks.values().filter(|t| !t.id.is_master()) {
        for to in targets(p, t.id) {
            reverse.entry(to).or_default().push(t.id);
        }
    }
    reverse
}

/// Every channel that reaches `target` through outputs and sends, `target` included.
pub fn feeding(p: &Project, target: TrackId) -> HashSet<TrackId> {
    let reverse = reverse_edges(p);
    let mut set = HashSet::new();
    let mut stack = vec![target];
    while let Some(n) = stack.pop() {
        if set.insert(n) {
            stack.extend(reverse.get(&n).into_iter().flatten().copied());
        }
    }
    set
}

/// Which channels a render includes and how the rest of the wiring is cut (EXP-013, EXP-014).
///
/// For a stem, the channels that feed the target are in; everything else is silent and unused.
/// The target plays to the master with its sends dropped; the others keep only the outputs and
/// sends that stay inside the stem, and what would leave it goes to the discard bus.
#[derive(Clone, Debug)]
pub struct Scope {
    pub select: Select,
    up: Option<HashSet<TrackId>>,
}

impl Scope {
    pub fn new(p: &Project, select: Select) -> Scope {
        let up = match select {
            Select::All => None,
            Select::Track(id) | Select::Bus(id) => Some(feeding(p, id)),
        };
        Scope { select, up }
    }

    pub fn is_stem(&self) -> bool {
        self.up.is_some()
    }

    pub fn includes(&self, id: TrackId) -> bool {
        self.up.as_ref().is_none_or(|u| u.contains(&id))
    }

    fn target(&self) -> Option<TrackId> {
        match self.select {
            Select::All => None,
            Select::Track(id) | Select::Bus(id) => Some(id),
        }
    }
}

/// A channel's output and sends as the engine takes them in `scope`, with the send levels.
struct Wiring {
    output: EOut,
    sends: Vec<(usize, bool, f32)>,
}

fn wiring(p: &Project, g: &Graph, scope: &Scope, t: &Track) -> Wiring {
    let discard = g.buses.len();
    if !scope.includes(t.id) {
        return Wiring {
            output: EOut::Master,
            sends: Vec::new(),
        };
    }
    let is_target = scope.target() == Some(t.id);
    let output = if is_target {
        EOut::Master
    } else {
        match p.effective_output(t.id) {
            Some(Output::Bus(b)) if scope.includes(b) => {
                g.bus_index(b).map_or(EOut::Master, EOut::Bus)
            }
            _ if scope.is_stem() => EOut::Bus(discard),
            Some(Output::Bus(b)) => g.bus_index(b).map_or(EOut::Master, EOut::Bus),
            _ => EOut::Master,
        }
    };
    let sends = if is_target {
        Vec::new()
    } else {
        t.sends
            .iter()
            .filter(|s| scope.includes(s.to))
            .filter_map(|s| {
                g.bus_index(s.to)
                    .map(|b| (b, s.pre, db_to_gain(s.level_db)))
            })
            .collect()
    };
    Wiring { output, sends }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeShape {
    pub id: TrackId,
    pub inserts: Vec<Plugin>,
    pub output: EOut,
    pub sends: Vec<(usize, bool)>,
    /// The MIDI port a track plays to instead of an instrument.
    pub midi: Option<MidiOut>,
}

/// What makes a new plan necessary when it changes (clips and values do not count).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Structure {
    pub tracks: Vec<NodeShape>,
    pub buses: Vec<NodeShape>,
    pub master_inserts: Vec<Plugin>,
}

impl Structure {
    pub fn of(p: &Project, g: &Graph, scope: &Scope) -> Structure {
        let shape = |id: &TrackId| {
            let t = &p.tracks[id];
            let w = wiring(p, g, scope, t);
            NodeShape {
                id: *id,
                // A frozen track's inserts are in its sound.
                inserts: if scope.includes(*id) && t.frozen.is_none() {
                    t.inserts.clone()
                } else {
                    Vec::new()
                },
                output: w.output,
                sends: w.sends.iter().map(|&(b, pre, _)| (b, pre)).collect(),
                midi: t.midi_out.clone().filter(|_| {
                    t.kind == TrackKind::Instrument && t.frozen.is_none() && scope.includes(*id)
                }),
            }
        };
        let mut buses: Vec<NodeShape> = g.buses.iter().map(shape).collect();
        if scope.is_stem() {
            buses.push(NodeShape {
                id: DISCARD,
                inserts: Vec::new(),
                output: EOut::Master,
                sends: Vec::new(),
                midi: None,
            });
        }
        Structure {
            tracks: g.tracks.iter().map(shape).collect(),
            buses,
            master_inserts: if scope.is_stem() {
                Vec::new()
            } else {
                p.tracks
                    .get(&TrackId::MASTER)
                    .map(|m| m.inserts.clone())
                    .unwrap_or_default()
            },
        }
    }
}

/// One keyed insert's live values (ENG-078): the model `SendId`s keep
/// addressing each source whatever its engine slot is.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyMix {
    /// The plug-in that takes the key.
    pub plugin: PluginId,
    /// `false`: the insert takes its own signal as its key.
    pub on: bool,
    /// (send row id, linear level) for each source that has a level.
    pub sends: Vec<(SendId, f32)>,
}

/// The numbers on one channel strip.
#[derive(Clone, Debug, PartialEq)]
pub struct StripMix {
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub sends: Vec<f32>,
    /// The keyed inserts' on flags and source levels, in insert order.
    pub keys: Vec<KeyMix>,
}

/// The values a running plan can take without being replaced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mix {
    pub tracks: Vec<StripMix>,
    pub buses: Vec<StripMix>,
    pub master_gain: f32,
    /// The master row's keyed inserts.
    pub master_keys: Vec<KeyMix>,
}

impl Mix {
    pub fn of(p: &Project, g: &Graph, scope: &Scope) -> Mix {
        // In a stem the solo buttons are ignored; mute still counts. What a
        // channel cannot be heard for — the user's mute or another channel's
        // solo — silences its sidechain keys the same way (ENG-078).
        let silent: HashMap<TrackId, bool> = if scope.is_stem() {
            p.tracks.values().map(|t| (t.id, t.mute)).collect()
        } else {
            silenced(p)
        };
        // A plug-in's keys as live values, matching what `key_spec` in build
        // keeps (sources that resolve inside the scope).
        let keys_of = |t: &Track| -> Vec<KeyMix> {
            t.inserts
                .iter()
                .filter(|x| x.kind == PluginKind::Effect)
                .filter_map(|x| {
                    let sc = x.sidechain.as_ref()?;
                    let sources: Vec<&xirius_core::model::KeySource> = sc
                        .sources
                        .iter()
                        .filter(|s| scope.includes(s.track))
                        .collect();
                    if sources.is_empty() {
                        return None;
                    }
                    Some(KeyMix {
                        plugin: x.id,
                        on: sc.on,
                        sends: sources
                            .iter()
                            .filter_map(|s| match s.mode {
                                KeyMode::Send { level_db, .. } => {
                                    Some((s.id, db_to_gain(level_db)))
                                }
                                KeyMode::Output => None,
                            })
                            .collect(),
                    })
                })
                .collect()
        };
        let strip = |id: &TrackId| {
            let t = &p.tracks[id];
            StripMix {
                gain: db_to_gain(t.volume_db),
                pan: t.pan as f32,
                mute: silent[id] || !scope.includes(*id),
                sends: wiring(p, g, scope, t)
                    .sends
                    .iter()
                    .map(|&(_, _, level)| level)
                    .collect(),
                keys: keys_of(t),
            }
        };
        let mut buses: Vec<StripMix> = g.buses.iter().map(strip).collect();
        if scope.is_stem() {
            buses.push(StripMix {
                gain: 1.0,
                pan: 0.0,
                mute: true,
                sends: Vec::new(),
                keys: Vec::new(),
            });
        }
        let master = p.tracks.get(&TrackId::MASTER);
        Mix {
            tracks: g.tracks.iter().map(strip).collect(),
            buses,
            master_gain: if scope.is_stem() {
                1.0
            } else {
                master.map_or(1.0, |m| if m.mute { 0.0 } else { db_to_gain(m.volume_db) })
            },
            master_keys: if scope.is_stem() {
                Vec::new()
            } else {
                master.map_or_else(Vec::new, |m| keys_of(m))
            },
        }
    }
}

/// Where the song stops: the end of the last clip that sounds.
pub fn song_end_tick(p: &Project) -> i64 {
    p.tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter(|c| !c.muted)
        .map(|c| c.end())
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use xirius_core::Session;

    fn run(s: &mut Session, name: &str, args: serde_json::Value) {
        s.run_value(name, args, &Default::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    /// The ids of everything but the master, in the order they were added.
    fn ids_of(s: &Session) -> Vec<TrackId> {
        s.project()
            .tracks
            .keys()
            .copied()
            .filter(|t| !t.is_master())
            .collect()
    }

    fn session_with(kinds: &[&str]) -> (Session, Vec<TrackId>) {
        let mut s = Session::from_project(Project::new("t"));
        for (i, k) in kinds.iter().enumerate() {
            run(&mut s, k, json!({ "name": format!("T{i}") }));
        }
        let ids = ids_of(&s);
        (s, ids)
    }

    #[test]
    fn buses_are_numbered_before_the_buses_they_feed() {
        let (mut s, ids) = session_with(&["track.add.bus", "track.add.bus"]);
        let (a, b) = (ids[0], ids[1]);
        run(
            &mut s,
            "track.output.set",
            json!({"ids": [a.to_string()], "to": b.to_string()}),
        );
        assert_eq!(Graph::of(s.project()).buses, vec![a, b]);
        run(
            &mut s,
            "track.output.set",
            json!({"ids": [a.to_string()], "to": "master"}),
        );
        run(
            &mut s,
            "track.output.set",
            json!({"ids": [b.to_string()], "to": a.to_string()}),
        );
        assert_eq!(Graph::of(s.project()).buses, vec![b, a]);
    }

    #[test]
    fn mute_wins_over_solo_and_solo_keeps_the_path_to_the_master() {
        let (mut s, ids) = session_with(&[
            "track.add.bus",
            "track.add.instrument",
            "track.add.instrument",
            "track.add.instrument",
        ]);
        let (bus, a, b, c) = (ids[0], ids[1], ids[2], ids[3]);
        for t in [a, b] {
            run(
                &mut s,
                "track.output.set",
                json!({"ids": [t.to_string()], "to": bus.to_string()}),
            );
        }
        assert!(silenced(s.project()).values().all(|v| !v));
        // Solo A: A and the bus it plays into are heard; B and C are not.
        run(&mut s, "track.solo", json!({"ids": [a.to_string()]}));
        let m = silenced(s.project());
        assert!(!m[&a] && !m[&bus] && m[&b] && m[&c]);
        // Solo the bus too: what feeds it (B) is heard now.
        run(&mut s, "track.solo", json!({"ids": [bus.to_string()]}));
        let m = silenced(s.project());
        assert!(!m[&a] && !m[&b] && !m[&bus] && m[&c]);
        // Mute wins over solo.
        run(&mut s, "track.mute", json!({"ids": [a.to_string()]}));
        assert!(silenced(s.project())[&a]);
    }

    #[test]
    fn a_key_edge_orders_buses_but_solo_walks_outputs_and_sends_only() {
        // Kick keys a plug-in on Bass's bus. The key edge orders Kick's bus
        // before Bass's (the render feeds the key before the receiver), but
        // solo sees only the output/send edges: soloing Kick does not wake
        // Bass's bus, and soloing Bass's bus does not pull Kick in.
        let (mut s, ids) =
            session_with(&["track.add.bus", "track.add.bus", "track.add.instrument"]);
        let (kick_bus, bass_bus, kick) = (ids[0], ids[1], ids[2]);
        run(
            &mut s,
            "track.output.set",
            json!({"ids": [kick.to_string()], "to": kick_bus.to_string()}),
        );
        let comp = s
            .run_value(
                "plugin.add",
                json!({"track": bass_bus.to_string(), "name": "Comp"}),
                &Default::default(),
            )
            .unwrap()
            .data["id"]
            .as_str()
            .unwrap()
            .to_string();
        run(
            &mut s,
            "plugin.sidechain.add",
            json!({"id": comp, "from": kick.to_string()}),
        );
        // The key edge kick_bus? No: the edge is kick -> bass_bus. Kick's bus
        // is not in the chain; buses are ordered kick's own chain first.
        assert!(feeding(s.project(), bass_bus).contains(&kick));
        run(&mut s, "track.solo", json!({"ids": [kick.to_string()]}));
        let m = silenced(s.project());
        assert!(!m[&kick] && !m[&kick_bus] && m[&bass_bus]);
        run(&mut s, "track.solo.clear", json!({}));
        run(&mut s, "track.solo", json!({"ids": [bass_bus.to_string()]}));
        let m = silenced(s.project());
        // The bus it is keyed by does not get pulled into the solo.
        assert!(!m[&bass_bus] && m[&kick] && m[&kick_bus]);
    }

    #[test]
    fn feeding_follows_outputs_and_sends() {
        let (mut s, ids) = session_with(&[
            "track.add.bus",
            "track.add.instrument",
            "track.add.instrument",
        ]);
        let (bus, a, b) = (ids[0], ids[1], ids[2]);
        run(
            &mut s,
            "send.add",
            json!({"ids": [a.to_string()], "to": bus.to_string()}),
        );
        let f = feeding(s.project(), bus);
        assert!(f.contains(&bus) && f.contains(&a) && !f.contains(&b));
    }

    #[test]
    fn a_stem_keeps_only_what_stays_inside_it() {
        // A plays into the bus; B sends to the bus and plays to the master; C is unrelated.
        let (mut s, ids) = session_with(&[
            "track.add.bus",
            "track.add.instrument",
            "track.add.instrument",
            "track.add.instrument",
        ]);
        let (bus, a, b, c) = (ids[0], ids[1], ids[2], ids[3]);
        run(
            &mut s,
            "track.output.set",
            json!({"ids": [a.to_string()], "to": bus.to_string()}),
        );
        run(
            &mut s,
            "send.add",
            json!({"ids": [b.to_string()], "to": bus.to_string()}),
        );
        let p = s.project();
        let g = Graph::of(p);
        let scope = Scope::new(p, Select::Bus(bus));
        let st = Structure::of(p, &g, &scope);
        let discard = g.buses.len();
        assert_eq!(st.buses.len(), 2);
        assert_eq!(st.buses[0].output, EOut::Master);
        assert_eq!(st.buses[discard].id, DISCARD);
        let by = |id| st.tracks.iter().find(|n| n.id == id).unwrap();
        assert_eq!(by(a).output, EOut::Bus(0));
        // B's own output leaves the stem, so it goes to the discard bus; its send stays.
        assert_eq!(by(b).output, EOut::Bus(discard));
        assert_eq!(by(b).sends, vec![(0, false)]);
        let mix = Mix::of(p, &g, &scope);
        assert!(mix.tracks[g.track_index(c).unwrap()].mute);
        assert!(mix.buses[discard].mute);
        // A track stem: itself to the master, nothing else.
        let st = Structure::of(p, &g, &Scope::new(p, Select::Track(a)));
        assert_eq!(
            st.tracks.iter().find(|n| n.id == a).unwrap().output,
            EOut::Master
        );
    }
}
