use serde_json::json;
use xirius_ui::context::{
    Item, Run,
    layout::{Category, Entry, Error, build},
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
    let cases: Vec<(&str, Vec<Entry>, Result<Vec<String>, Error>)> = vec![
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

/// The rows keep their order and what they carry: a disabled row keeps its reason and key.
#[test]
fn classification_moves_each_original_item_once_in_order() {
    let direct = operation();
    let tree = build(vec![
        Entry::new(Category::View, disabled()),
        Entry::new(Category::Edit, direct.clone()).direct(),
        Entry::new(Category::Audio, operation()),
    ])
    .unwrap();
    let rows: Vec<&Item> = tree.iter().filter(|i| i.run != Run::Gap).collect();
    assert_eq!(rows, [&direct, &operation(), &disabled()]);
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
        assert_eq!(route["command"], before["command"], "{id}");
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
