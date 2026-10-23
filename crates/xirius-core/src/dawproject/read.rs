//! A `.dawproject` as a new song (EXP-006).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use roxmltree::Node as XNode;

use super::zip::ZipReader;
use super::{Skips, vstpreset};
use crate::auto::{AutoLane, AutoPoint, AutoTarget};
use crate::id::*;
use crate::model::{
    AudioProcess, AuxSend, Clip, ClipBody, CtrlKind, CtrlLane, CtrlPoint, CtrlShape, Folder,
    MAX_DB, MIN_DB, Note, Output, Plugin, PluginIsolation, PluginKind, Project, TempoPoint,
    TimeSigPoint, Track, TrackKind,
};
use crate::tempo::TempoMap;
use crate::time::Tick;

/// How far a warp may sit off the straight line the clip plays, in seconds of the audio.
const WARP_SLACK: f64 = 0.002;

/// A song read from a `.dawproject`.
#[derive(Debug)]
pub struct Imported {
    pub project: Project,
    /// What the song could not take.
    pub skipped: Skips,
    /// The audio files taken out of the file, in `media_dir`.
    pub media: Vec<PathBuf>,
}

/// Reads the file at `path` as a new song, named after the file (its title when the name is
/// empty). The audio inside it goes to `media_dir`, which the clips then name.
pub fn import(path: &Path, media_dir: &Path) -> Result<Imported, String> {
    import_with(path, media_dir, &mut |_| {})
}

/// [`import`], telling `step` how far the file is read (0..=1) as it goes: by the files inside
/// it taken out.
pub fn import_with(
    path: &Path,
    media_dir: &Path,
    step: &mut dyn FnMut(f64),
) -> Result<Imported, String> {
    let f = fs::File::open(path).map_err(|e| format!("{} を開けません: {e}", path.display()))?;
    let mut zip = ZipReader::new(std::io::BufReader::new(f))?;
    let xml = zip
        .read("project.xml")?
        .ok_or_else(|| format!("{} は DAWproject ではありません", path.display()))?;
    let xml =
        String::from_utf8(xml).map_err(|_| "project.xml が UTF-8 ではありません".to_string())?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty());
    let name = match stem {
        Some(s) => s,
        None => zip
            .read("metadata.xml")
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|m| title_of(&m))
            .unwrap_or_else(|| "DAWproject".to_string()),
    };
    let total = zip.names().len().max(1) as f64;
    let mut read = 1.0;
    step(read / total);
    let got = read_project(
        &xml,
        &name,
        &mut |n| {
            let bytes = zip.read(n).ok().flatten();
            if bytes.is_some() {
                read += 1.0;
                step((read / total).min(1.0));
            }
            bytes
        },
        media_dir,
        path.parent(),
    )?;
    step(1.0);
    Ok(got)
}

/// Reads a `project.xml`. `file` gives the bytes of a file inside the `.dawproject` by its path.
pub fn import_from(
    xml: &str,
    name: &str,
    file: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    media_dir: &Path,
) -> Result<Imported, String> {
    read_project(xml, name, file, media_dir, None)
}

