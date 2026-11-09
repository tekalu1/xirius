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
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (name, text) = item.text();
    let path = dir.join(name);
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    std::fs::write(&path, text).map_err(|e| format!("{} に書けません: {e}", path.display()))?;
    Ok(path)
}

/// The shared manual check, independent of whether asking at launch is enabled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CheckState {
    #[default]
    Idle,
    Checking,
    Latest,
    NoRelease,
    Failed(String),
}

/// How long whatever is left of Xirius may take to end, once the window has closed for an
/// update and the cleanup of the close has finished (UI-282). The installer waits 120 s for it
/// (`xirius.nsi`; 30 s in the ones made up to 1.1.0-beta.3).
pub(crate) const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// How long the window waits for the installer to be checked before it closes for an update
/// (UI-282); after that the update is called off and the window stays.
pub(crate) const PREPARE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Said while the installer is checked, before the window closes for an update (UI-282).
pub(crate) const PREPARING: &str = "更新の準備をしています…";

/// Checks the installer once more and starts it (UI-282), for a window that closed without
/// the last step of [`crate::studio::Studio::commit_close`] (the OS closed it without asking):
/// a file replaced since it was checked is not run. When it cannot be started, the note the
/// next start reads is left ([`update::FAILED`]). Runs on the application-files worker.
pub(crate) fn launch_installer(
    install: &update::Install,
    installer: Option<&update::Installer>,
    key: &str,
    root: Option<&std::path::Path>,
    spawn: &update::Spawn,
) -> Result<(), String> {
    let started = installer
        .ok_or_else(|| "インストーラーがありません".to_string())
        // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only (finish_window_io dispatches it).
        .and_then(|i| i.hold_with(key))
        .and_then(|held| {
            // Held until it is started, as from the window (UI-282).
            let started = if held.is(&install.program) {
                spawn(install)
            } else {
                Err("確かめたインストーラーと違うため、起動しませんでした".into())
            };
            drop(held);
            started
        });
    match started {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Some(root) = root {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only, as the check above.
                let _ = std::fs::write(root.join(update::FAILED), b"");
            }
            Err(e)
        }
    }
}

/// The asking for a newer version at launch (UI-262), and getting it into place (UI-282).
pub struct Update {
    encoder: Option<crate::ancillary_io::Ticket<View>>,
    encoder_view: Option<View>,
    startup: Option<crate::ancillary_io::Ticket<StartupUpdate>>,
    arm: Option<(
        update::Installer,
        crate::ancillary_io::Ticket<update::Install>,
    )>,
    crashes: Option<crate::ancillary_io::Ticket<crate::crash_reports::View>>,
    asking: Option<std::thread::JoinHandle<Result<Option<update::Release>, String>>>,
    pub check: CheckState,
    channel: Option<bool>,
    /// The launch's asking (UI-282). The window reads the settings after it starts, so the
    /// launch may have asked for the other channel; it is asked again for the one now set.
    launch: Option<update::UpdateCheck>,
    /// The launch's asking is to be made again once nothing else is asking or downloading.
    recheck: bool,
    manual: bool,
    manual_pending: bool,
    download_channel: Option<bool>,
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
    /// The installer to start once the close is decided.
    pub pending: Option<update::Install>,
    /// The installer being checked before the window closes, and since when (UI-282).
    pub(crate) holding: Option<(
        crate::ancillary_io::Ticket<update::Held>,
        std::time::Instant,
    )>,
    /// The installer, checked and held open until it is started (UI-282).
    pub(crate) held: Option<update::Held>,
    /// The installer was started: the window goes, and what is left of Xirius ends within
    /// [`EXIT_WAIT`] of the end of the close's cleanup. Shared with the cleanup's worker, which
    /// starts an installer the window could not.
    pub launched: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Why an update could not be put in (the other programs running, a failed check, the
    /// note the installer left). Shown in the panel (UI-282).
    pub notice: Option<String>,
    /// How the installer is started; tests put their own in.
    pub spawn: update::Spawn,
    /// What makes Xirius end once the installer is started; tests put their own in.
    pub leave: std::sync::Arc<dyn Fn() + Send + Sync>,
    /// How GitHub is read and files are downloaded; tests put their own in.
    pub fetch: update::Fetch,
    pub download: update::Download,
    /// The public key this build checks with; tests put a key of their own.
    pub key: String,
}

impl Default for Update {
    fn default() -> Update {
        Update {
            encoder: None,
            encoder_view: None,
            startup: None,
            arm: None,
            crashes: None,
            asking: None,
            check: CheckState::Idle,
            channel: None,
            launch: None,
            recheck: false,
            manual: false,
            manual_pending: false,
            download_channel: None,
            newer: None,
            root: None,
            job: None,
            progress: std::sync::Arc::new(std::sync::Mutex::new(update::Progress::default())),
            ready: None,
            location: startup_cache()
                .and_then(|s| s.location.clone())
                .or_else(|| {
                    if startup_cache().is_some() {
                        None
                    } else {
                        update::Location::per_user()
                    }
                }),
            pending: None,
            holding: None,
            held: None,
            launched: Default::default(),
            notice: None,
            spawn: std::sync::Arc::new(update::spawn_detached),
            leave: if cfg!(test) {
                std::sync::Arc::new(|| {})
            } else {
                std::sync::Arc::new(|| update::leave_within(EXIT_WAIT))
            },
            fetch: update::curl(),
            download: update::curl_download(),
            key: update::UPDATE_KEY.to_string(),
        }
    }
}

/// What the end of Xirius still needs once its window has closed (UI-282).
pub(crate) struct AfterClose {
    /// When the close of the song began; the wait for its cleanup counts from there.
    pub recent_since: std::time::Instant,
    launched: std::sync::Arc<std::sync::atomic::AtomicBool>,
    leave: std::sync::Arc<dyn Fn() + Send + Sync>,
}

impl AfterClose {
    /// Right before Xirius quits, once the close's cleanup is done: when an installer was
    /// started and waits for Xirius to end, what is left of it (a plugin's thread, the end of
    /// the audio) may not keep it waiting; it ends within [`EXIT_WAIT`].
    pub(crate) fn quitting(&self) {
        if self.launched.load(std::sync::atomic::Ordering::SeqCst) {
            (self.leave)();
        }
    }
}

impl Update {
    pub fn asking(&self) -> bool {
        self.asking.is_some() || self.startup.is_some()
    }

    /// Is the installer being downloaded or checked?
    pub fn downloading(&self) -> bool {
        self.job.is_some()
    }

    /// Is the installer being checked, before the window closes for an update?
    pub fn preparing(&self) -> bool {
        self.holding.is_some()
    }

    /// Was the installer started? Then the window is going.
    pub fn launched(&self) -> bool {
        self.launched.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// What the download is doing now.
    pub fn progress(&self) -> update::Progress {
        self.progress
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(crate) fn set_progress(&self, progress: update::Progress) {
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = progress;
    }
}

/// A press in the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Check,
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
    pub spin: f32,
    pub check: CheckState,
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
        if let Some(cached) = startup_cache() {
            return cached.view.clone();
        }
        Self::read_now()
    }

