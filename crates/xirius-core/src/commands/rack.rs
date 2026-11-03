//! `rack.show`: the track's insert wiring as a tree (PRJ-026). The `rack.split.*`,
//! `rack.branch.*`, `rack.node.move` and `rack.flatten` edits shape it (ENG-074): a track
//! without a rack gets its serial one first, and the edited rack is fixed the way a song is
//! read, so a splitter left with one branch folds back into a line. `rack.layer.*` turn an
//! instrument track's source into a multi-instrument and edit its layers (ENG-075).
use serde_json::json;

use super::*;
use crate::auto::AutoTarget;
use crate::id::{AnyId, IdKind, NodeId, PluginId, SendId, TrackId};
use crate::model::{MAX_DB, MIN_DB, PluginKind, Project, Track, TrackKind};
use crate::rack::{
    Branch, Layer, MAX_BRANCHES, MAX_SPLIT_DEPTH, Rack, RackNode, Source, Split, SplitMode,
};

const MODES: &[&str] = &["normal", "channel", "frequency"];
/// The most layers a multi-instrument holds (the engine's `multi::MAX_LAYERS`).
const MAX_LAYERS: usize = 64;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        read(
            "rack.show",
            "トラックのラック（音源と FX の木）を JSON で返します。ラックの無いトラックは直列の形を implicit: true で返します",
            vec![ArgDef::id("track", IdKind::Track, "トラック").req()],
            |p, _, _, a| {
                let tid: TrackId = a.id("track")?;
                let t = p.track(tid)?;
                let v = t.rack_json();
                let has = t.rack.is_some();
                Ok(Output::with(
                    if has {
                        format!("{} のラックを表示しました", t.name)
                    } else {
                        format!("{} のラック（直列）を表示しました", t.name)
                    },
                    v,
                ))
            },
        ),
        edit(
            "rack.split.add",
            "トラックの FX にスプリッターを追加します（--at は track（FX の末尾）・after:<プラグインかスプリッター>・branch:<分岐>:<位置>・layer:<レイヤー>:<位置>。分岐は空で作ります）",
            vec![
                ArgDef::id("track", IdKind::Track, "トラック").req(),
                ArgDef::text(
                    "at",
                    "置く所: track・after:<id>・branch:<分岐の id>:<位置>・layer:<レイヤーの id>:<位置>（省略時は track）",
                ),
                ArgDef::choice("mode", MODES, "分け方（省略時は normal）"),
                ArgDef::int("branches", "分岐の数（2〜5。channel は 2）")
                    .range(2.0, MAX_BRANCHES as f64),
            ],
            |p, _, a| split_add(p, a),
        ),
        edit(
            "rack.split.mode",
            "スプリッターの分け方を変えます（frequency の --crossovers は Hz をカンマで区切り、分岐より 1 つ少なく）",
            vec![
                ArgDef::id("id", IdKind::RackNode, "スプリッター").req(),
                ArgDef::choice("mode", MODES, "分け方").req(),
                ArgDef::text(
                    "crossovers",
                    "frequency の境目（Hz、カンマ区切り。省略時は既定）",
                ),
            ],
            |p, _, a| split_mode(p, a),
        ),
        edit(
            "rack.branch.add",
            "スプリッターに空の分岐を足します（5 本まで。channel は 2 本のまま）",
            vec![ArgDef::id("split", IdKind::RackNode, "スプリッター").req()],
            |p, _, a| branch_add(p, a.id("split")?),
        ),
        edit(
            "rack.split.remove",
            "スプリッターを外し、分岐の中身を上の分岐から順にその場所へ並べます（分岐の音量・パン・ミュートは無くなります）",
            vec![ArgDef::id("id", IdKind::RackNode, "スプリッター").req()],
            |p, _, a| split_remove(p, a.id("id")?),
        ),
        destroy(
            "rack.branch.remove",
            "分岐を中のプラグインごと削除します（1 本だけ残ったスプリッターは直列に戻ります）",
            vec![ArgDef::id("id", IdKind::RackNode, "分岐").req()],
            |p, _, a| branch_remove(p, a.id("id")?),
        ),
        edit(
            "rack.branch.set",
            "分岐の音量・パン・ミュートを設定します",
            vec![
                ArgDef::id("id", IdKind::RackNode, "分岐").req(),
                ArgDef::float("gain", "合流の前の音量（dB）").range(MIN_DB, MAX_DB),
                ArgDef::float("pan", "パン（-1 左〜1 右）").range(-1.0, 1.0),
                ArgDef::choice("mute", &["on", "off"], "ミュート"),
            ],
            |p, _, a| branch_set(p, a),
        ),
        edit(
            "rack.node.move",
            "エフェクトかスプリッターを、同じトラックの FX・分岐・レイヤーの FX の中へ移します（--index は 0 から。省略時は末尾）",
            vec![
                ArgDef::id(
                    "id",
                    IdKind::RackNodeOrPlugin,
                    "移すエフェクトかスプリッター",
                )
                .req(),
                ArgDef::id(
                    "to",
                    IdKind::TrackOrRackNode,
                    "移す先: 分岐・レイヤーか、そのトラック",
                )
                .req(),
                ArgDef::int("index", "移す先での位置（0 から）").range(0.0, 1e6),
            ],
            |p, _, a| node_move(p, a),
        ),
        edit(
            "rack.flatten",
            "トラックのスプリッターを外して直列に戻します（分岐の中身は上から順に並びます）",
            vec![ArgDef::id("track", IdKind::Track, "トラック").req()],
            |p, _, a| {
                let tid: TrackId = a.id("track")?;
                let t = p.track_mut(tid)?;
                if t.rack.is_none() {
                    return Ok(Output::msg(format!("{} は直列です", t.name)));
                }
                t.flatten_rack();
                Ok(Output::msg(format!("{} を直列に戻しました", t.name)))
            },
        ),
        edit(
            "rack.layer.add",
            "インストゥルメントトラックにレイヤー（インストゥルメント）を足してマルチインストゥルメントにします（いまの音源はレイヤー 1 になります。--keys は C1-B2 か 36-59、--velocity は 1-127）",
            vec![
                ArgDef::id("track", IdKind::Track, "インストゥルメントトラック").req(),
                ArgDef::text("name", "インストゥルメントの名前").req(),
                ArgDef::text("format", "vst3・clap・builtin など").default(json!("builtin")),
                ArgDef::text("uid", "形式ごとの固有 ID（省略時は名前）"),
                ArgDef::text("keys", "鍵盤の範囲（C1-B2 か 36-59。省略時は全音域）"),
                ArgDef::text("velocity", "ベロシティの範囲（1-127。省略時は全域）"),
                ArgDef::int("transpose", "移調（半音）").range(-48.0, 48.0),
                ArgDef::text(
                    "label",
                    "レイヤーの名前（省略時はインストゥルメントの名前）",
                ),
            ],
            |p, _, a| layer_add(p, a),
        ),
        edit(
            "rack.layer.set",
            "レイヤーの鍵盤とベロシティの範囲・移調・音量・パン・ミュート・名前を設定します",
            vec![
                ArgDef::id("id", IdKind::RackNode, "レイヤー").req(),
                ArgDef::text("keys", "鍵盤の範囲（C1-B2 か 36-59）"),
                ArgDef::text("velocity", "ベロシティの範囲（1-127）"),
                ArgDef::int("transpose", "移調（半音）").range(-48.0, 48.0),
                ArgDef::float("gain", "音量（dB）").range(MIN_DB, MAX_DB),
                ArgDef::float("pan", "パン（-1 左〜1 右）").range(-1.0, 1.0),
                ArgDef::choice("mute", &["on", "off"], "ミュート"),
                ArgDef::text("label", "レイヤーの名前"),
            ],
            |p, _, a| layer_set(p, a),
        ),
        destroy(
            "rack.layer.remove",
            "レイヤーをインストゥルメントと FX ごと削除します（最後の 1 つは外せません）",
            vec![ArgDef::id("id", IdKind::RackNode, "レイヤー").req()],
            |p, _, a| layer_remove(p, a.id("id")?),
        ),
    ]
}

