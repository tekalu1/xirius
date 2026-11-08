//! The sample files of the Sampler and the Drum Kit (PLG-067, PLG-068). `plugin.samples.*`
//! adds, swaps and takes away the files a device plays, each in one step of the history. A
//! file's own values sit beside the device's in `params` as `file<n>.<id>`, `n` its place in
//! `samples` from 1, so taking a file away moves the later files' values down with them. A
//! Drum Kit's file is a layer of the pad its `file<n>.pad` names: files added together go on
//! the free pads in order, one pad a name (`kick_v1`, `kick_v2` are velocity layers of one
//! pad, `hat_rr1`, `hat_rr2` its round robin).

use std::collections::BTreeMap;

use serde_json::json;
use xirius_params::{BuiltinDef, PAD_NAMES, PADS, Scope, file_key, pad_key, pad_of};

use super::*;
use crate::command::EditCtx;
use crate::id::{IdKind, PluginId};
use crate::model::Plugin;

pub(super) fn defs() -> Vec<CommandDef> {
    let plugin = || ArgDef::id("id", IdKind::Plugin, "サンプラーかドラムキット").req();
    vec![
        session(
            "plugin.samples.load",
            "サンプラー・ドラムキットに音声ファイルか、フォルダーの中の音声ファイルを名前の順に読み込みます（plugin.samples.add と同じ置き方。--path と --paths は合わせて 1 回で取り消せます）",
            vec![
                plugin(),
                ArgDef::text(
                    "path",
                    "音声ファイルかフォルダー（絶対パスまたは曲のフォルダーからの相対パス）",
                ),
                ArgDef::objects(
                    "paths",
                    "まとめて読み込む音声ファイルかフォルダー [{path}]（画面へまとめてドロップしたもの）",
                ),
                ArgDef::text(
                    "pad",
                    "ドラムキットで置き始めるパッド（A1〜H16。そこから順に置き、ファイルのあるパッドは置き換え、H16 を越えた分は入れない。省略時は A1 から空いたパッド）",
                ),
                ArgDef::flag(
                    "layers",
                    "ドラムキットで、全部を 1 つのパッドのレイヤーにする（--pad があればそのパッドへ足す）",
                ),
                ArgDef::flag("browse", "選択パッドから空きパッドへ 1 ファイルずつ入れる"),
                ArgDef::flag("replace", "今のサンプルを外してから読み込む"),
                ArgDef::int(
                    "index",
                    "このサンプル（1 から）を、読み込んだ 1 つ目のファイルに差し替える（値はそのまま）",
                )
                .range(1.0, 100_000.0),
            ],
            Risk::Write,
            false,
        ),
        session(
            "plugin.samples.step",
            "サンプルを、同じフォルダーの前か次の音声ファイルに差し替えます（名前の順。端では反対の端へ回ります）",
            vec![
                plugin(),
                ArgDef::int("index", "差し替えるサンプル（1 から）")
                    .req()
                    .range(1.0, 100_000.0),
                ArgDef::int("by", "進む数（-1 で前、1 で次）")
                    .default(json!(1))
                    .range(-100_000.0, 100_000.0),
            ],
            Risk::Write,
            false,
        ),
        edit(
            "plugin.samples.add",
            "サンプラー・ドラムキットにサンプルのファイルを足します。ドラムキットでは、名前ごとに 1 つずつパッドへ置きます（名前の末尾の _v1・_v2 はベロシティのレイヤー、_rr1・_rr2 はラウンドロビン）",
            vec![
                plugin(),
                ArgDef::text(
                    "file",
                    "音声ファイル（絶対パスまたは曲のフォルダーからの相対パス）",
                ),
                ArgDef::objects("files", "まとめて足すファイル [{file}]"),
                ArgDef::text(
                    "pad",
                    "ドラムキットで置き始めるパッド（A1〜H16。そこから順に置き、ファイルのあるパッドは置き換え、H16 を越えた分は入れない。省略時は A1 から空いたパッド）",
                ),
                ArgDef::flag(
                    "layers",
                    "ドラムキットで、全部を 1 つのパッドのレイヤーにする（--pad があればそのパッドへ足す）",
                ),
                ArgDef::flag("browse", "選択パッドから空きパッドへ 1 ファイルずつ入れる"),
                ArgDef::flag("replace", "今のサンプルを外してから足す"),
            ],
            add,
        ),
        edit(
            "plugin.samples.set",
            "サンプルのファイルを差し替えます（そのサンプルの値はそのまま）",
            vec![
                plugin(),
                ArgDef::int("index", "差し替えるサンプル（1 から）")
                    .req()
                    .range(1.0, 100_000.0),
                ArgDef::text(
                    "file",
                    "音声ファイル（絶対パスまたは曲のフォルダーからの相対パス）",
                )
                .req(),
            ],
            set_file,
        ),
        destroy(
            "plugin.samples.remove",
            "サンプルを外します（--index・--pad・--bank・--all のいずれか 1 つ）",
            vec![
                plugin(),
                ArgDef::int("index", "外すサンプル（1 から）").range(1.0, 100_000.0),
                ArgDef::text(
                    "pad",
                    "ドラムキットで、このパッドのサンプルを全部外す（A1〜H16）",
                ),
                ArgDef::text(
                    "bank",
                    "ドラムキットで、このバンクの 16 パッドのサンプルを全部外す（A〜H）",
                ),
                ArgDef::flag("all", "全部外す"),
            ],
            remove,
        ),
    ]
}

