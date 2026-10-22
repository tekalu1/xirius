//! Offline export of the mix, and of each track and bus on its own, to WAV, FLAC or MP3
//! (EXP-001, EXP-002, EXP-005, EXP-010, EXP-011, EXP-013, EXP-014, EXP-015, EXP-016, EXP-017,
//! EXP-018, EXP-027).
//!
//! The song goes through the same engine and plan that play it, in blocks, without a device:
//! the file is the sound the plan makes, frame for frame. A render has no dither and no
//! randomness, so the same song gives the same file. A FLAC file holds the same integers as a
//! WAV file of the same bit depth. An MP3 file is encoded by LAME ([`crate::mp3`]), and its
//! loudness and true peak are measured on the file read back.
//!
//! Plugins in the plugin host process (OPEN-02) are rendered through the same shared memory as
//! when playing, one block behind, and the host is waited for rather than skipped, so the file
//! is the same as with the plugins in this process. The render starts the plan's delay
//! compensation earlier and drops it, so the file starts on the range (ENG-010, ENG-024).

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::{Output, Project, TrackKind};
use xirius_engine::engine::create;
use xirius_engine::{Command, EngineConfig, Plan, RenderMode};
use xirius_plugins::Catalog;

use crate::automation;
use crate::build::{Builder, setup};
use crate::graph::{Graph, Mix, Scope, Select, song_end_tick};
use crate::mp3::{self, Lame, Tags};
use crate::plugins::Isolation;
use crate::tempo::TempoMap;

/// The block the render runs in. The engine cuts it again at its own `max_block`.
const BLOCK: usize = 4096;
/// The block while automation moves values: a value steps at most every 10 ms at 48 kHz.
const AUTOMATION_BLOCK: usize = 480;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError(pub String);

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExportError {}

fn err<T>(msg: impl Into<String>) -> Result<T, ExportError> {
    Err(ExportError(msg.into()))
}

/// Which stretch of the song a file holds (EXP-010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeSpec {
    /// From the start to where the last clip ends.
    Song,
    /// The song's loop range.
    Loop,
    /// A section, by name.
    Section(String),
    /// Sections by name or id: from the start of the first to the end of the last, with what
    /// lies between (EXP-010).
    Sections(Vec<String>),
    /// Ticks, `from` up to but not including `to`.
    Ticks(i64, i64),
}

/// The kind of file (EXP-001, EXP-011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Wav,
    /// Lossless and smaller; 16 or 24 bit integer only.
    Flac,
    /// Constant bit rate (EXP-005), up to 48 kHz; it has no bit depth.
    Mp3,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Mp3 => "mp3",
        }
    }

    /// The file name extension, without the dot.
    pub fn ext(self) -> &'static str {
        self.name()
    }

    pub fn parse(s: &str) -> Option<Format> {
        match s.to_ascii_lowercase().as_str() {
            "wav" | "wave" => Some(Format::Wav),
            "flac" => Some(Format::Flac),
            "mp3" => Some(Format::Mp3),
            _ => None,
        }
    }

    /// The format a path's extension names, if any.
    pub fn of_path(path: &Path) -> Option<Format> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Format::parse)
    }
}

/// The word in a file name pattern that is replaced by the song's name (EXP-015).
pub const SONG_TAG: &str = "{曲}";
/// The word in a file name pattern that is replaced by the track's or bus's name.
pub const TRACK_TAG: &str = "{トラック}";

/// A file name from a pattern: `{曲}` (or `{song}`) is the song's name and `{トラック}` (or
/// `{track}`) the strip's. The result is safe as a file name, without the extension.
pub fn name_from(pattern: &str, song: &str, track: &str) -> String {
    let s = pattern
        .replace("{song}", SONG_TAG)
        .replace("{track}", TRACK_TAG)
        .replace(SONG_TAG, song)
        .replace(TRACK_TAG, track);
    sanitize(&s)
}

/// Whether a stems pattern names each file by its strip (EXP-016): without it every stem
/// would get the same name.
pub fn names_track(pattern: &str) -> bool {
    pattern.contains(TRACK_TAG) || pattern.contains("{track}")
}

