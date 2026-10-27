//! Export menu visibility and enabled checks only inspect completed metadata.
//! Unknown files require confirmation for trash; final commands validate again.
use crate::studio::Studio;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Presence {
    wanted: Vec<PathBuf>,
    known: HashMap<PathBuf, bool>,
    work: Option<xirius_session::background::Job<HashMap<PathBuf, bool>>>,
    due: Option<Instant>,
}
impl Studio {
    pub(crate) fn export_file_presence(&self, path: &Path) -> Option<bool> {
        if self.transport.is_windowed() {
            self.export.files.known.get(path).copied()
        } else {
            // Offline/script adapters have no event loop; window callbacks never
            // use this compatibility path.
            Some(path.exists())
        }
    }
    pub(crate) fn export_file_exists(&self, path: &Path) -> bool {
        self.export_file_presence(path) == Some(true)
    }
    pub(crate) fn poll_export_files(&mut self) -> bool {
        if !self.transport.is_windowed() {
            return false;
        }
        let mut wanted: Vec<_> = self
            .export
            .jobs
            .iter()
            .filter_map(|job| job["job"].as_str())
            .flat_map(|job| self.written_files(job))
            .collect();
        wanted.sort();
        wanted.dedup();
        let files = &mut self.export.files;
        if wanted != files.wanted {
            files.work = None;
            files
                .known
                .retain(|path, _| wanted.binary_search(path).is_ok());
            files.wanted = wanted;
            files.due = None;
        }
        let mut changed = false;
        if let Some(result) = files
            .work
            .as_ref()
            .and_then(xirius_session::background::Job::poll)
        {
            files.work = None;
            if let Ok(known) = result {
                changed = known != files.known;
                files.known = known;
            }
        }
        if files.work.is_none()
            && !files.wanted.is_empty()
            && files.due.is_none_or(|due| Instant::now() >= due)
        {
            let paths = files.wanted.clone();
            files.work = xirius_session::background::Job::spawn("export-metadata", move || {
                let mut known = HashMap::new();
                for path in paths {
                    #[cfg(any(test, feature = "loading-probe"))]
                    xirius_core::io_test::before("export_metadata", &path)
                        .map_err(|e| e.to_string())?;
                    known.insert(path.clone(), path.exists());
                }
                Ok(known)
            })
            .ok();
            files.due = Some(Instant::now() + Duration::from_millis(250));
        }
        changed
    }
}
