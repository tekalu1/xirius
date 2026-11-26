//! 違反の見本は落ち、きれいな見本は通る。
//!
//! 見本は `tests/fixtures/{violations,clean}/*.rs`（コンパイルはしない）。
//! 出るはずの違反は、行末の `//~ <規則>[, <規則>]` で書く。`//~^` は 1 つ前の行を指す。
//! 検査の結果が、書いた印と（行・規則とも）過不足なく一致しなければ落ちる。
//!
//! 1 つのファイルに、互いに効き合わない見本を並べるときは、`// ----` の行で区切る。
//! 区切りの間は 1 つずつ別のソースとして検査する（`use` も区切りの中だけに効く）。行の番号は
//! ファイルのまま。`// ---- crates/xirius-ui/src/draw.rs` のように後ろにパスを書くと、その区切りは
//! そのパスのファイルとして検査する。
//!
//! 落ちるときは、全部の見本の食い違いを `ファイル:行: 規則` で並べる（最初の 1 つで止まらない）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use xirius_design_lint::{Rule, lint_source};

const CUT: &str = "// ----";

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

/// 区切りごとの `(検査に渡すファイル名, ソース)`。ほかの区切りの行は空にして、行の番号を保つ。
fn pieces(label: &str, source: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut starts = vec![(0, label.to_string())];
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.trim().strip_prefix(CUT) {
            let path = rest.trim_start_matches('-').trim();
            let own = if path.is_empty() { label } else { path };
            starts.push((i + 1, own.to_string()));
        }
    }
    let mut out = Vec::new();
    for (n, (start, own)) in starts.iter().enumerate() {
        let end = starts.get(n + 1).map_or(lines.len(), |next| next.0 - 1);
        let text: Vec<&str> = (0..lines.len())
            .map(|i| {
                if (*start..end).contains(&i) {
                    lines[i]
                } else {
                    ""
                }
            })
            .collect();
        out.push((own.clone(), text.join("\n") + "\n"));
    }
    out
}

/// 見本の全部の区切りを検査した `(行, 規則, 説明)`。
fn actual(label: &str, source: &str) -> Vec<(usize, Rule, String)> {
    let mut out: Vec<(usize, Rule, String)> = pieces(label, source)
        .iter()
        .flat_map(|(own, text)| lint_source(own, text))
        .map(|v| (v.line, v.rule, v.to_string()))
        .collect();
    out.sort();
    out
}

/// 印と結果の食い違いを、`ファイル:行: 規則` で並べる（同じ行に同じ規則が 2 つ出るなら、印も 2 つ）。
fn mismatches(label: &str, want: &[(usize, Rule)], got: &[(usize, Rule, String)]) -> Vec<String> {
    let mut counts: BTreeMap<(usize, Rule), (usize, usize, Vec<&str>)> = BTreeMap::new();
    for key in want {
        counts.entry(*key).or_default().0 += 1;
    }
    for (line, rule, text) in got {
        let entry = counts.entry((*line, *rule)).or_default();
        entry.1 += 1;
        entry.2.push(text);
    }
    let mut out = Vec::new();
    for ((line, rule), (marked, found, texts)) in counts {
        let name = rule.name();
        if found > marked {
            out.push(format!(
                "{label}:{line}: {name} が {found} 回出たが、印は {marked} つ（{}）",
                texts.join(" / ")
            ));
        } else if found < marked {
            out.push(format!(
                "{label}:{line}: {name} の印が {marked} つあるが、出たのは {found} 回"
            ));
        }
    }
    out
}

#[test]
fn violating_fixtures_fail_exactly_where_marked() {
    let files = fixture_files("violations");
    assert!(!files.is_empty());
    let mut seen: BTreeSet<Rule> = BTreeSet::new();
    let mut failures = Vec::new();

    for path in files {
        let label = label_of(&path);
        let source = fs::read_to_string(&path).unwrap();
        let want = expected(&source);
        if want.is_empty() {
            failures.push(format!("{label}: 印が 1 つも無い"));
        }
        failures.extend(mismatches(&label, &want, &actual(&label, &source)));
        seen.extend(want.into_iter().map(|(_, r)| r));
    }

    // 全部の規則が、少なくとも 1 つの見本で落ちている。
    let missing: Vec<Rule> = Rule::ALL
        .into_iter()
        .filter(|r| *r != Rule::ParseError && !seen.contains(r))
        .collect();
    if !missing.is_empty() {
        failures.push(format!("見本の無い規則: {missing:?}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn clean_fixtures_pass() {
    let files = fixture_files("clean");
    assert!(!files.is_empty());
    let mut failures = Vec::new();
    for path in files {
        let label = label_of(&path);
        let source = fs::read_to_string(&path).unwrap();
        if !expected(&source).is_empty() {
            failures.push(format!("{label}: きれいな見本に印がある"));
        }
        failures.extend(mismatches(&label, &[], &actual(&label, &source)));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