#[derive(Debug, Clone)]
pub struct Options {
    pub range: RangeSpec,
    /// `None` takes the song's setting (EXP-011).
    pub sample_rate: Option<u32>,
    /// Not used for MP3.
    pub bit_depth: Option<u16>,
    pub format: Format,
    /// MP3's bit rate, kbps ([`mp3::DEFAULT_BITRATE`] when `None`); not used for the others.
    pub bitrate: Option<u16>,
    /// MP3's encoder ([`mp3::locate`] when `None`).
    pub lame: Option<PathBuf>,
    /// How stem files are named (`{トラック}` when `None`); see [`name_from`].
    pub pattern: Option<String>,
    /// The name a stem is given where it is not its strip's own: a folder's stem is the bus the
    /// folder is connected to, named after the folder (CTX-018).
    pub stem_names: Vec<(TrackId, String)>,
    /// Whether a stem may replace a file that is already there. When not, the new file gets
    /// a number (`Keys-2.wav`) (OPEN-10).
    pub overwrite: bool,
    /// Files other exports are writing: never written over, a stem takes a number instead.
    pub busy: Vec<PathBuf>,
    /// Seconds rendered after the range ends, for effect tails to ring out. 0 ends exactly at
    /// the end of the range.
    pub tail_secs: f64,
    /// The folder relative audio paths in the song are taken from.
    pub dir: Option<PathBuf>,
    /// Where VST3 plugins are looked up (the standard cache when `None`).
    pub catalog: Option<Arc<Catalog>>,
    /// Counts the frames rendered and is checked for a cancel between blocks.
    pub progress: Option<Arc<Progress>>,
    /// Where VST3 plugins run (`None`: [`Isolation::default`]).
    pub isolation: Option<Isolation>,
}

/// How far an export is, shared with whoever watches it (AI-002). Frames of every file count:
/// stems of four tracks have four times the frames of the song.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
    /// The files in the order they are written, each the same number of frames (EXP-017).
    files: Mutex<Vec<String>>,
    per_file: AtomicU64,
    rate: AtomicU64,
    /// The bundle a plugin scan opens last (PLG-059).
    seeing: Mutex<String>,
}

/// How far one file of an export is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileProgress {
    pub name: String,
    pub done: u64,
    pub total: u64,
}

impl Progress {
    /// Each file's own share of the frames, in the order they are written.
    pub fn files(&self) -> Vec<FileProgress> {
        let per = self.per_file.load(Ordering::Relaxed);
        let done = self.done();
        let names = self.files.lock().map(|f| f.clone()).unwrap_or_default();
        names
            .into_iter()
            .enumerate()
            .map(|(i, name)| FileProgress {
                name,
                done: done.saturating_sub(i as u64 * per).min(per),
                total: per,
            })
            .collect()
    }

    /// Seconds of sound rendered so far (0 until the rate is known).
    pub fn seconds_done(&self) -> f64 {
        match self.rate.load(Ordering::Relaxed) {
            0 => 0.0,
            r => self.done() as f64 / r as f64,
        }
    }

    pub(crate) fn plan(&self, files: Vec<String>, per_file: u64, rate: u32) {
        self.per_file.store(per_file, Ordering::Relaxed);
        self.rate.store(u64::from(rate), Ordering::Relaxed);
        self.set_total(per_file * files.len() as u64);
        if let Ok(mut f) = self.files.lock() {
            *f = files;
        }
    }

    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// 0..=1; 0 until the total is known.
    pub fn fraction(&self) -> f64 {
        match self.total() {
            0 => 0.0,
            t => (self.done() as f64 / t as f64).min(1.0),
        }
    }

    /// Asks the render to stop at the next block.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn set_total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
    }

    fn add(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }

    /// The file the work is on now (a plugin scan's bundle); empty until it says.
    pub fn seeing(&self) -> String {
        self.seeing.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub(crate) fn see(&self, file: String) {
        if let Ok(mut s) = self.seeing.lock() {
            *s = file;
        }
    }

    /// Both at once, for work that counts its own steps (a plugin scan).
    pub(crate) fn set(&self, done: u64, total: u64) {
        self.set_total(total);
        self.done.store(done, Ordering::Relaxed);
    }
}

/// The message of an export stopped by [`Progress::cancel`].
pub const CANCELLED: &str = "書き出しをキャンセルしました";

impl Default for Options {
    fn default() -> Self {
        Options {
            range: RangeSpec::Song,
            sample_rate: None,
            bit_depth: None,
            format: Format::Wav,
            bitrate: None,
            lame: None,
            pattern: None,
            stem_names: Vec::new(),
            overwrite: true,
            busy: Vec::new(),
            tail_secs: 0.0,
            dir: None,
            catalog: None,
            progress: None,
            isolation: None,
        }
    }
}

/// What one file came to.
#[derive(Debug, Clone, PartialEq)]
pub struct FileReport {
    /// Tracks with a manual mute setting in the song; automation may change it during mixdown.
    /// Stems list skipped files on their parent report.
    pub muted_tracks: Vec<String>,
    pub name: String,
    pub path: PathBuf,
    /// The track or bus of a stem; `None` for a mix (EXP-019).
    pub strip: Option<TrackId>,
    pub frames: u64,
    pub sample_rate: u32,
    /// 0 for MP3.
    pub bit_depth: u16,
    /// MP3's bit rate, kbps.
    pub bitrate_kbps: Option<u16>,
    /// Largest absolute sample, linear. These and the loudness are of the file's sound: of an
    /// MP3, as it decodes.
    pub peak: f32,
    /// Root mean square over both channels, linear.
    pub rms: f32,
    /// Samples above full scale (they are clipped in an integer file).
    pub clipped: u64,
    /// Integrated loudness, LUFS (EXP-018); `None` when too short or silent to measure.
    pub lufs: Option<f64>,
    /// True peak, dBTP: the peak between the samples as well (EXP-018).
    pub true_peak_db: Option<f64>,
    pub format: Format,
    pub warnings: Vec<String>,
    /// What an MP3 was titled (ID3).
    pub tags: Option<Tags>,
}

