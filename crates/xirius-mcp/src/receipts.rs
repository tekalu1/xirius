//! Receipts survive a caller's timeout; the operation keeps its identity.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use xirius_core::{
    CmdError,
    protocol::{Receipt, ReceiptState, VERSION},
};

#[derive(Clone)]
pub struct Receipts(Arc<Inner>);
struct Inner {
    records: Mutex<BTreeMap<String, Record>>,
    changed: Condvar,
    events: broadcast::Sender<Value>,
    stopped: AtomicBool,
}
struct Record {
    fingerprint: String,
    receipt: Receipt,
    result: Option<Value>,
}

impl Default for Receipts {
    fn default() -> Self {
        let (events, _) = broadcast::channel(1024);
        Self(Arc::new(Inner {
            records: Mutex::default(),
            changed: Condvar::new(),
            events,
            stopped: AtomicBool::new(false),
        }))
    }
}

impl Receipts {
    pub fn shutdown(&self) {
        self.0.stopped.store(true, Ordering::Release);
        let _ = self.0.events.send(json!({"shutdown": true}));
    }
    pub fn stopped(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.0.events.subscribe()
    }

    /// Sends what a run had to tell (`export.stale`, EXP-023) down the event stream.
    pub fn publish(&self, event: Value) {
        let _ = self.0.events.send(event);
    }

    pub fn accept(&self, id: &str, command: &str, fingerprint: String) -> Result<bool, CmdError> {
        let mut records = self.0.records.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = records.get(id) {
            if record.fingerprint != fingerprint {
                return Err(CmdError::conflict(
                    "同じ操作キーで異なる操作は実行できません",
                ));
            }
            return Ok(false);
        }
        if records.len() >= 4096
            || records
                .values()
                .map(|record| record.fingerprint.len())
                .sum::<usize>()
                + fingerprint.len()
                > 64 * 1024 * 1024
        {
            return Err(CmdError::range(
                "受領証の保存上限に達しました。MCP の接続を再起動してください",
            ));
        }
        let receipt = Receipt {
            id: id.into(),
            command: command.into(),
            state: ReceiptState::Accepted,
            command_protocol: VERSION,
        };
        let _ = self.0.events.send(json!({"receipt": receipt}));
        records.insert(
            id.into(),
            Record {
                fingerprint,
                receipt,
                result: None,
            },
        );
        Ok(true)
    }

    pub fn update(&self, id: &str, state: ReceiptState, result: Option<Value>) {
        let mut records = self.0.records.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = records.get_mut(id) {
            record.receipt.state = state;
            record.result = result;
            let _ = self
                .0
                .events
                .send(json!({"receipt": record.receipt, "result": record.result}));
            self.0.changed.notify_all();
        }
    }

    pub fn get(&self, id: &str) -> Result<Value, CmdError> {
        self.wait(id, Duration::ZERO)
    }

    /// Waits until the operation has started (or come to a person's answer, or finished),
    /// so a caller is told what it waits on instead of a bare "accepted".
    pub fn wait_started(&self, id: &str, timeout: Duration) -> Result<Value, CmdError> {
        let records = self.0.records.lock().unwrap_or_else(|e| e.into_inner());
        if !records.contains_key(id) {
            return Err(CmdError::not_found("受領証が見つかりません"));
        }
        let (records, _) = self
            .0
            .changed
            .wait_timeout_while(records, timeout, |r| {
                r[id].result.is_none() && r[id].receipt.state == ReceiptState::Accepted
            })
            .unwrap_or_else(|e| e.into_inner());
        let record = &records[id];
        let mut value = record
            .result
            .clone()
            .unwrap_or_else(|| json!({"ok": true, "pending": true}));
        value["receipt"] = json!(record.receipt.clone());
        Ok(value)
    }

    /// Waits until the operation is done, however long that is. A person's answer is a
    /// result too (the operation then waits on them in the background).
    pub fn wait_done(&self, id: &str) -> Result<Value, CmdError> {
        loop {
            let value = self.wait(id, Duration::from_secs(60))?;
            if value["receipt"]["state"] != "unknown" {
                return Ok(value);
            }
        }
    }

    pub fn wait(&self, id: &str, timeout: Duration) -> Result<Value, CmdError> {
        let records = self.0.records.lock().unwrap_or_else(|e| e.into_inner());
        if !records.contains_key(id) {
            return Err(CmdError::not_found("受領証が見つかりません"));
        }
        let (records, timed) = self
            .0
            .changed
            .wait_timeout_while(records, timeout, |r| {
                r[id].result.is_none() && r[id].receipt.state != ReceiptState::Completed
            })
            .unwrap_or_else(|e| e.into_inner());
        let record = &records[id];
        let mut receipt = record.receipt.clone();
        if timed.timed_out() && timeout != Duration::ZERO && record.result.is_none() {
            receipt.state = ReceiptState::Unknown;
        }
        let mut value = record
            .result
            .clone()
            .unwrap_or_else(|| json!({"ok": true, "pending": true}));
        value["receipt"] = json!(receipt);
        Ok(value)
    }
}
