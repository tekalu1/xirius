//! `macro.*`: the macros and their keys (KEY-015).
//!
//! `macro.set`, `macro.rename` and `macro.remove` change `macros.toml`, and with it the keys of
//! the macro in `keymap.toml`: a key of a macro is the row `macro.run --name <name>` in the
//! context of the macro's first command. `macro.run` leaves its steps for the session
//! ([`Extension::take_edits`](xirius_core::extension::Extension::take_edits)), which makes them
//! one step of the history.

use serde_json::{Map, Value, json};
use xirius_core::command::{Args, Handler, Registry};
use xirius_core::error::{CmdError, CmdResult, ErrorCode};
use xirius_core::extension::ExtContext;

use crate::catalog::ENTRIES;
use crate::cmdline;
use crate::ext::{check_binding, command_label};
use crate::keymap::Keymap;
use crate::macros::{self, Macros, check_name, name_of_line, run_line};
use crate::store::Settings;

/// What a macro command did to the files.
pub(crate) struct Done {
    pub message: String,
    pub data: Value,
    /// The keymap changed too.
    pub keys: bool,
}

fn invalid(e: String) -> CmdError {
    CmdError::invalid(e)
}

/// The context a macro's keys are in: that of its first command, `global` for none.
pub fn context_of(steps: &[String]) -> &'static str {
    let Some(first) = steps.first() else {
        return "global";
    };
    let name = first.split_whitespace().next().unwrap_or(first);
    let word = |c: &str| c.split_whitespace().next().unwrap_or("").to_string();
    ENTRIES
        .iter()
        .find(|e| e.command == first.as_str())
        .or_else(|| ENTRIES.iter().find(|e| word(e.command) == name))
        .map(|e| e.context)
        .unwrap_or("global")
}

/// A step of a macro: a command of the table that changes the song or only the selection.
/// Gives the line back in order.
pub fn check_step(line: &str, registry: &Registry) -> Result<String, String> {
    let line = check_binding(line.trim(), registry)?;
    let name = line.split_whitespace().next().unwrap_or("");
    let def = registry
        .get(name)
        .ok_or_else(|| format!("「{name}」というコマンドはありません"))?;
    match def.handler {
        Handler::Edit(_) | Handler::Read(_) => Ok(line),
        Handler::Session => Err(format!(
            "「{}」はマクロの手順にできません（曲を編集するコマンドだけです）",
            registry.label(name).unwrap_or(name)
        )),
    }
}

/// A command a macro can take as a step, as the catalog lists it.
pub struct Candidate {
    pub context: &'static str,
    pub command: &'static str,
    pub label: String,
}

/// The catalog's commands that are valid steps, in catalog order.
pub fn candidates(registry: &Registry) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for e in ENTRIES {
        if out.iter().any(|c| c.command == e.command) || check_step(e.command, registry).is_err() {
            continue;
        }
        out.push(Candidate {
            context: e.context,
            command: e.command,
            label: command_label(e.command, registry),
        });
    }
    out
}

fn steps_of(args: &Args, registry: &Registry) -> CmdResult<Option<Vec<String>>> {
    let Some(list) = args.0.get("steps") else {
        return Ok(None);
    };
    let items = list.as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let line = item
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("{} 番目の手順に command がありません", i + 1)))?;
        out.push(
            check_step(line, registry)
                .map_err(|e| invalid(format!("{} 番目の手順: {e}", i + 1)))?,
        );
    }
    Ok(Some(out))
}

/// Where the macro's keys are now: (context, key).
fn keys_of(k: &Keymap, name: &str) -> Vec<(String, crate::chord::Chord)> {
    k.bindings()
        .into_iter()
        .filter(|(_, cmds)| {
            cmds.iter()
                .any(|c| name_of_line(c).as_deref() == Some(name))
        })
        .map(|(key, _)| key)
        .collect()
}

