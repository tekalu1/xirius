//! The user's settings (SET-001, KEY-003, KEY-023): `prefs.toml`, `keymap.toml` and
//! `mousemap.toml`. They are not part of the song and do not enter its history; the settings
//! extension (`xirius-prefs`) answers them, so the screen, the CLI and the MCP tools change the
//! same files.

use serde_json::json;

use super::*;

const CONTEXTS: &[&str] = &[
    "global",
    "edit",
    "arrange",
    "roll",
    "lane",
    "automation",
    "mixer",
    "browser",
    "text",
];
/// The tools a held Ctrl can switch the arrow to (`tool.alt`), in the order pressing the arrow
/// again goes through them.
pub const ALT_TOOLS: &[&str] = &["range", "cut", "erase", "draw", "mute", "listen"];
/// The piano roll's (UI-286): its range tool is a marquee, which the arrow already draws on
/// empty grid, so it stays out.
pub const ROLL_ALT_TOOLS: &[&str] = &["draw", "erase", "cut", "mute", "listen"];
const PRESETS: &[&str] = &["studio-pro", "cubase", "logic", "ableton", "reaper"];

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        // prefs.toml
        session(
            "prefs.get",
            "設定の値を表示します（名前か、edit. のような先頭の部分。省略時はすべて）",
            vec![ArgDef::text("key", "設定の名前か、その先頭の部分（例: edit.automation）")],
            Risk::Read,
            false,
        ),
        session(
            "prefs.set",
            "設定の値を変えます（すぐに保存し、開いている画面にも反映されます）",
            vec![
                ArgDef::text("key", "設定の名前（例: edit.automation.follows-clips）").req(),
                ArgDef::text("value", "値（true / false、選択肢の名前、数、フォルダーなど）"),
                ArgDef::flag("add", "一覧の設定（フォルダー）に value を追加します"),
                ArgDef::flag("remove", "一覧の設定（フォルダー）から value を外します"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "prefs.reset",
            "設定を既定の値に戻します（名前か先頭の部分。省略時はすべて）",
            vec![ArgDef::text("key", "設定の名前か、その先頭の部分")],
            Risk::Write,
            false,
        ),
        // keymap.toml
        session(
            "keymap.get",
            "キーの割り当てを一覧します（プリセット・変更した行・衝突）",
            vec![
                ArgDef::text("key", "キーで絞り込み"),
                ArgDef::choice("context", CONTEXTS, "この場所の行だけ"),
                ArgDef::flag("changed", "プリセットから変えた行だけ"),
            ],
            Risk::Read,
            false,
        ),
        session(
            "keymap.which",
            "キーを押したときに実行されるコマンドを表示します",
            vec![
                ArgDef::text("key", "キーかコマンド名（例: Mod+Shift+Z、note.quantize）"),
                ArgDef::text("command", "キーを調べるコマンド"),
                ArgDef::choice("context", CONTEXTS, "どこで押すか"),
            ],
            Risk::Read,
            false,
        ),
        session(
            "keymap.set",
            "キーにコマンドを割り当てます（ほかのコマンドと重なると衝突として表示されます）",
            vec![
                ArgDef::text("key", "キー（例: Mod+Alt+V）").req(),
                ArgDef::text(
                    "command",
                    "実行するコマンド（引数付きでも可。例: \"note.transpose --by 12\"）",
                )
                .req(),
                ArgDef::choice(
                    "context",
                    CONTEXTS,
                    "どこで効くか（省略時はコマンドの既定の場所か global）",
                ),
                ArgDef::text("from", "このキーの代わりにします（コマンドの元のキー）"),
                ArgDef::flag("replace", "同じキーのほかのコマンドからキーを外します"),
                ArgDef::flag("swap", "同じキーのほかのコマンドに --from のキーを渡します"),
                ArgDef::flag("shadow", "広い文脈のキーを意図して隠します"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "keymap.unset",
            "キーの割り当てを外します（--command を付けるとそのコマンドからだけ）",
            vec![
                ArgDef::text("key", "キー").req(),
                ArgDef::choice("context", CONTEXTS, "どこのキーか（省略時はすべての場所）"),
                ArgDef::text("command", "このコマンドからだけ外します"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "keymap.reset",
            "キーの割り当てをプリセットの状態に戻します（キー・コマンド・場所で絞れます。省略時はすべて）",
            vec![
                ArgDef::text("key", "このキーだけ"),
                ArgDef::choice("context", CONTEXTS, "この場所だけ"),
                ArgDef::text("command", "このコマンドの行だけ"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "keymap.preset",
            "キーのプリセットを替えます（既定では変更した行はそのまま残ります）",
            vec![
                ArgDef::choice("name", PRESETS, "プリセット").req(),
                ArgDef::choice(
                    "keep",
                    &["mine", "none"],
                    "自分で変えた行の扱い（mine: 残す、none: 残さずプリセットのとおりにする）",
                )
                .default(json!("mine")),
            ],
            Risk::Write,
            false,
        ),
        session(
            "keymap.conflicts",
            "同じキーに複数のコマンドがある箇所と、OS が使うキーとの重なりを一覧します",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "keymap.import",
            "ほかのソフトの割り当てか keymap.toml を読み込みます（読み込んだキーは変更した行になります）",
            vec![
                ArgDef::choice("source", &["studio-pro", "toml"], "読み込む形式").req(),
                ArgDef::text("path", "読み込むファイル（Studio Pro は省略時に利用者の user.keyscheme）"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "keymap.export",
            "キーの割り当てを書き出します（--to を省くと内容を返します）",
            vec![
                ArgDef::choice("format", &["toml", "json", "html"], "形式").default(json!("toml")),
                ArgDef::text("to", "書き出すファイル（人の操作のみ）"),
            ],
            Risk::Read,
            false,
        ),
        // mousemap.toml
        session(
            "mousemap.get",
            "マウスの割り当てを一覧します（場所を省略すると変えた所と既定の所すべて）",
            vec![ArgDef::text(
                "context",
                "場所（例: roll.select.note-body.drag、roll.select.hold）",
            )],
            Risk::Read,
            false,
        ),
        session(
            "mousemap.set",
            "クリック・ドラッグに操作を、修飾キーを押している間にツールを割り当てます",
            vec![
                ArgDef::text(
                    "context",
                    "場所（<area>.<tool>.<zone>.<click|drag|wheel> か <area>.<tool>.hold。area は roll / arrange / lane / automation / ruler）",
                )
                .req(),
                ArgDef::text(
                    "modifier",
                    "修飾キー（none / Shift / Mod / Mod+Shift / Alt / Mod+Alt、ドラッグ中は during.Shift など）",
                )
                .req(),
                ArgDef::text(
                    "action",
                    "操作（例: note.duplicate）か、押している間のツール（例: tool:draw）",
                )
                .req(),
                ArgDef::text(
                    "move-from",
                    "ツールの切り替えを、この修飾キーから移す（移し先が使われていればエラーにして保存しない）",
                ),
            ],
            Risk::Write,
            false,
        ),
        session(
            "mousemap.conflicts",
            "押している間のツールの切り替えが、同じ修飾キーのクリック・ドラッグを隠している箇所を一覧します",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "mousemap.unset",
            "マウスの割り当てを外し、何もしないようにします",
            vec![
                ArgDef::text("context", "場所").req(),
                ArgDef::text("modifier", "修飾キー").req(),
            ],
            Risk::Write,
            false,
        ),
        session(
            "tool.alt",
            "矢印のまま Ctrl を押している間になる「代わりの道具」を選びます（省略時は次の道具。上のバーでは青い下線が付きます）",
            vec![
                ArgDef::choice("tool", ALT_TOOLS, "代わりの道具"),
                ArgDef::choice("area", &["arrange", "roll"], "どの画面か（省略時はアレンジ）"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "mousemap.reset",
            "マウスの割り当てを既定に戻します（場所・修飾キーで絞れます。省略時はすべて）",
            vec![
                ArgDef::text("context", "この場所だけ"),
                ArgDef::text("modifier", "この修飾キーだけ"),
            ],
            Risk::Write,
            false,
        ),
        // midimap.toml
        session(
            "midimap.list",
            "コントローラーの割り当てを一覧します（つまみやパッドと、動かすパートの値かコマンド）",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "midimap.set",
            "コントローラーのつまみ（CC）やパッド（ノート）を、パートの値かコマンドに割り当てます（同じつまみは置き換え、割り当てたメッセージは録音に入りません）",
            vec![
                ArgDef::text("message", "メッセージ（cc:21 のつまみ、note:41 のパッド）").req(),
                ArgDef::text(
                    "target",
                    "動かすもの（track.volume:<トラックの id>・track.pan:<トラックの id>・コマンド名）",
                )
                .req(),
                ArgDef::text("port", "MIDI の入力ポートの名前（省略時はどのポートでも）"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "midimap.unset",
            "コントローラーの割り当てを外します（--message を省くとすべて）",
            vec![
                ArgDef::text("message", "メッセージ（cc:21 のつまみ、note:41 のパッド）"),
                ArgDef::text("port", "MIDI の入力ポートの名前（省略時はどのポートでも）"),
                ArgDef::text(
                    "target",
                    "割り当て先（track.volume:<id>・track.pan:<id>・コマンド）で外す",
                ),
            ],
            Risk::Write,
            false,
        ),
        // macros.toml
        session(
            "macro.list",
            "マクロを一覧します（名前と、順に行うコマンド、割り当てたキー）",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "macro.set",
            "マクロを作ります（同じ名前があれば手順を置き換えます）。手順は曲を編集するコマンドの行で、\
             --steps に [{\"command\": \"note.quantize --grid 1/16\"}, ...] の形で渡します。\
             キーは keymap.set に \"macro.run --name <名前>\" を渡して割り当てます",
            vec![
                ArgDef::text("name", "マクロの名前").req(),
                ArgDef::objects("steps", "順に行うコマンドの並び（{\"command\": \"コマンドの行\"}）"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "macro.rename",
            "マクロの名前を変えます（割り当てたキーはそのまま使えます）",
            vec![
                ArgDef::text("name", "いまの名前").req(),
                ArgDef::text("new-name", "新しい名前").req(),
            ],
            Risk::Write,
            false,
        ),
        session(
            "macro.remove",
            "マクロを消します（割り当てたキーも外します）",
            vec![ArgDef::text("name", "マクロの名前").req()],
            Risk::Write,
            false,
        ),
        session(
            "macro.run",
            "マクロの手順を順に行います。手順は 1 回の取り消しでまとめて元に戻せます",
            vec![ArgDef::text("name", "マクロの名前").req()],
            Risk::Write,
            false,
        ),
        session(
            "edit.ripple",
            "リップル編集を切り替えます（クリップの削除・切り取り・範囲の削除・長さの変更で、同じトラックの後ろのクリップを詰める・押す。省略時は現在の状態を切り替え）",
            on_off().to_vec(),
            Risk::Write,
            false,
        ),
    ]
}
