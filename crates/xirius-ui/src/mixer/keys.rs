//! The sidechain on the mixer (MIX-048): the sends a strip keys a plug-in on another strip with,
//! listed after its own sends, and the plug-ins that can take a key.
use xirius_core::id::{PluginId, SendId, TrackId};
use xirius_core::model::{KEY_MAX_DB, KEY_MIN_DB, Plugin, Project};

use serde_json::json;
use xirius_core::model::KeyMode;

use super::MixerHit;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tools::Mods;

/// A key `t` sends: a [`KeySource`](xirius_core::model::KeySource) of a plug-in on another strip
/// that takes `t`'s sound through a send.
#[derive(Clone, Debug, PartialEq)]
pub struct KeySend {
    /// The source's id: the row's on the sending strip.
    pub id: SendId,
    /// The strip the receiving plug-in is on.
    pub owner: TrackId,
    pub plugin: PluginId,
    pub name: String,
    pub pre: bool,
    /// What the row shows: the held level of a silent row, else the level.
    pub level_db: f64,
    /// The receiver listens to its key (Ext).
    pub on: bool,
}

/// The keys `t` sends, by the order of the strips that take them, then as they were added.
pub fn key_sends(p: &Project, t: TrackId) -> Vec<KeySend> {
    let mut out = Vec::new();
    for owner in super::strip_order(p) {
        let Some(track) = p.tracks.get(&owner) else {
            continue;
        };
        for x in &track.inserts {
            let Some(sc) = &x.sidechain else {
                continue;
            };
            for s in sc.sources.iter().filter(|s| s.track == t) {
                let Some((pre, level, held)) = s.mode.send() else {
                    continue;
                };
                out.push(KeySend {
                    id: s.id,
                    owner,
                    plugin: x.id,
                    name: x.name.clone(),
                    pre,
                    level_db: held.unwrap_or(level),
                    on: sc.on,
                });
            }
        }
    }
    out
}

/// The key send `id` of `t`.
pub fn key_send(p: &Project, t: TrackId, id: SendId) -> Option<KeySend> {
    key_sends(p, t).into_iter().find(|k| k.id == id)
}

/// `x` can take a key: a built-in that has a sidechain input, or one already given sources.
pub fn takes_key(x: &Plugin) -> bool {
    x.builtin().is_some_and(crate::title_bar::takes_sidechain)
        || x.sidechain.as_ref().is_some_and(|sc| !sc.is_empty())
}

/// The plug-ins on other strips `t` could key, with their strip, by the order of the strips.
pub fn receivers(p: &Project, t: TrackId) -> Vec<(TrackId, PluginId)> {
    super::strip_order(p)
        .into_iter()
        .filter(|o| *o != t && !o.is_master())
        .filter_map(|o| p.tracks.get(&o))
        .flat_map(|o| {
            o.inserts
                .iter()
                .filter(|x| takes_key(x))
                .map(move |x| (o.id, x.id))
        })
        .collect()
}

/// A receiver as the send list and the reader name it: "<strip> › <plug-in>".
pub fn key_label(p: &Project, key: &KeySend) -> String {
    let owner = p.tracks.get(&key.owner).map_or("", |o| o.name.as_str());
    format!("{owner} › {}", key.name)
}

/// A key send's level as the reader says it: "+1.5 dB", "-12.0 dB".
pub fn level_words(db: f64) -> String {
    format!("{db:+.1} dB")
}

/// The pick value of the row that keys `plugin` from the send list's ＋.
pub const PICK: &str = "sc:";

