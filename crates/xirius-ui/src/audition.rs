//! The audition of the file browser (BRW-007, BRW-020..BRW-023): the selected audio file plays
//! by itself, on a player of its own beside the song's, and its row shows the file's waveform
//! with what has played lit.
//!
//! The browser only says which file plays ([`crate::browser::Browser::playing`]);
//! [`Studio::follow_audition`] hands it to the player at once, from the press, and lets go of
//! the file when it has played to the end. The player ([`xirius_session::audition`]) reads the
//! file on threads of its own: the window's thread never opens it. The sound goes where the
//! song's goes ([`crate::transport::Transport::mode`]): nowhere in a test.
//!
//! While the file is read, the row says so in itself (BRW-023): a row of dots stands for the
//! waveform, the bars come in from the middle as their part of the file is read, and when
//! nothing is heard after [`SPIN_AFTER`] a spinner turns round the stop square. With motion
//! reduced the dots and the spinner stand still and the bars come in whole.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_session::audition::{Audition, Now, Phase, Sink};
use xirius_session::output::OutputMode;
use xirius_session::peaks::{self, Peaks};

use crate::browser::{BRow, Browser};
use crate::draw::Pen;
use crate::studio::{Rect, Studio};

/// What the stop button says while the file is read.
pub const READING: &str = "読み込んでいます";
/// What a screen reader calls the button when a press plays the file, and when it stops it.
pub const PLAY_LABEL: &str = "ファイルを試聴";
pub const STOP_LABEL: &str = "停止";
/// How long a file asked for may be silent before the spinner says it is being read.
pub const SPIN_AFTER: Duration = Duration::from_millis(150);
/// How long a bar takes to grow to its height, and how much later each next one starts.
const GROW: Duration = Duration::from_millis(180);
const GROW_STEP: Duration = Duration::from_millis(4);
/// One breath of the dots, and how much later each next dot breathes.
const BREATHE_MS: f32 = 1200.0;
const BREATHE_STEP_MS: f32 = 18.0;
/// The audio rows read ahead below and above the chosen one (BRW-022).
const BELOW: usize = 3;
const ABOVE: usize = 1;