/// [`import_from`] for a file in `base`, against which the paths of audio outside it are read.
fn read_project(
    xml: &str,
    name: &str,
    file: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    media_dir: &Path,
    base: Option<&Path>,
) -> Result<Imported, String> {
    let doc =
        roxmltree::Document::parse(xml).map_err(|e| format!("project.xml を読めません: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "Project" {
        return Err("project.xml の中身が DAWproject ではありません".into());
    }
    let mut r = Reader {
        p: Project::new(name),
        skips: Skips::default(),
        file,
        media_dir,
        base,
        media: Vec::new(),
        media_names: HashMap::new(),
        owner: HashMap::new(),
        channel: HashMap::new(),
        params: HashMap::new(),
        dest: Vec::new(),
        send_dest: Vec::new(),
        tm: TempoMap::new(crate::time::DEFAULT_PPQ, &[]),
    };
    r.read(root)?;
    let Reader {
        p, skips, media, ..
    } = r;
    p.validate()
        .map_err(|e| format!("DAWproject から曲を作れません: {e}"))?;
    Ok(Imported {
        project: p,
        skipped: skips,
        media,
    })
}

fn title_of(metadata: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(metadata).ok()?;
    let t = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("Title"))?
        .text()?
        .trim()
        .to_string();
    (!t.is_empty()).then_some(t)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Unit {
    Beats,
    Seconds,
}

impl Unit {
    fn of(n: XNode, attr: &str) -> Option<Unit> {
        match n.attribute(attr)? {
            "beats" => Some(Unit::Beats),
            "seconds" => Some(Unit::Seconds),
            _ => None,
        }
    }
}

/// A timeline's clock: local time 0 sits at `base` beats or seconds of the song.
#[derive(Clone, Copy, Debug)]
struct Frame {
    unit: Unit,
    base: f64,
}

/// What an automation lane's target is in the song.
#[derive(Clone, Debug)]
enum Param {
    Volume(TrackId),
    Pan(TrackId, f64, f64),
    Mute(TrackId),
    Send(TrackId, SendId),
    /// A VST3 or CLAP parameter, normalised over `lo..hi`.
    Plugin(TrackId, PluginId, String, f64, f64),
    /// A parameter of a built-in, in its plain unit times `scale`.
    Builtin(TrackId, PluginId, &'static str, f64),
    Tempo,
    Meter,
    /// Something the song has no lane for; the name says what.
    Other(TrackId, String),
}

struct Reader<'a> {
    p: Project,
    skips: Skips,
    file: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
    media_dir: &'a Path,
    /// The folder of the `.dawproject`, against which the paths of audio outside it are read.
    base: Option<&'a Path>,
    media: Vec<PathBuf>,
    media_names: HashMap<String, PathBuf>,
    /// The track a `Track` or `Channel` element stands for.
    owner: HashMap<String, TrackId>,
    /// The track of a `Channel` element (the destinations of outputs and sends).
    channel: HashMap<String, TrackId>,
    params: HashMap<String, Param>,
    /// Outputs: the track and the channel it plays into.
    dest: Vec<(TrackId, String)>,
    /// Sends: the track, the send and the channel it goes to.
    send_dest: Vec<(TrackId, SendId, String)>,
    tm: TempoMap,
}

fn f(n: XNode, k: &str) -> Option<f64> {
    n.attribute(k)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn b(n: XNode, k: &str) -> Option<bool> {
    match n.attribute(k)? {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn kid<'a, 'i>(n: XNode<'a, 'i>, name: &str) -> Option<XNode<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(name))
}

/// The MIDI expression a `Points` draws (`channelController`, `pitchBend`, …), if it draws one.
fn expression<'a>(pts: XNode<'a, '_>) -> Option<&'a str> {
    kid(pts, "Target")?.attribute("expression")
}

/// What a clip plays: its timeline, or the one of another clip it is an alias of
/// (`reference`).
fn content_of<'a, 'i>(c: XNode<'a, 'i>) -> Option<XNode<'a, 'i>> {
    if let Some(k) = c.children().find(|n| n.is_element()) {
        return Some(k);
    }
    let id = c.attribute("reference")?;
    c.document()
        .descendants()
        .find(|n| n.is_element() && n.attribute("id") == Some(id))
}

fn kids<'a, 'i>(n: XNode<'a, 'i>, name: &'static str) -> impl Iterator<Item = XNode<'a, 'i>> {
    n.children().filter(move |c| c.has_tag_name(name))
}

/// A gain factor as dB on the fader's scale.
fn db_of(linear: f64) -> f64 {
    if linear <= 0.0 {
        MIN_DB
    } else {
        (20.0 * linear.log10()).clamp(MIN_DB, MAX_DB)
    }
}

/// A level parameter (`Volume`) as dB, whatever unit it is written in.
fn level_db(n: XNode, value: f64) -> f64 {
    match n.attribute("unit") {
        Some("decibel") => value.clamp(MIN_DB, MAX_DB),
        Some("normalized") => AutoTarget::Volume.from_norm(value),
        _ => db_of(value),
    }
}

/// A value over `min..max` (0..1 when they are not written) as 0..1.
fn norm(n: XNode, value: f64) -> f64 {
    let (lo, hi) = (f(n, "min").unwrap_or(0.0), f(n, "max").unwrap_or(1.0));
    if hi > lo {
        ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else {
        value.clamp(0.0, 1.0)
    }
}

/// A name a file can take on Windows as well.
fn file_name(s: &str) -> String {
    let base = s.rsplit(['/', '\\']).next().unwrap_or(s);
    let clean: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let clean = clean.trim().trim_end_matches('.').to_string();
    if clean.is_empty() {
        "audio".into()
    } else {
        clean
    }
}

impl Reader<'_> {
    fn ppq(&self) -> f64 {
        f64::from(self.p.ppq)
    }

    fn tick(&self, fr: Frame, t: f64) -> Tick {
        match fr.unit {
            Unit::Beats => ((fr.base + t) * self.ppq()).round() as Tick,
            Unit::Seconds => self.tm.tick_at((fr.base + t).max(0.0)),
        }
    }

    /// The clock of a timeline whose local time `play_start` sits at `origin`.
    fn frame(&self, origin: Tick, unit: Unit, play_start: f64) -> Frame {
        let base = match unit {
            Unit::Beats => origin as f64 / self.ppq(),
            Unit::Seconds => self.tm.seconds_at(origin),
        };
        Frame {
            unit,
            base: base - play_start,
        }
    }

    /// Seconds of the song at local time `t`, before its start too (where the audio of a clip
    /// that starts into it would begin).
    fn secs(&self, fr: Frame, t: f64) -> f64 {
        match fr.unit {
            Unit::Beats => self.tm.seconds_at(self.tick(fr, t)),
            Unit::Seconds => fr.base + t,
        }
    }

    /// The same clock read in another unit.
    fn reframe(&self, fr: Frame, unit: Unit) -> Frame {
        if unit == fr.unit {
            return fr;
        }
        self.frame(self.tick(fr, 0.0), unit, 0.0)
    }

    fn name(&self, t: TrackId) -> String {
        self.p
            .tracks
            .get(&t)
            .map_or_else(String::new, |t| t.name.clone())
    }

    fn track_mut(&mut self, t: TrackId) -> &mut Track {
        Arc::make_mut(self.p.tracks.get_mut(&t).expect("a track read before"))
    }

    fn read(&mut self, root: XNode) -> Result<(), String> {
        if let Some(t) = kid(root, "Transport") {
            self.transport(t);
        }
        if let Some(a) = kid(root, "Arrangement") {
            self.tempo_lane(a);
            self.meter_lane(a);
        }
        self.tm = TempoMap::from_project(&self.p);
        if let Some(s) = kid(root, "Structure") {
            let mut root_nodes = Vec::new();
            for t in s
                .children()
                .filter(|n| n.has_tag_name("Track") || n.has_tag_name("Channel"))
            {
                self.structure(t, &mut root_nodes);
            }
            self.p.root = root_nodes;
        }
        self.routing();
        if let Some(a) = kid(root, "Arrangement") {
            self.arrangement(a);
        }
        if let Some(s) = kid(root, "Scenes") {
            let n = kids(s, "Scene").count();
            self.skips.count("シーン", n, "");
        }
        Ok(())
    }

    // ---- the transport, tempo and meter --------------------------------------

    fn transport(&mut self, t: XNode) {
        if let Some(tempo) = kid(t, "Tempo") {
            if let Some(id) = tempo.attribute("id") {
                self.params.insert(id.into(), Param::Tempo);
            }
            if let Some(v) = f(tempo, "value") {
                self.p.tempo[0].bpm = v.clamp(20.0, 999.0);
            }
        }
        if let Some(ts) = kid(t, "TimeSignature") {
            if let Some(id) = ts.attribute("id") {
                self.params.insert(id.into(), Param::Meter);
            }
            self.set_meter(1, ts);
        }
    }

    fn set_meter(&mut self, bar: u32, n: XNode) {
        let num = n.attribute("numerator").and_then(|v| v.parse::<u32>().ok());
        let den = n
            .attribute("denominator")
            .and_then(|v| v.parse::<u32>().ok());
        let (Some(num), Some(den)) = (num, den) else {
            return;
        };
        if !(1..=64).contains(&num) || ![1, 2, 4, 8, 16, 32].contains(&den) {
            self.skips.item(format!("拍子 {num}/{den}"));
            return;
        }
        if let Some(t) = self.p.timesig.iter_mut().find(|t| t.bar == bar) {
            t.num = num;
            t.den = den;
            return;
        }
        let id = TimeSigId(self.p.alloc());
        self.p.timesig.push(TimeSigPoint { id, bar, num, den });
    }

    fn tempo_lane(&mut self, a: XNode) {
        let Some(lane) = kid(a, "TempoAutomation") else {
            return;
        };
        let unit = Unit::of(lane, "timeUnit").unwrap_or(Unit::Beats);
        let mut pts: Vec<(f64, f64, bool)> = kids(lane, "RealPoint")
            .filter_map(|pt| {
                let ramp = pt.attribute("interpolation") == Some("linear");
                Some((f(pt, "time")?, f(pt, "value")?, ramp))
            })
            .collect();
        pts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out: Vec<TempoPoint> = Vec::new();
        for (time, bpm, ramp) in pts {
            let tick = match unit {
                Unit::Beats => (time * self.ppq()).round() as Tick,
                Unit::Seconds => TempoMap::new(self.p.ppq, &out).tick_at(time.max(0.0)),
            }
            .max(0);
            let bpm = bpm.clamp(1.0, 999.0);
            if let Some(last) = out.last_mut()
                && last.tick >= tick
            {
                last.bpm = bpm;
                last.ramp = ramp;
                continue;
            }
            if out.is_empty() && tick > 0 {
                let first = self.p.tempo[0].clone();
                out.push(TempoPoint {
                    ramp: false,
                    bpm,
                    ..first
                });
            }
            let id = if out.is_empty() {
                self.p.tempo[0].id
            } else {
                TempoId(self.p.alloc())
            };
            out.push(TempoPoint {
                id,
                tick,
                bpm,
                ramp,
            });
        }
        if !out.is_empty() {
            self.p.tempo = out;
        }
    }

    fn meter_lane(&mut self, a: XNode) {
        let Some(lane) = kid(a, "TimeSignatureAutomation") else {
            return;
        };
        let unit = Unit::of(lane, "timeUnit").unwrap_or(Unit::Beats);
        let mut pts: Vec<(f64, XNode)> = kids(lane, "TimeSignaturePoint")
            .filter_map(|pt| Some((f(pt, "time")?, pt)))
            .collect();
        pts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let tm = TempoMap::from_project(&self.p);
        let mut moved = 0;
        for (time, pt) in pts {
            let tick = match unit {
                Unit::Beats => (time * self.ppq()).round() as Tick,
                Unit::Seconds => tm.tick_at(time.max(0.0)),
            };
            let (bar, beat, rest) = self.p.time_map().to_bbt(tick);
            let bar = if beat == 1 && rest == 0 {
                bar
            } else {
                moved += 1;
                bar + 1
            };
            self.set_meter(bar, pt);
        }
        self.p.timesig.sort_by_key(|t| t.bar);
        self.skips.count("小節の途中の拍子の変更", moved, "");
    }

    // ---- the structure -------------------------------------------------------

    fn structure(&mut self, el: XNode, out: &mut Vec<Node>) {
        let content = el.attribute("contentType").unwrap_or("");
        // Structure can contain mixer channels without a surrounding Track.
        let standalone = el.has_tag_name("Channel");
        let channel = if standalone {
            Some(el)
        } else {
            kid(el, "Channel")
        };
        let role = channel
            .and_then(|c| c.attribute("role"))
            .unwrap_or("regular");
        let name = el.attribute("name").unwrap_or("").to_string();
        let color = el.attribute("color").map(str::to_string);
        let children: Vec<XNode> = kids(el, "Track").collect();
        if role == "master" {
            if let Some(id) = el.attribute("id") {
                self.owner.insert(id.into(), TrackId::MASTER);
            }
            if let Some(c) = channel {
                self.channel_values(TrackId::MASTER, c);
            }
            for k in children {
                self.structure(k, out);
            }
            return;
        }
        if !children.is_empty() || content.split_whitespace().any(|w| w == "tracks") {
            let fid = FolderId(self.p.alloc());
            let mut kids_out = Vec::new();
            for k in children {
                self.structure(k, &mut kids_out);
            }
            let bus = channel.map(|c| {
                let tid = self.new_track(TrackKind::Bus, &name, color.clone());
                if let Some(id) = el.attribute("id") {
                    self.owner.insert(id.into(), tid);
                }
                self.channel_values(tid, c);
                tid
            });
            self.p.folders.insert(
                fid,
                Folder {
                    id: fid,
                    name: if name.is_empty() {
                        "フォルダー".into()
                    } else {
                        name
                    },
                    color,
                    bus,
                    children: kids_out,
                },
            );
            out.push(Node::Folder(fid));
            if let Some(b) = bus {
                out.push(Node::Track(b));
            }
            return;
        }
        let kind = if standalone || matches!(role, "effect" | "submix") {
            TrackKind::Bus
        } else if content.split_whitespace().any(|w| w == "notes") {
            TrackKind::Instrument
        } else {
            TrackKind::Audio
        };
        let tid = self.new_track(kind, &name, color);
        if let Some(id) = el.attribute("id") {
            self.owner.insert(id.into(), tid);
        }
        if let Some(c) = channel {
            self.channel_values(tid, c);
        }
        out.push(Node::Track(tid));
    }

    fn new_track(&mut self, kind: TrackKind, name: &str, color: Option<String>) -> TrackId {
        let id = TrackId(self.p.alloc());
        let name = if name.trim().is_empty() {
            match kind {
                TrackKind::Bus => "バス",
                TrackKind::Instrument => "インストゥルメント",
                _ => "オーディオ",
            }
            .to_string()
        } else {
            name.to_string()
        };
        let mut t = Track::new(id, kind, name);
        t.color = color.or_else(|| Some(self.p.next_track_color()));
        self.p.tracks.insert(id, Arc::new(t));
        id
    }

    fn channel_values(&mut self, tid: TrackId, c: XNode) {
        if let Some(id) = c.attribute("id") {
            self.owner.insert(id.into(), tid);
            self.channel.insert(id.into(), tid);
        }
        if let Some(d) = c.attribute("destination") {
            self.dest.push((tid, d.to_string()));
        }
        let solo = b(c, "solo").unwrap_or(false);
        let mut vol = None;
        let mut pan = None;
        let mut mute = None;
        if let Some(v) = kid(c, "Volume") {
            if let Some(id) = v.attribute("id") {
                self.params.insert(id.into(), Param::Volume(tid));
            }
            vol = f(v, "value").map(|x| level_db(v, x));
        }
        if let Some(v) = kid(c, "Pan") {
            if let Some(id) = v.attribute("id") {
                let (lo, hi) = (f(v, "min").unwrap_or(0.0), f(v, "max").unwrap_or(1.0));
                self.params.insert(id.into(), Param::Pan(tid, lo, hi));
            }
            pan = f(v, "value").map(|x| norm(v, x) * 2.0 - 1.0);
        }
        if let Some(v) = kid(c, "Mute") {
            if let Some(id) = v.attribute("id") {
                self.params.insert(id.into(), Param::Mute(tid));
            }
            mute = b(v, "value");
        }
        {
            let t = self.track_mut(tid);
            t.solo = solo && t.kind != TrackKind::Master;
            if let Some(v) = vol {
                t.volume_db = v;
            }
            if let Some(v) = pan {
                t.pan = v.clamp(-1.0, 1.0);
            }
            if let Some(v) = mute {
                t.mute = v;
            }
        }
        if let Some(s) = kid(c, "Sends") {
            for send in kids(s, "Send") {
                self.send(tid, send);
            }
        }
        if let Some(d) = kid(c, "Devices") {
            for dev in d.children().filter(|n| n.is_element()) {
                self.device(tid, dev);
            }
            self.order_instrument(tid);
        }
    }

    fn send(&mut self, tid: TrackId, s: XNode) {
        let name = self.name(tid);
        let Some(dest) = s.attribute("destination") else {
            self.skips.item(format!("{name} のセンド"));
            return;
        };
        if tid.is_master() {
            self.skips.item(format!("{name} のセンド"));
            return;
        }
        let sid = SendId(self.p.alloc());
        let level = kid(s, "Volume")
            .map(|v| {
                if let Some(id) = v.attribute("id") {
                    self.params.insert(id.into(), Param::Send(tid, sid));
                }
                f(v, "value").map_or(0.0, |x| level_db(v, x))
            })
            .unwrap_or(0.0);
        let on = kid(s, "Enable").and_then(|e| b(e, "value")).unwrap_or(true);
        if let Some(p) = kid(s, "Pan")
            && f(p, "value").is_some_and(|x| (norm(p, x) - 0.5).abs() > 1e-6)
        {
            self.skips.item(format!("{name} のセンドのパン"));
        }
        self.track_mut(tid).sends.push(AuxSend {
            id: sid,
            to: TrackId::MASTER,
            level_db: if on { level } else { MIN_DB },
            pre: s.attribute("type") == Some("pre"),
            muted_level: (!on).then_some(level),
        });
        self.send_dest.push((tid, sid, dest.to_string()));
    }

    /// Outputs and sends, now that every channel is known.
    fn routing(&mut self) {
        for (tid, sid, dest) in std::mem::take(&mut self.send_dest) {
            let to = self
                .channel
                .get(&dest)
                .copied()
                .filter(|t| *t != tid && self.p.tracks[t].kind == TrackKind::Bus);
            let name = self.name(tid);
            let t = self.track_mut(tid);
            match to {
                Some(to) => {
                    if let Some(s) = t.sends.iter_mut().find(|s| s.id == sid) {
                        s.to = to;
                    }
                }
                None => {
                    t.sends.retain(|s| s.id != sid);
                    self.skips.item(format!("{name} のセンド"));
                }
            }
        }
        for (tid, dest) in std::mem::take(&mut self.dest) {
            if tid.is_master() {
                continue;
            }
            let want = match self.channel.get(&dest).copied() {
                Some(TrackId::MASTER) => Output::Master,
                Some(b) if b != tid && self.p.tracks[&b].kind == TrackKind::Bus => Output::Bus(b),
                _ => {
                    self.skips.item(format!("{} の出力先", self.name(tid)));
                    continue;
                }
            };
            if self.p.effective_output(tid) != Some(want) {
                self.track_mut(tid).output = Some(want);
            }
        }
        if self.p.check_routing().is_err() {
            for t in self.p.tracks.values_mut() {
                let t = Arc::make_mut(t);
                t.output = None;
                t.sends.clear();
            }
            for f in self.p.folders.values_mut() {
                f.bus = None;
            }
            self.skips.item("出力とセンドの経路");
        }
    }

    // ---- devices -------------------------------------------------------------

    fn device(&mut self, tid: TrackId, d: XNode) {
        let tag = d.tag_name().name();
        let track = self.name(tid);
        let dev_name = d
            .attribute("deviceName")
            .or_else(|| d.attribute("name"))
            .unwrap_or(tag)
            .to_string();
        let def = match tag {
            "Equalizer" => xirius_params::find("eq"),
            "Compressor" => xirius_params::find("compressor"),
            "Limiter" => xirius_params::find("limiter"),
            _ => None,
        };
        if let Some(def) = def {
            self.builtin(tid, d, def);
            return;
        }
        let format = match tag {
            "Vst3Plugin" => "vst3",
            "ClapPlugin" => "clap",
            _ => {
                self.skips.item(format!("{track} の {dev_name}"));
                return;
            }
        };
        let kind = if d.attribute("deviceRole") == Some("instrument") {
            PluginKind::Instrument
        } else {
            PluginKind::Effect
        };
        let pid = PluginId(self.p.alloc());
        let mut x = Plugin {
            samples: Vec::new(),
            id: pid,
            name: dev_name.clone(),
            kind,
            format: format.into(),
            uid: device_id(format, d.attribute("deviceID").unwrap_or("")),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            isolation: PluginIsolation::default(),
            sidechain: None,
            preset: None,
        };
        if let Some(ps) = kid(d, "Parameters") {
            for prm in ps.children().filter(|n| n.is_element()) {
                let key = prm
                    .attribute("parameterID")
                    .and_then(|v| v.parse::<i64>().ok());
                let Some(key) = key.map(|k| (k as u32).to_string()) else {
                    continue;
                };
                let (lo, hi) = (f(prm, "min").unwrap_or(0.0), f(prm, "max").unwrap_or(1.0));
                let value = match prm.tag_name().name() {
                    "RealParameter" => f(prm, "value").map(|v| norm(prm, v)),
                    "BoolParameter" => b(prm, "value").map(|v| if v { 1.0 } else { 0.0 }),
                    _ => None,
                };
                if let Some(v) = value {
                    x.params.insert(key.clone(), v);
                }
                if let Some(id) = prm.attribute("id") {
                    self.params
                        .insert(id.into(), Param::Plugin(tid, pid, key, lo, hi));
                }
            }
        }
        x.bypass = !self.enabled(tid, d, &dev_name);
        if let Some(path) = kid(d, "State").and_then(|s| s.attribute("path")) {
            let state = (self.file)(path).and_then(|bytes| {
                if format == "vst3" {
                    // The preset names its plugin; a device without `deviceID` takes it.
                    let (class, s) = vstpreset::decode(&bytes)?;
                    if x.uid.is_empty() {
                        x.uid = device_id(format, &class);
                    }
                    Some(s)
                } else {
                    Some(vstpreset::State {
                        component: bytes,
                        controller: Vec::new(),
                    })
                }
            });
            match state {
                Some(s) => x.state = Some(vstpreset::to_text(&s)),
                None => self.skips.item(format!("{track} の {dev_name} の状態")),
            }
        }
        self.track_mut(tid).inserts.push(x);
    }

    /// The device's on/off; its lane is not kept.
    fn enabled(&mut self, tid: TrackId, d: XNode, name: &str) -> bool {
        let Some(e) = kid(d, "Enabled") else {
            return true;
        };
        if let Some(id) = e.attribute("id") {
            let what = format!("{name} のオン／オフ");
            self.params.insert(id.into(), Param::Other(tid, what));
        }
        b(e, "value").unwrap_or(true)
    }

    /// An instrument goes first on an instrument track; other tracks and a second one take none.
    fn order_instrument(&mut self, tid: TrackId) {
        let track = self.name(tid);
        let t = self.track_mut(tid);
        let is_inst = t.kind == TrackKind::Instrument;
        let mut kept = false;
        let mut dropped = Vec::new();
        t.inserts.retain(|x| {
            if x.kind != PluginKind::Instrument {
                return true;
            }
            if is_inst && !kept {
                kept = true;
                return true;
            }
            dropped.push(x.name.clone());
            false
        });
        if let Some(i) = t
            .inserts
            .iter()
            .position(|x| x.kind == PluginKind::Instrument)
        {
            let x = t.inserts.remove(i);
            t.inserts.insert(0, x);
        }
        for n in dropped {
            self.skips.item(format!("{track} の {n}"));
        }
    }

    fn builtin(&mut self, tid: TrackId, d: XNode, def: &'static xirius_params::BuiltinDef) {
        let track = self.name(tid);
        let pid = PluginId(self.p.alloc());
        let mut x = Plugin {
            samples: Vec::new(),
            id: pid,
            name: def.name.to_string(),
            kind: PluginKind::Effect,
            format: "builtin".into(),
            uid: def.uid.to_string(),
            bypass: false,
            deactivated: false,
            params: def.defaults().map(|(k, v)| (k.to_string(), v)).collect(),
            state: None,
            isolation: PluginIsolation::default(),
            sidechain: None,
            preset: None,
        };
        x.bypass = !self.enabled(tid, d, def.name);
        // (element, parameter, seconds to milliseconds)
        let set = |me: &mut Self,
                   ps: &mut BTreeMap<String, f64>,
                   el: Option<XNode>,
                   key: &'static str,
                   scale: f64| {
            let Some(el) = el else {
                return;
            };
            let (_, spec) = def.param(key).expect("a parameter of the catalogue");
            if let Some(id) = el.attribute("id") {
                me.params
                    .insert(id.into(), Param::Builtin(tid, pid, key, scale));
            }
            if let Some(v) = f(el, "value") {
                let plain = plain_of(el, v) * scale;
                ps.insert(
                    key.to_string(),
                    spec.to_norm(plain.clamp(spec.min, spec.max)),
                );
            }
        };
        let mut extra = Vec::new();
        match def.uid {
            "eq" => {
                let mut bells = 0;
                let (mut lo_cut, mut hi_cut, mut lo_shelf, mut hi_shelf) =
                    (false, false, false, false);
                x.params.insert("low_cut_on".into(), 0.0);
                x.params.insert("high_cut_on".into(), 0.0);
                for band in kids(d, "Band") {
                    let on = kid(band, "Enabled")
                        .and_then(|e| b(e, "value"))
                        .unwrap_or(true);
                    let kind = band.attribute("type").unwrap_or("");
                    let (freq, gain, q, flag, slope) = match kind {
                        "highPass" if !lo_cut => {
                            lo_cut = true;
                            (
                                "low_cut_freq",
                                None,
                                None,
                                Some("low_cut_on"),
                                Some("low_cut_slope"),
                            )
                        }
                        "lowPass" if !hi_cut => {
                            hi_cut = true;
                            (
                                "high_cut_freq",
                                None,
                                None,
                                Some("high_cut_on"),
                                Some("high_cut_slope"),
                            )
                        }
                        "lowShelf" if !lo_shelf => {
                            lo_shelf = true;
                            ("low_freq", Some("low_gain"), None, None, None)
                        }
                        "highShelf" if !hi_shelf => {
                            hi_shelf = true;
                            ("high_freq", Some("high_gain"), None, None, None)
                        }
                        "bell" if bells < 2 => {
                            bells += 1;
                            if bells == 1 {
                                ("mid1_freq", Some("mid1_gain"), Some("mid1_q"), None, None)
                            } else {
                                ("mid2_freq", Some("mid2_gain"), Some("mid2_q"), None, None)
                            }
                        }
                        _ => {
                            extra.push(format!("{track} の {} の {kind} のバンド", def.name));
                            continue;
                        }
                    };
                    set(self, &mut x.params, kid(band, "Freq"), freq, 1.0);
                    if let Some(g) = gain {
                        if on {
                            set(self, &mut x.params, kid(band, "Gain"), g, 1.0);
                        } else {
                            x.params
                                .insert(g.into(), def.param(g).unwrap().1.to_norm(0.0));
                        }
                    }
                    if let Some(q) = q {
                        set(self, &mut x.params, kid(band, "Q"), q, 1.0);
                    }
                    if let Some(flag) = flag {
                        x.params.insert(flag.into(), if on { 1.0 } else { 0.0 });
                    }
                    if let Some(slope) = slope {
                        let order = band.attribute("order").and_then(|o| o.parse::<u32>().ok());
                        let i = match order {
                            Some(0..=2) => 0.0,
                            Some(3..=5) | None => 1.0,
                            Some(_) => 2.0,
                        };
                        let spec = def.param(slope).unwrap().1;
                        x.params.insert(slope.into(), spec.to_norm(i));
                    }
                }
                set(self, &mut x.params, kid(d, "OutputGain"), "output", 1.0);
                if kid(d, "InputGain")
                    .and_then(|g| f(g, "value"))
                    .is_some_and(|v| v != 0.0)
                {
                    extra.push(format!("{track} の {} の入力のゲイン", def.name));
                }
            }
            "compressor" => {
                set(self, &mut x.params, kid(d, "Attack"), "attack", 1000.0);
                set(self, &mut x.params, kid(d, "Release"), "release", 1000.0);
                set(self, &mut x.params, kid(d, "Ratio"), "ratio", 1.0);
                set(self, &mut x.params, kid(d, "Threshold"), "threshold", 1.0);
                set(self, &mut x.params, kid(d, "OutputGain"), "makeup", 1.0);
                if kid(d, "InputGain")
                    .and_then(|g| f(g, "value"))
                    .is_some_and(|v| v != 0.0)
                {
                    extra.push(format!("{track} の {} の入力のゲイン", def.name));
                }
                if kid(d, "AutoMakeup").and_then(|g| b(g, "value")) == Some(true) {
                    extra.push(format!("{track} の {} の自動メイクアップ", def.name));
                }
            }
            _ => {
                set(self, &mut x.params, kid(d, "InputGain"), "input", 1.0);
                set(self, &mut x.params, kid(d, "Release"), "release", 1000.0);
                set(self, &mut x.params, kid(d, "Threshold"), "ceiling", 1.0);
                if kid(d, "Attack").is_some() {
                    extra.push(format!("{track} の {} のアタック", def.name));
                }
            }
        }
        for e in extra {
            self.skips.item(e);
        }
        self.track_mut(tid).inserts.push(x);
    }

    // ---- the arrangement -----------------------------------------------------

    fn arrangement(&mut self, a: XNode) {
        if let Some(lanes) = kid(a, "Lanes") {
            let unit = Unit::of(lanes, "timeUnit").unwrap_or(Unit::Beats);
            let fr = self.frame(0, unit, 0.0);
            let mut controllers = Vec::new();
            self.lanes(lanes, fr, None, &mut controllers);
            for (tid, pts, fr) in controllers {
                self.track_controllers(tid, pts, fr);
            }
        }
        if let Some(m) = kid(a, "Markers") {
            self.markers(m);
        }
    }

    /// A timeline of the arrangement. The controllers drawn on a track (`Points` of a MIDI
    /// expression) go to `controllers`, to be put in its clips once they are all read.
    fn lanes<'a, 'i>(
        &mut self,
        el: XNode<'a, 'i>,
        fr: Frame,
        track: Option<TrackId>,
        controllers: &mut Vec<(TrackId, XNode<'a, 'i>, Frame)>,
    ) {
        let fr = match Unit::of(el, "timeUnit") {
            Some(u) => self.reframe(fr, u),
            None => fr,
        };
        let Some(track) = self.track_of(el, track) else {
            return;
        };
        for k in el.children().filter(|n| n.is_element()) {
            let tag = k.tag_name().name();
            if matches!(tag, "Lanes" | "Markers") {
                if tag == "Lanes" {
                    self.lanes(k, fr, track, controllers);
                } else {
                    self.markers(k);
                }
                continue;
            }
            if !matches!(tag, "Clips" | "Notes" | "Points") {
                self.skips.item(format!("レーンの {tag}"));
                continue;
            }
            // Each timeline may name its own track (`<Clips track="…">` right under the
            // arrangement, as the reference library writes).
            let t = match self.track_of(k, track) {
                Some(Some(t)) => t,
                Some(None) => {
                    self.skips.item("トラックの無いレーン");
                    continue;
                }
                None => continue,
            };
            let fr = match Unit::of(k, "timeUnit") {
                Some(u) => self.reframe(fr, u),
                None => fr,
            };
            match tag {
                "Clips" => {
                    for c in kids(k, "Clip") {
                        self.clip(t, c, fr, (Tick::MIN, Tick::MAX), None);
                    }
                }
                "Notes" => self.loose_notes(t, k, fr),
                _ if expression(k).is_some() => controllers.push((t, k, fr)),
                _ => self.points(t, k, fr),
            }
        }
    }

    /// The track a timeline is on: its own `track`, or the one of the lanes around it. `None`
    /// when it names a track the file does not have (listed).
    fn track_of(&mut self, el: XNode, around: Option<TrackId>) -> Option<Option<TrackId>> {
        let Some(t) = el.attribute("track") else {
            return Some(around);
        };
        match self.owner.get(t) {
            Some(id) => Some(Some(*id)),
            None => {
                self.skips.item("トラックの無いレーン");
                None
            }
        }
    }

    /// A clip on the timeline `fr`, inside the window of the clips around it.
    fn clip(
        &mut self,
        tid: TrackId,
        c: XNode,
        fr: Frame,
        window: (Tick, Tick),
        outer: Option<&Outer>,
    ) {
        let Some(time) = f(c, "time") else {
            return;
        };
        let start = self.tick(fr, time);
        let end = f(c, "duration").map_or(window.1, |d| self.tick(fr, time + d));
        let (start, end) = (start.max(window.0).max(0), end.min(window.1));
        if end <= start {
            return;
        }
        let name = c.attribute("name").map(str::to_string);
        let color = c.attribute("color").map(str::to_string);
        let muted = b(c, "enable") == Some(false);
        let me = Outer {
            name: name.or_else(|| outer.and_then(|o| o.name.clone())),
            color: color.or_else(|| outer.and_then(|o| o.color.clone())),
            muted: muted || outer.is_some_and(|o| o.muted),
            fade_in: self
                .fade(c, "fadeInTime", start, fr)
                .or(outer.and_then(|o| o.fade_in)),
            fade_out: self
                .fade(c, "fadeOutTime", end, fr)
                .or(outer.and_then(|o| o.fade_out)),
        };
        let content_unit = content_of(c)
            .and_then(|n| Unit::of(n, "timeUnit"))
            .or_else(|| Unit::of(c, "contentTimeUnit"))
            .unwrap_or(fr.unit);
        let play_start = f(c, "playStart").unwrap_or(0.0);
        let origin = self.tick(fr, time);
        let inner = self.frame(origin, content_unit, play_start);
        if let (Some(ls), Some(le)) = (f(c, "loopStart"), f(c, "loopEnd")) {
            let len = f(c, "duration").unwrap_or(f64::MAX);
            let one = self.tick(inner, le) - self.tick(inner, ls);
            let span = self.tick(fr, time + len.min(1e9)) - origin;
            if le > ls && one + 1 < span && ls <= play_start + 1e-9 {
                self.skips
                    .item(format!("{} のループしたクリップ", self.name(tid)));
                let w_end = self.tick(inner, le).min(end);
                return self.clip_content(tid, c, inner, (start, w_end), &me);
            }
        }
        self.clip_content(tid, c, inner, (start, end), &me);
    }

    fn clip_content(&mut self, tid: TrackId, c: XNode, fr: Frame, w: (Tick, Tick), me: &Outer) {
        let Some(content) = content_of(c) else {
            return;
        };
        let fr = match Unit::of(content, "timeUnit") {
            Some(u) => self.reframe(fr, u),
            None => fr,
        };
        match content.tag_name().name() {
            "Clips" => {
                for k in kids(content, "Clip") {
                    self.clip(tid, k, fr, w, Some(me));
                }
            }
            "Notes" => self.midi_clip(tid, Some(content), None, None, fr, w, me),
            "Lanes" => {
                let notes = kid(content, "Notes");
                self.midi_clip(tid, notes, Some(content), None, fr, w, me);
            }
            "Points" if expression(content).is_some() => {
                self.midi_clip(tid, None, None, Some(content), fr, w, me);
            }
            "Points" => self.skips.item(format!(
                "{} のクリップの中のオートメーション",
                self.name(tid)
            )),
            "Warps" => self.audio_clip(tid, content, fr, w, me),
            "Audio" => self.audio_clip(tid, content, fr, w, me),
            other => self
                .skips
                .item(format!("{} の {other} のクリップ", self.name(tid))),
        }
    }

    fn fade(&self, c: XNode, attr: &str, at: Tick, fr: Frame) -> Option<Tick> {
        let v = f(c, attr).filter(|v| *v > 0.0)?;
        let unit = Unit::of(c, "fadeTimeUnit").unwrap_or(fr.unit);
        Some(match unit {
            Unit::Beats => (v * self.ppq()).round() as Tick,
            Unit::Seconds => {
                let s = self.tm.seconds_at(at);
                if attr == "fadeInTime" {
                    self.tm.tick_at(s + v) - at
                } else {
                    at - self.tm.tick_at((s - v).max(0.0))
                }
            }
        })
    }

    fn loose_notes(&mut self, tid: TrackId, notes: XNode, fr: Frame) {
        let times: Vec<(Tick, Tick)> = kids(notes, "Note")
            .filter_map(|n| {
                let t = f(n, "time")?;
                let d = f(n, "duration").unwrap_or(0.0);
                Some((self.tick(fr, t), self.tick(fr, t + d)))
            })
            .collect();
        let (Some(s), Some(e)) = (
            times.iter().map(|x| x.0).min(),
            times.iter().map(|x| x.1).max(),
        ) else {
            return;
        };
        let me = Outer::default();
        self.midi_clip(
            tid,
            Some(notes),
            None,
            None,
            fr,
            (s.max(0), e.max(s + 1)),
            &me,
        );
    }

    /// Controllers drawn on an instrument track: each clip they cross takes its part; with no
    /// clip on the track, a clip of their own holds them.
    fn track_controllers(&mut self, tid: TrackId, pts: XNode, fr: Frame) {
        let track = self.name(tid);
        if self.p.tracks[&tid].kind != TrackKind::Instrument {
            self.skips.item(format!("{track} のオートメーション"));
            return;
        }
        let Some(kind) = kid(pts, "Target").and_then(ctrl_kind) else {
            let what = expression(pts).unwrap_or_default();
            self.skips
                .item(format!("{track} の {what} のオートメーション"));
            return;
        };
        let times: Vec<Tick> = kids(pts, "RealPoint")
            .filter_map(|p| Some(self.tick(fr, f(p, "time")?)))
            .collect();
        let (Some(first), Some(last)) = (times.iter().min(), times.iter().max()) else {
            return;
        };
        let spans: Vec<(usize, Tick, Tick)> = self.p.tracks[&tid]
            .clips
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.body, ClipBody::Midi { .. }))
            .map(|(i, c)| (i, c.start, c.start + c.length))
            .collect();
        if spans.is_empty() {
            let w = ((*first).max(0), (*last).max(0) + self.p.ppq as Tick);
            self.midi_clip(tid, None, None, Some(pts), fr, w, &Outer::default());
            return;
        }
        for &(i, s, e) in &spans {
            let Some(lane) = self.ctrl_lane(pts, fr, (s, e)) else {
                continue;
            };
            let clip = &mut self.track_mut(tid).clips[i];
            if clip.controllers.iter().any(|l| l.kind == kind) {
                self.skips
                    .item(format!("{track} の重なったオートメーション"));
                continue;
            }
            clip.controllers.push(lane);
        }
        let outside = times
            .iter()
            .filter(|t| !spans.iter().any(|(_, s, e)| (*s..*e).contains(*t)))
            .count();
        self.skips.count(
            format!("{track} のクリップの外のオートメーション"),
            outside,
            "点",
        );
    }

    /// A MIDI clip of `notes`, and the controllers in `lanes` and `points`.
    #[allow(clippy::too_many_arguments)]
    fn midi_clip(
        &mut self,
        tid: TrackId,
        notes: Option<XNode>,
        lanes: Option<XNode>,
        points: Option<XNode>,
        fr: Frame,
        w: (Tick, Tick),
        me: &Outer,
    ) {
        let track = self.name(tid);
        if self.p.tracks[&tid].kind != TrackKind::Instrument {
            let what = if notes.is_some() {
                "ノート"
            } else {
                "オートメーション"
            };
            self.skips.item(format!("{track} の{what}"));
            return;
        }
        let mut out = Vec::new();
        let mut outside = 0;
        for n in notes.into_iter().flat_map(|n| kids(n, "Note")) {
            let (Some(t), Some(d), Some(key)) = (
                f(n, "time"),
                f(n, "duration"),
                n.attribute("key").and_then(|k| k.parse::<i64>().ok()),
            ) else {
                continue;
            };
            let s = self.tick(fr, t);
            let e = self.tick(fr, t + d);
            if s < w.0 || s >= w.1 || !(0..=127).contains(&key) {
                outside += 1;
                continue;
            }
            let vel = f(n, "vel").map_or(100, |v| (v * 127.0).round().clamp(1.0, 127.0) as u8);
            out.push(Note {
                id: NoteId(self.p.alloc()),
                start: s - w.0,
                length: (e - s).max(1),
                pitch: key as u8,
                velocity: vel,
                muted: false,
                ai: false,
            });
        }
        self.skips.count("クリップの外のノート", outside, "");
        let mut controllers = Vec::new();
        let drawn = lanes.into_iter().flat_map(|l| kids(l, "Points"));
        for pts in drawn.chain(points) {
            match self.ctrl_lane(pts, fr, w) {
                Some(l) => controllers.push(l),
                None => self
                    .skips
                    .item(format!("{track} のクリップの中のオートメーション")),
            }
        }
        if let Some(l) = lanes {
            for k in l.children().filter(|n| n.is_element()) {
                let tag = k.tag_name().name();
                if !matches!(tag, "Notes" | "Points") {
                    self.skips.item(format!("{track} のクリップの {tag}"));
                }
            }
        }
        let clip = Clip {
            id: ClipId(self.p.alloc()),
            name: me.name.clone().unwrap_or_else(|| track.clone()),
            color: me.color.clone(),
            start: w.0,
            length: (w.1 - w.0).max(1),
            muted: me.muted,
            take: None,
            controllers,
            body: ClipBody::Midi {
                notes: Arc::new(out),
                transpose: 0,
            },
        };
        self.track_mut(tid).clips.push(clip);
    }

    /// The part of a controller's points inside `w`, from the clip's start. The value in force
    /// where the clip starts comes with it.
    fn ctrl_lane(&mut self, pts: XNode, fr: Frame, w: (Tick, Tick)) -> Option<CtrlLane> {
        let kind = ctrl_kind(kid(pts, "Target")?)?;
        let (lo, hi) = kind.range();
        let unit_norm = pts.attribute("unit").is_none_or(|u| u == "normalized");
        let mut points: Vec<CtrlPoint> = Vec::new();
        let mut before: Option<i32> = None;
        for pt in kids(pts, "RealPoint") {
            let (Some(t), Some(v)) = (f(pt, "time"), f(pt, "value")) else {
                continue;
            };
            let tick = self.tick(fr, t) - w.0;
            let value = if !unit_norm {
                v.round() as i32
            } else if kind == CtrlKind::PitchBend {
                // 0.5 is the centre, as the other DAWs write it.
                (v * 16383.0 - 8191.5).round() as i32
            } else {
                (v * 127.0).round() as i32
            };
            let value = value.clamp(lo, hi);
            if tick < 0 {
                before = Some(value);
                continue;
            }
            if tick >= w.1 - w.0 {
                continue;
            }
            if let Some(v) = before.take().filter(|_| points.is_empty() && tick > 0) {
                points.push(CtrlPoint::new(0, v));
            }
            let shape = if pt.attribute("interpolation") == Some("linear") {
                CtrlShape::Linear
            } else {
                CtrlShape::Step
            };
            let mut p = CtrlPoint::new(tick, value);
            p.shape = shape;
            match points.last_mut() {
                Some(last) if last.tick >= tick => {
                    *last = CtrlPoint {
                        tick: last.tick,
                        ..p
                    }
                }
                _ => points.push(p),
            }
        }
        if let Some(v) = before.filter(|_| points.is_empty()) {
            points.push(CtrlPoint::new(0, v));
        }
        (!points.is_empty()).then(|| CtrlLane {
            kind,
            points: Arc::new(points),
        })
    }

    fn audio_clip(&mut self, tid: TrackId, content: XNode, fr: Frame, w: (Tick, Tick), me: &Outer) {
        let track = self.name(tid);
        if self.p.tracks[&tid].kind != TrackKind::Audio {
            self.skips.item(format!("{track} のオーディオ"));
            return;
        }
        let (audio, warps) = if content.has_tag_name("Warps") {
            let Some(a) = kid(content, "Audio") else {
                self.skips.item(format!(
                    "{track} の {} のクリップ",
                    content.tag_name().name()
                ));
                return;
            };
            if Unit::of(content, "contentTimeUnit") == Some(Unit::Beats) {
                self.skips.item(format!("{track} の拍で書かれたワープ"));
                return;
            }
            let pts: Vec<(f64, f64)> = kids(content, "Warp")
                .filter_map(|wp| Some((f(wp, "time")?, f(wp, "contentTime")?)))
                .collect();
            (a, pts)
        } else {
            (content, Vec::new())
        };
        let Some(file_el) = kid(audio, "File") else {
            return;
        };
        let Some(path) = file_el.attribute("path") else {
            return;
        };
        let Some(file) = self.media_file(path, b(file_el, "external") == Some(true)) else {
            self.skips
                .item(format!("見つからないファイル {}", file_name(path)));
            return;
        };
        // Song seconds against seconds of the audio.
        let pts: Vec<(f64, f64)> = if warps.len() >= 2 {
            warps.iter().map(|(t, c)| (self.secs(fr, *t), *c)).collect()
        } else {
            let s0 = self.secs(fr, 0.0);
            let e0 = self.secs(fr, 1.0);
            let one = match fr.unit {
                Unit::Seconds => 1.0,
                Unit::Beats => e0 - s0,
            };
            let first = warps.first().map_or((0.0, 0.0), |w| *w);
            let x0 = self.secs(fr, first.0);
            vec![(x0, first.1), (x0 + one, first.1 + one)]
        };
        let (slope, icpt) = fit(&pts);
        let off = pts
            .iter()
            .filter(|(x, y)| (icpt + slope * x - y).abs() > WARP_SLACK)
            .count();
        self.skips.count(format!("{track} のワープ"), off, "か所");
        if slope <= 0.0 {
            self.skips.item(format!("{track} の逆再生"));
            return;
        }
        // source(x) = icpt + slope x; the clip plays from where the audio starts.
        let r = 1.0 / slope;
        let mut start = w.0;
        let s_sec = self.tm.seconds_at(start);
        let mut skip = (icpt + slope * s_sec) * r;
        if skip < 0.0 {
            start = self.tm.tick_at(s_sec - skip);
            skip = 0.0;
        }
        if start >= w.1 {
            return;
        }
        let s_sec = self.tm.seconds_at(start);
        let offset = (self.tm.tick_at(s_sec + skip) - start).max(0);
        let mut process = AudioProcess::default();
        if (r - 1.0).abs() > 1e-4 {
            let bpm = self.tm.bpm_at(start);
            let follows = warps.len() >= 2 && fr.unit == Unit::Beats;
            let speed = 100.0 * slope;
            if !follows && (AudioProcess::MIN_SPEED..=AudioProcess::MAX_SPEED).contains(&speed) {
                process.speed = speed;
            } else {
                process.follow = true;
                process.tempo = Some((bpm * r).clamp(1.0, 999.0));
            }
        }
        let id = ClipId(self.p.alloc());
        let clip = Clip {
            id,
            name: me.name.clone().unwrap_or_else(|| {
                Path::new(&file)
                    .file_stem()
                    .map_or_else(|| track.clone(), |s| s.to_string_lossy().into_owned())
            }),
            color: me.color.clone(),
            start,
            length: w.1 - start,
            muted: me.muted,
            take: None,
            controllers: Vec::new(),
            body: ClipBody::Audio {
                file: file.to_string_lossy().into_owned(),
                offset,
                gain_db: 0.0,
                process,
            },
        };
        let len = clip.length;
        self.track_mut(tid).clips.push(clip);
        let (fi, fo) = (me.fade_in.unwrap_or(0), me.fade_out.unwrap_or(0));
        if fi > 0 || fo > 0 {
            let fx = self.p.fx_mut(id);
            fx.fade_in.length = fi.min(len);
            fx.fade_out.length = fo.min(len - fx.fade_in.length);
        }
    }

    /// The audio file a clip plays, on this machine.
    fn media_file(&mut self, path: &str, external: bool) -> Option<PathBuf> {
        if external {
            // A relative path is read from the folder of the file; a path that is not there
            // (the file came from another PC) is looked for by its name beside the file.
            let p = PathBuf::from(path);
            let mut tries = Vec::new();
            match self.base {
                Some(base) if p.is_relative() => tries.push(base.join(&p)),
                _ => tries.push(p.clone()),
            }
            if let Some(base) = self.base {
                tries.push(base.join(file_name(path)));
            }
            return tries.into_iter().find(|p| p.is_file());
        }
        if let Some(p) = self.media_names.get(path) {
            return Some(p.clone());
        }
        let bytes = (self.file)(path)?;
        fs::create_dir_all(self.media_dir).ok()?;
        let name = file_name(path);
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
            _ => (name.clone(), String::new()),
        };
        let mut out = self.media_dir.join(&name);
        let mut k = 2;
        while out.exists() || self.media.contains(&out) {
            out = self.media_dir.join(format!("{stem} ({k}){ext}"));
            k += 1;
        }
        fs::write(&out, bytes).ok()?;
        self.media.push(out.clone());
        self.media_names.insert(path.to_string(), out.clone());
        Some(out)
    }

    // ---- automation and markers ----------------------------------------------

    fn points(&mut self, tid: TrackId, pts: XNode, fr: Frame) {
        let fr = match Unit::of(pts, "timeUnit") {
            Some(u) => self.reframe(fr, u),
            None => fr,
        };
        let track = self.name(tid);
        let Some(target) = kid(pts, "Target") else {
            return;
        };
        let Some(param) = target
            .attribute("parameter")
            .and_then(|id| self.params.get(id))
            .cloned()
        else {
            self.skips.item(format!("{track} のオートメーション"));
            return;
        };
        let (owner, at) = match param {
            Param::Volume(t) => (t, AutoTarget::Volume),
            Param::Pan(t, ..) => (t, AutoTarget::Pan),
            Param::Mute(t) => (t, AutoTarget::Mute),
            Param::Send(t, s) => (t, AutoTarget::Send(s)),
            Param::Plugin(t, pid, ref k, ..) => (t, AutoTarget::Param(pid, k.clone())),
            Param::Builtin(t, pid, k, _) => (t, AutoTarget::Param(pid, k.to_string())),
            Param::Tempo | Param::Meter => {
                self.skips
                    .item("トラックの中のテンポと拍子のオートメーション");
                return;
            }
            Param::Other(t, what) => {
                self.skips
                    .item(format!("{} の {what} のオートメーション", self.name(t)));
                return;
            }
        };
        let unit = pts.attribute("unit");
        let mut out: Vec<AutoPoint> = Vec::new();
        for pt in pts.children().filter(|n| n.is_element()) {
            let Some(time) = f(pt, "time") else { continue };
            let raw = match pt.tag_name().name() {
                "RealPoint" => f(pt, "value"),
                "BoolPoint" => b(pt, "value").map(|v| if v { 1.0 } else { 0.0 }),
                _ => None,
            };
            let Some(raw) = raw else { continue };
            let v = match &param {
                Param::Volume(_) | Param::Send(..) => match unit {
                    Some("decibel") => raw,
                    Some("normalized") => AutoTarget::Volume.from_norm(raw),
                    _ => db_of(raw),
                },
                Param::Pan(_, lo, hi) => {
                    let n = if unit == Some("normalized") || unit.is_none() {
                        if hi > lo { (raw - lo) / (hi - lo) } else { raw }
                    } else {
                        raw
                    };
                    n * 2.0 - 1.0
                }
                Param::Mute(_) => {
                    if raw >= 0.5 {
                        1.0
                    } else {
                        0.0
                    }
                }
                Param::Plugin(_, _, _, lo, hi) => {
                    if unit == Some("normalized") || hi <= lo {
                        raw
                    } else {
                        (raw - lo) / (hi - lo)
                    }
                }
                Param::Builtin(t, pid, key, scale) => {
                    let def = self.p.tracks[t]
                        .inserts
                        .iter()
                        .find(|x| x.id == *pid)
                        .and_then(Plugin::builtin);
                    match def.and_then(|d| d.param(key)) {
                        Some((_, spec)) => spec.to_norm((raw * scale).clamp(spec.min, spec.max)),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let (lo, hi) = at.range();
            let mut p = AutoPoint::new(self.tick(fr, time).max(0), v.clamp(lo, hi));
            p.step = at.stepped() || pt.attribute("interpolation") == Some("hold");
            match out.last() {
                Some(last) if last.tick >= p.tick => p.tick = last.tick + 1,
                _ => {}
            }
            out.push(p);
        }
        if out.is_empty() {
            return;
        }
        let t = self.track_mut(owner);
        if t.automation.iter().any(|l| l.target == at) {
            self.skips
                .item(format!("{track} の重なったオートメーション"));
            return;
        }
        let mut lane = AutoLane::on_track(at, t);
        lane.points = Arc::new(out);
        t.automation.push(lane);
    }

    fn markers(&mut self, m: XNode) {
        let unit = Unit::of(m, "timeUnit").unwrap_or(Unit::Beats);
        let fr = self.frame(0, unit, 0.0);
        let mut all: Vec<(Tick, String, Option<String>)> = kids(m, "Marker")
            .filter_map(|k| {
                let tick = self.tick(fr, f(k, "time")?).max(0);
                let name = k.attribute("name").unwrap_or("").trim().to_string();
                Some((tick, name, k.attribute("color").map(str::to_string)))
            })
            .collect();
        all.sort_by_key(|x| x.0);
        let mut last: Option<Tick> = self.p.markers.last().map(|m| m.tick);
        for (tick, name, color) in all {
            let tick = match last {
                Some(l) if l >= tick => l + 1,
                _ => tick,
            };
            last = Some(tick);
            let id = MarkerId(self.p.alloc());
            let name = if name.is_empty() {
                format!("マーカー {}", self.p.markers.len() + 1)
            } else {
                name
            };
            self.p.markers.push(crate::marker::Marker {
                id,
                name,
                tick,
                color,
            });
        }
    }
}

/// The controller a `Target` of a MIDI expression names, if the song has lanes of it.
fn ctrl_kind(target: XNode) -> Option<CtrlKind> {
    Some(match target.attribute("expression")? {
        "channelController" => CtrlKind::Cc(
            target
                .attribute("controller")
                .and_then(|c| c.parse::<u8>().ok())
                .filter(|c| *c < 128)?,
        ),
        "pitchBend" => CtrlKind::PitchBend,
        "channelPressure" => CtrlKind::Aftertouch,
        _ => return None,
    })
}

/// The plugin's id as the song keeps it: for VST3 the class id in 32 upper-case hex digits
/// (`{5653…}` and `5653-…` as some hosts write it come to the same), for CLAP the id as written.
fn device_id(format: &str, id: &str) -> String {
    let id = id.trim();
    if format != "vst3" {
        return id.to_string();
    }
    let hex: String = id.chars().filter(char::is_ascii_hexdigit).collect();
    if hex.len() == 32 {
        hex.to_ascii_uppercase()
    } else {
        id.to_string()
    }
}

/// What a clip inside other clips takes from them.
#[derive(Clone, Debug, Default)]
struct Outer {
    name: Option<String>,
    color: Option<String>,
    muted: bool,
    fade_in: Option<Tick>,
    fade_out: Option<Tick>,
}

/// The straight line through the points by least squares: (slope, intercept).
fn fit(pts: &[(f64, f64)]) -> (f64, f64) {
    let n = pts.len() as f64;
    if pts.len() < 2 {
        return (1.0, pts.first().map_or(0.0, |p| p.1 - p.0));
    }
    let (mx, my) = (
        pts.iter().map(|p| p.0).sum::<f64>() / n,
        pts.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    if sxx <= 0.0 {
        return (1.0, my - mx);
    }
    let slope = sxy / sxx;
    (slope, my - slope * mx)
}

/// Plain value of a parameter whose unit is not `normalized`; a normalised one is spread over
/// its `min..max`.
fn plain_of(el: XNode, v: f64) -> f64 {
    if el.attribute("unit") == Some("normalized")
        && let (Some(lo), Some(hi)) = (f(el, "min"), f(el, "max"))
    {
        return lo + (hi - lo) * v;
    }
    v
}
