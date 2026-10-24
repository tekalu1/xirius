//! 「Xirius について」(LIC-015): the version, the app's licence and the software it ships with.
//! LAME's row gathers what the LGPL asks: its licence text, where its source is, and the file
//! the app loads, which can be swapped for another build of the same name.
//!
//! Opened from the logo at the top left and from the command palette; a second press or Esc
//! closes it.
//!
//! When a newer version is out (UI-262, asked once a day at launch off the window's thread), the
//! logo has a dot and the version row its number, 「ダウンロード」 (the release's page in the
//! browser) and what changed.

use std::path::PathBuf;

use xirius_design::{FontSize, Layout, Radius, Space, Weight};
use xirius_session::app::UPDATE_PREF;
use xirius_session::{mp3, update};

use crate::browser::text_width;
use crate::draw::{Align, Pen};
use crate::popup::{Action, Cell, Look, PopupKind};
use crate::studio::{Rect, Studio};
use crate::topbar::BarButton;

pub const W: f32 = 480.0;
const PAD: f32 = 20.0;
const ROW_H: f32 = 48.0;
const INSET: f32 = 12.0;
const BUTTON_H: f32 = 28.0;
/// The star's box: as tall as the heading's row (UI-308). The star's art is 336 of its 512.
const MARK: f32 = 32.0;
/// `lockup.svg`: the wordmark's 144 against the star art's 168 — 9/16 of the star's box.
const WORDMARK: f32 = MARK * 9.0 / 16.0;
/// `lockup.svg`: the star's 256-unit box ends at 256, the wordmark starts at 272.
const LOGO_GAP: f32 = MARK * (272.0 - 256.0) / 256.0;

/// The software listed, in the mock's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Lame,
    Inter,
    Noto,
    /// The GitHub mark of the version row (Octicons, MIT).
    Octicons,
    Rust,
}

impl Item {
    pub const ALL: [Item; 5] = [
        Item::Lame,
        Item::Inter,
        Item::Noto,
        Item::Octicons,
        Item::Rust,
    ];

    fn name(self) -> String {
        match self {
            Item::Lame => format!("LAME {}", mp3::LAME_VERSION),
            Item::Inter => "Inter 4.0".to_string(),
            Item::Noto => xirius_design::Font::NotoSansJp.family().to_string(),
            Item::Octicons => format!("Octicons {}", crate::assets::OCTICONS_VERSION),
            Item::Rust => format!("ライブラリ {} 件", libraries().len()),
        }
    }

    fn what(self) -> &'static str {
        match self {
            Item::Lame => "MP3 の書き出し",
            Item::Inter => "画面の文字",
            Item::Noto => "画面の文字（日本語）",
            Item::Octicons => "GitHub の印",
            Item::Rust => "アプリに組み込んでいるもの",
        }
    }

    fn license(self) -> &'static str {
        match self {
            Item::Lame => mp3::LAME_LICENSE_ID,
            Item::Inter | Item::Noto => "OFL-1.1",
            Item::Octicons => "MIT",
            Item::Rust => "MIT・Apache-2.0 ほか",
        }
    }

    /// The file the licence text is written to, and the text.
    fn text(self) -> (&'static str, String) {
        let font = |file: &str| {
            crate::fonts::LICENSES
                .iter()
                .find(|(n, _)| *n == file)
                .map_or(String::new(), |(_, t)| (*t).to_string())
        };
        match self {
            Item::Lame => ("LAME-COPYING.txt", mp3::LAME_LICENSE.to_string()),
            Item::Inter => ("OFL-Inter.txt", font("OFL-Inter.txt")),
            Item::Noto => ("OFL-NotoSansJP.txt", font("OFL-NotoSansJP.txt")),
            Item::Octicons => (
                "octicons-LICENSE.txt",
                crate::assets::OCTICONS_LICENSE.to_string(),
            ),
            Item::Rust => ("THIRD-PARTY-LICENSES.txt", THIRD_PARTY.to_string()),
        }
    }
}

/// The Rust libraries built into the app, each with its version and licence, then the licence
/// texts: the list cargo-about makes (LIC-007, `scripts/licenses.py`). After them, the code of
/// others copied into the repository and built with it, the same way (LIC-021).
pub const THIRD_PARTY: &str = include_str!("../../../THIRD-PARTY-LICENSES.txt");

/// A library built into the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crate {
    pub name: &'static str,
    /// Empty for a copy that has no version of its own.
    pub version: &'static str,
    /// Its licence, as an SPDX expression (`MIT OR Apache-2.0`).
    pub license: &'static str,
}

/// The Rust libraries built into the app, as [`THIRD_PARTY`] lists them.
pub fn crates() -> Vec<Crate> {
    listed("crate ")
}

/// The code of others copied into the repository and built into the app (LIC-021), as
/// [`THIRD_PARTY`] lists it.
pub fn vendored() -> Vec<Crate> {
    listed("vendor ")
}

fn listed(prefix: &str) -> Vec<Crate> {
    THIRD_PARTY
        .lines()
        .filter_map(|l| {
            let mut field = l.strip_prefix(prefix)?.splitn(3, ' ');
            let name = field.next()?;
            let version = field.next()?;
            Some(Crate {
                name,
                version: if version == "-" { "" } else { version },
                license: field.next()?,
            })
        })
        .collect()
}

/// Every library built into the app, the Rust ones and the copied ones, by name.
pub fn libraries() -> Vec<Crate> {
    let mut all = crates();
    all.extend(vendored());
    all.sort_by_key(|c| c.name.to_lowercase());
    all
}

/// The libraries whose name holds `query`, in any case.
pub fn matching(query: &str) -> Vec<Crate> {
    let q = query.trim().to_lowercase();
    libraries()
        .into_iter()
        .filter(|c| c.name.to_lowercase().contains(&q))
        .collect()
}

/// Where the licence texts are written for the OS to open.
pub fn license_dir() -> PathBuf {
    std::env::temp_dir().join("xirius-licenses")
}

/// Writes `item`'s licence text to [`license_dir`] and returns its path.
pub fn license_file(item: Item) -> Result<PathBuf, String> {
    write_license(&license_dir(), item)
}

/// Writes `item`'s licence text to `dir` and returns its path.
fn write_license(dir: &std::path::Path, item: Item) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (name, text) = item.text();
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| format!("{} に書けません: {e}", path.display()))?;
    Ok(path)
}

/// The asking for a newer version at launch (UI-262), and getting it into place (UI-282).
pub struct Update {
    asking: Option<std::thread::JoinHandle<Option<update::Release>>>,
    /// The newer version it found.
    pub newer: Option<update::Release>,
    /// The app's folder, where the installer is put (from the last asking).
    pub root: Option<PathBuf>,
    /// The worker that downloads and checks the installer.
    job: Option<std::thread::JoinHandle<Result<update::Installer, String>>>,
    /// How the job is going; the panel reads it on every frame.
    pub progress: std::sync::Arc<std::sync::Mutex<update::Progress>>,
    /// The installer, once it is downloaded and checked.
    pub ready: Option<update::Installer>,
    /// Where this build may install an update (the per-user NSIS folder).
    pub location: Option<update::Location>,
    /// The installer to start once the window is closed.
    pub pending: Option<update::Install>,
    /// Why an update could not be put in (the other programs running, a failed check, the
    /// note the installer left). Shown in the panel (UI-282).
    pub notice: Option<String>,
    /// How the installer is started; tests put their own in.
    pub spawn: update::Spawn,
    /// How GitHub is read and files are downloaded; tests put their own in.
    pub fetch: update::Fetch,
    pub download: update::Download,
    /// The public key this build checks with; tests put a key of their own.
    pub key: String,
}

impl Default for Update {
    fn default() -> Update {
        Update {
            asking: None,
            newer: None,
            root: None,
            job: None,
            progress: std::sync::Arc::new(std::sync::Mutex::new(update::Progress::default())),
            ready: None,
            location: update::Location::per_user(),
            pending: None,
            notice: None,
            spawn: std::sync::Arc::new(update::spawn_detached),
            fetch: update::curl(),
            download: update::curl_download(),
            key: update::UPDATE_KEY.to_string(),
        }
    }
}

impl Update {
    pub fn asking(&self) -> bool {
        self.asking.is_some()
    }

    /// Is the installer being downloaded or checked?
    pub fn downloading(&self) -> bool {
        self.job.is_some()
    }

    /// What the download is doing now.
    pub fn progress(&self) -> update::Progress {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(crate) fn set_progress(&self, progress: update::Progress) {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = progress;
    }
}

/// A press in the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Crashes,
    Crash(crate::crash_reports::Act),
    Close,
    /// Opens a row, or closes it when it is open.
    Toggle(Item),
    /// Shows the encoder's file in the OS's file window.
    Reveal,
    /// LAME's source, on the web.
    Source,
    /// The same source tarball, from this app's releases (LIC-035).
    SourceRelease,
    /// The app's repository, on the web (LIC-016).
    Repository,
    /// The newer version's release page, on the web (UI-262).
    Download,
    /// The release's page, on the web, when an update cannot be put in (UI-282).
    Page,
    /// Downloads the newer version, then puts it in and starts Xirius again (UI-282).
    UpdateNow,
    /// Opens the licence text (for the Rust libraries, their list).
    License(Item),
}

/// What the panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub crashes: Option<Box<crate::crash_reports::View>>,
    pub version: String,
    /// The encoder's file: the one loaded, or where it was looked for.
    pub lame_path: PathBuf,
    /// Why it could not be loaded.
    pub lame_error: Option<String>,
    /// The row that is open.
    pub open: Option<Item>,
    /// The newer version that is out (UI-262).
    pub newer: Option<update::Release>,
    /// How the download of it goes (UI-282).
    pub progress: update::Progress,
    /// Why an update could not be put in, or the note the installer left (UI-282).
    pub notice: Option<String>,
    /// What the search over the Rust libraries holds.
    pub query: String,
}

