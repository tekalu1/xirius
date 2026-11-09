//! Live input ownership at the instrument, after Multi's key and velocity routing.
//! Storage is bounded and allocated before the audio callback starts.

use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Audio-to-UI snapshot of physically held external MIDI keys, excluding pedal tails.
#[derive(Default)]
pub struct Pressed([AtomicU64; 2]);

impl Pressed {
    pub fn publish(&self, notes: impl Iterator<Item = u8>) {
        let mut bits = [0_u64; 2];
        for pitch in notes.filter(|p| *p < 128) {
            bits[usize::from(pitch / 64)] |= 1_u64 << (pitch % 64);
        }
        for (cell, value) in self.0.iter().zip(bits) {
            cell.store(value, Ordering::Relaxed);
        }
    }
    pub fn notes(&self) -> Vec<u8> {
        let bits = self.0.each_ref().map(|n| n.load(Ordering::Relaxed));
        (0_u8..128)
            .filter(|p| bits[usize::from(p / 64)] & (1_u64 << (p % 64)) != 0)
            .collect()
    }
}

/// Zero belongs to the timeline. MIDI and window sources use separate namespaces.
pub const MOUSE: u64 = 1 << 63;
pub fn midi_source(port: usize, channel: u8) -> u64 {
    1 + ((port as u64) << 4) + u64::from(channel & 15)
}

