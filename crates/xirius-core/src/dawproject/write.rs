//! A song as a `.dawproject` (EXP-007).

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use super::xml::{El, num};
use super::zip::ZipWriter;
use super::{AudioInfo, Skips, vstpreset};
use crate::auto::{AutoLane, AutoTarget};
use crate::id::{FolderId, Node, TrackId};
use crate::model::{
    AudioProcess, Clip, ClipBody, CtrlKind, CtrlShape, MIN_DB, Output, Plugin, PluginKind, Project,
    Track, TrackKind,
};
use crate::tempo::TempoMap;
use crate::time::Tick;

/// The tempo range DAWproject's transport takes.
const MIN_BPM: f64 = 20.0;
const MAX_BPM: f64 = 666.0;

/// How a song is written.
pub struct Export<'a> {
    /// The song's folder, which relative clip files are read from. `None` for a song never saved.
    pub song_dir: Option<&'a Path>,
    /// Puts the audio the clips play inside the file. Off, the file names them where they are.
    pub embed_audio: bool,
    /// The channels, rate and length of an audio file; `None` when it cannot be read.
    pub audio: &'a dyn Fn(&Path) -> Option<AudioInfo>,
}

/// Writes the song to `out` and returns what the file could not carry. The file is written
/// next to `out` first and takes its name only when it is whole.
pub fn export(p: &Project, out: &Path, opts: &Export) -> Result<Skips, String> {
    export_with(p, out, opts, &mut |_| {})
}

/// [`export`], telling `step` how far the file is written (0..=1) as it goes.
pub fn export_with(
    p: &Project,
    out: &Path,
    opts: &Export,
    step: &mut dyn FnMut(f64),
) -> Result<Skips, String> {
    let built = Writer::new(p, opts).build();
    let mut part = out.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    if let Err(e) = write_zip(&part, &built, step) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    fs::rename(&part, out).map_err(|e| {
        let _ = fs::remove_file(&part);
        format!("{} に書き出せません: {e}", out.display())
    })?;
    Ok(built.skips)
}

/// What [`export`] would leave out, without writing anything.
pub fn skipped_on_export(p: &Project, opts: &Export) -> Skips {
    Writer::new(p, opts).build().skips
}

fn write_zip(path: &Path, b: &Built, step: &mut dyn FnMut(f64)) -> Result<(), String> {
    // How far it is goes by the bytes put in.
    let size = |s: &Source| match s {
        Source::Bytes(d) => d.len() as u64,
        Source::File(p) => fs::metadata(p).map_or(0, |m| m.len()),
    };
    let total = (b.project.len() + b.metadata.len()) as u64
        + b.files.iter().map(|(_, s)| size(s)).sum::<u64>();
    let mut done = 0u64;
    let mut tell = |n: u64| {
        done += n;
        step(if total == 0 {
            1.0
        } else {
            (done as f64 / total as f64).min(1.0)
        });
    };
    let f =
        fs::File::create(path).map_err(|e| format!("{} に書き出せません: {e}", path.display()))?;
    let mut z = ZipWriter::new(BufWriter::new(f));
    z.add("project.xml", b.project.as_bytes(), true)?;
    z.add("metadata.xml", b.metadata.as_bytes(), true)?;
    tell((b.project.len() + b.metadata.len()) as u64);
    for (name, src) in &b.files {
        match src {
            Source::Bytes(data) => z.add(name, data, true)?,
            Source::File(p) => {
                let data = fs::read(p).map_err(|e| format!("{} を読めません: {e}", p.display()))?;
                // Audio hardly shrinks; storing it keeps the export fast.
                z.add(name, &data, false)?;
            }
        }
        tell(size(src));
    }
    let w = z.finish()?;
    w.into_inner()
        .map_err(|e| format!("{} に書き出せません: {e}", path.display()))?
        .sync_all()
        .map_err(|e| format!("{} に書き出せません: {e}", path.display()))
}

enum Source {
    Bytes(Vec<u8>),
    File(PathBuf),
}

struct Built {
    project: String,
    metadata: String,
    files: Vec<(String, Source)>,
    skips: Skips,
}

struct Writer<'a> {
    p: &'a Project,
    opts: &'a Export<'a>,
    tm: TempoMap,
    ppq: f64,
    next: u64,
    skips: Skips,
    /// The `Track` element whose lanes hold a channel's clips and automation.
    track_el: HashMap<TrackId, String>,
    channel: HashMap<TrackId, String>,
    folder_el: HashMap<FolderId, String>,
    /// A bus written as the channel of the folder connected to it.
    folded: HashMap<TrackId, FolderId>,
    /// The channels in the order their lanes are written.
    order: Vec<TrackId>,
    params: HashMap<(TrackId, AutoTarget), String>,
    tempo_id: String,
    meter_id: String,
    files: Vec<(String, Source)>,
    audio_names: HashMap<PathBuf, String>,
    /// The built-ins' parameters by their ids: their spec, what the plain value is multiplied
    /// by and the unit it is written in.
    plain: HashMap<String, (&'static xirius_params::ParamSpec, f64, &'static str)>,
}

