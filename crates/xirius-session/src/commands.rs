//! The commands core cannot run itself: exporting needs the engine, the transport needs a
//! running one and the plugin list needs the disk. They are in the command table (so
//! `commands`, the CLI help and the MCP tool list show them with the rest) with
//! `Handler::Session`, and [`crate::ext::SessionExt`], added to a session with [`attach`],
//! answers them.
//!
//! - `export.mixdown`, `export.stems`, `export.status`, `export.cancel`, `export.update`,
//!   `export.open`, `export.reveal_os`, `export.delete` (EXP-001, EXP-002, EXP-010, EXP-011,
//!   EXP-013 to EXP-018, EXP-024, EXP-025, AI-002)
//! - `transport.play`, `transport.stop`, `transport.seek`, `transport.loop`, `transport.status`
//!   (ARR-001, ENG-009)
//! - `plugin.rescan`, `browser.plugin.list` (PLG-001, PLG-003, BRW-009, PLG-021)
//! - `plugin.scan.status`: how the scan stands and what it found (PLG-059)
//! - `plugin.status`, `plugin.restart`, `plugin.collect`: the plugins in the plugin host
//!   process of a running engine (OPEN-02, PLG-022, PLG-023, PLG-024)
//! - `audio.test-tone`: the test tone on the device of a running engine (UI-260)
//! - `record.start`, `record.stop`, `record.status`: defined in core's table beside
//!   `record.take.add`, answered here with a running engine ([`crate::record`]; REC-001,
//!   REC-004, REC-016)
//! - `mix.listen`: the levels, the loudness of each bar and the spectrum of a range, rendered
//!   offline ([`crate::listen`]; AI-004, AI-013)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value, json};
use xirius_core::command::{ArgDef, Handler};
use xirius_core::diff::ChangeSet;
use xirius_core::id::{FolderId, IdKind, PluginId, TrackId};
use xirius_core::model::{Project, TrackKind};
use xirius_core::session::{RunOptions, RunResult};
use xirius_core::{Args, CmdError, CmdResult, CommandDef, Registry, Risk, Session};

use crate::export::{self, Format, Options, RangeSpec, StemSet};
use crate::ext::{ExtConfig, ExtExtras, SessionExt};
use crate::host::{SessionHost, collect_plugin_states};

/// Names [`crate::ext::SessionExt`] answers.
pub const NAMES: [&str; 27] = [
    "export.mixdown",
    "export.selection",
    "export.stems",
    "export.status",
    "export.cancel",
    "export.update",
    "export.open",
    "export.preview",
    "export.import",
    "export.reveal_os",
    "export.delete",
    "transport.play",
    "transport.stop",
    "transport.seek",
    "transport.goto",
    "transport.loop",
    "transport.status",
    "plugin.rescan",
    "plugin.scan.status",
    "browser.plugin.list",
    "plugin.status",
    "plugin.restart",
    "plugin.reload",
    "plugin.collect",
    "plugin.stats",
    "engine.stats",
    "audio.test-tone",
];

/// The recording commands of core's table, answered here too.
pub const RECORD: [&str; 3] = ["record.start", "record.stop", "record.status"];

/// Core's command [`crate::cleanup`] answers.
pub const CLEANUP: &str = "project.cleanup";

/// Commands a running engine answers, beside the transport.
const ON_HOST: [&str; 6] = [
    "plugin.status",
    "plugin.restart",
    "plugin.reload",
    "plugin.stats",
    "engine.stats",
    "audio.test-tone",
];

fn def(name: &'static str, summary: &'static str, args: Vec<ArgDef>, risk: Risk) -> CommandDef {
    CommandDef {
        name,
        summary,
        args,
        history: false,
        risk,
        handler: Handler::Session,
        screen_only: false,
    }
}

fn range_args() -> Vec<ArgDef> {
    vec![
        ArgDef::text(
            "range",
            "書き出す範囲（song = 曲の先頭から最後のクリップの終わりまで、loop = ループ範囲、section:名前または id（カンマ区切りで複数）、3.1.0..5.1.0 のような位置の範囲。section と custom は --section・--from・--to と組み合わせる古い書き方）",
        )
        .default(json!("song")),
        ArgDef::text(
            "section",
            "range が section のときのセクション名または id（カンマ区切りで複数指定すると、最初のセクションの開始から最後のセクションの終了まで）",
        ),
        ArgDef::pos("from", "range が custom のときの開始位置"),
        ArgDef::pos("to", "range が custom のときの終了位置"),
        ArgDef::int("sample_rate", "サンプルレート（省略時は曲の設定）").range(8000.0, 384000.0),
        ArgDef::int("bit_depth", "ビット深度 16・24・32（32 は浮動小数点。省略時は曲の設定。MP3 では使いません）")
            .range(16.0, 32.0),
        ArgDef::int("bitrate", "MP3 のビットレート（kbps、固定）128・192・256・320（省略時は 320）")
            .range(128.0, 320.0),
        ArgDef::float("tail", "範囲の後ろに追加する秒数（エフェクトの余韻用。既定は 0）").range(0.0, 600.0),
        ArgDef::choice(
            "format",
            &["wav", "flac", "mp3"],
            "ファイル形式（省略時は --out の拡張子、無ければ wav。FLAC は 16・24 bit のみ。MP3 は 48 kHz までで、--sample_rate を省略すると曲が 48 kHz を超えるときは 48 kHz（44.1 kHz の倍数なら 44.1 kHz）。MP3 は曲名・トラック名をタグに入れます）",
        ),
        ArgDef::text(
            "name",
            "ファイル名のテンプレート。{曲} は曲名、{トラック} はトラック・バスの名前（省略時は前回のテンプレート。初回はミックスダウンが {曲}、ステムが {曲}_{トラック}。ステムのテンプレートには {トラック} が必要です）",
        ),
        ArgDef::flag(
            "overwrite",
            "同じ名前のファイルがあれば、その場で上書きする（前のファイルをごみ箱に移動しない）",
        ),
        ArgDef::flag(
            "numbered",
            "同じ名前のファイルがあれば、番号を付けた名前で書き出す（省略時は前のファイルをごみ箱に移動し、同じ名前で書き出して一覧の行を置き換える）",
        ),
        ArgDef::flag(
            "background",
            "バックグラウンドで書き出し、すぐにジョブの id を返す（進行状況は export.status）",
        ),
    ]
}

