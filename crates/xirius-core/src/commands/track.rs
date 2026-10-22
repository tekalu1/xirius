//! Tracks, buses, folders, sends and plugins: everything of the track column
//! and the signal flow of a channel.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::auto::AutoTarget;
use crate::command::EditCtx;
use crate::id::{FolderId, IdKind, PluginId, SendId, TrackId};
use crate::model::{
    AuxSend, Folder, MAX_DB, MIN_DB, MidiOut, Output as Route, Plugin, PluginIsolation, PluginKind,
    Track, TrackIcon, TrackKind,
};

pub(super) fn defs() -> Vec<CommandDef> {
    let where_args = || {
        vec![
            ArgDef::text("name", "名前（省略時は自動で付けます）"),
            ArgDef::text(
                "into",
                "追加先のフォルダーの ID（root または省略時は最上位）",
            )
            .default(json!("root")),
            ArgDef::int("index", "フォルダー内の位置（0 が先頭。省略時は末尾）"),
        ]
    };
    let mut v = vec![
        edit(
            "track.add.audio",
            "オーディオトラックを追加します",
            where_args(),
            |p, _, a| add_track(p, a, TrackKind::Audio),
        ),
        edit(
            "track.add.instrument",
            "インストゥルメント（MIDI）トラックを追加します",
            where_args(),
            |p, _, a| add_track(p, a, TrackKind::Instrument),
        ),
        edit(
            "track.add.bus",
            "バスを追加します",
            [
                where_args(),
                vec![
                    {
                        let mut d = ArgDef::ids(IdKind::Track, "まとめるチャンネル");
                        d.name = "sources";
                        d
                    },
                    ArgDef::text(
                        "plugin",
                        "新しいバスに追加するエフェクトの名前（1 つの操作です）",
                    ),
                    ArgDef::text("format", "--plugin の形式（vst3・clap・builtin など）")
                        .default(json!("builtin")),
                    ArgDef::text("uid", "--plugin の固有 ID（省略時は名前）"),
                ],
            ]
            .concat(),
            |p, _, a| super::bus_group::bus(p, a),
        ),
        destroy(
            "track.delete",
            "トラックを削除します（バスを削除すると、そのバスへの出力はマスターに戻ります）",
            vec![ArgDef::ids(IdKind::Track, "削除するトラック")],
            delete_tracks,
        ),
        edit(
            "track.rename",
            "トラックの名前を変更します",
            vec![
                ArgDef::id("id", IdKind::Track, "トラック").req(),
                ArgDef::text("name", "新しい名前").req(),
            ],
            |p, cx, a| {
                let id: TrackId = a.id("id")?;
                let name = name_arg(a)?;
                p.track_mut(id)?.name = name.clone();
                let _ = cx;
                Ok(Output::msg(format!(
                    "{id} の名前を「{name}」に変更しました"
                )))
            },
        ),
        edit(
            "track.duplicate",
            "トラックを複製します。--mode settings では設定だけをコピーします。--into を指定すると、そのフォルダーに配置します",
            vec![
                ArgDef::ids(IdKind::Track, "複製するトラック"),
                ArgDef::choice("mode", &["all", "settings"], "中身ごと、または設定だけ")
                    .default(json!("all")),
                ArgDef::text(
                    "into",
                    "複製の配置先のフォルダーの ID または root（省略時は元のトラックの直後）",
                ),
                ArgDef::int("index", "--into のフォルダー内の位置（省略時は末尾）"),
            ],
            duplicate_tracks,
        ),
        edit(
            "track.mute",
            "ミュートを切り替えます（--on / --off。省略時は現在の状態を反転）",
            [
                vec![ArgDef::ids(IdKind::Track, "トラック")],
                on_off().to_vec(),
            ]
            .concat(),
            |p, _, a| set_flag(p, a, true, "ミュート", |t| &mut t.mute),
        ),
        edit(
            "track.solo",
            "ソロを切り替えます（--on / --off。省略時は現在の状態を反転）",
            [
                vec![ArgDef::ids(IdKind::Track, "トラック")],
                on_off().to_vec(),
            ]
            .concat(),
            |p, _, a| set_flag(p, a, false, "ソロ", |t| &mut t.solo),
        ),
        edit(
            "track.arm",
            "録音準備を切り替えます（--on / --off。省略時は現在の状態を反転）",
            [
                vec![ArgDef::ids(IdKind::Track, "トラック")],
                on_off().to_vec(),
            ]
            .concat(),
            |p, _, a| set_flag(p, a, false, "録音準備", |t| &mut t.armed),
        ),
        edit(
            "track.monitor",
            "入力のモニター（録音準備中に入力の音を出力で聴く）を切り替えます（--on / --off。省略時は現在の状態を反転）",
            [
                vec![ArgDef::ids(IdKind::Track, "トラック")],
                on_off().to_vec(),
            ]
            .concat(),
            |p, _, a| set_flag(p, a, false, "入力モニター", |t| &mut t.monitor),
        ),
        edit(
            "track.input.gain",
            "入力のゲイン（トリム）を dB で設定します",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::float("db", "dB（-24〜24）")
                    .req()
                    .range(-24.0, 24.0),
            ],
            |p, _, a| {
                let db = a.f64("db")?;
                let ids = track_ids(p, a, false)?;
                for id in &ids {
                    p.track_mut(*id)?.input_gain_db = db;
                }
                Ok(Output::msg(format!(
                    "{} トラックの入力ゲインを {db} dB に設定しました",
                    ids.len()
                )))
            },
        ),
        edit(
            "track.volume.set",
            "ボリューム（フェーダー）を dB で設定します",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::float("db", "dB（-120〜12）")
                    .req()
                    .range(MIN_DB, MAX_DB),
            ],
            |p, _, a| {
                let db = a.f64("db")?;
                let ids = track_ids(p, a, true)?;
                for id in &ids {
                    p.track_mut(*id)?.volume_db = db;
                }
                Ok(Output::msg(format!(
                    "{} トラックのボリュームを {db} dB に設定しました",
                    ids.len()
                )))
            },
        ),
        edit(
            "track.pan.set",
            "パンを設定します（-1 が左、1 が右）",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::float("pan", "-1〜1").req().range(-1.0, 1.0),
            ],
            |p, _, a| {
                let pan = a.f64("pan")?;
                let ids = track_ids(p, a, true)?;
                for id in &ids {
                    p.track_mut(*id)?.pan = pan;
                }
                Ok(Output::msg(format!(
                    "{} トラックのパンを {pan} に設定しました",
                    ids.len()
                )))
            },
        ),
        edit(
            "track.transpose",
            "トラックの音の高さを半音で設定します（画面のノートは動かさず、鳴らすときに足します。クリップのトランスポーズと足し合わせます）。範囲（−64〜64）の外は丸めます",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::int("semis", "トランスポーズ（半音。範囲の外は丸めます）")
                    .req()
                    .clamp_range(-f64::from(Track::SEMIS), f64::from(Track::SEMIS)),
            ],
            |p, _, a| {
                let semis = a.i64("semis")?;
                let ids = set_track_value(p, a, |t| t.set_transpose(semis))?;
                let v = p.track(ids[0])?.transpose;
                Ok(Output::msg(format!(
                    "{} トラックのトランスポーズを {} 半音にしました",
                    ids.len(),
                    signed(v)
                )))
            },
        ),
        edit(
            "track.velocity",
            "トラックのベロシティを % で設定します（画面のノートは変えず、鳴らすときに足します。−100% で 0、+100% で 2 倍になり、1〜127 に丸めます）。範囲（−100〜100）の外は丸めます",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::int("percent", "ベロシティ（%。範囲の外は丸めます）")
                    .req()
                    .clamp_range(-f64::from(Track::VELOCITY), f64::from(Track::VELOCITY)),
            ],
            |p, _, a| {
                let percent = a.i64("percent")?;
                let ids = set_track_value(p, a, |t| t.set_velocity(percent))?;
                let v = p.track(ids[0])?.velocity;
                Ok(Output::msg(format!(
                    "{} トラックのベロシティを {}% にしました",
                    ids.len(),
                    signed(v)
                )))
            },
        ),
        edit(
            "track.icon.set",
            "トラックのアイコンを設定します",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::choice(
                    "icon",
                    &["drums", "keys", "mic", "wave", "none"],
                    "アイコン（none でトラックの種類の既定に戻す）",
                )
                .req(),
            ],
            set_icon,
        ),
        edit(
            "track.color.set",
            "トラックかフォルダーの色を設定します",
            vec![
                ArgDef::ids(IdKind::Node, "トラックかフォルダー"),
                ArgDef::text("color", "#rrggbb または none").req(),
            ],
            set_color,
        ),
        edit(
            "track.input.set",
            "入力を割り当てます（none で解除）",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::text("input", "入力の名前（none で解除）").req(),
            ],
            |p, _, a| {
                let input = a.str("input")?;
                let input = (input != "none" && !input.is_empty()).then(|| input.to_string());
                let ids = track_ids(p, a, false)?;
                for id in &ids {
                    p.track_mut(*id)?.input = input.clone();
                }
                Ok(Output::msg(format!(
                    "{} トラックの入力を設定しました",
                    ids.len()
                )))
            },
        ),
        edit(
            "track.output.set",
            "出力先を設定します（master・バスの ID・default＝フォルダーの設定に従う・midi:ポート名＝MIDI 出力ポート）",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::text("to", "master・バスの ID・default・midi:ポート名 のいずれか").req(),
                ArgDef::int(
                    "channel",
                    "midi: のときの MIDI チャンネル（1〜16。省略時は 1）",
                ),
            ],
            set_output,
        ),
        edit(
            "track.move",
            "トラックやフォルダーを移動します。--into でフォルダーの中へ（root で最上位へ）、--out で 1 つ上の階層へ、--by で同じ階層の中で上下に移動",
            vec![
                ArgDef::ids(IdKind::Node, "移動するトラックかフォルダー"),
                ArgDef::text("into", "移動先のフォルダーの ID または root"),
                ArgDef::int("index", "--into のフォルダー内の位置（省略時は末尾）"),
                ArgDef::flag("out", "1 つ上の階層（親フォルダーの直後）へ移動する"),
                ArgDef::int("by", "同じ階層の中での移動数（負の値で上へ）"),
            ],
            move_nodes,
        ),
        edit(
            "send.add",
            "バスへのセンドを追加します（既定はポストフェーダー）",
            vec![
                ArgDef::ids(IdKind::Track, "送り元のトラック"),
                ArgDef::id("to", IdKind::Track, "送り先のバス").req(),
                ArgDef::float("level", "センド量（dB）")
                    .default(json!(0.0))
                    .range(MIN_DB, MAX_DB),
                ArgDef::flag("pre", "プリフェーダーで送る"),
            ],
            add_send,
        ),
        edit(
            "send.add.bus",
            "新しいバスを作成し、そのバスへのセンドを追加します（1 回の取り消しで両方とも元に戻ります）",
            vec![
                ArgDef::ids(IdKind::Track, "送り元のトラック"),
                ArgDef::text("name", "バスの名前（省略時は自動で付けます）"),
                ArgDef::text(
                    "into",
                    "バスの追加先のフォルダーの ID（root または省略時は最上位）",
                )
                .default(json!("root")),
                ArgDef::float("level", "センド量（dB）")
                    .default(json!(0.0))
                    .range(MIN_DB, MAX_DB),
                ArgDef::flag("pre", "プリフェーダーで送る"),
            ],
            add_send_to_new_bus,
        ),
        destroy(
            "send.remove",
            "センドを削除します",
            vec![ArgDef::ids(IdKind::Send, "センド")],
            |p, _, a| {
                let ids = a.require_ids::<SendId>("センド")?;
                let mut n = 0;
                for tid in p.tracks.keys().copied().collect::<Vec<_>>() {
                    if p.tracks[&tid].sends.iter().any(|s| ids.contains(&s.id)) {
                        let t = p.track_mut(tid)?;
                        let before = t.sends.len();
                        t.sends.retain(|s| !ids.contains(&s.id));
                        t.automation.retain(
                            |l| !matches!(l.target, AutoTarget::Send(id) if ids.contains(&id)),
                        );
                        n += before - t.sends.len();
                    }
                }
                if n != ids.len() {
                    return Err(CmdError::not_found("見つからないセンドがあります"));
                }
                Ok(Output::msg(format!("センド {n} 件を削除しました")))
            },
        ),
        edit(
            "send.set",
            "センドの量やプリ／ポストを変更します",
            vec![
                ArgDef::id("id", IdKind::Send, "センド").req(),
                ArgDef::float("level", "センド量（dB）").range(MIN_DB, MAX_DB),
                ArgDef::choice(
                    "position",
                    &["pre", "post"],
                    "プリフェーダーかポストフェーダーか",
                ),
            ],
            set_send,
        ),
        edit(
            "send.level.reset",
            "センドの量を 0 dB に戻します",
            vec![ArgDef::id("id", IdKind::Send, "センド").req()],
            |p, _, a| change_send(p, a.id("id")?, Some(0.0), None),
        ),
        edit(
            "plugin.add",
            "インサートにプラグインを追加します（インストゥルメントは先頭に1つだけ）",
            vec![
                ArgDef::id("track", IdKind::Track, "トラック").req(),
                ArgDef::text("name", "プラグインの名前").req(),
                ArgDef::choice(
                    "kind",
                    &["effect", "instrument"],
                    "エフェクトかインストゥルメントか",
                )
                .default(json!("effect")),
                ArgDef::text("format", "vst3・clap・builtin など").default(json!("builtin")),
                ArgDef::text("uid", "形式ごとの固有 ID（省略時は名前）"),
                ArgDef::int("index", "インサート内の位置（省略時は末尾）"),
            ],
            add_plugin,
        ),
        destroy(
            "plugin.remove",
            "プラグインを削除します",
            vec![ArgDef::ids(IdKind::Plugin, "プラグイン")],
            |p, _, a| {
                let ids = a.require_ids::<PluginId>("プラグイン")?;
                let mut n = 0;
                for tid in p.tracks.keys().copied().collect::<Vec<_>>() {
                    if p.tracks[&tid].inserts.iter().any(|x| ids.contains(&x.id)) {
                        let t = p.track_mut(tid)?;
                        let before = t.inserts.len();
                        t.inserts.retain(|x| !ids.contains(&x.id));
                        n += before - t.inserts.len();
                    }
                }
                if n != ids.len() {
                    return Err(CmdError::not_found("見つからないプラグインがあります"));
                }
                Ok(Output::msg(format!("プラグイン {n} 件を削除しました")))
            },
        ),
        edit(
            "plugin.bypass",
            "プラグインのバイパスを切り替えます（--on / --off。省略時は現在の状態を反転）",
            [
                vec![ArgDef::ids(IdKind::Plugin, "プラグイン")],
                on_off().to_vec(),
            ]
            .concat(),
            |p, _, a| {
                let ids = a.require_ids::<PluginId>("プラグイン")?;
                let mut state = a.toggle()?;
                for id in &ids {
                    let tid = plugin_track(p, *id)?;
                    let x = p
                        .track_mut(tid)?
                        .inserts
                        .iter_mut()
                        .find(|x| x.id == *id)
                        .expect("found");
                    let on = *state.get_or_insert(!x.bypass);
                    x.bypass = on;
                }
                Ok(Output::msg(format!(
                    "バイパスを{}（{} 件）",
                    onoff_word(state.unwrap_or(false)),
                    ids.len()
                )))
            },
        ),
        edit(
            "plugin.deactivate",
            "プラグインを無効にします（読み込みを止めて CPU を空けます。状態は残り、バイパスとは別です）",
            vec![ArgDef::ids(IdKind::Plugin, "プラグイン")],
            |p, _, a| set_deactivated(p, a, true),
        ),
        edit(
            "plugin.activate",
            "無効にしたプラグインを有効に戻します",
            vec![ArgDef::ids(IdKind::Plugin, "プラグイン")],
            |p, _, a| set_deactivated(p, a, false),
        ),
        edit(
            "plugin.param.set",
            "プラグインのパラメーターを設定します",
            vec![
                ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
                ArgDef::text("param", "パラメーターの名前").req(),
                ArgDef::float(
                    "value",
                    "値（内蔵プラグインと VST3 は 0〜1。--value か --plain のどちらか）",
                ),
                ArgDef::text(
                    "plain",
                    "内蔵プラグインの値を単位で指定します（-6 dB なら -6。0〜1 の割合は --value を使います。選択肢は名前、オン/オフは on/off でも可）",
                ),
            ],
            set_param,
        ),
        read(
            "plugin.param.list",
            "プラグインのパラメーターを一覧表示します（内蔵プラグインは範囲・単位・既定・今の値つき）",
            vec![ArgDef::id("id", IdKind::Plugin, "プラグイン").req()],
            |p, _, _, a| {
                let id: PluginId = a.id("id")?;
                let tid = plugin_track(p, id)?;
                let x = p
                    .track(tid)?
                    .inserts
                    .iter()
                    .find(|x| x.id == id)
                    .expect("found");
                let rows: Vec<Value> = match x.builtin() {
                    Some(def) => def
                        .params
                        .iter()
                        .map(|s| {
                            let norm = x
                                .params
                                .get(s.id)
                                .copied()
                                .unwrap_or_else(|| s.default_norm());
                            let mut row = spec_json(s);
                            row["value"] = json!(norm);
                            row["plain"] = json!(s.to_plain(norm));
                            row["text"] = json!(s.text(s.to_plain(norm)));
                            row
                        })
                        .collect(),
                    None => x
                        .params
                        .iter()
                        .map(|(k, v)| json!({ "id": k, "value": v }))
                        .collect(),
                };
                Ok(Output::with(
                    format!("{}（{}）のパラメーター {} 件", x.name, id, rows.len()),
                    Value::Array(rows),
                ))
            },
        ),
        read(
            "plugin.builtin.list",
            "内蔵のインストゥルメントとエフェクトを、パラメーターの規格つきで一覧表示します",
            vec![],
            |_, _, _, _| {
                let rows: Vec<Value> = xirius_params::BUILTINS
                    .iter()
                    .map(|d| {
                        json!({
                            "uid": d.uid,
                            "name": d.name,
                            "kind": match d.kind {
                                xirius_params::Kind::Instrument => "instrument",
                                xirius_params::Kind::Effect => "effect",
                            },
                            "params": d.params.iter().map(spec_json).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                Ok(Output::with(
                    format!("内蔵プラグイン {} 件", rows.len()),
                    Value::Array(rows),
                ))
            },
        ),
        edit(
            "plugin.state.set",
            "プラグインの保存状態を置き換えます（プラグインが出力したデータを base64 で指定。空にすると削除）",
            vec![
                ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
                ArgDef::text("state", "状態（base64。空の文字列で削除）").req(),
            ],
            |p, _, a| {
                let id: PluginId = a.id("id")?;
                let state = a.str("state")?.trim().to_string();
                let base64 = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=');
                if !state.chars().all(base64) || state.len() % 4 != 0 {
                    return Err(CmdError::invalid(
                        "--state には base64 の文字列を指定してください",
                    ));
                }
                let tid = plugin_track(p, id)?;
                let x = p
                    .track_mut(tid)?
                    .inserts
                    .iter_mut()
                    .find(|x| x.id == id)
                    .expect("found");
                let msg = if state.is_empty() {
                    format!("{id} の状態を削除しました")
                } else {
                    format!("{id} の状態を置き換えました（{} 文字）", state.len())
                };
                x.state = (!state.is_empty()).then_some(state);
                Ok(Output::msg(msg))
            },
        ),
        edit(
            "plugin.isolation.set",
            "プラグインを実行するプロセスを設定します（inprocess: エンジン内 / shared: 共有の別プロセス / dedicated: 専用の別プロセス）",
            vec![
                ArgDef::ids(IdKind::Plugin, "プラグイン"),
                ArgDef::choice("mode", &PluginIsolation::NAMES, "実行するプロセス").req(),
            ],
            |p, _, a| {
                let ids = a.require_ids::<PluginId>("プラグイン")?;
                let mode = PluginIsolation::from_name(a.str("mode")?).ok_or_else(|| {
                    CmdError::invalid(
                        "--mode には inprocess・shared・dedicated のいずれかを指定してください",
                    )
                })?;
                for id in &ids {
                    let tid = plugin_track(p, *id)?;
                    let x = p
                        .track_mut(tid)?
                        .inserts
                        .iter_mut()
                        .find(|x| x.id == *id)
                        .expect("found");
                    if mode.is_separate() && x.format.eq_ignore_ascii_case("builtin") {
                        return Err(CmdError::conflict(format!(
                            "{id} は内蔵プラグインのため、エンジン内でのみ実行できます"
                        )));
                    }
                    x.isolation = mode;
                }
                Ok(Output::msg(format!(
                    "実行するプロセスを {} に設定しました（{} 件）",
                    mode.name(),
                    ids.len()
                )))
            },
        ),
        edit(
            "folder.add",
            "フォルダーを追加します（入れ子にでき、バスに接続できます。フォルダー自体は音を出しません）",
            where_args(),
            add_folder,
        ),
        edit(
            "folder.rename",
            "フォルダーの名前を変更します",
            vec![
                ArgDef::id("id", IdKind::Folder, "フォルダー").req(),
                ArgDef::text("name", "新しい名前").req(),
            ],
            |p, _, a| {
                let id: FolderId = a.id("id")?;
                let name = name_arg(a)?;
                p.folder_mut(id)?.name = name.clone();
                Ok(Output::msg(format!(
                    "{id} の名前を「{name}」に変更しました"
                )))
            },
        ),
        destroy(
            "folder.delete",
            "フォルダーを削除します。中身は 1 つ上の階層へ移動します（--with-tracks で中のトラックごと削除）",
            vec![
                ArgDef::id("id", IdKind::Folder, "フォルダー").req(),
                ArgDef::flag("with-tracks", "中のトラックとフォルダーも削除する"),
            ],
            delete_folder,
        ),
        edit(
            "folder.bus.set",
            "フォルダーをバスに接続します。出力先が既定のままのトラックは、そのバスへ出力されます（none で解除）",
            vec![
                ArgDef::id("id", IdKind::Folder, "フォルダー").req(),
                ArgDef::text("bus", "バスの ID または none").req(),
            ],
            |p, _, a| {
                let id: FolderId = a.id("id")?;
                let s = a.str("bus")?;
                let bus = if s == "none" || s.is_empty() {
                    None
                } else {
                    let b: TrackId = s.parse().map_err(CmdError::invalid)?;
                    p.bus(b)?;
                    Some(b)
                };
                p.folder_mut(id)?.bus = bus;
                Ok(Output::msg(match bus {
                    Some(b) => format!("{id} を {b} に接続しました"),
                    None => format!("{id} のバスへの接続を解除しました"),
                }))
            },
        ),
        edit(
            "folder.bus.add",
            "新しいバスを作成し、フォルダーをそのバスに接続します（1 回の取り消しで両方とも元に戻ります）",
            vec![
                ArgDef::id("id", IdKind::Folder, "フォルダー").req(),
                ArgDef::text("name", "バスの名前（省略時は自動で付けます）"),
                ArgDef::text(
                    "into",
                    "バスの追加先のフォルダーの ID（root または省略時は最上位）",
                )
                .default(json!("root")),
            ],
            add_folder_bus,
        ),
    ];
    v.shrink_to_fit();
    v
}

// ---- helpers -------------------------------------------------------------------

fn name_arg(a: &Args) -> CmdResult<String> {
    let n = a.str("name")?.trim().to_string();
    if n.is_empty() {
        return Err(CmdError::invalid("名前を入力してください"));
    }
    Ok(n)
}

fn track_ids(p: &Project, a: &Args, allow_master: bool) -> CmdResult<Vec<TrackId>> {
    let ids = a.require_ids::<TrackId>("トラック")?;
    for id in &ids {
        let t = p.track(*id)?;
        if !allow_master && t.kind == TrackKind::Master {
            return Err(CmdError::conflict("マスターには使用できません"));
        }
    }
    Ok(ids)
}

/// Gives every named track the same value (`track.transpose`, `track.velocity`; UI-312).
fn set_track_value(p: &mut Project, a: &Args, set: impl Fn(&mut Track)) -> CmdResult<Vec<TrackId>> {
    let ids = track_ids(p, a, false)?;
    for id in &ids {
        set(p.track_mut(*id)?);
    }
    Ok(ids)
}

fn signed(v: i8) -> String {
    match v {
        0 => "0".to_string(),
        n if n > 0 => format!("+{n}"),
        n => format!("−{}", -n),
    }
}

/// Tracks and folders named in `--ids`, in column order.
pub(super) fn node_ids(p: &Project, a: &Args) -> CmdResult<Vec<Node>> {
    let any = a.ids::<AnyId>("ids")?;
    let mut out = Vec::new();
    for id in any {
        match id {
            AnyId::Track(t) => {
                if t.is_master() {
                    return Err(CmdError::conflict(
                        "マスターはトラックの順序に含まれていません",
                    ));
                }
                p.track(t)?;
                out.push(Node::Track(t));
            }
            AnyId::Folder(f) => {
                p.folder(f)?;
                out.push(Node::Folder(f));
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return Err(CmdError::invalid(
            "トラックかフォルダーを --ids で指定してください（選択中のものもありません）",
        ));
    }
    let order: HashMap<Node, usize> = column_nodes(p)
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n, i))
        .collect();
    out.sort_by_key(|n| order[n]);
    Ok(out)
}

/// Puts the effect `x` into the inserts of `tid` at `index` (the end when `None`), never in
/// front of its instrument.
pub(super) fn place_plugin(
    p: &mut Project,
    tid: TrackId,
    x: Plugin,
    index: Option<usize>,
) -> CmdResult<()> {
    if x.kind == PluginKind::Instrument {
        return Err(CmdError::conflict(
            "インストゥルメントは別のチャンネルへ動かしたり写したりできません",
        ));
    }
    let t = p.track_mut(tid)?;
    let floor = usize::from(
        t.inserts
            .first()
            .is_some_and(|f| f.kind == PluginKind::Instrument),
    );
    let at = index
        .unwrap_or(t.inserts.len())
        .clamp(floor, t.inserts.len());
    t.inserts.insert(at, x);
    Ok(())
}

/// The channel whose inserts hold the plugin.
pub(super) fn plugin_owner(p: &Project, id: PluginId) -> CmdResult<TrackId> {
    plugin_track(p, id)
}

fn set_deactivated(p: &mut Project, a: &Args, off: bool) -> CmdResult<Output> {
    let ids = a.require_ids::<PluginId>("プラグイン")?;
    for id in &ids {
        let tid = plugin_track(p, *id)?;
        p.track_mut(tid)?
            .inserts
            .iter_mut()
            .find(|x| x.id == *id)
            .expect("found")
            .deactivated = off;
    }
    let word = if off {
        "無効にしました"
    } else {
        "有効に戻しました"
    };
    Ok(Output::msg(format!(
        "プラグインを{word}（{} 件）",
        ids.len()
    )))
}

fn plugin_track(p: &Project, id: PluginId) -> CmdResult<TrackId> {
    p.tracks
        .iter()
        .find(|(_, t)| t.inserts.iter().any(|x| x.id == id))
        .map(|(tid, _)| *tid)
        .ok_or_else(|| CmdError::not_found(format!("プラグイン {id} が見つかりません")))
}

pub(super) fn unique_name(p: &Project, base: &str) -> String {
    let taken =
        |n: &str| p.tracks.values().any(|t| t.name == n) || p.folders.values().any(|f| f.name == n);
    (1..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken(n))
        .expect("endless")
}

fn set_flag(
    p: &mut Project,
    a: &Args,
    allow_master: bool,
    what: &str,
    field: fn(&mut Track) -> &mut bool,
) -> CmdResult<Output> {
    let ids = track_ids(p, a, allow_master)?;
    let want = match a.toggle()? {
        Some(v) => v,
        None => !*field(p.track_mut(ids[0])?),
    };
    for id in &ids {
        *field(p.track_mut(*id)?) = want;
    }
    Ok(Output::msg(format!(
        "{} トラックの{what}を{}",
        ids.len(),
        onoff_word(want)
    )))
}

// ---- tracks ----------------------------------------------------------------------

pub(super) fn add_track(p: &mut Project, a: &Args, kind: TrackKind) -> CmdResult<Output> {
    let parent = parent_arg(p, a, "into")?;
    let name = match a.opt_str("name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => unique_name(
            p,
            match kind {
                TrackKind::Audio => "Audio",
                TrackKind::Instrument => "Instrument",
                _ => "Bus",
            },
        ),
    };
    let id = TrackId(p.alloc());
    let mut track = Track::new(id, kind, name.clone());
    track.color = Some(p.next_track_color());
    p.tracks.insert(id, Arc::new(track));
    insert_at(p, parent, a.opt_i64("index"), Node::Track(id))?;
    Ok(created(&format!("トラック「{name}」"), id))
}

fn delete_tracks(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = track_ids(p, a, false)?;
    for id in &ids {
        p.delete_track(*id)?;
    }
    Ok(Output::msg(format!("{} トラックを削除しました", ids.len())))
}

pub(super) fn clone_track(p: &mut Project, t: &Track, settings_only: bool) -> Track {
    let mut copy = t.clone();
    copy.id = TrackId(p.alloc());
    copy.name = format!("{} コピー", t.name);
    for x in &mut copy.inserts {
        x.id = PluginId(p.alloc());
    }
    let mut sends = HashMap::new();
    for s in &mut copy.sends {
        let new = SendId(p.alloc());
        sends.insert(s.id, new);
        s.id = new;
    }
    // the lanes follow the new send and plug-in ids
    for (k, l) in copy.automation.iter_mut().enumerate() {
        l.target = match &t.automation[k].target {
            AutoTarget::Send(s) => AutoTarget::Send(sends.get(s).copied().unwrap_or(*s)),
            AutoTarget::Param(id, name) => {
                let i = t.inserts.iter().position(|x| x.id == *id);
                AutoTarget::Param(i.map_or(*id, |i| copy.inserts[i].id), name.clone())
            }
            other => other.clone(),
        };
    }
    copy.clips = if settings_only {
        Vec::new()
    } else {
        t.clips.iter().map(|c| clone_clip(p, c)).collect()
    };
    if settings_only {
        copy.automation.clear();
        copy.frozen = None;
    }
    let pairs: Vec<_> = t
        .clips
        .iter()
        .zip(&copy.clips)
        .map(|(a, b)| (a.id, b.id))
        .collect();
    p.copy_crosses(&pairs);
    copy
}

fn duplicate_tracks(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = track_ids(p, a, false)?;
    let place = if a.has("into") {
        Some((parent_arg(p, a, "into")?, a.opt_i64("index")))
    } else {
        None
    };
    let mut made = Vec::new();
    for (k, id) in ids.into_iter().enumerate() {
        let copy = clone_track(p, &p.track(id)?.clone(), a.str("mode")? == "settings");
        let new = copy.id;
        p.tracks.insert(new, Arc::new(copy));
        match place {
            Some((parent, index)) => {
                insert_at(p, parent, index.map(|i| i + k as i64), Node::Track(new))?
            }
            None => {
                let (parent, i) = p.find_node(Node::Track(id)).expect("in the column");
                p.children_mut(parent).insert(i + 1, Node::Track(new));
            }
        }
        made.push(new.to_string());
    }
    Ok(Output::with(
        format!("{} トラックを複製しました", made.len()),
        json!({ "ids": made }),
    ))
}

fn set_color(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let color = color_arg(a)?;
    let nodes = node_ids(p, a)?;
    for n in &nodes {
        match n {
            Node::Track(t) => p.track_mut(*t)?.color = color.clone(),
            Node::Folder(f) => p.folder_mut(*f)?.color = color.clone(),
        }
    }
    Ok(Output::msg(format!("{} 件の色を設定しました", nodes.len())))
}

fn set_icon(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let icon = match a.str("icon")? {
        "none" => None,
        name => Some(
            TrackIcon::ALL
                .into_iter()
                .find(|i| i.name() == name)
                .ok_or_else(|| {
                    CmdError::invalid(
                        "--icon には drums・keys・mic・wave・none のいずれかを指定してください",
                    )
                })?,
        ),
    };
    let ids = track_ids(p, a, false)?;
    for id in &ids {
        p.track_mut(*id)?.icon = icon;
    }
    Ok(Output::msg(format!(
        "{} トラックのアイコンを設定しました",
        ids.len()
    )))
}

fn set_output(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let to = a.str("to")?;
    if let Some(port) = to.strip_prefix("midi:") {
        return set_midi_out(p, a, port);
    }
    let route = match to {
        "default" => None,
        "master" => Some(Route::Master),
        other => {
            let b: TrackId = other.parse().map_err(CmdError::invalid)?;
            if b.is_master() {
                Some(Route::Master)
            } else {
                p.bus(b)?;
                Some(Route::Bus(b))
            }
        }
    };
    let ids = track_ids(p, a, false)?;
    for id in &ids {
        let t = p.track_mut(*id)?;
        t.output = route;
        // An audio output takes the track back from a MIDI port.
        t.midi_out = None;
    }
    Ok(Output::msg(format!(
        "{} トラックの出力先を {to} に設定しました",
        ids.len()
    )))
}

/// `track.output.set --to midi:<port>`: the instrument tracks send their notes to a MIDI port.
fn set_midi_out(p: &mut Project, a: &Args, port: &str) -> CmdResult<Output> {
    let port = port.trim();
    if port.is_empty() {
        return Err(CmdError::invalid("midi: の後にポート名を指定してください"));
    }
    let channel = match a.opt_i64("channel") {
        None => None,
        Some(c) if (1..=16).contains(&c) => Some(c as u8),
        Some(_) => {
            return Err(CmdError::invalid("--channel には 1〜16 を指定してください"));
        }
    };
    let ids = track_ids(p, a, false)?;
    for id in &ids {
        if p.track(*id)?.kind != TrackKind::Instrument {
            return Err(CmdError::invalid(
                "MIDI の出力先を設定できるのはインストゥルメントトラックだけです",
            ));
        }
    }
    for id in &ids {
        let t = p.track_mut(*id)?;
        // The channel stays when only the port changes.
        let channel = channel.or_else(|| t.midi_out.as_ref().map(|m| m.channel));
        t.midi_out = Some(MidiOut {
            port: port.to_string(),
            channel: channel.unwrap_or(1),
        });
    }
    Ok(Output::msg(format!(
        "{} トラックの出力先を MIDI の {port} に設定しました",
        ids.len()
    )))
}

fn move_nodes(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let nodes = node_ids(p, a)?;
    let (into, by, out) = (a.opt_str("into"), a.opt_i64("by"), a.flag("out"));
    if usize::from(into.is_some()) + usize::from(by.is_some()) + usize::from(out) != 1 {
        return Err(CmdError::invalid(
            "--into・--out・--by のいずれか 1 つだけを指定してください",
        ));
    }

    if let Some(by) = by {
        let (parent, _) = p.find_node(nodes[0]).expect("in the column");
        if nodes
            .iter()
            .any(|n| p.find_node(*n).map(|x| x.0) != Some(parent))
        {
            return Err(CmdError::invalid(
                "--by では同じ階層のものだけを移動できます",
            ));
        }
        let mut order = nodes.clone(); // column order: top first
        if by > 0 {
            order.reverse();
        }
        for (k, n) in order.iter().enumerate() {
            let kids = p.children_mut(parent);
            let i = kids.iter().position(|x| x == n).expect("in the list");
            let j = if by < 0 {
                (i as i64 + by).max(k as i64)
            } else {
                (i as i64 + by).min(kids.len() as i64 - 1 - k as i64)
            }
            .clamp(0, kids.len() as i64 - 1) as usize;
            let x = kids.remove(i);
            kids.insert(j, x);
        }
        return Ok(Output::msg(format!(
            "{} 件を {by} 移動しました",
            nodes.len()
        )));
    }

    if out {
        let mut moved: HashMap<FolderId, usize> = HashMap::new();
        for n in &nodes {
            let (parent, _) = p.find_node(*n).expect("in the column");
            let Parent::Folder(f) = parent else {
                return Err(CmdError::conflict(format!("{n} はすでに最上位にあります")));
            };
            p.detach(*n);
            let (gp, gi) = p
                .find_node(Node::Folder(f))
                .expect("folder is in the column");
            let k = moved.entry(f).or_insert(0);
            p.children_mut(gp).insert(gi + 1 + *k, *n);
            *k += 1;
        }
        return Ok(Output::msg(format!(
            "{} 件を 1 つ上の階層へ移動しました",
            nodes.len()
        )));
    }

    let target = parent_arg(p, a, "into")?;
    if let Parent::Folder(t) = target {
        for n in &nodes {
            if let Node::Folder(f) = n
                && p.folder_contains(*f, t)
            {
                return Err(CmdError::conflict(format!(
                    "フォルダー {f} は自身の中に移動できません"
                )));
            }
        }
    }
    for n in &nodes {
        p.detach(*n);
    }
    let len = p.children(target).len();
    let base = match a.opt_i64("index") {
        None => len,
        Some(i) if i < 0 => return Err(CmdError::range("--index には 0 以上を指定してください")),
        Some(i) => (i as usize).min(len),
    };
    for (k, n) in nodes.iter().enumerate() {
        let at = (base + k).min(p.children(target).len());
        p.children_mut(target).insert(at, *n);
    }
    Ok(Output::msg(format!("{} 件を移動しました", nodes.len())))
}

// ---- sends -----------------------------------------------------------------------

fn add_send(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let to: TrackId = a.id("to")?;
    p.bus(to)?;
    let level = a.opt_f64("level").unwrap_or(0.0);
    let pre = a.flag("pre");
    let ids = track_ids(p, a, false)?;
    let mut made = Vec::new();
    for id in &ids {
        let sid = SendId(p.alloc());
        p.track_mut(*id)?.sends.push(AuxSend {
            id: sid,
            to,
            level_db: level,
            pre,
            muted_level: None,
        });
        made.push(sid.to_string());
    }
    Ok(Output::with(
        format!("センド {} 件を追加しました", made.len()),
        json!({ "ids": made }),
    ))
}

/// `track.add.bus` and `send.add` as one step: the bus goes where `--into`
/// says, at the end, and every source sends to it.
fn add_send_to_new_bus(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = track_ids(p, a, false)?;
    let bus = add_track(p, a, TrackKind::Bus)?;
    let to: TrackId = bus.data["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| CmdError::invalid("バスを作成できませんでした"))?;
    let mut sends = a.clone();
    let ids_json: Vec<String> = ids.iter().map(ToString::to_string).collect();
    sends.0.insert("ids".into(), json!(ids_json));
    sends.0.insert("to".into(), json!(to.to_string()));
    let made = add_send(p, cx, &sends)?;
    let name = &p.track(to)?.name;
    Ok(Output::with(
        format!(
            "バス「{name}」 {to} を作成し、{} トラックからのセンドを追加しました",
            ids.len()
        ),
        json!({ "id": to.to_string(), "sends": made.data["ids"] }),
    ))
}

fn add_folder_bus(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let f: FolderId = a.id("id")?;
    p.folder_mut(f)?;
    let bus = add_track(p, a, TrackKind::Bus)?;
    let to: TrackId = bus.data["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| CmdError::invalid("バスを作成できませんでした"))?;
    p.folder_mut(f)?.bus = Some(to);
    let name = &p.track(to)?.name;
    Ok(Output::with(
        format!("バス「{name}」 {to} を作成し、{f} を接続しました"),
        json!({ "id": to.to_string(), "folder": f.to_string() }),
    ))
}

fn set_send(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    change_send(p, a.id("id")?, a.opt_f64("level"), a.opt_str("position"))
}

/// Sets the level or the position of a send (`send.set`, `send.level.set`, `send.level.reset`).
fn change_send(
    p: &mut Project,
    id: SendId,
    level: Option<f64>,
    position: Option<&str>,
) -> CmdResult<Output> {
    let tid = p
        .tracks
        .iter()
        .find(|(_, t)| t.sends.iter().any(|s| s.id == id))
        .map(|(t, _)| *t)
        .ok_or_else(|| CmdError::not_found(format!("センド {id} が見つかりません")))?;
    if level.is_none() && position.is_none() {
        return Err(CmdError::invalid(
            "--level か --position のどちらかが必要です",
        ));
    }
    let s = p
        .track_mut(tid)?
        .sends
        .iter_mut()
        .find(|s| s.id == id)
        .expect("found");
    if let Some(l) = level {
        if s.muted_level.is_some() {
            s.muted_level = Some(l);
        } else {
            s.level_db = l;
        }
    }
    if let Some(pos) = position {
        s.pre = pos == "pre";
    }
    Ok(Output::msg(format!("{id} を変更しました")))
}

// ---- plugins ---------------------------------------------------------------------

/// A parameter's spec as the list commands show it.
fn spec_json(s: &xirius_params::ParamSpec) -> Value {
    let (min, max) = s.range();
    let mut row = json!({
        "id": s.id,
        "label": s.label,
        "unit": s.unit.symbol(),
        "min": min,
        "max": max,
        "default": s.default,
        "default_value": s.default_norm(),
    });
    if !s.choices().is_empty() {
        row["choices"] = json!(s.choices());
    }
    if let Some(n) = s.steps() {
        row["steps"] = json!(n);
    }
    row
}

fn set_param(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: PluginId = a.id("id")?;
    let tid = plugin_track(p, id)?;
    let name = a.str("param")?.to_string();
    let x = p
        .track_mut(tid)?
        .inserts
        .iter_mut()
        .find(|x| x.id == id)
        .expect("found");
    let spec = x.builtin().and_then(|d| d.param(&name)).map(|(_, s)| s);
    let mut plain_value: Option<f64> = None;
    let value = match (a.opt_f64("value"), a.opt_str("plain")) {
        (Some(v), None) => v,
        (None, Some(text)) => {
            let Some(s) = spec else {
                return Err(CmdError::invalid(format!(
                    "--plain は内蔵プラグインのパラメーターにだけ使えます（{} に {name} はありません）",
                    x.name
                )));
            };
            let Some(plain) = s.parse(text) else {
                let (lo, hi) = s.range();
                let choices = s.choices().join("・");
                return Err(CmdError::range(if choices.is_empty() {
                    format!(
                        "{} は {}〜{} {} で指定してください",
                        s.label,
                        lo,
                        hi,
                        s.unit.symbol()
                    )
                } else {
                    format!("{} は {choices} のいずれかで指定してください", s.label)
                }));
            };
            plain_value = Some(plain);
            s.to_norm(plain)
        }
        (Some(_), Some(_)) => {
            return Err(CmdError::invalid(
                "--value と --plain はどちらか一方だけを指定してください",
            ));
        }
        (None, None) => {
            return Err(CmdError::invalid("--value か --plain を指定してください"));
        }
    };
    match spec {
        Some(s) => {
            x.params.insert(s.id.to_string(), value);
            // `--plain 0.25` on a 0〜100 % parameter is the value in the unit (0.25 %) and
            // stays so, but a person who meant the 0〜1 fraction needs to hear which flag
            // says that.
            let fraction = plain_value
                .filter(|v| s.unit == xirius_params::Unit::Percent && *v > 0.0 && *v <= 1.0)
                .map(|v| format!("{v} {}", s.unit.symbol()));
            let text = fraction
                .clone()
                .unwrap_or_else(|| s.text(s.to_plain(value)));
            let hint = fraction.map(|_| "0〜1 の割合なら --value を使います");
            let message = match hint {
                Some(hint) => format!(
                    "{id} の {}（{}）を {text} に設定しました。{hint}",
                    s.label, s.id
                ),
                None => format!("{id} の {}（{}）を {text} に設定しました", s.label, s.id),
            };
            Ok(match hint {
                Some(hint) => Output::with(message, json!({ "warnings": [hint] })),
                None => Output::msg(message),
            })
        }
        None => {
            x.params.insert(name.clone(), value);
            Ok(Output::msg(format!(
                "{id} の {name} を {value} に設定しました"
            )))
        }
    }
}

fn add_plugin(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let kind = if a.str("kind")? == "instrument" {
        PluginKind::Instrument
    } else {
        PluginKind::Effect
    };
    let name = name_arg(a)?;
    let id = place_new_plugin(
        p,
        tid,
        kind,
        &name,
        a.str("format")?,
        a.opt_str("uid"),
        a.opt_i64("index"),
    )?;
    Ok(created(&format!("プラグイン「{name}」"), id))
}

/// A new plug-in in a track's inserts: an effect at `index` (the end without one), the
/// instrument at the front.
pub(super) fn place_new_plugin(
    p: &mut Project,
    tid: TrackId,
    kind: PluginKind,
    name: &str,
    format: &str,
    uid: Option<&str>,
    index: Option<i64>,
) -> CmdResult<PluginId> {
    let track_kind = p.track(tid)?.kind;
    if kind == PluginKind::Instrument && track_kind != TrackKind::Instrument {
        return Err(CmdError::conflict(
            "インストゥルメントはインストゥルメントトラックにのみ挿入できます",
        ));
    }
    if kind == PluginKind::Instrument
        && p.track(tid)?
            .inserts
            .iter()
            .any(|x| x.kind == PluginKind::Instrument)
    {
        return Err(CmdError::conflict(
            "インストゥルメントは1つだけ挿入できます",
        ));
    }
    let id = PluginId(p.alloc());
    let mut x = Plugin {
        id,
        name: name.to_string(),
        kind,
        format: format.to_string(),
        uid: uid.map_or_else(|| name.to_string(), str::to_string),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        isolation: Default::default(),
    };
    // A built-in starts with every parameter at its default, so each can be listed and
    // automated before it is first moved (PLG-045).
    if let Some(def) = x.builtin() {
        x.params = def.defaults().map(|(k, v)| (k.to_string(), v)).collect();
    }
    let t = p.track_mut(tid)?;
    let at = match (kind, index) {
        (PluginKind::Instrument, _) => 0,
        (_, None) => t.inserts.len(),
        (_, Some(i)) if i < 0 => {
            return Err(CmdError::range("--index には 0 以上を指定してください"));
        }
        (_, Some(i)) => (i as usize).min(t.inserts.len()),
    };
    // an effect may not sit in front of the instrument
    let at = if kind == PluginKind::Effect
        && t.inserts
            .first()
            .is_some_and(|f| f.kind == PluginKind::Instrument)
    {
        at.max(1)
    } else {
        at
    };
    t.inserts.insert(at, x);
    Ok(id)
}

// ---- folders -----------------------------------------------------------------------

fn add_folder(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let parent = parent_arg(p, a, "into")?;
    let name = match a.opt_str("name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => unique_name(p, "Folder"),
    };
    let id = FolderId(p.alloc());
    p.folders.insert(
        id,
        Folder {
            id,
            name: name.clone(),
            color: None,
            bus: None,
            children: Vec::new(),
        },
    );
    insert_at(p, parent, a.opt_i64("index"), Node::Folder(id))?;
    Ok(created(&format!("フォルダー「{name}」"), id))
}

pub(super) fn delete_folder(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: FolderId = a.id("id")?;
    p.folder(id)?;
    if a.flag("with-tracks") {
        let tracks = p.tracks_in(id);
        let mut folders = Vec::new();
        fn collect(p: &Project, f: FolderId, out: &mut Vec<FolderId>) {
            for n in &p.folders[&f].children {
                if let Node::Folder(c) = n {
                    collect(p, *c, out);
                }
            }
            out.push(f);
        }
        collect(p, id, &mut folders);
        p.detach(Node::Folder(id));
        for t in &tracks {
            // out of the column first, so `delete_track` finds nothing to detach twice
            p.delete_track(*t)?;
        }
        for f in &folders {
            p.folders.remove(f);
        }
        return Ok(Output::msg(format!(
            "フォルダーと {} トラックを削除しました",
            tracks.len()
        )));
    }
    let (parent, i) = p.find_node(Node::Folder(id)).expect("in the column");
    let kids = std::mem::take(&mut p.folder_mut(id)?.children);
    let n = kids.len();
    p.children_mut(parent).splice(i..=i, kids);
    p.folders.remove(&id);
    Ok(Output::msg(format!(
        "フォルダーを削除しました（中の {n} 件は 1 つ上の階層へ移動しました）"
    )))
}
