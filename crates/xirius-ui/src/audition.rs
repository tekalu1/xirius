//! The audition of the file browser (BRW-007, BRW-020): the selected audio file plays
//! by itself, on a player of its own beside the song's, and its row shows the file's waveform
//! with what has played lit.
//!
//! The browser only says which file plays ([`crate::browser::Browser::playing`]);
//! [`Studio::follow_audition`] hands it to the player at once, from the press, and lets go of
//! the file when it has played to the end. The player ([`xirius_session::audition`]) reads the
//! file on threads of its own: the window's thread never opens it. The sound goes where the
//! song's goes ([`crate::transport::Transport::mode`]): nowhere in a test.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use xirius_session::audition::{Audition, Now, Phase, Sink};
use xirius_session::output::OutputMode;
use xirius_session::peaks;

use crate::browser::Browser;
use crate::draw::Pen;
use crate::studio::{Rect, Studio};

/// What the window keeps of the audition: what it asked for.
#[derive(Default)]
struct Shown {
    /// The file the player was told to play.
    asked: Option<String>,
}

thread_local! {
    static RELEASING: RefCell<Option<(u64, std::sync::mpsc::Receiver<()>)>> = const { RefCell::new(None) };
    static RELEASED: Cell<bool> = const { Cell::new(true) };
    static OUTPUT_EPOCH: Cell<u64> = const { Cell::new(0) };
    static PLAYER: RefCell<Option<Audition>> = const { RefCell::new(None) };
    static SHOWN: RefCell<Shown> = RefCell::new(Shown::default());
}

/// The player, made the first time it is wanted.
fn player<R>(f: impl FnOnce(&Audition) -> R) -> R {
    PLAYER.with(|p| f(p.borrow_mut().get_or_insert_with(Audition::new)))
}

/// What the player is doing, if there is one.
fn current() -> Option<Now> {
    PLAYER.with(|p| p.borrow().as_ref().map(Audition::now))
}

fn of<'a>(n: &'a Option<Now>, key: &str) -> Option<&'a Now> {
    n.as_ref()
        .filter(|n| n.path.as_deref() == Some(Path::new(key)))
}

/// How far the file `key` has played (0 to 1), from when it was asked for until it ended.
pub fn progress(key: &str) -> Option<f32> {
    let n = current();
    let n = of(&n, key)?;
    matches!(n.phase, Phase::Loading | Phase::Playing | Phase::Ended).then_some(n.played)
}

/// How far the file `key` has played, as [`progress`]: the player says it from its own
/// thread, so a test can watch a short file play without it letting go between two looks.
#[cfg(test)]
pub(crate) fn polled(key: &str) -> Option<f32> {
    progress(key)
}

/// A file is being read or plays.
pub fn is_playing() -> bool {
    current().is_some_and(|n| matches!(n.phase, Phase::Loading | Phase::Playing))
}

/// Stops the sound; the player stays open for the next file.
pub fn stop() {
    PLAYER.with(|p| {
        if let Some(a) = p.borrow().as_ref() {
            a.stop();
        }
    });
    // Keep the last asked key: follow_audition consumes the Idle result instead
    // of accidentally submitting the old browser selection again.
}

/// Closes the player and its device: before the process ends, where an output left open would
/// wait for a thread the OS has already ended, and the exit would abort.
pub fn close() {
    PLAYER.with(|p| p.borrow_mut().take());
    RELEASING.with(|r| r.borrow_mut().take());
    RELEASED.with(|r| r.set(true));
    SHOWN.with(|s| s.borrow_mut().asked = None);
}

/// Stops the sound and lets go of the device, so the song's player can open it alone.
pub fn release() {
    PLAYER.with(|p| {
        if let Some(a) = p.borrow().as_ref() {
            a.release();
        }
    });
}

/// Stops and invalidates the request before a song/output transition.
pub(crate) fn invalidate() {
    stop();
}

pub(crate) fn halt_ready() -> bool {
    PLAYER.with(|p| {
        let p = p.borrow();
        let Some(a) = p.as_ref() else {
            return true;
        };
        if matches!(a.now().phase, Phase::Loading | Phase::Playing) {
            a.stop();
        }
        a.halted()
    })
}

/// The shared audition stream is closed and acknowledged on its own thread.
pub(crate) fn release_output_ready() -> bool {
    RELEASING.with(|r| {
        let mut pending = r.borrow_mut();
        if let Some((epoch, rx)) = pending.as_ref() {
            if matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)) {
                return false;
            }
            // A later selection must get its own close acknowledgement. An old
            // acknowledgement cannot permit a newer device stream to open later.
            if *epoch == OUTPUT_EPOCH.with(Cell::get) {
                RELEASED.with(|r| r.set(true));
            }
            *pending = None;
        }
        if RELEASED.with(Cell::get) {
            return true;
        }
        let epoch = OUTPUT_EPOCH.with(Cell::get);
        *pending = PLAYER.with(|p| p.borrow().as_ref().map(|a| (epoch, a.release_async())));
        if pending.is_none() {
            RELEASED.with(|r| r.set(true));
            return true;
        }
        false
    })
}

