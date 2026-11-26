//! The keymap: a preset and what the user changed from it (06 §3.2, §3.5).
//!
//! Only the difference from the preset is kept: the keys the user added (`[roll]
//! "Alt+Up" = "note.transpose --by 1"`) and the preset's keys the user took away
//! (`[unbind] edit = ["Mod+D"]`). Changing the preset keeps both, so the rows the user changed
//! stay as they are. Every change is made on the keys in effect and written back as the
//! smallest difference from the preset.

use std::cell::{Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::catalog::{self, CONTEXTS, Cat, ENTRIES};
use crate::chord::{Chord, Os};
use crate::tomlw;

pub const PRESET_VERSION: i64 = 1;

/// (context, key) → its commands. Multiple commands are an unresolved conflict.
pub type Bindings = BTreeMap<(String, Chord), Vec<String>>;

#[derive(Clone)]
pub struct Keymap {
    pub preset: String,
    /// Keys the user added, per context.
    pub add: BTreeMap<String, BTreeMap<Chord, Vec<String>>>,
    /// The preset's keys the user took away, per context.
    pub unbind: BTreeMap<String, BTreeSet<Chord>>,
    /// Deliberately hiding a wider binding.
    pub shadow: BTreeSet<(String, Chord)>,
    /// What `bindings()`/`rows()` last gave, and the fields it was read from.
    /// The fields are compared on every read — a `set`, a line read from the
    /// file, or a field written directly all look the same to it — so the next
    /// read after any change makes them again.
    resolved: RefCell<Option<Box<Resolved>>>,
    /// How often the effective map and the row list were made, so a test can
    /// tell a screen's worth of reads from a screen's worth of remakes.
    #[cfg(test)]
    builds: std::cell::Cell<usize>,
    #[cfg(test)]
    row_builds: std::cell::Cell<usize>,
}

impl PartialEq for Keymap {
    fn eq(&self, other: &Keymap) -> bool {
        self.preset == other.preset
            && self.add == other.add
            && self.unbind == other.unbind
            && self.shadow == other.shadow
    }
}

impl std::fmt::Debug for Keymap {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Keymap")
            .field("preset", &self.preset)
            .field("add", &self.add)
            .field("unbind", &self.unbind)
            .field("shadow", &self.shadow)
            .finish()
    }
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap {
            preset: "studio-pro".into(),
            add: BTreeMap::new(),
            unbind: BTreeMap::new(),
            shadow: BTreeSet::new(),
            resolved: RefCell::new(None),
            #[cfg(test)]
            builds: std::cell::Cell::new(0),
            #[cfg(test)]
            row_builds: std::cell::Cell::new(0),
        }
    }
}

/// The maps `bindings()` and `rows()` are read from, and the fields they came from.
#[derive(Clone)]
struct Resolved {
    preset: String,
    add: BTreeMap<String, BTreeMap<Chord, Vec<String>>>,
    unbind: BTreeMap<String, BTreeSet<Chord>>,
    shadow: BTreeSet<(String, Chord)>,
    bindings: Arc<Bindings>,
    preset_bindings: Arc<Bindings>,
    rows: Option<Arc<Vec<Row>>>,
}

impl Resolved {
    /// The fields still say what these maps were read from.
    fn current(&self, k: &Keymap) -> bool {
        self.preset == k.preset
            && self.add == k.add
            && self.unbind == k.unbind
            && self.shadow == k.shadow
    }
}

/// The keys a preset gives.
pub fn preset_bindings(name: &str) -> Bindings {
    let preset = catalog::preset(name);
    let mut out = Bindings::new();
    for e in ENTRIES {
        let keys = preset
            .and_then(|p| {
                p.diff
                    .iter()
                    .find(|(c, cmd, _)| *c == e.context && *cmd == e.command)
            })
            .map(|(_, _, k)| *k)
            .unwrap_or(e.keys);
        for k in keys {
            let chord = Chord::parse(k, Os::Win).expect("the catalog's keys read");
            out.entry((e.context.to_string(), chord))
                .or_default()
                .push(e.command.to_string());
        }
    }
    out
}

/// One row of the screen's list: a command in a context and its keys.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub context: String,
    pub command: String,
    pub cat: Cat,
    pub keys: Vec<Chord>,
    pub preset_keys: Vec<Chord>,
    /// The row is in the catalog (otherwise the user bound a command of the table).
    pub listed: bool,
}

impl Row {
    pub fn changed(&self) -> bool {
        self.keys != self.preset_keys
    }
}

/// What `conflicts` finds.
#[derive(Clone, Debug, PartialEq)]
pub enum Issue {
    /// Two commands on one key in one context: an error.
    Conflict {
        context: String,
        chord: Chord,
        commands: Vec<String>,
    },
    /// A narrower context's key hides a wider one's.
    Shadow {
        context: String,
        chord: Chord,
        command: String,
        hidden_context: String,
        hidden: String,
    },
    /// The system keeps the key for itself.
    Reserved {
        context: String,
        chord: Chord,
        command: String,
    },
    Text {
        context: String,
        chord: Chord,
        command: String,
    },
}

