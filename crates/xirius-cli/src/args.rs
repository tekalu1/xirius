//! Turns a command line into a command name and a JSON argument map. The
//! argument names and types come from the registry, so a command added to
//! the core is on the command line without any change here.

use std::path::PathBuf;

use serde_json::{Map, Value};
use xirius_core::command::{ArgType, CommandDef};
use xirius_core::{CmdError, Registry};

/// Options that apply to the whole run, not to one command.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Global {
    pub song: Option<PathBuf>,
    pub json: bool,
    pub dry_run: bool,
    /// Run as an AI of this name (history marks it; permission applies).
    pub author: Option<String>,
    pub no_save: bool,
}

#[derive(Debug, PartialEq)]
pub enum Invocation {
    Help,
    Version,
    Init { dir: PathBuf, name: Option<String> },
    Batch,
    Mcp(McpArgs),
    Play(PlayOptions),
    Run { name: String, tokens: Vec<String> },
}

/// What `xirius play` was asked to do.
#[derive(Debug, Default, PartialEq)]
pub struct PlayOptions {
    /// Where to start (ticks or `bar.beat.tick`). `None` starts at the beginning.
    pub from: Option<String>,
    pub looping: bool,
    /// Play into a silent output instead of the sound device.
    pub no_device: bool,
    /// With `no_device`: render as fast as possible instead of at the speed of the clock.
    pub unpaced: bool,
    /// The MIDI port to record from (`all`, `none`, or a port's name).
    pub midi_in: Option<String>,
    /// A file of MIDI messages to record instead of a device.
    pub midi_script: Option<PathBuf>,
}

/// Options of `xirius mcp`.
#[derive(Debug, Default, PartialEq)]
pub struct McpArgs {
    pub bind: Option<std::net::IpAddr>,
    /// Run transport commands without opening an audio device.
    pub no_device: bool,
    /// Serve over HTTP on this PC instead of standard input and output.
    pub http: bool,
    /// 0 (the default) lets the OS choose.
    pub port: u16,
    /// Given on the command line or by `XIRIUS_MCP_TOKEN`; made when absent.
    pub token: Option<String>,
    /// Given by `--admin-token` or `XIRIUS_MCP_ADMIN_TOKEN`; made when absent.
    pub admin_token: Option<String>,
    /// Only the commands that look.
    pub read_only: bool,
    /// Seconds between automatic versions (0 = off); `None` = the plan's 30 s.
    pub autosave_sec: Option<u64>,
    /// The MIDI port to record from (`all`, `none`, or a port's name).
    pub midi_in: Option<String>,
    /// A file of MIDI messages to record instead of a device.
    pub midi_script: Option<PathBuf>,
}

fn parse_mcp(words: &[String]) -> Result<McpArgs, Usage> {
    let mut m = McpArgs {
        token: env_token("XIRIUS_MCP_TOKEN"),
        admin_token: env_token("XIRIUS_MCP_ADMIN_TOKEN"),
        ..McpArgs::default()
    };
    let mut i = 0;
    while i < words.len() {
        let (key, inline) = match words[i].split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
            _ => (words[i].as_str(), None),
        };
        let value = |i: &mut usize| -> Result<String, Usage> {
            if let Some(v) = &inline {
                return Ok(v.clone());
            }
            *i += 1;
            words
                .get(*i)
                .cloned()
                .ok_or_else(|| Usage(format!("{key} には値が必要です")))
        };
        match key {
            "--bind" => {
                m.bind = Some(
                    value(&mut i)?
                        .parse()
                        .map_err(|_| Usage("--bind には IP アドレスを指定してください".into()))?,
                );
                m.http = true;
            }
            "--http" => m.http = true,
            "--read-only" => m.read_only = true,
            "--no-device" | "--silent" => m.no_device = true,
            "--port" => {
                let v = value(&mut i)?;
                m.port = v.parse().map_err(|_| {
                    Usage(format!(
                        "--port には 0〜65535 の整数を指定してください（{v}）"
                    ))
                })?;
                m.http = true;
            }
            "--token" => m.token = Some(value(&mut i)?),
            "--midi-in" => m.midi_in = Some(value(&mut i)?),
            "--midi-script" => m.midi_script = Some(PathBuf::from(value(&mut i)?)),
            "--admin-token" => m.admin_token = Some(value(&mut i)?),
            "--autosave-sec" => {
                let v = value(&mut i)?;
                m.autosave_sec = Some(v.parse().map_err(|_| {
                    Usage(format!(
                        "--autosave-sec には 0 以上の整数を指定してください（{v}）"
                    ))
                })?);
            }
            other => return usage(format!("mcp に `{other}` というオプションはありません")),
        }
        i += 1;
    }
    Ok(m)
}

