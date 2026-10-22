//! Where `xirius play` and `xirius mcp` record MIDI from (ENG-028): `--midi-in` names a port of
//! this PC (`all` for every port, `none` for no MIDI), `--midi-script` plays a file of messages
//! instead of a device (how the tests record MIDI). Without either, playing into the sound
//! device records from every port, and a silent output records no MIDI.

use std::path::Path;

use serde_json::Value;
use xirius_core::CmdError;
use xirius_engine::midi_in::{MidirInput, find_port, midi_ports};
use xirius_engine::record::{MidiInput, MockMidi};
use xirius_session::ext::MidiMaker;

/// The MIDI input of each recording, or `None` for no MIDI. Says on stderr what it chose.
pub fn source(
    midi_in: Option<&str>,
    script: Option<&Path>,
    device: bool,
) -> Result<Option<MidiMaker>, CmdError> {
    if let Some(path) = script {
        let events = read_script(path)?;
        eprintln!(
            "MIDI 入力: {}（{} 個のメッセージ）",
            path.display(),
            events.len()
        );
        return Ok(Some(Box::new(move || {
            Box::new(MockMidi::new(events.clone())) as Box<dyn MidiInput>
        })));
    }
    let wanted = match midi_in {
        Some(n) if n.eq_ignore_ascii_case("none") => return Ok(None),
        Some(n) => Some(n.to_string()),
        None if device => None,
        None => return Ok(None),
    };
    let ports = midi_ports().map_err(CmdError::conflict)?;
    let picked = find_port(wanted.as_deref(), &ports).map_err(CmdError::conflict)?;
    if ports.is_empty() {
        if wanted
            .as_deref()
            .is_some_and(|n| !n.eq_ignore_ascii_case("all"))
        {
            return Err(CmdError::conflict("MIDI 入力デバイスが見つかりません"));
        }
        eprintln!("MIDI 入力: なし（MIDI 入力デバイスが見つかりません）");
        return Ok(None);
    }
    eprintln!(
        "MIDI 入力: {}",
        match picked {
            Some(i) => ports[i].clone(),
            None => ports.join("・"),
        }
    );
    // The ports are opened for each recording and let go when it ends.
    Ok(Some(Box::new(move || {
        match MidirInput::open(wanted.as_deref()) {
            Ok(m) => Box::new(m) as Box<dyn MidiInput>,
            Err(e) => {
                eprintln!("注意: {e}");
                Box::new(MockMidi::new(Vec::new()))
            }
        }
    })))
}

/// `[[frame, [status, data1, data2]], ...]`: each message `frame` frames after the input is
/// first read (when recording starts).
fn read_script(path: &Path) -> Result<Vec<(u64, [u8; 3])>, CmdError> {
    let bad = |why: String| {
        CmdError::invalid(format!(
            "--midi-script {} を読み取れません: {why}（[[フレーム, [0x90, 60, 100]], ...] の JSON で指定します）",
            path.display()
        ))
    };
    let text = std::fs::read_to_string(path).map_err(|e| bad(e.to_string()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
    let items = v
        .as_array()
        .ok_or_else(|| bad("配列ではありません".into()))?;
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let at = |why: &str| bad(format!("{} 番目の{why}", i + 1));
            let frame = item[0]
                .as_u64()
                .ok_or_else(|| at("フレームが整数ではありません"))?;
            let bytes = item[1]
                .as_array()
                .filter(|b| (1..=3).contains(&b.len()))
                .ok_or_else(|| at("メッセージが 1〜3 バイトの配列ではありません"))?;
            let mut msg = [0u8; 3];
            for (k, b) in bytes.iter().enumerate() {
                msg[k] = b
                    .as_u64()
                    .and_then(|b| u8::try_from(b).ok())
                    .ok_or_else(|| at("メッセージに 0〜255 以外の値があります"))?;
            }
            Ok((frame, msg))
        })
        .collect()
}
