//! The pop-overs: history, grid, song menu, version list, track colour, track
//! output and the browser's view choice.
//!
//! A [`Popup`] is a snapshot of what it lists, taken when it was opened (the
//! history, the versions, the grid value), so laying it out and painting it
//! need neither the song nor a window. [`Popup::layout`] turns it into a flat
//! list of [`Cell`]s with rectangles; a cell with an [`Action`] is a button and
//! `Hit::Item(i)` names it by its index. `Studio` runs the action through
//! [`crate::song::Song`], so every effect is a core command.

use xirius_core::id::{FolderId, PluginId, TrackId};
use xirius_core::model::PluginIsolation;
use xirius_design::{FontSize, Layout, Radius, Space, TrackColor, Weight};

use crate::browser::{View, text_width};
use crate::draw::{Align, Pen};
use crate::health::PlugAct;
use crate::pick_list::PickList;
use crate::song::{HistoryItem, VersionDetail, VersionItem};
use crate::studio::Rect;
use crate::tools::SnapMode;

/// Gap between the button and the pop-over under it.
pub const ANCHOR_GAP: f32 = 6.0;
/// The most history rows listed (the newest ones).
pub const HISTORY_ROWS: usize = 9;
/// The most versions listed (the newest ones).
pub const VERSION_ROWS: usize = 6;
/// The most recent songs the song menu lists.
pub const RECENT_ROWS: usize = 5;
/// The grid pop-over: its width, and the buttons of its segmented rows.
const GRID_W: f32 = 300.0;
pub(crate) const SEGMENT_H: f32 = 24.0;
pub(crate) const SEGMENT_INSET: f32 = 2.0;
pub(crate) const LABEL_H: f32 = 22.0;
const VERSION_ROW_H: f32 = 40.0;
/// The one line of an operation in the history: a line of text, and the room around them.
const CLI_LINE_H: f32 = 16.0;
const CLI_PAD: f32 = 6.0;
pub(crate) const SWATCH: f32 = 24.0;
/// The list of the devices (UI-260): its width.
/// An icon that keeps its room in a row but draws nothing (the rows without the check).
pub const BLANK: &str = "blank";
pub(crate) const SWATCH_GAP: f32 = 4.0;
/// Two rows of eight for the sixteen colours; eight of them fill a menu's width.
pub(crate) const SWATCH_COLUMNS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupKind {
    History,
    Grid,
    Song,
    Versions,
    Color(TrackId),
    Output(TrackId),
    /// The input device of an audio track (REC-017).
    Input(TrackId),
    /// A plugin for a new insert row of the track (MIX-016).
    Insert(TrackId),
    /// A bus for a new send of the track (MIX-017).
    Send(TrackId),
    /// The instrument of an instrument track (PLG-019).
    Instrument(TrackId),
    AutomationTargets(TrackId, bool),
    BrowserView,
    /// The + at the head of the track column.
    AddTrack,
    /// The export panel (EXP-009).
    Export,
    /// A plugin on the track's insert rows: its restart and its process (PLG-022, PLG-053).
    Plugin(TrackId, PluginId),
    CrashedPlugin(TrackId, PluginId),
    /// The AI's permission and how to reach it (AI-003, AI-022).
    Ai,
    /// An automation point's value, typed (AUT-008).
    AutoValue(crate::autovalue::AutoRef),
    /// The quantize panel (EDT-073).
    Quantize,
    NoteEdit(crate::note_edit_panel::Kind),
    /// A folder's output bus (EDT-073, the folder head of the mock).
    FolderBus(FolderId),
    /// The input quantize's value, under its box in the record panel (REC-009).
    RecQuantize,
    /// 「Xirius について」 under the logo (LIC-015).
    About,
    /// A menu of an instrument: its presets (UI-098).
    InstMenu(crate::instrument::menu::Menu),
    /// The right-click menu of a scene (CTX-001): its rows are in `Popup::ctx`.
    Context,
    /// The meter of the top bar: the usual meters and the two fields (UI-119).
    Meter,
    /// What applying a track preset adjusted, in full (PRJ-007, MIX-030).
    Report,
    /// The CPU of the song and of each plugin, the heaviest first (ENG-008).
    Perf,
    /// The audio devices, from the device panel of the entrance (UI-260); only those with
    /// inputs when true (opened by 「オーディオを録音」 on a device without inputs).
    Device(bool),
    /// What a `.dawproject` could not carry, over the toast that counted it (EXP-006, EXP-007).
    Skipped,
    /// The recordings nothing uses, before they go to the recycle bin (BRW-004), in the middle
    /// of the window over a veil.
    Cleanup,
}