fn split_add(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let mode = mode_of(a.opt_str("mode").unwrap_or("normal"))?;
    let n = a.opt_i64("branches").unwrap_or(2) as usize;
    if mode == SplitMode::Channel && n != 2 {
        return Err(CmdError::range("channel の分岐は 2 本です"));
    }
    let id = NodeId(p.alloc());
    let branches: Vec<Branch> = (0..n).map(|_| empty_branch(NodeId(p.alloc()))).collect();
    let ids: Vec<String> = branches.iter().map(|b| b.id.to_string()).collect();
    let split = RackNode::Split(Split {
        id,
        mode,
        crossovers: Vec::new(),
        branches,
    });
    let at = a.opt_str("at").unwrap_or("track").trim().to_string();
    let t = p.track_mut(tid)?;
    let name = t.name.clone();
    let r = rack_of(t);
    let (chain, index, depth) = place(r, &at)?;
    if depth >= MAX_SPLIT_DEPTH {
        return Err(CmdError::conflict(format!(
            "スプリッターの入れ子は {MAX_SPLIT_DEPTH} 段までです"
        )));
    }
    insert_at(r, chain, index, split);
    p.fix_track_rack(tid);
    Ok(Output::with(
        format!("{name} にスプリッターを追加しました"),
        json!({"id": id.to_string(), "track": tid.to_string(), "branches": ids}),
    ))
}

