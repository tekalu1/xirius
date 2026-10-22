//! The command-line program: opens the song, runs one command (or a batch,
//! or the MCP server) and saves.

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};
use xirius_core::autosave::DEFAULT_INTERVAL;
use xirius_core::command::{ArgDef, ArgType, CommandDef};
use xirius_core::session::{RunOptions, RunResult};
use xirius_core::{AiPermission, CmdError, ErrorCode, Project, Registry, Risk, Session};
use xirius_mcp::persist::{load_selection, result_json, store_selection};
use xirius_mcp::{HttpConfig, HttpServer, Scope, ServeOptions, run_http, run_stdio};
use xirius_prefs::Settings;
use xirius_session::cleanup;
use xirius_session::commands::{self, CLEANUP};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::host::SessionHost;
use xirius_session::output::OutputMode;

use crate::args::{self, Global, Invocation, McpArgs, Usage};
use crate::{play, render};

pub const EXIT_OK: u8 = 0;
pub const EXIT_COMMAND: u8 = 1;
pub const EXIT_USAGE: u8 = 2;

const HELP: &str = "\
xirius - 登録されたコマンドで Xirius の曲を操作します

使い方:
  xirius [--song <曲のフォルダー>] <コマンド> [--引数 値 ...]
  xirius init <フォルダー> [--name 曲名]      新しい曲を作成
  xirius batch [--song <曲>] < 手順.txt        1 行 1 コマンドで一括実行
  xirius mcp --song <曲> [--http] [...]        MCP サーバーを起動（標準入出力。--http で 127.0.0.1 の HTTP）
  xirius play [--from 位置] [--loop] [--no-device] [--midi-in ポート] [--midi-script ファイル]
                                               再生しながらコマンドを受け付ける（標準入力。quit で終了）
  xirius ops list [--grep 語]                  コマンドの一覧
  xirius ops schema [コマンド名]               引数の JSON Schema
  xirius export.mixdown --range song --out mix.wav
                                               WAV・FLAC・MP3 にミックスダウン（ステムは export.stems）
  xirius export.status [--job ジョブ]          書き出しの一覧と進行状況（曲のフォルダーの exports/ に記録）
  xirius prefs set edit.automation.follows-clips false
  xirius audio get
  xirius audio set buffer 128
  xirius midi set follow-selection true
                                               設定を変更（曲は不要。画面の設定と同じファイル）
  xirius keymap set Mod+Alt+V \"note.transpose --by 12\"
                                               キーを割り当て（keymap.get で一覧、keymap.conflicts で衝突）
  xirius <コマンド> --help                     そのコマンドの引数

共通オプション:
  --song <フォルダー>   対象の曲（省略時は環境変数 XIRIUS_SONG、それも無ければ現在のフォルダー）
  --dry-run             曲は変更せず、変更内容だけを表示
  --json                結果を JSON で出力
  --as <名前>           AI として実行（履歴に AI として記録し、AI の権限を適用）
  --no-save             実行後に曲を保存しない（履歴には記録）

play と mcp は、保存した曲を閉じるときに、どのクリップにも履歴にもバージョンにも
使われていない録音のファイルをごみ箱に移動します（project.cleanup。--dry-run で一覧だけ）。
環境変数 XIRIUS_TRASH_DIR を指定すると、ごみ箱の代わりにそのフォルダーへ移動します。

設定（prefs.*・keymap.*・mousemap.*）は、画面の設定と同じフォルダーの prefs.toml・keymap.toml・
mousemap.toml に保存します。環境変数 XIRIUS_CONFIG_DIR で別のフォルダーを指定できます。