/// What a button of a pop-over does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    AutomationTarget(TrackId, Option<String>, String),
    OpenNoteEdit(crate::note_edit_panel::Kind),
    EditValue(crate::note_edit_panel::Kind, &'static str),
    Link,
    Undo,
    Redo,
    /// `history.jump` to this state.
    Jump(u64),
    /// Opens or closes the one-line detail of this operation in the history (AI-025).
    StepInfo(u64),
    /// `view.snap`
    Snap(bool),
    /// `view.grid` with this base value (`1/16`); a triplet keeps its `t`.
    GridValue(&'static str),
    Triplet,
    /// How the grid step is found (`Studio::snap_mode`).
    Mode(SnapMode),
    /// AQ: a new grid value quantizes the selected notes again (EDT-075).
    AutoQuantize,
    /// Add a track of this kind (`audio`, `instrument`, `folder`, `bus`) at the end.
    AddTrack(&'static str),
    /// Open the browser's files to bring a file in.
    FromFile,
    OpenSong,
    /// 新規: a new, empty song (UI-078).
    NewSong,
    /// A song of the recent list (PRJ-014): its place in `Popup::recent`.
    OpenRecent(usize),
    /// A press on the card shown after a crash (PRJ-012).
    Recovery(crate::recovery_card::Button),
    Save,
    SaveAs,
    ShowVersions,
    /// The song's own settings (sample rate, bit depth), as a form of the command.
    SongSettings,
    Export,
    NameVersion,
    SelectVersion(usize),
    Restore,
    OpenAsSong,
    /// `track.color.set`: a palette index, or None to take the colour away.
    Color(Option<usize>),
    /// `track.output.set`: `default`, `master` or a bus id.
    Output(String),
    /// `track.input.set`: channels of the input (`In 1+2`), or `none` for the default.
    Input(String),
    View(View),
    /// The plugin (its uid) or the bus picked in a pick list.
    Pick(String),
    /// A press in the export panel.
    Xp(crate::export::Act),
    /// A press in the plugin menu.
    Plugin(crate::health::PlugAct),
    /// A press in the AI's pop-over.
    Ai(crate::ai::Act),
    /// A press in the quantize panel, or its row in the grid pop-over.
    Quant(crate::quantize_panel::QAct),
    /// A row of a folder's bus menu: a bus id, `none` or `new`.
    FolderBus(String),
    /// A row of the record panel's input quantize menu: its value.
    RecQuantize(String),
    /// A press in 「Xirius について」.
    About(crate::about::Act),
    /// Row `i` of a right-click menu.
    Ctx(usize),
    /// The search box at the top of a long menu: it takes the keys (UI-313).
    CtxSearch,
    /// A press in the meter's pop-over.
    Meter(crate::meter_popup::MAct),
    /// A press in the list of what a preset applied adjusted.
    Report(crate::adjust_report::Act),
    /// A button of 「使われていない録音を整理」.
    Cleanup(crate::cleanup_dialog::Act),
    /// A press in the CPU list.
    Cpu(crate::cpu_list::CAct),
    /// A part of the field of row `i` at the head of an audio clip's menu.
    Field(usize, crate::context::fields::FieldAct),
    /// `audio.set` of the device: its name.
    Device(String),
    /// 「DAWproject を開く…」 of the song menu (EXP-006).
    OpenDawproject,
    /// 「DAWproject として書き出す…」 of the song menu (EXP-007).
    ExportDawproject,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// What `track.output.set` takes.
    pub to: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    /// The list menu's part and its state (UI-320), when the kind is one.
    pub pick: Option<PickList>,
    pub kind: PopupKind,
    /// The button it hangs from (body coordinates).
    pub anchor: Rect,
    pub history: Vec<HistoryItem>,
    /// The commands that left no step, listed with the history (`Song::history_notes`).
    pub notes: Vec<HistoryItem>,
    /// The operation whose detail is open in the history.
    pub open_step: Option<u64>,
    pub grid: String,
    pub snap: bool,
    pub mode: SnapMode,
    /// AQ (`Studio::auto_quantize`).
    pub aq: bool,
    /// Does the song live in a folder yet (can it keep versions)?
    pub has_dir: bool,
    pub versions: Vec<VersionItem>,
    pub selected: Option<usize>,
    pub detail: Option<VersionDetail>,
    /// The name being typed for a new version.
    pub naming: Option<String>,
    pub now_ms: u64,
    /// The palette index of the track's colour (Color popups).
    pub color: Option<usize>,
    pub view: View,
    /// The views the browser's tab offers.
    pub views: &'static [View],
    /// Is `export.mixdown` reachable from this screen?
    pub can_export: bool,
    pub offset: usize,
    /// What the export panel shows.
    pub export: Option<Box<crate::export::View>>,
    /// What the plugin menu shows.
    pub plugin: Option<crate::health::PluginView>,
    /// What the AI's pop-over shows.
    pub ai: Option<Box<crate::ai::View>>,
    /// What an automation point's menu and value box show.
    pub auto: Option<crate::autovalue::PointView>,
    /// What the quantize panel shows.
    pub quant: Option<crate::quantize_panel::QuantView>,
    /// What the meter's pop-over shows.
    pub meter: Option<crate::meter_popup::MeterView>,
    /// What the list of a preset's adjustments shows.
    pub report: Option<crate::adjust_report::View>,
    /// What the CPU list shows.
    pub perf: Option<crate::cpu_list::View>,
    /// What 「Xirius について」 shows.
    pub about: Option<Box<crate::about::View>>,
    /// The songs the song menu lists under 最近の曲 (PRJ-014).
    pub recent: Vec<crate::keep::RecentSong>,
    /// The rows of a right-click menu.
    pub ctx: Vec<crate::context::Row>,
    /// What a right-click menu's name box shows, while it asks for a name.
    pub ask: Option<String>,
    pub ctx_query: Option<String>,
    /// The search box of a right-click menu takes the keys now (no value's field does).
    pub ctx_search: bool,
    /// The list of a [`PopupKind::Skipped`].
    pub skipped: Option<crate::dawproject_ui::List>,
    /// The files of a [`PopupKind::Cleanup`].
    pub cleanup: Option<crate::cleanup_dialog::View>,
}

impl Popup {
    pub fn new(kind: PopupKind, anchor: Rect) -> Popup {
        Popup {
            pick: None,
            kind,
            anchor,
            history: Vec::new(),
            notes: Vec::new(),
            open_step: None,
            grid: "1/16".to_string(),
            snap: true,
            mode: SnapMode::Auto,
            aq: false,
            has_dir: false,
            versions: Vec::new(),
            selected: None,
            detail: None,
            naming: None,
            now_ms: 0,
            color: None,
            view: View::Kind,
            views: &View::ALL,
            can_export: false,
            offset: 0,
            export: None,
            plugin: None,
            ai: None,
            auto: None,
            quant: None,
            meter: None,
            report: None,
            perf: None,
            about: None,
            recent: Vec::new(),
            ctx: Vec::new(),
            ask: None,
            ctx_query: None,
            ctx_search: false,
            skipped: None,
            cleanup: None,
        }
    }

    /// The list menu's rows as the old flat `choices` (the tests' view:
    /// every group's rows, then the footer the search never filters).
    pub fn choices(&self) -> Vec<Choice> {
        self.pick.as_ref().map(|p| p.choices()).unwrap_or_default()
    }

    /// The lit row's `to` (the old `current` field).
    pub fn current(&self) -> String {
        self.pick.as_ref().map(|p| p.current()).unwrap_or_default()
    }

    /// How far the wheel can move the list down at the window `size` (a list
    /// menu's rows shrink to the screen's edge, so it counts with it).
    pub fn max_offset(&self, size: (f32, f32)) -> usize {
        match self.kind {
            PopupKind::History => {
                (self.history.len() + self.notes.len()).saturating_sub(HISTORY_ROWS + 1)
            }
            PopupKind::Versions => self.versions.len().saturating_sub(VERSION_ROWS),
            PopupKind::Report => self
                .report
                .as_ref()
                .map_or(0, crate::adjust_report::View::max_offset),
            PopupKind::Perf => self
                .perf
                .as_ref()
                .map_or(0, crate::cpu_list::View::max_offset),
            PopupKind::Skipped => self
                .skipped
                .as_ref()
                .map_or(0, crate::dawproject_ui::List::max_offset),
            PopupKind::Cleanup => self
                .cleanup
                .as_ref()
                .map_or(0, crate::cleanup_dialog::View::max_offset),
            PopupKind::About => self.about.as_deref().map_or(0, crate::about::max_offset),
            _ if self.pick.is_some() => self.pick.as_ref().unwrap().max_offset(size),
            _ => 0,
        }
    }

    /// Does the popup take typed characters now?
    pub fn typing(&self) -> bool {
        self.naming.is_some()
    }

    pub fn typed(&mut self, key: &str, ch: Option<&str>) -> bool {
        let Some(name) = self.naming.as_mut() else {
            return false;
        };
        match key {
            "backspace" => {
                name.pop();
            }
            "escape" => self.naming = None,
            _ => match ch {
                Some(c) if !c.is_empty() && !c.chars().any(char::is_control) => name.push_str(c),
                _ => return false,
            },
        }
        true
    }
}

/// `1/16` and `1/8t`: the base value and whether it is a triplet.
pub fn split_grid(grid: &str) -> (&str, bool) {
    match grid.strip_suffix('t') {
        Some(base) => (base, true),
        None => (grid, false),
    }
}

pub const GRID_VALUES: [&str; 7] = ["1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64"];

/// How long ago, in words a person would say.
pub fn ago(now_ms: u64, at_ms: u64) -> String {
    let s = now_ms.saturating_sub(at_ms) / 1000;
    match s {
        0..=44 => "たった今".to_string(),
        45..=3599 => format!("{} 分前", (s + 30) / 60),
        3600..=86_399 => format!("{} 時間前", s / 3600),
        _ => format!("{} 日前", s / 86_400),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Look {
    /// A small heading.
    Label,
    Row,
    Chip,
    /// The tray a row of [`Look::Segment`]s sits in.
    Tray,
    /// One button of a segmented row: lit when on.
    Segment,
    Button,
    Primary,
    Swatch(Option<usize>),
    /// A history row.
    Step {
        ai: bool,
        undone: bool,
        opening: bool,
    },
    /// The button at the end of a history row that opens its one line.
    StepInfo,
    /// The one line of an operation: `command` lines of `xirius …`, then what it changed
    /// (`text` holds all the lines).
    Cli {
        ai: bool,
        command: usize,
    },
    /// A version on the timeline.
    Version {
        big: bool,
        first: bool,
        last: bool,
    },
    /// Tracks, length and the colours of the tracks that changed.
    Detail(Vec<Option<usize>>),
    Counter,
    Input,
    Note,
    /// The field of a clip's value in its menu.
    Field,
    /// A part of the export panel.
    Xp(crate::export::XpLook),
    /// A plugin that stopped: the bolt and its name.
    Down,
    /// A part of the AI's pop-over.
    Ai(crate::ai::AiLook),
    /// A part of an automation point's menu or value box.
    Auto(crate::autovalue::AutoLook),
    /// A part of the quantize panel.
    Quant(crate::quantize_panel::QLook),
    /// A part of the meter's pop-over.
    Meter(crate::meter_popup::MLook),
    /// A part of the CPU list.
    Cpu(crate::cpu_list::CLook),
    /// A part of 「Xirius について」.
    About(crate::about::AboutLook),
    /// A part of 「使われていない録音を整理」.
    Clean(crate::cleanup_dialog::CleanLook),
    /// The scroll thumb of a pick list with more rows than show (UI-297).
    Thumb,
    /// 「一致する…はありません」: a status a screen reader reads (UI-320).
    Status,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub rect: Rect,
    pub look: Look,
    pub text: String,
    pub hint: String,
    pub icon: Option<&'static str>,
    /// A colour square before the text (the menu's 「色」 row shows the track's colour).
    pub mark: Option<usize>,
    /// Current, selected or switched on.
    pub on: bool,
    /// The row the arrow keys are on (UI-320: the list menus' cursor).
    pub cursor: bool,
    /// Under the list: the query never filters it (UI-320's footer).
    pub foot: bool,
    /// A list's row that cannot run now: a lock at its right says so, its
    /// name in the body ink, not fainter (UI-338).
    pub locked: bool,
    pub enabled: bool,
    pub action: Option<Action>,
    /// Said in a balloon while the pointer is on it (why a dim cell cannot be chosen).
    pub tip: String,
    /// What a screen reader says for it, when that is more than its text (a value field keeps
    /// its unit there, UI-311). Empty: its text.
    pub spoken: String,
}

impl Cell {
    pub(crate) fn new(rect: Rect, look: Look) -> Cell {
        Cell {
            rect,
            look,
            text: String::new(),
            hint: String::new(),
            icon: None,
            mark: None,
            on: false,
            cursor: false,
            foot: false,
            locked: false,
            enabled: true,
            action: None,
            tip: String::new(),
            spoken: String::new(),
        }
    }

    pub(crate) fn mark(mut self, color: Option<usize>) -> Cell {
        self.mark = color;
        self
    }

    pub(crate) fn text(mut self, text: impl Into<String>) -> Cell {
        self.text = text.into();
        self
    }

    pub(crate) fn tip(mut self, tip: impl Into<String>) -> Cell {
        self.tip = tip.into();
        self
    }

    pub(crate) fn hint(mut self, hint: impl Into<String>) -> Cell {
        self.hint = hint.into();
        self
    }

    /// Dim and picking nothing, with the lock at its right (a list's row
    /// that cannot run now, UI-338).
    pub(crate) fn locked(mut self) -> Cell {
        self.locked = true;
        self.off()
    }

    pub(crate) fn spoken(mut self, spoken: impl Into<String>) -> Cell {
        self.spoken = spoken.into();
        self
    }

    pub(crate) fn icon(mut self, icon: &'static str) -> Cell {
        self.icon = Some(icon);
        self
    }

    pub(crate) fn on(mut self, on: bool) -> Cell {
        self.on = on;
        self
    }

    /// The arrow keys' row: the same lit band a hovered row gets.
    pub(crate) fn cur(mut self, cur: bool) -> Cell {
        self.cursor = cur;
        self
    }

    /// Under the list: the query never filters it (UI-320's footer).
    pub(crate) fn foot(mut self) -> Cell {
        self.foot = true;
        self
    }

    /// A cell with an action that cannot run now keeps the action but is dim.
    pub(crate) fn act(mut self, action: Action) -> Cell {
        self.action = Some(action);
        self
    }

    pub(crate) fn off(mut self) -> Cell {
        self.enabled = false;
        self.action = None;
        self
    }

    /// Can a press on it do something?
    pub fn live(&self) -> bool {
        self.enabled && self.action.is_some()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopupLayout {
    pub rect: Rect,
    pub cells: Vec<Cell>,
}

impl PopupLayout {
    /// The cell under a point that a press can use, or that has a tip to show.
    pub fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        self.cells
            .iter()
            .position(|c| (c.live() || !c.tip.is_empty()) && c.rect.contains(x, y))
    }
}

impl Popup {
    /// The cells and the rectangle, for a body of `size`.
    pub fn layout(&self, size: (f32, f32)) -> PopupLayout {
        let pad = Layout::POPUP_PAD;
        let (width, cells) = match self.kind {
            PopupKind::History => (300.0, self.history_cells(300.0)),
            PopupKind::Grid => (GRID_W, self.grid_cells(GRID_W)),
            PopupKind::Song => (Layout::MENU_W, self.song_cells(Layout::MENU_W)),
            PopupKind::Versions => (380.0, self.version_cells(380.0)),
            PopupKind::Color(_) => {
                let w = pad * 2.0
                    + SWATCH_COLUMNS as f32 * SWATCH
                    + (SWATCH_COLUMNS - 1) as f32 * SWATCH_GAP;
                (w, self.color_cells())
            }
            PopupKind::Export => match &self.export {
                Some(v) => (
                    crate::export::W,
                    crate::export::cells(v, self.naming.as_deref()),
                ),
                None => (crate::export::W, Vec::new()),
            },
            PopupKind::Plugin(..) => (Layout::MENU_W, self.plugin_cells(Layout::MENU_W)),
            PopupKind::CrashedPlugin(..) => (
                Layout::MENU_W,
                self.plugin
                    .as_ref()
                    .map(|v| crate::plugin_crash_card::cells(v, Layout::MENU_W))
                    .unwrap_or_default(),
            ),
            PopupKind::Ai => (
                crate::ai::W,
                self.ai.as_deref().map(crate::ai::cells).unwrap_or_default(),
            ),
            PopupKind::AutoValue(_) => (
                Layout::MENU_W,
                crate::autovalue::value_cells(
                    self.auto.as_ref(),
                    self.naming.as_deref(),
                    Layout::MENU_W,
                ),
            ),
            PopupKind::Context => match &self.ask {
                Some(text) => (
                    Layout::MENU_W,
                    crate::context::ask::cells(text, Layout::MENU_W),
                ),
                None => {
                    let w = crate::context::width(&self.ctx);
                    let mut cells = crate::context::cells(&self.ctx, w);
                    if let Some(query) = &self.ctx_query {
                        for cell in &mut cells {
                            cell.rect.y += 40.0;
                        }
                        let mut input = crate::context::ask::cells(query, w);
                        input[0].icon = Some("search");
                        input[0] = input[0].clone().act(Action::CtxSearch).on(self.ctx_search);
                        input.extend(cells);
                        cells = input;
                    }
                    (w, cells)
                }
            },
            PopupKind::NoteEdit(kind) => (220.0, crate::note_edit_panel::cells(kind)),
            PopupKind::Quantize => (
                crate::quantize_panel::W,
                crate::quantize_panel::cells(
                    self.quant.as_ref(),
                    &self.grid,
                    crate::quantize_panel::W,
                ),
            ),
            PopupKind::Meter => (
                crate::meter_popup::W,
                crate::meter_popup::cells(self.meter.as_ref()),
            ),
            PopupKind::Report => (
                crate::adjust_report::W,
                self.report
                    .as_ref()
                    .map(|v| crate::adjust_report::cells(v, self.offset))
                    .unwrap_or_default(),
            ),
            PopupKind::Perf => (
                crate::cpu_list::W,
                crate::cpu_list::cells(self.perf.as_ref(), self.offset),
            ),
            PopupKind::About => (
                crate::about::W,
                self.about
                    .as_deref()
                    .map(|v| crate::about::cells(v, self.offset))
                    .unwrap_or_default(),
            ),
            PopupKind::Skipped => match &self.skipped {
                Some(list) => (list.width(), crate::dawproject_ui::cells(list, self.offset)),
                None => (crate::dawproject_ui::MIN_W, Vec::new()),
            },
            PopupKind::Cleanup => (
                crate::cleanup_dialog::W,
                self.cleanup
                    .as_ref()
                    .map(|v| crate::cleanup_dialog::cells(v, self.offset))
                    .unwrap_or_default(),
            ),
            // A list menu is always the PickList part (UI-320); every other
            // kind names its arm above, so the catch-all is theirs.
            kind => {
                debug_assert!(
                    crate::pick_list::is(kind),
                    "a list menu's cells come only from PickList (UI-320): {kind:?}"
                );
                let w = crate::pick_list::width(kind);
                (
                    w,
                    self.pick
                        .as_ref()
                        .map(|p| crate::pick_list::cells(p, w, size))
                        .unwrap_or_default(),
                )
            }
        };
        let height = match &self.pick {
            // A list keeps its height while it scrolls (UI-338): the rows in
            // view vary with the headings, the window does not.
            Some(p) if crate::pick_list::is(self.kind) => crate::pick_list::height(p, size),
            _ => cells.iter().map(|c| c.rect.bottom()).fold(0.0f32, f32::max) + pad,
        };
        // Right-aligned under the buttons that sit at the right. The CPU is at the left of the
        // bottom bar (UI-300), so its list opens from its left edge.
        let right_aligned = matches!(
            self.kind,
            PopupKind::History | PopupKind::Export | PopupKind::Ai
        );
        let mut x = if right_aligned {
            self.anchor.right() - width
        } else {
            self.anchor.x
        };
        x = x.clamp(4.0, (size.0 - width - 4.0).max(4.0));
        // Above the anchor when there is no room under it (the mixer's rows).
        let below = self.anchor.bottom() + ANCHOR_GAP;
        if self.kind == PopupKind::Cleanup {
            // In the middle of the window, over the veil.
            x = ((size.0 - width) / 2.0).max(4.0);
        }
        let y = if self.kind == PopupKind::Cleanup {
            ((size.1 - height) / 2.0).max(4.0)
        } else if self.kind == PopupKind::Skipped {
            // Over the toast at the foot of the window, which stays.
            (self.anchor.y - ANCHOR_GAP - height).max(4.0)
        } else if below + height > size.1 - 4.0 && self.anchor.y - ANCHOR_GAP - height >= 4.0 {
            self.anchor.y - ANCHOR_GAP - height
        } else {
            below.clamp(4.0, (size.1 - height - 4.0).max(4.0))
        };
        let cells = cells
            .into_iter()
            .map(|mut c| {
                c.rect.x += x;
                c.rect.y += y;
                c
            })
            .collect();
        PopupLayout {
            rect: Rect::new(x, y, width, height),
            cells,
        }
    }

    // Cells below are laid out from (0, 0) = the popup's top-left corner.

    /// The rows of the history, newest first, with the commands that left no step among
    /// them by time; the opening state apart.
    fn listing(&self) -> (Vec<&HistoryItem>, Option<&HistoryItem>) {
        let mut steps = self.history.iter();
        let opening = steps.next().filter(|h| h.n == 0);
        let mut notes = self.notes.iter().peekable();
        let mut rows: Vec<&HistoryItem> = Vec::new();
        for step in steps {
            while let Some(note) = notes.next_if(|n| n.at_ms <= step.at_ms) {
                rows.push(note);
            }
            rows.push(step);
        }
        rows.extend(notes);
        rows.reverse();
        (rows, opening)
    }

    fn history_cells(&self, w: f32) -> Vec<Cell> {
        let pad = Layout::POPUP_PAD;
        let mut cells = Vec::new();
        let b = 28.0;
        let can_undo = self.history.iter().any(|h| h.current && h.n > 0);
        let can_redo = self.history.iter().any(|h| h.undone);
        let undo = Cell::new(Rect::new(pad, pad, b, b), Look::Button).icon("undo");
        cells.push(if can_undo {
            undo.act(Action::Undo)
        } else {
            undo.off()
        });
        let redo = Cell::new(Rect::new(pad + b + 2.0, pad, b, b), Look::Button).icon("redo");
        cells.push(if can_redo {
            redo.act(Action::Redo)
        } else {
            redo.off()
        });
        let done = self.history.iter().position(|h| h.current).unwrap_or(0);
        let total = self.history.len().saturating_sub(1);
        cells.push(
            Cell::new(Rect::new(pad, pad, w - pad * 2.0, b), Look::Counter)
                .text(format!("{done} / {total}")),
        );
        let mut y = pad + b + Space::X1.px();
        let info_w = Layout::MENU_ROW_H;
        let (rows, opening) = self.listing();
        for h in rows
            .into_iter()
            .skip(self.offset)
            .take(HISTORY_ROWS)
            .chain(opening)
        {
            let first = h.n == 0;
            let w_row = if first {
                w - pad * 2.0
            } else {
                w - pad * 2.0 - info_w - 2.0
            };
            let r = Rect::new(pad, y, w_row, Layout::MENU_ROW_H);
            let row = Cell::new(
                r,
                Look::Step {
                    ai: h.ai,
                    undone: h.undone,
                    opening: first,
                },
            )
            .text(h.label.clone())
            .on(h.current);
            // The time of the step on the right (PRJ-011); the opening state has none.
            let row = if first || h.at_ms == 0 {
                row
            } else {
                row.hint(crate::local_time::clock(self.now_ms, h.at_ms))
            };
            cells.push(if h.note {
                row
            } else {
                row.act(Action::Jump(h.n))
            });
            let open = self.open_step == Some(h.n);
            if !first {
                cells.push(
                    Cell::new(
                        Rect::new(r.right() + 2.0, y, info_w, Layout::MENU_ROW_H),
                        Look::StepInfo,
                    )
                    .on(open)
                    .act(Action::StepInfo(h.n)),
                );
            }
            y += Layout::MENU_ROW_H;
            if open && !first {
                let inner = w - pad * 2.0 - Space::X2.px() * 2.0;
                let line = crate::opline::of(h, inner, |t| text_width(t, FontSize::Micro));
                let lines = line.command.len() + line.result.len();
                let height = lines as f32 * CLI_LINE_H + CLI_PAD * 2.0;
                cells.push(
                    Cell::new(
                        Rect::new(pad, y, w - pad * 2.0, height),
                        Look::Cli {
                            ai: h.ai,
                            command: line.command.len(),
                        },
                    )
                    .text(line.lines().collect::<Vec<_>>().join("\n")),
                );
                y += height + Space::X1.px();
            }
        }
        cells
    }

    /// Like the mock: segmented rows for on/off, the method and the value,
    /// then 3連 and AQ side by side.
    fn grid_cells(&self, w: f32) -> Vec<Cell> {
        let pad = Space::X3.px() - SEGMENT_INSET;
        let inner = w - pad * 2.0;
        let (base, triplet) = split_grid(&self.grid);
        let mut cells = Vec::new();
        let mut y = pad;
        let row_h = SEGMENT_H + SEGMENT_INSET * 2.0;
        let gap = Space::X2.px();
        let label = |cells: &mut Vec<Cell>, y: &mut f32, text: &str| {
            cells.push(Cell::new(Rect::new(pad + 2.0, *y, inner, LABEL_H), Look::Label).text(text));
            *y += LABEL_H;
        };
        let on_off = [("オン", true), ("オフ", false)]
            .map(|(text, on)| (text, self.snap == on, Action::Snap(on)));
        segments(&mut cells, Rect::new(pad, y, inner, row_h), &on_off);
        y += row_h + gap;
        label(&mut cells, &mut y, "方式");
        let modes = SnapMode::ALL.map(|m| (m.label(), self.mode == m, Action::Mode(m)));
        segments(&mut cells, Rect::new(pad, y, inner, row_h), &modes);
        y += row_h + gap;
        label(&mut cells, &mut y, "グリッド");
        let values = GRID_VALUES.map(|v| (v, base == v, Action::GridValue(v)));
        segments(&mut cells, Rect::new(pad, y, inner, row_h), &values);
        y += row_h + gap;
        let half = (inner - Space::X1.px()) / 2.0;
        segments(
            &mut cells,
            Rect::new(pad, y, half, row_h),
            &[("3連", triplet, Action::Triplet)],
        );
        segments(
            &mut cells,
            Rect::new(w - pad - half, y, half, row_h),
            &[("AQ", self.aq, Action::AutoQuantize)],
        );
        y += row_h + gap;
        cells.push(
            Cell::new(Rect::new(pad, y, inner, Layout::MENU_ROW_H), Look::Row)
                .text("クオンタイズの設定…")
                .act(Action::Quant(crate::quantize_panel::QAct::Open)),
        );
        cells
    }

    fn song_cells(&self, w: f32) -> Vec<Cell> {
        let pad = Layout::POPUP_PAD;
        let mut cells = Vec::new();
        let mut y = pad;
        let row = |y: f32, text: &str, hint: &str, icon: &'static str| {
            Cell::new(
                Rect::new(pad, y, w - pad * 2.0, Layout::MENU_ROW_H),
                Look::Row,
            )
            .text(text)
            .hint(hint)
            .icon(icon)
        };
        let gap = Space::X2.px();
        let put = |cells: &mut Vec<Cell>, y: &mut f32, cell: Cell, action: Option<Action>| {
            cells.push(match action {
                Some(a) => cell.act(a),
                None => cell.off(),
            });
            *y += Layout::MENU_ROW_H;
        };
        // 新規 / 開く… / DAWproject を開く… / 最近の曲 (the songs, one row each) — / 保存 /
        // 別名で保存… / DAWproject として書き出す… / 版の一覧… — / 書き出し / 曲の設定
        let cell = row(y, "新規", "Ctrl+N", "plus");
        put(&mut cells, &mut y, cell, Some(Action::NewSong));
        let cell = row(y, "開く…", "Ctrl+O", "folder-open");
        put(&mut cells, &mut y, cell, Some(Action::OpenSong));
        let cell = row(y, "DAWproject を開く…", "", "open");
        put(&mut cells, &mut y, cell, Some(Action::OpenDawproject));
        if !self.recent.is_empty() {
            cells.push(
                Cell::new(Rect::new(pad, y + gap, w - pad * 2.0, LABEL_H), Look::Label)
                    .text("最近の曲"),
            );
            y += gap + LABEL_H;
        }
        for (i, r) in self.recent.iter().take(RECENT_ROWS).enumerate() {
            let name = if r.draft {
                format!("{}（保存前）", r.name)
            } else {
                r.name.clone()
            };
            let cell = row(y, &name, &ago(self.now_ms, r.at_ms), "file");
            put(&mut cells, &mut y, cell, Some(Action::OpenRecent(i)));
        }
        y += gap;
        let rest: [(&str, &str, &'static str, Option<Action>); 6] = [
            ("保存", "Ctrl+S", "saved", Some(Action::Save)),
            ("別名で保存…", "Ctrl+Shift+S", "file", Some(Action::SaveAs)),
            (
                "DAWproject として書き出す…",
                "",
                "export",
                Some(Action::ExportDawproject),
            ),
            ("バージョン一覧…", "", "history", Some(Action::ShowVersions)),
            (
                "書き出し",
                "Ctrl+E",
                "export",
                self.can_export.then_some(Action::Export),
            ),
            ("曲の設定", "", "song-settings", Some(Action::SongSettings)),
        ];
        for (i, (text, hint, icon, action)) in rest.into_iter().enumerate() {
            if i == 4 {
                y += gap;
            }
            let cell = row(y, text, hint, icon);
            put(&mut cells, &mut y, cell, action);
        }
        cells
    }

    fn version_cells(&self, w: f32) -> Vec<Cell> {
        let pad = Layout::POPUP_PAD;
        let inner = w - pad * 2.0;
        let mut cells = Vec::new();
        let mut y = pad;
        let head = Rect::new(pad, y, inner, 32.0);
        match &self.naming {
            Some(name) => {
                cells.push(
                    Cell::new(head, Look::Input)
                        .text(name.clone())
                        .icon("saved")
                        .on(true),
                );
            }
            None => {
                let cell = Cell::new(head, Look::Primary)
                    .text("現在の状態に名前を付けて保存")
                    .icon("saved");
                cells.push(if self.has_dir {
                    cell.act(Action::NameVersion)
                } else {
                    cell.off()
                });
            }
        }
        y += head.h + Space::X2.px();
        if self.versions.is_empty() {
            let note = "保存されたバージョンはありません";
            cells.push(
                Cell::new(Rect::new(pad, y, inner, Layout::MENU_ROW_H), Look::Note).text(note),
            );
            return cells;
        }
        let shown = self
            .versions
            .len()
            .saturating_sub(self.offset)
            .min(VERSION_ROWS);
        for (i, v) in self
            .versions
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(shown)
        {
            let r = Rect::new(pad, y, inner, VERSION_ROW_H);
            cells.push(
                Cell::new(
                    r,
                    Look::Version {
                        big: v.named,
                        first: i == self.offset,
                        last: i + 1 == self.offset + shown,
                    },
                )
                .text(if v.name.is_empty() {
                    "自動保存"
                } else {
                    &v.name
                })
                .hint(ago(self.now_ms, v.at_ms))
                .on(self.selected == Some(i))
                .act(Action::SelectVersion(i)),
            );
            y += VERSION_ROW_H;
        }
        if let (Some(_), Some(d)) = (self.selected, &self.detail) {
            y += Space::X2.px();
            let colors = d
                .changed
                .iter()
                .filter_map(|t| d.colors.iter().find(|(id, _)| id == t))
                .map(|(_, hex)| {
                    hex.as_deref()
                        .and_then(crate::song::palette_color)
                        .map(TrackColor::index)
                })
                .collect();
            let r = Rect::new(pad, y, inner, 32.0);
            cells.push(
                Cell::new(r, Look::Detail(colors))
                    .text(format!("{} トラック ・ {} 小節", d.tracks, d.length_bars)),
            );
            y += r.h + Space::X1.px();
            let half = (inner - Space::X1.px()) / 2.0;
            cells.push(
                Cell::new(Rect::new(pad, y, half, 30.0), Look::Primary)
                    .text("このバージョンに戻す")
                    .icon("restore")
                    .act(Action::Restore),
            );
            cells.push(
                Cell::new(
                    Rect::new(pad + half + Space::X1.px(), y, half, 30.0),
                    Look::Button,
                )
                .text("別の曲として開く")
                .icon("open")
                .act(Action::OpenAsSong),
            );
        }
        cells
    }

    fn color_cells(&self) -> Vec<Cell> {
        let pad = Layout::POPUP_PAD;
        let mut cells = Vec::new();
        let swatch = |i: usize, look: Option<usize>| {
            let (col, row) = (i % SWATCH_COLUMNS, i / SWATCH_COLUMNS);
            Cell::new(
                Rect::new(
                    pad + col as f32 * (SWATCH + SWATCH_GAP),
                    pad + row as f32 * (SWATCH + SWATCH_GAP),
                    SWATCH,
                    SWATCH,
                ),
                Look::Swatch(look),
            )
            // A track with no colour of its own shows the first one.
            .on(self.color.unwrap_or(0) == i)
            .act(Action::Color(look))
        };
        for i in 0..TrackColor::all().count() {
            cells.push(swatch(i, Some(i)));
        }
        cells
    }

    /// A plugin that stopped: its name, 再起動 and 外す (PLG-053), and a process of its own
    /// when it took the shared one down. Else 開く and 外す. Then where it runs (PLG-022).
    fn plugin_cells(&self, w: f32) -> Vec<Cell> {
        let pad = Layout::POPUP_PAD;
        let inner = w - pad * 2.0;
        let Some(v) = &self.plugin else {
            return Vec::new();
        };
        let mut cells = Vec::new();
        let mut y = pad;
        let row = |y: f32| Rect::new(pad, y, inner, Layout::MENU_ROW_H);
        if v.crashed {
            cells.push(Cell::new(row(y), Look::Down).text(v.name.clone()));
            y += Layout::MENU_ROW_H + Space::X1.px();
            let half = (inner - Space::X1.px()) / 2.0;
            cells.push(
                Cell::new(Rect::new(pad, y, half, 30.0), Look::Primary)
                    .text("再起動")
                    .icon("restore")
                    .act(Action::Plugin(PlugAct::Restart)),
            );
            cells.push(
                Cell::new(
                    Rect::new(pad + half + Space::X1.px(), y, half, 30.0),
                    Look::Button,
                )
                .text("外す")
                .icon("close")
                .act(Action::Plugin(PlugAct::Remove)),
            );
            y += 30.0 + Space::X1.px();
            if v.suggest {
                cells.push(
                    Cell::new(row(y), Look::Row)
                        .text("専用で動かす")
                        .icon("bolt")
                        .act(Action::Plugin(PlugAct::Mode(PluginIsolation::Dedicated))),
                );
                y += Layout::MENU_ROW_H;
            }
        } else {
            for (text, icon, act) in [
                ("開く", "open", PlugAct::Open),
                ("外す", "close", PlugAct::Remove),
            ] {
                cells.push(
                    Cell::new(row(y), Look::Row)
                        .text(text)
                        .icon(icon)
                        .act(Action::Plugin(act)),
                );
                y += Layout::MENU_ROW_H;
            }
        }
        y += Space::X2.px();
        cells.push(
            Cell::new(Rect::new(pad + 2.0, y, inner, LABEL_H), Look::Label).text("動かす場所"),
        );
        y += LABEL_H;
        let modes = [
            ("アプリ内", PluginIsolation::InProcess),
            ("共有", PluginIsolation::Shared),
            ("専用", PluginIsolation::Dedicated),
        ]
        .map(|(text, m)| (text, v.mode == m, Action::Plugin(PlugAct::Mode(m))));
        let first = cells.len();
        segments(
            &mut cells,
            Rect::new(pad, y, inner, SEGMENT_H + SEGMENT_INSET * 2.0),
            &modes,
        );
        // A built-in plugin runs in the engine only, an ARA plugin in this process only.
        if v.builtin || v.ara {
            for cell in &mut cells[first..] {
                if cell.look == Look::Segment && !cell.on {
                    *cell = cell.clone().off();
                }
            }
        }
        cells
    }
}

/// A tray across `r` with a button of equal width for each item.
pub(crate) fn segments(cells: &mut Vec<Cell>, r: Rect, items: &[(&str, bool, Action)]) {
    cells.push(Cell::new(r, Look::Tray));
    let inset = SEGMENT_INSET;
    let n = items.len() as f32;
    let w = (r.w - inset * 2.0 - (n - 1.0) * inset) / n;
    for (i, (text, on, action)) in items.iter().enumerate() {
        let x = r.x + inset + i as f32 * (w + inset);
        cells.push(
            Cell::new(Rect::new(x, r.y + inset, w, SEGMENT_H), Look::Segment)
                .text(*text)
                .on(*on)
                .act(action.clone()),
        );
    }
}

// -------------------------------------------------------------------- painting

fn center(r: Rect, w: f32, h: f32) -> Rect {
    Rect::new(r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0, w, h)
}

fn icon_at(r: Rect, x: f32) -> Rect {
    Rect::new(
        x,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    )
}

/// The pop-over, from the layout; `hover` is the cell under the pointer.
pub fn paint(pen: &mut Pen, layout: &PopupLayout, hover: Option<usize>) {
    paint_with(pen, layout, hover, &|_, _| {});
}

/// [`paint`], with `over` painted on top of each cell (what moves: the AI's
/// history row while it is being written, AI-036).
pub fn paint_with(
    pen: &mut Pen,
    layout: &PopupLayout,
    hover: Option<usize>,
    over: &dyn Fn(&mut Pen, &Cell),
) {
    let c = pen.c;
    pen.floating(layout.rect, c.paper, Radius::M);
    for (i, cell) in layout.cells.iter().enumerate() {
        paint_cell(pen, cell, hover == Some(i));
        over(pen, cell);
    }
    if let Some(cell) = hover.and_then(|i| layout.cells.get(i)) {
        tip(pen, cell, layout.rect);
    }
}

/// The balloon over a cell with a tip, kept inside the pop-over `within`. A tip of two lines
/// (`\n`) names the cell on the first and says more, quieter, on the second (UI-303).
fn tip(pen: &mut Pen, cell: &Cell, within: Rect) {
    if cell.tip.is_empty() {
        return;
    }
    let pad = Space::X2.px();
    let lines: Vec<&str> = cell.tip.lines().collect();
    let w = lines
        .iter()
        .map(|l| text_width(l, FontSize::Caption))
        .fold(0.0, f32::max)
        + pad * 2.0;
    let r = tip_rect(cell.rect, within, w, lines.len());
    pen.floating(r, pen.c.surface_3, Radius::S);
    for (n, line) in lines.iter().enumerate() {
        let (weight, ink) = if n == 0 {
            (Weight::Medium, pen.c.ink_strong)
        } else {
            (Weight::Regular, pen.c.ink)
        };
        pen.text(
            line,
            FontSize::Caption,
            weight,
            ink,
            Rect::new(
                r.x + pad,
                r.y + TIP_PAD + n as f32 * TIP_LINE,
                r.w - pad,
                TIP_LINE,
            ),
            Align::Left,
        );
    }
}

/// A line of a balloon, and the room above and below its lines.
const TIP_LINE: f32 = 16.0;
const TIP_PAD: f32 = 4.0;

/// Where a balloon `w` wide with `lines` lines goes: over the cell, or under it when the
/// pop-over has no room above (the version row of 「Xirius について」).
pub(crate) fn tip_rect(cell: Rect, within: Rect, w: f32, lines: usize) -> Rect {
    let h = TIP_PAD * 2.0 + TIP_LINE * lines.max(1) as f32;
    let edge = Space::X1.px();
    let x = (cell.x + (cell.w - w) / 2.0)
        .min(within.right() - edge - w)
        .max(within.x + edge);
    let above = cell.y - h - edge;
    let y = if above < within.y + edge {
        cell.y + cell.h + edge
    } else {
        above
    };
    Rect::new(x, y, w, h)
}

/// `s` as it fits in `w` of a row's name: whole, or its front and 「…」.
fn fit(pen: &mut Pen, s: &str, w: f32) -> String {
    let wide = |pen: &mut Pen, t: &str| pen.measure(t, FontSize::Small, Weight::Medium);
    if wide(pen, s) <= w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    // The most characters that fit with 「…」 after them.
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let t: String = chars[..mid].iter().chain(['…'].iter()).collect();
        if wide(pen, &t) <= w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let kept: String = chars[..lo].iter().collect();
    format!("{}…", kept.trim_end())
}

/// The round badge at the head of a history row.
pub fn step_badge(r: Rect) -> Rect {
    Rect::new(r.x + Space::X2.px(), r.y + (r.h - 20.0) / 2.0, 20.0, 20.0)
}

fn paint_cell(pen: &mut Pen, cell: &Cell, hot: bool) {
    let c = pen.c;
    let r = cell.rect;
    let pad = Space::X2.px();
    let hot = hot && cell.live();
    let strong = if cell.enabled { c.ink_strong } else { c.ink };
    let soft = c.ink;
    match &cell.look {
        Look::Xp(x) => crate::export::paint_cell(pen, cell, x, hot),
        Look::Ai(x) => crate::ai::paint_cell(pen, cell, x, hot),
        Look::Auto(a) => crate::autovalue::paint_cell(pen, cell, *a),
        Look::Quant(q) => crate::quantize_panel::paint_cell(pen, cell, *q),
        Look::Meter(m) => crate::meter_popup::paint_cell(pen, cell, *m),
        Look::Cpu(l) => crate::cpu_list::paint_cell(pen, cell, l, hot),
        Look::Field => crate::context::fields::paint_cell(pen, cell),
        Look::About(a) => crate::about::paint_cell(pen, cell, a, hot),
        Look::Clean(l) => crate::cleanup_dialog::paint_cell(pen, cell, *l),
        Look::Label => {
            pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Semibold,
                c.ink,
                r,
                Align::Left,
            );
        }
        Look::Note => {
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                Rect::new(r.x + pad, r.y, r.w, r.h),
                Align::Left,
            );
            // How many, at the right (the list of what a `.dawproject` could not carry).
            if !cell.hint.is_empty() {
                pen.text(
                    &cell.hint,
                    FontSize::Caption,
                    Weight::Regular,
                    soft,
                    Rect::new(r.x, r.y, r.w - pad, r.h),
                    Align::Right,
                );
            }
        }
        Look::Counter => {
            pen.digits(
                &cell.text,
                FontSize::Micro,
                Weight::Medium,
                c.ink,
                Rect::new(r.x, r.y, r.w - pad, r.h),
                Align::Right,
            );
        }
        Look::Row => {
            if cell.on || cell.cursor {
                pen.round(r, c.sel, Radius::S);
            } else if hot {
                pen.round(r, c.hover, Radius::S);
            }
            let mut x = r.x + pad;
            if let Some(icon) = cell.icon {
                let chevron = icon == "chevron-right";
                if !chevron {
                    if icon != BLANK {
                        pen.icon(icon, icon_at(r, x), strong);
                    }
                    x += Layout::ICON_SIZE + pad;
                }
            }
            if let Some(color) = cell.mark {
                let m = crate::studio::RowParts::SWATCH_MARK;
                let at = Rect::new(x, r.y + (r.h - m) / 2.0, m, m);
                pen.round(at, c.tracks[color % c.tracks.len()], Radius::Xs);
                x += m + pad;
            }
            // The name ends before what the row shows at its right; a longer one is cut there
            // with 「…」 (the pick lists keep their width, UI-297).
            let end = if cell.icon == Some("chevron-right")
                || (cell.hint.is_empty() && (cell.on || cell.locked))
            {
                r.right() - pad - Layout::ICON_SIZE - pad
            } else if cell.hint.is_empty() {
                r.right() - pad
            } else {
                r.right()
            };
            let room = Rect::new(x, r.y, end - x, r.h);
            let name = fit(pen, &cell.text, room.w);
            pen.mask(room, |pen| {
                pen.text(
                    &name,
                    FontSize::Small,
                    Weight::Medium,
                    strong,
                    Rect::new(x, r.y, r.w, r.h),
                    Align::Left,
                );
            });
            if cell.icon == Some("chevron-right") {
                pen.icon(
                    "chevron-right",
                    icon_at(r, r.right() - pad - Layout::ICON_SIZE),
                    soft,
                );
            } else if cell.locked {
                // The lock says the row cannot run now; a hint sits before it.
                let lock = r.right() - pad - Layout::ICON_SIZE;
                pen.icon("lock", icon_at(r, lock), soft);
                if !cell.hint.is_empty() {
                    pen.text(
                        &cell.hint,
                        FontSize::Caption,
                        Weight::Regular,
                        soft,
                        Rect::new(r.x, r.y, lock - pad - r.x, r.h),
                        Align::Right,
                    );
                }
            } else if !cell.hint.is_empty() {
                pen.text(
                    &cell.hint,
                    FontSize::Caption,
                    Weight::Regular,
                    soft,
                    Rect::new(r.x, r.y, r.w - pad, r.h),
                    Align::Right,
                );
            } else if cell.on {
                pen.icon(
                    "check",
                    icon_at(r, r.right() - pad - Layout::ICON_SIZE),
                    strong,
                );
            }
        }
        Look::Chip => {
            let (face, ink) = if cell.on {
                (c.fill, c.on_fill)
            } else if hot {
                (c.hover, c.ink_strong)
            } else {
                (c.surface_3, soft)
            };
            pen.round(r, face, Radius::S);
            pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Medium,
                ink,
                r,
                Align::Center,
            );
        }
        Look::Tray => pen.round(r, c.surface_2, Radius::S),
        Look::Down => {
            pen.icon("alert", icon_at(r, r.x + pad), c.alert);
            let x = r.x + pad + Layout::ICON_SIZE + pad;
            let text_r = Rect::new(x, r.y, r.right() - x, r.h);
            pen.mask(text_r, |pen| {
                pen.text(
                    &cell.text,
                    FontSize::Small,
                    Weight::Semibold,
                    c.ink_strong,
                    text_r,
                    Align::Left,
                );
            });
        }
        Look::Segment => {
            let ink = if cell.on {
                pen.round(r, c.fill, Radius::Xs);
                c.on_fill
            } else if hot {
                pen.round(r, c.surface_3, Radius::Xs);
                c.ink_strong
            } else {
                soft
            };
            pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Medium,
                ink,
                r,
                Align::Center,
            );
        }
        Look::Button | Look::Primary => {
            let primary = cell.look == Look::Primary;
            let (face, ink) = if !cell.enabled {
                (c.surface_3, c.ink_on_toggle)
            } else if primary {
                (if hot { c.fill_hover } else { c.fill }, c.on_fill)
            } else if hot {
                (c.hover, c.ink_strong)
            } else {
                (c.surface_3, c.ink_strong)
            };
            pen.round(r, face, Radius::S);
            let text_w = if cell.text.is_empty() {
                0.0
            } else {
                text_width(&cell.text, FontSize::Small)
            };
            let icon_w = if cell.icon.is_some() {
                Layout::ICON_SIZE
            } else {
                0.0
            };
            let gap = if icon_w > 0.0 && text_w > 0.0 {
                Space::X2.px()
            } else {
                0.0
            };
            let total = icon_w + gap + text_w;
            let x = r.x + (r.w - total) / 2.0;
            if let Some(icon) = cell.icon {
                pen.icon(icon, icon_at(r, x), ink);
            }
            if !cell.text.is_empty() {
                pen.text(
                    &cell.text,
                    FontSize::Small,
                    Weight::Medium,
                    ink,
                    Rect::new(x + icon_w + gap, r.y, r.w, r.h),
                    Align::Left,
                );
            }
        }
        Look::Swatch(look) => {
            // The colour fills its cell; the current one carries a check, and the one
            // under the pointer a ring around it.
            if hot {
                pen.round(r, c.ring, Radius::S);
            }
            let s = if hot {
                center(r, r.w - 4.0, r.h - 4.0)
            } else {
                r
            };
            match look {
                Some(i) => {
                    pen.round(s, c.tracks[i % c.tracks.len()], Radius::S);
                    if cell.on {
                        pen.icon(
                            "check",
                            center(s, Layout::ICON_SIZE, Layout::ICON_SIZE),
                            c.ink_on_track,
                        );
                    }
                }
                None => {
                    pen.round(s, c.surface_3, Radius::S);
                    pen.icon("close", center(s, 14.0, 14.0), c.ink);
                }
            }
        }
        Look::Step {
            ai,
            undone,
            opening,
        } => {
            if cell.on {
                pen.round(r, c.ring, Radius::S);
                pen.round(
                    Rect::new(r.x + 1.5, r.y + 1.5, r.w - 3.0, r.h - 3.0),
                    c.sel,
                    Radius::S,
                );
            } else if hot {
                pen.round(r, c.hover, Radius::S);
            }
            let badge = step_badge(r);
            pen.round(badge, c.surface_3, Radius::Pill);
            let (icon, ink) = if *opening {
                ("file", c.ink)
            } else if *ai {
                ("ai", c.ink_ai)
            } else {
                ("person", c.ink)
            };
            pen.icon(icon, center(badge, 14.0, 14.0), ink);
            let x = badge.right() + pad;
            let hint_w = if cell.hint.is_empty() {
                0.0
            } else {
                text_width(&cell.hint, FontSize::Micro) + pad
            };
            let text_r = Rect::new(x, r.y, r.right() - hint_w - pad - x, r.h);
            let ink = if *undone { c.ink } else { c.ink_strong };
            pen.mask(text_r, |pen| {
                let w = pen.text(
                    &cell.text,
                    FontSize::Small,
                    Weight::Regular,
                    ink,
                    text_r,
                    Align::Left,
                );
                if *undone {
                    pen.fill(Rect::new(x, r.y + r.h / 2.0, w, 1.0), ink);
                }
            });
            if !cell.hint.is_empty() {
                pen.digits(
                    &cell.hint,
                    FontSize::Micro,
                    Weight::Regular,
                    c.ink,
                    Rect::new(r.x, r.y, r.w - pad, r.h),
                    Align::Right,
                );
            }
        }
        Look::StepInfo => {
            if cell.on {
                pen.round(r, c.sel, Radius::S);
            } else if hot {
                pen.round(r, c.hover, Radius::S);
            }
            let ink = if cell.on { c.ink_strong } else { c.ink };
            pen.icon("prompt", center(r, 14.0, 14.0), ink);
        }
        Look::Cli { ai, command } => {
            pen.round(r, c.surface_0, Radius::S);
            for (i, line) in cell.text.split('\n').enumerate() {
                let ink = match (i < *command, *ai) {
                    (true, true) => c.ink_ai,
                    (true, false) => c.ink_strong,
                    (false, _) => c.ink,
                };
                let at = Rect::new(
                    r.x + pad,
                    r.y + CLI_PAD + i as f32 * CLI_LINE_H,
                    r.w - pad * 2.0,
                    CLI_LINE_H,
                );
                pen.digits(line, FontSize::Micro, Weight::Regular, ink, at, Align::Left);
            }
        }
        Look::Version { big, first, last } => {
            if cell.on {
                pen.round(r, c.sel, Radius::S);
            } else if hot {
                pen.round(r, c.hover, Radius::S);
            }
            // The timeline: a thin column through the dots.
            let cx = r.x + 16.0;
            let top = if *first { r.y + r.h / 2.0 } else { r.y };
            let bottom = if *last { r.y + r.h / 2.0 } else { r.bottom() };
            pen.fill(Rect::new(cx - 1.0, top, 2.0, bottom - top), c.surface_3);
            let d = if *big { 12.0 } else { 6.0 };
            let dot = Rect::new(cx - d / 2.0, r.y + (r.h - d) / 2.0, d, d);
            pen.round(
                dot,
                if *big { c.fill } else { c.ink_on_toggle },
                Radius::Pill,
            );
            let x = r.x + 36.0;
            pen.text(
                &cell.text,
                FontSize::Small,
                if *big {
                    Weight::Semibold
                } else {
                    Weight::Regular
                },
                c.ink_strong,
                Rect::new(x, r.y + 3.0, r.w, 18.0),
                Align::Left,
            );
            pen.text(
                &cell.hint,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(x, r.y + 20.0, r.w, 16.0),
                Align::Left,
            );
        }
        Look::Detail(colors) => {
            pen.round(r, c.surface_3, Radius::S);
            let w = pen.text(
                &cell.text,
                FontSize::Caption,
                Weight::Medium,
                c.ink_strong,
                Rect::new(r.x + pad, r.y, r.w, r.h),
                Align::Left,
            );
            let mut x = r.x + pad + w + Space::X3.px();
            for look in colors.iter().take(12) {
                let dot = Rect::new(x, r.y + (r.h - 8.0) / 2.0, 8.0, 8.0);
                let face = look.map_or(c.ink_on_toggle, |i| c.tracks[i % c.tracks.len()]);
                pen.round(dot, face, Radius::Pill);
                x += 11.0;
            }
        }
        Look::Thumb => pen.round(r, c.list_thumb, Radius::Xs),
        Look::Status => {
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                r,
                Align::Center,
            );
        }
        Look::Input => {
            pen.round(r, c.well, Radius::S);
            pen.icon(cell.icon.unwrap_or("saved"), icon_at(r, r.x + pad), c.ink);
            let tx = r.x + pad + Layout::ICON_SIZE + pad;
            let (text, ink) = if cell.text.is_empty() {
                (
                    if cell.icon == Some("search") {
                        "検索"
                    } else {
                        "名前を入力"
                    },
                    c.ink_on_toggle,
                )
            } else {
                (cell.text.as_str(), c.ink_strong)
            };
            let w = pen.text(
                text,
                FontSize::Small,
                Weight::Regular,
                ink,
                Rect::new(tx, r.y, r.w, r.h),
                Align::Left,
            );
            let caret = if cell.text.is_empty() { 0.0 } else { w };
            if cell.on {
                pen.fill(
                    Rect::new(tx + caret + 1.0, r.y + 8.0, 1.5, r.h - 16.0),
                    c.ink_strong,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        n: u64,
        label: &str,
        command: &str,
        ai: bool,
        current: bool,
        undone: bool,
    ) -> HistoryItem {
        HistoryItem {
            n,
            label: label.to_string(),
            command: command.to_string(),
            ai,
            current,
            undone,
            args: serde_json::Value::Null,
            summary: String::new(),
            at_ms: n * 1000,
            note: false,
        }
    }

    fn history_popup() -> Popup {
        let mut p = Popup::new(PopupKind::History, Rect::new(1000.0, -34.0, 28.0, 28.0));
        p.history = vec![
            item(0, "開いた時", "", false, false, false),
            item(1, "Drums をミュート", "track.mute", false, false, false),
            item(2, "Bass の色を変える", "track.color.set", true, true, false),
            item(3, "Keys をソロ", "track.solo", false, false, true),
        ];
        p
    }

    fn texts(l: &PopupLayout) -> Vec<String> {
        l.cells.iter().map(|c| c.text.clone()).collect()
    }

    #[test]
    fn history_lists_newest_first_and_the_opening_state_last() {
        let l = history_popup().layout((1440.0, 860.0));
        let steps: Vec<&Cell> = l
            .cells
            .iter()
            .filter(|c| matches!(c.look, Look::Step { .. }))
            .collect();
        assert_eq!(
            steps.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
            [
                "Keys をソロ",
                "Bass の色を変える",
                "Drums をミュート",
                "開いた時"
            ]
        );
        // Each step shows its time; the opening state none.
        let now = 1_791_000_000_000;
        let mut p = history_popup();
        p.now_ms = now;
        for h in &mut p.history {
            h.at_ms = if h.n == 0 {
                0
            } else {
                now - (3 - h.n) * 60_000
            };
        }
        let l = p.layout((1440.0, 860.0));
        let hints: Vec<String> = l
            .cells
            .iter()
            .filter(|c| matches!(c.look, Look::Step { .. }))
            .map(|c| c.hint.clone())
            .collect();
        assert_eq!(
            hints,
            [
                crate::local_time::clock(now, now),
                crate::local_time::clock(now, now - 60_000),
                crate::local_time::clock(now, now - 120_000),
                String::new(),
            ]
        );
        assert!(hints[0].contains(':') && hints[0] != hints[2]);
        assert!(steps[1].on && matches!(steps[1].look, Look::Step { ai: true, .. }));
        assert_eq!(steps[0].action, Some(Action::Jump(3)));
    }

    #[test]
    fn history_head_has_undo_redo_and_a_counter() {
        let l = history_popup().layout((1440.0, 860.0));
        assert_eq!(l.cells[0].action, Some(Action::Undo));
        assert_eq!(l.cells[1].action, Some(Action::Redo));
        assert_eq!(l.cells[2].text, "2 / 3");
        // At the opening state nothing can be undone.
        let mut p = history_popup();
        for h in &mut p.history {
            h.current = h.n == 0;
            h.undone = h.n > 0;
        }
        let l = p.layout((1440.0, 860.0));
        assert!(!l.cells[0].live() && l.cells[1].live());
        assert_eq!(l.cells[2].text, "0 / 3");
    }

    #[test]
    fn history_is_right_aligned_under_its_button_and_stays_in_the_window() {
        let p = history_popup();
        let l = p.layout((1440.0, 860.0));
        assert_eq!(l.rect.right(), p.anchor.right());
        assert!(l.rect.y >= p.anchor.bottom());
        for c in &l.cells {
            assert!(
                c.rect.x >= l.rect.x && c.rect.right() <= l.rect.right() + 0.5,
                "{c:?}"
            );
            assert!(c.rect.bottom() <= l.rect.bottom());
        }
        let narrow = p.layout((900.0, 860.0));
        assert!(narrow.rect.right() <= 900.0);
    }

    fn cli_cell(l: &PopupLayout) -> Option<&Cell> {
        l.cells.iter().find(|c| matches!(c.look, Look::Cli { .. }))
    }

    #[test]
    fn every_operation_row_has_a_button_that_opens_its_one_line() {
        let mut p = history_popup();
        let l = p.layout((1440.0, 860.0));
        let buttons: Vec<&Cell> = l
            .cells
            .iter()
            .filter(|c| matches!(c.look, Look::StepInfo))
            .collect();
        // Three operations, not the opening state.
        assert_eq!(buttons.len(), 3);
        assert_eq!(buttons[0].action, Some(Action::StepInfo(3)));
        assert!(cli_cell(&l).is_none());
        // The button does not sit on the row it belongs to, so each is hit on its own.
        let rows: Vec<&Cell> = l
            .cells
            .iter()
            .filter(|c| matches!(c.look, Look::Step { .. }))
            .collect();
        assert!(rows[0].rect.right() <= buttons[0].rect.x);
        let b = buttons[1].rect;
        let hit = l.item_at(b.x + b.w / 2.0, b.y + b.h / 2.0).unwrap();
        assert_eq!(l.cells[hit].action, Some(Action::StepInfo(2)));

        p.open_step = Some(2);
        p.history[2].command = "track.color.set".to_string();
        p.history[2].args = serde_json::json!({"track": "t1", "color": "#ff0000"});
        p.history[2].summary = "トラック 1 件".to_string();
        let open = p.layout((1440.0, 860.0));
        let cli = cli_cell(&open).expect("the detail is open");
        assert!(matches!(cli.look, Look::Cli { ai: true, .. }));
        // The command line is broken to the width of the popup.
        assert!(
            cli.text
                .replace('\n', " ")
                .starts_with("AI mcp › xirius track.color.set --track t1 --color #ff0000")
        );
        assert!(cli.text.contains("→ トラック 1 件"));
        assert!(cli.text.ends_with("取り消し 1 操作"));
        // It opens under its own row, and the rows after it move down.
        let own = open
            .cells
            .iter()
            .find(|c| c.action == Some(Action::Jump(2)))
            .unwrap();
        assert_eq!(cli.rect.y, own.rect.bottom());
        let after = open
            .cells
            .iter()
            .find(|c| c.action == Some(Action::Jump(1)))
            .unwrap();
        assert!(after.rect.y >= cli.rect.bottom());
        assert!(open.rect.h > l.rect.h);
        for c in &open.cells {
            assert!(
                c.rect.right() <= open.rect.right() + 0.5 && c.rect.bottom() <= open.rect.bottom()
            );
        }
    }

    #[test]
    fn a_change_of_the_ai_permission_is_a_line_among_the_steps_by_time() {
        let mut p = history_popup();
        let mut note = item(
            crate::song::NOTE_N,
            "AI の権限を変更",
            "ai.permission.set",
            false,
            false,
            false,
        );
        note.note = true;
        note.args = serde_json::json!({"level": "read"});
        note.summary = "読み取りのみ".to_string();
        // After step 2 (2 s) and before step 3 (3 s).
        note.at_ms = 2500;
        p.notes = vec![note];
        p.open_step = Some(crate::song::NOTE_N);
        let l = p.layout((1440.0, 860.0));
        let rows: Vec<&str> = l
            .cells
            .iter()
            .filter(|c| matches!(c.look, Look::Step { .. }))
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(
            rows,
            [
                "Keys をソロ",
                "AI の権限を変更",
                "Bass の色を変える",
                "Drums をミュート",
                "開いた時"
            ]
        );
        // The line cannot be jumped to, but its detail opens.
        let line = l
            .cells
            .iter()
            .find(|c| c.text == "AI の権限を変更")
            .unwrap();
        assert!(!line.live());
        let cli = cli_cell(&l).unwrap();
        assert_eq!(
            cli.text,
            "$ xirius ai.permission.set --level read\n→ 読み取りのみ"
        );
    }

    #[test]
    fn a_long_history_shows_only_the_newest_rows_and_the_opening_state() {
        let mut p = Popup::new(PopupKind::History, Rect::new(900.0, -34.0, 28.0, 28.0));
        p.history.push(item(0, "開いた時", "", false, false, false));
        for n in 1..=30 {
            p.history
                .push(item(n, "step", "track.mute", false, n == 30, false));
        }
        let l = p.layout((1440.0, 860.0));
        let steps = l
            .cells
            .iter()
            .filter(|c| matches!(c.look, Look::Step { .. }))
            .count();
        assert_eq!(steps, HISTORY_ROWS + 1);
    }

    #[test]
    fn grid_has_mode_method_value_triplet_and_aq() {
        let mut p = Popup::new(PopupKind::Grid, Rect::new(700.0, -34.0, 90.0, 28.0));
        p.grid = "1/8t".to_string();
        p.snap = true;
        p.mode = SnapMode::Auto;
        p.aq = true;
        let l = p.layout((1440.0, 860.0));
        let live: Vec<_> = l
            .cells
            .iter()
            .filter(|c| c.live())
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(
            live,
            [
                "オン",
                "オフ",
                "自動",
                "小節",
                "クオンタイズ",
                "フレーム",
                "1/1",
                "1/2",
                "1/4",
                "1/8",
                "1/16",
                "1/32",
                "1/64",
                "3連",
                "AQ",
                "クオンタイズの設定…"
            ]
        );
        let on: Vec<_> = l
            .cells
            .iter()
            .filter(|c| c.on)
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(on, ["オン", "自動", "1/8", "3連", "AQ"]);
        // Each segmented row is one line inside the pop-over, like the mock.
        let ys: std::collections::BTreeSet<_> = l
            .cells
            .iter()
            .filter(|c| c.look == Look::Segment)
            .map(|c| c.rect.y as i32)
            .collect();
        assert_eq!(ys.len(), 4);
        assert!(!l.cells.iter().any(|c| c.text == "モード"));
        for c in &l.cells {
            assert!(c.rect.right() <= l.rect.right(), "{c:?}");
        }
    }

    #[test]
    fn the_grid_value_is_split_into_base_and_triplet() {
        assert_eq!(split_grid("1/16"), ("1/16", false));
        assert_eq!(split_grid("1/8t"), ("1/8", true));
    }

    fn recent(name: &str, draft: bool) -> crate::keep::RecentSong {
        crate::keep::RecentSong {
            dir: std::path::PathBuf::from(name),
            name: name.to_string(),
            at_ms: 0,
            draft,
        }
    }

    #[test]
    fn the_song_menu_is_in_the_agreed_order_and_marks_what_core_lacks() {
        let mut p = Popup::new(PopupKind::Song, Rect::new(40.0, -34.0, 160.0, 28.0));
        p.can_export = false;
        p.recent = vec![recent("夏の曲", false), recent("無題", true)];
        p.now_ms = 3 * 60 * 1000;
        let l = p.layout((1440.0, 860.0));
        assert_eq!(
            texts(&l),
            [
                "新規",
                "開く…",
                "DAWproject を開く…",
                "最近の曲",
                "夏の曲",
                "無題（保存前）",
                "保存",
                "別名で保存…",
                "DAWproject として書き出す…",
                "バージョン一覧…",
                "書き出し",
                "曲の設定"
            ]
        );
        let live: Vec<_> = l
            .cells
            .iter()
            .filter(|c| c.live())
            .map(|c| (c.text.as_str(), c.action.clone().unwrap()))
            .collect();
        assert_eq!(
            live,
            [
                ("新規", Action::NewSong),
                ("開く…", Action::OpenSong),
                ("DAWproject を開く…", Action::OpenDawproject),
                ("夏の曲", Action::OpenRecent(0)),
                ("無題（保存前）", Action::OpenRecent(1)),
                ("保存", Action::Save),
                ("別名で保存…", Action::SaveAs),
                ("DAWproject として書き出す…", Action::ExportDawproject),
                ("バージョン一覧…", Action::ShowVersions),
                ("曲の設定", Action::SongSettings),
            ]
        );
        let by = |t: &str| l.cells.iter().find(|c| c.text == t).unwrap();
        assert_eq!(by("保存").hint, "Ctrl+S");
        assert_eq!(by("別名で保存…").hint, "Ctrl+Shift+S");
        assert_eq!(by("夏の曲").hint, "3 分前");
        p.can_export = true;
        assert!(
            p.layout((1440.0, 860.0))
                .cells
                .iter()
                .any(|c| c.text == "書き出し" && c.live())
        );
        // Groups are told apart by a gap, not a line.
        assert!(by("保存").rect.y - by("無題（保存前）").rect.bottom() > 0.0);
        assert!(by("書き出し").rect.y - by("バージョン一覧…").rect.bottom() > 0.0);
        assert_eq!(by("新規").rect.bottom(), by("開く…").rect.y);
    }

    #[test]
    fn with_no_recent_songs_the_menu_has_no_rows_for_them_and_no_note() {
        let p = Popup::new(PopupKind::Song, Rect::new(40.0, -34.0, 160.0, 28.0));
        let l = p.layout((1440.0, 860.0));
        assert!(l.cells.iter().all(|c| c.look != Look::Note));
        assert!(l.cells.iter().all(|c| c.text != "最近の曲"));
        assert!(
            !l.cells
                .iter()
                .any(|c| matches!(c.action, Some(Action::OpenRecent(_))))
        );
    }

    #[test]
    fn the_song_menu_lists_at_most_the_newest_few() {
        let mut p = Popup::new(PopupKind::Song, Rect::new(40.0, -34.0, 160.0, 28.0));
        p.recent = (0..9).map(|i| recent(&format!("曲{i}"), false)).collect();
        let l = p.layout((1440.0, 860.0));
        let n = l
            .cells
            .iter()
            .filter(|c| matches!(c.action, Some(Action::OpenRecent(_))))
            .count();
        assert_eq!(n, RECENT_ROWS);
        assert!(l.rect.bottom() <= 860.0);
    }

    fn versions_popup() -> Popup {
        let mut p = Popup::new(PopupKind::Versions, Rect::new(200.0, -34.0, 80.0, 28.0));
        p.has_dir = true;
        p.now_ms = 10 * 60 * 60 * 1000;
        p.versions = vec![
            VersionItem {
                id: "c".into(),
                name: "ミックス前".into(),
                named: true,
                at_ms: p.now_ms - 5 * 60 * 1000,
            },
            VersionItem {
                id: "b".into(),
                name: String::new(),
                named: false,
                at_ms: p.now_ms - 2 * 3600 * 1000,
            },
            VersionItem {
                id: "a".into(),
                name: "最初".into(),
                named: true,
                at_ms: 0,
            },
        ];
        p
    }

    #[test]
    fn the_version_list_has_a_naming_button_and_a_timeline_with_big_and_small_dots() {
        let l = versions_popup().layout((1440.0, 860.0));
        assert_eq!(l.cells[0].text, "現在の状態に名前を付けて保存");
        assert_eq!(l.cells[0].action, Some(Action::NameVersion));
        let dots: Vec<bool> = l
            .cells
            .iter()
            .filter_map(|c| match c.look {
                Look::Version { big, .. } => Some(big),
                _ => None,
            })
            .collect();
        assert_eq!(dots, [true, false, true]);
        assert_eq!(l.cells[1].hint, "5 分前");
        assert_eq!(l.cells[2].hint, "2 時間前");
        assert_eq!(l.cells[2].text, "自動保存");
    }

    #[test]
    fn a_selected_version_shows_its_detail_and_the_two_actions() {
        let mut p = versions_popup();
        assert!(
            p.layout((1440.0, 860.0))
                .cells
                .iter()
                .all(|c| c.action != Some(Action::Restore))
        );
        p.selected = Some(0);
        p.detail = Some(VersionDetail {
            tracks: 6,
            length_bars: 24,
            changed: vec![],
            colors: vec![],
        });
        let l = p.layout((1440.0, 860.0));
        let detail = l
            .cells
            .iter()
            .find(|c| matches!(c.look, Look::Detail(_)))
            .expect("detail");
        assert_eq!(detail.text, "6 トラック ・ 24 小節");
        let actions: Vec<_> = l.cells.iter().filter_map(|c| c.action.clone()).collect();
        assert!(actions.contains(&Action::Restore) && actions.contains(&Action::OpenAsSong));
    }

    #[test]
    fn naming_a_version_turns_the_button_into_an_input_that_takes_typing() {
        let mut p = versions_popup();
        p.naming = Some(String::new());
        assert!(p.typing());
        assert!(p.typed("a", Some("a")));
        assert!(p.typed("b", Some("b")));
        assert!(p.typed("backspace", None));
        assert_eq!(p.naming.as_deref(), Some("a"));
        assert_eq!(p.layout((1440.0, 860.0)).cells[0].look, Look::Input);
        assert!(p.typed("escape", None));
        assert!(!p.typing());
        assert!(!p.typed("a", Some("a")));
    }

    #[test]
    fn a_song_without_a_folder_cannot_name_versions() {
        let mut p = versions_popup();
        p.has_dir = false;
        p.versions.clear();
        let l = p.layout((1440.0, 860.0));
        assert!(!l.cells[0].live());
        assert_eq!(l.cells[1].text, "保存されたバージョンはありません");
    }

    #[test]
    fn colour_has_one_swatch_per_palette_colour() {
        let mut p = Popup::new(
            PopupKind::Color(TrackId::MASTER),
            Rect::new(20.0, 100.0, 24.0, 24.0),
        );
        p.color = Some(3);
        let l = p.layout((1440.0, 860.0));
        assert_eq!(l.cells.len(), TrackColor::all().count());
        assert_eq!(l.cells.iter().filter(|c| c.on).count(), 1);
        assert!(l.cells[3].on);
        for c in &l.cells {
            assert!(c.rect.right() <= l.rect.right() && c.rect.bottom() <= l.rect.bottom());
        }
    }

    #[test]
    fn output_lists_its_choices_and_marks_the_current_one() {
        let mut p = Popup::new(
            PopupKind::Output(TrackId::MASTER),
            Rect::new(20.0, 100.0, 160.0, 22.0),
        );
        p.pick = Some(crate::pick_list::PickList::of_choices(
            vec![
                Choice {
                    to: "default".into(),
                    label: "既定".into(),
                },
                Choice {
                    to: "master".into(),
                    label: "マスター".into(),
                },
                Choice {
                    to: "t9".into(),
                    label: "ドラムバス".into(),
                },
            ],
            "t9",
            "output",
            |to| Action::Output(to.to_string()),
        ));
        let l = p.layout((1440.0, 860.0));
        // The list menu's rows come after its search box (UI-320).
        assert_eq!(texts(&l), ["", "既定", "マスター", "ドラムバス"]);
        assert_eq!(
            l.cells.iter().position(|c| c.look == Look::Row && c.on),
            Some(3)
        );
        assert_eq!(l.cells[2].action, Some(Action::Output("master".into())));
    }

    #[test]
    fn the_browser_view_choice_lists_the_five_views() {
        let rows = |views: &[View], view: View| {
            crate::pick_list::PickList::of(
                views
                    .iter()
                    .map(|v| {
                        let row = crate::pick_list::PickRow::new(v.label())
                            .on(*v == view)
                            .act(Action::View(*v));
                        match v.icon() {
                            Some(icon) => row.icon(icon),
                            None => row,
                        }
                    })
                    .collect(),
            )
        };
        let mut p = Popup::new(PopupKind::BrowserView, Rect::new(200.0, 20.0, 28.0, 28.0));
        p.view = View::Vendor;
        p.pick = Some(rows(&View::ALL, p.view));
        let l = p.layout((1440.0, 860.0));
        // The first cell is the list menu's search box (UI-320).
        assert_eq!(
            texts(&l),
            ["", "種類別", "ベンダー別", "形式別", "名前順", "お気に入り"]
        );
        assert_eq!(
            l.cells.iter().position(|c| c.look == Look::Row && c.on),
            Some(2)
        );
        // The song files order only by kind or by name.
        p.views = View::of(crate::browser::RailTab::SongFiles);
        p.view = View::Name;
        p.pick = Some(rows(p.views, p.view));
        let l = p.layout((1440.0, 860.0));
        assert_eq!(texts(&l), ["", "種類別", "名前順"]);
        assert_eq!(
            l.cells.iter().position(|c| c.look == Look::Row && c.on),
            Some(2)
        );
    }

    #[test]
    fn a_popup_near_the_bottom_moves_up_to_stay_in_the_window() {
        let p = Popup::new(PopupKind::BrowserView, Rect::new(200.0, 440.0, 28.0, 28.0));
        let l = p.layout((1440.0, 480.0));
        assert!(l.rect.bottom() <= 480.0);
    }

    #[test]
    fn a_dim_cell_with_a_tip_is_found_but_does_nothing() {
        let l = PopupLayout {
            rect: Rect::new(0.0, 0.0, 200.0, 100.0),
            cells: vec![
                Cell {
                    tip: "why".into(),
                    ..Cell::new(Rect::new(10.0, 10.0, 40.0, 28.0), Look::Note)
                        .text("96")
                        .off()
                },
                Cell::new(Rect::new(60.0, 10.0, 40.0, 28.0), Look::Note)
                    .text("48")
                    .off(),
            ],
        };
        assert_eq!(l.item_at(20.0, 20.0), Some(0));
        assert!(!l.cells[0].live());
        assert_eq!(l.item_at(70.0, 20.0), None);
    }

    #[test]
    fn item_at_finds_only_live_cells() {
        let l = history_popup().layout((1440.0, 860.0));
        let undo = l.cells[0].rect;
        assert_eq!(l.item_at(undo.x + 2.0, undo.y + 2.0), Some(0));
        let mut p = history_popup();
        for h in &mut p.history {
            h.current = h.n == 0;
        }
        let l = p.layout((1440.0, 860.0));
        let undo = l.cells[0].rect;
        assert_eq!(
            l.item_at(undo.x + 2.0, undo.y + 2.0),
            None,
            "undo is dim at the opening state"
        );
    }

    #[test]
    fn ago_speaks_in_minutes_hours_and_days() {
        assert_eq!(ago(100_000, 99_000), "たった今");
        assert_eq!(ago(10 * 60_000, 0), "10 分前");
        assert_eq!(ago(3 * 3_600_000, 0), "3 時間前");
        assert_eq!(ago(2 * 86_400_000, 0), "2 日前");
    }
}