fn split_mode(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let id: NodeId = a.id("id")?;
    let mode = mode_of(a.str("mode")?)?;
    let crossovers = match a.opt_str("crossovers").map(str::trim) {
        None | Some("") => None,
        Some(s) => Some(
            s.split(',')
                .map(|x| x.trim().parse::<f64>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| CmdError::invalid(format!("--crossovers `{s}` を読み取れません")))?,
        ),
    };
    let tid = node_track(p, id)?;
    let r = rack_of(p.track_mut(tid)?);
    let s = find_split(r, id).ok_or_else(|| not_split(id))?;
    if mode == SplitMode::Channel && s.branches.len() != 2 {
        return Err(CmdError::conflict(
            "channel にできるのは分岐が 2 本のスプリッターだけです",
        ));
    }
    s.crossovers = match crossovers {
        Some(_) if mode != SplitMode::Frequency => {
            return Err(CmdError::invalid(
                "--crossovers は frequency のときだけ使えます",
            ));
        }
        Some(c) => {
            let ok = c.len() == s.branches.len() - 1
                && c.iter().all(|f| f.is_finite() && *f > 0.0)
                && c.windows(2).all(|w| w[0] < w[1]);
            if !ok {
                return Err(CmdError::range(
                    "クロスオーバーは分岐より 1 つ少なく、昇順にしてください",
                ));
            }
            c
        }
        None if mode == SplitMode::Frequency && s.mode == SplitMode::Frequency => {
            std::mem::take(&mut s.crossovers)
        }
        None => Vec::new(),
    };
    s.mode = mode;
    p.fix_track_rack(tid);
    Ok(Output::msg(format!(
        "スプリッターの分け方を {} にしました",
        MODES[mode as usize]
    )))
}

fn branch_add(p: &mut Project, id: NodeId) -> CmdResult<Output> {
    let tid = node_track(p, id)?;
    let new = NodeId(p.alloc());
    let r = rack_of(p.track_mut(tid)?);
    let s = find_split(r, id).ok_or_else(|| not_split(id))?;
    if s.mode == SplitMode::Channel {
        return Err(CmdError::conflict("channel の分岐は 2 本のままです"));
    }
    if s.branches.len() >= MAX_BRANCHES {
        return Err(CmdError::conflict(format!(
            "分岐は {MAX_BRANCHES} 本までです"
        )));
    }
    s.branches.push(empty_branch(new));
    p.fix_track_rack(tid);
    Ok(Output::with(
        "分岐を追加しました",
        json!({"id": new.to_string(), "split": id.to_string()}),
    ))
}

fn split_remove(p: &mut Project, id: NodeId) -> CmdResult<Output> {
    fn walk(nodes: &mut Vec<RackNode>, id: NodeId) -> bool {
        if let Some(i) = nodes
            .iter()
            .position(|n| matches!(n, RackNode::Split(s) if s.id == id))
        {
            let RackNode::Split(s) = nodes.remove(i) else {
                unreachable!("matched a split")
            };
            let fx: Vec<RackNode> = s.branches.into_iter().flat_map(|b| b.fx).collect();
            nodes.splice(i..i, fx);
            return true;
        }
        nodes.iter_mut().any(|n| match n {
            RackNode::Split(s) => s.branches.iter_mut().any(|b| walk(&mut b.fx, id)),
            RackNode::Plugin(_) => false,
        })
    }
    let tid = node_track(p, id)?;
    let t = p.track_mut(tid)?;
    let r = rack_of(t);
    if !roots(r).into_iter().any(|c| walk(c, id)) {
        return Err(not_split(id));
    }
    let name = t.name.clone();
    p.fix_track_rack(tid);
    Ok(Output::msg(format!(
        "{name} のスプリッター {id} を外しました"
    )))
}

fn branch_remove(p: &mut Project, id: NodeId) -> CmdResult<Output> {
    let tid = node_track(p, id)?;
    let t = p.track_mut(tid)?;
    let r = rack_of(t);
    let (s, i) = find_branch_parent(r, id).ok_or_else(|| not_branch(id))?;
    // The one branch left folds into a line, which has no level, pan or mute of its own.
    if let [a, b] = &s.branches[..] {
        let left = if i == 0 { b } else { a };
        if left.gain_db != 0.0 || left.pan != 0.0 || left.mute {
            return Err(CmdError::conflict(
                "残る分岐の音量・パン・ミュートが失われます。先に rack.branch.set で 0 dB・中央・ミュートなしに戻してください",
            ));
        }
    }
    let gone = s.branches.remove(i);
    let mut plugins = Vec::new();
    plugins_in(&gone.fx, &mut plugins);
    // The sidechain sends the removed plug-ins received go with them, as with `plugin.remove`.
    let gone_keys: Vec<SendId> = t
        .inserts
        .iter()
        .filter(|x| plugins.contains(&x.id))
        .filter_map(|x| x.sidechain.as_ref())
        .flat_map(|sc| sc.sources.iter().map(|s| s.id))
        .collect();
    t.inserts.retain(|x| !plugins.contains(&x.id));
    p.fix_track_rack(tid);
    for t in p.tracks.keys().copied().collect::<Vec<_>>() {
        p.track_mut(t)?
            .automation
            .retain(|l| !matches!(&l.target, AutoTarget::Send(id) if gone_keys.contains(id)));
    }
    Ok(Output::msg(if plugins.is_empty() {
        "分岐を削除しました".to_string()
    } else {
        format!("分岐をプラグイン {} 件とともに削除しました", plugins.len())
    }))
}