impl Issue {
    pub fn to_json(&self, os: Os) -> Value {
        match self {
            Issue::Text {
                context,
                chord,
                command,
            } => json!({"kind":"text", "severity":"warning", "context":context,
                "key":chord.to_string(), "shown":chord.display(os), "command":command}),
            Issue::Conflict {
                context,
                chord,
                commands,
            } => {
                json!({"kind": "conflict", "severity":"error", "context": context, "key": chord.to_string(),
                "shown": chord.display(os), "commands": commands})
            }
            Issue::Shadow {
                context,
                chord,
                command,
                hidden_context,
                hidden,
            } => {
                json!({"kind": "shadow", "severity":"warning", "context": context, "key": chord.to_string(),
                "shown": chord.display(os), "command": command,
                "hides": {"context": hidden_context, "command": hidden}})
            }
            Issue::Reserved {
                context,
                chord,
                command,
            } => {
                json!({"kind": "reserved", "severity":"warning", "context": context, "key": chord.to_string(),
                "shown": chord.display(os), "command": command})
            }
        }
    }

    pub fn chord(&self) -> &Chord {
        match self {
            Issue::Conflict { chord, .. }
            | Issue::Shadow { chord, .. }
            | Issue::Reserved { chord, .. } => chord,
            Issue::Text { chord, .. } => chord,
        }
    }

    pub fn context(&self) -> &str {
        match self {
            Issue::Conflict { context, .. }
            | Issue::Shadow { context, .. }
            | Issue::Reserved { context, .. } => context,
            Issue::Text { context, .. } => context,
        }
    }
}

/// Which of a key's bindings to keep when another command takes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Others {
    /// Leave them (the key is then in conflict).
    #[default]
    Keep,
    /// Take the key away from them.
    Remove,
    /// Give them the key the command had before (`from`).
    Swap,
}

impl Keymap {
    /// The keys in effect.
    pub fn bindings(&self) -> Bindings {
        (*self.shared_bindings()).clone()
    }

