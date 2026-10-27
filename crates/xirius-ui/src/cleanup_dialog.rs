//! 「使われていない録音を整理」 (BRW-004, REC-016): before the recordings nothing uses go to the
//! recycle bin, a dialog over a veil lists each with its size, then their count and total, and
//! asks. ごみ箱に移動 (where the keys start: Enter presses it) runs `project.cleanup`; キャンセル
//! and Esc close it. The 「…」 of the song-files tab opens it, and so does `view.cleanup`.

use std::path::Path;

use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::browser::text_width;
use crate::draw::{Align, Pen};
use crate::popup::{Action, Cell, Look, Popup, PopupKind};
use crate::studio::{Geometry, Rect, Studio};

pub const W: f32 = 330.0;
const PAD: f32 = 16.0;
const TITLE_H: f32 = 24.0;
const ROW_H: f32 = 26.0;
const ROW_GAP: f32 = 2.0;
/// The most files shown at once; the wheel moves on.
pub const ROWS: usize = 5;
const TOTAL_H: f32 = 26.0;
const BUTTON_H: f32 = 28.0;
const BUTTON_PAD: f32 = 14.0;

pub const TITLE: &str = "使われていない録音を整理";
pub const CANCEL: &str = "キャンセル";
pub const TRASH: &str = "ごみ箱に移動";
/// What the menu row and `view.cleanup` say when there is nothing to put in the bin.
pub const NO_UNUSED: &str = "使われていない録音はありません";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub size: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub files: Vec<File>,
    /// The size of them all.
    pub total: String,
    /// The button the keys are on.
    pub focus: Act,
    /// The keys moved it: the ring shows.
    pub keyed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Cancel,
    Trash,
}

/// A part of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanLook {
    Title,
    /// A file: its icon and name, and its size at the end (`hint`).
    File,
    /// The count and, at the end, the size of them all.
    Total,
}

impl View {
    /// How far the wheel can move the list.
    pub fn max_offset(&self) -> usize {
        self.files.len().saturating_sub(ROWS)
    }
}

/// The dialog from (0, 0) = its top-left corner.
pub fn cells(v: &View, offset: usize) -> Vec<Cell> {
    let inner = W - PAD * 2.0;
    let mut cells = vec![
        Cell::new(
            Rect::new(PAD, PAD, inner, TITLE_H),
            Look::Clean(CleanLook::Title),
        )
        .text(TITLE),
    ];
    let mut y = PAD + TITLE_H + Space::X2.px();
    for f in v.files.iter().skip(offset).take(ROWS) {
        cells.push(
            Cell::new(
                Rect::new(PAD, y, inner, ROW_H),
                Look::Clean(CleanLook::File),
            )
            .text(f.name.clone())
            .hint(f.size.clone())
            .icon("audio"),
        );
        y += ROW_H + ROW_GAP;
    }
    y -= ROW_GAP;
    cells.push(
        Cell::new(
            Rect::new(PAD, y, inner, TOTAL_H),
            Look::Clean(CleanLook::Total),
        )
        .text(format!("{} 個", v.files.len()))
        .hint(v.total.clone()),
    );
    y += TOTAL_H + Space::X3.px();
    let width = |s: &str| text_width(s, FontSize::Small) + BUTTON_PAD * 2.0;
    let trash_w = width(TRASH);
    let cancel_w = width(CANCEL);
    let trash_x = W - PAD - trash_w;
    let cancel_x = trash_x - Space::X2.px() - cancel_w;
    cells.push(
        Cell::new(Rect::new(cancel_x, y, cancel_w, BUTTON_H), Look::Button)
            .text(CANCEL)
            .act(Action::Cleanup(Act::Cancel)),
    );
    cells.push(
        Cell::new(Rect::new(trash_x, y, trash_w, BUTTON_H), Look::Primary)
            .text(TRASH)
            .act(Action::Cleanup(Act::Trash)),
    );
    cells
}

pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: CleanLook) {
    let c = pen.c;
    let r = cell.rect;
    let pad = Space::X1.px() + 2.0;
    match look {
        CleanLook::Title => {
            pen.text(
                &cell.text,
                FontSize::Lead,
                Weight::Semibold,
                c.ink_strong,
                r,
                Align::Left,
            );
        }
        CleanLook::File => {
            let size = Layout::ICON_SIZE;
            if let Some(icon) = cell.icon {
                pen.icon(
                    icon,
                    Rect::new(r.x + pad, r.y + (r.h - size) / 2.0, size, size),
                    c.ink_strong,
                );
            }
            let hint_w = text_width(&cell.hint, FontSize::Caption);
            let x = r.x + pad + size + Space::X2.px();
            let name = Rect::new(x, r.y, r.right() - pad - hint_w - Space::X2.px() - x, r.h);
            pen.mask(name, |pen| {
                pen.text(
                    &cell.text,
                    FontSize::Small,
                    Weight::Regular,
                    pen.c.ink_strong,
                    name,
                    Align::Left,
                );
            });
            pen.digits(
                &cell.hint,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(r.x, r.y, r.w - pad, r.h),
                Align::Right,
            );
        }
        CleanLook::Total => {
            let r = Rect::new(r.x + pad, r.y, r.w - pad * 2.0, r.h);
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Semibold,
                c.ink_strong,
                r,
                Align::Left,
            );
            pen.digits(
                &cell.hint,
                FontSize::Small,
                Weight::Semibold,
                c.ink_strong,
                r,
                Align::Right,
            );
        }
    }
}

/// The veil under the dialog, over the whole window.
pub fn paint_veil(pen: &mut Pen, g: &Geometry) {
    pen.fill(Rect::new(0.0, 0.0, g.size.0, g.size.1), pen.c.veil);
}

/// The ring on the button the keys are on, once the keys moved it.
pub fn paint_ring(pen: &mut Pen, popup: &Popup, g: &Geometry) {
    let Some(v) = popup.cleanup.as_ref().filter(|v| v.keyed) else {
        return;
    };
    let l = popup.layout(g.size);
    if let Some(cell) = l
        .cells
        .iter()
        .find(|c| c.action == Some(Action::Cleanup(v.focus)))
    {
        pen.outline(cell.rect, pen.c.ring, Radius::S, 2.0);
    }
}