fn extras() -> Vec<CommandDef> {
    let mut mix = vec![ArgDef::text(
        "out",
        "書き出し先のファイル（省略時は曲のフォルダーの Mixdown/ に --name のテンプレートで書き出し）",
    )];
    mix.extend(range_args());
    let mut selection = mix.clone();
    selection.push(ArgDef::ids(IdKind::Clip, "書き出す範囲を決めるクリップ").req());
    let mut stems = vec![
        ArgDef::text(
            "dir",
            "書き出し先のフォルダー（省略時は曲のフォルダーの Stems/）",
        ),
        ArgDef::choice(
            "what",
            &["tracks", "buses", "all", "by_bus", "selected"],
            "書き出す単位（トラックごと・バスごと（バスと、出力先がマスターのトラック）・両方・by_bus・selected（--ids で指定したもの））。tracks = 全トラック、buses = 全バス",
        )
        .default(json!("all")),
        ArgDef::ids(
            IdKind::Track,
            "書き出すトラックとバスの id（指定すると what は selected）",
        ),
        ArgDef::id_list(
            "folders",
            IdKind::Folder,
            "フォルダーの id。フォルダーにつないだバスをフォルダー名の 1 本のステムにする（バスにつないでいないフォルダーは書き出せません。指定すると what は selected）",
        ),
    ];
    stems.extend(range_args());
    vec![
        def(
            "plugin.preset.save",
            "プラグインのプリセットをファイルに保存します",
            vec![
                ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
                ArgDef::text("file", "ファイル").req(),
            ],
            Risk::Guarded,
        ),
        def(
            "plugin.preset.open",
            "プリセットをファイルから読み込みます",
            vec![
                ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
                ArgDef::text("file", "ファイル").req(),
            ],
            Risk::Write,
        ),
        def(
            "export.selection",
            "選んだクリップの先頭から末尾までを書き出します",
            selection,
            Risk::Guarded,
        ),
        def(
            "export.mixdown",
            "曲全体を 1 つの WAV・FLAC・MP3 ファイルにミックスダウンします（画面を使わず、再生と同じエンジンでレンダリングし、完了後にラウドネスとトゥルーピークを表示します）",
            mix,
            Risk::Guarded,
        ),
        def(
            "export.stems",
            "トラックとバスを 1 つずつ WAV・FLAC・MP3 ファイルにステムとして書き出します（ミュート中のものはスキップして通知します）",
            stems,
            Risk::Guarded,
        ),
        def(
            "export.status",
            "書き出しとプラグインのスキャンの進行状況と結果を表示します（--job を省略するとすべて）。保存済みの曲の書き出しは曲のフォルダーの exports/ に記録され、別のプロセスからも参照でき、異常終了したものは interrupted（中断）になります",
            vec![ArgDef::text(
                "job",
                "ジョブの id（export.mixdown・export.stems・plugin.rescan が返すもの）",
            )],
            Risk::Read,
        ),
        def(
            "export.cancel",
            "書き出しまたはプラグインのスキャンをキャンセルします（書き出しはファイルを書き出しません。別のプロセスの書き出しもキャンセルできます。スキャンは読み込み済みの分を一覧に残します）",
            vec![ArgDef::text("job", "ジョブの id").req()],
            Risk::Write,
        ),
        def(
            "export.update",
            "書き出したファイルを、現在の曲から同じ設定で書き出し直します（ステムは変わったファイルだけを書き出し直し、ほかのファイルはそのまま残します。書き直すファイルの前の版はごみ箱に移動します）",
            vec![
                ArgDef::text("job", "書き出し直すジョブの id").req(),
                ArgDef::text(
                    "file",
                    "書き出し直すファイル（名前かパス。省略時は変わったファイル。テンポ・拍子・曲の設定・範囲が変わったときはすべて）",
                ),
                ArgDef::flag(
                    "background",
                    "バックグラウンドで書き出し、すぐにジョブの id を返す（進行状況は export.status）",
                ),
            ],
            Risk::Guarded,
        ),
        def(
            "export.open",
            "書き出しの設定を返します（--job を指定するとそのジョブと同じ設定、省略時は既定の設定と前回のファイル名のテンプレート）",
            vec![ArgDef::text("job", "設定をコピーする書き出しジョブの id")],
            Risk::Read,
        ),
        def(
            "export.preview",
            "書き出したファイルを試聴します",
            vec![
                ArgDef::text("job", "書き出しジョブの id").req(),
                ArgDef::text("file", "聴くファイル（名前かパス。省略時は書き出したすべてのファイル）"),
                ArgDef::text("output", "出力（device・silent。省略時は実行中の出力設定）"),
                ArgDef::flag("stop", "試聴を停止"),
                ArgDef::flag("status", "試聴の状態を取得"),
            ],
            Risk::Read,
        ),
        def(
            "export.import",
            "書き出したファイルを新しいトラックへ取り込みます",
            vec![
                ArgDef::text("job", "書き出しジョブの id").req(),
                ArgDef::text(
                    "file",
                    "取り込むファイル（名前かパス。省略時は書き出したすべてのファイル）",
                ),
                ArgDef::pos("at", "配置する位置（省略時は書き出した範囲の頭）"),
            ],
            Risk::Write,
        ),
        def(
            "export.reveal_os",
            "書き出したファイルをエクスプローラー（Finder）で表示します",
            vec![
                ArgDef::text("job", "書き出しジョブの id").req(),
                ArgDef::text("file", "表示するファイル（名前かパス。省略時は最初のファイル）"),
            ],
            Risk::Read,
        ),
        def(
            "export.delete",
            "書き出したファイルをごみ箱に移動し、一覧から削除します",
            vec![ArgDef::text("job", "書き出しジョブの id").req()],
            Risk::Guarded,
        ),
        def(
            "transport.play",
            "再生します（--at で開始位置を指定、--loop でループ範囲を繰り返し再生）",
            vec![
                ArgDef::pos("at", "再生の開始位置（省略時は現在の位置）").alias(&["from"]),
                ArgDef::flag("loop", "ループ範囲を繰り返し再生する"),
            ],
            Risk::Read,
        ),
        def("transport.stop", "再生を停止します", vec![], Risk::Read),
        def(
            "transport.seek",
            "再生位置を移動します",
            vec![ArgDef::pos("at", "移動先").alias(&["to"]).req()],
            Risk::Read,
        ),
        def(
            "transport.goto",
            "再生位置を、曲の頭・終わり・ループの端・マーカー・拍や小節の線・クリップの端へ移動します",
            vec![ArgDef::text(
                "to",
                "移動先: start・end・loop-start・loop-end・next-marker・prev-marker・next-beat・prev-beat・next-bar・prev-bar・next-event・prev-event、または marker-1 のようなマーカーの番号",
            )
            .req()],
            Risk::Read,
        ),
        def(
            "transport.loop",
            "ループ再生を切り替えます（範囲は loop.set で設定）",
            vec![
                ArgDef::flag("on", "オン（省略時は現在の状態を切り替え）"),
                ArgDef::flag("off", "オフ"),
            ],
            Risk::Read,
        ),
        def(
            "transport.status",
            "再生状態・現在の位置・ループを表示します",
            vec![],
            Risk::Read,
        ),
        def(
            "plugin.rescan",
            "インストール済みのプラグインを再スキャンします（新規と変更されたもののみ。--full ですべて）",
            vec![
                ArgDef::flag(
                    "full",
                    "キャッシュを使わず、すべてのプラグインをスキャンし直す",
                ),
                ArgDef::flag(
                    "background",
                    "バックグラウンドでスキャンし、すぐにジョブの id を返す（進行状況は export.status）",
                ),
                ArgDef::text(
                    "path",
                    "このプラグインのファイルは、前回の結果に関わらず開き直す（使えなかったものを試し直すとき）",
                ),
            ],
            Risk::Write,
        ),
        def(
            "plugin.scan.status",
            "プラグインのスキャンの進行状況と、見つかったプラグイン・読み込めないプラグインの数を表示します",
            vec![],
            Risk::Read,
        ),
        def(
            "browser.plugin.list",
            "インストール済みのプラグインを一覧表示します（前回のスキャン結果。plugin.add に渡す値も表示）",
            vec![
                ArgDef::text("query", "名前・ベンダー・カテゴリーに含まれる文字列"),
                ArgDef::choice(
                    "kind",
                    &["instrument", "effect"],
                    "インストゥルメントかエフェクトか",
                ),
                ArgDef::flag("failed", "読み込めなかったバンドルも含める"),
                ArgDef::int("limit", "最大件数（既定は 100）").range(0.0, 100000.0),
            ],
            Risk::Read,
        ),
        def(
            "plugin.status",
            "別のプロセスで実行中のプラグインの状態を表示します（クラッシュの有無・遅延・バイパスしたブロック数）",
            vec![],
            Risk::Read,
        ),
        def(
            "plugin.restart",
            "クラッシュしたプラグインのプロセスを再起動します（クラッシュ前の状態に復元します）",
            vec![ArgDef::id(
                "id",
                IdKind::Plugin,
                "再起動するプラグイン（省略時はクラッシュしたプロセスすべて）",
            )],
            Risk::Write,
        ),
        def(
            "plugin.reload",
            "プラグインを再読み込みします",
            vec![],
            Risk::Write,
        ),
        def(
            "plugin.stats",
            "プラグインごとの処理時間を表示します（直近 1 秒ほどの平均と最大、再生の時間に対する割合。重いものから順。別プロセスのプラグインは、そのプロセスで測った時間）",
            vec![ArgDef::id(
                "track",
                IdKind::Track,
                "このトラックのプラグインだけ表示する",
            )],
            Risk::Read,
        ),
        def(
            "engine.stats",
            "オーディオエンジンの全体の処理の負荷と、間に合わなかった回数を表示します",
            vec![],
            Risk::Read,
        ),
        def(
            "audio.test-tone",
            "テスト音を再生します（左、右の順に約 −18 dBFS のサイン波）",
            vec![ArgDef::flag("stop", "再生中のテスト音を止める")],
            Risk::Read,
        ),
        def(
            "plugin.collect",
            "プラグインの画面で変更した値を曲に反映します（1 回で取り消せます）",
            vec![],
            Risk::Write,
        ),
        def(
            "export.touch",
            "トラックを変更したものとして、そのトラックを含む書き出したファイルを古いものとして扱います（プラグインの画面での変更など）",
            vec![
                ArgDef::id_list("tracks", IdKind::Track, "変更したトラック").req(),
            ],
            Risk::Write,
        ),
        def(
            "export.watch",
            "書き出したファイルが古くなったとき・最新に戻ったときに export.stale のイベントで知らせます",
            vec![ArgDef::flag("stop", "知らせるのをやめる")],
            Risk::Read,
        ),
        def(
            "export.midi",
            "インストゥルメントトラックのノートを MIDI ファイル（.mid、フォーマット 1）に書き出します。曲名・テンポ・拍子とトラック名を含みます",
            vec![
                ArgDef::id_list(
                    "tracks",
                    IdKind::Track,
                    "書き出すインストゥルメントトラック（省略時は --clips のトラック、それも無ければすべてのインストゥルメントトラック）",
                ),
                ArgDef::id_list(
                    "clips",
                    IdKind::Clip,
                    "このクリップのトラックと範囲を書き出す",
                ),
                ArgDef::text(
                    "range",
                    "書き出す範囲（song・loop・section:名前または id・3.1.0..5.1.0 のような位置の範囲。省略時は曲全体、--clips を指定したときはそのクリップの範囲）",
                ),
                ArgDef::text("section", "range が section のときのセクション名または id"),
                ArgDef::pos("from", "range が custom のときの開始位置"),
                ArgDef::pos("to", "range が custom のときの終了位置"),
                ArgDef::flag(
                    "each",
                    "トラックごとに別のファイルにする（省略時は 1 つのファイルにまとめる）",
                ),
                ArgDef::text(
                    "out",
                    "書き出すファイル（--each のときはフォルダー。省略時は曲のフォルダーの MIDI）",
                ),
                ArgDef::text(
                    "name",
                    "ファイル名のテンプレート。{曲} は曲名、{トラック} はトラックの名前（省略時は前回のテンプレート。初回は {曲}、--each のときは {曲}_{トラック}）",
                ),
                ArgDef::flag(
                    "overwrite",
                    "同じ名前のファイルがあれば、その場で上書きする（前のファイルをごみ箱に移動しない）",
                ),
                ArgDef::flag(
                    "numbered",
                    "同じ名前のファイルがあれば、番号を付けた名前で書き出す（省略時は前のファイルをごみ箱に移動し、同じ名前で書き出して一覧の行を置き換える）",
                ),
            ],
            Risk::Guarded,
        ),
        def(
            "export.dawproject",
            "曲を DAWproject（.dawproject）に書き出します。トラック・クリップ・センド・オートメーション・テンポ・拍子と、クリップの音のファイルを含みます。同じ名前のファイルはごみ箱に移動してから書き出し、含めなかったものを一覧にして返します",
            vec![
                ArgDef::text(
                    "out",
                    "書き出すファイル（省略時は曲のフォルダーの「曲名.dawproject」）",
                ),
                ArgDef::flag(
                    "external",
                    "音のファイルを中に入れず、今の場所を指す（ファイルは軽くなるが、渡すときに音のファイルも要る）",
                ),
            ],
            Risk::Guarded,
        ),
        def(
            "mix.listen",
            "指定した範囲をオフラインでレンダリングし、ピーク・RMS・ラウドネス（LUFS・トゥルーピーク・LRA）・ステレオ幅・小節ごとの値・スペクトルを JSON で返します（ファイルは書き出しません。同じ曲と範囲なら同じ値になります）",
            vec![
                ArgDef::text(
                    "range",
                    "解析する範囲（song = 曲の先頭から最後のクリップの終わりまで、loop = ループ範囲、section:名前または id（カンマ区切りで複数）、3.1.0..5.1.0 のような位置の範囲）",
                )
                .default(json!("song")),
                ArgDef::id(
                    "track",
                    IdKind::Track,
                    "解析するトラックまたはバス（省略時はマスター）",
                ),
                ArgDef::choice(
                    "bands",
                    &["octave", "third"],
                    "スペクトルの帯域（octave = オクターブ 10 帯域、third = 1/3 オクターブ 30 帯域）",
                )
                .default(json!("octave")),
            ],
            Risk::Read,
        ),
    ]
}

