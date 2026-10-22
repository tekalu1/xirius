//! MP3 files (EXP-005, LIC-006): LAME encodes them and symphonia reads them back.
//!
//! LAME is LGPL, so it is not linked into Xirius (MIT OR Apache-2.0) and is not a cargo
//! dependency: its own dynamic library (`lame_enc.dll`, `libmp3lame.dylib`, `libmp3lame.so`)
//! sits next to the executable and is loaded when an MP3 is first written. Replacing that file
//! with another build of the same name changes the encoder from the next start (LIC-015).
//! When it cannot be loaded only MP3 is refused; WAV and FLAC do not need it.
//!
//! A file is constant bit rate, joint stereo, at the rate of the render (no resampling), with an
//! ID3v2.3 tag of its title and album first and LAME's info frame (the encoder delay and
//! padding, for gapless playback) after it. Loudness and true peak are measured on the file read
//! back, the sound a listener gets, not on the render before it was encoded.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The encoder's file, next to the executable.
#[cfg(windows)]
pub const LAME_FILE: &str = "lame_enc.dll";
#[cfg(target_os = "macos")]
pub const LAME_FILE: &str = "libmp3lame.dylib";
#[cfg(not(any(windows, target_os = "macos")))]
pub const LAME_FILE: &str = "libmp3lame.so";

/// The environment variable that names the encoder's file instead.
pub const LAME_ENV: &str = "XIRIUS_LAME";

/// The constant bit rates offered, kbps.
pub const BITRATES: [u16; 4] = [128, 192, 256, 320];
pub const DEFAULT_BITRATE: u16 = 320;
/// The sample rates an MP3 file is written at (MPEG-1 Layer III).
pub const RATES: [u32; 3] = [32_000, 44_100, 48_000];
/// The highest of [`RATES`]: a song above it is written at it.
pub const MAX_RATE: u32 = 48_000;

/// Why MP3 cannot be chosen, when the encoder cannot be loaded.
pub const CANNOT_LOAD: &str = "MP3 エンコーダーを読み込めません。アプリを入れ直してください";
/// Why a rate above 48 kHz cannot be chosen for MP3.
pub const OVER_48K: &str = "MP3 は 48 kHz までです";
/// An export whose MP3 could not be made.
pub const ENCODE_FAILED: &str = "MP3 に変換できませんでした";

/// The encoder Xirius is built and tested with, and where its source is (LIC-015).
pub const LAME_VERSION: &str = "3.100";
pub const LAME_LICENSE_ID: &str = "LGPL-2.0-or-later";
pub const LAME_SOURCE_URL: &str =
    "https://downloads.sourceforge.net/project/lame/lame/3.100/lame-3.100.tar.gz";
/// The same tarball, attached to every release (LIC-035): where the app points beside the
/// project's own place, so the source stays reachable if that page moves.
pub const LAME_RELEASE_URL: &str =
    "https://github.com/tekalu1/xirius/releases/latest/download/lame-3.100.tar.gz";
pub const LAME_SOURCE_SHA256: &str =
    "ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e";
/// LAME's licence, in full.
pub const LAME_LICENSE: &str = include_str!("../../../assets/lame/COPYING.txt");

/// What an MP3 file is titled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    /// TIT2: the song's name for a mixdown, the track's for a stem.
    pub title: String,
    /// TALB: the song's name, for a stem.
    pub album: Option<String>,
}

/// Where the encoder is looked for, in order.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os(LAME_ENV).filter(|p| !p.is_empty()) {
        out.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        out.push(dir.join(LAME_FILE));
        #[cfg(not(any(windows, target_os = "macos")))]
        out.push(dir.join("libmp3lame.so.0"));
        #[cfg(target_os = "macos")]
        out.push(dir.join("../Frameworks").join(LAME_FILE));
        // Tests and examples run from target/<profile>/deps or /examples.
        if dir
            .file_name()
            .is_some_and(|n| n == "deps" || n == "examples")
            && let Some(up) = dir.parent()
        {
            out.push(up.join(LAME_FILE));
        }
    }
    out
}

/// The encoder's file: the one [`LAME_ENV`] names, there or not; else the first of
/// [`candidates`] that is there, else where it should be.
pub fn locate() -> PathBuf {
    if let Some(p) = std::env::var_os(LAME_ENV).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let all = candidates();
    all.iter()
        .find(|p| p.is_file())
        .or(all.first())
        .cloned()
        .unwrap_or_else(|| PathBuf::from(LAME_FILE))
}