impl Studio {
    pub(crate) fn stop_browser_audition(&mut self) {
        let active = self.browser.playing.take().is_some() || is_playing();
        if active {
            self.transport.stop_preview();
        }
    }

    /// Hands the player the file the browser says, stops it, and takes in what it says of the
    /// file: that it ended, or could not be read. True when the browser's row should be drawn
    /// again.
    pub fn follow_audition(&mut self) -> bool {
        let want = self.browser.playing.clone();
        let mode = self.transport.mode;
        let asked = SHOWN.with(|s| s.borrow().asked.clone());
        if want != asked {
            if want.is_some()
                && let Some(job) = self.export.preview.take()
            {
                let _ = self
                    .song
                    .run("export.preview", serde_json::json!({"job":job,"stop":true}));
                self.transport.auditioning_export = false;
                self.refresh_popup();
            }
            // Exclusive settings use the song's prepared engine, never a second device.
            // If preparation was deferred, resume the existing preparation path first.
            let mixed = want.is_some()
                && mode == OutputMode::Device
                && self.transport.io_prefs.text("audio.driver") == "wasapi-exclusive";
            if mixed && !self.transport.is_live() {
                if self.transport.down.is_some() {
                    self.error = self.transport.down.as_ref().map(|d| d.said.clone());
                    self.browser.playing = None;
                    return true;
                }
                if !self.transport.preparing() {
                    self.prepare_song();
                }
                if !self.transport.is_live() {
                    return true;
                }
            }
            SHOWN.with(|s| s.borrow_mut().asked = want.clone());
            let sink = match mode {
                // A screen without sound shows the file as playing and never ends it.
                OutputMode::None => None,
                OutputMode::Silent { paced } => Some(Sink::Silent { paced }),
                OutputMode::Device if mixed => self.transport.audition_port().map(Sink::Mix),
                OutputMode::Device => Some(Sink::Device {
                    prefs: self.transport.io_prefs.clone(),
                    drivers: self.transport.drivers().clone(),
                    pair: self.transport.output_pair(),
                }),
            };
            match (&want, sink) {
                (Some(key), Some(sink)) => {
                    let path = Path::new(key);
                    OUTPUT_EPOCH.with(|e| e.set(e.get() + 1));
                    RELEASED.with(|r| r.set(!matches!(sink, Sink::Device { .. })));
                    player(|a| a.play(path, &store_for(&self.browser, path), sink));
                }
                _ => {
                    PLAYER.with(|p| {
                        if let Some(a) = p.borrow().as_ref() {
                            a.stop();
                        }
                    });
                }
            }
            return true;
        }
        let Some(key) = asked else {
            return false;
        };
        if mode == OutputMode::None {
            return false;
        }
        let n = current();
        let Some(n) = of(&n, &key) else {
            // The player let go of it (the song's player took the device alone).
            self.browser.playing = None;
            SHOWN.with(|s| s.borrow_mut().asked = None);
            return true;
        };
        match &n.phase {
            Phase::Ended | Phase::Idle => {
                self.browser.playing = None;
                SHOWN.with(|s| s.borrow_mut().asked = None);
            }
            Phase::Failed(e) => {
                self.browser.playing = None;
                self.error = Some(e.clone());
                SHOWN.with(|s| s.borrow_mut().asked = None);
            }
            Phase::Loading | Phase::Playing => {}
        }
        true
    }

    /// Once a frame: follows the audition, and true while the file is read, so frames keep
    /// coming.
    pub(crate) fn audition_frame(&mut self) -> bool {
        let before = self.browser.playing.clone();
        self.follow_audition();
        let n = current();
        let reading = self
            .browser
            .playing
            .as_deref()
            .and_then(|k| of(&n, k))
            .is_some_and(|n| n.phase == Phase::Loading);
        before != self.browser.playing || reading
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

/// Whether the peaks of `key` are whole in the row, for a test or a picture to wait on.
pub fn drawn(key: &str) -> bool {
    let n = current();
    of(&n, key).is_some_and(|n| n.complete(Path::new(key)))
}

/// The waveform of the audition row: bars from the file's peaks once it is read, those played
/// lit, and a line until then.
pub fn paint_wave(pen: &mut Pen, key: &str, r: Rect, playing: bool) {
    let ink = pen.c.wave_preview;
    let rest = pen.c.wave_rest;
    let mid = r.y + r.h / 2.0;
    let now = current();
    let Some((peaks, played)) = of(&now, key)
        .filter(|n| n.complete(Path::new(key)))
        .and_then(|n| Some((n.peaks.clone()?, n.played)))
    else {
        pen.fill(Rect::new(r.x, mid, r.w, 1.0), rest);
        return;
    };
    let n = (r.w / 3.0).floor() as usize;
    if n == 0 {
        return;
    }
    let lit = if playing { played } else { 0.0 };
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