/// What the window keeps of the audition: what it asked for, and the bars of the row drawn.
#[derive(Default)]
struct Shown {
    /// The file the player was told to play.
    asked: Option<String>,
    /// The file the row's bars are of, the press they are of, and their peaks (kept when it
    /// stops, so the row still shows them).
    key: Option<String>,
    ask: u64,
    peaks: Option<Arc<Peaks>>,
    covered: u64,
    whole: Option<u64>,
    /// When each bar of the row came in, while it grows; by the bars there are room for.
    appear: Vec<Option<Instant>>,
    /// The bars drawn so far.
    drawn: usize,
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

/// The file `key` has been asked for, and after [`SPIN_AFTER`] nothing is heard yet.
pub fn waiting(key: &str) -> bool {
    let n = current();
    of(&n, key).is_some_and(|n| {
        n.phase == Phase::Loading && n.asked.is_some_and(|t| t.elapsed() >= SPIN_AFTER)
    })
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

/// A preparation worker can wait for the preview stream without borrowing UI
/// thread-local state or asking the window to poll a close acknowledgement.
pub(crate) fn release_for_preparation() -> Result<Arc<std::sync::atomic::AtomicBool>, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let done = Arc::new(AtomicBool::new(false));
    let ack = PLAYER.with(|p| p.borrow().as_ref().map(Audition::release_async));
    let Some(ack) = ack else {
        done.store(true, Ordering::Release);
        return Ok(done);
    };
    let finished = done.clone();
    // The job owns only an acknowledgement. Dropping its receipt does not join
    // the preview's output thread or move a device back to the window.
    xirius_session::background::Job::spawn("preview-prepare-release", move || {
        let _ = ack.recv();
        finished.store(true, Ordering::Release);
        Ok(())
    })?;
    Ok(done)
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
                    self.read_ahead(key);
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

    /// Once a frame: follows the audition, and true while its row moves (the file is read, or
    /// its bars grow), so frames keep coming.
    pub(crate) fn audition_frame(&mut self) -> bool {
        let before = self.browser.playing.clone();
        self.follow_audition();
        let n = current();
        let key = self.browser.playing.as_deref();
        let now = key.and_then(|k| of(&n, k));
        let reading = now.is_some_and(|n| n.phase == Phase::Loading);
        if reading {
            self.browser.spin = self.spin();
        }
        let at = Instant::now();
        let growing = SHOWN.with(|s| s.borrow().appear.iter().flatten().any(|t| at < *t + GROW));
        before != self.browser.playing || reading || growing
    }

    /// Has the player read the audio rows next to `key` ahead: [`BELOW`] under it and
    /// [`ABOVE`] over it, the nearest first.
    fn read_ahead(&self, key: &str) {
        let rows = if self.pool_shown() {
            self.browser.pool_rows()
        } else {
            self.browser.file_rows()
        };
        let files: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                BRow::File { key, .. }
                | BRow::Pool {
                    key,
                    missing: false,
                    ..
                } if crate::drop_audio::is_audio(Path::new(key)) => Some(key.as_str()),
                _ => None,
            })
            .collect();
        let Some(i) = files.iter().position(|k| *k == key) else {
            return;
        };
        let below = files[i + 1..].iter().take(BELOW);
        let above = files[..i].iter().rev().take(ABOVE);
        let list: Vec<(PathBuf, PathBuf)> = below
            .chain(above)
            .map(|k| {
                let p = Path::new(k);
                (p.to_path_buf(), store_for(&self.browser, p))
            })
            .collect();
        player(|a| a.prefetch(list));
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

impl Shown {
    /// Takes in what the player says of `key`, for a row with room for `bars` bars, and gives
    /// how many of them have their part of the file read.
    fn take(&mut self, key: &str, now: Option<&Now>, bars: usize, at: Instant) -> usize {
        // Another file, or the same one pressed again, starts over: no bars of an earlier
        // press stay in the row.
        let ask = now.map_or(self.ask, |n| n.ask);
        if self.key.as_deref() != Some(key) || ask != self.ask {
            *self = Shown {
                asked: self.asked.take(),
                key: Some(key.to_string()),
                ask,
                ..Shown::default()
            };
        }
        if self.appear.len() != bars {
            self.appear = vec![None; bars];
            self.drawn = self.count(bars);
        }
        if let Some(n) = now
            && let Some(p) = &n.peaks
            && !self.peaks.as_ref().is_some_and(|q| Arc::ptr_eq(p, q))
        {
            let first = self.peaks.is_none();
            self.peaks = Some(p.clone());
            self.covered = n.covered;
            self.whole = n.whole;
            let shown = self.count(bars);
            // Bars from what was kept come in whole, without an appearance animation.
            let whole = first && n.complete(Path::new(key));
            for (j, i) in (self.drawn.min(shown)..shown).enumerate() {
                self.appear[i] = (!whole).then(|| at + GROW_STEP * j as u32);
            }
            self.drawn = shown;
        }
        self.count(bars)
    }

