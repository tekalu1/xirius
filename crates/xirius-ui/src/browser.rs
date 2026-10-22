//! The left browser: a rail of three icons (instruments, effects, files) and a
//! rounded panel next to it, in the shape of mock v4.
//!
//! Like [`crate::studio`], this file has no window in it. [`Browser`] is the
//! state (which tab, which view, what was typed in the search box, which
//! favourites), [`Browser::layout`] says where everything sits for a panel
//! rectangle, and [`Browser::hit`] says what is under a point. `paint` only
//! draws those numbers.
//!
//! The instruments and effects are the built-in ones and what the plugin scan
//! found ([`Browser::set_plugins`]). Until a window hands it a scan, the list is
//! the built-in list ([`LIB`]). The files are the disk's ([`crate::browser_files`]).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use xirius_core::model::PluginKind;
use xirius_plugins::{Catalog, FailureKind};

use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::studio::Rect;

// ------------------------------------------------------------------ the rail

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailTab {
    Instruments,
    Effects,
    Files,
    /// The files the song uses and its recordings (BRW-004).
    SongFiles,
    /// The chosen track's and clip's details (UI-312).
    Inspector,
}

impl RailTab {
    pub const ALL: [RailTab; 5] = [
        RailTab::Instruments,
        RailTab::Effects,
        RailTab::Files,
        RailTab::SongFiles,
        RailTab::Inspector,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            RailTab::Instruments => "keys",
            RailTab::Effects => "effects",
            RailTab::Files => "file",
            RailTab::SongFiles => "pool",
            RailTab::Inspector => "inspector",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RailTab::Instruments => "インストゥルメント",
            RailTab::Effects => "エフェクト",
            RailTab::Files => "ファイル",
            RailTab::SongFiles => "プール",
            RailTab::Inspector => "インスペクター",
        }
    }

    /// The name the commands give it (`view.browser --tab`, `browser.tab --tab`).
    pub fn name(self) -> &'static str {
        match self {
            RailTab::Instruments => "instruments",
            RailTab::Effects => "effects",
            RailTab::Files => "files",
            RailTab::SongFiles => "song_files",
            RailTab::Inspector => "inspector",
        }
    }

    /// The tab a command names (`view.browser --tab`, `browser.tab`).
    pub fn from_name(name: &str) -> Option<RailTab> {
        match name {
            "instruments" => Some(RailTab::Instruments),
            "effects" => Some(RailTab::Effects),
            "files" => Some(RailTab::Files),
            "song_files" => Some(RailTab::SongFiles),
            "inspector" => Some(RailTab::Inspector),
            _ => None,
        }
    }
}

/// The icon buttons of the rail, from the top of `rail`; the settings' gear sits at
/// the bottom ([`rail_io_button`]).
pub fn rail_button(rail: Rect, tab: RailTab) -> Rect {
    let i = RailTab::ALL.iter().position(|t| *t == tab).unwrap_or(0) as f32;
    let size = Layout::ICON_BUTTON;
    Rect::new(
        rail.x + (rail.w - size) / 2.0,
        rail.y + Space::X2.px() + i * (size + Space::X1.px()),
        size,
        size,
    )
}

/// The gear at the bottom of the rail: it opens the settings on 一般 (SET-001).
pub fn rail_io_button(rail: Rect) -> Rect {
    let size = Layout::ICON_BUTTON;
    Rect::new(
        rail.x + (rail.w - size) / 2.0,
        rail.bottom() - Space::X2.px() - size,
        size,
        size,
    )
}

// ---------------------------------------------------------------- the library

/// One instrument or effect in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LibItem {
    pub effect: bool,
    pub name: &'static str,
    pub icon: &'static str,
    pub vendor: &'static str,
    pub format: &'static str,
    pub kind: &'static str,
}

pub const OWN: &str = "Xirius";
const BUILT_IN: &str = "内蔵";

const fn inst(
    name: &'static str,
    icon: &'static str,
    vendor: &'static str,
    format: &'static str,
    kind: &'static str,
) -> LibItem {
    LibItem {
        effect: false,
        name,
        icon,
        vendor,
        format,
        kind,
    }
}

const fn fx(
    name: &'static str,
    vendor: &'static str,
    format: &'static str,
    kind: &'static str,
) -> LibItem {
    LibItem {
        effect: true,
        name,
        icon: "effects",
        vendor,
        format,
        kind,
    }
}

pub const LIB: [LibItem; 7] = [
    inst("Sampler", "keys", OWN, BUILT_IN, "サンプラー"),
    inst("Drum Kit", "drum", OWN, BUILT_IN, "ドラム"),
    fx("EQ", OWN, BUILT_IN, "EQ"),
    fx("Compressor", OWN, BUILT_IN, "ダイナミクス"),
    fx("Limiter", OWN, BUILT_IN, "ダイナミクス"),
    fx("Reverb", OWN, BUILT_IN, "空間"),
    fx("Delay", OWN, BUILT_IN, "空間"),
];

/// A scanned name as `&'static str`, like the built-in ones. Each distinct name
/// is kept once, so scanning again does not grow it.
pub(crate) fn intern(text: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Mutex::default).lock().unwrap();
    if let Some(&t) = names.get(text) {
        return t;
    }
    let t: &'static str = Box::leak(text.to_string().into_boxed_str());
    names.insert(t);
    t
}

/// The group of the kind view, from the VST3 sub-categories (`Fx|Dynamics`).
fn kind_of(category: &str, effect: bool) -> &'static str {
    const KINDS: [(&str, &str); 12] = [
        ("Drum", "ドラム"),
        ("Sampler", "サンプラー"),
        ("Synth", "シンセ"),
        ("Piano", "ピアノ"),
        ("EQ", "EQ"),
        ("Dynamics", "ダイナミクス"),
        ("Reverb", "空間"),
        ("Delay", "空間"),
        ("Spatial", "空間"),
        ("Distortion", "歪み"),
        ("Modulation", "モジュレーション"),
        ("Filter", "フィルター"),
    ];
    let parts: Vec<&str> = category.split('|').collect();
    KINDS
        .iter()
        .find(|(k, _)| parts.iter().any(|p| p.eq_ignore_ascii_case(k)))
        .map_or(
            if effect {
                "その他"
            } else {
                "インストゥルメント"
            },
            |(_, v)| v,
        )
}

/// One row per plugin the scan found.
fn scanned(catalog: &Catalog) -> Vec<LibItem> {
    catalog
        .entries
        .iter()
        .map(|e| {
            let effect = e.kind == PluginKind::Effect;
            LibItem {
                effect,
                name: intern(&e.name),
                icon: if effect { "effects" } else { "keys" },
                vendor: intern(&e.vendor),
                format: intern(&e.format.to_uppercase()),
                kind: kind_of(&e.category, effect),
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Kind,
    Vendor,
    Format,
    Name,
    Favourites,
}

impl View {
    pub const ALL: [View; 5] = [
        View::Kind,
        View::Vendor,
        View::Format,
        View::Name,
        View::Favourites,
    ];
    /// The orders of the song-files tab: by kind under headings, or by name.
    pub const POOL: [View; 2] = [View::Kind, View::Name];

    /// The views a tab offers.
    pub fn of(tab: RailTab) -> &'static [View] {
        match tab {
            RailTab::SongFiles => &View::POOL,
            RailTab::Files | RailTab::Inspector => &[],
            _ => &View::ALL,
        }
    }

    /// The name the commands give it (`view.browser.sort --order`).
    pub fn name(self) -> &'static str {
        match self {
            View::Kind => "kind",
            View::Vendor => "vendor",
            View::Format => "format",
            View::Name => "name",
            View::Favourites => "favourites",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            View::Kind => "種類別",
            View::Vendor => "ベンダー別",
            View::Format => "形式別",
            View::Name => "名前順",
            View::Favourites => "お気に入り",
        }
    }

    pub fn icon(self) -> Option<&'static str> {
        (self == View::Favourites).then_some("star")
    }

    /// Does this view put the items in named groups?
    pub(crate) fn grouped(self) -> bool {
        !matches!(self, View::Name | View::Favourites)
    }
}

fn group_name(item: &LibItem, view: View) -> &'static str {
    match view {
        View::Kind => item.kind,
        View::Vendor => item.vendor,
        View::Format => {
            if item.format == BUILT_IN {
                OWN
            } else {
                item.format
            }
        }
        View::Name | View::Favourites => "",
    }
}

