//! Turns the song into what the engine plays: a [`MixSpec`] with a source and inserts on each
//! track, the routing, the strip values and the notes and audio on each track's timeline.
//!
//! VST3 plugins are opened through [`crate::plugins`] (PLG-052), in this process or a plugin
//! host process as each one says (OPEN-02, PLG-022). The built-in instruments and effects are
//! the engine's ([`xirius_engine::stock`], PLG-016), as is the built-in Gain; anything else is
//! left out with a warning in [`Built::warnings`]. An instrument that cannot be opened, or a
//! track with none, is played by the built-in Sampler (the Drum Kit for a drum machine; the
//! placeholder of [`crate::synth`] for a built-in without a processor); such an effect is left
//! out.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use xirius_core::clipfx::{Fade, fade_exponent, pieces};
use xirius_core::model::{
    Clip, ClipBody, CtrlKind, CtrlShape, Plugin, PluginKind, Project, Track, TrackKind,
};
use xirius_engine::{
    BusSpec, CTRL_AFTERTOUCH, CTRL_PITCH_BEND, EnvPoint, MidiTarget, MixSpec, NoteKind,
    Output as EOut, ProcessSetup, Processor, Region, RenderMode, SendSpec, Shape, TimedNote,
    Timeline, TrackSlot, TrackSpec,
    builtin::Gain,
    timing::{Timed, Timings},
};

use xirius_engine::disk_stream::DiskAudio;
use xirius_engine::timeline::StreamRegion;
use xirius_params::Kind;
use xirius_plugins::Catalog;

use crate::clap_plugins::is_hosted;
use crate::graph::{DISCARD, Graph, Mix, Scope, Select, Structure, db_to_gain, song_end_tick};
use crate::plugins::{Isolation, OpenTime, PluginLoader};
use crate::synth::{Flavor, PlaceholderSynth, Silence};
use crate::tempo::TempoMap;
use crate::wav::WavCache;

/// The controller number the engine and VST3 use for a lane.
fn control_number(kind: CtrlKind) -> u8 {
    match kind {
        CtrlKind::Cc(n) => n,
        CtrlKind::Aftertouch => CTRL_AFTERTOUCH,
        CtrlKind::PitchBend => CTRL_PITCH_BEND,
    }
}

/// How often a straight stretch between two points sends a value (ticks).
const LINE_STEP: i64 = 30;

/// The key a MIDI clip's note sounds at with the clip's transpose (UI-311); none when the note
/// would leave the keyboard (0..=127), which is then not played.
pub fn playable(transpose: i8, pitch: u8) -> Option<u8> {
    u8::try_from(i32::from(pitch) + i32::from(transpose))
        .ok()
        .filter(|p| *p <= 127)
}

/// The controller changes of a MIDI clip's lanes on the frame timeline (EDT-004). A value holds
/// from its point until the next, as the lane draws it, so each point is one event; a point at or
/// after the clip's end is not played. A point made a straight line (CTX-034) also sends the
/// values between it and the next point, one every `LINE_STEP` ticks where the value has moved.
/// Where the clip ends, a lane that is not at rest goes back to it, so a bend or a held pedal
/// does not outlast its clip.
fn control_events(
    clip: &Clip,
    tmap: &TempoMap,
    rate: f64,
    points: &mut Vec<TimedNote>,
    resets: &mut Vec<TimedNote>,
) {
    for lane in &clip.controllers {
        let number = control_number(lane.kind);
        let (lo, hi) = lane.kind.range();
        let mut last = None;
        let mut push = |tick: i64, value: i32, last: &mut Option<i32>| {
            points.push(TimedNote {
                frame: tmap.frame_at(clip.start + tick, rate),
                kind: NoteKind::Control { number, value },
            });
            *last = Some(value);
        };
        for (i, p) in lane
            .points
            .iter()
            .enumerate()
            .filter(|(_, p)| p.tick < clip.length)
        {
            let value = p.value.clamp(lo, hi);
            push(p.tick, value, &mut last);
            let Some(next) = lane.points.get(i + 1) else {
                continue;
            };
            if p.shape != CtrlShape::Linear || next.tick <= p.tick {
                continue;
            }
            let (from, to) = (f64::from(value), f64::from(next.value.clamp(lo, hi)));
            let span = (next.tick - p.tick) as f64;
            let mut tick = p.tick + LINE_STEP;
            while tick < next.tick.min(clip.length) {
                let at = from + (to - from) * (tick - p.tick) as f64 / span;
                let between = at.round() as i32;
                if Some(between) != last {
                    push(tick, between, &mut last);
                }
                tick += LINE_STEP;
            }
        }
        if last.is_some_and(|v| v != lane.kind.rest()) {
            resets.push(TimedNote {
                frame: tmap.frame_at(clip.end(), rate),
                kind: NoteKind::Control {
                    number,
                    value: lane.kind.rest(),
                },
            });
        }
    }
}