mcp のオプション:
  --http                127.0.0.1 の HTTP で待ち受ける（起動時に token を 2 つ表示）
  --port <番号>         HTTP のポート（省略時は空いているポート）
  --token <文字列>      AI 用の token（省略時は自動生成。環境変数 XIRIUS_MCP_TOKEN でも指定可）
  --admin-token <文字列>  承認用の token（XIRIUS_MCP_ADMIN_TOKEN でも指定可。AI 用とは別の値にする）
  --read-only           読み取り専用のコマンドだけを公開
  --no-device           オーディオを出力せずに再生を動かす
  --autosave-sec <秒>   自動保存の間隔（既定 30。0 で無効）
  --midi-in <ポート>    録音する MIDI 入力のポート（all ですべて、none で使わない。既定はオーディオ出力があればすべて）
  --midi-script <ファイル>  MIDI 入力デバイスの代わりに、JSON のメッセージ [[フレーム, [0x90, 60, 100]], ...] を録音する

終了コード: 0 成功 / 1 コマンドの失敗 / 2 使い方の誤り

例:
  xirius init song && cd song
  xirius track.add.audio --name Vocal
  xirius note.transpose --by 12 --dry-run
  xirius ops list --grep note
";

pub fn main(argv: Vec<String>) -> u8 {
    let registry = commands::shared_registry();
    let (global, inv) = match args::parse(&argv, &registry) {
        Ok(v) => v,
        Err(Usage(m)) => return usage_error(&m),
    };
    match inv {
        Invocation::Help => {
            print!("{HELP}");
            EXIT_OK
        }
        Invocation::Version => {
            println!("xirius {}", env!("CARGO_PKG_VERSION"));
            EXIT_OK
        }
        Invocation::Init { dir, name } => init(&global, &dir, name),
        Invocation::Run { name, tokens } => run_one(&global, &registry, &name, &tokens),
        Invocation::Batch => batch(&global, &registry),
        Invocation::Mcp(m) => serve_mcp(&global, &m),
        Invocation::Play(opts) => play::play(&global, &registry, opts),
    }
}

pub fn usage_error(msg: &str) -> u8 {
    eprintln!("使い方が正しくありません: {msg}");
    eprintln!("（xirius help で使い方を表示）");
    EXIT_USAGE
}

pub fn report_error(g: &Global, e: &CmdError) -> u8 {
    if g.json {
        println!("{}", json!({"ok": false, "error": e}));
    } else {
        let code = serde_json::to_value(e.code)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        eprintln!("error[{code}]: {}", e.message);
    }
    EXIT_COMMAND
}

fn init(g: &Global, dir: &Path, name: Option<String>) -> u8 {
    let name = name.unwrap_or_else(|| {
        dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled")
            .to_string()
    });
    match Session::create(dir, Project::new(name.clone())) {
        Ok(_) => {
            if g.json {
                println!("{}", json!({"ok": true, "song": dir, "name": name}));
            } else {
                println!("曲「{name}」を {} に作成しました", dir.display());
            }
            EXIT_OK
        }
        Err(e) => report_error(g, &e),
    }
}

/// The song folder: `--song`, `XIRIUS_SONG`, or the current folder when it
/// holds a song.
pub fn song_dir(g: &Global) -> Option<PathBuf> {
    if let Some(d) = &g.song {
        return Some(d.clone());
    }
    if let Some(d) = std::env::var_os("XIRIUS_SONG").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let cwd = std::env::current_dir().ok()?;
    cwd.join("manifest.toml").is_file().then_some(cwd)
}

/// An empty song for commands that need no song (`ops list`, a typo to explain).
pub fn blank(registry: &Arc<Registry>) -> Session {
    let mut s = Session::with_registry(Project::new("untitled"), registry.clone());
    commands::attach(&mut s, ExtConfig::default());
    attach_settings(&mut s);
    s
}

