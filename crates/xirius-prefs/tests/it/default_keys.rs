//! Every preset's keys and the mouse's defaults, written out as text and compared with
//! `tests/default-keys.txt` (ARC-054 G11). A key added, moved or taken away shows up as a changed
//! line in the PR; a changed table fails until the file is written again.
use xirius_prefs::catalog::PRESETS;
use xirius_prefs::mousemap::{self, Mousemap};
use xirius_prefs::{Keymap, Os};

const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/default-keys.txt");
const WRITE: &str = "XIRIUS_WRITE_KEYS=1 cargo nextest run -p xirius-prefs default_keys";

/// One line per key of every preset, then one per modifier the mouse's defaults give.
fn snapshot() -> String {
    let mut out = String::from(
        "# Every preset's keys and the mouse's defaults (crates/xirius-prefs/tests/it/default_keys.rs).\n\
         # keys\tpreset\tcontext\tkey\tWindows\tMac\tcommand / mouse\tcontext\tmodifier\taction\n",
    );
    for preset in PRESETS {
        let mut map = Keymap::default();
        map.preset = preset.name.to_string();
        for ((context, chord), commands) in map.bindings().iter() {
            for command in commands {
                out.push_str(&format!(
                    "keys\t{}\t{context}\t{chord}\t{}\t{}\t{command}\n",
                    preset.name,
                    chord.display(Os::Win),
                    chord.display(Os::Mac),
                ));
            }
        }
    }
    let mouse = Mousemap::default();
    for context in mousemap::contexts() {
        for (modifier, action) in mouse.table(&context) {
            out.push_str(&format!("mouse\t{context}\t{modifier}\t{action}\n"));
        }
    }
    out
}

#[test]
fn every_presets_keys_and_the_mouses_defaults_match_their_snapshot() {
    let new = snapshot();
    if std::env::var_os("XIRIUS_WRITE_KEYS").is_some() {
        std::fs::write(SNAPSHOT, &new).unwrap();
        return;
    }
    let old = std::fs::read_to_string(SNAPSHOT)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if old == new {
        return;
    }
    // Every line that moved, by its place in the file or in the new table.
    let mut changes = Vec::new();
    for (n, line) in old.lines().enumerate() {
        if !new.lines().any(|l| l == line) {
            changes.push(format!("tests/default-keys.txt:{}: 消えた  {line}", n + 1));
        }
    }
    for (n, line) in new.lines().enumerate() {
        if !old.lines().any(|l| l == line) {
            changes.push(format!("表の {} 行め: 増えた  {line}", n + 1));
        }
    }
    if changes.is_empty() {
        changes.push("行の並びが違う".to_string());
    }
    panic!(
        "既定のキーが tests/default-keys.txt と違う（変えるつもりなら {WRITE}）\n  {}",
        changes.join("\n  ")
    );
}
