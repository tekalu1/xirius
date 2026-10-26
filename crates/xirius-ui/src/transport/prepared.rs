//! Initial device preparation and retirement never wait on the window thread.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use xirius_core::model::TrackKind;
use xirius_session::{background::Job, starting::UnitState};
use xirius_vst3::owner::Retirement;

struct Cleanup {
    owners: Vec<Retirement>,
    previous: Vec<Arc<AtomicBool>>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        // SDK objects stay on their owning thread, even if it never returns.
        // Once audio is returned and the output stopped, that thread is safely
        // isolated; it must not prevent cancellation or another song's output.
        while self
            .owners
            .iter()
            .any(|o| !o.finished() && !o.audio_returned())
            || self.previous.iter().any(|o| !o.load(Ordering::Acquire))
        {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
struct Ready {
    player: Option<Player>,
    notice: Option<String>,
}
impl Drop for Ready {
    fn drop(&mut self) {
        if let Some(player) = self.player.take() {
            retire(player);
        }
    }
}
fn retire(player: Player) {
    let _cleanup = Cleanup {
        owners: player.host.plugins().owner_retirements(),
        previous: Vec::new(),
    };
    let Player { out, host, dir, .. } = player;
    #[cfg(any(test, feature = "loading-probe"))]
    let _ = xirius_core::io_test::before(
        "prepare_output_stop",
        dir.as_deref().unwrap_or_else(|| std::path::Path::new("")),
    );
    drop(out);
    drop(host);
    drop(dir);
}
struct Done(Arc<AtomicBool>);
impl Drop for Done {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub(super) struct Retiring {
    job: Job<()>,
    done: Arc<AtomicBool>,
}
impl Retiring {
    pub fn new(player: Player) -> Result<Self, String> {
        let done = Arc::new(AtomicBool::new(false));
        let marker = done.clone();
        let job = Job::spawn("player-retire", move || {
            let _done = Done(marker);
            retire(player);
            Ok(())
        })?;
        Ok(Self { job, done })
    }
    pub fn finished(&self) -> bool {
        self.job.poll().is_some() || self.job.finished()
    }
}

pub(super) struct Start {
    job: Job<Ready>,
    stop: Arc<AtomicBool>,
    progress: Arc<Mutex<Progress>>,
    dir: Option<PathBuf>,
    latest: std::cell::RefCell<Progress>,
    failures: Arc<Mutex<Option<Vec<xirius_session::build::PreparationFailure>>>>,
    choice: Arc<AtomicU8>,
    track_snapshot: Vec<(TrackId, Arc<xirius_core::model::Track>)>,
}
impl Start {
    pub fn new(t: &Transport, p: &Project, dir: Option<PathBuf>) -> Result<Self, String> {
        let tracks: Vec<_> = p
            .tracks
            .values()
            .filter(|t| {
                matches!(
                    t.kind,
                    TrackKind::Audio | TrackKind::Instrument | TrackKind::Bus
                )
            })
            .map(|t| (t.id, t.name.clone()))
            .collect();
        let progress = Arc::new(Mutex::new(Progress {
            units: tracks
                .iter()
                .map(|(_, name)| (name.clone(), UnitState::Waiting))
                .collect(),
            done: 0,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let latest = std::cell::RefCell::new(progress.lock().unwrap().clone());
        let stopped = stop.clone();
        let failures = Arc::new(Mutex::new(None));
        let failed = failures.clone();
        let choice = Arc::new(AtomicU8::new(0));
        let choosing = choice.clone();
        let status = progress.clone();
        let mut config = t.host_config(dir.clone());
        config.threaded_local = true;
        let track_snapshot = p.tracks.iter().map(|(id, t)| (*id, t.clone())).collect();
        let p = p.clone();
        let prefs = t.io_prefs.clone();
        let mode = t.mode;
        let drivers = t.drivers.clone();
        let pair = t.output_pair;
        let looping = t.looping;
        let metronome = t.metronome;
        let parked = t.parked;
        let mut previous: Vec<_> = t.retired.iter().map(|r| r.done.clone()).collect();
        if mode == OutputMode::Device && prefs.text("audio.driver") == "wasapi-exclusive" {
            previous.push(crate::audition::release_for_preparation()?);
        }
        let player_dir = dir.clone();
        let job = Job::spawn("song-prepare", move || {
            let mut cleanup = Cleanup {
                owners: Vec::new(),
                previous,
            };
            let mut starting = Starting::new(&p, config);
            while !starting.step() {
                if stopped.load(Ordering::Acquire) {
                    cleanup.owners = starting.owner_retirements();
                    return Err("中止しました".into());
                }
                if !stopped.load(Ordering::Acquire)
                    && let Some((name, _)) = starting
                        .progress()
                        .units
                        .iter()
                        .find(|(_, s)| *s == UnitState::Working)
                {
                    let mut progress = status.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some((_, state)) = progress.units.iter_mut().find(|(n, _)| n == name) {
                        *state = UnitState::Working;
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            cleanup.owners = starting.owner_retirements();
            if stopped.load(Ordering::Acquire) {
                return Err("中止しました".into());
            }
            let mut observe = |id, done| {
                if let Some(index) = tracks.iter().position(|(track, _)| *track == id) {
                    let mut progress = status.lock().unwrap_or_else(|e| e.into_inner());
                    for (_, s) in &mut progress.units {
                        if *s == UnitState::Working {
                            *s = UnitState::Waiting;
                        }
                    }
                    progress.units[index].1 = if done {
                        UnitState::Done
                    } else {
                        UnitState::Working
                    };
                    progress.done = progress
                        .units
                        .iter()
                        .filter(|(_, s)| *s == UnitState::Done)
                        .count();
                }
            };
            let (mut host, mut audio, mut track_failures) = starting
                .finish_prepared(&p, &mut observe)
                .map_err(|e| e.to_string())?;
            while !track_failures.is_empty() {
                {
                    let mut progress = status.lock().unwrap_or_else(|e| e.into_inner());
                    for (index, (id, _)) in tracks.iter().enumerate() {
                        if track_failures.iter().any(|f| f.track == Some(*id)) {
                            progress.units[index].1 = UnitState::Waiting;
                        }
                    }
                    progress.done = progress.units.len().saturating_sub(track_failures.len());
                }
                *failed.lock().unwrap_or_else(|e| e.into_inner()) = Some(track_failures);
                while choosing.load(Ordering::Acquire) == 0 {
                    if stopped.load(Ordering::Acquire) {
                        return Err("中止しました".into());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                if choosing.load(Ordering::Acquire) == 1 {
                    break;
                }
                choosing.store(0, Ordering::Release);
                track_failures = host.retry_prepared(&mut audio, &p, &mut observe, || {
                    stopped.load(Ordering::Acquire)
                })?;
                cleanup.owners = host.plugins().owner_retirements();
            }
            if stopped.load(Ordering::Acquire) {
                return Err("中止しました".into());
            }
            host.set_windowed();
            host.drivers = drivers.clone();
            host.set_looping(&p, looping);
            host.controller()
                .send(Command::OutputChannels(pair))
                .map_err(|_| "出力を設定できません")?;
            let mut settings = xirius_prefs::Settings::memory();
            settings.prefs = prefs.clone();
            host.io_settings = Some(settings.shared());
            if mode == OutputMode::Device {
                host.set_midi_out(Some(xirius_session::midi_out::system_maker()));
            }
            while cleanup.previous.iter().any(|d| !d.load(Ordering::Acquire)) {
                std::thread::sleep(Duration::from_millis(2));
            }
            if stopped.load(Ordering::Acquire) {
                return Err("中止しました".into());
            }
            let opened = Output::open(mode, audio, &prefs, &drivers)?;
            let out = opened.output.ok_or("オーディオ出力がありません")?;
            if metronome {
                send_click(&mut host, &p, true);
            }
            let frame = host.tempo_map().frame_at(parked, f64::from(host.rate()));
            host.seek_frame(frame);
            cleanup.owners.clear();
            Ok(Ready {
                player: Some(Player {
                    host,
                    out,
                    dir: player_dir,
                    threaded: true,
                }),
                notice: opened.notice,
            })
        })?;
        Ok(Self {
            job,
            stop,
            progress,
            dir,
            latest,
            failures,
            choice,
            track_snapshot,
        })
    }
    pub fn progress(&self) -> Progress {
        if let Ok(progress) = self.progress.try_lock() {
            *self.latest.borrow_mut() = progress.clone();
        }
        self.latest.borrow().clone()
    }
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Release);
        self.job.cancel();
    }
}
impl Drop for Start {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Transport {
    pub(crate) fn background_starting(&self) -> bool {
        self.prepared_start.is_some()
    }
    pub(crate) fn cancel_preparation(&mut self) -> bool {
        if let Some(start) = &self.prepared_start {
            start.cancel();
            true
        } else {
            false
        }
    }
    pub(crate) fn continue_preparation(&self) -> bool {
        let Some(start) = &self.prepared_start else {
            return false;
        };
        let Ok(failures) = start.failures.try_lock() else {
            return false;
        };
        if failures.as_ref().is_none_or(Vec::is_empty) || start.stop.load(Ordering::Acquire) {
            return false;
        }
        start.choice.store(1, Ordering::Release);
        true
    }

    pub(crate) fn retry_preparation(&mut self, p: &Project, dir: Option<PathBuf>) -> bool {
        // Edits are blocked while this immutable song is preparing. A partial
        // candidate stays with its worker so healthy audio/plugin instances are
        // carried into the retried graph. A replaced song gets a separate job.
        if let Some(start) = &self.prepared_start
            && start.dir == dir
            && start.track_snapshot.len() == p.tracks.len()
            && start
                .track_snapshot
                .iter()
                .all(|(id, t)| p.tracks.get(id).is_some_and(|now| Arc::ptr_eq(t, now)))
            && !start.stop.load(Ordering::Acquire)
            && let Ok(mut failed) = start.failures.try_lock()
            && failed.as_ref().is_some_and(|f| !f.is_empty())
        {
            *failed = None;
            start.choice.store(2, Ordering::Release);
            return true;
        }
        // The old job owns its candidate until its worker retires it. Its channel
        // is never polled by this attempt, including any already queued reply.
        if let Some(start) = self.prepared_start.take() {
            start.cancel();
        }
        self.begin_start(p, dir)
    }
    pub(super) fn step_prepared(&mut self, dir: Option<PathBuf>) -> Option<StartStep> {
        let start = self.prepared_start.as_ref()?;
        if start.dir != dir {
            start.cancel();
        }
        if start.stop.load(Ordering::Acquire) {
            if !start.job.finished() {
                return Some(StartStep::Going(start.progress()));
            }
            self.prepared_start = None;
            return Some(StartStep::Cancelled);
        }
        if start.choice.load(Ordering::Acquire) == 0
            && let Ok(failures) = start.failures.try_lock()
            && let Some(failures) = failures.as_ref()
        {
            return Some(StartStep::Failed(failures.clone()));
        }
        if let Some(result) = start.job.poll() {
            let result = result.map(|mut ready| {
                self.player = ready.player.take();
                self.driver_notice = ready.notice.take();
                self.down = None;
            });
            self.prepared_start = None;
            return Some(StartStep::Done(result));
        }
        Some(StartStep::Going(start.progress()))
    }
}

#[cfg(test)]
#[path = "prepared_tests.rs"]
pub(super) mod tests;
