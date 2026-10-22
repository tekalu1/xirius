//! The real-time half of a CLAP instance: activation (on the main thread), then `process` with
//! buffers and event lists made at [`AudioPart::prepare`].

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use clack_extensions::audio_ports::AudioPortInfoBuffer;
use clack_extensions::note_ports::{NoteDialects, NotePortInfoBuffer};
use clack_extensions::render::RenderMode;
use clack_host::events::EventFlags;
use clack_host::events::event_types::{
    MidiEvent, NoteOffEvent, NoteOnEvent, ParamValueEvent, TransportEvent, TransportFlags,
};
use clack_host::events::io::{OutputEventBuffer, TryPushError};
use clack_host::events::spaces::CoreEventSpace;
use clack_host::events::{Match, UnknownEvent};
use clack_host::prelude::*;
use clack_host::utils::{Cookie, FixedPoint};

use crate::host::{Flags, XiriusHost};
use crate::plugin::{Gesture, Job, Main, ParamInfo};
use crate::{CTRL_AFTERTOUCH, CTRL_PITCH_BEND, Control, Error, Note, ParamChange, error};

/// Events handed to the plugin in one block; more are dropped (and counted).
const MAX_EVENTS: u32 = 1024;
/// Events the plugin may send back in one block.
const MAX_OUT_EVENTS: u32 = 1024;

/// An output event list that refuses events past its capacity instead of growing.
struct Bounded {
    events: EventBuffer,
    cap: u32,
    dropped: u64,
}

impl OutputEventBuffer for Bounded {
    fn try_push(&mut self, event: &UnknownEvent) -> Result<(), TryPushError> {
        if self.events.len() >= self.cap {
            self.dropped += 1;
            return Err(TryPushError::new());
        }
        self.events.push(event);
        Ok(())
    }
}

/// What activation found out, sent back from the main thread.
struct Activated {
    processor: StoppedPluginAudioProcessor<XiriusHost>,
    inputs: Vec<usize>,
    outputs: Vec<usize>,
    notes: Option<NoteDialects>,
    latency: u32,
    /// Read again here: a plugin may only change its list (and the cookies) while inactive.
    params: Vec<ParamInfo>,
}

fn activate(
    m: &mut Main,
    sample_rate: f64,
    max_block: u32,
    offline: bool,
) -> Result<Activated, Error> {
    if let Some(render) = m.render {
        // A plugin may refuse offline mode; it then simply renders in real time.
        let mode = if offline {
            RenderMode::Offline
        } else {
            RenderMode::Realtime
        };
        let _ = render.set(&m.instance.plugin_handle(), mode);
    }
    let (inputs, outputs, notes) = {
        let h = m.instance.plugin_handle();
        let ports = |is_input: bool| -> Vec<usize> {
            let Some(ext) = m.audio_ports else {
                return Vec::new();
            };
            let mut buf = AudioPortInfoBuffer::new();
            (0..ext.count(&h, is_input))
                .map(|i| {
                    ext.get(&h, i, is_input, &mut buf)
                        .map_or(0, |p| p.channel_count as usize)
                })
                .collect()
        };
        let notes = m.note_ports.and_then(|ext| {
            if ext.count(&h, true) == 0 {
                return None;
            }
            let mut buf = NotePortInfoBuffer::new();
            ext.get(&h, 0, true, &mut buf).map(|p| p.supported_dialects)
        });
        (ports(true), ports(false), notes)
    };
    let processor = m
        .instance
        .activate(
            |_, _| (),
            PluginAudioConfiguration {
                sample_rate,
                min_frames_count: 1,
                max_frames_count: max_block,
            },
        )
        .map_err(|e| error(format!("activating the plugin: {e}")))?;
    m.flags.active.store(true, Ordering::Release);
    let latency = m.read_latency();
    let params = m.param_infos();
    Ok(Activated {
        processor,
        inputs,
        outputs,
        notes,
        latency,
        params,
    })
}

/// The part of a plugin that the audio thread runs. Made by [`Plugin::audio`](crate::Plugin::audio).
pub struct AudioPart {
    name: String,
    jobs: mpsc::Sender<Job>,
    flags: Arc<Flags>,
    params: Vec<ParamInfo>,
    edits: rtrb::Consumer<(u32, f64)>,
    gestures: rtrb::Producer<Gesture>,
    stopped: Option<StoppedPluginAudioProcessor<XiriusHost>>,
    started: Option<StartedPluginAudioProcessor<XiriusHost>>,
    in_ports: AudioPorts,
    out_ports: AudioPorts,
    /// Per port, per channel, `max_block` frames.
    in_bufs: Vec<Vec<Vec<f32>>>,
    out_bufs: Vec<Vec<Vec<f32>>>,
    in_events: EventBuffer,
    out_events: Bounded,
    notes: Option<NoteDialects>,
    max_block: usize,
    steady: u64,
    sample_rate: f64,
    latency: u32,
    process_errors: u64,
    dropped_events: u64,
}

