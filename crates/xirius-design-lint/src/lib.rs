//! Xirius のデザインの規則を、ソースの上で検査する lint。
//!
//! - Rust: `lint_source`（1 ファイル）・`lint_workspace`（`crates/*`）。
//! - CSS（モックの HTML）: `css::check`（数を数えるだけで、直さない）。
//! - デザイントークン: `tokens`（`tokens.json` から Rust を作り、合っているかを検査する）。
//!
//! 規則は [`Rule`]。`xirius-design` の外では、生の色・生の角丸・書体名・線・グラデーション・
//! 角丸の 1 辺の色を書けない。例外は `// design-lint: allow(<規則>) <理由>`（理由が要る）。

pub mod allow;
mod color_rules;
pub mod config;
pub mod css;
mod list_menu;
mod ops_coverage;
pub mod rule;
mod scan;
pub mod tokens;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use config::Config;
pub use rule::Rule;

use allow::Scope;

/// 規則の違反 1 件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub rule: Rule,
    pub message: String,
}

impl fmt::Display for Violation {
    /// `file:line:rule: message`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.file, self.line, self.rule, self.message
        )
    }
}

/// 文字列が画面に出るファイルか（`xirius-ui` の `src`。試験のファイルは除く）。
/// CLI・MCP のエラーは、呼び出す側に直し方を教えてよいので、案内の規則を当てない。
fn is_screen_file(label: &str) -> bool {
    let label = label.replace('\\', "/");
    let name = label.rsplit('/').next().unwrap_or(&label);
    label.contains("xirius-ui/src/")
        && !label.contains("/tests/")
        && name != "tests.rs"
        && !name.ends_with("_tests.rs")
}

/// 1 つのソースを検査する。`label` は出力に出すファイル名。
pub fn lint_source(label: &str, source: &str) -> Vec<Violation> {
    let violation = |line: usize, rule: Rule, message: String| Violation {
        file: label.to_string(),
        line,
        rule,
        message,
    };

    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(e) => {
            let line = e.span().start().line.max(1);
            return vec![violation(line, Rule::ParseError, e.to_string())];
        }
    };

    let mut hits = scan::scan(&file, is_screen_file(label));
    hits.extend(color_rules::scan(&file, label));
    if is_screen_file(label) {
        hits.extend(ops_coverage::scan(&file));
        hits.extend(list_menu::scan(&file, label));
    }
    let mut directives = allow::parse(source);
    let mut out = Vec::new();

    for hit in hits {
        let mut suppressed = false;
        for a in &mut directives.allows {
            let applies = match &a.scope {
                Scope::File => true,
                Scope::Line(target) => *target == hit.line || *target == hit.anchor,
            };
            if a.rule == hit.rule && applies {
                a.used = true;
                suppressed = true;
            }
        }
        if !suppressed {
            out.push(violation(hit.line, hit.rule, hit.message));
        }
    }

    for p in directives.problems {
        out.push(violation(p.line, p.rule, p.message));
    }
    for a in directives.allows.iter().filter(|a| !a.used) {
        out.push(violation(
            a.at,
            Rule::UnusedAllow,
            format!("`allow({})` が何も止めていない。消す", a.rule),
        ));
    }

    out.sort_by_key(|a| (a.line, a.rule));
    out
}

/// ワークスペース全体の結果。
#[derive(Debug, Default)]
pub struct Report {
    pub violations: Vec<Violation>,
    pub files_checked: usize,
    pub crates_checked: Vec<String>,
    /// 検査から外している crate（名前, 理由）。
    pub exempt: Vec<(String, String)>,
    /// 設定と実際のずれなど。
    pub notes: Vec<String>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    /// 規則ごとの件数。
    pub fn counts(&self) -> Vec<(Rule, usize)> {
        Rule::ALL
            .into_iter()
            .map(|r| (r, self.violations.iter().filter(|v| v.rule == r).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }
}

/// `<root>/crates/*` の Rust を検査する。
pub fn lint_workspace(root: &Path, config: &Config) -> io::Result<Report> {
    let mut report = Report::default();
    let crates_dir = root.join("crates");

    let mut crate_dirs: Vec<PathBuf> = fs::read_dir(&crates_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();

    let names: Vec<String> = crate_dirs
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();

    for e in &config.exempt {
        if names.contains(&e.krate) {
            report.exempt.push((e.krate.clone(), e.reason.clone()));
        } else {
            report.notes.push(format!(
                "design-lint.toml: exempt の `{}` という crate は無い（消してよい）",
                e.krate
            ));
        }
    }

    for dir in &crate_dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if config.design_crates.contains(&name) || config.exempt.iter().any(|e| e.krate == name) {
            continue;
        }
        report.crates_checked.push(name);

        let mut files = Vec::new();
        for sub in ["src", "examples", "benches"] {
            collect_rs(&dir.join(sub), &mut files)?;
        }
        if config.include_tests {
            collect_rs(&dir.join("tests"), &mut files)?;
        }
        let build = dir.join("build.rs");
        if build.is_file() {
            files.push(build);
        }
        files.sort();

        for path in files {
            let source = fs::read_to_string(&path)?;
            let label = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            report.files_checked += 1;
            report.violations.extend(lint_source(&label, &source));
        }
    }
    Ok(report)
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "target" || n == "fixtures")
            {
                continue;
            }
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}