/// The pads a set of names fills on a Drum Kit: each pad's files (indexes into the names) in
/// the order of their layers, and whether they take turns (round robin) rather than split
/// the velocities. With `layers` every name is a layer of one pad.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KitGroup {
    pub files: Vec<usize>,
    pub round_robin: bool,
}

/// Groups `names` into pads ([`KitGroup`]): names that differ only in a `_v<n>` or `_rr<n>`
/// at the end of the name share a pad, in the order of `n`; the pads keep the order the
/// names first come in.
pub fn kit_groups(names: &[String], layers: bool) -> Vec<KitGroup> {
    let parsed: Vec<(String, Option<(bool, u32)>)> = names.iter().map(|n| layer_of(n)).collect();
    if layers {
        let mut files: Vec<usize> = (0..names.len()).collect();
        files.sort_by_key(|i| parsed[*i].1.map_or(0, |(_, n)| n));
        let round_robin = parsed.iter().any(|(_, l)| matches!(l, Some((true, _))));
        return vec![KitGroup { files, round_robin }];
    }
    let mut keys: Vec<String> = Vec::new();
    let mut groups: Vec<KitGroup> = Vec::new();
    for (i, (base, layer)) in parsed.iter().enumerate() {
        let key = match layer {
            Some(_) => base.to_lowercase(),
            // A name without a layer is a pad of its own, even when another one's base is
            // the same.
            None => format!("\0{i}"),
        };
        match keys.iter().position(|k| *k == key) {
            Some(g) => groups[g].files.push(i),
            None => {
                keys.push(key);
                groups.push(KitGroup {
                    files: vec![i],
                    round_robin: matches!(layer, Some((true, _))),
                });
            }
        }
    }
    for g in &mut groups {
        g.files.sort_by_key(|i| parsed[*i].1.map_or(0, |(_, n)| n));
    }
    groups
}

/// A file name's base and its layer: `kick_v2.wav` is (`kick`, velocity 2), `hat-rr1.wav`
/// (`hat`, round robin 1).
fn layer_of(name: &str) -> (String, Option<(bool, u32)>) {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = leaf.rsplit_once('.').map_or(leaf, |(s, _)| s);
    let Some(cut) = stem.rfind(['_', '-', ' ']) else {
        return (stem.to_string(), None);
    };
    let (base, tail) = (&stem[..cut], &stem[cut + 1..]);
    let lower = tail.to_ascii_lowercase();
    let (rr, digits) = if let Some(d) = lower.strip_prefix("rr") {
        (true, d)
    } else if let Some(d) = lower.strip_prefix('v') {
        (false, d)
    } else {
        return (stem.to_string(), None);
    };
    match digits.parse::<u32>() {
        Ok(n) if !base.is_empty() && !digits.is_empty() => (base.to_string(), Some((rr, n))),
        _ => (stem.to_string(), None),
    }
}

