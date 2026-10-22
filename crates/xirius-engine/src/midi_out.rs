//! MIDI output ports (ENG-028), through midir.
//!
//! A track whose output is a MIDI port plays its notes and controllers to that port instead of to
//! an instrument. The audio thread turns the events of the block into messages and hands them to
//! a [`MidiOutput`], which must not allocate, lock or wait. [`MidirOutput`] holds the ports open
//! on a thread of its own: the audio thread pushes each message with the time it is due into a
//! lock-free ring, and that thread sends it then. [`MockMidiOut`] takes the place of a device in
//! tests and opens nothing.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::processor::{CTRL_AFTERTOUCH, CTRL_PITCH_BEND, NoteKind};

/// Messages the ring to the port thread (and the mock's log) holds.
const RING: usize = 4096;

/// The client name the ports see.
const CLIENT: &str = "Xirius";

/// The names of the MIDI output ports on this PC, in midir's order.
pub fn midi_out_ports() -> Result<Vec<String>, String> {
    let midi =
        midir::MidiOutput::new(CLIENT).map_err(|e| format!("MIDI 出力を初期化できません: {e}"))?;
    Ok(midi
        .ports()
        .iter()
        .map(|p| midi.port_name(p).unwrap_or_else(|_| "(名前なし)".into()))
        .collect())
}

/// Where a track's notes go when its output is a MIDI port.
pub trait MidiOutput: Send {
    /// The ports this output holds open, in order.
    fn ports(&self) -> Vec<String>;

    /// The index in [`MidiOutput::ports`] of the port `name` names. Does not allocate.
    fn port_index(&self, name: &str) -> Option<usize>;

    /// Sends `msg` to `port`, `offset` frames after the start of the block being rendered.
    /// Called on the audio thread; must not allocate, lock or wait.
    fn send(&mut self, port: usize, offset: usize, msg: [u8; 3]);
}

/// The channel message a note or controller event is, on `channel` (0..=15). `None` for a
/// controller number MIDI has no message for.
pub fn midi_message(kind: NoteKind, channel: u8) -> Option<[u8; 3]> {
    let ch = channel & 0x0f;
    match kind {
        NoteKind::On { pitch, velocity } => Some([
            0x90 | ch,
            pitch & 0x7f,
            ((velocity * 127.0).round() as i32).clamp(1, 127) as u8,
        ]),
        NoteKind::Off { pitch } => Some([0x80 | ch, pitch & 0x7f, 0]),
        NoteKind::Control { number, value } => match number {
            CTRL_AFTERTOUCH => Some([0xd0 | ch, value.clamp(0, 127) as u8, 0]),
            CTRL_PITCH_BEND => {
                let v = (value + 8192).clamp(0, 16383) as u16;
                Some([0xe0 | ch, (v & 0x7f) as u8, (v >> 7) as u8])
            }
            n if n < 128 => Some([0xb0 | ch, n, value.clamp(0, 127) as u8]),
            _ => None,
        },
    }
}

/// How many bytes of a three-byte message a status byte uses.
pub fn message_len(status: u8) -> usize {
    match status & 0xf0 {
        0xc0 | 0xd0 => 2,
        _ => 3,
    }
}

// ---- midir ------------------------------------------------------------------------------------

struct Timed {
    port: usize,
    at: Instant,
    msg: [u8; 3],
}

/// The MIDI output ports a song names, held open for as long as this lives.
pub struct MidirOutput {
    names: Vec<String>,
    ring: Producer<Timed>,
    stop: Arc<AtomicBool>,
    rate: f64,
    /// A full ring: messages that were not sent.
    dropped: u64,
}

/// The port `name` names among `ports`: the exact name, else the same name in other case.
fn find_out(name: &str, ports: &[String]) -> Option<usize> {
    let name = name.trim();
    ports
        .iter()
        .position(|p| p == name)
        .or_else(|| ports.iter().position(|p| p.eq_ignore_ascii_case(name)))
}

