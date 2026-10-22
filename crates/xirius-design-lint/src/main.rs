//! `cargo run -p xirius-design-lint` — Rust の検査（CI）。
//! `cargo run -p xirius-design-lint -- css <file.html>` — モックの CSS を数える（直さない）。
//! `cargo run -p xirius-design-lint -- tokens [--write]` — tokens.json と生成した Rust を合わせる。

use std::path::PathBuf;
use std::process::ExitCode;

use xirius_design_lint::css::{self, CssRule, Kind};
use xirius_design_lint::{Config, Rule, lint_workspace, tokens};

const USAGE: &str = "\
使い方:
  xirius-design-lint [--root <dir>] [--config <file>]   Rust を検査する（違反があれば終了コード 1）
  xirius-design-lint css <file> [--verbose] [--deny]    CSS の違反を数える（--deny で、あれば終了コード 1）
  xirius-design-lint tokens [--write] [--root <dir>]    tokens.json から作った Rust が合っているか検査する（--write で作り直す）
  xirius-design-lint --list-rules                       規則の一覧";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("--list-rules") => {
            for rule in Rule::ALL {
                println!("{:<22} {}", rule.name(), rule.summary());
            }
            ExitCode::SUCCESS
        }
        Some("css") => run_css(&args[1..]),
        Some("tokens") => run_tokens(&args[1..]),
        _ => run_rust(&args),
    }
}

fn run_rust(args: &[String]) -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut config_path: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = it.next().map(PathBuf::from).unwrap_or(root),
            "--config" => config_path = it.next().map(PathBuf::from),
            other => {
                eprintln!("知らない引数: {other}\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let config_path = config_path.unwrap_or_else(|| root.join("design-lint.toml"));
    let config = match Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("設定が読めない: {e}");
            return ExitCode::from(2);
        }
    };
    let report = match lint_workspace(&root, &config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "検査できない（{} に crates/ はあるか）: {e}",
                root.display()
            );
            return ExitCode::from(2);
        }
    };

    for v in &report.violations {
        println!("{v}");
    }
    for note in &report.notes {
        eprintln!("注意: {note}");
    }
    for (name, reason) in &report.exempt {
        eprintln!("検査から外している: {name}（{reason}）");
    }
    eprintln!(
        "design-lint: {} crate・{} ファイルを検査、違反 {} 件{}",
        report.crates_checked.len(),
        report.files_checked,
        report.violations.len(),
        if report.is_clean() {
            String::new()
        } else {
            let counts: Vec<String> = report
                .counts()
                .iter()
                .map(|(r, n)| format!("{r}={n}"))
                .collect();
            format!("（{}）", counts.join(", "))
        }
    );
    // トークンは、元がある木でだけ検査する（試験用の木には無い）。
    let tokens_ok = !root.join(tokens::SOURCE).is_file()
        || match tokens::check(&root) {
            Ok(()) => true,
            Err(e) => {
                println!("{e}");
                false
            }
        };
    if report.is_clean() && tokens_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `tokens`: 元の JSON と生成した Rust が合っているか。`--write` で作り直す。
fn run_tokens(args: &[String]) -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut write = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = it.next().map(PathBuf::from).unwrap_or(root),
            "--write" => write = true,
            other => {
                eprintln!(
                    "知らない引数: {other}
{USAGE}"
                );
                return ExitCode::from(2);
            }
        }
    }
    let result = if write {
        tokens::write(&root).map(|changed| {
            if changed {
                eprintln!("{} を作り直した", tokens::OUTPUT);
            } else {
                eprintln!("{} は元と合っている", tokens::OUTPUT);
            }
        })
    } else {
        tokens::check(&root)
            .map(|()| eprintln!("{} は {} と合っている", tokens::OUTPUT, tokens::SOURCE))
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn run_css(args: &[String]) -> ExitCode {
    let verbose = args.iter().any(|a| a == "--verbose");
    let deny = args.iter().any(|a| a == "--deny");
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(2);
        }
    };
    let report = css::check(&text);

    println!("{path}: 宣言 {} 個を検査", report.declarations);
    for rule in CssRule::ALL {
        println!(
            "  {:<10} 違反 {:>3}  除外 {:>3}",
            rule.name(),
            report.violations(rule),
            report.excluded(rule)
        );
    }
    println!(
        "  合計の違反 {} 件。参考: *-tint のトークン {} 個、style 属性 {} 個（数えていない）",
        report.total_violations(),
        report.tint_tokens,
        report.inline_styles
    );
    if verbose {
        for f in &report.findings {
            let kind = match &f.kind {
                Kind::Violation => "違反".to_string(),
                Kind::Excluded(why) => format!("除外: {why}"),
            };
            println!(
                "{path}:{}:{}: [{kind}] {} {{ {} }}",
                f.line, f.rule, f.selector, f.text
            );
        }
    }
    if deny && report.total_violations() > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
