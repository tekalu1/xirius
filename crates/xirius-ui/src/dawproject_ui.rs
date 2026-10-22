//! DAWproject on the screen (EXP-006, EXP-007): the song menu writes the song to a
//! `.dawproject` with its audio inside, and opens one as a new, unsaved song; a `.dawproject`
//! dropped on the window opens too. Both run on a thread of their own and say how far they are
//! in the toast; what the file could not carry is a count on the toast, and the list behind its
//! button.
//!
//! The work is the session's own (`xirius_session::dawproject::write`,
//! `xirius_core::dawproject::import_with`), so `export.dawproject` and `project.open` of the CLI
//! and the AI write and read the same.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use xirius_core::dawproject::{self, Imported, Skips};
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::popup::{Cell, Look, Popup, PopupKind};
use crate::studio::{Geometry, Rect, Studio};
use crate::toast::{Answer, Toast};

/// The list is at least this wide.
pub const MIN_W: f32 = 230.0;
const ROW_H: f32 = 26.0;
/// The most rows shown at once; the wheel moves on.
pub const ROWS: usize = 12;
/// Between a row's name and its count.
const GAP: f32 = 24.0;

/// What the toast says on its way, and over the lists.
pub const EXPORTING: &str = "DAWproject を書き出しています";
pub const READING: &str = "読み込んでいます";
pub const LEFT_OUT: &str = "含めなかったもの";
pub const NOT_READ: &str = "読み込めなかったもの";
pub const REVEAL: &str = "フォルダーを開く";
pub const SEE: &str = "見る";

/// What a file could not carry: a row each, its name at the left and how many at the right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

impl List {
    pub fn of(title: &str, skips: &Skips) -> List {
        List {
            title: title.to_string(),
            rows: skips
                .rows()
                .iter()
                .map(|s| (s.label.clone(), s.detail()))
                .collect(),
        }
    }

    pub fn width(&self) -> f32 {
        let pad = Layout::POPUP_PAD + Space::X2.px();
        let title = crate::browser::text_width(&self.title, FontSize::Caption);
        let rows = self
            .rows
            .iter()
            .map(|(label, detail)| {
                let detail = if detail.is_empty() {
                    0.0
                } else {
                    GAP + crate::browser::text_width(detail, FontSize::Caption)
                };
                crate::browser::text_width(label, FontSize::Small) + detail
            })
            .fold(title, f32::max);
        (rows + pad * 2.0).max(MIN_W).ceil()
    }

    /// How far the wheel can move the list.
    pub fn max_offset(&self) -> usize {
        self.rows.len().saturating_sub(ROWS)
    }
}

/// The list from (0, 0) = the pop-over's top-left corner: the title, then the rows.
pub fn cells(list: &List, offset: usize) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    let inner = list.width() - pad * 2.0;
    let mut cells = vec![
        Cell::new(
            Rect::new(
                pad + Space::X2.px(),
                pad,
                inner - Space::X2.px(),
                Layout::MENU_ROW_H,
            ),
            Look::Label,
        )
        .text(list.title.clone()),
    ];
    let mut y = pad + Layout::MENU_ROW_H;
    for (label, detail) in list.rows.iter().skip(offset).take(ROWS) {
        cells.push(
            Cell::new(Rect::new(pad, y, inner, ROW_H), Look::Note)
                .text(label.clone())
                .hint(detail.clone()),
        );
        y += ROW_H;
    }
    cells
}

/// A `.dawproject` being written or read.
pub struct Job {
    work: Work,
    /// How far it is (0..=1), as the bits of an `f64`.
    done: Arc<AtomicU64>,
    wait: crate::wait::Id,
}

enum Work {
    Export {
        out: PathBuf,
        rx: Receiver<Result<Skips, String>>,
    },
    Open {
        path: PathBuf,
        media: PathBuf,
        rx: Receiver<Result<Imported, String>>,
    },
}

