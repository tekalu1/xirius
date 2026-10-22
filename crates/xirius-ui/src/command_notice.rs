//! What the studio says after a command of the presets and the channel settings ran from the
//! window (MIX-007, MIX-030, PRJ-007): the line at the foot of the lanes, with 元に戻す, or with
//! 詳しく when applying a preset had to adjust something.
//!
//! The words are the mock's. The same command run from the CLI or MCP returns the rows in its
//! data (`adjusted`); this only puts them in front of a person.

use serde_json::Value;

use crate::adjust_report::View;
use crate::notice::Press;
use crate::studio::Studio;

impl Studio {
    /// Says what `command` did, when it is one that says something. `data` is what it returned.
    pub(crate) fn say_command(&mut self, command: &str, args: &Value, data: &Value) {
        let text_of = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
        match command {
            "track.preset.save" => {
                if let Some(name) = text_of(data, "name") {
                    self.tell(format!("トラックのプリセット「{name}」を保存しました"));
                }
            }
            "track.preset.update" => {
                if let Some(name) = text_of(data, "name") {
                    self.tell(format!("トラックのプリセット「{name}」を更新しました"));
                }
            }
            "track.preset.apply" => {
                let name = text_of(args, "preset").unwrap_or_default();
                self.tell_adjusted(
                    format!("トラックのプリセット「{name}」を適用しました"),
                    format!("トラックのプリセット「{name}」"),
                    data,
                );
            }
            "track.preset.add" => {
                let made = text_of(data, "id")
                    .and_then(|id| id.parse().ok())
                    .and_then(|id| self.project().tracks.get(&id).map(|t| t.name.clone()));
                let preset = text_of(args, "preset").unwrap_or_default();
                let name = made.unwrap_or_else(|| preset.clone());
                self.tell_adjusted(
                    format!("トラック「{name}」を追加しました"),
                    format!("トラックのプリセット「{preset}」"),
                    data,
                );
            }
            "channel.settings.paste" => {
                let n = data
                    .get("ids")
                    .and_then(Value::as_array)
                    .map_or(1, Vec::len);
                let text = if n == 1 {
                    "チャンネルの設定を貼り付けました".to_string()
                } else {
                    format!("チャンネルの設定を {n} つのチャンネルに貼り付けました")
                };
                self.tell_with(text, "元に戻す", Press::Undo);
            }
            // The song-files tab (BRW-004): the first file by its name, the rest by their count.
            "file.locate" | "file.collect" => {
                let files = data["files"].as_array().map_or(&[][..], Vec::as_slice);
                let Some(first) = files.first().and_then(|f| f["from"].as_str()) else {
                    return;
                };
                let name = std::path::Path::new(first)
                    .file_name()
                    .map_or_else(|| first.to_string(), |n| n.to_string_lossy().into_owned());
                let n = files.len();
                let text = match (command, n) {
                    ("file.locate", 1) => format!("{name} の場所を指定しました"),
                    ("file.locate", _) => format!("{name} ほか {} 個の場所を指定しました", n - 1),
                    (_, 1) => format!("{name} を曲のフォルダーにコピーしました"),
                    _ => format!("ファイル {n} 個を曲のフォルダーにコピーしました"),
                };
                self.tell_with(text, "元に戻す", Press::Undo);
            }
            // The recordings nothing used went to the recycle bin (BRW-004): how many.
            "project.cleanup" => {
                let n = data["moved"].as_array().map_or(0, Vec::len);
                if let Some(text) = crate::notice::cleaned(n) {
                    self.tell(text);
                }
            }
            _ => {}
        }
    }

    /// `text` with 元に戻す; with the count and 詳しく when the command adjusted something.
    fn tell_adjusted(&mut self, text: String, title: String, data: &Value) {
        let report = data.get("adjusted").and_then(|a| View::of(title, a));
        match report {
            Some(view) => {
                let n = view.rows.len();
                self.tell_with(
                    format!("{text}（{n} 件を調整）"),
                    "詳しく",
                    Press::Report(view),
                );
            }
            None => self.tell_with(text, "元に戻す", Press::Undo),
        }
    }
}
