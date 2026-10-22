//! Commands used by input maps. View actions never enter song history.
use super::*;

pub(super) fn defs() -> Vec<CommandDef> {
    let mut out = Vec::new();
    for (name, label) in [
        ("project.new", "新しい曲"),
        ("view.export-stems", "ステムを書き出す…"),
        ("transport.start", "再生を開始"),
        ("transport.metronome", "メトロノーム"),
        ("view.palette", "コマンドパレット"),
        ("view.context-menu", "コンテキストメニューを開く"),
        ("view.settings", "設定"),
        ("view.lanes", "オートメーションを表示"),
        ("view.mixer", "ミキサーを表示"),
        ("view.record-panel", "録音パネルを表示"),
        ("view.browser", "ブラウザーを表示"),
        ("view.autoscroll", "自動スクロール"),
        ("tool.select", "選択ツール"),
        ("tool.range", "範囲ツール"),
        ("tool.cut", "分割ツール"),
        ("tool.erase", "消しゴムツール"),
        ("tool.draw", "鉛筆ツール"),
        ("tool.mute", "ミュートツール"),
        ("tool.listen", "試聴ツール"),
        ("view.zoom.full", "全体を表示"),
        ("view.zoom.loop", "ループを表示"),
        ("view.zoom.undo", "ズームを戻す"),
        ("view.zoom.redo", "ズームをやり直す"),
        ("view.zoom.toggle", "ズームを切り替え"),
        ("view.zoom.store", "ズームを記憶"),
        ("clip.open", "エディターを表示"),
        ("view.instrument", "インストゥルメントを表示"),
        ("view.add-track", "トラックを追加…"),
        ("view.zoom.rect", "囲んだ範囲を表示"),
        ("view.tracklist.collapse", "トラック一覧を畳む"),
    ] {
        // These are window actions, dispatched by the same registry name as the palette.
        if name.starts_with("tool.") {
            // The window picks the tool; a session with a screen queues it for the window
            // (and one without says it cannot).
            out.push(session(name, label, vec![], Risk::Read, false).screen_only());
        } else if name == "view.browser" {
            // BRW-004: with a tab, that tab opens (and the browser stays open).
            let tab = ArgDef::choice(
                "tab",
                &["instruments", "effects", "files", "song_files"],
                "開くタブ（省略時はブラウザーを開閉します）",
            );
            out.push(session(name, label, vec![tab], Risk::Read, false).screen_only());
        } else {
            // A new song replaces the one open: the AI must see it as a change.
            let risk = if name == "project.new" {
                Risk::Write
            } else {
                Risk::Read
            };
            out.push(session(name, label, vec![], risk, false).screen_only());
        }
    }
    out.push(
        session(
            "view.export-run",
            "書き出しを開始",
            vec![],
            Risk::Guarded,
            false,
        )
        .screen_only(),
    );
    out.push(
        read(
            "snap.invert",
            "ドラッグ中のスナップを一時反転",
            vec![],
            |_, _, _, _| Ok(Output::with("", json!({"mark":"snap.invert"}))),
        )
        .screen_only(),
    );
    out.push(session(
        "view.zoom.time",
        "ズームを変更",
        vec![
            ArgDef::float("by", "段数"),
            ArgDef::float(
                "to",
                "1 拍の幅（px、8 から、1 サンプルが 6 px になる幅まで）。見ている中央の位置を動かさずに合わせます",
            ),
        ],
        Risk::Read,
        false,
    )
    .screen_only());
    out.push(
        session(
            "view.zoom.data",
            "ズームを変更",
            vec![ArgDef::float("by", "段数").req()],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out.push(
        session(
            "view.zoom.wave",
            "波形の縦の拡大",
            vec![
                ArgDef::float("by", "段数（×1・×2・×4 を上下）"),
                ArgDef::float("to", "倍率（1・2・4）"),
            ],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out.push(
        session(
            "view.zoom.selection",
            "選択範囲を表示",
            vec![ArgDef::flag("horizontal", "横だけ")],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out.push(
        session(
            "auto.lane.next",
            "パラメーターを切り替え",
            vec![ArgDef::flag("back", "前へ")],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out.push(
        session(
            "edit.nudge.left",
            "グリッドの前へ",
            vec![ArgDef::grid("grid", "動かす幅（無ければ今のグリッド）")],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out.push(
        session(
            "edit.nudge.right",
            "グリッドの後ろへ",
            vec![ArgDef::grid("grid", "動かす幅（無ければ今のグリッド）")],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out.push(
        session(
            "loop.selection",
            "選択範囲をループ",
            vec![ArgDef::flag("no-snap", "グリッドに合わせない")],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out.push(
        session(
            "transport.locate-selection",
            "選択範囲に移動",
            vec![],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out.push(
        session(
            "transport.locate-mouse",
            "ポインターに移動",
            vec![],
            Risk::Read,
            false,
        )
        .screen_only(),
    );
    out
}