/// The pads of the Drum Kit `x` that have files.
pub fn used_pads(def: &BuiltinDef, x: &Plugin) -> [bool; PADS] {
    let mut used = [false; PADS];
    for i in 0..x.samples.len() {
        used[def.file_pad(&x.params, i)] = true;
    }
    used
}

/// The pads files go on, as the mock fills them (N3-kit-2): with `layers` and a pad given,
/// that pad, whether it has files or not (they are added to its layers); with a pad, the
/// pads from it in order, a pad that has files given the new ones in place of its own; with
/// none (a command that says no pad), the free pads from A1. The groups past H16, or past
/// the last free pad, do not go in: fewer pads than groups.
pub fn kit_pads(
    used: &[bool; PADS],
    start: Option<usize>,
    groups: usize,
    layers: bool,
) -> Vec<usize> {
    match start {
        Some(pad) if layers && groups == 1 => vec![pad.min(PADS - 1)],
        Some(pad) => (pad..PADS).take(groups).collect(),
        None => (0..PADS).filter(|p| !used[*p]).take(groups).collect(),
    }
}

/// Browse keeps the selected pad and skips occupied pads after it. Each selection is
/// one sound, even when filenames resemble velocity layers.
pub fn kit_browse(used: &[bool; PADS], from: usize, count: usize) -> (Vec<KitGroup>, Vec<usize>) {
    let from = from.min(PADS - 1);
    let groups = (0..count)
        .map(|i| KitGroup {
            files: vec![i],
            round_robin: false,
        })
        .collect();
    let pads = (from..PADS)
        .filter(|p| *p == from || !used[*p])
        .take(count)
        .collect();
    (groups, pads)
}

/// The pads of `pads` whose files a drop takes the place of: the ones that have files, but
/// not the one it adds layers to.
pub fn kit_replaced(used: &[bool; PADS], pads: &[usize], layers: bool) -> Vec<usize> {
    if layers && pads.len() == 1 {
        return Vec::new();
    }
    pads.iter().copied().filter(|p| used[*p]).collect()
}

/// The bank a letter like `A` or `h` means (0 to 7).
fn bank_of(name: &str) -> Option<usize> {
    let mut chars = name.trim().chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let b = (c.to_ascii_uppercase() as usize).checked_sub('A' as usize)?;
    (b < PADS / BANK).then_some(b)
}

/// The pads of a bank.
const BANK: usize = 16;

/// The device a sample command works on: a built-in that has sample files.
fn device(p: &mut Project, id: PluginId) -> CmdResult<(&mut Plugin, &'static BuiltinDef)> {
    let tid = p
        .tracks
        .iter()
        .find(|(_, t)| t.inserts.iter().any(|x| x.id == id))
        .map(|(tid, _)| *tid)
        .ok_or_else(|| CmdError::not_found(format!("プラグイン {id} が見つかりません")))?;
    let x = p
        .track_mut(tid)?
        .inserts
        .iter_mut()
        .find(|x| x.id == id)
        .expect("found");
    match x.builtin().filter(|d| !d.per_file.is_empty()) {
        Some(def) => Ok((x, def)),
        None => Err(CmdError::invalid(format!(
            "{} はサンプルを読みません（サンプラーかドラムキットを指定してください）",
            x.name
        ))),
    }
}

fn pad_arg(a: &Args) -> CmdResult<Option<usize>> {
    a.opt_str("pad")
        .map(|s| {
            pad_of(s).ok_or_else(|| {
                CmdError::invalid(format!("パッドは A1〜H16 で指定してください（`{s}`）"))
            })
        })
        .transpose()
}

