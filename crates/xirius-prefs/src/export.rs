//! The keys as a page to print or share (KEY-011).

use xirius_core::command::Registry;

use crate::catalog::CONTEXTS;
use crate::chord::Os;
use crate::ext::command_label;
use crate::keymap::Keymap;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn html(k: &Keymap, os: Os, registry: &Registry) -> String {
    let mut s = String::from(concat!(
        "<!doctype html>\n<html lang=\"ja\"><head><meta charset=\"utf-8\">",
        "<title>Xirius のキーの割り当て</title>\n",
        "<style>body{font-family:system-ui,sans-serif}",
        "table{border-collapse:collapse;margin-bottom:1.5em}",
        "th,td{border:1px solid;padding:.2em .6em;text-align:left}</style></head><body>\n",
    ));
    s.push_str(&format!(
        "<h1>キーの割り当て</h1>\n<p>プリセット: {}（変更した行 {}）</p>\n",
        esc(&k.preset),
        k.changed_rows()
    ));
    let rows = k.rows();
    for c in CONTEXTS {
        let here: Vec<_> = rows
            .iter()
            .filter(|r| r.context == c.name && !r.keys.is_empty())
            .collect();
        if here.is_empty() {
            continue;
        }
        s.push_str(&format!(
            "<h2>{}</h2>\n<table><tr><th>操作</th><th>キー</th></tr>\n",
            esc(c.label)
        ));
        for r in here {
            let keys: Vec<String> = r.keys.iter().map(|k| esc(&k.display(os))).collect();
            s.push_str(&format!(
                "<tr><td>{}{}</td><td>{}</td></tr>\n",
                esc(&command_label(&r.command, registry)),
                if r.changed() { "（変更）" } else { "" },
                keys.join(" / ")
            ));
        }
        s.push_str("</table>\n");
    }
    s.push_str("</body></html>\n");
    s
}