/// The rows of the send list's group サイドチェーン for `t`: each receiver as
/// "<strip> › <plug-in>", checked when `t` keys it already, dim with the loop mark when a key
/// from `t` would make the routing loop.
pub fn key_rows(p: &Project, t: TrackId) -> Vec<crate::pick_list::PickRow> {
    use crate::pick_list::PickRow;
    use crate::popup::Action;
    receivers(p, t)
        .into_iter()
        .filter_map(|(owner, plugin)| {
            let o = p.tracks.get(&owner)?;
            let x = o.inserts.iter().find(|x| x.id == plugin)?;
            let label = format!("{} › {}", o.name, x.name);
            let on = keys(p, t, plugin);
            Some(if !on && !p.sidechain_ok(plugin, t) {
                PickRow::new(label).icon("loop").disabled()
            } else {
                PickRow::new(label)
                    .icon("sc")
                    .on(on)
                    .act(Action::Pick(format!("{PICK}{plugin}")))
            })
        })
        .collect()
}

/// `t` keys `plugin` already, by a send or by its output.
pub fn keys(p: &Project, t: TrackId, plugin: PluginId) -> bool {
    p.tracks.values().any(|o| {
        o.inserts.iter().any(|x| {
            x.id == plugin
                && x.sidechain
                    .iter()
                    .flat_map(|sc| &sc.sources)
                    .any(|s| s.track == t)
        })
    })
}

/// A key level held to what a sidechain send may be set to.
pub fn clamp(db: f64) -> f64 {
    db.clamp(KEY_MIN_DB, KEY_MAX_DB)
}

/// Where `db` sits along a key send's bar, 0 at -60 dB and 1 at +6 dB.
pub fn bar_t(db: f64) -> f32 {
    ((clamp(db) - KEY_MIN_DB) / (KEY_MAX_DB - KEY_MIN_DB)) as f32
}

/// The level at `t` along a key send's bar, by halves of a dB as the mock sets it.
pub fn bar_db(t: f32) -> f64 {
    let db = KEY_MIN_DB + f64::from(t.clamp(0.0, 1.0)) * (KEY_MAX_DB - KEY_MIN_DB);
    (db * 2.0).round() / 2.0
}

/// How many keys `t` sends through sends.
pub fn key_count(p: &Project, t: TrackId) -> usize {
    p.tracks
        .values()
        .flat_map(|o| &o.inserts)
        .filter_map(|x| x.sidechain.as_ref())
        .flat_map(|sc| &sc.sources)
        .filter(|s| s.track == t && s.mode.send().is_some())
        .count()
}

/// The parts of a key send's row: the sidechain mark, the receiving strip's chip, the plug-in's
/// name and the stretch the bar of its level takes presses in.
pub fn key_parts(row: Rect) -> (Rect, Rect, Rect, Rect) {
    let mid = |h: f32| row.y + (row.h - h) / 2.0;
    let mark = Rect::new(row.x + PAD, mid(MARK), MARK, MARK);
    let chip = Rect::new(mark.right() + GAP, mid(CHIP), CHIP, CHIP);
    let zone = Rect::new(
        row.right() - PAD - BAR_W - GAP,
        row.y,
        BAR_W + GAP + PAD,
        row.h,
    );
    let name = Rect::new(
        chip.right() + GAP,
        row.y,
        zone.x - chip.right() - GAP,
        row.h,
    );
    (mark, chip, name, zone)
}

/// The bar inside the stretch [`key_parts`] gives.
pub fn key_bar(zone: Rect) -> Rect {
    Rect::new(zone.x + GAP, zone.y + (zone.h - BAR_H) / 2.0, BAR_W, BAR_H)
}

/// A key send row's sidechain mark, as the mock's small icon.
const MARK: f32 = 12.0;
const CHIP: f32 = 8.0;
const PAD: f32 = 4.0;
const GAP: f32 = 4.0;
/// The bar of a key send's level, as the mock's `.lv`.
pub const BAR_W: f32 = 26.0;
const BAR_H: f32 = 6.0;

/// One step of ←→ on a key send's bar, in dB; Shift takes [`FINE_STEP`].
const STEP: f64 = 1.0;
const FINE_STEP: f64 = 0.2;

/// The bar of a key send held: where it was pressed and the level shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyDrag {
    pub track: TrackId,
    pub id: SendId,
    pub plugin: PluginId,
    start: f64,
    pub value: f64,
    x0: f32,
    t0: f32,
    w: f32,
}