fn add(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: PluginId = a.id("id")?;
    let mut names: Vec<String> = a.opt_str("file").map(str::to_string).into_iter().collect();
    if let Some(list) = a.0.get("files").and_then(Value::as_array) {
        for o in list {
            match o.get("file").and_then(Value::as_str) {
                Some(f) => names.push(f.to_string()),
                None => return Err(CmdError::invalid("files には [{file}] を指定してください")),
            }
        }
    }
    names.retain(|n| !n.trim().is_empty());
    if names.is_empty() {
        return Err(CmdError::invalid("--file か --files を指定してください"));
    }
    let start = pad_arg(a)?;
    let layers = a.flag("layers");
    let (x, def) = device(p, id)?;
    if def.per_pad.is_empty() && (start.is_some() || layers) {
        return Err(CmdError::invalid(
            "--pad と --layers はドラムキットにだけ使えます",
        ));
    }
    // A pad the new files fill keeps its Mute and Solo, as one a drop replaces does (the
    // fifth review's low 11).
    let mut hand = Vec::new();
    if a.flag("replace") {
        if !def.per_pad.is_empty() {
            hand = (0..PADS)
                .flat_map(|pad| ["mute", "solo"].map(|id| (pad, pad_key(pad, id))))
                .filter_map(|(pad, k)| x.params.get(&k).map(|v| (pad, k, *v)))
                .collect();
        }
        // The pads' own values go with their files: a file dropped there later does not
        // sound with the old Choke or Trigger (L2-smp-2).
        x.samples.clear();
        x.params
            .retain(|k, _| !matches!(scope(def, k), Some(Scope::File(_) | Scope::Pad(_))));
    }
    if def.per_pad.is_empty() {
        let first = x.samples.len();
        // Values left past the end (a hand-written song's) are not the new files' (L2-smp-2).
        x.params
            .retain(|k, _| !matches!(scope(def, k), Some(Scope::File(n)) if n >= first));
        x.samples.extend(names.iter().cloned());
        return Ok(Output::with(
            format!("{} にサンプルを {} 個足しました", x.name, names.len()),
            json!({ "plugin": id.to_string(), "first": first + 1, "count": names.len() }),
        ));
    }
    let (groups, pads) = if a.flag("browse") {
        kit_browse(&used_pads(def, x), start.unwrap_or(0), names.len())
    } else {
        let groups = kit_groups(&names, layers);
        let pads = kit_pads(&used_pads(def, x), start, groups.len(), layers);
        (groups, pads)
    };
    if pads.is_empty() {
        return Err(CmdError::range("空いたパッドがありません"));
    }
    // A pad the drop falls on that has files takes the new ones in their place, its own
    // values with them (the mock's `fill`). Its Mute and Solo are the hand's say over the
    // pad, not its sound's, and stay (the fourth review's low 21).
    let replaced = kit_replaced(&used_pads(def, x), &pads, layers);
    if !replaced.is_empty() {
        let held: Vec<(String, f64)> = replaced
            .iter()
            .flat_map(|pad| ["mute", "solo"].map(|id| pad_key(*pad, id)))
            .filter_map(|k| x.params.get(&k).map(|v| (k, *v)))
            .collect();
        let gone: Vec<bool> = (0..x.samples.len())
            .map(|i| replaced.contains(&def.file_pad(&x.params, i)))
            .collect();
        drop_files(def, x, &gone);
        x.params.extend(held);
    }
    // Every file says its pad, so taking one away later leaves the others where they are.
    pin_pads(def, x);
    let first = x.samples.len();
    // Values left past the end (a hand-written song's) are not the new files' (L2-smp-2).
    x.params
        .retain(|k, _| !matches!(scope(def, k), Some(Scope::File(n)) if n >= first));
    let used = used_pads(def, x);
    let placed: usize = groups.iter().take(pads.len()).map(|g| g.files.len()).sum();
    let left_out = names.len() - placed;
    let pad_spec = spec(def, "pad");
    let mode = def.per_pad.iter().find(|s| s.id == "layer_mode");
    for (g, pad) in groups.iter().zip(&pads) {
        for f in &g.files {
            let n = x.samples.len();
            x.samples.push(names[*f].clone());
            x.params
                .insert(file_key(n, "pad"), pad_spec.to_norm(*pad as f64));
        }
        // A pad that had files keeps how it picks among them.
        if let Some(m) = mode.filter(|_| !used[*pad]) {
            let k = pad_key(*pad, m.id);
            if g.round_robin {
                x.params.insert(k, m.to_norm(1.0));
            } else {
                x.params.remove(&k);
            }
        }
    }
    let filled = &pads[..groups.len().min(pads.len())];
    x.params.extend(
        hand.into_iter()
            .filter(|(pad, _, _)| filled.contains(pad))
            .map(|(_, k, v)| (k, v)),
    );
    let span = match (pads.first(), pads.last()) {
        (Some(a), Some(b)) if a != b => format!("{}–{}", PAD_NAMES[*a], PAD_NAMES[*b]),
        (Some(a), _) => PAD_NAMES[*a].to_string(),
        _ => String::new(),
    };
    let mut message = format!(
        "{} にサンプルを {placed} 個足しました（{} パッド、{span}）",
        x.name,
        pads.len()
    );
    if left_out > 0 {
        message.push_str(&format!("。入りきらない {left_out} 個は入れていません"));
    }
    Ok(Output::with(
        message,
        json!({
            "plugin": id.to_string(),
            "first": first + 1,
            "count": placed,
            "pads": pads.iter().map(|p| PAD_NAMES[*p]).collect::<Vec<_>>(),
            "replaced": replaced.iter().map(|p| PAD_NAMES[*p]).collect::<Vec<_>>(),
            "left_out": left_out,
        }),
    ))
}

