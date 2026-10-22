//! Short reasons for an unavailable action, shown only on hover.

pub fn command(s: &crate::studio::Studio, name: &str) -> &'static str {
    match name {
        "history.undo" => "元に戻す操作がありません",
        "history.redo" => "やり直す操作がありません",
        "transport.stop" => "再生していません",
        "record.stop" => "録音していません",
        "record.start" => "録音準備のトラックがありません",
        "plugin.restart" => "再起動するプラグインがありません",
        "version.save" | "version.list" | "export.mixdown" | "export.stems"
            if s.song.dir().is_none() =>
        {
            "曲を保存していません"
        }
        "edit.paste" => "コピーした内容がありません",
        n if n.starts_with("note.") => {
            if s.roll.selected.is_empty() {
                "ノートを選択していません"
            } else {
                "このノートには適用できません"
            }
        }
        n if n.starts_with("clip.") => {
            if s.arrange.clips.is_empty() {
                "クリップを選択していません"
            } else {
                "このクリップでは使えません"
            }
        }
        n if n.starts_with("track.") => {
            if s.selected_row.is_none() {
                "トラックを選択していません"
            } else {
                "このトラックでは使えません"
            }
        }
        _ => "現在の状態では使えません",
    }
}

pub fn label(label: &str) -> &'static str {
    if label.contains("貼り付け") {
        "コピーした内容がありません"
    } else if label.contains("ループ") {
        "ループ範囲がありません"
    } else if label.contains("元に戻") {
        "元に戻す操作がありません"
    } else if label.contains("やり直") {
        "やり直す操作がありません"
    } else if label.contains("ノート") {
        "対象のノートを選択していません"
    } else if label.contains("クリップ") {
        "対象のクリップを選択していません"
    } else if label.contains("トラック") {
        "対象のトラックを選択していません"
    } else {
        "現在の状態では使えません"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reasons_describe_the_missing_input() {
        assert_eq!(label("貼り付け"), "コピーした内容がありません");
        assert_eq!(label("クリップを分割"), "対象のクリップを選択していません");
    }
}