impl FileReport {
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate)
    }

    pub fn peak_db(&self) -> f64 {
        to_db(self.peak)
    }

    pub fn rms_db(&self) -> f64 {
        to_db(self.rms)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "path": self.path.display().to_string(),
            "strip": self.strip.map(|s| s.to_string()),
            "frames": self.frames,
            "seconds": self.seconds(),
            "sample_rate": self.sample_rate,
            "bit_depth": (self.bit_depth > 0).then_some(self.bit_depth),
            "bitrate_kbps": self.bitrate_kbps,
            "peak": self.peak,
            "peak_db": self.peak_db(),
            "rms": self.rms,
            "rms_db": self.rms_db(),
            "clipped": self.clipped,
            "lufs": self.lufs,
            "true_peak_db": self.true_peak_db,
            "format": self.format.name(),
            "warnings": self.warnings,
            "muted_tracks": self.muted_tracks,
            "tags": self.tags.as_ref().map(|t| json!({ "title": t.title, "album": t.album })),
        })
    }

    pub fn summary(&self) -> String {
        let mut summary = format!(
            "{}: {:.2} 秒（{} frames, {} Hz, {}）peak {:.1} dBFS / rms {:.1} dBFS{}{}",
            self.path.display(),
            self.seconds(),
            self.frames,
            self.sample_rate,
            match self.bitrate_kbps {
                Some(k) => format!("{k} kbps"),
                None => format!("{} bit", self.bit_depth),
            },
            self.peak_db(),
            self.rms_db(),
            match (self.lufs, self.true_peak_db) {
                (Some(l), Some(t)) => format!(" / {l:.1} LUFS / {t:.1} dBTP"),
                (None, Some(t)) => format!(" / {t:.1} dBTP"),
                _ => String::new(),
            },
            if self.clipped > 0 {
                format!(" / 0 dBFS を超えたサンプル {} 個", self.clipped)
            } else {
                String::new()
            }
        );
        if !self.muted_tracks.is_empty() {
            summary.push_str(&format!(" / ミュート: {}", self.muted_tracks.join("、")));
        }
        summary
    }
}

fn to_db(x: f32) -> f64 {
    if x <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * f64::from(x).log10()
    }
}

/// A stem that was not written, and why (ENG-037).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub name: String,
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StemsReport {
    pub files: Vec<FileReport>,
    pub skipped: Vec<Skipped>,
}

impl StemsReport {
    pub fn to_json(&self) -> Value {
        json!({
            "files": self.files.iter().map(FileReport::to_json).collect::<Vec<_>>(),
            "skipped": self.skipped.iter().map(|s| json!({"name": s.name, "why": s.why})).collect::<Vec<_>>(),
        })
    }
}

/// The ticks a range covers.
pub fn resolve_range(p: &Project, range: &RangeSpec) -> Result<(i64, i64), ExportError> {
    let (from, to) = match range {
        RangeSpec::Song => (0, song_end_tick(p)),
        RangeSpec::Loop => match p.loop_range {
            Some(l) => (l.start, l.end),
            None => {
                return err(
                    "ループ範囲がありません（loop.set で設定するか、--range song を指定してください）",
                );
            }
        },
        RangeSpec::Section(name) => match p
            .sections
            .iter()
            .find(|s| &s.name == name || s.id.to_string() == *name)
        {
            Some(s) => (s.start, s.start + s.length),
            None => {
                let names: Vec<&str> = p.sections.iter().map(|s| s.name.as_str()).collect();
                return err(format!(
                    "セクション `{name}` が見つかりません（使用できるセクション: {}）",
                    if names.is_empty() {
                        "なし".to_string()
                    } else {
                        names.join("・")
                    }
                ));
            }
        },
        RangeSpec::Sections(names) => {
            let mut found = Vec::new();
            for name in names {
                match p
                    .sections
                    .iter()
                    .find(|s| &s.name == name || s.id.to_string() == *name)
                {
                    Some(s) => found.push((s.start, s.start + s.length)),
                    None => return resolve_range(p, &RangeSpec::Section(name.clone())),
                }
            }
            match (
                found.iter().map(|f| f.0).min(),
                found.iter().map(|f| f.1).max(),
            ) {
                (Some(a), Some(b)) => (a, b),
                _ => return err("セクションを 1 つ以上選択してください"),
            }
        }
        RangeSpec::Ticks(a, b) => (*a, *b),
    };
    if from < 0 || to <= from {
        return err(format!("書き出す範囲が空です（{from}〜{to} tick）"));
    }
    Ok((from, to))
}

