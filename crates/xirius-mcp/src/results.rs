//! Immutable result pages. Offsets count Unicode characters, never bytes.

use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use xirius_core::CmdError;

pub const PAGE_CHARS: usize = 4096;

#[derive(Clone, Default)]
pub struct Results(Arc<Mutex<Pages>>);

#[derive(Default)]
struct Pages {
    values: BTreeMap<String, String>,
    order: VecDeque<String>,
    bytes: usize,
}
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESULTS: usize = 256;
static NEXT_RESULT: AtomicU64 = AtomicU64::new(0);

impl Results {
    pub fn store(&self, id: &str, value: Value) -> Value {
        let text = value.to_string();
        let chars = text.chars().count();
        if chars <= PAGE_CHARS {
            return value;
        }
        if text.len() > MAX_BYTES {
            return json!({"ok": false, "error": CmdError::range("結果の保存上限を超えました。対象を絞って取得してください"), "summary": summary(&value)});
        }
        let mut pages = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let id = if pages.values.contains_key(id) {
            format!("{id}-{}", NEXT_RESULT.fetch_add(1, Ordering::Relaxed))
        } else {
            id.to_owned()
        };
        while pages.values.len() >= MAX_RESULTS || pages.bytes + text.len() > MAX_BYTES {
            if let Some(old) = pages.order.pop_front()
                && let Some(value) = pages.values.remove(&old)
            {
                pages.bytes -= value.len();
            }
        }
        pages.bytes += text.len();
        pages.values.insert(id.clone(), text);
        pages.order.push_back(id.clone());
        let mut output = json!({"result_id": id, "chars": chars,
            "pages": chars.div_ceil(PAGE_CHARS), "page_chars": PAGE_CHARS,
            "summary": summary(&value)});
        for key in [
            "ok",
            "command",
            "message",
            "dry_run",
            "forced_dry_run",
            "confirmation_required",
            "error",
            "step",
        ] {
            if let Some(value) = value.get(key) {
                output[key] = value.clone();
            }
        }
        if let Some(decision) = value.get("decision").and_then(Value::as_object) {
            let mut decision = decision.clone();
            decision.remove("args");
            output["decision"] = json!(decision);
        }
        for key in ["state", "job", "state_hash"] {
            if let Some(value) = value["data"].get(key) {
                output["data"][key] = value.clone();
            }
        }
        output
    }

    pub fn get_result(&self, id: &str, page: usize) -> Result<Value, CmdError> {
        let pages = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let text = pages
            .values
            .get(id)
            .ok_or_else(|| CmdError::not_found("結果が見つかりません"))?;
        let total = text.chars().count().div_ceil(PAGE_CHARS);
        if page >= total {
            return Err(CmdError::range("結果のページが範囲外です"));
        }
        let text: String = text
            .chars()
            .skip(page * PAGE_CHARS)
            .take(PAGE_CHARS)
            .collect();
        Ok(json!({"ok": true, "result_id": id, "page": page, "pages": total, "text": text}))
    }
}

fn summary(value: &Value) -> Value {
    let mut summary = serde_json::Map::new();
    for key in ["command", "message"] {
        if let Some(value) = value.get(key) {
            summary.insert(key.into(), value.clone());
        }
    }
    if let Some(counts) = value["changes"].get("counts") {
        summary.insert("changes".into(), counts.clone());
    }
    if let Some(hash) = value["data"].get("state_hash") {
        summary.insert("state_hash".into(), hash.clone());
    }
    Value::Object(summary)
}