/// The largest block the engine runs at once.
pub const MAX_BLOCK: usize = 512;

pub fn setup(rate: u32, mode: RenderMode) -> ProcessSetup {
    ProcessSetup {
        sample_rate: f64::from(rate),
        max_block: MAX_BLOCK,
        mode,
    }
}

/// Everything one build produced. `structure` and `mix` are what the spec was made from; the
/// host compares later builds against them to tell a new plan from a few commands.
pub struct Built {
    pub spec: MixSpec,
    pub graph: Graph,
    pub structure: Structure,
    pub mix: Mix,
    pub warnings: Vec<String>,
    /// Frame where the last clip ends.
    pub end_frame: u64,
}

pub struct Builder {
    rate: u32,
    dir: Option<PathBuf>,
    cache: WavCache,
    plugins: PluginLoader,
    /// How long each plugin takes to process a block (ENG-008), by plugin id.
    timings: Arc<Timings>,
    /// The plugins the build in progress has put a meter on.
    timed: Vec<u64>,
    /// Clips with a tempo, speed or pitch of their own are rendered in the background and
    /// left silent until ready (the live mix), instead of being rendered on the spot.
    background: bool,
    owner: u64,
}

impl Builder {
    /// `dir` is the song folder, which relative audio paths are taken from.
    pub fn new(rate: u32, dir: Option<PathBuf>) -> Builder {
        Builder {
            rate,
            dir,
            cache: WavCache::default(),
            plugins: PluginLoader::default(),
            timings: Arc::new(Timings::new()),
            timed: Vec::new(),
            background: false,
            owner: crate::processed::owner(),
        }
    }

    /// The meters of the plugins the last build made: read by anyone, written by the audio thread.
    pub fn timings(&self) -> Arc<Timings> {
        self.timings.clone()
    }

    /// Keeps how long each plugin of the last build took to open in [`Builder::timings`] and
    /// writes it to the log (ENG-064). Called once the mix is prepared, when the plugins in
    /// host processes have answered.
    pub fn note_open_times(&self) -> Vec<OpenTime> {
        let times = self.plugins.open_times();
        for t in &times {
            self.timings.set_opened(t.plugin.0, t.took);
            xirius_engine::crash::note(&format!(
                "plugin_opened owner={} name={} ms={:.1} process={}",
                t.owner,
                t.name,
                t.took.as_secs_f64() * 1000.0,
                if t.apart { "host" } else { "here" }
            ));
        }
        times
    }

    /// `p`, timed under the plugin's id.
    fn timed(&mut self, id: u64, p: Box<dyn Processor>) -> Box<dyn Processor> {
        self.timed.push(id);
        Box::new(Timed::new(p, self.timings.meter(id)))
    }

    /// Renders clips' tempo, speed and pitch on a worker; they play silent until ready
    /// ([`crate::processed`]). Off by default, as an export waits for them.
    pub fn set_background(&mut self, on: bool) {
        self.background = on;
    }