impl Studio {
    /// A press on a key send's bar: the level goes where it is pressed, and follows.
    pub(crate) fn key_down(&mut self, g: &Geometry, hit: Hit, x: f32) -> bool {
        let Hit::Mixer(h @ MixerHit::KeyLevel(t, id)) = hit else {
            return false;
        };
        let (Some(zone), Some(k)) = (self.mixer_rect(g, h), key_send(&self.project(), t, id))
        else {
            return false;
        };
        let bar = key_bar(zone);
        let w = bar.w.max(1.0);
        self.mixer.key_drag = Some(KeyDrag {
            track: t,
            id,
            plugin: k.plugin,
            start: k.level_db,
            value: k.level_db,
            x0: x,
            t0: ((x - bar.x) / w).clamp(0.0, 1.0),
            w,
        });
        self.key_move(x, false);
        true
    }

    /// The pointer moved with a key send's bar held; `fine` (Shift) moves a quarter as far.
    pub(crate) fn key_move(&mut self, x: f32, fine: bool) -> bool {
        let Some(mut d) = self.mixer.key_drag else {
            return false;
        };
        let dx = (x - d.x0) / d.w * if fine { 0.25 } else { 1.0 };
        let db = bar_db(d.t0 + dx);
        if db == d.value {
            return false;
        }
        d.value = db;
        self.mixer.key_drag = Some(d);
        if self.transport.is_live() {
            let mut p = (*self.project()).clone();
            set_level(&mut p, d.plugin, d.track, db);
            let heard = self.auto_heard(&p);
            self.transport.sync(&heard, self.song().dir());
        }
        true
    }

    /// A key send's bar let go: one step when the level moved.
    pub(crate) fn key_up(&mut self) -> bool {
        let Some(d) = self.mixer.key_drag.take() else {
            return false;
        };
        if d.value == d.start {
            let p = self.project();
            self.transport.sync(&p, self.song().dir());
            return true;
        }
        self.set_key_level(d.plugin, d.track, d.value);
        true
    }

    /// The level a key send's bar shows: the held one's, else the song's.
    pub fn key_level(&self, t: TrackId, id: SendId) -> Option<f64> {
        if let Some(d) = self.mixer.key_drag.filter(|d| d.track == t && d.id == id) {
            return Some(d.value);
        }
        key_send(&self.project(), t, id).map(|k| k.level_db)
    }

    /// ←→ on the key send's bar pressed last: 1 dB, with Shift 0.2. Esc lets it go.
    pub(crate) fn key_bar_key(&mut self, key: &str, m: Mods) -> bool {
        let Some((t, id)) = self.mixer.key_focus else {
            return false;
        };
        if !self.mixer_shown() || self.popup.is_some() || m.ctrl || m.alt {
            return false;
        }
        let Some(k) = key_send(&self.project(), t, id) else {
            self.mixer.key_focus = None;
            return false;
        };
        let step = if m.shift { FINE_STEP } else { STEP };
        let by = match key {
            "left" => -step,
            "right" => step,
            "escape" => {
                self.mixer.key_focus = None;
                return true;
            }
            _ => return false,
        };
        // by tenths, so that five steps of 0.2 make a whole dB
        let to = (clamp(k.level_db + by) * 10.0).round() / 10.0;
        if to != k.level_db {
            self.set_key_level(k.plugin, t, to);
        }
        true
    }