/// Stereo samples, one channel each.
pub struct Rendered {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub warnings: Vec<String>,
}

/// Renders `[from, to)` frames of `select`, then `tail` more frames. Notes that began before
/// `from` are not played.
pub fn render(
    builder: &mut Builder,
    p: &Project,
    select: Select,
    from: u64,
    to: u64,
    tail: u64,
) -> Result<Rendered, ExportError> {
    render_with(builder, p, select, from, to, tail, None)
}

/// [`render`], counting into `progress` and stopping when it is cancelled.
pub fn render_with(
    builder: &mut Builder,
    p: &Project,
    select: Select,
    from: u64,
    to: u64,
    tail: u64,
    progress: Option<&Progress>,
) -> Result<Rendered, ExportError> {
    // Automation (AUT-001): the mix is built as the lanes set it at `from`, and the values
    // that move are sent before each block, which is shorter while anything is automated.
    let tmap = TempoMap::from_project(p);
    let rate = f64::from(builder.rate());
    let automated = automation::has_lanes(p);
    let heard_at = |frame: u64| automation::song_at(p, tmap.tick_of_frame(frame, rate) as f64);
    let mut built = builder.build(&heard_at(from), select);
    builder.plugins_mut().built_offline();
    let (graph, mut mix) = (built.graph, built.mix);
    let block = if automated { AUTOMATION_BLOCK } else { BLOCK };
    let plan = Plan::build(built.spec, setup(builder.rate(), RenderMode::Offline), None)
        .map_err(|e| ExportError(e.to_string()))?;
    built.warnings.extend(builder.plugins().opening_warnings());
    let timing = crate::automation_timing::Timing::new(p, &graph, &plan.shape(), builder.plugins())
        .with_start(from);
    // The plan's output is late by its delay compensation: those first frames are dropped.
    let skip = plan.latency() as usize;
    let (mut audio, mut control) = create(
        plan,
        EngineConfig {
            command_capacity: if automated { 1024 } else { 16 },
            ..EngineConfig::default()
        },
    );
    for cmd in [
        Command::Seek(from),
        Command::SetEnd(Some(to)),
        Command::Play,
    ] {
        if control.send(cmd).is_err() {
            return err("オーディオエンジンに指示を送れませんでした");
        }
    }
    let total = (to - from + tail) as usize;
    let mut left = Vec::with_capacity(total);
    let mut right = Vec::with_capacity(total);
    let mut buf = vec![0.0f32; block * 2];
    let mut skipped = 0;
    let mut params = automation::ParamFollow::default();
    while left.len() < total {
        if progress.is_some_and(Progress::cancelled) {
            return err(CANCELLED);
        }
        if automated {
            // The engine's position: the dropped delay compensation is played too.
            let frame = from + (left.len() + skipped) as u64;
            let at = timing.song_at(p, frame, &tmap, rate);
            let next = Mix::of(&at, &graph, &Scope::new(&at, select));
            params.send_each(
                p,
                |id, target| timing.tick(id, target, frame, &tmap, rate),
                builder.plugins(),
            );
            for cmd in automation::changes(&mix, &next) {
                if control.send(cmd).is_err() {
                    return err("オーディオエンジンに指示を送れませんでした");
                }
            }
            mix = next;
        }
        let n = block.min(total - left.len() + skip - skipped);
        audio.render(&mut buf[..n * 2], 2);
        let (missed, failed) = audio.stream_health();
        if missed > 0 || failed {
            return err("音声ファイルの先読みで読み込みに失敗したため、書き出しを中止しました");
        }
        let drop = (skip - skipped).min(n);
        skipped += drop;
        left.extend(buf[drop * 2..n * 2].iter().step_by(2));
        right.extend(buf[drop * 2..n * 2].iter().skip(1).step_by(2));
        control.poll(|_| {});
        if let Some(pr) = progress {
            pr.add((n - drop) as u64);
        }
    }
    Ok(Rendered {
        left,
        right,
        warnings: built.warnings,
    })
}

fn quantize(x: f32, scale: f32) -> i32 {
    (x.clamp(-1.0, 1.0) * scale).round() as i32
}

/// Writes a stereo WAV: 16 or 24 bit integer, or 32 bit float.
pub fn write_wav(
    path: &Path,
    left: &[f32],
    right: &[f32],
    rate: u32,
    bits: u16,
) -> Result<(), ExportError> {
    let float = match bits {
        16 | 24 => false,
        32 => true,
        n => {
            return err(format!(
                "ビット深度には 16・24・32 のいずれかを指定してください（{n} は使用できません）"
            ));
        }
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| ExportError(format!("{} を作成できません: {e}", dir.display())))?;
    }
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: if float {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };
    let fail = |e: hound::Error| ExportError(format!("{} に書き込めません: {e}", path.display()));
    let mut w = hound::WavWriter::create(path, spec).map_err(fail)?;
    for (l, r) in left.iter().zip(right) {
        for s in [*l, *r] {
            match bits {
                16 => w.write_sample(quantize(s, 32_767.0) as i16),
                24 => w.write_sample(quantize(s, 8_388_607.0)),
                _ => w.write_sample(s),
            }
            .map_err(fail)?;
        }
    }
    w.finalize().map_err(fail)
}