    fn read_now() -> View {
        let (lame_path, lame_error) = match mp3::load(None) {
            Ok(l) => (l.path.clone(), None),
            Err(_) => (mp3::locate(), Some(mp3::CANNOT_LOAD.to_string())),
        };
        View {
            spin: 0.0,
            check: CheckState::Idle,
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

struct Startup {
    reduced: bool,
    view: View,
    location: Option<update::Location>,
    installable: bool,
}
static STARTUP: std::sync::OnceLock<Startup> = std::sync::OnceLock::new();
fn startup_cache() -> Option<&'static Startup> {
    STARTUP.get()
}

pub(crate) fn initial_reduced() -> bool {
    startup_cache().is_some_and(|s| s.reduced)
}

/// Probe the encoder and installation before GPUI starts accepting callbacks.
pub(crate) fn prepare_startup() {
    STARTUP.get_or_init(|| {
        // This join precedes creation of the GPUI executor and every window.
        std::thread::spawn(|| {
            let location = update::Location::per_user();
            let installable = location.as_ref().is_some_and(|l| l.installable());
            Startup {
                reduced: crate::motion_watch::os_reduced(),
                view: View::read_now(),
                location,
                installable,
            }
        })
        // design-lint: allow(ui-blocking-io) PRJ-031: bootstrap join before GPUI/executor/window creation; all probes run on that worker.
        .join()
        .expect("startup probe")
    });
}

struct StartupUpdate {
    note: Option<String>,
    installer: Option<update::Installer>,
    newer: Option<update::Release>,
    beta: bool,
    total: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AboutLook {
    Spinner(f32),
    Busy(f32),
    Disabled,
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
    if let Some(crashes) = &v.crashes {
        cells.extend(crate::crash_reports::cells(crashes));
        return cells;
    }
    let section = update_cells(v, W - PAD * 2.0);
    let end = section
        .iter()
        .map(|c| c.rect.bottom())
        .fold(0.0_f32, f32::max);
    for mut cell in section {
        cell.rect.x += PAD;
        cell.rect.y += 46.0;
        cells.push(cell);
    }
    cells.push(
        about(
            Rect::new(PAD, 46.0 + end + Space::X2.px(), W - PAD * 2.0, NOTE_H),
            AboutLook::Sub,
        )
        .text("MIT OR Apache-2.0"),
    );
    rest(&mut cells, v, 46.0 + end + NOTE_H + INSET * 2.0, offset);
    cells
}

/// The same state and actions in settings and About. The palette opens About.
pub(crate) fn update_cells(v: &View, width: f32) -> Vec<Cell> {
    let mut cells = Vec::new();
    let version = format!("バージョン {}", v.version);
    let vw = text_width(&version, FontSize::Small);
    cells.push(about(Rect::new(0.0, 0.0, vw, BUTTON_H), AboutLook::Sub).text(version));
    github(&mut cells, width - 28.0, 0.0);
    let mut y = BUTTON_H + Space::X2.px();
    if let Some(newer) = &v.newer {
        let title = format!(
            "{}{}",
            newer.version,
            if newer.prerelease {
                " · ベータ版"
            } else {
                ""
            }
        );
        let rw = release_w(&title);
        let (x, ry) = if vw + Space::X2.px() + rw + 36.0 <= width {
            (vw + Space::X2.px(), 0.0)
        } else {
            let ry = y;
            y += BUTTON_H + Space::X2.px();
            (0.0, ry)
        };
        cells.push(
            about(Rect::new(x, ry, rw, BUTTON_H), AboutLook::Release)
                .text(title)
                .icon("download"),
        );
    }
    let checking = v.check == CheckState::Checking;
    let status = if checking {
        Some("更新を確認しています")
    } else if v.check == CheckState::NoRelease {
        Some(update::NO_RELEASE)
    } else if v.newer.is_some() {
        Some("新しいバージョンがあります")
    } else if v.check == CheckState::Latest {
        Some("最新のバージョンです")
    } else {
        None
    };
    let check_label = if checking {
        "確認中"
    } else if matches!(v.check, CheckState::Failed(_)) {
        "再試行"
    } else {
        "更新を確認"
    };
    let check_width =
        Space::X2.px() * 3.0 + Layout::ICON_SIZE + text_width(check_label, FontSize::Small);
    let status_top = y;
    if let Some(status) = status {
        // A narrow section stacks the next action under the status.
        let room = if width >= 400.0 && v.newer.is_some() {
            width - 184.0
        } else if width >= 400.0 {
            width - check_width - Space::X2.px()
        } else {
            width
        };
        let sx = if checking {
            Layout::ICON_SIZE + Space::X2.px()
        } else {
            0.0
        };
        if checking {
            cells.push(about(
                Rect::new(0.0, y, Layout::ICON_SIZE, NOTE_H),
                AboutLook::Spinner(v.spin),
            ));
        }
        let lines = crate::opline::wrap(status, room - sx, &|t| text_width(t, FontSize::Small));
        for line in lines {
            cells.push(about(Rect::new(sx, y, room - sx, NOTE_H), AboutLook::Sub).text(line));
            y += NOTE_H;
        }
        y = y.max(status_top + BUTTON_H);
    }
    if let Some(newer) = &v.newer {
        let (label, icon, act) = match &v.progress.phase {
            update::Phase::Ready => (
                "更新して再起動".to_string(),
                "restore",
                Some(Act::UpdateNow),
            ),
            update::Phase::Downloading => (download_label(&v.progress), "download", None),
            update::Phase::Verifying => ("確かめています".to_string(), "sync", None),
            _ => ("ダウンロード".to_string(), "download", Some(Act::Download)),
        };
        let bw = Space::X2.px() * 3.0 + Layout::ICON_SIZE + text_width(&label, FontSize::Small);
        let (bx, by) = if width >= 400.0 {
            (width - bw, status_top)
        } else {
            (0.0, y + Space::X2.px())
        };
        let mut primary = Cell::new(Rect::new(bx, by, bw.min(width), BUTTON_H), Look::Primary)
            .text(label)
            .icon(icon);
        if matches!(
            v.progress.phase,
            update::Phase::Downloading | update::Phase::Verifying
        ) {
            primary.look = Look::About(AboutLook::Busy(v.spin));
        }
        primary.action = act.filter(|_| !checking).map(Action::About);
        if checking {
            primary.look = Look::About(AboutLook::Disabled);
        }
        primary.tip = newer.url.clone();
        cells.push(primary);
        y = y.max(by + BUTTON_H) + Space::X2.px();
    }
    let (check_x, check_y) = if v.newer.is_none() && width >= 400.0 {
        (width - check_width, status_top)
    } else {
        (0.0, y)
    };
    button(
        &mut cells,
        check_x,
        check_y,
        check_label,
        "sync",
        Act::Check,
    );
    if checking
        || matches!(
            v.progress.phase,
            update::Phase::Downloading | update::Phase::Verifying
        )
    {
        cells.last_mut().unwrap().action = None;
    }
    y = y.max(check_y + BUTTON_H) + Space::X2.px();
    if let CheckState::Failed(reason) = &v.check {
        for line in crate::opline::wrap(reason, width, &|t| text_width(t, FontSize::Small)) {
            cells.push(about(Rect::new(0.0, y, width, NOTE_H), AboutLook::Alert).text(line));
            y += NOTE_H;
        }
        if v.newer.is_some() {
            button(
                &mut cells,
                0.0,
                y,
                "Release のページを開く",
                "open",
                Act::Page,
            );
            cells.last_mut().unwrap().tip = page_url(v);
            y += BUTTON_H + Space::X2.px();
        }
    }
    if let Some(newer) = &v.newer {
        let block = cells.len();
        cells.push(about(Rect::new(0.0, y, width, 0.0), AboutLook::Block));
        let top = y;
        y += INSET;
        cells.push(
            about(
                Rect::new(INSET, y, width - INSET * 2.0, NOTE_H),
                AboutLook::Note {
                    head: true,
                    bullet: false,
                },
            )
            .text(&newer.version),
        );
        y += NOTE_H;
        for note in &newer.notes {
            for (i, line) in crate::opline::wrap(note, width - INSET * 2.0 - BULLET_W, &|t| {
                text_width(t, FontSize::Small)
            })
            .into_iter()
            .enumerate()
            {
                cells.push(
                    about(
                        Rect::new(INSET, y, width - INSET * 2.0, NOTE_H),
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
        if let update::Phase::Failed(reason) = &v.progress.phase {
            y = notice_lines(&mut cells, v, INSET, y, width - INSET * 2.0, reason, true);
        } else if let Some(reason) = &v.notice {
            y = notice_lines(&mut cells, v, INSET, y, width - INSET * 2.0, reason, true);
        }
        y += INSET;
        cells[block].rect.h = y - top;
    } else if let Some(reason) = &v.notice {
        notice_lines(&mut cells, v, 0.0, y, width, reason, true);
    }
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
    room: f32,
    text: &str,
    page: bool,
) -> f32 {
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
        AboutLook::Spinner(turn) => pen.icon_turned("sync", icon_at(r.x), c.ink, *turn),
        AboutLook::Busy(_) | AboutLook::Disabled => {
            let (face, ink) = if matches!(look, AboutLook::Disabled) {
                (c.surface_2, c.ink)
            } else {
                c.primary
            };
            pen.round(r, face, Radius::S);
            if let AboutLook::Busy(turn) = look {
                pen.icon_turned("sync", icon_at(r.x + Space::X2.px()), ink, *turn);
            } else {
                pen.icon(
                    cell.icon.unwrap_or("download"),
                    icon_at(r.x + Space::X2.px()),
                    ink,
                );
            }
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Medium,
                ink,
                Rect::new(
                    r.x + Space::X2.px() * 2.0 + Layout::ICON_SIZE,
                    r.y,
                    r.w - Space::X2.px() * 3.0 - Layout::ICON_SIZE,
                    r.h,
                ),
                Align::Left,
            );
        }

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

fn installer_matches(release: &update::Release, installer: &update::Installer) -> bool {
    release.version == installer.version
        && installer.manifest.channel
            == if release.prerelease {
                update::BETA
            } else {
                update::STABLE
            }
}

impl Studio {
    /// Opens the panel under the logo, or closes it when it is open.
    pub(crate) fn open_about(&mut self, anchor: Rect) {
        self.update.crashes = None;
        self.open_popup(PopupKind::About, anchor);
        let newer = self.newer_release().cloned();
        let progress = self.update.progress();
        let notice = self.update.notice.clone();
        let spin = self.spin();
        let view = self.update.encoder_view.clone().unwrap_or_else(View::now);
        if self.transport.is_windowed() {
            match crate::ancillary_io::submit(|| {
                #[cfg(any(test, feature = "loading-probe"))]
                xirius_core::io_test::before("encoder_probe", std::path::Path::new(mp3::LAME_FILE))
                    .map_err(|e| e.to_string())?;
                Ok(View::read_now())
            }) {
                Ok(job) => self.update.encoder = Some(job),
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(p) = &mut self.popup
            && p.kind == PopupKind::About
        {
            p.about = Some(Box::new(View {
                spin,
                check: self.update.check.clone(),
                newer,
                progress,
                notice,
                ..view
            }));
        }
    }

    /// Asks GitHub whether a newer version is out, off the window's thread, unless the setting
    /// is off (UI-262). [`Studio::poll_update`] takes the answer. A checked installer kept from
    /// the last run is taken up now, so it can be put in without asking again (UI-282).
    pub(crate) fn check_for_update(&mut self, check: update::UpdateCheck, now_ms: u64) {
        if self.update.asking() || self.update.downloading() {
            return;
        }
        let root = check.root().to_path_buf();
        self.update.root = Some(root.clone());
        self.update.launch = Some(check.clone());
        self.update.recheck = false;
        let beta = self.update_beta();
        if self.transport.is_windowed() {
            let enabled = self.update_on();
            let key = self.update.key.clone();
            self.update.channel = Some(beta);
            match crate::ancillary_io::submit(move || {
                #[cfg(any(test, feature = "loading-probe"))]
                xirius_core::io_test::before("update_startup", &root).map_err(|e| e.to_string())?;
                let note = update::take_failure(&root);
                let installer = update::ready_installer(&root, update::CURRENT, beta, &key);
                let total = installer
                    .as_ref()
                    .and_then(|i| i.path.metadata().ok())
                    .map_or(0, |m| m.len());
                let newer = if enabled {
                    check.at_launch(true, beta, now_ms, update::CURRENT)
                } else {
                    check
                        .stored()
                        .filter(|c| c.beta == beta)
                        .and_then(|c| c.newer_than(update::CURRENT).cloned())
                };
                Ok(StartupUpdate {
                    note,
                    installer,
                    newer,
                    beta,
                    total,
                })
            }) {
                Ok(job) => self.update.startup = Some(job),
                Err(e) => self.update.notice = Some(e),
            }
            return;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: synchronous offline adapter; window mode returns through the startup/installer owner before this call.
        if let Some(note) = update::take_failure(&root) {
            self.update.notice = Some(note);
        }
        if let Some(installer) =
            // design-lint: allow(ui-blocking-io) PRJ-031 review: synchronous offline adapter; window mode returns through the startup/installer owner before this call.
            update::ready_installer(&root, update::CURRENT, beta, &self.update.key)
        {
            // design-lint: allow(ui-blocking-io) PRJ-031: synchronous offline adapter; window mode returns through the startup ticket above.
            let total = installer.path.metadata().map(|m| m.len()).unwrap_or(0);
            self.update.ready = Some(installer);
            self.update.set_progress(update::Progress {
                phase: update::Phase::Ready,
                got: total,
                total,
            });
        }
        self.update.channel = Some(beta);
        if !self.update_on() {
            self.update.newer = check
                // design-lint: allow(ui-blocking-io) PRJ-031 review: synchronous offline adapter; window mode returns through the startup/installer owner before this call.
                .stored()
                .filter(|c| c.beta == beta)
                .and_then(|c| c.newer_than(update::CURRENT).cloned());
            self.poll_update();
            return;
        }
        self.update.manual = false;
        self.update.asking = Some(std::thread::spawn(move || {
            Ok(check.at_launch(true, beta, now_ms, update::CURRENT))
        }));
    }

    /// Every GUI entry uses this worker; a manual check bypasses the daily cache.
    pub(crate) fn manual_update_check(&mut self) {
        if self.update.asking() {
            if !self.update.manual {
                self.update.manual_pending = true;
                self.update.check = CheckState::Checking;
            }
            return;
        }
        if self.update.downloading() {
            return;
        }
        let beta = self.update_beta();
        let root = self
            .update
            .root
            .clone()
            .unwrap_or_else(xirius_session::recent::root);
        self.update.root = Some(root.clone());
        let check = update::UpdateCheck::in_dir(&root, self.update.fetch.clone());
        self.update.channel = Some(beta);
        self.update.manual = true;
        self.update.check = CheckState::Checking;
        self.update.asking = Some(std::thread::spawn(move || {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            check
                .ask_ui(now, beta)
                .map(|c| c.newer_than(update::CURRENT).cloned())
                .map_err(|e| {
                    if e == update::NO_RELEASE {
                        e
                    } else {
                        format!("更新を確認できません: {e}")
                    }
                })
        }));
        self.poll_update();
    }

    pub(crate) fn update_view(&self) -> View {
        View {
            spin: self.spin(),
            newer: self.newer_release().cloned(),
            check: self.update.check.clone(),
            progress: self.update.progress(),
            notice: self.update.notice.clone(),
            crashes: None,
            version: update::CURRENT.into(),
            lame_path: View::now().lame_path,
            lame_error: None,
            open: None,
            query: String::new(),
        }
    }

    fn update_on(&self) -> bool {
        let shared = self.song.settings().clone();
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let s = shared.lock().unwrap_or_else(|e| e.into_inner());
        s.prefs.bool(UPDATE_PREF)
    }

    /// Does this window take the beta channel (UI-282)?
    fn update_beta(&self) -> bool {
        let shared = self.song.settings().clone();
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let s = shared.lock().unwrap_or_else(|e| e.into_inner());
        s.prefs.bool(update::BETA_PREF)
    }

    /// Once a frame. True while the asking or the download runs and on the frame it ends.
    fn adopt_update_release(&mut self, newer: Option<update::Release>) {
        if self
            .update
            .ready
            .as_ref()
            .is_some_and(|i| newer.as_ref().is_none_or(|r| !installer_matches(r, i)))
        {
            self.update.ready = None;
            self.update.set_progress(update::Progress::default());
        }
        self.update.newer = newer;
    }

    /// Once a frame. True while the asking or the download runs and on the frame it ends.
    pub(crate) fn poll_update(&mut self) -> bool {
        let mut changed = false;
        if let Some(result) = self
            .update
            .encoder
            .as_ref()
            .and_then(crate::ancillary_io::Ticket::poll)
        {
            self.update.encoder = None;
            match result {
                Ok(view) => {
                    if let Some(shown) = self.popup.as_mut().and_then(|p| p.about.as_mut()) {
                        shown.lame_path = view.lame_path.clone();
                        shown.lame_error = view.lame_error.clone();
                    }
                    self.update.encoder_view = Some(view);
                }
                Err(e) => self.error = Some(e),
            }
            changed = true;
        }
        let beta = self.update_beta();
        if let Some(result) = self
            .update
            .startup
            .as_ref()
            .and_then(crate::ancillary_io::Ticket::poll)
        {
            self.update.startup = None;
            match result {
                Ok(got) if got.beta == beta => {
                    if got.note.is_some() {
                        self.update.notice = got.note;
                    }
                    self.update.ready = got.installer;
                    self.adopt_update_release(got.newer);
                    if self.update.ready.is_some() {
                        self.update.set_progress(update::Progress {
                            phase: update::Phase::Ready,
                            got: got.total,
                            total: got.total,
                        });
                    }
                }
                // Asked for the channel set before the settings were read: the installer's
                // note is kept (it is read once), the rest is asked again for this channel.
                Ok(got) => {
                    if got.note.is_some() {
                        self.update.notice = got.note;
                    }
                    self.update.recheck = true;
                }
                Err(e) => self.update.notice = Some(e),
            }
            changed = true;
        }
        if let Some((installer, result)) = self
            .update
            .arm
            .as_ref()
            .and_then(|(i, job)| job.poll().map(|r| (i.clone(), r)))
        {
            self.update.arm = None;
            if self.update.ready.as_ref() == Some(&installer)
                && (installer.manifest.channel != update::BETA || beta)
            {
                match result {
                    Ok(install) => {
                        self.update.pending = Some(install);
                        self.window_ask = Some(crate::context::key_more::WindowAsk::Quit);
                    }
                    Err(e) => self.update.notice = Some(e),
                }
            }
            changed = true;
        }
        if let Some((result, since)) = self
            .update
            .holding
            .as_ref()
            .map(|(job, since)| (job.poll(), *since))
        {
            match result {
                Some(Ok(held)) => {
                    self.update.holding = None;
                    self.update.held = Some(held);
                    // The window is asked to close again; now it may.
                    self.window_ask = Some(crate::context::key_more::WindowAsk::Quit);
                    changed = true;
                }
                Some(Err(e)) => {
                    self.drop_update_install(format!("更新を取りやめました: {e}"));
                    changed = true;
                }
                None if since.elapsed() >= PREPARE_WAIT => {
                    self.drop_update_install(
                        "更新の準備に時間がかかりすぎたため、更新を取りやめました".into(),
                    );
                    changed = true;
                }
                None => {}
            }
        }
        if let Some(result) = self
            .update
            .crashes
            .as_ref()
            .and_then(crate::ancillary_io::Ticket::poll)
        {
            self.update.crashes = None;
            if let Some(v) = self.popup.as_mut().and_then(|p| p.about.as_mut())
                && v.crashes.is_none()
            {
                match result {
                    Ok(view) => v.crashes = Some(Box::new(view)),
                    Err(e) => self.error = Some(e),
                }
                changed = true;
            }
        }
        if self.update.channel.is_some_and(|c| c != beta) {
            self.update.newer = None;
            self.update.ready = None;
            if !self.update.downloading() {
                self.update.set_progress(update::Progress::default());
            }
            self.update.check = CheckState::Idle;
            if self.update.asking.is_none() {
                self.update.channel = Some(beta);
            }
            self.update.recheck = true;
            changed = true;
        }
        if self.update.asking.as_ref().is_some_and(|a| a.is_finished())
            && let Some(asking) = self.update.asking.take()
        {
            if self.update.channel == Some(self.update_beta()) {
                match asking
                    // design-lint: allow(ui-blocking-io) PRJ-031: is_finished was observed before taking this handle; no running worker is joined.
                    .join()
                    .unwrap_or_else(|_| Err("更新を確認できません: 確認処理が終了しました".into()))
                {
                    Ok(newer) => {
                        self.adopt_update_release(newer);
                        if self.update.manual {
                            self.update.check = CheckState::Latest;
                        }
                    }
                    Err(reason) => {
                        if self.update.manual {
                            self.update.check = if reason == update::NO_RELEASE {
                                CheckState::NoRelease
                            } else {
                                CheckState::Failed(reason)
                            };
                        }
                    }
                }
            }
            changed = true;
        }
        if self.update.job.as_ref().is_some_and(|j| j.is_finished())
            && let Some(job) = self.update.job.take()
        {
            let before = self.update.progress();
            let obsolete = self.update.download_channel.is_some_and(|b| b != beta);
            // design-lint: allow(ui-blocking-io) PRJ-031: is_finished was observed before taking this handle; no running worker is joined.
            let phase = match job.join() {
                Ok(Ok(_)) if obsolete => update::Phase::Idle,
                Ok(Err(_)) if obsolete => update::Phase::Idle,
                Ok(Ok(installer))
                    if self
                        .update
                        .newer
                        .as_ref()
                        .is_none_or(|r| !installer_matches(r, &installer)) =>
                {
                    update::Phase::Idle
                }
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
        if self.update.manual_pending && !self.update.asking() && !self.update.downloading() {
            self.update.manual_pending = false;
            self.manual_update_check();
        }
        if self.update.recheck
            && self.update.startup.is_none()
            && !self.update.asking()
            && !self.update.downloading()
            && self.update.arm.is_none()
            && self.update.pending.is_none()
            && self.update.holding.is_none()
            && self.update.held.is_none()
            && let Some(check) = self.update.launch.clone()
        {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            self.check_for_update(check, now_ms);
            changed = true;
        }
        // The worker cannot reach the panel's copy: it is brought up to date here.
        let newer = self.newer_release().cloned();
        let progress = self.update.progress();
        let notice = self.update.notice.clone();
        let spin = self.spin();
        if let Some(p) = &mut self.popup
            && p.kind == PopupKind::About
            && let Some(v) = &mut p.about
        {
            v.spin = spin;
            v.check = self.update.check.clone();
            v.newer = newer;
            v.progress = progress;
            v.notice = notice;
        }
        self.update.asking()
            || self.update.arm.is_some()
            || self.update.crashes.is_some()
            || self.update.job.is_some()
            || changed
    }

    /// The discovered update remains visible when asking at launch is off.
    pub fn newer_release(&self) -> Option<&update::Release> {
        self.update.newer.as_ref()
    }

    /// May this build and this folder put an update in (UI-282)? The build without a key, and
    /// one unpacked from the zip, only open the release's page in the browser.
    pub(crate) fn can_download_update(&self) -> bool {
        !update::is_placeholder_key(&self.update.key)
            && self.update.location.as_ref().is_some_and(|l| {
                if self.transport.is_windowed() {
                    startup_cache().is_some_and(|c| c.installable)
                } else {
                    l.installable()
                }
            })
    }

    /// Starts the download off the window's thread; [`Studio::poll_update`] takes it.
    fn start_update_download(&mut self) -> Result<(), String> {
        if self.update.asking() || self.update.check == CheckState::Checking {
            return Err("更新を確認中です".into());
        }
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
        self.update.download_channel = Some(self.update_beta());
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
        if self.update.asking() || self.update.check == CheckState::Checking {
            return Err("更新を確認中です".into());
        }
        if self.update.ready.as_ref().is_some_and(|i| {
            self.update
                .newer
                .as_ref()
                .is_none_or(|r| !installer_matches(r, i))
        }) {
            return Err("更新の対象が変わりました。再ダウンロードが必要です".into());
        }
        if self
            .update
            .ready
            .as_ref()
            .is_some_and(|i| i.manifest.channel == update::BETA)
            && !self.update_beta()
        {
            return Err("ベータ版は受け取らない設定です".into());
        }
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
        if self.transport.is_windowed() {
            if self.update.arm.is_some() {
                return Ok(());
            }
            let key = self.update.key.clone();
            let checked = installer.clone();
            let job = crate::ancillary_io::submit(move || {
                #[cfg(any(test, feature = "loading-probe"))]
                xirius_core::io_test::before("update_verify", &installer.path)
                    .map_err(|e| e.to_string())?;
                installer.verify_with(&key)?;
                let running = update::running_programs(&location.install_dir, std::process::id());
                if !running.is_empty() {
                    return Err(format!("{} が動いています", running.join("、")));
                }
                location.install_command(&installer.path)
            })?;
            self.update.arm = Some((checked, job));
            return Ok(());
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: synchronous offline adapter; window mode returns through the startup/installer owner before this call.
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

    /// Before the window may close for an update (UI-282), its installer is checked once more
    /// (the song's save may have taken a while) and held open, so that what is started is what
    /// was checked. The window stays meanwhile, with nothing done to the song
    /// ([`Studio::input_blocked`]), for at most [`PREPARE_WAIT`]; a check that fails or takes
    /// longer calls the update off and says why, and the window stays. Nothing is started
    /// here: [`Studio::commit_close`] does, once nothing can stop the close any more. False
    /// while it is checked; the window is asked to close again once it is.
    pub(crate) fn prepare_install(&mut self) -> bool {
        if self.update.pending.is_none() || self.update.held.is_some() {
            return true;
        }
        if self.update.holding.is_some() {
            return false;
        }
        let Some(installer) = self.update.ready.clone() else {
            self.drop_update_install("インストーラーがないため、更新を取りやめました".into());
            return false;
        };
        let key = self.update.key.clone();
        if self.transport.is_windowed() {
            match crate::ancillary_io::submit(move || {
                #[cfg(any(test, feature = "loading-probe"))]
                xirius_core::io_test::before("update_hold", &installer.path)
                    .map_err(|e| e.to_string())?;
                installer.hold_with(&key)
            }) {
                Ok(job) => {
                    self.update.holding = Some((job, std::time::Instant::now()));
                    self.toast = Some(crate::toast::Toast {
                        busy: true,
                        ..crate::toast::Toast::plain(PREPARING.into())
                    });
                }
                Err(e) => self.drop_update_install(format!("更新を取りやめました: {e}")),
            }
            return false;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: synchronous offline adapter; window mode checks it on the application-files worker above.
        match installer.hold_with(&key) {
            Ok(held) => {
                self.update.held = Some(held);
                true
            }
            Err(e) => {
                self.drop_update_install(format!("更新を取りやめました: {e}"));
                false
            }
        }
    }

    /// The last step before the window goes (UI-282): the close is decided and nothing can
    /// stop it any more, so the installer an update waits for is started now, from the file
    /// checked and held by [`Studio::prepare_install`]. It waits for Xirius to end. When it
    /// cannot be started, the window stays and says why.
    pub(crate) fn commit_close(&mut self) -> bool {
        let Some(install) = self.update.pending.take() else {
            return true;
        };
        let held = self.update.held.take();
        // Only the file checked and held is started.
        let started = match &held {
            Some(h) if h.is(&install.program) => (self.update.spawn)(&install),
            _ => Err("確かめたインストーラーと違うため、起動しませんでした".into()),
        };
        drop(held);
        match started {
            Ok(()) => {
                self.update
                    .launched
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                true
            }
            Err(e) => {
                self.close_called_off();
                self.update.notice = Some(e.clone());
                self.warn(format!("更新を取りやめました: {e}"), None);
                false
            }
        }
    }

    /// The close stopped and the window stays (UI-282): what it got ready (the plugins' state
    /// taken into the song, the recent list told) is of that moment, and the window can be used
    /// again before the next close, so that close gets it all ready again.
    fn close_called_off(&mut self) {
        self.leaving_ready = false;
        self.keep.close_called_off();
    }

    /// The window's close went through every step: saved, checked and the installer started
    /// (UI-282). Only the window's frame calls it, right before the window goes.
    pub(crate) fn close_now(&mut self) -> bool {
        self.request_close() && self.commit_close()
    }

    /// The window's ask for this frame. A close the OS asked for (the caption's ×, Alt+F4, the
    /// taskbar: [`Studio::close_knock`]) goes the way of 「終了」, from the frame (UI-282).
    pub(crate) fn take_window_ask(&mut self) -> Option<crate::context::key_more::WindowAsk> {
        use crate::context::key_more::WindowAsk;
        let ask = self.window_ask.take();
        if !self.close_knock.take() {
            return ask;
        }
        if let Some(other) = ask.filter(|a| *a != WindowAsk::Quit) {
            self.window_ask = Some(other);
        }
        Some(WindowAsk::Quit)
    }

    /// The close that was to put the update in stopped (a save cancelled or failed, a
    /// question answered 「戻る」, the installer not checked in time): nothing is put in later,
    /// and the person is told why (UI-282).
    pub(crate) fn drop_update_install(&mut self, why: String) {
        self.update.holding = None;
        self.update.held = None;
        self.close_called_off();
        if self.update.pending.take().is_none() {
            return;
        }
        self.update.notice = Some(why.clone());
        // A failure already said (a save's, with its buttons) stays; the panel says this one.
        if self.toast.as_ref().is_none_or(|t| !t.alert) {
            self.warn(why, None);
        }
    }

    /// What is left once the last window has closed (UI-282): the song's cleanup and the
    /// application files are handed to their workers; an installer the window could not start
    /// (the OS closed it without asking) is started there.
    pub(crate) fn after_window_closed(&mut self) -> AfterClose {
        let recent_since = self
            .keep
            .recent_close_since()
            .unwrap_or_else(std::time::Instant::now);
        // This song's cleanup joins those of the songs closed before (REC-026), and its folder
        // is let go: a close is not a crash.
        if let Err(e) = self.close_window_song() {
            crate::ancillary_io::log!("注意: {e}");
        }
        if let Err(e) = self.finish_window_io() {
            crate::ancillary_io::log!("注意: {e}");
        }
        AfterClose {
            recent_since,
            launched: self.update.launched.clone(),
            leave: self.update.leave.clone(),
        }
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
            let url = url.to_owned();
            crate::ancillary_io::dispatch(move || xirius_session::os::open(&url))
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
        if self.settings.is_none() && self.popup.as_ref().and_then(|p| p.about.as_ref()).is_none() {
            return false;
        }
        let done = match act {
            Act::Check => {
                self.manual_update_check();
                Ok(())
            }
            Act::Source => self.open_web(mp3::LAME_SOURCE_URL),
            Act::SourceRelease => self.open_web(mp3::LAME_RELEASE_URL),
            Act::Repository => self.open_web(update::REPOSITORY),
            // UI-282: with a key and the per-user folder, the button downloads and checks;
            // otherwise it opens the release's page, as it did before.
            Act::Download if self.update.asking() || self.update.check == CheckState::Checking => {
                Ok(())
            }
            Act::Download if self.can_download_update() => self.start_update_download(),
            Act::Download | Act::Page => self.open_web(&page_url(&self.update_view())),
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
                self.update.crashes = None;
                v.crashes = if v.crashes.is_some() {
                    None
                } else if self.transport.is_windowed() {
                    match crate::ancillary_io::submit(|| {
                        #[cfg(any(test, feature = "loading-probe"))]
                        xirius_core::io_test::before(
                            "crash_reports",
                            &xirius_engine::crash::directory(),
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(crate::crash_reports::View::load())
                    }) {
                        Ok(job) => self.update.crashes = Some(job),
                        Err(e) => self.error = Some(e),
                    }
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
                        Some(path) if self.os_files => crate::ancillary_io::dispatch(move || {
                            if matches!(act, crate::crash_reports::Act::Folder(_)) {
                                xirius_session::os::open(&path.display().to_string())
                            } else {
                                xirius_session::os::reveal(&path)
                            }
                        }),
                        Some(_) => Ok(()),
                        None => Err("クラッシュ記録がありません".into()),
                    },
                }
            }
            Act::Close => {
                self.update.crashes = None;
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
                crate::ancillary_io::dispatch(move || {
                    if path.exists() {
                        xirius_session::os::reveal(&path)
                    } else {
                        path.parent()
                            .map(|d| d.display().to_string())
                            .ok_or_else(|| "フォルダーがありません".to_string())
                            .and_then(|d| xirius_session::os::open(&d))
                    }
                })
            }
            // The web pages: [`Studio::about_action`] opens them.
            Act::Source
            | Act::SourceRelease
            | Act::Repository
            | Act::Download
            | Act::Page
            | Act::UpdateNow
            | Act::Check => Ok(()),
            Act::License(item) => crate::ancillary_io::dispatch(move || {
                license_file(item).and_then(|p| xirius_session::os::open(&p.display().to_string()))
            }),
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
        assert!(texts.iter().any(|t| t.starts_with("バージョン ")));
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
        // Layout cells are screen-relative, including the popup anchor offset.
        let last = cells
            .iter()
            .find(|c| c.action == Some(Action::About(Act::License(Item::Lame))))
            .unwrap();
        let panel = s.popup.as_ref().unwrap().layout(g.size).rect;
        assert!(
            last.rect.right() <= panel.right() - PAD,
            "{:?} in {panel:?}",
            last.rect
        );
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
        const ELSEWHERE: &[(&str, &str, &str)] = &[
            (
                "dawproject",
                "MIT",
                "crates/xirius-core/src/dawproject/testdata/reference/LICENSE",
            ),
            // LIC-004: the ARA API headers, which crates/xirius-ara is written from.
            ("ara-api", "Apache-2.0", "third_party/ara_api/LICENSE.txt"),
        ];
        for &(name, license, path) in ELSEWHERE {
            let c = listed
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{name}: run python scripts/licenses.py"));
            assert_eq!(c.license, license, "{name}");
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

    #[test]
    fn startup_and_manual_answers_discard_an_installer_for_another_release() {
        for startup in [true, false] {
            let mut s = Studio::sample();
            crate::update_shots::stage(&mut s, 2);
            let installer = s.update.ready.take().unwrap();
            let mut release = release_of(&installer);
            release.version = "999.0.0".into();
            s.update.channel = Some(false);
            s.bind_window_io();
            if startup {
                s.update.startup = Some(
                    crate::ancillary_io::submit(move || {
                        Ok(StartupUpdate {
                            note: None,
                            installer: Some(installer),
                            newer: Some(release),
                            beta: false,
                            total: 100,
                        })
                    })
                    .unwrap(),
                );
            } else {
                s.update.ready = Some(installer);
                s.update.asking = Some(std::thread::spawn(move || Ok(Some(release))));
            }
            let _ui = xirius_core::io_test::forbid_ui_io();
            let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while s.update.startup.is_some() || s.update.asking.is_some() {
                s.poll_update();
                assert!(std::time::Instant::now() < end);
                std::thread::yield_now();
            }
            assert!(s.update.ready.is_none());
            assert_eq!(s.update.progress().phase, update::Phase::Idle);
            assert_eq!(s.update.newer.as_ref().unwrap().version, "999.0.0");
            let actions = acts(&update_cells(&s.update_view(), 440.0));
            assert!(actions.contains(&Act::Download));
            assert!(!actions.contains(&Act::UpdateNow));
        }
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
        assert!(release.rect.bottom() <= download.rect.y);
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
        assert_eq!(repo.rect.y, version.rect.y);
        assert!(version.rect.right() <= repo.rect.x);
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
        assert!(cells.iter().any(|c| c.text == "MIT OR Apache-2.0"));
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

    /// A checked installer in `root`'s download folder, and the key it is signed with (UI-282).
    pub(crate) fn signed_installer(root: &std::path::Path) -> (update::Installer, String) {
        signed_installer_bytes(root, b"the installer")
    }

    fn signed_installer_bytes(root: &std::path::Path, body: &[u8]) -> (update::Installer, String) {
        use ed25519_dalek::Signer;

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
        let path = root.join(update::DOWNLOAD_DIR).join(version).join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        let installer = update::Installer {
            version: version.into(),
            path,
            manifest: serde_json::from_slice(&manifest_bytes).unwrap(),
            manifest_bytes,
            signature,
            sums,
        };
        (installer, public)
    }

    /// What a test's installer was asked to do: the programs started, and the times Xirius was
    /// told to end soon (UI-282).
    #[derive(Clone, Default)]
    pub(crate) struct Started {
        pub(crate) programs: std::sync::Arc<std::sync::Mutex<Vec<update::Install>>>,
        pub(crate) leaves: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Started {
        /// Puts a checked installer in `root`, waiting for the close, with this record in place
        /// of starting it.
        pub(crate) fn arm(&self, s: &mut Studio, root: &std::path::Path) -> std::path::PathBuf {
            let (installer, key) = signed_installer(root);
            let path = installer.path.clone();
            s.update.key = key;
            s.update.root = Some(root.to_path_buf());
            s.update.ready = Some(installer);
            s.update.pending = Some(update::Install {
                program: path.clone(),
                args: vec!["/S".into(), "/XRUN".into()],
            });
            let programs = self.programs.clone();
            s.update.spawn = std::sync::Arc::new(move |install| {
                programs.lock().unwrap().push(install.clone());
                Ok(())
            });
            let leaves = self.leaves.clone();
            s.update.leave = std::sync::Arc::new(move || {
                leaves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
            path
        }

        pub(crate) fn count(&self) -> usize {
            self.programs.lock().unwrap().len()
        }

        pub(crate) fn leaves(&self) -> usize {
            self.leaves.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    /// UI-282: before the window closes, the installer is checked once more (the song's save
    /// may take a while) and held, but nothing is started: only the last step of the close
    /// starts it, once nothing can stop the close. A file replaced after the download is not
    /// started; the window stays and says why, and leaves no note for a next start.
    #[test]
    fn the_installer_is_started_only_once_the_close_is_decided() {
        let tmp = scratch("recheck");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let started = Started::default();
        let path = started.arm(&mut s, tmp.path());

        // Intact: checked and held, not started yet.
        assert!(s.prepare_install());
        assert_eq!(
            started.count(),
            0,
            "nothing is started before the close is decided"
        );
        assert!(s.update.held.is_some());
        assert!(s.commit_close());
        assert_eq!(started.count(), 1);
        assert!(s.update.launched());
        assert_eq!(started.leaves(), 0, "Xirius is not ended while it closes");
        assert!(s.update.pending.is_none() && s.update.held.is_none());
        // Nothing waits any more: a second ask closes at once and starts nothing.
        assert!(s.prepare_install() && s.commit_close());
        assert_eq!(started.count(), 1);

        // Replaced after it was checked: it is not started, and the window stays.
        s.update
            .launched
            .store(false, std::sync::atomic::Ordering::SeqCst);
        s.update.pending = Some(update::Install {
            program: path.clone(),
            args: vec!["/S".into(), "/XRUN".into()],
        });
        std::fs::write(&path, b"another installer").unwrap();
        assert!(!s.prepare_install(), "the window stays");
        assert_eq!(started.count(), 1);
        assert!(!s.update.launched());
        assert!(s.update.pending.is_none(), "nothing is put in later");
        assert!(
            s.update
                .notice
                .as_deref()
                .is_some_and(|n| n.contains(update::BAD_SUM))
        );
        assert!(!tmp.path().join(update::FAILED).exists());
    }

    /// UI-282: an installer the OS will not start keeps the window (with its song) open and
    /// says why; Xirius is not ended, and no note is left for a next start.
    #[test]
    fn an_installer_that_does_not_start_keeps_the_window() {
        let tmp = scratch("nostart");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let started = Started::default();
        started.arm(&mut s, tmp.path());
        s.update.spawn = std::sync::Arc::new(|_| Err("起動できません".into()));
        assert!(s.prepare_install());
        assert!(!s.commit_close(), "the window stays");
        assert!(!s.update.launched());
        assert!(s.update.pending.is_none() && s.update.held.is_none());
        assert!(
            s.update
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("起動できません"))
        );
        assert!(s.toast.as_ref().is_some_and(|t| t.alert));
        assert!(!tmp.path().join(update::FAILED).exists());
        // The next ask closes the window the plain way.
        assert!(s.prepare_install() && s.commit_close());
        assert_eq!(started.leaves(), 0);
    }

    /// UI-282: in the window, the installer is checked on the application-files worker while
    /// the window waits (an × pressed meanwhile starts nothing and closes nothing), and the
    /// window is asked to close again once it is.
    #[test]
    fn in_the_window_the_close_waits_for_the_check() {
        let tmp = scratch("order");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        s.transport.bind_window();
        let started = Started::default();
        started.arm(&mut s, tmp.path());

        assert!(!s.prepare_install(), "the close waits for the check");
        assert!(s.update.preparing());
        assert!(s.input_blocked(), "nothing is done to the song meanwhile");
        assert!(
            s.toast
                .as_ref()
                .is_some_and(|t| t.busy && t.text == PREPARING)
        );
        // A second ask (the caption's ×) neither checks it twice nor closes.
        assert!(!s.prepare_install());
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while s.update.preparing() {
            assert!(std::time::Instant::now() < end);
            s.poll_update();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(started.count(), 0);
        assert!(s.update.held.is_some());
        assert_eq!(
            s.window_ask,
            Some(crate::context::key_more::WindowAsk::Quit),
            "the window is asked to close again"
        );
        assert!(s.prepare_install(), "now it may close");
        assert!(s.commit_close());
        assert_eq!(started.count(), 1);
    }

    /// UI-282: a check that does not come back in time calls the update off; the window stays
    /// and can be used again.
    #[test]
    fn a_check_that_takes_too_long_calls_the_update_off() {
        let tmp = scratch("late");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        s.transport.bind_window();
        let started = Started::default();
        started.arm(&mut s, tmp.path());
        let now = std::time::Instant::now();
        s.update.holding = Some((crate::ancillary_io::Ticket::never(), now));
        assert!(s.input_blocked());
        s.poll_update();
        assert!(s.update.preparing(), "it waits a while");
        let Some(long_ago) = now.checked_sub(PREPARE_WAIT + std::time::Duration::from_secs(1))
        else {
            return;
        };
        s.update.holding = Some((crate::ancillary_io::Ticket::never(), long_ago));
        s.poll_update();
        assert!(!s.update.preparing());
        assert!(!s.input_blocked(), "the window can be used again");
        assert!(s.update.pending.is_none());
        assert!(
            s.update
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("時間がかかりすぎた"))
        );
        assert_eq!(s.window_ask, None, "the window stays");
        assert_eq!(started.count(), 0);
    }

    /// UI-282: 「戻る」 to the question a close asks (the plugins' state could not be read)
    /// keeps the window, and the update that waited for the close is called off.
    #[test]
    fn a_close_answered_back_calls_the_update_off() {
        let tmp = scratch("back");
        let mut s = Studio::new(crate::song::Song::empty());
        s.song.save_as(&tmp.path().join("song")).unwrap();
        s.transport.bind_window();
        let started = Started::default();
        started.arm(&mut s, tmp.path());
        s.keep.plugin_state_failed = true;
        s.window_ask = Some(crate::context::key_more::WindowAsk::Quit);
        assert_eq!(
            s.take_window_ask(),
            Some(crate::context::key_more::WindowAsk::Quit)
        );
        assert!(!s.close_now());
        assert!(s.unsaved.is_some(), "the close asks");
        assert!(
            s.update.pending.is_some(),
            "the update waits for the answer"
        );
        s.answer_unsaved(false);
        assert!(s.update.pending.is_none());
        assert!(s.update.notice.is_some());
        assert_eq!(started.count(), 0);
    }

    /// UI-282: a song whose record could not be written (保存できません) is not closed over
    /// for an update: the close asks, 「戻る」 keeps the window with the edit and calls the
    /// update off, and only 「保存せずに進む」 lets it go on.
    #[test]
    fn ui_282_a_song_that_could_not_be_written_is_not_closed_for_an_update() {
        use crate::context::key_more::WindowAsk;

        for proceed in [false, true] {
            let tmp = scratch("unwritten");
            let mut s = Studio::new(crate::song::Song::empty());
            s.song.save_as(&tmp.path().join("song")).unwrap();
            s.run_command("track.add.audio", serde_json::json!({"name":"kept"}));
            s.transport.bind_window();
            let started = Started::default();
            started.arm(&mut s, tmp.path());
            s.keep.failed = Some("保存できません".into());
            s.window_ask = Some(WindowAsk::Quit);
            assert_eq!(s.take_window_ask(), Some(WindowAsk::Quit));
            assert!(!s.close_now(), "the window stays");
            assert!(s.unsaved.is_some(), "the close asks");
            assert!(!s.keep.plugin_state_failed, "not the plugins' question");
            assert!(
                s.update.pending.is_some(),
                "the update waits for the answer"
            );
            assert_eq!(started.count(), 0);
            s.answer_unsaved(proceed);
            if !proceed {
                assert!(s.update.pending.is_none());
                assert!(s.update.notice.is_some());
                assert_eq!(s.window_ask, None, "the window stays");
                assert!(
                    s.project().tracks.values().any(|t| t.name == "kept"),
                    "with the edit"
                );
                assert_eq!(started.count(), 0);
                continue;
            }
            let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                assert!(
                    std::time::Instant::now() < end,
                    "the person said to go on, and the window did not close"
                );
                s.idle();
                s.poll_saving();
                if s.take_window_ask() == Some(WindowAsk::Quit) && s.close_now() {
                    break;
                }
                assert_eq!(started.count(), 0);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(started.count(), 1);
        }
    }

    /// UI-282 review: 「保存せずに進む」 to a song whose record could not be written goes on
    /// though the record still cannot be written when the window closes: the person chose to
    /// lose those edits, so the update is not called off for them.
    #[test]
    fn ui_282_going_on_without_the_record_closes_though_its_write_still_fails() {
        use crate::context::key_more::WindowAsk;

        let tmp = scratch("unwritable");
        let dir = tmp.path().join("song");
        let mut s = Studio::new(crate::song::Song::empty());
        s.song.save_as(&dir).unwrap();
        let denied = dir.clone();
        let _observe = xirius_core::io_test::observe(std::sync::Arc::new(move |op, path| {
            if op == "write" && path.starts_with(&denied) {
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            Ok(())
        }));
        s.run_command("track.add.audio", serde_json::json!({"name":"lost"}));
        s.transport.bind_window();
        let started = Started::default();
        started.arm(&mut s, tmp.path());
        s.keep.failed = Some("保存できません".into());
        s.window_ask = Some(WindowAsk::Quit);
        assert_eq!(s.take_window_ask(), Some(WindowAsk::Quit));
        assert!(!s.close_now());
        assert!(s.unsaved.is_some(), "the close asks");
        s.answer_unsaved(true);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            assert!(
                std::time::Instant::now() < end,
                "the person said to go on, and the window did not close"
            );
            assert!(
                s.update.pending.is_some(),
                "the update was called off: {:?}",
                s.update.notice
            );
            s.idle();
            s.poll_saving();
            s.poll_update();
            if s.take_window_ask() == Some(WindowAsk::Quit) && s.close_now() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(started.count(), 1);
    }

    /// UI-282 review: a close for an update that stops after the song was made ready (here the
    /// installer's check takes too long) leaves the window to be used again, so the next close
    /// takes the plugins' state into the song and saves it again rather than go on with what
    /// the stopped one had got ready.
    #[test]
    fn ui_282_a_close_called_off_gets_the_song_ready_again_the_next_time() {
        use crate::context::key_more::WindowAsk;

        let tmp = scratch("again");
        let mut s = Studio::new(crate::song::Song::empty());
        s.song.save_as(&tmp.path().join("song")).unwrap();
        s.run_command("track.add.audio", serde_json::json!({"name":"first"}));
        s.save_test_delay = std::time::Duration::from_millis(10);
        s.transport.bind_window();
        let started = Started::default();
        started.arm(&mut s, tmp.path());
        s.window_ask = Some(WindowAsk::Quit);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !s.update.preparing() {
            assert!(
                std::time::Instant::now() < end,
                "the close did not get ready"
            );
            s.idle();
            s.poll_saving();
            if s.take_window_ask() == Some(WindowAsk::Quit) {
                assert!(!s.close_now(), "the close waits for the check");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(s.leaving_ready, "the song was made ready for the close");
        let Some(long_ago) =
            std::time::Instant::now().checked_sub(PREPARE_WAIT + std::time::Duration::from_secs(1))
        else {
            return;
        };
        s.update.holding = Some((crate::ancillary_io::Ticket::never(), long_ago));
        s.poll_update();
        assert!(s.update.pending.is_none(), "the update is called off");
        assert!(
            !s.leaving_ready,
            "what the stopped close got ready is not kept"
        );

        // The window is used, and closed again: the song is made ready once more.
        assert!(s.saving.is_none());
        assert!(!s.close_now(), "the close saves first");
        assert!(
            s.saving.is_some(),
            "the next close takes the song's state again"
        );
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            assert!(std::time::Instant::now() < end, "the window did not close");
            s.idle();
            s.poll_saving();
            if s.take_window_ask() == Some(WindowAsk::Quit) && s.close_now() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(started.count(), 0, "nothing is installed");
    }

    /// UI-282 review: the last step of the close starts only the very file that was checked
    /// and held. Another file asked for at that moment is not started, the window stays, and
    /// the next close gets the song ready again.
    #[test]
    fn ui_282_only_the_held_installer_is_started() {
        let tmp = scratch("other");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let started = Started::default();
        started.arm(&mut s, tmp.path());
        assert!(s.prepare_install());
        assert!(s.update.held.is_some());
        s.leaving_ready = true;
        if let Some(install) = &mut s.update.pending {
            install.program = tmp.path().join("another.exe");
        }
        assert!(!s.commit_close(), "the window stays");
        assert_eq!(started.count(), 0);
        assert!(!s.update.launched());
        assert!(s.update.pending.is_none() && s.update.held.is_none());
        assert!(!s.leaving_ready);
        assert!(
            s.update
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("確かめたインストーラーと違う"))
        );
    }

    /// How a test closes the window of a song with unsaved changes and an update waiting.
    #[derive(Clone, Copy, Debug)]
    enum Way {
        /// The caption's × (or Alt+F4) with the update already waiting for the close.
        Caption,
        /// 「更新して再起動」 in the panel.
        Button,
    }

    /// UI-282: the window's close for an update, as it runs in use: the frames poll, the OS's
    /// close only knocks, the song is saved, the installer is checked, and it is started at the
    /// last step, once. The cleanup after the window starts nothing more, and only then is
    /// what is left of Xirius told to end.
    #[test]
    fn ui_282_a_window_closes_for_an_update_as_in_use() {
        use crate::context::key_more::WindowAsk;

        for way in [Way::Caption, Way::Button] {
            let tmp = scratch("in-use");
            let root = tmp.path();
            let mut s = Studio::new(crate::song::Song::empty());
            s.song.save_as(&root.join("song")).unwrap();
            s.run_command("track.add.audio", serde_json::json!({"name":"pending"}));
            s.transport.bind_window();
            let started = Started::default();
            let installer_path = started.arm(&mut s, root);
            let release = crate::browser_io_tests::Release::new();
            let gate = release.0.clone();
            let _observe = xirius_core::io_test::observe(std::sync::Arc::new(move |op, path| {
                if op == "update_hold" && path == installer_path {
                    drop(
                        gate.1
                            .wait_while(gate.0.lock().unwrap(), |done| !*done)
                            .unwrap(),
                    );
                }
                Ok(())
            }));
            match way {
                Way::Caption => s.close_knock.set(true),
                Way::Button => {
                    s.update.pending = None;
                    let installer = s.update.ready.clone().unwrap();
                    s.update.newer = Some(release_of(&installer));
                    let app = root.join("app");
                    s.update.location = Some(update::Location {
                        exe_dir: app.clone(),
                        install_dir: app,
                    });
                    s.update.progress.lock().unwrap().phase = update::Phase::Ready;
                    // The panel's button, pressed as the person does.
                    let g = s.geometry(1600.0, 900.0);
                    s.press(&g, Hit::Bar(BarButton::Logo));
                    let now = cells_of(&s, &g)
                        .iter()
                        .position(|c| c.action == Some(Action::About(Act::UpdateNow)))
                        .expect("「更新して再起動」");
                    assert!(s.press(&g, Hit::Item(now)));
                    assert_eq!(s.update.notice, None);
                }
            }
            let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
            let mut knocked = false;
            loop {
                assert!(
                    std::time::Instant::now() < end,
                    "{way:?}: the window did not close"
                );
                s.idle();
                s.poll_saving();
                if s.update.preparing() && !knocked {
                    // An × pressed while it waits starts nothing twice.
                    s.close_knock.set(true);
                    knocked = true;
                    release.release();
                }
                if s.take_window_ask() == Some(WindowAsk::Quit) && s.close_now() {
                    break;
                }
                assert_eq!(
                    started.count(),
                    0,
                    "{way:?}: started before the close was decided"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(knocked, "{way:?}: the installer was checked in the window");
            assert_eq!(started.count(), 1, "{way:?}");
            assert!(s.update.launched());
            assert_eq!(s.update.notice, None);
            let exit = s.after_window_closed();
            assert_eq!(
                started.leaves(),
                0,
                "{way:?}: not ended before the cleanup is done"
            );
            exit.quitting();
            assert_eq!(started.leaves(), 1, "{way:?}");
            assert_eq!(
                started.count(),
                1,
                "{way:?}: the cleanup starts nothing more"
            );
            assert!(!root.join(update::FAILED).exists());
        }
    }

    /// UI-282: a close the OS asks for only knocks; the frame turns it into 「終了」, and an
    /// ask for the full screen made in the same frame is kept for the next.
    #[test]
    fn the_caption_close_knocks_and_the_frame_closes() {
        use crate::context::key_more::WindowAsk;

        let mut s = Studio::sample();
        assert_eq!(s.take_window_ask(), None);
        s.close_knock.set(true);
        s.window_ask = Some(WindowAsk::Fullscreen);
        assert_eq!(s.take_window_ask(), Some(WindowAsk::Quit));
        assert_eq!(s.take_window_ask(), Some(WindowAsk::Fullscreen));
        assert_eq!(s.take_window_ask(), None);
    }

    /// The process a test runs as Xirius closing for an update (UI-282): it saves, closes the
    /// way the window does, starting the real installer program, and then falls over the way
    /// a plugin may while the song is torn down. Run by the test below; alone it does nothing.
    #[cfg(windows)]
    #[test]
    fn ui_282_closing_child() {
        let Some(root) = std::env::var_os("XIRIUS_UPDATE_CLOSE_TEST") else {
            return;
        };
        if std::env::var_os("XIRIUS_UPDATE_CLOSE_ROLE").is_some_and(|r| r == "installer") {
            return;
        }
        let root = std::path::PathBuf::from(root);
        let mut s = Studio::new(crate::song::Song::empty());
        s.song.save_as(&root.join("song")).unwrap();
        s.run_command("track.add.audio", serde_json::json!({"name":"pending"}));
        let started = Started::default();
        started.arm(&mut s, &root);
        // Sign the exact executable that will run the installer half of this test.
        let body = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        let (installer, key) = signed_installer_bytes(&root, &body);
        let program = installer.path.clone();
        s.update.key = key;
        s.update.ready = Some(installer);
        s.update.pending = Some(update::Install {
            program,
            args: vec![
                "--exact".into(),
                "about::tests::ui_282_installer_child".into(),
                "--nocapture".into(),
            ],
        });
        s.update.spawn = std::sync::Arc::new(update::spawn_detached);
        // SAFETY: set before the installer is started, from the only thread that reads it.
        unsafe { std::env::set_var("XIRIUS_UPDATE_CLOSE_ROLE", "installer") };
        s.transport.bind_window();
        // As the window does: each ask to close, then the polls of the frames between.
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        s.window_ask = Some(crate::context::key_more::WindowAsk::Quit);
        loop {
            assert!(
                std::time::Instant::now() < end,
                "the window did not close: {:?}",
                s.update.notice
            );
            if s.take_window_ask() == Some(crate::context::key_more::WindowAsk::Quit)
                && s.close_now()
            {
                break;
            }
            s.idle();
            s.poll_saving();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(s.update.launched(), "started before the window goes");
        std::fs::write(root.join("closing"), b"").unwrap();
        // A plugin falls over while the song is torn down (with no unwinding and no report box).
        // SAFETY: ends this process; nothing runs after it.
        unsafe {
            use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
            TerminateProcess(GetCurrentProcess(), 0xC000_0005);
        }
    }

    /// The installer the child above starts: it waits for Xirius to be gone, as the real one
    /// does, and then puts its mark down.
    #[cfg(windows)]
    #[test]
    fn ui_282_installer_child() {
        let Some(root) = std::env::var_os("XIRIUS_UPDATE_CLOSE_TEST") else {
            return;
        };
        if std::env::var_os("XIRIUS_UPDATE_CLOSE_ROLE").is_none_or(|r| r != "installer") {
            return;
        }
        let root = std::path::PathBuf::from(root);
        std::fs::write(root.join("installer started"), b"").unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !root.join("go").exists() {
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::write(root.join("installed"), b"").unwrap();
    }

    /// UI-282: the installer of 「更新して再起動」 is started while the window is still there, and
    /// it outlives whatever happens to Xirius after: a crash while the song is torn down, and
    /// the end of a job Xirius was started in (a terminal's or a launcher's that kills what is
    /// in it when it closes).
    #[cfg(windows)]
    #[test]
    fn an_installer_started_on_the_way_out_outlives_a_crash_and_the_job() {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        };

        // Refusing an installer in a killing job is covered in xirius-session. This test
        // must actually launch one; an unsupported runner must not be reported as a pass.
        assert!(
            may_leave_jobs() || !update::job_kills_on_close(),
            "unsupported test environment: KILLING_JOB prevents installer launch; run through nextest"
        );
        let tmp = scratch("outlives");
        let root = tmp.path();
        // A job this test runs in that lets nothing leave would keep the installer in ours as
        // well; then only the crash is tried.
        // SAFETY: the job is ours, its limits are a zeroed structure of the right size, and
        // its handle is closed below.
        let job = may_leave_jobs().then(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            assert!(!job.is_null());
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            assert_ne!(
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ),
                0
            );
            job
        });
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_SUSPENDED: u32 = 0x0000_0004;
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "about::tests::ui_282_closing_child",
                "--nocapture",
            ])
            .env("XIRIUS_UPDATE_CLOSE_TEST", root)
            .env_remove("XIRIUS_UPDATE_CLOSE_ROLE")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
            .spawn()
            .unwrap();
        if let Some(job) = job {
            // SAFETY: the job and the child are ours; the child is suspended, so it is in the
            // job before it runs.
            unsafe {
                assert_ne!(AssignProcessToJobObject(job, child.as_raw_handle() as _), 0);
            }
        }
        resume(&child);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(std::time::Instant::now() < end, "the closing child hung");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(!status.success(), "it fell over, as asked");
        assert!(
            root.join("closing").is_file(),
            "it fell over after the close"
        );
        // The job ends at once: everything still in it is killed.
        if let Some(job) = job {
            // SAFETY: the job handle is ours.
            unsafe { CloseHandle(job) };
        }
        std::fs::write(root.join("go"), b"").unwrap();
        while !root.join("installed").is_file() {
            assert!(
                std::time::Instant::now() < end,
                "the installer did not outlive Xirius"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// May a process this test starts leave the job the test runs in (or is there none)?
    #[cfg(windows)]
    fn may_leave_jobs() -> bool {
        use windows_sys::Win32::System::JobObjects::{
            IsProcessInJob, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            QueryInformationJobObject,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        // SAFETY: the structures are ours, zeroed and sized; a null job is this process's own.
        unsafe {
            let mut in_job = 0;
            if IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) == 0 {
                return false;
            }
            if in_job == 0 {
                return true;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            QueryInformationJobObject(
                std::ptr::null_mut(),
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as *mut _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            ) != 0
                && limits.BasicLimitInformation.LimitFlags
                    & (JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK)
                    != 0
        }
    }

    /// Resumes the first thread of a process started suspended.
    #[cfg(windows)]
    fn resume(child: &std::process::Child) {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        };
        use windows_sys::Win32::System::Threading::{
            OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
        };

        // SAFETY: the snapshot and the thread handles are ours and closed; the entry is sized.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            assert_ne!(snap, INVALID_HANDLE_VALUE);
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut more = Thread32First(snap, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == child.id() {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    assert!(!thread.is_null());
                    ResumeThread(thread);
                    CloseHandle(thread);
                }
                more = Thread32Next(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
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
    fn finish_manual(s: &mut Studio) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while s.update.asking() {
            assert!(std::time::Instant::now() < until);
            s.poll_update();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        s.poll_update();
    }

    #[test]
    fn manual_three_entries_share_state_even_when_launch_is_off() {
        use crate::settings::{Do, Section};
        for entry in 0..3 {
            let tmp = scratch("manual-entries");
            let mut s = Studio::sample();
            s.song.load_settings(tmp.path());
            let g = s.geometry(1440.0, 820.0);
            assert!(s.run_command(
                "prefs.set",
                serde_json::json!({"key":UPDATE_PREF,"value":"false"})
            ));
            s.update.root = Some(tmp.path().join("updates"));
            let (send, receive) = std::sync::mpsc::channel();
            let receive = std::sync::Mutex::new(receive);
            let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let count = calls.clone();
            s.update.fetch = std::sync::Arc::new(move |_| {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                receive
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                Ok(newer_answer())
            });
            match entry {
                0 => {
                    s.open_settings(Section::General);
                    assert!(s.settings_scene(&g).find(&Do::Update(Act::Check)).is_some());
                    s.settings_do(&g, Do::Update(Act::Check), Rect::new(0.0, 0.0, 1.0, 1.0));
                }
                1 => {
                    s.open_about(s.about_anchor(&g));
                    assert!(acts(&cells_of(&s, &g)).contains(&Act::Check));
                    s.about_action(Act::Check);
                }
                _ => {
                    s.open_palette(&g);
                    let row = s
                        .palette_rows(&g)
                        .into_iter()
                        .find(|r| r.name == "app.update.check")
                        .unwrap();
                    assert!(s.run_row(&g, &row));
                    assert!(s.palette.is_none());
                    assert!(s.popup.as_ref().unwrap().about.is_some());
                }
            }
            assert_eq!(s.update.check, CheckState::Checking);
            assert!(!acts(&update_cells(&s.update_view(), 440.0)).contains(&Act::Check));
            s.manual_update_check(); // no duplicate worker
            send.send(()).unwrap();
            finish_manual(&mut s);
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(s.update.check, CheckState::Latest);
            assert!(s.bar_view().update);
            assert_eq!(s.newer_release().unwrap().version, newer_version());
            s.open_settings(Section::General);
            let scene = s.settings_scene(&g);
            assert!(scene.has_text(newer_version()));
            s.settings_do(
                &g,
                Do::Pref(UPDATE_PREF.into(), "true".into()),
                Rect::new(0.0, 0.0, 1.0, 1.0),
            );
            s.settings_do(
                &g,
                Do::Pref(UPDATE_PREF.into(), "false".into()),
                Rect::new(0.0, 0.0, 1.0, 1.0),
            );
            assert!(s.bar_view().update);
            s.settings = None;
            s.popup = None;
            s.open_about(s.about_anchor(&g));
            assert_eq!(
                s.popup.as_ref().unwrap().about.as_ref().unwrap().newer,
                s.update.newer
            );
        }
    }

    #[test]
    fn manual_failure_keeps_discovery_and_ready_and_exposes_reason_with_retry() {
        let tmp = scratch("manual-fail");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        s.update.root = Some(tmp.path().into());
        crate::update_shots::stage(&mut s, 2);
        let ready = s.update.ready.clone();
        s.update.fetch = std::sync::Arc::new(|_| Err("GitHub の利用上限です (HTTP 403)".into()));
        s.manual_update_check();
        finish_manual(&mut s);
        assert!(matches!(&s.update.check,CheckState::Failed(e) if e.contains("HTTP 403")));
        assert!(s.bar_view().update);
        let cells = update_cells(&s.update_view(), 300.0);
        assert!(acts(&cells).contains(&Act::Check));
        assert!(cells.iter().any(|c| c.text == "再試行"));
        assert!(
            cells
                .iter()
                .map(|c| c.text.replace(' ', ""))
                .collect::<String>()
                .contains("HTTP403")
        );
        assert!(s.error.is_none());
        assert_eq!(s.update.ready, ready);
    }

    #[test]
    fn manual_beta_uses_channel_and_ignores_an_old_channel_answer() {
        let tmp = scratch("manual-beta");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        s.update.root = Some(tmp.path().into());
        let urls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = urls.clone();
        s.update.fetch = std::sync::Arc::new(move |url| {
            seen.lock().unwrap().push(url.to_string());
            let r = serde_json::json!({"tag_name":"v99.0.0-beta.1","html_url":"https://github.com/tekalu1/xirius/releases/tag/v99.0.0-beta.1","prerelease":true,"body":"beta"});
            Ok(if url == update::RELEASES {
                serde_json::json!([r]).to_string()
            } else {
                newer_answer()
            })
        });
        s.manual_update_check();
        finish_manual(&mut s);
        assert!(!s.newer_release().unwrap().prerelease);
        s.run_command(
            "prefs.set",
            serde_json::json!({"key":update::BETA_PREF,"value":"true"}),
        );
        s.poll_update();
        assert!(s.newer_release().is_none());
        s.manual_update_check();
        finish_manual(&mut s);
        assert!(s.newer_release().unwrap().prerelease);
        assert!(
            update_cells(&s.update_view(), 440.0)
                .iter()
                .any(|c| c.text.contains("ベータ版"))
        );
        assert_eq!(*urls.lock().unwrap(), [update::LATEST, update::RELEASES]);
        s.update.asking = Some(std::thread::spawn(|| {
            Ok(Some(crate::update_shots::newer()))
        }));
        s.update.check = CheckState::Checking;
        s.run_command(
            "prefs.set",
            serde_json::json!({"key":update::BETA_PREF,"value":"false"}),
        );
        finish_manual(&mut s);
        assert!(s.newer_release().is_none());
        assert_eq!(s.update.check, CheckState::Idle);
    }

    /// The window reads the settings after the launch asked: a beta channel set there is asked
    /// for then, and the waiting beta is offered (UI-282).
    #[test]
    fn ui_282_the_launch_asks_again_for_the_channel_the_settings_bring() {
        let tmp = scratch("launch-channel");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let urls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = urls.clone();
        let fetch: update::Fetch = std::sync::Arc::new(move |url| {
            seen.lock().unwrap().push(url.to_string());
            let beta = serde_json::json!({"tag_name":"v99.0.0-beta.1","html_url":"https://github.com/tekalu1/xirius/releases/tag/v99.0.0-beta.1","prerelease":true,"body":"beta"});
            Ok(if url == update::RELEASES {
                serde_json::json!([beta]).to_string()
            } else {
                serde_json::json!({"tag_name":"v0.0.1","html_url":"https://github.com/tekalu1/xirius/releases/tag/v0.0.1","body":"old"}).to_string()
            })
        });
        s.check_for_update(update::UpdateCheck::in_dir(tmp.path(), fetch), 1);
        finish_manual(&mut s);
        assert!(s.newer_release().is_none());
        s.run_command(
            "prefs.set",
            serde_json::json!({"key":update::BETA_PREF,"value":"true"}),
        );
        s.poll_update();
        finish_manual(&mut s);
        assert!(s.newer_release().is_some_and(|r| r.prerelease));
        assert_eq!(*urls.lock().unwrap(), [update::LATEST, update::RELEASES]);
        // Asked once for the channel: another frame does not ask again.
        s.poll_update();
        assert!(!s.update.asking());
        assert_eq!(urls.lock().unwrap().len(), 2);
    }

    #[test]
    fn shared_update_section_wraps_and_ready_survives_a_same_release_check() {
        let tmp = scratch("manual-ready");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        crate::update_shots::stage(&mut s, 2);
        let before = s.update.ready.clone();
        s.update.root = Some(tmp.path().into());
        s.update.fetch = std::sync::Arc::new(|_| {
            Ok(serde_json::json!({"tag_name":"v1.1.0","html_url":"https://github.com/tekalu1/xirius/releases/tag/v1.1.0","body":"changes"}).to_string())
        });
        s.manual_update_check();
        finish_manual(&mut s);
        assert_eq!(s.update.ready, before);
        assert_eq!(s.update.progress().phase, update::Phase::Ready);
        for width in [280.0, 440.0, 620.0] {
            for cell in update_cells(&s.update_view(), width) {
                assert!(
                    cell.rect.x >= 0.0 && cell.rect.right() <= width + 0.01,
                    "{width}: {cell:?}"
                );
            }
        }
    }
    #[test]
    fn a_manual_request_during_launch_runs_fresh_after_the_launch_worker() {
        let tmp = scratch("manual-during-launch");
        let mut s = Studio::sample();
        s.song.load_settings(tmp.path());
        let (send, receive) = std::sync::mpsc::channel();
        let receive = std::sync::Mutex::new(receive);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        let fetch: update::Fetch = std::sync::Arc::new(move |_| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            receive
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            Ok(newer_answer())
        });
        s.update.fetch = fetch.clone();
        s.check_for_update(update::UpdateCheck::in_dir(tmp.path(), fetch), 1);
        s.manual_update_check();
        assert_eq!(s.update.check, CheckState::Checking);
        send.send(()).unwrap();
        send.send(()).unwrap();
        finish_manual(&mut s);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(s.bar_view().update);
    }
    #[test]
    fn recheck_blocks_primary_and_start_paths() {
        for ready in [false, true] {
            let mut s = Studio::sample();
            crate::update_shots::stage(&mut s, if ready { 2 } else { 0 });
            s.update.check = CheckState::Checking;
            let actions = acts(&update_cells(&s.update_view(), 440.0));
            assert!(!actions.contains(&Act::Download) && !actions.contains(&Act::UpdateNow));
            assert!(s.start_update_download().is_err());
            assert!(s.update.job.is_none());
            assert!(s.arm_install().unwrap_err().contains("確認中"));
        }
    }

    #[test]
    fn recheck_download_completion_orders_never_accept_another_candidate() {
        for download_first in [false, true] {
            for channel_changed in [false, true] {
                let mut s = Studio::sample();
                crate::update_shots::stage(&mut s, 2);
                let installer = s.update.ready.take().unwrap();
                let mut next = crate::update_shots::newer();
                if channel_changed {
                    next.prerelease = true;
                } else {
                    next.version = "99.0.0".into();
                }
                s.update.channel = Some(false);
                s.update.download_channel = Some(false);
                s.update.manual = true;
                s.update.check = CheckState::Checking;
                let (ask_send, ask_recv) = std::sync::mpsc::channel();
                let (job_send, job_recv) = std::sync::mpsc::channel();
                s.update.asking = Some(std::thread::spawn(move || {
                    ask_recv
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    Ok(Some(next))
                }));
                s.update.job = Some(std::thread::spawn(move || {
                    job_recv
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    Ok(installer)
                }));
                if download_first {
                    job_send.send(()).unwrap();
                    while !s.update.job.as_ref().unwrap().is_finished() {
                        std::thread::yield_now();
                    }
                    s.poll_update();
                    ask_send.send(()).unwrap();
                    finish_manual(&mut s);
                } else {
                    ask_send.send(()).unwrap();
                    finish_manual(&mut s);
                    job_send.send(()).unwrap();
                    while !s.update.job.as_ref().unwrap().is_finished() {
                        std::thread::yield_now();
                    }
                    s.poll_update();
                }
                assert!(
                    s.update.ready.is_none(),
                    "order={download_first}, channel={channel_changed}"
                );
                assert_ne!(s.update.progress().phase, update::Phase::Ready);
            }
        }
    }
    #[test]
    fn manual_distinguishes_missing_release_transport_limit_and_response_errors() {
        for (beta, reply, expected) in [
            (
                false,
                Err("curl: (22) The requested URL returned error: 404"),
                update::NO_RELEASE,
            ),
            (true, Ok("[]"), update::NO_RELEASE),
            (
                false,
                Err("curl: (6) Could not resolve host: api.github.com"),
                "ネットワーク接続",
            ),
            (
                false,
                Err("curl: (22) The requested URL returned error: 403"),
                "GitHub の利用制限",
            ),
            (
                false,
                Err("curl: (22) The requested URL returned error: 429"),
                "GitHub の利用制限",
            ),
            (false, Ok("not json"), "リリースの情報を読み取れません"),
        ] {
            let tmp = scratch("manual-reasons");
            let mut s = Studio::sample();
            s.song.load_settings(tmp.path());
            s.run_command(
                "prefs.set",
                serde_json::json!({"key":update::BETA_PREF,"value":beta.to_string()}),
            );
            s.update.fetch =
                std::sync::Arc::new(move |_| reply.map(str::to_string).map_err(str::to_string));
            s.update.root = Some(tmp.path().into());
            s.manual_update_check();
            finish_manual(&mut s);
            assert!(
                update_cells(&s.update_view(), 440.0)
                    .iter()
                    .map(|cell| cell.text.replace(' ', ""))
                    .collect::<String>()
                    .contains(&expected.replace(' ', "")),
                "{:?}",
                s.update.check
            );
            if expected == update::NO_RELEASE {
                assert_eq!(s.update.check, CheckState::NoRelease);
            }
        }
    }
}
