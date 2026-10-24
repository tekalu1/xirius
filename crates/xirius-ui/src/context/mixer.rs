//! The menus of the mixer (S27 a channel's head, S28 a fader, S29 a knob, S31 an effect,
//! S32 the ＋ of the inserts, S33 a send, S34 the ＋ of the sends, S35 a bus's head, S36 the
//! master, S37 the room right of the last strip).
//!
//! Every row names the channel, the effect or the send it acts on, so the menu does the
//! same wherever the selection is. A plugin that went down and the instrument keep the
//! menus they had (the restart and the isolation; the instrument's own).

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::clipboard::Clipboard;
use xirius_core::id::{PluginId, SendId, TrackId};
use xirius_core::model::{Output, Plugin, Project, TrackKind};
use xirius_prefs::midimap::Target;

use super::{Act, Item};
use crate::arrange::{lane_name, targets_of};
use crate::instrument::window::{PRESETS, is_built_in, preset_of};
use crate::mixer::{List, MixerHit, NEW_BUS, row_at, send_choices, send_parts};
use crate::popup::PopupKind;
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

/// What a right press in the mixer landed on.
enum Spot {
    /// The name of a channel, or any part of it that only selects it.
    Head(TrackId),
    Fader(TrackId),
    Pan(TrackId),
    SendLevel(TrackId, SendId),
    /// A send's name or its before/after glyph.
    Send(TrackId, SendId),
    AddSend(TrackId),
    Plugin(TrackId, PluginId),
    AddInsert(TrackId),
    /// The room right of the last strip.
    Empty,
}

