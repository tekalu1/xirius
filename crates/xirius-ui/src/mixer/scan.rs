//! The plugin scan behind the insert list and the browser (PLG-021).
//!
//! The screen runs the same command as the CLI and MCP, `plugin.rescan --background`. Each
//! bundle is opened in a child process (this program with `__scan-plugin`), so a plugin
//! that crashes is one failure in the list and the scan goes on. The screen does not wait:
//! [`Studio::tick`] asks `export.status` a few times a second, and when the job is done
//! the insert list and the browser read the list again.
//!
//! Both places that show a running scan can stop it: `export.cancel` on the job ends the
//! processes still opening a bundle, and the list keeps the bundles opened by then.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_plugins::{CHILD_ARG, Catalog, ChildProgram, ScanOptions};

use crate::popup::{Action, Choice, PopupKind};
use crate::studio::Studio;

/// How often a running scan is asked how far it got.
const POLL: Duration = Duration::from_millis(100);

/// The insert list's choice that scans again.
pub const RESCAN: &str = "plugin-rescan";
/// The insert list's choice that stops the running scan.
pub const STOP: &str = "plugin-scan-stop";

#[cfg(test)]
mod menu_io_tests {
    use super::*;
    use std::sync::{Arc, mpsc};

    #[test]
    fn catalog_and_preset_menu_reads_are_worker_owned_and_keep_events_responding() {
        for operation in ["plugin_menu_catalog", "preset_menu_list"] {
            std::thread::spawn(move || {
                let root = tempfile::tempdir().unwrap();
                let mut s = Studio::new(crate::song::Song::sample_with(ScanOptions {
                    cache: root.path().join("catalog.json"),
                    ..ScanOptions::default()
                }));
                let ui = std::thread::current().id();
                let release = crate::browser_io_tests::Release::new();
                let gate = release.0.clone();
                let cache = s.song.plugin_cache().to_owned();
                // Studio's test Session gives presets a private, per-song directory.
                let preset_dir = s.song.run("track.preset.list", json!({})).unwrap()["dir"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let preset_dir = std::path::PathBuf::from(preset_dir);
                let (entered_tx, entered) = mpsc::channel();
                let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
                    if op == operation && (path == cache || path == preset_dir) {
                        assert_ne!(ui, std::thread::current().id());
                        let _ = entered_tx.send(());
                        let held = gate.0.lock().unwrap();
                        drop(gate.1.wait_while(held, |done| !*done).unwrap());
                    }
                    Ok(())
                }));
                s.bind_window_io();
                xirius_session::host::mark_window_thread();
                if operation == "plugin_menu_catalog" {
                    s.insert_pick();
                } else {
                    s.song.run("track.preset.list", json!({})).unwrap();
                }
                entered.recv_timeout(Duration::from_secs(20)).unwrap();
                for _ in 0..24 {
                    s.insert_list();
                    s.song.run("track.preset.list", json!({})).unwrap();
                    s.idle();
                    s.tick();
                    s.geometry(900.0, 600.0);
                }
                release.release();
                let deadline = Instant::now() + Duration::from_secs(20);
                while s.mixer.scan.catalog_read.is_some() {
                    s.poll_plugin_scan();
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
            })
            .join()
            .unwrap();
        }
    }

    #[test]
    fn a_command_worker_background_scan_refreshes_the_menu_after_its_job_finishes() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(crate::song::Song::sample_with(ScanOptions {
            dirs: Vec::new(),
            cache: root.path().join("catalog.json"),
            ..ScanOptions::default()
        }));
        s.bind_window_io();
        s.io_command_on_worker(
            "plugin.rescan",
            json!({"background":true}).as_object().unwrap().clone(),
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        while s.saving.is_some() {
            s.poll_saving();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(
            s.mixer.scan.job.is_some(),
            "completion keeps the background continuation"
        );
        while s.mixer.scan.job.is_some() || s.mixer.scan.catalog_read.is_some() {
            s.poll_plugin_scan();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(s.mixer.scan.cache_current, Some(true));
        assert!(s.mixer.catalog.is_some());
        assert!(s.error.is_none(), "{:?}", s.error);
    }

    #[test]
    fn an_explicit_catalog_reload_discards_a_late_pre_scan_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(crate::song::Song::sample_with(ScanOptions {
            cache: root.path().join("catalog.json"),
            ..ScanOptions::default()
        }));
        s.bind_window_io();
        let release = crate::browser_io_tests::Release::new();
        let gate = release.0.clone();
        let (tx, entered) = mpsc::channel();
        s.mixer.scan.catalog_read = Some(
            xirius_session::background::Job::spawn("old-catalog", move || {
                tx.send(()).unwrap();
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |done| !*done).unwrap());
                Ok((Catalog::default(), true))
            })
            .unwrap(),
        );
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        s.reload_catalog_later();
        let deadline = Instant::now() + Duration::from_secs(20);
        while s.mixer.scan.catalog_read.is_some() {
            s.poll_catalog();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            s.mixer.scan.cache_current,
            Some(false),
            "new cache is missing"
        );
        release.release();
        for _ in 0..32 {
            s.poll_catalog();
        }
        assert_eq!(s.mixer.scan.cache_current, Some(false));
    }
}

