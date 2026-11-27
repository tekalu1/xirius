use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};
use serde_json::json;
use xirius_ui::context::{
    Item, Run,
    layout::{Category, DIRECT_ORDER, Entry, Error, ROOT_ROWS, build, direct_rank},
};

fn operation() -> Item {
    Item::ask(
        "名前を変更…",
        "clip.rename",
        json!({"id":"c7"}),
        "name",
        "old",
    )
}

fn disabled() -> Item {
    let mut item = operation().enabled(false);
    item.reason = "フリーズ中".into();
    item.key = "F2".into();
    item
}

/// The root of a built menu: each row's label, with `>` before a group.
fn outline(tree: &[Item]) -> Vec<String> {
    tree.iter()
        .filter(|i| i.run != Run::Gap)
        .map(|i| match i.run {
            Run::Sub(_) => format!(">{}", i.label),
            _ => i.label.clone(),
        })
        .collect()
}

/// R3 and the root budget: the shape `build` gives each set of entries.
#[test]
fn build_lays_out_rows_by_the_shape_rules() {
    let rename = || "名前を変更…".to_string();
    let mut every_category: Vec<_> = Category::ALL
        .into_iter()
        .map(|c| Entry::new(c, operation()))
        .collect();
    every_category.push(Entry::new(Category::Edit, operation()).direct());
    // (row, the entries, the labels `build` gives or its error)
    type Case = (&'static str, Vec<Entry>, Result<Vec<String>, Error>);
    let cases: Vec<Case> = vec![
        (
            "one-row categories go to the root after the direct rows",
            vec![
                Entry::new(Category::View, disabled()),
                Entry::new(Category::Edit, operation()).direct(),
                Entry::new(Category::Audio, operation()),
            ],
            Ok(vec![rename(), rename(), rename()]),
        ),
        (
            "a category with two rows is a group after the single rows",
            vec![
                Entry::new(Category::Audio, operation()),
                Entry::new(Category::Audio, operation()),
                Entry::new(Category::Time, operation()),
            ],
            Ok(vec![rename(), ">オーディオ".into()]),
        ),
        (
            "rows of one category stay flat even past twelve",
            (0..13)
                .map(|_| Entry::new(Category::Edit, operation()))
                .collect(),
            Ok(vec![rename(); 13]),
        ),
        (
            "every category and a direct row are over the root budget",
            every_category,
            Err(Error::TooManyRootRows(13)),
        ),
        (
            "a gap is not an operation",
            vec![Entry::new(Category::Edit, Item::gap())],
            Err(Error::NonOperation),
        ),
    ];
    let mut failures = Vec::new();
    for (name, entries, want) in cases {
        let got = build(entries).map(|tree| outline(&tree));
        if got != want {
            failures.push(format!("{name}: got {got:?}, want {want:?}"));
        }
    }
    assert!(failures.is_empty(), "context menu shape: {failures:#?}");
}

/// One row a property gives `build`: its category, whether it is direct, which label of
/// `LABELS` it has, whether it is enabled, and the caller's gaps before it.
type Given = (usize, bool, usize, bool, usize);

/// The labels of the direct-row table, and two it does not know.
fn labels() -> Vec<&'static str> {
    let known = DIRECT_ORDER.iter().flat_map(|names| names.iter().copied());
    known.chain(["ミュート", "フリーズ"]).collect()
}

/// Row `i` of the caller's list: its place is its `ledger_id` and in its arguments, so each
/// row can be told apart; a disabled one carries a reason and a key.
fn made(i: usize, (_, _, label, enabled, _): Given) -> Item {
    let mut item = Item::ask(
        labels()[label],
        "clip.rename",
        json!({ "id": i }),
        "name",
        "old",
    )
    .enabled(enabled);
    item.ledger_id = Some(i.to_string());
    if !enabled {
        item.reason = format!("理由 {i}");
        item.key = "F2".into();
    }
    item
}

/// A built menu with its gaps left out: `label#place` for a row, `>label[…]` for a group.
fn places(tree: &[Item]) -> Vec<String> {
    tree.iter()
        .filter(|i| i.run != Run::Gap)
        .map(|i| match &i.run {
            Run::Sub(rows) => format!(">{}[{}]", i.label, places(rows).join(" ")),
            _ => format!("{}#{}", i.label, i.ledger_id.as_deref().unwrap_or("?")),
        })
        .collect()
}

/// Every row of a built menu, inside its groups too.
fn leaves(tree: &[Item]) -> Vec<&Item> {
    tree.iter()
        .flat_map(|i| match &i.run {
            Run::Gap => Vec::new(),
            Run::Sub(rows) => leaves(rows),
            _ => vec![i],
        })
        .collect()
}

/// A gap that leads, trails or follows another, at the root or in a group.
fn stray_gap(tree: &[Item]) -> Option<String> {
    let gap = |i: &Item| i.run == Run::Gap;
    if tree.first().is_some_and(gap) || tree.last().is_some_and(gap) {
        return Some(format!("a gap at an end of {:?}", places(tree)));
    }
    if tree.windows(2).any(|w| gap(&w[0]) && gap(&w[1])) {
        return Some(format!("two gaps together in {:?}", places(tree)));
    }
    tree.iter().find_map(|i| match &i.run {
        Run::Sub(rows) => stray_gap(rows),
        _ => None,
    })
}