type Gfp = *mut c_void;

/// The functions of LAME's API that are used.
struct Api {
    init: unsafe extern "C" fn() -> Gfp,
    set_in_samplerate: unsafe extern "C" fn(Gfp, c_int) -> c_int,
    set_out_samplerate: unsafe extern "C" fn(Gfp, c_int) -> c_int,
    set_num_channels: unsafe extern "C" fn(Gfp, c_int) -> c_int,
    set_brate: unsafe extern "C" fn(Gfp, c_int) -> c_int,
    set_vbr: unsafe extern "C" fn(Gfp, c_int) -> c_int,
    set_mode: unsafe extern "C" fn(Gfp, c_int) -> c_int,
    set_quality: unsafe extern "C" fn(Gfp, c_int) -> c_int,
    set_write_vbr_tag: unsafe extern "C" fn(Gfp, c_int) -> c_int,
    set_write_id3tag_automatic: unsafe extern "C" fn(Gfp, c_int),
    init_params: unsafe extern "C" fn(Gfp) -> c_int,
    encode_float: unsafe extern "C" fn(Gfp, *const f32, *const f32, c_int, *mut u8, c_int) -> c_int,
    encode_flush: unsafe extern "C" fn(Gfp, *mut u8, c_int) -> c_int,
    lametag_frame: unsafe extern "C" fn(Gfp, *mut u8, usize) -> usize,
    close: unsafe extern "C" fn(Gfp) -> c_int,
}

/// A loaded encoder.
pub struct Lame {
    api: Api,
    /// LAME's own version, as it reports it.
    pub version: String,
    pub path: PathBuf,
    /// One encode at a time: LAME fills some shared tables the first time round.
    lock: Mutex<()>,
    /// Kept loaded for as long as the program runs ([`load`] keeps every one it loads).
    _lib: libloading::Library,
}

impl std::fmt::Debug for Lame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lame")
            .field("version", &self.version)
            .field("path", &self.path)
            .finish()
    }
}

/// The encoders loaded so far, by path: each stays loaded until the program ends.
static LOADED: Mutex<Vec<(PathBuf, Arc<Lame>)>> = Mutex::new(Vec::new());

/// The encoder at `path` ([`locate`] when `None`), loaded once. The error is why it could not
/// be: the file is missing, is not a library, or lacks a function.
pub fn load(path: Option<&Path>) -> Result<Arc<Lame>, String> {
    let path = path.map_or_else(locate, Path::to_path_buf);
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, l)) = loaded.iter().find(|(p, _)| *p == path) {
        return Ok(l.clone());
    }
    let lame = Arc::new(open(&path)?);
    loaded.push((path, lame.clone()));
    Ok(lame)
}

fn open(path: &Path) -> Result<Lame, String> {
    #[cfg(not(any(windows, target_os = "macos")))]
    let system = !path.is_file() && path.file_name().is_some_and(|n| n == LAME_FILE);
    #[cfg(any(windows, target_os = "macos"))]
    let system = false;
    if !path.is_file() && !system {
        return Err(format!("{} がありません", path.display()));
    }
    // SAFETY: loading a library runs its initialisers. The file is the one the app ships next
    // to itself (or the one the user put there instead, LGPL's right); LAME's do nothing else.
    let lib = unsafe {
        if system {
            libloading::Library::new("libmp3lame.so.0")
        } else {
            libloading::Library::new(path)
        }
    }
    .map_err(|e| format!("{} を読み込めません（{e}）", path.display()))?;
    macro_rules! get {
        ($name:literal) => {
            // SAFETY: the type is the one lame.h declares for the function of that name.
            *unsafe { lib.get(concat!($name, "\0").as_bytes()) }
                .map_err(|_| format!("{} に {} がありません", path.display(), $name))?
        };
    }
    let api = Api {
        init: get!("lame_init"),
        set_in_samplerate: get!("lame_set_in_samplerate"),
        set_out_samplerate: get!("lame_set_out_samplerate"),
        set_num_channels: get!("lame_set_num_channels"),
        set_brate: get!("lame_set_brate"),
        set_vbr: get!("lame_set_VBR"),
        set_mode: get!("lame_set_mode"),
        set_quality: get!("lame_set_quality"),
        set_write_vbr_tag: get!("lame_set_bWriteVbrTag"),
        set_write_id3tag_automatic: get!("lame_set_write_id3tag_automatic"),
        init_params: get!("lame_init_params"),
        encode_float: get!("lame_encode_buffer_ieee_float"),
        encode_flush: get!("lame_encode_flush"),
        lametag_frame: get!("lame_get_lametag_frame"),
        close: get!("lame_close"),
    };
    let version_fn: unsafe extern "C" fn() -> *const c_char = get!("get_lame_version");
    // SAFETY: LAME returns a static, NUL-terminated string.
    let version = unsafe {
        let s = version_fn();
        if s.is_null() {
            String::new()
        } else {
            CStr::from_ptr(s).to_string_lossy().into_owned()
        }
    };
    Ok(Lame {
        api,
        version,
        path: path.to_path_buf(),
        lock: Mutex::new(()),
        _lib: lib,
    })
}

