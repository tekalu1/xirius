//! Cancellable, transactional saves for the screen (UI-288).

use std::cell::RefCell;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

/// Bytes copied (audio and versions); generated project/history metadata is not a copy.
#[derive(Default, Debug)]
pub struct SaveProgress {
    copied: AtomicU64,
    total: AtomicU64,
    cancelled: AtomicBool,
    delay: Mutex<Duration>,
}

impl SaveProgress {
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        (self.copied.load(Ordering::Relaxed) as f64 / total as f64).clamp(0.0, 1.0) as f32
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// A slow disk without opening a device, for window and unit tests.
    #[doc(hidden)]
    pub fn simulate_slow_disk(&self, delay: Duration) {
        *self.delay.lock().unwrap() = delay;
    }

    fn check(&self) -> CmdResult<()> {
        if self.cancelled() {
            Err(std::io::Error::from(std::io::ErrorKind::Interrupted).into())
        } else {
            Ok(())
        }
    }
}

thread_local! {
    static PROGRESS: RefCell<Option<Arc<SaveProgress>>> = const { RefCell::new(None) };
}

pub(crate) fn check() -> CmdResult<()> {
    PROGRESS.with(|p| p.borrow().as_ref().map_or(Ok(()), |p| p.check()))
}

pub(crate) fn copy_file_atomic(from: &Path, to: &Path) -> CmdResult<()> {
    let mut temporary = to.as_os_str().to_owned();
    temporary.push(format!(".tmp{}", std::process::id()));
    let temporary = PathBuf::from(temporary);
    let result = copy_file(from, &temporary).and_then(|()| {
        check()?;
        fs::rename(&temporary, to)?;
        Ok(())
    });
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(crate) fn copy_file(from: &Path, to: &Path) -> CmdResult<()> {
    let progress = PROGRESS.with(|p| p.borrow().clone());
    let Some(progress) = progress else {
        fs::copy(from, to)?;
        return Ok(());
    };
    let mut input = fs::File::open(from)?;
    let mut output = fs::File::create(to)?;
    let mut buf = vec![0; 256 * 1024];
    loop {
        progress.check()?;
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        progress.copied.fetch_add(n as u64, Ordering::Relaxed);
        let delay = *progress.delay.lock().unwrap();
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
    }
    output.sync_all()?;
    Ok(())
}

impl Session {
    /// A presentation copy with no writer, locks or extensions. The screen must not edit it.
    /// Dropping it cannot remove the real session's marker.
    pub fn save_preview(&self) -> Session {
        let mut preview = Session::from_project((*self.current).clone());
        preview.registry = self.registry.clone();
        preview.hist = self.hist.clone();
        preview.current = self.current.clone();
        preview.saved = self.saved.clone();
        preview.state = self.state.clone();
        preview.store = self.store.clone();
        preview.ai = self.ai;
        preview.next_version = self.next_version;
        preview.saved_step = self.saved_step;
        preview.disk_hash = self.disk_hash.clone();
        preview.auto_base = self.auto_base.clone();
        preview.last_auto_ms = self.last_auto_ms;
        preview.auto_interval_ms = self.auto_interval_ms;
        preview
    }

    /// Saves into an exclusively created folder. Nothing in this session changes until all
    /// files are there; cancellation and any failure remove only that new folder.
    pub fn save_as_cancellable(
        &mut self,
        dir: &Path,
        progress: Arc<SaveProgress>,
    ) -> CmdResult<()> {
        while self.writer.pending() {
            progress.check()?;
            std::thread::sleep(Duration::from_millis(5));
        }
        progress.check()?;
        // create_dir is the ownership boundary: never remove an existing or another job's folder.
        fs::create_dir(dir)?;
        let mut candidate = self.save_preview();
        let result = (|| {
            let mut total = 0;
            if let Some(store) = &self.store {
                for version in store.list_versions()? {
                    total += fs::metadata(store.dir.join(format!("versions/{}.json", version.id)))?
                        .len();
                }
            }
            let mut names = std::collections::BTreeSet::new();
            let mut relative_destinations = std::collections::BTreeSet::new();
            for track in self.current.tracks.values() {
                for clip in &track.clips {
                    if let crate::model::ClipBody::Audio { file, .. } = &clip.body {
                        names.insert(file.clone());
                    }
                }
                if let Some(frozen) = &track.frozen {
                    names.insert(frozen.file.clone());
                }
            }
            for name in names {
                let file = Path::new(name.trim());
                let source = if file.is_absolute() {
                    if !self.current.media.copy_imports
                        || !is_external(&self.current.media, dir, name.trim())
                    {
                        continue;
                    }
                    file.to_path_buf()
                } else if let Some(store) = &self.store {
                    let destination = file
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/");
                    let destination = if cfg!(windows) {
                        destination.to_lowercase()
                    } else {
                        destination
                    };
                    if !relative_destinations.insert(destination) {
                        continue;
                    }
                    store.dir.join(file)
                } else {
                    continue;
                };
                if let Ok(metadata) = fs::metadata(source) {
                    total += metadata.len();
                }
            }
            progress.total.store(total, Ordering::Relaxed);
            struct Reset;
            impl Drop for Reset {
                fn drop(&mut self) {
                    PROGRESS.with(|p| *p.borrow_mut() = None);
                }
            }
            PROGRESS.with(|p| *p.borrow_mut() = Some(progress.clone()));
            let _reset = Reset;
            candidate.save_as_collecting(dir)?;
            if let Some(name) = dir
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| !n.is_empty())
            {
                candidate.run_value(
                    "project.rename",
                    json!({ "name": name }),
                    &RunOptions::human(),
                )?;
                candidate.save()?;
            }
            progress.check()?;
            Ok(())
        })();
        if result.is_err() {
            drop(candidate);
            // dir was created above by this operation; no pre-existing folder is removed.
            if let Err(cleanup) = fs::remove_dir_all(dir) {
                return Err(cleanup.into());
            }
            return result;
        }
        // Only a completed fork changes the original's redo cursor or releases its lane.
        if let Some(old) = &self.store
            && self.is_dirty()
            && self.saved_step >= self.hist.first
            && self.saved_step <= self.hist.last()
        {
            let mut index = self.hist.index();
            index.cursor = self.saved_step;
            let _ = old.write_index(&index);
        }
        std::mem::swap(&mut candidate.extensions, &mut self.extensions);
        self.hand_writer_to(&mut candidate);
        std::mem::swap(self, &mut candidate);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imported(session: &mut Session, file: &Path) {
        session
            .run_value(
                "clip.import.place",
                json!({"file": file, "length": 1920}),
                &RunOptions::human(),
            )
            .unwrap();
    }

    #[test]
    fn cancelling_a_copy_preserves_the_source_session_and_removes_only_its_new_folder() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let target = root.path().join("target");
        let audio = root.path().join("audio.wav");
        fs::write(&audio, vec![7; 4 * 1024 * 1024]).unwrap();
        let mut session = Session::new("original");
        session.save_as(&original).unwrap();
        imported(&mut session, &audio);
        let project = session.project_arc();
        let index = fs::read(original.join("history/index.json")).unwrap();
        let progress = Arc::new(SaveProgress::default());
        progress.simulate_slow_disk(Duration::from_millis(20));
        let worker_progress = progress.clone();
        let worker_target = target.clone();
        let worker = std::thread::spawn(move || {
            let result = session.save_as_cancellable(&worker_target, worker_progress);
            (session, result)
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while progress.fraction() == 0.0
            && !worker.is_finished()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(progress.fraction() > 0.0 && progress.fraction() < 1.0);
        progress.cancel();
        let (session, result) = worker.join().unwrap();
        assert_eq!(
            result.unwrap_err().io_kind,
            Some(std::io::ErrorKind::Interrupted)
        );
        assert_eq!(session.dir(), Some(original.as_path()));
        assert_eq!(*session.project_arc(), *project);
        assert_eq!(
            fs::read(original.join("history/index.json")).unwrap(),
            index
        );
        assert!(!target.exists());
        assert!(audio.exists());
    }

    #[test]
    fn completed_copy_counts_real_bytes_copies_audio_and_changes_name_only_on_success() {
        let root = tempfile::tempdir().unwrap();
        let audio = root.path().join("audio.wav");
        fs::write(&audio, vec![5; 600_000]).unwrap();
        let mut session = Session::new("original");
        imported(&mut session, &audio);
        let progress = Arc::new(SaveProgress::default());
        let target = root.path().join("別の曲");
        session
            .save_as_cancellable(&target, progress.clone())
            .unwrap();
        assert_eq!(progress.total.load(Ordering::Relaxed), 600_000);
        assert_eq!(progress.copied.load(Ordering::Relaxed), 600_000);
        assert_eq!(progress.fraction(), 1.0);
        assert_eq!(session.project().name, "別の曲");
        assert_eq!(
            fs::read(target.join("Media/Imports/audio.wav")).unwrap(),
            fs::read(audio).unwrap()
        );
        assert!(!session.is_dirty());
    }

    #[test]
    fn a_failed_target_never_replaces_the_session_or_removes_a_preexisting_folder() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("existing");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("precious"), b"keep").unwrap();
        let mut session = Session::new("original");
        assert!(
            session
                .save_as_cancellable(&target, Arc::default())
                .is_err()
        );
        assert_eq!(session.project().name, "original");
        assert_eq!(fs::read(target.join("precious")).unwrap(), b"keep");
        let missing_parent = root.path().join("missing/target");
        let error = session
            .save_as_cancellable(&missing_parent, Arc::default())
            .unwrap_err();
        assert_eq!(error.io_kind, Some(std::io::ErrorKind::NotFound));
        assert_eq!(session.dir(), None);
    }

    #[test]
    fn the_preview_does_not_release_the_original_lane_or_its_marker() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("song");
        let mut session = Session::new("song");
        session.save_as(&dir).unwrap();
        drop(session.save_preview());
        assert!(dir.join("session.lock").exists());
        assert_eq!(session.lane.as_ref().unwrap().k, 0);
    }

    #[test]
    fn relative_aliases_count_the_file_only_once_in_the_copy_total() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let mut session = Session::new("song");
        session.save_as(&original).unwrap();
        fs::create_dir_all(original.join("Media")).unwrap();
        fs::write(original.join("Media/hit.wav"), vec![1; 1024]).unwrap();
        for file in ["Media/hit.wav", "Media/./hit.wav"] {
            session
                .run_value(
                    "clip.import.place",
                    json!({"file":file,"length":1920}),
                    &RunOptions::human(),
                )
                .unwrap();
        }
        let progress = Arc::new(SaveProgress::default());
        session
            .save_as_cancellable(&root.path().join("copy"), progress.clone())
            .unwrap();
        assert_eq!(progress.total.load(Ordering::Relaxed), 1024);
        assert_eq!(progress.copied.load(Ordering::Relaxed), 1024);
    }
}
