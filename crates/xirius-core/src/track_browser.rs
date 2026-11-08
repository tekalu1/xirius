//! Shared track-browser index. Files retain their original preset formats; the index
//! describes the subtree they contain, independently of their display names.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Instrument,
    MultiInstrument,
    Fx,
    FxChain,
    Track,
}

impl Category {
    pub fn tab(self) -> &'static str {
        match self {
            Self::Instrument | Self::MultiInstrument => "instruments",
            Self::Fx | Self::FxChain => "effects",
            Self::Track => "tracks",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Instrument => "インストゥルメント",
            Self::MultiInstrument => "マルチインストゥルメント",
            Self::Fx => "FX",
            Self::FxChain => "FX チェーン",
            Self::Track => "トラック",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Stable reference: a device UID or a preset file, never its display name.
    pub id: String,
    pub name: String,
    pub category: Category,
    pub device: String,
    pub device_name: String,
    pub vendor: String,
    pub source: String,
    pub folder: Vec<String>,
    pub file: Option<String>,
    pub has_fx: bool,
}

impl Entry {
    pub fn matches(&self, query: &str) -> bool {
        let haystack = format!(
            "{} {} {} {} {}",
            self.name,
            self.device_name,
            self.vendor,
            self.source,
            self.folder.join(" ")
        )
        .to_lowercase();
        query
            .split_whitespace()
            .all(|word| haystack.contains(&word.to_lowercase()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub key: String,
    pub label: String,
    pub depth: usize,
    pub open: bool,
    pub branch: bool,
    pub entry: Option<Entry>,
    pub favourite: bool,
}

/// Build only populated branches. Search expands ancestors without changing the user's
/// stored expansion state. Favourite rows point at exactly the same entry IDs.
pub fn tree(
    entries: &[Entry],
    tab: &str,
    query: &str,
    closed: &BTreeSet<String>,
    favourites: &BTreeSet<String>,
) -> Vec<Row> {
    let hits: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.category.tab() == tab && e.matches(query))
        .collect();
    let mut rows = Vec::new();
    let fav: Vec<_> = hits
        .iter()
        .copied()
        .filter(|e| favourites.contains(&e.id))
        .collect();
    if !fav.is_empty() && branch(&mut rows, "favourites", "お気に入り", 0, query, closed) {
        for e in fav {
            leaf(&mut rows, e, 1, favourites);
        }
    }
    let kinds: &[Category] = match tab {
        "instruments" => &[Category::MultiInstrument, Category::Instrument],
        "effects" => &[Category::FxChain, Category::Fx],
        "tracks" => &[Category::Track],
        _ => &[],
    };
    for &kind in kinds {
        let found: Vec<_> = hits
            .iter()
            .copied()
            .filter(|e| e.category == kind)
            .collect();
        if found.is_empty() {
            continue;
        }
        let key = format!("category/{}", kind.label());
        let depth = if kind == Category::Track {
            0
        } else {
            if !branch(&mut rows, &key, kind.label(), 0, query, closed) {
                continue;
            }
            1
        };
        if matches!(kind, Category::Instrument | Category::Fx) {
            let devices: BTreeSet<_> = found.iter().map(|e| (&e.device_name, &e.device)).collect();
            for (name, uid) in devices {
                let children: Vec<_> = found
                    .iter()
                    .copied()
                    .filter(|e| &e.device == uid && e.file.is_some())
                    .collect();
                let device = entries
                    .iter()
                    .find(|e| e.file.is_none() && &e.device == uid && e.category == kind);
                let device_key = format!("device/{uid}");
                let open = !query.trim().is_empty() || !closed.contains(&device_key);
                rows.push(Row {
                    key: device_key.clone(),
                    label: name.clone(),
                    depth,
                    open,
                    branch: !children.is_empty(),
                    entry: device.cloned(),
                    favourite: device.is_some_and(|e| favourites.contains(&e.id)),
                });
                if open {
                    locations(
                        &mut rows,
                        &children,
                        &device_key,
                        depth + 1,
                        query,
                        closed,
                        favourites,
                    );
                }
            }
        } else {
            locations(&mut rows, &found, &key, depth, query, closed, favourites);
        }
    }
    rows
}

fn branch(
    rows: &mut Vec<Row>,
    key: &str,
    label: &str,
    depth: usize,
    query: &str,
    closed: &BTreeSet<String>,
) -> bool {
    let open = !query.trim().is_empty() || !closed.contains(key);
    rows.push(Row {
        key: key.into(),
        label: label.into(),
        depth,
        open,
        branch: true,
        entry: None,
        favourite: false,
    });
    open
}

fn leaf(rows: &mut Vec<Row>, e: &Entry, depth: usize, favourites: &BTreeSet<String>) {
    rows.push(Row {
        key: e.id.clone(),
        label: e.name.clone(),
        depth,
        open: false,
        branch: false,
        entry: Some(e.clone()),
        favourite: favourites.contains(&e.id),
    });
}

fn locations(
    rows: &mut Vec<Row>,
    entries: &[&Entry],
    parent: &str,
    depth: usize,
    query: &str,
    closed: &BTreeSet<String>,
    favourites: &BTreeSet<String>,
) {
    for source in ["Factory", "User"] {
        let found: Vec<_> = entries
            .iter()
            .copied()
            .filter(|e| e.source == source)
            .collect();
        let key = format!("{parent}/{source}");
        if !found.is_empty() && branch(rows, &key, source, depth, query, closed) {
            folders(rows, &found, &key, depth + 1, 0, query, closed, favourites);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn folders(
    rows: &mut Vec<Row>,
    entries: &[&Entry],
    parent: &str,
    depth: usize,
    level: usize,
    query: &str,
    closed: &BTreeSet<String>,
    favourites: &BTreeSet<String>,
) {
    let names: BTreeSet<_> = entries.iter().filter_map(|e| e.folder.get(level)).collect();
    for name in names {
        let key = format!("{parent}/{name}");
        if branch(rows, &key, name, depth, query, closed) {
            let found: Vec<_> = entries
                .iter()
                .copied()
                .filter(|e| e.folder.get(level) == Some(name))
                .collect();
            folders(
                rows,
                &found,
                &key,
                depth + 1,
                level + 1,
                query,
                closed,
                favourites,
            );
        }
    }
    let mut leaves: Vec<_> = entries
        .iter()
        .copied()
        .filter(|e| e.folder.len() == level)
        .collect();
    leaves.sort_by_key(|e| e.name.to_lowercase());
    for e in leaves {
        leaf(rows, e, depth, favourites);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preset() -> Entry {
        Entry {
            id: "preset/one".into(),
            name: "Warm piano".into(),
            category: Category::Instrument,
            device: "sampler".into(),
            device_name: "Sampler".into(),
            vendor: "Xirius".into(),
            source: "User".into(),
            folder: vec!["Keys".into()],
            file: Some("one.xpreset".into()),
            has_fx: true,
        }
    }
    #[test]
    fn and_search_keeps_parents_and_opens_only_populated_branches() {
        let e = preset();
        assert!(e.matches("PIANO Xirius user keys sampler"));
        assert!(!e.matches("piano factory"));
        let closed = BTreeSet::from(["device/sampler".into()]);
        let rows = tree(&[e], "instruments", "piano keys", &closed, &BTreeSet::new());
        assert_eq!(
            rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            [
                "インストゥルメント",
                "Sampler",
                "User",
                "Keys",
                "Warm piano"
            ]
        );
        assert!(rows[1].open);
        assert!(rows.last().unwrap().entry.as_ref().unwrap().has_fx);
    }
    #[test]
    fn favourites_are_references_and_folding_does_not_remove_entries() {
        let e = preset();
        let fav = BTreeSet::from([e.id.clone()]);
        let rows = tree(
            std::slice::from_ref(&e),
            "instruments",
            "",
            &BTreeSet::new(),
            &fav,
        );
        assert_eq!(
            rows.iter().filter(|r| r.entry.as_ref() == Some(&e)).count(),
            2
        );
        let closed = BTreeSet::from(["device/sampler".into()]);
        assert!(
            !tree(&[e], "instruments", "", &closed, &BTreeSet::new())
                .iter()
                .any(|r| r.label == "User")
        );
        assert!(tree(&[], "tracks", "", &closed, &fav).is_empty());
    }
}