    /// The maps the fields stand for, shared by every read until they change.
    fn resolve(&self) -> Ref<'_, Resolved> {
        {
            let mut slot = self.resolved.borrow_mut();
            if slot.as_ref().is_none_or(|r| !r.current(self)) {
                *slot = Some(Box::new(self.build()));
            }
        }
        Ref::map(self.resolved.borrow(), |r| &**r.as_ref().unwrap())
    }

    /// `resolve()`'s make, counted in tests.
    fn build(&self) -> Resolved {
        #[cfg(test)]
        self.builds.set(self.builds.get() + 1);
        let preset_bindings = Arc::new(preset_bindings(&self.preset));
        Resolved {
            preset: self.preset.clone(),
            add: self.add.clone(),
            unbind: self.unbind.clone(),
            shadow: self.shadow.clone(),
            bindings: Arc::new(self.apply_to((*preset_bindings).clone())),
            preset_bindings,
            rows: None,
        }
    }

    fn shared_bindings(&self) -> Arc<Bindings> {
        self.resolve().bindings.clone()
    }

    fn shared_preset_bindings(&self) -> Arc<Bindings> {
        self.resolve().preset_bindings.clone()
    }

    fn shared_rows(&self) -> Arc<Vec<Row>> {
        if let Some(rows) = &self.resolve().rows {
            return rows.clone();
        }
        let rows = Arc::new(self.build_rows());
        self.resolved.borrow_mut().as_mut().unwrap().rows = Some(rows.clone());
        rows
    }

    fn apply_to(&self, mut b: Bindings) -> Bindings {
        for (ctx, chords) in &self.unbind {
            for c in chords {
                b.remove(&(ctx.clone(), c.clone()));
            }
        }
        for (ctx, chords) in &self.add {
            for (c, cmds) in chords {
                let v = b.entry((ctx.clone(), c.clone())).or_default();
                for cmd in cmds {
                    v.retain(|x| x != cmd);
                    v.push(cmd.clone());
                }
            }
        }
        b
    }

    /// Keeps `b` as the keys in effect: the smallest difference from the preset.
    pub fn set_bindings(&mut self, b: &Bindings) {
        let p = self.shared_preset_bindings();
        self.shadow.retain(|key| b.contains_key(key));
        self.add.clear();
        self.unbind.clear();
        let keys: BTreeSet<&(String, Chord)> = p.keys().chain(b.keys()).collect();
        for k in keys {
            let want = b.get(k).map(Vec::as_slice).unwrap_or(&[]);
            let has = p.get(k).map(Vec::as_slice).unwrap_or(&[]);
            if want == has {
                continue;
            }
            // The preset's commands must come first, in its order, to be kept as they are.
            let kept = !has.is_empty() && want.len() >= has.len() && want[..has.len()] == *has;
            let added: Vec<String> = if kept {
                want[has.len()..].to_vec()
            } else {
                if !has.is_empty() {
                    self.unbind
                        .entry(k.0.clone())
                        .or_default()
                        .insert(k.1.clone());
                }
                want.to_vec()
            };
            if !added.is_empty() {
                self.add
                    .entry(k.0.clone())
                    .or_default()
                    .insert(k.1.clone(), added);
            }
        }
    }

    /// The command a key runs in `context` (or the contexts around it), and where it was found.
    pub fn lookup(&self, context: &str, chord: &Chord) -> Option<(String, String)> {
        lookup_in(&self.shared_bindings(), context, chord)
    }

    /// Shortcut text for a command in the active context, including its wider contexts.
    /// Hidden or conflicting bindings are excluded. An unbound command has no text.
    pub fn display_for(&self, context: &str, command: &str, os: Os) -> String {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let b = self.shared_bindings();
        let p = self.shared_preset_bindings();
        for ctx in catalog::chain(context) {
            for cmd in display_commands(&b, ctx, command) {
                for chord in keys_in(&b, &p, ctx, &cmd) {
                    if lookup_in(&b, context, &chord).is_some_and(|(_, c)| c == cmd)
                        && seen.insert(chord.clone())
                    {
                        out.push(chord.display(os));
                    }
                }
            }
        }
        out.join(" / ")
    }

    /// The keys of `command` in `context`, in the preset's order and then the user's.
    pub fn keys_of(&self, context: &str, command: &str) -> Vec<Chord> {
        keys_in(
            &self.shared_bindings(),
            &self.shared_preset_bindings(),
            context,
            command,
        )
    }

    /// Every row: the catalog's, then the commands the user bound that it does not list.
    pub fn rows(&self) -> Vec<Row> {
        (*self.shared_rows()).clone()
    }

    /// `rows()`'s make, counted in tests.
    fn build_rows(&self) -> Vec<Row> {
        #[cfg(test)]
        self.row_builds.set(self.row_builds.get() + 1);
        rows_of(&self.shared_bindings(), &self.shared_preset_bindings())
    }

    /// How many rows would change if the preset became `name` (the rows the user changed stay).
    pub fn rows_changed_by(&self, name: &str) -> usize {
        let now = self.shared_rows();
        let mut other = self.clone();
        other.preset = name.to_string();
        let then = other.shared_rows();
        then.iter()
            .filter(|r| {
                now.iter()
                    .find(|n| n.context == r.context && n.command == r.command)
                    .is_none_or(|n| n.keys != r.keys)
            })
            .count()
    }

    /// Rows the user changed from the preset.
    pub fn changed_rows(&self) -> usize {
        self.shared_rows().iter().filter(|r| r.changed()).count()
    }

    /// Binds `chord` to `command` in `context`. With `from`, the key replaces that key of the
    /// command (the one clicked on the screen).
    pub fn set(
        &mut self,
        context: &str,
        chord: &Chord,
        command: &str,
        from: Option<&Chord>,
        others: Others,
    ) {
        let mut b = self.bindings();
        let here = (context.to_string(), chord.clone());
        self.shadow.remove(&here);
        let v = b.entry(here.clone()).or_default();
        v.retain(|c| c != command);
        let moved: Vec<String> = match others {
            Others::Keep => Vec::new(),
            Others::Remove | Others::Swap => std::mem::take(v),
        };
        v.push(command.to_string());
        if let Some(from) = from.filter(|f| *f != chord) {
            let there = (context.to_string(), from.clone());
            self.shadow.remove(&there);
            let w = b.entry(there.clone()).or_default();
            w.retain(|c| c != command);
            if others == Others::Swap {
                for m in &moved {
                    w.retain(|c| c != m);
                    w.push(m.clone());
                }
            }
            if w.is_empty() {
                b.remove(&there);
            }
        }
        self.set_bindings(&b);
    }

    /// Takes `chord` away in `context` (from `command` only, when given).
    pub fn unset(&mut self, context: &str, chord: &Chord, command: Option<&str>) {
        let mut b = self.bindings();
        let k = (context.to_string(), chord.clone());
        self.shadow.remove(&k);
        if let Some(v) = b.get_mut(&k) {
            match command {
                Some(cmd) => v.retain(|c| c != cmd),
                None => v.clear(),
            }
            if v.is_empty() {
                b.remove(&k);
            }
        }
        self.set_bindings(&b);
    }

    /// Back to the preset: all, one context, one key (in one context or all) or one row.
    pub fn reset(&mut self, context: Option<&str>, chord: Option<&Chord>, command: Option<&str>) {
        let p = self.shared_preset_bindings();
        let mut b = self.bindings();
        let in_ctx = |c: &str| context.is_none_or(|x| x == c);
        self.shadow.retain(|key| {
            !in_ctx(&key.0)
                || chord.is_some_and(|c| c != &key.1)
                || command
                    .is_some_and(|cmd| !b.get(key).is_some_and(|v| v.iter().any(|c| c == cmd)))
        });
        if let Some(cmd) = command {
            for ((c, _), v) in b.iter_mut() {
                if in_ctx(c) {
                    v.retain(|x| x != cmd);
                }
            }
            for ((c, k), v) in p.iter() {
                if in_ctx(c) && v.iter().any(|x| x == cmd) {
                    let w = b.entry((c.clone(), k.clone())).or_default();
                    w.retain(|x| x != cmd);
                    w.insert(0, cmd.to_string());
                }
            }
        } else {
            let keys: BTreeSet<(String, Chord)> = p.keys().chain(b.keys()).cloned().collect();
            for k in keys {
                if !in_ctx(&k.0) || chord.is_some_and(|c| *c != k.1) {
                    continue;
                }
                match p.get(&k) {
                    Some(v) => b.insert(k, v.clone()),
                    None => b.remove(&k),
                };
            }
        }
        b.retain(|_, v| !v.is_empty());
        self.set_bindings(&b);
    }

    /// Conflicts, hidden keys and the system's keys.
    pub fn conflicts(&self, os: Os) -> Vec<Issue> {
        let b = self.shared_bindings();
        let reserved: Vec<Chord> = match os {
            Os::Win => catalog::OS_RESERVED_WIN,
            Os::Mac => catalog::OS_RESERVED_MAC,
        }
        .iter()
        .map(|k| Chord::parse(k, Os::Win).expect("reserved keys read"))
        .collect();
        // The preset's own narrow keys hide its wide ones on purpose (the roll's arrows, UI-088).
        let preset = self.shared_preset_bindings();
        let deliberate = |ctx: &String, chord: &Chord, wider: &str| {
            let here = (ctx.clone(), chord.clone());
            let there = (wider.to_string(), chord.clone());
            preset.contains_key(&here)
                && preset.get(&here) == b.get(&here)
                && preset.get(&there) == b.get(&there)
        };
        let mut out = Vec::new();
        for ((ctx, chord), cmds) in b.iter() {
            if cmds.len() > 1 {
                out.push(Issue::Conflict {
                    context: ctx.clone(),
                    chord: chord.clone(),
                    commands: cmds.clone(),
                });
            }
            let Some(cmd) = cmds.last() else { continue };
            for wider in catalog::chain(ctx).into_iter().skip(1) {
                if let Some(h) = b.get(&(wider.to_string(), chord.clone()))
                    && let Some(hidden) = h.last()
                    && hidden != cmd
                    && !self.shadow.contains(&(ctx.clone(), chord.clone()))
                    && !deliberate(ctx, chord, wider)
                {
                    out.push(Issue::Shadow {
                        context: ctx.clone(),
                        chord: chord.clone(),
                        command: cmd.clone(),
                        hidden_context: wider.to_string(),
                        hidden: hidden.clone(),
                    });
                    break;
                }
            }
            if reserved.contains(chord) {
                out.push(Issue::Reserved {
                    context: ctx.clone(),
                    chord: chord.clone(),
                    command: cmd.clone(),
                });
            }
            if ctx == "text" && chord.mods == 0 && chord.key.chars().count() == 1 {
                out.push(Issue::Text {
                    context: ctx.clone(),
                    chord: chord.clone(),
                    command: cmd.clone(),
                });
            }
        }
        out
    }

    /// Reads `keymap.toml`. Lines it cannot use are skipped and named in `problems`.
    pub fn from_toml(t: &toml::Table, problems: &mut Vec<String>) -> Keymap {
        let mut k = Keymap::default();
        let registry = xirius_core::command::Registry::standard();
        for (key, v) in t {
            match (key.as_str(), v) {
                ("preset", toml::Value::String(s)) => {
                    if catalog::preset(s).is_some() {
                        k.preset = s.clone();
                    } else {
                        problems.push(format!("keymap.toml: preset「{s}」はありません"));
                    }
                }
                ("preset-version", _) => {}
                ("unbind", toml::Value::Table(ctxs)) => {
                    for (ctx, list) in ctxs {
                        let Some(items) = list.as_array() else {
                            problems
                                .push(format!("keymap.toml: [unbind] {ctx} はキーの配列にします"));
                            continue;
                        };
                        for item in items {
                            match item.as_str().map(|s| Chord::parse(s, Os::Win)) {
                                Some(Ok(c)) if catalog::context(ctx).is_some() => {
                                    k.unbind.entry(ctx.clone()).or_default().insert(c);
                                }
                                Some(Err(e)) => problems.push(format!("keymap.toml: {e}")),
                                _ => problems.push(format!("keymap.toml: [unbind] {ctx} {item}")),
                            }
                        }
                    }
                }
                (ctx, toml::Value::Table(rows)) if catalog::context(ctx).is_some() => {
                    for (chord, cmd) in rows {
                        let c = match Chord::parse(chord, Os::Win) {
                            Ok(c) => c,
                            Err(e) => {
                                problems.push(format!("keymap.toml: [{ctx}] {e}"));
                                continue;
                            }
                        };
                        let cmds: Vec<String> = match cmd {
                            toml::Value::String(s) => vec![s.clone()],
                            toml::Value::Table(t) => {
                                if t.get("shadow").and_then(toml::Value::as_bool) == Some(true) {
                                    k.shadow.insert((ctx.into(), c.clone()));
                                }
                                t.get("command")
                                    .and_then(toml::Value::as_str)
                                    .map(|s| vec![s.into()])
                                    .unwrap_or_default()
                            }
                            toml::Value::Array(a) => a
                                .iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect(),
                            _ => Vec::new(),
                        };
                        if cmds.is_empty() {
                            problems.push(format!(
                                "keymap.toml: [{ctx}] {chord} にコマンドがありません"
                            ));
                            continue;
                        }
                        let valid: Vec<String> = cmds
                            .into_iter()
                            .filter(|cmd| {
                                let name = cmd.split_whitespace().next().unwrap_or("");
                                let known = if registry.get(name).is_some() {
                                    crate::ext::check_binding(cmd, &registry).is_ok()
                                } else {
                                    // Session extensions supply transport/export definitions.
                                    xirius_core::commands::labels::label(name).is_some()
                                        && crate::cmdline::words(cmd).is_ok()
                                };
                                if !known {
                                    problems.push(format!("keymap.toml: [{ctx}] {chord}: {cmd}"));
                                }
                                known
                            })
                            .collect();
                        k.add
                            .entry(ctx.to_string())
                            .or_default()
                            .entry(c)
                            .or_default()
                            .extend(valid);
                    }
                }
                (other, _) => problems.push(format!("keymap.toml: 「{other}」は使用できません")),
            }
        }
        k
    }

    /// `keymap.toml`: the preset, the keys added per context, the keys taken away.
    pub fn to_toml(&self) -> String {
        let mut s = String::from("# keymap.toml（プリセットからの差分）\n");
        s.push_str(&format!("preset = {}\n", tomlw::string(&self.preset)));
        s.push_str(&format!("preset-version = {PRESET_VERSION}\n"));
        let mut additions = self.add.clone();
        let bindings = self.shared_bindings();
        for (ctx, chord) in &self.shadow {
            if let Some(commands) = bindings.get(&(ctx.clone(), chord.clone())) {
                additions
                    .entry(ctx.clone())
                    .or_default()
                    .entry(chord.clone())
                    .or_insert_with(|| commands.clone());
            }
        }
        for ctx in ordered(additions.keys()) {
            let rows = &additions[ctx];
            if rows.is_empty() {
                continue;
            }
            s.push_str(&format!("\n[{}]\n", tomlw::key(ctx)));
            for (c, cmds) in rows {
                let v = if cmds.len() == 1 && self.shadow.contains(&(ctx.into(), c.clone())) {
                    format!("{{ command = {}, shadow = true }}", tomlw::string(&cmds[0]))
                } else if cmds.len() == 1 {
                    tomlw::string(&cmds[0])
                } else {
                    let items: Vec<String> = cmds.iter().map(|c| tomlw::string(c)).collect();
                    format!("[{}]", items.join(", "))
                };
                s.push_str(&format!("{} = {v}\n", tomlw::string(&c.to_string())));
            }
        }
        if self.unbind.values().any(|v| !v.is_empty()) {
            s.push_str("\n[unbind]\n");
            for ctx in ordered(self.unbind.keys()) {
                let items: Vec<String> = self.unbind[ctx]
                    .iter()
                    .map(|c| tomlw::string(&c.to_string()))
                    .collect();
                if !items.is_empty() {
                    s.push_str(&format!("{} = [{}]\n", tomlw::key(ctx), items.join(", ")));
                }
            }
        }
        s
    }

    /// Everything in effect, for `keymap export --format json` and the CLI.
    pub fn to_json(&self, os: Os) -> Value {
        let rows: Vec<Value> = self.shared_rows().iter().map(|r| row_json(r, os)).collect();
        json!({"preset": self.preset, "rows": rows})
    }
}

