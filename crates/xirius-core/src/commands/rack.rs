//! `rack.show`: the track's insert wiring as a tree (PRJ-026). The `rack.split.*`,
//! `rack.branch.*`, `rack.node.move` and `rack.flatten` edits shape it (ENG-074): a track
//! without a rack gets its serial one first, and the edited rack is fixed the way a song is
//! read, so a splitter left with one branch folds back into a line.
use serde_json::json;

use super::*;
use crate::auto::AutoTarget;
use crate::id::{AnyId, IdKind, NodeId, PluginId, SendId, TrackId};
use crate::model::{MAX_DB, MIN_DB, PluginKind, Project, Track};
use crate::rack::{
    Branch, MAX_BRANCHES, MAX_SPLIT_DEPTH, Rack, RackNode, Source, Split, SplitMode,
};

const MODES: &[&str] = &["normal", "channel", "frequency"];

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
            "トラックの FX にスプリッターを追加します（--at は track（FX の末尾）・after:<プラグインかスプリッター>・branch:<分岐>:<位置>。分岐は空で作ります）",
            vec![
                ArgDef::id("track", IdKind::Track, "トラック").req(),
                ArgDef::text(
                    "at",
                    "置く所: track・after:<id>・branch:<分岐の id>:<位置>（省略時は track）",
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
            "エフェクトかスプリッターを、同じトラックの FX か分岐の中へ移します（--index は 0 から。省略時は末尾）",
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
                    "移す先: 分岐か、そのトラック",
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
        _ => return Err(CmdError::invalid("--to は分岐かトラックです")),
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

/// A chain of the rack, by what holds it: the track's fx, or a branch.
#[derive(Clone, Copy)]
enum Chain {
    Top,
    Branch(NodeId),
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
fn rack_of(t: &mut Track) -> &mut Rack {
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

fn chain_mut(r: &mut Rack, c: Chain) -> Option<&mut Vec<RackNode>> {
    match c {
        Chain::Top => Some(&mut r.fx),
        Chain::Branch(b) => find_branch_parent(r, b).map(|(s, i)| &mut s.branches[i].fx),
    }
}

fn insert_at(r: &mut Rack, c: Chain, index: usize, node: RackNode) {
    let chain = chain_mut(r, c).expect("chain checked by the caller");
    let at = index.min(chain.len());
    chain.insert(at, node);
}

/// Where `--at` points: the chain, the index in it and the chain's split depth.
fn place(r: &mut Rack, at: &str) -> CmdResult<(Chain, usize, usize)> {
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
        return locate(&r.fx, &hit, Chain::Top, 0)
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
    Err(CmdError::invalid(format!(
        "--at `{at}` を読み取れません（track・after:<id>・branch:<分岐>:<位置>）"
    )))
}

/// The node `hit` picks in `nodes` (the track's fx only; a layer's chain is not a place a
/// command names): its chain, index and split depth.
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
    walk(&r.fx, id, 0)
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
fn height(s: &Split) -> usize {
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

fn plugins_in(nodes: &[RackNode], out: &mut Vec<PluginId>) {
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