/// The person's settings (`prefs.*`, `keymap.*`, `mousemap.*`): the folder the app reads,
/// or `XIRIUS_CONFIG_DIR`. They need no song.
fn attach_settings(s: &mut Session) -> xirius_prefs::Shared {
    let settings = Settings::open(xirius_prefs::config_dir()).shared();
    xirius_prefs::attach(s, settings.clone());
    let mut io = xirius_session::audio_settings::Io::new(Box::new(
        xirius_engine::devices::Unopened::enumerate(),
    ));
    io.view.ports = xirius_engine::midi_in::midi_ports().unwrap_or_default();
    s.add_extension(Box::new(xirius_session::audio_settings::IoExt {
        io: Arc::new(std::sync::Mutex::new(io)),
        settings: settings.clone(),
    }));
    s.add_extension(Box::new(xirius_session::app::AppExt::standard(
        settings.clone(),
    )));
    settings
}

pub fn open(g: &Global, registry: &Arc<Registry>) -> Result<Option<Session>, CmdError> {
    open_with_config(g, registry, ExtConfig::default(), ExtExtras::default())
}

fn open_with_config(
    g: &Global,
    registry: &Arc<Registry>,
    config: ExtConfig,
    extras: ExtExtras,
) -> Result<Option<Session>, CmdError> {
    let Some(dir) = song_dir(g) else {
        return Ok(None);
    };
    let mut s = Session::open_with(&dir, registry.clone())?;
    for n in &s.notices {
        eprintln!("お知らせ: {n}");
    }
    if let Some(r) = s.recovery() {
        eprintln!("お知らせ: {}", r.line(str::to_string));
    }
    load_selection(&mut s);
    let settings = attach_settings(&mut s);
    let extras = ExtExtras {
        io_settings: Some(settings),
        trash: extras.trash.or_else(cleanup::trash_from_env),
        ..extras
    };
    commands::attach_with(&mut s, config, extras);
    Ok(Some(s))
}

/// Closing a saved song (`xirius play`): the recordings nothing uses any more go to the
/// recycle bin. Said on stderr when there were any.
pub fn close(g: &Global, session: &mut Session) {
    if g.no_save || session.dir().is_none() {
        return;
    }
    let mut opts = RunOptions::human();
    opts.dry_run = g.dry_run;
    match commands::run(session, None, CLEANUP, &Map::new(), &opts) {
        Ok(r) if g.dry_run || r.data["moved"].as_array().is_some_and(|m| !m.is_empty()) => {
            eprintln!("{}", r.message);
        }
        Ok(_) => {}
        Err(e) => eprintln!("注意: {}", e.message),
    }
}

pub fn options(g: &Global) -> RunOptions {
    let mut o = match &g.author {
        Some(n) => RunOptions::ai(n.clone()),
        None => RunOptions::human(),
    };
    o.dry_run = g.dry_run;
    o
}

pub fn no_song() -> CmdError {
    CmdError::new(
        ErrorCode::InvalidArgs,
        "曲が指定されていません。--song <フォルダー> か環境変数 XIRIUS_SONG で指定してください（新しい曲は xirius init <フォルダー> で作成）",
    )
}

/// Runs a command and, if the song changed, saves it. A person's edit always
/// saves; an AI's only when it holds the `save` permission (its steps are in
/// the history either way, so nothing is lost).
///
/// With a running engine, `host` follows the song after the command. The export and
/// transport commands of `xirius-session` are answered here too.
pub fn run_and_persist(
    s: &mut Session,
    host: Option<&mut SessionHost>,
    name: &str,
    raw: &Map<String, Value>,
    opts: &RunOptions,
    save: bool,
) -> Result<RunResult, CmdError> {
    let risk = s.registry().get(name).map(|d| d.risk);
    let before = s.state.selection.clone();
    let r = commands::run(s, host, name, raw, opts)?;
    if s.state.selection != before {
        store_selection(s);
    }
    let may_save = !opts.author.is_ai() || s.ai_permission() == AiPermission::Save;
    if save
        && may_save
        && !r.dry_run
        && r.decision.is_none()
        && risk.is_some_and(|k| k != Risk::Read)
        && s.dir().is_some()
        && s.is_dirty()
    {
        s.save()?;
    }
    Ok(r)
}

pub fn print_result(g: &Global, r: &RunResult) {
    if g.json {
        println!("{}", result_json(r));
    } else {
        print!("{}", render::run_result(r));
    }
}