/// The contexts in the catalog's order, then any others.
fn ordered<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<&'a String> {
    let mut v: Vec<&String> = keys.collect();
    v.sort_by_key(|k| {
        (
            CONTEXTS
                .iter()
                .position(|c| c.name == k.as_str())
                .unwrap_or(usize::MAX),
            (*k).clone(),
        )
    });
    v
}

/// [`Keymap::lookup`] over a map already in hand.
fn lookup_in(b: &Bindings, context: &str, chord: &Chord) -> Option<(String, String)> {
    if context == "text" && chord.mods == 0 && chord.key.chars().count() == 1 {
        return None;
    }
    for c in catalog::chain(context) {
        if let Some(cmds) = b.get(&(c.to_string(), chord.clone())) {
            return (cmds.len() == 1).then(|| (c.to_string(), cmds[0].clone()));
        }
    }
    None
}

/// The commands [`Keymap::display_for`] reads in `ctx` for `command`, in the
/// order `rows_of` gives them: the catalog's, then what the user bound that it
/// does not list.
fn display_commands(b: &Bindings, ctx: &str, command: &str) -> Vec<String> {
    let matches = |cmd: &str| {
        cmd == command
            || (!command.contains(char::is_whitespace)
                && cmd.split_whitespace().next() == Some(command))
    };
    let mut out: Vec<String> = ENTRIES
        .iter()
        .filter(|e| e.context == ctx && matches(e.command))
        .map(|e| e.command.to_string())
        .collect();
    let mut extra: BTreeSet<&String> = BTreeSet::new();
    for ((c, _), cmds) in b {
        if c != ctx {
            continue;
        }
        for cmd in cmds {
            if matches(cmd) && catalog::entry(ctx, cmd).is_none() {
                extra.insert(cmd);
            }
        }
    }
    out.extend(extra.into_iter().cloned());
    out
}