impl View {
    pub fn now() -> View {
        let (lame_path, lame_error) = match mp3::load(None) {
            Ok(l) => (l.path.clone(), None),
            Err(_) => (mp3::locate(), Some(mp3::CANNOT_LOAD.to_string())),
        };
        View {
            crashes: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            lame_path,
            lame_error,
            open: Some(Item::Lame),
            newer: None,
            progress: update::Progress::default(),
            notice: None,
            query: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AboutLook {
    Title,
    Sub,
    /// The face behind an open row and what it holds.
    Block,
    /// A row: `text` the name, `hint` what it is for.
    Item {
        license: &'static str,
        open: bool,
    },
    /// The encoder's file.
    File,
    Alert,
    /// The newer version's number, after an arrow down.
    Release,
    /// A line of what changed in it: `head` its number, else a bullet's line (`bullet` on its
    /// first).
    Note {
        head: bool,
        bullet: bool,
    },
    /// An icon button without a face.
    Bare,
    /// A Rust library: `text` its name.
    Crate {
        version: &'static str,
        license: &'static str,
    },
    /// A line of small print, under the list.
    Caption,
}

fn about(rect: Rect, look: AboutLook) -> Cell {
    Cell::new(rect, Look::About(look))
}

fn button(cells: &mut Vec<Cell>, x: f32, y: f32, text: &str, icon: &'static str, act: Act) -> f32 {
    let w = Space::X2.px() * 2.0
        + Layout::ICON_SIZE
        + Space::X2.px()
        + text_width(text, FontSize::Small);
    cells.push(
        Cell::new(Rect::new(x, y, w, BUTTON_H), Look::Button)
            .text(text)
            .icon(icon)
            .act(Action::About(act)),
    );
    w
}

/// The width a file row leaves its path, after the icon.
fn file_room(r: Rect) -> f32 {
    r.w - Layout::ICON_SIZE - Space::X2.px()
}

/// `text` as it fits `w`: whole, or its end after a `…`.
fn tail(text: &str, w: f32) -> String {
    if text_width(text, FontSize::Small) <= w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    (1..chars.len())
        .map(|i| format!("…{}", chars[i..].iter().collect::<String>()))
        .find(|t| text_width(t, FontSize::Small) <= w)
        .unwrap_or_else(|| "…".to_string())
}

/// The panel's cells from (0, 0) = its top-left corner.
pub fn cells(v: &View, offset: usize) -> Vec<Cell> {
    let mut cells = Vec::new();
    cells.push(about(Rect::new(PAD, 14.0, 200.0, 32.0), AboutLook::Title).text("Xirius"));
    cells.push(
        about(
            Rect::new(W - PAD - 28.0 + 6.0, 16.0, 28.0, 28.0),
            AboutLook::Bare,
        )
        .icon("close")
        .act(Action::About(Act::Close)),
    );
    let version = format!("バージョン {} · MIT OR Apache-2.0", v.version);
    let Some(newer) = v.newer.as_ref().filter(|_| v.crashes.is_none()) else {
        if let Some(v) = &v.crashes {
            cells.push(
                about(Rect::new(PAD, 48.0, W - PAD * 2.0, 20.0), AboutLook::Sub).text(version),
            );
            cells.extend(crate::crash_reports::cells(v));
            return cells;
        }
        // Under the heading (14 + 32), the row as tall as the mark.
        let row_y = 46.0;
        let w = text_width(&version, FontSize::Small);
        cells.push(about(Rect::new(PAD, row_y, w, BUTTON_H), AboutLook::Sub).text(version));
        github(&mut cells, PAD + w + Space::X1.px(), row_y);
        let mut y = row_y + BUTTON_H + INSET;
        // UI-282: the installer's note from the last start, with the page to go on by hand.
        if let Some(note) = &v.notice {
            y = notice_lines(&mut cells, v, PAD, y, note, true);
        }
        rest(&mut cells, v, y, offset);
        return cells;
    };
    // The version row: this one, the newer one and the button that gets it (UI-262, UI-282).
    let row_y = 44.0;
    let mut x = PAD;
    let version = format!("バージョン {}", v.version);
    let w = text_width(&version, FontSize::Small);
    cells.push(about(Rect::new(x, row_y, w, BUTTON_H), AboutLook::Sub).text(&version));
    x += w + Space::X2.px();
    let w = release_w(&newer.version);
    cells.push(
        about(Rect::new(x, row_y, w, BUTTON_H), AboutLook::Release)
            .text(&newer.version)
            .icon("download"),
    );
    x += w + Space::X2.px();
    // UI-282: the one button of the row says what the update is doing and moves it on.
    let (label, icon, act) = match &v.progress.phase {
        update::Phase::Ready => (
            "更新して再起動".to_string(),
            "restore",
            Some(Act::UpdateNow),
        ),
        update::Phase::Downloading => (download_label(&v.progress), "download", None),
        update::Phase::Verifying => ("確かめています".to_string(), "download", None),
        _ => ("ダウンロード".to_string(), "download", Some(Act::Download)),
    };
    let w = Space::X2.px() * 2.0
        + Layout::ICON_SIZE
        + Space::X2.px()
        + text_width(&label, FontSize::Small);
    let mut primary = Cell::new(Rect::new(x, row_y, w, BUTTON_H), Look::Primary)
        .text(label)
        .icon(icon);
    if let Some(act) = act {
        primary = primary.act(Action::About(act));
    }
    if matches!(
        &v.progress.phase,
        update::Phase::Idle | update::Phase::Failed(_)
    ) {
        primary.tip = newer.url.clone();
    }
    cells.push(primary);
    github(&mut cells, x + w + Space::X1.px(), row_y);
    // What changed, under it.
    let mut y = row_y + BUTTON_H + Space::X2.px();
    let inner = W - PAD * 2.0;
    let block = cells.len();
    cells.push(about(Rect::new(PAD, y, inner, 0.0), AboutLook::Block));
    let top = y;
    y += INSET - 2.0;
    let x = PAD + INSET;
    cells.push(
        about(
            Rect::new(x, y, inner - INSET * 2.0, NOTE_H),
            AboutLook::Note {
                head: true,
                bullet: false,
            },
        )
        .text(&newer.version),
    );
    y += NOTE_H;
    let room = inner - INSET * 2.0 - BULLET_W;
    for note in &newer.notes {
        let lines = crate::opline::wrap(note, room, &|t| text_width(t, FontSize::Small));
        for (i, line) in lines.into_iter().enumerate() {
            cells.push(
                about(
                    Rect::new(x, y, inner - INSET * 2.0, NOTE_H),
                    AboutLook::Note {
                        head: false,
                        bullet: i == 0,
                    },
                )
                .text(line),
            );
            y += NOTE_H;
        }
    }
    // UI-282: the reason it could not go on, or the installer's note. The download's progress is
    // in the button itself, so it is not said twice.
    if let Some(note) = &v.notice
        && !matches!(&v.progress.phase, update::Phase::Failed(_))
    {
        y = notice_lines(&mut cells, v, x, y, note, false);
    }
    if let update::Phase::Failed(reason) = &v.progress.phase {
        let lines = crate::opline::wrap(reason, inner - INSET * 2.0, &|t| {
            text_width(t, FontSize::Small)
        });
        for line in lines {
            cells.push(
                about(
                    Rect::new(x, y, inner - INSET * 2.0, NOTE_H),
                    AboutLook::Alert,
                )
                .text(line),
            );
            y += NOTE_H;
        }
        button(
            &mut cells,
            x,
            y,
            "Release のページを開く",
            "open",
            Act::Page,
        );
        cells.last_mut().expect("the button").tip = page_url(v);
        y += BUTTON_H + Space::X1.px();
    }
    y += INSET - 2.0;
    cells[block].rect.h = y - top;
    rest(&mut cells, v, y + INSET, offset);
    cells
}

/// The button while the installer comes: how far it is, or its size when the release did not
/// say (`ダウンロード 42%`, `ダウンロード 12.3 MB`, UI-282).
fn download_label(progress: &update::Progress) -> String {
    match progress.percent() {
        Some(percent) => format!("ダウンロード {percent}%"),
        None => format!("ダウンロード {}", size_text(progress.got)),
    }
}

/// The page an update is got from: the release's, or the releases list when none is known.
fn page_url(v: &View) -> String {
    v.newer
        .as_ref()
        .map(|r| r.url.clone())
        .unwrap_or_else(|| format!("{}/releases", update::REPOSITORY))
}

/// The release a kept installer stands for, when the asking could not say it (UI-282).
fn release_of(installer: &update::Installer) -> update::Release {
    update::Release {
        version: installer.version.clone(),
        url: format!("{}/releases/tag/v{}", update::REPOSITORY, installer.version),
        notes: Vec::new(),
        prerelease: installer.manifest.channel == update::BETA,
        assets: Vec::new(),
    }
}

/// A reason or the installer's note and, when `page`, the button to the release's page.
fn notice_lines(
    cells: &mut Vec<Cell>,
    v: &View,
    x: f32,
    mut y: f32,
    text: &str,
    page: bool,
) -> f32 {
    let room = W - x - PAD;
    let lines = crate::opline::wrap(text, room, &|t| text_width(t, FontSize::Small));
    for line in lines {
        cells.push(about(Rect::new(x, y, room, NOTE_H), AboutLook::Alert).text(line));
        y += NOTE_H;
    }
    if page {
        button(cells, x, y, "Release のページを開く", "open", Act::Page);
        cells.last_mut().expect("the button").tip = page_url(v);
        y += BUTTON_H + Space::X1.px();
    }
    y
}

/// A size in the units a person reads (`12.3 MB`).
fn size_text(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b >= K * K {
        format!("{:.1} MB", b / (K * K))
    } else if b >= K {
        format!("{:.0} KB", b / K)
    } else {
        format!("{bytes} B")
    }
}

/// A line of what changed.
const NOTE_H: f32 = 20.0;
/// A Rust library's row, and how many of them show at once.
const CRATE_H: f32 = 26.0;
pub const CRATE_ROWS: usize = 6;

/// How far the wheel can move the list of the Rust libraries.
pub fn max_offset(v: &View) -> usize {
    if v.open != Some(Item::Rust) || v.crashes.is_some() {
        return 0;
    }
    matching(&v.query).len().saturating_sub(CRATE_ROWS)
}
/// The bullet's room before a line of what changed.
const BULLET_W: f32 = 14.0;

/// The programs the key presets are named after belong to their owners; the panel says so once
/// (LIC-009). The words of the screens are Japanese.
pub const TRADEMARKS_NOTE: &str = "Studio Pro・Cubase・Logic Pro・Ableton Live・REAPER は各社の製品名で、Xirius は各社と関係がありません。";

/// The newer version's chip: the arrow and its number.
fn release_w(version: &str) -> f32 {
    Space::X2.px() * 2.0 + Layout::ICON_SIZE + Space::X1.px() + text_width(version, FontSize::Small)
}

/// The name a screen reader reads for the repository's button, and the first line of its
/// balloon (UI-303).
pub const GITHUB: &str = "GitHub のリポジトリ";

/// At the end of the version row, the GitHub mark that opens the repository (LIC-016, UI-303).
/// Its balloon names it and says where it goes. No donation until there is somewhere to give to.
fn github(cells: &mut Vec<Cell>, x: f32, y: f32) {
    let url = update::REPOSITORY.trim_start_matches("https://");
    cells.push(
        about(Rect::new(x, y, BUTTON_H, BUTTON_H), AboutLook::Bare)
            .icon("github")
            .act(Action::About(Act::Repository))
            .tip(format!("{GITHUB}\n{url}")),
    );
}

/// The software, from `y` down.
fn rest(cells: &mut Vec<Cell>, v: &View, mut y: f32, offset: usize) {
    cells.push(
        Cell::new(Rect::new(PAD, y, W - PAD * 2.0, 20.0), Look::Label)
            .text("使っているソフトウェア"),
    );
    y += 26.0;
    let inner = W - PAD * 2.0;
    for item in Item::ALL {
        let open = v.open == Some(item);
        let block = cells.len();
        if open {
            cells.push(about(Rect::new(PAD, y, inner, 0.0), AboutLook::Block));
        }
        cells.push(
            about(
                Rect::new(PAD, y, inner, ROW_H),
                AboutLook::Item {
                    license: item.license(),
                    open,
                },
            )
            .text(item.name())
            .hint(item.what())
            .on(open)
            .act(Action::About(Act::Toggle(item))),
        );
        let top = y;
        y += ROW_H;
        if !open {
            continue;
        }
        let x = PAD + INSET;
        let room = inner - INSET * 2.0;
        match item {
            Item::Lame => {
                let t = "フォルダーを開く";
                let bw = Space::X2.px() * 2.0
                    + Layout::ICON_SIZE
                    + Space::X2.px()
                    + text_width(t, FontSize::Small);
                let path = v.lame_path.display().to_string();
                let mut file = about(
                    Rect::new(x, y, room - bw - Space::X2.px(), BUTTON_H),
                    AboutLook::File,
                )
                .text(path.clone());
                // A long path keeps its end, the file's name; the balloon has it whole.
                if tail(&path, file_room(file.rect)) != path {
                    file.tip = path;
                }
                cells.push(file);
                button(cells, x + room - bw, y, t, "folder-open", Act::Reveal);
                y += BUTTON_H + Space::X2.px();
                if let Some(e) = &v.lame_error {
                    cells.push(about(Rect::new(x, y, room, 20.0), AboutLook::Alert).text(e));
                    y += 20.0 + Space::X1.px();
                }
                let w = button(cells, x, y, "ソースコード", "open", Act::Source);
                let w2 = button(
                    cells,
                    x + w + Space::X2.px(),
                    y,
                    "リリースのソース",
                    "download",
                    Act::SourceRelease,
                );
                button(
                    cells,
                    x + w + w2 + Space::X2.px() * 2.0,
                    y,
                    "ライセンス文",
                    "file",
                    Act::License(item),
                );
            }
            Item::Inter | Item::Noto | Item::Octicons => {
                button(cells, x, y, "ライセンス文", "file", Act::License(item));
            }
            Item::Rust => {
                // The search, the libraries that match (the wheel moves the list) and the
                // licence texts.
                cells.push(
                    Cell::new(Rect::new(x, y, room, BUTTON_H), Look::Input)
                        .text(v.query.clone())
                        .icon("search")
                        .on(true),
                );
                y += BUTTON_H + Space::X1.px();
                for (i, c) in matching(&v.query)
                    .into_iter()
                    .skip(offset)
                    .take(CRATE_ROWS)
                    .enumerate()
                {
                    cells.push(
                        about(
                            Rect::new(x, y + i as f32 * CRATE_H, room, CRATE_H),
                            AboutLook::Crate {
                                version: c.version,
                                license: c.license,
                            },
                        )
                        .text(c.name),
                    );
                }
                y += CRATE_ROWS as f32 * CRATE_H + Space::X1.px();
                button(cells, x, y, "ライセンス文", "file", Act::License(item));
            }
        }
        y += BUTTON_H + INSET;
        cells[block].rect.h = y - top;
    }
    // The names of other programs are theirs: one line of small print under the list.
    for line in crate::opline::wrap(TRADEMARKS_NOTE, W - PAD * 2.0, &|t| {
        text_width(t, FontSize::Caption)
    }) {
        cells.push(about(Rect::new(PAD, y, W - PAD * 2.0, 16.0), AboutLook::Caption).text(line));
        y += 16.0;
    }
    y += INSET;
    cells.push(
        Cell::new(Rect::new(PAD, y, W - PAD * 2.0, 30.0), Look::Row)
            .text("クラッシュ記録")
            .icon("file")
            .act(Action::About(Act::Crashes)),
    );
    y += 38.0;
    // The bottom margin is the pop-over's own.
    cells.push(about(
        Rect::new(PAD, y, 1.0, PAD - Layout::POPUP_PAD),
        AboutLook::Sub,
    ));
}

/// The heading uses only SVG masks; `Cell::text` is its accessible name.
fn paint_title(
    r: Rect,
    c: &crate::paint::Colors,
    mut svg: impl FnMut(gpui::SharedString, Rect, gpui::Hsla),
) {
    let mark = Rect::new(r.x, r.y + (r.h - MARK) / 2.0, MARK, MARK);
    for (n, color) in [c.fill, c.ink_blue].into_iter().enumerate() {
        svg(crate::assets::mark_layer(n), mark, color);
    }
    // GPUI uses the SVG's alpha mask. Blue is the exact logo-dark / logo-light ink. In
    // lockup.svg the wordmark's midline is the star's, so it is centred on the mark's box.
    svg(
        crate::assets::LOGO_PATH.into(),
        Rect::new(
            mark.right() + LOGO_GAP,
            mark.y + (mark.h - WORDMARK) / 2.0,
            WORDMARK * crate::assets::LOGO_ASPECT,
            WORDMARK,
        ),
        c.ink_blue,
    );
}

pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: &AboutLook, hot: bool) {
    let c = pen.c;
    let r = cell.rect;
    let icon_at = |x: f32| {
        Rect::new(
            x,
            r.y + (r.h - Layout::ICON_SIZE) / 2.0,
            Layout::ICON_SIZE,
            Layout::ICON_SIZE,
        )
    };
    match look {
        AboutLook::Title => paint_title(r, c, |path, rect, color| pen.svg(path, rect, color)),
        AboutLook::Sub => {
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                r,
                Align::Left,
            );
        }
        AboutLook::Caption => {
            pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                r,
                Align::Left,
            );
        }
        AboutLook::Block => pen.round(r, c.surface_2, Radius::M),
        AboutLook::Item { license, open } => {
            if hot && !*open {
                pen.round(r, c.hover, Radius::M);
            }
            let x = r.x + INSET;
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Semibold,
                c.ink_strong,
                Rect::new(x, r.y + 7.0, r.w, 18.0),
                Align::Left,
            );
            pen.text(
                &cell.hint,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(x, r.y + 25.0, r.w, 16.0),
                Align::Left,
            );
            let chev = r.right() - INSET - Layout::ICON_SIZE;
            pen.icon(
                if *open { "chevron-up" } else { "chevron-down" },
                icon_at(chev),
                if hot { c.ink_strong } else { c.ink },
            );
            pen.text(
                license,
                FontSize::Caption,
                Weight::Medium,
                c.ink,
                Rect::new(r.x, r.y, chev - Space::X3.px() - r.x, r.h),
                Align::Right,
            );
        }
        AboutLook::File => {
            pen.icon("file", icon_at(r.x), c.ink);
            let x = r.x + Layout::ICON_SIZE + Space::X2.px();
            let room = Rect::new(x, r.y, r.right() - x, r.h);
            pen.text(
                &tail(&cell.text, room.w),
                FontSize::Small,
                Weight::Medium,
                c.ink_strong,
                room,
                Align::Left,
            );
        }
        AboutLook::Release => {
            pen.round(r, c.surface_3, Radius::S);
            let x = r.x + Space::X2.px();
            pen.icon("download", icon_at(x), c.ink_strong);
            let x = x + Layout::ICON_SIZE + Space::X1.px();
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Medium,
                c.ink_strong,
                Rect::new(x, r.y, r.right() - x, r.h),
                Align::Left,
            );
        }
        AboutLook::Note { head, bullet } => {
            let weight = if *head {
                Weight::Semibold
            } else {
                Weight::Regular
            };
            let x = if *head { r.x } else { r.x + BULLET_W };
            if *bullet {
                pen.text(
                    "•",
                    FontSize::Small,
                    Weight::Regular,
                    c.ink_strong,
                    Rect::new(r.x, r.y, BULLET_W, r.h),
                    Align::Left,
                );
            }
            pen.text(
                &cell.text,
                FontSize::Small,
                weight,
                c.ink_strong,
                Rect::new(x, r.y, r.right() - x, r.h),
                Align::Left,
            );
        }
        AboutLook::Alert => {
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Regular,
                c.ink_alert,
                r,
                Align::Left,
            );
        }
        AboutLook::Crate { version, license } => {
            let pad = Space::X2.px() - 2.0;
            let x = r.x + pad;
            let w = pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Medium,
                c.ink_strong,
                Rect::new(x, r.y, r.w, r.h),
                Align::Left,
            );
            let x = x + w + Space::X2.px();
            pen.digits(
                version,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(x, r.y, r.right() - x, r.h),
                Align::Left,
            );
            pen.text(
                license,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(r.x, r.y, r.w - pad, r.h),
                Align::Right,
            );
        }
        AboutLook::Bare => {
            if hot {
                pen.round(r, c.hover, Radius::S);
            }
            if let Some(icon) = cell.icon {
                let ink = if hot { c.ink_strong } else { c.ink };
                pen.icon(icon, icon_at(r.x + (r.w - Layout::ICON_SIZE) / 2.0), ink);
            }
        }
    }
}

