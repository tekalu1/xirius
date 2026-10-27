//! Window readers only observe completed values. No record lock, DLL probe or
//! settings/file read is performed by export panels or song-change callbacks.
use super::*;
use std::cell::RefCell;

#[derive(Clone, Default)]
pub(super) struct View {
    pub records: Vec<Value>,
    pub defaults: Value,
}
struct Entry {
    source: Arc<Project>,
    view: View,
    due: Instant,
    work: Option<crate::background::Job<View>>,
}
#[derive(Default)]
pub(super) struct Cache(RefCell<std::collections::HashMap<Option<PathBuf>, Entry>>);
impl Cache {
    pub(super) fn read(&self, project: &Arc<Project>, dir: Option<&Path>) -> View {
        let mut entries = self.0.borrow_mut();
        let entry = entries
            .entry(dir.map(Path::to_owned))
            .or_insert_with(|| Entry {
                source: project.clone(),
                view: View::default(),
                due: Instant::now(),
                work: None,
            });
        if !Arc::ptr_eq(&entry.source, project) {
            entry.work = None;
            entry.source = project.clone();
            entry.due = Instant::now();
        }
        if let Some(result) = entry.work.as_ref().and_then(crate::background::Job::poll) {
            entry.work = None;
            if let Ok(view) = result {
                entry.view = view;
            }
        }
        if entry.work.is_none() && Instant::now() >= entry.due {
            let source = project.clone();
            let dir = dir.map(Path::to_owned);
            entry.work = crate::background::Job::spawn("export-list", move || {
                #[cfg(feature = "test-io")]
                let _ = xirius_core::io_test::before(
                    "export_list",
                    dir.as_deref().unwrap_or(Path::new("")),
                );
                let disk = dir.as_deref().map(Store::of_song);
                let records = disk.as_ref().map(Store::list).unwrap_or_default();
                let names = disk.as_ref().map(Store::settings).unwrap_or_default();
                let defaults = commands::default_settings(&source, dir.as_deref(), &names);
                Ok(View { records, defaults })
            })
            .ok();
            entry.due = Instant::now() + Duration::from_millis(250);
        }
        entry.view.clone()
    }
}