/// Where this program looks for plugins: the standard folders and cache, each bundle
/// opened by this program in a child process.
pub fn options() -> ScanOptions {
    ScanOptions {
        program: std::env::current_exe().ok().map(|program| ChildProgram {
            program,
            args: vec![CHILD_ARG.to_string()],
        }),
        ..ScanOptions::default()
    }
}

#[derive(Default)]
pub struct PluginScan {
    catalog_read: Option<xirius_session::background::Job<(Catalog, bool)>>,
    cache_current: Option<bool>,
    announce_catalog: bool,
    preset_rows: serde_json::Value,
    /// Scan by itself when there is no list yet (the real window; tests leave it off).
    pub auto: bool,
    checked: bool,
    job: Option<String>,
    pub done: u64,
    pub total: u64,
    /// The bundle it opens now (PLG-059); empty until it says.
    pub file: String,
    /// Why the last scan failed.
    pub error: Option<String>,
    polled: Option<Instant>,
    /// Asked to stop; ending.
    pub stopping: bool,
}

impl PluginScan {
    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// `スキャン中 12 / 295` once the number of bundles is known.
    fn progress_text(&self) -> String {
        if self.stopping {
            "中止しています…".to_string()
        } else if self.total == 0 {
            "検索中".to_string()
        } else {
            format!("スキャン中 {} / {}", self.done.min(self.total), self.total)
        }
    }
}

impl Studio {
    /// Starts `plugin.rescan --background`; a scan already running is left to finish.
    pub fn start_plugin_scan(&mut self) -> Result<(), String> {
        self.start_scan(json!({ "background": true }))
    }

    /// Scans the unusable plugin `at` (an index into the browser's list of them) again; the
    /// rest of the list stays as it is.
    pub fn retry_plugin(&mut self, at: usize) -> Result<(), String> {
        let Some(bad) = self.browser.bad.get(at) else {
            return Ok(());
        };
        if self.mixer.scan.running() {
            return Ok(());
        }
        let path = bad.path.clone();
        self.start_scan(json!({ "background": true, "path": path.to_string_lossy() }))?;
        self.browser.retrying = Some(path);
        Ok(())
    }

    fn start_scan(&mut self, args: serde_json::Value) -> Result<(), String> {
        if self.mixer.scan.running() {
            return Ok(());
        }
        let data = self.song.run("plugin.rescan", args)?;
        self.track_scan(&data)
    }

