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
    #[serde(default)]
    pub fx_names: Vec<String>,
}

impl Entry {
    /// Old browser keys remain readable; new edits use the indexed ID.
    pub fn legacy_keys(&self) -> Vec<String> {
        if self.id.starts_with("device:") {
            vec![self.name.clone()]
        } else if !self.device.is_empty() {
            vec![format!("preset:{}/{}", self.device, self.name)]
        } else {
            Vec::new()
        }
    }
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

/// Resolve only unambiguous old references; retain unknown keys for missing plugins.
pub fn favourite_id(key: &str, entries: &[Entry]) -> String {
    if entries.iter().any(|e| e.id == key) {
        return key.into();
    }
    let mut matches = entries
        .iter()
        .filter(|e| e.legacy_keys().iter().any(|old| old == key));
    match (matches.next(), matches.next()) {
        (Some(entry), None) => entry.id.clone(),
        _ => key.into(),
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
/// stored expansion state. Favourite rows point at exactly the same entry IDs. A favourite
/// device starts folded: it is open only while its key is in `opened`.
pub fn tree(
    entries: &[Entry],
    tab: &str,
    query: &str,
    closed: &BTreeSet<String>,
    opened: &BTreeSet<String>,
    favourites: &BTreeSet<String>,
) -> Vec<Row> {
    let canonical: BTreeSet<_> = favourites
        .iter()
        .map(|key| favourite_id(key, entries))
        .collect();
    let favourites = &canonical;
    let hits: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.category.tab() == tab && e.matches(query))
        .collect();
    let mut rows = Vec::new();
    // A favourite device shows for a search its presets answer too, like its place below.
    let fav: Vec<_> = entries
        .iter()
        .filter(|e| e.category.tab() == tab && favourites.contains(&e.id))
        .filter(|e| e.matches(query) || (e.file.is_none() && !presets_of(&hits, e).is_empty()))
        .collect();
    if !fav.is_empty() && branch(&mut rows, "favourites", "お気に入り", 0, query, closed) {
        for e in fav {
            if e.file.is_none() {
                // A favourite device unfolds like the one in the list below, with a fold of
                // its own, and starts folded.
                let children = presets_of(&hits, e);
                device_row(
                    &mut rows,
                    &e.device_name,
                    Some(e),
                    &format!("favourites/device/{}", e.device),
                    1,
                    false,
                    &children,
                    (query, closed, opened, favourites),
                );
            } else {
                leaf(
                    &mut rows,
                    e,
                    &format!("favourites/preset/{}", e.id),
                    1,
                    favourites,
                );
            }
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
                device_row(
                    &mut rows,
                    name,
                    device,
                    &format!("device/{uid}"),
                    depth,
                    true,
                    &children,
                    (query, closed, opened, favourites),
                );
            }
        } else {
            locations(&mut rows, &found, &key, depth, query, closed, favourites);
        }
    }
    rows
}

/// The preset files of a device entry among the hits.
fn presets_of<'a>(hits: &[&'a Entry], device: &Entry) -> Vec<&'a Entry> {
    hits.iter()
        .copied()
        .filter(|e| e.device == device.device && e.category == device.category && e.file.is_some())
        .collect()
}