fn keys_in(b: &Bindings, p: &Bindings, context: &str, command: &str) -> Vec<Chord> {
    let mut keys: Vec<Chord> = b
        .iter()
        .filter(|((c, _), v)| c == context && v.iter().any(|x| x == command))
        .map(|((_, k), _)| k.clone())
        .collect();
    // The preset's order first (the catalog writes the main key first).
    let order = catalog::entry(context, command)
        .map(|e| e.keys)
        .unwrap_or(&[]);
    let pos = |k: &Chord| {
        order
            .iter()
            .position(|o| Chord::parse(o, Os::Win).ok().as_ref() == Some(k))
            .unwrap_or(usize::MAX)
    };
    let in_p = |k: &Chord| {
        p.get(&(context.to_string(), k.clone()))
            .is_some_and(|v| v.iter().any(|x| x == command))
    };
    keys.sort_by(|a, b| (!in_p(a), pos(a), a).cmp(&(!in_p(b), pos(b), b)));
    keys
}

fn rows_of(b: &Bindings, p: &Bindings) -> Vec<Row> {
    let mut out: Vec<Row> = ENTRIES
        .iter()
        .map(|e| Row {
            context: e.context.to_string(),
            command: e.command.to_string(),
            cat: e.cat,
            keys: keys_in(b, p, e.context, e.command),
            preset_keys: keys_in(p, p, e.context, e.command),
            listed: true,
        })
        .collect();
    let mut extra: BTreeSet<(String, String)> = BTreeSet::new();
    for ((ctx, _), cmds) in b {
        for cmd in cmds {
            if catalog::entry(ctx, cmd).is_none() {
                extra.insert((ctx.clone(), cmd.clone()));
            }
        }
    }
    for (ctx, cmd) in extra {
        out.push(Row {
            keys: keys_in(b, p, &ctx, &cmd),
            preset_keys: keys_in(p, p, &ctx, &cmd),
            cat: Cat::of(&cmd),
            context: ctx,
            command: cmd,
            listed: false,
        });
    }
    out
}

