//! 違反の見本は落ち、きれいな見本は通る。
//!
//! 見本は `tests/fixtures/{violations,clean}/*.rs`（コンパイルはしない）。
//! 出るはずの違反は、行末の `//~ <規則>[, <規則>]` で書く。`//~^` は 1 つ前の行を指す。
//! 検査の結果が、書いた印と（行・規則とも）過不足なく一致しなければ落ちる。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use xirius_design_lint::{Rule, lint_source};

fn fixture_files(kind: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(kind);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    files
}

/// `(行, 規則)` の印。
fn expected(source: &str) -> Vec<(usize, Rule)> {
    let mut out = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let number = i + 1;
        let Some(at) = line.find("//~") else {
            continue;
        };
        let mark = &line[at + 3..];
        let (target, names) = match mark.strip_prefix('^') {
            Some(rest) => (number - 1, rest),
            None => (number, mark),
        };
        for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            let rule = Rule::from_name(name)
                .unwrap_or_else(|| panic!("{number} 行目: 規則名 `{name}` が違う"));
            out.push((target, rule));
        }
    }
    out.sort();
    out
}

/// 見本の検査に渡すファイル名。`ui_` で始まる見本は、`xirius-ui` の画面のファイルとして見る。
fn label_of(path: &Path) -> String {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    if name.starts_with("track_") {
        format!("crates/xirius-ui/src/arrange/{name}")
    } else if name.starts_with("ui_") {
        format!("crates/xirius-ui/src/{name}")
    } else {
        name
    }
}

fn actual(label: &str, source: &str) -> Vec<(usize, Rule)> {
    let mut out: Vec<(usize, Rule)> = lint_source(label, source)
        .into_iter()
        .map(|v| (v.line, v.rule))
        .collect();
    out.sort();
    out
}

#[test]
fn violating_fixtures_fail_exactly_where_marked() {
    let files = fixture_files("violations");
    assert!(!files.is_empty());
    let mut seen: BTreeSet<Rule> = BTreeSet::new();

    for path in files {
        let label = label_of(&path);
        let source = fs::read_to_string(&path).unwrap();
        let want = expected(&source);
        assert!(!want.is_empty(), "{label}: 印が 1 つも無い");
        let got = actual(&label, &source);
        assert_eq!(
            got,
            want,
            "{label}: 検査の結果（左）と、書いた印（右）が違う\n{:#?}",
            lint_source(&label, &source)
        );
        seen.extend(want.into_iter().map(|(_, r)| r));
    }

    // 全部の規則が、少なくとも 1 つの見本で落ちている。
    let missing: Vec<Rule> = Rule::ALL
        .into_iter()
        .filter(|r| *r != Rule::ParseError && !seen.contains(r))
        .collect();
    assert!(missing.is_empty(), "見本の無い規則: {missing:?}");
}

#[test]
fn clean_fixtures_pass() {
    let files = fixture_files("clean");
    assert!(!files.is_empty());
    for path in files {
        let label = label_of(&path);
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            expected(&source).is_empty(),
            "{label}: きれいな見本に印がある"
        );
        let got = lint_source(&label, &source);
        assert!(
            got.is_empty(),
            "{label}: 何も出ないはずが出た:\n{}",
            got.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
