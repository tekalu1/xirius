//! A nonblocking proxy for one ARA document in the plugin host (PLG-078).

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant};

use crate::document::{FactoryInfo, Note};
use crate::host::{Request, SourceAudio};
use crate::remote::{self, Command as WireCommand, Reply, Snapshot, WireClip, WireSong};
use crate::service::{
    CachedArchive, Caption, ClipState, Phase, Placement, Probe, Rendered, Song, Stored,
};
use crate::shared_audio::SharedAudio;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

enum Job {
    Sync(Song),
    Wire(WireCommand),
    Store(bool, mpsc::Sender<Option<(Vec<u8>, String)>>),
    Notes(u64, bool, mpsc::Sender<Vec<Note>>),
    Probe(mpsc::Sender<Option<Probe>>),
}

#[derive(Default)]
struct State {
    clips: HashMap<u64, ClipState>,
    renders: HashMap<String, Arc<Rendered>>,
    wanted: Vec<u64>,
    synced: bool,
    crashed: Option<String>,
    failed: Option<String>,
    loaded: bool,
    dirty: bool,
    editing: bool,
    editor_handle: u64,
    opening_editor: bool,
    editor_error: Option<String>,
    analysis_archive: Option<CachedArchive>,
    requests: Vec<Request>,
    info: Option<FactoryInfo>,
    generation: u64,
    source_names: HashMap<String, String>,
}

/// The app side of an ARA service. All pipe I/O runs on its own thread.
pub struct RemoteService {
    tx: mpsc::Sender<Job>,
    state: Arc<Mutex<State>>,
    child: Arc<Mutex<Option<Child>>>,
    alive: Arc<AtomicBool>,
}