/// Core's commands and these.
pub fn registry() -> Registry {
    let mut defs = Registry::standard().all().to_vec();
    defs.extend(extras());
    defs.extend(crate::audio_settings::definitions());
    defs.extend(crate::app::definitions());
    Registry::from_defs(defs).with_aliases(xirius_core::commands::ALIASES)
}

pub fn shared_registry() -> Arc<Registry> {
    Arc::new(registry())
}

/// Adds the extension that answers these commands to `session`.
pub fn attach(session: &mut Session, config: ExtConfig) {
    session.add_extension(Box::new(SessionExt::new(config)));
}

/// Like [`attach`], with what the extension is given besides the config (a MIDI input).
pub fn attach_with(session: &mut Session, config: ExtConfig, extras: ExtExtras) {
    session.add_extension(Box::new(SessionExt::with(config, extras)));
}

/// Runs a command. With a `host` (`xirius play` keeps one with the device), the transport and
/// recording commands drive it (`record.stop` places the takes as one step); everything else goes to the session, and then the host is brought up to
/// date with the song, whether the command was an edit, an undo or a redo.
pub fn run(
    session: &mut Session,
    host: Option<&mut SessionHost>,
    name: &str,
    raw: &Map<String, Value>,
    opts: &RunOptions,
) -> CmdResult<RunResult> {
    if let Some(host) = host {
        if (name.starts_with("transport.") && NAMES.contains(&name))
            || ON_HOST.contains(&name)
            || name == "record.start"
            || name == "record.status"
        {
            return run_on_host(session, host, name, raw, opts);
        }
        if name == "record.stop" {
            return record_stop(session, host, raw, opts);
        }
        if name == "plugin.collect" {
            return collect(session, host, raw, opts);
        }
        if name == CLEANUP && host.is_recording() {
            return Err(CmdError::conflict(
                "録音中は片付けできません（record.stop で止めてから実行してください）",
            ));
        }
        if name == "plugin.preset.save" {
            session.check_permission(name, &opts.author)?;
            let registry = session.registry().clone();
            let def = registry.get(name).expect("preset definition");
            let mut args = raw.clone();
            let dry = opts.dry_run
                || args
                    .remove("dry_run")
                    .is_some_and(|v| v == Value::Bool(true) || v == "true");
            registry.normalize(def, &args)?;
            if !dry {
                collect_plugin_states(session, host, opts)?;
            }
        }
        let result = session.run(name, raw, opts)?;
        if !result.dry_run {
            host.sync(session.project());
        }
        return Ok(result);
    }
    session.run(name, raw, opts)
}