fn measure(left: &[f32], right: &[f32]) -> (f32, f32, u64) {
    let (mut peak, mut sum, mut clipped) = (0.0f32, 0.0f64, 0u64);
    for s in left.iter().chain(right) {
        let a = s.abs();
        peak = peak.max(a);
        sum += f64::from(*s) * f64::from(*s);
        if a > 1.0 {
            clipped += 1;
        }
    }
    let n = (left.len() + right.len()).max(1) as f64;
    (peak, (sum / n).sqrt() as f32, clipped)
}

/// Writes a stereo FLAC: 16 or 24 bit integer, the same integers [`write_wav`] writes.
pub fn write_flac(
    path: &Path,
    left: &[f32],
    right: &[f32],
    rate: u32,
    bits: u16,
) -> Result<(), ExportError> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    let scale = match bits {
        16 => 32_767.0,
        24 => 8_388_607.0,
        32 => return err(FLAC_NO_FLOAT),
        n => {
            return err(format!(
                "ビット深度には 16 か 24 を指定してください（{n} は使用できません）"
            ));
        }
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| ExportError(format!("{} を作成できません: {e}", dir.display())))?;
    }
    let mut samples = Vec::with_capacity(left.len() * 2);
    for (l, r) in left.iter().zip(right) {
        samples.push(quantize(*l, scale));
        samples.push(quantize(*r, scale));
    }
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, e)| ExportError(format!("FLAC の設定が無効です: {e}")))?;
    let source =
        flacenc::source::MemSource::from_samples(&samples, 2, usize::from(bits), rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| ExportError(format!("FLAC にエンコードできません: {e}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| ExportError(format!("FLAC にエンコードできません: {e:?}")))?;
    // The blocks are of one size, but flacenc counts the shorter last one in the stream info's
    // minimum, which the format leaves out; readers that check it (symphonia) refuse the file.
    let mut bytes = sink.as_slice().to_vec();
    if bytes.len() > 12 && &bytes[..4] == b"fLaC" && bytes[4] & 0x7f == 0 {
        bytes.copy_within(10..12, 8);
    }
    std::fs::write(path, bytes)
        .map_err(|e| ExportError(format!("{} に書き込めません: {e}", path.display())))
}

/// The message for FLAC with 32 bit float, which FLAC does not hold (EXP-011).
pub const FLAC_NO_FLOAT: &str =
    "FLAC では 32 bit float を選択できません（16 bit か 24 bit を選択してください）";

/// Writes `format` (WAV or FLAC: an MP3 needs its encoder, [`write_mp3`]).
pub fn write_file(
    path: &Path,
    left: &[f32],
    right: &[f32],
    rate: u32,
    bits: u16,
    format: Format,
) -> Result<(), ExportError> {
    match format {
        Format::Wav => write_wav(path, left, right, rate, bits),
        Format::Flac => write_flac(path, left, right, rate, bits),
        Format::Mp3 => {
            let lame = mp3::load(None).map_err(|why| cannot_load(&why))?;
            write_mp3(
                path,
                left,
                right,
                rate,
                mp3::DEFAULT_BITRATE,
                &lame,
                &Tags::default(),
            )
        }
    }
}

/// Writes a stereo MP3 at `kbps` (EXP-005).
pub fn write_mp3(
    path: &Path,
    left: &[f32],
    right: &[f32],
    rate: u32,
    kbps: u16,
    lame: &Lame,
    tags: &Tags,
) -> Result<(), ExportError> {
    let bytes = lame
        .encode(left, right, rate, kbps, tags)
        .map_err(|e| ExportError(format!("{}（{e}）", mp3::ENCODE_FAILED)))?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| ExportError(format!("{} を作成できません: {e}", dir.display())))?;
    }
    std::fs::write(path, bytes)
        .map_err(|e| ExportError(format!("{} に書き込めません: {e}", path.display())))
}

fn cannot_load(why: &str) -> ExportError {
    ExportError(format!("{}（{why}）", mp3::CANNOT_LOAD))
}

/// The rate an MP3 of a song at `rate` is written at when none is asked for: the song's own
/// when MP3 has it, else 44.1 kHz for the 44.1 kHz family and 48 kHz for the rest.
pub fn mp3_rate(rate: u32) -> u32 {
    if mp3::RATES.contains(&rate) {
        rate
    } else if rate.is_multiple_of(44_100) {
        44_100
    } else {
        mp3::MAX_RATE
    }
}

