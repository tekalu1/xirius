//! The reviewed per-target routing table. This moves live Items, never recreates commands.
use super::{
    Item, Run,
    layout::{self, Category, Entry},
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Debug)]
struct Route {
    id: String,
    target: String,
    before: Vec<String>,
    after: Vec<String>,
    category: Category,
    dynamic: bool,
    command: String,
}

fn routes() -> &'static [Route] {
    static ROUTES: OnceLock<Vec<Route>> = OnceLock::new();
    ROUTES.get_or_init(|| {
        let mut before: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/context_menus/inventory.json"))
                .expect("approved inventory");
        let mut after: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/context_menus/mapping.json"))
                .expect("approved mapping");
        let supplemental: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/context_menus/supplemental.json"))
                .expect("live inventory additions");
        before.extend(supplemental.clone());
        after.extend(supplemental);
        after
            .iter()
            .map(|to| {
                let from = before
                    .iter()
                    .find(|v| v["id"] == to["id"])
                    .expect("mapped identity");
                let strings = |v: &Value| {
                    v.as_array()
                        .unwrap()
                        .iter()
                        .map(|s| s.as_str().unwrap().to_owned())
                        .collect()
                };
                Route {
                    id: to["id"].as_str().unwrap().into(),
                    target: to["key"].as_str().unwrap().into(),
                    before: strings(&from["asis"]),
                    after: strings(&to["tobe"]),
                    category: Category::ALL
                        .into_iter()
                        .find(|c| Some(c.label()) == to["category"].as_str())
                        .expect("classified route"),
                    dynamic: from["dynamic"].as_bool().unwrap_or(false),
                    command: from["command"].as_str().unwrap_or("").into(),
                }
            })
            .collect()
    })
}

fn normalized(text: &str) -> String {
    let text = text.strip_suffix("（所有中は解除）").unwrap_or(text);
    let text = match text {
        "専用で動かす" => "このプラグインを別のプロセスへ",
        "別のチャンネルへコピー" => "別のチャンネルへ写す",
        _ => text,
    };
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '／' => '/',
            '−' => '-',
            _ => c,
        })
        .collect()
}

fn same_path(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| normalized(a) == normalized(b))
}

// The reviewed inventory used a few descriptive captions instead of the live
// command labels. These aliases are lookup-only; the displayed operation is retained.
fn lookup_path(target: &str, path: &[String]) -> Vec<String> {
    let mut out = path.to_vec();
    for part in &mut out {
        let alias = match (target, part.as_str()) {
            ("other-67", label) if label == "学習" || label.starts_with("学習: ") => {
                "学習（最近触ったターゲット）"
            }
            ("other-19", "まとめを解く") => "フォルダーを展開",
            ("other-21", "ループの始まりをここに") => "ループ開始を設定",
            ("other-18", "出力先を設定") => "出力",
            ("other-21", "ループの終わりをここに") => "ループ終了を設定",
            ("other-22", "ループ") => "ループ切替",
            ("other-22", "ループ範囲のものを選択") => "ループ内選択",
            ("other-24", "セクションのものを選択") => "セクションを選択",
            ("other-26", "テンポポイントを削除") => "テンポを削除",
            ("other-27", "テンポの変化をすべて削除") => "テンポをクリア",
            ("other-41", "ノートを削除") => "削除",
            ("other-49", "トラックを複製") => "トラックを複製（設定だけ）",
            ("other-50", "ミックスダウンを書き出す…") => "ミックスダウン",
            (
                "other-72" | "other-73" | "track-browser-device-preset" | "track-browser-blank",
                "工場出荷のプリセットを読み込み直す",
            ) => "標準プリセットを復元",
            ("other-76", "場所名を変更") => "名前を変更",
            ("multi-audio", "クロスフェードを作成") => "クロスフェード",
            _ => continue,
        };
        *part = alias.into();
    }
    if target == "other-19" && out.first().is_some_and(|s| s == "中のトラックにも塗る") {
        out.insert(0, "色".into());
    }
    out
}