// ------------------------------------------------------------------- the state

#[derive(Clone, Debug, PartialEq)]
pub struct Browser {
    /// Is the panel next to the rail shown?
    pub open: bool,
    pub tab: RailTab,
    pub instrument_view: View,
    pub effect_view: View,
    /// How the song-files tab orders its files: by kind under headings, or by name (BRW-004).
    pub pool_view: View,
    pub search: String,
    /// Is the search box taking key presses?
    pub typing: bool,
    pub favourites: BTreeSet<String>,
    /// Items left out of the list; a search still finds them (CTX-048).
    pub hidden: BTreeSet<String>,
    /// The list shows the hidden items too (BRW-012).
    pub show_hidden: bool,
    /// An instrument being dragged out of the list (CTX-048).
    pub drag: Option<crate::browser_inst::LibDrag>,
    /// A file row being dragged out of the list (UI-046).
    pub file_drag: Option<crate::browser_pick::FileDrag>,
    /// Where the peaks of files outside the song go instead of the user's store (a script's).
    pub peak_store: Option<PathBuf>,
    closed_groups: HashSet<String>,
    /// The places the user added; the song's folder is the tab before them.
    pub places: Vec<xirius_session::places::Place>,
    /// The song's folder, once it is saved.
    pub song_dir: Option<PathBuf>,
    pub loc: usize,
    /// The folder the file list shows, from the location's folder.
    pub path: Vec<String>,
    /// Open folders of the tree, by their paths.
    pub(crate) open_dirs: HashSet<String>,
    pub selected_item: Option<&'static str>,
    pub selected_file: Option<String>,
    pub playing: Option<String>,
    pub scroll: f32,
    /// The built-in items and what the scan found; `None` shows the built-in [`LIB`].
    items: Option<Vec<LibItem>>,
    /// A plugin scan is running: the list says so on top.
    pub scanning: bool,
    /// The scan was asked to stop and is ending.
    pub scan_stopping: bool,
    /// How many bundles the running scan has opened, of how many (0 of 0 until it knows).
    pub scan_done: u64,
    pub scan_total: u64,
    /// The bundles the last scan could not use, for the group at the end of the list.
    pub bad: Vec<BadPlugin>,
    /// The bundle being tried again, while its scan runs.
    pub retrying: Option<PathBuf>,
    /// The item of the list whose plugin is being loaded; it shows the spinner.
    pub loading: Option<crate::loading::Loading>,
    /// How far round (0..1) the spinner is.
    pub spin: f32,
    /// Where what the browser keeps is written; `None` keeps it only here.
    pub(crate) prefs_file: Option<PathBuf>,
    pub(crate) prefs_stamp: Option<SystemTime>,
    /// The file could not be read or written.
    pub prefs_error: Option<String>,
    /// The files of the song's pool, as the pool tab (「プール」) lists them (BRW-004).
    pub pool: Vec<xirius_session::file_ops::Entry>,
    /// The song the pool was read for and when, so it is read again only when the song
    /// changed or a second went by.
    pub(crate) pool_read: Option<(usize, std::time::Instant)>,
    /// The heading of the pool the keys stand on, instead of a file row.
    pub pool_heading: Option<String>,
    /// The clip the pool last followed, so pressing another selects its file's row.
    pub(crate) pool_clip: Option<(xirius_core::id::TrackId, xirius_core::id::ClipId)>,
}

impl Default for Browser {
    fn default() -> Self {
        Browser::new()
    }
}

/// What the browser reports a press as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserHit {
    Search,
    /// The button that opens the view choice.
    ViewButton,
    /// Index into [`BrowserLayout::rows`].
    Group(usize),
    Item(usize),
    Fav(usize),
    Dir(usize),
    File(usize),
    /// The play / stop button of the audition row.
    Audition,
    Loc(usize),
    /// The cross on the selected place's tab: it takes the tab away.
    LocRemove(usize),
    /// The button that adds a place (it asks for a folder).
    LocAdd,
    /// Index into [`BrowserLayout::crumbs`].
    Crumb(usize),
    /// The button that stops the plugin scan, on its row (PLG-021).
    StopScan,
    /// The button that tries an unusable plugin again; index into [`BrowserLayout::rows`].
    Retry(usize),
    /// A file of the song's pool; index into [`BrowserLayout::rows`] (BRW-004).
    Pool(usize),
    /// The count of the clips at the right end of a pool row.
    PoolCount(usize),
    /// The 「…」 beside the search box of the pool.
    PoolMenu,
}

/// A row of the list, as it is shown.
#[derive(Clone, Debug, PartialEq)]
pub enum BRow {
    Group {
        key: String,
        name: &'static str,
        count: usize,
        open: bool,
    },
    Item {
        item: LibItem,
        vendor_text: bool,
        fav: bool,
        selected: bool,
    },
    Dir {
        key: String,
        name: String,
        depth: usize,
        open: bool,
    },
    File {
        key: String,
        name: String,
        /// `0:24`, `8 小節`; empty until it is read.
        len: String,
        depth: usize,
        selected: bool,
    },
    /// Under the selected file: play / stop and the waveform.
    Audition {
        key: String,
        playing: bool,
        depth: usize,
    },
    Note(&'static str),
    /// The plugin scan is running: what it says, how far it is, and a button that stops it.
    Scanning,
    /// A plugin the scan could not use: its name, why, and the button that tries it again.
    Bad {
        path: PathBuf,
        name: String,
        why: &'static str,
        retrying: bool,
    },
    /// A file of the song's pool (BRW-004).
    Pool {
        /// Its path on the disk.
        key: String,
        /// As the song names it (`file.list`'s `file`).
        file: String,
        name: String,
        /// The clips that use it; `None` for a file only a frozen track uses.
        clips: Option<usize>,
        missing: bool,
        external: bool,
        selected: bool,
        /// Its icon: the kind's in the name order, or the alert when it is missing.
        icon: &'static str,
    },
}

/// A bundle the scan could not use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadPlugin {
    pub path: PathBuf,
    pub name: String,
    /// Why, in a few words.
    pub why: &'static str,
}

/// The key of the group of unusable plugins.
const BAD_GROUP: &str = "unusable";
/// What the group of unusable plugins is called.
pub const BAD_TITLE: &str = "使えないプラグイン";
/// The scanning row: the line with the count and the stop button, and the progress line under it.
const SCAN_H: f32 = Layout::BROWSER_ROW_H + 12.0;
const SCAN_BAR_H: f32 = 3.0;
/// An unusable plugin's row: the name over the reason.
const BAD_H: f32 = 40.0;

/// Why a bundle could not be used, in a few words.
pub(crate) fn failure_why(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::Timeout => "応答がありません",
        FailureKind::Crashed => "スキャン中に終了しました",
        FailureKind::Failed => "読み込めませんでした",
    }
}

