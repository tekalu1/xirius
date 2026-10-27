//! Version popovers read disk on the application FIFO, at most once per second.
use super::*;
use crate::ancillary_io::{Ticket, submit};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use xirius_core::store::Store;

struct Cache<T> {
    job: Option<Ticket<T>>,
    value: Option<T>,
    looked: Option<Instant>,
}
impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            job: None,
            value: None,
            looked: None,
        }
    }
}
impl<T: Clone + Send + 'static> Cache<T> {
    fn poll(&mut self) -> bool {
        let Some(got) = self.job.as_ref().and_then(Ticket::poll) else {
            return false;
        };
        self.job = None;
        self.looked = Some(Instant::now());
        match got {
            Ok(value) => self.value = Some(value),
            Err(e) => crate::ancillary_io::log!("version read: {e}"),
        }
        true
    }
    fn read(&mut self, work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Option<T> {
        if let Some(got) = self.job.as_ref().and_then(Ticket::poll) {
            self.job = None;
            self.looked = Some(Instant::now());
            match got {
                Ok(value) => self.value = Some(value),
                Err(e) => crate::ancillary_io::log!("version list: {e}"),
            }
        }
        if self.job.is_none()
            && self
                .looked
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
        {
            match submit(work) {
                Ok(job) => self.job = Some(job),
                Err(e) => crate::ancillary_io::log!("version list: {e}"),
            }
        }
        self.value.clone()
    }
}
#[derive(Default)]
pub(super) struct Reads {
    source: Option<(PathBuf, u32)>,
    versions: Cache<Vec<VersionItem>>,
    project: Option<Arc<Project>>,
    details: HashMap<String, Cache<VersionDetail>>,
}
impl Reads {
    pub(super) fn poll(&mut self) -> bool {
        let mut changed = self.versions.poll();
        for value in self.details.values_mut() {
            changed |= value.poll();
        }
        changed
    }
    fn source(&mut self, store: &Store) {
        let key = (store.dir.clone(), store.lane);
        if self.source.as_ref() != Some(&key) {
            *self = Self {
                source: Some(key),
                ..Self::default()
            };
        }
    }
    pub(super) fn versions(&mut self, store: Option<Store>) -> Vec<VersionItem> {
        let Some(store) = store else {
            *self = Self::default();
            return Vec::new();
        };
        self.source(&store);
        self.versions
            // design-lint: allow(ui-blocking-io) PRJ-031: Cache::read submits/polls the application FIFO; it does not read a file handle.
            .read(move || {
                #[cfg(any(test, feature = "loading-probe"))]
                xirius_core::io_test::before("version_list", &store.dir)
                    .map_err(|e| e.to_string())?;
                let mut all = store.list_versions().map_err(|e| e.to_string())?;
                all.sort_by_key(|v| std::cmp::Reverse((v.created_ms, v.id.clone())));
                Ok(all
                    .into_iter()
                    .map(|v| VersionItem {
                        named: v.kind == VersionKind::Named,
                        id: v.id,
                        name: v.name,
                        at_ms: v.created_ms,
                    })
                    .collect())
            })
            .unwrap_or_default()
    }
    pub(super) fn detail(
        &mut self,
        store: Option<Store>,
        now: Arc<Project>,
        id: &str,
    ) -> Option<VersionDetail> {
        let store = store?;
        self.source(&store);
        if self.project.as_ref().is_none_or(|p| !Arc::ptr_eq(p, &now)) {
            self.details.clear();
            self.project = Some(now.clone());
        }
        let id = id.to_owned();
        self.details.retain(|key, _| key == &id);
        // design-lint: allow(ui-blocking-io) PRJ-031: Cache::read submits/polls the application FIFO; it does not read a file handle.
        self.details.entry(id.clone()).or_default().read(move || {
            #[cfg(any(test, feature = "loading-probe"))]
            xirius_core::io_test::before("version_detail", &store.dir)
                .map_err(|e| e.to_string())?;
            let p = store.read_version(&id).map_err(|e| e.to_string())?;
            let changed = p
                .tracks
                .values()
                .filter(|t| now.tracks.get(&t.id).is_none_or(|n| **n != ***t))
                .map(|t| t.id)
                .chain(
                    now.tracks
                        .keys()
                        .filter(|id| !p.tracks.contains_key(id))
                        .copied(),
                )
                .collect();
            let length = p
                .tracks
                .values()
                .flat_map(|t| t.clips.iter().map(|c| c.start + c.length))
                .chain(p.sections.iter().map(|s| s.start + s.length))
                .max()
                .unwrap_or(0);
            Ok(VersionDetail {
                tracks: crate::draw::ordered_tracks(&p).len(),
                length_bars: p.time_map().to_bbt(length).0.saturating_sub(1),
                changed,
                colors: p.tracks.values().map(|t| (t.id, t.color.clone())).collect(),
            })
        })
    }
}