    /// One step: the key `t` sends `plugin` at `db`.
    fn set_key_level(&mut self, plugin: PluginId, t: TrackId, db: f64) {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "plugin.sidechain.set",
            json!({ "id": plugin.to_string(), "from": t.to_string(), "level": db }),
        );
    }

    /// The receiving plug-ins whose gain reduction the mixer shows: those keyed and listening
    /// while it shows.
    fn gr_wanted(&self, p: &Project) -> Vec<PluginId> {
        if !self.mixer_shown() {
            return Vec::new();
        }
        p.tracks
            .values()
            .flat_map(|t| &t.inserts)
            .filter(|x| {
                x.sidechain
                    .as_ref()
                    .is_some_and(|sc| sc.on && !sc.is_empty())
            })
            .map(|x| x.id)
            .collect()
    }

    /// The plug-ins [`Studio::poll_gr`] reads the probes of: those it shows and those it let go.
    pub(crate) fn gr_ids(&self, p: &Project) -> Vec<PluginId> {
        let mut ids = self.gr_wanted(p);
        for id in &self.mixer.gr_watched {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        ids
    }

    /// One frame of the gain reduction of the receiving plug-ins the mixer shows: their probes
    /// are watched while it shows, and let go after unless their window watches them. True
    /// while a meter is down. `probes`: those of [`Studio::gr_ids`].
    pub(crate) fn poll_gr(&mut self, p: &Project, probes: &[(PluginId, Probe)]) -> bool {
        let probe_of = |id: PluginId| {
            probes
                .iter()
                .find(|(x, _)| *x == id)
                .map(|(_, p)| p.clone())
        };
        let want = self.gr_wanted(p);
        for id in &self.mixer.gr_watched {
            if !want.contains(id)
                && !self.instruments.is_open(*id)
                && let Some(probe) = probe_of(*id)
            {
                probe.watch(false);
            }
        }
        self.mixer.gr.retain(|id, _| want.contains(id));
        let mut moving = false;
        for id in &want {
            let Some(probe) = probe_of(*id) else {
                continue;
            };
            probe.watch(true);
            let gr = probe.gr_db();
            moving |= gr > 0.0 || self.mixer.gr.get(id).is_some_and(|v| *v > 0.0);
            self.mixer.gr.insert(*id, gr);
        }
        self.mixer.gr_watched = want;
        moving
    }
}

type Probe = std::sync::Arc<xirius_engine::stock::Probe>;

/// `p` with the key `t` sends `plugin` at `db`: what the player hears during a drag.
fn set_level(p: &mut Project, plugin: PluginId, t: TrackId, db: f64) {
    for track in p.tracks.values_mut() {
        for x in std::sync::Arc::make_mut(track)
            .inserts
            .iter_mut()
            .filter(|x| x.id == plugin)
        {
            for s in x
                .sidechain
                .iter_mut()
                .flat_map(|sc| &mut sc.sources)
                .filter(|s| s.track == t)
            {
                if let KeyMode::Send {
                    level_db,
                    muted_level,
                    ..
                } = &mut s.mode
                {
                    match muted_level {
                        Some(held) => *held = db,
                        None => *level_db = db,
                    }
                }
            }
        }
    }
}

/// The rows lit while the pointer is on their partner (MIX-048): a key send's row and the row
/// of the plug-in it keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Partners {
    pub sends: Vec<(TrackId, SendId)>,
    pub plugins: Vec<PluginId>,
}

impl Studio {
    /// What the row under the pointer lights: a key send its receiver, a receiving row the key
    /// sends it is given; each lights itself too.
    pub fn mixer_partners(&self) -> Partners {
        let Hit::Mixer(h) = self.hover else {
            return Partners::default();
        };
        let p = self.project();
        match h {
            MixerHit::KeySend(t, id) | MixerHit::KeyLevel(t, id) => key_send(&p, t, id)
                .map(|k| Partners {
                    sends: vec![(t, id)],
                    plugins: vec![k.plugin],
                })
                .unwrap_or_default(),
            MixerHit::Open(t, id) | MixerHit::Bypass(t, id) => p
                .tracks
                .get(&t)
                .and_then(|track| track.inserts.iter().find(|x| x.id == id))
                .and_then(|x| x.sidechain.as_ref())
                .filter(|sc| !sc.is_empty())
                .map(|sc| Partners {
                    sends: sc
                        .sources
                        .iter()
                        .filter(|s| s.mode.send().is_some())
                        .map(|s| (s.track, s.id))
                        .collect(),
                    plugins: vec![id],
                })
                .unwrap_or_default(),
            _ => Partners::default(),
        }
    }
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