/// Changes what the macro's key rows say: `f` gives the command line to put in place of a row
/// of the macro (`None` takes the row away), and the context to move it to.
fn rewrite_keys(
    k: &mut Keymap,
    name: &str,
    f: impl Fn(&str) -> Option<(String, Option<&'static str>)>,
) -> bool {
    let mut b = k.bindings();
    let mut moved = Vec::new();
    let mut changed = false;
    for ((ctx, chord), cmds) in b.iter_mut() {
        let mut keep = Vec::new();
        for c in cmds.drain(..) {
            if name_of_line(&c).as_deref() != Some(name) {
                keep.push(c);
                continue;
            }
            changed = true;
            if let Some((line, to)) = f(&c) {
                match to {
                    Some(to) if to != ctx => moved.push((to.to_string(), chord.clone(), line)),
                    _ => keep.push(line),
                }
            }
        }
        *cmds = keep;
    }
    b.retain(|_, v| !v.is_empty());
    for (ctx, chord, line) in moved {
        let v = b.entry((ctx, chord)).or_default();
        if !v.contains(&line) {
            v.push(line);
        }
    }
    if changed {
        k.set_bindings(&b);
    }
    changed
}

fn row_json(m: &macros::Macro, s: &Settings, registry: &Registry) -> Value {
    let os = s.os;
    let keys: Vec<Value> = keys_of(&s.keymap, &m.name)
        .into_iter()
        .map(|(ctx, chord)| json!({"context": ctx, "key": chord.to_string(), "shown": chord.display(os)}))
        .collect();
    json!({
        "name": m.name,
        "context": context_of(&m.steps),
        "steps": m.steps.iter().map(|c| json!({
            "command": c,
            "label": command_label(c, registry),
        })).collect::<Vec<_>>(),
        "keys": keys,
    })
}

pub(crate) fn rows(s: &Settings, registry: &Registry) -> Value {
    Value::Array(
        s.macros
            .list()
            .iter()
            .map(|m| row_json(m, s, registry))
            .collect(),
    )
}

pub(crate) fn list(s: &Settings, registry: &Registry) -> (String, Value) {
    let text = if s.macros.is_empty() {
        "マクロはありません".to_string()
    } else {
        s.macros
            .list()
            .iter()
            .map(|m| {
                let keys: Vec<String> = keys_of(&s.keymap, &m.name)
                    .iter()
                    .map(|(_, c)| c.display(s.os))
                    .collect();
                let keys = if keys.is_empty() {
                    String::new()
                } else {
                    format!("  [{}]", keys.join(" "))
                };
                let steps: Vec<String> =
                    m.steps.iter().map(|c| command_label(c, registry)).collect();
                format!("{}{keys}: {}", m.name, steps.join(" → "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    (text, json!({"macros": rows(s, registry)}))
}

pub(crate) fn set(s: &mut Settings, cx: &ExtContext<'_>, args: &Args) -> CmdResult<Done> {
    let name = check_name(args.str("name")?).map_err(invalid)?;
    let steps = steps_of(args, cx.registry)?;
    let steps = match steps {
        Some(v) => v,
        None => s
            .macros
            .get(&name)
            .map(|m| m.steps.clone())
            .unwrap_or_default(),
    };
    let ctx = context_of(&steps);
    let new = s.macros.set(&name, steps.clone());
    // The keys follow the first command.
    let line = run_line(&name);
    let keys = rewrite_keys(&mut s.keymap, &name, |_| Some((line.clone(), Some(ctx))));
    Ok(Done {
        message: if new {
            format!("マクロ「{name}」を作りました（{} 手順）", steps.len())
        } else {
            format!("マクロ「{name}」を保存しました（{} 手順）", steps.len())
        },
        data: json!({"name": name, "new": new}),
        keys,
    })
}

pub(crate) fn rename(s: &mut Settings, args: &Args) -> CmdResult<Done> {
    let from = args.str("name")?.trim().to_string();
    let to = check_name(args.str("new-name")?).map_err(invalid)?;
    s.macros.rename(&from, &to).map_err(|e| {
        if s.macros.get(&from).is_none() {
            CmdError::not_found(e)
        } else {
            invalid(e)
        }
    })?;
    let line = run_line(&to);
    let keys = rewrite_keys(&mut s.keymap, &from, |_| Some((line.clone(), None)));
    Ok(Done {
        message: format!("マクロ「{from}」の名前を「{to}」にしました"),
        data: json!({"name": to}),
        keys,
    })
}

pub(crate) fn remove(s: &mut Settings, args: &Args) -> CmdResult<Done> {
    let name = args.str("name")?.trim().to_string();
    if !s.macros.remove(&name) {
        return Err(CmdError::not_found(format!(
            "「{name}」というマクロはありません"
        )));
    }
    let keys = rewrite_keys(&mut s.keymap, &name, |_| None);
    Ok(Done {
        message: format!("マクロ「{name}」を消しました"),
        data: json!({"name": name}),
        keys,
    })
}

/// A command a macro run leaves for the session: its name and its arguments.
pub(crate) type Edit = (String, Map<String, Value>);

/// The commands a run of the macro leaves for the session.
pub(crate) fn plan_run(
    macros: &Macros,
    cx: &ExtContext<'_>,
    args: &Args,
) -> CmdResult<(String, Vec<Edit>)> {
    let name = args.str("name")?.trim();
    let m = macros
        .get(name)
        .ok_or_else(|| CmdError::not_found(format!("「{name}」というマクロはありません")))?;
    if m.steps.is_empty() {
        return Err(CmdError::new(
            ErrorCode::NothingToDo,
            format!("マクロ「{name}」に手順がありません"),
        ));
    }
    let mut edits = Vec::new();
    for (i, line) in m.steps.iter().enumerate() {
        check_step(line, cx.registry)
            .map_err(|e| invalid(format!("マクロ「{name}」の {} 番目の手順: {e}", i + 1)))?;
        let (cmd, raw) = cmdline::parse(line, Some(cx.registry)).map_err(invalid)?;
        edits.push((cmd, raw));
    }
    Ok((
        format!("マクロ「{name}」を実行しました（{} 手順）", edits.len()),
        edits,
    ))
}