fn branch_set(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let id: NodeId = a.id("id")?;
    let gain = a.opt_f64("gain");
    let pan = a.opt_f64("pan");
    let mute = a.opt_str("mute").map(|m| m == "on");
    if gain.is_none() && pan.is_none() && mute.is_none() {
        return Err(CmdError::invalid(
            "--gain・--pan・--mute のいずれかが必要です",
        ));
    }
    let tid = node_track(p, id)?;
    let r = rack_of(p.track_mut(tid)?);
    let (s, i) = find_branch_parent(r, id).ok_or_else(|| not_branch(id))?;
    let b = &mut s.branches[i];
    if let Some(g) = gain {
        b.gain_db = g;
    }
    if let Some(x) = pan {
        b.pan = x;
    }
    if let Some(m) = mute {
        b.mute = m;
    }
    Ok(Output::msg("分岐の量を設定しました"))
}

fn node_move(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let what: AnyId = a.id("id")?;
    let to: AnyId = a.id("to")?;
    let tid = match what {
        AnyId::Plugin(x) => {
            let tid = super::track::plugin_owner(p, x)?;
            let t = p.track(tid)?;
            let plugin = t.inserts.iter().find(|i| i.id == x).expect("owner");
            if plugin.kind == PluginKind::Instrument {
                return Err(CmdError::conflict(format!(
                    "インストゥルメント {} は FX の中へ移せません",
                    plugin.name
                )));
            }
            tid
        }
        AnyId::RackNode(x) => {
            let tid = node_track(p, x)?;
            if find_split(rack_of(p.track_mut(tid)?), x).is_none() {
                return Err(not_split(x));
            }
            tid
        }
        _ => return Err(CmdError::invalid("--id はエフェクトかスプリッターです")),
    };
    let index = a.opt_i64("index").map(|i| i as usize);
    let t = p.track_mut(tid)?;
    let name = t.name.clone();
    let r = rack_of(t);
    let node = match what {
        AnyId::Plugin(x) => take_node(r, &|n| matches!(n, RackNode::Plugin(y) if *y == x))
            .unwrap_or(RackNode::Plugin(x)),
        AnyId::RackNode(x) => {
            take_node(r, &|n| matches!(n, RackNode::Split(s) if s.id == x)).expect("found above")
        }
        _ => unreachable!(),
    };
    let (chain, depth) = match to {
        AnyId::Track(x) if x == tid => (Chain::Top, 0),
        AnyId::Track(_) => {
            return Err(CmdError::conflict(format!(
                "{name} の外へは移せません（plugin.move を使ってください）"
            )));
        }
        AnyId::RackNode(l) if layer_fx(r, l).is_some() => (Chain::Layer(l), 0),
        AnyId::RackNode(b) => match branch_depth(r, b) {
            Some(d) => (Chain::Branch(b), d),
            None if matches!(&node, RackNode::Split(s) if node_ids_of(s).contains(&b)) => {
                return Err(CmdError::conflict(
                    "スプリッターを自分の分岐の中へは移せません",
                ));
            }
            None => {
                return Err(CmdError::not_found(format!(
                    "{name} に分岐 {b} がありません"
                )));
            }
        },
        _ => return Err(CmdError::invalid("--to は分岐・レイヤーかトラックです")),
    };
    if let RackNode::Split(s) = &node
        && depth + height(s) > MAX_SPLIT_DEPTH
    {
        return Err(CmdError::conflict(format!(
            "スプリッターの入れ子は {MAX_SPLIT_DEPTH} 段までです"
        )));
    }
    let len = chain_mut(r, chain).map(|c| c.len()).unwrap_or(0);
    let at = index.unwrap_or(len).min(len);
    insert_at(r, chain, at, node);
    p.fix_track_rack(tid);
    Ok(Output::with(
        "ラックの中で移しました",
        json!({"id": what.to_string(), "to": to.to_string(), "index": at}),
    ))
}

// ---- the rack tree ---------------------------------------------------------------