fn target_matches(route: &str, target: &str) -> bool {
    route == target
        || match target {
            "track-browser-device-preset" => route == "other-72",
            "track-browser-blank" => route == "other-73",
            "audio" | "multi-audio" => matches!(route, "audio-states" | "other-15"),
            "midi" | "multi-midi" | "multi-mixed" => matches!(route, "midi-states" | "other-15"),
            "track" | "other-48" => route == "track-states",
            "insert" | "flow-plugin" => route == "insert-states",
            _ => false,
        }
}

fn command_of(item: &Item) -> &str {
    match &item.run {
        Run::Cmd(c, _) | Run::Key(c) | Run::Ask(c, ..) | Run::AskNumber(c, ..) => {
            if *c == "plugin.copy_to" {
                "plugin.copy"
            } else {
                c
            }
        }
        Run::Field(kind, ..) => kind.command(),
        Run::Do(super::Act::Lane(..)) => "auto.lane.show",
        Run::Do(super::Act::AraAttach(..)) => "clip.ara.attach",
        Run::Do(super::Act::Preset(..)) => "plugin.preset.choose",
        Run::Do(super::Act::InsertRecent(..)) => "plugin.recent",
        Run::Legacy(
            crate::popup::PopupKind::InstMenu(crate::instrument::menu::Menu::Preset(..)),
            _,
            action,
        ) if matches!(action.as_ref(), crate::popup::Action::Pick(to) if to.starts_with(crate::instrument::menu::LOAD)) => {
            "instrument.preset.load"
        }
        Run::Legacy(
            crate::popup::PopupKind::InstMenu(crate::instrument::menu::Menu::Params(..)),
            ..,
        ) => "auto.lane.show",
        _ => "",
    }
}