fn bad_plugins(catalog: &Catalog) -> Vec<BadPlugin> {
    let mut bad: Vec<BadPlugin> = catalog
        .failures
        .iter()
        .map(|f| BadPlugin {
            path: f.path.clone(),
            name: f
                .path
                .file_stem()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            why: failure_why(f.kind),
        })
        .collect();
    bad.sort_by_key(|b| b.name.to_lowercase());
    bad
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub row: BRow,
    /// Top, from the top of the list (before scrolling).
    pub y: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrowserLayout {
    pub panel: Rect,
    pub search: Rect,
    pub view: Option<Rect>,
    /// The 「…」 of the pool, where the view button sits on the other tabs.
    pub more: Option<Rect>,
    pub tabs: Vec<Rect>,
    /// The cross on the selected tab (not on the song's).
    pub remove: Option<Rect>,
    pub add: Option<Rect>,
    pub crumbs: Vec<(String, Rect)>,
    pub list: Rect,
    pub rows: Vec<Placed>,
    pub content_h: f32,
}

/// How wide a text of this size is, close enough for placing buttons.
pub fn text_width(s: &str, size: FontSize) -> f32 {
    let px = size.px();
    s.chars()
        .map(|c| if c.is_ascii() { px * 0.58 } else { px })
        .sum()
}

pub const AUDITION_H: f32 = 32.0;

impl Browser {
    pub fn new() -> Browser {
        Browser {
            open: true,
            tab: RailTab::Instruments,
            instrument_view: View::Kind,
            effect_view: View::Kind,
            pool_view: View::Kind,
            search: String::new(),
            typing: false,
            favourites: xirius_session::places::Prefs::first_run(None).favourites,
            hidden: BTreeSet::new(),
            show_hidden: false,
            drag: None,
            file_drag: None,
            peak_store: None,
            closed_groups: HashSet::new(),
            places: Vec::new(),
            song_dir: None,
            loc: 0,
            path: Vec::new(),
            open_dirs: HashSet::new(),
            selected_item: None,
            selected_file: None,
            playing: None,
            scroll: 0.0,
            items: None,
            scanning: false,
            scan_stopping: false,
            scan_done: 0,
            scan_total: 0,
            bad: Vec::new(),
            retrying: None,
            loading: None,
            spin: 0.0,
            prefs_file: None,
            prefs_stamp: None,
            prefs_error: None,
            pool: Vec::new(),
            pool_read: None,
            pool_heading: None,
            pool_clip: None,
        }
    }

    /// The list is the built-in items and these plugins from now on.
    pub fn set_plugins(&mut self, catalog: &Catalog) {
        let mut items: Vec<LibItem> = LIB
            .iter()
            .filter(|x| x.format == BUILT_IN)
            .copied()
            .collect();
        items.extend(scanned(catalog));
        self.items = Some(items);
        self.bad = bad_plugins(catalog);
    }

    pub fn view(&self) -> View {
        match self.tab {
            RailTab::Effects => self.effect_view,
            RailTab::SongFiles => self.pool_view,
            _ => self.instrument_view,
        }
    }

    pub fn set_view(&mut self, view: View) {
        match self.tab {
            RailTab::Effects => self.effect_view = view,
            RailTab::SongFiles => self.pool_view = view,
            _ => self.instrument_view = view,
        }
        self.scroll = 0.0;
    }

    /// A press on a rail button: a second press on the open tab shuts the panel.
    pub fn press_rail(&mut self, tab: RailTab) {
        if self.open && self.tab == tab {
            self.open = false;
            self.typing = false;
        } else {
            self.open = true;
            self.tab = tab;
            self.search.clear();
            self.typing = false;
            self.scroll = 0.0;
        }
    }

    pub(crate) fn query(&self) -> String {
        self.search.trim().to_lowercase()
    }

    /// The instruments or effects the list shows now.
    pub fn library(&self) -> Vec<LibItem> {
        let q = self.query();
        let effects = self.tab == RailTab::Effects;
        let mut items: Vec<LibItem> = self
            .items
            .as_deref()
            .unwrap_or(&LIB)
            .iter()
            .filter(|x| x.effect == effects)
            .filter(|x| !q.is_empty() || self.show_hidden || !self.hidden.contains(x.name))
            .filter(|x| {
                q.is_empty()
                    || format!("{} {} {}", x.name, x.vendor, x.kind)
                        .to_lowercase()
                        .contains(&q)
            })
            .copied()
            .collect();
        if self.view() == View::Favourites {
            items.retain(|x| self.favourites.contains(x.name));
        }
        items.sort_by_key(|x| x.name);
        items
    }

    // ------------------------------------------------------------------ rows

    /// The rows of the list, with no positions yet.
    pub fn rows(&self) -> Vec<BRow> {
        match self.tab {
            RailTab::Files => self.file_rows(),
            RailTab::SongFiles => self.pool_rows(),
            // The inspector is not a list (UI-312).
            RailTab::Inspector => Vec::new(),
            _ => {
                let mut rows = Vec::new();
                if self.scanning {
                    rows.push(BRow::Scanning);
                }
                rows.extend(self.library_rows());
                rows.extend(self.bad_rows());
                rows
            }
        }
    }

    /// The group of unusable plugins at the end of the list: none while a full scan runs (the
    /// list is being made again), while searching, or when the scan found nothing wrong.
    fn bad_rows(&self) -> Vec<BRow> {
        if self.bad.is_empty()
            || !self.query().is_empty()
            || (self.scanning && self.retrying.is_none())
        {
            return Vec::new();
        }
        let open = !self.closed_groups.contains(BAD_GROUP);
        let mut rows = vec![BRow::Group {
            key: BAD_GROUP.to_string(),
            name: BAD_TITLE,
            count: self.bad.len(),
            open,
        }];
        if open {
            rows.extend(self.bad.iter().map(|b| BRow::Bad {
                path: b.path.clone(),
                name: b.name.clone(),
                why: b.why,
                retrying: self.retrying.as_ref() == Some(&b.path),
            }));
        }
        rows
    }

    fn library_rows(&self) -> Vec<BRow> {
        let view = self.view();
        let items = self.library();
        let q = self.query();
        let item_row = |item: LibItem| BRow::Item {
            item,
            vendor_text: view != View::Vendor,
            fav: self.favourites.contains(item.name),
            selected: self.selected_item == Some(item.name),
        };
        if items.is_empty() {
            return vec![BRow::Note(if q.is_empty() {
                "まだありません"
            } else {
                "見つかりません"
            })];
        }
        if !view.grouped() {
            return items.into_iter().map(item_row).collect();
        }
        let mut groups: BTreeMap<&'static str, Vec<LibItem>> = BTreeMap::new();
        for item in items {
            groups
                .entry(group_name(&item, view))
                .or_default()
                .push(item);
        }
        let mut rows = Vec::new();
        for (name, items) in groups {
            let key = format!("{:?}/{:?}/{name}", self.tab, view);
            // A search opens every group that has a hit.
            let open = !q.is_empty() || !self.closed_groups.contains(&key);
            rows.push(BRow::Group {
                key,
                name,
                count: items.len(),
                open,
            });
            if open {
                rows.extend(items.into_iter().map(item_row));
            }
        }
        rows
    }

    /// Whether a group of the list is folded.
    pub(crate) fn is_folded(&self, key: &str) -> bool {
        self.closed_groups.contains(key)
    }

    /// Toggles a group of the grouped list.
    pub fn toggle_group(&mut self, key: &str) {
        if !self.closed_groups.remove(key) {
            self.closed_groups.insert(key.to_string());
        }
    }

    /// The groups the list has now (none in a flat view or on the file tab).
    pub fn groups(&self) -> Vec<String> {
        let view = self.view();
        if self.tab == RailTab::Files || !view.grouped() {
            return Vec::new();
        }
        let names: BTreeSet<&'static str> =
            self.library().iter().map(|x| group_name(x, view)).collect();
        names
            .into_iter()
            .map(|n| format!("{:?}/{:?}/{n}", self.tab, view))
            .collect()
    }

    /// Opens (`true`) or folds every group of the list.
    pub fn fold_groups(&mut self, open: bool) {
        for key in self.groups() {
            if open {
                self.closed_groups.remove(&key);
            } else {
                self.closed_groups.insert(key);
            }
        }
    }

    // ---------------------------------------------------------------- layout

    pub fn layout(&self, panel: Rect) -> BrowserLayout {
        let pad = Space::X2.px();
        let files = self.tab == RailTab::Files;
        let pool = self.tab == RailTab::SongFiles;
        let button = Layout::ICON_BUTTON;
        let mut y = panel.y + pad;
        let corner = Rect::new(panel.right() - pad - button, y, button, button);
        // The song-files tab has its 「…」 in the corner and the order's button before it.
        let beside = Rect::new(corner.x - Space::X1.px() - button, y, button, button);
        let view = (!files).then_some(if pool { beside } else { corner });
        let more = pool.then_some(corner);
        let buttons = if files {
            0.0
        } else if pool {
            2.0
        } else {
            1.0
        };
        let search_w = panel.w - pad * 2.0 - buttons * (button + Space::X1.px());
        let search = Rect::new(panel.x + pad, y, search_w.max(0.0), button);
        y += button + Space::X1.px();

        let mut tabs = Vec::new();
        let mut add = None;
        let mut remove = None;
        let mut crumbs = Vec::new();
        if files {
            // The tabs share the width when they are too many for it; the plus is last.
            let locs = self.locations();
            let gap = Space::X1.px();
            let room = (panel.w - pad * 2.0 - 24.0 - gap * locs.len() as f32).max(0.0);
            let fair = room / locs.len() as f32;
            let mut x = panel.x + pad;
            for (i, l) in locs.iter().enumerate() {
                let cross = if i == self.loc && !l.song {
                    TAB_CROSS
                } else {
                    0.0
                };
                let w = (text_width(&l.name, FontSize::Caption) + pad + cross)
                    .min(fair.max(cross + pad));
                let r = Rect::new(x, y, w, Layout::BROWSER_ROW_H);
                if cross > 0.0 {
                    remove = Some(Rect::new(
                        r.right() - cross - 2.0,
                        y + (r.h - cross) / 2.0,
                        cross,
                        cross,
                    ));
                }
                tabs.push(r);
                x += w + gap;
            }
            add = Some(Rect::new(x, y, 24.0, Layout::BROWSER_ROW_H));
            y += Layout::BROWSER_ROW_H + Space::X1.px();
            if !self.path.is_empty() {
                let mut x = panel.x + pad;
                let names = std::iter::once(self.location().name).chain(self.path.clone());
                for (i, n) in names.enumerate() {
                    let w = text_width(&n, FontSize::Caption) + Space::X2.px();
                    if i > 0 {
                        x += Layout::ICON_SIZE - 4.0;
                    }
                    crumbs.push((n, Rect::new(x, y, w, 24.0)));
                    x += w;
                }
                y += 24.0 + Space::X1.px();
            }
        }
        let list = Rect::new(panel.x, y, panel.w, (panel.bottom() - y).max(0.0));

        let mut rows = Vec::new();
        let mut at = 0.0;
        for row in self.rows() {
            let h = match &row {
                BRow::Group { .. } => Layout::BROWSER_GROUP_H,
                BRow::Audition { .. } => AUDITION_H,
                BRow::Scanning => SCAN_H,
                BRow::Bad { .. } => BAD_H,
                _ => Layout::BROWSER_ROW_H,
            };
            rows.push(Placed { row, y: at, h });
            at += h;
        }
        BrowserLayout {
            panel,
            search,
            view,
            more,
            tabs,
            remove,
            add,
            crumbs,
            list,
            rows,
            content_h: at,
        }
    }

    pub fn max_scroll(&self, l: &BrowserLayout) -> f32 {
        (l.content_h - l.list.h).max(0.0)
    }

    pub fn clamp_scroll(&mut self, l: &BrowserLayout) {
        self.scroll = self.scroll.clamp(0.0, self.max_scroll(l));
    }

    /// Where a row sits on screen.
    pub fn row_rect(&self, l: &BrowserLayout, p: &Placed) -> Rect {
        Rect::new(l.list.x, l.list.y + p.y - self.scroll, l.list.w, p.h)
    }

    // -------------------------------------------------------------- hit test

    pub fn hit(&self, l: &BrowserLayout, x: f32, y: f32) -> Option<BrowserHit> {
        if !l.panel.contains(x, y) {
            return None;
        }
        if l.search.contains(x, y) {
            return Some(BrowserHit::Search);
        }
        if l.view.is_some_and(|r| r.contains(x, y)) {
            return Some(BrowserHit::ViewButton);
        }
        if l.more.is_some_and(|r| r.contains(x, y)) {
            return Some(BrowserHit::PoolMenu);
        }
        if l.remove.is_some_and(|r| r.contains(x, y)) {
            return Some(BrowserHit::LocRemove(self.loc));
        }
        if let Some(i) = l.tabs.iter().position(|r| r.contains(x, y)) {
            return Some(BrowserHit::Loc(i));
        }
        if l.add.is_some_and(|r| r.contains(x, y)) {
            return Some(BrowserHit::LocAdd);
        }
        if let Some(i) = l.crumbs.iter().position(|(_, r)| r.contains(x, y)) {
            return Some(BrowserHit::Crumb(i));
        }
        if !l.list.contains(x, y) {
            return None;
        }
        for (i, p) in l.rows.iter().enumerate() {
            let r = self.row_rect(l, p);
            if !r.contains(x, y) {
                continue;
            }
            return match &p.row {
                BRow::Group { .. } => Some(BrowserHit::Group(i)),
                BRow::Item { .. } => {
                    if self.fav_rect(r).contains(x, y) {
                        Some(BrowserHit::Fav(i))
                    } else {
                        Some(BrowserHit::Item(i))
                    }
                }
                BRow::Dir { .. } => Some(BrowserHit::Dir(i)),
                BRow::File { .. } => Some(BrowserHit::File(i)),
                BRow::Audition { depth, .. } => {
                    let (play, _) = self.audition_parts(r, *depth);
                    play.contains(x, y).then_some(BrowserHit::Audition)
                }
                BRow::Note(_) => None,
                BRow::Scanning => self
                    .stop_rect(r)
                    .contains(x, y)
                    .then_some(BrowserHit::StopScan),
                BRow::Bad { retrying, .. } => {
                    (!retrying && self.retry_rect(r).contains(x, y)).then_some(BrowserHit::Retry(i))
                }
                BRow::Pool { .. } => Some(
                    if self
                        .pool_count_rect(r, &p.row)
                        .is_some_and(|c| c.contains(x, y))
                    {
                        BrowserHit::PoolCount(i)
                    } else {
                        BrowserHit::Pool(i)
                    },
                ),
            };
        }
        None
    }

    /// The stop button at the right end of the scanning row's top line; nothing once it is
    /// stopping.
    pub fn stop_rect(&self, row: Rect) -> Rect {
        if self.scan_stopping {
            return Rect::new(row.right(), row.y, 0.0, 0.0);
        }
        let w = text_width(SCAN_STOP, FontSize::Caption) + Space::X2.px() * 2.0;
        let h = Layout::BROWSER_ROW_H - Space::X1.px();
        Rect::new(
            row.right() - Space::X1.px() - w,
            row.y + (Layout::BROWSER_ROW_H - h) / 2.0,
            w,
            h,
        )
    }

    /// What the scanning row says.
    pub fn scanning_text(&self) -> &'static str {
        if self.scan_stopping {
            "中止しています…"
        } else if self.scan_total == 0 {
            "検索中"
        } else {
            "スキャン中"
        }
    }

    /// `12 / 48` once the scan knows how many bundles it has to open.
    pub fn scan_count(&self) -> Option<String> {
        (self.scan_total > 0 && !self.scan_stopping).then(|| {
            format!(
                "{} / {}",
                self.scan_done.min(self.scan_total),
                self.scan_total
            )
        })
    }

    /// The line of the scanning row that shows how far the scan is, under its text.
    pub fn scan_bar(&self, row: Rect) -> Rect {
        Rect::new(
            row.x + Space::X2.px(),
            row.bottom() - Space::X1.px() - SCAN_BAR_H,
            (row.w - Space::X2.px() * 2.0).max(0.0),
            SCAN_BAR_H,
        )
    }

    /// The button at the right end of an unusable plugin's row.
    pub fn retry_rect(&self, row: Rect) -> Rect {
        let s = 28.0;
        Rect::new(
            row.right() - Space::X2.px() - s,
            row.y + (row.h - s) / 2.0,
            s,
            s,
        )
    }

    /// The star at the right end of an item row.
    pub fn fav_rect(&self, row: Rect) -> Rect {
        let s = Layout::ICON_SIZE + Space::X1.px();
        Rect::new(
            row.right() - Space::X2.px() - s,
            row.y + (row.h - s) / 2.0,
            s,
            s,
        )
    }

    /// The play button and the waveform of an audition row.
    pub fn audition_parts(&self, row: Rect, depth: usize) -> (Rect, Rect) {
        let x = row.x + Self::indent(depth) + Layout::ICON_SIZE + Space::X2.px();
        let play = Rect::new(x, row.y + 4.0, 24.0, row.h - 8.0);
        let wave = Rect::new(
            play.right() + Space::X2.px(),
            row.y + 6.0,
            (row.right() - Space::X2.px() - play.right() - Space::X2.px()).max(0.0),
            row.h - 12.0,
        );
        (play, wave)
    }

    pub fn indent(depth: usize) -> f32 {
        Space::X2.px() + depth as f32 * 14.0
    }

    /// The centre of a part, for aimed input.
    pub fn center_of(&self, l: &BrowserLayout, hit: BrowserHit) -> Option<(f32, f32)> {
        let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let row = |i: usize| l.rows.get(i).map(|p| self.row_rect(l, p));
        let at = match hit {
            BrowserHit::Search => Some(mid(l.search)),
            BrowserHit::ViewButton => l.view.map(mid),
            BrowserHit::PoolMenu => l.more.map(mid),
            BrowserHit::Pool(i) => row(i).map(|r| (r.x + r.w / 3.0, r.y + r.h / 2.0)),
            BrowserHit::PoolCount(i) => l
                .rows
                .get(i)
                .and_then(|p| self.pool_count_rect(self.row_rect(l, p), &p.row))
                .map(mid),
            BrowserHit::Loc(i) => l.tabs.get(i).copied().map(mid),
            BrowserHit::LocRemove(i) => l.remove.filter(|_| i == self.loc).map(mid),
            BrowserHit::LocAdd => l.add.map(mid),
            BrowserHit::Crumb(i) => l.crumbs.get(i).map(|(_, r)| mid(*r)),
            BrowserHit::Group(i) | BrowserHit::Dir(i) | BrowserHit::File(i) => row(i).map(mid),
            BrowserHit::Item(i) => row(i).map(|r| (r.x + r.w / 3.0, r.y + r.h / 2.0)),
            BrowserHit::Fav(i) => row(i).map(|r| mid(self.fav_rect(r))),
            BrowserHit::Audition => l.rows.iter().find_map(|p| match p.row {
                BRow::Audition { depth, .. } => {
                    Some(mid(self.audition_parts(self.row_rect(l, p), depth).0))
                }
                _ => None,
            }),
            BrowserHit::Retry(i) => row(i).map(|r| mid(self.retry_rect(r))),
            BrowserHit::StopScan => l
                .rows
                .iter()
                .find(|p| p.row == BRow::Scanning)
                .map(|p| self.stop_rect(self.row_rect(l, p)))
                .filter(|r| r.w > 0.0)
                .map(mid),
        }?;
        // On screen and not scrolled out of the list.
        let inside = l.list.contains(at.0, at.1)
            || matches!(
                hit,
                BrowserHit::Search
                    | BrowserHit::ViewButton
                    | BrowserHit::PoolMenu
                    | BrowserHit::Loc(_)
                    | BrowserHit::LocRemove(_)
                    | BrowserHit::LocAdd
                    | BrowserHit::Crumb(_)
            );
        inside.then_some(at)
    }

    // ----------------------------------------------------------------- press

    /// Applies a press. Returns what the screen has to do besides redrawing.
    pub fn press(&mut self, l: &BrowserLayout, hit: BrowserHit) -> BrowserEffect {
        self.typing = hit == BrowserHit::Search;
        let row = |i: usize| l.rows.get(i).map(|p| p.row.clone());
        match hit {
            BrowserHit::Search => {}
            BrowserHit::ViewButton => return BrowserEffect::ChooseView,
            BrowserHit::Group(i) => {
                if let Some(BRow::Group { key, .. }) = row(i) {
                    self.toggle_group(&key);
                }
            }
            BrowserHit::Item(i) => {
                if let Some(BRow::Item { item, .. }) = row(i) {
                    self.selected_item = Some(item.name);
                }
            }
            BrowserHit::Fav(i) => {
                if let Some(BRow::Item { item, .. }) = row(i) {
                    self.toggle_favourite(item.name);
                }
            }
            BrowserHit::Dir(i) => {
                if let Some(BRow::Dir { key, .. }) = row(i) {
                    self.toggle_dir(&key);
                }
            }
            BrowserHit::File(i) => {
                if let Some(BRow::File { key, .. }) = row(i) {
                    self.select_file(&key);
                }
            }
            BrowserHit::Audition => self.toggle_audition(),
            BrowserHit::PoolMenu => return BrowserEffect::PoolMenu,
            BrowserHit::Pool(i) | BrowserHit::PoolCount(i) => {
                if let Some(BRow::Pool { key, missing, .. }) = row(i) {
                    self.select_pool(&key, missing);
                }
            }
            BrowserHit::Loc(i) => self.set_location(i),
            BrowserHit::LocRemove(i) => {
                if let Err(e) = self.remove_place(i) {
                    self.prefs_error = Some(e);
                }
            }
            BrowserHit::LocAdd => return BrowserEffect::AddPlace,
            BrowserHit::Crumb(i) => self.go_up_to(i),
            BrowserHit::StopScan => return BrowserEffect::StopScan,
            BrowserHit::Retry(i) => {
                if let Some(BRow::Bad { path, .. }) = row(i)
                    && let Some(at) = self.bad.iter().position(|b| b.path == path)
                {
                    return BrowserEffect::Retry(at);
                }
            }
        }
        BrowserEffect::None
    }

    /// A typed character or key in the search box. Returns true when it was used.
    pub fn typed(&mut self, key: &str, ch: Option<&str>) -> bool {
        if !self.typing {
            return false;
        }
        match key {
            "escape" => {
                self.search.clear();
                self.typing = false;
            }
            "enter" => self.typing = false,
            "backspace" => {
                self.search.pop();
            }
            _ => match ch {
                Some(c) if !c.is_empty() && !c.chars().any(char::is_control) => {
                    self.search.push_str(c)
                }
                _ => return false,
            },
        }
        self.scroll = 0.0;
        true
    }
}