/// A chain of the rack, by what holds it: the track's fx, a branch, or a layer's fx.
#[derive(Clone, Copy)]
pub(super) enum Chain {
    Top,
    Branch(NodeId),
    Layer(NodeId),
}

// ---- layers (ENG-075) ---------------------------------------------------------------

fn layer_add(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let keys = a.opt_str("keys").map(keys_of).transpose()?;
    let velocity = a.opt_str("velocity").map(velocity_of).transpose()?;
    let transpose = a.opt_i64("transpose").map(|t| t as i8);
    let label = a.opt_str("label").map(str::trim).filter(|l| !l.is_empty());
    let t = p.track(tid)?;
    if t.kind != TrackKind::Instrument {
        return Err(CmdError::conflict(
            "レイヤーはインストゥルメントトラックにだけ足せます",
        ));
    }
    let name = t.name.clone();
    if let Some(Source::Multi { layers, .. }) = t.rack.as_ref().and_then(|r| r.source.as_ref())
        && layers.len() >= MAX_LAYERS
    {
        return Err(CmdError::conflict(format!(
            "レイヤーは {MAX_LAYERS} 個までです"
        )));
    }
    // The instrument the track plays becomes the first layer.
    make_multi(p, tid)?;
    let plugin = super::track::place_new_plugin(
        p,
        tid,
        PluginKind::Instrument,
        a.str("name")?.trim(),
        a.str("format")?,
        a.opt_str("uid"),
        None,
    )?;
    // A track that had no instrument gets it as its source: that is the first layer.
    make_multi(p, tid)?;
    let layer = layer_mut(p, tid, |l| l.instrument == plugin).expect("the fix made a layer");
    if let Some(k) = keys {
        layer.keys = k;
    }
    if let Some(v) = velocity {
        layer.velocity = v;
    }
    if let Some(x) = transpose {
        layer.transpose = x;
    }
    if let Some(l) = label {
        layer.name = l.to_string();
    }
    let id = layer.id;
    Ok(Output::with(
        format!("{name} にレイヤー「{}」を追加しました", layer.name),
        json!({"id": id.to_string(), "plugin": plugin.to_string(), "track": tid.to_string()}),
    ))
}

fn layer_set(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let id: NodeId = a.id("id")?;
    let keys = a.opt_str("keys").map(keys_of).transpose()?;
    let velocity = a.opt_str("velocity").map(velocity_of).transpose()?;
    let transpose = a.opt_i64("transpose").map(|t| t as i8);
    let gain = a.opt_f64("gain");
    let pan = a.opt_f64("pan");
    let mute = a.opt_str("mute").map(|m| m == "on");
    let label = a.opt_str("label").map(str::trim);
    if label == Some("") {
        return Err(CmdError::invalid("--label が空です"));
    }
    if keys.is_none()
        && velocity.is_none()
        && transpose.is_none()
        && gain.is_none()
        && pan.is_none()
        && mute.is_none()
        && label.is_none()
    {
        return Err(CmdError::invalid(
            "--keys・--velocity・--transpose・--gain・--pan・--mute・--label のいずれかが必要です",
        ));
    }
    let tid = node_track(p, id)?;
    let l = layer_mut(p, tid, |l| l.id == id).ok_or_else(|| not_layer(id))?;
    if let Some(k) = keys {
        l.keys = k;
    }
    if let Some(v) = velocity {
        l.velocity = v;
    }
    if let Some(x) = transpose {
        l.transpose = x;
    }
    if let Some(g) = gain {
        l.gain_db = g;
    }
    if let Some(x) = pan {
        l.pan = x;
    }
    if let Some(m) = mute {
        l.mute = m;
    }
    if let Some(n) = label {
        l.name = n.to_string();
    }
    Ok(Output::msg(format!("レイヤー「{}」を設定しました", l.name)))
}

fn layer_remove(p: &mut Project, id: NodeId) -> CmdResult<Output> {
    let tid = node_track(p, id)?;
    let t = p.track_mut(tid)?;
    let Some(Source::Multi { layers, .. }) = t.rack.as_mut().and_then(|r| r.source.as_mut()) else {
        return Err(not_layer(id));
    };
    let i = layers
        .iter()
        .position(|l| l.id == id)
        .ok_or_else(|| not_layer(id))?;
    if layers.len() == 1 {
        return Err(CmdError::conflict(
            "最後のレイヤーは外せません（インストゥルメントごと消すなら plugin.remove を使ってください）",
        ));
    }
    let gone = layers.remove(i);
    let mut plugins = vec![gone.instrument];
    plugins_in(&gone.fx, &mut plugins);
    // The sidechain sends the removed plug-ins received go with them, as with `plugin.remove`.
    let gone_keys: Vec<SendId> = t
        .inserts
        .iter()
        .filter(|x| plugins.contains(&x.id))
        .filter_map(|x| x.sidechain.as_ref())
        .flat_map(|sc| sc.sources.iter().map(|s| s.id))
        .collect();
    t.inserts.retain(|x| !plugins.contains(&x.id));
    p.fix_track_rack(tid);
    for t in p.tracks.keys().copied().collect::<Vec<_>>() {
        p.track_mut(t)?
            .automation
            .retain(|l| !matches!(&l.target, AutoTarget::Send(id) if gone_keys.contains(id)));
    }
    Ok(Output::msg(format!(
        "レイヤー「{}」をプラグイン {} 件とともに削除しました",
        gone.name,
        plugins.len()
    )))
}

