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

#[derive(Default)]
pub(super) struct Presets {
    view: Value,
    due: Option<Instant>,
    work: Option<crate::background::Job<Value>>,
}
impl Presets {
    pub(super) fn read(&mut self, dir: PathBuf, project: Arc<Project>) -> Value {
        if let Some(result) = self.work.as_ref().and_then(crate::background::Job::poll) {
            self.work = None;
            if let Ok(view) = result {
                self.view = view;
            }
        }
        if self.work.is_none() && self.due.is_none_or(|due| Instant::now() >= due) {
            self.work = crate::background::Job::spawn("preset-menu-list", move || {
                #[cfg(feature = "test-io")]
                let _ = xirius_core::io_test::before("preset_menu_list", &dir);
                let raw = serde_json::Map::new();
                let args = xirius_core::command::Args(raw);
                crate::track_preset::run(
                    &dir,
                    &project,
                    &Catalog::default(),
                    "track.preset.list",
                    &args,
                    false,
                )
                .map(|(output, _)| output.data)
                .map_err(|e| e.to_string())
            })
            .ok();
            self.due = Some(Instant::now() + Duration::from_millis(250));
        }
        self.view.clone()
    }
}
impl Cache {
    pub(super) fn removed(&self, id: &str) {
        for entry in self.0.borrow_mut().values_mut() {
            entry.work = None;
            entry.view.records.retain(|record| record["job"] != id);
            entry.due = Instant::now();
        }
    }
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