fn run_on_host(
    session: &mut Session,
    host: &mut SessionHost,
    name: &str,
    raw: &Map<String, Value>,
    opts: &RunOptions,
) -> CmdResult<RunResult> {
    session.check_permission(name, &opts.author)?;
    let registry = session.registry().clone();
    let def = registry.get(name).expect("checked above");
    let mut raw = raw.clone();
    let dry = opts.dry_run
        || raw
            .remove("dry_run")
            .is_some_and(|v| v == Value::Bool(true) || v == "true");
    let args = registry.normalize(def, &raw)?;
    let (message, data) = if dry {
        (
            format!("{name} をドライランで実行しました（変更はありません）"),
            Value::Null,
        )
    } else {
        // The engine follows the song as it is now, so a command typed right after an edit
        // sees it.
        host.sync(session.project());
        if matches!(name, "plugin.stats" | "engine.stats") {
            stats_on(Some(host), session.project(), name, &args)?
        } else if name == "audio.test-tone" {
            tone_on(host, &args)?
        } else if ON_HOST.contains(&name) {
            plugins_on(host, name, &args)?
        } else if RECORD.contains(&name) {
            let p = session.project();
            crate::record::record_on(host, p, session.dir(), name, &args)?
        } else {
            transport_on(host, session.project(), name, &args)?
        }
    };
    Ok(RunResult {
        command: name.to_string(),
        message,
        data,
        changes: ChangeSet::default(),
        dry_run: dry,
        forced_dry_run: false,
        step: None,
        decision: None,
        events: Vec::new(),
    })
}

/// `record.stop` with a host: the takes become one step of the song's history.
fn record_stop(
    session: &mut Session,
    host: &mut SessionHost,
    raw: &Map<String, Value>,
    opts: &RunOptions,
) -> CmdResult<RunResult> {
    session.check_permission("record.stop", &opts.author)?;
    let dry = opts.dry_run || raw.get("dry_run").is_some_and(|v| v == true || v == "true");
    if !dry {
        return crate::record::finish_recording(session, host, opts, crate::record::STOP_WAIT);
    }
    Ok(RunResult {
        command: "record.stop".into(),
        message: "record.stop をドライランで実行しました（変更はありません）".into(),
        data: Value::Null,
        changes: ChangeSet::default(),
        dry_run: true,
        forced_dry_run: false,
        step: None,
        decision: None,
        events: Vec::new(),
    })
}

