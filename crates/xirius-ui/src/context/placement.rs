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
        let before: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/context_menus/inventory.json"))
                .expect("approved inventory");
        let after: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/context_menus/mapping.json"))
                .expect("approved mapping");
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

fn target_matches(route: &str, target: &str) -> bool {
    route == target
        || match target {
            "audio" | "multi-audio" => matches!(route, "audio-states" | "other-15"),
            "midi" | "multi-midi" | "multi-mixed" => matches!(route, "midi-states" | "other-15"),
            "track" | "other-48" => route == "track-states",
            "insert" | "flow-plugin" => route == "insert-states",
            _ => false,
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
        marks: &mut BTreeMap<String, usize>,
    ) {
        for item in items {
            if matches!(item.run, Run::Gap | Run::Title | Run::Back) {
                continue;
            }
            let mut here = path.to_vec();
            if let Some(mark) = item.mark {
                marks.insert(item.label.clone(), mark);
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
    let mut direct = Vec::new();
    let mut groups: [Vec<Item>; 12] = std::array::from_fn(|_| Vec::new());
    for (path, mut item) in leaves {
        let candidates: Vec<_> = routes()
            .iter()
            .filter(|r| target_matches(&r.target, target))
            .collect();
        let command = match &item.run {
            Run::Cmd(c, _) | Run::Key(c) | Run::Ask(c, ..) | Run::AskNumber(c, ..) => *c,
            _ => "",
        };
        let command = if command == "plugin.copy_to" {
            "plugin.copy"
        } else {
            command
        };
        let mut lookup = path.clone();
        let empty = matches!(&item.run, Run::Sub(children) if children.is_empty());
        if let Run::Cmd("control.assign" | "control.unassign", args) = &item.run
            && let Some(control) = args["control"].as_str().and_then(|s| s.strip_prefix('k'))
        {
            *lookup.last_mut().unwrap() = format!("Macro {control}");
        }
        let route = candidates
            .iter()
            .copied()
            .find(|r| same_path(&r.before, &lookup))
            .or_else(|| {
                candidates.iter().copied().find(|r| {
                    (r.dynamic || matches!(item.run, Run::Do(super::Act::Preset(..))))
                        && same_path(
                            &r.before[..r.before.len() - 1],
                            if empty {
                                &path
                            } else {
                                &path[..path.len() - 1]
                            },
                        )
                        && (r.command.is_empty() || command.is_empty() || r.command == command)
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
            item.label = if item.label.ends_with("名を変更…") || item.label.ends_with("名を変更")
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
    layout::build(entries).map_err(|_| Missing {
        target: target.into(),
        path: vec!["root exceeds 12 rows".into()],
    })
}

fn insert(items: &mut Vec<Item>, parents: &[String], leaf: Item, marks: &BTreeMap<String, usize>) {
    let Some((parent, rest)) = parents.split_first() else {
        items.push(leaf);
        return;
    };
    let index = items
        .iter()
        .position(|i| i.label == *parent && matches!(i.run, Run::Sub(_)))
        .unwrap_or_else(|| {
            let mut group = Item::sub(parent, Vec::new()).enabled(true);
            group.mark = marks.get(parent).copied();
            items.push(group);
            items.len() - 1
        });
    if let Run::Sub(children) = &mut items[index].run {
        insert(children, rest, leaf, marks);
    }
}