/// Integrated loudness (LUFS) and true peak (dBTP) by EBU R 128 / ITU-R BS.1770 (EXP-018).
/// Either is `None` when it does not come to a number (silence, or too short to gate).
pub fn loudness(left: &[f32], right: &[f32], rate: u32) -> (Option<f64>, Option<f64>) {
    use ebur128::{EbuR128, Mode};
    let Ok(mut m) = EbuR128::new(2, rate, Mode::I | Mode::TRUE_PEAK) else {
        return (None, None);
    };
    if m.add_frames_planar_f32(&[left, right]).is_err() {
        return (None, None);
    }
    let lufs = m.loudness_global().ok().filter(|l| l.is_finite());
    let tp = (0..2)
        .filter_map(|c| m.true_peak(c).ok())
        .fold(0.0f64, f64::max);
    let tp_db = (tp > 0.0).then(|| 20.0 * tp.log10());
    (lufs, tp_db)
}

struct Resolved {
    rate: u32,
    /// 0 for MP3.
    bits: u16,
    format: Format,
    /// MP3's bit rate and encoder.
    mp3: Option<(u16, Arc<Lame>)>,
    from: u64,
    to: u64,
    tail: u64,
    progress: Option<Arc<Progress>>,
}

impl Resolved {
    fn frames(&self) -> u64 {
        self.to - self.from + self.tail
    }

    fn builder(&self, o: &Options) -> Builder {
        let mut b = Builder::new(self.rate, o.dir.clone());
        b.set_catalog(o.catalog.clone());
        if let Some(i) = &o.isolation {
            b.set_isolation(i.clone());
        }
        b
    }

    fn plan(&self, files: Vec<String>) {
        if let Some(pr) = &self.progress {
            pr.plan(files, self.frames(), self.rate);
        }
    }
}

fn resolve(p: &Project, o: &Options) -> Result<Resolved, ExportError> {
    let is_mp3 = o.format == Format::Mp3;
    let rate = match o.sample_rate {
        Some(r) => r,
        None if is_mp3 => mp3_rate(p.settings.sample_rate),
        None => p.settings.sample_rate,
    };
    if !(8_000..=384_000).contains(&rate) {
        return err(format!(
            "サンプルレートは 8000〜384000 Hz の範囲で指定してください（{rate} は範囲外です）"
        ));
    }
    let (bits, mp3) = if is_mp3 {
        if rate > mp3::MAX_RATE {
            return err(format!("{}（{rate} Hz では書き出せません）", mp3::OVER_48K));
        }
        if !mp3::RATES.contains(&rate) {
            return err(format!(
                "MP3 のサンプルレートには 32000・44100・48000 のいずれかを指定してください（{rate} は使用できません）"
            ));
        }
        let kbps = o.bitrate.unwrap_or(mp3::DEFAULT_BITRATE);
        if !mp3::BITRATES.contains(&kbps) {
            return err(format!(
                "ビットレートには 128・192・256・320 kbps のいずれかを指定してください（{kbps} は使用できません）"
            ));
        }
        // Before anything is rendered, so a missing encoder is found at once.
        let lame = mp3::load(o.lame.as_deref()).map_err(|why| cannot_load(&why))?;
        (0, Some((kbps, lame)))
    } else {
        let bits = o.bit_depth.unwrap_or(p.settings.bit_depth);
        if !matches!(bits, 16 | 24 | 32) {
            return err(format!(
                "ビット深度には 16・24・32 のいずれかを指定してください（{bits} は使用できません）"
            ));
        }
        if o.format == Format::Flac && bits == 32 {
            return err(FLAC_NO_FLOAT);
        }
        (bits, None)
    };
    if !(0.0..=600.0).contains(&o.tail_secs) {
        return err("--tail は 0〜600 秒の範囲で指定してください");
    }
    let (a, b) = resolve_range(p, &o.range)?;
    let tmap = TempoMap::from_project(p);
    let r = f64::from(rate);
    let (from, to) = (tmap.frame_at(a, r), tmap.frame_at(b, r));
    if to <= from {
        return err("書き出す範囲の長さが 0 です");
    }
    Ok(Resolved {
        rate,
        bits,
        format: o.format,
        mp3,
        from,
        to,
        tail: (o.tail_secs * r).round() as u64,
        progress: o.progress.clone(),
    })
}

