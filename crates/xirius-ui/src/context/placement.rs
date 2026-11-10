//! The reviewed per-target routing table. This moves live Items, never recreates commands.
use super::{
    Item, Run,
    layout::{self, Category, Entry, Parent, Shape},
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

/// One operation after the ledger has looked at it.
struct Placed {
    item: Item,
    category: Category,
    destination: Vec<String>,
    /// The caller put it below a submenu, but the ledger keeps it on the root.
    lift: bool,
    gaps: usize,
    titles: Vec<Item>,
}

/// An explicit target is required. Unknown rows are errors, rather than silently hidden.
///
/// The shape is decided here, from the rows alone (R1–R4): up to 12 root rows keep the
/// caller's own order, gaps and submenus; more rows that span two or more categories become
/// category groups.
pub fn organize(target: &str, items: Vec<Item>) -> Result<Vec<Item>, Missing> {
    let caller_rows = layout::root_rows(&items);
    let caller = items.clone();
    #[cfg(test)]
    let caller_tree = items.clone();
    let mut leaves = Vec::new();
    let mut marks = BTreeMap::new();
    // Keep the recursive walk separate so no command payload is copied from the ledger.
    struct Walk {
        gaps: usize,
        titles: Vec<Item>,
    }
    fn collect(
        items: Vec<Item>,
        path: &[String],
        out: &mut Vec<(Vec<String>, Item, usize, Vec<Item>)>,
        marks: &mut BTreeMap<String, (Option<usize>, String)>,
        walk: &mut Walk,
    ) {
        for item in items {
            match item.run {
                Run::Gap => {
                    if path.is_empty() {
                        walk.gaps += 1;
                    }
                    continue;
                }
                Run::Title => {
                    if path.is_empty() {
                        walk.titles.push(item);
                    }
                    continue;
                }
                Run::Back => continue,
                _ => {}
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
                Run::Sub(ref children) if children.is_empty() => {
                    out.push((here, item, walk.gaps, std::mem::take(&mut walk.titles)));
                }
                Run::Sub(mut children) => {
                    if !item.enabled {
                        for child in &mut children {
                            child.enabled = false;
                            if child.reason.is_empty() {
                                child.reason = item.reason.clone();
                            }
                        }
                    }
                    let titles = std::mem::take(&mut walk.titles);
                    let start = out.len();
                    collect(children, &here, out, marks, walk);
                    // Nested rows stand where their submenu stood.
                    if let Some(first) = out.get_mut(start) {
                        first.3.splice(0..0, titles);
                    }
                }
                _ => out.push((here, item, walk.gaps, std::mem::take(&mut walk.titles))),
            }
        }
    }
    collect(
        items,
        &[],
        &mut leaves,
        &mut marks,
        &mut Walk {
            gaps: 0,
            titles: Vec::new(),
        },
    );
    #[cfg(test)]
    let original: Vec<Item> = leaves.iter().map(|(_, i, ..)| i.clone()).collect();
    let mut placed = Vec::new();
    for (path, mut item, gaps, titles) in leaves {
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
        let quantize = target == "note"
            && destination.len() == 1
            && destination[0].starts_with("クオンタイズ（");
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
            } else if quantize {
                destination[0].clone()
            } else {
                item.label
            };
        }
        placed.push(Placed {
            item,
            category: route.category,
            lift: quantize && path.len() > 1,
            destination,
            gaps,
            titles,
        });
    }
    let mut spans: Vec<_> = placed.iter().map(|p| p.category).collect();
    spans.sort();
    spans.dedup();
    let grouped = layout::shape(caller_rows, spans.len()) == Shape::Grouped;
    // R4: many rows in one category are laid out by `build` too, without a group.
    let result = if grouped || caller_rows > layout::ROOT_ROWS {
        let entries = placed
            .into_iter()
            .map(|p| {
                let parents = p
                    .destination
                    .get(1..p.destination.len() - 1)
                    .unwrap_or_default()
                    .iter()
                    .map(|label| {
                        let label = if label == "別のチャンネルへ写す" {
                            "別のチャンネルへコピー"
                        } else {
                            label.as_str()
                        };
                        let (mark, key) = marks.get(label).cloned().unwrap_or_default();
                        Parent {
                            label: label.into(),
                            mark,
                            key,
                        }
                    })
                    .collect();
                Entry {
                    category: p.category,
                    direct: p.destination.len() == 1,
                    item: p.item,
                    parents,
                    gaps: p.gaps,
                    titles: p.titles,
                }
            })
            .collect();
        layout::build(entries).map_err(|_| Missing {
            target: target.into(),
            path: vec!["root exceeds 12 rows".into()],
        })?
    } else {
        let mut rows = placed.into_iter().map(|p| (p.item, p.lift));
        layout::collapse_gaps(rebuild(caller, &mut rows))
    };
    #[cfg(test)]
    verify_live_tree(target, &original, &result, grouped);
    #[cfg(test)]
    verify_shape(target, &caller_tree, &result, grouped);
    Ok(result)
}

