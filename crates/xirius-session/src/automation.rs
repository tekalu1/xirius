//! Playing automation lanes (AUT-001, AUT-011): the song as its lanes set it at one tick.
//!
//! The engine has no lanes of its own. The host and the offline render ask [`at`] for the song
//! at the current position and send what differs from the last mix, once per poll or block.
//! Fader, pan, mute and send lanes follow the play position. A plugin parameter lane is set
//! where the mix is built, and from there [`ParamFollow`] moves the running plugin as the
//! position moves, in this process or in a plugin host process.

use std::collections::HashMap;
use std::sync::Arc;

use xirius_core::auto::AutoTarget;
use xirius_core::id::PluginId;
use xirius_core::model::{Project, Track};
use xirius_engine::Command;

use crate::graph::{Mix, StripMix};

/// Does any channel play a lane with points?
pub fn has_lanes(p: &Project) -> bool {
    p.tracks.values().any(|t| plays(t))
}

fn plays(t: &Track) -> bool {
    t.auto_mode.plays() && t.automation.iter().any(|l| !l.points.is_empty())
}

/// Parameter lanes arrive at the current transport position, so their processors cannot
/// run ahead of it. Fader/pan/mute/send lanes act after the prefetched chain and can.
pub(crate) fn realtime(t: &Track) -> bool {
    t.armed
        || t.monitor
        || (t.auto_mode.plays()
            && t.automation.iter().any(|lane| {
                !lane.points.is_empty()
                    && matches!(&lane.target, AutoTarget::Param(id, _) if t.inserts.iter().any(|p| p.id == *id && p.runs()))
            }))
}

/// The song with every played lane's value written into its channel at `tick`.
pub fn at(p: &Project, tick: f64) -> Project {
    at_each(p, |_, _| tick)
}

pub(crate) fn at_each(
    p: &Project,
    mut tick: impl FnMut(xirius_core::id::TrackId, &AutoTarget) -> f64,
) -> Project {
    let mut out = p.clone();
    for (id, t) in &p.tracks {
        if !plays(t) {
            continue;
        }
        let Some(slot) = out.tracks.get_mut(id) else {
            continue;
        };
        let t = Arc::make_mut(slot);
        for lane in t.automation.clone() {
            let Some(v) = lane.value_at(tick(*id, &lane.target)) else {
                continue;
            };
            match &lane.target {
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
    }
    out
}

/// What the plugin parameter lanes set at `tick`: plugin, parameter, value (0..1).
pub fn params_at(p: &Project, tick: f64) -> Vec<(PluginId, String, f64)> {
    let mut out = Vec::new();
    for t in p.tracks.values().filter(|t| plays(t)) {
        for lane in &t.automation {
            if let AutoTarget::Param(pid, name) = &lane.target
                && t.inserts.iter().any(|x| x.id == *pid)
                && let Some(v) = lane.value_at(tick)
            {
                out.push((*pid, name.clone(), v));
            }
        }
    }
    out
}

/// Moves running plugins as their parameter lanes say, sending each value once.
#[derive(Default)]
pub struct ParamFollow {
    sent: HashMap<(PluginId, String), f64>,
}

impl ParamFollow {
    /// Sends what changed since the last call; returns how many values went.
    pub fn send(&mut self, p: &Project, tick: f64, loader: &crate::plugins::PluginLoader) -> usize {
        self.send_each(p, |_, _| tick, loader)
    }

    pub(crate) fn send_each(
        &mut self,
        p: &Project,
        mut tick: impl FnMut(xirius_core::id::TrackId, &AutoTarget) -> f64,
        loader: &crate::plugins::PluginLoader,
    ) -> usize {
        let mut values = Vec::new();
        for t in p.tracks.values().filter(|t| plays(t)) {
            for lane in &t.automation {
                if let AutoTarget::Param(pid, name) = &lane.target
                    && t.inserts.iter().any(|x| x.id == *pid)
                    && let Some(v) = lane.value_at(tick(t.id, &lane.target))
                {
                    values.push((*pid, name.clone(), v));
                }
            }
        }
        let mut n = 0;
        for (pid, name, v) in values {
            let key = (pid, name);
            if self.sent.get(&key) == Some(&v) {
                continue;
            }
            if loader.set_live_param(pid, &key.1, v) {
                n += 1;
                self.sent.insert(key, v);
            }
        }
        n
    }

    /// Forgets what was sent: the plugins were opened again or given the song's values.
    pub fn clear(&mut self) {
        self.sent.clear();
    }
}

/// The engine commands that take a running plan from `old` to `new`.
pub fn changes(old: &Mix, new: &Mix) -> Vec<Command> {
    let mut out = Vec::new();
    for (i, (o, n)) in old.tracks.iter().zip(&new.tracks).enumerate() {
        strip(o, n, &mut out, |c| match c {
            Strip::Gain(gain) => Command::TrackGain { track: i, gain },
            Strip::Pan(pan) => Command::TrackPan { track: i, pan },
            Strip::Mute(mute) => Command::TrackMute { track: i, mute },
            Strip::Send(send, level) => Command::SendLevel {
                track: i,
                send,
                level,
            },
        });
    }
    for (i, (o, n)) in old.buses.iter().zip(&new.buses).enumerate() {
        strip(o, n, &mut out, |c| match c {
            Strip::Gain(gain) => Command::BusGain { bus: i, gain },
            Strip::Pan(pan) => Command::BusPan { bus: i, pan },
            Strip::Mute(mute) => Command::BusMute { bus: i, mute },
            Strip::Send(send, level) => Command::BusSendLevel {
                bus: i,
                send,
                level,
            },
        });
    }
    if old.master_gain != new.master_gain {
        out.push(Command::MasterGain(new.master_gain));
    }
    out
}

enum Strip {
    Gain(f32),
    Pan(f32),
    Mute(bool),
    Send(usize, f32),
}

fn strip(o: &StripMix, n: &StripMix, out: &mut Vec<Command>, cmd: impl Fn(Strip) -> Command) {
    if o.gain != n.gain {
        out.push(cmd(Strip::Gain(n.gain)));
    }
    if o.pan != n.pan {
        out.push(cmd(Strip::Pan(n.pan)));
    }
    if o.mute != n.mute {
        out.push(cmd(Strip::Mute(n.mute)));
    }
    for (k, level) in n.sends.iter().enumerate() {
        if o.sends.get(k) != Some(level) {
            out.push(cmd(Strip::Send(k, *level)));
        }
    }
}

/// The song as the mix should hear it at `tick`: itself when nothing is automated.
pub fn song_at(p: &Project, tick: f64) -> std::borrow::Cow<'_, Project> {
    if has_lanes(p) {
        std::borrow::Cow::Owned(at(p, tick))
    } else {
        std::borrow::Cow::Borrowed(p)
    }
}
