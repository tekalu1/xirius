//! Changes of the screen that the menus make: the dock, a row shown in a column, the window of an
//! effect, a controller lane of the roll. They have a name, arguments and a label like every
//! command, so the menu row, the palette and the command line call the same thing. They never
//! enter the history; the window answers them (the table has no window of its own).
use super::*;
use crate::id::IdKind;

/// The names this file adds, so the window can tell which ones it answers.
pub const NAMES: &[&str] = &[
    "view.dock",
    "track.reveal",
    "mixer.focus",
    "select.sources",
    "select.pitch",
    "plugin.editor.open",
    "cc.lane.set",
    "cc.lane.hide",
    "auto.lane.show",
    "auto.point.value",
    "view.export",
    "plugin.preset.choose",
    "plugin.preset.file",
    "plugin.pick",
    "plugin.insert",
    "browser.use",
    "browser.fx",
    "browser.groups",
    "browser.put",
    "midimap.learn",
    "view.export.close",
    "mixer.channel.next",
    "mixer.channel.prev",
    "plugin.preset.next",
    "plugin.preset.prev",
    "view.page",
    "view.focus",
    "view.fullscreen",
    "app.quit",
    "marker.add.named",
    "range.from-clips",
    "browser.search",
    "browser.tab",
    "mixer.bank",
    "transport.panic",
    "view.pc-keyboard",
    "transport.locate",
    "view.browser.sort",
    "view.cleanup",
];

