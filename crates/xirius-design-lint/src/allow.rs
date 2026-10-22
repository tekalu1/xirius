//! コメントの例外。
//!
//! ```text
//! // design-lint: allow(<規則>) <理由>            この行（行末のコメント）か、次のコード行
//! // design-lint: allow-file(<規則>) <理由>       ファイル全体
//! ```
//!
//! - 規則は `,` で並べられる（`allow(border, raw-radius)`）。
//! - 理由が無い印は効かず、`allow-without-reason` になる。
//! - 規則名が違う印は `unknown-rule`。何も止めていない印は `unused-allow`。

use crate::rule::Rule;

const MARKER: &str = "design-lint:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// 1 行（この行か、コメントだけの行なら次のコード行）。
    Line(usize),
    File,
}

/// 効く例外の印。
#[derive(Clone, Debug)]
pub struct Allow {
    pub rule: Rule,
    pub scope: Scope,
    /// 印のある行。
    pub at: usize,
    pub used: bool,
}

/// 印そのものの誤り。
#[derive(Clone, Debug)]
pub struct Problem {
    pub line: usize,
    pub rule: Rule,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Directives {
    pub allows: Vec<Allow>,
    pub problems: Vec<Problem>,
}

pub fn parse(source: &str) -> Directives {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Directives::default();

    for (i, line) in lines.iter().enumerate() {
        let number = i + 1;
        let Some(comment_at) = line.find("//") else {
            continue;
        };
        let comment = &line[comment_at + 2..];
        let Some(marker_at) = comment.find(MARKER) else {
            continue;
        };
        let directive = comment[marker_at + MARKER.len()..].trim();

        let (file_wide, rest) = if let Some(rest) = directive.strip_prefix("allow-file(") {
            (true, rest)
        } else if let Some(rest) = directive.strip_prefix("allow(") {
            (false, rest)
        } else {
            out.problems.push(Problem {
                line: number,
                rule: Rule::UnknownRule,
                message:
                    "`design-lint:` の後は `allow(<規則>) <理由>` か `allow-file(<規則>) <理由>`"
                        .into(),
            });
            continue;
        };
        let Some(close) = rest.find(')') else {
            out.problems.push(Problem {
                line: number,
                rule: Rule::UnknownRule,
                message: "`)` が無い".into(),
            });
            continue;
        };
        let names = &rest[..close];
        let reason =
            rest[close + 1..].trim_matches(|c: char| c.is_whitespace() || "-—:：".contains(c));

        let standalone = line[..comment_at].trim().is_empty();
        let target = if standalone {
            next_code_line(&lines, i)
        } else {
            number
        };

        let mut rules = Vec::new();
        for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            match Rule::from_name(name) {
                Some(rule) if rule.is_allowable() => rules.push(rule),
                _ => out.problems.push(Problem {
                    line: number,
                    rule: Rule::UnknownRule,
                    message: format!("`{name}` は例外にできる規則ではない"),
                }),
            }
        }
        if rules.is_empty() && names.trim().is_empty() {
            out.problems.push(Problem {
                line: number,
                rule: Rule::UnknownRule,
                message: "規則名が空".into(),
            });
        }
        if reason.is_empty() {
            if !rules.is_empty() {
                out.problems.push(Problem {
                    line: number,
                    rule: Rule::AllowWithoutReason,
                    message: "例外には理由を書く（`allow(<規則>) <理由>`）。理由の無い印は効かない"
                        .into(),
                });
            }
            continue;
        }
        for rule in rules {
            out.allows.push(Allow {
                rule,
                scope: if file_wide {
                    Scope::File
                } else {
                    Scope::Line(target)
                },
                at: number,
                used: false,
            });
        }
    }
    out
}

/// コメントだけの行から見て、次のコードの行（空行・コメントだけの行は飛ばす）。
fn next_code_line(lines: &[&str], from: usize) -> usize {
    for (j, line) in lines.iter().enumerate().skip(from + 1) {
        let t = line.trim();
        if !t.is_empty() && !t.starts_with("//") {
            return j + 1;
        }
    }
    from + 2
}
