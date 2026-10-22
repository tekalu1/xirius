//! The one line of an operation (AI-025, AI-032): the `xirius …` the command is, what it
//! changed, and the unit an undo takes back. Every row of the history shows the same lines,
//! whether the person or the AI ran it.

use serde_json::Value;

use crate::song::HistoryItem;

/// The command as a person types it: `xirius track.add --kind audio`.
pub fn cli(command: &str, args: &Value) -> String {
    let mut line = format!("xirius {command}");
    let Some(map) = args.as_object() else {
        return line;
    };
    for (key, value) in map {
        match value {
            Value::Null | Value::Bool(false) => {}
            Value::Bool(true) => line.push_str(&format!(" --{key}")),
            other => line.push_str(&format!(" --{key} {}", word(other))),
        }
    }
    line
}

/// One value on the command line: a bare word when it can be, otherwise quoted.
fn word(value: &Value) -> String {
    let text = match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| !c.is_whitespace() && c != '"' && c != '\'');
    if plain || !matches!(value, Value::String(_)) {
        text
    } else {
        format!("\"{}\"", text.replace('"', "\\\""))
    }
}

/// What the detail of an operation shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpLine {
    /// The command line, broken to fit; the first line carries the prompt.
    pub command: Vec<String>,
    /// What it changed, then the undo unit.
    pub result: Vec<String>,
}

impl OpLine {
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.command
            .iter()
            .chain(self.result.iter())
            .map(String::as_str)
    }
}

/// The prompt of a command line: `$` for the person, `AI mcp ›` for the AI.
pub fn prompt(ai: bool) -> &'static str {
    if ai { "AI mcp ›" } else { "$" }
}

/// The lines of `item`, each at most `max_w` wide as `fits` measures it.
pub fn of(item: &HistoryItem, max_w: f32, fits: impl Fn(&str) -> f32) -> OpLine {
    let full = format!("{} {}", prompt(item.ai), cli(&item.command, &item.args));
    let command = wrap(&full, max_w, &fits);
    let mut result = Vec::new();
    if !item.summary.is_empty() {
        result.push(format!("→ {}", item.summary));
    }
    // A change of the AI's permission is not a step: there is nothing to take back.
    if !item.note {
        result.push("取り消し 1 操作".to_string());
    }
    OpLine { command, result }
}

/// Breaks `text` at spaces where the next word would pass `max_w`; a word wider than the line
/// is broken by characters.
pub(crate) fn wrap(text: &str, max_w: f32, fits: &impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let joined = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if fits(&joined) <= max_w {
            line = joined;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        for c in word.chars() {
            let mut next = line.clone();
            next.push(c);
            if fits(&next) > max_w && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn item(command: &str, args: Value, ai: bool, note: bool) -> HistoryItem {
        HistoryItem {
            n: 3,
            label: command.to_string(),
            command: command.to_string(),
            ai,
            current: false,
            undone: false,
            args,
            summary: "ノート 12 件".to_string(),
            at_ms: 0,
            note,
        }
    }

    fn width(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn the_command_line_is_what_the_cli_takes() {
        assert_eq!(
            cli(
                "note.quantize",
                &json!({"clip": "melody", "grid": "1/16", "swing": null, "all": true})
            ),
            "xirius note.quantize --clip melody --grid 1/16 --all"
        );
        assert_eq!(cli("history.undo", &Value::Null), "xirius history.undo");
        assert_eq!(
            cli("track.rename", &json!({"name": "Lead Synth"})),
            "xirius track.rename --name \"Lead Synth\""
        );
        assert_eq!(
            cli("note.add", &json!({"notes": [1, 2]})),
            "xirius note.add --notes [1,2]"
        );
    }

    #[test]
    fn a_person_and_the_ai_get_the_same_lines() {
        let args = json!({"clip": "melody"});
        let person = of(
            &item("note.quantize", args.clone(), false, false),
            99.0,
            width,
        );
        let ai = of(&item("note.quantize", args, true, false), 99.0, width);
        assert_eq!(person.command, ["$ xirius note.quantize --clip melody"]);
        assert_eq!(ai.command, ["AI mcp › xirius note.quantize --clip melody"]);
        assert_eq!(person.result, ai.result);
        assert_eq!(person.result, ["→ ノート 12 件", "取り消し 1 操作"]);
    }

    #[test]
    fn a_permission_change_has_no_undo_unit() {
        let line = of(
            &item("ai.permission.set", json!({"level": "edit"}), false, true),
            99.0,
            width,
        );
        assert_eq!(line.result, ["→ ノート 12 件"]);
    }

    #[test]
    fn a_long_line_is_broken_at_spaces() {
        let it = item(
            "note.quantize",
            json!({"clip": "melody", "grid": "1/16"}),
            false,
            false,
        );
        let line = of(&it, 30.0, width);
        assert!(line.command.len() >= 2);
        assert!(line.command.iter().all(|l| width(l) <= 30.0));
        assert_eq!(
            line.command.join(" "),
            "$ xirius note.quantize --clip melody --grid 1/16"
        );
    }
}
