//! What the browser keeps between runs (BRW-001, BRW-005, BRW-012, BRW-015): favourite and
//! hidden instruments and effects, and the folders the file tab has a tab for. They are the
//! user's, not the song's, so the session answers them and none is in the history.
//!
//! The files on the disk (BRW-013, BRW-014, BRW-004): shown, renamed, made, moved to the
//! recycle bin and the song's listed. The session answers these too; a rename leaves the
//! song's change of references as a step of the history.

use super::*;

pub(super) fn defs() -> Vec<CommandDef> {
    let item = || {
        ArgDef::text(
            "item",
            "インストゥルメントまたはエフェクトの名前（ブラウザーに出る名前）",
        )
        .req()
    };
    let location = || ArgDef::text("location", "場所のタブの名前またはフォルダーのパス").req();
    vec![
        session(
            "browser.favorite",
            "インストゥルメントやエフェクトをお気に入りにします（--off で外します。次に起動しても残ります）",
            [vec![item()], on_off().to_vec()].concat(),
            Risk::Read,
            false,
        ),
        session(
            "browser.hide",
            "インストゥルメントやエフェクトをブラウザーの一覧から隠します（--off で戻します。検索では見つかります）",
            [vec![item()], on_off().to_vec()].concat(),
            Risk::Read,
            false,
        ),
        session(
            "view.browser.hidden",
            "隠したインストゥルメントやエフェクトも一覧に表示します（--off で表示しません）",
            on_off().to_vec(),
            Risk::Read,
            false,
        ),
        session(
            "location.list",
            "ファイルのブラウザーの場所のタブを一覧します（先頭は曲のフォルダー）",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "location.add",
            "フォルダーを場所のタブとしてファイルのブラウザーに足します",
            vec![
                ArgDef::text("path", "フォルダーのパス").req(),
                ArgDef::text("name", "タブの名前（省略時はフォルダー名）"),
            ],
            Risk::Read,
            false,
        ),
        session(
            "location.rename",
            "場所のタブの名前を変えます（曲のフォルダーは変えられません）",
            vec![location(), ArgDef::text("name", "新しい名前").req()],
            Risk::Read,
            false,
        ),
        session(
            "location.remove",
            "場所のタブを外します（フォルダーとその中身はそのままです。曲のフォルダーは外せません）",
            vec![location()],
            Risk::Read,
            false,
        ),
        session(
            "file.reveal_os",
            "ファイルやフォルダーをエクスプローラー（Finder）で表示します（相対パスは曲のフォルダーから。省くと曲のフォルダー）",
            vec![ArgDef::text(
                "path",
                "ファイルまたはフォルダーのパス（省略時は曲のフォルダー）",
            )],
            Risk::Read,
            false,
        ),
        session(
            "file.rename",
            "ファイルやフォルダーの名前を変えます。曲が使うファイルは曲の中の参照も新しい名前に付け替えます（1 操作で取り消せ、取り消すとファイルの名前も戻ります）。拡張子を省くと元の拡張子のままです",
            vec![
                ArgDef::text(
                    "path",
                    "ファイルまたはフォルダーのパス（相対パスは曲のフォルダーから）",
                )
                .req(),
                ArgDef::text("name", "新しい名前").req(),
            ],
            Risk::Guarded,
            false,
        ),
        session(
            "file.trash",
            "ファイルやフォルダーを OS のごみ箱に移動します。曲が使うファイルと、それを含むフォルダーは移動できません",
            vec![
                ArgDef::text(
                    "path",
                    "ファイルまたはフォルダーのパス（相対パスは曲のフォルダーから）",
                )
                .req(),
            ],
            Risk::Guarded,
            false,
        ),
        session(
            "file.add.folder",
            "フォルダーの中に新しいフォルダーを作成します",
            vec![
                ArgDef::text(
                    "path",
                    "作成する場所のフォルダーのパス（相対パスは曲のフォルダーから）",
                )
                .req(),
                ArgDef::text("name", "新しいフォルダーの名前").req(),
            ],
            Risk::Guarded,
            false,
        ),
        session(
            "file.list",
            "曲が使う音声ファイル（クリップ・フリーズ・機器のサンプルの音）と、使われていない録音を一覧します。ファイルごとに、種類（record・import・bounce・freeze）、大きさ、使うクリップと機器の数、曲のフォルダーの中か外か、見つかるかを表示します",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "file.collect",
            "曲のフォルダーの外のファイルを、置き場所の Imports にコピーし、使うクリップと機器をコピーへ付け替えます（1 回で取り消せます。取り消してもコピーは残ります）",
            vec![ArgDef::text(
                "paths",
                "コピーするファイル（file.list の file。いくつも渡すときは | で区切ります。省略時は曲のフォルダーの外のファイルすべて）",
            )],
            Risk::Write,
            false,
        ),
        session(
            "file.locate",
            "見つからないファイルの場所を指定し、使うクリップと機器を付け替えます（1 回で取り消せます）。--new_path は同じフォルダーにあったほかの見つからないファイルも、選んだファイルの隣の同じ名前のファイルへ付け替えます",
            vec![
                ArgDef::text("path", "見つからないファイル（file.list の file）"),
                ArgDef::text(
                    "new_path",
                    "付け替える先のファイル（--path と一緒に指定します）",
                ),
                ArgDef::text(
                    "folder",
                    "探すフォルダー（見つからないファイルを、この下から同じ名前で探します。--path を省くと全部）",
                ),
            ],
            Risk::Write,
            false,
        ),
        read(
            "select.using_file",
            "ファイルを使うクリップを選択します",
            vec![ArgDef::text("file", "ファイル（file.list の file）").req()],
            |p, s, _, a| {
                let norm = |f: &str| {
                    let f = f.trim().replace('\\', "/");
                    if cfg!(any(windows, target_os = "macos")) {
                        f.to_lowercase()
                    } else {
                        f
                    }
                };
                let want = norm(a.str("file")?);
                let mut ids = Vec::new();
                for t in p.tracks.values() {
                    for c in &t.clips {
                        if let ClipBody::Audio { file, .. } = &c.body
                            && norm(file) == want
                        {
                            ids.push(AnyId::Clip(c.id));
                        }
                    }
                }
                if ids.is_empty() {
                    return Err(CmdError::new(
                        crate::ErrorCode::NothingToDo,
                        format!("{} を使うクリップはありません", a.str("file")?),
                    ));
                }
                s.selection = ids;
                Ok(Output::msg(format!(
                    "クリップ {} 個を選択しました",
                    s.selection.len()
                )))
            },
        ),
    ]
}