impl Studio {
    /// Opens the panel under the logo, or closes it when it is open.
    pub(crate) fn open_about(&mut self, anchor: Rect) {
        self.open_popup(PopupKind::About, anchor);
        let newer = self.newer_release().cloned();
        let progress = self.update.progress();
        let notice = self.update.notice.clone();
        if let Some(p) = &mut self.popup
            && p.kind == PopupKind::About
        {
            p.about = Some(Box::new(View {
                newer,
                progress,
                notice,
                ..View::now()
            }));
        }
    }

    /// Asks GitHub whether a newer version is out, off the window's thread, unless the setting
    /// is off (UI-262). [`Studio::poll_update`] takes the answer. A checked installer kept from
    /// the last run is taken up now, so it can be put in without asking again (UI-282).
    pub(crate) fn check_for_update(&mut self, check: update::UpdateCheck, now_ms: u64) {
        if !self.update_on() {
            return;
        }
        let root = check.root().to_path_buf();
        self.update.root = Some(root.clone());
        let beta = self.update_beta();
        if let Some(note) = update::take_failure(&root) {
            self.update.notice = Some(note);
        }
        if let Some(installer) =
            update::ready_installer(&root, update::CURRENT, beta, &self.update.key)
        {
            let total = installer.path.metadata().map(|m| m.len()).unwrap_or(0);
            self.update.ready = Some(installer);
            self.update.set_progress(update::Progress {
                phase: update::Phase::Ready,
                got: total,
                total,
            });
        }
        self.update.asking = Some(std::thread::spawn(move || {
            check.at_launch(true, beta, now_ms, update::CURRENT)
        }));
    }

