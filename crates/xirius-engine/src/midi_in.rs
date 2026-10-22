//! MIDI input devices (ENG-028), through midir.
//!
//! [`MidirInput`] connects to the chosen ports on a thread of its own, which holds the
//! connections until the input is dropped. Each port's callback pushes its messages into a
//! lock-free ring of its own; the audio thread reads them at the start of the next block, with
//! the port they came in on, so an instrument track can take one keyboard of several.
//! System messages (clock, SysEx) and messages longer than three bytes are left out.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rtrb::{Consumer, RingBuffer};

use crate::record::MidiInput;

/// Messages a port holds between two blocks.
const RING: usize = 1024;

/// The client name the ports see.
const CLIENT: &str = "Xirius";

/// The names of the MIDI input ports on this PC, in midir's order.
pub fn midi_ports() -> Result<Vec<String>, String> {
    let midi =
        midir::MidiInput::new(CLIENT).map_err(|e| format!("MIDI 入力を初期化できません: {e}"))?;
    Ok(midi
        .ports()
        .iter()
        .map(|p| midi.port_name(p).unwrap_or_else(|_| "(名前なし)".into()))
        .collect())
}

/// The port (an index into `ports`) `name` names: the exact name, else the same name in other
/// case, else the one port whose name contains it. `None`: every port (no name, `all`, or ports
/// without names).
pub fn find_port(name: Option<&str>, ports: &[String]) -> Result<Option<usize>, String> {
    let Some(text) = name.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let lower = text.to_lowercase();
    if ports.is_empty() || ["all", "midi", "すべて"].contains(&lower.as_str()) {
        return Ok(None);
    }
    if let Some(i) = ports.iter().position(|p| p == text) {
        return Ok(Some(i));
    }
    if let Some(i) = ports.iter().position(|p| p.to_lowercase() == lower) {
        return Ok(Some(i));
    }
    let hits: Vec<usize> = (0..ports.len())
        .filter(|&i| ports[i].to_lowercase().contains(&lower))
        .collect();
    match hits[..] {
        [i] => Ok(Some(i)),
        [] => Err(format!(
            "MIDI 入力「{text}」が見つかりません（ポート: {}）",
            if ports.is_empty() {
                "なし".to_string()
            } else {
                ports.join("・")
            }
        )),
        _ => Err(format!(
            "MIDI 入力「{text}」に当てはまるポートが複数あります（{}）",
            hits.iter()
                .map(|&i| ports[i].as_str())
                .collect::<Vec<_>>()
                .join("・")
        )),
    }
}

/// The MIDI input ports of this PC, connected for as long as this lives.
pub struct MidirInput {
    rings: Vec<Consumer<[u8; 3]>>,
    names: Vec<String>,
    stop: Arc<AtomicBool>,
}

impl MidirInput {
    /// Connects to the port `name` names ([`find_port`]), or to every port when `None` or
    /// `all`. An error when there is no port to connect to.
    pub fn open(name: Option<&str>) -> Result<MidirInput, String> {
        let wanted = name.map(str::to_string);
        let stop = Arc::new(AtomicBool::new(false));
        type Ready = Result<Vec<(String, Consumer<[u8; 3]>)>, String>;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Ready>();
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("xirius-midi-in".into())
            .spawn(move || {
                let opened = (|| {
                    let names = midi_ports()?;
                    if names.is_empty() {
                        return Err("MIDI 入力デバイスが見つかりません".to_string());
                    }
                    let picked: Vec<usize> = match find_port(wanted.as_deref(), &names)? {
                        Some(i) => vec![i],
                        None => (0..names.len()).collect(),
                    };
                    let mut conns = Vec::new();
                    let mut rings = Vec::new();
                    for i in picked {
                        // A connection takes its midir::MidiInput, so each port has its own.
                        let midi = midir::MidiInput::new(CLIENT)
                            .map_err(|e| format!("MIDI 入力を初期化できません: {e}"))?;
                        let ports = midi.ports();
                        let port = ports
                            .get(i)
                            .ok_or_else(|| format!("MIDI 入力「{}」が見つかりません", names[i]))?;
                        let (tx, rx) = RingBuffer::<[u8; 3]>::new(RING);
                        let conn = midi
                            .connect(
                                port,
                                "xirius-in",
                                |_stamp, bytes: &[u8], tx: &mut rtrb::Producer<[u8; 3]>| {
                                    if let Some(msg) = channel_message(bytes) {
                                        let _ = tx.push(msg);
                                    }
                                },
                                tx,
                            )
                            .map_err(|e| format!("MIDI 入力「{}」を開けません: {e}", names[i]))?;
                        conns.push(conn);
                        rings.push((names[i].clone(), rx));
                    }
                    Ok((conns, rings))
                })();
                match opened {
                    Ok((conns, rings)) => {
                        let _ = ready_tx.send(Ok(rings));
                        while !flag.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        for c in conns {
                            c.close();
                        }
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| format!("MIDI 入力のスレッドを作成できません: {e}"))?;
        let rings = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "MIDI 入力デバイスが応答しません".to_string())??;
        let (names, rings) = rings.into_iter().unzip();
        Ok(MidirInput { rings, names, stop })
    }
}

/// A channel message of up to three bytes, padded with zeros; `None` for anything else.
fn channel_message(bytes: &[u8]) -> Option<[u8; 3]> {
    match bytes {
        [status, ..] if *status >= 0x80 && *status < 0xf0 && bytes.len() <= 3 => {
            let mut msg = [0; 3];
            msg[..bytes.len()].copy_from_slice(bytes);
            Some(msg)
        }
        _ => None,
    }
}

impl MidiInput for MidirInput {
    fn read(&mut self, frames: usize, f: &mut dyn FnMut(usize, [u8; 3])) {
        self.read_ports(frames, &mut |offset, _, msg| f(offset, msg));
    }

    fn read_ports(&mut self, _frames: usize, f: &mut dyn FnMut(usize, usize, [u8; 3])) {
        for (port, rx) in self.rings.iter_mut().enumerate() {
            while let Ok(msg) = rx.pop() {
                f(0, port, msg);
            }
        }
    }

    fn ports(&self) -> Vec<String> {
        self.names.clone()
    }
}

impl Drop for MidirInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_channel_messages_pass() {
        assert_eq!(channel_message(&[0x90, 60, 100]), Some([0x90, 60, 100]));
        assert_eq!(channel_message(&[0xc0, 5]), Some([0xc0, 5, 0]));
        assert_eq!(channel_message(&[0xf8]), None, "clock");
        assert_eq!(channel_message(&[0xf0, 1, 2, 3, 0xf7]), None, "SysEx");
        assert_eq!(channel_message(&[60, 100]), None, "running status");
        assert_eq!(channel_message(&[]), None);
    }

    #[test]
    fn a_port_is_found_by_name_or_part_of_it() {
        let ports = vec![
            "Keystation 49".to_string(),
            "Launchpad".to_string(),
            "Launchkey".to_string(),
        ];
        assert_eq!(find_port(None, &ports), Ok(None));
        assert_eq!(find_port(Some("all"), &ports), Ok(None));
        assert_eq!(find_port(Some("Launchpad"), &ports), Ok(Some(1)));
        assert_eq!(find_port(Some("launchkey"), &ports), Ok(Some(2)));
        assert_eq!(find_port(Some("station"), &ports), Ok(Some(0)));
        assert!(find_port(Some("launch"), &ports).is_err());
        assert!(find_port(Some("Pads"), &ports).is_err());
        assert_eq!(find_port(Some("Pads"), &[]), Ok(None));
    }
}