impl Studio {
    /// The right press at a point of the open mixer. False when the point is not the
    /// mixer's, or is a part that has a menu elsewhere (the instrument, a plugin that
    /// went down).
    pub(super) fn mixer_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if !self.mixer_shown() || !g.drawer_body.contains(x, y) {
            return false;
        }
        let Some(spot) = self.mixer_spot(g, x, y) else {
            return false;
        };
        self.focus_arrange(false);
        // The name chooses as a press does; any other part of a track outside the selection acts
        // on that track and leaves the selection as it is (MIX-050).
        let head = matches!(spot, Spot::Head(_))
            && matches!(self.mixer_hit(g, x, y), Some(MixerHit::Strip(_)));
        if let Some(t) = spot_track(&spot)
            && !t.is_master()
            && (head || self.is_picked(RowKind::Track(t)))
        {
            self.select_row(Some(RowKind::Track(t)));
        }
        let items = match spot {
            Spot::Head(t) => self.head_items(g, t),
            Spot::Fader(t) => self.fader_items(t),
            Spot::Pan(t) => self.pan_items(t),
            Spot::SendLevel(t, s) => self.send_level_items(t, s),
            Spot::Send(t, s) => self.send_items(t, s),
            Spot::AddSend(t) => self.add_send_items(t),
            Spot::Plugin(t, id) => self.plugin_items(t, id),
            Spot::AddInsert(t) => self.add_insert_items(g, t),
            Spot::Empty => self.empty_items(),
        };
        if items.is_empty() {
            return false;
        }
        self.open_context(items, x, y);
        true
    }

    fn mixer_spot(&self, g: &Geometry, x: f32, y: f32) -> Option<Spot> {
        let Some(hit) = self.mixer_hit(g, x, y) else {
            return Some(Spot::Empty);
        };
        let p = self.project();
        Some(match hit {
            MixerHit::Fader(t) => Spot::Fader(t),
            MixerHit::Pan(t) => Spot::Pan(t),
            MixerHit::SendLevel(t, s) => Spot::SendLevel(t, s),
            MixerHit::SendPre(t, s) => Spot::Send(t, s),
            MixerHit::AddSend(t) => Spot::AddSend(t),
            MixerHit::AddInsert(t) => Spot::AddInsert(t),
            // The instrument row has its own menu; a plugin that went down keeps the one
            // that restarts it.
            MixerHit::AddInstrument(_) => return None,
            MixerHit::Bypass(t, id) | MixerHit::Open(t, id) => {
                if crate::instrument::is_instrument_of(&p, t, id) || self.health.crashed(id) {
                    return None;
                }
                Spot::Plugin(t, id)
            }
            MixerHit::Strip(t) => match self.send_under(g, t, x, y) {
                Some(s) => Spot::Send(t, s),
                None => Spot::Head(t),
            },
            MixerHit::Mute(t)
            | MixerHit::Solo(t)
            | MixerHit::Arm(t)
            | MixerHit::Output(t)
            | MixerHit::Input(t) => Spot::Head(t),
        })
    }

    /// The send whose row holds the point (its name part).
    fn send_under(&self, g: &Geometry, t: TrackId, x: f32, y: f32) -> Option<SendId> {
        let s = self.mixer_strips(g).into_iter().find(|s| s.track == t)?;
        let list = s.sends.filter(|l| l.contains(x, y))?;
        let i = row_at(list, y)?;
        let k = self.mixer.offset(t, List::Sends) + i;
        let p = self.project();
        let send = p.tracks.get(&t)?.sends.get(k)?;
        let (name, _, _) = send_parts(crate::mixer::list_row(list, i));
        name.contains(x, y).then_some(send.id)
    }

    // ------------------------------------------------------------ the rows' parts

    /// Row "オートメーションを表示" for one lane; on while the lane shows.
    fn lane_item(&self, t: TrackId, target: AutoTarget) -> Item {
        let p = self.project();
        let shown = p.tracks.get(&t).is_some_and(|track| {
            self.arrange.open.contains(&t)
                && track
                    .automation
                    .iter()
                    .any(|l| !l.hidden && l.target == target)
        });
        Item::act(
            "オートメーションを表示",
            "",
            Act::Lane(t, target.to_string()),
        )
        .on(shown)
    }

    /// Row "すべてのポイントを削除" for one lane: dim while the lane has no points.
    fn lane_clear_item(&mut self, t: TrackId, target: AutoTarget) -> Item {
        self.ctx_as(
            "すべてのポイントを削除",
            "auto.clear",
            json!({ "track": t.to_string(), "target": target.to_string() }),
        )
    }

    // ------------------------------------------------------------ S27 S35 S36 a head

    fn head_items(&mut self, g: &Geometry, t: TrackId) -> Vec<Item> {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return Vec::new();
        };
        let id = t.to_string();
        let ids = vec![id.clone()];
        let lanes = self.lane_rows(&p, t);
        let _ = g;
        match track.kind {
            TrackKind::Master => {
                let mixdown = self.ctx_key(g, "export.mixdown");
                let mut rows = self.channel_copy_items(t);
                rows.extend([
                    mixdown,
                    Item::gap(),
                    Item::sub("オートメーションを表示", lanes),
                ]);
                rows
            }
            TrackKind::Bus => {
                let rename = self.rename_item("track.rename", id.clone(), &track.name);
                let color = Item::colors("既定", "track.color.set", json!({ "ids": ids }), true)
                    .current(crate::draw::track_color(track));
                let sources = !self.sources_of(&p, t).is_empty();
                let duplicate = self.ctx_as(
                    &self.label_of("track.duplicate"),
                    "track.duplicate",
                    json!({ "ids": ids, "mode": "settings" }),
                );
                let delete = self.ctx_cmd("track.delete", json!({ "ids": ids }));
                let mut rows = self.channel_copy_items(t);
                rows.extend([
                    rename,
                    color,
                    Item::act("送っているチャンネルを選択", "", Act::SelectSources(t))
                        .enabled(sources),
                    duplicate,
                    Item::act("トラックの列で表示", "", Act::Reveal(t)),
                    Item::gap(),
                    Item::sub("オートメーションを表示", lanes),
                    Item::gap(),
                    Item::act("このバスを書き出す…", "", Act::Stems(vec![t])),
                    Item::gap(),
                    delete,
                ]);
                rows
            }
            TrackKind::Audio | TrackKind::Instrument => {
                let rename = self.rename_item("track.rename", id.clone(), &track.name);
                let color = Item::colors("既定", "track.color.set", json!({ "ids": ids }), true)
                    .current(crate::draw::track_color(track));
                let duplicate = self.duplicate_item(ids.clone());
                let stems = self.stems_item(vec![t]);
                let delete = self.ctx_cmd("track.delete", json!({ "ids": ids }));
                let mut rows = self.channel_copy_items(t);
                rows.extend([
                    rename,
                    color,
                    duplicate,
                    Item::act("トラックの列で表示", "", Act::Reveal(t)),
                    Item::gap(),
                ]);
                rows.extend(self.preset_items(t));
                let freeze = self.freeze_item(t);
                rows.extend([
                    Item::sub("オートメーションを表示", lanes),
                    Item::gap(),
                    freeze,
                    stems,
                    Item::gap(),
                    delete,
                ]);
                rows
            }
        }
    }

    /// 無効にする / 有効にする: the plugin stops using the CPU and keeps what it was set to
    /// (ENG-008). Not the bypass, which keeps it loaded.
    pub(crate) fn power_item(&mut self, id: PluginId, deactivated: bool) -> Item {
        let pid = json!({ "ids": [id.to_string()] });
        if deactivated {
            self.ctx_as("有効にする", "plugin.activate", pid)
        } else {
            self.ctx_as("無効にする", "plugin.deactivate", pid)
        }
    }

    /// トラックをフリーズ / フリーズを解除 (EXP-004), as the track's own menu has them.
    pub(crate) fn freeze_item(&mut self, t: TrackId) -> Item {
        let ids = json!([t.to_string()]);
        let Some(track) = self.project().tracks.get(&t).cloned() else {
            return Item::gap();
        };
        if track.frozen.is_some() {
            return self.ctx_cmd("track.unfreeze", json!({ "ids": ids }));
        }
        let can =
            track.has_clips() && !(track.kind == TrackKind::Instrument && track.midi_out.is_some());
        self.ctx_session(
            "トラックをフリーズ",
            "track.freeze",
            "track.freeze",
            json!({ "ids": ids }),
            can,
            "フリーズできません",
        )
    }

    /// The right-click menu of a row of the CPU list.
    pub(crate) fn cpu_items(&mut self, t: TrackId, id: PluginId) -> Vec<Item> {
        let deactivated = self
            .project()
            .tracks
            .get(&t)
            .and_then(|x| x.inserts.iter().find(|p| p.id == id))
            .is_some_and(|p| p.deactivated);
        let power = self.power_item(id, deactivated);
        let freeze = self.freeze_item(t);
        vec![
            Item::act("画面を開く", "", Act::OpenPlugin(t, id)),
            power,
            Item::gap(),
            freeze,
        ]
    }

    /// The channels that play into a bus: by their output or by a send.
    fn sources_of(&self, p: &Project, bus: TrackId) -> Vec<TrackId> {
        p.tracks
            .values()
            .filter(|x| x.id != bus && x.kind != TrackKind::Master)
            .filter(|x| x.output == Some(Output::Bus(bus)) || x.sends.iter().any(|s| s.to == bus))
            .map(|x| x.id)
            .collect()
    }

    // ------------------------------------------------------------ S28 S29 a fader or a knob

    fn fader_items(&mut self, t: TrackId) -> Vec<Item> {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return Vec::new();
        };
        // Every selected track takes the value (UI-340).
        let targets = self.track_ids(t);
        let ids = json!({ "ids": targets });
        let reset = self.ctx_as(
            "0 dB に戻す",
            "track.volume.set",
            json!({ "ids": targets, "db": 0.0 }),
        );
        let now = format!("{:.1}", track.volume_db);
        let clear = self.lane_clear_item(t, AutoTarget::Volume);
        let held = crate::auto_lock::locked(track, &AutoTarget::Volume);
        vec![
            crate::auto_lock::gate(reset, held),
            crate::auto_lock::gate(
                Item::ask_number("値を入力…", "track.volume.set", ids, "db", &now),
                held,
            ),
            Item::gap(),
            self.lane_item(t, AutoTarget::Volume),
            Item::gap(),
            clear,
            Item::gap(),
        ]
        .into_iter()
        .chain(self.midi_items(t, false))
        .collect()
    }

    /// The rows of a part's controller: wait for a knob, and let the knob go (OPS-007).
    fn midi_items(&mut self, t: TrackId, pan: bool) -> Vec<Item> {
        let target = if pan {
            Target::Pan(t.to_string())
        } else {
            Target::Volume(t.to_string())
        };
        let learning = self.live_midi.assign.learning.as_ref() == Some(&target);
        let learn = if learning {
            Item::act("MIDI 学習", "", Act::MidiLearnCancel).on(true)
        } else {
            Item::act("MIDI 学習", "", Act::MidiLearn(target.to_string()))
        };
        let assigned = self.midimap().of_target(&target).is_some();
        let unset = self
            .ctx_as(
                "MIDI 割り当てを解除",
                "midimap.unset",
                json!({ "target": target.to_string() }),
            )
            .enabled(assigned);
        vec![learn, unset]
    }

    fn pan_items(&mut self, t: TrackId) -> Vec<Item> {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return Vec::new();
        };
        // Every selected track takes the value (UI-340).
        let targets = self.track_ids(t);
        let ids = json!({ "ids": targets });
        let reset = self.ctx_as(
            "既定に戻す",
            "track.pan.set",
            json!({ "ids": targets, "pan": 0.0 }),
        );
        let now = format!("{:.2}", track.pan);
        let clear = self.lane_clear_item(t, AutoTarget::Pan);
        let held = crate::auto_lock::locked(track, &AutoTarget::Pan);
        vec![
            crate::auto_lock::gate(reset, held),
            crate::auto_lock::gate(
                Item::ask_number("値を入力…", "track.pan.set", ids, "pan", &now),
                held,
            ),
            Item::gap(),
            self.lane_item(t, AutoTarget::Pan),
            Item::gap(),
            clear,
            Item::gap(),
        ]
        .into_iter()
        .chain(self.midi_items(t, true))
        .collect()
    }

    fn send_level_items(&mut self, t: TrackId, s: SendId) -> Vec<Item> {
        let p = self.project();
        let Some(send) = p
            .tracks
            .get(&t)
            .and_then(|x| x.sends.iter().find(|x| x.id == s))
        else {
            return Vec::new();
        };
        let id = s.to_string();
        let reset = self.ctx_as("0 dB に戻す", "send.level.reset", json!({ "id": id }));
        let now = format!("{:.1}", crate::mixer::send_level_db(send));
        let target = AutoTarget::Send(s);
        let clear = self.lane_clear_item(t, target.clone());
        let held = p
            .tracks
            .get(&t)
            .is_some_and(|x| crate::auto_lock::locked(x, &target));
        vec![
            crate::auto_lock::gate(reset, held),
            crate::auto_lock::gate(
                Item::ask_number("値を入力…", "send.set", json!({ "id": id }), "level", &now),
                held,
            ),
            Item::gap(),
            self.lane_item(t, target),
            Item::gap(),
            clear,
        ]
    }

    // ------------------------------------------------------------ S33 S34 the sends

    fn send_items(&mut self, t: TrackId, s: SendId) -> Vec<Item> {
        let p = self.project();
        let Some(send) = p
            .tracks
            .get(&t)
            .and_then(|x| x.sends.iter().find(|x| x.id == s))
        else {
            return Vec::new();
        };
        let id = s.to_string();
        let level = self.ctx_as("0 dB に戻す", "send.level.reset", json!({ "id": id }));
        let mut remove = self.ctx_as("センドを削除", "send.remove", json!({ "ids": [id] }));
        remove.key = self.key_display("mixer", "edit.delete");
        self.input_map.context_delete = Some(("send.remove", json!({"ids":[id]})));
        let mut targets: Vec<Item> = p
            .tracks
            .values()
            .filter(|b| b.kind == TrackKind::Bus)
            .filter(|b| crate::mixer::send_ok(&p, t, b.id))
            .map(|b| {
                let item = self
                    .ctx_as(
                        &b.name,
                        "send.target.set",
                        json!({"id": id, "to": b.id.to_string()}),
                    )
                    .on(b.id == send.to);
                if b.id == send.to {
                    item.enabled(true)
                } else {
                    item
                }
            })
            .collect();
        targets.push(Item::gap());
        targets.push(self.ctx_as(
            "新しいバス",
            "send.target.set",
            json!({"id": id, "to": "new"}),
        ));
        let mute = self
            .ctx_cmd("send.mute", json!({"id": id}))
            .on(send.muted_level.is_some());
        vec![
            Item::sub("送り先", targets),
            level,
            mute,
            Item::act("送り先のバスを表示", "", Act::ShowStrip(send.to)),
            Item::gap(),
            self.lane_item(t, AutoTarget::Send(s)),
            Item::gap(),
            remove,
        ]
    }

    fn add_send_items(&mut self, t: TrackId) -> Vec<Item> {
        let p = self.project();
        let mut rows = Vec::new();
        let mut new_bus = None;
        for c in send_choices(&p, t) {
            if c.to.is_empty() {
                continue;
            }
            if c.to == NEW_BUS {
                let name = format!("バス {}", crate::mixer::strip_order(&p).len());
                new_bus = Some(self.ctx_as(
                    &c.label,
                    "send.add.bus",
                    json!({ "ids": [t.to_string()], "name": name, "into": "root" }),
                ));
            } else {
                rows.push(self.ctx_as(
                    &c.label,
                    "send.add",
                    json!({ "ids": [t.to_string()], "to": c.to }),
                ));
            }
        }
        if let Some(n) = new_bus {
            if !rows.is_empty() {
                rows.push(Item::gap());
            }
            rows.push(n);
        }
        rows
    }

    // ------------------------------------------------------------ S31 S32 the inserts

    fn plugin_items(&mut self, t: TrackId, id: PluginId) -> Vec<Item> {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return Vec::new();
        };
        let Some(plugin) = track.inserts.iter().find(|x| x.id == id) else {
            return Vec::new();
        };
        let pid = id.to_string();
        let builtin = is_built_in(plugin);
        let now = preset_of(plugin);
        let mut presets: Vec<Item> = Vec::new();
        if builtin {
            presets.extend(
                PRESETS
                    .iter()
                    .map(|n| Item::act(n, "", Act::Preset(id, (*n).to_string())).on(*n == now)),
            );
            presets.push(Item::gap());
        }
        presets.push(Item::act(
            &self.label_of("plugin.preset.open"),
            "",
            Act::PresetFile(id, false),
        ));
        presets.push(Item::act(
            &self.label_of("plugin.preset.save"),
            "",
            Act::PresetFile(id, true),
        ));
        let duplicate = self.ctx_as("プラグインを複製", "plugin.duplicate", json!({ "id": pid }));
        let others = |me: &mut Studio, name: &'static str| -> Vec<Item> {
            crate::mixer::strip_order(&p)
                .into_iter()
                .filter(|c| *c != t)
                .filter_map(|c| p.tracks.get(&c))
                .map(|c| {
                    me.ctx_as(
                        &c.name,
                        name,
                        json!({ "id": pid, "track": c.id.to_string() }),
                    )
                })
                .collect()
        };
        let moves = others(self, "plugin.move");
        let copies = others(self, "plugin.copy_to");
        let lanes = self.plugin_lanes(&p, t, plugin);
        let mut cut = self.ctx_as("切り取り", "edit.cut", json!({ "plugin": pid }));
        cut.key = self.menu_key("edit.cut");
        // A copy changes nothing in the song, so the dry run would dim it.
        let mut copy = self
            .ctx_as("コピー", "edit.copy", json!({ "plugin": pid }))
            .enabled(true);
        copy.key = self.menu_key("edit.copy");
        let mut remove = self.ctx_as("プラグインを削除", "plugin.remove", json!({ "ids": [pid] }));
        remove.key = self.key_display("mixer", "edit.delete");
        self.input_map.context_delete = Some(("plugin.remove", json!({"ids":[pid]})));
        self.input_map.context_clipboard = Some(json!({"plugin":pid}));
        let power = self.power_item(id, plugin.deactivated);
        let mut restart = Item::act("再起動", "", Act::RestartPlugin(id));
        restart.enabled = self.health.crashed(id);
        restart.reason = "クラッシュしたプラグインだけ再起動できます".into();
        vec![
            Item::act("画面を開く", "", Act::OpenPlugin(t, id)),
            Item::sub("プリセット", presets),
            // A plugin that went down has the menu of PLG-053 instead; this one is for the
            // ones that run.
            restart,
            power,
            duplicate,
            Item::sub("別のチャンネルへ移動", moves),
            Item::sub("別のチャンネルへ写す", copies),
            Item::gap(),
            Item::sub("オートメーションを表示", lanes),
            Item::gap(),
            cut,
            copy,
            Item::gap(),
            remove,
        ]
    }

    /// The parameters of one plugin that can have a lane.
    fn plugin_lanes(&self, p: &Project, t: TrackId, plugin: &Plugin) -> Vec<Item> {
        let Some(track) = p.tracks.get(&t) else {
            return Vec::new();
        };
        let prefix = format!("{} · ", plugin.name);
        targets_of(track)
            .into_iter()
            .filter(|x| matches!(x, AutoTarget::Param(pid, _) if *pid == plugin.id))
            .map(|x| {
                let name = lane_name(p, track, &x);
                let name = name.strip_prefix(&prefix).unwrap_or(&name).to_string();
                let mut row = self.lane_item(t, x);
                row.label = name;
                row
            })
            .collect()
    }

    fn add_insert_items(&mut self, g: &Geometry, t: TrackId) -> Vec<Item> {
        let _ = g;
        let _ = t;
        let board = matches!(
            self.song.session().state.clipboard,
            Some(Clipboard::Plugins(_))
        );
        let mut paste = self.ctx_as("貼り付け", "edit.paste", json!({ "track": t.to_string() }));
        paste.key = self.menu_key("edit.paste");
        let paste_on = board && paste.enabled;
        let recent: Vec<Item> = self
            .recent_effects()
            .into_iter()
            .map(|c| Item::act(&c.label, "", Act::InsertRecent(t, c.to)))
            .collect();
        let mut rows = vec![Item::act("エフェクトを追加…", "", Act::PickInsert(t))];
        if !recent.is_empty() {
            rows.push(Item::sub("最近使ったもの", recent));
        }
        rows.push(paste.enabled(paste_on));
        rows
    }

    // ------------------------------------------------------------ S37 the room

    fn empty_items(&mut self) -> Vec<Item> {
        let add = self.add_rows(json!({}));
        let bus = add.into_iter().find(|i| i.label == "バス");
        let audio = self.ctx_as("オーディオ", "track.add.audio", json!({}));
        let inst = self.ctx_as("インストゥルメント", "track.add.instrument", json!({}));
        let mut out = Vec::new();
        if let Some(b) = bus {
            out.push(b.labelled("バスを追加"));
        }
        let mut kinds = vec![audio, inst];
        kinds.extend(self.preset_add_rows(&json!({})));
        out.push(Item::sub("トラックを追加", kinds));
        // `edit.paste` takes tracks, and an effect (it goes after the one it was copied from).
        let board = matches!(
            self.song.session().state.clipboard,
            Some(Clipboard::Tracks { .. } | Clipboard::Plugins(_))
        );
        let mut paste = self.ctx_as("貼り付け", "edit.paste", json!({}));
        paste.enabled = board;
        paste.key = self.menu_key("edit.paste");
        out.push(paste);
        let wide = self.song.session().state.view.mixer_wide;
        out.push(Item::sub(
            "チャンネルの幅",
            vec![
                self.ctx_as("細い", "view.channel.width", json!({"width":"narrow"}))
                    .enabled(true)
                    .on(!wide),
                self.ctx_as("広い", "view.channel.width", json!({"width":"wide"}))
                    .enabled(true)
                    .on(wide),
            ],
        ));
        out
    }

    // ------------------------------------------------------------ what the rows do

    /// Shows the channel's row in the track column.
    pub(super) fn reveal_row(&mut self, g: &Geometry, t: TrackId) -> bool {
        let Some(row) = self.tree().row_of(RowKind::Track(t)).cloned() else {
            return false;
        };
        let top = row.y;
        let bottom = row.y + row.h;
        if top < self.scroll.1 {
            self.scroll.1 = top;
        } else if bottom > self.scroll.1 + g.column.h {
            self.scroll.1 = bottom - g.column.h;
        }
        self.clamp_scroll(g);
        self.pick_row(RowKind::Track(t), crate::tools::Mods::NONE);
        true
    }

    /// Scrolls the mixer so the channel's strip shows.
    pub(super) fn show_strip(&mut self, g: &Geometry, t: TrackId) -> bool {
        let Some(i) = crate::mixer::strip_order(&self.project())
            .iter()
            .position(|x| *x == t)
        else {
            return false;
        };
        if t.is_master() {
            // The master does not scroll: it is always at the right edge.
            return true;
        }
        let w = crate::mixer::STRIP_W
            * if self.song.session().state.view.mixer_wide {
                1.4
            } else {
                1.0
            }
            + crate::mixer::STRIP_GAP;
        let left = i as f32 * w;
        let view = self.mixer_view(g).w;
        if left < self.mixer.scroll {
            self.mixer.scroll = left;
        } else if left + w > self.mixer.scroll + view {
            self.mixer.scroll = left + w - view;
        }
        self.mixer.scroll = self.mixer.scroll.clamp(0.0, self.mixer_max_scroll(g));
        true
    }

    /// The bottom drawer becomes the mixer, with the channel selected, in view and lit for a
    /// moment (a double click on the track's head).
    pub(super) fn focus_strip(&mut self, g: &Geometry, t: TrackId) -> bool {
        if !crate::mixer::strip_order(&self.project()).contains(&t) {
            return false;
        }
        self.drawer_open = true;
        self.tab = crate::studio::DrawerTab::Mixer;
        self.resized(g);
        // A shut drawer slides open: the strip is brought into the mixer it opens to.
        let g = self.settled_geometry(g.size.0, g.size.1);
        self.show_strip(&g, t);
        if !t.is_master() {
            self.pick_row(RowKind::Track(t), crate::tools::Mods::NONE);
        }
        self.mixer.flash_strip(t, xirius_core::store::now_ms());
        true
    }

    /// The channels that play into the bus become the selection.
    pub(super) fn select_sources(&mut self, bus: TrackId) -> bool {
        let p = self.project();
        let rows: Vec<RowKind> = self
            .sources_of(&p, bus)
            .into_iter()
            .map(RowKind::Track)
            .collect();
        if rows.is_empty() {
            return false;
        }
        self.select_row(rows.first().copied());
        self.arrange.tracks = rows;
        self.arrange.touched();
        true
    }

    /// Opens the list of effects under the ＋ row of the channel.
    /// An effect used lately goes to the end of the channel, as a pick from the list does.
    pub(super) fn insert_recent(&mut self, t: TrackId, to: &str) -> bool {
        let Some(pick) = self.effect_pick(to) else {
            return false;
        };
        let end = self.project().tracks.get(&t).map_or(0, |x| x.inserts.len());
        self.add_effect(t, end, &pick)
    }

    pub(super) fn pick_insert(&mut self, g: &Geometry, t: TrackId) -> bool {
        let Some(anchor) = self.mixer_rect(g, MixerHit::AddInsert(t)) else {
            return false;
        };
        self.open_popup(PopupKind::Insert(t), anchor);
        true
    }
}

fn spot_track(spot: &Spot) -> Option<TrackId> {
    match spot {
        Spot::Head(t)
        | Spot::Fader(t)
        | Spot::Pan(t)
        | Spot::SendLevel(t, _)
        | Spot::Send(t, _)
        | Spot::AddSend(t)
        | Spot::Plugin(t, _)
        | Spot::AddInsert(t) => Some(*t),
        Spot::Empty => None,
    }
}

#[allow(dead_code)]
fn _kinds(_: Value) {}
