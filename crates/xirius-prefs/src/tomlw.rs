//! Writing TOML by hand, so the files keep one order and diff line by line (06 §3.5).

use serde_json::Value;

/// A key, bare when it can be.
pub fn key(k: &str) -> String {
    if !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        k.to_string()
    } else {
        string(k)
    }
}

/// A basic string.
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A value (a string, a number, true / false, or a list of them).
pub fn value(v: &Value) -> String {
    match v {
        Value::String(s) => string(s),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(f) = n.as_f64()
                && n.as_i64().is_none()
                && n.as_u64().is_none()
                && f.fract() == 0.0
            {
                format!("{f:.1}")
            } else {
                n.to_string()
            }
        }
        Value::Array(a) => {
            let items: Vec<String> = a.iter().map(value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Null => "\"\"".into(),
        Value::Object(_) => string(&v.to_string()),
    }
}

/// A table's leaves as dotted keys (`edit.arrow.mode`), with their values.
pub fn flatten(table: &toml::Table) -> Vec<(Vec<String>, toml::Value)> {
    let mut out = Vec::new();
    walk(table, &mut Vec::new(), &mut out);
    out
}

fn walk(t: &toml::Table, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, toml::Value)>) {
    for (k, v) in t {
        path.push(k.clone());
        match v {
            toml::Value::Table(inner) => walk(inner, path, out),
            v => out.push((path.clone(), v.clone())),
        }
        path.pop();
    }
}

/// A TOML value as JSON.
pub fn to_json(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::from(*i),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Array(a) => Value::Array(a.iter().map(to_json).collect()),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Table(t) => {
            Value::Object(t.iter().map(|(k, v)| (k.clone(), to_json(v))).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn it_writes_what_toml_reads_back() {
        let text = format!(
            "{} = {}\n{} = {}\n{} = {}\n",
            key("arrow.mode"),
            value(&json!("a \"b\" \\ c")),
            key("buffer"),
            value(&json!(256)),
            key("offset"),
            value(&json!(2.0)),
        );
        let t: toml::Table = text.parse().unwrap();
        assert_eq!(t["arrow.mode"].as_str(), Some("a \"b\" \\ c"));
        assert_eq!(t["buffer"].as_integer(), Some(256));
        assert_eq!(t["offset"].as_float(), Some(2.0));
    }
}