    fn count(&self, bars: usize) -> usize {
        match (self.peaks.as_ref(), self.whole) {
            (Some(_), Some(w)) if w > 0 => {
                ((bars as f64 * self.covered as f64 / w as f64).floor() as usize).min(bars)
            }
            _ => 0,
        }
    }
}

/// How far a bar that came in at `appear` has grown at `at`: its height and its opacity.
fn grown(appear: Option<Instant>, at: Instant, motion: xirius_design::Motion) -> (f32, f32) {
    if motion.reduced {
        return (1.0, 1.0);
    }
    let Some(t) = appear else {
        return (1.0, 1.0);
    };
    let x = if at <= t {
        0.0
    } else {
        ((at - t).as_secs_f32() / GROW.as_secs_f32()).min(1.0)
    };
    let e = 1.0 - (1.0 - x) * (1.0 - x);
    (0.12 + 0.88 * e, 0.35 + 0.65 * e)
}

/// The opacity of dot `i` at `ms`: a slow wave of light running left to right.
fn breath(i: usize, ms: u64, motion: xirius_design::Motion) -> f32 {
    let ms = if motion.reduced {
        0.0
    } else {
        (ms % BREATHE_MS as u64) as f32
    };
    let x = ((ms - i as f32 * BREATHE_STEP_MS).rem_euclid(BREATHE_MS)) / BREATHE_MS;
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let up = if x < 0.4 {
        ease(x / 0.4)
    } else {
        1.0 - ease((x - 0.4) / 0.6)
    };
    xirius_design::WaveColor::DOT_MIN
        + (xirius_design::WaveColor::DOT_MAX - xirius_design::WaveColor::DOT_MIN) * up
}

/// The play button's mark: play, stop, or, while nothing is heard yet, a smaller stop square
/// with the spinner turning round it (pressing it still stops).
pub fn paint_button(pen: &mut Pen, key: &str, playing: bool, r: Rect, turn: f32) {
    let ink = pen.c.on_fill;
    if playing && waiting(key) {
        // On the 24 grid of the icons the stop square is 12 wide; inside the spinner's arc it
        // is 7, so the stop mark is drawn at 7/12 of the box, round the same middle.
        let w = r.w * 7.0 / 12.0;
        pen.icon(
            "stop",
            Rect::new(r.x + (r.w - w) / 2.0, r.y + (r.h - w) / 2.0, w, w),
            ink,
        );
        pen.icon_turned("spinner", r, ink, turn);
        return;
    }
    pen.icon(if playing { "stop" } else { "play" }, r, ink);
}

/// The waveform of the audition row: bars from the file's peaks, those played lit, and dots
/// where the file is not read yet, breathing while it is read.
pub fn paint_wave(pen: &mut Pen, key: &str, r: Rect, playing: bool, motion: xirius_design::Motion) {
    let bars = (r.w / 3.0).floor() as usize;
    if bars == 0 {
        return;
    }
    let at = Instant::now();
    let now = current();
    let now = of(&now, key);
    let reading = playing && now.is_some_and(|n| n.phase == Phase::Loading);
    let (shown, peaks, whole, appear) = SHOWN.with(|s| {
        let mut s = s.borrow_mut();
        let shown = s.take(key, now, bars, at);
        (shown, s.peaks.clone(), s.whole, s.appear.clone())
    });
    let lit = if playing {
        now.filter(|n| n.phase != Phase::Loading)
            .map_or(0.0, |n| n.played)
    } else {
        0.0
    };
    let ink = pen.c.wave_preview;
    let rest = pen.c.wave_rest;
    let mid = r.y + r.h / 2.0;
    let half = r.h / 2.0;
    let ms = xirius_core::store::now_ms();
    let per = whole.unwrap_or(0) as f64 / bars as f64;
    let level = peaks.as_ref().map_or(0, |p| p.level_for(per));
    for (i, appear) in appear.iter().enumerate().take(bars) {
        let x = r.x + i as f32 * 3.0;
        if let Some(p) = peaks.as_ref().filter(|_| i < shown) {
            let p = p
                .reduce(level, i as f64 * per, (i + 1) as f64 * per)
                .unwrap_or_default();
            let top = mid - f32::from(p.max) / 127.0 * half;
            let bottom = mid - f32::from(p.min) / 127.0 * half;
            let (scale, alpha) = grown(*appear, at, motion);
            let h = (bottom - top).max(2.0) * scale;
            let on = (i as f32 + 0.5) / bars as f32 <= lit;
            let color = if on { ink } else { rest };
            pen.round(
                Rect::new(x, (top + bottom - h) / 2.0, 2.0, h),
                color.opacity(color.a * alpha),
                xirius_design::Radius::Xs,
            );
            continue;
        }
        let alpha = if reading {
            breath(i, ms, motion)
        } else {
            xirius_design::WaveColor::DOT_REST
        };
        pen.round(
            Rect::new(x, mid - 1.0, 2.0, 2.0),
            rest.opacity(rest.a * alpha),
            xirius_design::Radius::Xs,
        );
    }
}

#[cfg(test)]
mod tests;