/// What a press asks of the screen besides a redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserEffect {
    None,
    /// Open the view choice under the view button.
    ChooseView,
    /// Stop the plugin scan.
    StopScan,
    /// Scan this unusable plugin (an index into [`Browser::bad`]) again.
    Retry(usize),
    /// Ask for a folder to add as a place.
    AddPlace,
    /// Open the menu of the pool's 「…」.
    PoolMenu,
}

/// The width of the cross on the selected place's tab.
pub const TAB_CROSS: f32 = 16.0;

/// The label of the scanning row's stop button.
const SCAN_STOP: &str = "中止";

// -------------------------------------------------------------------- painting

/// The rail and the panel, drawn into the rectangles the geometry settled.
pub fn paint(pen: &mut Pen, browser: &Browser, hover: Option<BrowserHit>, rail: Rect, panel: Rect) {
    let c = pen.c;
    pen.round(rail, c.surface_2, Radius::M);
    for tab in RailTab::ALL {
        let r = rail_button(rail, tab);
        let active = browser.open && browser.tab == tab;
        if active {
            pen.round(r, c.sel, Radius::S);
        }
        let ink = if active { c.ink_strong } else { c.ink };
        pen.icon(tab.icon(), icon_box(r), ink);
        if tab == RailTab::SongFiles && browser.pool_missing() > 0 {
            // A file the song uses is not found: a dot in the warning colour on the corner.
            let ring = if active { c.sel } else { c.surface_2 };
            let dot = Rect::new(r.right() - 10.0, r.y + 3.0, 7.0, 7.0);
            pen.round(
                Rect::new(dot.x - 2.0, dot.y - 2.0, dot.w + 4.0, dot.h + 4.0),
                ring,
                Radius::Pill,
            );
            pen.round(dot, c.ink_alert, Radius::Pill);
        }
    }
    pen.icon("gear", icon_box(rail_io_button(rail)), c.ink);
    if !browser.open || panel.w <= 0.0 {
        return;
    }
    // The inspector draws the panel itself (UI-312).
    if browser.tab == RailTab::Inspector {
        return;
    }
    pen.round(panel, c.surface_2, Radius::M);
    let l = browser.layout(panel);
    pen.mask(panel, |pen| paint_panel(pen, browser, hover, &l));
}