impl Job {
    /// 0..=1.
    pub fn fraction(&self) -> f64 {
        f64::from_bits(self.done.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }

    /// What the toast says while it goes.
    pub fn text(&self) -> String {
        let what = match self.work {
            Work::Export { .. } => EXPORTING,
            Work::Open { .. } => READING,
        };
        format!("{what} {}%", (self.fraction() * 100.0).floor() as u32)
    }
}

/// The name of a file, as the toast says it.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// `path`, ending in `.dawproject`.
pub fn with_extension(path: &Path) -> PathBuf {
    if dawproject::is_dawproject(path) {
        return path.to_path_buf();
    }
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(dawproject::EXTENSION);
    PathBuf::from(s)
}

/// Where the audio of an opened `.dawproject` goes. The song names those files where they are
/// until it is saved, so the real window keeps them next to the app's own files; a test or a
/// script leaves them in the temporary folder.
fn media_dir(real: bool, path: &Path) -> PathBuf {
    if !real {
        return xirius_core::session::media_dir_for(path);
    }
    let stem = path
        .file_stem()
        .map_or_else(|| "song".into(), |s| s.to_string_lossy().into_owned());
    xirius_plugins::default_cache_path()
        .with_file_name("imported")
        .join(format!("{stem}-{}", xirius_core::store::now_ms()))
}

impl Studio {
    /// Writes the song to `out` (`.dawproject` added when it has no such ending), its audio
    /// inside; a file already there goes to the recycle bin. False when it did not start.
    pub fn dawproject_export(&mut self, out: &Path) -> bool {
        if self.busy_dawproject() {
            return false;
        }
        self.pull_plugins();
        let out = with_extension(out);
        let project = self.project();
        let dir = self.song.dir();
        let trash = self.song.trash();
        let done = Arc::new(AtomicU64::new(0f64.to_bits()));
        let (tx, rx) = std::sync::mpsc::channel();
        let step = done.clone();
        let target = out.clone();
        std::thread::spawn(move || {
            let got = xirius_session::dawproject::write(
                &project,
                dir.as_deref(),
                &target,
                true,
                &trash,
                &mut |f| step.store(f.to_bits(), Ordering::Relaxed),
            );
            let _ = tx.send(got);
        });
        self.start_dawproject(Work::Export { out, rx }, done);
        true
    }

    /// Opens `path` as a new song in place of this one, once it is read. False when it did
    /// not start.
    pub fn dawproject_open(&mut self, path: &Path) -> bool {
        if self.busy_dawproject() {
            return false;
        }
        let media = media_dir(self.os_files, path);
        let done = Arc::new(AtomicU64::new(0f64.to_bits()));
        let (tx, rx) = std::sync::mpsc::channel();
        let step = done.clone();
        let (file, into) = (path.to_path_buf(), media.clone());
        std::thread::spawn(move || {
            let got = dawproject::import_with(&file, &into, &mut |f| {
                step.store(f.to_bits(), Ordering::Relaxed);
            });
            let _ = tx.send(got);
        });
        let path = path.to_path_buf();
        self.start_dawproject(Work::Open { path, media, rx }, done);
        true
    }

    /// One at a time: a second is refused with a toast.
    fn busy_dawproject(&mut self) -> bool {
        if self.dawproject.is_none() {
            return false;
        }
        self.warn(
            "ほかの DAWproject の書き出しか読み込みの途中です".into(),
            None,
        );
        true
    }

    fn start_dawproject(&mut self, work: Work, done: Arc<AtomicU64>) {
        let label = match work {
            Work::Export { .. } => EXPORTING,
            Work::Open { .. } => READING,
        };
        let job = Job {
            work,
            done,
            wait: crate::wait::begin(crate::wait::Kind::Line, label),
        };
        self.toast = Some(Toast {
            busy: true,
            ..Toast::plain(job.text())
        });
        self.error = None;
        self.dawproject = Some(job);
    }

    /// Once a frame and between frames: moves the toast on, and when the work is done says so
    /// (and opens the song read). True while it runs and once when it ends.
    pub fn poll_dawproject(&mut self) -> bool {
        let Some(job) = &self.dawproject else {
            return false;
        };
        let ended = match &job.work {
            Work::Export { rx, .. } => match rx.try_recv() {
                Err(TryRecvError::Empty) => None,
                got => Some(Ended::Export(got.unwrap_or_else(|_| Err(STOPPED.into())))),
            },
            Work::Open { rx, .. } => match rx.try_recv() {
                Err(TryRecvError::Empty) => None,
                got => Some(Ended::Open(Box::new(
                    got.unwrap_or_else(|_| Err(STOPPED.into())),
                ))),
            },
        };
        let Some(ended) = ended else {
            let text = job.text();
            // A toast said meanwhile stays; the progress comes back once it goes.
            let ours = self.toast.as_ref().is_none_or(|t| t.busy);
            if ours && self.toast.as_ref().is_none_or(|t| t.text != text) {
                self.toast = Some(Toast {
                    busy: true,
                    ..Toast::plain(text)
                });
            }
            return true;
        };
        let job = self.dawproject.take().expect("checked above");
        crate::wait::end(job.wait);
        if self.toast.as_ref().is_some_and(|t| t.busy) {
            self.toast = None;
        }
        match (job.work, ended) {
            (Work::Export { out, .. }, Ended::Export(got)) => self.dawproject_written(&out, got),
            (Work::Open { path, media, .. }, Ended::Open(got)) => {
                self.dawproject_read(&path, &media, *got);
            }
            _ => unreachable!("the work and its end are of a kind"),
        }
        true
    }