/// Turns the track's single instrument into a multi-instrument of one layer; a multi, or a
/// track with no instrument, is left as it is.
fn make_multi(p: &mut Project, tid: TrackId) -> CmdResult<()> {
    let t = p.track(tid)?;
    let single = match t
        .rack
        .as_ref()
        .map_or_else(|| Rack::serial(t).source, |r| r.source.clone())
    {
        Some(Source::Single(i)) => i,
        _ => return Ok(()),
    };
    let name = t
        .inserts
        .iter()
        .find(|x| x.id == single)
        .map(|x| x.name.clone())
        .unwrap_or_default();
    let (multi, layer) = (NodeId(p.alloc()), NodeId(p.alloc()));
    let r = rack_of(p.track_mut(tid)?);
    r.source = Some(Source::Multi {
        id: multi,
        layers: vec![Layer {
            id: layer,
            name,
            instrument: single,
            fx: Vec::new(),
            keys: (0, 127),
            velocity: (1, 127),
            transpose: 0,
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
        }],
    });
    p.fix_track_rack(tid);
    Ok(())
}

/// The layer `hit` picks in the track's multi-instrument.
fn layer_mut(p: &mut Project, tid: TrackId, hit: impl Fn(&Layer) -> bool) -> Option<&mut Layer> {
    match p.track_mut(tid).ok()?.rack.as_mut()?.source.as_mut()? {
        Source::Multi { layers, .. } => layers.iter_mut().find(|l| hit(l)),
        Source::Single(_) => None,
    }
}

fn not_layer(id: NodeId) -> CmdError {
    CmdError::not_found(format!("レイヤー {id} が見つかりません"))
}

/// A key range: `C1-B2` (60 is C3, as the piano roll names it, UI-335) or `36-59`; one note is a
/// range of one.
fn keys_of(s: &str) -> CmdResult<(u8, u8)> {
    let s = s.trim();
    let bad = || CmdError::invalid(format!("--keys `{s}` を読み取れません（C1-B2 か 36-59）"));
    let pitch = xirius_params::pitch::parse;
    // A note name may hold a dash of its own (`C-2`): the range splits at the dash that leaves
    // a pitch on both sides.
    let (lo, hi) = pitch(s)
        .map(|p| (p, p))
        .or_else(|| {
            s.char_indices()
                .filter(|(_, c)| *c == '-')
                .find_map(|(i, _)| Some((pitch(&s[..i])?, pitch(&s[i + 1..])?)))
        })
        .ok_or_else(bad)?;
    if lo > hi {
        return Err(CmdError::range("--keys の下限が上限を越えています"));
    }
    Ok((lo, hi))
}

/// A velocity range `1-127`; one number is a range of one.
fn velocity_of(s: &str) -> CmdResult<(u8, u8)> {
    let bad = || CmdError::invalid(format!("--velocity `{s}` を読み取れません（1-127）"));
    let (lo, hi) = s.trim().split_once('-').unwrap_or((s.trim(), s.trim()));
    let (lo, hi): (u8, u8) = (
        lo.trim().parse().map_err(|_| bad())?,
        hi.trim().parse().map_err(|_| bad())?,
    );
    if !(1..=127).contains(&lo) || !(1..=127).contains(&hi) {
        return Err(CmdError::range("--velocity は 1〜127 です"));
    }
    if lo > hi {
        return Err(CmdError::range("--velocity の下限が上限を越えています"));
    }
    Ok((lo, hi))
}

fn mode_of(s: &str) -> CmdResult<SplitMode> {
    match s {
        "normal" => Ok(SplitMode::Normal),
        "channel" => Ok(SplitMode::Channel),
        "frequency" => Ok(SplitMode::Frequency),
        _ => Err(CmdError::invalid(format!("--mode `{s}` を読み取れません"))),
    }
}

fn empty_branch(id: NodeId) -> Branch {
    Branch {
        id,
        fx: Vec::new(),
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
    }
}

fn not_split(id: NodeId) -> CmdError {
    CmdError::not_found(format!("スプリッター {id} が見つかりません"))
}

fn not_branch(id: NodeId) -> CmdError {
    CmdError::not_found(format!("分岐 {id} が見つかりません"))
}