impl RemoteService {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        host: &Path,
        format: &str,
        path: &Path,
        id: &str,
        name: &str,
        archive: Option<(Vec<u8>, String)>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> RemoteService {
        Self::start_with_fixture(host, format, path, id, name, archive, notify, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_with_fixture(
        host: &Path,
        format: &str,
        path: &Path,
        id: &str,
        name: &str,
        archive: Option<(Vec<u8>, String)>,
        notify: Arc<dyn Fn() + Send + Sync>,
        fixture: Option<crate::fake::Config>,
    ) -> RemoteService {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(State::default()));
        let child = Arc::new(Mutex::new(None));
        let alive = Arc::new(AtomicBool::new(true));
        let service = RemoteService {
            tx,
            state: state.clone(),
            child: child.clone(),
            alive: alive.clone(),
        };
        let mut command = Command::new(host);
        command
            .arg("--ara-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if cfg!(debug_assertions) {
                Stdio::inherit()
            } else {
                Stdio::null()
            });
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut process = match command.spawn() {
            Ok(process) => process,
            Err(e) => {
                lock(&state).failed = Some(format!("ARA host could not start: {e}"));
                return service;
            }
        };
        let mut input = process.stdin.take().expect("piped ARA stdin");
        let mut output = process.stdout.take().expect("piped ARA stdout");
        *lock(&child) = Some(process);
        let pending = Arc::new(Mutex::new(None::<Instant>));
        let watcher_pending = pending.clone();
        let watcher_child = child.clone();
        let watcher_state = state.clone();
        std::thread::spawn(move || {
            while alive.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(100));
                let active = {
                    let state = lock(&watcher_state);
                    state.loaded && !state.opening_editor
                };
                if active
                    && lock(&watcher_pending)
                        .is_some_and(|since| since.elapsed() > Duration::from_secs(30))
                {
                    if let Some(process) = lock(&watcher_child).as_mut() {
                        let _ = process.kill();
                    }
                    break;
                }
            }
        });
        let format = format.to_string();
        let path = path.to_string_lossy().into_owned();
        let id = id.to_string();
        let name = name.to_string();
        std::thread::spawn(move || {
            let open = WireCommand::Open {
                protocol: remote::PROTOCOL,
                format,
                path,
                id,
                name,
                archive,
                fixture,
            };
            let result = exchange(&mut input, &mut output, &pending, open);
            if !matches!(result, Ok(Reply::Ack)) {
                let why = match result {
                    Ok(Reply::Error(e)) => e,
                    Err(e) => e.to_string(),
                    _ => "ARA host did not accept Open".into(),
                };
                crash(&state, why);
                notify();
                return;
            }
            let mut source_maps: HashMap<String, (Arc<SourceAudio>, String, SharedAudio)> =
                HashMap::new();
            let mut number = 0_u64;
            loop {
                let job = match rx.recv_timeout(Duration::from_millis(30)) {
                    Ok(job) => Some(job),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let editing = matches!(&job, Some(Job::Wire(WireCommand::Edit { .. })));
                let syncing = matches!(&job, Some(Job::Sync(_)));
                let result = match job {
                    Some(Job::Sync(song)) => {
                        let wire = wire_song(song, &mut source_maps, &mut number);
                        if wire.is_ok() {
                            lock(&state).source_names = source_maps
                                .iter()
                                .map(|(source, (_, name, _))| (source.clone(), name.clone()))
                                .collect();
                        }
                        wire.and_then(|song| {
                            exchange(&mut input, &mut output, &pending, WireCommand::Sync(song))
                        })
                    }
                    Some(Job::Wire(cmd)) => exchange(&mut input, &mut output, &pending, cmd),
                    Some(Job::Store(changed, reply)) => {
                        let answer = exchange(
                            &mut input,
                            &mut output,
                            &pending,
                            WireCommand::Store { changed },
                        );
                        match answer {
                            Ok(Reply::Archive { edited, analysis }) => {
                                if let Some(analysis) = analysis {
                                    lock(&state).analysis_archive = Some(Arc::new(analysis));
                                }
                                let _ = reply.send(edited);
                                continue;
                            }
                            other => {
                                let _ = reply.send(None);
                                other
                            }
                        }
                    }
                    Some(Job::Notes(key, modification, reply)) => {
                        let answer = exchange(
                            &mut input,
                            &mut output,
                            &pending,
                            WireCommand::Notes { key, modification },
                        );
                        let value = match &answer {
                            Ok(Reply::Notes(a)) => a.clone(),
                            _ => Vec::new(),
                        };
                        let _ = reply.send(value);
                        answer
                    }
                    Some(Job::Probe(reply)) => {
                        let answer =
                            exchange(&mut input, &mut output, &pending, WireCommand::Probe);
                        let value = match &answer {
                            Ok(Reply::Probe(a)) => a.clone(),
                            _ => None,
                        };
                        let _ = reply.send(value);
                        answer
                    }
                    None => exchange(&mut input, &mut output, &pending, WireCommand::Poll),
                };
                match result {
                    Ok(Reply::Snapshot(snapshot)) => match publish(&state, snapshot) {
                        Ok(true) => notify(),
                        Ok(false) => {}
                        Err(e) => {
                            crash(&state, e.to_string());
                            notify();
                            break;
                        }
                    },
                    Ok(Reply::Ack) => {
                        if syncing {
                            lock(&state).synced = true;
                        }
                    }
                    Ok(Reply::Error(e)) if editing => {
                        let mut state = lock(&state);
                        state.opening_editor = false;
                        state.editor_error = Some(e);
                        notify();
                    }
                    Ok(Reply::Error(e)) => {
                        crash(&state, e);
                        notify();
                        break;
                    }
                    Err(e) => {
                        crash(&state, e.to_string());
                        notify();
                        break;
                    }
                    _ => {}
                }
            }
        });
        service
    }

    fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    pub fn sync(&self, song: &Song) {
        let mut state = lock(&self.state);
        state.wanted = song.clips.iter().map(|c| c.key).collect();
        state.synced = false;
        for key in &state.wanted.clone() {
            state.clips.entry(*key).or_default();
        }
        drop(state);
        self.send(Job::Sync(song.clone()));
    }

    pub fn state(&self, key: u64) -> Option<ClipState> {
        lock(&self.state).clips.get(&key).cloned()
    }
    pub fn states(&self) -> HashMap<u64, ClipState> {
        lock(&self.state).clips.clone()
    }
    pub fn crashed(&self) -> Option<String> {
        lock(&self.state).crashed.clone()
    }
    pub fn failed(&self) -> Option<String> {
        lock(&self.state).failed.clone()
    }
    pub fn loaded(&self) -> bool {
        lock(&self.state).loaded
    }
    pub fn info(&self) -> Option<FactoryInfo> {
        lock(&self.state).info.clone()
    }
    pub fn generation(&self) -> u64 {
        lock(&self.state).generation
    }
    pub fn editing(&self) -> bool {
        lock(&self.state).editing
    }

    pub fn editor_handle(&self) -> u64 {
        let state = lock(&self.state);
        if state.crashed.is_some() {
            0
        } else {
            state.editor_handle
        }
    }
    pub fn take_editor_error(&self) -> Option<String> {
        lock(&self.state).editor_error.take()
    }
    pub fn take_dirty(&self) -> bool {
        std::mem::take(&mut lock(&self.state).dirty)
    }
    pub fn take_requests(&self) -> Vec<Request> {
        std::mem::take(&mut lock(&self.state).requests)
    }
    pub fn analysis_archive(&self) -> Option<CachedArchive> {
        lock(&self.state).analysis_archive.clone()
    }
    pub fn try_analysis_archive(&self) -> Option<CachedArchive> {
        self.state
            .try_lock()
            .ok()
            .and_then(|s| s.analysis_archive.clone())
    }
    pub fn mark_changed(&self) {
        self.send(Job::Wire(WireCommand::MarkChanged));
    }

    #[doc(hidden)]
    pub fn fixture_edit(&self, key: u64, gain: f32) {
        self.send(Job::Wire(WireCommand::FixtureEdit { key, gain }));
    }

    #[doc(hidden)]
    pub fn source_mapping_name_for_test(&self, source: &str) -> Option<String> {
        lock(&self.state).source_names.get(source).cloned()
    }

    #[doc(hidden)]
    pub fn terminate_host_for_test(&self) {
        if let Some(child) = lock(&self.child).as_mut() {
            let _ = child.kill();
        }
    }
    pub fn close_editor(&self) {
        lock(&self.state).opening_editor = false;
        self.send(Job::Wire(WireCommand::CloseEditor));
    }
    pub fn set_editor_caption(&self, c: Caption) {
        self.send(Job::Wire(WireCommand::Caption(c.into())));
    }
    pub fn request_editor(
        &self,
        title: &str,
        place: Placement,
        clips: &[u64],
    ) -> Result<(), String> {
        if let Some(e) = self.crashed().or_else(|| self.failed()) {
            return Err(e);
        }
        lock(&self.state).opening_editor = true;
        self.send(Job::Wire(WireCommand::Edit {
            title: title.into(),
            place: place.into(),
            clips: clips.to_vec(),
        }));
        Ok(())
    }
    pub fn open_editor(
        &self,
        title: &str,
        place: Placement,
        clips: &[u64],
        limit: Duration,
    ) -> Result<(), String> {
        self.request_editor(title, place, clips)?;
        let until = Instant::now() + limit;
        loop {
            if self.editing() {
                return Ok(());
            }
            if let Some(e) = self
                .take_editor_error()
                .or_else(|| self.crashed())
                .or_else(|| self.failed())
            {
                return Err(e);
            }
            if Instant::now() >= until {
                return Err("the plugin did not open its editor in time".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn store_later(&self) -> Option<Stored> {
        if self.crashed().is_some() || self.failed().is_some() {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.send(Job::Store(false, tx));
        Some(rx)
    }
    pub fn store_changed_later(&self) -> Option<Stored> {
        if self.crashed().is_some() || self.failed().is_some() {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.send(Job::Store(true, tx));
        Some(rx)
    }
    pub fn store(&self, limit: Duration) -> Option<(Vec<u8>, String)> {
        self.store_later()?.recv_timeout(limit).ok().flatten()
    }
    pub fn notes(&self, key: u64, modification: bool, limit: Duration) -> Vec<Note> {
        let (tx, rx) = mpsc::channel();
        self.send(Job::Notes(key, modification, tx));
        rx.recv_timeout(limit).unwrap_or_default()
    }
    pub fn probe(&self, limit: Duration) -> Option<Probe> {
        let (tx, rx) = mpsc::channel();
        self.send(Job::Probe(tx));
        rx.recv_timeout(limit).ok().flatten()
    }
    pub fn wait_idle(&self, deadline: Instant) -> bool {
        loop {
            let state = lock(&self.state);
            let idle = state.synced
                && state.wanted.iter().all(|key| {
                    state.clips.get(key).is_some_and(|s| {
                        matches!(s.phase, Phase::Ready | Phase::Failed | Phase::Crashed)
                    })
                });
            let ended = state.crashed.is_some() || state.failed.is_some();
            drop(state);
            if idle || ended || Instant::now() >= deadline {
                return idle;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for RemoteService {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        if let Some(child) = lock(&self.child).as_mut() {
            let _ = child.kill();
        }
    }
}

fn exchange(
    input: &mut ChildStdin,
    output: &mut ChildStdout,
    pending: &Mutex<Option<Instant>>,
    command: WireCommand,
) -> io::Result<Reply> {
    *lock(pending) = Some(Instant::now());
    let answer = remote::send(input, &command).and_then(|()| remote::recv(output));
    *lock(pending) = None;
    answer
}

fn crash(state: &Mutex<State>, why: String) {
    let mut state = lock(state);
    state.crashed = Some(format!("ARA host ended: {why}"));
    state.loaded = false;
    state.opening_editor = false;
    for clip in state.clips.values_mut() {
        clip.phase = Phase::Crashed;
        clip.rendered = None;
    }
}

fn wire_song(
    song: Song,
    maps: &mut HashMap<String, (Arc<SourceAudio>, String, SharedAudio)>,
    number: &mut u64,
) -> io::Result<WireSong> {
    let mut active = HashMap::new();
    let mut clips = Vec::with_capacity(song.clips.len());
    for c in song.clips {
        let mapping = if let Some((audio, name, mapping)) = maps.remove(&c.source) {
            if Arc::ptr_eq(&audio, &c.audio) {
                let result = name.clone();
                active.insert(c.source.clone(), (audio, name, mapping));
                result
            } else {
                make_source(&c, &mut active, number)?
            }
        } else if let Some((audio, name, _)) = active.get(&c.source) {
            if Arc::ptr_eq(audio, &c.audio) {
                name.clone()
            } else {
                make_source(&c, &mut active, number)?
            }
        } else {
            make_source(&c, &mut active, number)?
        };
        clips.push(WireClip {
            regions: c.regions,
            key: c.key,
            clone_of: c.clone_of,
            source: c.source,
            name: c.name,
            mapping,
            start: c.start,
            offset: c.offset,
            duration: c.duration,
        });
    }
    *maps = active;
    Ok(WireSong {
        tempos: song.tempos,
        bars: song.bars,
        bpm: song.bpm,
        numerator: song.numerator,
        denominator: song.denominator,
        clips,
    })
}

fn make_source(
    c: &crate::service::ClipSpec,
    active: &mut HashMap<String, (Arc<SourceAudio>, String, SharedAudio)>,
    number: &mut u64,
) -> io::Result<String> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    *number += 1;
    let name = format!(
        "xirius-ara-source-{}-{}-{number}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let left = c.audio.channel(0).unwrap_or_default();
    let right = c.audio.channel(1).unwrap_or_default();
    let map = SharedAudio::create(&name, c.audio.rate, left, right)?;
    active.insert(c.source.clone(), (c.audio.clone(), name.clone(), map));
    Ok(name)
}

fn publish(state: &Mutex<State>, snapshot: Snapshot) -> io::Result<bool> {
    let mut state = lock(state);
    // Heartbeats with the same generation should not rebuild the live timelines.
    let changed = state.generation != snapshot.generation;
    let mut clips = HashMap::new();
    let mut active = HashMap::new();
    for (key, wire) in snapshot.states {
        let rendered = if let Some(render) = wire.rendered {
            let sound = if let Some(old) = state.renders.get(&render.name) {
                old.clone()
            } else {
                let map = SharedAudio::open(&render.name)?;
                Arc::new(Rendered {
                    rate: render.rate,
                    duration: render.duration,
                    offset: render.offset,
                    left: map.channel(0).to_vec(),
                    right: map.channel(1).to_vec(),
                })
            };
            active.insert(render.name, sound.clone());
            Some(sound)
        } else {
            None
        };
        clips.insert(
            key,
            ClipState {
                phase: wire.phase,
                progress: wire.progress,
                rendered,
            },
        );
    }
    state.clips = clips;
    state.renders = active;
    state.crashed = snapshot.crashed;
    state.failed = snapshot.failed;
    state.loaded = snapshot.loaded;
    state.dirty |= snapshot.dirty;
    state.editing = snapshot.editing;
    state.editor_handle = snapshot.editor_handle;
    state.editor_error = snapshot.editor_error.or(state.editor_error.take());
    if state.editing || state.editor_error.is_some() {
        state.opening_editor = false;
    }
    if let Some(archive) = snapshot.analysis_archive
        && !state
            .analysis_archive
            .as_ref()
            .is_some_and(|old| **old == archive)
    {
        state.analysis_archive = Some(Arc::new(archive));
    }
    state.requests.extend(snapshot.requests);
    state.info = snapshot.info;
    state.generation = snapshot.generation;
    Ok(changed)
}
