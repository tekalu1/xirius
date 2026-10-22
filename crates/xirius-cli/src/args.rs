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
    fn arguments_follow_the_registry_types() {
        let r = Registry::standard();
        let def = r.get("note.transpose").unwrap();
        let m = parse_args(def, &s(&["--by", "-12", "nte_1", "nte_2"])).unwrap();
        assert_eq!(m["by"], "-12");
        assert_eq!(m["ids"], "nte_1,nte_2");
        let m = parse_args(def, &s(&["--ids", "nte_1", "--ids", "nte_3", "--by=3"])).unwrap();
        assert_eq!(m["ids"], "nte_1,nte_3");
        assert_eq!(m["by"], "3");
    }

    #[test]
    fn a_flag_takes_a_value_only_when_one_follows() {
        let r = Registry::standard();
        let def = r.get("track.mute").unwrap();
        let m = parse_args(def, &s(&["trk_1", "--on"])).unwrap();
        assert_eq!(m["on"], "true");
        let m = parse_args(def, &s(&["--on", "false", "trk_1"])).unwrap();
        assert_eq!(m["on"], "false");
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
}
