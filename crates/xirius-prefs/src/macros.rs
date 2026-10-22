//! The macros: `macros.toml` (KEY-015).
//!
//! A macro is a name and the command lines it runs in order. It runs from the shortcut list, the
//! palette, the command line and the MCP tools as `macro.run --name <name>`; a key given to it is
//! a row of `keymap.toml` whose command is that line, in the context of the macro's first command.
//! The steps of one run are taken back with one undo.

use serde_json::{Value, json};

use crate::tomlw;

/// The longest name; the list shows it on one line.
pub const NAME_MAX: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Macro {
    pub name: String,
    /// Command lines as the command line takes them (`note.quantize --grid 1/16`).
    pub steps: Vec<String>,
}

/// The command a key of the macro runs, written as a binding is written once checked (the name
/// is quoted only when it has a space).
pub fn run_line(name: &str) -> String {
    if name.contains(char::is_whitespace) {
        format!("macro.run --name \"{name}\"")
    } else {
        format!("macro.run --name {name}")
    }
}

/// The macro's name when `command` is a line of `macro.run`.
pub fn name_of_line(command: &str) -> Option<String> {
    let words = crate::cmdline::words(command).ok()?;
    if words.first().map(String::as_str) != Some("macro.run") {
        return None;
    }
    let at = words.iter().position(|w| w == "--name")?;
    words.get(at + 1).cloned()
}

/// Checks a macro's name: not empty, not too long, one line and no quotation mark (the key's
/// command line quotes it).
pub fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("マクロの名前がありません".into());
    }
    if name.chars().count() > NAME_MAX {
        return Err(format!("マクロの名前は {NAME_MAX} 文字までです"));
    }
    if name.contains(['"', '\'', '\n', '\r']) || name.starts_with('-') {
        return Err("マクロの名前に引用符・改行・先頭のハイフンは使えません".into());
    }
    Ok(name.to_string())
}

impl Macro {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "steps": self.steps.iter().map(|s| json!({"command": s})).collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Macros {
    list: Vec<Macro>,
}

impl Macros {
    pub fn list(&self) -> &[Macro] {
        &self.list
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&Macro> {
        self.list.iter().find(|m| m.name == name)
    }

    /// Makes the macro, or replaces the steps of the one with this name (it keeps its place in
    /// the list). True when it was new.
    pub fn set(&mut self, name: &str, steps: Vec<String>) -> bool {
        match self.list.iter_mut().find(|m| m.name == name) {
            Some(m) => {
                m.steps = steps;
                false
            }
            None => {
                self.list.push(Macro {
                    name: name.into(),
                    steps,
                });
                true
            }
        }
    }

    /// Gives the macro another name. Errors when there is none of the first or one of the second.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        if from != to && self.get(to).is_some() {
            return Err(format!("「{to}」というマクロはもうあります"));
        }
        let m = self
            .list
            .iter_mut()
            .find(|m| m.name == from)
            .ok_or_else(|| format!("「{from}」というマクロはありません"))?;
        m.name = to.into();
        Ok(())
    }

    /// Removes the macro. False when there was none.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|m| m.name != name);
        self.list.len() != before
    }

    pub fn to_json(&self) -> Value {
        Value::Array(self.list.iter().map(Macro::to_json).collect())
    }

    pub fn from_toml(t: &toml::Table, problems: &mut Vec<String>) -> Macros {
        let mut out = Macros::default();
        let Some(list) = t.get("macro") else {
            return out;
        };
        let Some(list) = list.as_array() else {
            problems.push("macros.toml: macro は [[macro]] の並びです".into());
            return out;
        };
        for (i, item) in list.iter().enumerate() {
            let name = item
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| "name がありません".to_string())
                .and_then(check_name);
            let steps: Result<Vec<String>, String> = match item.get("steps") {
                None => Ok(Vec::new()),
                Some(v) => v
                    .as_array()
                    .ok_or_else(|| "steps は文字列の並びです".to_string())
                    .and_then(|a| {
                        a.iter()
                            .map(|s| {
                                s.as_str()
                                    .map(String::from)
                                    .ok_or_else(|| "steps は文字列の並びです".to_string())
                            })
                            .collect()
                    }),
            };
            match (name, steps) {
                (Ok(name), Ok(steps)) if out.get(&name).is_none() => {
                    out.set(&name, steps);
                }
                (Ok(name), Ok(_)) => problems.push(format!(
                    "macros.toml: {} 番目のマクロ: 「{name}」が重なっています",
                    i + 1
                )),
                (Err(e), _) | (_, Err(e)) => {
                    problems.push(format!("macros.toml: {} 番目のマクロ: {e}", i + 1));
                }
            }
        }
        out
    }

    pub fn to_toml(&self) -> String {
        let mut s = String::from("# macros.toml（マクロ）\n");
        for m in &self.list {
            s.push_str(&format!(
                "\n[[macro]]\nname = {}\nsteps = [",
                tomlw::string(&m.name)
            ));
            if m.steps.is_empty() {
                s.push_str("]\n");
                continue;
            }
            s.push('\n');
            for step in &m.steps {
                s.push_str(&format!("  {},\n", tomlw::string(step)));
            }
            s.push_str("]\n");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn set_replaces_the_steps_and_keeps_the_place() {
        let mut m = Macros::default();
        assert!(m.set("a", steps(&["note.mute"])));
        assert!(m.set("b", steps(&[])));
        assert!(!m.set("a", steps(&["note.delete", "note.mute"])));
        assert_eq!(m.list()[0].name, "a");
        assert_eq!(m.list()[0].steps.len(), 2);
    }

    #[test]
    fn rename_refuses_a_name_in_use_and_a_missing_one() {
        let mut m = Macros::default();
        m.set("a", steps(&[]));
        m.set("b", steps(&[]));
        assert!(m.rename("a", "b").is_err());
        assert!(m.rename("x", "y").is_err());
        m.rename("a", "c").unwrap();
        assert_eq!(m.list()[0].name, "c");
        m.rename("c", "c").unwrap();
    }

    #[test]
    fn toml_round_trips_with_quotes_and_japanese() {
        let mut m = Macros::default();
        m.set(
            "打ち込みを整える",
            steps(&["note.quantize --grid 1/16", "note.transpose --by \"1\""]),
        );
        m.set("空", steps(&[]));
        let table: toml::Table = m.to_toml().parse().unwrap();
        let mut problems = Vec::new();
        let back = Macros::from_toml(&table, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, m);
    }

    #[test]
    fn a_broken_macro_is_reported_and_the_rest_is_read() {
        let table: toml::Table = "[[macro]]\nname = \"a\"\nsteps = [1]\n[[macro]]\nname = \"b\"\n[[macro]]\nname = \"b\"\n[[macro]]\nsteps = []\n"
            .parse()
            .unwrap();
        let mut problems = Vec::new();
        let m = Macros::from_toml(&table, &mut problems);
        assert_eq!(m.list().len(), 1);
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    #[test]
    fn the_key_line_gives_the_name_back() {
        let line = run_line("打ち込みを整える");
        assert_eq!(name_of_line(&line).as_deref(), Some("打ち込みを整える"));
        assert_eq!(name_of_line("note.mute"), None);
    }

    #[test]
    fn names_are_checked() {
        assert!(check_name("  ").is_err());
        assert!(check_name("a\"b").is_err());
        assert!(check_name(&"あ".repeat(41)).is_err());
        assert_eq!(check_name(" 整える ").unwrap(), "整える");
    }
}