/// LAME's `vbr_off` and `JOINT_STEREO`.
const VBR_OFF: c_int = 0;
const JOINT_STEREO: c_int = 1;
/// 2: near-best quality, a few times faster than real time even so.
const QUALITY: c_int = 2;
/// Frames handed to LAME at a time.
const CHUNK: usize = 8192;

/// Frees LAME's state however the encode ends.
struct Handle<'a>(&'a Api, Gfp);

impl Drop for Handle<'_> {
    fn drop(&mut self) {
        // SAFETY: the handle came from lame_init and is closed once.
        unsafe { (self.0.close)(self.1) };
    }
}

impl Lame {
    /// The whole file: the ID3v2 tag, then the frames, the first of which is LAME's info frame.
    /// `rate` must be one of [`RATES`] and `kbps` one of [`BITRATES`].
    pub fn encode(
        &self,
        left: &[f32],
        right: &[f32],
        rate: u32,
        kbps: u16,
        tags: &Tags,
    ) -> Result<Vec<u8>, String> {
        if !RATES.contains(&rate) {
            return Err(format!("{rate} Hz の MP3 は作れません"));
        }
        if !BITRATES.contains(&kbps) {
            return Err(format!("{kbps} kbps の MP3 は作れません"));
        }
        let _one = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let a = &self.api;
        // SAFETY: every call gets the handle lame_init gave, which `Handle` closes at the end;
        // the buffers are as long as the lengths passed with them.
        unsafe {
            let gfp = (a.init)();
            if gfp.is_null() {
                return Err("LAME を初期化できません".into());
            }
            let h = Handle(a, gfp);
            (a.set_in_samplerate)(h.1, rate as c_int);
            (a.set_out_samplerate)(h.1, rate as c_int);
            (a.set_num_channels)(h.1, 2);
            (a.set_vbr)(h.1, VBR_OFF);
            (a.set_brate)(h.1, c_int::from(kbps));
            (a.set_mode)(h.1, JOINT_STEREO);
            (a.set_quality)(h.1, QUALITY);
            (a.set_write_vbr_tag)(h.1, 1);
            (a.set_write_id3tag_automatic)(h.1, 0);
            if (a.init_params)(h.1) < 0 {
                return Err("LAME の設定が無効です".into());
            }
            let head = id3v2(tags);
            let mut out = head.clone();
            out.reserve(left.len() * usize::from(kbps) / 8 * 1000 / rate as usize + 8192);
            let mut buf = vec![0u8; CHUNK * 5 / 4 + 7200];
            let (mut l, mut r) = (vec![0f32; CHUNK], vec![0f32; CHUNK]);
            for (cl, cr) in left.chunks(CHUNK).zip(right.chunks(CHUNK)) {
                let n = cl.len().min(cr.len());
                for i in 0..n {
                    l[i] = cl[i].clamp(-1.0, 1.0);
                    r[i] = cr[i].clamp(-1.0, 1.0);
                }
                let got = (a.encode_float)(
                    h.1,
                    l.as_ptr(),
                    r.as_ptr(),
                    n as c_int,
                    buf.as_mut_ptr(),
                    buf.len() as c_int,
                );
                if got < 0 {
                    return Err(format!("LAME がエンコードできません（{got}）"));
                }
                out.extend_from_slice(&buf[..got as usize]);
            }
            let got = (a.encode_flush)(h.1, buf.as_mut_ptr(), buf.len() as c_int);
            if got < 0 {
                return Err(format!("LAME がエンコードできません（{got}）"));
            }
            out.extend_from_slice(&buf[..got as usize]);
            // The info frame, written now that the length is known, over the empty one LAME
            // put first.
            let n = (a.lametag_frame)(h.1, buf.as_mut_ptr(), buf.len());
            if n > 0 && n <= buf.len() && head.len() + n <= out.len() {
                out[head.len()..head.len() + n].copy_from_slice(&buf[..n]);
            }
            Ok(out)
        }
    }
}

