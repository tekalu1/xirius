//! The export panel's side of the song (EXP-023): it is told which tracks were edited in a way
//! the song does not show (`export.touch`, as a plugin's own window), and, from
//! `export.watch` on, it tells with an `export.stale` event when the files of an export turn
//! stale or current again.
//!
//! The events go out with the result of the run that made them
//! ([`RunResult::events`](xirius_core::session::RunResult::events)); the MCP server also sends
//! them down its event stream.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// Names [`crate::ext::SessionExt`] answers here.
pub const NAMES: [&str; 2] = ["export.touch", "export.watch"];

/// The name of the event.
pub const EVENT: &str = "export.stale";

/// What an `export.stale` event says of a finished export whose record has been annotated
/// ([`crate::export_stale::annotate`]); `None` for one that says nothing of staleness.
pub fn summary(record: &Value) -> Option<Value> {
    if record["state"] != "done" || record.get("stale").is_none() {
        return None;
    }
    let files: Vec<Value> = record["stale_files"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| json!({ "name": f["name"], "path": f["path"], "stale": f["stale"] }))
                .collect()
        })
        .unwrap_or_default();
    Some(json!({
        "event": EVENT,
        "job": record["job"],
        "stale": record["stale"],
        "stale_tracks": record["stale_tracks"],
        "files": files,
    }))
}

/// Whether the events are sent, how each export stood when last looked at, and the events not
/// taken yet.
#[derive(Debug, Default)]
pub struct Watch {
    on: bool,
    seen: BTreeMap<String, Value>,
    events: Vec<Value>,
}

impl Watch {
    pub fn on(&self) -> bool {
        self.on
    }

    /// Starts or stops. Starting takes how the exports of `records` stand without telling of
    /// them; stopping forgets them and the events not taken.
    pub fn set(&mut self, on: bool, records: &[Value]) {
        self.on = on;
        self.seen.clear();
        self.events.clear();
        if on {
            self.seen = Self::standing(records);
        }
    }

    fn standing(records: &[Value]) -> BTreeMap<String, Value> {
        records
            .iter()
            .filter_map(|r| Some((r["job"].as_str()?.to_string(), summary(r)?)))
            .collect()
    }

    /// Tells of each export of the annotated `records` that stands otherwise than when last
    /// looked at, and of one first seen stale. An export gone from the list says nothing.
    pub fn look(&mut self, records: &[Value]) {
        if !self.on {
            return;
        }
        let now = Self::standing(records);
        for (job, s) in &now {
            let told = match self.seen.get(job) {
                Some(before) => before != s,
                None => s["stale"] == true,
            };
            if told {
                self.events.push(s.clone());
            }
        }
        self.seen = now;
    }

    /// The events not taken yet, oldest first.
    pub fn take(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(job: &str, stale: bool) -> Value {
        json!({
            "job": job,
            "state": "done",
            "stale": stale,
            "stale_tracks": if stale { json!([{ "id": "t1", "name": "Keys" }]) } else { json!([]) },
            "stale_files": [{ "name": "a", "path": "C:/x/a.wav", "strip": "t1", "stale": stale, "tracks": [] }],
        })
    }

    #[test]
    fn tells_only_of_a_change_while_on() {
        let mut w = Watch::default();
        w.look(&[record("e1", true)]);
        assert!(w.take().is_empty(), "nothing while off");

        w.set(true, &[record("e1", false)]);
        assert!(w.take().is_empty(), "starting tells nothing");
        w.look(&[record("e1", false)]);
        assert!(w.take().is_empty(), "nothing changed");

        w.look(&[record("e1", true)]);
        let told = w.take();
        assert_eq!(told.len(), 1);
        assert_eq!(told[0]["event"], EVENT);
        assert_eq!(told[0]["job"], "e1");
        assert_eq!(told[0]["stale"], true);
        assert_eq!(told[0]["stale_tracks"][0]["name"], "Keys");
        assert_eq!(
            told[0]["files"],
            json!([{ "name": "a", "path": "C:/x/a.wav", "stale": true }])
        );

        // Back to current, and an export first seen: told only when stale.
        w.look(&[record("e1", false), record("e2", false)]);
        let told = w.take();
        assert_eq!(told.len(), 1);
        assert_eq!(
            (told[0]["job"].clone(), told[0]["stale"].clone()),
            (json!("e1"), json!(false))
        );
        w.look(&[record("e1", false), record("e2", false), record("e3", true)]);
        assert_eq!(w.take()[0]["job"], "e3");

        // A running export says nothing.
        let mut running = record("e4", true);
        running["state"] = json!("running");
        w.look(&[running]);
        assert!(w.take().is_empty());

        w.look(&[record("e1", true)]);
        w.set(false, &[]);
        assert!(w.take().is_empty(), "stopping forgets what was not taken");
    }
}