    fn update_on(&self) -> bool {
        let shared = self.song.settings().clone();
        let s = shared.lock().unwrap_or_else(|e| e.into_inner());
        s.prefs.bool(UPDATE_PREF)
    }

    /// Does this window take the beta channel (UI-282)?
    fn update_beta(&self) -> bool {
        let shared = self.song.settings().clone();
        let s = shared.lock().unwrap_or_else(|e| e.into_inner());
        s.prefs.bool(update::BETA_PREF)
    }

    /// Once a frame. True while the asking or the download runs and on the frame it ends.
    pub(crate) fn poll_update(&mut self) -> bool {
        let mut changed = false;
        if self.update.asking.as_ref().is_some_and(|a| a.is_finished())
            && let Some(asking) = self.update.asking.take()
        {
            self.update.newer = asking.join().ok().flatten();
            // The kept installer is only for the version just found.
            if let (Some(release), Some(installer)) = (&self.update.newer, &self.update.ready)
                && release.version != installer.version
            {
                self.update.ready = None;
                self.update.set_progress(update::Progress::default());
            }
            changed = true;
        }
        if self.update.job.as_ref().is_some_and(|j| j.is_finished())
            && let Some(job) = self.update.job.take()
        {
            let before = self.update.progress();
            let phase = match job.join() {
                Ok(Ok(installer)) => {
                    self.update.ready = Some(installer);
                    update::Phase::Ready
                }
                Ok(Err(e)) => update::Phase::Failed(e),
                Err(_) => update::Phase::Failed("ダウンロードに失敗しました".into()),
            };
            self.update
                .set_progress(update::Progress { phase, ..before });
            changed = true;
        }
        // A checked installer is offered even when the asking could not be made.
        if self.update.newer.is_none()
            && let Some(installer) = &self.update.ready
        {
            self.update.newer = Some(release_of(installer));
            changed = true;
        }
        // The worker cannot reach the panel's copy: it is brought up to date here.
        let newer = self.newer_release().cloned();
        let progress = self.update.progress();
        let notice = self.update.notice.clone();
        if let Some(p) = &mut self.popup
            && p.kind == PopupKind::About
            && let Some(v) = &mut p.about
        {
            v.newer = newer;
            v.progress = progress;
            v.notice = notice;
        }
        self.update.asking.is_some() || self.update.job.is_some() || changed
    }

    /// The newer version to show: none while the setting is off.
    pub fn newer_release(&self) -> Option<&update::Release> {
        self.update.newer.as_ref().filter(|_| self.update_on())
    }

    /// May this build and this folder put an update in (UI-282)? The build without a key, and
    /// one unpacked from the zip, only open the release's page in the browser.
    pub(crate) fn can_download_update(&self) -> bool {
        !update::is_placeholder_key(&self.update.key)
            && self
                .update
                .location
                .as_ref()
                .is_some_and(|l| l.installable())
    }

    /// Starts the download off the window's thread; [`Studio::poll_update`] takes it.
    fn start_update_download(&mut self) -> Result<(), String> {
        if self.update.job.is_some() {
            return Ok(());
        }
        let release = self
            .update
            .newer
            .clone()
            .ok_or("新しいバージョンがありません")?;
        let root = self
            .update
            .root
            .clone()
            .unwrap_or_else(xirius_session::recent::root);
        let total = release.setup().map_or(0, |a| a.size);
        self.update.set_progress(update::Progress {
            phase: update::Phase::Downloading,
            got: 0,
            total,
        });
        self.update.ready = None;
        self.update.notice = None;
        let progress = self.update.progress.clone();
        let fetch = self.update.fetch.clone();
        let download = self.update.download.clone();
        let key = self.update.key.clone();
        let beta = self.update_beta();
        self.update.job = Some(std::thread::spawn(move || {
            let mut report = |p: update::Progress| {
                *progress.lock().unwrap_or_else(|e| e.into_inner()) = p;
            };
            update::prepare(&root, &release, &fetch, &download, &key, beta, &mut report)
        }));
        Ok(())
    }

    /// Schedules the checked installer for the end of this window (UI-282). It is checked once
    /// more, and no other Xirius program may still be running (the silent installer would wait
    /// for it and give up).
    fn arm_install(&mut self) -> Result<(), String> {
        let installer = self
            .update
            .ready
            .clone()
            .ok_or("インストーラーがありません")?;
        let location = self
            .update
            .location
            .clone()
            .ok_or("インストールの場所が分かりません")?;
        installer.verify_with(&self.update.key)?;
        let running = update::running_programs(&location.install_dir, std::process::id());
        if !running.is_empty() {
            return Err(format!("{} が動いています", running.join("、")));
        }
        let install = location.install_command(&installer.path)?;
        self.update.notice = None;
        self.update.pending = Some(install);
        self.window_ask = Some(crate::context::key_more::WindowAsk::Quit);
        Ok(())
    }

    /// The installer to start once the window has closed (UI-282). It is checked once more
    /// here, at the last moment before it is started; a file replaced while the song was being
    /// saved is not run, and the next start says so.
    pub(crate) fn take_pending_install(&mut self) -> Option<update::Install> {
        let install = self.update.pending.take()?;
        let problem = match &self.update.ready {
            Some(installer) => installer.verify_with(&self.update.key).err(),
            None => Some("インストーラーがありません".to_string()),
        };
        if let Some(e) = problem {
            if let Some(root) = &self.update.root {
                let _ = std::fs::write(root.join(update::FAILED), b"");
            }
            eprintln!("注意: {e}");
            return None;
        }
        Some(install)
    }

    /// The logo, in body coordinates (the palette opens the panel under it too).
    pub(crate) fn about_anchor(&self, g: &crate::studio::Geometry) -> Rect {
        let mut r =
            crate::topbar::BarLayout::new(g.size.0, &self.project().name).rect(BarButton::Logo);
        r.y -= Layout::TOP_BAR_H;
        if r.w <= 0.0 {
            // The narrow bar has no logo: under the song's name instead.
            r = Rect::new(Space::X3.px(), r.y, 0.0, r.h);
        }
        r
    }

    /// Opens `url` in the browser; tests and the screenshot scripts only note it in `opened`.
    pub(crate) fn open_web(&mut self, url: &str) -> Result<(), String> {
        self.opened.push(url.to_string());
        if self.os_files {
            xirius_session::os::open(url)
        } else {
            Ok(())
        }
    }

    /// Typing into the search over the Rust libraries, while their row is open.
    pub(crate) fn about_typed(&mut self, key: &str, ch: Option<&str>) -> Option<bool> {
        let p = self.popup.as_mut().filter(|p| p.kind == PopupKind::About)?;
        let v = p
            .about
            .as_mut()
            .filter(|v| v.open == Some(Item::Rust) && v.crashes.is_none())?;
        if key == "backspace" {
            v.query.pop();
        } else {
            let ch = ch.filter(|c| !c.is_empty() && !c.chars().any(char::is_control))?;
            v.query.push_str(ch);
        }
        p.offset = 0;
        Some(true)
    }

    pub(crate) fn about_action(&mut self, act: Act) -> bool {
        if self.popup.as_ref().and_then(|p| p.about.as_ref()).is_none() {
            return false;
        }
        let done = match act {
            Act::Source => self.open_web(mp3::LAME_SOURCE_URL),
            Act::SourceRelease => self.open_web(mp3::LAME_RELEASE_URL),
            Act::Repository => self.open_web(update::REPOSITORY),
            // UI-282: with a key and the per-user folder, the button downloads and checks;
            // otherwise it opens the release's page, as it did before.
            Act::Download if self.can_download_update() => self.start_update_download(),
            Act::Download | Act::Page => {
                let url = self
                    .popup
                    .as_ref()
                    .and_then(|p| p.about.as_deref())
                    .map(page_url);
                match url {
                    Some(url) => self.open_web(&url),
                    None => return false,
                }
            }
            // UI-282: one button. It saves, closes, and Xirius comes back in the new version.
            Act::UpdateNow => {
                if let Err(why) = self.arm_install() {
                    // Something is still running or the installer changed: say it here, and
                    // let the user press the button again.
                    self.update.notice = Some(why);
                }
                Ok(())
            }
            act => return self.about_view_action(act),
        };
        if let Err(e) = done {
            let before = self.update.progress();
            self.update.set_progress(update::Progress {
                phase: update::Phase::Failed(e.clone()),
                ..before
            });
            self.error = Some(e);
        }
        true
    }