fn render_file(
    builder: &mut Builder,
    p: &Project,
    select: Select,
    r: &Resolved,
    name: &str,
    path: &Path,
    tags: &Tags,
) -> Result<FileReport, ExportError> {
    let out = render_with(
        builder,
        p,
        select,
        r.from,
        r.to,
        r.tail,
        r.progress.as_deref(),
    )?;
    let frames = out.left.len() as u64;
    // What is measured: the render, or for an MP3 the file as it decodes (EXP-005).
    let heard = match &r.mp3 {
        Some((kbps, lame)) => {
            write_mp3(path, &out.left, &out.right, r.rate, *kbps, lame, tags)?;
            let d = mp3::read(path)
                .map_err(|e| ExportError(format!("{}（{e}）", mp3::ENCODE_FAILED)))?;
            Rendered {
                left: d.left,
                right: d.right,
                warnings: out.warnings,
            }
        }
        None => {
            write_file(path, &out.left, &out.right, r.rate, r.bits, r.format)?;
            out
        }
    };
    let (peak, rms, clipped) = measure(&heard.left, &heard.right);
    let (lufs, true_peak_db) = loudness(&heard.left, &heard.right, r.rate);
    // A file that goes over full scale (or over 0 dBTP between samples) says so in
    // `warnings` too: the JSON alone must not read as "nothing to report".
    let mut warnings = heard.warnings;
    if clipped > 0 {
        warnings.push(format!("0 dBFS を超えたサンプルが {clipped} 個あります"));
    }
    if let Some(tp) = true_peak_db
        && tp > 0.0
    {
        warnings.push(format!(
            "0 dBTP を超えています（トゥルーピーク {tp:.2} dBTP）"
        ));
    }
    Ok(FileReport {
        muted_tracks: if matches!(select, Select::All) {
            p.tracks
                .values()
                .filter(|t| t.mute && t.has_clips())
                .map(|t| t.name.clone())
                .collect()
        } else {
            Vec::new()
        },
        name: name.to_string(),
        path: path.to_path_buf(),
        strip: match select {
            Select::All => None,
            Select::Track(id) | Select::Bus(id) => Some(id),
        },
        frames,
        sample_rate: r.rate,
        bit_depth: r.bits,
        bitrate_kbps: r.mp3.as_ref().map(|m| m.0),
        peak,
        rms,
        clipped,
        lufs,
        true_peak_db,
        format: r.format,
        warnings,
        tags: r.mp3.as_ref().map(|_| tags.clone()),
    })
}

/// The whole mix to one file. An MP3 is titled with the song's name.
pub fn mixdown(p: &Project, out: &Path, o: &Options) -> Result<FileReport, ExportError> {
    let r = resolve(p, o)?;
    r.plan(vec!["mixdown".to_string()]);
    let mut builder = r.builder(o);
    let title = if p.name.trim().is_empty() {
        out.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        p.name.clone()
    };
    let tags = Tags { title, album: None };
    render_file(&mut builder, p, Select::All, &r, "mixdown", out, &tags)
}

/// Which strips get a stem (EXP-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StemSet {
    /// Every track.
    Tracks,
    /// Every bus.
    Buses,
    /// Every track and every bus.
    All,
    /// The buses, and the tracks whose output is the master: a track that goes out through a
    /// bus is in that bus's stem.
    ByBus,
}

impl StemSet {
    /// The strips, tracks first.
    pub fn strips(self, p: &Project) -> Vec<TrackId> {
        let graph = Graph::of(p);
        let mut wanted = Vec::new();
        match self {
            StemSet::Tracks => wanted.extend(&graph.tracks),
            StemSet::Buses => wanted.extend(&graph.buses),
            StemSet::All => {
                wanted.extend(&graph.tracks);
                wanted.extend(&graph.buses);
            }
            StemSet::ByBus => {
                wanted.extend(
                    graph
                        .tracks
                        .iter()
                        .filter(|t| matches!(p.effective_output(**t), Some(Output::Master))),
                );
                wanted.extend(&graph.buses);
            }
        }
        wanted
    }
}

/// A file name that is safe on Windows, macOS and Linux.
pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let s = s.trim().trim_end_matches('.').to_string();
    let reserved = ["CON", "PRN", "AUX", "NUL"];
    let upper = s.to_ascii_uppercase();
    let com = upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT"));
    if s.is_empty() || reserved.contains(&upper.as_str()) || com {
        format!("_{s}")
    } else {
        s
    }
}

/// `path`, or when a file is already there, the first of `name-2.ext`, `name-3.ext`, ... that
/// is free (OPEN-10). `taken` holds names given out in the same export.
pub fn free_path(path: &Path, taken: &[PathBuf]) -> PathBuf {
    numbered(path, taken, true)
}

