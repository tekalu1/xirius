//! The words on the rows of the browser's and the export panel's menus (CTX-008, ARC-027): short
//! names, no sentence, and the same word as the command table where a row is one of its commands.
//! The product's own terms are held by `xirius-design-lint` (wording).

use super::tests::studio;

const SOURCES: &[(&str, &str)] = &[
    ("browser.rs", include_str!("browser.rs")),
    ("export.rs", include_str!("export.rs")),
];

/// The Japanese string literals of a source, outside comments.
fn literals(src: &str) -> Vec<(usize, String)> {
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
fn no_row_of_the_browser_or_the_export_panel_explains_in_a_sentence() {
    for (file, src) in SOURCES {
        for (line, s) in literals(src) {
            for c in ['。', '（', '）', '(', ')'] {
                assert!(!s.contains(c), "{file}:{line}: 「{s}」に補足「{c}」");
            }
        }
    }
}

#[test]
fn the_words_the_matrix_and_the_table_disagreed_on_are_the_tables() {
    let (s, _) = studio();
    // Rename rows say what the command table says: 〜名を変更.
    for d in s.song.registry().all() {
        if d.name.ends_with(".rename")
            && let Some(label) = d.label()
        {
            assert!(label.contains("名を変更"), "{}: 「{label}」", d.name);
        }
    }
    assert_eq!(s.label_of("location.rename"), "場所名を変更");
    assert_eq!(s.label_of("location.remove"), "場所を外す");
    // The strings that were in the sources before are gone from them.
    for (file, src) in SOURCES {
        for old in [
            "名前を変える",
            "場所から外す",
            "場所のタブに足す",
            "カードを消す",
        ] {
            assert!(!src.contains(&format!("\"{old}\"")), "{file}: 「{old}」");
        }
    }
}