impl Studio {
    /// The recordings `project.cleanup` would move, with their sizes; why there are none when
    /// there are none.
    pub(crate) fn cleanup_view(&mut self) -> Result<View, String> {
        let Some(dir) = self.song.dir() else {
            self.view_read.cleanup = Read::default();
            return Err(NO_UNUSED.to_string());
        };
        let project = self.project();
        let uses = self.song.session().uses_id();
        if self.transport.is_windowed() {
            let read = &mut self.view_read.cleanup;
            if read.source.as_ref().is_none_or(|(id, p, d)| {
                *id != uses || !std::sync::Arc::ptr_eq(p, &project) || *d != dir
            }) {
                *read = Read {
                    source: Some((uses, project.clone(), dir.clone())),
                    ..Read::default()
                };
            }
            if let Some(got) = read
                .job
                .as_ref()
                .and_then(crate::ancillary_io::Ticket::poll)
            {
                read.job = None;
                read.result = Some(got);
                read.at = Some(std::time::Instant::now());
            }
            if read.job.is_none()
                && read
                    .at
                    .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1))
            {
                match crate::ancillary_io::submit(move || read_view(project, dir, uses)) {
                    Ok(job) => read.job = Some(job),
                    Err(e) => read.result = Some(Err(e)),
                }
            }
            return read.result.clone().unwrap_or_else(|| Err(NO_UNUSED.into()));
        }
        read_view(project, dir, uses)
    }

    pub(crate) fn poll_cleanup_read(&mut self) -> bool {
        if self.view_read.cleanup.job.is_none() || self.input_blocked() {
            return false;
        }
        let result = self.cleanup_view();
        if self.view_read.cleanup.job.is_some() {
            return false;
        }
        let wanted = std::mem::take(&mut self.view_read.cleanup.want_open);
        if wanted
            && self.view_read.cleanup.popup == self.popup.as_ref().map(|p| p.intent_id)
            && self.palette.is_none()
            && self.settings.is_none()
        {
            self.view_read.cleanup.want_open = false;
            self.show_cleanup(result);
        } else if self.ctx.is_some() {
            self.refresh_popup();
        }
        true
    }

    /// Opens the existing dialog only after its read is complete.
    pub(crate) fn open_cleanup(&mut self) -> bool {
        if self.transport.is_windowed() {
            self.view_read.cleanup.at = None;
            let result = self.cleanup_view();
            if self.view_read.cleanup.job.is_none() {
                return self.show_cleanup(result);
            }
            self.view_read.cleanup.want_open = true;
            self.view_read.cleanup.popup = self.popup.as_ref().map(|p| p.intent_id);
            return true;
        }
        let result = self.cleanup_view();
        self.show_cleanup(result)
    }

    fn show_cleanup(&mut self, result: Result<View, String>) -> bool {
        match result {
            Ok(view) => {
                self.ctx = None;
                let mut popup = Popup::new(PopupKind::Cleanup, Rect::default());
                popup.cleanup = Some(view);
                self.popup = Some(popup);
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Read {
    source: Option<(
        u64,
        std::sync::Arc<xirius_core::model::Project>,
        std::path::PathBuf,
    )>,
    pub(crate) job: Option<crate::ancillary_io::Ticket<View>>,
    result: Option<Result<View, String>>,
    at: Option<std::time::Instant>,
    pub(crate) want_open: bool,
    popup: Option<u64>,
}

fn read_view(
    project: std::sync::Arc<xirius_core::model::Project>,
    dir: std::path::PathBuf,
    uses: u64,
) -> Result<View, String> {
    #[cfg(any(test, feature = "loading-probe"))]
    xirius_core::io_test::before("cleanup_list", &dir).map_err(|e| e.to_string())?;
    let own = xirius_session::cleanup::Own {
        uses: Some(uses),
        lane: None,
    };
    // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only browser transaction/read helper or cleanup read_view; UI only submits and polls.
    let found = xirius_session::cleanup::unused(&project, &dir, own).map_err(|e| e.to_string())?;
    if found.files.is_empty() {
        return Err(NO_UNUSED.to_string());
    }
    let files = found
        .files
        .iter()
        .map(|f| File {
            name: Path::new(f)
                .file_name()
                .map_or_else(|| f.clone(), |n| n.to_string_lossy().into_owned()),
            size: xirius_session::cleanup::megabytes(
                // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
                std::fs::metadata(dir.join(f)).map_or(0, |m| m.len()),
            ),
        })
        .collect();
    Ok(View {
        files,
        total: xirius_session::cleanup::megabytes(found.bytes),
        focus: Act::Trash,
        keyed: false,
    })
}

impl Studio {
    pub(crate) fn cleanup_action(&mut self, g: &Geometry, act: Act) -> bool {
        self.popup = None;
        match act {
            Act::Cancel => true,
            Act::Trash => {
                let ran = self
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    .arrange_run("project.cleanup", serde_json::json!({}))
                    .is_some();
                self.rebuild(g);
                ran
            }
        }
    }

    /// While it is open it takes every key: Enter presses the button the keys are on, Tab and
    /// ←→ go to the other one, Esc closes it. False when it is not open.
    pub(crate) fn cleanup_key(&mut self, g: &Geometry, key: &str) -> bool {
        let Some(v) = self
            .popup
            .as_mut()
            .filter(|p| p.kind == PopupKind::Cleanup)
            .and_then(|p| p.cleanup.as_mut())
        else {
            return false;
        };
        match key {
            "enter" => {
                let act = v.focus;
                self.cleanup_action(g, act);
            }
            "escape" => self.popup = None,
            "tab" | "left" | "right" => {
                v.focus = match v.focus {
                    Act::Cancel => Act::Trash,
                    Act::Trash => Act::Cancel,
                };
                v.keyed = true;
            }
            _ => {}
        }
        true
    }
}