    pub(crate) fn track_scan(&mut self, data: &serde_json::Value) -> Result<(), String> {
        let job = data["job"].as_str().ok_or("plugin.rescan gave no job")?;
        let scan = &mut self.mixer.scan;
        if scan.job.as_deref() == Some(job) {
            return Ok(());
        }
        scan.job = Some(job.to_string());
        (scan.done, scan.total, scan.error, scan.polled) = (0, 0, None, None);
        scan.stopping = false;
        self.browser.scanning = true;
        self.browser.scan_stopping = false;
        (self.browser.scan_done, self.browser.scan_total) = (0, 0);
        self.browser.retrying = None;
        Ok(())
    }

    /// For the screenshots: a scan shown half way, which no frame polls and nothing ends.
    pub(crate) fn pose_scan(&mut self, done: u64, total: u64) {
        let scan = &mut self.mixer.scan;
        scan.job = Some("pose".to_string());
        (scan.done, scan.total, scan.stopping) = (done, total, false);
        scan.polled = Some(Instant::now() + std::time::Duration::from_secs(3600));
        self.browser.scanning = true;
        self.browser.scan_stopping = false;
        (self.browser.scan_done, self.browser.scan_total) = (done, total);
    }

    /// Asks the running scan to stop (`export.cancel`). It ends on a later frame, keeping
    /// what it opened.
    pub fn stop_plugin_scan(&mut self) {
        let Some(job) = self.mixer.scan.job.clone() else {
            return;
        };
        if self.mixer.scan.stopping {
            return;
        }
        match self.song.run("export.cancel", json!({ "job": job })) {
            Ok(_) => {
                self.mixer.scan.stopping = true;
                self.browser.scan_stopping = true;
                // Asked at once, so the end shows on the next frame.
                self.mixer.scan.polled = None;
                self.refresh_insert_list();
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn refresh_insert_list(&mut self) {
        if self
            .popup
            .as_ref()
            .is_some_and(|p| matches!(p.kind, PopupKind::Insert(_) | PopupKind::Instrument(_)))
        {
            self.refresh_popup();
        }
    }

    /// The real window: the list the last scan kept, and a scan of its own when there is none.
    pub fn use_installed_plugins(&mut self) {
        self.mixer.scan.auto = true;
        if self.transport.is_windowed() {
            self.read_catalog_later();
            return;
        }
        let catalog = Catalog::load(self.song.plugin_cache());
        self.use_plugins(catalog);
    }

    /// The insert list and the browser show these plugins.
    pub fn use_plugins(&mut self, catalog: Catalog) {
        self.mixer.scan.catalog_read = None;
        self.mixer.scan.cache_current = Some(true);
        self.browser.set_plugins(&catalog);
        let catalog = Arc::new(catalog);
        self.transport.set_catalog(catalog.clone());
        self.mixer.catalog = Some(catalog);
    }

    /// The plugins the insert list offers; read from the cache when first needed. The player
    /// looks its plugins up in the same list.
    pub(crate) fn plugin_catalog(&mut self) -> Arc<Catalog> {
        if self.transport.is_windowed() {
            self.poll_catalog();
            if self.mixer.catalog.is_none() && self.mixer.scan.error.is_none() {
                self.read_catalog_later();
            }
            // A menu may show built-ins while the list is loading. Do not
            // install this placeholder in Transport: its preparation worker
            // must load the real catalogue when no snapshot is available.
            return self
                .mixer
                .catalog
                .clone()
                .unwrap_or_else(|| Arc::new(Catalog::default()));
        }
        let cache = self.song.plugin_cache();
        let catalog = self
            .mixer
            .catalog
            .get_or_insert_with(|| Arc::new(Catalog::load(cache)))
            .clone();
        self.transport.set_catalog(catalog.clone());
        catalog
    }

    /// Once a frame. True while a scan runs, so frames keep coming to show it.
    pub(crate) fn poll_plugin_scan(&mut self) -> bool {
        let mut loaded = self.poll_catalog();
        if self.transport.is_windowed()
            && !self.input_blocked()
            && let Ok(rows) = self.song.run("track.preset.list", json!({}))
            && rows != self.mixer.scan.preset_rows
        {
            self.mixer.scan.preset_rows = rows;
            self.refresh_popup();
            loaded = true;
        }
        let scan = &mut self.mixer.scan;
        if scan.auto && !scan.checked {
            if self.transport.is_windowed() && scan.cache_current.is_none() {
                if scan.error.is_none() {
                    self.read_catalog_later();
                }
                return self.mixer.scan.catalog_read.is_some();
            }
            scan.checked = true;
            let current = if self.transport.is_windowed() {
                scan.cache_current.unwrap_or(false)
            } else {
                xirius_plugins::CacheFile::is_current(self.song.plugin_cache())
            };
            if !current {
                // The frame that starts it only starts it: the next ones read how it goes.
                match self.start_plugin_scan() {
                    Ok(()) => return true,
                    Err(e) => self.mixer.scan.error = Some(e),
                }
            }
        }
        let scan = &mut self.mixer.scan;
        let Some(job) = scan.job.clone() else {
            return loaded || scan.catalog_read.is_some();
        };
        if scan.polled.is_some_and(|t| t.elapsed() < POLL) {
            return true;
        }
        scan.polled = Some(Instant::now());
        let status = self.song.run("export.status", json!({ "job": job }));
        let scan = &mut self.mixer.scan;
        let state = match &status {
            Ok(v) => {
                scan.done = v["done"].as_u64().unwrap_or(0);
                scan.total = v["total"].as_u64().unwrap_or(0);
                scan.file = v["file"].as_str().unwrap_or_default().to_string();
                self.browser.scan_done = scan.done;
                self.browser.scan_total = scan.total;
                v["state"].as_str().unwrap_or("failed").to_string()
            }
            Err(_) => "failed".to_string(),
        };
        if state != "running" {
            let again = self.browser.retrying.is_some();
            let scan = &mut self.mixer.scan;
            scan.job = None;
            scan.stopping = false;
            scan.file.clear();
            self.browser.scanning = false;
            self.browser.scan_stopping = false;
            self.browser.retrying = None;
            // A stopped scan kept what it opened: the list is read again all the same.
            if state == "done" || state == "cancelled" {
                if self.transport.is_windowed() {
                    self.mixer.scan.announce_catalog = state == "done" && !again;
                    self.reload_catalog_later();
                    self.refresh_insert_list();
                    return true;
                }
                let catalog = Catalog::load(self.song.plugin_cache());
                let found = catalog.entries.len();
                self.use_plugins(catalog);
                // The settings' plugin page says how many a whole scan found.
                if state == "done"
                    && !again
                    && let Some(ui) = &mut self.settings
                {
                    ui.tell(format!("{found} 個のプラグインが見つかりました"), None);
                }
            } else {
                self.mixer.scan.error = Some(match status {
                    Ok(v) => v["error"].as_str().unwrap_or(&state).to_string(),
                    Err(e) => e,
                });
            }
        }
        self.refresh_insert_list();
        true
    }

    pub(crate) fn reload_catalog_later(&mut self) {
        self.mixer.scan.catalog_read = None;
        self.read_catalog_later();
    }
    pub(crate) fn read_catalog_later(&mut self) {
        if self.mixer.scan.catalog_read.is_some() {
            return;
        }
        let cache = self.song.plugin_cache().to_owned();
        match xirius_session::background::Job::spawn("plugin-menu-catalog", move || {
            #[cfg(any(test, feature = "loading-probe"))]
            xirius_core::io_test::before("plugin_menu_catalog", &cache)
                .map_err(|e| e.to_string())?;
            let current = xirius_plugins::CacheFile::is_current(&cache);
            Ok((Catalog::load(&cache), current))
        }) {
            Ok(job) => self.mixer.scan.catalog_read = Some(job),
            Err(error) => self.mixer.scan.error = Some(error),
        }
    }
    fn poll_catalog(&mut self) -> bool {
        let Some(result) = self
            .mixer
            .scan
            .catalog_read
            .as_ref()
            .and_then(xirius_session::background::Job::poll)
        else {
            return false;
        };
        self.mixer.scan.catalog_read = None;
        match result {
            Ok((catalog, current)) => {
                let found = catalog.entries.len();
                self.use_plugins(catalog);
                self.mixer.scan.cache_current = Some(current);
                if std::mem::take(&mut self.mixer.scan.announce_catalog)
                    && let Some(ui) = &mut self.settings
                {
                    ui.tell(format!("{found} 個のプラグインが見つかりました"), None);
                }
                self.refresh_insert_list();
            }
            Err(error) => self.mixer.scan.error = Some(error),
        }
        true
    }

    /// The insert list's menu (UI-320): 最近, エフェクト, プラグイン,
    /// with the scan's rows under it (never filtered).
    pub(crate) fn insert_pick(&mut self) -> crate::pick_list::PickList {
        use crate::pick_list::PickList;
        let catalog = self.plugin_catalog();
        let all = super::insert_choices(&catalog);
        let recent = self.recent_effects();
        let seen: std::collections::HashSet<String> = recent.iter().map(|c| c.to.clone()).collect();
        let all: Vec<_> = all.into_iter().filter(|c| !seen.contains(&c.to)).collect();
        let (builtin, plugins): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|c| c.to.starts_with("builtin:"));
        let mut list = PickList::grouped(
            vec![
                ("最近", recent),
                ("エフェクト", builtin),
                ("プラグイン", plugins),
            ],
            "effects",
            |to| Action::Pick(to.to_string()),
        )
        .named("一致するエフェクトはありません");
        list.footer = self.scan_footer();
        list
    }

    /// The list menu's footer: how the scan is doing, or the row that scans
    /// again (UI-320: the query never filters these).
    pub(crate) fn scan_footer(&self) -> Vec<crate::pick_list::PickRow> {
        use crate::pick_list::PickRow;
        let scan = &self.mixer.scan;
        if scan.running() {
            let mut out = vec![PickRow::new(scan.progress_text()).note()];
            if !scan.stopping {
                out.push(
                    PickRow::new("スキャンを中止")
                        .icon("close")
                        .act(Action::Pick(STOP.to_string())),
                );
            }
            out
        } else {
            let mut out = Vec::new();
            if let Some(e) = &scan.error {
                out.push(PickRow::new(format!("スキャンに失敗しました: {e}")).note());
            }
            out.push(
                PickRow::new("プラグインを再スキャン")
                    .icon("restore")
                    .act(Action::Pick(RESCAN.to_string())),
            );
            out
        }
    }

    /// The insert list: the effects, and how the scan is doing.
    pub fn insert_list(&mut self) -> Vec<Choice> {
        let catalog = self.plugin_catalog();
        let out = super::insert_choices(&catalog);
        let mut recent = self.recent_effects();
        let rest = out
            .into_iter()
            .filter(|c| !recent.iter().any(|r| r.to == c.to))
            .collect::<Vec<_>>();
        recent.extend(rest);
        self.with_scan_rows(recent)
    }

    /// `out` with how the scan is doing around it: while it runs, its progress and the choice
    /// that stops it on top; otherwise the choice that scans again at the end.
    pub(crate) fn with_scan_rows(&self, mut out: Vec<Choice>) -> Vec<Choice> {
        let scan = &self.mixer.scan;
        let none = out.len() == 1 && out[0].to.is_empty();
        if scan.running() {
            if none {
                out.clear();
            }
            out.insert(
                0,
                Choice {
                    to: String::new(),
                    label: scan.progress_text(),
                },
            );
            if !scan.stopping {
                out.insert(
                    1,
                    Choice {
                        to: STOP.to_string(),
                        label: "スキャンを中止".to_string(),
                    },
                );
            }
        } else {
            if let Some(e) = &scan.error {
                out.push(Choice {
                    to: String::new(),
                    label: format!("スキャンに失敗しました: {e}"),
                });
            }
            out.push(Choice {
                to: RESCAN.to_string(),
                label: "プラグインを再スキャン".to_string(),
            });
        }
        out
    }
}