/// [`free_path`]; a file already on disk counts only when `keep` (else it is replaced).
pub fn numbered(path: &Path, taken: &[PathBuf], keep: bool) -> PathBuf {
    let used = |q: &Path| {
        (keep && q.exists())
            || taken.iter().any(|t| {
                t.to_string_lossy()
                    .eq_ignore_ascii_case(&q.to_string_lossy())
            })
    };
    if !used(path) {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut n = 2;
    loop {
        let q = path.with_file_name(format!("{stem}-{n}{ext}"));
        if !used(&q) {
            return q;
        }
        n += 1;
    }
}

/// Each track and/or bus on its own, one file each, in `dir` (EXP-013, EXP-014). A stem holds
/// the strip with its inserts, fader, pan and sends into it, and what flows into it; its output
/// level is its own (the master is left out). A muted strip is skipped and listed.
pub fn stems(
    p: &Project,
    dir: &Path,
    set: StemSet,
    o: &Options,
) -> Result<StemsReport, ExportError> {
    stems_of(p, dir, &set.strips(p), o)
}

/// What an export would refuse, found before anything is rendered: the options, and for
/// stems (`strips` given) the file name pattern and the strips. Returns the range in ticks.
pub fn check(
    p: &Project,
    o: &Options,
    strips: Option<&[TrackId]>,
) -> Result<(i64, i64), ExportError> {
    resolve(p, o)?;
    if let Some(strips) = strips {
        stem_strips(p, strips, o)?;
    }
    resolve_range(p, &o.range)
}

/// What a stem is called: the name given in `o.stem_names`, else the strip's own.
fn stem_label(p: &Project, o: &Options, id: TrackId) -> String {
    o.stem_names
        .iter()
        .find(|(t, _)| *t == id)
        .map_or_else(|| p.tracks[&id].name.clone(), |(_, n)| n.clone())
}

/// The strips to render, each with whether it is a bus.
fn stem_strips(
    p: &Project,
    strips: &[TrackId],
    o: &Options,
) -> Result<Vec<(TrackId, bool)>, ExportError> {
    let pattern = o.pattern.as_deref().unwrap_or(TRACK_TAG);
    if !names_track(pattern) {
        return err(format!(
            "ステムのファイル名のテンプレートに {TRACK_TAG} がありません（すべてのファイルが同じ名前になります）"
        ));
    }
    let mut wanted: Vec<(TrackId, bool)> = Vec::new();
    for id in strips {
        let Some(t) = p.tracks.get(id) else {
            return err(format!("{id} というトラックもバスも見つかりません"));
        };
        match t.kind {
            TrackKind::Audio | TrackKind::Instrument => wanted.push((*id, false)),
            TrackKind::Bus => wanted.push((*id, true)),
            TrackKind::Master => {
                return err(
                    "マスターはステムとして書き出せません（ミックスダウンで書き出してください）",
                );
            }
        }
    }
    if wanted.is_empty() {
        return err("書き出すトラックもバスもありません");
    }
    Ok(wanted)
}

/// The file each stem of `strips` goes to, in that order; `None` for a muted strip, which is
/// skipped.
pub fn stem_paths(
    p: &Project,
    dir: &Path,
    strips: &[TrackId],
    o: &Options,
) -> Result<Vec<Option<PathBuf>>, ExportError> {
    Ok(names_of(p, dir, &stem_strips(p, strips, o)?, o, o.format))
}

fn names_of(
    p: &Project,
    dir: &Path,
    wanted: &[(TrackId, bool)],
    o: &Options,
    format: Format,
) -> Vec<Option<PathBuf>> {
    let pattern = o.pattern.as_deref().unwrap_or(TRACK_TAG);
    let mut taken = o.busy.clone();
    let mut out = Vec::new();
    for (id, _) in wanted {
        if p.tracks[id].mute {
            out.push(None);
            continue;
        }
        let path = dir.join(format!(
            "{}.{}",
            name_from(pattern, &p.name, &stem_label(p, o, *id)),
            format.ext()
        ));
        let path = numbered(&path, &taken, !o.overwrite);
        taken.push(path.clone());
        out.push(Some(path));
    }
    out
}

/// [`stems`] of the strips given, in that order ("選んだもの").
pub fn stems_of(
    p: &Project,
    dir: &Path,
    strips: &[TrackId],
    o: &Options,
) -> Result<StemsReport, ExportError> {
    let r = resolve(p, o)?;
    let wanted = stem_strips(p, strips, o)?;
    let label = |id: &TrackId| stem_label(p, o, *id);
    // Each file's name, before any is written, so the progress lists them all (EXP-017).
    let paths: Vec<PathBuf> = names_of(p, dir, &wanted, o, r.format)
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect();
    r.plan(wanted.iter().map(|(id, _)| label(id)).collect());
    let mut builder = r.builder(o);
    let mut report = StemsReport::default();
    for ((id, is_bus), path) in wanted.into_iter().zip(paths) {
        let t = &p.tracks[&id];
        if t.mute {
            report.skipped.push(Skipped {
                name: label(&id),
                why: "ミュート中".into(),
            });
            if let Some(pr) = &r.progress {
                pr.add(r.frames());
            }
            continue;
        }
        let select = if is_bus {
            Select::Bus(id)
        } else {
            Select::Track(id)
        };
        // An MP3 stem is titled with the strip's name, on an album of the song's.
        let name = label(&id);
        let tags = Tags {
            title: name.clone(),
            album: Some(p.name.clone()).filter(|n| !n.trim().is_empty()),
        };
        report.files.push(render_file(
            &mut builder,
            p,
            select,
            &r,
            &name,
            &path,
            &tags,
        )?);
    }
    Ok(report)
}
