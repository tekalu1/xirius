//! File menu preparation performs its dry-run checks on the I/O workers.
use crate::studio::{Geometry, Studio};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use xirius_core::{Args, CmdError, ExtContext, model::Project};
use xirius_session::read_jobs::ReadJobs;
type Key = (usize, Option<PathBuf>, PathBuf);
#[derive(Clone)]
pub(crate) struct Info {
    pub dir: bool,
    pub name: String,
    pub rename: Option<CmdError>,
    pub trash: Option<CmdError>,
}
struct Pending {
    key: Key,
    id: Option<u64>,
    project: Arc<Project>,
    at: (f32, f32),
    size: (f32, f32),
    here: Option<PathBuf>,
    rename: bool,
    tab: crate::browser::RailTab,
}
#[derive(Default)]
pub(crate) struct MenuIo {
    jobs: ReadJobs<Key, Info>,
    pending: Option<Pending>,
    ready: Option<(Key, Info)>,
    applying: bool,
}
impl Studio {
    pub(crate) fn cancel_browser_menu(&mut self) {
        if !self.browser_menu_io.applying {
            self.browser_menu_io.pending = None;
            self.browser_menu_io.jobs.retain(|_| false);
        }
    }
    pub(crate) fn menu_io_ready(&mut self, path: &Path, g: &Geometry, x: f32, y: f32) -> bool {
        let project = self.project();
        let key = (
            Arc::as_ptr(&project) as usize,
            self.song.dir(),
            path.to_path_buf(),
        );
        if self.browser_menu_io.applying
            && self
                .browser_menu_io
                .ready
                .as_ref()
                .is_some_and(|(k, _)| k == &key)
        {
            return true;
        }
        self.browser_menu_io.jobs.retain(|k| k == &key);
        self.browser_menu_io.ready = None;
        let work = project.clone();
        let dir = key.1.clone();
        let file = key.2.clone();
        let registry = Arc::new(self.song.registry().clone());
        let id = self.browser_menu_io.jobs.request(key.clone(), move || {
            #[cfg(test)]
            let _ = xirius_core::io_test::before("browser_menu", &file);
            let is_dir = file.is_dir();
            let base = "新しいフォルダー";
            let name = if is_dir { (1..).map(|i| if i == 1 { base.into() } else { format!("{base} ({i})") }).find(|s| std::fs::symlink_metadata(file.join(s)).is_err()).unwrap_or_else(|| base.into()) } else { base.into() };
            let author = xirius_core::command::Author::Human;
            let cx = ExtContext { project: &work, dir: dir.as_deref(), author: &author, registry: &registry, work_copy:false, lane:None, uses:0 };
            let trash: xirius_session::cleanup::Trash = Arc::new(|_| Err("読み取り専用です".into()));
            let args = Args(serde_json::json!({"path":file, "name":file.file_name().unwrap_or_default().to_string_lossy()}).as_object().unwrap().clone());
            let rename = xirius_session::file_ops::run(&cx,"file.rename",&args,true,&trash).err().filter(|e| e.code != xirius_core::ErrorCode::NothingToDo);
            let trash = xirius_session::file_ops::run(&cx,"file.trash",&args,true,&trash).err();
            Info { dir:is_dir, name, rename, trash }
        });
        self.browser_menu_io.pending = Some(Pending {
            key,
            id,
            project,
            at: (x, y),
            size: g.size,
            here: self.browser.here_dir(),
            rename: false,
            tab: self.browser.tab,
        });
        false
    }
    pub(crate) fn browser_rename_later(
        &mut self,
        path: &Path,
        g: &Geometry,
        x: f32,
        y: f32,
    ) -> bool {
        self.menu_io_ready(path, g, x, y);
        if let Some(p) = self.browser_menu_io.pending.as_mut() {
            p.rename = true;
        }
        true
    }
    pub(crate) fn file_menu_info(&self, path: &Path) -> Option<&Info> {
        self.browser_menu_io
            .ready
            .as_ref()
            .filter(|(k, _)| {
                k.2 == path
                    && k.1 == self.song.dir()
                    && k.0 == Arc::as_ptr(&self.project()) as usize
            })
            .map(|(_, v)| v)
    }
    pub(crate) fn poll_browser_menu(&mut self) -> bool {
        let Some(p) = self.browser_menu_io.pending.as_ref() else {
            return false;
        };
        if !Arc::ptr_eq(&p.project, &self.project())
            || p.key.1 != self.song.dir()
            || p.here != self.browser.here_dir()
            || p.tab != self.browser.tab
        {
            self.cancel_browser_menu();
            return false;
        }
        if p.id.is_none() {
            let (path, size, at, rename) = (p.key.2.clone(), p.size, p.at, p.rename);
            let g = self.geometry(size.0, size.1);
            self.menu_io_ready(&path, &g, at.0, at.1);
            if let Some(p) = self.browser_menu_io.pending.as_mut() {
                p.rename = rename;
            }
            return false;
        }
        let Some(info) = self
            .browser_menu_io
            .jobs
            .take(&p.key, p.id.unwrap())
            .map(|r| {
                r.unwrap_or_else(|_| Info {
                    dir: false,
                    name: "新しいフォルダー".into(),
                    rename: Some(CmdError::io("読み込みが止まりました")),
                    trash: Some(CmdError::io("読み込みが止まりました")),
                })
            })
        else {
            return false;
        };
        let p = self.browser_menu_io.pending.take().unwrap();
        if !Arc::ptr_eq(&p.project, &self.project())
            || p.key.1 != self.song.dir()
            || p.here != self.browser.here_dir()
            || p.tab != self.browser.tab
        {
            return false;
        }
        self.browser_menu_io.ready = Some((p.key, info));
        self.browser_menu_io.applying = true;
        let g = self.geometry(p.size.0, p.size.1);
        let changed = self.lib_context(&g, p.at.0, p.at.1);
        if p.rename && self.context_open() {
            self.context_choose(&g, "名前を変更");
        }
        self.browser_menu_io.applying = false;
        changed
    }
    #[cfg(test)]
    pub(crate) fn settle_browser_menu(&mut self) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while self.browser_menu_io.pending.is_some() {
            self.poll_browser_menu();
            assert!(std::time::Instant::now() < end);
            std::thread::yield_now();
        }
    }
}