/// The caller's own tree, with each operation replaced by its classified Item (same order).
/// A row the ledger keeps on the root comes out of its submenu (only the note's quantize).
fn rebuild(items: Vec<Item>, rows: &mut impl Iterator<Item = (Item, bool)>) -> Vec<Item> {
    let mut out = Vec::new();
    for mut item in items {
        match item.run {
            Run::Back => {}
            Run::Gap | Run::Title => out.push(item),
            Run::Sub(children) if !children.is_empty() => {
                let (mut lifted, mut kept) = (Vec::new(), Vec::new());
                for child in children {
                    match child.run {
                        Run::Back => {}
                        Run::Gap | Run::Title => kept.push(child),
                        Run::Sub(ref grand) if !grand.is_empty() => {
                            let mut sub = child.clone();
                            sub.run = Run::Sub(rebuild(grand.clone(), rows));
                            kept.push(sub);
                        }
                        _ => {
                            let (row, lift) =
                                rows.next().expect("one classified row per operation");
                            let list = if lift { &mut lifted } else { &mut kept };
                            list.push(row);
                        }
                    }
                }
                out.extend(lifted);
                if kept.iter().any(|i| !matches!(i.run, Run::Gap | Run::Title)) {
                    item.run = Run::Sub(kept);
                    out.push(item);
                }
            }
            _ => out.push(rows.next().expect("one classified row per operation").0),
        }
    }
    out
}

/// Every real builder call in the unit suite checks the moved live payloads, not a
/// reconstruction of operations from JSON. Dynamic rules may produce several candidates.
/// The right-click menu shape rules (T1-T4, T10, T11, T13) on one organized tree.
#[cfg(test)]
fn verify_shape(target: &str, original: &[Item], result: &[Item], grouped: bool) {
    fn names<'a>(items: &'a [Item], out: &mut Vec<&'a str>) {
        for item in items {
            out.push(&item.label);
            if let Run::Sub(children) = &item.run {
                names(children, out);
            }
        }
    }
    let mut caller = Vec::new();
    names(original, &mut caller);
    let is_group = |i: &Item| {
        matches!(&i.run, Run::Sub(_))
            && !caller.contains(&i.label.as_str())
            && layout::Category::ALL.iter().any(|c| c.label() == i.label)
    };
    let rows = |items: &[Item]| {
        items
            .iter()
            .filter(|i| !matches!(i.run, Run::Gap | Run::Title | Run::Back))
            .count()
    };
    let groups: Vec<&Item> = result.iter().filter(|i| is_group(i)).collect();
    // T3: nothing to group when the menu fits one face (or sits in one category, R4).
    assert!(
        grouped || groups.is_empty(),
        "{target}: category groups in a flat menu: {:?}",
        groups.iter().map(|g| &g.label).collect::<Vec<_>>()
    );
    // T1: the root is never a single group.
    assert!(
        !(groups.len() == 1 && rows(result) == 1),
        "{target}: root is a single group"
    );
    // T2/T13: a group holds at least 2 rows or entries.
    for g in &groups {
        let Run::Sub(children) = &g.run else { continue };
        assert!(
            rows(children) >= 2,
            "{target}: group {} holds fewer than 2 rows",
            g.label
        );
    }
    // T4: the direct rows lead a grouped menu: at most 4, in table order.
    if grouped {
        let lead: Vec<&Item> = result
            .iter()
            .take_while(|i| !matches!(i.run, Run::Gap | Run::Title))
            .collect();
        let ranks: Vec<usize> = lead.iter().map(|i| layout::direct_rank(&i.label)).collect();
        if ranks.iter().all(|r| *r < layout::DIRECT_ORDER.len()) {
            assert!(ranks.len() <= 4, "{target}: more than 4 direct rows");
            assert!(
                ranks.windows(2).all(|w| w[0] <= w[1]),
                "{target}: direct rows out of order"
            );
        }
    }
    // T10: no panel has two submenus of the same name.
    fn twice(target: &str, items: &[Item]) {
        let mut seen: Vec<&str> = Vec::new();
        for item in items {
            if let Run::Sub(children) = &item.run {
                assert!(
                    !seen.contains(&item.label.as_str()),
                    "{target}: two submenus named {}",
                    item.label
                );
                seen.push(&item.label);
                twice(target, children);
            }
        }
    }
    twice(target, result);
}