/// `plugin.collect` with a host: the edits become one step of the song's history.
fn collect(
    session: &mut Session,
    host: &mut SessionHost,
    raw: &Map<String, Value>,
    opts: &RunOptions,
) -> CmdResult<RunResult> {
    session.check_permission("plugin.collect", &opts.author)?;
    let dry = opts.dry_run || raw.get("dry_run").is_some_and(|v| v == true || v == "true");
    let done = if dry {
        None
    } else {
        Some(collect_plugin_states(session, host, opts)?)
    };
    let moves = done.as_ref().map_or(0, |c| c.params.len());
    let data = json!({ "param_moves": moves });
    Ok(match done.and_then(|c| c.result) {
        Some(mut r) => {
            r.command = "plugin.collect".into();
            r.message = format!("プラグインの編集を曲に反映しました（{}）", r.message);
            r.data = data;
            r
        }
        None => RunResult {
            command: "plugin.collect".into(),
            message: if dry {
                "plugin.collect をドライランで実行しました（変更はありません）".into()
            } else {
                "反映する編集はありません".into()
            },
            data,
            changes: ChangeSet::default(),
            dry_run: dry,
            forced_dry_run: false,
            step: None,
            decision: None,
            events: Vec::new(),
        },
    })
}

fn io_err(e: export::ExportError) -> CmdError {
    CmdError::conflict(e.0)
}

pub(crate) fn options(p: &Project, dir: Option<&Path>, args: &Args) -> CmdResult<Options> {
    let tick = |name: &str| -> CmdResult<i64> {
        match args.0.get(name) {
            Some(Value::Number(n)) => n.as_i64().ok_or_else(|| {
                CmdError::invalid(format!(
                    "--{name} には tick 数（整数）か小節.拍.tick を指定してください"
                ))
            }),
            Some(Value::String(s)) => p.time_map().parse(s),
            _ => Err(CmdError::invalid(format!("--{name} が必要です"))),
        }
    };
    let sections = |text: &str| {
        let names: Vec<String> = text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        match names.len() {
            1 => RangeSpec::Section(names[0].clone()),
            _ => RangeSpec::Sections(names),
        }
    };
    let range = match args.opt_str("range").unwrap_or("song") {
        "song" => RangeSpec::Song,
        "loop" => RangeSpec::Loop,
        "section" => sections(
            args.opt_str("section")
                .ok_or_else(|| CmdError::invalid("--range section には --section が必要です"))?,
        ),
        "custom" => RangeSpec::Ticks(tick("from")?, tick("to")?),
        text if text.starts_with("section:") => sections(&text["section:".len()..]),
        text => match text.split_once("..") {
            Some((a, b)) => {
                RangeSpec::Ticks(p.time_map().parse(a.trim())?, p.time_map().parse(b.trim())?)
            }
            None => {
                return Err(CmdError::invalid(format!(
                    "--range に `{text}` は使えません（song・loop・section:名前・開始..終了のいずれか）"
                )));
            }
        },
    };
    let bit_depth = args.opt_i64("bit_depth").map(|b| b as u16);
    if let Some(b) = bit_depth
        && !matches!(b, 16 | 24 | 32)
    {
        return Err(CmdError::range(format!(
            "--bit_depth には 16・24・32 のいずれかを指定してください（{b} は使用できません）"
        )));
    }
    Ok(Options {
        range,
        sample_rate: args.opt_i64("sample_rate").map(|r| r as u32),
        bit_depth,
        bitrate: args.opt_i64("bitrate").map(|b| b as u16),
        stem_names: Vec::new(),
        tail_secs: args.opt_f64("tail").unwrap_or(0.0),
        format: match args.opt_str("format") {
            Some(f) => Format::parse(f).ok_or_else(|| {
                CmdError::invalid(format!(
                    "--format には wav・flac・mp3 のいずれかを指定してください（{f} は使用できません）"
                ))
            })?,
            None => Format::Wav,
        },
        pattern: args.opt_str("name").map(str::to_string),
        overwrite: !args.flag("numbered"),
        dir: dir.map(Path::to_path_buf),
        ..Options::default()
    })
}

/// A folder under the song's folder, or under the working folder for a song not saved yet.
fn default_folder(dir: Option<&Path>, folder: &str) -> PathBuf {
    dir.map_or_else(|| PathBuf::from(folder), |d| d.join(folder))
}

/// The file name patterns used when `--name` is left out and none is remembered (EXP-015).
pub(crate) const MIX_NAME: &str = "{曲}";
pub(crate) const STEMS_NAME: &str = "{曲}_{トラック}";

/// Where the remembered pattern of each kind is kept.
pub(crate) fn name_key(stems: bool) -> &'static str {
    if stems { "stems_name" } else { "mix_name" }
}

/// Where the range of the last export of a song is kept (with the sections it named).
pub(crate) const RANGE_KEY: &str = "range";
pub(crate) const SECTIONS_KEY: &str = "sections";

/// The range the panel starts from next time: the loop, or the sections it named. A range typed
/// in is not one the panel can show again, so it comes back as the whole song.
pub(crate) fn range_to_remember(args: &Args) -> (&'static str, String) {
    match args.opt_str("range").unwrap_or("song") {
        "loop" => ("loop", String::new()),
        "section" => (
            "section",
            args.opt_str("section").unwrap_or_default().to_string(),
        ),
        text if text.starts_with("section:") => ("section", text["section:".len()..].to_string()),
        _ => ("song", String::new()),
    }
}

