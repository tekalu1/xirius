//! The extension that answers `prefs.*`, `keymap.*` and `mousemap.*` (SET-001, KEY-003).
//!
//! Each run reads the files again if another program changed them, makes the change on the
//! settings in hand and writes the file at once. The commands do not touch the song and do not
//! enter its history (KEY-023).

use serde_json::{Value, json};
use xirius_core::command::{Args, Author, CommandDef, Output, Registry};
use xirius_core::error::{CmdError, CmdResult, ErrorCode};
use xirius_core::extension::{ExtContext, Extension};
use xirius_core::id::TrackId;

use crate::catalog::{self, CONTEXTS, ENTRIES};
use crate::chord::{Chord, Os};
use crate::cmdline;
use crate::keymap::{Issue, Keymap, Others, row_json};
use crate::macro_ext;
use crate::midimap::{Message, Midimap, Target};
use crate::store::{File, Settings, Shared};
use crate::{export, import};

pub const NAMES: &[&str] = &[
    "prefs.get",
    "prefs.set",
    "prefs.reset",
    "record.punch",
    "record.preroll",
    "record.postroll",
    "record.count-in",
    "record.input-quantize",
    "keymap.get",
    "keymap.which",
    "keymap.set",
    "keymap.unset",
    "keymap.reset",
    "keymap.preset",
    "keymap.conflicts",
    "keymap.import",
    "keymap.export",
    "mousemap.get",
    "mousemap.set",
    "mousemap.unset",
    "mousemap.reset",
    "midimap.list",
    "midimap.set",
    "midimap.unset",
    "macro.list",
    "macro.set",
    "macro.rename",
    "macro.remove",
    "macro.run",
    "tool.alt",
    "mousemap.conflicts",
    "edit.ripple",
];

/// Whether `name` is one of the settings' commands (they need no song). `macro.run` works on a
/// song, so it is not one.
pub fn is_settings_command(name: &str) -> bool {
    name != "macro.run" && NAMES.contains(&name)
}

pub struct PrefsExt {
    shared: Shared,
    /// What the last `macro.run` leaves for the session.
    pending: Vec<(String, serde_json::Map<String, Value>)>,
}

impl PrefsExt {
    pub fn new(shared: Shared) -> PrefsExt {
        PrefsExt {
            shared,
            pending: Vec::new(),
        }
    }
}

/// The holds that cover a click or drag on the same modifier (the hold wins while it is down).
fn mouse_conflicts(m: &crate::mousemap::Mousemap) -> Vec<Value> {
    m.warnings()
        .into_iter()
        .map(|(c, m, t)| json!({"kind": "warning", "context": c, "modifier": m, "hold": t}))
        .collect()
}