    fn about_view_action(&mut self, act: Act) -> bool {
        let Some(v) = self.popup.as_mut().and_then(|p| p.about.as_mut()) else {
            return false;
        };
        let done = match act {
            Act::Crashes => {
                v.crashes = if v.crashes.is_some() {
                    None
                } else {
                    Some(Box::new(crate::crash_reports::View::load()))
                };
                return true;
            }
            Act::Crash(act) => {
                let Some(crashes) = v.crashes.as_mut() else {
                    return false;
                };
                match act {
                    crate::crash_reports::Act::Previous => {
                        crashes.page = crashes.page.saturating_sub(1);
                        return true;
                    }
                    crate::crash_reports::Act::Next => {
                        if (crashes.page + 1) * 4 < crashes.records.len() {
                            crashes.page += 1;
                        }
                        return true;
                    }
                    _ => match crashes.path(act) {
                        Some(path) if self.os_files => {
                            if matches!(act, crate::crash_reports::Act::Folder(_)) {
                                xirius_session::os::open(&path.display().to_string())
                            } else {
                                xirius_session::os::reveal(&path)
                            }
                        }
                        Some(_) => Ok(()),
                        None => Err("クラッシュ記録がありません".into()),
                    },
                }
            }
            Act::Close => {
                self.popup = None;
                return true;
            }
            Act::Toggle(item) => {
                v.open = (v.open != Some(item)).then_some(item);
                v.query.clear();
                if let Some(p) = &mut self.popup {
                    p.offset = 0;
                }
                return true;
            }
            Act::Reveal => {
                let path = v.lame_path.clone();
                if path.exists() {
                    xirius_session::os::reveal(&path)
                } else {
                    // Nothing to pick: open the folder it is looked for in.
                    path.parent()
                        .map(|d| d.display().to_string())
                        .ok_or_else(|| "フォルダーがありません".to_string())
                        .and_then(|d| xirius_session::os::open(&d))
                }
            }
            // The web pages: [`Studio::about_action`] opens them.
            Act::Source
            | Act::SourceRelease
            | Act::Repository
            | Act::Download
            | Act::Page
            | Act::UpdateNow => Ok(()),
            Act::License(item) => {
                license_file(item).and_then(|p| xirius_session::os::open(&p.display().to_string()))
            }
        };
        if let Err(e) = done {
            self.error = Some(e);
        }
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::studio::Hit;

    fn cells_of(s: &Studio, g: &crate::studio::Geometry) -> Vec<Cell> {
        s.popup.as_ref().unwrap().layout(g.size).cells
    }

    /// `mark.svg`'s viewBox height and its first path's top and bottom, so a test can work
    /// out how much of the drawn box is the star's art.
    fn mark_art_y() -> (f32, f32, f32) {
        let svg = include_str!("../../../assets/brand/mark.svg");
        let view = svg.split("viewBox=\"").nth(1).unwrap();
        let view_h: f32 = view
            .split('"')
            .next()
            .unwrap()
            .split(' ')
            .nth(3)
            .unwrap()
            .parse()
            .unwrap();
        let d = svg
            .split(" d=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let nums: Vec<f32> = d
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .filter(|s| !s.is_empty())
            .map(|n| n.parse().unwrap())
            .collect();
        let (mut top, mut bottom) = (f32::MAX, f32::MIN);
        for y in nums.iter().skip(1).step_by(2) {
            top = top.min(*y);
            bottom = bottom.max(*y);
        }
        (view_h, top, bottom)
    }

    /// UI-308: record the same SVG calls as the title painter, without opening a window.
    #[test]
    fn the_heading_paints_the_wordmark_and_keeps_its_accessible_name() {
        use gpui::AssetSource;
        use xirius_design::Theme;

        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.press(&g, Hit::Bar(BarButton::Logo));
        let cells = cells_of(&s, &g);
        let (at, title) = cells
            .iter()
            .enumerate()
            .find(|(_, c)| c.look == Look::About(AboutLook::Title))
            .unwrap();
        assert_eq!(title.text, "Xirius");
        let nodes = crate::a11y::about_nodes(&s, &g);
        let (_, node) = nodes.iter().find(|(i, _)| *i == at).unwrap();
        assert_eq!(node.role(), gpui::accesskit::Role::Heading);
        assert_eq!(node.label(), Some("Xirius"));

        let lockup = include_str!("../../../assets/brand/lockup.svg");
        assert!(lockup.contains("scale(.5)"));
        assert!(lockup.contains("translate(272 56)"));
        for (theme, artwork) in [
            (
                Theme::Dark,
                include_str!("../../../assets/brand/logo-dark.svg"),
            ),
            (
                Theme::Light,
                include_str!("../../../assets/brand/logo-light.svg"),
            ),
        ] {
            let c = crate::paint::Colors::new(theme);
            let mut masks = Vec::new();
            paint_title(title.rect, &c, |path, rect, color| {
                masks.push((path, rect, color))
            });
            assert_eq!(masks.len(), 3, "two star layers and the wordmark; no text");
            for (n, color) in [c.fill, c.ink_blue].into_iter().enumerate() {
                assert_eq!(masks[n].0, crate::assets::mark_layer(n));
                assert_eq!(masks[n].2, color);
            }
            let (path, logo, ink) = &masks[2];
            assert_eq!(path.as_ref(), crate::assets::LOGO_PATH);
            let bytes = crate::assets::Assets.load(path).unwrap().unwrap();
            let svg = std::str::from_utf8(&bytes).unwrap();
            assert_eq!(svg.matches("data-glyph=").count(), 6);
            assert!(
                !svg.contains("<text"),
                "the letters are artwork, not font text"
            );
            let color = artwork
                .split("<g fill=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            let expected: gpui::Hsla =
                gpui::rgb(u32::from_str_radix(&color[1..], 16).unwrap()).into();
            assert_eq!(*ink, expected, "the brand's colour for {theme:?}");
            let mark = masks[0].1;
            assert_eq!(masks[1].1, mark);
            assert_eq!(
                (mark.w, mark.h),
                (32.0, 32.0),
                "the star's box is the row's"
            );
            assert_eq!(logo.h, 18.0);
            assert_eq!(logo.w, logo.h * crate::assets::LOGO_ASPECT);
            // lockup.svg's ratio: the star's art is 336 of its 512 box (its top and bottom
            // margins match, so the box's midline is the art's), the wordmark's fills its
            // 144; the seen star is 168/144 ≈ 1.17 of it on the same midline.
            let (view_h, art_top, art_bottom) = mark_art_y();
            assert_eq!(
                art_top,
                view_h - art_bottom,
                "the art is centred in the box"
            );
            let star = mark.h * (art_bottom - art_top) / view_h;
            assert!(
                (star / logo.h - 168.0 / 144.0).abs() < 0.01,
                "the seen star is the wordmark's 168/144 ({star} vs {})",
                logo.h
            );
            assert_eq!(mark.y + mark.h / 2.0, logo.y + logo.h / 2.0, "one midline");
            assert_eq!(logo.x - mark.right(), mark.w * 16.0 / 256.0);
            assert!(title.rect.contains(logo.x, logo.y));
            assert!(logo.right() <= title.rect.right());
        }
    }

    #[test]
    fn a_long_path_keeps_its_file_name_and_says_itself_whole_in_a_balloon() {
        let short = format!(r"C:\Xirius\{}", mp3::LAME_FILE);
        assert_eq!(tail(&short, 300.0), short);
        let long = format!(r"D:\{}{}", r"a-long-folder-name\".repeat(8), mp3::LAME_FILE);
        let t = tail(&long, 300.0);
        assert!(t.starts_with('…') && t.ends_with(mp3::LAME_FILE), "{t}");
        assert!(text_width(&t, FontSize::Small) <= 300.0);

        let mut v = View::now();
        v.open = Some(Item::Lame);
        v.lame_path = PathBuf::from(&long);
        let cells = cells(&v, 0);
        let file = cells
            .iter()
            .find(|c| matches!(c.look, Look::About(AboutLook::File)))
            .unwrap();
        assert_eq!(file.tip, long);
        v.lame_path = PathBuf::from(&short);
        let cells = super::cells(&v, 0);
        let file = cells
            .iter()
            .find(|c| matches!(c.look, Look::About(AboutLook::File)))
            .unwrap();
        assert!(file.tip.is_empty(), "a path that fits needs no balloon");
    }

    fn acts(cells: &[Cell]) -> Vec<Act> {
        cells
            .iter()
            .filter(|c| c.live())
            .filter_map(|c| match c.action {
                Some(Action::About(a)) => Some(a),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_logo_opens_it_and_a_second_press_or_escape_closes_it() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        assert!(s.press(&g, Hit::Bar(BarButton::Logo)));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::About));
        assert_eq!(s.bar_view().open, Some(BarButton::Logo));
        s.press(&g, Hit::Bar(BarButton::Logo));
        assert!(s.popup.is_none(), "a second press closes it");

        s.press(&g, Hit::Bar(BarButton::Logo));
        assert!(s.key(&g, "escape", false, false));
        assert!(s.popup.is_none(), "Esc closes it");

        s.press(&g, Hit::Bar(BarButton::Logo));
        let close = cells_of(&s, &g)
            .into_iter()
            .position(|c| c.action == Some(Action::About(Act::Close)))
            .unwrap();
        s.press(&g, Hit::Item(close));
        assert!(s.popup.is_none(), "the × closes it");
    }

    #[test]
    fn it_lists_the_software_and_lames_three_lgpl_duties() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.press(&g, Hit::Bar(BarButton::Logo));
        let cells = cells_of(&s, &g);
        let texts: Vec<&str> = cells.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"Xirius"));
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("バージョン ") && t.ends_with(" · MIT OR Apache-2.0"))
        );
        assert!(texts.contains(&"使っているソフトウェア"));
        for (name, what, license) in [
            ("LAME 3.100", "MP3 の書き出し", "LGPL-2.0-or-later"),
            ("Inter 4.0", "画面の文字", "OFL-1.1"),
            ("Noto Sans JP", "画面の文字（日本語）", "OFL-1.1"),
            ("Octicons 19.41.0", "GitHub の印", "MIT"),
        ] {
            let c = cells.iter().find(|c| c.text == name).expect(name);
            assert_eq!(c.hint, what);
            assert!(
                matches!(c.look, Look::About(AboutLook::Item { license: l, .. }) if l == license)
            );
        }
        let n = libraries().len();
        assert!(n > 100, "{n}");
        assert!(texts.contains(&format!("ライブラリ {n} 件").as_str()));

