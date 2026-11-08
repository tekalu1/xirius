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
    assert_eq!(tree.len(), 3);
    assert_eq!(tree[0], direct);
    assert_eq!(tree[1].label, "オーディオ");
    assert_eq!(tree[2].label, "表示");
    assert!(tree[2].enabled, "a disabled reason must remain reachable");
    assert_eq!(tree[2].run, Run::Sub(vec![disabled]));
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