pub fn row_json(r: &Row, os: Os) -> Value {
    json!({
        "context": r.context,
        "command": r.command,
        "keys": r.keys.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        "shown": r.keys.iter().map(|k| k.display(os)).collect::<Vec<_>>(),
        "changed": r.changed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> Chord {
        Chord::parse(s, Os::Win).unwrap()
    }

    #[test]
    fn the_preset_has_no_conflict_and_a_wider_context_answers() {
        // The keys themselves are tests/default-keys.txt (tests/it/default_keys.rs).
        let k = Keymap::default();
        // A wider context answers when the narrow one has no key.
        assert_eq!(
            k.lookup("roll", &ch("Space")),
            Some(("global".into(), "transport.play".into()))
        );
        assert!(
            k.conflicts(Os::Win).is_empty(),
            "{:?}",
            k.conflicts(Os::Win)
        );
        assert_eq!(k.changed_rows(), 0);
    }

    #[test]
    fn the_rolls_arrows_hide_the_selection_step_on_purpose_until_one_side_changes() {
        // UI-088: ←→ in the roll move the notes; elsewhere they still step the selection.
        let mut k = Keymap::default();
        assert_eq!(
            k.lookup("roll", &ch("Right")),
            Some(("roll".into(), "edit.nudge.right --grid 1/16".into()))
        );
        assert_eq!(
            k.lookup("arrange", &ch("Left")),
            Some(("global".into(), "select.step --dir left".into()))
        );
        // UI-284: ↑↓ change the pitch of the selected notes, as Alt+↑↓ still do.
        for (key, cmd) in [
            ("Up", "note.transpose --by 1"),
            ("Alt+Up", "note.transpose --by 1"),
            ("Down", "note.transpose --by -1"),
            ("Alt+Down", "note.transpose --by -1"),
            // UI-295: Shift+↑↓ an octave, as Alt+Shift+↑↓ still do.
            ("Shift+Up", "note.transpose --by 12"),
            ("Alt+Shift+Up", "note.transpose --by 12"),
            ("Shift+Down", "note.transpose --by -12"),
            ("Alt+Shift+Down", "note.transpose --by -12"),
        ] {
            assert_eq!(
                k.lookup("roll", &ch(key)),
                Some(("roll".into(), cmd.into()))
            );
        }
        assert_eq!(
            k.lookup("arrange", &ch("Up")),
            Some(("global".into(), "select.step --dir up".into()))
        );
        // Outside the roll Shift+↑↓ still grow the selection, and Mod+Shift+↑↓ do in the roll.
        assert_eq!(
            k.lookup("arrange", &ch("Shift+Up")),
            Some(("global".into(), "select.step --dir up --extend".into()))
        );
        assert_eq!(
            k.lookup("roll", &ch("Mod+Shift+Down")),
            Some(("global".into(), "select.step --dir down --extend".into()))
        );
        assert!(k.conflicts(Os::Win).is_empty());
        // A key of the user's own over the wide one is told again.
        k.set(
            "global",
            &ch("Right"),
            "transport.play",
            None,
            Others::Remove,
        );
        assert!(k.conflicts(Os::Win).iter().any(|i| matches!(
            i,
            Issue::Shadow { context, hidden, .. } if context == "roll" && hidden == "transport.play"
        )));
    }

    #[test]
    fn a_key_given_to_another_row_is_a_conflict_until_it_is_settled() {
        let mut k = Keymap::default();
        // The roll's 「複製」 gets Q in place of D.
        k.set(
            "roll",
            &ch("Q"),
            "note.duplicate",
            Some(&ch("D")),
            Others::Keep,
        );
        assert_eq!(k.keys_of("roll", "note.duplicate"), [ch("Q")]);
        let c = k.conflicts(Os::Win);
        assert!(matches!(&c[..], [Issue::Conflict { commands, .. }]
            if commands == &["note.quantize", "note.duplicate"]));
        // An unresolved conflict runs neither command.
        assert_eq!(k.lookup("roll", &ch("Q")), None);

        // 入れ替え: the other row takes D.
        let mut swap = k.clone();
        swap.set(
            "roll",
            &ch("Q"),
            "note.duplicate",
            Some(&ch("D")),
            Others::Swap,
        );
        assert_eq!(swap.keys_of("roll", "note.quantize"), [ch("D")]);
        assert!(swap.conflicts(Os::Win).is_empty());

        // 相手のキーを外す.
        let mut remove = k.clone();
        remove.unset("roll", &ch("Q"), Some("note.quantize"));
        assert!(remove.keys_of("roll", "note.quantize").is_empty());
        assert!(remove.conflicts(Os::Win).is_empty());

        // ↺: the row goes back to the preset's key.
        k.reset(Some("roll"), None, Some("note.duplicate"));
        assert_eq!(k.keys_of("roll", "note.duplicate"), [ch("D")]);
        assert!(k.conflicts(Os::Win).is_empty());
        assert_eq!(k, Keymap::default());
    }

    #[test]
    fn changing_the_preset_keeps_the_users_rows() {
        let mut k = Keymap::default();
        k.set(
            "roll",
            &ch("Alt+M"),
            "note.mute",
            Some(&ch("Shift+M")),
            Others::Keep,
        );
        assert_eq!(k.changed_rows(), 1);
        let n = k.rows_changed_by("cubase");
        assert!(n > 0);
        k.preset = "cubase".into();
        assert_eq!(k.keys_of("roll", "note.mute"), [ch("Alt+M")]);
        assert_eq!(k.keys_of("roll", "note.duplicate"), [ch("Mod+D")]);
        assert_eq!(k.keys_of("edit", "tool.draw"), [ch("8")]);
    }

    #[test]
    fn key_012_the_ableton_preset_has_its_redo_metronome_and_reverse_keys() {
        let k = Keymap {
            preset: "ableton".into(),
            ..Keymap::default()
        };
        assert_eq!(k.keys_of("global", "history.redo"), [ch("Mod+Y")]);
        assert_eq!(
            k.lookup("arrange", &ch("O")),
            Some(("global".into(), "transport.metronome".into()))
        );
        assert_eq!(
            k.lookup("arrange", &ch("R")),
            Some(("arrange".into(), "clip.reverse".into()))
        );
        // The Studio Pro rows whose keys these took have none.
        assert!(k.keys_of("global", "record.preroll").is_empty());
        assert!(k.keys_of("edit", "track.arm").is_empty());
        assert!(
            k.conflicts(Os::Win).is_empty(),
            "{:?}",
            k.conflicts(Os::Win)
        );
    }

    #[test]
    fn the_file_holds_only_the_difference_and_reads_back() {
        let mut k = Keymap::default();
        k.set(
            "roll",
            &ch("Mod+Alt+V"),
            "note.velocity.set --value 64",
            None,
            Others::Keep,
        );
        k.set("arrange", &ch("Shift+M"), "clip.mute", None, Others::Keep);
        k.unset("global", &ch("Mod+Y"), None);
        let text = k.to_toml();
        assert!(
            text.contains("[roll]\n\"Mod+Alt+V\" = \"note.velocity.set --value 64\""),
            "{text}"
        );
        assert!(text.contains("[unbind]\nglobal = [\"Mod+Y\"]"), "{text}");
        let mut problems = Vec::new();
        let back = Keymap::from_toml(&text.parse().unwrap(), &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, k);
        // The same keys in effect give the same file.
        assert_eq!(back.to_toml(), text);
    }

    #[test]
    fn a_line_it_cannot_use_is_skipped_and_named() {
        let t: toml::Table =
            "preset = \"nope\"\n[roll]\n\"Hyper+Q\" = \"note.quantize\"\nQ = \"note.mute\"\n"
                .parse()
                .unwrap();
        let mut problems = Vec::new();
        let k = Keymap::from_toml(&t, &mut problems);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(k.preset, "studio-pro");
        assert_eq!(k.lookup("roll", &ch("Q")), None);
    }

    #[test]
    fn a_narrow_key_hiding_a_wide_one_and_a_system_key_are_warnings() {
        let mut k = Keymap::default();
        k.set("roll", &ch("NumPad*"), "note.mute", None, Others::Keep);
        k.set("global", &ch("Alt+F4"), "project.save", None, Others::Keep);
        let c = k.conflicts(Os::Win);
        assert!(
            c.iter()
                .any(|i| matches!(i, Issue::Shadow { hidden, .. } if hidden == "record.start"))
        );
        assert!(c.iter().any(|i| matches!(i, Issue::Reserved { .. })));
        assert!(
            !k.conflicts(Os::Mac)
                .iter()
                .any(|i| matches!(i, Issue::Reserved { chord,.. } if chord == &ch("Alt+F4")))
        );
    }

    /// `display_for` as it read before a call stopped building every row: build
    /// all the rows, then look each of the row's keys back up. `rows` is taken
    /// once by the caller so the whole check below stays quick.
    fn display_for_reference(
        k: &Keymap,
        rows: &[Row],
        context: &str,
        command: &str,
        os: Os,
    ) -> String {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for ctx in catalog::chain(context) {
            for row in rows.iter().filter(|r| {
                r.context == ctx
                    && (r.command == command
                        || (!command.contains(char::is_whitespace)
                            && r.command.split_whitespace().next() == Some(command)))
            }) {
                for chord in &row.keys {
                    if k.lookup(context, chord)
                        .is_some_and(|(_, cmd)| cmd == row.command)
                        && seen.insert(chord.clone())
                    {
                        out.push(chord.display(os));
                    }
                }
            }
        }
        out.join(" / ")
    }

    /// Every command a caller may name: the catalog's lines, their bare names,
    /// what the user bound that is not listed, and names nothing answers.
    fn every_command(k: &Keymap) -> Vec<String> {
        let mut v: Vec<String> = ENTRIES.iter().map(|e| e.command.to_string()).collect();
        v.extend(
            ENTRIES
                .iter()
                .filter_map(|e| e.command.split_whitespace().next().map(str::to_string)),
        );
        for cmds in k.bindings().values() {
            v.extend(cmds.iter().cloned());
        }
        v.extend(
            ["nothing.bound", "note.transpose --by 99"]
                .iter()
                .map(|s| s.to_string()),
        );
        v.sort();
        v.dedup();
        v
    }

    /// Keymaps worth checking: the defaults, a second key on one command, a key
    /// taken from another row (a conflict), a key taken away, a command the
    /// catalog does not list, and other presets with and without the user's own.
    fn keymap_states() -> Vec<(&'static str, Keymap)> {
        let mut states = vec![("default", Keymap::default())];
        let mut two = Keymap::default();
        two.set("roll", &ch("F9"), "note.delete", None, Others::Keep);
        states.push(("two-keys", two.clone()));
        let mut stolen = Keymap::default();
        stolen.set(
            "roll",
            &ch("Q"),
            "note.duplicate",
            Some(&ch("D")),
            Others::Keep,
        );
        states.push(("stolen", stolen));
        let mut unbound = Keymap::default();
        unbound.unset("roll", &ch("D"), Some("note.duplicate"));
        states.push(("unbound", unbound));
        let mut unlisted = Keymap::default();
        unlisted.set("roll", &ch("F12"), "draft.unlisted", None, Others::Keep);
        states.push(("unlisted", unlisted));
        for name in catalog::PRESET_NAMES {
            if name == "studio-pro" {
                continue;
            }
            let mut plain = Keymap {
                preset: name.to_string(),
                ..Keymap::default()
            };
            states.push(("preset", plain.clone()));
            plain.add = two.add.clone();
            states.push(("preset-mine", plain));
        }
        states
    }

    #[test]
    fn display_for_says_what_it_said() {
        for (name, k) in keymap_states() {
            let commands = every_command(&k);
            let rows = k.rows();
            for os in [Os::Win, Os::Mac] {
                for ctx in CONTEXTS.iter().map(|c| c.name).chain(["nowhere"]) {
                    for cmd in &commands {
                        assert_eq!(
                            k.display_for(ctx, cmd, os),
                            display_for_reference(&k, &rows, ctx, cmd, os),
                            "{name} {ctx} {cmd} {os:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn display_for_follows_every_way_the_map_changes() {
        let read = |k: &Keymap| k.display_for("roll", "note.delete", Os::Win);
        let mut k = Keymap::default();
        let before = read(&k);
        // A key the user adds shows at once, and goes when it is taken away.
        k.set("roll", &ch("F9"), "note.delete", None, Others::Keep);
        assert!(read(&k).contains("F9"), "{}", read(&k));
        k.unset("roll", &ch("F9"), Some("note.delete"));
        assert_eq!(read(&k), before);
        // A file another program wrote reads the new key.
        let dir = tempfile::tempdir().unwrap();
        let mut s = crate::Settings::open(dir.path());
        std::fs::write(
            dir.path().join("keymap.toml"),
            "[roll]\nF9 = \"note.delete\"\n",
        )
        .unwrap();
        assert!(s.reload_if_changed());
        assert!(read(&s.keymap).contains("F9"), "{}", read(&s.keymap));
        // A preset switch shows that preset's keys, the user's own kept.
        k.set("roll", &ch("F9"), "note.delete", None, Others::Keep);
        k.preset = "cubase".into();
        let mine = read(&k);
        assert!(mine.contains("F9"), "{mine}");
        assert_eq!(
            mine,
            display_for_reference(&k, &k.rows(), "roll", "note.delete", Os::Win)
        );
        // Back to the preset: the added key leaves.
        k.reset(None, None, None);
        let after = read(&k);
        assert!(!after.contains("F9"), "{after}");
        assert_eq!(
            after,
            display_for_reference(&k, &k.rows(), "roll", "note.delete", Os::Win)
        );
    }

    #[test]
    fn a_palettes_worth_of_keys_does_not_rebuild_the_table() {
        // The list of commands comes from another map so `k` is still cold.
        let commands = every_command(&Keymap::default());
        let k = Keymap::default();
        k.builds.set(0);
        k.row_builds.set(0);
        for cmd in &commands {
            k.display_for("roll", cmd, Os::Win);
        }
        assert_eq!(
            k.builds.get(),
            1,
            "the table is made once for a whole palette, not once a row"
        );
        assert_eq!(
            k.row_builds.get(),
            0,
            "no row list is made for the palette's key chips"
        );
    }

    #[test]
    fn the_row_list_is_made_once_while_the_map_stands() {
        let k = Keymap::default();
        let first = k.rows();
        k.builds.set(0);
        k.row_builds.set(0);
        for _ in 0..5 {
            assert_eq!(k.rows(), first);
        }
        assert_eq!(k.row_builds.get(), 0);
        assert_eq!(k.builds.get(), 0);
    }
}