    fn dawproject_written(&mut self, out: &Path, got: Result<Skips, String>) {
        let skips = match got {
            Ok(skips) => skips,
            Err(e) => {
                self.warn(format!("DAWproject に書き出せません（{e}）"), None);
                return;
            }
        };
        let reveal = (REVEAL.to_string(), Answer::Reveal(out.to_path_buf()));
        let (button, also) = if skips.is_empty() {
            (reveal, None)
        } else {
            let list = List::of(LEFT_OUT, &skips);
            (
                (format!("{LEFT_OUT} {}", skips.len()), Answer::Skipped(list)),
                Some(reveal),
            )
        };
        self.toast = Some(Toast {
            done: true,
            button: Some(button),
            also,
            ..Toast::plain(format!("{} を書き出しました", name_of(out)))
        });
    }

    fn dawproject_read(&mut self, path: &Path, media: &Path, got: Result<Imported, String>) {
        let mut got = match got {
            Ok(got) => got,
            Err(e) => {
                // The folder was made for this file alone.
                let _ = std::fs::remove_dir_all(media);
                self.warn(format!("{} を開けません（{e}）", name_of(path)), None);
                return;
            }
        };
        // The plugins of the song this PC has no copy of are kept, and listed.
        let catalog = self.plugin_catalog();
        dawproject::not_here(&got.project, &mut got.skipped, &mut |x| {
            (!catalog.entries.is_empty()).then(|| catalog.find(&x.uid).is_some())
        });
        self.keep_leaving();
        let cleaned = self.close_song();
        self.song = crate::song::Song::imported_after(&self.song, got.project);
        self.error = None;
        self.song_replaced(cleaned);
        if !got.skipped.is_empty() {
            let list = List::of(NOT_READ, &got.skipped);
            self.warn(
                format!("{NOT_READ} {}", got.skipped.len()),
                Some((SEE.to_string(), Answer::Skipped(list))),
            );
        }
    }

    /// Waits for the work to end (tests and scripts).
    pub fn dawproject_wait(&mut self) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while self.dawproject.is_some() && std::time::Instant::now() < until {
            self.poll_dawproject();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Opens the list over the toast at `anchor`, its left edge with the toast's.
    pub(crate) fn open_skipped(&mut self, anchor: Rect, list: List) {
        let mut popup = Popup::new(PopupKind::Skipped, anchor);
        popup.skipped = Some(list);
        self.popup = Some(popup);
    }

    /// Shows `path` in its folder. Only the real window opens the OS's file window.
    pub(crate) fn reveal_file(&mut self, path: PathBuf) {
        if self.os_files
            && let Err(e) = xirius_session::os::reveal(&path)
        {
            self.error = Some(e);
        }
        self.revealed.push(path);
    }

    /// Files dragged over the window: a `.dawproject` among them names itself over the lanes.
    pub fn drag_names(&mut self, paths: &[PathBuf]) -> bool {
        let name = paths
            .iter()
            .find(|p| dawproject::is_dawproject(p))
            .map(|p| name_of(p));
        let changed = self.drop_dawproject != name;
        self.drop_dawproject = name;
        changed
    }

    /// A `.dawproject` among dropped files opens; the rest are left. True when one did.
    pub(crate) fn drop_dawproject_file(&mut self, paths: &[PathBuf]) -> bool {
        let Some(path) = paths.iter().find(|p| dawproject::is_dawproject(p)) else {
            return false;
        };
        self.drop_over = false;
        self.drop_dawproject = None;
        self.dawproject_open(&path.clone());
        true
    }
}

const STOPPED: &str = "処理が止まりました";

enum Ended {
    Export(Result<Skips, String>),
    Open(Box<Result<Imported, String>>),
}

/// Over the lanes while a `.dawproject` is dragged over the window: 「〜 を開く」.
pub fn paint_drop(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let Some(name) = &studio.drop_dawproject else {
        return;
    };
    // The entrance lights its own zone.
    if studio.start_shown() {
        return;
    }
    let c = pen.c;
    let r = g.lanes;
    pen.round(r, c.surface_0.opacity(0.7), Radius::M);
    pen.outline(r, c.ring, Radius::M, 2.0);
    // The words sit on a solid plate: the lanes show through the veil.
    let text = format!("{name} を開く");
    let w = pen.measure(&text, FontSize::Body, Weight::Semibold) + 2.0 * Space::X4.px();
    let h = Layout::MENU_ROW_H + Space::X2.px();
    let plate = Rect::new(r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0, w, h);
    pen.floating(plate, c.surface_2, Radius::M);
    pen.text(
        &text,
        FontSize::Body,
        Weight::Semibold,
        c.ink,
        plate,
        Align::Center,
    );
}

#[cfg(test)]
mod tests;
