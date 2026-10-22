//! The preferences: `prefs.toml`, one value per name (06 §3.4).
//!
//! The names and what each may be are fixed here; a few are families with a name of their
//! own after the prefix (`midi.inputs.<port>`, `plugins.process.<plugin>`). Only the values that
//! differ from the default are kept, so the file shows what the user changed.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::tomlw;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bool,
    Choice(&'static [&'static str]),
    Int(i64, i64),
    Float(f64, f64),
    Text,
    /// A list of texts (folders).
    List,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pref {
    /// The name, or the prefix of a family ending in `.`.
    pub key: &'static str,
    pub kind: Kind,
    /// The default, written as `prefs set` takes it.
    pub default: &'static str,
}

const fn p(key: &'static str, kind: Kind, default: &'static str) -> Pref {
    Pref { key, kind, default }
}

pub const THEMES: &[&str] = &["dark", "light", "auto"];
pub const LISTEN: &[&str] = &["localhost", "network"];
pub const DRIVERS: &[&str] = &["wasapi-shared", "wasapi-exclusive"];
pub const BUFFERS: &[i64] = &[32, 64, 128, 256, 512, 1024];
pub const PROCESS: &[&str] = &["same", "shared", "dedicated"];
/// What an input-quantized recording snaps a note's head to (REC-009).
pub const INPUT_QUANTIZE: &[&str] = &["1/4", "1/8", "1/16", "1/8T"];

/// Settings an older file may still hold; they are read and dropped without a word.
const REMOVED: &[&str] = &["edit.arrow.alternative-tool"];

pub const PREFS: &[Pref] = &[
    p("ui.theme", Kind::Choice(THEMES), "dark"),
    p("app.update.check", Kind::Bool, "true"),
    p("app.update.beta", Kind::Bool, "false"),
    p("ai.mcp.enabled", Kind::Bool, "true"),
    p("ai.mcp.listen", Kind::Choice(LISTEN), "localhost"),
    p("audio.driver", Kind::Choice(DRIVERS), "wasapi-exclusive"),
    p("audio.device", Kind::Text, ""),
    p("audio.buffer", Kind::Int(32, 1024), "256"),
    p("audio.record-offset", Kind::Float(-500.0, 500.0), "0"),
    p("audio.count-in", Kind::Int(0, 8), "0"),
    p("record.punch", Kind::Bool, "false"),
    p("record.preroll", Kind::Bool, "false"),
    p("record.preroll-bars", Kind::Int(1, 8), "1"),
    p("record.postroll", Kind::Bool, "false"),
    p("record.postroll-bars", Kind::Int(1, 8), "1"),
    p("midi.follow-selection", Kind::Bool, "true"),
    p("midi.input-offset", Kind::Float(-500.0, 500.0), "0"),
    p("midi.inputs.", Kind::Bool, "true"),
    p("plugins.vst3.paths", Kind::List, ""),
    p("plugins.clap.paths", Kind::List, ""),
    p("plugins.process.", Kind::Choice(PROCESS), "same"),
    p(
        "edit.arrow.mode",
        Kind::Choice(&["extended", "basic"]),
        "basic",
    ),
    p("edit.arrange.link", Kind::Bool, "false"),
    p("edit.quantize.auto", Kind::Bool, "false"),
    p(
        "edit.snap.arrange.mode",
        Kind::Choice(&["adaptive", "bar", "quantize", "frames"]),
        "adaptive",
    ),
    p(
        "edit.snap.roll.mode",
        Kind::Choice(&["adaptive", "bar", "quantize", "frames"]),
        "adaptive",
    ),
    p("edit.snap.arrange.grid", Kind::Bool, "true"),
    p("edit.snap.roll.grid", Kind::Bool, "true"),
    p("edit.snap.arrange.events", Kind::Bool, "true"),
    p("edit.snap.roll.events", Kind::Bool, "true"),
    p("edit.snap.arrange.relative", Kind::Bool, "false"),
    p("edit.snap.roll.relative", Kind::Bool, "false"),
    p("edit.cursor.crosshair", Kind::Bool, "false"),
    p("edit.drag.value-tip", Kind::Bool, "true"),
    p(
        "edit.wheel.ruler",
        Kind::Choice(&["zoom", "scroll"]),
        "zoom",
    ),
    p(
        "edit.draw.length",
        Kind::Choice(&["inspector", "quantize", "last"]),
        "inspector",
    ),
    p(
        "edit.draw.velocity",
        Kind::Choice(&["inspector", "fixed", "last"]),
        "inspector",
    ),
    p("edit.draw.velocity-value", Kind::Int(1, 127), "100"),
    p("edit.audition", Kind::Bool, "true"),
    p(
        "edit.note.overlap",
        Kind::Choice(&["allow", "trim"]),
        "allow",
    ),
    p("edit.record.input-quantize", Kind::Bool, "false"),
    p(
        "edit.record.input-quantize-value",
        Kind::Choice(INPUT_QUANTIZE),
        "1/16",
    ),
    p("edit.clip.no-overlap", Kind::Bool, "false"),
    p("edit.ripple", Kind::Bool, "false"),
    p("edit.automation.follows-clips", Kind::Bool, "true"),
    p("edit.automation.locks-clips", Kind::Bool, "true"),
    p("edit.click-empty.locate", Kind::Bool, "true"),
    p("edit.cursor.follows-edit", Kind::Bool, "false"),
    p(
        "edit.autoscroll",
        Kind::Choice(&["turn-over", "center", "left"]),
        "turn-over",
    ),
];