/// A device row and, while it is open, its presets by place and folder. The fold is kept
/// under `key`, so the same device in two places folds on its own in each. A row that does
/// not start open (`open_at_first` false) is open only while `opened` has its key.
#[allow(clippy::too_many_arguments)]
fn device_row(
    rows: &mut Vec<Row>,
    name: &str,
    device: Option<&Entry>,
    key: &str,
    depth: usize,
    open_at_first: bool,
    children: &[&Entry],
    (query, closed, opened, favourites): (
        &str,
        &BTreeSet<String>,
        &BTreeSet<String>,
        &BTreeSet<String>,
    ),
) {
    let folded = if open_at_first {
        closed.contains(key)
    } else {
        !opened.contains(key)
    };
    let open = !query.trim().is_empty() || !folded;
    rows.push(Row {
        key: key.into(),
        label: name.into(),
        depth,
        open,
        branch: !children.is_empty(),
        entry: device.cloned(),
        favourite: device.is_some_and(|e| favourites.contains(&e.id)),
    });
    if open {
        locations(rows, children, key, depth + 1, query, closed, favourites);
    }
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

fn leaf(rows: &mut Vec<Row>, e: &Entry, key: &str, depth: usize, favourites: &BTreeSet<String>) {
    rows.push(Row {
        key: key.into(),
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
        leaf(rows, e, &e.id, depth, favourites);
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
            fx_names: vec!["EQ".into()],
        }
    }
    #[test]
    fn legacy_keys_resolve_only_to_one_index_entry_and_survive_filtering() {
        let e = preset();
        let old = "preset:sampler/Warm piano";
        assert_eq!(favourite_id(old, std::slice::from_ref(&e)), e.id);
        let favourites = BTreeSet::from([old.into()]);
        for query in ["", "warm sampler", "USER keys"] {
            assert!(
                tree(
                    std::slice::from_ref(&e),
                    "instruments",
                    query,
                    &BTreeSet::new(),
                    &BTreeSet::new(),
                    &favourites
                )
                .iter()
                .any(|row| row.entry.as_ref() == Some(&e) && row.favourite)
            );
        }
        assert!(
            tree(
                std::slice::from_ref(&e),
                "effects",
                "",
                &BTreeSet::new(),
                &BTreeSet::new(),
                &favourites
            )
            .is_empty()
        );
        let mut duplicate = e.clone();
        duplicate.id = "preset/two".into();
        assert_eq!(favourite_id(old, &[e, duplicate]), old);
        assert_eq!(favourite_id("Missing device", &[]), "Missing device");
        assert_eq!(favourites, BTreeSet::from([old.into()]));
    }
    #[test]
    fn and_search_keeps_parents_and_opens_only_populated_branches() {
        let e = preset();
        assert!(e.matches("PIANO Xirius user keys sampler"));
        assert!(!e.matches("piano factory"));
        let closed = BTreeSet::from(["device/sampler".into()]);
        let rows = tree(
            &[e],
            "instruments",
            "piano keys",
            &closed,
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
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
            &BTreeSet::new(),
            &fav,
        );
        assert_eq!(
            rows.iter().filter(|r| r.entry.as_ref() == Some(&e)).count(),
            2
        );
        let closed = BTreeSet::from(["device/sampler".into()]);
        assert!(
            !tree(
                &[e],
                "instruments",
                "",
                &closed,
                &BTreeSet::new(),
                &BTreeSet::new()
            )
            .iter()
            .any(|r| r.label == "User")
        );
        assert!(tree(&[], "tracks", "", &closed, &BTreeSet::new(), &fav).is_empty());
    }
    fn device() -> Entry {
        Entry {
            id: "device/sampler".into(),
            name: "Sampler".into(),
            file: None,
            source: String::new(),
            folder: Vec::new(),
            has_fx: false,
            fx_names: Vec::new(),
            ..preset()
        }
    }
    fn labels(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.label.as_str()).collect()
    }
    #[test]
    fn a_favourite_device_has_its_presets_and_starts_folded() {
        let entries = [device(), preset()];
        let fav = BTreeSet::from([device().id]);
        let none = BTreeSet::new();
        let rows = tree(&entries, "instruments", "", &none, &none, &fav);
        let at = rows.iter().find(|r| r.key == "favourites/device/sampler");
        let at = at.expect("the favourite device row");
        assert!(at.branch && !at.open && at.favourite && at.depth == 1);
        assert_eq!(
            labels(&rows),
            [
                "お気に入り",
                "Sampler",
                "インストゥルメント",
                "Sampler",
                "User",
                "Keys",
                "Warm piano"
            ]
        );
    }
    #[test]
    fn a_favourite_device_folds_on_its_own() {
        let entries = [device(), preset()];
        let fav = BTreeSet::from([device().id]);
        let none = BTreeSet::new();
        let opened = BTreeSet::from(["favourites/device/sampler".into()]);
        let rows = tree(&entries, "instruments", "", &none, &opened, &fav);
        assert_eq!(
            labels(&rows),
            [
                "お気に入り",
                "Sampler",
                "User",
                "Keys",
                "Warm piano",
                "インストゥルメント",
                "Sampler",
                "User",
                "Keys",
                "Warm piano"
            ]
        );
        // Folding the list's device leaves the favourite's open, and the other way round.
        let closed = BTreeSet::from(["device/sampler".into()]);
        let rows = tree(&entries, "instruments", "", &closed, &opened, &fav);
        assert_eq!(
            labels(&rows).iter().filter(|l| **l == "Warm piano").count(),
            1
        );
        assert!(rows[1].open && !rows[6].open);
        let rows = tree(&entries, "instruments", "", &none, &none, &fav);
        assert!(!rows[1].open && rows[3].open);
    }
    #[test]
    fn a_search_opens_the_favourite_device_that_has_a_hit() {
        let entries = [device(), preset()];
        let fav = BTreeSet::from([device().id]);
        let none = BTreeSet::new();
        let rows = tree(&entries, "instruments", "warm", &none, &none, &fav);
        let at = rows.iter().find(|r| r.key == "favourites/device/sampler");
        assert!(at.is_some_and(|r| r.open));
        assert_eq!(rows.iter().filter(|r| r.label == "Warm piano").count(), 2);
    }
    #[test]
    fn a_search_narrows_the_favourites_like_the_list() {
        let mut cold = preset();
        cold.id = "preset/two".into();
        cold.name = "Cold pad".into();
        cold.file = Some("two.xpreset".into());
        let entries = [device(), preset(), cold];
        let fav = BTreeSet::from([device().id, preset().id, "preset/two".into()]);
        let none = BTreeSet::new();
        // Nothing a favourite answers: no favourites section at all.
        let rows = tree(&entries, "instruments", "zzz", &none, &none, &fav);
        assert!(rows.is_empty());
        // A hit: the section stays, holding only the favourites that answer.
        let rows = tree(&entries, "instruments", "warm", &none, &none, &fav);
        assert!(rows.iter().any(|r| r.key == "favourites"));
        assert!(rows.iter().any(|r| r.key == "favourites/preset/preset/one"));
        assert!(rows.iter().any(|r| r.key == "favourites/device/sampler"));
        assert!(!rows.iter().any(|r| r.key == "favourites/preset/preset/two"));
        // No search: the section is there with its favourites.
        let rows = tree(&entries, "instruments", "", &none, &none, &fav);
        assert!(rows.iter().any(|r| r.key == "favourites"));
    }
}