pub(super) fn defs() -> Vec<CommandDef> {
    let screen =
        |name, summary, args| session(name, summary, args, Risk::Read, false).screen_only();
    // The rows that change the song (a file placed, a plugin or preset chosen): read-only is
    // the wrong hint for them.
    let changes =
        |name, summary, args| session(name, summary, args, Risk::Write, false).screen_only();
    vec![
        screen(
            "view.dock",
            "下のドック（ピアノロール・ミキサー）を開閉します",
            vec![
                ArgDef::choice("tab", &["roll", "mixer"], "ドックのタブ").req(),
                ArgDef::choice(
                    "state",
                    &["open", "closed", "toggle"],
                    "開くか閉じるか（省略時は切り替え）",
                )
                .default(json!("toggle")),
            ],
        ),
        screen(
            "track.reveal",
            "チャンネルをトラックの列かミキサーで見せます",
            vec![
                ArgDef::id("id", IdKind::Track, "トラック").req(),
                ArgDef::choice(
                    "in",
                    &["tracks", "mixer"],
                    "見せる場所（省略時はトラックの列）",
                )
                .default(json!("tracks")),
            ],
        ),
        screen(
            "mixer.focus",
            "下のドックをミキサーにして、チャンネルを選んで光らせます",
            vec![ArgDef::id("id", IdKind::Track, "トラック").req()],
        ),
        screen(
            "select.sources",
            "バスに送っているチャンネルを選択します",
            vec![ArgDef::id("id", IdKind::Track, "バス").req()],
        ),
        screen(
            "select.pitch",
            "ピアノロールで同じ高さのノートを選択します",
            vec![
                ArgDef::int("pitch", "ノート番号（0〜127）")
                    .req()
                    .range(0.0, 127.0),
            ],
        ),
        screen(
            "plugin.editor.open",
            "エフェクトの画面を開きます",
            vec![
                ArgDef::id("track", IdKind::Track, "トラック").req(),
                ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
            ],
        ),
        screen(
            "cc.lane.set",
            "ピアノロールのレーンの種類を変えます",
            vec![
                ArgDef::int("index", "レーンの位置（0 から）").req(),
                ArgDef::text("lane", "velocity・bend・at・cc<番号>").req(),
            ],
        ),
        screen(
            "cc.lane.hide",
            "ピアノロールのレーンを隠します",
            vec![ArgDef::int("index", "レーンの位置（0 から）").req()],
        ),
        screen(
            "auto.lane.show",
            "トラックのオートメーションのレーンを見せます（無ければ足します）",
            vec![
                ArgDef::id("track", IdKind::Track, "トラック").req(),
                ArgDef::text("target", "volume・pan・mute・send:<id>・param:<id>:<名前>").req(),
            ],
        ),
        screen(
            "auto.point.value",
            "オートメーションの点の値を入力する枠を開きます",
            vec![
                ArgDef::id("track", IdKind::Track, "トラック").req(),
                ArgDef::text("target", "レーンの対象").req(),
                ArgDef::pos("at", "点の位置").req(),
            ],
        ),
        screen(
            "view.export",
            "書き出しの画面を開きます（--ids を渡すとそのステム、省略時は --range のミックスダウン）",
            vec![
                ArgDef::ids(
                    IdKind::Node,
                    "ステムにするトラック（フォルダー 1 つならその 1 本のステム）",
                ),
                ArgDef::text("range", "song・loop・a..b（位置の範囲）").default(json!("song")),
                ArgDef::text(
                    "kind",
                    "midi で MIDI ファイル（--ids はそのインストゥルメントのトラック）",
                ),
            ],
        ),
        changes(
            "plugin.preset.choose",
            "内蔵エフェクトのプリセットを読み込みます",
            vec![
                ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
                ArgDef::text("preset", "プリセットの名前").req(),
            ],
        ),
        changes(
            "plugin.preset.file",
            "プリセットのファイルを選ぶ窓を開きます（--save で書き出し、省略時は読み込み）",
            vec![
                ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
                ArgDef::flag("save", "ファイルへ書き出す"),
            ],
        ),
        screen(
            "plugin.pick",
            "チャンネルの ＋ の下に、挿すエフェクトの一覧を開きます",
            vec![ArgDef::id("id", IdKind::Track, "トラック").req()],
        ),
        changes(
            "plugin.insert",
            "最近使ったエフェクトをチャンネルの末尾に挿します",
            vec![
                ArgDef::id("id", IdKind::Track, "トラック").req(),
                ArgDef::text("item", "エフェクトの名前（内蔵は builtin:<id>）").req(),
            ],
        ),
        changes(
            "browser.use",
            "ブラウザーのインストゥルメントを、新しいトラックか選んだトラックで使います",
            vec![
                ArgDef::text("item", "インストゥルメントの名前").req(),
                ArgDef::choice(
                    "into",
                    &["new", "track"],
                    "使う場所（省略時は新しいトラック）",
                )
                .default(json!("new")),
            ],
        ),
        changes(
            "browser.fx",
            "ブラウザーのエフェクトを、選んだチャンネル・新しいバス・マスターに挿します",
            vec![
                ArgDef::text("item", "エフェクトの名前").req(),
                ArgDef::choice(
                    "into",
                    &["channel", "bus", "master"],
                    "挿す場所（省略時は選んだチャンネル）",
                )
                .default(json!("channel")),
            ],
        ),
        screen(
            "browser.groups",
            "ブラウザーの一覧のグループをすべて開く・畳みます",
            vec![ArgDef::choice("state", &["open", "closed"], "開閉").req()],
        ),
        changes(
            "browser.put",
            "ファイルを新しいトラックか選んだオーディオトラックに置きます",
            vec![
                ArgDef::text("path", "ファイルのパス").req(),
                ArgDef::choice(
                    "into",
                    &["new", "track"],
                    "置く場所（省略時は新しいトラック）",
                )
                .default(json!("new")),
            ],
        ),
        screen(
            "midimap.learn",
            "パートの値を、次に動かしたコントローラーのつまみに割り当てます（--cancel でやめる、--confirm で別のパートから譲り受ける）",
            vec![
                ArgDef::text(
                    "target",
                    "割り当てるもの（track.volume:<id>・track.pan:<id>）",
                ),
                ArgDef::flag("cancel", "待つのをやめる"),
                ArgDef::flag("confirm", "つまみを持っていたパートから譲り受ける"),
            ],
        ),
        screen("view.export.close", "書き出しの画面を閉じます", vec![]),
        screen(
            "mixer.channel.next",
            "選んだチャンネルの次（右）のチャンネルを選びます。ミキサーが開いていればそのストリップを見せます",
            vec![],
        ),
        screen(
            "mixer.channel.prev",
            "選んだチャンネルの前（左）のチャンネルを選びます。ミキサーが開いていればそのストリップを見せます",
            vec![],
        ),
        changes(
            "plugin.preset.next",
            "いちばん手前のプラグインの窓に、次のプリセットを読み込みます（最後の次は最初）",
            vec![],
        ),
        changes(
            "plugin.preset.prev",
            "いちばん手前のプラグインの窓に、前のプリセットを読み込みます（最初の前は最後）",
            vec![],
        ),
        screen(
            "view.page",
            "トラックの列を 1 画面ぶん上（--way up）か下（--way down）へスクロールします",
            vec![ArgDef::choice("way", &["up", "down"], "スクロールの向き").req()],
        ),
        screen(
            "view.focus",
            "キーの効く場所を、アレンジ・下のドック・ブラウザーの順に次へ移します（--way prev で前へ）",
            vec![ArgDef::choice("way", &["next", "prev"], "移す向き").default(json!("next"))],
        ),
        screen("view.fullscreen", "窓を全画面にする・元に戻します", vec![]),
        session(
            "app.quit",
            "窓を閉じて終了します",
            vec![],
            Risk::HumanOnly,
            false,
        ),
        screen(
            "marker.add.named",
            "再生位置にマーカーを追加する枠を開き、名前を入力させます",
            vec![],
        ),
        screen(
            "range.from-clips",
            "選択中のクリップがある時間を範囲にします",
            vec![],
        ),
        screen(
            "browser.search",
            "ブラウザーを開いて、検索の文字を入力できるようにします",
            vec![],
        ),
        screen(
            "browser.tab",
            "ブラウザーかインスペクターをその面で開きます（その面が開いていれば閉じます）",
            vec![
                ArgDef::choice(
                    "tab",
                    &["instruments", "effects", "files", "song_files", "inspector"],
                    "面",
                )
                .req(),
            ],
        ),
        screen(
            "view.browser.sort",
            "ブラウザーの一覧の並べ方を変えます（プールは kind と name だけ）",
            vec![
                ArgDef::choice(
                    "order",
                    &["kind", "vendor", "format", "name", "favourites"],
                    "並べ方",
                )
                .req(),
                ArgDef::choice(
                    "tab",
                    &["instruments", "effects", "song_files"],
                    "タブ（省略時は表示中のタブ）",
                ),
            ],
        ),
        screen(
            "view.cleanup",
            "使われていない録音のファイルと合計の大きさを一覧にして、ごみ箱に移動する前に確かめます",
            vec![],
        ),
        screen(
            "mixer.bank",
            "ミキサーを、見えているチャンネルの数だけ右（--way next）か左（--way prev）へスクロールします",
            vec![ArgDef::choice("way", &["next", "prev"], "スクロールの向き").req()],
        ),
        screen(
            "transport.panic",
            "鳴っているノートをすべて止めます（オールノートオフ）",
            vec![],
        ),
        screen(
            "view.pc-keyboard",
            "PC のキーボードで演奏するのを入・切します",
            vec![],
        ),
        screen(
            "transport.locate",
            "移動先の位置を入力する枠を開きます（初めは再生位置）",
            vec![],
        ),
    ]
}
