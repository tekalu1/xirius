//! The audition of the file browser (BRW-007): the selected audio file plays by itself, on a
//! player of its own beside the song's, and its row shows the file's waveform with what has
//! played lit.
//!
//! The browser only says which file plays ([`crate::browser::Browser::playing`]);
//! [`Studio::follow_audition`] starts and stops the player to match, from the screen's idle
//! poll, and lets go of the file when it has played to the end. The sound goes where the
//! song's goes ([`crate::transport::Transport::mode`]): nowhere in a test.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{Clip, ClipBody, Project, Track, TrackKind};
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::output::{Output, OutputMode};
use xirius_session::peaks;

use crate::browser::Browser;
use crate::draw::Pen;
use crate::studio::{Rect, Studio};
use crate::wave::{self, Look};

/// How long a player that has never said it plays is waited for before it counts as ended (an
/// engine that did not start). Long: on a busy machine the engine can take seconds to start, and
/// a player that has started ends by what the engine says, not by this.
const START_WAIT: Duration = Duration::from_secs(10);

struct Player {
    key: String,
    host: SessionHost,
    _out: Output,
    /// Frames of the file, at the engine's rate.
    end: u64,
    started: Instant,
    /// The engine has said it plays.
    heard: bool,
}

thread_local! {
    static PLAYER: RefCell<Option<Player>> = const { RefCell::new(None) };
}

/// A song of one audio track that holds the whole file from its start.
fn song_of(path: &Path, seconds: f64) -> Project {
    let mut p = Project::new("試聴");
    let track = TrackId(p.alloc());
    let clip = ClipId(p.alloc());
    let mut t = Track::new(track, TrackKind::Audio, "試聴");
    // At 120 BPM a second is two beats.
    let length = (seconds * 2.0 * f64::from(p.ppq)).ceil().max(1.0) as i64;
    t.clips.push(Clip {
        id: clip,
        name: "試聴".into(),
        color: None,
        start: 0,
        length,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: path.to_string_lossy().into_owned(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    });
    p.tracks.insert(track, Arc::new(t));
    p.root.push(Node::Track(track));
    p
}

fn start(path: &Path, key: &str, mode: OutputMode) -> Result<Player, String> {
    if mode == OutputMode::None {
        return Err("オーディオ出力がありません".into());
    }
    let info = xirius_session::decode::probe(path)?;
    let song = song_of(path, info.seconds());
    let (mut host, audio, _warnings) = SessionHost::start(&song, HostConfig::default())
        .map_err(|e| format!("試聴を開始できませんでした: {e}"))?;
    let out = Output::start(mode, audio)?.ok_or("オーディオ出力がありません")?;
    if !(host.seek_frame(0) && host.play()) {
        return Err("試聴の指示を送れませんでした".into());
    }
    let end = (info.seconds() * f64::from(host.rate())).round() as u64;
    Ok(Player {
        key: key.to_string(),
        host,
        _out: out,
        end,
        started: Instant::now(),
        heard: false,
    })
}

/// How far the file `key` has played (0 to 1), while it plays.
pub fn progress(key: &str) -> Option<f32> {
    PLAYER.with(|p| {
        let p = p.borrow();
        let p = p.as_ref().filter(|p| p.key == key)?;
        let frame = p.host.status().frame;
        Some((frame as f64 / p.end.max(1) as f64).clamp(0.0, 1.0) as f32)
    })
}

/// How far the file `key` has played as the engine says now (0 to 1), while it plays: polls
/// the player without deciding whether it has ended, so a test can watch a short file play
/// without the player letting go between two looks.
#[cfg(test)]
pub(crate) fn polled(key: &str) -> Option<f32> {
    PLAYER.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            p.host.poll();
        }
    });
    progress(key)
}

/// A player runs.
pub fn is_playing() -> bool {
    PLAYER.with(|p| p.borrow().is_some())
}

/// Stops the player.
pub fn stop() {
    PLAYER.with(|p| p.borrow_mut().take());
}

impl Studio {
    /// Starts, stops and polls the audition so it plays what the browser says. True when the
    /// browser's row should be drawn again.
    pub fn follow_audition(&mut self) -> bool {
        let want = self.browser.playing.clone();
        let mode = self.transport.mode;
        let have = PLAYER.with(|p| p.borrow().as_ref().map(|p| p.key.clone()));
        if want != have {
            if want.is_some()
                && let Some(job) = self.export.preview.take()
            {
                let _ = self
                    .song
                    .run("export.preview", serde_json::json!({"job":job,"stop":true}));
                self.transport.auditioning_export = false;
                self.refresh_popup();
            }
            stop();
            let Some(key) = want else {
                return true;
            };
            match start(Path::new(&key), &key, mode) {
                Ok(player) => PLAYER.with(|p| *p.borrow_mut() = Some(player)),
                // A screen without sound shows the file as playing and never ends it.
                Err(_) if mode == OutputMode::None => return false,
                Err(e) => {
                    self.browser.playing = None;
                    self.error = Some(e);
                }
            }
            return true;
        }
        let ended = PLAYER.with(|p| {
            let mut p = p.borrow_mut();
            let Some(pl) = p.as_mut() else {
                return false;
            };
            let s = pl.host.poll();
            pl.heard |= s.playing;
            s.frame >= pl.end || (!s.playing && (pl.heard || pl.started.elapsed() > START_WAIT))
        });
        if ended {
            stop();
            self.browser.playing = None;
        }
        have.is_some()
    }
}

/// Where the peaks of a file the browser shows are kept: the song's when it is in the song's
/// folder, the user's otherwise.
pub fn store_for(browser: &Browser, path: &Path) -> PathBuf {
    match &browser.song_dir {
        Some(d) if path.starts_with(d) => peaks::store_dir(Some(d)),
        _ => browser
            .peak_store
            .clone()
            .unwrap_or_else(|| peaks::store_dir(None)),
    }
}

/// The waveform of the audition row: bars from the file's peaks, those played lit; a flat line
/// until the peaks are made.
pub fn paint_wave(pen: &mut Pen, browser: &Browser, key: &str, r: Rect, playing: bool) {
    let path = Path::new(key);
    let ink = pen.c.wave_preview;
    let rest = pen.c.wave_rest;
    let mid = r.y + r.h / 2.0;
    let peaks = match wave::look(path, &store_for(browser, path)) {
        Look::Ready(p) => p,
        Look::Making | Look::Missing => {
            pen.fill(Rect::new(r.x, mid, r.w, 1.0), rest);
            return;
        }
    };
    let n = (r.w / 3.0).floor() as usize;
    if n == 0 {
        return;
    }
    let lit = if playing {
        progress(key).unwrap_or(0.0)
    } else {
        0.0
    };
    let per = peaks.frames as f64 / n as f64;
    let level = peaks.level_for(per);
    let half = r.h / 2.0;
    for i in 0..n {
        let p = peaks
            .reduce(level, i as f64 * per, (i + 1) as f64 * per)
            .unwrap_or_default();
        let top = mid - f32::from(p.max) / 127.0 * half;
        let bottom = mid - f32::from(p.min) / 127.0 * half;
        let h = (bottom - top).max(2.0);
        let on = (i as f32 + 0.5) / n as f32 <= lit;
        pen.round(
            Rect::new(r.x + i as f32 * 3.0, (top + bottom - h) / 2.0, 2.0, h),
            if on { ink } else { rest },
            xirius_design::Radius::Xs,
        );
    }
}

#[cfg(test)]
mod tests;
