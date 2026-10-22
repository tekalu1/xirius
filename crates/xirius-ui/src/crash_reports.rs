//! Crash records reached from About, including records left by an earlier run.
use crate::{
    popup::{Action, Cell, Look},
    studio::Rect,
};
use std::path::PathBuf;
use xirius_design::Layout;
use xirius_engine::crash::{self, CrashReport};

const PAGE: usize = 4;

pub fn install() -> std::io::Result<()> {
    // Off the start: earlier builds may have left thousands of empty folders.
    let _ = std::thread::Builder::new()
        .name("crash records".into())
        .spawn(|| {
            if let Err(e) = crash::tidy() {
                eprintln!("could not tidy the crash records: {e}");
            }
        });
    crash::install("xirius-ui", true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Log(usize),
    Dump(usize),
    Folder(usize),
    Previous,
    Next,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub folder: PathBuf,
    pub log: PathBuf,
    pub dump: PathBuf,
}

impl From<CrashReport> for Record {
    fn from(r: CrashReport) -> Self {
        Self {
            folder: r.folder,
            log: r.log,
            dump: r.dump,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub records: Vec<Record>,
    pub error: Option<String>,
    pub page: usize,
}

impl View {
    pub fn load() -> Self {
        match crash::reports() {
            Ok(records) => Self {
                records: records.into_iter().map(Into::into).collect(),
                error: None,
                page: 0,
            },
            Err(e) => Self {
                records: vec![],
                error: Some(format!("クラッシュ記録を開けません: {e}")),
                page: 0,
            },
        }
    }

    pub fn path(&self, act: Act) -> Option<PathBuf> {
        match act {
            Act::Log(i) => Some(self.records.get(i)?.log.clone()),
            Act::Dump(i) => Some(self.records.get(i)?.dump.clone()),
            Act::Folder(i) => Some(self.records.get(i)?.folder.clone()),
            _ => None,
        }
    }
}

pub fn cells(v: &View) -> Vec<Cell> {
    let mut cells = Vec::new();
    let w = crate::about::W;
    let pad = 20.0;
    let mut y = 80.0;
    cells.push(
        Cell::new(Rect::new(pad, y, w - pad * 2.0, 30.0), Look::Row)
            .text("クラッシュ記録")
            .icon("chevron-left")
            .act(Action::About(crate::about::Act::Crashes)),
    );
    y += 38.0;
    if let Some(error) = &v.error {
        cells.push(Cell::new(Rect::new(pad, y, w - pad * 2.0, 30.0), Look::Label).text(error));
    }
    for (i, r) in v.records.iter().enumerate().skip(v.page * PAGE).take(PAGE) {
        let name = r.folder.file_name().unwrap_or_default().to_string_lossy();
        cells.push(Cell::new(Rect::new(pad, y, w - pad * 2.0, 26.0), Look::Label).text(name));
        y += 28.0;
        for (j, (name, act)) in [
            ("ログ", Act::Log(i)),
            ("ダンプ", Act::Dump(i)),
            ("OS で表示", Act::Folder(i)),
        ]
        .into_iter()
        .enumerate()
        {
            cells.push(
                Cell::new(
                    Rect::new(pad + j as f32 * 140.0, y, 132.0, 28.0),
                    Look::Button,
                )
                .text(name)
                .act(Action::About(crate::about::Act::Crash(act))),
            );
        }
        y += 42.0;
    }
    if v.records.len() > PAGE {
        for (x, icon, act, on) in [
            (pad, "chevron-left", Act::Previous, v.page > 0),
            (
                w - pad - 28.0,
                "chevron-right",
                Act::Next,
                (v.page + 1) * PAGE < v.records.len(),
            ),
        ] {
            let cell = Cell::new(
                Rect::new(x, y, Layout::ICON_BUTTON, Layout::ICON_BUTTON),
                Look::Button,
            )
            .icon(icon);
            cells.push(if on {
                cell.act(Action::About(crate::about::Act::Crash(act)))
            } else {
                cell.off()
            });
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_keep_every_record_reachable_and_open_the_stored_paths() {
        let records = (0..9)
            .map(|i| Record {
                folder: format!("case-{i}").into(),
                log: format!("case-{i}/crash.log").into(),
                dump: format!("case-{i}/crash.dmp").into(),
            })
            .collect();
        let mut v = View {
            records,
            page: 0,
            error: None,
        };
        for page in 0..3 {
            v.page = page;
            let rows = cells(&v);
            for i in page * PAGE..((page + 1) * PAGE).min(9) {
                assert!(rows.iter().any(
                    |c| c.action == Some(Action::About(crate::about::Act::Crash(Act::Log(i))))
                ));
                assert_eq!(
                    v.path(Act::Dump(i)).unwrap(),
                    PathBuf::from(format!("case-{i}/crash.dmp"))
                );
            }
        }
    }
}
