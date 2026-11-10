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

#[test]
fn classification_is_ordered_and_moves_each_original_item_once() {
    let direct = operation();
    let mut disabled = operation().enabled(false);
    disabled.reason = "フリーズ中".into();
    disabled.key = "F2".into();
    let tree = build(vec![
        Entry::new(Category::View, disabled.clone()),
        Entry::new(Category::Edit, direct.clone()).direct(),
        Entry::new(Category::Audio, operation()),
    ])
    .unwrap();
    // R3: a category holding one row makes no group; its row goes to the root, after the
    // direct rows, in category order.
    let rows: Vec<&Item> = tree.iter().filter(|i| i.run != Run::Gap).collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(*rows[0], direct);
    assert_eq!(*rows[1], operation());
    assert_eq!(
        *rows[2], disabled,
        "a disabled reason must remain reachable"
    );
    assert!(tree.iter().all(|i| !matches!(i.run, Run::Sub(_))));
}

#[test]
fn a_category_with_two_rows_is_a_group_and_one_row_is_not() {
    let tree = build(vec![
        Entry::new(Category::Audio, operation()),
        Entry::new(Category::Audio, operation()),
        Entry::new(Category::Time, operation()),
    ])
    .unwrap();
    let subs: Vec<&Item> = tree
        .iter()
        .filter(|i| matches!(i.run, Run::Sub(_)))
        .collect();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].label, "オーディオ");
    assert!(tree.iter().any(|i| i.label == "名前を変更…"));
}

#[test]
fn rows_of_one_category_stay_flat_even_past_twelve() {
    let entries: Vec<_> = (0..13)
        .map(|_| Entry::new(Category::Edit, operation()))
        .collect();
    let tree = build(entries).unwrap();
    assert!(tree.iter().all(|i| !matches!(i.run, Run::Sub(_))));
    assert_eq!(tree.iter().filter(|i| i.run != Run::Gap).count(), 13);
}

#[test]
fn swapping_pairs_sit_in_the_same_place() {
    let mapping: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("context_menus/mapping.json")).unwrap();
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
fn root_budget_counts_categories_and_direct_entries() {
    let mut entries: Vec<_> = Category::ALL
        .into_iter()
        .map(|c| Entry::new(c, operation()))
        .collect();
    entries.push(Entry::new(Category::Edit, operation()).direct());
    assert_eq!(build(entries), Err(Error::TooManyRootRows(13)));
    assert_eq!(
        build(vec![Entry::new(Category::Edit, Item::gap())]),
        Err(Error::NonOperation)
    );
}

#[test]
fn approved_inventory_and_mapping_keep_all_927_identities_and_payloads() {
    use std::collections::BTreeMap;
    let inventory: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("context_menus/inventory.json")).unwrap();
    let mapping: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("context_menus/mapping.json")).unwrap();
    assert_eq!(inventory.len(), 927);
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