/// Performance controllers pass to monitoring without changing the recorded note format.
pub fn message(msg: [u8; 3]) -> Option<NoteKind> {
    crate::record::note_of(msg).or_else(|| {
        (msg[0] & 0xf0 == 0xb0).then(|| NoteKind::control(msg[1] & 127, i32::from(msg[2] & 127)))
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub source: u64,
    /// None enters the track/Multi; Some auditions that particular instrument.
    pub plugin: Option<u64>,
    pub kind: NoteKind,
}

const CAPACITY: usize = 2048;

#[derive(Clone, Copy)]
struct Held {
    source: u64,
    pitch: u8,
    presses: u32,
    sustained: bool,
}

/// A sounding instrument owns notes independently for each input source.
pub struct Instrument {
    pub pressed: Arc<Pressed>,
    id: u64,
    inner: Box<dyn Processor>,
    held: Vec<Held>,
    pedals: Vec<(u64, i32)>,
    native_pedal: bool,
    pending: Vec<NoteEvent>,
}

impl Instrument {
    pub fn new(id: u64, inner: Box<dyn Processor>) -> Self {
        Self {
            pressed: Arc::default(),
            id,
            inner,
            held: Vec::with_capacity(CAPACITY),
            pedals: Vec::with_capacity(CAPACITY),
            native_pedal: false,
            pending: Vec::with_capacity(CAPACITY),
        }
    }

    /// External instruments retain their native continuous damper response on
    /// the existing shared output channel. Built-ins release per input owner.
    pub fn with_native_pedal(mut self) -> Self {
        self.native_pedal = true;
        self
    }

    fn pedal(&mut self, source: u64, value: i32, offset: u32) {
        let value = value.clamp(0, 127);
        if value == 0 {
            self.pedals.retain(|(s, _)| *s != source);
        } else if let Some((_, old)) = self.pedals.iter_mut().find(|(s, _)| *s == source) {
            *old = value;
        } else if self.pedals.len() < self.pedals.capacity() {
            self.pedals.push((source, value));
        }
        if self.native_pedal {
            let value = self.pedals.iter().map(|(_, v)| *v).max().unwrap_or(0);
            self.emit(NoteEvent {
                offset,
                kind: NoteKind::control(64, value),
            });
        }
    }

    fn emit(&mut self, event: NoteEvent) {
        if self.pending.len() < self.pending.capacity() {
            self.pending.push(event);
        } else {
            // A bounded queue must never discard the last release and leave a voice stuck.
            // Overflow is a panic boundary, not a partially delivered performance.
            self.held.clear();
            self.pedals.clear();
            self.pending.clear();
            if self.native_pedal {
                self.pending.push(NoteEvent {
                    offset: event.offset,
                    kind: NoteKind::control(64, 0),
                });
            }
            self.pending.push(NoteEvent {
                offset: event.offset,
                kind: NoteKind::control(120, 0),
            });
            for pitch in 0..128 {
                self.pending.push(NoteEvent {
                    offset: event.offset,
                    kind: NoteKind::Off { pitch },
                });
            }
        }
    }

    fn release(&mut self, index: usize, offset: u32) {
        let held = self.held.swap_remove(index);
        if !self.held.iter().any(|h| h.pitch == held.pitch) {
            self.emit(NoteEvent {
                offset,
                kind: NoteKind::Off { pitch: held.pitch },
            });
        }
    }

    fn take(&mut self, source: u64, event: NoteEvent) {
        match event.kind {
            NoteKind::On { pitch, .. } => {
                if let Some(h) = self
                    .held
                    .iter_mut()
                    .find(|h| h.source == source && h.pitch == pitch)
                {
                    // Legacy timeline events identify a pitch, not individual
                    // presses. Preserve its Off contract while counting live owners.
                    h.presses = if source == 0 {
                        1
                    } else {
                        h.presses.saturating_add(1)
                    };
                    h.sustained = false;
                    // A note-on is an articulation, even after a pedal-held release or
                    // over another press. Ownership postpones off, never a new attack.
                    self.emit(event);
                    return;
                }
                if self.held.len() == self.held.capacity()
                    || self.pending.len() == self.pending.capacity()
                {
                    return;
                }
                self.held.push(Held {
                    source,
                    pitch,
                    presses: 1,
                    sustained: false,
                });
                self.emit(event);
            }
            NoteKind::Off { pitch } => {
                let Some(i) = self
                    .held
                    .iter()
                    .position(|h| h.source == source && h.pitch == pitch)
                else {
                    if source == 0 && !self.held.iter().any(|h| h.pitch == pitch) {
                        self.emit(event);
                    }
                    return;
                };
                let h = &mut self.held[i];
                h.presses = h.presses.saturating_sub(1);
                if h.presses != 0 {
                    return;
                }
                h.sustained =
                    !self.native_pedal && self.pedals.iter().any(|(s, v)| *s == source && *v >= 64);
                if !h.sustained {
                    self.release(i, event.offset);
                }
            }
            NoteKind::Control { number: 64, value } => {
                self.pedal(source, value, event.offset);
                if !self.native_pedal && value < 64 {
                    for i in (0..self.held.len()).rev() {
                        if i >= self.held.len() {
                            continue;
                        }
                        if self.held[i].source == source && self.held[i].presses == 0 {
                            self.release(i, event.offset);
                        }
                    }
                }
            }
            NoteKind::Control {
                number: 120 | 123, ..
            } => {
                self.pedal(source, 0, event.offset);
                for i in (0..self.held.len()).rev() {
                    if i >= self.held.len() {
                        continue;
                    }
                    if self.held[i].source == source {
                        self.release(i, event.offset);
                    }
                }
            }
            _ => self.emit(event),
        }
    }
}

impl Processor for Instrument {
    fn release_live_inputs(&mut self) {
        self.pedals.clear();
        if self.native_pedal {
            self.emit(NoteEvent {
                offset: 0,
                kind: NoteKind::control(64, 0),
            });
        }
        while !self.held.is_empty() {
            self.release(self.held.len() - 1, 0);
        }
        self.pressed.publish(std::iter::empty());
    }
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn prepare(&mut self, setup: &ProcessSetup) {
        self.inner.prepare(setup);
    }
    fn begin_prepare(&mut self, setup: &ProcessSetup) {
        self.inner.begin_prepare(setup);
    }
    fn finish_prepare(&mut self, setup: &ProcessSetup) {
        self.inner.finish_prepare(setup);
    }
    fn preparing(&self) -> bool {
        self.inner.preparing()
    }
    fn allow_late(&mut self) {
        self.inner.allow_late();
    }
    fn latency(&self) -> u32 {
        self.inner.latency()
    }
    fn child_time(&self) -> Option<(u64, u64)> {
        self.inner.child_time()
    }
    fn live_input(&mut self, input: Input) -> bool {
        if input.plugin.is_some_and(|id| id != self.id) {
            return false;
        }
        self.take(
            input.source,
            NoteEvent {
                offset: 0,
                kind: input.kind,
            },
        );
        true
    }
    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        for event in events {
            self.take(0, *event);
        }
        self.inner.process(io, &self.pending);
        self.pending.clear();
        self.pressed.publish(
            self.held
                .iter()
                .filter(|h| h.source != 0 && h.source & MOUSE == 0 && h.presses > 0)
                .map(|h| h.pitch),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::RenderMode;
    use std::sync::Arc;

    fn sampler(id: u64) -> Instrument {
        let wave: Vec<_> = (0..96000)
            .map(|i| ((i as f32 * 0.0576).sin()) * 0.3)
            .collect();
        sampler_with_wave(id, wave)
    }

    fn sampler_with_wave(id: u64, wave: Vec<f32>) -> Instrument {
        let (inner, handle) = crate::stock::create("sampler").unwrap();
        assert!(handle.set("attack", 0.0));
        assert!(handle.set("release", 0.0));
        assert!(handle.set("sustain", 1.0));
        assert!(
            handle.set_files(vec![Some(crate::stock::files::FileSlot::new(
                "C3.wav",
                Arc::new(crate::timeline::AudioData {
                    left: wave.clone(),
                    right: wave
                }),
            ))])
        );
        let mut instrument = Instrument::new(id, inner);
        instrument.prepare(&ProcessSetup {
            sample_rate: 48000.0,
            max_block: 256,
            mode: RenderMode::Offline,
        });
        instrument
    }

    fn attack_level(p: &mut dyn Processor) -> f32 {
        let mut left = [0.0; 256];
        let mut right = [0.0; 256];
        p.process(&mut AudioIo::new(&mut left, &mut right), &[]);
        left.iter().map(|x| x * x).sum::<f32>() / left.len() as f32
    }

    #[test]
    fn pedal_repress_and_overlapping_timeline_notes_restart_the_sample_at_the_new_velocity() {
        for source in [midi_source(0, 0), 0] {
            // A short attack followed by silence: changing the old voice's gain cannot
            // create a second attack. The sample must actually start again.
            let wave = (0..96000)
                .map(|i| {
                    if i < 512 {
                        (i as f32 * 0.0576).sin() * 0.3
                    } else {
                        0.0
                    }
                })
                .collect();
            let mut p = sampler_with_wave(7, wave);
            p.take(
                source,
                NoteEvent {
                    offset: 0,
                    kind: NoteKind::control(64, 127),
                },
            );
            p.take(
                source,
                NoteEvent {
                    offset: 0,
                    kind: NoteKind::On {
                        pitch: 60,
                        velocity: 0.2,
                    },
                },
            );
            let quiet_attack = attack_level(&mut p);
            assert!(quiet_attack > 1e-7);
            assert_eq!(level(&mut p), 0.0);
            if source != 0 {
                p.take(
                    source,
                    NoteEvent {
                        offset: 0,
                        kind: off(),
                    },
                );
                assert_eq!(p.held[0].presses, 0);
                assert!(p.held[0].sustained);
            }
            p.take(
                source,
                NoteEvent {
                    offset: 0,
                    kind: NoteKind::On {
                        pitch: 60,
                        velocity: 0.8,
                    },
                },
            );
            let loud_attack = attack_level(&mut p);
            assert!(
                loud_attack > quiet_attack * 8.0,
                "source {source}: {quiet_attack} -> {loud_attack}"
            );
            p.take(
                source,
                NoteEvent {
                    offset: 0,
                    kind: off(),
                },
            );
            assert_eq!(
                p.held[0].presses, 0,
                "one legacy Off releases the timeline pitch"
            );
            p.take(
                source,
                NoteEvent {
                    offset: 0,
                    kind: NoteKind::control(64, 0),
                },
            );
            assert!(level(&mut p) < 1e-10);
        }
    }

    fn input(p: &mut dyn Processor, source: u64, plugin: Option<u64>, kind: NoteKind) {
        assert!(p.live_input(Input {
            source,
            plugin,
            kind
        }));
    }
    fn on() -> NoteKind {
        NoteKind::On {
            pitch: 60,
            velocity: 0.8,
        }
    }
    fn off() -> NoteKind {
        NoteKind::Off { pitch: 60 }
    }
    fn level(p: &mut dyn Processor) -> f32 {
        let mut left = [0.0; 256];
        let mut right = [0.0; 256];
        for _ in 0..32 {
            p.process(&mut AudioIo::new(&mut left, &mut right), &[]);
        }
        left.iter().map(|x| x * x).sum::<f32>() / left.len() as f32
    }

    #[test]
    fn mouse_and_two_midi_channels_release_only_the_last_owner_in_pcm() {
        let mut p = sampler(7);
        let a = midi_source(0, 0);
        let b = midi_source(0, 1);
        input(&mut p, MOUSE | 7, Some(7), on());
        input(&mut p, a, None, on());
        input(&mut p, b, None, on());
        assert!(level(&mut p) > 0.00001);
        input(&mut p, MOUSE | 7, Some(7), off());
        input(&mut p, a, None, off());
        assert!(level(&mut p) > 0.00001);
        input(&mut p, b, None, off());
        assert!(level(&mut p) < 1e-10);
    }

    #[test]
    fn multi_input_capacity_panics_then_accepts_a_tracked_attack() {
        use crate::multi::{LayerSpec, MultiProc};
        let mut p = MultiProc::new(1, vec![LayerSpec::new(7, Box::new(sampler(7)), Vec::new())]);
        p.prepare(&crate::processor::ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 256,
            mode: crate::processor::RenderMode::Offline,
        });
        let a = midi_source(0, 0);
        let b = midi_source(1, 0);
        for _ in 0..1024 {
            input(&mut p, a, None, on());
        }
        input(&mut p, b, None, on());
        assert!(level(&mut p) > 0.00001);
        // The old history was explicitly released at capacity. Its later offs
        // cannot consume the new owner's tracked press.
        input(&mut p, a, None, off());
        assert!(level(&mut p) > 0.00001);
        input(&mut p, b, None, off());
        assert!(level(&mut p) < 1e-10);
    }

    #[test]
    fn sustain_and_disconnect_belong_to_their_port_and_channel() {
        let mut p = sampler(7);
        let a = midi_source(0, 0);
        let b = midi_source(1, 0);
        for source in [a, b] {
            input(&mut p, source, None, on());
            input(&mut p, source, None, NoteKind::control(64, 127));
            input(&mut p, source, None, off());
        }
        assert!(p.held.iter().all(|h| h.presses == 0 && h.sustained));
        input(&mut p, a, None, NoteKind::control(64, 0));
        assert!(level(&mut p) > 0.00001);
        input(&mut p, b, None, NoteKind::control(123, 0));
        assert!(level(&mut p) < 1e-10);
    }

    #[test]
    fn duplicate_on_and_wrong_target_do_not_cut_or_start_another_instrument() {
        let mut p = sampler(7);
        assert!(!p.live_input(Input {
            source: MOUSE | 8,
            plugin: Some(8),
            kind: on()
        }));
        assert_eq!(level(&mut p), 0.0);
        let source = midi_source(2, 3);
        input(&mut p, source, None, on());
        input(&mut p, source, None, on());
        input(&mut p, source, None, off());
        assert!(level(&mut p) > 0.00001);
        input(&mut p, source, None, off());
        assert!(level(&mut p) < 1e-10);
    }

    #[test]
    fn multi_routes_before_combining_mouse_and_midi_and_reports_physical_keys() {
        use crate::multi::{LayerSpec, MultiProc};
        let instrument = sampler(7);
        let pressed = instrument.pressed.clone();
        let mut layer = LayerSpec::new(17, Box::new(instrument), Vec::new());
        layer.keys = (48, 59);
        layer.velocity = (80, 127);
        layer.transpose = 12;
        let mut p = MultiProc::new(1, vec![layer]);
        p.prepare(&ProcessSetup {
            sample_rate: 48000.0,
            max_block: 256,
            mode: RenderMode::Offline,
        });
        let source = midi_source(1, 2);
        input(
            &mut p,
            source,
            None,
            NoteKind::On {
                pitch: 48,
                velocity: 0.2,
            },
        );
        assert_eq!(
            level(&mut p),
            0.0,
            "velocity gate must remain in the audio route"
        );
        input(&mut p, source, None, NoteKind::Off { pitch: 48 });
        input(
            &mut p,
            source,
            None,
            NoteKind::On {
                pitch: 48,
                velocity: 0.8,
            },
        );
        input(&mut p, MOUSE | 7, Some(7), on());
        assert!(level(&mut p) > 0.00001);
        assert_eq!(pressed.notes(), vec![60]);
        assert_eq!(p.pressed.notes(), vec![48]);
        input(&mut p, MOUSE | 7, Some(7), off());
        assert!(level(&mut p) > 0.00001);
        input(&mut p, source, None, NoteKind::control(64, 127));
        input(&mut p, source, None, NoteKind::Off { pitch: 48 });
        assert!(level(&mut p) > 0.00001);
        assert!(pressed.notes().is_empty());
        assert!(p.pressed.notes().is_empty());
        input(&mut p, source, None, NoteKind::control(64, 0));
        assert!(level(&mut p) < 1e-10);
    }

    #[test]
    fn an_overfull_control_queue_cannot_drop_the_last_release() {
        let mut p = sampler(7);
        let source = midi_source(0, 0);
        input(&mut p, source, None, on());
        assert!(level(&mut p) > 0.00001);
        for _ in 0..CAPACITY {
            input(&mut p, source, None, NoteKind::control(1, 0));
        }
        input(&mut p, source, None, off());
        assert!(level(&mut p) < 1e-10);
        assert!(p.pressed.notes().is_empty());
    }

    #[test]
    fn recording_port_disconnect_releases_only_its_owner_and_ignores_queued_messages() {
        use crate::record::{MockMidi, RecordSpec, Recorder, Thru};
        use crate::{Command, MixSpec, Plan, TrackSlot, TrackSpec};
        let instrument = sampler(7);
        let pressed = instrument.pressed.clone();
        let plan = Plan::build(
            MixSpec {
                tracks: vec![TrackSlot::New(TrackSpec::new("Keys", Box::new(instrument)))],
                ..Default::default()
            },
            ProcessSetup {
                sample_rate: 48_000.0,
                max_block: 256,
                mode: RenderMode::Offline,
            },
            None,
        )
        .unwrap();
        let (mut audio, mut control) = crate::engine::create(plan, Default::default());
        let (_recorder, capture) = Recorder::start(RecordSpec {
            sample_rate: 48_000,
            lanes: vec![],
            midi: false,
            latency: 0,
            midi_latency: 0,
            align: Box::new(|f| f),
            ring_seconds: 1.0,
            thru: vec![Thru {
                track: 0,
                port: None,
                transpose: 0,
                velocity: 0,
            }],
        })
        .unwrap();
        control.send(Command::Record(Some(capture))).ok().unwrap();
        control
            .send(Command::SetMidiInput(Some(Box::new(MockMidi::with_ports(
                vec!["A".into(), "B".into()],
                vec![
                    (0, 0, [0x90, 60, 100]),
                    (0, 1, [0x91, 60, 100]),
                    (0, 0, [0xb0, 64, 127]),
                    (0, 1, [0xb1, 64, 127]),
                    (256, 0, [0x80, 60, 0]),
                    (256, 1, [0x81, 60, 0]),
                    (1024, 1, [0x91, 60, 100]),
                ],
            )))))
            .ok()
            .unwrap();
        let render = |audio: &mut crate::AudioEngine| {
            let mut out = [0.0; 512];
            audio.render(&mut out, 2);
            out.iter().map(|x| x.abs()).fold(0.0_f32, f32::max)
        };
        assert!(render(&mut audio) > 0.001);
        render(&mut audio);
        assert!(
            pressed.notes().is_empty(),
            "pedal tails are not physical presses"
        );
        control.send(Command::ReleaseMidiPort(1)).ok().unwrap();
        assert!(
            render(&mut audio) > 0.001,
            "A's pedal survives B disconnect"
        );
        control.send(Command::ReleaseMidiPort(0)).ok().unwrap();
        // Let the Sampler's release envelope finish; the release block can
        // contain its decaying first samples even at the minimum release.
        render(&mut audio);
        assert!(
            render(&mut audio) < 1e-6,
            "queued B On after disconnect is ignored"
        );
        assert!(render(&mut audio) < 1e-6);
        assert!(pressed.notes().is_empty());
    }
}
