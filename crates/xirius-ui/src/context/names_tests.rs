//! The words on the menu rows of every scene (CTX-008, ARC-027): short names and no sentence that
//! explains. The product's own terms are held by `xirius-design-lint` (wording).

use super::tests::{BAR, labels, studio};
use crate::arrange::probe::clip_mid;

/// The sources that put words on menu rows.
pub(super) const SOURCES: &[(&str, &str)] = &[
    ("auto.rs", include_str!("auto.rs")),
    ("clip.rs", include_str!("clip.rs")),
    ("marker.rs", include_str!("marker.rs")),
    ("mixer.rs", include_str!("mixer.rs")),
    ("roll.rs", include_str!("roll.rs")),
    ("ruler.rs", include_str!("ruler.rs")),
    ("take.rs", include_str!("take.rs")),
    ("tempo.rs", include_str!("tempo.rs")),
    ("timeline.rs", include_str!("timeline.rs")),
    ("track.rs", include_str!("track.rs")),
    ("mixed.rs", include_str!("mixed.rs")),
    ("../roll/menu.rs", include_str!("../roll/menu.rs")),
];

/// The Japanese string literals of a source, outside comments and test modules.
pub(super) fn literals(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mut rest = line;
        while let Some(a) = rest.find('"') {
            let tail = &rest[a + 1..];
            let Some(b) = tail.find('"') else { break };
            let s = &tail[..b];
            if s.chars().any(|c| ('\u{3040}'..='\u{9fff}').contains(&c)) {
                out.push((i + 1, s.to_string()));
            }
            rest = &tail[b + 1..];
        }
    }
    out
}

#[test]
fn no_row_explains_in_a_sentence() {
    for (file, src) in SOURCES {
        for (line, s) in literals(src) {
            for c in ['。', '（', '）', '(', ')'] {
                assert!(!s.contains(c), "{file}:{line}: 「{s}」に補足「{c}」");
            }
        }
    }
}

#[test]
fn the_rename_rows_say_what_the_registry_says() {
    let (mut s, g) = studio();
    let rename = s.label_of("clip.rename");
    let x = clip_mid(&s, &g, "Keys", 0);
    assert!(s.scene_context(&g, x.0, x.1));
    assert!(labels(&s).contains(&rename), "{:?}", labels(&s));

    let marker = s.label_of("marker.rename");
    s.song
        .run("marker.add", serde_json::json!({"at": 4 * BAR}))
        .unwrap();
    let id = s.project().markers[0].id;
    let r = s.marker_rect(&g, id).unwrap();
    assert!(s.scene_context(&g, r.x + 4.0, r.y + 4.0));
    assert!(labels(&s).contains(&marker), "{:?}", labels(&s));
}

#[test]
fn the_registry_labels_are_short_and_have_no_sentence() {
    let (s, _) = studio();
    for d in s.song.registry().all() {
        let name = d.name;
        let Some(label) = d.label() else {
            continue;
        };
        assert!(label.chars().count() <= 20, "{name}: 「{label}」");
        assert!(!label.contains('。'), "{name}: 「{label}」");
    }
}