fn set_file(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: PluginId = a.id("id")?;
    let index = a.i64("index")? as usize;
    let file = a.str("file")?.trim().to_string();
    if file.is_empty() {
        return Err(CmdError::invalid("音声ファイルを指定してください"));
    }
    let (x, _) = device(p, id)?;
    let Some(slot) = x.samples.get_mut(index - 1) else {
        return Err(CmdError::not_found(format!(
            "{} にサンプル {index} はありません（{} 個）",
            x.name,
            x.samples.len()
        )));
    };
    *slot = file.clone();
    Ok(Output::with(
        format!("{} のサンプル {index} を差し替えました", x.name),
        json!({ "plugin": id.to_string(), "index": index, "file": file }),
    ))
}

fn remove(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: PluginId = a.id("id")?;
    let index = a.opt_i64("index").map(|i| i as usize);
    let pad = pad_arg(a)?;
    let bank = a
        .opt_str("bank")
        .map(|s| {
            bank_of(s).ok_or_else(|| {
                CmdError::invalid(format!("バンクは A〜H で指定してください（`{s}`）"))
            })
        })
        .transpose()?;
    let all = a.flag("all");
    let given = [index.is_some(), pad.is_some(), bank.is_some(), all];
    if given.iter().filter(|g| **g).count() != 1 {
        return Err(CmdError::invalid(
            "--index・--pad・--bank・--all のいずれか 1 つを指定してください",
        ));
    }
    let (x, def) = device(p, id)?;
    if (pad.is_some() || bank.is_some()) && def.per_pad.is_empty() {
        return Err(CmdError::invalid(
            "--pad と --bank はドラムキットにだけ使えます",
        ));
    }
    let gone: Vec<bool> = (0..x.samples.len())
        .map(|i| match (index, pad, bank) {
            (Some(n), _, _) => i + 1 == n,
            (_, Some(p), _) => def.file_pad(&x.params, i) == p,
            (_, _, Some(b)) => def.file_pad(&x.params, i) / BANK == b,
            _ => true,
        })
        .collect();
    let count = gone.iter().filter(|g| **g).count();
    if count == 0 {
        return Err(CmdError::not_found(match index {
            Some(n) => format!("{} にサンプル {n} はありません", x.name),
            None => format!("{} のそのパッドにサンプルはありません", x.name),
        }));
    }
    drop_files(def, x, &gone);
    Ok(Output::with(
        format!("{} のサンプルを {count} 個外しました", x.name),
        json!({ "plugin": id.to_string(), "removed": count, "left": x.samples.len() }),
    ))
}