fn env_token(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// A mistake in how the command was typed (exit code 2).
#[derive(Debug, PartialEq)]
pub struct Usage(pub String);

fn usage<T>(msg: impl Into<String>) -> Result<T, Usage> {
    Err(Usage(msg.into()))
}

impl From<CmdError> for Usage {
    fn from(e: CmdError) -> Usage {
        Usage(e.message)
    }
}

/// Splits the global options off, from anywhere in the line.
pub fn parse(argv: &[String], registry: &Registry) -> Result<(Global, Invocation), Usage> {
    let mut g = Global::default();
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let t = argv[i].as_str();
        let (key, inline) = match t.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
            _ => (t, None),
        };
        match key {
            "--song" | "--as" => {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match argv.get(i) {
                            Some(v) => v.clone(),
                            None => return usage(format!("{key} には値が必要です")),
                        }
                    }
                };
                if key == "--song" {
                    g.song = Some(PathBuf::from(v));
                } else {
                    g.author = Some(v);
                }
            }
            "--json" => g.json = true,
            "--dry-run" => g.dry_run = true,
            "--no-save" => g.no_save = true,
            _ => rest.push(argv[i].clone()),
        }
        i += 1;
    }

    let Some(first) = rest.first().cloned() else {
        return Ok((g, Invocation::Help));
    };
    let inv = match first.as_str() {
        "help" | "-h" | "--help" => Invocation::Help,
        "version" | "-V" | "--version" => Invocation::Version,
        "batch" => Invocation::Batch,
        "mcp" => Invocation::Mcp(parse_mcp(&rest[1..])?),
        "play" => {
            let mut o = PlayOptions::default();
            let mut j = 1;
            while j < rest.len() {
                let (key, inline) = match rest[j].split_once('=') {
                    Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
                    _ => (rest[j].as_str(), None),
                };
                match key {
                    "--loop" => o.looping = true,
                    "--no-device" | "--silent" => o.no_device = true,
                    "--unpaced" => o.unpaced = true,
                    "--from" | "--midi-in" | "--midi-script" => {
                        let v = match inline {
                            Some(v) => v,
                            None => {
                                j += 1;
                                match rest.get(j) {
                                    Some(v) => v.clone(),
                                    None if key == "--from" => {
                                        return usage("--from には位置が必要です");
                                    }
                                    None => return usage(format!("{key} には値が必要です")),
                                }
                            }
                        };
                        match key {
                            "--from" => o.from = Some(v),
                            "--midi-in" => o.midi_in = Some(v),
                            _ => o.midi_script = Some(PathBuf::from(v)),
                        }
                    }
                    other => {
                        return usage(format!(
                            "play に `{other}` というオプションはありません（--from --loop --no-device --midi-in --midi-script）"
                        ));
                    }
                }
                j += 1;
            }
            Invocation::Play(o)
        }
        "init" => {
            let mut dir = None;
            let mut name = None;
            let mut j = 1;
            while j < rest.len() {
                if rest[j] == "--name" {
                    j += 1;
                    name = rest.get(j).cloned();
                    if name.is_none() {
                        return usage("--name には値が必要です");
                    }
                } else if dir.is_none() {
                    dir = Some(PathBuf::from(&rest[j]));
                } else {
                    return usage(format!("init の引数が多すぎます: {}", rest[j]));
                }
                j += 1;
            }
            let Some(dir) = dir.or_else(|| g.song.clone()) else {
                return usage("init <フォルダー> [--name 名前]");
            };
            Invocation::Init { dir, name }
        }
        _ => {
            let (name, used) = resolve_name(registry, &rest);
            Invocation::Run {
                name,
                tokens: rest[used..].to_vec(),
            }
        }
    };
    Ok((g, inv))
}

/// `ops list` → `ops.list`, `track add audio` → `track.add.audio`: bare words
/// are joined with dots until they name a registered command. Returns the
/// name and how many words it used.
pub fn resolve_name(registry: &Registry, words: &[String]) -> (String, usize) {
    let mut acc = String::new();
    for (i, w) in words.iter().enumerate() {
        if w.starts_with("--") {
            break;
        }
        if !acc.is_empty() {
            acc.push('.');
        }
        acc.push_str(w);
        if registry.get(&acc).is_some() {
            return (acc, i + 1);
        }
    }
    (words[0].clone(), 1)
}

