//! What a command changed (or would change, for `--dry-run`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::id::Node;
use crate::model::{ClipBody, Note, Project, Track};

/// Items listed in detail; the counts always cover everything.
const MAX_ITEMS: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Counts {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Change {
    /// `track`, `clip`, `note`, `plugin`, `send`, `folder`, `tempo`, ...
    pub kind: String,
    pub id: String,
    pub change: ChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct ChangeSet {
    pub counts: BTreeMap<String, Counts>,
    pub items: Vec<Change>,
    pub truncated: bool,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    pub fn count(&self, kind: &str) -> usize {
        self.counts
            .get(kind)
            .map_or(0, |c| c.added + c.removed + c.changed)
    }

    pub fn total(&self) -> usize {
        self.counts
            .values()
            .map(|c| c.added + c.removed + c.changed)
            .sum()
    }

    fn push(&mut self, kind: &str, id: String, change: ChangeKind, detail: Option<String>) {
        let c = self.counts.entry(kind.to_string()).or_default();
        match change {
            ChangeKind::Added => c.added += 1,
            ChangeKind::Removed => c.removed += 1,
            ChangeKind::Changed => c.changed += 1,
        }
        if self.items.len() < MAX_ITEMS {
            self.items.push(Change {
                kind: kind.to_string(),
                id,
                change,
                detail,
            });
        } else {
            self.truncated = true;
        }
    }

    /// `ノート 12 個・クリップ 1 個` style text.
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "変更はありません".to_string();
        }
        let names = [
            ("track", "トラック"),
            ("folder", "フォルダー"),
            ("clip", "クリップ"),
            ("note", "ノート"),
            ("plugin", "プラグイン"),
            ("send", "センド"),
            ("tempo", "テンポポイント"),
            ("timesig", "拍子"),
            ("section", "セクション"),
            ("order", "トラックの順序"),
            ("project", "曲の設定"),
        ];
        let mut parts = Vec::new();
        for (k, label) in names {
            let n = self.count(k);
            if n > 0 {
                parts.push(format!("{label} {n} 件"));
            }
        }
        parts.join("・")
    }
}

fn track_fields(a: &Track, b: &Track) -> Vec<&'static str> {
    let mut f = Vec::new();
    macro_rules! cmp {
        ($($field:ident),*) => { $( if a.$field != b.$field { f.push(stringify!($field)); } )* };
    }
    cmp!(
        name,
        color,
        input,
        volume_db,
        pan,
        mute,
        solo,
        armed,
        monitor,
        input_gain_db,
        output,
        icon,
        automation,
        auto_mode,
        transpose,
        velocity
    );
    f
}

pub fn diff(a: &Project, b: &Project) -> ChangeSet {
    let mut cs = ChangeSet::default();
    if a.name != b.name
        || a.ppq != b.ppq
        || a.settings != b.settings
        || a.media != b.media
        || a.loop_range != b.loop_range
    {
        cs.push(
            "project",
            "project".into(),
            ChangeKind::Changed,
            Some("name".into()),
        );
    }

    for (id, ta) in &a.tracks {
        match b.tracks.get(id) {
            None => cs.push(
                "track",
                id.to_string(),
                ChangeKind::Removed,
                Some(ta.name.clone()),
            ),
            Some(tb) if Arc::ptr_eq(ta, tb) => {}
            Some(tb) => diff_track(&mut cs, ta, tb),
        }
    }
    for (id, tb) in &b.tracks {
        if !a.tracks.contains_key(id) {
            cs.push(
                "track",
                id.to_string(),
                ChangeKind::Added,
                Some(tb.name.clone()),
            );
        }
    }

    for (id, fa) in &a.folders {
        match b.folders.get(id) {
            None => cs.push(
                "folder",
                id.to_string(),
                ChangeKind::Removed,
                Some(fa.name.clone()),
            ),
            Some(fb) if fa == fb => {}
            Some(fb) => {
                let mut f = Vec::new();
                if fa.name != fb.name {
                    f.push("name");
                }
                if fa.color != fb.color {
                    f.push("color");
                }
                if fa.bus != fb.bus {
                    f.push("bus");
                }
                // children order is reported as "order" below
                cs.push(
                    "folder",
                    id.to_string(),
                    ChangeKind::Changed,
                    Some(f.join(",")).filter(|s| !s.is_empty()),
                );
            }
        }
    }
    for (id, fb) in &b.folders {
        if !a.folders.contains_key(id) {
            cs.push(
                "folder",
                id.to_string(),
                ChangeKind::Added,
                Some(fb.name.clone()),
            );
        }
    }
    if column_order(a, b) {
        cs.push("order", "track-column".into(), ChangeKind::Changed, None);
    }

    diff_list(&mut cs, "tempo", &a.tempo, &b.tempo, |t| t.id.to_string());
    diff_list(&mut cs, "timesig", &a.timesig, &b.timesig, |t| {
        t.id.to_string()
    });
    diff_list(&mut cs, "section", &a.sections, &b.sections, |t| {
        t.id.to_string()
    });
    diff_clip_fx(&mut cs, a, b);
    cs
}

/// Fades, envelopes and crossfades live beside the clips: a change in them is
/// a change of the clip, reported once.
fn diff_clip_fx(cs: &mut ChangeSet, a: &Project, b: &Project) {
    if a.clip_fx == b.clip_fx {
        return;
    }
    let ids: std::collections::BTreeSet<_> = a.clip_fx.keys().chain(b.clip_fx.keys()).collect();
    for id in ids {
        if a.clip_fx.get(id) == b.clip_fx.get(id) || b.clip(*id).is_err() {
            continue;
        }
        let key = id.to_string();
        if cs.items.iter().any(|c| c.kind == "clip" && c.id == key) {
            continue;
        }
        cs.push("clip", key, ChangeKind::Changed, Some("fx".into()));
    }
}