#[cfg(test)]
fn verify_live_tree(target: &str, original: &[Item], result: &[Item], grouped: bool) {
    fn leaves<'a>(items: &'a [Item], grouped: bool, out: &mut Vec<&'a Item>) {
        for item in items {
            if matches!(item.run, Run::Gap | Run::Title) {
                continue;
            }
            if let Run::Sub(children) = &item.run
                && !children.is_empty()
            {
                assert!(
                    item.enabled || !grouped || item.ledger_id.is_some(),
                    "disabled category hides its reasons: {}",
                    item.label
                );
                leaves(children, grouped, out);
            } else {
                out.push(item);
            }
        }
    }
    let rows = result
        .iter()
        .filter(|i| !matches!(i.run, Run::Gap | Run::Title))
        .count();
    if grouped {
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
    }
    let mut moved = Vec::new();
    leaves(result, grouped, &mut moved);
    assert_eq!(
        original.len(),
        moved.len(),
        "{target}: missing or duplicate leaves"
    );
    let single_category = {
        let mut categories: Vec<_> = moved
            .iter()
            .filter_map(|i| {
                routes()
                    .iter()
                    .find(|r| Some(&r.id) == i.ledger_id.as_ref())
            })
            .map(|r| r.category)
            .collect();
        categories.sort();
        categories.dedup();
        categories.len() < 2
    };
    assert!(
        rows <= layout::ROOT_ROWS || single_category,
        "{target}: root row budget"
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
                            && item.label.starts_with("クオンタイズ（")
                            && item.label.contains(&old.label)))
            })
            .unwrap_or_else(|| panic!("{target}: changed or duplicated operation {}", item.label));
        remaining.remove(n);
    }
    assert!(remaining.is_empty(), "{target}: unreachable operations");
    fn paths(items: &[Item], path: &mut Vec<String>, ids: &mut Vec<String>, grouped: bool) {
        for item in items {
            if let Some(id) = &item.ledger_id {
                let route = routes().iter().find(|r| &r.id == id).unwrap();
                let empty = matches!(&item.run, Run::Sub(rows) if rows.is_empty());
                let parent_count = route.after.len() - if empty { 2 } else { 1 };
                // A one-row category is not made (R3), so its row stands without the category.
                assert!(
                    !grouped
                        || same_path(&route.after[..parent_count], path.as_slice())
                        || same_path(
                            &route.after[1.min(parent_count)..parent_count],
                            path.as_slice()
                        ),
                    "{id}: approved destination"
                );
                if !empty {
                    ids.push(id.clone());
                }
            } else if let Run::Sub(children) = &item.run {
                path.push(item.label.clone());
                paths(children, path, ids, grouped);
                path.pop();
            }
        }
    }
    let mut ids = Vec::new();
    paths(result, &mut Vec::new(), &mut ids, grouped);
    // The workspace suite aggregates coverage across real scene fixtures. This is
    // test-only output, and contains rule IDs, never project paths or plugin data.
    if let Some(directory) = std::env::var_os("XIRIUS_CONTEXT_COVERAGE_DIR") {
        use std::sync::atomic::AtomicUsize;
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        write_coverage(
            std::path::Path::new(&directory),
            &NEXT,
            &serde_json::to_vec(&serde_json::json!({
                "target": target, "ids": ids, "root_rows": rows,
                "missing": remaining.len(), "duplicate": 0, "unreachable": 0
            }))
            .unwrap(),
        )
        .expect("write context coverage");
    }
}

#[cfg(test)]
fn write_coverage(
    directory: &std::path::Path,
    next: &std::sync::atomic::AtomicUsize,
    bytes: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::Ordering;
    loop {
        let file = directory.join(format!(
            "{}-{}.json",
            std::process::id(),
            next.fetch_add(1, Ordering::Relaxed)
        ));
        // A process ID can be reused within one run. Never replace an earlier observation.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(file)
        {
            Ok(mut file) => return file.write_all(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reused_process_counters_preserve_every_coverage_record() {
        use std::sync::atomic::AtomicUsize;
        let dir = tempfile::tempdir().unwrap();
        let first = br#"{"ids":["first"]}"#;
        let second = br#"{"ids":["second"]}"#;
        write_coverage(dir.path(), &AtomicUsize::new(0), first).unwrap();
        write_coverage(dir.path(), &AtomicUsize::new(0), second).unwrap();
        let mut records: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|p| std::fs::read(p.unwrap().path()).unwrap())
            .collect();
        records.sort();
        assert_eq!(records, vec![first.to_vec(), second.to_vec()]);
    }

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