fn run_one(g: &Global, registry: &Arc<Registry>, name: &str, tokens: &[String]) -> u8 {
    let Some(def) = registry.get(name) else {
        // Let the session explain (it lists near names).
        return match blank(registry).run(name, &Map::new(), &options(g)) {
            Err(e) => report_error(g, &e),
            Ok(_) => EXIT_OK,
        };
    };
    if tokens.iter().any(|t| t == "--help" || t == "-h") {
        print!("{}", command_help(def));
        return EXIT_OK;
    }
    let raw = match args::parse_args(def, tokens) {
        Ok(m) => m,
        Err(Usage(m)) => return usage_error(&m),
    };
    if !g.dry_run && def.args.iter().any(|a| a.name == "background") {
        match registry.normalize(def, &raw) {
            Ok(args) if args.flag("background") => {
                return report_error(
                    g,
                    &CmdError::invalid(
                        "--background は継続中のセッション（mcp・play・batch）で使用します。単発の書き出しでは省略してください",
                    ),
                );
            }
            Err(e) => return report_error(g, &e),
            _ => {}
        }
    }
    let mut session = match open(g, registry) {
        Ok(Some(s)) => s,
        Ok(None)
            if def.risk == Risk::Read
                || name == "plugin.rescan"
                || (xirius_prefs::is_settings_command(name)
                    || xirius_session::audio_settings::NAMES.contains(&name)) =>
        {
            blank(registry)
        }
        Ok(None) => return report_error(g, &no_song()),
        Err(e) => return report_error(g, &e),
    };
    match run_and_persist(&mut session, None, name, &raw, &options(g), !g.no_save) {
        Ok(r) => {
            print_result(g, &r);
            EXIT_OK
        }
        Err(e) => report_error(g, &e),
    }
}

fn batch(g: &Global, registry: &Arc<Registry>) -> u8 {
    let mut session = match open(g, registry) {
        Ok(Some(s)) => s,
        Ok(None) => blank(registry),
        Err(e) => return report_error(g, &e),
    };
    let stdin = io::stdin();
    for (i, line) in stdin.lock().lines().enumerate() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = i + 1;
        let words = match args::split_line(line) {
            Ok(w) => w,
            Err(Usage(m)) => return usage_error(&format!("{at} 行目: {m}")),
        };
        let (name, used) = args::resolve_name(registry, &words);
        let Some(def) = registry.get(&name) else {
            let e = blank(registry).run(&name, &Map::new(), &options(g)).err();
            eprintln!("{at} 行目:");
            return e.map_or(EXIT_COMMAND, |e| report_error(g, &e));
        };
        let raw = match args::parse_args(def, &words[used..]) {
            Ok(m) => m,
            Err(Usage(m)) => return usage_error(&format!("{at} 行目: {m}")),
        };
        if session.dir().is_none()
            && def.risk != Risk::Read
            && !(xirius_prefs::is_settings_command(&name)
                || xirius_session::audio_settings::NAMES.contains(&name.as_str()))
        {
            return report_error(g, &no_song());
        }
        match run_and_persist(&mut session, None, &name, &raw, &options(g), !g.no_save) {
            Ok(r) => print_result(g, &r),
            Err(e) => {
                eprintln!("{at} 行目で停止しました:");
                return report_error(g, &e);
            }
        }
    }
    EXIT_OK
}