/// An ID3v2.3 tag of TIT2 and TALB, in UTF-16 (which every player reads; v2.3 has no UTF-8).
pub fn id3v2(tags: &Tags) -> Vec<u8> {
    let mut frames = Vec::new();
    let mut frame = |id: &[u8; 4], text: &str| {
        if text.is_empty() {
            return;
        }
        let mut body = vec![1u8, 0xFF, 0xFE];
        for u in text.encode_utf16() {
            body.extend_from_slice(&u.to_le_bytes());
        }
        frames.extend_from_slice(id);
        frames.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frames.extend_from_slice(&[0, 0]);
        frames.extend_from_slice(&body);
    };
    frame(b"TIT2", &tags.title);
    if let Some(album) = &tags.album {
        frame(b"TALB", album);
    }
    if frames.is_empty() {
        return Vec::new();
    }
    let n = frames.len() as u32;
    let mut out = b"ID3\x03\x00\x00".to_vec();
    // The size is "syncsafe": 7 bits a byte.
    out.extend_from_slice(&[
        (n >> 21 & 0x7F) as u8,
        (n >> 14 & 0x7F) as u8,
        (n >> 7 & 0x7F) as u8,
        (n & 0x7F) as u8,
    ]);
    out.extend_from_slice(&frames);
    out
}

/// An MP3 file read back.
#[derive(Debug, Clone, Default)]
pub struct Decoded {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub rate: u32,
    pub title: Option<String>,
    pub album: Option<String>,
}

/// Decodes an MP3 file, without the encoder's delay and padding (LAME's info frame says how
/// much), so it lines up with the sound it was made from.
pub fn read(path: &Path) -> Result<Decoded, String> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::TrackType;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::StandardTag;

    let file =
        std::fs::File::open(path).map_err(|e| format!("{} を開けません: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, Default::default(), Default::default())
        .map_err(|e| format!("{} を MP3 として読めません: {e}", path.display()))?;
    let mut out = Decoded::default();
    let mut tags = |rev: &symphonia::core::meta::MetadataRevision| {
        let all = rev
            .media
            .tags
            .iter()
            .chain(rev.per_track.iter().flat_map(|t| t.metadata.tags.iter()));
        for t in all {
            match &t.std {
                Some(StandardTag::TrackTitle(s)) => out.title = Some(s.to_string()),
                Some(StandardTag::Album(s)) => out.album = Some(s.to_string()),
                _ => {}
            }
        }
    };
    {
        let mut md = format.metadata();
        loop {
            if let Some(rev) = md.current() {
                tags(rev);
            }
            if md.pop().is_none() {
                break;
            }
        }
    }
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| format!("{} に音がありません", path.display()))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|c| c.audio())
        .ok_or_else(|| format!("{} の形式が分かりません", path.display()))?
        .clone();
    let rate = params.sample_rate.unwrap_or(0);
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default().gapless(true))
        .map_err(|e| format!("{} をデコードできません: {e}", path.display()))?;
    let mut planes: Vec<Vec<f32>> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("{} を読めません: {e}", path.display())),
        };
        if packet.track_id != id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let r = buf.spec().rate();
                if r != 0 {
                    out.rate = r;
                }
                buf.copy_to_vecs_planar::<f32>(&mut planes);
                match planes.as_slice() {
                    [mono] => {
                        out.left.extend_from_slice(mono);
                        out.right.extend_from_slice(mono);
                    }
                    [l, r, ..] => {
                        out.left.extend_from_slice(l);
                        out.right.extend_from_slice(r);
                    }
                    [] => {}
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("{} をデコードできません: {e}", path.display())),
        }
    }
    if out.rate == 0 {
        out.rate = rate;
    }
    Ok(out)
}