proptest! {
    #![proptest_config(Config {
        cases: 512,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(54),
        ..Config::default()
    })]

    /// Any rows in any categories: each row comes back once, as the caller made it (its
    /// reason, key and state), where the shape rules put it, with no stray gap; or, past the
    /// root's budget, the error counts the rows the root would have had.
    ///
    /// The rules, over the rows' places: the direct rows first, in the table's order (the
    /// caller's among equals); then the rows of each category with one row; then a group per
    /// category with more, in the categories' order, each keeping the caller's order. Rows that
    /// all fall in one category stay flat, the direct ones first.
    #[test]
    fn build_moves_each_row_once_to_the_place_the_rules_give(
        given in prop::collection::vec(
            (0..Category::ALL.len(), any::<bool>(), 0..labels().len(), any::<bool>(), 0..3usize),
            0..24,
        ),
    ) {
        let entries = given.iter().enumerate().map(|(i, &g)| {
            let mut e = Entry::new(Category::ALL[g.0], made(i, g));
            e.gaps = g.4;
            if g.1 { e.direct() } else { e }
        });
        let got = build(entries.collect());
        let given = &given;

        let place = |i: usize| format!("{}#{i}", labels()[given[i].2]);
        let mut direct: Vec<usize> = (0..given.len()).filter(|&i| given[i].1).collect();
        direct.sort_by_key(|&i| direct_rank(labels()[given[i].2]));
        let rest = |c: usize| (0..given.len()).filter(move |&i| !given[i].1 && given[i].0 == c);
        let categories = 0..Category::ALL.len();
        let spans = categories.clone().filter(|&c| given.iter().any(|g| g.0 == c)).count();
        let mut want: Vec<String> = direct.into_iter().map(place).collect();
        if spans < 2 {
            want.extend((0..given.len()).filter(|&i| !given[i].1).map(place));
        } else {
            let single = categories.clone().filter(|&c| rest(c).count() == 1);
            want.extend(single.flat_map(rest).map(place));
            for c in categories.filter(|&c| rest(c).count() > 1) {
                let rows: Vec<String> = rest(c).map(place).collect();
                want.push(format!(">{}[{}]", Category::ALL[c].label(), rows.join(" ")));
            }
        }

        match got {
            Ok(tree) => {
                prop_assert_eq!(places(&tree), want);
                let root = tree.iter().filter(|i| i.run != Run::Gap).count();
                prop_assert!(spans < 2 || root <= ROOT_ROWS, "a root of {}", root);
                prop_assert_eq!(stray_gap(&tree), None);
                let mut rows = leaves(&tree);
                rows.sort_by_key(|i| i.ledger_id.as_deref().and_then(|p| p.parse::<usize>().ok()));
                let originals: Vec<Item> =
                    given.iter().enumerate().map(|(i, &g)| made(i, g)).collect();
                prop_assert!(rows.iter().copied().eq(originals.iter()), "a row changed on its way");
            }
            Err(e) => prop_assert!(
                spans >= 2 && want.len() > ROOT_ROWS && e == Error::TooManyRootRows(want.len()),
                "{:?} for a root of {}: {:?}", e, want.len(), want
            ),
        }
    }
}

#[test]
fn swapping_pairs_sit_in_the_same_place() {
    let mapping: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../context_menus/mapping.json")).unwrap();
    let pairs = [
        ("無効にする", "有効にする"),
        ("フリーズ", "フリーズを解除"),
        ("ミュート", "ミュートを解除"),
        ("ソロ", "ソロを解除"),
    ];
    for (a, b) in pairs {
        for key in mapping.iter().map(|r| r["key"].as_str().unwrap()) {
            let tobe = |label: &str| {
                mapping
                    .iter()
                    .find(|r| r["key"] == key && r["label"] == label)
                    .map(|r| {
                        let t = r["tobe"].as_array().unwrap();
                        t[..t.len() - 1].to_vec()
                    })
            };
            if let (Some(x), Some(y)) = (tobe(a), tobe(b)) {
                assert_eq!(x, y, "{key}: {a} / {b} sit in different places");
            }
        }
    }
}

#[test]
fn approved_inventory_and_mapping_keep_all_924_identities_and_payloads() {
    use std::collections::BTreeMap;
    let inventory: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../context_menus/inventory.json")).unwrap();
    let mapping: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../context_menus/mapping.json")).unwrap();
    assert_eq!(mapping.len(), inventory.len());
    let by_id: BTreeMap<_, _> = inventory
        .iter()
        .map(|v| (v["id"].as_str().unwrap(), v))
        .collect();
    assert_eq!(by_id.len(), inventory.len());
    let mut visited = std::collections::BTreeSet::new();
    for route in &mapping {
        let id = route["id"].as_str().unwrap();
        assert!(visited.insert(id), "duplicate {id}");
        let before = by_id[id];
        // The mapping fills in the command where the inventory left it out, and names every one
        // a row runs by what it stands on; it keeps the one the inventory names.
        let named = before["command"].as_str().unwrap();
        let runs = match &route["command"] {
            serde_json::Value::Array(all) => all.iter().any(|c| c == named),
            c => c == named,
        };
        assert!(named.is_empty() || runs, "{id}");
        assert_eq!(route["args"], before["args"], "{id}");
        assert!(
            Category::ALL
                .iter()
                .any(|c| Some(c.label()) == route["category"].as_str()),
            "{id}"
        );
        assert!(!route["tobe"].as_array().unwrap().is_empty(), "{id}");
    }
}