/// The track's rack, made from its serial inserts when it has none yet.
pub(super) fn rack_of(t: &mut Track) -> &mut Rack {
    if t.rack.is_none() {
        t.rack = Some(Rack::serial(t));
    }
    t.rack.as_mut().expect("just set")
}

/// The track whose rack holds node `id`.
fn node_track(p: &Project, id: NodeId) -> CmdResult<TrackId> {
    p.tracks
        .iter()
        .find(|(_, t)| t.rack.as_ref().is_some_and(|r| r.node_ids().contains(&id)))
        .map(|(tid, _)| *tid)
        .ok_or_else(|| CmdError::not_found(format!("ラックの {id} が見つかりません")))
}

/// Every chain the rack starts from: the track's fx, then each layer's.
fn roots(r: &mut Rack) -> Vec<&mut Vec<RackNode>> {
    let mut out = vec![&mut r.fx];
    if let Some(Source::Multi { layers, .. }) = &mut r.source {
        out.extend(layers.iter_mut().map(|l| &mut l.fx));
    }
    out
}

pub(super) fn chain_mut(r: &mut Rack, c: Chain) -> Option<&mut Vec<RackNode>> {
    match c {
        Chain::Top => Some(&mut r.fx),
        Chain::Branch(b) => find_branch_parent(r, b).map(|(s, i)| &mut s.branches[i].fx),
        Chain::Layer(l) => layer_fx(r, l),
    }
}

/// The fx chain of layer `id` of the rack's multi-instrument.
fn layer_fx(r: &mut Rack, id: NodeId) -> Option<&mut Vec<RackNode>> {
    match &mut r.source {
        Some(Source::Multi { layers, .. }) => {
            layers.iter_mut().find(|l| l.id == id).map(|l| &mut l.fx)
        }
        _ => None,
    }
}

pub(super) fn insert_at(r: &mut Rack, c: Chain, index: usize, node: RackNode) {
    let chain = chain_mut(r, c).expect("chain checked by the caller");
    let at = index.min(chain.len());
    chain.insert(at, node);
}

/// Where `--at` points: the chain, the index in it and the chain's split depth.
pub(super) fn place(r: &mut Rack, at: &str) -> CmdResult<(Chain, usize, usize)> {
    if at == "track" {
        return Ok((Chain::Top, r.fx.len(), 0));
    }
    if let Some(id) = at.strip_prefix("after:") {
        let id: AnyId = id.parse().map_err(CmdError::invalid)?;
        // After the instrument is the head of the fx.
        if let (AnyId::Plugin(x), Some(Source::Single(i))) = (id, &r.source)
            && x == *i
        {
            return Ok((Chain::Top, 0, 0));
        }
        let hit = |n: &RackNode| match (n, id) {
            (RackNode::Plugin(x), AnyId::Plugin(y)) => *x == y,
            (RackNode::Split(s), AnyId::RackNode(y)) => s.id == y,
            _ => false,
        };
        let layers: Vec<(NodeId, &[RackNode])> = match &r.source {
            Some(Source::Multi { layers, .. }) => {
                layers.iter().map(|l| (l.id, l.fx.as_slice())).collect()
            }
            _ => Vec::new(),
        };
        return locate(&r.fx, &hit, Chain::Top, 0)
            .or_else(|| {
                layers
                    .into_iter()
                    .find_map(|(l, fx)| locate(fx, &hit, Chain::Layer(l), 0))
            })
            .map(|(c, i, d)| (c, i + 1, d))
            .ok_or_else(|| CmdError::not_found(format!("FX に {id} がありません")));
    }
    if let Some(rest) = at.strip_prefix("branch:") {
        let (b, i) = rest.split_once(':').unwrap_or((rest, ""));
        let b: NodeId = b.parse().map_err(CmdError::invalid)?;
        let d = branch_depth(r, b).ok_or_else(|| not_branch(b))?;
        let len = chain_mut(r, Chain::Branch(b)).map(|c| c.len()).unwrap_or(0);
        let i = if i.is_empty() {
            len
        } else {
            i.parse::<usize>()
                .map_err(|_| CmdError::invalid(format!("--at `{at}` の位置を読み取れません")))?
        };
        return Ok((Chain::Branch(b), i.min(len), d));
    }
    if let Some(rest) = at.strip_prefix("layer:") {
        let (l, i) = rest.split_once(':').unwrap_or((rest, ""));
        let l: NodeId = l.parse().map_err(CmdError::invalid)?;
        let len = layer_fx(r, l)
            .map(|c| c.len())
            .ok_or_else(|| CmdError::not_found(format!("レイヤー {l} が見つかりません")))?;
        let i = if i.is_empty() {
            len
        } else {
            i.parse::<usize>()
                .map_err(|_| CmdError::invalid(format!("--at `{at}` の位置を読み取れません")))?
        };
        return Ok((Chain::Layer(l), i.min(len), 0));
    }
    Err(CmdError::invalid(format!(
        "--at `{at}` を読み取れません（track・after:<id>・branch:<分岐>:<位置>・layer:<レイヤー>:<位置>）"
    )))
}