/// Did the nodes that exist on both sides change parent or order?
fn column_order(a: &Project, b: &Project) -> bool {
    let common = |n: &Node| match n {
        Node::Track(t) => a.tracks.contains_key(t) && b.tracks.contains_key(t),
        Node::Folder(f) => a.folders.contains_key(f) && b.folders.contains_key(f),
    };
    let lists = |p: &Project| -> BTreeMap<String, Vec<String>> {
        let mut m = BTreeMap::new();
        m.insert(
            "root".to_string(),
            p.root
                .iter()
                .filter(|n| common(n))
                .map(|n| n.to_string())
                .collect(),
        );
        for (id, f) in &p.folders {
            if a.folders.contains_key(id) && b.folders.contains_key(id) {
                m.insert(
                    id.to_string(),
                    f.children
                        .iter()
                        .filter(|n| common(n))
                        .map(|n| n.to_string())
                        .collect(),
                );
            }
        }
        m
    };
    lists(a) != lists(b)
}

fn diff_list<T: PartialEq>(
    cs: &mut ChangeSet,
    kind: &str,
    a: &[T],
    b: &[T],
    id: impl Fn(&T) -> String,
) {
    let ma: HashMap<String, &T> = a.iter().map(|x| (id(x), x)).collect();
    let mb: HashMap<String, &T> = b.iter().map(|x| (id(x), x)).collect();
    for x in a {
        match mb.get(&id(x)) {
            None => cs.push(kind, id(x), ChangeKind::Removed, None),
            Some(y) if **y != *x => cs.push(kind, id(x), ChangeKind::Changed, None),
            _ => {}
        }
    }
    for y in b {
        if !ma.contains_key(&id(y)) {
            cs.push(kind, id(y), ChangeKind::Added, None);
        }
    }
}

fn diff_track(cs: &mut ChangeSet, a: &Track, b: &Track) {
    let f = track_fields(a, b);
    if !f.is_empty() {
        cs.push(
            "track",
            a.id.to_string(),
            ChangeKind::Changed,
            Some(f.join(",")),
        );
    }
    diff_list(cs, "plugin", &a.inserts, &b.inserts, |p| p.id.to_string());
    diff_list(cs, "send", &a.sends, &b.sends, |s| s.id.to_string());
    if a.inserts
        .iter()
        .map(|p| p.id)
        .ne(b.inserts.iter().map(|p| p.id))
        && a.inserts.len() == b.inserts.len()
    {
        cs.push(
            "plugin",
            format!("{}:order", a.id),
            ChangeKind::Changed,
            Some("order".into()),
        );
    }

    let ca: HashMap<_, _> = a.clips.iter().map(|c| (c.id, c)).collect();
    let cb: HashMap<_, _> = b.clips.iter().map(|c| (c.id, c)).collect();
    for c in &a.clips {
        match cb.get(&c.id) {
            None => cs.push(
                "clip",
                c.id.to_string(),
                ChangeKind::Removed,
                Some(c.name.clone()),
            ),
            Some(d) => {
                let mut f = Vec::new();
                if c.name != d.name {
                    f.push("name");
                }
                if c.color != d.color {
                    f.push("color");
                }
                if c.start != d.start {
                    f.push("start");
                }
                if c.length != d.length {
                    f.push("length");
                }
                if c.muted != d.muted {
                    f.push("muted");
                }
                if c.controllers != d.controllers {
                    f.push("controllers");
                }
                match (&c.body, &d.body) {
                    (
                        ClipBody::Midi {
                            notes: na,
                            transpose: ta,
                        },
                        ClipBody::Midi {
                            notes: nb,
                            transpose: tb,
                        },
                    ) => {
                        if ta != tb {
                            f.push("transpose");
                        }
                        if !Arc::ptr_eq(na, nb) {
                            diff_notes(cs, na, nb);
                        }
                    }
                    (x, y) if x != y => f.push("audio"),
                    _ => {}
                }
                if !f.is_empty() {
                    cs.push(
                        "clip",
                        c.id.to_string(),
                        ChangeKind::Changed,
                        Some(f.join(",")),
                    );
                }
            }
        }
    }
    for c in &b.clips {
        if !ca.contains_key(&c.id) {
            cs.push(
                "clip",
                c.id.to_string(),
                ChangeKind::Added,
                Some(c.name.clone()),
            );
        }
    }
}

fn diff_notes(cs: &mut ChangeSet, a: &[Note], b: &[Note]) {
    let ma: HashMap<_, &Note> = a.iter().map(|n| (n.id, n)).collect();
    let mut seen = 0usize;
    for n in b {
        match ma.get(&n.id) {
            None => cs.push("note", n.id.to_string(), ChangeKind::Added, None),
            Some(o) => {
                seen += 1;
                if *o != n {
                    let mut f = Vec::new();
                    if o.start != n.start {
                        f.push("start");
                    }
                    if o.length != n.length {
                        f.push("length");
                    }
                    if o.pitch != n.pitch {
                        f.push("pitch");
                    }
                    if o.velocity != n.velocity {
                        f.push("velocity");
                    }
                    if o.muted != n.muted {
                        f.push("muted");
                    }
                    if f.is_empty() {
                        f.push("ai");
                    }
                    cs.push(
                        "note",
                        n.id.to_string(),
                        ChangeKind::Changed,
                        Some(f.join(",")),
                    );
                }
            }
        }
    }
    if seen < a.len() {
        let mb: std::collections::HashSet<_> = b.iter().map(|n| n.id).collect();
        for n in a {
            if !mb.contains(&n.id) {
                cs.push("note", n.id.to_string(), ChangeKind::Removed, None);
            }
        }
    }
}