impl AudioPart {
    pub(crate) fn new(
        name: String,
        jobs: mpsc::Sender<Job>,
        flags: Arc<Flags>,
        params: Vec<ParamInfo>,
        edits: rtrb::Consumer<(u32, f64)>,
        gestures: rtrb::Producer<Gesture>,
    ) -> AudioPart {
        AudioPart {
            name,
            jobs,
            flags,
            params,
            edits,
            gestures,
            stopped: None,
            started: None,
            in_ports: AudioPorts::with_capacity(0, 0),
            out_ports: AudioPorts::with_capacity(0, 0),
            in_bufs: Vec::new(),
            out_bufs: Vec::new(),
            in_events: EventBuffer::new(),
            out_events: Bounded {
                events: EventBuffer::new(),
                cap: MAX_OUT_EVENTS,
                dropped: 0,
            },
            notes: None,
            max_block: 0,
            steady: 0,
            sample_rate: 48_000.0,
            latency: 0,
            process_errors: 0,
            dropped_events: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Activates the plugin for this rate and block size. Returns its latency in frames.
    /// Call it off the audio thread; it allocates and waits for the plugin's main thread.
    pub fn prepare(
        &mut self,
        sample_rate: f64,
        max_block: usize,
        offline: bool,
    ) -> Result<u32, Error> {
        self.release();
        let max_block = max_block.max(1);
        let frames = u32::try_from(max_block).map_err(|_| error("the block is too long"))?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.jobs
            .send(Box::new(move |m| {
                let _ = tx.send(activate(m, sample_rate, frames, offline));
            }))
            .map_err(|_| error("the plugin's main thread is gone"))?;
        let a = rx
            .recv()
            .map_err(|_| error("the plugin's main thread is gone"))??;
        let bufs = |chans: &[usize]| -> Vec<Vec<Vec<f32>>> {
            chans
                .iter()
                .map(|&c| vec![vec![0.0; max_block]; c])
                .collect()
        };
        self.in_bufs = bufs(&a.inputs);
        self.out_bufs = bufs(&a.outputs);
        // Room for more than asked: clack's path for regrowing these is best avoided.
        let room = |chans: &[usize]| (chans.iter().sum::<usize>() + 2, chans.len() + 1);
        let (c, p) = room(&a.inputs);
        self.in_ports = AudioPorts::with_capacity(c, p);
        let (c, p) = room(&a.outputs);
        self.out_ports = AudioPorts::with_capacity(c, p);
        self.in_events = EventBuffer::with_capacity(MAX_EVENTS as usize);
        self.out_events.events = EventBuffer::with_capacity(MAX_OUT_EVENTS as usize);
        self.notes = a.notes;
        self.max_block = max_block;
        self.latency = a.latency;
        self.params = a.params;
        self.steady = 0;
        self.sample_rate = sample_rate;
        self.stopped = Some(a.processor);
        Ok(a.latency)
    }

    /// Stops processing and has the main thread deactivate the plugin.
    fn release(&mut self) {
        let stopped = match (self.started.take(), self.stopped.take()) {
            (Some(s), _) => Some(s.stop_processing()),
            (None, s) => s,
        };
        if let Some(s) = stopped {
            let _ = self.jobs.send(Box::new(move |m| {
                m.instance.deactivate(s);
                m.flags.active.store(false, Ordering::Release);
            }));
        }
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    /// The channels of each audio input and output port, as activated.
    pub fn input_channels(&self) -> Vec<usize> {
        self.in_bufs.iter().map(Vec::len).collect()
    }

    pub fn output_channels(&self) -> Vec<usize> {
        self.out_bufs.iter().map(Vec::len).collect()
    }

    /// Whether the plugin has a note input.
    pub fn has_note_input(&self) -> bool {
        self.notes.is_some()
    }

    /// `process` calls that failed, and events dropped because a block held more than fit.
    pub fn counts(&self) -> (u64, u64) {
        (
            self.process_errors,
            self.dropped_events + self.out_events.dropped,
        )
    }

    /// Processes `left`/`right` in place (at most `max_block` frames). Instruments ignore the
    /// input and write their output.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], notes: &[Note]) {
        self.process_with_params(left, right, notes, &[], &[]);
    }

    /// Processes block-timed parameter changes together with audio, notes and controllers,
    /// each sorted by offset. Does not allocate.
    pub fn process_with_params(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        notes: &[Note],
        controls: &[Control],
        params: &[ParamChange],
    ) {
        let n = left.len().min(right.len()).min(self.max_block);
        if n == 0 {
            return;
        }
        if self.started.is_none() {
            let Some(stopped) = self.stopped.take() else {
                return;
            };
            match stopped.start_processing() {
                Ok(s) => self.started = Some(s),
                Err(e) => {
                    self.stopped = Some(e.into_stopped_processor());
                    self.process_errors += 1;
                    return;
                }
            }
        }
        self.fill_inputs(left, right, n);
        self.fill_events(notes, controls, params, n as u32);
        self.out_events.events.clear();

        let transport = transport(self.steady, self.sample_rate);
        let Self {
            started,
            in_ports,
            out_ports,
            in_bufs,
            out_bufs,
            in_events,
            out_events,
            steady,
            ..
        } = self;
        let Some(started) = started.as_mut() else {
            return;
        };
        let inputs = in_ports.with_input_buffers(in_bufs.iter_mut().map(|port| AudioPortBuffer {
            channels: AudioPortBufferType::f32_input_only(
                port.iter_mut().map(|c| InputChannel::variable(&mut c[..n])),
            ),
            latency: 0,
        }));
        let mut outputs =
            out_ports.with_output_buffers(out_bufs.iter_mut().map(|port| AudioPortBuffer {
                channels: AudioPortBufferType::f32_output_only(
                    port.iter_mut().map(|c| &mut c[..n]),
                ),
                latency: 0,
            }));
        let result = started.process(
            &inputs,
            &mut outputs,
            &in_events.as_input(),
            &mut OutputEvents::from_buffer(out_events),
            Some(*steady),
            Some(&transport),
        );
        *steady += n as u64;
        if result.is_err() {
            self.process_errors += 1;
        }

        if let Some(port) = self.out_bufs.first() {
            match port.as_slice() {
                [] => {}
                [mono] => {
                    left[..n].copy_from_slice(&mono[..n]);
                    right[..n].copy_from_slice(&mono[..n]);
                }
                [l, r, ..] => {
                    left[..n].copy_from_slice(&l[..n]);
                    right[..n].copy_from_slice(&r[..n]);
                }
            }
        }
        self.collect_gestures();
    }

    fn fill_inputs(&mut self, left: &[f32], right: &[f32], n: usize) {
        let Some(port) = self.in_bufs.first_mut() else {
            return;
        };
        match port.as_mut_slice() {
            [] => {}
            [mono] => {
                for ((m, l), r) in mono[..n].iter_mut().zip(left).zip(right) {
                    *m = 0.5 * (l + r);
                }
            }
            [l, r, rest @ ..] => {
                l[..n].copy_from_slice(&left[..n]);
                r[..n].copy_from_slice(&right[..n]);
                for c in rest {
                    c[..n].fill(0.0);
                }
            }
        }
    }

    fn push(&mut self, event: &impl AsRef<UnknownEvent>) {
        if self.in_events.len() < MAX_EVENTS {
            self.in_events.push(event);
        } else {
            self.dropped_events += 1;
        }
    }

    /// The cookie the plugin gave for `id` at activation (null for an unknown id).
    fn cookie(&self, id: u32) -> Cookie {
        self.params
            .iter()
            .find(|i| i.id == id)
            .map_or(Cookie::empty(), |i| i.cookie)
    }

    fn fill_events(
        &mut self,
        notes: &[Note],
        controls: &[Control],
        params: &[ParamChange],
        n: u32,
    ) {
        self.in_events.clear();
        let last = n - 1;
        while let Ok((id, plain)) = self.edits.pop() {
            let cookie = self.cookie(id);
            self.push(&value_event(0, id, plain, cookie));
        }
        for p in params {
            let Some((plain, cookie)) = self
                .params
                .iter()
                .find(|i| i.id == p.id)
                .map(|i| (i.plain(p.value), i.cookie))
            else {
                continue;
            };
            self.push(&value_event(p.offset.min(last), p.id, plain, cookie));
        }
        if let Some(dialects) = self.notes {
            let clap = dialects.contains(NoteDialects::CLAP);
            let midi = dialects.contains(NoteDialects::MIDI);
            for note in notes {
                let t = note.offset.min(last);
                let key = Pckn::new(0u16, 0u16, u16::from(note.pitch), Match::All);
                let velocity = note.velocity.clamp(0.0, 1.0);
                if clap {
                    if note.on {
                        self.push(&NoteOnEvent::new(t, key, f64::from(velocity)));
                    } else {
                        self.push(&NoteOffEvent::new(t, key, 0.0));
                    }
                } else if midi {
                    let pitch = note.pitch.min(127);
                    let data = if note.on {
                        [0x90, pitch, ((velocity * 127.0).round() as u8).max(1)]
                    } else {
                        [0x80, pitch, 0]
                    };
                    self.push(&MidiEvent::new(t, 0, data));
                }
            }
            if midi {
                for c in controls {
                    let t = c.offset.min(last);
                    let data = match c.number {
                        CTRL_PITCH_BEND => {
                            let v = (c.value + 8192).clamp(0, 16383) as u16;
                            [0xE0, (v & 0x7f) as u8, (v >> 7) as u8]
                        }
                        CTRL_AFTERTOUCH => [0xD0, c.value.clamp(0, 127) as u8, 0],
                        cc @ 0..=127 => [0xB0, cc, c.value.clamp(0, 127) as u8],
                        _ => continue,
                    };
                    self.push(&MidiEvent::new(t, 0, data));
                }
            }
        }
        self.in_events.sort();
    }

    /// Turns the plugin's own parameter events (its editor's) into [`Gesture`]s.
    fn collect_gestures(&mut self) {
        let normalize = |params: &[ParamInfo], id: u32, v: f64| {
            params
                .iter()
                .find(|p| p.id == id)
                .map_or(v, |p| p.normalize(v))
        };
        for event in self.out_events.events.iter() {
            let g = match event.as_core_event() {
                Some(CoreEventSpace::ParamGestureBegin(e)) => {
                    e.param_id().map(|id| Gesture::Begin(id.get()))
                }
                Some(CoreEventSpace::ParamGestureEnd(e)) => {
                    e.param_id().map(|id| Gesture::End(id.get()))
                }
                Some(CoreEventSpace::ParamValue(e)) => e.param_id().map(|id| {
                    Gesture::Perform(id.get(), normalize(&self.params, id.get(), e.value()))
                }),
                _ => None,
            };
            if let Some(g) = g {
                // A full ring drops the newest; the reader is behind anyway.
                let _ = self.gestures.push(g);
            }
        }
    }
}

impl Drop for AudioPart {
    fn drop(&mut self) {
        // Before the release, so edits made from here on are flushed rather than queued.
        self.flags.audio_alive.store(false, Ordering::Release);
        self.release();
    }
}

/// The tempo and metre the plugin is told, as the VST3 host tells its plugins.
const TEMPO: f64 = 120.0;
const BEATS_PER_BAR: u16 = 4;

/// The transport at `frame`: playing from the start at [`TEMPO`] in 4/4. Some plugins read it
/// whenever a tempo-synced parameter moves and do not expect a free-running host (Vital
/// crashed on its tempo parameter without it).
fn transport(frame: u64, sample_rate: f64) -> TransportEvent {
    let seconds = frame as f64 / sample_rate.max(1.0);
    let beats = seconds * TEMPO / 60.0;
    let bar = (beats / f64::from(BEATS_PER_BAR)).floor();
    TransportEvent {
        header: EventHeader::new_core(0, EventFlags::empty()),
        flags: TransportFlags::HAS_TEMPO
            | TransportFlags::HAS_BEATS_TIMELINE
            | TransportFlags::HAS_SECONDS_TIMELINE
            | TransportFlags::HAS_TIME_SIGNATURE
            | TransportFlags::IS_PLAYING,
        song_pos_beats: FixedPoint::from_float(beats),
        song_pos_seconds: FixedPoint::from_float(seconds),
        tempo: TEMPO,
        tempo_inc: 0.0,
        loop_start_beats: FixedPoint::from_int(0),
        loop_end_beats: FixedPoint::from_int(0),
        loop_start_seconds: FixedPoint::from_int(0),
        loop_end_seconds: FixedPoint::from_int(0),
        bar_start: FixedPoint::from_float(bar * f64::from(BEATS_PER_BAR)),
        bar_number: bar as i32,
        time_signature_numerator: BEATS_PER_BAR,
        time_signature_denominator: 4,
    }
}

/// A value change for every note, with the plugin's cookie for the parameter.
pub(crate) fn value_event(time: u32, id: u32, plain: f64, cookie: Cookie) -> ParamValueEvent {
    let event = ParamValueEvent::new(time, ClapId::new(id), Pckn::match_all(), plain);
    // SAFETY: the cookie is the one the plugin gave in `params.get_info` at this activation (or
    // null); it stays valid until a full rescan, which the plugin may only ask for while
    // inactive, and `prepare` reads the list again (the plugin's own list is read at creation and
    // at `rescan_params`).
    unsafe { event.with_cookie(cookie) }
}
