//! Plain-text output of a run. `--json` bypasses this and prints the
//! serialized result as is.

use serde_json::Value;
use xirius_core::diff::{ChangeKind, ChangeSet};
use xirius_core::session::RunResult;

pub fn run_result(r: &RunResult) -> String {
    let mut s = String::new();
    if r.dry_run {
        s.push_str("[ドライラン] 曲は変更していません。実行すると次のようになります\n");
    }
    s.push_str(&r.message);
    s.push('\n');
    if !r.data.is_null() {
        value_lines(&r.data, 0, &mut s);
    }
    if !r.changes.is_empty() {
        s.push_str(&format!("変更: {}\n", r.changes.summary()));
        change_items(&r.changes, &mut s);
    }
    if let Some(step) = r.step {
        s.push_str(&format!("手順 {step} として履歴に記録しました\n"));
    }
    s
}

pub fn change_items(cs: &ChangeSet, out: &mut String) {
    for c in &cs.items {
        let mark = match c.change {
            ChangeKind::Added => '+',
            ChangeKind::Removed => '-',
            ChangeKind::Changed => '~',
        };
        out.push_str(&format!("  {mark} {} {}", c.kind, c.id));
        if let Some(d) = &c.detail {
            out.push_str(&format!("  {d}"));
        }
        out.push('\n');
    }
    if cs.truncated {
        out.push_str("  …（以降は件数のみ。--json ですべての件数を確認できます）\n");
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => {
            if s.is_empty() || s.contains(char::is_whitespace) {
                format!("{s:?}")
            } else {
                s.clone()
            }
        }
        Value::Null => "-".to_string(),
        Value::Array(a) => {
            let items: Vec<String> = a.iter().map(scalar).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

fn is_scalar_list(v: &Value) -> bool {
    matches!(v, Value::Array(a) if a.iter().all(|x| !x.is_object() && !x.is_array()))
}

fn is_flat(v: &Value) -> bool {
    match v {
        Value::Object(m) => m
            .values()
            .all(|x| (!x.is_object() && !x.is_array()) || is_scalar_list(x)),
        _ => false,
    }
}

/// Rows of flat objects become one `key=value` line each; anything deeper is
/// indented.
fn value_lines(v: &Value, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match v {
        Value::Array(items) => {
            for it in items {
                if is_flat(it) {
                    let Value::Object(m) = it else { continue };
                    let line: Vec<String> = m
                        .iter()
                        .map(|(k, x)| format!("{k}={}", scalar(x)))
                        .collect();
                    out.push_str(&format!("{pad}- {}\n", line.join("  ")));
                } else if it.is_object() || it.is_array() {
                    out.push_str(&format!("{pad}-\n"));
                    value_lines(it, indent + 1, out);
                } else {
                    out.push_str(&format!("{pad}- {}\n", scalar(it)));
                }
            }
        }
        Value::Object(m) => {
            for (k, x) in m {
                match x {
                    Value::Object(_) => {
                        out.push_str(&format!("{pad}{k}:\n"));
                        value_lines(x, indent + 1, out);
                    }
                    Value::Array(_) if !is_scalar_list(x) => {
                        out.push_str(&format!("{pad}{k}:\n"));
                        value_lines(x, indent + 1, out);
                    }
                    _ => out.push_str(&format!("{pad}{k}: {}\n", scalar(x))),
                }
            }
        }
        other => out.push_str(&format!("{pad}{}\n", scalar(other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rows_of_flat_objects_are_one_line_each() {
        let mut s = String::new();
        value_lines(
            &json!({"count": 2, "notes": [{"id": "nte_1", "pitch": 60}, {"id": "nte_2", "pitch": 64}]}),
            0,
            &mut s,
        );
        assert_eq!(
            s,
            "count: 2\nnotes:\n  - id=nte_1  pitch=60\n  - id=nte_2  pitch=64\n"
        );
    }

    #[test]
    fn strings_with_spaces_are_quoted() {
        assert_eq!(scalar(&json!("Lead Vox")), "\"Lead Vox\"");
        assert_eq!(scalar(&json!("trk_1")), "trk_1");
    }
}
