//! Per-port filtering, shared by live input and recording.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use xirius_engine::record::MidiInput;
use xirius_prefs::Midimap;

/// A message of an assigned knob or pad that the recording did not take (OPS-007): the screen
/// applies it to the part or the command it is assigned to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taken {
    pub port: String,
    pub message: [u8; 3],
}

/// The assignments the input leaves out of a recording, with a counter that tells the input
/// when they changed.
#[derive(Default)]
struct Rules {
    map: Arc<Midimap>,
    rev: u64,
}

struct PortState {
    name: String,
    enabled: AtomicBool,
    received: AtomicBool,
}

#[derive(Clone)]
pub struct Control {
    ports: Arc<Vec<PortState>>,
    rules: Arc<Mutex<Rules>>,
    taken: Arc<Mutex<mpsc::Receiver<Taken>>>,
}

impl Control {
    /// The knobs and pads assigned to a part or a command: a recording leaves them out.
    pub fn assign(&self, map: &Midimap) {
        let mut rules = self.rules.lock().unwrap_or_else(|e| e.into_inner());
        if *rules.map != *map {
            rules.map = Arc::new(map.clone());
            rules.rev += 1;
        }
    }

    /// What the assigned knobs and pads sent since the last call, oldest first.
    pub fn take(&self) -> Vec<Taken> {
        let rx = self.taken.lock().unwrap_or_else(|e| e.into_inner());
        rx.try_iter().collect()
    }

    pub fn update(&self, prefs: &xirius_prefs::Prefs, mut received: impl FnMut(&str)) {
        for p in self.ports.iter() {
            p.enabled.store(
                prefs.bool(&format!("midi.inputs.{}", p.name)),
                Ordering::Relaxed,
            );
            if p.received.swap(false, Ordering::Relaxed) {
                received(&p.name);
            }
        }
    }
}

pub struct EnabledInput {
    input: Box<dyn MidiInput>,
    control: Control,
    held: Vec<[[bool; 128]; 16]>,
    was_enabled: Vec<bool>,
    ports: Vec<String>,
    /// The assignments as of the revision read last.
    rules: (u64, Arc<Midimap>),
    taken: mpsc::Sender<Taken>,
}

impl EnabledInput {
    pub fn new(input: Box<dyn MidiInput>, prefs: &xirius_prefs::Prefs) -> Self {
        let ports = input.ports();
        let states = ports
            .iter()
            .map(|p| PortState {
                name: p.clone(),
                enabled: AtomicBool::new(prefs.bool(&format!("midi.inputs.{p}"))),
                received: AtomicBool::new(false),
            })
            .collect();
        let (taken, rx) = mpsc::channel();
        Self {
            input,
            held: vec![[[false; 128]; 16]; ports.len()],
            was_enabled: vec![false; ports.len()],
            ports,
            rules: (0, Arc::default()),
            taken,
            control: Control {
                ports: Arc::new(states),
                rules: Arc::default(),
                taken: Arc::new(Mutex::new(rx)),
            },
        }
    }

    /// An input that leaves the assigned knobs and pads out of what it gives, and hands them
    /// to [`Control::take`].
    pub fn with_assignments(self, map: &Midimap) -> Self {
        self.control.assign(map);
        self
    }
    pub fn control(&self) -> Control {
        self.control.clone()
    }
}

impl MidiInput for EnabledInput {
    fn ports(&self) -> Vec<String> {
        self.ports.clone()
    }
    fn read(&mut self, frames: usize, f: &mut dyn FnMut(usize, [u8; 3])) {
        self.read_ports(frames, &mut |at, _, msg| f(at, msg));
    }
    fn read_ports(&mut self, frames: usize, f: &mut dyn FnMut(usize, usize, [u8; 3])) {
        let states = &self.control.ports;
        for (port, state) in states.iter().enumerate() {
            let enabled = state.enabled.load(Ordering::Relaxed);
            if self.was_enabled[port] && !enabled {
                for (channel, pitches) in self.held[port].iter_mut().enumerate() {
                    for (pitch, held) in pitches.iter_mut().enumerate() {
                        if std::mem::take(held) {
                            f(0, port, [0x80 | channel as u8, pitch as u8, 0]);
                        }
                    }
                    f(0, port, [0xb0 | channel as u8, 64, 0]);
                }
            }
            self.was_enabled[port] = enabled;
        }
        // The audio thread never waits for the screen: a lock in use leaves the last rules.
        if let Ok(rules) = self.control.rules.try_lock()
            && rules.rev != self.rules.0
        {
            self.rules = (rules.rev, rules.map.clone());
        }
        let (map, taken, names) = (&self.rules.1, &self.taken, &self.ports);
        let held = &mut self.held;
        self.input.read_ports(frames, &mut |at, port, msg| {
            if states
                .get(port)
                .map_or(states.is_empty(), |p| p.enabled.load(Ordering::Relaxed))
            {
                if let Some(p) = states.get(port) {
                    p.received.store(true, Ordering::Relaxed);
                }
                let name = names.get(port).map_or("", String::as_str);
                if map.find(name, msg).is_some() {
                    let _ = taken.send(Taken {
                        port: name.to_string(),
                        message: msg,
                    });
                    return;
                }
                if msg[1] < 128
                    && let Some(notes) = held.get_mut(port)
                {
                    match msg[0] & 0xf0 {
                        0x90 => notes[(msg[0] & 15) as usize][msg[1] as usize] = msg[2] != 0,
                        0x80 => notes[(msg[0] & 15) as usize][msg[1] as usize] = false,
                        _ => {}
                    }
                }
                f(at, port, msg);
            }
        });
    }
}

/// Positive offsets move a recorded take later; negative offsets move it earlier.
pub fn shift_frame(frame: u64, ms: f64, rate: f64) -> u64 {
    frame.saturating_add_signed((ms * rate / 1000.0).round() as i64)
}

/// Apply earlier placement while capturing, including a take that begins at frame zero.
/// Only a positive remainder needs to move the finished clip.
pub fn compensate(latency: u64, ms: f64, rate: f64) -> (u64, f64) {
    let frames = (ms * rate / 1000.0).round() as i64;
    if frames < 0 {
        (latency.saturating_add(frames.unsigned_abs()), 0.0)
    } else {
        (
            latency.saturating_sub(frames as u64),
            (frames as u64).saturating_sub(latency) as f64 * 1000.0 / rate,
        )
    }
}
