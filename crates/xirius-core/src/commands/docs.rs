//! Reproducible command reference from the same schemas used by MCP and CLI.
use crate::command::{Handler, Registry, Risk};
use std::fmt::Write;

pub fn markdown(registry: &Registry) -> String {
    let mut out = String::from(
        "# コマンド一覧\n\nこの文書は `xirius --commands-doc docs/commands.md` でコマンド表から生成します。\n検査は `xirius --commands-doc --check docs/commands.md`。\n\nCLI は `xirius --song <曲のフォルダー> <コマンド> --json`。MCP のツール名は `.` を `_` に置き換えます。\n結果は `ok`、`command`、`message`、`data`、`changes`、`dry_run`。失敗は `ok: false` と型付きの `error`。編集結果の `data.state_hash` は変更前後の状態を示します。\nMCP の編集には受領証が付き、完了はイベントでも届きます。大きい結果は要約と `ops.get-result` のページで取得します。\n\n",
    );
    let mut defs: Vec<_> = registry.all().iter().collect();
    defs.sort_by_key(|def| def.name);
    for def in defs {
        let schema = def.json_schema();
        writeln!(
            out,
            "## `{}`\n\n{}\n\n{}\n",
            def.name,
            def.label().unwrap_or(def.name),
            def.summary
        )
        .unwrap();
        let pre = match def.risk {
            Risk::Read => "AI の権限が読み取り以上",
            Risk::Write => "AI の権限が編集以上（読み取りではプレビューのみ）",
            Risk::Destructive => "AI の権限が編集以上。削除の確認設定に従う",
            Risk::Guarded if def.name == "ai.permission.set" => {
                "AI からの変更はユーザーの承認が必要"
            }
            Risk::Guarded => "AI の権限が保存以上、またはユーザーの承認",
            Risk::HumanOnly => "ユーザーのみ",
        };
        let history = if def.history {
            "1 操作で取り消し可能"
        } else if matches!(def.handler, Handler::Session) && def.risk != Risk::Read {
            "実行結果の step で確認"
        } else {
            "記録なし"
        };
        writeln!(out, "事前条件: {pre}。指定した ID が存在し、引数が次の型・範囲に一致すること。\n\n履歴: {history}。アイコン: `{}`。\n", def.icon()).unwrap();
        let bindings: Vec<_> = def
            .bindings()
            .map(|e| format!("{}: {} (`{}`)", e.context, e.keys.join(" / "), e.command))
            .collect();
        if !bindings.is_empty() {
            writeln!(out, "キー: {}。\n", bindings.join("、")).unwrap();
        }
        out.push_str("| 引数 | 必須 | 型・範囲・既定値 | 内容 |\n|---|---|---|---|\n");
        let mut properties: Vec<_> = schema["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .iter()
            .collect();
        properties.sort_by_key(|(name, _)| *name);
        for (name, property) in properties {
            let mut ty = property.clone();
            ty.as_object_mut().unwrap().remove("description");
            let ty = canonical(&ty);
            let required = def.args.iter().any(|a| a.name == name && a.required);
            let help = property["description"]
                .as_str()
                .unwrap_or("")
                .replace('|', "\\|")
                .replace('\n', " ");
            writeln!(
                out,
                "| `{name}` | {} | `{}` | {help} |",
                if required { "必須" } else { "—" },
                ty.to_string().replace('|', "\\|")
            )
            .unwrap();
        }
        out.push('\n');
    }
    out
}

fn canonical(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(values) => {
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_by_key(|(key, _)| *key);
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonical).collect())
        }
        other => other.clone(),
    }
}