fn is_bool_word(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "true" | "false" | "on" | "off" | "yes" | "no" | "0" | "1"
    )
}

fn find_arg<'a>(def: &'a CommandDef, key: &str) -> Option<&'a xirius_core::command::ArgDef> {
    let norm = key.replace('-', "_");
    // An alias (`seek --to`) is the argument under its own name.
    def.args
        .iter()
        .find(|a| a.name == key || a.name.replace('-', "_") == norm || a.aliases.contains(&key))
}

/// `--by 12 --ids a,b nte_9` → `{"by": "12", "ids": "a,b,nte_9"}`. Values stay
/// strings; the registry's `normalize` reads them as the declared types.
pub fn parse_args(def: &CommandDef, tokens: &[String]) -> Result<Map<String, Value>, Usage> {
    let mut out = Map::new();
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if t.len() > 2 && t.starts_with("--") {
            let (key, inline) = match t[2..].split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (&t[2..], None),
            };
            let Some(arg) = find_arg(def, key) else {
                let valid: Vec<String> = def.args.iter().map(|a| format!("--{}", a.name)).collect();
                return usage(format!(
                    "{} に `--{key}` という引数はありません（使用できる引数: {}）",
                    def.name,
                    if valid.is_empty() {
                        "なし".to_string()
                    } else {
                        valid.join(" ")
                    }
                ));
            };
            let value = match inline {
                Some(v) => v,
                None if arg.ty == ArgType::Bool => match tokens.get(i + 1) {
                    Some(n) if is_bool_word(n) => {
                        i += 1;
                        n.clone()
                    }
                    _ => "true".to_string(),
                },
                None => match tokens.get(i + 1) {
                    Some(n) if !n.starts_with("--") => {
                        i += 1;
                        n.clone()
                    }
                    _ => return usage(format!("--{key} には値が必要です")),
                },
            };
            put(&mut out, arg.name, &arg.ty, value);
        } else {
            positional.push(t);
        }
        i += 1;
    }

    // Bare words fill the arguments not given by name, in declared order.
    let mut words = positional.into_iter();
    for arg in &def.args {
        if arg.named_only || out.contains_key(arg.name) {
            continue;
        }
        match &arg.ty {
            ArgType::Ids(_) => {
                let all: Vec<&str> = words.by_ref().collect();
                if !all.is_empty() {
                    out.insert(arg.name.to_string(), Value::String(all.join(",")));
                }
            }
            _ => {
                if let Some(w) = words.next() {
                    out.insert(arg.name.to_string(), Value::String(w.to_string()));
                }
            }
        }
    }
    if let Some(extra) = words.next() {
        return usage(format!("{} の引数が多すぎます: {extra}", def.name));
    }
    Ok(out)
}

fn put(out: &mut Map<String, Value>, name: &str, ty: &ArgType, value: String) {
    if matches!(ty, ArgType::Ids(_))
        && let Some(Value::String(old)) = out.get(name)
    {
        let joined = format!("{old},{value}");
        out.insert(name.to_string(), Value::String(joined));
        return;
    }
    out.insert(name.to_string(), Value::String(value));
}