fn icon_box(r: Rect) -> Rect {
    Rect::new(
        r.x + (r.w - Layout::ICON_SIZE) / 2.0,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    )
}

fn paint_panel(pen: &mut Pen, browser: &Browser, hover: Option<BrowserHit>, l: &BrowserLayout) {
    let c = pen.c;
    // The search box: a sunken well with the magnifier and the typed text.
    pen.round(l.search, c.well, Radius::S);
    let lens = Rect::new(
        l.search.x + Space::X2.px(),
        l.search.y + (l.search.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    );
    pen.icon("search", lens, c.ink);
    let text_r = Rect::new(
        lens.right() + Space::X2.px(),
        l.search.y,
        l.search.w - lens.w - Space::X2.px() * 3.0,
        l.search.h,
    );
    let (shown, ink) = if browser.search.is_empty() && !browser.typing {
        ("検索", c.ink)
    } else {
        (browser.search.as_str(), c.ink_strong)
    };
    let w = pen.text(
        shown,
        FontSize::Small,
        Weight::Regular,
        ink,
        text_r,
        Align::Left,
    );
    if browser.typing {
        let caret_x = text_r.x + if browser.search.is_empty() { 0.0 } else { w } + 1.0;
        pen.fill(
            Rect::new(caret_x, l.search.y + 7.0, 1.5, l.search.h - 14.0),
            c.ink_strong,
        );
    }
    if let Some(v) = l.view {
        if hover == Some(BrowserHit::ViewButton) {
            pen.round(v, c.hover, Radius::S);
        }
        pen.icon("view", icon_box(v), c.ink_strong);
    }
    if let Some(m) = l.more {
        if hover == Some(BrowserHit::PoolMenu) {
            pen.round(m, c.hover, Radius::S);
        }
        pen.icon("more", icon_box(m), c.ink_strong);
    }

    if browser.tab == RailTab::Files {
        let locs = browser.locations();
        for (i, r) in l.tabs.iter().enumerate() {
            let on = browser.loc == i;
            let hot = hover == Some(BrowserHit::Loc(i));
            if on {
                pen.round(*r, c.sel, Radius::S);
            } else if hot {
                pen.round(*r, c.hover, Radius::S);
            }
            let text_r = match l.remove.filter(|_| on) {
                Some(x) => Rect::new(r.x, r.y, x.x - r.x, r.h),
                None => *r,
            };
            pen.mask(text_r, |pen| {
                pen.text(
                    &locs[i].name,
                    FontSize::Caption,
                    Weight::Medium,
                    if on || hot {
                        pen.c.ink_strong
                    } else {
                        pen.c.ink
                    },
                    text_r,
                    Align::Center,
                );
            });
        }
        if let Some(x) = l.remove {
            let hot = hover == Some(BrowserHit::LocRemove(browser.loc));
            if hot {
                pen.round(x, c.hover, Radius::Xs);
            }
            pen.icon(
                "close",
                Rect::new(x.x + 2.0, x.y + 2.0, x.w - 4.0, x.h - 4.0),
                if hot { c.ink_strong } else { c.ink },
            );
        }
        if let Some(a) = l.add {
            if hover == Some(BrowserHit::LocAdd) {
                pen.round(a, c.hover, Radius::S);
            }
            pen.icon("plus", icon_box(a), c.ink_strong);
        }
        for (i, (name, r)) in l.crumbs.iter().enumerate() {
            if i > 0 {
                let chev = Rect::new(r.x - Layout::ICON_SIZE + 2.0, r.y + 4.0, 16.0, 16.0);
                pen.icon("chevron-right", chev, c.ink_on_toggle);
            }
            let last = i + 1 == l.crumbs.len();
            let hot = hover == Some(BrowserHit::Crumb(i));
            pen.text(
                name,
                FontSize::Caption,
                Weight::Medium,
                if last || hot { c.ink_strong } else { c.ink },
                *r,
                Align::Center,
            );
        }
    }

    pen.mask(l.list, |pen| {
        for (i, p) in l.rows.iter().enumerate() {
            let r = browser.row_rect(l, p);
            if r.bottom() < l.list.y || r.y > l.list.bottom() {
                continue;
            }
            paint_row(pen, browser, hover, i, p, r);
        }
    });
}

fn paint_row(
    pen: &mut Pen,
    browser: &Browser,
    hover: Option<BrowserHit>,
    i: usize,
    p: &Placed,
    r: Rect,
) {
    let c = pen.c;
    let pad = Space::X2.px();
    let inner = Rect::new(r.x + Space::X1.px(), r.y, r.w - Space::X1.px() * 2.0, r.h);
    let row_hot = matches!(
        hover,
        Some(BrowserHit::Item(j) | BrowserHit::Group(j) | BrowserHit::Dir(j) | BrowserHit::File(j) | BrowserHit::Fav(j) | BrowserHit::Pool(j) | BrowserHit::PoolCount(j))
            if j == i
    );
    let icon_at = |x: f32| Rect::new(x, r.y + (r.h - 14.0) / 2.0, 14.0, 14.0);
    match &p.row {
        BRow::Pool { .. } => crate::browser_pool::paint_row(pen, browser, &p.row, r, row_hot),
        BRow::Group {
            key,
            name,
            count,
            open,
        } => {
            if browser.pool_heading.as_deref() == Some(key.as_str()) {
                // The keys stand on this heading of the pool.
                pen.round(inner, c.sel, Radius::S);
            }
            let chev = if *open {
                "chevron-down"
            } else {
                "chevron-right"
            };
            pen.icon(chev, icon_at(r.x + pad), c.ink);
            let w = pen.text(
                name,
                FontSize::Caption,
                Weight::Semibold,
                if row_hot { c.ink_strong } else { c.ink },
                Rect::new(r.x + pad + 14.0 + Space::X2.px(), r.y, r.w, r.h),
                Align::Left,
            );
            // The count in a small chip after the name.
            let chip = Rect::new(
                r.x + pad + 14.0 + Space::X2.px() + w + Space::X2.px(),
                r.y + (r.h - 16.0) / 2.0,
                18.0,
                16.0,
            );
            let on_sel = browser.pool_heading.as_deref() == Some(key.as_str());
            pen.round(
                chip,
                if on_sel { c.surface_1 } else { c.surface_3 },
                Radius::Clip,
            );
            pen.digits(
                &count.to_string(),
                FontSize::Micro,
                Weight::Semibold,
                c.ink_strong,
                chip,
                Align::Center,
            );
        }
        BRow::Item {
            item,
            vendor_text,
            fav,
            selected,
        } => {
            if *selected {
                pen.round(inner, c.sel, Radius::S);
            } else if row_hot {
                pen.round(inner, c.hover, Radius::S);
            }
            let x = r.x + pad + 14.0 + Space::X2.px();
            pen.icon(item.icon, icon_at(x), c.ink_strong);
            let name_x = x + 14.0 + Space::X2.px();
            let star = browser.fav_rect(r);
            let name_r = Rect::new(name_x, r.y, star.x - name_x, r.h);
            pen.mask(name_r, |pen| {
                let w = pen.text(
                    item.name,
                    FontSize::Small,
                    Weight::Medium,
                    pen.c.ink_strong,
                    name_r,
                    Align::Left,
                );
                if *vendor_text {
                    pen.text(
                        item.vendor,
                        FontSize::Caption,
                        Weight::Regular,
                        pen.c.ink,
                        Rect::new(name_x + w + Space::X2.px(), r.y, name_r.w, r.h),
                        Align::Left,
                    );
                }
            });
            if browser
                .loading
                .as_ref()
                .is_some_and(|l| l.name == item.name)
            {
                // The plugin is being loaded: the spinner takes the star's place.
                pen.icon_turned("spinner", icon_box(star), c.ink_strong, browser.spin);
            } else {
                let star_hot = hover == Some(BrowserHit::Fav(i));
                pen.icon(
                    if *fav { "star-fill" } else { "star" },
                    icon_box(star),
                    if *fav || star_hot {
                        c.ink_strong
                    } else {
                        c.ink
                    },
                );
            }
        }
        BRow::Dir {
            name, depth, open, ..
        } => {
            if row_hot {
                pen.round(inner, c.hover, Radius::S);
            }
            let x = r.x + Browser::indent(*depth);
            pen.icon(
                if *open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                icon_at(x),
                c.ink,
            );
            pen.icon("folder", icon_at(x + 14.0 + Space::X1.px()), c.ink_strong);
            pen.text(
                name,
                FontSize::Small,
                Weight::Medium,
                c.ink_strong,
                Rect::new(x + 28.0 + Space::X2.px(), r.y, r.w, r.h),
                Align::Left,
            );
        }
        BRow::File {
            name,
            len,
            depth,
            selected,
            ..
        } => {
            if *selected {
                pen.round(inner, c.sel, Radius::S);
            } else if row_hot {
                pen.round(inner, c.hover, Radius::S);
            }
            let x = r.x + Browser::indent(*depth) + 18.0;
            let midi = crate::browser_files::is_midi(std::path::Path::new(name));
            pen.icon(if midi { "keys" } else { "wave" }, icon_at(x), c.ink_strong);
            let len_w = text_width(len, FontSize::Caption) + pad;
            let name_r = Rect::new(
                x + 14.0 + Space::X2.px(),
                r.y,
                r.right() - len_w - pad - x - 22.0,
                r.h,
            );
            pen.mask(name_r, |pen| {
                pen.text(
                    name,
                    FontSize::Small,
                    Weight::Regular,
                    pen.c.ink_strong,
                    name_r,
                    Align::Left,
                );
            });
            pen.digits(
                len,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(r.right() - len_w - pad, r.y, len_w, r.h),
                Align::Right,
            );
        }
        BRow::Audition {
            playing,
            depth,
            key,
        } => {
            pen.round(inner, c.sel, Radius::S);
            let (play, wave) = browser.audition_parts(r, *depth);
            let hot = hover == Some(BrowserHit::Audition);
            pen.round(play, if hot { c.fill_hover } else { c.fill }, Radius::Pill);
            pen.icon(
                if *playing { "stop" } else { "play" },
                Rect::new(play.x + 4.0, play.y + (play.h - 16.0) / 2.0, 16.0, 16.0),
                c.on_fill,
            );
            crate::audition::paint_wave(pen, browser, key, wave, *playing);
        }
        BRow::Note(text) => {
            pen.text(
                text,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                Rect::new(r.x + pad, r.y, r.w, r.h),
                Align::Left,
            );
        }
        BRow::Bad {
            name,
            why,
            retrying,
            ..
        } => {
            let retry = browser.retry_rect(r);
            let icon_r = Rect::new(r.x + pad, r.y + (r.h - 14.0) / 2.0, 14.0, 14.0);
            pen.icon("alert", icon_r, c.ink);
            let x = icon_r.right() + Space::X2.px();
            let text_r =
                |y: f32, h: f32| Rect::new(x, y, (retry.x - x - Space::X1.px()).max(0.0), h);
            pen.mask(text_r(r.y, r.h), |pen| {
                pen.text(
                    name,
                    FontSize::Small,
                    Weight::Medium,
                    pen.c.ink_strong,
                    text_r(r.y + 4.0, 16.0),
                    Align::Left,
                );
                pen.text(
                    why,
                    FontSize::Caption,
                    Weight::Regular,
                    pen.c.ink,
                    text_r(r.y + 20.0, 16.0),
                    Align::Left,
                );
            });
            let hot = hover == Some(BrowserHit::Retry(i));
            if hot && !*retrying {
                pen.round(retry, c.hover, Radius::S);
            }
            let glyph = Rect::new(
                retry.x + (retry.w - Layout::ICON_SIZE) / 2.0,
                retry.y + (retry.h - Layout::ICON_SIZE) / 2.0,
                Layout::ICON_SIZE,
                Layout::ICON_SIZE,
            );
            if *retrying {
                pen.icon_turned("spinner", glyph, c.ink_strong, browser.spin);
            } else {
                pen.icon("restore", glyph, if hot { c.ink_strong } else { c.ink });
            }
        }
        BRow::Scanning => {
            let stop = browser.stop_rect(r);
            let spin = Rect::new(
                r.x + pad,
                r.y + (Layout::BROWSER_ROW_H - 14.0) / 2.0,
                14.0,
                14.0,
            );
            pen.icon_turned("spinner", spin, c.ink_strong, browser.spin);
            let text_x = spin.right() + Space::X2.px();
            let w = pen.text(
                browser.scanning_text(),
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                Rect::new(
                    text_x,
                    r.y,
                    (stop.x - text_x).max(0.0),
                    Layout::BROWSER_ROW_H,
                ),
                Align::Left,
            );
            if let Some(count) = browser.scan_count() {
                pen.digits(
                    &count,
                    FontSize::Caption,
                    Weight::Regular,
                    c.ink,
                    Rect::new(
                        text_x + w + Space::X2.px(),
                        r.y,
                        (stop.x - text_x - w - Space::X2.px()).max(0.0),
                        Layout::BROWSER_ROW_H,
                    ),
                    Align::Left,
                );
            }
            // How far it is: a thin line that fills with the bundles opened.
            let bar = browser.scan_bar(r);
            pen.round(bar, c.surface_3, Radius::Pill);
            if browser.scan_total > 0 {
                let done =
                    browser.scan_done.min(browser.scan_total) as f32 / browser.scan_total as f32;
                if done > 0.0 {
                    pen.round(
                        Rect::new(bar.x, bar.y, bar.w * done, bar.h),
                        c.fill,
                        Radius::Pill,
                    );
                }
            }
            if stop.w > 0.0 {
                let hot = hover == Some(BrowserHit::StopScan);
                pen.round(stop, if hot { c.hover } else { c.surface_3 }, Radius::S);
                pen.text(
                    SCAN_STOP,
                    FontSize::Caption,
                    Weight::Medium,
                    c.ink_strong,
                    stop,
                    Align::Center,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "browser_test_library.rs"]
pub(crate) mod test_library;

#[cfg(test)]
mod tests {
    use super::test_library::example;
    use super::*;

    fn panel() -> Rect {
        Rect::new(52.0, 6.0, Layout::BROWSER_W, 470.0)
    }

    fn layout(b: &Browser) -> BrowserLayout {
        b.layout(panel())
    }

    fn names(rows: &[BRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                BRow::Group { name, count, .. } => format!("[{name} {count}]"),
                BRow::Item { item, .. } => item.name.to_string(),
                BRow::Dir { name, .. } => format!("{name}/"),
                BRow::File { name, .. } => name.clone(),
                BRow::Audition { .. } => "<audition>".to_string(),
                BRow::Note(n) => (*n).to_string(),
                BRow::Scanning => "<scanning>".to_string(),
                BRow::Bad { name, .. } => format!("!{name}"),
                BRow::Pool {
                    name,
                    clips,
                    missing,
                    external,
                    ..
                } => {
                    let mut s = name.clone();
                    if *missing {
                        s.push_str(" ?");
                    }
                    if *external {
                        s.push_str(" ext");
                    }
                    if let Some(n) = clips {
                        s.push_str(&format!(" {n}"));
                    }
                    s
                }
            })
            .collect()
    }

    fn a_bad_one(name: &str) -> BadPlugin {
        BadPlugin {
            path: PathBuf::from(format!("C:/VST3/{name}.vst3")),
            name: name.to_string(),
            why: "応答がありません",
        }
    }

    #[test]
    fn the_scanning_row_has_the_count_the_bar_and_the_stop_inside_it() {
        let mut b = example();
        b.press_rail(RailTab::Effects);
        b.scanning = true;
        assert_eq!(b.scanning_text(), "検索中");
        assert_eq!(b.scan_count(), None);
        (b.scan_done, b.scan_total) = (12, 48);
        assert_eq!(b.scanning_text(), "スキャン中");
        assert_eq!(b.scan_count().as_deref(), Some("12 / 48"));
        let l = layout(&b);
        let first = &l.rows[0];
        assert_eq!(first.row, BRow::Scanning);
        assert_eq!(first.h, SCAN_H);
        // The rows under it start where it ends.
        assert_eq!(l.rows[1].y, first.h);
        let row = Rect::new(l.list.x, l.list.y, l.list.w, first.h);
        let stop = b.stop_rect(row);
        let bar = b.scan_bar(row);
        assert!(stop.bottom() <= bar.y, "the stop is above the bar");
        assert!(bar.bottom() <= row.bottom());
        assert!(stop.right() <= row.right() && stop.x >= row.x);
        // Stopping: the button and the count are gone.
        b.scan_stopping = true;
        assert_eq!(b.scanning_text(), "中止しています…");
        assert_eq!(b.scan_count(), None);
        assert_eq!(b.stop_rect(row).w, 0.0);
    }

    #[test]
    fn unusable_plugins_form_a_group_at_the_end_that_a_scan_or_a_search_hides() {
        let mut b = example();
        b.press_rail(RailTab::Effects);
        let before = names(&b.rows());
        b.bad = vec![a_bad_one("Alpha"), a_bad_one("Beta")];
        let rows = names(&b.rows());
        assert_eq!(rows[..before.len()], before[..]);
        assert_eq!(
            rows[before.len()..],
            ["[使えないプラグイン 2]", "!Alpha", "!Beta"]
        );
        // The rows do not overlap and the last one ends the content.
        let l = layout(&b);
        for w in l.rows.windows(2) {
            assert!(w[0].y + w[0].h <= w[1].y + 0.001, "{w:?}");
        }
        let last = l.rows.last().unwrap();
        assert_eq!(
            last.row,
            BRow::Bad {
                path: PathBuf::from("C:/VST3/Beta.vst3"),
                name: "Beta".into(),
                why: "応答がありません",
                retrying: false,
            }
        );
        assert_eq!(last.h, BAD_H);
        let row = Rect::new(l.list.x, l.list.y, l.list.w, last.h);
        let retry = b.retry_rect(row);
        assert!(retry.x >= row.x && retry.right() <= row.right());
        assert!(retry.y >= row.y && retry.bottom() <= row.bottom());
        // Folded: only the header stays.
        b.closed_groups.insert(BAD_GROUP.to_string());
        assert_eq!(names(&b.rows())[before.len()..], ["[使えないプラグイン 2]"]);
        b.closed_groups.clear();
        // A full scan makes the list again: the group waits. Retrying one keeps it.
        b.scanning = true;
        assert!(b.rows().iter().all(|r| !matches!(r, BRow::Bad { .. })));
        b.retrying = Some(PathBuf::from("C:/VST3/Beta.vst3"));
        let rows = b.rows();
        assert!(
            rows.iter()
                .any(|r| matches!(r, BRow::Bad { retrying: true, .. }))
        );
        assert_eq!(
            rows.iter()
                .filter(|r| matches!(
                    r,
                    BRow::Bad {
                        retrying: false,
                        ..
                    }
                ))
                .count(),
            1
        );
        b.scanning = false;
        b.retrying = None;
        // A search leaves it out.
        b.search = "a".into();
        assert!(b.rows().iter().all(|r| !matches!(
            r,
            BRow::Bad { .. }
                | BRow::Group {
                    name: BAD_TITLE,
                    ..
                }
        )));
    }

    #[test]
    fn instruments_are_grouped_by_kind_with_counts() {
        let b = example();
        assert_eq!(
            names(&b.rows()),
            [
                "[サンプラー 1]",
                "Sampler",
                "[シンセ 3]",
                "Dexed",
                "Surge XT",
                "Vital",
                "[ドラム 1]",
                "Drum Kit"
            ]
        );
    }

    #[test]
    fn the_effect_tab_lists_only_effects_and_keeps_its_own_view() {
        let mut b = example();
        b.press_rail(RailTab::Effects);
        b.set_view(View::Vendor);
        assert!(names(&b.rows()).contains(&"[Valhalla DSP 1]".to_string()));
        b.press_rail(RailTab::Instruments);
        assert_eq!(b.view(), View::Kind, "instruments keep their own view");
        assert!(!names(&b.rows()).contains(&"EQ".to_string()));
    }

    #[test]
    fn every_view_lists_every_item_once() {
        let mut b = example();
        for view in [View::Kind, View::Vendor, View::Format, View::Name] {
            b.set_view(view);
            let items = b
                .rows()
                .iter()
                .filter(|r| matches!(r, BRow::Item { .. }))
                .count();
            assert_eq!(items, 5, "{view:?}");
        }
    }

    #[test]
    fn the_name_and_favourite_views_are_flat() {
        let mut b = example();
        b.set_view(View::Name);
        assert_eq!(
            names(&b.rows()),
            ["Dexed", "Drum Kit", "Sampler", "Surge XT", "Vital"]
        );
        b.set_view(View::Favourites);
        assert_eq!(names(&b.rows()), ["Sampler", "Vital"]);
    }

    #[test]
    fn the_vendor_text_is_left_out_when_the_list_is_grouped_by_vendor() {
        let mut b = example();
        b.set_view(View::Vendor);
        assert!(b.rows().iter().all(|r| match r {
            BRow::Item { vendor_text, .. } => !vendor_text,
            _ => true,
        }));
        b.set_view(View::Kind);
        assert!(b.rows().iter().any(|r| matches!(
            r,
            BRow::Item {
                vendor_text: true,
                ..
            }
        )));
    }

    #[test]
    fn a_star_press_toggles_a_favourite_and_the_favourite_view_follows() {
        let mut b = example();
        let l = layout(&b);
        let i = l
            .rows
            .iter()
            .position(|p| matches!(&p.row, BRow::Item { item, .. } if item.name == "Dexed"))
            .unwrap();
        assert!(!b.favourites.contains("Dexed"));
        b.press(&l, BrowserHit::Fav(i));
        assert!(b.favourites.contains("Dexed"));
        b.set_view(View::Favourites);
        assert_eq!(names(&b.rows()), ["Dexed", "Sampler", "Vital"]);
        b.toggle_favourite("Dexed");
        assert!(!b.favourites.contains("Dexed"));
    }

    #[test]
    fn a_group_header_folds_its_items() {
        let mut b = example();
        let l = layout(&b);
        b.press(&l, BrowserHit::Group(2));
        assert!(!names(&b.rows()).contains(&"Dexed".to_string()));
        let l = layout(&b);
        b.press(&l, BrowserHit::Group(2));
        assert!(names(&b.rows()).contains(&"Dexed".to_string()));
    }

    #[test]
    fn typing_in_the_search_box_filters_and_opens_every_group() {
        let mut b = example();
        let l = layout(&b);
        b.press(&l, BrowserHit::Group(2));
        let l = layout(&b);
        assert_eq!(
            b.hit(&l, l.search.x + 4.0, l.search.y + 4.0),
            Some(BrowserHit::Search)
        );
        b.press(&l, BrowserHit::Search);
        assert!(b.typing);
        for ch in ["s", "u", "r"] {
            assert!(b.typed(ch, Some(ch)));
        }
        assert_eq!(names(&b.rows()), ["[シンセ 1]", "Surge XT"]);
        assert!(b.typed("backspace", None));
        assert_eq!(b.search, "su");
        assert!(b.typed("escape", None));
        assert!(b.search.is_empty() && !b.typing);
        assert!(
            !b.typed("a", Some("a")),
            "no key goes to a box that is not focused"
        );
    }

    #[test]
    fn a_search_with_no_hit_says_so() {
        let mut b = example();
        b.search = "zzz".into();
        assert_eq!(names(&b.rows()), ["見つかりません"]);
    }

    #[test]
    fn the_search_matches_vendor_and_kind_too() {
        let mut b = example();
        b.search = "digital".into();
        assert_eq!(names(&b.rows()), ["[シンセ 1]", "Dexed"]);
        b.search = "ドラム".into();
        assert_eq!(names(&b.rows()), ["[ドラム 1]", "Drum Kit"]);
    }

    #[test]
    fn the_second_press_on_the_open_tab_shuts_the_panel() {
        let mut b = example();
        b.press_rail(RailTab::Instruments);
        assert!(!b.open);
        b.press_rail(RailTab::Files);
        assert!(b.open && b.tab == RailTab::Files);
        b.press_rail(RailTab::Instruments);
        assert!(b.open && b.tab == RailTab::Instruments);
    }

    #[test]
    fn rows_are_hit_by_position_and_scrolling_moves_them() {
        let mut b = example();
        let l = layout(&b);
        let first = &l.rows[0];
        let r = b.row_rect(&l, first);
        assert_eq!(b.hit(&l, r.x + 20.0, r.y + 4.0), Some(BrowserHit::Group(0)));
        let mut small = l.clone();
        small.list.h = 60.0;
        assert!(b.max_scroll(&small) > 0.0);
        b.scroll = 10_000.0;
        b.clamp_scroll(&small);
        assert_eq!(b.scroll, b.max_scroll(&small));
    }

    #[test]
    fn the_view_button_is_on_every_tab_but_the_files() {
        let mut b = example();
        assert!(layout(&b).view.is_some());
        b.press_rail(RailTab::Files);
        let l = layout(&b);
        assert!(l.view.is_none());
        assert!(l.add.is_some());
        // The song files have it before their 「…」, clear of the search.
        b.press_rail(RailTab::SongFiles);
        let l = layout(&b);
        let (v, m) = (l.view.unwrap(), l.more.unwrap());
        assert!(
            v.right() < m.x && l.search.right() < v.x,
            "{:?} {v:?} {m:?}",
            l.search
        );
    }

    #[test]
    fn every_part_fits_inside_the_panel() {
        let mut b = example();
        for tab in RailTab::ALL {
            b.press_rail(tab);
            b.open = true;
            b.tab = tab;
            let l = layout(&b);
            let p = l.panel;
            for r in [Some(l.search), l.view, l.more, l.add]
                .into_iter()
                .flatten()
            {
                assert!(r.x >= p.x && r.right() <= p.right() + 0.5, "{tab:?} {r:?}");
            }
            for r in &l.tabs {
                assert!(r.right() <= p.right(), "{tab:?} tab {r:?}");
            }
            assert!(l.list.y > l.search.bottom());
        }
    }

    #[test]
    fn the_rail_buttons_do_not_overlap_the_io_button() {
        let rail = Rect::new(6.0, 6.0, Layout::RAIL_W, 470.0);
        let last = rail_button(rail, RailTab::Files);
        assert!(last.bottom() < rail_io_button(rail).y);
        assert!(rail_button(rail, RailTab::Instruments).right() <= rail.right());
    }
}