fn dynamic_command(route: &Route) -> &str {
    if !route.command.is_empty() {
        return &route.command;
    }
    match route.id.as_str() {
        "other-18:002" => "track.output.set",
        "other-20:004" | "other-69:003" => "track.preset.add",
        "other-31:000" | "other-48:033" | "other-49:027" | "other-50:005" | "other-59:010" => {
            "auto.lane.show"
        }
        "other-36:000" => "roll.drum.hide",
        "other-36:001" => "roll.drum.rename",
        "other-48:029" => "track.preset.apply",
        "other-54:000" => "send.target.set",
        "other-55:000" => "send.add",
        "other-56:001" => "plugin.recent",
        "other-59:001" => "instrument.preset.load",
        "other-67:002" => "control.unassign",
        _ => "unclassified dynamic rule",
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Missing {
    pub target: String,
    pub path: Vec<String>,
}

/// An explicit target is required. Unknown rows are errors, rather than silently hidden.
pub fn organize(target: &str, items: Vec<Item>) -> Result<Vec<Item>, Missing> {
    let mut leaves = Vec::new();
    let mut marks = BTreeMap::new();
    // Keep the recursive walk separate so no command payload is copied from the ledger.
    fn collect(
        items: Vec<Item>,
        path: &[String],
        out: &mut Vec<(Vec<String>, Item)>,
        marks: &mut BTreeMap<String, (Option<usize>, String)>,
    ) {
        for item in items {
            if matches!(item.run, Run::Gap | Run::Title | Run::Back) {
                continue;
            }
            let mut here = path.to_vec();
            if matches!(item.run, Run::Sub(_)) {
                marks.insert(item.label.clone(), (item.mark, item.key.clone()));
            }
            here.push(
                item.swatch
                    .map_or_else(|| item.label.clone(), |n| format!("色 {:02}", n + 1)),
            );
            match item.run {
                Run::Sub(ref children) if children.is_empty() => out.push((here, item)),
                Run::Sub(mut children) => {
                    if !item.enabled {
                        for child in &mut children {
                            child.enabled = false;
                            if child.reason.is_empty() {
                                child.reason = item.reason.clone();
                            }
                        }
                    }
                    collect(children, &here, out, marks);
                }
                _ => out.push((here, item)),
            }
        }
    }
    collect(items, &[], &mut leaves, &mut marks);
    #[cfg(test)]
    let original: Vec<Item> = leaves.iter().map(|(_, i)| i.clone()).collect();
    let mut direct = Vec::new();
    let mut groups: [Vec<Item>; 12] = std::array::from_fn(|_| Vec::new());
    for (path, mut item) in leaves {
        let mut candidates: Vec<_> = routes()
            .iter()
            .filter(|r| target_matches(&r.target, target))
            .collect();
        candidates.sort_by_key(|route| route.target != target);
        let command = command_of(&item);
        let mut lookup = lookup_path(target, &path);
        let empty = matches!(&item.run, Run::Sub(children) if children.is_empty());
        if let Run::Cmd("control.assign" | "control.unassign", args) = &item.run
            && let Some(control) = args["control"].as_str().and_then(|s| s.strip_prefix('k'))
        {
            *lookup.last_mut().unwrap() = format!("Macro {control}");
        }
        let route = candidates
            .iter()
            .copied()
            .find(|r| {
                same_path(&r.before, &lookup)
                    && (r.command.is_empty() || command.is_empty() || r.command == command)
            })
            .or_else(|| {
                candidates.iter().copied().find(|r| {
                    (r.dynamic || empty || matches!(item.run, Run::Do(super::Act::Preset(..))))
                        && if empty {
                            same_path(&r.before[..r.before.len() - 1], &lookup)
                        } else {
                            lookup.len() >= r.before.len()
                                && same_path(
                                    &r.before[..r.before.len() - 1],
                                    &lookup[..r.before.len() - 1],
                                )
                        }
                        && (empty
                            || if r.dynamic {
                                dynamic_command(r) == command
                            } else {
                                matches!(item.run, Run::Do(super::Act::Preset(..)))
                                    && r.command == "plugin.preset.choose"
                            })
                })
            })
            .ok_or_else(|| Missing {
                target: target.into(),
                path: path.clone(),
            })?;
        item.ledger_id = Some(route.id.clone());
        let mut destination = route.after.clone();
        if empty {
            destination.pop();
        } else if route.dynamic {
            *destination.last_mut().unwrap() = item.label.clone();
        }
        if item.swatch.is_none() {
            // Only the approved operation renames. Ledger descriptions of generated
            // candidates (e.g. a renamed Macro knob) are not their display labels.
            item.label = if item.label.ends_with("名を変更…")
                || item.label.ends_with("名を変更")
                || item.label == "名前を変更"
            {
                "名前を変更…".into()
            } else if item.label == "別のチャンネルへ写す" {
                "別のチャンネルへコピー".into()
            } else if target == "note" && destination == ["クオンタイズ（今のグリッド）"]
            {
                destination[0].clone()
            } else {
                item.label
            };
        }
        if destination.len() == 1 {
            direct.push((route.category, item));
        } else {
            insert(
                &mut groups[route.category as usize],
                &destination[1..destination.len() - 1],
                item,
                &marks,
            );
        }
    }
    let mut entries: Vec<_> = direct
        .into_iter()
        .map(|(c, i)| Entry::new(c, i).direct())
        .collect();
    for (category, items) in Category::ALL.into_iter().zip(groups) {
        entries.extend(items.into_iter().map(|i| Entry::new(category, i)));
    }
    let result = layout::build(entries).map_err(|_| Missing {
        target: target.into(),
        path: vec!["root exceeds 12 rows".into()],
    })?;
    #[cfg(test)]
    verify_live_tree(target, &original, &result);
    Ok(result)
}

/// Every real builder call in the unit suite checks the moved live payloads, not a
/// reconstruction of operations from JSON. Dynamic rules may produce several candidates.
#[cfg(test)]
fn verify_live_tree(target: &str, original: &[Item], result: &[Item]) {
    fn leaves<'a>(items: &'a [Item], out: &mut Vec<&'a Item>) {
        for item in items {
            if let Run::Sub(children) = &item.run
                && !children.is_empty()
            {
                assert!(
                    item.enabled,
                    "disabled category hides its reasons: {}",
                    item.label
                );
                leaves(children, out);
            } else {
                out.push(item);
            }
        }
    }
    assert!(result.len() <= 12, "{target}: root row budget");
    let order: Vec<_> = result
        .iter()
        .filter_map(|i| {
            if i.ledger_id.is_some() {
                return None;
            }
            Category::ALL.into_iter().position(|c| c.label() == i.label)
        })
        .collect();
    assert!(
        order.windows(2).all(|p| p[0] < p[1]),
        "{target}: category order"
    );
    let mut moved = Vec::new();
    leaves(result, &mut moved);
    assert_eq!(
        original.len(),
        moved.len(),
        "{target}: missing or duplicate leaves"
    );
    let mut remaining = original.to_vec();
    let mut fixed = std::collections::BTreeSet::new();
    for item in moved {
        let id = item
            .ledger_id
            .as_ref()
            .expect("an operation without classification");
        assert!(
            routes()
                .iter()
                .any(|r| &r.id == id && target_matches(&r.target, target)),
            "{target}: unknown ledger identity {id}"
        );
        let route = routes().iter().find(|r| &r.id == id).unwrap();
        if !route.dynamic && !matches!(item.run, Run::Do(super::Act::Preset(..))) {
            assert!(fixed.insert(id), "{target}: duplicate fixed operation {id}");
        }
        let n = remaining
            .iter()
            .position(|old| {
                old.run == item.run
                    && old.key == item.key
                    && old.enabled == item.enabled
                    && old.reason == item.reason
                    && old.on == item.on
                    && old.selects == item.selects
                    && old.swatch == item.swatch
                    && (old.label == item.label
                        || (item.label == "名前を変更…"
                            && (old.label.ends_with("名を変更")
                                || old.label.ends_with("名を変更…")
                                || old.label == "名前を変更"))
                        || (old.label == "別のチャンネルへ写す"
                            && item.label == "別のチャンネルへコピー")
                        || (target == "note"
                            && old.label == "今のグリッド"
                            && item.label == "クオンタイズ（今のグリッド）"))
            })
            .unwrap_or_else(|| panic!("{target}: changed or duplicated operation {}", item.label));
        remaining.remove(n);
    }
    assert!(remaining.is_empty(), "{target}: unreachable operations");
    fn paths(items: &[Item], path: &mut Vec<String>, ids: &mut Vec<String>) {
        for item in items {
            if let Some(id) = &item.ledger_id {
                let route = routes().iter().find(|r| &r.id == id).unwrap();
                let empty = matches!(&item.run, Run::Sub(rows) if rows.is_empty());
                let parent_count = route.after.len() - if empty { 2 } else { 1 };
                assert!(
                    same_path(&route.after[..parent_count], path.as_slice()),
                    "{id}: approved destination"
                );
                if !empty {
                    ids.push(id.clone());
                }
            } else if let Run::Sub(children) = &item.run {
                path.push(item.label.clone());
                paths(children, path, ids);
                path.pop();
            }
        }
    }
    let mut ids = Vec::new();
    paths(result, &mut Vec::new(), &mut ids);
    // The workspace suite aggregates coverage across real scene fixtures. This is
    // test-only output, and contains rule IDs, never project paths or plugin data.
    if let Some(directory) = std::env::var_os("XIRIUS_CONTEXT_COVERAGE_DIR") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let file = std::path::PathBuf::from(directory).join(format!(
            "{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            file,
            serde_json::to_vec(&serde_json::json!({
                "target": target, "ids": ids, "root_rows": result.len(),
                "missing": remaining.len(), "duplicate": 0, "unreachable": 0
            }))
            .unwrap(),
        )
        .expect("write context coverage");
    }
}

fn insert(
    items: &mut Vec<Item>,
    parents: &[String],
    leaf: Item,
    marks: &BTreeMap<String, (Option<usize>, String)>,
) {
    let Some((parent, rest)) = parents.split_first() else {
        items.push(leaf);
        return;
    };
    let parent = if parent == "別のチャンネルへ写す" {
        "別のチャンネルへコピー"
    } else {
        parent.as_str()
    };
    let index = items
        .iter()
        .position(|i| i.label == parent && matches!(i.run, Run::Sub(_)))
        .unwrap_or_else(|| {
            let mut group = Item::sub(parent, Vec::new()).enabled(true);
            if let Some((mark, key)) = marks.get(parent) {
                group.mark = *mark;
                group.key = key.clone();
            }
            items.push(group);
            items.len() - 1
        });
    if let Run::Sub(children) = &mut items[index].run {
        insert(children, rest, leaf, marks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_added_unclassified_operation_fails() {
        let mut s = crate::studio::Studio::sample();
        let item = s.ctx_as(
            "Unclassified operation",
            "marker.add",
            serde_json::json!({"at": 0}),
        );
        assert!(organize("audio", vec![item]).is_err());
    }
}