        // LAME is open: its file, the swap, the source and the licence text.
        let file = cells
            .iter()
            .find(|c| c.look == Look::About(AboutLook::File))
            .unwrap();
        assert!(file.text.ends_with(mp3::LAME_FILE), "{}", file.text);
        assert!(
            !texts.iter().any(|t| t.contains("置き換え")),
            "the row says what it is, not how to use it: {texts:?}"
        );
        assert!(texts.contains(&"フォルダーを開く"));
        assert!(texts.contains(&"ソースコード"));
        assert!(texts.contains(&"リリースのソース"));
        let a = acts(&cells);
        for act in [
            Act::Reveal,
            Act::Source,
            Act::SourceRelease,
            Act::License(Item::Lame),
        ] {
            assert!(a.contains(&act), "{act:?}");
        }
        // The three buttons of the source row fit the panel side by side.
        let last = cells
            .iter()
            .find(|c| c.action == Some(Action::About(Act::License(Item::Lame))))
            .unwrap();
        assert!(last.rect.right() <= W - PAD, "{:?}", last.rect);
        assert!(
            !a.contains(&Act::License(Item::Inter)),
            "one row open at a time"
        );
    }

    #[test]
    fn a_row_opens_and_closes_by_its_head() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.press(&g, Hit::Bar(BarButton::Logo));
        let h0 = s.popup.as_ref().unwrap().layout(g.size).rect.h;
        assert!(s.popup_action(&g, Action::About(Act::Toggle(Item::Inter))));
        let cells = cells_of(&s, &g);
        let a = acts(&cells);
        assert!(a.contains(&Act::License(Item::Inter)));
        assert!(!a.contains(&Act::Reveal), "LAME closed");
        assert!(s.popup_action(&g, Action::About(Act::Toggle(Item::Inter))));
        let a = acts(&cells_of(&s, &g));
        assert!(!a.iter().any(|a| matches!(a, Act::License(_))));
        assert!(s.popup.as_ref().unwrap().layout(g.size).rect.h < h0);
    }

    #[test]
    fn the_licence_texts_are_written_whole() {
        // A folder of the test's own, so the test leaves nothing in the temp folder.
        let dir = tempfile::Builder::new()
            .prefix("xirius-licenses-")
            .tempdir()
            .unwrap();
        let license_file = |item| write_license(dir.path(), item);
        let lame = std::fs::read_to_string(license_file(Item::Lame).unwrap()).unwrap();
        assert!(
            lame.contains("GNU LIBRARY GENERAL PUBLIC LICENSE"),
            "{}",
            &lame[..200]
        );
        for item in [Item::Inter, Item::Noto] {
            let t = std::fs::read_to_string(license_file(item).unwrap()).unwrap();
            assert!(t.contains("SIL OPEN FONT LICENSE"), "{item:?}");
        }
        let octicons = std::fs::read_to_string(license_file(Item::Octicons).unwrap()).unwrap();
        assert!(octicons.starts_with("MIT License") && octicons.contains("GitHub Inc."));
        // The Rust libraries: cargo-about's list whole, the libraries and then their texts.
        let rust = std::fs::read_to_string(license_file(Item::Rust).unwrap()).unwrap();
        assert_eq!(rust, THIRD_PARTY);
        assert!(
            rust.contains(
                "
crate serde_json "
            ),
            "{}",
            &rust[..200]
        );
        assert!(
            rust.contains(
                "
==== MIT
"
            ) && rust.contains("Apache License")
        );
        assert!(
            !rust.contains(
                "
crate xirius-"
            ),
            "the workspace's own crates are not listed"
        );
    }

    /// LIC-007: the list the app ships is the one the lock file gives. Every library in it is
    /// in `Cargo.lock` at the same version, and what the app is built of is in it.
    #[test]
    fn the_list_of_the_rust_libraries_is_the_lock_files() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
        let locked: std::collections::HashSet<(String, String)> = lock
            .split("[[package]]")
            .skip(1)
            .filter(|p| {
                p.contains(
                    "
source = ",
                )
            })
            .filter_map(|p| {
                let field = |key: &str| {
                    p.lines()
                        .find_map(|l| l.strip_prefix(key))
                        .map(|v| v.trim().trim_matches('"').to_string())
                };
                Some((field("name = ")?, field("version = ")?))
            })
            .collect();
        let list = crates();
        assert!(list.len() > 100, "{}", list.len());
        for c in &list {
            assert!(
                locked.contains(&(c.name.to_string(), c.version.to_string())),
                "{} {} is not in Cargo.lock: run python scripts/licenses.py",
                c.name,
                c.version
            );
            assert!(!c.license.is_empty(), "{}", c.name);
        }
        for name in ["gpui-pre", "serde_json", "windows-sys", "vst3", "cpal"] {
            assert!(list.iter().any(|c| c.name == name), "{name}");
        }
        // Each library has its licence's text.
        let texts = THIRD_PARTY
            .split(
                "
==== ",
            )
            .count()
            - 1;
        assert!(texts > 10, "{texts}");
        for c in list.iter().filter(|c| c.name == "serde_json") {
            assert!(
                THIRD_PARTY.contains(&format!(
                    "
used {} {}
",
                    c.name, c.version
                )),
                "serde_json's licence text"
            );
        }
    }

    /// LIC-021: the code of others copied into a `vendor` folder is not a crate, so cargo-about
    /// does not list it; `scripts/licenses.py` adds it. Each copy with a licence file is in the
    /// list, under its licence, and its text is in what 「ライセンス文」 opens.
    #[test]
    fn copied_code_that_is_not_a_crate_is_listed_with_its_licence_text() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let listed = vendored();
        let mut copies = 0;
        for krate in std::fs::read_dir(root.join("crates")).unwrap() {
            let Ok(dir) = std::fs::read_dir(krate.unwrap().path().join("vendor")) else {
                continue;
            };
            for copy in dir.map(|e| e.unwrap().path()).filter(|p| p.is_dir()) {
                let name = copy.file_name().unwrap().to_string_lossy().into_owned();
                let file = ["LICENSE.txt", "LICENSE", "LICENSE.md", "COPYING"]
                    .iter()
                    .map(|f| copy.join(f))
                    .find(|f| f.is_file())
                    .unwrap_or_else(|| panic!("{name} has a licence file"));
                let c = listed
                    .iter()
                    .find(|c| c.name == name)
                    .unwrap_or_else(|| panic!("{name}: run python scripts/licenses.py"));
                let text = std::fs::read_to_string(file).unwrap().replace("\r\n", "\n");
                if text.starts_with("MIT License") {
                    assert_eq!(c.license, "MIT", "{name}");
                }
                // Lines as the script tidies them.
                let tidied: Vec<&str> = text.lines().map(str::trim_end).collect();
                assert!(
                    THIRD_PARTY.contains(&tidied.join("\n").trim().to_string()),
                    "{name}'s licence text"
                );
                assert!(libraries().contains(c) && matching(&name).contains(c));
                copies += 1;
            }
        }
        // Copies that sit with what they are, not in a `vendor` folder.
        const ELSEWHERE: &[(&str, &str)] = &[(
            "dawproject",
            "crates/xirius-core/src/dawproject/testdata/reference/LICENSE",
        )];
        for &(name, path) in ELSEWHERE {
            let c = listed
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{name}: run python scripts/licenses.py"));
            assert_eq!(c.license, "MIT", "{name}");
            let text = std::fs::read_to_string(root.join(path))
                .unwrap()
                .replace("\r\n", "\n");
            let tidied: Vec<&str> = text.lines().map(str::trim_end).collect();
            assert!(
                THIRD_PARTY.contains(&tidied.join("\n").trim().to_string()),
                "{name}'s licence text"
            );
            copies += 1;
        }
        assert_eq!(
            copies,
            listed.len(),
            "every listed copy is in a vendor folder"
        );
        for name in ["signalsmith-stretch", "signalsmith-linear"] {
            assert!(
                listed.iter().any(|c| c.name == name && c.license == "MIT"),
                "{name}"
            );
        }
        let dir = tempfile::Builder::new()
            .prefix("xirius-licenses-")
            .tempdir()
            .unwrap();
        let opened = std::fs::read_to_string(write_license(dir.path(), Item::Rust).unwrap());
        assert!(
            opened
                .unwrap()
                .contains("Geraint Luff / Signalsmith Audio Ltd.")
        );
    }

    /// LIC-015: the Rust libraries' row lists each with its version and licence, a search
    /// narrows them, the wheel moves the list and 「ライセンス文」 opens the texts.
    #[test]
    fn the_rust_row_lists_the_libraries_and_searches_them() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.press(&g, Hit::Bar(BarButton::Logo));
        let rows = |s: &Studio| -> Vec<(String, AboutLook)> {
            cells_of(s, &g)
                .into_iter()
                .filter(|c| matches!(c.look, Look::About(AboutLook::Crate { .. })))
                .map(|c| {
                    let Look::About(look) = c.look else {
                        unreachable!()
                    };
                    (c.text, look)
                })
                .collect()
        };
        assert!(rows(&s).is_empty(), "shut until the row opens");
        // Opened by pressing its head.
        let head = cells_of(&s, &g)
            .iter()
            .position(|c| c.action == Some(Action::About(Act::Toggle(Item::Rust))))
            .unwrap();
        assert!(s.press(&g, Hit::Item(head)));
        let all = libraries();
        let first = rows(&s);
        assert_eq!(first.len(), CRATE_ROWS);
        assert_eq!(first[0].0, all[0].name);
        assert_eq!(
            first[0].1,
            AboutLook::Crate {
                version: all[0].version,
                license: all[0].license
            }
        );
        let cells = cells_of(&s, &g);
        let search = cells.iter().find(|c| c.look == Look::Input).unwrap();
        assert_eq!(search.icon, Some("search"));
        assert!(acts(&cells).contains(&Act::License(Item::Rust)));
        let p = s.popup.as_ref().unwrap();
        assert_eq!(p.max_offset(g.size), all.len() - CRATE_ROWS);
        // The wheel moves the list a row.
        s.popup.as_mut().unwrap().offset = 1;
        assert_eq!(rows(&s)[0].0, all[1].name);

        // Typing narrows it, from its top; Backspace takes a letter back.
        for ch in ["S", "e", "r", "d", "e", "_", "j"] {
            assert!(s.typed(&g, ch, Some(ch)));
        }
        let found = rows(&s);
        assert_eq!(s.popup.as_ref().unwrap().offset, 0);
        assert_eq!(
            found.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
            ["serde_json"]
        );
        assert_eq!(s.popup.as_ref().unwrap().max_offset(g.size), 0);
        let input = cells_of(&s, &g)
            .into_iter()
            .find(|c| c.look == Look::Input)
            .unwrap();
        assert_eq!(input.text, "Serde_j");
        assert!(s.typed(&g, "backspace", None));
        assert!(s.typed(&g, "backspace", None));
        assert!(rows(&s).iter().all(|r| r.0.contains("serde")));
        // The panel fits the window with the list open.
        let rect = s.popup.as_ref().unwrap().layout(g.size).rect;
        assert!(rect.y + rect.h <= g.size.1, "{rect:?}");
        // Another row shut it: the search is gone and the keys are the screen's again.
        assert!(s.popup_action(&g, Action::About(Act::Toggle(Item::Inter))));
        assert!(rows(&s).is_empty());
        assert_eq!(s.about_typed("a", Some("a")), None);
    }

    /// LIC-030: the NOTICE that ships next to the app names who holds Xirius and the work of
    /// others it is built with; the design values are Xirius's own and carry no notice.
    #[test]
    fn the_notice_names_the_holder_the_fonts_and_the_copied_code() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let file = std::fs::read_to_string(root.join("NOTICE"))
            .unwrap()
            .replace("\r\n", "\n");
        assert!(
            !file.contains("Pleiad"),
            "no attribution for the design values"
        );
        assert!(
            !root.join("crates/xirius-design/NOTICE").exists(),
            "the design crate has no notice of its own"
        );
        // LIC-023: the fonts are built in, and the notice names the texts that ship with them.
        assert!(
            !file.contains("not included"),
            "the fonts are built into the app"
        );
        for (name, _) in crate::fonts::LICENSES {
            assert!(file.contains(name), "{name}");
            assert!(root.join("assets/fonts").join(name).is_file(), "{name}");
        }
        assert!(file.contains("signalsmith-stretch") && file.contains("THIRD-PARTY-LICENSES.txt"));
        // The holder is the licence's.
        let mit = std::fs::read_to_string(root.join("LICENSE-MIT")).unwrap();
        let holder = mit.lines().find(|l| l.starts_with("Copyright")).unwrap();
        assert!(file.lines().any(|l| l.starts_with(holder)), "{holder}");
        // The screen lists the software without a row for the design values.
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.press(&g, Hit::Bar(BarButton::Logo));
        assert!(!cells_of(&s, &g).iter().any(|c| c.text.contains("Pleiad")));
        let rect = s.popup.as_ref().unwrap().layout(g.size).rect;
        assert!(rect.y + rect.h <= g.size.1, "{rect:?}");
    }

    #[test]
    fn the_palette_opens_it_too() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        let rows = s.palette_rows(&g);
        let row = rows.iter().find(|r| r.label == "Xirius について").unwrap();
        assert!(row.enabled);
        s.open_palette(&g);
        assert!(s.run_row(&g, &row.clone()));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::About));
        // The narrow bar has no logo, and the palette still opens it.
        let narrow = s.geometry(1200.0, 800.0);
        s.popup = None;
        assert!(s.run_row(&narrow, &row.clone()));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::About));
    }

    /// LIC-016, UI-303: at the end of the version row, the GitHub mark opens the repository in
    /// the browser. It has no words; its name and where it goes are in its balloon.
    #[test]
    fn the_repository_is_the_github_mark_in_the_version_row() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        s.press(&g, Hit::Bar(BarButton::Logo));
        let cells = cells_of(&s, &g);
        let version = cells
            .iter()
            .find(|c| c.text.starts_with("バージョン "))
            .unwrap();
        let at = cells
            .iter()
            .position(|c| c.action == Some(Action::About(Act::Repository)))
            .expect("the button");
        let repo = &cells[at];
        assert_eq!(repo.text, "");
        assert_eq!(repo.icon, Some("github"));
        assert_eq!(repo.look, Look::About(AboutLook::Bare));
        assert_eq!(repo.tip, "GitHub のリポジトリ\ngithub.com/tekalu1/xirius");
        assert_eq!((repo.rect.w, repo.rect.h), (28.0, 28.0));
        assert_eq!(
            repo.rect.y + repo.rect.h / 2.0,
            version.rect.y + version.rect.h / 2.0,
            "in the row"
        );
        assert!(
            version.rect.x + text_width(&version.text, FontSize::Small) <= repo.rect.x,
            "after the version"
        );
        let title = cells.iter().find(|c| c.text == "Xirius").unwrap();
        assert!(title.rect.bottom() <= repo.rect.y, "under the heading");
        // Its balloon has no room above in the panel, so it opens under the mark.
        let panel = s.popup.as_ref().unwrap().layout(g.size).rect;
        let balloon = crate::popup::tip_rect(repo.rect, panel, 160.0, 2);
        assert!(balloon.y >= repo.rect.bottom(), "{balloon:?}");
        assert!(balloon.right() <= panel.right());
        let label = cells
            .iter()
            .find(|c| c.text == "使っているソフトウェア")
            .unwrap();
        assert!(repo.rect.bottom() < label.rect.y, "above the list");
        assert!(
            !cells.iter().any(|c| c.text.contains("寄付")),
            "no donation yet"
        );

        assert!(s.opened.is_empty());
        assert!(s.press(&g, Hit::Item(at)));
        assert_eq!(s.opened, ["https://github.com/tekalu1/xirius"]);
        assert_eq!(s.error, None);
        assert!(s.popup.is_some(), "the panel stays open behind the browser");
        // LAME's source opens the same way, and so does the tarball on the releases page.
        for (act, url) in [
            (Act::Source, mp3::LAME_SOURCE_URL),
            (Act::SourceRelease, mp3::LAME_RELEASE_URL),
        ] {
            let at = cells_of(&s, &g)
                .iter()
                .position(|c| c.action == Some(Action::About(act)))
                .unwrap();
            s.press(&g, Hit::Item(at));
            assert_eq!(s.opened.last().map(String::as_str), Some(url));
        }
    }

    fn newer_version() -> &'static str {
        static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        VERSION.get_or_init(|| {
            let major = env!("CARGO_PKG_VERSION_MAJOR").parse::<u64>().unwrap() + 1;
            format!("{major}.0.0")
        })
    }

    fn newer_url() -> String {
        format!(
            "https://github.com/tekalu1/xirius/releases/tag/v{}",
            newer_version()
        )
    }

    pub(crate) fn newer_answer() -> String {
        serde_json::json!({
            "tag_name": format!("v{}", newer_version()),
            "html_url": newer_url(),
            "body": "- 録音の入力が、既定の入力でなく Loopback になる不具合を直しました。録音の画面で選んだ入力が、次に開いたときもそのまま使われます\r\n- 新しい曲に替えても、テーマなどの設定が既定に戻らない",
        }).to_string()
    }

    /// A fetch that answers `body` and counts the asks, never reaching the network.
    pub(crate) fn stub(
        body: Result<&str, &str>,
    ) -> (
        update::Fetch,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let a = asked.clone();
        let body = body.map(str::to_string).map_err(str::to_string);
        let fetch: update::Fetch = std::sync::Arc::new(move |_: &str| {
            a.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            body.clone()
        });
        (fetch, asked)
    }

    /// Asks as a launch does and waits for the answer, the frames reading it.
    pub(crate) fn ask(s: &mut Studio, dir: &std::path::Path, fetch: update::Fetch) {
        s.check_for_update(update::UpdateCheck::in_dir(dir, fetch), 1_000_000);
        let start = std::time::Instant::now();
        while s.update.asking() {
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            s.poll_update();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn scratch(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("xirius-ui-update-{tag}-"))
            .tempdir()
            .unwrap()
    }

    /// A signed manifest and the three small files of a release (UI-282).
    fn signed_release(
        version: &'static str,
        body: &'static [u8],
    ) -> (update::Release, update::Fetch, String) {
        use ed25519_dalek::Signer;
        let key = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]);
        let public = key
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let setup = update::setup_name(version);
        let manifest = format!(
            "{{\"version\":\"{version}\",\"file\":\"{setup}\",\"sha256\":\"{}\",\"channel\":\"stable\",\"min_version\":\"1.0.0\"}}\n",
            update::sha256_hex(body)
        );
        let manifest_bytes = manifest.clone().into_bytes();
        let signature = key
            .sign(&manifest_bytes)
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let sums = format!("{}  {setup}\n", update::sha256_hex(body));
        let fetch: update::Fetch = std::sync::Arc::new(move |url: &str| {
            Ok(match url {
                u if u.ends_with(update::manifest_name(version).as_str()) => manifest.clone(),
                u if u.ends_with(update::manifest_signature_name(version).as_str()) => {
                    signature.clone()
                }
                u if u.ends_with(update::SUMS) => sums.clone(),
                other => return Err(format!("no {other}")),
            })
        });
        let release = update::Release {
            version: version.into(),
            url: format!("https://github.com/tekalu1/xirius/releases/tag/v{version}"),
            notes: vec!["変わったこと".into()],
            prerelease: false,
            assets: vec![
                update::Asset {
                    name: setup.clone(),
                    size: body.len() as u64,
                },
                update::Asset {
                    name: update::manifest_name(version),
                    size: manifest_bytes.len() as u64,
                },
                update::Asset {
                    name: update::manifest_signature_name(version),
                    size: 129,
                },
                update::Asset {
                    name: update::SUMS.into(),
                    size: 200,
                },
            ],
        };
        (release, fetch, public)
    }

    /// A download that writes `body` to the file, within the size cap.
    fn download_of(body: &'static [u8]) -> update::Download {
        std::sync::Arc::new(move |_, path, max, progress| {
            assert!(body.len() as u64 <= max, "the size cap");
            std::fs::write(path, body).map_err(|e| e.to_string())?;
            progress(body.len() as u64, 0);
            Ok(())
        })
    }

    /// UI-262: a newer version puts a dot on the logo, and the version row its number,
    /// 「ダウンロード」 and what changed.
    #[test]
    fn a_newer_version_puts_a_dot_on_the_logo_and_its_number_and_changes_in_the_panel() {
        let tmp = scratch("newer");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let g = s.geometry(1600.0, 900.0);
        assert!(!s.bar_view().update);
        let (fetch, asked) = stub(Ok(&newer_answer()));
        ask(&mut s, tmp.path(), fetch);
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(s.bar_view().update);
        assert_eq!(s.error, None);

        // The dot sits at the logo's top right; the bar too narrow for the logo has none.
        let bar = s.bar_layout(1600.0);
        let logo = bar.rect(BarButton::Logo);
        let dot = bar.update_dot().expect("the dot");
        let (cx, cy) = (dot.x + dot.w / 2.0, dot.y + dot.h / 2.0);
        assert!(
            cx > logo.x + logo.w / 2.0 && cx < logo.right(),
            "{dot:?} {logo:?}"
        );
        assert!(
            cy > logo.y && cy < logo.y + logo.h / 2.0,
            "{dot:?} {logo:?}"
        );
        // The logo stands at every width now (UI-300), and the dot with it.
        assert!(s.bar_layout(960.0).update_dot().is_some());

        s.press(&g, Hit::Bar(BarButton::Logo));
        let cells = cells_of(&s, &g);
        let version = cells
            .iter()
            .find(|c| c.text.starts_with("バージョン "))
            .unwrap();
        let release = cells
            .iter()
            .find(|c| c.look == Look::About(AboutLook::Release))
            .expect("the newer number");
        assert_eq!(
            (release.text.as_str(), release.icon),
            (newer_version(), Some("download"))
        );
        let at = cells
            .iter()
            .position(|c| c.action == Some(Action::About(Act::Download)))
            .expect("the download");
        let download = &cells[at];
        assert_eq!(download.text, "ダウンロード");
        assert_eq!(download.look, Look::Primary);
        assert_eq!(download.tip, newer_url());
        // One row, left to right, inside the panel.
        assert_eq!(version.rect.y, release.rect.y);
        assert_eq!(release.rect.y, download.rect.y);
        assert!(version.rect.right() <= release.rect.x);
        assert!(release.rect.right() <= download.rect.x);
        assert_eq!(version.text, format!("バージョン {}", update::CURRENT));

        // What changed, under it: the number, then each change; a long one is broken into
        // lines that fit, its bullet on the first.
        let block = cells
            .iter()
            .find(|c| c.look == Look::About(AboutLook::Block) && c.rect.y > version.rect.y)
            .expect("the changes");
        assert!(block.rect.y >= download.rect.bottom());
        assert!(
            download.rect.right() <= block.rect.right(),
            "inside the panel"
        );
        let notes: Vec<&Cell> = cells
            .iter()
            .filter(|c| matches!(c.look, Look::About(AboutLook::Note { .. })))
            .collect();
        assert_eq!(
            notes[0].look,
            Look::About(AboutLook::Note {
                head: true,
                bullet: false
            })
        );
        assert_eq!(notes[0].text, newer_version());
        let bullets: Vec<&str> = notes
            .iter()
            .filter(|c| {
                c.look
                    == Look::About(AboutLook::Note {
                        head: false,
                        bullet: true,
                    })
            })
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(bullets.len(), 2, "{bullets:?}");
        assert!(bullets[0].starts_with("録音の入力が"), "{bullets:?}");
        assert!(bullets[1].starts_with("新しい曲に替えても"), "{bullets:?}");
        assert!(notes.len() > 3, "the long one takes two lines");
        for n in &notes[1..] {
            assert!(
                n.rect.x + BULLET_W + text_width(&n.text, FontSize::Small) <= block.rect.right(),
                "{}",
                n.text
            );
            assert!(n.rect.bottom() <= block.rect.bottom());
        }
        // The repository's mark ends the row, inside the panel.
        let repo = cells
            .iter()
            .find(|c| c.action == Some(Action::About(Act::Repository)))
            .unwrap();
        assert_eq!(repo.rect.y, download.rect.y);
        assert!(download.rect.right() < repo.rect.x);
        assert!(repo.rect.right() <= block.rect.right());

        assert!(s.press(&g, Hit::Item(at)));
        assert_eq!(s.opened, [newer_url()]);
        assert!(s.popup.is_some());
        assert_eq!(s.error, None);
    }

    /// UI-262: when GitHub cannot be reached, nothing is said and nothing changes.
    #[test]
    fn a_failed_asking_says_nothing() {
        let tmp = scratch("offline");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let g = s.geometry(1600.0, 900.0);
        let (fetch, asked) = stub(Err("Could not resolve host: api.github.com"));
        ask(&mut s, tmp.path(), fetch);
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(!s.bar_view().update);
        assert_eq!(s.error, None);
        assert_eq!(s.notice, None);
        s.press(&g, Hit::Bar(BarButton::Logo));
        let cells = cells_of(&s, &g);
        assert!(
            !cells
                .iter()
                .any(|c| c.text == "ダウンロード" || c.look == Look::About(AboutLook::Release))
        );
        assert!(
            cells
                .iter()
                .any(|c| c.text.ends_with(" · MIT OR Apache-2.0"))
        );
    }

    /// UI-282: 「ダウンロード」 takes the manifest, its signature, the sums and the installer
    /// off the window's thread, checks them all, and the row's button becomes
    /// 「更新して再起動」; pressing it arms the silent installer for the end of the window. The
    /// checked installer stays for the next start.
    #[test]
    fn a_checked_download_turns_the_button_into_the_install_and_arms_the_command() {
        let tmp = scratch("download");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let g = s.geometry(1600.0, 900.0);

        let body: &'static [u8] = b"the installer";
        let name = update::setup_name(newer_version());
        let (release, fetch, public) = signed_release(newer_version(), body);
        s.update.fetch = fetch;
        s.update.download = download_of(body);
        s.update.key = public;
        s.update.root = Some(tmp.path().to_path_buf());
        let app = tmp.path().join("local").join("Programs").join("Xirius");
        s.update.location = Some(update::Location {
            exe_dir: app.clone(),
            install_dir: app,
        });
        s.update.newer = Some(release);

        s.press(&g, Hit::Bar(BarButton::Logo));
        let at = cells_of(&s, &g)
            .iter()
            .position(|c| c.action == Some(Action::About(Act::Download)))
            .expect("the download");
        assert!(s.press(&g, Hit::Item(at)));
        let start = std::time::Instant::now();
        while s.update.downloading() {
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            s.poll_update();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(s.update.progress().phase, update::Phase::Ready);
        let cells = cells_of(&s, &g);
        assert_eq!(
            cells
                .iter()
                .find(|c| c.look == Look::Primary)
                .map(|c| c.text.as_str()),
            Some("更新して再起動")
        );
        // One button: there is no 「終了時に入れる」 (UI-282).
        assert!(
            !cells.iter().any(|c| c.text.contains("終了時に")),
            "one button only"
        );

        // 「更新して再起動」: /S and /XRUN, and the window is asked to close.
        let now = cells
            .iter()
            .position(|c| c.action == Some(Action::About(Act::UpdateNow)))
            .unwrap();
        assert!(s.press(&g, Hit::Item(now)));
        let pending = s.update.pending.clone().expect("the installer");
        assert_eq!(pending.args, ["/S", "/XRUN"]);
        assert_eq!(
            pending.program,
            tmp.path()
                .join(update::DOWNLOAD_DIR)
                .join(newer_version())
                .join(&name)
        );
        assert_eq!(
            s.window_ask,
            Some(crate::context::key_more::WindowAsk::Quit)
        );

        // The checked installer is kept: the next start (and a reopened panel) offers it again.
        s.popup = None;
        s.update.pending = None;
        s.window_ask = None;
        s.update.newer = None;
        s.press(&g, Hit::Bar(BarButton::Logo));
        s.poll_update();
        let cells = cells_of(&s, &g);
        assert_eq!(
            cells
                .iter()
                .find(|c| c.look == Look::Primary)
                .map(|c| c.text.as_str()),
            Some("更新して再起動")
        );
    }

    /// UI-282: a manifest whose signature does not match is thrown away, its reason is shown,
    /// and 「Release のページを開く」 is there to go on by hand.
    #[test]
    fn a_bad_signature_is_said_and_the_page_is_offered() {
        let tmp = scratch("badsig");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let g = s.geometry(1600.0, 900.0);
        let body: &'static [u8] = b"the installer";
        let (release, good_fetch, public) = signed_release(newer_version(), body);
        let fetch: update::Fetch = std::sync::Arc::new(move |url: &str| {
            if url.ends_with(".sig") {
                return Ok("00".repeat(64));
            }
            good_fetch(url)
        });
        s.update.fetch = fetch;
        s.update.download = download_of(body);
        s.update.key = public;
        s.update.root = Some(tmp.path().to_path_buf());
        let app = tmp.path().join("app");
        s.update.location = Some(update::Location {
            exe_dir: app.clone(),
            install_dir: app,
        });
        s.update.newer = Some(release);

        s.press(&g, Hit::Bar(BarButton::Logo));
        let at = cells_of(&s, &g)
            .iter()
            .position(|c| c.action == Some(Action::About(Act::Download)))
            .unwrap();
        assert!(s.press(&g, Hit::Item(at)));
        let start = std::time::Instant::now();
        while s.update.downloading() {
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            s.poll_update();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            s.update.progress().phase,
            update::Phase::Failed(update::BAD_SIGNATURE.into())
        );
        // Nothing is kept.
        let dir = tmp.path().join(update::DOWNLOAD_DIR).join(newer_version());
        assert!(
            !dir.join(update::setup_name(newer_version())).exists(),
            "no installer is kept"
        );
        let cells = cells_of(&s, &g);
        assert!(
            cells
                .iter()
                .any(|c| c.look == Look::About(AboutLook::Alert)
                    && c.text == update::BAD_SIGNATURE)
        );
        let page = cells
            .iter()
            .position(|c| c.action == Some(Action::About(Act::Page)))
            .expect("the page");
        assert!(s.press(&g, Hit::Item(page)));
        assert_eq!(
            s.opened.last().map(String::as_str),
            Some(newer_url().as_str())
        );
    }

    /// UI-282: a note (another Xirius program running, or the installer's note from the last
    /// start) is said in the panel, with the release's page; it never adds a second button.
    #[test]
    fn a_note_is_shown_with_the_page_and_no_second_button() {
        let mut v = View::now();
        v.notice = Some("xirius.exe が動いています".into());
        let shown = cells(&v, 0);
        assert!(
            shown
                .iter()
                .any(|c| c.look == Look::About(AboutLook::Alert) && c.text.contains("xirius.exe"))
        );
        assert!(acts(&shown).contains(&Act::Page));

        v.newer = Some(update::Release {
            version: newer_version().into(),
            url: newer_url(),
            notes: Vec::new(),
            prerelease: false,
            assets: Vec::new(),
        });
        v.progress = update::Progress {
            phase: update::Phase::Ready,
            got: 1,
            total: 1,
        };
        let shown = cells(&v, 0);
        assert!(
            shown
                .iter()
                .any(|c| c.look == Look::About(AboutLook::Alert)),
            "the note is there"
        );
        assert!(acts(&shown).contains(&Act::UpdateNow));
        assert!(
            !shown.iter().any(|c| c.text.contains("終了時に")),
            "one button only"
        );
    }

    /// UI-282: the installer is checked once more at the last moment (the song's save may take
    /// a while); a file replaced in between is not started, and the next start says so.
    #[test]
    fn the_installer_is_checked_again_when_the_window_closes() {
        use ed25519_dalek::Signer;

        let tmp = scratch("recheck");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let body: &'static [u8] = b"the installer";
        let version = newer_version();
        let name = update::setup_name(version);
        let manifest = format!(
            "{{\"version\":\"{version}\",\"file\":\"{name}\",\"sha256\":\"{}\",\"channel\":\"stable\",\"min_version\":\"1.0.0\"}}\n",
            update::sha256_hex(body)
        );
        let manifest_bytes = manifest.clone().into_bytes();
        let key = ed25519_dalek::SigningKey::from_bytes(&[5u8; 32]);
        let public = key
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let signature = key
            .sign(&manifest_bytes)
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let sums = format!("{}  {name}\n", update::sha256_hex(body));
        let path = tmp
            .path()
            .join(update::DOWNLOAD_DIR)
            .join(version)
            .join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        s.update.key = public;
        s.update.root = Some(tmp.path().to_path_buf());
        s.update.ready = Some(update::Installer {
            version: version.into(),
            path: path.clone(),
            manifest: serde_json::from_slice(&manifest_bytes).unwrap(),
            manifest_bytes,
            signature,
            sums,
        });
        s.update.pending = Some(update::Install {
            program: path.clone(),
            args: vec!["/S".into(), "/XRUN".into()],
        });

        // Intact: it starts.
        assert!(s.take_pending_install().is_some());
        // Replaced after it was checked: it is not started, and the note is left.
        s.update.pending = Some(update::Install {
            program: path.clone(),
            args: vec!["/S".into(), "/XRUN".into()],
        });
        std::fs::write(&path, b"another installer").unwrap();
        assert!(s.take_pending_install().is_none());
        assert!(tmp.path().join(update::FAILED).is_file());
    }

    /// UI-282: with the placeholder key or outside the per-user folder, the button only opens
    /// the release's page, as it did before.
    #[test]
    fn without_a_key_or_outside_the_installer_folder_nothing_is_downloaded() {
        let tmp = scratch("fallback");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let g = s.geometry(1600.0, 900.0);
        s.update.newer = Some(update::Release {
            version: newer_version().into(),
            url: newer_url(),
            notes: Vec::new(),
            prerelease: false,
            assets: Vec::new(),
        });
        // The placeholder key (the build itself has the real one).
        s.update.key = update::PLACEHOLDER_KEY.to_string();
        s.update.location = Some(update::Location {
            exe_dir: tmp.path().join("app"),
            install_dir: tmp.path().join("app"),
        });
        s.press(&g, Hit::Bar(BarButton::Logo));
        let at = cells_of(&s, &g)
            .iter()
            .position(|c| c.action == Some(Action::About(Act::Download)))
            .unwrap();
        assert!(s.press(&g, Hit::Item(at)));
        assert!(!s.update.downloading(), "nothing is downloaded");
        assert_eq!(
            s.opened.last().map(String::as_str),
            Some(newer_url().as_str())
        );
        // A key, but a folder the installer does not put Xirius in.
        s.update.key = "ab".repeat(32);
        s.update.location = Some(update::Location {
            exe_dir: tmp.path().join("Downloads").join("Xirius-1.0.0-win-x64"),
            install_dir: tmp.path().join("app"),
        });
        assert!(!s.can_download_update());
        assert!(s.press(&g, Hit::Item(at)));
        assert!(!s.update.downloading());
        assert!(s.update.ready.is_none());
    }
}