impl<'a> Writer<'a> {
    fn new(p: &'a Project, opts: &'a Export<'a>) -> Writer<'a> {
        Writer {
            p,
            opts,
            tm: TempoMap::from_project(p),
            ppq: f64::from(p.ppq.max(1)),
            next: 0,
            skips: Skips::default(),
            track_el: HashMap::new(),
            channel: HashMap::new(),
            folder_el: HashMap::new(),
            folded: HashMap::new(),
            order: Vec::new(),
            params: HashMap::new(),
            tempo_id: String::new(),
            meter_id: String::new(),
            files: Vec::new(),
            audio_names: HashMap::new(),
            plain: HashMap::new(),
        }
    }

    fn id(&mut self) -> String {
        self.next += 1;
        format!("id{}", self.next)
    }

    fn beats(&self, t: Tick) -> String {
        num(t as f64 / self.ppq)
    }

    fn build(mut self) -> Built {
        let p = self.p;
        let mut folders = Vec::new();
        folders_in_order(p, &p.root, &mut folders);
        for f in folders {
            if let Some(b) = p.folders[&f].bus
                && p.tracks.get(&b).is_some_and(|t| t.kind == TrackKind::Bus)
                && !self.folded.contains_key(&b)
            {
                self.folded.insert(b, f);
            }
        }
        self.assign(&p.root);
        let e = self.id();
        self.track_el.insert(TrackId::MASTER, e);
        let c = self.id();
        self.channel.insert(TrackId::MASTER, c);
        self.order.push(TrackId::MASTER);

        let transport = self.transport();
        let mut structure = El::new("Structure");
        for n in &p.root {
            if let Some(el) = self.node(*n) {
                structure.push(el);
            }
        }
        let master = &p.tracks[&TrackId::MASTER];
        let el = self.track(master, "audio notes", "master");
        structure.push(el);
        let arrangement = self.arrangement();

        if let Some(t) = p.tracks.values().find(|t| t.midi_out.is_some()) {
            let n = p.tracks.values().filter(|t| t.midi_out.is_some()).count();
            if n == 1 {
                self.skips.item(format!("{} の MIDI 出力", t.name));
            } else {
                self.skips.count("トラックの MIDI 出力", n, "");
            }
        }

        let project = El::new("Project")
            .a("version", "1.0")
            .k(El::new("Application")
                .a("name", "Xirius")
                .a("version", env!("CARGO_PKG_VERSION")))
            .k(transport)
            .k(structure)
            .k(arrangement)
            .k(El::new("Scenes"))
            .document();
        let metadata = El::new("MetaData")
            .k(El::new("Title").t(p.name.clone()))
            .document();
        Built {
            project,
            metadata,
            files: self.files,
            skips: self.skips,
        }
    }

    fn assign(&mut self, nodes: &[Node]) {
        let p = self.p;
        for n in nodes {
            match *n {
                Node::Track(t) => {
                    if self.folded.contains_key(&t) || !p.tracks.contains_key(&t) {
                        continue;
                    }
                    let e = self.id();
                    self.track_el.insert(t, e);
                    let c = self.id();
                    self.channel.insert(t, c);
                    self.order.push(t);
                }
                Node::Folder(f) => {
                    let Some(folder) = p.folders.get(&f) else {
                        continue;
                    };
                    let e = self.id();
                    self.folder_el.insert(f, e.clone());
                    if let Some(b) = folder.bus.filter(|b| self.folded.get(b) == Some(&f)) {
                        self.track_el.insert(b, e);
                        let c = self.id();
                        self.channel.insert(b, c);
                        self.order.push(b);
                    }
                    self.assign(&folder.children);
                }
            }
        }
    }

    // ---- the transport -----------------------------------------------------

    fn transport(&mut self) -> El {
        let p = self.p;
        let first = p.tempo.first().map_or(120.0, |t| t.bpm);
        let tempo_id = self.id();
        self.tempo_id = tempo_id.clone();
        let (num_, den) = p.timesig.first().map_or((4, 4), |t| (t.num, t.den));
        let ts_id = self.id();
        self.meter_id = ts_id.clone();
        El::new("Transport")
            .k(El::new("Tempo")
                .a("max", num(MAX_BPM))
                .a("min", num(MIN_BPM))
                .a("unit", "bpm")
                .a("value", num(self.bpm(first)))
                .a("id", tempo_id)
                .a("name", "Tempo"))
            .k(El::new("TimeSignature")
                .a("denominator", den)
                .a("numerator", num_)
                .a("id", ts_id))
    }

    fn bpm(&mut self, bpm: f64) -> f64 {
        let v = bpm.clamp(MIN_BPM, MAX_BPM);
        if v != bpm {
            self.skips.count("範囲外のテンポ", 1, "");
        }
        v
    }

    // ---- the structure -----------------------------------------------------

    fn node(&mut self, n: Node) -> Option<El> {
        let p = self.p;
        match n {
            Node::Track(t) => {
                if self.folded.contains_key(&t) {
                    return None;
                }
                let t = p.tracks.get(&t)?;
                let (content, role) = match t.kind {
                    TrackKind::Audio => ("audio", "regular"),
                    TrackKind::Instrument => ("notes", "regular"),
                    TrackKind::Bus => ("audio", "effect"),
                    TrackKind::Master => return None,
                };
                Some(self.track(t, content, role))
            }
            Node::Folder(f) => {
                let folder = p.folders.get(&f)?;
                let mut el = El::new("Track")
                    .a("contentType", "tracks")
                    .a("loaded", true)
                    .a("id", self.folder_el[&f].clone())
                    .a("name", &folder.name)
                    .opt("color", folder.color.as_ref());
                if let Some(b) = folder.bus.filter(|b| self.folded.get(b) == Some(&f)) {
                    el.push(self.channel(&p.tracks[&b], "submix"));
                }
                for k in &folder.children {
                    if let Some(kid) = self.node(*k) {
                        el.push(kid);
                    }
                }
                Some(el)
            }
        }
    }

    fn track(&mut self, t: &Track, content: &str, role: &str) -> El {
        El::new("Track")
            .a("contentType", content)
            .a("loaded", true)
            .a("id", self.track_el[&t.id].clone())
            .a("name", &t.name)
            .opt("color", t.color.as_ref())
            .k(self.channel(t, role))
    }

    fn channel(&mut self, t: &Track, role: &str) -> El {
        let p = self.p;
        let dest = p.effective_output(t.id).map(|o| match o {
            Output::Master => self.channel[&TrackId::MASTER].clone(),
            Output::Bus(b) => self.channel.get(&b).cloned().unwrap_or_default(),
        });
        let mut el = El::new("Channel")
            .a("audioChannels", 2)
            .opt("destination", dest.filter(|d| !d.is_empty()))
            .a("role", role)
            .a("solo", t.solo)
            .a("id", self.channel[&t.id].clone());
        let mut devices = El::new("Devices");
        for x in &t.inserts {
            if let Some(d) = self.device(t, x) {
                devices.push(d);
            }
        }
        if !devices.is_empty() {
            el.push(devices);
        }
        let id = self.param(t.id, AutoTarget::Mute);
        el.push(
            El::new("Mute")
                .a("value", t.mute)
                .a("id", id)
                .a("name", "Mute"),
        );
        let id = self.param(t.id, AutoTarget::Pan);
        el.push(
            El::new("Pan")
                .a("max", 1)
                .a("min", 0)
                .a("unit", "normalized")
                .a("value", num((t.pan + 1.0) / 2.0))
                .a("id", id)
                .a("name", "Pan"),
        );
        if !t.sends.is_empty() {
            let mut sends = El::new("Sends");
            for s in &t.sends {
                let Some(dest) = self.channel.get(&s.to).cloned() else {
                    continue;
                };
                let level = linear(s.muted_level.unwrap_or(s.level_db));
                let enable = self.id();
                let volume = self.param(t.id, AutoTarget::Send(s.id));
                let send_id = self.id();
                sends.push(
                    El::new("Send")
                        .a("destination", dest)
                        .a("type", if s.pre { "pre" } else { "post" })
                        .a("id", send_id)
                        .k(El::new("Enable")
                            .a("value", s.muted_level.is_none())
                            .a("id", enable)
                            .a("name", "On/Off"))
                        .k(El::new("Volume")
                            .a("max", num(level.max(2.0)))
                            .a("min", 0)
                            .a("unit", "linear")
                            .a("value", num(level))
                            .a("id", volume)
                            .a("name", "Volume")),
                );
            }
            el.push(sends);
        }
        let v = linear(t.volume_db);
        let id = self.param(t.id, AutoTarget::Volume);
        el.push(
            El::new("Volume")
                .a("max", num(v.max(2.0)))
                .a("min", 0)
                .a("unit", "linear")
                .a("value", num(v))
                .a("id", id)
                .a("name", "Volume"),
        );
        el
    }

    /// A fresh id for a parameter an automation lane can point at.
    fn param(&mut self, track: TrackId, target: AutoTarget) -> String {
        let id = self.id();
        self.params.insert((track, target), id.clone());
        id
    }

    // ---- devices -----------------------------------------------------------

    fn device(&mut self, t: &Track, x: &Plugin) -> Option<El> {
        // The sidechain's key routing has no DAWproject shape yet (MIX-047);
        // name it rather than dropping it in silence.
        if x.sidechain
            .as_ref()
            .is_some_and(|sc| !sc.sources.is_empty())
        {
            self.skips
                .item(format!("{} の {} のサイドチェーン", t.name, x.name));
        }
        if let Some(def) = x.builtin() {
            return self.builtin(t, x, def);
        }
        let tag = match x.format.to_ascii_lowercase().as_str() {
            "vst3" => "Vst3Plugin",
            "clap" => "ClapPlugin",
            _ => {
                self.skips.item(format!("{} の {}", t.name, x.name));
                return None;
            }
        };
        let role = match x.kind {
            PluginKind::Instrument => "instrument",
            PluginKind::Effect => "audioFX",
        };
        let id = self.id();
        let mut el = El::new(tag)
            .a("deviceID", &x.uid)
            .a("deviceName", &x.name)
            .a("deviceRole", role)
            .a("loaded", true)
            .a("id", id)
            .a("name", &x.name);
        let mut keys: BTreeSet<&str> = x.params.keys().map(String::as_str).collect();
        for l in &t.automation {
            if let AutoTarget::Param(pid, k) = &l.target
                && *pid == x.id
            {
                keys.insert(k);
            }
        }
        let mut params = El::new("Parameters");
        for k in keys {
            let Ok(n) = k.parse::<u32>() else {
                self.skips
                    .item(format!("{} の {} のパラメーター {k}", t.name, x.name));
                continue;
            };
            let id = self.param(t.id, AutoTarget::Param(x.id, k.to_string()));
            params.push(
                El::new("RealParameter")
                    .a("max", 1)
                    .a("min", 0)
                    .a("unit", "normalized")
                    .opt("value", x.params.get(k).map(|v| num(*v)))
                    .a("id", id)
                    .a("parameterID", n as i32),
            );
        }
        if !params.is_empty() {
            el.push(params);
        }
        el.push(self.enabled(x));
        if let Some(text) = &x.state {
            match vstpreset::from_text(text) {
                Some(s) if tag == "Vst3Plugin" => {
                    let path = format!("plugins/{}.vstpreset", x.id);
                    let data = vstpreset::encode(&x.uid, &s);
                    el.push(El::new("State").a("path", &path));
                    self.files.push((path, Source::Bytes(data)));
                }
                Some(s) => {
                    let path = format!("plugins/{}.clap-preset", x.id);
                    el.push(El::new("State").a("path", &path));
                    self.files.push((path, Source::Bytes(s.component)));
                }
                None => self.skips.item(format!("{} の {} の状態", t.name, x.name)),
            }
        }
        Some(el)
    }

    fn enabled(&mut self, x: &Plugin) -> El {
        El::new("Enabled")
            .a("value", x.runs())
            .a("id", self.id())
            .a("name", "On/Off")
    }

    fn builtin(
        &mut self,
        t: &Track,
        x: &Plugin,
        def: &'static xirius_params::BuiltinDef,
    ) -> Option<El> {
        let tag = match def.uid {
            "eq" => "Equalizer",
            "compressor" => "Compressor",
            "limiter" => "Limiter",
            _ => {
                self.skips.item(format!("{} の {}", t.name, x.name));
                return None;
            }
        };
        let norm = |key: &str| -> f64 {
            let (_, spec) = def.param(key).expect("a parameter of the catalogue");
            x.params
                .get(spec.id)
                .copied()
                .unwrap_or_else(|| spec.default_norm())
        };
        let plain = |key: &str| -> f64 {
            let (_, spec) = def.param(key).expect("a parameter of the catalogue");
            spec.to_plain(norm(key))
        };
        let id = self.id();
        let mut el = El::new(tag)
            .a("deviceName", &x.name)
            .a("deviceRole", "audioFX")
            .a("loaded", true)
            .a("id", id)
            .a("name", &x.name);
        el.push(self.enabled(x));
        match def.uid {
            "eq" => {
                let order = |key: &str| match plain(key) as i64 {
                    0 => 2,
                    2 => 8,
                    _ => 4,
                };
                let bands: [Band; 6] = [
                    (
                        "highPass",
                        Some(order("low_cut_slope")),
                        "low_cut_freq",
                        None,
                        None,
                        Some("low_cut_on"),
                    ),
                    ("lowShelf", None, "low_freq", Some("low_gain"), None, None),
                    (
                        "bell",
                        None,
                        "mid1_freq",
                        Some("mid1_gain"),
                        Some("mid1_q"),
                        None,
                    ),
                    (
                        "bell",
                        None,
                        "mid2_freq",
                        Some("mid2_gain"),
                        Some("mid2_q"),
                        None,
                    ),
                    (
                        "highShelf",
                        None,
                        "high_freq",
                        Some("high_gain"),
                        None,
                        None,
                    ),
                    (
                        "lowPass",
                        Some(order("high_cut_slope")),
                        "high_cut_freq",
                        None,
                        None,
                        Some("high_cut_on"),
                    ),
                ];
                for (kind, ord, freq, gain, q, on) in bands {
                    let mut band = El::new("Band")
                        .a("type", kind)
                        .opt("order", ord)
                        .k(self.knob(t, x, def, freq, "Freq", "hertz", 1.0));
                    if let Some(g) = gain {
                        band.push(self.knob(t, x, def, g, "Gain", "decibel", 1.0));
                    }
                    if let Some(q) = q {
                        band.push(self.knob(t, x, def, q, "Q", "linear", 1.0));
                    }
                    let enabled = on.is_none_or(|k| plain(k) >= 0.5);
                    band.push(El::new("Enabled").a("value", enabled).a("id", self.id()));
                    el.push(band);
                }
                el.push(self.knob(t, x, def, "output", "OutputGain", "decibel", 1.0));
            }
            "compressor" => {
                el.push(self.knob(t, x, def, "attack", "Attack", "seconds", 0.001));
                el.push(El::new("AutoMakeup").a("value", false).a("id", self.id()));
                el.push(self.real("InputGain", "decibel", 0.0));
                el.push(self.knob(t, x, def, "makeup", "OutputGain", "decibel", 1.0));
                el.push(self.knob(t, x, def, "ratio", "Ratio", "linear", 1.0));
                el.push(self.knob(t, x, def, "release", "Release", "seconds", 0.001));
                el.push(self.knob(t, x, def, "threshold", "Threshold", "decibel", 1.0));
                for key in ["knee", "mix"] {
                    let (_, spec) = def.param(key).expect("a parameter of the catalogue");
                    if (norm(key) - spec.default_norm()).abs() > 1e-6 {
                        self.skips
                            .item(format!("{} の {} の{}", t.name, x.name, spec.label));
                    }
                }
            }
            _ => {
                el.push(self.knob(t, x, def, "input", "InputGain", "decibel", 1.0));
                el.push(self.knob(t, x, def, "release", "Release", "seconds", 0.001));
                el.push(self.knob(t, x, def, "ceiling", "Threshold", "decibel", 1.0));
            }
        }
        Some(el)
    }

    /// A parameter of a built-in in its plain unit, which an automation lane can point at.
    #[allow(clippy::too_many_arguments)]
    fn knob(
        &mut self,
        t: &Track,
        x: &Plugin,
        def: &'static xirius_params::BuiltinDef,
        key: &'static str,
        name: &'static str,
        unit: &'static str,
        scale: f64,
    ) -> El {
        let (_, spec) = def.param(key).expect("a parameter of the catalogue");
        let n = x
            .params
            .get(spec.id)
            .copied()
            .unwrap_or_else(|| spec.default_norm());
        let id = self.param(t.id, AutoTarget::Param(x.id, key.to_string()));
        self.plain.insert(id.clone(), (spec, scale, unit));
        El::new(name)
            .a("unit", unit)
            .a("value", num(spec.to_plain(n) * scale))
            .a("id", id)
    }

    fn real(&mut self, name: &'static str, unit: &str, value: f64) -> El {
        El::new(name)
            .a("unit", unit)
            .a("value", num(value))
            .a("id", self.id())
    }

    // ---- the arrangement ---------------------------------------------------

    fn arrangement(&mut self) -> El {
        let p = self.p;
        let arrangement_id = self.id();
        let lanes_id = self.id();
        let mut lanes = El::new("Lanes").a("timeUnit", "beats").a("id", lanes_id);
        for tid in self.order.clone() {
            let Some(t) = p.tracks.get(&tid) else {
                continue;
            };
            if let Some(l) = self.track_lanes(t) {
                lanes.push(l);
            }
        }
        let mut el = El::new("Arrangement").a("id", arrangement_id).k(lanes);
        if let Some(m) = self.markers() {
            el.push(m);
        }
        if p.tempo.len() > 1 {
            let target = self.tempo_id.clone();
            let mut pts = El::new("TempoAutomation")
                .a("id", self.id())
                .a("unit", "bpm")
                .k(El::new("Target").a("parameter", target));
            for t in &p.tempo {
                let bpm = self.bpm(t.bpm);
                pts.push(
                    El::new("RealPoint")
                        .a("time", self.beats(t.tick))
                        .a("value", num(bpm))
                        .a("interpolation", if t.ramp { "linear" } else { "hold" }),
                );
            }
            el.push(pts);
        }
        if p.timesig.len() > 1 {
            let target = self.meter_id.clone();
            let map = p.time_map();
            let mut pts = El::new("TimeSignatureAutomation")
                .a("id", self.id())
                .k(El::new("Target").a("parameter", target));
            for s in &p.timesig {
                pts.push(
                    El::new("TimeSignaturePoint")
                        .a("time", self.beats(map.bar_start(s.bar)))
                        .a("numerator", s.num)
                        .a("denominator", s.den),
                );
            }
            el.push(pts);
        }
        el
    }

    fn markers(&mut self) -> Option<El> {
        let p = self.p;
        let mut all: Vec<(Tick, &str, Option<&String>)> = p
            .markers
            .iter()
            .map(|m| (m.tick, m.name.as_str(), m.color.as_ref()))
            .chain(
                p.sections
                    .iter()
                    .map(|s| (s.start, s.name.as_str(), s.color.as_ref())),
            )
            .collect();
        if all.is_empty() {
            return None;
        }
        all.sort_by_key(|m| m.0);
        self.skips.count("セクションの長さ", p.sections.len(), "");
        let mut el = El::new("Markers").a("id", self.id());
        for (tick, name, color) in all {
            el.push(
                El::new("Marker")
                    .a("time", self.beats(tick))
                    .a("name", name)
                    .opt("color", color),
            );
        }
        Some(el)
    }

    fn track_lanes(&mut self, t: &Track) -> Option<El> {
        let mut el = El::new("Lanes")
            .a("track", self.track_el[&t.id].clone())
            .a("id", self.id());
        let mut clips = El::new("Clips").a("id", self.id());
        let mut sorted: Vec<&Clip> = t.clips.iter().collect();
        sorted.sort_by_key(|c| c.start);
        for c in sorted {
            let clip = match &c.body {
                ClipBody::Midi { .. } => Some(self.midi_clip(t, c)),
                ClipBody::Audio {
                    file,
                    offset,
                    gain_db,
                    process,
                } => self.audio_clip(t, c, file, *offset, *gain_db, process),
            };
            if let Some(clip) = clip {
                clips.push(clip);
            }
        }
        if !clips.is_empty() {
            el.push(clips);
        }
        let mut curved = 0;
        for l in &t.automation {
            if let Some(pts) = self.auto_points(t, l, &mut curved) {
                el.push(pts);
            }
        }
        self.skips.count(
            format!("{} のオートメーションの曲線", t.name),
            curved,
            "か所",
        );
        (!el.is_empty()).then_some(el)
    }

    fn clip_el(&self, c: &Clip) -> El {
        El::new("Clip")
            .a("time", self.beats(c.start))
            .a("duration", self.beats(c.length))
            .a("playStart", 0)
            .a("name", &c.name)
            .opt("color", c.color.as_ref())
            .opt("enable", c.muted.then_some(false))
    }

    fn midi_clip(&mut self, t: &Track, c: &Clip) -> El {
        let mut notes = El::new("Notes").a("id", self.id());
        let mut muted = 0;
        let mut outside = 0;
        // A DAWproject clip has nowhere to keep a transpose: the notes go out as they play
        // (UI-311). A track's transpose is added there too (UI-312). A note the transposes take
        // off the keyboard is left out, as it is not played either.
        let by = i32::from(c.transpose()) + i32::from(t.transpose);
        let mut sorted: Vec<_> = c.notes().iter().collect();
        sorted.sort_by_key(|n| (n.start, n.pitch));
        for n in sorted {
            if n.muted {
                muted += 1;
                continue;
            }
            let key = i32::from(n.pitch) + by;
            if !(0..=127).contains(&key) {
                outside += 1;
                continue;
            }
            let vel = crate::model::track_velocity(n.velocity, t.velocity);
            notes.push(
                El::new("Note")
                    .a("time", self.beats(n.start))
                    .a("duration", self.beats(n.length))
                    .a("channel", 0)
                    .a("key", key)
                    .a("vel", num(f64::from(vel) / 127.0)),
            );
        }
        self.skips.count("ミュートしたノート", muted, "");
        self.skips.count("キーボードの外のノート", outside, "");
        let lanes: Vec<El> = c
            .controllers
            .iter()
            .filter(|l| !l.points.is_empty())
            .map(|l| self.ctrl_points(l.kind, &l.points))
            .collect();
        let el = self.clip_el(c);
        if lanes.is_empty() {
            return el.k(notes);
        }
        let mut inner = El::new("Lanes").a("id", self.id()).k(notes);
        for l in lanes {
            inner.push(l);
        }
        el.k(inner)
    }

    fn ctrl_points(&mut self, kind: CtrlKind, points: &[crate::model::CtrlPoint]) -> El {
        let target = match kind {
            CtrlKind::Cc(n) => El::new("Target")
                .a("expression", "channelController")
                .a("channel", 0)
                .a("controller", n),
            CtrlKind::PitchBend => El::new("Target")
                .a("expression", "pitchBend")
                .a("channel", 0),
            CtrlKind::Aftertouch => El::new("Target")
                .a("expression", "channelPressure")
                .a("channel", 0),
        };
        let mut el = El::new("Points")
            .a("id", self.id())
            .a("unit", "normalized")
            .k(target);
        for pt in points {
            let v = match kind {
                // The centre (0) is 0.5.
                CtrlKind::PitchBend => (f64::from(pt.value) + 8191.5) / 16383.0,
                _ => f64::from(pt.value) / 127.0,
            };
            el.push(
                El::new("RealPoint")
                    .a("time", self.beats(pt.tick))
                    .a("value", num(v.clamp(0.0, 1.0)))
                    .a(
                        "interpolation",
                        match pt.shape {
                            CtrlShape::Step => "hold",
                            CtrlShape::Linear => "linear",
                        },
                    ),
            );
        }
        el
    }

    fn audio_clip(
        &mut self,
        t: &Track,
        c: &Clip,
        file: &str,
        offset: Tick,
        gain_db: f64,
        process: &AudioProcess,
    ) -> Option<El> {
        let name = Path::new(file)
            .file_name()
            .map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned());
        let path = self.source(file);
        let info = path.as_deref().and_then(|p| (self.opts.audio)(p));
        let (Some(path), Some(info)) = (path, info) else {
            self.skips.item(format!("見つからないファイル {name}"));
            return None;
        };
        if gain_db != 0.0 {
            self.skips.item(format!("{} のクリップのゲイン", t.name));
        }
        if process.reverse {
            self.skips.item(format!("{} の逆再生", t.name));
        }
        if process.semis != 0 || process.cents != 0 {
            self.skips.item(format!("{} のピッチ", t.name));
        }
        let fx = self.p.resolve_fx(t, c);
        if !fx.points.is_empty() {
            self.skips
                .item(format!("{} のクリップのゲインの線", t.name));
        }
        if (fx.fade_in.length > 0 && fx.fade_in.curve != 0.0)
            || (fx.fade_out.length > 0 && fx.fade_out.curve != 0.0)
        {
            self.skips.item(format!("{} のフェードの曲線", t.name));
        }

        let file_el = if self.opts.embed_audio {
            El::new("File").a("path", self.embedded(&path, &name))
        } else {
            El::new("File")
                .a("path", path.to_string_lossy().replace('\\', "/"))
                .a("external", true)
        };
        let audio = El::new("Audio")
            .a("algorithm", "stretch")
            .a("channels", info.channels)
            .a("duration", num(info.seconds))
            .a("sampleRate", info.sample_rate)
            .a("id", self.id())
            .k(file_el);
        // A clip that follows the tempo is warped in beats; one that keeps its length in seconds.
        let unit = if process.follow { "beats" } else { "seconds" };
        let mut warps = El::new("Warps")
            .a("contentTimeUnit", "seconds")
            .a("timeUnit", unit)
            .a("id", self.id())
            .k(audio);
        for (time, content) in self.warps(c, offset, process) {
            warps.push(
                El::new("Warp")
                    .a("time", num(time))
                    .a("contentTime", num(content)),
            );
        }
        let mut el = self
            .clip_el(c)
            .a("contentTimeUnit", unit)
            .a("fadeTimeUnit", "beats");
        el = el
            .a("fadeInTime", self.beats(fx.fade_in.length.max(0)))
            .a("fadeOutTime", self.beats(fx.fade_out.length.max(0)));
        Some(el.k(warps))
    }