/// Takes the files `gone` says away from the device `x`: the later files' values move down
/// with them, and a Drum Kit's files stay on their pads. A pad left with no file loses its
/// own values (L2-smp-2).
pub fn drop_files(def: &BuiltinDef, x: &mut Plugin, gone: &[bool]) {
    pin_pads(def, x);
    let before = used_pads(def, x);
    // Where each kept file goes: its values move down with it.
    let mut to = vec![None; gone.len()];
    let mut next = 0;
    for (i, g) in gone.iter().enumerate() {
        if !g {
            to[i] = Some(next);
            next += 1;
        }
    }
    let mut kept = [false; PADS];
    for (i, g) in gone.iter().enumerate() {
        if !g {
            kept[def.file_pad(&x.params, i)] = true;
        }
    }
    let mut params = BTreeMap::new();
    for (k, v) in std::mem::take(&mut x.params) {
        match def.resolve(&k) {
            Some(r) => match r.scope {
                Scope::File(i) => {
                    if let Some(Some(n)) = to.get(i) {
                        params.insert(file_key(*n, r.spec.id), v);
                    }
                }
                Scope::Pad(pad) if before[pad] && !kept[pad] => {}
                _ => {
                    params.insert(k, v);
                }
            },
            None => {
                params.insert(k, v);
            }
        }
    }
    x.params = params;
    let mut i = 0;
    x.samples.retain(|_| {
        i += 1;
        !gone.get(i - 1).copied().unwrap_or(false)
    });
}

/// Writes down the pad of every file of a Drum Kit that has none (it was on the pad of its
/// place in the list), so moving the files keeps them where they are.
fn pin_pads(def: &BuiltinDef, x: &mut Plugin) {
    if def.per_pad.is_empty() {
        return;
    }
    let s = spec(def, "pad");
    for i in 0..x.samples.len() {
        let pad = def.file_pad(&x.params, i);
        x.params
            .entry(file_key(i, "pad"))
            .or_insert_with(|| s.to_norm(pad as f64));
    }
}

fn spec(def: &BuiltinDef, id: &str) -> &'static xirius_params::ParamSpec {
    def.per_file
        .iter()
        .find(|s| s.id == id)
        .expect("a Drum Kit's file has a pad")
}

fn scope(def: &BuiltinDef, key: &str) -> Option<Scope> {
    def.resolve(key).map(|r| r.scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn layered_names_share_a_pad_in_the_order_of_their_number() {
        let n = names(&[
            "kick_v2.wav",
            "snare.wav",
            "kick_v1.wav",
            "hat-rr2.wav",
            "hat-rr1.wav",
            "Kick_V3.wav",
            "snare.wav",
        ]);
        let g = kit_groups(&n, false);
        let files: Vec<(Vec<usize>, bool)> =
            g.iter().map(|g| (g.files.clone(), g.round_robin)).collect();
        assert_eq!(
            files,
            [
                (vec![2, 0, 5], false),
                (vec![1], false),
                (vec![4, 3], true),
                (vec![6], false),
            ]
        );
        let one = kit_groups(&n, true);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].files.len(), 7);
        assert!(one[0].round_robin);
    }

    #[test]
    fn a_name_ending_in_other_words_is_its_own_pad() {
        assert_eq!(layer_of("vox_verb.wav"), ("vox_verb".into(), None));
        assert_eq!(
            layer_of("dir/tom_v10.flac"),
            ("tom".into(), Some((false, 10)))
        );
        assert_eq!(layer_of("rr1.wav"), ("rr1".into(), None));
        assert_eq!(layer_of("a_v.wav"), ("a_v".into(), None));
    }

    #[test]
    fn n3_kit_2_a_drop_fills_the_pads_from_its_own_and_stops_at_h16() {
        let mut used = [false; PADS];
        used[1] = true;
        // From a pad: in order, the used one too (it is replaced).
        assert_eq!(kit_pads(&used, Some(0), 3, false), [0, 1, 2]);
        assert_eq!(kit_replaced(&used, &[0, 1, 2], false), [1]);
        // Past H16 they do not go in.
        assert_eq!(kit_pads(&used, Some(126), 3, false), [126, 127]);
        // Layers go on the pad, which keeps its files.
        assert_eq!(kit_pads(&used, Some(1), 1, true), [1]);
        assert!(kit_replaced(&used, &[1], true).is_empty());
        // One sound on a pad that has one: it takes its place.
        assert_eq!(kit_replaced(&used, &[1], false), [1]);
        // No pad given: the free ones from A1.
        assert_eq!(kit_pads(&used, None, 3, false), [0, 2, 3]);
        assert!(kit_pads(&[true; PADS], None, 3, false).is_empty());
    }
}