    /// Who this builder's renders belong to ([`crate::processed::sweep`]).
    pub fn owner(&self) -> u64 {
        self.owner
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn set_dir(&mut self, dir: Option<PathBuf>) {
        self.dir = dir;
    }

    /// The catalogue VST3 plugins are looked up in (the standard cache when `None`).
    pub fn set_catalog(&mut self, catalog: Option<Arc<Catalog>>) {
        self.plugins.set_catalog(catalog);
    }

    /// Where VST3 plugins run from the next build on.
    pub fn set_isolation(&mut self, isolation: Isolation) {
        self.plugins.set_isolation(isolation);
    }

    pub fn plugins(&self) -> &PluginLoader {
        &self.plugins
    }

    pub fn plugins_mut(&mut self) -> &mut PluginLoader {
        &mut self.plugins
    }

    pub fn build(&mut self, p: &Project, select: Select) -> Built {
        let graph = Graph::of(p);
        let scope = Scope::new(p, select);
        let structure = Structure::of(p, &graph, &scope);
        let mix = Mix::of(p, &graph, &scope);
        let tmap = TempoMap::from_project(p);
        let mut warnings = Vec::new();
        self.plugins.begin_build();
        self.timed.clear();
        self.plugins.set_tempo(tmap.bpm_at(0));

        let mut tracks = Vec::with_capacity(graph.tracks.len());
        for (i, id) in graph.tracks.iter().enumerate() {
            let t = &p.tracks[id];
            let (shape, strip) = (&structure.tracks[i], &mix.tracks[i]);
            let included = scope.includes(*id);
            // A track that plays to a MIDI port makes no sound of its own.
            let source: Box<dyn Processor> = if included && shape.midi.is_none() {
                self.source_for(t, &mut warnings)
            } else {
                Box::new(Silence)
            };
            let mut spec = TrackSpec::new(t.name.clone(), source);
            spec.live = crate::automation::realtime(t);
            spec.inserts = self.inserts_for(&t.name, &shape.inserts, &mut warnings);
            spec.gain = strip.gain;
            spec.pan = strip.pan;
            spec.mute = strip.mute;
            spec.sends = sends_for(&shape.sends, &strip.sends);
            spec.output = shape.output;
            spec.midi = shape.midi.as_ref().map(|m| {
                Box::new(MidiTarget {
                    port: m.port.clone(),
                    channel: m.channel.clamp(1, 16) - 1,
                })
            });
            if included {
                spec.timeline = self.timeline(p, t, &tmap, &mut warnings);
            }
            tracks.push(TrackSlot::New(spec));
        }

        let mut buses = Vec::with_capacity(structure.buses.len());
        for (shape, strip) in structure.buses.iter().zip(&mix.buses) {
            let name = if shape.id == DISCARD {
                "discard".to_string()
            } else {
                p.tracks[&shape.id].name.clone()
            };
            let mut spec = BusSpec::new(name.clone());
            spec.inserts = self.inserts_for(&name, &shape.inserts, &mut warnings);
            spec.gain = strip.gain;
            spec.pan = strip.pan;
            spec.mute = strip.mute;
            spec.output = shape.output;
            spec.sends = sends_for(&shape.sends, &strip.sends);
            buses.push(spec);
        }

        let master_inserts = self.inserts_for("master", &structure.master_inserts, &mut warnings);
        self.plugins.end_build();
        // A whole-song build keeps the meters of the plugins it used; a partial one (a track
        // rendered alone) leaves the song's meters as they are.
        if matches!(select, Select::All) {
            let used = std::mem::take(&mut self.timed);
            self.timings.retain(|id| used.contains(&id));
        }
        Built {
            spec: MixSpec {
                tracks,
                buses,
                master_inserts,
                master_gain: mix.master_gain,
            },
            end_frame: tmap.frame_at(song_end_tick(p), f64::from(self.rate)),
            graph,
            structure,
            mix,
            warnings,
        }
    }

    /// The notes and audio of a track on the frame timeline (ENG-030).
    ///
    /// A note is a pair of events at the frames its start and end fall on in the tempo map, so a
    /// tempo change inside a note changes its length in frames. A note stops where its clip does.
    /// A note that starts at or after the clip's end is not played, and a note is at least one
    /// frame long.
    pub fn timeline(
        &mut self,
        p: &Project,
        t: &Track,
        tmap: &TempoMap,
        warnings: &mut Vec<String>,
    ) -> Timeline {
        let rate = f64::from(self.rate);
        if let Some(f) = &t.frozen {
            return self.frozen(t, f, tmap, warnings);
        }
        let mut notes = Vec::new();
        // Controllers go back to rest where their clip ends. They are added before the points so
        // that, at the frame where one clip ends and the next starts, the next clip's points win.
        let mut resets = Vec::new();
        let mut regions = Vec::new();
        let mut streams = Vec::new();
        for clip in t.clips.iter().filter(|c| !c.muted) {
            // The content's stretches: all of it, or up to a loop's end and then the loop again
            // until the clip ends (EDT-041).
            let looped = p.clip_loop(clip.id);
            let parts = pieces(looped, clip.length);
            match &clip.body {
                ClipBody::Midi {
                    notes: src,
                    transpose,
                } if t.kind == TrackKind::Instrument => {
                    for (n, at, from, len) in parts.iter().flat_map(|&(at, from, len)| {
                        src.iter()
                            .filter(move |n| {
                                !n.muted && n.start < from + len && (at == 0 || n.start >= from)
                            })
                            .map(move |n| (n, at, from, len))
                    }) {
                        // The clip's transpose and the track's are added when the notes play; a
                        // note that would leave the keyboard is not played (as Studio Pro's
                        // event and track transpose, UI-311/UI-312). The track's velocity is
                        // applied to the note's (UI-312).
                        let Some(pitch) = playable(transpose.saturating_add(t.transpose), n.pitch)
                        else {
                            continue;
                        };
                        let on_tick = clip.start + at + n.start - from;
                        let off_tick = (on_tick + n.length).min(clip.start + at + len);
                        let on = tmap.frame_at(on_tick, rate);
                        let off = tmap.frame_at(off_tick, rate).max(on + 1);
                        notes.push(TimedNote {
                            frame: on,
                            kind: NoteKind::On {
                                pitch,
                                velocity: f32::from(xirius_core::model::track_velocity(
                                    n.velocity, t.velocity,
                                )) / 127.0,
                            },
                        });
                        notes.push(TimedNote {
                            frame: off,
                            kind: NoteKind::Off { pitch },
                        });
                    }
                    control_events(clip, tmap, rate, &mut notes, &mut resets);
                }
                ClipBody::Audio {
                    file,
                    offset,
                    gain_db,
                    process,
                } if t.kind == TrackKind::Audio => {
                    let path = self.resolve(file);
                    let regions_of = |data: Arc<_>| {
                        let whole = shape_of(p, t, clip, tmap, rate);
                        let first = tmap.frame_at(clip.start, rate);
                        let last = parts.len() - 1;
                        parts
                            .iter()
                            .enumerate()
                            .map(|(i, &(at, from, len))| {
                                let start = tmap.frame_at(clip.start + at, rate);
                                let end = tmap.frame_at(clip.start + at + len, rate);
                                let skip = tmap.seconds_at(clip.start + offset + from)
                                    - tmap.seconds_at(clip.start);
                                let len = end.saturating_sub(start);
                                Region {
                                    start,
                                    len,
                                    offset: (skip * rate).round().max(0.0) as u64,
                                    gain: db_to_gain(*gain_db),
                                    data: Arc::clone(&data),
                                    shape: if last == 0 {
                                        whole.clone()
                                    } else {
                                        piece_shape(&whole, start - first, len, i == 0, i == last)
                                    },
                                }
                            })
                            .collect::<Vec<_>>()
                    };
                    let bpm = tmap.bpm_at(clip.start);
                    if let Some(key) = crate::processed::Key::of(&path, self.rate, process, bpm) {
                        let (cache, at) = (&mut self.cache, self.rate);
                        match crate::processed::get(&key, self.owner, !self.background, || {
                            cache.load(&path, at)
                        }) {
                            crate::processed::State::Ready(data) => {
                                regions.extend(regions_of(data))
                            }
                            // Silent until it is ready.
                            crate::processed::State::Working(_) => {}
                            crate::processed::State::Failed(e) => {
                                warnings.push(format!("{} / {}: {e}", t.name, clip.name));
                            }
                        }
                        continue;
                    }
                    // Native-rate, large WAVs stay on disk. Other formats and rate conversion
                    // still use the decoder, so streaming never changes conversion quality.
                    // A looped clip reads its file in several places, so it is loaded whole.
                    if looped.is_none()
                        && std::fs::metadata(&path).is_ok_and(|m| m.len() >= 16 * 1024 * 1024)
                    {
                        match DiskAudio::open(&path, self.rate) {
                            Ok(Some(data)) => {
                                let start = tmap.frame_at(clip.start, rate);
                                let end = tmap.frame_at(clip.end(), rate);
                                let offset = ((tmap.seconds_at(clip.start + offset)
                                    - tmap.seconds_at(clip.start))
                                    * rate)
                                    .round()
                                    .max(0.0) as u64;
                                data.prime(offset);
                                if data.failed() || data.missed_frames() > 0 {
                                    warnings.push(format!(
                                        "{} / {}: 音声ファイルを読み込めません",
                                        t.name, clip.name
                                    ));
                                    continue;
                                }
                                streams.push((
                                    regions.len(),
                                    StreamRegion {
                                        start,
                                        len: end.saturating_sub(start),
                                        offset,
                                        gain: db_to_gain(*gain_db),
                                        data,
                                        shape: shape_of(p, t, clip, tmap, rate),
                                    },
                                ));
                                continue;
                            }
                            Err(e) => {
                                warnings.push(format!("{} / {}: {e}", t.name, clip.name));
                                continue;
                            }
                            Ok(None) => {}
                        }
                    }
                    match self.cache.load(&path, self.rate) {
                        Ok(data) => regions.extend(regions_of(data)),
                        Err(e) => warnings.push(format!("{} / {}: {e}", t.name, clip.name)),
                    }
                }
                _ => {}
            }
        }
        resets.append(&mut notes);
        let mut timeline = Timeline::new(resets, regions);
        for (before, region) in streams {
            timeline.push_stream_at(before, region);
        }
        timeline
    }

    /// What a frozen track plays: its file from `f.start`, whole.
    fn frozen(
        &mut self,
        t: &Track,
        f: &xirius_core::model::Frozen,
        tmap: &TempoMap,
        warnings: &mut Vec<String>,
    ) -> Timeline {
        let path = self.resolve(&f.file);
        let mut regions = Vec::new();
        match self.cache.load(&path, self.rate) {
            Ok(data) => regions.push(Region {
                start: tmap.frame_at(f.start, f64::from(self.rate)),
                len: data.frames() as u64,
                offset: 0,
                gain: 1.0,
                data,
                shape: Shape::default(),
            }),
            Err(e) => warnings.push(format!("{}: {e}", t.name)),
        }
        Timeline::new(Vec::new(), regions)
    }

    fn source_for(&mut self, t: &Track, warnings: &mut Vec<String>) -> Box<dyn Processor> {
        if t.kind != TrackKind::Instrument || t.frozen.is_some() {
            return Box::new(Silence);
        }
        let inst = t.inserts.iter().find(|x| x.kind == PluginKind::Instrument);
        let source = self.instrument_for(t, inst, warnings);
        match inst {
            Some(x) if x.runs() => self.timed(x.id.0, source),
            _ => source,
        }
    }

    fn instrument_for(
        &mut self,
        t: &Track,
        inst: Option<&Plugin>,
        warnings: &mut Vec<String>,
    ) -> Box<dyn Processor> {
        match inst {
            Some(x) if !x.runs() => Box::new(Silence),
            Some(x) if is_hosted(x) => match self.plugins.instantiate(&t.name, x) {
                Ok((p, w)) => {
                    warnings.extend(w.into_iter().map(|w| format!("{}: {w}", t.name)));
                    p
                }
                Err(e) => {
                    warnings.push(format!(
                        "{}: {e}。代わりのインストゥルメントで再生します",
                        t.name
                    ));
                    stand_in(flavor_of(x))
                }
            },
            Some(x) if x.builtin().is_some_and(|d| d.kind == Kind::Instrument) => {
                match self.plugins.stock(x) {
                    Some((p, w)) => {
                        warnings.extend(w.into_iter().map(|w| format!("{}: {w}", t.name)));
                        p
                    }
                    None => Box::new(PlaceholderSynth::new(flavor_of(x))),
                }
            }
            Some(x) => {
                warnings.push(format!(
                    "{}: {}（{} {}）はまだ読み込めないため、代わりのインストゥルメントで再生します",
                    t.name, x.name, x.format, x.uid
                ));
                stand_in(flavor_of(x))
            }
            None => stand_in(Flavor::Tone),
        }
    }

    /// The effects of an insert row: the built-in Gain and VST3 plugins.
    fn inserts_for(
        &mut self,
        owner: &str,
        plugins: &[Plugin],
        warnings: &mut Vec<String>,
    ) -> Vec<Box<dyn Processor>> {
        let mut out: Vec<Box<dyn Processor>> = Vec::new();
        for x in plugins
            .iter()
            .filter(|x| x.kind == PluginKind::Effect && x.runs())
        {
            if x.format == "builtin" && x.uid.eq_ignore_ascii_case("gain") {
                let db = x.params.get("gain_db").copied().unwrap_or(0.0);
                out.push(self.timed(x.id.0, Box::new(Gain::new(db_to_gain(db)))));
            } else if let Some(def) = x.builtin() {
                if def.kind == Kind::Instrument {
                    warnings.push(format!(
                        "{owner}: {} はインストゥルメントのため、エフェクトの列ではスキップします",
                        x.name
                    ));
                    continue;
                }
                match self.plugins.stock(x) {
                    Some((p, w)) => {
                        warnings.extend(w.into_iter().map(|w| format!("{owner}: {w}")));
                        out.push(self.timed(x.id.0, p));
                    }
                    None => warnings.push(format!(
                        "{owner}: {} はまだ用意していないため、スキップします",
                        x.name
                    )),
                }
            } else if is_hosted(x) {
                match self.plugins.instantiate(owner, x) {
                    Ok((p, w)) => {
                        warnings.extend(w.into_iter().map(|w| format!("{owner}: {w}")));
                        out.push(self.timed(x.id.0, p));
                    }
                    Err(e) => warnings.push(format!("{owner}: {e}。スキップします")),
                }
            } else {
                warnings.push(format!(
                    "{owner}: {}（{} {}）はまだ読み込めないため、スキップします",
                    x.name, x.format, x.uid
                ));
            }
        }
        out
    }

    fn resolve(&self, file: &str) -> PathBuf {
        let path = Path::new(file);
        match (&self.dir, path.is_absolute()) {
            (Some(dir), false) => dir.join(path),
            _ => path.to_path_buf(),
        }
    }
}

fn sends_for(shape: &[(usize, bool)], levels: &[f32]) -> Vec<SendSpec> {
    shape
        .iter()
        .zip(levels)
        .map(|(&(bus, pre), &level)| SendSpec {
            bus,
            level,
            pre_fader: pre,
        })
        .collect()
}

/// A word that marks a plugin as a drum machine.
fn is_drums(x: &Plugin) -> bool {
    let hit = |s: &str| s.to_ascii_lowercase().contains("drum");
    hit(&x.name) || hit(&x.uid)
}

/// The instrument that plays a track whose own cannot: the built-in Sampler or Drum Kit, or the
/// placeholder while the engine has no processor for it.
fn stand_in(flavor: Flavor) -> Box<dyn Processor> {
    let key = match flavor {
        Flavor::Tone => "sampler",
        Flavor::Drums => "drum-kit",
    };
    match xirius_engine::stock::create(key) {
        Some((p, _)) => p,
        None => Box::new(PlaceholderSynth::new(flavor)),
    }
}

fn flavor_of(x: &Plugin) -> Flavor {
    if is_drums(x) {
        Flavor::Drums
    } else {
        Flavor::Tone
    }
}

/// What the engine calls an output, for messages.
pub fn describe_output(o: EOut) -> String {
    match o {
        EOut::Master => "master".into(),
        EOut::Bus(i) => format!("bus {i}"),
    }
}

/// The fades and the gain line of a clip on the frame timeline (EDT-039, EDT-040, EDT-017). The
/// lengths in ticks become lengths in frames where the clip sits in the tempo map, so a tempo
/// change inside a fade changes its length in frames; the two fades of a crossfade cover the same
/// frames because they cover the same ticks.
/// The part of a clip's fades and gain line that falls on one pass of its loop,
/// `from` frames into the clip and `len` long: the fade-in on the first pass,
/// the fade-out on the last, and the gain line from where the pass starts.
fn piece_shape(whole: &Shape, from: u64, len: u64, first: bool, last: bool) -> Shape {
    let env = &whole.envelope;
    let mut envelope = Vec::new();
    if let Some(i) = env.iter().position(|e| e.frame > from) {
        if i > 0 {
            let (a, b) = (&env[i - 1], &env[i]);
            let x = (from - a.frame) as f32 / (b.frame - a.frame).max(1) as f32;
            envelope.push(EnvPoint {
                frame: 0,
                gain: a.gain + (b.gain - a.gain) * x,
            });
        }
        envelope.extend(env[i..].iter().map(|e| EnvPoint {
            frame: e.frame - from,
            gain: e.gain,
        }));
    } else if let Some(e) = env.last() {
        envelope.push(EnvPoint {
            frame: 0,
            gain: e.gain,
        });
    }
    Shape {
        fade_in: if first { whole.fade_in.min(len) } else { 0 },
        fade_out: if last { whole.fade_out.min(len) } else { 0 },
        envelope,
        ..*whole
    }
}

fn shape_of(
    p: &Project,
    t: &Track,
    clip: &xirius_core::model::Clip,
    tmap: &TempoMap,
    rate: f64,
) -> Shape {
    let fx = p.resolve_fx(t, clip);
    if fx.is_plain() {
        return Shape::default();
    }
    let frame = |tick| tmap.frame_at(tick, rate);
    let start = frame(clip.start);
    let end = frame(clip.end());
    let fade_in = |f: &Fade| frame(clip.start + f.length).saturating_sub(start);
    let fade_out = |f: &Fade| end.saturating_sub(frame(clip.end() - f.length));
    Shape {
        fade_in: fade_in(&fx.fade_in),
        fade_in_exp: fade_exponent(fx.fade_in.curve) as f32,
        fade_out: fade_out(&fx.fade_out),
        fade_out_exp: fade_exponent(fx.fade_out.curve) as f32,
        envelope: fx
            .points
            .iter()
            .map(|g| EnvPoint {
                frame: frame(clip.start + g.tick).saturating_sub(start),
                gain: db_to_gain(g.db),
            })
            .collect(),
    }
}
