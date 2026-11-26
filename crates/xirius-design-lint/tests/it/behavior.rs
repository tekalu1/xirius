//! 見本では書きにくい挙動: 出力の形・設定・ワークスペースの走査・面の上辺の光。

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

#[test]
fn a_reasonless_allow_does_not_suppress() {
    let src = "// design-lint: allow(border)\nfn f() { div().border_1(); }\n";
    let got = rules(src);
    assert!(got.contains(&Rule::Border));
    assert!(got.contains(&Rule::AllowWithoutReason));
}

#[test]
fn an_allow_for_another_rule_does_not_suppress() {
    let src = "// design-lint: allow(raw-color) 色の例外\nfn f() { div().border_1(); }\n";
    let got = rules(src);
    assert!(got.contains(&Rule::Border));
    assert!(got.contains(&Rule::UnusedAllow));
}

#[test]
fn meta_rules_cannot_be_allowed() {
    let src = "// design-lint: allow(unused-allow) 試し\nfn f() {}\n";
    assert_eq!(rules(src), vec![Rule::UnknownRule]);
}

#[test]
fn test_code_is_skipped() {
    let src = "#[cfg(test)]\nmod t { fn f() { rgb(0x1); } }\n#[test]\nfn g() { rgb(0x2); }\n";
    assert!(rules(src).is_empty());
    let src = "#[cfg(not(test))]\nfn f() { rgb(0x1); }\n";
    assert_eq!(rules(src), vec![Rule::RawColor]);
}

// UI-007: 面の上辺の光は無い。中立の色でも、片側だけの光・影・縁は違反。
#[test]
fn a_one_sided_light_is_a_violation_even_when_neutral() {
    let light = "fn f() { div().absolute().top_0().h(px(1.0)).bg(Surface::S3.color(t)); }";
    assert_eq!(rules(light), vec![Rule::OneSidedEdge]);

    let coloured = "fn f() { div().absolute().top_0().h(px(1.0)).bg(Ink::Blue.color(t)); }";
    assert_eq!(rules(coloured), vec![Rule::OneSidedEdge, Rule::ColorFamily]);

    // 線で作った光は、線なので違反。
    let by_border =
        "fn f() { div().rounded(Radius::M.px()).border_t_1().border_color(Surface::S3.color(t)); }";
    assert_eq!(
        rules(by_border),
        vec![Rule::Border, Rule::Border, Rule::OneSidedEdge]
    );
}

#[test]
fn a_one_sided_box_shadow_is_a_violation_but_a_soft_outer_shadow_is_not() {
    let top = "fn f() { let s = BoxShadow { color: c, offset: point(px(0.), px(-1.)), blur_radius: px(0.), spread_radius: px(0.) }; }";
    assert_eq!(rules(top), vec![Rule::OneSidedEdge]);
    let side = "fn f() { let s = BoxShadow { color: c, offset: point(px(2.), px(0.)), blur_radius: px(0.), spread_radius: px(0.) }; }";
    assert_eq!(rules(side), vec![Rule::OneSidedEdge]);

    // 浮く面の影: ずらしてぼかした全周の外側の影。
    let float = "fn f() { let s = BoxShadow { color: c, offset: point(px(0.), px(12.)), blur_radius: px(32.), spread_radius: px(0.) }; }";
    assert!(rules(float).is_empty());
    // ずらしが無い（四辺に同じ）影も通す。
    let even = "fn f() { let s = BoxShadow { color: c, offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.) }; }";
    assert!(rules(even).is_empty());
}

#[test]
fn rounded_without_a_side_color_is_fine() {
    assert!(
        rules("fn f() { div().rounded(Radius::M.px()).bg(Surface::Panel.color(t)); }").is_empty()
    );
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

#[test]
fn only_structure_line_may_be_a_thin_bar() {
    // 固定値の細い棒は線。
    let literal = "fn f() { let a = div().w(px(2.0)).h_full().bg(c); }";
    assert_eq!(rules(literal), vec![Rule::Line]);
    // 太さを StructureLine から取れば通る。
    let api = "fn f() { let a = div().w(px(StructureLine::THICKNESS)).h_full().bg(c); }";
    assert!(rules(api).is_empty());
    // 長さに数が混ざっていても、StructureLine を通した引数は数えない。
    let inset = "fn f() { let a = div().w(px(StructureLine::THICKNESS)).h(px(h - StructureLine::END_INSET * 2.0)).bg(c); }";
    assert!(rules(inset).is_empty());
    // 太い面・色の無い物は線ではない。
    assert!(rules("fn f() { let a = div().w(px(8.0)).h(px(2.0)); }").is_empty());
    assert!(rules("fn f() { let a = div().w(px(24.0)).h(px(24.0)).bg(c); }").is_empty());
}

#[test]
fn a_line_can_be_allowed_with_a_reason_but_not_without() {
    let ok = "fn f() {\n    // design-lint: allow(line) 再生位置の針。区切りではない\n    let a = div().w(px(2.0)).h_full().bg(c);\n}\n";
    assert!(rules(ok).is_empty(), "{:?}", rules(ok));
    let bare = "fn f() {\n    // design-lint: allow(line)\n    let a = div().w(px(2.0)).h_full().bg(c);\n}\n";
    assert!(rules(bare).contains(&Rule::AllowWithoutReason));
}
