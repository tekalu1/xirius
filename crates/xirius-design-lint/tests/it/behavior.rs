//! 見本では書きにくい挙動: 出力の形・読めないソース・設定・ワークスペースの走査。
//! 規則ごとの合否と例外の印の効き方（面の上辺の光・細い線・テストの中を見ないこと・理由の無い印など）は fixtures の見本が持つ。

use std::fs;

use xirius_design_lint::{Config, Rule, lint_source, lint_workspace};

fn rules(source: &str) -> Vec<Rule> {
    lint_source("x.rs", source)
        .into_iter()
        .map(|v| v.rule)
        .collect()
}

#[test]
fn output_is_file_line_rule() {
    let v = lint_source("crates/a/src/b.rs", "fn f() {\n    let a = rgb(0x1);\n}\n");
    assert_eq!(v.len(), 1);
    let text = v[0].to_string();
    assert!(
        text.starts_with("crates/a/src/b.rs:2:raw-color: "),
        "{text}"
    );
}

#[test]
fn unreadable_source_is_a_parse_error() {
    assert_eq!(rules("fn f( {"), vec![Rule::ParseError]);
}

/// A folder of the test's own, removed when dropped (also when the test fails).
fn temp_workspace(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-design-lint-{name}-"))
        .tempdir()
        .unwrap()
}

fn write(root: &std::path::Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn workspace_walk_respects_design_crates_and_exemptions() {
    let root_tmp = temp_workspace("walk");
    let root = root_tmp.path().to_path_buf();
    let bad = "fn f() { let a = rgb(0x336699); }\n";
    write(&root, "crates/xirius-design/src/lib.rs", bad);
    write(&root, "crates/xirius-core/src/lib.rs", "fn f() {}\n");
    write(&root, "crates/xirius-ui/src/theme.rs", bad);
    write(&root, "crates/xirius-engine/src/lib.rs", bad);
    write(&root, "crates/xirius-engine/tests/t.rs", bad);
    write(&root, "crates/xirius-engine/src/fixtures/skip.rs", bad);

    let config = Config::parse(
        r#"
        design_crates = ["xirius-design"]

        [[exempt]]
        crate = "xirius-ui"
        reason = "GPUI の試作の theme.rs。xirius-design に移すまで"

        [[exempt]]
        crate = "xirius-gone"
        reason = "もう無い"
        "#,
    )
    .unwrap();
    let report = lint_workspace(&root, &config).unwrap();

    // 規則の持ち主と、外した crate は見ない。tests/ と fixtures/ も見ない。
    assert_eq!(report.crates_checked, vec!["xirius-core", "xirius-engine"]);
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    assert_eq!(report.violations[0].file, "crates/xirius-engine/src/lib.rs");
    // 外した crate は黙って通さず、一覧に出る。無い crate の設定は注意になる。
    assert_eq!(report.exempt.len(), 1);
    assert_eq!(report.exempt[0].0, "xirius-ui");
    assert_eq!(report.notes.len(), 1);

    let with_tests = Config {
        include_tests: true,
        ..Config::default()
    };
    let report = lint_workspace(&root, &with_tests).unwrap();
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.file == "crates/xirius-engine/tests/t.rs")
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn config_needs_a_reason_and_rejects_unknown_keys() {
    assert!(Config::parse("[[exempt]]\ncrate = \"a\"\nreason = \"  \"\n").is_err());
    assert!(Config::parse("[[exempt]]\ncrate = \"a\"\n").is_err());
    assert!(Config::parse("unknown = 1\n").is_err());
    let c = Config::parse("").unwrap();
    assert!(c.design_crates.contains(&"xirius-design".to_string()));
}

#[test]
fn a_missing_config_file_means_defaults() {
    let c = Config::load(std::path::Path::new("definitely/not/here.toml")).unwrap();
    assert!(c.exempt.is_empty());
}