fn invalid(e: String) -> CmdError {
    CmdError::invalid(e)
}

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Settings> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// Checks a binding's command: a command of the table (its arguments too) or an action of the
/// screen. Gives it back with its spaces put in order.
pub fn check_binding(command: &str, registry: &Registry) -> Result<String, String> {
    let (name, args) = cmdline::parse(command, Some(registry))?;
    let def = registry
        .get(&name)
        .ok_or_else(|| format!("「{name}」というコマンドはありません"))?;
    // Selection and pointer arguments are supplied when the binding is invoked.
    // Check every argument that was specified without demanding those implicit values.
    let mut binding_def = def.clone();
    for arg in &mut binding_def.args {
        arg.required = false;
    }
    registry
        .normalize(&binding_def, &args)
        .map_err(|e| e.message)?;
    let words = cmdline::words(command)?;
    Ok(words
        .iter()
        .map(|w| {
            if w.is_empty() || w.contains(char::is_whitespace) {
                format!("\"{w}\"")
            } else {
                w.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" "))
}

/// The name the screen shows for a binding's command.
pub fn command_label(command: &str, registry: &Registry) -> String {
    if let Some(name) = crate::macros::name_of_line(command) {
        return name;
    }
    if let Some(e) = ENTRIES.iter().find(|e| e.command == command)
        && let Some(l) = e.label
    {
        return l.to_string();
    }
    let name = command.split_whitespace().next().unwrap_or(command);
    match registry.label(name) {
        Some(l) if name == command => l.to_string(),
        Some(l) => format!("{l}（{}）", command[name.len()..].trim()),
        None => command.to_string(),
    }
}

/// The context a command is bound in when none is given: the catalog's, else `global`.
pub fn default_context(command: &str) -> &'static str {
    ENTRIES
        .iter()
        .find(|e| e.command == command)
        .map(|e| e.context)
        .unwrap_or("global")
}

fn chord(args: &Args, name: &str, os: Os) -> CmdResult<Chord> {
    Chord::parse(args.str(name)?, os).map_err(invalid)
}

fn issues_json(k: &Keymap, os: Os) -> Value {
    Value::Array(k.conflicts(os).iter().map(|i| i.to_json(os)).collect())
}

fn rows_json(k: &Keymap, os: Os, ctx: Option<&str>, changed_only: bool) -> Value {
    Value::Array(
        k.rows()
            .iter()
            .filter(|r| ctx.is_none_or(|c| r.context == c))
            .filter(|r| !changed_only || r.changed())
            .map(|r| row_json(r, os))
            .collect(),
    )
}

/// A target of `midimap.set`: a part of the song, or a command of the table (checked as a
/// binding is).
fn checked_target(text: &str, cx: &ExtContext<'_>) -> CmdResult<Target> {
    let target = Target::parse(text).map_err(invalid)?;
    match &target {
        Target::Volume(id) | Target::Pan(id) => {
            let found = id
                .parse::<TrackId>()
                .ok()
                .is_some_and(|t| cx.project.tracks.contains_key(&t));
            if !found {
                return Err(CmdError::not_found(format!(
                    "「{id}」というトラックはありません"
                )));
            }
            Ok(target)
        }
        Target::Command(c) => check_binding(c, cx.registry)
            .map(Target::Command)
            .map_err(invalid),
    }
}

/// The name the screen shows for a target: the part's name and the value, or the command's.
pub fn target_label(target: &Target, cx: &ExtContext<'_>) -> String {
    let part = |id: &str| {
        id.parse::<TrackId>()
            .ok()
            .and_then(|t| cx.project.tracks.get(&t))
            .map(|t| t.name.clone())
    };
    match target {
        Target::Volume(id) => format!("{} · ボリューム", part(id).unwrap_or_else(|| id.clone())),
        Target::Pan(id) => format!("{} · パン", part(id).unwrap_or_else(|| id.clone())),
        Target::Command(c) => command_label(c, cx.registry),
    }
}

fn midimap_rows(m: &Midimap, cx: &ExtContext<'_>) -> Value {
    Value::Array(
        m.rows()
            .iter()
            .map(|r| {
                let mut v = r.to_json();
                v["label"] = json!(target_label(&r.target, cx));
                v["missing"] = json!(r.target.track().is_some_and(|id| {
                    !id.parse::<TrackId>()
                        .ok()
                        .is_some_and(|t| cx.project.tracks.contains_key(&t))
                }));
                v
            })
            .collect(),
    )
}

fn midimap_lines(m: &Midimap, cx: &ExtContext<'_>) -> String {
    if m.is_empty() {
        return "コントローラーの割り当てはありません".into();
    }
    m.rows()
        .iter()
        .map(|r| {
            format!(
                "{} {} → {}",
                r.port.as_deref().unwrap_or("（すべてのポート）"),
                r.message.label(),
                target_label(&r.target, cx)
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

fn switch_word(on: bool) -> &'static str {
    if on { "オン" } else { "オフ" }
}

/// `record.punch` and the record panel's other switches (REC-007, REC-009). They are
/// preferences, so the panel, the keys, the CLI and the MCP tools change the same values. With
/// neither `--on` nor `--off` nor a value, the switch flips. The output says whether anything was
/// changed (and so has to be saved).
fn record_setting(
    prefs: &mut crate::prefs::Prefs,
    name: &str,
    args: &Args,
) -> CmdResult<(Output, bool)> {
    let toggle = args.toggle()?;
    let bars = args.opt_i64("bars");
    let value = args.opt_str("value");
    let asked = toggle.is_some() || bars.is_some() || value.is_some();
    let set = |p: &mut crate::prefs::Prefs, key: &str, v: Value| {
        p.set(key, &v).map(|_| ()).map_err(invalid)
    };
    if name == "record.postroll" && asked && !prefs.bool("record.punch") {
        return Err(CmdError::conflict(
            "オートパンチが切れているため、ポストロールは使えません",
        ));
    }
    let (label, on, count, text): (&str, bool, Option<i64>, Option<String>) = match name {
        "record.punch" => {
            let on = toggle.unwrap_or(!prefs.bool("record.punch"));
            set(prefs, "record.punch", json!(on))?;
            ("オートパンチ", on, None, None)
        }
        "edit.ripple" => {
            let on = toggle.unwrap_or(!prefs.bool("edit.ripple"));
            set(prefs, "edit.ripple", json!(on))?;
            ("リップル編集", on, None, None)
        }
        "record.preroll" | "record.postroll" => {
            let (key, bars_key, label) = if name == "record.preroll" {
                ("record.preroll", "record.preroll-bars", "プリロール")
            } else {
                ("record.postroll", "record.postroll-bars", "ポストロール")
            };
            let flip = !asked;
            if let Some(n) = bars {
                set(prefs, bars_key, json!(n))?;
            }
            if toggle.is_some() || flip {
                set(prefs, key, json!(toggle.unwrap_or(!prefs.bool(key))))?;
            }
            (
                label,
                prefs.bool(key),
                Some(prefs.number(bars_key) as i64),
                None,
            )
        }
        "record.count-in" => {
            let now = prefs.number("audio.count-in") as i64;
            let next = match (toggle, bars) {
                (Some(false), _) => 0,
                (_, Some(n)) => n,
                (Some(true), None) => now.max(1),
                (None, None) => i64::from(now == 0),
            };
            if next != now {
                set(prefs, "audio.count-in", json!(next))?;
            }
            ("プリカウント", next > 0, (next > 0).then_some(next), None)
        }
        _ => {
            const KEY: &str = "edit.record.input-quantize";
            if let Some(v) = value {
                set(prefs, "edit.record.input-quantize-value", json!(v))?;
            }
            if toggle.is_some() || !asked {
                set(prefs, KEY, json!(toggle.unwrap_or(!prefs.bool(KEY))))?;
            }
            (
                "入力クオンタイズ",
                prefs.bool(KEY),
                None,
                Some(prefs.text("edit.record.input-quantize-value")),
            )
        }
    };
    let mut message = format!("{label}: {}", switch_word(on));
    if let Some(n) = count {
        message.push_str(&format!("（{n} 小節）"));
    }
    if let Some(t) = &text {
        message.push_str(&format!("（{t}）"));
    }
    let data = json!({"on": on, "bars": count, "value": text});
    Ok((Output::with(message, data), true))
}

fn shown_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Extension for PrefsExt {
    fn answers(&self, name: &str) -> bool {
        NAMES.contains(&name)
    }

    fn take_edits(&mut self) -> Vec<(String, serde_json::Map<String, Value>)> {
        std::mem::take(&mut self.pending)
    }

    fn groups_edits(&self, name: &str) -> bool {
        name == "macro.run"
    }

    /// EDT-135: the clip edits that can ripple do so when `edit.ripple` is on, and carry the
    /// automation under the clips they move when `edit.automation.follows-clips` is on.
    /// EDT-137 / EDT-138: the note edits trim overlapping notes when `edit.note.overlap` is
    /// `trim`, the clip edits cut away the clips under theirs when `edit.clip.no-overlap` is on.
    fn fill_args(&self, def: &CommandDef, raw: &mut serde_json::Map<String, Value>) {
        let has = |name: &str| def.args.iter().any(|a| a.name == name);
        let missing = |raw: &serde_json::Map<String, Value>, name: &str| {
            has(name) && raw.get(name).is_none_or(Value::is_null)
        };
        if !["ripple", "automation", "overlap", "no-overlap"]
            .iter()
            .any(|n| missing(raw, n))
        {
            return;
        }
        let snapshot = Settings::command_snapshot(&self.shared);
        let prefs = &snapshot.prefs;
        if missing(raw, "overlap") {
            raw.insert("overlap".into(), json!(prefs.text("edit.note.overlap")));
        }
        if missing(raw, "no-overlap") {
            raw.insert(
                "no-overlap".into(),
                json!(prefs.bool("edit.clip.no-overlap")),
            );
        }
        if !has("ripple") {
            return;
        }
        if raw.get("ripple").is_none_or(Value::is_null) {
            raw.insert("ripple".into(), json!(prefs.bool("edit.ripple")));
        }
        if has("automation") && raw.get("automation").is_none_or(Value::is_null) {
            let follow = prefs.bool("edit.automation.follows-clips");
            raw.insert(
                "automation".into(),
                json!(if follow { "follow" } else { "stay" }),
            );
        }
    }

    fn run(
        &mut self,
        cx: &ExtContext<'_>,
        def: &CommandDef,
        args: &Args,
        dry: bool,
    ) -> CmdResult<Output> {
        self.pending.clear();
        let mut s = Settings::command_snapshot(&self.shared);
        // macro.run only plans validated memory-only edit/read handlers.
        if s.on_ui_thread() && def.name != "macro.run" {
            return Err(CmdError::conflict("設定の処理は裏で行う必要があります"));
        }
        let os = s.os;
        let human = matches!(cx.author, Author::Human);
        let mut keys_too = false;
        let (out, file) = match def.name {
            "prefs.get" => {
                let key = args.opt_str("key");
                if let Some(k) = key
                    && let Some(v) = s.prefs.get(k)
                {
                    return Ok(Output::with(
                        format!("{k} = {}", shown_value(&v)),
                        json!({k: v}),
                    ));
                }
                let all = s.prefs.list_all(key);
                if all.is_empty() {
                    return Err(CmdError::not_found(format!(
                        "「{}」という設定はありません",
                        key.unwrap_or("")
                    )));
                }
                let lines: Vec<String> = all
                    .iter()
                    .map(|(k, v, d)| {
                        let mark = if v != d { "  （変更）" } else { "" };
                        format!("{k} = {}{mark}", shown_value(v))
                    })
                    .collect();
                let data: serde_json::Map<String, Value> =
                    all.into_iter().map(|(k, v, _)| (k, v)).collect();
                return Ok(Output::with(lines.join("\n"), Value::Object(data)));
            }
            "prefs.set" => {
                let key = args.str("key")?;
                if !human && key.starts_with("ai.mcp.") {
                    return Err(CmdError::new(
                        ErrorCode::Denied,
                        "AI からは MCP の設定を変更できません",
                    ));
                }
                let value = args.opt_str("value");
                let v = match (args.flag("add"), args.flag("remove")) {
                    (true, true) => {
                        return Err(invalid("--add と --remove は同時に指定できません".into()));
                    }
                    (add, remove) if add || remove => {
                        let item = value.ok_or_else(|| invalid("--value が必要です".into()))?;
                        s.prefs.edit_list(key, item, add).map_err(invalid)?
                    }
                    _ => {
                        let v = value.ok_or_else(|| invalid("--value が必要です".into()))?;
                        s.prefs
                            .set(key, &Value::String(v.into()))
                            .map_err(invalid)?
                    }
                };
                (
                    Output::with(format!("{key} = {}", shown_value(&v)), json!({key: v})),
                    File::Prefs,
                )
            }
            "prefs.reset" => {
                // An AI does not change the MCP settings, by name or with the rest.
                let keep: Vec<(String, Value)> = if human {
                    Vec::new()
                } else {
                    s.prefs
                        .values
                        .iter()
                        .filter(|(k, _)| k.starts_with("ai.mcp."))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                };
                let before = s.prefs.values.len();
                s.prefs.reset(args.opt_str("key"));
                s.prefs.values.extend(keep);
                let n = before - s.prefs.values.len();
                (
                    Output::msg(format!("{n} 件を既定の値に戻しました")),
                    File::Prefs,
                )
            }
            "record.punch"
            | "record.preroll"
            | "record.postroll"
            | "record.count-in"
            | "record.input-quantize"
            | "edit.ripple" => {
                let (out, changed) = record_setting(&mut s.prefs, def.name, args)?;
                if !changed {
                    return Ok(out);
                }
                (out, File::Prefs)
            }
            "keymap.get" => {
                if args.opt_str("key").is_some() {
                    let chord = chord(args, "key", os)?;
                    let rows: Vec<Value> = s
                        .keymap
                        .rows()
                        .into_iter()
                        .filter(|r| {
                            r.keys.contains(&chord)
                                && args.opt_str("context").is_none_or(|ctx| ctx == r.context)
                        })
                        .map(|r| row_json(&r, os))
                        .collect();
                    return Ok(Output::with(
                        chord.display(os),
                        json!({"rows":rows,"conflicts":issues_json(&s.keymap,os)}),
                    ));
                }
                let ctx = args.opt_str("context");
                let changed = args.flag("changed");
                let rows = rows_json(&s.keymap, os, ctx, changed);
                let mut lines = vec![format!(
                    "プリセット: {}（変更した行 {}）",
                    s.keymap.preset,
                    s.keymap.changed_rows()
                )];
                for r in s.keymap.rows() {
                    if ctx.is_some_and(|c| r.context != c) || (changed && !r.changed()) {
                        continue;
                    }
                    let keys: Vec<String> = r.keys.iter().map(|k| k.display(os)).collect();
                    lines.push(format!(
                        "{:<8} {:<28} {}{}",
                        r.context,
                        r.command,
                        if keys.is_empty() {
                            "—".into()
                        } else {
                            keys.join(", ")
                        },
                        if r.changed() { "  （変更）" } else { "" }
                    ));
                }
                return Ok(Output::with(
                    lines.join("\n"),
                    json!({"preset": s.keymap.preset, "rows": rows,
                        "conflicts": issues_json(&s.keymap, os)}),
                ));
            }
            "keymap.which" => {
                let name = args.opt_str("command").or_else(|| {
                    args.opt_str("key")
                        .filter(|s| s.contains('.') && !s.contains('+'))
                });
                if let Some(command) = name {
                    let rows: Vec<Value> = s
                        .keymap
                        .rows()
                        .into_iter()
                        .filter(|r| {
                            r.command == command
                                && args.opt_str("context").is_none_or(|ctx| ctx == r.context)
                        })
                        .map(|r| row_json(&r, os))
                        .collect();
                    return Ok(Output::with(
                        command,
                        json!({"command":command,"rows":rows,"conflicts":issues_json(&s.keymap,os)}),
                    ));
                }
                let c = chord(args, "key", os)?;
                let ctx = args.opt_str("context").unwrap_or("roll");
                return Ok(match s.keymap.lookup(ctx, &c) {
                    Some((found, cmd)) => Output::with(
                        format!(
                            "{}（{found}）: {cmd} — {}",
                            c.display(os),
                            command_label(&cmd, cx.registry)
                        ),
                        json!({"key": c.to_string(), "context": found, "command": cmd}),
                    ),
                    None => Output::with(
                        format!("{} には何も割り当てられていません（{ctx}）", c.display(os)),
                        json!({"key": c.to_string(), "context": ctx, "command": null}),
                    ),
                });
            }
            "keymap.set" => {
                let c = chord(args, "key", os)?;
                let cmd = check_binding(args.str("command")?, cx.registry).map_err(invalid)?;
                let ctx = args
                    .opt_str("context")
                    .unwrap_or_else(|| default_context(&cmd));
                let from = match args.opt_str("from") {
                    Some(f) => Some(Chord::parse(f, os).map_err(invalid)?),
                    None => None,
                };
                let others = match (args.flag("replace"), args.flag("swap")) {
                    (true, true) => {
                        return Err(invalid("--replace と --swap は同時に指定できません".into()));
                    }
                    (true, _) => Others::Remove,
                    (_, true) if from.is_none() => {
                        return Err(invalid("--swap には --from（元のキー）が必要です".into()));
                    }
                    (_, true) => Others::Swap,
                    _ => Others::Keep,
                };
                s.keymap.set(ctx, &c, &cmd, from.as_ref(), others);
                if args.flag("shadow") {
                    s.keymap.shadow.insert((ctx.into(), c.clone()));
                }
                let issues = issues_json(&s.keymap, os);
                let here: Vec<&Value> = issues
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|i| {
                        i["kind"] == "conflict" && i["context"] == ctx && i["key"] == c.to_string()
                    })
                    .collect();
                let note = match here.first() {
                    Some(i) => format!(
                        "（衝突: {}）",
                        i["commands"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .filter(|x| *x != cmd)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    None => String::new(),
                };
                (
                    Output::with(
                        format!("{ctx}: {} → {cmd}{note}", c.display(os)),
                        json!({"rows": rows_json(&s.keymap, os, Some(ctx), false), "conflicts": issues}),
                    ),
                    File::Keymap,
                )
            }
            "keymap.unset" => {
                let c = chord(args, "key", os)?;
                let cmd = args.opt_str("command");
                let ctxs: Vec<&str> = match args.opt_str("context") {
                    Some(x) => vec![x],
                    None => CONTEXTS.iter().map(|x| x.name).collect(),
                };
                let before = s.keymap.bindings();
                for ctx in &ctxs {
                    s.keymap.unset(ctx, &c, cmd);
                }
                if s.keymap.bindings() == before {
                    return Err(CmdError::new(
                        ErrorCode::NothingToDo,
                        format!("{} には外す割り当てがありません", c.display(os)),
                    ));
                }
                (
                    Output::with(
                        format!("{} の割り当てを外しました", c.display(os)),
                        json!({"conflicts": issues_json(&s.keymap, os)}),
                    ),
                    File::Keymap,
                )
            }
            "keymap.reset" => {
                let c = match args.opt_str("key") {
                    Some(k) => Some(Chord::parse(k, os).map_err(invalid)?),
                    None => None,
                };
                let cmd = match args.opt_str("command") {
                    Some(x) => Some(check_binding(x, cx.registry).map_err(invalid)?),
                    None => None,
                };
                let before = s.keymap.changed_rows();
                s.keymap
                    .reset(args.opt_str("context"), c.as_ref(), cmd.as_deref());
                let n = before.saturating_sub(s.keymap.changed_rows());
                (
                    Output::with(
                        format!("{n} 行をプリセットの状態に戻しました"),
                        json!({"conflicts": issues_json(&s.keymap, os)}),
                    ),
                    File::Keymap,
                )
            }
            "keymap.preset" => {
                let name = args.str("name")?;
                let p = catalog::preset(name).ok_or_else(|| {
                    CmdError::not_found(format!("プリセット「{name}」はありません"))
                })?;
                // `mine` (the default) keeps the rows the user changed; `none` starts over from
                // the preset.
                let keep = match args.opt_str("keep").unwrap_or("mine") {
                    "mine" => true,
                    "none" => false,
                    other => {
                        return Err(CmdError::invalid(format!(
                            "keep は mine か none です（「{other}」は使えません）"
                        )));
                    }
                };
                let dropped = s.keymap.changed_rows();
                s.keymap.preset = name.to_string();
                if !keep {
                    s.keymap.reset(None, None, None);
                }
                let kept = if keep { dropped } else { 0 };
                let message = if keep {
                    format!(
                        "「{}」を適用しました（変更済みの {kept} 件はそのまま）",
                        p.label
                    )
                } else {
                    format!(
                        "「{}」にそろえました（変更済みの {dropped} 件を戻しました）",
                        p.label
                    )
                };
                (
                    Output::with(
                        message,
                        json!({"preset": name, "kept": kept, "keep": if keep { "mine" } else { "none" },
                            "sources": p.sources.iter()
                                .map(|(title, url)| json!({"title": title, "url": url}))
                                .collect::<Vec<_>>(),
                            "conflicts": issues_json(&s.keymap, os)}),
                    ),
                    File::Keymap,
                )
            }
            "keymap.conflicts" => {
                let issues = s.keymap.conflicts(os);
                let lines: Vec<String> = issues
                    .iter()
                    .map(|i| {
                        let at = format!("{} {}", i.context(), i.chord().display(os));
                        match i {
                            Issue::Conflict { commands, .. } => {
                                format!("衝突 {at}: {}", commands.join(" / "))
                            }
                            Issue::Shadow {
                                command,
                                hidden,
                                hidden_context,
                                ..
                            } => format!(
                                "優先 {at}: {command}（{hidden_context} の {hidden} より先）"
                            ),
                            Issue::Reserved { command, .. } => {
                                format!("OS {at}: {command}（OS が使うキー）")
                            }
                            Issue::Text { command, .. } => format!("入力 {at}: {command}"),
                        }
                    })
                    .collect();
                let msg = if lines.is_empty() {
                    "衝突はありません".to_string()
                } else {
                    lines.join("\n")
                };
                return Ok(Output::with(msg, issues_json(&s.keymap, os)));
            }
            "keymap.import" => {
                if args.opt_str("text").is_some()
                    && (args.opt_str("path").is_some() || args.str("source")? != "toml")
                {
                    return Err(invalid("text は TOML の内容だけ指定できます".into()));
                }
                let default_path;
                let path = if args.opt_str("text").is_some() {
                    "TOML"
                } else if let Some(path) = args.opt_str("path") {
                    path
                } else {
                    if args.str("source")? != "studio-pro" {
                        return Err(invalid("--path が必要です".into()));
                    }
                    default_path=import::default_studio_pro_path().ok_or_else(||CmdError::not_found("Studio Pro の user.keyscheme が見つかりません（--path で指定できます）"))?.to_string_lossy().into_owned();
                    &default_path
                };
                let text = match args.opt_str("text") {
                    Some(text) => text.to_owned(),
                    None => std::fs::read_to_string(path)
                        .map_err(|e| CmdError::io(format!("{path} を読めません: {e}")))?,
                };
                let mut unsupported = Vec::new();
                let mut import_problems = Vec::new();
                let n = match args.str("source")? {
                    "toml" => {
                        let t: toml::Table = text
                            .parse()
                            .map_err(|e: toml::de::Error| invalid(format!("{path}: {e}")))?;
                        let mut problems = Vec::new();
                        let k = Keymap::from_toml(&t, &mut problems);
                        if !problems.is_empty() {
                            return Err(invalid(problems.join("\n")));
                        }
                        s.keymap = k;
                        s.keymap.changed_rows()
                    }
                    _ => {
                        let report =
                            import::studio_pro_report(&text, &mut s.keymap).map_err(invalid)?;
                        unsupported = report.unsupported;
                        import_problems = report.problems;
                        report.count
                    }
                };
                (
                    Output::with(
                        format!("{n} 件のショートカットを読み込みました"),
                        json!({"count": n, "unsupported": unsupported, "problems": import_problems,
                            "rows": rows_json(&s.keymap, os, None, false), "conflicts": issues_json(&s.keymap, os)}),
                    ),
                    File::Keymap,
                )
            }
            "keymap.export" => {
                let format = args.opt_str("format").unwrap_or("toml");
                let text = match format {
                    "json" => {
                        serde_json::to_string_pretty(&s.keymap.to_json(os)).unwrap_or_default()
                    }
                    "html" => export::html(&s.keymap, os, cx.registry),
                    _ => s.keymap.to_toml(),
                };
                let Some(to) = args.opt_str("to") else {
                    return Ok(Output::with(
                        text.clone(),
                        json!({"format": format, "text": text}),
                    ));
                };
                if !human {
                    return Err(CmdError::new(
                        ErrorCode::Denied,
                        "AI からはファイルに書き出せません（--to を省くと内容を返します）",
                    ));
                }
                if !dry {
                    std::fs::write(to, &text)
                        .map_err(|e| CmdError::io(format!("{to} に書き出せません: {e}")))?;
                }
                return Ok(Output::with(
                    format!("keymap.{format} を保存しました"),
                    json!({"format": format, "path": to}),
                ));
            }
            "mousemap.get" => {
                let ctx = args.opt_str("context");
                if let Some(c) = ctx {
                    crate::mousemap::Ctx::parse(c).map_err(invalid)?;
                }
                let data = s.mousemap.to_json(ctx);
                let mut lines = Vec::new();
                for (c, rows) in data.as_object().into_iter().flatten() {
                    lines.push(format!("[{c}]"));
                    for r in rows.as_array().into_iter().flatten() {
                        lines.push(format!(
                            "  {} = {}{}",
                            r["modifier"].as_str().unwrap_or(""),
                            r["action"].as_str().unwrap_or(""),
                            if r["changed"] == true {
                                "  （変更）"
                            } else {
                                ""
                            }
                        ));
                    }
                }
                let w: Vec<Value> = s
                    .mousemap
                    .warnings()
                    .into_iter()
                    .map(|(c, m, t)| json!({"context": c, "modifier": m, "hold": t}))
                    .collect();
                return Ok(Output::with(
                    lines.join("\n"),
                    json!({"contexts": data, "warnings": w}),
                ));
            }
            "mousemap.set" | "mousemap.unset" => {
                let ctx = args.str("context")?;
                let m = args.str("modifier")?;
                let action = if def.name == "mousemap.set" {
                    args.str("action")?
                } else {
                    ""
                };
                if let Some(from) = args.opt_str("move-from") {
                    s.mousemap.move_hold(ctx, from, m).map_err(invalid)?;
                }
                s.mousemap.set(ctx, m, action).map_err(invalid)?;
                let c = crate::mousemap::Ctx::parse(ctx).map_err(invalid)?;
                let m = c.modifier(m).map_err(invalid)?;
                let now = s.mousemap.get(ctx, &m);
                let w: Vec<Value> = s
                    .mousemap
                    .warnings()
                    .into_iter()
                    .map(|(c, m, t)| json!({"context": c, "modifier": m, "hold": t}))
                    .collect();
                (
                    Output::with(
                        format!(
                            "{ctx} {m} = {}",
                            if now.is_empty() {
                                "（何もしない）"
                            } else {
                                now.as_str()
                            }
                        ),
                        json!({"context": ctx, "modifier": m, "action": now, "warnings": w}),
                    ),
                    File::Mousemap,
                )
            }
            "tool.alt" => {
                let area = args.opt_str("area").unwrap_or("arrange");
                let ctx = format!("{area}.select.hold");
                let order = if area == "roll" {
                    xirius_core::commands::ROLL_ALT_TOOLS
                } else {
                    xirius_core::commands::ALT_TOOLS
                };
                let now = s.mousemap.get(&ctx, "Mod");
                let now = now.strip_prefix("tool:").unwrap_or(&now);
                let tool = match args.opt_str("tool") {
                    Some(t) => {
                        s.mousemap
                            .set(&ctx, "Mod", &format!("tool:{t}"))
                            .map_err(invalid)?;
                        t.to_string()
                    }
                    None => {
                        // The next tool the screen has (the roll has no range tool, say).
                        let at = order.iter().position(|t| *t == now).map_or(0, |i| i + 1);
                        let next = (0..order.len())
                            .map(|k| order[(at + k) % order.len()])
                            .find(|t| s.mousemap.set(&ctx, "Mod", &format!("tool:{t}")).is_ok());
                        next.ok_or_else(|| invalid("代わりにできる道具がありません".into()))?
                            .to_string()
                    }
                };
                (
                    Output::with(
                        format!("{area} の代わりの道具 = {tool}"),
                        json!({"area": area, "tool": tool}),
                    ),
                    File::Mousemap,
                )
            }
            "mousemap.conflicts" => {
                let w = mouse_conflicts(&s.mousemap);
                let lines: Vec<String> = w
                    .iter()
                    .map(|x| {
                        format!(
                            "優先 {} {}: 押している間は「{}」に切り替わります",
                            x["context"].as_str().unwrap_or(""),
                            x["modifier"].as_str().unwrap_or(""),
                            x["hold"].as_str().unwrap_or(""),
                        )
                    })
                    .collect();
                let msg = if lines.is_empty() {
                    "衝突はありません".to_string()
                } else {
                    lines.join("\n")
                };
                return Ok(Output::with(msg, json!({"conflicts": w})));
            }
            "mousemap.reset" => {
                let before = s.mousemap.changed_count();
                s.mousemap
                    .reset(args.opt_str("context"), args.opt_str("modifier"))
                    .map_err(invalid)?;
                let n = before - s.mousemap.changed_count();
                (
                    Output::msg(format!("{n} 件を既定に戻しました")),
                    File::Mousemap,
                )
            }
            "midimap.list" => {
                return Ok(Output::with(
                    midimap_lines(&s.midimap, cx),
                    json!({"rows": midimap_rows(&s.midimap, cx)}),
                ));
            }
            "midimap.set" => {
                let message = Message::parse(args.str("message")?).map_err(invalid)?;
                let target = checked_target(args.str("target")?, cx)?;
                let port = args.opt_str("port");
                let replaced = s.midimap.assign(port, message, target.clone());
                (
                    Output::with(
                        format!("{} → {}", message.label(), target_label(&target, cx)),
                        json!({"replaced": replaced.iter().map(|r| r.to_json()).collect::<Vec<_>>()}),
                    ),
                    File::Midimap,
                )
            }
            "midimap.unset" => {
                let port = args.opt_str("port");
                let n = match (args.opt_str("message"), args.opt_str("target")) {
                    (None, Some(t)) => {
                        let target = Target::parse(t).map_err(invalid)?;
                        s.midimap.unassign_target(&target)
                    }
                    (Some(m), _) => {
                        let m = Message::parse(m).map_err(invalid)?;
                        match port {
                            Some(_) => usize::from(s.midimap.unassign(port, m)),
                            None => s.midimap.unassign_any(m),
                        }
                    }
                    (None, None) => match port {
                        Some(p) => s.midimap.unassign_port(p),
                        None => s.midimap.clear(),
                    },
                };
                if n == 0 {
                    return Err(CmdError::new(
                        ErrorCode::NothingToDo,
                        "外す割り当てがありません",
                    ));
                }
                (
                    Output::msg(format!("{n} 件の割り当てを外しました")),
                    File::Midimap,
                )
            }
            "macro.list" => {
                let (text, data) = macro_ext::list(&s, cx.registry);
                return Ok(Output::with(text, data));
            }
            "macro.run" => {
                let (message, edits) = macro_ext::plan_run(&s.macros, cx, args)?;
                if dry {
                    return Ok(Output::msg(format!("（試行）{message}")));
                }
                self.pending = edits;
                return Ok(Output::msg(message));
            }
            "macro.set" | "macro.rename" | "macro.remove" => {
                let done = match def.name {
                    "macro.set" => macro_ext::set(&mut s, cx, args)?,
                    "macro.rename" => macro_ext::rename(&mut s, args)?,
                    _ => macro_ext::remove(&mut s, args)?,
                };
                keys_too = done.keys;
                (Output::with(done.message, done.data), File::Macros)
            }
            other => {
                return Err(CmdError::new(
                    ErrorCode::UnknownCommand,
                    format!("{other} は設定のコマンドではありません"),
                ));
            }
        };
        let mut out = out;
        if file == File::Keymap {
            out.data["rows"] = rows_json(&s.keymap, os, None, false);
            out.data["conflicts"] = issues_json(&s.keymap, os);
        } else if file == File::Midimap {
            out.data["rows"] = midimap_rows(&s.midimap, cx);
        } else if file == File::Macros {
            out.data["macros"] = macro_ext::rows(&s, cx.registry);
        } else if file == File::Mousemap {
            out.data["contexts"] = s.mousemap.to_json(None);
            out.data["conflicts"] = json!([]);
            out.data["warnings"] = json!(
                s.mousemap
                    .warnings()
                    .into_iter()
                    .map(|(c, m, t)| json!({"context":c,"modifier":m,"hold":t}))
                    .collect::<Vec<_>>()
            );
        }
        if dry {
            return Ok(Output::with(format!("（試行）{}", out.message), out.data));
        }
        s.save(file).map_err(CmdError::io)?;
        if keys_too {
            s.save(File::Keymap).map_err(CmdError::io)?;
        }
        lock(&self.shared).adopt_worker(s);
        Ok(out)
    }
}