    /// Where the clip's file is on this machine.
    fn source(&self, file: &str) -> Option<PathBuf> {
        let f = Path::new(file);
        if f.is_absolute() {
            return Some(f.to_path_buf());
        }
        self.opts.song_dir.map(|d| d.join(f))
    }

    /// The name of the file inside the `.dawproject`, one per source.
    fn embedded(&mut self, path: &Path, name: &str) -> String {
        if let Some(n) = self.audio_names.get(path) {
            return n.clone();
        }
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
            _ => (name.to_string(), String::new()),
        };
        let mut zip_name = format!("audio/{name}");
        let mut k = 2;
        while self
            .audio_names
            .values()
            .any(|n| n.eq_ignore_ascii_case(&zip_name))
        {
            zip_name = format!("audio/{stem} ({k}){ext}");
            k += 1;
        }
        self.audio_names
            .insert(path.to_path_buf(), zip_name.clone());
        self.files
            .push((zip_name.clone(), Source::File(path.to_path_buf())));
        zip_name
    }

    /// The clip's warp points: beats from the clip's start against seconds of the source. The
    /// line between two points is straight, so there is one wherever the tempo changes, and one
    /// a beat while it glides.
    fn warps(&self, c: &Clip, offset: Tick, process: &AudioProcess) -> Vec<(f64, f64)> {
        let (start, end) = (c.start, c.end());
        let r = process.ratio(self.tm.bpm_at(start));
        let s0 = self.tm.seconds_at(start);
        let skip = self.tm.seconds_at(start + offset) - s0;
        let tempo = &self.p.tempo;
        let mut ticks = vec![start, end];
        for (i, t) in tempo.iter().enumerate() {
            let seg_end = tempo.get(i + 1).map_or(Tick::MAX, |n| n.tick);
            if t.tick > start && t.tick < end {
                ticks.push(t.tick);
            }
            if t.ramp && t.tick < end && seg_end > start {
                let step = self.p.ppq.max(1) as Tick;
                let mut k = start.max(t.tick);
                while k < end.min(seg_end) {
                    ticks.push(k);
                    k += step;
                }
            }
        }
        ticks.sort_unstable();
        ticks.dedup();
        ticks
            .into_iter()
            .map(|k| {
                let sec = self.tm.seconds_at(k) - s0;
                let time = if process.follow {
                    (k - start) as f64 / self.ppq
                } else {
                    sec
                };
                (time, (skip + sec) / r)
            })
            .collect()
    }

    fn auto_points(&mut self, t: &Track, l: &AutoLane, curved: &mut usize) -> Option<El> {
        if l.points.is_empty() {
            return None;
        }
        let Some(target) = self.params.get(&(t.id, l.target.clone())).cloned() else {
            let what = match &l.target {
                AutoTarget::Param(pid, _) => t
                    .inserts
                    .iter()
                    .find(|x| x.id == *pid)
                    .map_or_else(|| "プラグイン".to_string(), |x| x.name.clone()),
                AutoTarget::Send(_) => "センド".to_string(),
                other => other.to_string(),
            };
            self.skips
                .item(format!("{} の {what} のオートメーション", t.name));
            return None;
        };
        let plain = self.plain.get(&target).copied();
        let unit = match l.target {
            _ if plain.is_some() => plain.map(|p| p.2),
            AutoTarget::Volume | AutoTarget::Send(_) => Some("linear"),
            AutoTarget::Pan | AutoTarget::Param(..) => Some("normalized"),
            AutoTarget::Mute => None,
        };
        let mut el = El::new("Points")
            .a("id", self.id())
            .opt("unit", unit)
            .k(El::new("Target").a("parameter", target));
        for pt in l.points.iter() {
            let time = self.beats(pt.tick);
            if l.target == AutoTarget::Mute {
                el.push(
                    El::new("BoolPoint")
                        .a("time", time)
                        .a("value", pt.value >= 0.5),
                );
                continue;
            }
            let v = match l.target {
                _ if let Some((spec, scale, _)) = plain => spec.to_plain(pt.value) * scale,
                AutoTarget::Volume | AutoTarget::Send(_) => linear(pt.value),
                AutoTarget::Pan => (pt.value + 1.0) / 2.0,
                _ => pt.value,
            };
            if !pt.step && pt.curve != 0.0 {
                *curved += 1;
            }
            el.push(
                El::new("RealPoint")
                    .a("time", time)
                    .a("value", num(v))
                    .a("interpolation", if pt.step { "hold" } else { "linear" }),
            );
        }
        Some(el)
    }
}

/// A band of the EQ: its type and order, and the keys of its frequency, gain, Q and switch.
type Band = (
    &'static str,
    Option<i64>,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

/// A level in dB as a gain factor; the bottom of the fader is silence.
fn linear(db: f64) -> f64 {
    if db <= MIN_DB {
        0.0
    } else {
        10f64.powf(db / 20.0)
    }
}

fn folders_in_order(p: &Project, nodes: &[Node], out: &mut Vec<FolderId>) {
    for n in nodes {
        if let Node::Folder(f) = n
            && let Some(folder) = p.folders.get(f)
        {
            out.push(*f);
            folders_in_order(p, &folder.children, out);
        }
    }
}