/// The definition of `key` (a family's for a name in it).
pub fn schema(key: &str) -> Option<&'static Pref> {
    PREFS.iter().find(|p| {
        if p.key.ends_with('.') {
            key.len() > p.key.len() && key.starts_with(p.key)
        } else {
            p.key == key
        }
    })
}

impl Pref {
    pub fn is_family(&self) -> bool {
        self.key.ends_with('.')
    }

    pub fn default_value(&self) -> Value {
        if self.kind == Kind::List {
            return json!([]);
        }
        self.read(&Value::String(self.default.into()))
            .expect("the defaults read")
    }

    /// `v` as this preference keeps it, or why it cannot be.
    pub fn read(&self, v: &Value) -> Result<Value, String> {
        let text = match v {
            Value::String(s) => s.trim().to_string(),
            other => other.to_string(),
        };
        let name = self.key.trim_end_matches('.');
        match self.kind {
            Kind::Bool => match (v, text.to_lowercase().as_str()) {
                (Value::Bool(b), _) => Ok(Value::Bool(*b)),
                (_, "true" | "on" | "yes" | "1") => Ok(Value::Bool(true)),
                (_, "false" | "off" | "no" | "0") => Ok(Value::Bool(false)),
                _ => Err(format!("{name} は true か false です（「{text}」）")),
            },
            Kind::Choice(list) => {
                if list.contains(&text.as_str()) {
                    Ok(Value::String(text))
                } else {
                    Err(format!(
                        "{name} は {} のいずれかです（「{text}」）",
                        list.join(" / ")
                    ))
                }
            }
            Kind::Int(lo, hi) => match text.parse::<i64>() {
                Ok(n) if (lo..=hi).contains(&n) => Ok(json!(n)),
                _ => Err(format!("{name} は {lo}〜{hi} の整数です（「{text}」）")),
            },
            Kind::Float(lo, hi) => match text.parse::<f64>() {
                Ok(f) if f.is_finite() && (lo..=hi).contains(&f) => Ok(json!(f)),
                _ => Err(format!("{name} は {lo}〜{hi} の数です（「{text}」）")),
            },
            Kind::Text => Ok(Value::String(text)),
            Kind::List => match v {
                Value::Array(a) => {
                    let mut out = Vec::new();
                    for x in a {
                        match x.as_str() {
                            Some(s) if !s.trim().is_empty() => {
                                out.push(Value::String(s.trim().into()))
                            }
                            Some(_) => {}
                            None => return Err(format!("{name} は文字列の配列です")),
                        }
                    }
                    Ok(Value::Array(out))
                }
                _ if text.starts_with('[') => serde_json::from_str::<Value>(&text)
                    .map_err(|e| format!("{name}: {e}"))
                    .and_then(|a| self.read(&a)),
                _ if text.is_empty() => Ok(json!([])),
                _ => Ok(json!([text])),
            },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prefs {
    /// The values that differ from the default.
    pub values: BTreeMap<String, Value>,
}

impl Prefs {
    /// The value of `key` (the default when unset); `None` for a name that is not a preference.
    pub fn get(&self, key: &str) -> Option<Value> {
        let s = schema(key)?;
        Some(
            self.values
                .get(key)
                .cloned()
                .unwrap_or_else(|| s.default_value()),
        )
    }

    pub fn bool(&self, key: &str) -> bool {
        self.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
    }

    pub fn text(&self, key: &str) -> String {
        match self.get(key) {
            Some(Value::String(s)) => s,
            Some(v) => v.to_string(),
            None => String::new(),
        }
    }

    pub fn number(&self, key: &str) -> f64 {
        self.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Sets `key`; the value it now has.
    pub fn set(&mut self, key: &str, v: &Value) -> Result<Value, String> {
        let s = schema(key).ok_or_else(|| format!("「{key}」は設定の名前ではありません"))?;
        let v = s.read(v)?;
        if v == s.default_value() {
            self.values.remove(key);
        } else {
            self.values.insert(key.to_string(), v.clone());
        }
        Ok(v)
    }

    /// Adds `item` to a list (once), or takes it away.
    pub fn edit_list(&mut self, key: &str, item: &str, add: bool) -> Result<Value, String> {
        let s = schema(key).ok_or_else(|| format!("「{key}」は設定の名前ではありません"))?;
        if s.kind != Kind::List {
            return Err(format!(
                "{key} は一覧ではありません（--add / --remove は使用できません）"
            ));
        }
        let item = item.trim();
        let mut list = self.list(key);
        let same = |x: &String| x.eq_ignore_ascii_case(item);
        if add {
            if item.is_empty() {
                return Err("追加するものがありません".into());
            }
            if !list.iter().any(same) {
                list.push(item.to_string());
            }
        } else {
            list.retain(|x| !same(x));
        }
        self.set(key, &json!(list))
    }

    /// Back to the default: one name, or every name under a prefix (`edit.`), or all.
    pub fn reset(&mut self, key: Option<&str>) -> usize {
        let before = self.values.len();
        match key {
            None => self.values.clear(),
            Some(k) => {
                let prefix = format!("{}.", k.trim_end_matches('.'));
                self.values
                    .retain(|name, _| name != k && !name.starts_with(&prefix));
            }
        }
        before - self.values.len()
    }

    /// Every preference under `prefix` (all with `None`): name → value, default.
    pub fn list_all(&self, prefix: Option<&str>) -> Vec<(String, Value, Value)> {
        let under = |k: &str| {
            prefix.is_none_or(|p| {
                let p = p.trim_end_matches('.');
                k == p || k.starts_with(&format!("{p}."))
            })
        };
        let mut out: Vec<(String, Value, Value)> = PREFS
            .iter()
            .filter(|s| !s.is_family() && under(s.key))
            .map(|s| {
                (
                    s.key.to_string(),
                    self.get(s.key).unwrap(),
                    s.default_value(),
                )
            })
            .collect();
        for (k, v) in &self.values {
            if let Some(s) = schema(k)
                && s.is_family()
                && under(k)
            {
                out.push((k.clone(), v.clone(), s.default_value()));
            }
        }
        out
    }

    pub fn from_toml(t: &toml::Table, problems: &mut Vec<String>) -> Prefs {
        let mut out = Prefs::default();
        for (path, v) in tomlw::flatten(t) {
            let key = path.join(".");
            if REMOVED.contains(&key.as_str()) {
                continue;
            }
            if let Err(e) = out.set(&key, &tomlw::to_json(&v)) {
                problems.push(format!("prefs.toml: {e}"));
            }
        }
        out
    }

    /// Grouped by the first part of the name: `[edit]` then `arrow.mode = "extended"`.
    pub fn to_toml(&self) -> String {
        let mut s = String::from("# prefs.toml（既定から変えた値）\n");
        let mut groups: Vec<&str> = Vec::new();
        for p in PREFS {
            let g = p.key.split('.').next().unwrap_or("");
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        for g in groups {
            let mut lines: Vec<(usize, String)> = Vec::new();
            for (k, v) in &self.values {
                let Some(sc) = schema(k) else { continue };
                if k.split('.').next() != Some(g) {
                    continue;
                }
                let rest = &k[g.len() + 1..];
                let key = if sc.is_family() {
                    let fixed = &sc.key[g.len() + 1..];
                    let name = &k[sc.key.len()..];
                    let parts: Vec<String> = fixed
                        .trim_end_matches('.')
                        .split('.')
                        .map(tomlw::key)
                        .collect();
                    format!("{}.{}", parts.join("."), tomlw::key(name))
                } else {
                    rest.split('.')
                        .map(tomlw::key)
                        .collect::<Vec<_>>()
                        .join(".")
                };
                let order = PREFS.iter().position(|x| x.key == sc.key).unwrap_or(0);
                lines.push((order, format!("{key} = {}\n", tomlw::value(v))));
            }
            if lines.is_empty() {
                continue;
            }
            lines.sort();
            s.push_str(&format!("\n[{g}]\n"));
            for (_, l) in lines {
                s.push_str(&l);
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_changed_value_is_kept() {
        let mut p = Prefs::default();
        assert_eq!(p.get("ui.theme"), Some(json!("dark")));
        assert!(p.bool("edit.automation.follows-clips"));
        p.set("edit.automation.follows-clips", &json!("false"))
            .unwrap();
        assert!(!p.bool("edit.automation.follows-clips"));
        p.set("ui.theme", &json!("dark")).unwrap();
        assert_eq!(p.values.len(), 1);
        assert!(p.set("ui.theme", &json!("pink")).is_err());
        assert!(p.set("audio.buffer", &json!("4096")).is_err());
        assert!(p.set("no.such", &json!("1")).is_err());
        assert_eq!(p.get("midi.inputs.Launchkey 49"), Some(json!(true)));
        p.set("midi.inputs.Launchkey 49", &json!(false)).unwrap();
        assert!(!p.bool("midi.inputs.Launchkey 49"));
    }

    #[test]
    fn a_list_takes_and_gives() {
        let mut p = Prefs::default();
        p.edit_list("plugins.vst3.paths", "D:\\VST3", true).unwrap();
        p.edit_list("plugins.vst3.paths", "d:\\vst3", true).unwrap();
        assert_eq!(p.list("plugins.vst3.paths"), ["D:\\VST3"]);
        p.edit_list("plugins.vst3.paths", "D:\\VST3", false)
            .unwrap();
        assert!(p.values.is_empty());
        assert!(p.edit_list("ui.theme", "x", true).is_err());
    }

    #[test]
    fn the_file_reads_back() {
        let mut p = Prefs::default();
        p.set("edit.arrow.mode", &json!("extended")).unwrap();
        p.set("edit.automation.locks-clips", &json!(false)).unwrap();
        p.set("audio.record-offset", &json!("2")).unwrap();
        p.set("midi.inputs.Port. 1", &json!("off")).unwrap();
        p.set("plugins.process.Serum 2", &json!("dedicated"))
            .unwrap();
        p.edit_list("plugins.clap.paths", "C:\\a \"b\"", true)
            .unwrap();
        let text = p.to_toml();
        assert!(
            text.contains("[edit]\narrow.mode = \"extended\"\nautomation.locks-clips = false\n"),
            "{text}"
        );
        assert!(text.contains("inputs.\"Port. 1\" = false"), "{text}");
        let mut problems = Vec::new();
        let back = Prefs::from_toml(&text.parse().unwrap(), &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, p);
        assert_eq!(back.to_toml(), text);
    }

    #[test]
    fn a_file_from_before_the_record_panel_reads_with_its_defaults() {
        let t: toml::Table = "[audio]\ncount-in = 2\n[edit.record]\ninput-quantize = true\n"
            .parse()
            .unwrap();
        let mut problems = Vec::new();
        let p = Prefs::from_toml(&t, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(p.number("audio.count-in"), 2.0);
        assert!(p.bool("edit.record.input-quantize"));
        assert!(!p.bool("record.punch"));
        assert!(!p.bool("record.preroll"));
        assert_eq!(p.number("record.preroll-bars"), 1.0);
        assert!(!p.bool("record.postroll"));
        assert_eq!(p.number("record.postroll-bars"), 1.0);
        assert_eq!(p.text("edit.record.input-quantize-value"), "1/16");
    }

    #[test]
    fn the_record_panel_values_survive_a_save_and_a_reopen() {
        let mut p = Prefs::default();
        p.set("record.punch", &json!(true)).unwrap();
        p.set("record.preroll", &json!(true)).unwrap();
        p.set("record.preroll-bars", &json!(2)).unwrap();
        p.set("record.postroll", &json!(true)).unwrap();
        p.set("record.postroll-bars", &json!(3)).unwrap();
        p.set("audio.count-in", &json!(1)).unwrap();
        p.set("edit.record.input-quantize", &json!(true)).unwrap();
        p.set("edit.record.input-quantize-value", &json!("1/8T"))
            .unwrap();
        let text = p.to_toml();
        let mut problems = Vec::new();
        let back = Prefs::from_toml(&text.parse().unwrap(), &mut problems);
        assert!(problems.is_empty(), "{problems:?}\n{text}");
        assert_eq!(back, p);
        assert_eq!(back.text("edit.record.input-quantize-value"), "1/8T");
        assert!(p.set("record.preroll-bars", &json!(0)).is_err());
        assert!(
            p.set("edit.record.input-quantize-value", &json!("1/32"))
                .is_err()
        );
    }

    #[test]
    fn a_bad_line_is_skipped_and_named() {
        let t: toml::Table = "[ui]\ntheme = \"pink\"\n[edit]\nripple = true\n"
            .parse()
            .unwrap();
        let mut problems = Vec::new();
        let p = Prefs::from_toml(&t, &mut problems);
        assert_eq!(problems.len(), 1);
        assert!(p.bool("edit.ripple"));
    }
}