/// The node `hit` picks in `nodes` (a chain and what it holds): its chain, index and split
/// depth.
fn locate(
    nodes: &[RackNode],
    hit: &dyn Fn(&RackNode) -> bool,
    chain: Chain,
    depth: usize,
) -> Option<(Chain, usize, usize)> {
    for (i, n) in nodes.iter().enumerate() {
        if hit(n) {
            return Some((chain, i, depth));
        }
        if let RackNode::Split(s) = n {
            for b in &s.branches {
                if let Some(x) = locate(&b.fx, hit, Chain::Branch(b.id), depth + 1) {
                    return Some(x);
                }
            }
        }
    }
    None
}

/// The split depth of branch `id`'s chain (a top-level split's branches are at 1).
fn branch_depth(r: &Rack, id: NodeId) -> Option<usize> {
    fn walk(nodes: &[RackNode], id: NodeId, depth: usize) -> Option<usize> {
        nodes.iter().find_map(|n| match n {
            RackNode::Split(s) => s.branches.iter().find_map(|b| {
                if b.id == id {
                    Some(depth + 1)
                } else {
                    walk(&b.fx, id, depth + 1)
                }
            }),
            RackNode::Plugin(_) => None,
        })
    }
    let layers = match &r.source {
        Some(Source::Multi { layers, .. }) => layers.as_slice(),
        _ => &[],
    };
    walk(&r.fx, id, 0).or_else(|| layers.iter().find_map(|l| walk(&l.fx, id, 0)))
}

fn find_split(r: &mut Rack, id: NodeId) -> Option<&mut Split> {
    fn walk(nodes: &mut [RackNode], id: NodeId) -> Option<&mut Split> {
        for n in nodes {
            if let RackNode::Split(s) = n {
                if s.id == id {
                    return Some(s);
                }
                for b in &mut s.branches {
                    if let Some(x) = walk(&mut b.fx, id) {
                        return Some(x);
                    }
                }
            }
        }
        None
    }
    roots(r).into_iter().find_map(|c| walk(c, id))
}

/// The split holding branch `id`, and the branch's index in it.
fn find_branch_parent(r: &mut Rack, id: NodeId) -> Option<(&mut Split, usize)> {
    fn walk(nodes: &mut [RackNode], id: NodeId) -> Option<(&mut Split, usize)> {
        for n in nodes {
            if let RackNode::Split(s) = n {
                if let Some(i) = s.branches.iter().position(|b| b.id == id) {
                    return Some((s, i));
                }
                for b in &mut s.branches {
                    if let Some(x) = walk(&mut b.fx, id) {
                        return Some(x);
                    }
                }
            }
        }
        None
    }
    roots(r).into_iter().find_map(|c| walk(c, id))
}

/// Takes the node `hit` picks out of whichever chain holds it.
fn take_node(r: &mut Rack, hit: &dyn Fn(&RackNode) -> bool) -> Option<RackNode> {
    fn walk(nodes: &mut Vec<RackNode>, hit: &dyn Fn(&RackNode) -> bool) -> Option<RackNode> {
        if let Some(i) = nodes.iter().position(hit) {
            return Some(nodes.remove(i));
        }
        for n in nodes.iter_mut() {
            if let RackNode::Split(s) = n {
                for b in &mut s.branches {
                    if let Some(x) = walk(&mut b.fx, hit) {
                        return Some(x);
                    }
                }
            }
        }
        None
    }
    roots(r).into_iter().find_map(|c| walk(c, hit))
}

/// How many splits deep `s` reaches, itself counted.
pub(super) fn height(s: &Split) -> usize {
    1 + s
        .branches
        .iter()
        .flat_map(|b| &b.fx)
        .map(|n| match n {
            RackNode::Split(x) => height(x),
            RackNode::Plugin(_) => 0,
        })
        .max()
        .unwrap_or(0)
}

/// The node ids inside `s`, itself included.
fn node_ids_of(s: &Split) -> Vec<NodeId> {
    let mut out = vec![s.id];
    for b in &s.branches {
        out.push(b.id);
        for n in &b.fx {
            if let RackNode::Split(x) = n {
                out.extend(node_ids_of(x));
            }
        }
    }
    out
}

pub(super) fn plugins_in(nodes: &[RackNode], out: &mut Vec<PluginId>) {
    for n in nodes {
        match n {
            RackNode::Plugin(id) => out.push(*id),
            RackNode::Split(s) => {
                for b in &s.branches {
                    plugins_in(&b.fx, out);
                }
            }
        }
    }
}
