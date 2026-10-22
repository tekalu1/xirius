//! A command written on one line (`note.quantize --strength 50`), as a binding holds it.

use serde_json::{Map, Value};
use xirius_core::command::{ArgType, Registry};

/// Splits a line into words; `"..."` and `'...'` keep spaces.
pub fn words(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => {
                cur.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(format!("引用符が閉じていません: {line}"));
    }
    if started {
        out.push(cur);
    }
    Ok(out)
}

/// The command's name and its arguments. With the table, a flag of the command takes no value.
pub fn parse(
    line: &str,
    registry: Option<&Registry>,
) -> Result<(String, Map<String, Value>), String> {
    let w = words(line)?;
    let Some((name, rest)) = w.split_first() else {
        return Err("コマンドがありません".into());
    };
    let def = registry.and_then(|r| r.get(name));
    let mut args = Map::new();
    let mut i = 0;
    while i < rest.len() {
        let t = &rest[i];
        let Some(key) = t.strip_prefix("--").filter(|k| !k.is_empty()) else {
            return Err(format!(
                "「{t}」は引数ではありません（--名前 値 の形で書きます）"
            ));
        };
        if let Some((k, v)) = key.split_once('=') {
            args.insert(k.to_string(), Value::String(v.to_string()));
            i += 1;
            continue;
        }
        let is_flag = def
            .and_then(|d| {
                d.args
                    .iter()
                    .find(|a| a.name == key || a.aliases.contains(&key))
            })
            .is_some_and(|a| a.ty == ArgType::Bool);
        match rest.get(i + 1) {
            Some(v) if !is_flag && !v.starts_with("--") => {
                args.insert(key.to_string(), Value::String(v.clone()));
                i += 2;
            }
            _ => {
                args.insert(key.to_string(), Value::Bool(true));
                i += 1;
            }
        }
    }
    Ok((name.clone(), args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_a_binding() {
        let (n, a) = parse("note.transpose --by -1", None).unwrap();
        assert_eq!(n, "note.transpose");
        assert_eq!(a["by"], "-1");
        let (_, a) = parse("note.resize --adjacent", None).unwrap();
        assert_eq!(a["adjacent"], true);
        let (_, a) = parse("track.rename --name 'Lead Vox'", None).unwrap();
        assert_eq!(a["name"], "Lead Vox");
        assert!(parse("note.move 3", None).is_err());
        assert!(parse("", None).is_err());
    }

    #[test]
    fn a_flag_of_the_table_takes_no_value() {
        let r = Registry::standard();
        let (_, a) = parse("note.quantize --lengths --strength 50", Some(&r)).unwrap();
        assert_eq!(a["lengths"], true);
        assert_eq!(a["strength"], "50");
    }
}