impl MidirOutput {
    /// Opens the ports `wanted` names, for blocks of a song at `rate`. A name no port has is
    /// left out of [`MidiOutput::ports`]; the caller says so.
    pub fn open(wanted: &[String], rate: f64) -> Result<MidirOutput, String> {
        let wanted = wanted.to_vec();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = RingBuffer::<Timed>::new(RING);
        type Ready = Result<Vec<String>, String>;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Ready>();
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("xirius-midi-out".into())
            .spawn(move || {
                let opened = (|| {
                    let all = midi_out_ports()?;
                    let mut conns = Vec::new();
                    let mut names = Vec::new();
                    for name in &wanted {
                        let Some(i) = find_out(name, &all) else {
                            continue;
                        };
                        if names.contains(&all[i]) {
                            continue;
                        }
                        let midi = midir::MidiOutput::new(CLIENT)
                            .map_err(|e| format!("MIDI 出力を初期化できません: {e}"))?;
                        let ports = midi.ports();
                        let port = ports
                            .get(i)
                            .ok_or_else(|| format!("MIDI 出力「{}」が見つかりません", all[i]))?;
                        let conn = midi
                            .connect(port, "xirius-out")
                            .map_err(|e| format!("MIDI 出力「{}」を開けません: {e}", all[i]))?;
                        conns.push(conn);
                        names.push(all[i].clone());
                    }
                    Ok((conns, names))
                })();
                match opened {
                    Ok((conns, names)) => {
                        let _ = ready_tx.send(Ok(names));
                        port_thread(conns, rx, &flag);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| format!("MIDI 出力のスレッドを作成できません: {e}"))?;
        let names = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "MIDI 出力デバイスが応答しません".to_string())??;
        Ok(MidirOutput {
            names,
            ring: tx,
            stop,
            rate,
            dropped: 0,
        })
    }

    /// Messages the port thread had no room for.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Sends what is due, from the ring, until told to stop; then lets every note go.
fn port_thread(
    mut conns: Vec<midir::MidiOutputConnection>,
    mut rx: Consumer<Timed>,
    stop: &AtomicBool,
) {
    let _timer = Resolution::begin();
    let mut pending: VecDeque<Timed> = VecDeque::new();
    loop {
        while let Ok(m) = rx.pop() {
            pending.push_back(m);
        }
        let now = Instant::now();
        while pending.front().is_some_and(|m| m.at <= now) {
            let m = pending.pop_front().expect("a front");
            if let Some(c) = conns.get_mut(m.port) {
                let _ = c.send(&m.msg[..message_len(m.msg[0])]);
            }
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // Nothing is left sounding on the other end.
    for c in &mut conns {
        for ch in 0..16u8 {
            let _ = c.send(&[0xb0 | ch, 123, 0]);
            let _ = c.send(&[0xb0 | ch, 64, 0]);
        }
    }
    for c in conns {
        c.close();
    }
}

/// Asks Windows for 1 ms timer ticks while alive, so `sleep(1 ms)` is about 1 ms.
struct Resolution;

impl Resolution {
    fn begin() -> Resolution {
        #[cfg(windows)]
        // SAFETY: timeBeginPeriod only takes a period; it is paired with timeEndPeriod on drop.
        unsafe {
            windows_sys::Win32::Media::timeBeginPeriod(1);
        }
        Resolution
    }
}

impl Drop for Resolution {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: pairs the timeBeginPeriod of `begin`.
        unsafe {
            windows_sys::Win32::Media::timeEndPeriod(1);
        }
    }
}

impl MidiOutput for MidirOutput {
    fn ports(&self) -> Vec<String> {
        self.names.clone()
    }

    fn port_index(&self, name: &str) -> Option<usize> {
        let name = name.trim();
        self.names
            .iter()
            .position(|p| p == name)
            .or_else(|| self.names.iter().position(|p| p.eq_ignore_ascii_case(name)))
    }

    fn send(&mut self, port: usize, offset: usize, msg: [u8; 3]) {
        let at = Instant::now() + Duration::from_secs_f64(offset as f64 / self.rate);
        if self.ring.push(Timed { port, at, msg }).is_err() {
            self.dropped += 1;
        }
    }
}

impl Drop for MidirOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

// ---- stand-in ---------------------------------------------------------------------------------

/// A message a [`MockMidiOut`] took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sent {
    pub port: usize,
    pub offset: usize,
    pub msg: [u8; 3],
}

/// A MIDI output that opens no device: it keeps what it was sent, for [`MockLog::take`].
pub struct MockMidiOut {
    names: Vec<String>,
    ring: Producer<Sent>,
}

/// The messages a [`MockMidiOut`] took, read on the control side.
pub struct MockLog {
    ring: Consumer<Sent>,
}

impl MockMidiOut {
    /// A stand-in holding ports of these names.
    pub fn new(ports: &[&str]) -> (MockMidiOut, MockLog) {
        let (tx, rx) = RingBuffer::new(RING);
        (
            MockMidiOut {
                names: ports.iter().map(|s| s.to_string()).collect(),
                ring: tx,
            },
            MockLog { ring: rx },
        )
    }
}

impl MockLog {
    /// What was sent since the last call, oldest first.
    pub fn take(&mut self) -> Vec<Sent> {
        let mut out = Vec::new();
        while let Ok(m) = self.ring.pop() {
            out.push(m);
        }
        out
    }
}

impl MidiOutput for MockMidiOut {
    fn ports(&self) -> Vec<String> {
        self.names.clone()
    }

    fn port_index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|p| p == name.trim())
    }

    fn send(&mut self, port: usize, offset: usize, msg: [u8; 3]) {
        let _ = self.ring.push(Sent { port, offset, msg });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_and_controllers_become_channel_messages() {
        let on = NoteKind::On {
            pitch: 60,
            velocity: 1.0,
        };
        assert_eq!(midi_message(on, 0), Some([0x90, 60, 127]));
        assert_eq!(midi_message(on, 3), Some([0x93, 60, 127]));
        let soft = NoteKind::On {
            pitch: 61,
            velocity: 0.0,
        };
        assert_eq!(
            midi_message(soft, 0),
            Some([0x90, 61, 1]),
            "velocity 0 would be a note-off"
        );
        assert_eq!(
            midi_message(NoteKind::Off { pitch: 60 }, 0),
            Some([0x80, 60, 0])
        );
        assert_eq!(
            midi_message(NoteKind::control(64, 127), 0),
            Some([0xb0, 64, 127])
        );
        assert_eq!(
            midi_message(NoteKind::control(CTRL_AFTERTOUCH, 50), 0),
            Some([0xd0, 50, 0])
        );
        assert_eq!(
            midi_message(NoteKind::control(CTRL_PITCH_BEND, 0), 0),
            Some([0xe0, 0, 64]),
            "centre is 8192"
        );
        assert_eq!(
            midi_message(NoteKind::control(CTRL_PITCH_BEND, -8192), 0),
            Some([0xe0, 0, 0])
        );
        assert_eq!(
            midi_message(NoteKind::control(CTRL_PITCH_BEND, 8191), 0),
            Some([0xe0, 127, 127])
        );
        assert_eq!(message_len(0xd0), 2);
        assert_eq!(message_len(0xe3), 3);
    }

    #[test]
    fn a_note_on_comes_back_through_note_of() {
        let on = NoteKind::On {
            pitch: 72,
            velocity: 100.0 / 127.0,
        };
        let msg = midi_message(on, 0).unwrap();
        assert_eq!(crate::record::note_of(msg), Some(on));
    }

    #[test]
    fn the_mock_keeps_what_it_is_sent_in_order() {
        let (mut out, mut log) = MockMidiOut::new(&["A", "B"]);
        assert_eq!(out.port_index("B"), Some(1));
        assert_eq!(out.port_index("C"), None);
        out.send(1, 5, [0x90, 60, 100]);
        out.send(0, 0, [0x80, 60, 0]);
        let got = log.take();
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            Sent {
                port: 1,
                offset: 5,
                msg: [0x90, 60, 100]
            }
        );
        assert!(log.take().is_empty());
    }

    #[test]
    fn a_port_is_found_by_name_in_any_case() {
        let ports = vec!["Loop MIDI".to_string(), "Synth".to_string()];
        assert_eq!(find_out("synth", &ports), Some(1));
        assert_eq!(find_out(" Loop MIDI ", &ports), Some(0));
        assert_eq!(find_out("Other", &ports), None);
    }
}
