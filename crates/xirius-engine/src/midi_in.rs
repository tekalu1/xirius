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
    done: Arc<AtomicBool>,
}

impl MidirInput {
    /// Connects to the port `name` names ([`find_port`]), or to every port when `None` or
    /// `all`. An error when there is no port to connect to.
    pub fn open(name: Option<&str>) -> Result<MidirInput, String> {
        let wanted = name.map(str::to_string);
        open_owned(
            move || {
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
            },
            |conns| {
                for c in conns {
                    c.close();
                }
            },
            |rx| {
                rx.recv_timeout(Duration::from_secs(5))
                    .map_err(|e| e.to_string())?
            },
        )
    }
}

type Rings = Vec<(String, Consumer<[u8; 3]>)>;
type Ready = Result<Rings, String>;

// Failed readiness attempts remain serialized with later connections. Waiting
// here is worker-only; no driver is forcibly terminated.
static ABANDONED: std::sync::Mutex<Vec<Arc<AtomicBool>>> = std::sync::Mutex::new(Vec::new());

pub(crate) struct Finished(pub(crate) Arc<AtomicBool>);
impl Drop for Finished {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub(crate) fn wait_closed(done: &AtomicBool) {
    while !done.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn open_owned<C: 'static>(
    connect: impl FnOnce() -> Result<(C, Rings), String> + Send + 'static,
    close: impl FnOnce(C) + Send + 'static,
    wait: impl FnOnce(&std::sync::mpsc::Receiver<Ready>) -> Ready,
) -> Result<MidirInput, String> {
    // Keep pending entries visible to every concurrent retry, rather than
    // taking the list and allowing a third opener to overtake its waiter.
    let pending = ABANDONED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for done in pending {
        wait_closed(&done);
    }
    ABANDONED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|done| !done.load(Ordering::Acquire));
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let flag = stop.clone();
    let finished = Finished(done.clone());
    std::thread::Builder::new()
        .name("xirius-midi-in".into())
        .spawn(move || {
            let _finished = finished;
            match connect() {
                Ok((connections, rings)) => {
                    if tx.send(Ok(rings)).is_ok() {
                        while !flag.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                    }
                    close(connections);
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    match wait(&rx) {
        Ok(rings) => {
            let (names, rings) = rings.into_iter().unzip();
            Ok(MidirInput {
                rings,
                names,
                stop,
                done,
            })
        }
        Err(error) => {
            stop.store(true, Ordering::Release);
            ABANDONED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(done);
            Err(error)
        }
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
    fn closed(&self) -> Option<Arc<AtomicBool>> {
        Some(self.done.clone())
    }
    fn close(self: Box<Self>) {
        let done = self.done.clone();
        drop(self);
        wait_closed(&done);
    }

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
    fn abandoned_readiness_closes_late_connection_before_a_retry_can_connect() {
        use std::sync::mpsc;
        let (entered_tx, entered) = mpsc::channel();
        let (release, connect) = mpsc::channel();
        let (closing_tx, closing) = mpsc::channel();
        let (finish, close) = mpsc::channel();
        let failed = open_owned(
            move || {
                entered_tx.send(()).unwrap();
                connect.recv_timeout(Duration::from_secs(20)).unwrap();
                // The resource is created on its owner and need not be Send.
                Ok((std::rc::Rc::new(()), Vec::new()))
            },
            move |resource| {
                closing_tx.send(()).unwrap();
                close.recv_timeout(Duration::from_secs(20)).unwrap();
                drop(resource);
            },
            |_| {
                entered.recv_timeout(Duration::from_secs(20)).unwrap();
                Err("readiness abandoned".into())
            },
        );
        assert!(failed.is_err());
        release.send(()).unwrap();
        closing.recv_timeout(Duration::from_secs(20)).unwrap();
        let (retry_tx, retry) = mpsc::channel();
        let pending = ABANDONED.lock().unwrap().clone();
        assert!(pending.iter().any(|done| !done.load(Ordering::Acquire)));
        let work = std::thread::spawn(move || {
            open_owned(
                move || {
                    retry_tx.send(()).unwrap();
                    Ok(((), Vec::new()))
                },
                |_| {},
                |rx| rx.recv_timeout(Duration::from_secs(20)).unwrap(),
            )
        });
        assert!(retry.try_recv().is_err());
        finish.send(()).unwrap();
        let input = work.join().unwrap().unwrap();
        retry.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(pending.iter().all(|done| done.load(Ordering::Acquire)));
        let done = input.closed().unwrap();
        Box::new(input).close();
        assert!(done.load(Ordering::Acquire));
    }

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