/// Splits a line of a batch file like a shell would: spaces separate words,
/// `'…'` is literal, `"…"` allows `\"` and `\\`. Elsewhere a backslash is
/// just a backslash, so Windows paths work.
pub fn split_line(line: &str) -> Result<Vec<String>, Usage> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return usage("' が閉じられていません"),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                            cur.push(chars.next().unwrap_or('\\'));
                        }
                        Some(c) => cur.push(c),
                        None => return usage("\" が閉じられていません"),
                    }
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn global_options_can_stand_anywhere() {
        let r = Registry::standard();
        let (g, inv) = parse(
            &s(&[
                "note.transpose",
                "--by",
                "12",
                "--dry-run",
                "--song=x",
                "--json",
            ]),
            &r,
        )
        .unwrap();
        assert!(g.dry_run && g.json);
        assert_eq!(g.song, Some(PathBuf::from("x")));
        assert_eq!(
            inv,
            Invocation::Run {
                name: "note.transpose".into(),
                tokens: s(&["--by", "12"])
            }
        );
    }

    #[test]
    fn bare_words_join_into_a_command_name() {
        let r = Registry::standard();
        let (_, inv) = parse(&s(&["ops", "list", "--grep", "note"]), &r).unwrap();
        assert_eq!(
            inv,
            Invocation::Run {
                name: "ops.list".into(),
                tokens: s(&["--grep", "note"])
            }
        );
        let (_, inv) = parse(&s(&["track", "add", "audio", "Vox"]), &r).unwrap();
        assert_eq!(
            inv,
            Invocation::Run {
                name: "track.add.audio".into(),
                tokens: s(&["Vox"])
            }
        );
    }

    #[test]
    fn unknown_options_and_missing_values_are_usage_errors() {
        let r = Registry::standard();
        let def = r.get("note.transpose").unwrap();
        assert!(parse_args(def, &s(&["--nope", "1"])).is_err());
        assert!(parse_args(def, &s(&["--by"])).is_err());
        // Bare words fill what was not named; `--ids` already took it.
        assert!(parse_args(def, &s(&["--by", "1", "--ids", "c", "d"])).is_err());
    }

    #[test]
    fn the_overlap_modes_are_given_by_name_only() {
        let r = Registry::standard();
        let def = r.get("note.transpose").unwrap();
        let m = parse_args(def, &s(&["--by", "1", "--overlap", "trim", "c", "d"])).unwrap();
        assert_eq!(m["overlap"], "trim");
        assert_eq!(m["ids"], "c,d");
        let def = r.get("clip.move").unwrap();
        let m = parse_args(def, &s(&["--no-overlap", "c"])).unwrap();
        assert_eq!(m["no-overlap"], "true");
        assert_eq!(m["ids"], "c");
    }

    #[test]
    fn batch_lines_split_like_a_shell() {
        assert_eq!(
            split_line(r#"track.add.audio "Lead Vox" --color 'red'"#).unwrap(),
            s(&["track.add.audio", "Lead Vox", "--color", "red"])
        );
        assert_eq!(
            split_line(r"project.save-as D:\songs\a").unwrap(),
            s(&["project.save-as", r"D:\songs\a"])
        );
        assert!(split_line("\"open").is_err());
        assert_eq!(split_line("   ").unwrap(), Vec::<String>::new());
    }

    /// The ID prefix of each kind the song has. A kind none of them is fails the test by its name.
    const IDS: [&str; 11] = [
        "trk", "fld", "clp", "nte", "plg", "snd", "sec", "mrk", "tmp", "tsg", "nod",
    ];

    /// Values of an argument's type, each as an MCP client sends it and as it is typed on the
    /// command line. A new `ArgType` does not build until it has its row here.
    fn samples(a: &xirius_core::command::ArgDef) -> Vec<(Value, String)> {
        use serde_json::json;
        // Two of each kind, so that a list of one kind has more than one ID.
        let ids = |kind: xirius_core::id::IdKind| -> Vec<String> {
            IDS.into_iter()
                .filter(|p| kind.accepts(&format!("{p}_3").parse().unwrap()))
                .flat_map(|p| [format!("{p}_3"), format!("{p}_7")])
                .collect()
        };
        let both = |v: Value| (v.clone(), v.as_str().map_or(v.to_string(), str::to_string));
        match &a.ty {
            ArgType::Bool => vec![(json!(true), "on".into()), (json!(false), "false".into())],
            ArgType::Int => {
                let lo = a.min.map_or(-3, |m| m.ceil() as i64);
                let hi = a.max.map_or(lo + 12, |m| m.floor() as i64);
                [lo, hi].map(|n| both(json!(n))).to_vec()
            }
            ArgType::Float => {
                let lo = a.min.unwrap_or(-1.5);
                let hi = a.max.unwrap_or(lo + 2.25);
                [lo, (lo + hi) / 2.0, hi].map(|n| both(json!(n))).to_vec()
            }
            ArgType::Text => vec![both(json!("Lead Vox")), (json!(12), "12".into())],
            ArgType::Id(kind) => ids(*kind).into_iter().map(|s| both(json!(s))).collect(),
            ArgType::Ids(kind) => match ids(*kind) {
                ids if ids.is_empty() => vec![],
                ids => vec![
                    (json!([ids[0]]), ids[0].clone()),
                    (json!(ids), ids.join(",")),
                ],
            },
            ArgType::Choice(options) => options.iter().map(|o| both(json!(o))).collect(),
            ArgType::Position => vec![both(json!("2.1.0")), both(json!("960"))],
            ArgType::Grid => vec![
                both(json!("1/8t")),
                both(json!("1/16d")),
                both(json!("120")),
            ],
            ArgType::Pitch => vec![
                (json!(60), "C3".into()),
                (json!("F#2"), "F#2".into()),
                (json!(127), "127".into()),
            ],
            ArgType::Objects => {
                let list = json!([{"pad": "A1", "at": 0}, {"pad": "A2"}]);
                vec![(list.clone(), list.to_string())]
            }
        }
    }

    /// Every command the program has, every argument and every way of typing it on the command
    /// line (`--name value`, `--name=value`, an alias, a bare word, a flag alone, an ID list
    /// given twice) reads as the same arguments as the JSON an MCP client sends for it.
    #[test]
    fn every_argument_of_every_command_reads_as_the_mcp_call_does() {
        let registry = xirius_session::commands::registry();
        let mut wrong = Vec::new();
        let mut checked = 0;
        for def in registry.all() {
            // Each other required argument is given by name, so the arguments are complete.
            let first = |a: &xirius_core::command::ArgDef| samples(a).swap_remove(0);
            for (k, a) in def.args.iter().enumerate() {
                let samples = samples(a);
                if samples.is_empty() {
                    wrong.push(format!(
                        "{} --{}: no ID of kind {:?}",
                        def.name, a.name, a.ty
                    ));
                    continue;
                }
                let others = |before_too: bool| {
                    let mut tokens = Vec::new();
                    let mut json = Map::new();
                    for (j, b) in def.args.iter().enumerate() {
                        if j != k && (b.required || (before_too && j < k && !b.named_only)) {
                            let (v, text) = first(b);
                            tokens.extend([format!("--{}", b.name), text]);
                            json.insert(b.name.to_string(), v);
                        }
                    }
                    (tokens, json)
                };
                for (value, text) in samples {
                    let mut forms: Vec<(String, Vec<String>, bool)> = vec![
                        (
                            "named".into(),
                            vec![format!("--{}", a.name), text.clone()],
                            false,
                        ),
                        ("inline".into(), vec![format!("--{}={text}", a.name)], false),
                    ];
                    for alias in a.aliases {
                        let tokens = vec![format!("--{alias}"), text.clone()];
                        forms.push((format!("alias --{alias}"), tokens, false));
                    }
                    if a.ty == ArgType::Bool && value == Value::Bool(true) {
                        forms.push(("a flag alone".into(), vec![format!("--{}", a.name)], false));
                    }
                    if let ArgType::Ids(_) = a.ty {
                        let twice = text
                            .split(',')
                            .flat_map(|id| [format!("--{}", a.name), id.to_string()])
                            .collect();
                        forms.push(("named once per ID".into(), twice, false));
                    }
                    if !a.named_only {
                        let words = match a.ty {
                            ArgType::Ids(_) => text.split(',').map(str::to_string).collect(),
                            _ => vec![text.clone()],
                        };
                        forms.push(("a bare word".into(), words, true));
                    }
                    for (form, tokens, positional) in forms {
                        checked += 1;
                        let (mut line, mut json) = others(positional);
                        line.extend(tokens);
                        json.insert(a.name.to_string(), value.clone());
                        let mcp = registry.normalize(def, &json).map(|a| a.0);
                        let cli = parse_args(def, &line)
                            .map_err(|Usage(m)| format!("usage: {m}"))
                            .and_then(|raw| {
                                registry
                                    .normalize(def, &raw)
                                    .map(|a| a.0)
                                    .map_err(|e| e.message)
                            });
                        match (&mcp, &cli) {
                            (Ok(m), Ok(c)) if m == c => {}
                            (Err(e), _) => wrong.push(format!(
                                "{} --{} ({form}): the sample {value} is refused: {}",
                                def.name, a.name, e.message
                            )),
                            _ => wrong.push(format!(
                                "{} --{} ({form}): `{}` reads as {cli:?}, the MCP call {} as {mcp:?}",
                                def.name,
                                a.name,
                                line.join(" "),
                                Value::Object(json)
                            )),
                        }
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {checked} command lines read otherwise than the MCP call, the first:\n{}",
            wrong.len(),
            wrong[..wrong.len().min(12)].join("\n")
        );
    }
}