fn serve_mcp(g: &Global, m: &McpArgs) -> u8 {
    let config = ExtConfig {
        output: if m.no_device {
            OutputMode::Silent { paced: true }
        } else {
            OutputMode::Device
        },
        ..ExtConfig::default()
    };
    let midi =
        match crate::midi::source(m.midi_in.as_deref(), m.midi_script.as_deref(), !m.no_device) {
            Ok(midi) => midi,
            Err(e) => return report_error(g, &e),
        };
    let extras = ExtExtras {
        midi,
        midi_out: (!m.no_device).then(xirius_session::midi_out::system_maker),
        ..ExtExtras::default()
    };
    let session = match open_with_config(g, &commands::shared_registry(), config, extras) {
        Ok(Some(s)) => s,
        Ok(None) => return report_error(g, &no_song()),
        Err(e) => return report_error(g, &e),
    };
    let opts = ServeOptions {
        author: g.author.clone().unwrap_or_else(|| "mcp".to_string()),
        scope: if m.read_only {
            Scope::Read
        } else {
            Scope::Edit
        },
        save: !g.no_save,
        autosave: match m.autosave_sec {
            None => Some(DEFAULT_INTERVAL),
            Some(0) => None,
            Some(n) => Some(Duration::from_secs(n)),
        },
    };
    let out = if m.http {
        let cfg = HttpConfig {
            bind: m.bind.unwrap_or_else(|| HttpConfig::default().bind),
            port: m.port,
            token: m.token.clone(),
            admin_token: m.admin_token.clone(),
        };
        run_http(session, &opts, cfg, |srv| announce(g, srv))
    } else {
        run_stdio(session, &opts)
    };
    match out {
        Ok(()) => EXIT_OK,
        Err(e) => {
            eprintln!("error[io]: {e}");
            EXIT_COMMAND
        }
    }
}

/// Tells the person where the server is and the two tokens. They go to
/// stderr (or one JSON line on stdout with `--json`), never into a log file.
fn announce(g: &Global, srv: &HttpServer) {
    if g.json {
        println!(
            "{}",
            json!({
                "ok": true,
                "mcp_url": srv.mcp_url(),
                "events_url": srv.events_url(),
                "admin_url": srv.admin_url(),
                "token": srv.token,
                "admin_token": srv.admin_token,
            })
        );
    }
    eprintln!("MCP: {}", srv.mcp_url());
    eprintln!("  AI 用の token（Authorization: Bearer）: {}", srv.token);
    eprintln!(
        "  承認用の token（{}）: {}",
        srv.admin_url(),
        srv.admin_token
    );
    eprintln!("終了するには Ctrl-C を押してください");
}

fn type_name(t: &ArgType) -> String {
    match t {
        ArgType::Bool => "true|false".into(),
        ArgType::Int => "整数".into(),
        ArgType::Float => "数値".into(),
        ArgType::Text => "文字列".into(),
        ArgType::Id(k) => format!("{k:?} の ID").to_lowercase(),
        ArgType::Ids(k) => format!("{k:?} の ID（複数可）").to_lowercase(),
        ArgType::Choice(o) => o.join("|"),
        ArgType::Position => "位置（小節.拍.tick または tick）".into(),
        ArgType::Objects => "JSON（オブジェクトの配列）".into(),
        ArgType::Grid => "グリッド（1/16・1/8t・tick）".into(),
    }
}

fn arg_line(a: &ArgDef) -> String {
    let mut s = format!("  --{} <{}>", a.name, type_name(&a.ty));
    if a.required {
        s.push_str(" 必須");
    }
    if let Some(d) = &a.default {
        s.push_str(&format!(" 既定 {d}"));
    }
    if let (Some(lo), Some(hi)) = (a.min, a.max) {
        s.push_str(&format!(" {lo}〜{hi}"));
    }
    s.push_str(&format!("  {}", a.help));
    s
}

pub fn command_help(def: &CommandDef) -> String {
    let mut s = format!("{}\n  {}\n", def.name, def.summary);
    s.push_str(&format!(
        "  履歴: {}  危険度: {}\n",
        if def.history {
            "記録する"
        } else {
            "記録しない"
        },
        serde_json::to_value(def.risk)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    ));
    if def.risk != Risk::Read {
        s.push_str("  --dry-run  曲は変更せず、変更内容だけを表示します\n");
    }
    for a in &def.args {
        s.push_str(&arg_line(a));
        s.push('\n');
    }
    s
}