/// The range and the section ids `export.open` offers: the last ones exported, so long as the
/// song still has the loop or the sections.
fn remembered_range(p: &Project, remembered: &Map<String, Value>) -> (&'static str, Vec<String>) {
    let text = |k: &str| {
        remembered
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    match text(RANGE_KEY) {
        "loop" if p.loop_range.is_some() => ("loop", Vec::new()),
        "section" => {
            let ids: Vec<String> = text(SECTIONS_KEY)
                .split(',')
                .map(str::trim)
                .filter_map(|n| {
                    p.sections
                        .iter()
                        .find(|s| s.id.to_string() == n || s.name == n)
                        .map(|s| s.id.to_string())
                })
                .collect();
            if ids.is_empty() {
                ("song", ids)
            } else {
                ("section", ids)
            }
        }
        _ => ("song", Vec::new()),
    }
}

/// What the mix stands for in a pattern that names a track.
const MIX_TRACK: &str = "Mix";

fn pattern_of(remembered: &Map<String, Value>, stems: bool) -> String {
    remembered
        .get(name_key(stems))
        .and_then(Value::as_str)
        .unwrap_or(if stems { STEMS_NAME } else { MIX_NAME })
        .to_string()
}

/// An export worked out from its arguments, before anything is rendered.
pub(crate) struct Plan {
    pub stems: bool,
    /// The file of a mix, the folder of stems.
    pub target: PathBuf,
    pub strips: Vec<TrackId>,
    pub options: Options,
    /// The arguments that make the same export again (`export.update`, `export.open`).
    pub stored: Map<String, Value>,
}

/// Works out an export. `remembered` holds the patterns used last; `busy` the files other
/// exports of this process are writing.
pub(crate) fn plan(
    p: &Project,
    dir: Option<&Path>,
    stems: bool,
    args: &Args,
    remembered: &Map<String, Value>,
    busy: &[PathBuf],
) -> CmdResult<Plan> {
    let mut o = options(p, dir, args)?;
    o.busy = busy.to_vec();
    let out = if stems { None } else { args.opt_str("out") };
    // The format: --format, else the extension of --out, else WAV.
    let from_ext = match out.map(Path::new).and_then(Path::extension) {
        None => None,
        Some(e) => Some(Format::parse(&e.to_string_lossy()).ok_or_else(|| {
            CmdError::invalid(format!(
                ".{} のファイルには書き出せません（.wav・.flac・.mp3 のいずれかを指定してください）",
                e.to_string_lossy()
            ))
        })?),
    };
    o.format = match (args.opt_str("format"), from_ext) {
        (Some(_), Some(e)) if e != o.format => {
            return Err(CmdError::invalid(format!(
                "--format {} と --out の拡張子 .{} が一致しません",
                o.format.name(),
                e.ext()
            )));
        }
        (None, Some(e)) => e,
        _ => o.format,
    };
    let pattern = o
        .pattern
        .clone()
        .unwrap_or_else(|| pattern_of(remembered, stems));
    if pattern.trim().is_empty() {
        return Err(CmdError::invalid("--name を入力してください"));
    }
    o.pattern = Some(pattern.clone());
    let (target, strips) = if stems {
        let ids = args.ids::<TrackId>("ids")?;
        let folders = args.ids::<FolderId>("folders")?;
        let folder_buses = folder_stems(p, &folders)?;
        let strips = match args.opt_str("what").unwrap_or("all") {
            _ if !ids.is_empty() || !folders.is_empty() => {
                let mut strips = ids;
                for (bus, name) in folder_buses {
                    if !strips.contains(&bus) {
                        strips.push(bus);
                    }
                    o.stem_names.push((bus, name));
                }
                strips
            }
            "selected" => {
                return Err(CmdError::invalid(
                    "--what selected には、書き出すトラックとバスの --ids か、フォルダーの --folders が必要です",
                ));
            }
            "tracks" => StemSet::Tracks.strips(p),
            "buses" => StemSet::Buses.strips(p),
            "by_bus" => StemSet::ByBus.strips(p),
            _ => StemSet::All.strips(p),
        };
        (stems_dir(dir, args), strips)
    } else {
        let target = match out {
            Some(s) if Path::new(s).extension().is_some() => PathBuf::from(s),
            Some(s) => PathBuf::from(format!("{s}.{}", o.format.ext())),
            None => default_folder(dir, "Mixdown").join(format!(
                "{}.{}",
                export::name_from(&pattern, &p.name, MIX_TRACK),
                o.format.ext()
            )),
        };
        let target = crate::export_paths::absolute(&target)?;
        // A file another export is writing keeps its name and this one takes a number; so
        // does a file that is there, when numbers are asked for (OPEN-10).
        let target = export::numbered(&target, busy, !o.overwrite);
        (target, Vec::new())
    };
    let target = crate::export_paths::absolute(&target)?;
    let mut stored = args.0.clone();
    stored.remove("background");
    stored.remove("dry_run");
    stored.insert("format".into(), json!(o.format.name()));
    stored.insert("name".into(), json!(pattern));
    if let RangeSpec::Ticks(start, end) = &o.range {
        stored.insert("from".into(), json!(start));
        stored.insert("to".into(), json!(end));
    }
    if stems {
        stored.insert("dir".into(), json!(target.display().to_string()));
        // A folder's bus is kept as the folder, so a later export names it the same way.
        let ids: Vec<String> = strips
            .iter()
            .filter(|s| !o.stem_names.iter().any(|(b, _)| b == *s))
            .map(ToString::to_string)
            .collect();
        stored.insert("ids".into(), json!(ids));
    } else {
        stored.insert("out".into(), json!(target.display().to_string()));
    }
    Ok(Plan {
        stems,
        target,
        strips,
        options: o,
        stored,
    })
}

/// The bus of each folder, with the folder's name (CTX-018): a folder becomes one stem only by
/// being connected to a bus, where what plays into it is summed.
fn folder_stems(p: &Project, folders: &[FolderId]) -> CmdResult<Vec<(TrackId, String)>> {
    let mut out = Vec::new();
    for id in folders {
        let f = p
            .folders
            .get(id)
            .ok_or_else(|| CmdError::not_found(format!("{id} というフォルダーが見つかりません")))?;
        let bus = f
            .bus
            .filter(|b| p.tracks.get(b).is_some_and(|t| t.kind == TrackKind::Bus));
        let Some(bus) = bus else {
            return Err(CmdError::invalid(format!(
                "フォルダー「{}」にはバスがつながっていないため、1 本のステムにできません（フォルダーのバスを決めるか、中のトラックごとに書き出してください）",
                f.name
            )));
        };
        out.push((bus, f.name.clone()));
    }
    Ok(out)
}

pub(crate) fn stems_dir(dir: Option<&Path>, args: &Args) -> PathBuf {
    match args.opt_str("dir") {
        Some(s) => PathBuf::from(s),
        None => default_folder(dir, "Stems"),
    }
}

/// Whether the export of `plan` would be refused, found before anything runs. Returns the
/// range in ticks.
pub(crate) fn check(p: &Project, plan: &Plan) -> CmdResult<(i64, i64)> {
    let strips = plan.stems.then_some(plan.strips.as_slice());
    export::check(p, &plan.options, strips).map_err(io_err)
}

/// What a dry run of `plan` says.
pub(crate) fn preview(p: &Project, plan: &Plan) -> CmdResult<(String, Value)> {
    let (a, b) = check(p, plan)?;
    let key = if plan.stems { "dir" } else { "path" };
    Ok((
        format!(
            "{} に {} で書き出します（{a}〜{b} tick）",
            plan.target.display(),
            plan.options.format.name().to_uppercase()
        ),
        json!({
            key: plan.target.display().to_string(),
            "from": a,
            "to": b,
            "format": plan.options.format.name(),
            "name": plan.options.pattern,
        }),
    ))
}

/// The files an export writes: the mix, or each stem not muted.
pub(crate) fn files_of(p: &Project, plan: &Plan) -> CmdResult<Vec<PathBuf>> {
    if !plan.stems {
        return Ok(vec![plan.target.clone()]);
    }
    let paths = export::stem_paths(p, &plan.target, &plan.strips, &plan.options).map_err(io_err)?;
    Ok(paths.into_iter().flatten().collect())
}

/// Renders a mix to the file `target`, or stems of `strips` into the folder `target`.
pub(crate) fn render(
    p: &Project,
    stems: bool,
    target: &Path,
    strips: &[TrackId],
    o: &Options,
) -> CmdResult<(String, Value)> {
    if !stems {
        let r = export::mixdown(p, target, o).map_err(io_err)?;
        return Ok((r.summary(), r.to_json()));
    }
    let r = export::stems_of(p, target, strips, o).map_err(io_err)?;
    let mut lines: Vec<String> = r.files.iter().map(export::FileReport::summary).collect();
    for s in &r.skipped {
        lines.push(format!("スキップ: {}（{}）", s.name, s.why));
    }
    Ok((lines.join("\n"), r.to_json()))
}

/// The settings `export.open` starts from when no export is named: the song's own, the
/// patterns used last and where the files go.
pub(crate) fn default_settings(
    p: &Project,
    dir: Option<&Path>,
    remembered: &Map<String, Value>,
) -> Value {
    let (mix, stem) = (pattern_of(remembered, false), pattern_of(remembered, true));
    let (range, picked) = remembered_range(p, remembered);
    let sections: Vec<Value> = p
        .sections
        .iter()
        .map(|s| json!({ "id": s.id.to_string(), "name": s.name }))
        .collect();
    json!({
        "format": "wav",
        "sample_rate": p.settings.sample_rate,
        "bit_depth": p.settings.bit_depth,
        "bitrate": crate::mp3::DEFAULT_BITRATE,
        "mp3": mp3_status(),
        "range": range,
        "tail": 0.0,
        "overwrite": false,
        "sections": sections,
        "picked_sections": picked,
        "has_loop": p.loop_range.is_some(),
        "mixdown": {
            "name": mix,
            "dir": default_folder(dir, "Mixdown").display().to_string(),
            "file": format!("{}.wav", export::name_from(&mix, &p.name, MIX_TRACK)),
        },
        "stems": {
            "name": stem,
            "dir": default_folder(dir, "Stems").display().to_string(),
            "what": "all",
        },
        "midi": {
            "name": crate::midi_export::pattern_of(remembered, false),
            "each_name": crate::midi_export::pattern_of(remembered, true),
            "dir": default_folder(dir, crate::midi_export::FOLDER).display().to_string(),
            "tracks": crate::midi_export::instrument_tracks(p)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        },
    })
}

/// Whether MP3 can be written (EXP-005): the encoder's file, and why it cannot be loaded.
pub(crate) fn mp3_status() -> Value {
    let path = crate::mp3::locate();
    match crate::mp3::load(Some(&path)) {
        Ok(l) => json!({
            "available": true,
            "path": path.display().to_string(),
            "version": l.version,
        }),
        Err(why) => json!({
            "available": false,
            "path": path.display().to_string(),
            "error": crate::mp3::CANNOT_LOAD,
            "why": why,
        }),
    }
}

/// Where the host is and whether it plays.
pub(crate) fn transport_status(host: &mut SessionHost, p: &Project) -> (String, Value) {
    let st = host.poll();
    let rate = f64::from(host.rate());
    let tick = host.tempo_map().tick_of_frame(st.frame, rate);
    let position = p.time_map().format(tick);
    (
        format!(
            "{} {position}（{} フレーム）{}",
            if st.playing { "再生中" } else { "停止中" },
            st.frame,
            if host.looping() { " ループ" } else { "" }
        ),
        json!({
            "playing": st.playing,
            "frame": st.frame,
            "tick": tick,
            "position": position,
            "seconds": st.frame as f64 / rate,
            "looping": host.looping(),
        }),
    )
}

/// `audio.test-tone` (UI-260): plays the test tone once on the device, or stops it.
pub(crate) fn tone_on(host: &mut SessionHost, args: &Args) -> CmdResult<(String, Value)> {
    let rate = f64::from(host.rate());
    let tone = (!args.flag("stop")).then(|| Box::new(xirius_engine::tone::Tone::new(rate)));
    let seconds = tone.as_ref().map_or(0.0, |t| f64::from(t.frames()) / rate);
    host.controller()
        .send(xirius_engine::Command::SetTone(tone))
        .map_err(|_| CmdError::conflict("テスト音を再生できません"))?;
    let message = if seconds > 0.0 {
        "テスト音を再生しています（左、右の順）"
    } else {
        "テスト音を止めました"
    };
    Ok((
        message.into(),
        json!({
            "playing": seconds > 0.0,
            "seconds": seconds,
            "order": ["left", "right"],
            "level_db": xirius_engine::tone::LEVEL_DB,
        }),
    ))
}

/// `plugin.stats` or `engine.stats` (ENG-008), with the host that is playing the song, or
/// without one: then nothing is measured and the plugins are listed with how they are set.
pub(crate) fn stats_on(
    host: Option<&mut SessionHost>,
    p: &Project,
    name: &str,
    args: &Args,
) -> CmdResult<(String, Value)> {
    if name == "engine.stats" {
        let Some(host) = host else {
            return Ok((
                "再生していないため、エンジンの負荷は計れていません".into(),
                json!({ "playing": false, "load": 0.0, "xruns": 0, "callbacks": 0 }),
            ));
        };
        let cpu = host.cpu();
        let snap = host.controller().stats().snapshot();
        let us = |d: std::time::Duration| d.as_secs_f64() * 1e6;
        let data = json!({
            "playing": true,
            "load": (cpu.load * 1e4).round() / 1e4,
            "xruns": cpu.xruns,
            "callbacks": snap.callbacks,
            "deadline_misses": snap.deadline_misses,
            "device_xruns": snap.device_xruns,
            "starved": snap.starved,
            "busy_mean_us": us(snap.busy_mean).round(),
            "busy_max_us": us(snap.busy_max).round(),
            "busy_p99_us": us(snap.busy_p99).round(),
            "tone_left_ms": (host.controller().stats().tone_left() as f64 * 1000.0
                / f64::from(host.rate()))
            .round(),
            "output_peak": host.controller().stats().take_output_peak(),
        });
        let msg = format!(
            "CPU {:.0}%、音切れ {} 回（{} 回のコールバックのうち、間に合わなかったのは {} 回）",
            cpu.load * 100.0,
            cpu.xruns,
            snap.callbacks,
            snap.deadline_misses
        );
        return Ok((msg, data));
    }
    let only: Option<TrackId> = args.opt_id("track")?;
    if let Some(t) = only
        && !p.tracks.contains_key(&t)
    {
        return Err(CmdError::not_found(format!("トラック {t} がありません")));
    }
    let (timings, crashed) = match &host {
        Some(h) => (
            h.plugin_timings(),
            h.plugin_statuses()
                .into_iter()
                .filter(|(_, _, s)| s.crashed)
                .map(|(id, _, _)| id)
                .collect(),
        ),
        None => (Default::default(), Default::default()),
    };
    let rows = crate::perf::loads(p, &timings, only);
    let data = json!({
        "playing": host.is_some(),
        "plugins": crate::perf::rows_json(&rows, &crashed),
    });
    let msg = if rows.is_empty() {
        "プラグインがありません".to_string()
    } else if host.is_none() {
        format!(
            "{} 個のプラグインがあります（再生していないため、処理時間は計れていません）",
            rows.len()
        )
    } else {
        let mut lines = vec![format!("{} 個のプラグイン（重い順）", rows.len())];
        for r in rows.iter().take(20) {
            lines.push(format!(
                "{} / {}（{}{}）: {:.1}%（平均 {:.0} µs、最大 {:.0} µs）",
                r.track_name,
                r.name,
                r.run.name(),
                if r.reading.child {
                    "、別プロセス"
                } else {
                    ""
                },
                r.reading.load * 100.0,
                r.reading.mean_us,
                r.reading.max_us
            ));
        }
        lines.join(
            "
",
        )
    };
    Ok((msg, data))
}

/// `plugin.status` or `plugin.restart` on a running host.
pub(crate) fn plugins_on(
    host: &mut SessionHost,
    name: &str,
    args: &Args,
) -> CmdResult<(String, Value)> {
    if matches!(name, "plugin.restart" | "plugin.reload") {
        match args.opt_id::<PluginId>("id")? {
            Some(id) => host.restart_plugin(id),
            None => host.restart_plugins(),
        }
        .map_err(CmdError::conflict)?;
        return Ok(("プラグインのプロセスを再起動しました".into(), Value::Null));
    }
    let list = host.plugins().reports();
    let rows: Vec<Value> = list
        .iter()
        .map(|r| {
            let s = &r.status;
            json!({
                "id": r.id.to_string(),
                "name": r.name,
                "mode": r.placement.name(),
                "requested": r.requested.name(),
                "suggest": r.suggest().map(|m| m.name()),
                "crashed": s.crashed,
                "error": s.error,
                "exit_code": s.exit_code,
                "crashes": s.crashes,
                "faults": s.faults,
                "latency": s.latency,
                "wet_blocks": s.wet_blocks,
                "dry_blocks": s.dry_blocks,
                "misses": s.misses,
                "editor_open": s.editor_open,
            })
        })
        .collect();
    let down = list.iter().filter(|r| r.status.crashed).count();
    let mut msg = if list.is_empty() {
        "別のプロセスで実行中のプラグインはありません".to_string()
    } else if down > 0 {
        format!(
            "{} 個のうち {down} 個がクラッシュし、バイパス状態で再生されています（plugin.restart で再起動できます）",
            list.len()
        )
    } else {
        format!("{} 個のプラグインが別のプロセスで実行中です", list.len())
    };
    // Only suggested: the song decides where a plugin runs (PLG-022).
    for r in &list {
        if let Some(m) = r.suggest() {
            msg.push_str(&format!(
                "
{}（{}）は共有のプロセスを {} 回クラッシュさせました。plugin.isolation.set --ids {} --mode {} で専用のプロセスに切り替えられます",
                r.name,
                r.id,
                r.status.faults,
                r.id,
                m.name()
            ));
        }
    }
    Ok((msg, json!({ "plugins": rows })))
}

/// A transport command on a host that follows `p`.
pub(crate) fn transport_on(
    host: &mut SessionHost,
    p: &Project,
    name: &str,
    args: &Args,
) -> CmdResult<(String, Value)> {
    let tick = |key: &str| -> CmdResult<Option<i64>> {
        match args.0.get(key) {
            Some(Value::Number(n)) => Ok(n.as_i64()),
            Some(Value::String(s)) => p.time_map().parse(s).map(Some),
            _ => Ok(None),
        }
    };
    let queue_full = || {
        CmdError::conflict("オーディオエンジンが処理中です。しばらく待ってからやり直してください")
    };
    let msg = match name {
        "transport.status" => return Ok(transport_status(host, p)),
        "transport.play" => {
            if let Some(on) = args.opt_flag("loop") {
                host.set_looping(p, on);
            }
            if let Some(t) = tick("at")?
                && !host.seek_tick(t)
            {
                return Err(queue_full());
            }
            if !host.play() {
                return Err(queue_full());
            }
            if host.looping() && p.loop_range.is_none() {
                "再生を開始しました（ループ範囲が無いため、最後まで再生して停止します）".to_string()
            } else {
                "再生を開始しました".to_string()
            }
        }
        "transport.stop" => {
            if !host.stop() {
                return Err(queue_full());
            }
            "停止しました".to_string()
        }
        "transport.seek" => {
            let t = tick("at")?.ok_or_else(|| CmdError::invalid("--at が必要です"))?;
            if t < 0 {
                return Err(CmdError::range("位置は 0 以上で指定してください"));
            }
            if !host.seek_tick(t) {
                return Err(queue_full());
            }
            format!("{} に移動しました", p.time_map().format(t))
        }
        "transport.goto" => {
            let st = host.poll();
            let from = host
                .tempo_map()
                .tick_of_frame(st.frame, f64::from(host.rate()));
            let to = xirius_core::nav::destination(p, from, args.str("to")?)?;
            if !host.seek_tick(to) {
                return Err(queue_full());
            }
            format!("{} に移動しました", p.time_map().format(to))
        }
        _ => {
            let now = host.looping();
            let on = match (args.opt_flag("on"), args.opt_flag("off")) {
                (Some(true), _) => true,
                (_, Some(true)) => false,
                _ => !now,
            };
            host.set_looping(p, on);
            if on && p.loop_range.is_none() {
                "ループをオンにしました（範囲が未設定です。loop.set で設定してください）"
                    .to_string()
            } else if on {
                "ループをオンにしました".to_string()
            } else {
                "ループをオフにしました".to_string()
            }
        }
    };
    Ok((msg, json!({ "looping": host.looping() })))
}
