//! What the real screen knows and where things are, without a window.
//!
//! [`Studio`] holds the song (through [`Song`], the one place that talks to
//! `xirius-core`) and the small amount of view state: which folders are
//! collapsed, what is selected or under the pointer, the scroll position, the
//! drawer. [`Geometry`] is where the regions sit for a window size, and
//! [`Studio::hit`] says what is under a point. `screen.rs` only paints these
//! numbers and forwards events to them, so all of it is tested headless.
//!
//! Coordinates are in logical pixels, relative to the body (the window below
//! the top bar).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use xirius_core::id::{ClipId, FolderId, TrackId};
use xirius_core::model::{Clip, Note, Project, Track};
use xirius_core::time::Tick;
use xirius_design::{Layout, LineState, Motion, Space, TrackColor};

use crate::browser::{self, Browser, BrowserEffect, BrowserHit, RailTab};
use crate::popup::{Action, Popup, PopupKind};
use crate::song::Song;
use crate::tools::Mods;
use crate::tools::SnapMode;
use crate::topbar::{BarButton, BarLayout, BarView, TempoMode};
use crate::tree::{Bar, Row, RowKind, Tree};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawerTab {
    Roll,
    Mixer,
}

impl DrawerTab {
    pub const ALL: [DrawerTab; 2] = [DrawerTab::Roll, DrawerTab::Mixer];

    pub fn label(self) -> &'static str {
        match self {
            DrawerTab::Roll => "ピアノロール",
            DrawerTab::Mixer => "ミキサー",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            DrawerTab::Roll => "roll",
            DrawerTab::Mixer => "mixer",
        }
    }
}

/// What is under a point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hit {
    #[default]
    None,
    /// The button that gives up waiting for a controller's knob (OPS-007).
    MidiCancel,
    Chevron(FolderId),
    Mute(TrackId),
    Solo(TrackId),
    /// A track or folder row, in the column or in its lane.
    Row(RowKind),
    Clip(TrackId, ClipId),
    /// The ruler or the section band: a click moves the playhead here.
    Ruler(Tick),
    Tab(DrawerTab),
    /// The roll's switch between the keyboard and the drum rows; `true` is the drums (EDT-006).
    RollView(bool),
    /// The button of the roll that picks the drum map (EDT-058).
    RollMap,
    /// The roll's "入力の長さ" switch: `false` for the grid's value, `true` for the note before
    /// (EDT-131).
    RollLength(bool),
    /// The record-arm button of a track.
    Arm(TrackId),
    /// The input monitor button of an audio track (REC-011).
    Monitor(TrackId),
    /// The keys button of an instrument track: its instrument's window (UI-098).
    Keys(TrackId),
    /// The colour square of a track: opens the colour choice.
    TrackColor(TrackId),
    /// The kind icon of a track; only a crashed plugin's bolt answers it.
    TrackIcon(TrackId),
    /// The pill after a track's name with its transpose and velocity (UI-312): its hover tip
    /// says the values; a press selects the row.
    TrackMark(TrackId),
    /// The chevron that widens a track (it then shows its output).
    Widen(TrackId),
    /// The snowflake of an audio or instrument track: freezes it, lets it go, or stops the
    /// freeze while it runs (EXP-004).
    Freeze(TrackId),
    Output(TrackId),
    /// A folder's output bus button (EDT-073).
    FolderBus(FolderId),
    /// The icons at the head of the column that show and hide the rows under the ruler.
    ToggleArrange,
    ToggleTempo,
    /// The + at the head of the column: the menu of tracks to add.
    AddTrack,
    /// The zoom buttons at the head of the column (the time axis).
    ZoomOut,
    ZoomIn,
    /// The vertical zoom of the waveforms, after the zoom buttons: ×1, ×2, ×4 and round (UI-310).
    WaveZoom,
    /// The autoscroll switch at the head of the column (EDT-070, as F).
    Follow,
    /// The ‹ / › at the head of the column: folds the track list to a strip and back (UI-066).
    Fold,
    /// A row of the folded strip: a press on it opens the list again.
    Folded(RowKind),
    /// A button of the left rail.
    Rail(RailTab),
    /// The gear at the bottom of the left rail: the settings (SET-001).
    RailSettings,
    /// Something in the browser panel.
    Browser(BrowserHit),
    /// A part of the inspector's panel (UI-312).
    Inspector(crate::inspector::Part),
    /// A button of the top bar. Its body y is negative.
    Bar(BarButton),
    /// A part of the mixer in the drawer.
    Mixer(crate::mixer::MixerHit),
    /// A cell of the open pop-over (an index into its layout).
    Item(usize),
    /// A part of the open pop-over that does nothing.
    PopupBody,
    /// An entry of the empty song's screen (UI-074).
    Start(crate::start::Entry),
    /// A row of the recent songs under the entries (UI-261).
    Entrance(crate::start::Part),
    /// A switch or field of the record panel (REC-007).
    Rec(crate::rec_panel::RecPart),
    /// A part of the toast at the foot of the lanes.
    Toast(crate::toast::ToastPart),
}

impl Hit {
    /// The buttons at the head of the column.
    pub const HEAD: [Hit; 7] = [
        Hit::AddTrack,
        Hit::ToggleArrange,
        Hit::ToggleTempo,
        Hit::ZoomOut,
        Hit::ZoomIn,
        Hit::WaveZoom,
        Hit::Follow,
    ];
}

/// Which parts of the body are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panels {
    pub drawer: bool,
    /// The row of sections under the ruler.
    pub arrange: bool,
    pub tempo: bool,
    /// The browser's panel (its rail is always there).
    pub browser: bool,
    /// The drawer shows the mixer, which needs more height than the roll.
    pub mixer: bool,
    /// The track list is folded to a strip (UI-066).
    pub folded: bool,
    /// The record panel under the top bar (REC-007).
    pub record: bool,
}

/// Where the regions sit, for a body of the given size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub size: (f32, f32),
    pub panels: Panels,
    /// The record panel across the top of the body (zero high when it is shut).
    pub rec: Rect,
    /// The browser's rail of buttons, at the left edge.
    pub rail: Rect,
    /// The browser's panel (zero wide when it is shut).
    pub panel: Rect,
    /// The corner above the track column.
    pub head: Rect,
    /// The bar numbers; the top of the header.
    pub ruler: Rect,
    /// The row of sections under the ruler (zero high when hidden).
    pub section_band: Rect,
    /// The row with the tempo line under that (zero high when hidden).
    pub tempo: Rect,
    /// The part of the track column that shows rows.
    pub column: Rect,
    /// The part of the arrangement that shows lanes.
    pub lanes: Rect,
    pub drawer: Rect,
    /// The tab strip along the top of the drawer.
    pub drawer_bar: Rect,
    /// The drawer's content (empty when it is shut).
    pub drawer_body: Rect,
}

impl Geometry {
    pub fn new(w: f32, h: f32, drawer_open: bool) -> Geometry {
        Geometry::with_panels(
            w,
            h,
            Panels {
                drawer: drawer_open,
                arrange: true,
                tempo: true,
                browser: false,
                mixer: false,
                folded: false,
                record: false,
            },
        )
    }

    pub fn with_panels(w: f32, h: f32, panels: Panels) -> Geometry {
        Geometry::with_head(w, h, panels, Layout::TRACK_HEAD_W)
    }

    /// Like [`Geometry::with_panels`], with a track head `head_w` wide
    /// ([`Tree::head_w`]).
    pub fn with_head(w: f32, h: f32, panels: Panels, head_w: f32) -> Geometry {
        Geometry::sized(w, h, panels, head_w, None)
    }

    /// Like [`Geometry::with_head`], with the open drawer `drawer_h` high when
    /// one is asked for (UI-095), kept inside [`crate::drawer_edge::limits`].
    pub fn sized(w: f32, h: f32, panels: Panels, head_w: f32, drawer_h: Option<f32>) -> Geometry {
        Geometry::sized_to(w, h, panels, head_w, drawer_h, Layout::BROWSER_W)
    }

    /// Like [`Geometry::sized`], with the browser's panel `browser_w` wide when it
    /// is open (UI-048), kept inside [`crate::side_edge::browser_limits`].
    pub fn sized_to(
        w: f32,
        h: f32,
        panels: Panels,
        head_w: f32,
        drawer_h: Option<f32>,
        browser_w: f32,
    ) -> Geometry {
        let gap = Layout::REGION_GAP;
        let drawer_h = if !panels.drawer {
            Layout::DRAWER_BAR_H
        } else {
            let want = drawer_h.unwrap_or(if panels.mixer {
                (Layout::DRAWER_BAR_H + crate::mixer::BODY_H)
                    .min(h * 0.55)
                    .max(Layout::DRAWER_H)
            } else {
                Layout::DRAWER_H
            });
            crate::drawer_edge::fit(want, h, panels)
        };
        let drawer = Rect::new(gap, h - gap - drawer_h, (w - gap * 2.0).max(0.0), drawer_h);
        let rec_h = if panels.record {
            crate::rec_panel::H
        } else {
            0.0
        };
        let rec = Rect::new(gap, gap, (w - gap * 2.0).max(0.0), rec_h);
        // Everything under the record panel starts below it.
        let top = if panels.record {
            rec.bottom() + gap
        } else {
            gap
        };
        let side_h = (drawer.y - gap - top).max(0.0);
        let rail = Rect::new(gap, top, Layout::RAIL_W, side_h);
        let panel_w = if panels.browser { browser_w } else { 0.0 };
        let panel = Rect::new(rail.right() + gap, top, panel_w, side_h);
        let column_x = if panels.browser {
            panel.right() + gap
        } else {
            rail.right() + gap
        };
        let arrange_x = column_x + head_w + gap;
        let arrange_w = (w - arrange_x - gap).max(0.0);
        let band_h = if panels.arrange {
            Layout::SECTION_BAND_H
        } else {
            0.0
        };
        let tempo_h = if panels.tempo {
            Layout::TEMPO_LANE_H
        } else {
            0.0
        };
        let header_h = Layout::RULER_H + band_h + tempo_h;
        let lanes_y = top + header_h + gap;
        let view_h = (drawer.y - gap - lanes_y).max(0.0);
        Geometry {
            size: (w, h),
            panels,
            rec,
            rail,
            panel,
            head: Rect::new(column_x, top, head_w, header_h),
            ruler: Rect::new(arrange_x, top, arrange_w, Layout::RULER_H),
            section_band: Rect::new(arrange_x, top + Layout::RULER_H, arrange_w, band_h),
            tempo: Rect::new(
                arrange_x,
                top + Layout::RULER_H + band_h,
                arrange_w,
                tempo_h,
            ),
            column: Rect::new(column_x, lanes_y, head_w, view_h),
            lanes: Rect::new(arrange_x, lanes_y, arrange_w, view_h),
            drawer,
            drawer_bar: Rect::new(drawer.x, drawer.y, drawer.w, Layout::DRAWER_BAR_H),
            drawer_body: Rect::new(
                drawer.x,
                drawer.y + Layout::DRAWER_BAR_H,
                drawer.w,
                (drawer.h - Layout::DRAWER_BAR_H).max(0.0),
            ),
        }
    }

    /// The header's height: the ruler and the rows that are shown.
    pub fn header_h(&self) -> f32 {
        self.head.h
    }

    /// The two icons in the head that show and hide the rows under the ruler.
    /// A button at the head of the column: the + at the left, the two
    /// toggles, the zoom buttons and the autoscroll switch at the right (as
    /// in the mock).
    pub fn toggle(&self, hit: Hit) -> Option<Rect> {
        let s = Self::HEAD_TOGGLE;
        let y = self.ruler.y + (self.ruler.h - s) / 2.0;
        // Folded, the head is the strip's width: the one button left is the › that opens it.
        if self.panels.folded {
            return (hit == Hit::Fold).then(|| Rect::new(self.head.x, y, self.head.w, s));
        }
        // Eight buttons in the narrowest head (UI-310): a small gap between them, a wider one
        // between the + and ‹ at the left and the six at the right.
        let gap = Space::X1.px();
        let n = match hit {
            Hit::AddTrack => return Some(Rect::new(self.head.x + gap, y, s, s)),
            // The ‹ sits after the +: the right of the head is full of the other six.
            Hit::Fold => return Some(Rect::new(self.head.x + gap + s + gap, y, s, s)),
            Hit::ToggleArrange => 0.0,
            Hit::ToggleTempo => 1.0,
            Hit::ZoomOut => 2.0,
            Hit::ZoomIn => 3.0,
            Hit::WaveZoom => 4.0,
            Hit::Follow => 5.0,
            _ => return None,
        };
        let right = self.head.right() - gap;
        let after = Self::HEAD_BUTTONS - 1.0 - n;
        Some(Rect::new(right - (after + 1.0) * s - after * gap, y, s, s))
    }

    /// The buttons at the right of the head of the column.
    const HEAD_BUTTONS: f32 = 6.0;

    /// A head button: 22 px, so that the eight fit [`Layout::TRACK_HEAD_W`] (UI-310).
    pub const HEAD_TOGGLE: f32 = 22.0;

    /// The tab buttons: icon only, side by side from the drawer's left.
    pub fn tab(&self, tab: DrawerTab) -> Rect {
        let i = DrawerTab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
        let (w, gap) = (Self::TAB_W, Space::X1.px());
        Rect::new(
            self.drawer_bar.x + Space::X2.px() + i as f32 * (w + gap),
            self.drawer_bar.y + (self.drawer_bar.h - Self::TAB_H) / 2.0,
            w,
            Self::TAB_H,
        )
    }

    pub const TAB_W: f32 = 28.0;
    pub const TAB_H: f32 = 24.0;
    /// One half of the roll's "鍵盤｜ドラム" switch.
    pub const SWITCH_W: f32 = 28.0;
    /// The button that picks the drum map.
    pub const MAP_W: f32 = 104.0;
    /// The keys half (`false`) or the drums half (`true`) of the roll's view switch, after the
    /// tabs.
    pub fn roll_switch(&self, drums: bool) -> Rect {
        let last = self.tab(DrawerTab::ALL[DrawerTab::ALL.len() - 1]);
        Rect::new(
            last.right() + Space::X3.px() + 2.0 + if drums { Self::SWITCH_W } else { 0.0 },
            last.y,
            Self::SWITCH_W,
            Self::TAB_H,
        )
    }

    /// The drum map button, right of the switch.
    pub fn roll_map(&self) -> Rect {
        let drums = self.roll_switch(true);
        Rect::new(
            drums.right() + Space::X2.px() + 2.0,
            drums.y,
            Self::MAP_W,
            Self::TAB_H,
        )
    }

    /// The single last-note toggle (EDT-131).
    pub fn roll_length(&self) -> Rect {
        let drums = self.roll_switch(true);
        Rect::new(drums.right() + 18.0, drums.y, 28.0, Self::TAB_H)
    }

    /// The name stays between the controls and the bar's right padding (UI-343).
    pub fn roll_name(&self, drums: bool) -> Rect {
        let from = if drums {
            self.roll_map().right()
        } else {
            self.roll_length().right()
        } + Space::X4.px();
        Rect::new(
            from,
            self.drawer_bar.y,
            (self.drawer_bar.right() - Space::X3.px() - from).max(0.0),
            self.drawer_bar.h,
        )
    }
}

/// The parts of a row, relative to the column's left edge and the row's top.
///
/// A track has two lines: the icon, the name and the widen chevron above;
/// below, the buttons (mute, solo, record, audition, keys). A widened track
/// has a third line with where it plays to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowParts {
    /// A folder's open / shut chevron.
    pub chevron: Option<Rect>,
    /// The track's colour square, left of its kind icon; a track only.
    pub swatch: Option<Rect>,
    pub icon: Rect,
    pub name_x: f32,
    /// Where the name has to stop: before the widen chevron, or the row's right edge.
    pub name_right: f32,
    pub mute: Option<Rect>,
    pub solo: Option<Rect>,
    pub arm: Option<Rect>,
    /// The audition (monitor) button. Core has no command for it yet, so it is dim.
    pub ear: Option<Rect>,
    /// Instrument editor; only on instrument tracks, currently disabled.
    pub keys: Option<Rect>,
    pub widen: Option<Rect>,
    /// Where the track plays to; only on a widened track.
    pub output: Option<Rect>,
    /// Shows and hides the track's automation lanes (AUT-001).
    pub auto: Option<Rect>,
    /// The track's level, a thin bar at the head's right edge (as in the mock).
    pub meter: Option<Rect>,
    /// Freezes the track (EXP-004), at the right of the buttons' line when there is room.
    pub freeze: Option<Rect>,
}

impl RowParts {
    pub fn of(row: &Row) -> RowParts {
        let x = row.content_x();
        let right = row.head_w - Space::X2.px();
        match row.kind {
            RowKind::Folder(_) => {
                let icon_y = (row.h - Layout::ROW_ICON) / 2.0;
                let chevron = Rect::new(
                    x,
                    (row.h - Layout::ICON_SIZE) / 2.0,
                    Layout::ICON_SIZE,
                    Layout::ICON_SIZE,
                );
                let icon_x = chevron.right() + Space::X1.px();
                RowParts {
                    chevron: Some(chevron),
                    swatch: None,
                    icon: Rect::new(icon_x, icon_y, Layout::ROW_ICON, Layout::ROW_ICON),
                    name_x: icon_x + Layout::ROW_ICON + Space::X2.px(),
                    name_right: right,
                    mute: None,
                    solo: None,
                    arm: None,
                    ear: None,
                    keys: None,
                    widen: None,
                    output: None,
                    auto: None,
                    meter: None,
                    freeze: None,
                }
            }
            RowKind::Track(_) => {
                let swatch = Rect::new(
                    x,
                    Self::LINE_1_Y + (Layout::ROW_ICON - Self::SWATCH_CELL) / 2.0,
                    Self::SWATCH_CELL,
                    Self::SWATCH_CELL,
                );
                let icon = Rect::new(
                    swatch.right(),
                    Self::LINE_1_Y,
                    Layout::ROW_ICON,
                    Layout::ROW_ICON,
                );
                let name_x = icon.right() + Space::X2.px();
                let meter = Rect::new(
                    right - Self::METER_W,
                    Self::LINE_1_Y,
                    Self::METER_W,
                    Self::LINE_2_Y + Layout::TOGGLE - Self::LINE_1_Y,
                );
                let widen = Rect::new(
                    meter.x - Space::X2.px() - Layout::ICON_SIZE,
                    Self::LINE_1_Y + (Layout::ROW_ICON - Layout::ICON_SIZE) / 2.0,
                    Layout::ICON_SIZE,
                    Layout::ICON_SIZE,
                );
                let auto = Rect::new(
                    widen.x - Space::X1.px() - Layout::ICON_SIZE,
                    widen.y,
                    Layout::ICON_SIZE,
                    Layout::ICON_SIZE,
                );
                let t = Layout::TOGGLE;
                let step = t + Layout::TOGGLE_GAP;
                let button = |i: f32| Rect::new(name_x + i * step, Self::LINE_2_Y, t, t);
                let freeze = Rect::new(meter.x - Space::X2.px() - t, Self::LINE_2_Y, t, t);
                let freeze =
                    (freeze.x >= button(4.0).right() + Layout::TOGGLE_GAP).then_some(freeze);
                RowParts {
                    chevron: None,
                    swatch: Some(swatch),
                    icon,
                    name_x,
                    name_right: crate::automation_track_mode::rect(auto).x - Space::X1.px(),
                    mute: Some(button(0.0)),
                    solo: Some(button(1.0)),
                    arm: Some(button(2.0)),
                    ear: Some(button(3.0)),
                    keys: Some(button(4.0)),
                    widen: Some(widen),
                    auto: Some(auto),
                    meter: Some(meter),
                    freeze,
                    output: row.tall.then(|| {
                        Rect::new(
                            name_x,
                            Self::LINE_3_Y,
                            right - name_x,
                            Layout::ICON_BUTTON - Space::X1.px(),
                        )
                    }),
                }
            }
        }
    }

    pub const LINE_1_Y: f32 = 6.0;
    /// The colour square's hit cell; the square drawn in it is [`Self::SWATCH_MARK`].
    pub const SWATCH_CELL: f32 = 16.0;
    pub const SWATCH_MARK: f32 = 12.0;
    pub const METER_W: f32 = 6.0;
    pub const LINE_2_Y: f32 = 36.0;
    pub const LINE_3_Y: f32 = 64.0;

    /// A small dot for each level of depth beyond the indent's cap: a
    /// column at the head's left edge, centred on the row, left of every
    /// folder's line.
    pub fn depth_dot(row: &Row, i: usize) -> Rect {
        let d = Layout::DEPTH_DOT;
        let gap = Layout::DEPTH_DOT_GAP;
        let n = row.depth_dots().min(Layout::DEPTH_DOTS_MAX) as f32;
        let top = (row.h - (n * d + (n - 1.0) * gap)) / 2.0;
        Rect::new(Layout::DEPTH_DOT_X, top + i as f32 * (d + gap), d, d)
    }
}

/// The notes of the clip the piano roll shows.
pub struct RollTarget {
    pub track: Arc<Track>,
    pub clip: Clip,
    pub notes: Arc<Vec<Note>>,
}

pub struct Studio {
    pub input_map: crate::input_dispatch::InputState,
    pub(crate) song: Song,
    pub roll: crate::roll::Roll,
    /// The tool picked in the top bar or with a number key, for the roll too.
    pub tool: crate::tools::Tool,
    /// How the grid step is found. The grid value and snap on/off are the
    /// session's (`Song::grid`, `Song::snap`); [`Studio::tools`] joins them.
    pub snap_mode: crate::tools::SnapMode,
    /// AQ: picking a grid value quantizes the roll's selected notes to it
    /// again (EDT-075). Off at start, as in the mock.
    pub auto_quantize: bool,
    /// What the quantize panel is set to (EDT-073); Q sends it with the grid.
    pub(crate) quant: crate::quantize_panel::QuantSet,
    tree: Tree,
    collapsed: HashSet<FolderId>,
    tall: HashSet<TrackId>,
    pub browser: Browser,
    /// The inspector's fields: the one that has the keys (UI-312).
    pub inspector: crate::inspector::Inspector,
    /// The widths measured while painting each track's name and mark, so the hit test, the
    /// name's field and the drawing share the same rectangles (UI-312).
    pub(crate) mark_widths:
        std::cell::RefCell<std::collections::BTreeMap<TrackId, crate::draw::MarkWidths>>,
    /// The song's meters and tempo, for the grid's lines and the snap (UI-339).
    pub(crate) meters: crate::tools::Meters,
    pub popup: Option<Popup>,
    pub arrange_visible: bool,
    pub tempo_visible: bool,
    pub file_request: Option<FileRequest>,
    /// The OS's file window is opened for the browser's 「エクスプローラーで表示」 rows (the real
    /// window; tests and the screenshot scripts only note the path in `revealed`), and the
    /// system's open and save windows answer `file_request` (a script answers it itself).
    pub os_files: bool,
    pub revealed: Vec<std::path::PathBuf>,
    /// The web pages opened in the browser (with `os_files` off, only noted here).
    pub opened: Vec<String>,
    /// The asking for a newer version at launch and what it found (UI-262).
    pub update: crate::about::Update,
    pub selected_row: Option<RowKind>,
    pub selected_clip: Option<(TrackId, ClipId)>,
    /// The last thing the pointer was over.
    pub hover: Hit,
    pub drawer_open: bool,
    /// The colours the system draws the plugins' windows' captions in: the app's own (UI-183).
    pub(crate) caption: Option<xirius_vst3::Caption>,
    /// `edit.cursor.crosshair`: the system's crosshair for every tool, not the tool's picture.
    pub(crate) crosshair: bool,
    /// `edit.drag.value-tip`: a drag shows its value next to the pointer.
    pub(crate) value_tip: bool,
    pub motion: std::rc::Rc<crate::view_motion::ViewMotion>,
    pub tab: DrawerTab,
    /// The drawer's height dragged at its top edge (UI-095).
    pub drawer_size: crate::drawer_edge::DrawerSize,
    /// The browser's width and the track head's, dragged at their right edges (UI-048).
    pub side_size: crate::side_edge::SideSize,
    /// Reads the OS's "less motion" while the app runs (UI-055); a script keeps none.
    pub motion_watch: Option<crate::motion_watch::MotionWatch>,
    /// The view held by the middle button (EDT-068).
    pub pan: Option<crate::pan::Pan>,
    /// The press on the ruler that a drag turns into a zoom (KEY-016).
    pub ruler_grab: Option<crate::ruler_drag::Grab>,
    /// Distance scrolled right and down, in px.
    pub scroll: (f32, f32),
    pub beat_w: f32,
    /// How many times taller the waveforms are drawn than the clips play them (UI-310): ×1, ×2
    /// or ×4. Only the picture; like the zoom, the song does not keep it.
    pub wave_zoom: f32,
    /// Alt + wheel turned short of a notch (a touchpad turns a little at a time).
    pub(crate) wave_zoom_turn: f32,
    pub playhead: Tick,
    /// Whether the view follows the playhead, and how (EDT-070).
    pub follow: crate::follow::Follow,
    /// What the last command said when it failed.
    pub error: Option<String>,
    /// Editing the arrangement with the pointer (ARR, AUT, EDT).
    pub arrange: crate::arrange::Arrange,
    pub transport: crate::transport::Transport,
    pub mixer: crate::mixer::Mixer,
    /// The levels of the inputs of the armed tracks (REC-002).
    pub inputs: crate::inmeter::InputMeters,
    /// Tracks whose Comp and take rows are open under the track (REC-005). They start folded:
    /// the clip's count of takes opens them (UI-296).
    pub takes_open: HashSet<TrackId>,
    /// The stretch of a take stack the last press or drag on a take row chose; the arrow keys
    /// switch its take and move it (REC-006).
    pub comp_sel: Option<crate::rec_area::CompSel>,
    /// The track whose arm button the record button sent the eye to, when
    /// pressed with nothing armed (REC-010). Any press lets it go.
    pub arm_cue: Option<TrackId>,
    /// The tempo readout of the top bar being dragged, focused or typed into (UI-119).
    pub bar_edit: crate::bar_tempo::BarEdit,
    /// When the record button was pressed with nothing armed: the button shakes for a moment
    /// (REC-010).
    pub record_shake: Option<std::time::Instant>,
    /// The record panel under the top bar: shown or not, and the field that takes digits (REC-007).
    pub rec_panel: crate::rec_panel::RecPanel,
    /// The loudest held peak each armed track's input reached since the recording began, on the
    /// meter scale (REC-013).
    pub rec_peaks: HashMap<TrackId, f32>,
    /// The export panel's settings and the exports running or written.
    pub export: crate::export::Export,
    /// Plugins whose process went down (PLG-053).
    pub health: crate::health::Health,
    /// The CPU the host reports, and the plugins' share of it (ENG-008).
    pub perf: crate::cpu_list::Perf,
    /// The command palette, while it is open (UI-087).
    pub palette: Option<crate::palette::Palette>,
    /// The settings screen, while it is open (SET-001).
    pub settings: Option<crate::settings::SettingsUi>,
    /// The AI: how to reach it, and how its work shows (AI-003, AI-036).
    pub ai: crate::ai::Ai,
    /// A line said once and let go of ([`crate::notice`]).
    pub notice: Option<crate::notice::Notice>,
    /// What the audio processing has to say, with ⚠ or a button ([`crate::toast`]).
    pub toast: Option<crate::toast::Toast>,
    pub(crate) opening: Option<crate::opening::Opening>,
    pub(crate) open_generation: u64,
    pub(crate) open_closing: Option<crate::opening::CloseJob>,
    pub(crate) saving: Option<crate::saving::Saving>,
    pub(crate) unsaved: Option<crate::unsaved::Question>,
    /// The watch on the song's file for another app's save ([`crate::other_change`], UI-289).
    pub(crate) other: crate::other_change::OtherChange,
    /// A mouse button is down in the window: something may be dragged (UI-289).
    pub(crate) pointer_held: bool,
    pub(crate) leaving_ready: bool,
    pub(crate) save_test_delay: std::time::Duration,
    #[cfg(test)]
    pub(crate) save_test_failure: bool,
    pub(crate) save_test_destination: Option<std::path::PathBuf>,
    /// What a normalize raised the clips by, for a moment ([`crate::gain_tip`]).
    pub gain_tip: Option<crate::gain_tip::GainTip>,
    /// A bounce or a freeze rendering on a thread ([`crate::audio_job`]).
    pub job: Option<crate::audio_job::Job>,
    /// The bounces and freezes waiting for it (ENG-033).
    pub job_queue: crate::audio_job::Queue,
    pub(crate) prefs_closing: bool,
    pub(crate) browser_menu_io: crate::browser_menu_io::MenuIo,
    pub(crate) browser_import_io: crate::browser_import_io::ImportIo,
    /// The clips whose failed stretch the toast has said ([`crate::clip_marks`]).
    pub stretch_told: HashSet<xirius_core::id::ClipId>,
    /// The device's channels, while a thread reads them ([`crate::input_row`]).
    pub input_channels: crate::input_row::ChannelsRead,
    /// This PC's MIDI ports, as last listed ([`crate::midi_watch`]).
    pub midi_ports: crate::midi_watch::Ports,
    pub live_midi: crate::midi_live::LiveMidi,
    /// The open windows of built-in instruments (UI-098).
    pub instruments: crate::instrument::window::Windows,
    /// The right-click menu that is open (CTX-001).
    pub(crate) ctx: Option<crate::context::State>,
    /// The PC keys as a keyboard (UI-074).
    pub pc_keys: crate::instrument::keys::PcKeys,
    /// Files are dragged over the window (UI-076).
    pub drop_over: bool,
    /// The record of the song while it is edited: the save mark, drafts, the recent list.
    pub keep: crate::keep::Keep,
    /// What came back after a crash, until it is answered (PRJ-012).
    pub recovery: Option<crate::recovery_card::Card>,
    /// 音源を準備中: the song's instruments and effects open behind it (ENG-064).
    pub preparing: Option<crate::prepare_card::Card>,
    /// The output's level the device panel shows, left and right, falling (UI-260).
    pub rig_out: [f32; 2],
    /// The test tone sounds.
    pub rig_tone: bool,
    /// A `.dawproject` being written or read (EXP-006, EXP-007).
    pub dawproject: Option<crate::dawproject_ui::Job>,
    /// The cleanups of the songs closed, running on their threads (REC-026).
    pub tidying: Vec<crate::tidy::Tidy>,
    /// The name of the `.dawproject` dragged over the window: it says it opens.
    pub drop_dawproject: Option<String>,
    /// What a key asks of the window itself (KEY-012): full screen, or to close.
    pub(crate) window_ask: Option<crate::context::key_more::WindowAsk>,
    /// The undo step of a wheel that is still turning a field, and whether a wheel is running
    /// it now (UI-313).
    pub(crate) wheel_group: Option<crate::context::fields::WheelGroup>,
    pub(crate) wheel_running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileRequest {
    PresetOpen(xirius_core::id::PluginId),
    PresetSave(xirius_core::id::PluginId),
    Open,
    SaveAs,
    OpenVersion(String),
    /// Audio or MIDI files for 「ファイルから…」 (UI-017).
    Audio,
    /// A folder for a place tab of the file browser (BRW-005).
    Place,
    /// A `.dawproject` to open as a new song (EXP-006).
    OpenDawproject,
    /// Where to write the song as a `.dawproject` (EXP-007).
    ExportDawproject,
    /// The new place of a missing file of the song (BRW-004).
    Locate(String),
    /// A folder to look for every missing file in (BRW-004).
    LocateFolder,
    /// The folder the song records and bounces into, for its settings form (BRW-004).
    MediaFolder,
}

/// Quarter notes per pixel row in the roll: the roll shows this many px per beat.
pub const ROLL_BEAT_W: f32 = 48.0;

impl Studio {
    pub fn new(song: Song) -> Studio {
        let project = song.project();
        let collapsed = HashSet::new();
        let tree = Tree::build(&project, &collapsed);
        let mut studio = Studio {
            song,
            roll: crate::roll::Roll::new(),
            tool: crate::tools::Tool::Select,
            snap_mode: crate::tools::SnapMode::Auto,
            input_map: crate::input_dispatch::InputState::default(),
            auto_quantize: false,
            quant: Default::default(),
            tree,
            collapsed,
            tall: HashSet::new(),
            browser: Browser::new(),
            inspector: crate::inspector::Inspector::default(),
            mark_widths: Default::default(),
            meters: Default::default(),
            popup: None,
            arrange_visible: true,
            tempo_visible: true,
            file_request: None,
            os_files: false,
            revealed: Vec::new(),
            opened: Vec::new(),
            update: crate::about::Update::default(),
            selected_row: None,
            selected_clip: None,
            hover: Hit::None,
            drawer_open: true,
            caption: None,
            crosshair: false,
            value_tip: true,
            motion: std::rc::Rc::new(crate::view_motion::ViewMotion::default()),
            tab: DrawerTab::Roll,
            drawer_size: Default::default(),
            side_size: Default::default(),
            motion_watch: None,
            pan: None,
            ruler_grab: None,
            scroll: (0.0, 0.0),
            beat_w: Layout::BEAT_W,
            wave_zoom: 1.0,
            wave_zoom_turn: 0.0,
            playhead: 0,
            follow: crate::follow::Follow::default(),
            error: None,
            arrange: crate::arrange::Arrange::default(),
            transport: crate::transport::Transport::new(xirius_session::output::OutputMode::None),
            mixer: crate::mixer::Mixer::default(),
            inputs: crate::inmeter::InputMeters::default(),
            takes_open: HashSet::new(),
            comp_sel: None,
            arm_cue: None,
            bar_edit: crate::bar_tempo::BarEdit::default(),
            record_shake: None,
            rec_panel: Default::default(),
            rec_peaks: HashMap::new(),
            export: crate::export::Export::default(),
            health: crate::health::Health::default(),
            perf: Default::default(),
            palette: None,
            settings: None,
            ai: crate::ai::Ai::default(),
            notice: None,
            toast: None,
            saving: None,
            unsaved: None,
            opening: None,
            open_generation: 0,
            open_closing: None,
            other: Default::default(),
            pointer_held: false,
            leaving_ready: false,
            save_test_delay: std::time::Duration::ZERO,
            #[cfg(test)]
            save_test_failure: false,
            save_test_destination: None,
            gain_tip: None,
            job: None,
            job_queue: Default::default(),
            prefs_closing: false,
            browser_menu_io: Default::default(),
            browser_import_io: Default::default(),
            stretch_told: HashSet::new(),
            input_channels: None,
            midi_ports: crate::midi_watch::Ports::default(),
            live_midi: Default::default(),
            instruments: crate::instrument::window::Windows::default(),
            ctx: None,
            pc_keys: crate::instrument::keys::PcKeys::default(),
            drop_over: false,
            keep: crate::keep::Keep::default(),
            recovery: None,
            preparing: None,
            rig_out: [0.0; 2],
            rig_tone: false,
            dawproject: None,
            tidying: Vec::new(),
            drop_dawproject: None,
            window_ask: None,
            wheel_group: None,
            wheel_running: false,
        };
        // A new song starts at its head; the entries sit where bar 5 would be.
        if !studio.start_shown() {
            studio.playhead = studio.project().time_map().bar_start(5);
        }
        studio.apply_edit_prefs();
        // The song's files are read with it, so the first idle poll has nothing new to say
        // and the rail's dot is there from the first frame (BRW-004).
        studio.follow_pool();
        studio
    }

    /// The sample song, with the Keys clip in the piano roll.
    pub fn sample() -> Studio {
        let mut studio = Studio::new(Song::sample());
        // The fixture's selection is set, not chosen: the instrument input
        // does not follow it, so the song's arms stay as the song was made.
        let keys = studio
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .and_then(|t| t.clips.first().map(|c| (t.id, c.id)));
        if let Some((track, clip)) = keys {
            studio.selected_clip = Some((track, clip));
            studio.selected_row = Some(RowKind::Track(track));
        }
        studio.choose_editor();
        studio
    }

    pub fn project(&self) -> Arc<Project> {
        self.song.project()
    }

    pub fn song(&self) -> &Song {
        &self.song
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn is_collapsed(&self, folder: FolderId) -> bool {
        self.collapsed.contains(&folder)
    }

    /// Closes every folder (or opens them all).
    pub(crate) fn fold_all(&mut self, g: &Geometry, close: bool) -> bool {
        self.collapsed = if close {
            self.project().folders.keys().copied().collect()
        } else {
            HashSet::new()
        };
        self.rebuild(g);
        true
    }

    /// Selects the first clip of the track with this name, and its row.
    pub fn select_clip_of(&mut self, track_name: &str) -> bool {
        let project = self.project();
        let found = project
            .tracks
            .values()
            .find(|t| t.name == track_name)
            .and_then(|t| t.clips.first().map(|c| (t.id, c.id)));
        if let Some((track, clip)) = found {
            self.select_clip_here(track, clip);
        }
        found.is_some()
    }

    /// Chooses a clip in the screen: its track's row, the highlight in the arrangement, and the
    /// clip whose piano roll and inspector show. Everything that chooses a clip from the screen
    /// (a drop, the roll, a press) goes through here, so the three stay together (P3-3 of review
    /// 15).
    pub(crate) fn select_clip_here(&mut self, track: TrackId, clip: ClipId) {
        self.arrange.clips = std::collections::BTreeSet::from([(track, clip)]);
        self.selected_clip = Some((track, clip));
        self.arrange.touched();
        self.select_row(Some(RowKind::Track(track)));
    }

    pub fn geometry(&self, w: f32, h: f32) -> Geometry {
        self.motion.geometry(
            self.settled_geometry(w, h),
            // A panel follows the pointer while its edge is held; it slides only when
            // it opens, shuts or is given back its width.
            if self.side_size.drag.is_some() || self.drawer_size.drag.is_some() {
                Motion::new(true)
            } else {
                self.ai.motion
            },
        )
    }

    /// The layout the panels come to rest in: what [`Self::geometry`] slides toward.
    pub fn settled_geometry(&self, w: f32, h: f32) -> Geometry {
        let panels = Panels {
            drawer: self.drawer_open,
            arrange: self.arrange_visible,
            tempo: self.tempo_visible,
            browser: self.browser.open,
            mixer: self.mixer_shown(),
            record: self.rec_panel.open,
            folded: self.side_size.folded,
        };
        let head_w = self.tree.head_w.max(Layout::TRACK_HEAD_W);
        let head_w = self.side_size.head_w(w, panels, head_w);
        Geometry::sized_to(
            w,
            h,
            panels,
            head_w,
            self.drawer_size.want(self.tab),
            self.side_size.browser_w(w, panels, head_w),
        )
    }

    // ------------------------------------------------------------- time axis

    pub fn ticks_to_px(&self, ticks: Tick) -> f32 {
        self.ticks_to_px_f64(ticks) as f32
    }

    /// [`Studio::ticks_to_px`] before it is rounded to an f32: zoomed in to the samples a song
    /// is millions of pixels wide, past where an f32 holds a pixel.
    pub(crate) fn ticks_to_px_f64(&self, ticks: Tick) -> f64 {
        ticks as f64 / f64::from(self.project().ppq) * f64::from(self.beat_w)
    }

    pub fn px_to_ticks(&self, px: f32) -> Tick {
        let ppq = f64::from(self.project().ppq);
        (f64::from(px) / f64::from(self.beat_w) * ppq).round() as Tick
    }

    /// The end of what there is to see: the last clip or section, plus a bar.
    pub fn end_tick(&self) -> Tick {
        let project = self.project();
        let clips = project
            .tracks
            .values()
            .flat_map(|t| t.clips.iter().map(|c| c.start + c.length));
        let sections = project.sections.iter().map(|s| s.start + s.length);
        let last = clips.chain(sections).max().unwrap_or(0);
        let tm = project.time_map();
        let (bar, _, _) = tm.to_bbt(last);
        tm.bar_start(bar + 2)
    }

    /// Window x of a tick in the lanes, the ruler or the section band.
    pub fn tick_x(&self, g: &Geometry, ticks: Tick) -> f32 {
        g.lanes.x + (self.ticks_to_px_f64(ticks) - f64::from(self.scroll.0)) as f32
    }

    /// [`Studio::tick_x`] of a place between two ticks (a second's line zoomed in).
    pub fn tick_x_f(&self, g: &Geometry, ticks: f64) -> f32 {
        let px = ticks / f64::from(self.project().ppq) * f64::from(self.beat_w);
        g.lanes.x + (px - f64::from(self.scroll.0)) as f32
    }

    pub fn max_scroll(&self, g: &Geometry) -> (f32, f32) {
        (
            (self.ticks_to_px(self.end_tick()) - g.lanes.w).max(0.0),
            (self.tree.height + Space::X6.px() - g.lanes.h).max(0.0),
        )
    }

    pub fn clamp_scroll(&mut self, g: &Geometry) {
        let (mx, my) = self.max_scroll(g);
        // A drag that scrolled past the song's end keeps its view until it is let go (EDT-046).
        let mx = if self.arrange.drag.as_ref().is_some_and(|d| d.started) {
            mx.max(self.scroll.0)
        } else {
            mx
        };
        self.scroll.0 = self.scroll.0.clamp(0.0, mx);
        self.scroll.1 = self.scroll.1.clamp(0.0, my);
    }

    /// Is the inspector the rail's open face (UI-312)?
    pub fn inspector_open(&self) -> bool {
        self.browser.open && self.browser.tab == RailTab::Inspector
    }

    /// A wheel turn: vertical moves the rows (both columns), Shift or a
    /// sideways wheel moves the time axis.
    pub fn scroll_by(&mut self, g: &Geometry, dx: f32, dy: f32, shift: bool) {
        let (dx, dy) = if shift { (dy + dx, 0.0) } else { (dx, dy) };
        self.scroll.0 -= dx;
        self.scroll.1 -= dy;
        self.clamp_scroll(g);
    }

    /// Ctrl + wheel: zoom the time axis, keeping the tick under `x` where it is.
    pub fn zoom(&mut self, g: &Geometry, x: f32, steps: f32) {
        let anchor = self.px_to_ticks(x - g.lanes.x + self.scroll.0);
        let factor = 1.1f32.powf(steps);
        self.beat_w = (self.beat_w * factor).clamp(Self::BEAT_W_MIN, self.beat_w_max());
        self.scroll.0 = self.ticks_to_px(anchor) - (x - g.lanes.x);
        self.clamp_scroll(g);
    }

    /// One press of a zoom button, in wheel notches (the left edge stays put).
    pub const ZOOM_BUTTON_STEPS: f32 = 4.0;
    pub const BEAT_W_MIN: f32 = 8.0;
    /// Wide enough that one beat fills a 4K window's lanes (EDT-069); the most a beat is when its
    /// samples are fewer than [`Studio::SAMPLE_PX`] a pixel wide.
    pub const BEAT_W_MAX: f32 = 4096.0;
    /// How wide a sample is drawn zoomed in all the way (UI-310).
    pub const SAMPLE_PX: f64 = 6.0;

    /// The widest a beat is drawn: a sample [`Studio::SAMPLE_PX`] wide where the song is
    /// slowest, at the song's rate (UI-310), and no less than [`Studio::BEAT_W_MAX`].
    pub fn beat_w_max(&self) -> f32 {
        let project = self.project();
        beat_w_max(project.settings.sample_rate, &project.tempo)
    }

    /// A point that [`Studio::hit`] answers `hit` for, or None when that part is
    /// not on screen. Scripted input (the screenshot run) aims with this.
    pub fn center_of(&self, g: &Geometry, hit: Hit) -> Option<(f32, f32)> {
        let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let part = |kind: RowKind, pick: fn(&RowParts) -> Option<Rect>| {
            let row = self.tree.row_of(kind)?;
            let r = pick(&RowParts::of(row))?;
            let top = self.row_y(g, row);
            Some(mid(Rect::new(g.column.x + r.x, top + r.y, r.w, r.h)))
        };
        let at = match hit {
            Hit::None | Hit::PopupBody => None,
            Hit::MidiCancel => self.midi_pill(g).map(|(_, r)| mid(r)),
            Hit::Toast(part) => crate::toast::layout(self, g).and_then(|t| match part {
                crate::toast::ToastPart::Body => Some(mid(t.text)),
                crate::toast::ToastPart::Button => t.button.map(mid),
                crate::toast::ToastPart::Also => t.also.map(mid),
                crate::toast::ToastPart::Close => t.close.map(mid),
            }),
            Hit::Bar(b) => {
                let r = self.bar_rect(g, b);
                (r.w > 0.0).then(|| mid(r))
            }
            Hit::Item(i) => self
                .popup
                .as_ref()?
                .layout(g.size)
                .cells
                .get(i)
                .map(|c| mid(c.rect)),
            Hit::Rail(tab) => Some(mid(browser::rail_button(g.rail, tab))),
            Hit::Start(e) => Some(mid(self.entry_rects(g, e).0)),
            Hit::Entrance(part) => self.entrance_rect(g, part).map(mid),
            Hit::Rec(part) => self.rec_center(g, part),
            Hit::RailSettings => Some(mid(browser::rail_io_button(g.rail))),
            Hit::Browser(hit) => self.browser.center_of(&self.browser.layout(g.panel), hit),
            Hit::Inspector(part) => {
                let l = self.inspector_layout(g);
                match part {
                    crate::inspector::Part::Field(i) => l.rows.get(i).map(|r| mid(r.rect)),
                }
            }
            Hit::ToggleArrange
            | Hit::ToggleTempo
            | Hit::AddTrack
            | Hit::ZoomOut
            | Hit::ZoomIn
            | Hit::WaveZoom
            | Hit::Follow
            | Hit::Fold => g.toggle(hit).map(mid),
            Hit::Folded(kind) => {
                let row = self.tree.row_of(kind)?;
                Some((
                    g.column.x + g.column.w / 2.0,
                    self.row_y(g, row) + row.h / 2.0,
                ))
            }
            Hit::Arm(t) => part(RowKind::Track(t), |p| p.arm),
            Hit::Monitor(t) => part(RowKind::Track(t), |p| p.ear),
            Hit::Keys(t) => part(RowKind::Track(t), |p| p.keys),
            Hit::Widen(t) => part(RowKind::Track(t), |p| p.widen),
            Hit::Freeze(t) => part(RowKind::Track(t), |p| p.freeze),
            Hit::Output(t) => part(RowKind::Track(t), |p| p.output),
            Hit::TrackColor(t) => part(RowKind::Track(t), |p| p.swatch),
            Hit::TrackIcon(t) => part(RowKind::Track(t), |p| Some(p.icon)),
            Hit::TrackMark(t) => {
                let row = self.tree.row_of(RowKind::Track(t))?;
                let track = self.project().tracks.get(&t)?.clone();
                let mark = self.row_mark(row, &track);
                mark.text.as_ref()?;
                let r = crate::draw::track_mark_rect(row, &mark);
                Some((
                    g.column.x + r.x + r.w / 2.0,
                    self.row_y(g, row) + r.y + r.h / 2.0,
                ))
            }
            Hit::Tab(tab) => Some(mid(g.tab(tab))),
            Hit::RollView(drums) => Some(mid(g.roll_switch(drums))),
            Hit::RollMap => Some(mid(g.roll_map())),
            Hit::RollLength(_) => Some(mid(g.roll_length())),
            Hit::Ruler(tick) => Some((self.tick_x(g, tick), g.ruler.y + g.ruler.h - 4.0)),
            Hit::Mixer(m) => self.mixer_center(g, m),
            Hit::Chevron(f) => part(RowKind::Folder(f), |p| p.chevron),
            Hit::FolderBus(f) => self.folder_bus_center(g, f),
            Hit::Mute(t) => part(RowKind::Track(t), |p| p.mute),
            Hit::Solo(t) => part(RowKind::Track(t), |p| p.solo),
            Hit::Row(kind) => {
                let row = self.tree.row_of(kind)?;
                let name = RowParts::of(row).name_x;
                let y = if matches!(kind, RowKind::Track(_)) {
                    18.0
                } else {
                    row.h / 2.0
                };
                Some((g.column.x + name + 2.0, self.row_y(g, row) + y))
            }
            Hit::Clip(t, c) => {
                let row = self.tree.row_of(RowKind::Track(t))?;
                let project = self.project();
                let clip = project.tracks.get(&t)?.clips.iter().find(|k| k.id == c)?;
                Some(mid(self.clip_rect(g, row, clip)))
            }
        }?;
        // Only if what is there is really that: not scrolled out, not covered.
        let got = self.hit(g, at.0, at.1);
        let same = match (hit, got) {
            (Hit::Ruler(_), Hit::Ruler(_)) => true,
            _ => got == hit,
        };
        same.then_some(at)
    }

    /// Top of a row on screen.
    pub fn row_y(&self, g: &Geometry, row: &Row) -> f32 {
        g.column.y + row.y - self.scroll.1
    }

    // ---------------------------------------------------------------- hits

    pub fn hit(&self, g: &Geometry, x: f32, y: f32) -> Hit {
        if let Some(p) = &self.popup {
            let l = p.layout(g.size);
            if l.rect.contains(x, y) {
                return l.item_at(x, y).map_or(Hit::PopupBody, Hit::Item);
            }
        }
        // An instrument's window floats over everything but the pop-over.
        if self.over_instrument(g, x, y) {
            return Hit::None;
        }
        if let Some(part) = crate::toast::part_at(self, g, x, y) {
            return Hit::Toast(part);
        }
        if self.midi_pill(g).is_some_and(|(_, r)| r.contains(x, y)) {
            return Hit::MidiCancel;
        }
        if y < 0.0 {
            return self
                .bar_layout(g.size.0)
                .hit(x, y + Layout::TOP_BAR_H)
                .map_or(Hit::None, Hit::Bar);
        }
        // The bottom bar, under the body (UI-300).
        if y >= g.size.1 {
            return self
                .bar_layout(g.size.0)
                .hit_bottom(x, y - g.size.1)
                .map_or(Hit::None, Hit::Bar);
        }
        if let Some(hit) = self.rec_hit(g, x, y) {
            return hit;
        }
        for tab in RailTab::ALL {
            if browser::rail_button(g.rail, tab).contains(x, y) {
                return Hit::Rail(tab);
            }
        }
        if browser::rail_io_button(g.rail).contains(x, y) {
            return Hit::RailSettings;
        }
        if self.browser.open && g.panel.contains(x, y) {
            if self.browser.tab == RailTab::Inspector {
                return self
                    .inspector_hit(g, x, y)
                    .map_or(Hit::None, Hit::Inspector);
            }
            return self
                .browser
                .hit(&self.browser.layout(g.panel), x, y)
                .map_or(Hit::None, Hit::Browser);
        }
        for hit in Hit::HEAD.into_iter().chain([Hit::Fold]) {
            if g.toggle(hit).is_some_and(|r| r.contains(x, y)) {
                return hit;
            }
        }
        for tab in DrawerTab::ALL {
            if g.tab(tab).contains(x, y) {
                return Hit::Tab(tab);
            }
        }
        if self.roll_switch_shown() {
            for drums in [false, true] {
                if g.roll_switch(drums).contains(x, y) {
                    return Hit::RollView(drums);
                }
            }
            if self.roll_drum().is_some() && g.roll_map().contains(x, y) {
                return Hit::RollMap;
            }
            if self.roll_drum().is_none() && g.roll_length().contains(x, y) {
                return Hit::RollLength(!self.draw_length_last());
            }
        }
        if let Some(m) = self.mixer_hit(g, x, y) {
            return Hit::Mixer(m);
        }
        if g.section_band.contains(x, y) || g.ruler.contains(x, y) {
            let tick = self.px_to_ticks(x - g.lanes.x + self.scroll.0).max(0);
            return Hit::Ruler(tick);
        }
        if let Some(e) = self.start_at(g, x, y) {
            return Hit::Start(e);
        }
        if let Some(part) = self.entrance_at(g, x, y) {
            return Hit::Entrance(part);
        }
        if let Some(h) = self.fx_header_hit(g, x, y) {
            return Hit::Mixer(h);
        }
        let in_column = g.column.contains(x, y);
        let in_lanes = g.lanes.contains(x, y);
        if !in_column && !in_lanes {
            return Hit::None;
        }
        // Folded, the whole strip opens the list again, a row of it naming itself.
        if g.panels.folded && in_column {
            return self
                .tree
                .row_at(y - g.column.y + self.scroll.1)
                .map_or(Hit::Fold, |row| Hit::Folded(row.kind));
        }
        let Some(row) = self.tree.row_at(y - g.column.y + self.scroll.1) else {
            return Hit::None;
        };
        let top = self.row_y(g, row);
        if in_column {
            let parts = RowParts::of(row);
            let (lx, ly) = (x - g.column.x, y - top);
            let on = |r: Option<Rect>| r.is_some_and(|r| r.contains(lx, ly));
            match row.kind {
                RowKind::Folder(f) if on(parts.chevron) => return Hit::Chevron(f),
                RowKind::Folder(f) if self.folder_bus_rect(row, f).contains(lx, ly) => {
                    return Hit::FolderBus(f);
                }
                RowKind::Track(t) if on(parts.freeze) && self.freezable(t) => {
                    return Hit::Freeze(t);
                }
                // While the track freezes, its line of buttons says how far it is.
                RowKind::Track(t)
                    if self.job_freezing(t).is_some()
                        && [parts.mute, parts.solo, parts.arm, parts.ear, parts.keys]
                            .into_iter()
                            .any(on) =>
                {
                    return Hit::Row(row.kind);
                }
                RowKind::Track(t) if on(parts.mute) => return Hit::Mute(t),
                RowKind::Track(t) if on(parts.solo) => return Hit::Solo(t),
                RowKind::Track(t) if on(parts.arm) => return Hit::Arm(t),
                RowKind::Track(t) if on(parts.keys) && self.is_instrument(t) => {
                    return Hit::Keys(t);
                }
                // The mark says its values on hover; a press is the row's (UI-312).
                RowKind::Track(t)
                    if self.project().tracks.get(&t).is_some_and(|track| {
                        let mark = self.row_mark(row, track);
                        mark.text.is_some()
                            && crate::draw::track_mark_rect(row, &mark).contains(lx, ly)
                    }) =>
                {
                    return Hit::TrackMark(t);
                }
                RowKind::Track(t) if on(parts.widen) => return Hit::Widen(t),
                RowKind::Track(t) if on(parts.output) => return Hit::Output(t),
                RowKind::Track(t) if on(parts.swatch) => return Hit::TrackColor(t),
                RowKind::Track(t) if parts.icon.contains(lx, ly) => return Hit::TrackIcon(t),
                RowKind::Track(t) if on(parts.ear) && self.is_audio(t) => return Hit::Monitor(t),
                RowKind::Track(_) if on(parts.ear) || on(parts.keys) => return Hit::None,
                _ => {}
            }
            return Hit::Row(row.kind);
        }
        if let RowKind::Track(t) = row.kind {
            let project = self.project();
            if let Some(track) = project.tracks.get(&t) {
                let stacks = xirius_core::takes::stacks(track);
                for clip in &track.clips {
                    // A take that does not play is not drawn over the one that does (EDT-139).
                    if clip.muted && crate::draw::in_stack(&stacks, clip) {
                        continue;
                    }
                    if self.clip_rect(g, row, clip).contains(x, y) {
                        return Hit::Clip(t, clip.id);
                    }
                }
            }
        }
        Hit::Row(row.kind)
    }

    /// Where a clip sits on screen.
    pub fn clip_rect(&self, g: &Geometry, row: &Row, clip: &Clip) -> Rect {
        let inset = Space::X1.px();
        Rect::new(
            self.tick_x(g, clip.start),
            self.row_y(g, row) + inset,
            self.ticks_to_px(clip.length),
            row.h - inset * 2.0,
        )
    }

    // ------------------------------------------------------------- changes

    /// Applies a press. Returns true when the picture has to be redrawn.
    pub fn press(&mut self, g: &Geometry, hit: Hit) -> bool {
        self.cancel_browser_menu();
        if self.input_blocked() {
            return false;
        }
        // A press outside the inspector's face gives up what was half typed there (UI-312).
        if !matches!(hit, Hit::Inspector(_)) {
            self.field_blur();
        }
        self.error = None;
        self.arm_cue = None;
        if !matches!(hit, Hit::Browser(BrowserHit::Search)) {
            self.browser.typing = false;
        }
        if !matches!(hit, Hit::Rec(_)) {
            self.rec_panel.focus = None;
        }
        if self.popup.is_some() && !matches!(hit, Hit::Item(_) | Hit::PopupBody | Hit::Bar(_)) {
            self.popup = None;
            return true;
        }
        match hit {
            Hit::None => false,
            Hit::PopupBody => false,
            Hit::Toast(part) => {
                match part {
                    crate::toast::ToastPart::Close => {
                        self.other_refused();
                        self.toast = None;
                    }
                    crate::toast::ToastPart::Button => self.toast_button(Some(g), false),
                    crate::toast::ToastPart::Also => self.toast_button(Some(g), true),
                    crate::toast::ToastPart::Body => {}
                }
                true
            }
            Hit::MidiCancel => self.midi_learn(&serde_json::Map::from_iter([(
                "cancel".to_string(),
                serde_json::Value::Bool(true),
            )])),
            Hit::Item(i) => {
                let action = self.popup.as_ref().and_then(|p| {
                    p.layout(g.size)
                        .cells
                        .get(i)
                        .filter(|c| c.live())
                        .and_then(|c| c.action.clone())
                });
                action.is_some_and(|a| self.popup_action(g, a))
            }
            Hit::Bar(b) => {
                let kind = match b {
                    BarButton::Logo => {
                        self.open_about(self.about_anchor(g));
                        return true;
                    }
                    BarButton::SongName => PopupKind::Song,
                    BarButton::Meter => PopupKind::Meter,
                    BarButton::Cpu => PopupKind::Perf,
                    // A press on the readout itself is `bar_down`'s (the drag
                    // needs where it started); this gives it the keys.
                    BarButton::Tempo => {
                        self.popup = None;
                        self.bar_edit.focus_tempo();
                        return true;
                    }
                    BarButton::Saved => PopupKind::Versions,
                    BarButton::Grid => PopupKind::Grid,
                    BarButton::History => PopupKind::History,
                    BarButton::Ai => PopupKind::Ai,
                    BarButton::Export => {
                        self.open_export(self.export_anchor(g));
                        return true;
                    }
                    BarButton::Palette => {
                        self.open_palette(g);
                        return true;
                    }
                    BarButton::AltTool => {
                        self.popup = None;
                        self.open_alt_menu(g);
                        return true;
                    }
                    BarButton::Shape => {
                        self.popup = None;
                        self.open_shape_menu(g);
                        return true;
                    }
                    BarButton::DrawLength => return self.draw_length_choose(false),
                    BarButton::Tool(t) => {
                        self.arrow_chosen(g, t);
                        self.tool = t;
                        return true;
                    }
                    BarButton::ToStart
                    | BarButton::Play
                    | BarButton::Stop
                    | BarButton::Record
                    | BarButton::Loop
                    | BarButton::PreRoll
                    | BarButton::Metronome => return self.transport_press(b),
                    BarButton::RecPanel => {
                        self.toggle_rec_panel(g);
                        return true;
                    }
                    BarButton::Ripple => {
                        self.rec_set("edit.ripple", serde_json::json!({}));
                        return true;
                    }
                    BarButton::Snap => {
                        self.command(|s| s.set_snap(!s.snap()));
                        return true;
                    }
                };
                let r = self.bar_rect(g, b);
                self.open_popup(kind, r);
                true
            }
            Hit::Rail(tab) => {
                self.browser.press_rail(tab);
                self.inspector.field = None;
                self.rebuild(g);
                true
            }
            Hit::RailSettings => {
                self.open_settings(crate::settings::Section::General);
                true
            }
            Hit::Inspector(part) => self.inspector_press(g, part),
            Hit::Browser(hit) => {
                let l = self.browser.layout(g.panel);
                match self.browser.press(&l, hit) {
                    BrowserEffect::ChooseView => {
                        if let Some(r) = l.view {
                            self.open_popup(PopupKind::BrowserView, r);
                        }
                    }
                    BrowserEffect::StopScan => self.stop_plugin_scan(),
                    BrowserEffect::Retry(at) => {
                        if let Err(e) = self.retry_plugin(at) {
                            self.error = Some(e);
                        }
                    }
                    BrowserEffect::AddPlace => self.file_request = Some(FileRequest::Place),
                    BrowserEffect::PoolMenu => {
                        if let Some(r) = l.more {
                            self.open_pool_menu(r);
                        }
                    }
                    BrowserEffect::None => {}
                }
                // A file chosen to play is handed to the audition now, not at the next look.
                self.follow_audition();
                let l = self.browser.layout(g.panel);
                self.browser.clamp_scroll(&l);
                true
            }
            Hit::ToggleArrange => {
                self.arrange_visible = !self.arrange_visible;
                self.rebuild(g);
                true
            }
            Hit::ToggleTempo => {
                self.tempo_visible = !self.tempo_visible;
                self.rebuild(g);
                true
            }
            Hit::AddTrack => {
                if let Some(r) = g.toggle(hit) {
                    self.open_popup(PopupKind::AddTrack, r);
                }
                true
            }
            Hit::Fold | Hit::Folded(_) => {
                self.toggle_tracklist(g);
                true
            }
            Hit::ZoomOut | Hit::ZoomIn => {
                let steps = if hit == Hit::ZoomIn {
                    Self::ZOOM_BUTTON_STEPS
                } else {
                    -Self::ZOOM_BUTTON_STEPS
                };
                self.zoom(g, g.lanes.x, steps);
                true
            }
            Hit::WaveZoom => {
                self.cycle_wave_zoom();
                true
            }
            Hit::Follow => {
                self.toggle_follow();
                true
            }
            Hit::Freeze(t) => {
                if self.job_freezing(t).is_some() {
                    self.job_cancel();
                } else {
                    let frozen = self
                        .project()
                        .tracks
                        .get(&t)
                        .is_some_and(|x| x.frozen.is_some());
                    let name = if frozen {
                        "track.unfreeze"
                    } else {
                        "track.freeze"
                    };
                    self.arrange_run(name, serde_json::json!({ "ids": [t.to_string()] }));
                    self.rebuild(g);
                }
                true
            }
            Hit::Widen(t) => {
                if !self.tall.remove(&t) {
                    self.tall.insert(t);
                }
                self.rebuild(g);
                true
            }
            // A plugin of the track went down: its bolt starts it again.
            Hit::TrackIcon(t) if self.crashed_on(t).is_some() => {
                let id = self.crashed_on(t).expect("checked above");
                self.restart_plugin(id)
            }
            Hit::TrackIcon(_) => false,
            Hit::TrackMark(t) => {
                self.press_row(RowKind::Track(t));
                true
            }
            Hit::TrackColor(t) | Hit::Output(t) => {
                let Some(row) = self.tree.row_of(RowKind::Track(t)) else {
                    return false;
                };
                let parts = RowParts::of(row);
                let (kind, r) = if hit == Hit::TrackColor(t) {
                    let Some(r) = parts.swatch else { return false };
                    (PopupKind::Color(t), r)
                } else {
                    let Some(r) = parts.output else { return false };
                    (PopupKind::Output(t), r)
                };
                let anchor = Rect::new(g.column.x + r.x, self.row_y(g, row) + r.y, r.w, r.h);
                self.open_popup(kind, anchor);
                true
            }
            Hit::Keys(t) => {
                let Some(row) = self.tree.row_of(RowKind::Track(t)) else {
                    return false;
                };
                let Some(r) = RowParts::of(row).keys else {
                    return false;
                };
                let anchor = Rect::new(g.column.x + r.x, self.row_y(g, row) + r.y, r.w, r.h);
                self.open_instrument(t, anchor)
            }
            Hit::Start(e) => self.start_press(g, e),
            Hit::Entrance(part) => self.entrance_press(g, part),
            Hit::Rec(part) => self.rec_press(g, part),
            Hit::Arm(t) => self.track_toggle("track.arm", t),
            Hit::Monitor(t) => self.track_toggle("track.monitor", t),
            Hit::Mute(t) => self.track_toggle("track.mute", t),
            Hit::Solo(t) => self.track_toggle("track.solo", t),
            Hit::FolderBus(f) => self.open_folder_bus(g, f),
            Hit::Chevron(f) => {
                if !self.collapsed.remove(&f) {
                    self.collapsed.insert(f);
                }
                self.rebuild(g);
                true
            }
            Hit::Row(kind) => {
                // A press on a track's head chooses the track alone: the clip's part of the
                // inspector leaves, but the clip the piano roll shows stays (UI-313, P2-5 of
                // review 15).
                if matches!(kind, RowKind::Track(_)) {
                    self.arrange.clips.clear();
                }
                self.press_row(kind);
                true
            }
            Hit::Clip(t, c) => {
                self.select_clip_here(t, c);
                true
            }
            Hit::Ruler(tick) => {
                // `ruler.any.empty.click`: the map says what a click on the ruler does.
                let act = {
                    let s = self
                        .song
                        .settings()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    crate::mouse_dispatch::action(
                        &s.mousemap,
                        "ruler.any.empty.click",
                        self.arrange.mods,
                    )
                };
                if matches!(act.as_deref(), None | Some("transport.locate")) {
                    self.seek(tick);
                }
                true
            }
            Hit::Mixer(m) => self.mixer_press(g, m),
            Hit::Tab(tab) => {
                self.drawer_open = !self.drawer_open || self.tab != tab;
                self.tab = tab;
                self.rebuild(g);
                true
            }
            Hit::RollView(drums) => self.roll_view_choose(g, drums),
            Hit::RollMap => self.roll_map_menu(g),
            Hit::RollLength(last) => self.draw_length_choose(last),
        }
    }

    pub(crate) fn command(&mut self, run: impl FnOnce(&mut Song) -> Result<(), String>) -> bool {
        if self.preparing.is_some() || self.unsaved.is_some() {
            return false;
        }
        if let Err(e) = run(&mut self.song) {
            self.error = Some(e);
            return true;
        }
        self.after_edit();
        true
    }

    /// The song changed under us (a command, undo or redo).
    pub(crate) fn after_edit(&mut self) {
        self.roll.scale = self.song.session().state.view.scale;
        self.tree = self.shape_tree();
        let raw = self.project();
        self.instruments.learn(&raw);
        let project = self.auto_heard(&raw);
        self.transport.sync(&project, self.song.dir());
        self.mixer_follow();
        self.keep_edit(xirius_core::store::now_ms());
        self.refresh_popup();
    }

    /// Lays the rows out again, the song unchanged (a bounce's new tracks came or went).
    pub(crate) fn reshape(&mut self) {
        self.tree = self.shape_tree();
    }

    fn shape_tree(&self) -> Tree {
        let p = self.project();
        let lanes = self.arrange.lane_counts(&p);
        let rec = crate::rec_area::heights(self, &p);
        Tree::build_job(
            &p,
            &self.collapsed,
            &self.tall,
            self.arrange.track_h,
            &lanes,
            &rec,
            &crate::bounce_row::heights(self),
        )
    }

    /// The tree, the drawer or the window changed shape: build again and keep
    /// the scroll inside what there is.
    pub(crate) fn rebuild(&mut self, g: &Geometry) {
        self.tree = self.shape_tree();
        let g = self.geometry(g.size.0, g.size.1);
        self.clamp_scroll(&g);
    }

    /// The keys a field takes first, as [`crate::screen::Screen`] sends them: the name being
    /// typed, the top bar's tempo, the inspector's field and the clip menu's field. A field
    /// that does not take a key (F4, Space, Ctrl+Z …) lets it go on to the screen's commands
    /// (UI-312).
    pub(crate) fn field_keys(
        &mut self,
        g: &Geometry,
        key: &str,
        ch: Option<&str>,
        mods: Mods,
    ) -> bool {
        self.inspector_follow(g);
        self.name_key(key, mods)
            || self.bar_key(key, ch, mods)
            || self.inspector_key(g, key, ch, mods)
            || self.field_key(g, key, ch, mods.shift).is_some()
    }

    /// Keyboard shortcuts. `key` is GPUI's key name ("z", "y").
    pub fn key(&mut self, g: &Geometry, key: &str, ctrl: bool, shift: bool) -> bool {
        if self.unsaved_key(key)
            || self.prepare_key(key)
            || self.opening_key(key)
            || self.saving_key(key)
        {
            return true;
        }
        if key == "escape" && self.popup.take().is_some() {
            self.close_wheel_group();
            return true;
        }
        if ctrl && !shift && key == "n" {
            self.start_leaving(crate::saving::Leave::New);
            return true;
        }
        if ctrl && matches!(key, "o" | "s") {
            if key == "o" {
                self.file_request = Some(FileRequest::Open);
            } else if shift || self.song.dir().is_none() {
                self.file_request = Some(FileRequest::SaveAs);
            } else {
                return self.start_save();
            }
            return true;
        }
        if !ctrl {
            return false;
        }
        let moved = match (key, shift) {
            ("z", false) => self.automation_history(false),
            ("z", true) | ("y", _) => self.automation_history(true),
            _ => return false,
        };
        if moved {
            self.after_edit();
            self.follow_selection();
            self.rebuild(g);
        }
        moved
    }

    pub fn undo(&mut self, g: &Geometry) -> bool {
        self.key(g, "z", true, false)
    }

    pub fn redo(&mut self, g: &Geometry) -> bool {
        self.key(g, "z", true, true)
    }

    /// The window changed size: keep the scroll inside what there is.
    pub fn resized(&mut self, g: &Geometry) {
        self.clamp_scroll(g);
        if let Some(ctx) = self.roll_ctx(g) {
            self.roll.sync(&ctx);
        }
    }

    // ----------------------------------------------------------- appearance

    /// What the bar's grid value says (UI-302: it also sets the button's width).
    pub(crate) fn grid_text(&self) -> String {
        let p = self.project();
        // Each view's own width of a beat: the arrangement's auto step is not the roll's.
        let (tools, beat_px) = if self.song.session().state.view.editor == "roll" {
            (self.roll_tools(), self.roll.beat_px)
        } else {
            (self.tools(), self.beat_w)
        };
        // The view's left edge, in song ticks: the meter there decides what the lines show.
        let left = if self.song.session().state.view.editor == "roll" {
            let head = self
                .roll
                .clip
                .and_then(|id| p.clip(id).ok())
                .map_or(0, |(_, c)| c.start);
            head + (f64::from(self.roll.scroll.0 / beat_px.max(1e-3)) * f64::from(p.ppq)) as Tick
        } else {
            self.px_to_ticks(self.scroll.0)
        };
        let m = self.meters.of(&p, beat_px);
        crate::topbar::grid_text(
            self.grid_mode(),
            self.song.grid(),
            tools.snap.step(&m),
            crate::grid_lines::visible(&tools.snap, &m, left),
            p.ppq,
        )
    }

    pub fn bar_view(&self) -> BarView {
        let p = self.project();
        let tempo = xirius_session::tempo::TempoMap::from_project(&p);
        let (shown_tempo, mode) = self.tempo_readout();
        BarView {
            song_name: p.name.clone(),
            save: self.save_mark(),
            save_turn: if self.save_mark() == crate::keep::SaveMark::Saving {
                self.spin()
            } else {
                0.0
            },
            grid: self.song.grid().to_string(),
            grid_text: self.grid_text(),
            draw_length: self.draw_length_text(),
            snap: self.song.snap(),
            tool: self.tool,
            alt: self.alt_tool(),
            arrow_extended: self.tool_area() == crate::tools::Area::Roll && self.roll.extended,
            shape: self.song.session().state.view.draw_shape,
            held: self.held_tool(),
            time: crate::topbar::bar_beat_tick(&p.time_map(), self.playhead),
            seconds: crate::transport::clock(tempo.seconds_at(self.playhead)),
            bpm: match mode {
                TempoMode::Drag | TempoMode::Edit { .. } => shown_tempo,
                _ => format!("{:.1}", tempo.bpm_at(self.playhead)),
            },
            tempo: mode,
            meter: {
                let m = crate::metronome_map::meter_at(&p, self.playhead);
                format!("{}/{}", m.num, m.den)
            },
            open: if self.palette.is_some() {
                Some(BarButton::Palette)
            } else {
                self.popup.as_ref().and_then(|p| match p.kind {
                    PopupKind::Song => Some(BarButton::SongName),
                    PopupKind::Versions => Some(BarButton::Saved),
                    PopupKind::History => Some(BarButton::History),
                    PopupKind::Grid | PopupKind::Quantize => Some(BarButton::Grid),
                    PopupKind::Export => Some(BarButton::Export),
                    PopupKind::Ai => Some(BarButton::Ai),
                    PopupKind::About => Some(BarButton::Logo),
                    PopupKind::Meter => Some(BarButton::Meter),
                    PopupKind::Perf => Some(BarButton::Cpu),
                    _ => None,
                })
            },
            hover: if let Hit::Bar(b) = self.hover {
                Some(b)
            } else {
                None
            },
            can_export: true,
            export_stale: self.export.stale(&self.project()),
            update: self.newer_release().is_some(),
            motion: self.motion.clone(),
            motion_setting: self.ai.motion,
            playing: self.transport.playing,
            recording: self.transport.recording,
            pre_count: self.transport.pre_count.map(|c| c.left),
            record_shake: crate::topbar::shake_offset(
                self.record_shake.map(|t| t.elapsed()),
                self.ai.motion.reduced,
            ),
            pre_roll: self.pre_rolling(),
            pre_roll_on: crate::rec_panel::RecView::of(self).preroll,
            rec_panel: self.rec_panel.open,
            cpu: self.perf.percent(),
            looping: self.transport.looping,
            metronome: self.transport.metronome,
            ripple: self
                .song
                .settings()
                .lock()
                .is_ok_and(|s| s.prefs.bool("edit.ripple")),
            ai: self.ai_bar(),
        }
    }

    pub(crate) fn open_popup(&mut self, kind: PopupKind, anchor: Rect) {
        if matches!(kind, PopupKind::Grid | PopupKind::Quantize) {
            self.choose_editor();
        }
        if self.popup.as_ref().is_some_and(|p| p.kind == kind) {
            self.popup = None;
        } else {
            self.popup = Some(Popup::new(kind, anchor));
            self.refresh_popup();
        }
    }

    /// The topbar's export button, in body coordinates.
    pub(crate) fn export_anchor(&self, g: &Geometry) -> Rect {
        let mut r = BarLayout::new(g.size.0, &self.project().name).rect(BarButton::Export);
        r.y -= Layout::TOP_BAR_H;
        r
    }

    /// A button of the top bar or the bottom bar, in body coordinates: the top bar is above
    /// the body (y < 0) and the bottom bar under it (y ≥ `g.size.1`, UI-300).
    pub(crate) fn bar_rect(&self, g: &Geometry, b: BarButton) -> Rect {
        let mut r = self.bar_layout(g.size.0).rect(b);
        if b.in_bottom() {
            r.y += g.size.1;
        } else {
            r.y -= Layout::TOP_BAR_H;
        }
        r
    }

    pub(crate) fn refresh_popup(&mut self) {
        let project = self.project();
        let export = (self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Export))
            .then(|| Box::new(self.export_view()));
        let plugin = match self.popup.as_ref().map(|p| p.kind) {
            Some(PopupKind::Plugin(t, id) | PopupKind::CrashedPlugin(t, id)) => {
                self.plugin_view(t, id)
            }
            _ => None,
        };
        let auto = match self.popup.as_ref().map(|p| p.kind) {
            Some(PopupKind::AutoValue(at)) => self.point_view(at),
            _ => None,
        };
        let quant = (self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Quantize))
            .then(|| self.quant_view());
        let ai = (self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Ai))
            .then(|| Box::new(self.ai_view()));
        let perf = (self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Perf))
            .then(|| self.perf_view());
        let ctx = (self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Context))
            .then(|| self.context_rows());
        let ask = self
            .ctx
            .as_ref()
            .and_then(|s| s.ask.as_ref())
            .map(|a| a.input.text());
        let query = if ask.is_none() {
            self.context_query()
        } else {
            None
        };
        let mode = self.grid_mode();
        // The view and its choices the list menu builds on are set first.
        if let Some(p) = &mut self.popup {
            p.view = self.browser.view();
            p.views = browser::View::of(self.browser.tab);
        }
        // The list menu's rows as they are now (UI-320); what was typed, the
        // cursor and the scroll are kept by `refill`.
        let pick = match self.popup.as_ref().map(|p| p.kind) {
            Some(kind) if crate::pick_list::is(kind) => self.pick_view(kind),
            _ => None,
        };
        let Some(p) = &mut self.popup else { return };
        if let Some(ctx) = ctx {
            p.ctx = ctx;
            p.ask = ask;
            p.ctx_query = query;
            p.ctx_search = self.ctx.as_ref().is_some_and(|s| s.field.is_none());
        }
        p.history = self.song.history();
        p.notes = self.song.history_notes();
        p.grid = self.song.grid().to_string();
        p.snap = self.song.snap();
        p.mode = mode;
        p.aq = self.auto_quantize;
        p.has_dir = self.song.dir().is_some();
        p.versions = self.song.versions();
        p.offset = p.offset.min(p.max_offset((f32::MAX, f32::MAX)));
        p.now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        p.view = self.browser.view();
        p.views = browser::View::of(self.browser.tab);
        p.can_export = true;
        if p.kind == PopupKind::Song {
            // The song that is open is not offered as a place to go.
            let here = self.song.record_dir();
            p.recent = self
                .keep
                .recent()
                .into_iter()
                .filter(|r| Some(&r.dir) != here.as_ref())
                .collect();
        }
        p.export = export;
        p.perf = perf;
        p.plugin = plugin;
        p.ai = ai;
        p.auto = auto;
        p.quant = quant;
        // The list menu: its rows are built again, but what was typed, the
        // cursor and the scroll stay (UI-320).
        if let Some(mut list) = pick {
            match &mut p.pick {
                Some(old) => old.refill(list),
                _ => {
                    // A list opens on its current row (UI-338).
                    list.start_on_current();
                    p.pick = Some(list);
                }
            }
        }
        if let PopupKind::Color(t) = p.kind
            && let Some(t) = project.tracks.get(&t)
        {
            p.color = t
                .color
                .as_deref()
                .and_then(crate::song::palette_color)
                .map(TrackColor::index);
        }
        p.detail = p
            .selected
            .and_then(|i| p.versions.get(i))
            .and_then(|v| self.song.version_detail(&v.id));
        self.refresh_meter();
    }

    /// With AQ on, the roll's selected notes go to the new grid value.
    fn requantize(&mut self, g: &Geometry) {
        if !self.auto_quantize {
            return;
        }
        if let Some(ctx) = self.roll_ctx(g) {
            let out = self.roll.auto_quantize(&ctx);
            self.apply_roll(g, out);
            self.refresh_popup();
        }
    }

    pub(crate) fn popup_action(&mut self, g: &Geometry, action: Action) -> bool {
        let Some(p) = self.popup.clone() else {
            return false;
        };
        match action {
            Action::Undo => {
                self.undo(g);
            }
            Action::Redo => {
                self.redo(g);
            }
            Action::Jump(n) => {
                self.drag_drop();
                self.command(|s| s.jump(n));
                self.follow_selection();
                self.rebuild(g);
            }
            Action::StepInfo(n) => {
                let p = self.popup.as_mut().unwrap();
                p.open_step = if p.open_step == Some(n) {
                    None
                } else {
                    Some(n)
                };
            }
            Action::Snap(on) => {
                self.command(|s| s.set_snap(on));
            }
            // Picking a value or 3連 also picks クオンタイズ, so the value is used.
            Action::GridValue(base) => {
                let grid = format!("{base}{}", if p.grid.ends_with('t') { "t" } else { "" });
                self.set_grid_mode(SnapMode::Quantize);
                self.command(|s| s.set_grid(&grid));
                self.requantize(g);
            }
            Action::Triplet => {
                let grid = if let Some(base) = p.grid.strip_suffix('t') {
                    base.to_string()
                } else {
                    format!("{}t", p.grid)
                };
                self.set_grid_mode(SnapMode::Quantize);
                self.command(|s| s.set_grid(&grid));
                self.requantize(g);
            }
            // AQ is the setting edit.quantize.auto, so moving between the editors (which reads
            // the settings again) keeps it.
            Action::AutoQuantize => {
                let on = !self.auto_quantize;
                self.rec_set(
                    "prefs.set",
                    serde_json::json!({ "key": "edit.quantize.auto", "value": on.to_string() }),
                );
                self.apply_edit_prefs();
                self.refresh_popup();
            }
            Action::Mode(mode) => {
                self.set_grid_mode(mode);
                self.refresh_popup();
            }
            Action::Color(color) => {
                if let PopupKind::Color(t) = p.kind {
                    let ids = self.track_ids(t);
                    let color =
                        color.map_or("none".to_string(), |c| crate::song::hex(TrackColor::new(c)));
                    self.command(|s| {
                        s.run(
                            "track.color.set",
                            serde_json::json!({"ids": ids, "color": color}),
                        )
                        .map(|_| ())
                    });
                }
                self.popup = None;
            }
            Action::Output(to) => {
                if let PopupKind::Output(t) = p.kind {
                    if t == xirius_core::id::TrackId::MASTER {
                        if let Some((left, right)) = to.split_once(':') {
                            self.run_command(
                                "audio.output.set",
                                serde_json::json!({"left": left, "right": right}),
                            );
                            let pair = self
                                .song
                                .io
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .view
                                .output;
                            self.transport.route_output(pair);
                        }
                        self.popup = None;
                        return true;
                    }
                    let ids = self.track_ids(t);
                    self.command(|s| {
                        s.run(
                            "track.output.set",
                            serde_json::json!({"ids": ids, "to": to}),
                        )
                        .map(|_| ())
                    });
                }
                self.popup = None;
            }
            Action::Input(to) => {
                self.popup = None;
                if let PopupKind::Input(t) = p.kind {
                    self.run_command(
                        "track.input.set",
                        serde_json::json!({ "ids": self.track_ids(t), "input": to }),
                    );
                }
            }
            Action::AddTrack(kind) => {
                self.popup = None;
                self.command(|s| s.add_track(kind));
                self.rebuild(g);
            }
            Action::FromFile => {
                self.popup = None;
                self.file_request = Some(FileRequest::Audio);
            }
            Action::View(view) => {
                self.browser.set_view(view);
                self.popup = None;
            }
            Action::OpenSong => {
                self.file_request = Some(FileRequest::Open);
                self.popup = None;
            }
            Action::OpenDawproject => {
                self.file_request = Some(FileRequest::OpenDawproject);
                self.popup = None;
            }
            Action::ExportDawproject => {
                self.file_request = Some(FileRequest::ExportDawproject);
                self.popup = None;
            }
            Action::NewSong => {
                self.popup = None;
                self.start_leaving(crate::saving::Leave::New);
                self.rebuild(g);
            }
            Action::OpenRecent(i) => {
                self.popup = None;
                if let Some(song) = p.recent.get(i) {
                    self.start_file_request(FileRequest::Open, &song.dir);
                    self.rebuild(g);
                }
            }
            Action::Recovery(b) => self.recovery_button(g, b),
            Action::SaveAs => {
                self.file_request = Some(FileRequest::SaveAs);
                self.popup = None;
            }
            Action::Save => {
                if self.song.dir().is_none() {
                    self.file_request = Some(FileRequest::SaveAs);
                } else {
                    self.start_save();
                }
                self.popup = None;
            }
            Action::ShowVersions => {
                self.popup = None;
                self.open_popup(PopupKind::Versions, p.anchor);
            }
            Action::SongSettings => {
                self.popup = None;
                self.open_palette_form("project.settings", serde_json::json!({}));
            }
            Action::NameVersion => {
                if p.has_dir {
                    self.popup.as_mut().unwrap().naming = Some(String::new());
                }
            }
            Action::SelectVersion(i) => {
                self.popup.as_mut().unwrap().selected = Some(i);
            }
            Action::Restore | Action::OpenAsSong => {
                if let Some(v) = p.selected.and_then(|i| p.versions.get(i)) {
                    if action == Action::Restore {
                        self.command(|s| s.restore_version(&v.id));
                        self.follow_selection();
                        self.rebuild(g);
                    } else {
                        self.file_request = Some(FileRequest::OpenVersion(v.id.clone()));
                    }
                    self.popup = None;
                }
            }
            Action::Pick(to) => {
                // Scanning again or stopping keeps the list open to show how it goes.
                if to != crate::mixer::scan::RESCAN && to != crate::mixer::scan::STOP {
                    self.popup = None;
                }
                if !to.is_empty() {
                    self.mixer_pick(p.kind, &to);
                }
            }
            Action::Export => {
                self.popup = None;
                self.open_export(self.export_anchor(g));
                return true;
            }
            Action::Xp(act) => return self.export_action(act),
            Action::Plugin(act) => return self.plugin_action(act),
            Action::Ai(act) => return self.ai_act(act),
            Action::Quant(act) => return self.quant_action(g, act),
            Action::Meter(act) => return self.meter_action(act),
            Action::Report(act) => return self.report_action(act),
            Action::Cleanup(act) => return self.cleanup_action(g, act),
            Action::Cpu(act) => return self.cpu_action(act),
            Action::OpenNoteEdit(kind) => {
                self.open_note_edit(kind, p.anchor);
                return true;
            }
            Action::EditValue(kind, value) => return self.edit_note_value(g, kind, value),
            Action::Link => {
                self.run_command("view.link", serde_json::json!({}));
                self.popup = None;
                return true;
            }
            Action::AutomationTarget(t, from, to) => return self.pick_auto_target(g, t, from, to),
            Action::FolderBus(to) => return self.folder_bus_action(g, to),
            Action::RecQuantize(v) => return self.rec_quantize_action(v),
            Action::Device(name) => return self.device_action(g, name),
            Action::About(act) => return self.about_action(act),
            Action::Ctx(i) => return self.context_pick(g, i),
            Action::CtxSearch => {
                self.context_search();
                return true;
            }
            Action::Field(i, act) => return self.field_action(g, i, act),
        }
        self.refresh_popup();
        true
    }

    pub fn typed(&mut self, g: &Geometry, key: &str, ch: Option<&str>) -> bool {
        if let Some(done) = self.ask_key(key) {
            return done;
        }
        if self.pick_typed(g, key, ch) {
            return true;
        }
        if let Some(done) = self.auto_typed(key, ch) {
            return done;
        }
        if let Some(done) = self.meter_typed(key, ch) {
            return done;
        }
        if let Some(done) = self.about_typed(key, ch) {
            return done;
        }
        if let Some(p) = &mut self.popup {
            if let Some(name) = &p.naming {
                if key == "enter" && p.kind == PopupKind::Export {
                    self.commit_export_name();
                    self.refresh_popup();
                    return true;
                }
                if key == "enter" {
                    let name = name.trim().to_string();
                    if !name.is_empty() {
                        self.pull_plugins();
                        self.command(|s| s.save_version(&name));
                        if self.error.is_none() {
                            self.popup.as_mut().unwrap().naming = None;
                        }
                    }
                    return true;
                }
                return p.typed(key, ch);
            }
            return false;
        }
        self.browser.typed(key, ch)
    }

    /// Nonblocking file-dialog and script entry. Synchronous fixtures have an
    /// explicitly named helper below and never run from a window callback.
    pub fn finish_file_request(&mut self, request: FileRequest, path: &std::path::Path) {
        if self.preparing.is_some() {
            return;
        }
        if matches!(
            request,
            FileRequest::Open | FileRequest::OpenVersion(_) | FileRequest::OpenDawproject
        ) {
            self.request_open_file(request, path);
        } else {
            self.finish_file_request_sync(request, path);
        }
    }

    pub fn finish_file_request_sync(&mut self, request: FileRequest, path: &std::path::Path) {
        if self.preparing.is_some() {
            return;
        }
        if self.finish_preset_file(request.clone(), path) {
            return;
        }
        if matches!(
            request,
            FileRequest::Audio
                | FileRequest::Place
                | FileRequest::Locate(_)
                | FileRequest::LocateFolder
                | FileRequest::MediaFolder
        ) {
            return self.finish_picked(request, &[path.to_path_buf()]);
        }
        if request == FileRequest::OpenDawproject {
            self.dawproject_open(path);
            return;
        }
        if request == FileRequest::ExportDawproject {
            self.dawproject_export(path);
            return;
        }
        let replaces_song = !matches!(request, FileRequest::SaveAs);
        if !replaces_song && self.transport.take.is_some() {
            self.error = Some("録音中は名前を付けて保存できません".into());
            return;
        }
        // A cleanup of the song in `path` still running moves nothing more; nor does the
        // request wait for it.
        self.stop_tidy(path);
        if replaces_song {
            self.transport.pause();
            self.keep_leaving();
        } else {
            self.pull_plugins();
        }
        let leaving_draft = self
            .song
            .is_draft()
            .then(|| self.song.record_dir())
            .flatten();
        let closing = if replaces_song {
            self.closing()
        } else {
            Ok(None)
        };
        self.error = None;
        let result = match request {
            FileRequest::Open => self.song.open(path),
            FileRequest::SaveAs => self.song.save_as(path),
            FileRequest::OpenVersion(id) => self.song.open_version_as_song(&id, path),
            FileRequest::Audio
            | FileRequest::Place
            | FileRequest::PresetOpen(_)
            | FileRequest::PresetSave(_)
            | FileRequest::OpenDawproject
            | FileRequest::ExportDawproject
            | FileRequest::Locate(_)
            | FileRequest::LocateFolder
            | FileRequest::MediaFolder => Ok(()),
        };
        if let Err(e) = result {
            self.error = Some(e);
            return;
        }
        if !replaces_song {
            self.transport.relocate(self.song.dir());
        }
        self.after_edit();
        if self.error.is_some() {
            return;
        }
        let now = xirius_core::store::now_ms();
        if replaces_song {
            self.song.mark_draft(self.keep.is_draft_dir(path));
            self.keep
                .remember(path, &self.project().name, self.song.is_draft(), now);
            self.song_replaced(closing);
        } else {
            // The song has a place of its own: the draft that kept it is not needed.
            if let Some(draft) = leaving_draft {
                self.keep.delete_draft(&draft);
            }
            self.keep.remember(path, &self.project().name, false, now);
        }
    }

    /// A new, empty song in place of this one (UI-078): the screen is back at the
    /// entrance, where it starts.
    pub fn new_song(&mut self) {
        self.request_open(crate::opening::Request::New);
    }

    #[cfg(test)]
    pub fn new_song_sync(&mut self) {
        self.keep_leaving();
        let closing = self.closing();
        self.song = Song::empty_after(&self.song);
        self.error = None;
        self.song_replaced(closing);
    }

    /// The screen's state belonged to the song that was closed: begin again, start its
    /// cleanup (said when it ends), and say what came back if it was opened after a crash.
    pub(crate) fn song_replaced(
        &mut self,
        closing: Result<Option<xirius_session::cleanup::Closed>, String>,
    ) {
        self.song_replaced_with(closing, false);
    }

    pub(crate) fn song_replaced_with(
        &mut self,
        closing: Result<Option<xirius_session::cleanup::Closed>, String>,
        deferred: bool,
    ) {
        self.drag_drop();
        self.select_row(None);
        self.selected_clip = None;
        self.collapsed.clear();
        self.tall.clear();
        self.arrange = crate::arrange::Arrange::default();
        self.scroll = (0.0, 0.0);
        self.playhead = 0;
        self.transport.reset();
        self.mixer.meters.clear();
        if deferred {
            self.keep.sync_open(
                &self.song.project(),
                self.song.record_dir(),
                self.song.is_draft(),
            );
        } else {
            self.keep.sync(&self.song.project());
        }
        self.recovery = None;
        self.export.forget_bases();
        self.after_edit();
        // The song that was closed is tidied once the new one is open, off the screen's
        // thread (REC-026): on a slow drive it took minutes.
        match closing {
            Err(e) => self.error = Some(e),
            Ok(Some(closed)) => self.start_tidy(closed),
            Ok(None) => {}
        }
        self.show_recovery();
        self.prepare_on_open();
    }

    /// The song is closed (another is opened, or the window is): the
    /// recordings nothing names any more go to the recycle bin (REC-016), as
    /// `xirius play` does when it ends. Only a saved song has recordings;
    /// while a take is being made they are all left, since it is not in the
    /// song yet. Returns how many moved.
    pub fn close_song(&mut self) -> Result<usize, String> {
        if self.song.dir().is_none() || self.transport.recording {
            return Ok(0);
        }
        self.song.cleanup()
    }

    /// The song is about to close for another: its cleanup (REC-016), to run once the next one
    /// is open (REC-026), with the song as it is now. None when it has nothing to tidy (not
    /// saved, or a take is being made); an error while an AI's work copy may still need its
    /// files, as `project.cleanup` says.
    pub(crate) fn closing(&self) -> Result<Option<xirius_session::cleanup::Closed>, String> {
        let Some(dir) = self.song.dir() else {
            return Ok(None);
        };
        if self.transport.recording {
            return Ok(None);
        }
        if self.song.session().draft_project().is_some() {
            return Err(
                xirius_core::CmdError::conflict(xirius_session::cleanup::WORK_COPY).to_string(),
            );
        }
        Ok(Some(
            xirius_session::cleanup::Closed::new(self.song.project(), dir, self.song.trash())
                .closed_by(self.song.session().uses_id()),
        ))
    }

    fn is_audio(&self, t: TrackId) -> bool {
        self.project()
            .tracks
            .get(&t)
            .is_some_and(|x| x.kind == xirius_core::model::TrackKind::Audio)
    }

    /// The row the pointer is over, whichever part of it.
    pub fn hover_row(&self) -> Option<RowKind> {
        match self.hover {
            Hit::Row(kind) => Some(kind),
            Hit::Chevron(f) | Hit::FolderBus(f) => Some(RowKind::Folder(f)),
            Hit::Mute(t) | Hit::Solo(t) | Hit::TrackMark(t) | Hit::Clip(t, _) => {
                Some(RowKind::Track(t))
            }
            _ => None,
        }
    }

    /// How a folder's range line is drawn right now (MIX-038).
    pub fn line_state(&self, bar: &Bar) -> LineState {
        if self.selected_row == Some(RowKind::Folder(bar.folder)) {
            return LineState::Selected;
        }
        let scope = self
            .hover_row()
            .and_then(|kind| self.tree.row_of(kind))
            .and_then(Row::folder_in_scope);
        if scope == Some(bar.folder) {
            LineState::Hover
        } else {
            LineState::Rest
        }
    }

    /// What the piano roll shows: the selected clip, when it holds notes.
    pub fn roll_target(&self) -> Option<RollTarget> {
        let (t, c) = self.selected_clip?;
        let project = self.project();
        let track = project.tracks.get(&t)?.clone();
        let clip = track.clips.iter().find(|clip| clip.id == c)?.clone();
        let notes = match &clip.body {
            xirius_core::model::ClipBody::Midi { notes, .. } => notes.clone(),
            _ => return None,
        };
        Some(RollTarget { track, clip, notes })
    }

    /// Bars in the sections' band and ruler that are in view: (bar number, tick).
    pub fn visible_bars(&self, g: &Geometry) -> Vec<(u32, Tick)> {
        let project = self.project();
        let scale = self.ruler_scale(g);
        crate::ruler::bars_in(
            &project.time_map(),
            scale.tick(g.lanes.x),
            scale.tick(g.lanes.right()),
        )
    }
}

/// [`Studio::beat_w_max`] of a song at `rate` with `tempo`: a beat of the slowest tempo (20 BPM
/// at the least, as the tempo track goes) holds `rate * 60 / bpm` samples.
pub fn beat_w_max(rate: u32, tempo: &[xirius_core::model::TempoPoint]) -> f32 {
    let slowest = tempo.iter().map(|t| t.bpm).fold(f64::INFINITY, f64::min);
    let slowest = if slowest.is_finite() { slowest } else { 120.0 }.max(20.0);
    let samples = f64::from(rate.max(1)) * 60.0 / slowest;
    ((samples * Studio::SAMPLE_PX) as f32).max(Studio::BEAT_W_MAX)
}

/// The highest pitch the roll shows, so that `rows` rows are centred on the
/// notes (and the roll never runs off the keyboard).
pub fn roll_top_pitch(notes: &[Note], rows: usize) -> u8 {
    let (lo, hi) = notes.iter().fold((127u8, 0u8), |(lo, hi), n| {
        (lo.min(n.pitch), hi.max(n.pitch))
    });
    if notes.is_empty() {
        return 72;
    }
    let mid = (u32::from(lo) + u32::from(hi)) / 2;
    let top = mid + rows as u32 / 2;
    let min_top = (rows as u32).saturating_sub(1);
    top.clamp(min_top, 127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::id::NoteId;
    use xirius_core::model::Output;

    fn studio() -> Studio {
        Studio::sample()
    }

    #[test]
    fn the_songs_meter_is_built_again_only_when_the_song_changes() {
        // The views snap with it at every pointer move and draw with it at
        // every frame (UI-339).
        let mut s = studio();
        let same = |a: &crate::tools::Meter, b: &crate::tools::Meter| {
            Arc::ptr_eq(&a.time, &b.time) && Arc::ptr_eq(&a.tempo, &b.tempo)
        };
        let a = s.meter();
        assert!(same(&a, &s.meter()));
        s.song
            .run("timesig.set", serde_json::json!({ "num": 7, "den": 8 }))
            .unwrap();
        let b = s.meter();
        assert!(!same(&a, &b));
        assert_eq!(b.time.meter_at(1).num, 7);
        assert!(same(&b, &s.meter()));
    }

    fn geometry(s: &Studio) -> Geometry {
        s.geometry(1440.0, 860.0)
    }

    fn track_named(s: &Studio, name: &str) -> TrackId {
        s.project()
            .tracks
            .values()
            .find(|t| t.name == name)
            .expect("a track")
            .id
    }

    /// The screen centre of a row's part.
    fn centre(
        s: &Studio,
        g: &Geometry,
        kind: RowKind,
        part: impl Fn(&RowParts) -> Option<Rect>,
    ) -> (f32, f32) {
        let row = s.tree().row_of(kind).expect("a row");
        let r = part(&RowParts::of(row)).expect("a part");
        (
            g.column.x + r.x + r.w / 2.0,
            s.row_y(g, row) + r.y + r.h / 2.0,
        )
    }

    #[test]
    fn regions_do_not_overlap_and_stay_in_the_window() {
        for open in [true, false] {
            let g = Geometry::new(1440.0, 860.0, open);
            for r in [g.head, g.section_band, g.ruler, g.column, g.lanes, g.drawer] {
                assert!(r.x >= 0.0 && r.right() <= 1440.0, "{r:?}");
                assert!(r.y >= 0.0 && r.bottom() <= 860.0, "{r:?}");
            }
            assert!(g.column.bottom() < g.drawer.y);
            assert!(g.head.right() < g.lanes.x);
            assert!(g.ruler.bottom() < g.lanes.y);
            assert_eq!(
                g.drawer.h,
                if open {
                    Layout::DRAWER_H
                } else {
                    Layout::DRAWER_BAR_H
                }
            );
        }
    }

    #[test]
    fn shutting_the_drawer_gives_its_room_to_the_rows() {
        let open = Geometry::new(1440.0, 860.0, true);
        let shut = Geometry::new(1440.0, 860.0, false);
        assert_eq!(
            shut.lanes.h - open.lanes.h,
            Layout::DRAWER_H - Layout::DRAWER_BAR_H
        );
    }

    #[test]
    fn a_track_row_has_both_toggles_inside_the_column_and_a_name_that_stops_before_them() {
        let s = studio();
        for row in &s.tree().rows {
            let p = RowParts::of(row);
            let right = row.head_w - Space::X2.px();
            assert!(p.icon.right() <= right);
            assert!(p.name_x < p.name_right, "{:?}", row.kind);
            if let (Some(m), Some(so)) = (p.mute, p.solo) {
                assert!(m.right() <= so.x);
                assert!(m.bottom() <= row.h);
            }
            // Conga, three folders deep, keeps its last button inside the head.
            for b in [p.mute, p.solo, p.arm, p.ear, p.keys].into_iter().flatten() {
                assert!(b.right() <= right, "{:?} {b:?}", row.kind);
            }
            // The level sits at the right edge, right of every button.
            if let Some(m) = p.meter {
                assert!((m.right() - right).abs() < 0.5);
                for b in [p.mute, p.solo, p.arm, p.ear, p.keys, p.widen, p.auto]
                    .into_iter()
                    .flatten()
                {
                    assert!(b.right() < m.x, "{:?} {b:?}", row.kind);
                }
                assert!(m.bottom() <= row.h);
            }
            assert_eq!(p.meter.is_some(), matches!(row.kind, RowKind::Track(_)));
            assert_eq!(p.chevron.is_some(), matches!(row.kind, RowKind::Folder(_)));
        }
    }

    #[test]
    fn the_press_on_m_runs_a_core_command_and_ctrl_z_takes_it_back() {
        let mut s = studio();
        let g = geometry(&s);
        let drums = track_named(&s, "Drums");
        let (x, y) = centre(&s, &g, RowKind::Track(drums), |p| p.mute);
        let hit = s.hit(&g, x, y);
        assert_eq!(hit, Hit::Mute(drums));
        assert!(!s.project().tracks[&drums].mute);
        assert!(s.press(&g, hit));
        assert!(s.project().tracks[&drums].mute);
        assert_eq!(s.song().steps(), 1);
        assert!(s.key(&g, "z", true, false));
        assert!(!s.project().tracks[&drums].mute);
        assert!(s.key(&g, "z", true, true));
        assert!(s.project().tracks[&drums].mute);
        assert!(!s.key(&g, "z", false, false), "z alone is not undo");
    }

    #[test]
    fn solo_and_mute_are_separate_presses() {
        let mut s = studio();
        let g = geometry(&s);
        let keys = track_named(&s, "Keys");
        let (x, y) = centre(&s, &g, RowKind::Track(keys), |p| p.solo);
        s.press(&g, s.hit(&g, x, y));
        let p = s.project();
        assert!(p.tracks[&keys].solo && !p.tracks[&keys].mute);
    }

    #[test]
    fn the_chevron_folds_a_folder_and_the_lines_and_rows_go_with_it() {
        let mut s = studio();
        let g = geometry(&s);
        let rows = s.tree().rows.len();
        let bars = s.tree().bars.len();
        let rhythm = s
            .tree()
            .rows
            .iter()
            .find_map(|r| match r.kind {
                RowKind::Folder(f) if r.depth == 0 => Some(f),
                _ => None,
            })
            .expect("a root folder");
        let (x, y) = centre(&s, &g, RowKind::Folder(rhythm), |p| p.chevron);
        assert_eq!(s.hit(&g, x, y), Hit::Chevron(rhythm));
        s.press(&g, Hit::Chevron(rhythm));
        assert!(s.is_collapsed(rhythm));
        assert!(s.tree().rows.len() < rows);
        assert!(s.tree().bars.len() < bars);
        s.press(&g, Hit::Chevron(rhythm));
        assert_eq!(s.tree().rows.len(), rows);
        assert_eq!(s.tree().bars.len(), bars);
    }

    #[test]
    fn a_click_on_a_clip_selects_it_and_the_roll_follows() {
        let mut s = studio();
        let g = geometry(&s);
        let bass = track_named(&s, "Bass");
        let row = *s.tree().row_of(RowKind::Track(bass)).expect("a row");
        let project = s.project();
        let clip = project.tracks[&bass].clips[0].clone();
        let r = s.clip_rect(&g, &row, &clip);
        let hit = s.hit(&g, r.x + 4.0, r.y + 4.0);
        assert_eq!(hit, Hit::Clip(bass, clip.id));
        s.press(&g, hit);
        assert_eq!(s.roll_target().expect("a roll").track.name, "Bass");
        // Beside the clip is the lane, not the clip.
        assert_eq!(
            s.hit(&g, r.right() + 4.0, r.y + 4.0),
            Hit::Row(RowKind::Track(bass))
        );
    }

    #[test]
    fn an_audio_clip_has_nothing_for_the_roll() {
        let mut s = studio();
        assert!(s.select_clip_of("Vocal"));
        assert!(s.roll_target().is_none());
    }

    #[test]
    fn the_ruler_moves_the_playhead_to_the_tick_under_the_pointer() {
        let mut s = studio();
        let g = geometry(&s);
        let x = s.tick_x(&g, 3840 * 2);
        // The top of the ruler is the loop band; the numbers are below it.
        assert!(matches!(
            s.arrange_spot(&g, x, g.ruler.y + 4.0),
            crate::arrange::Spot::Loop(_)
        ));
        let hit = s.hit(&g, x + 0.2, g.ruler.y + g.ruler.h - 4.0);
        assert!(matches!(hit, Hit::Ruler(t) if (t - 3840 * 2).abs() < 100));
        s.press(&g, hit);
        assert!((s.playhead - 3840 * 2).abs() < 100);
    }

    #[test]
    fn the_playhead_starts_at_the_verse() {
        let s = studio();
        assert_eq!(s.project().time_map().format(s.playhead), "5.1.0");
    }

    #[test]
    fn scrolling_stays_inside_the_content() {
        let mut s = studio();
        let g = geometry(&s);
        let (mx, my) = s.max_scroll(&g);
        s.scroll_by(&g, 0.0, -10_000.0, false);
        assert_eq!(s.scroll.1, my);
        s.scroll_by(&g, 0.0, 10_000.0, false);
        assert_eq!(s.scroll.1, 0.0);
        s.scroll_by(&g, 0.0, -10_000.0, true);
        assert_eq!(s.scroll.0, mx);
        s.scroll_by(&g, 0.0, 10_000.0, true);
        assert_eq!(s.scroll.0, 0.0);
    }

    #[test]
    fn zoom_keeps_the_tick_under_the_pointer_and_stays_in_range() {
        let mut s = studio();
        let g = geometry(&s);
        let x = g.lanes.x + 600.0;
        let before = s.px_to_ticks(x - g.lanes.x + s.scroll.0);
        s.zoom(&g, x, 3.0);
        assert!(s.beat_w > Layout::BEAT_W);
        let after = s.px_to_ticks(x - g.lanes.x + s.scroll.0);
        assert!((after - before).abs() <= 2, "{before} -> {after}");
        s.zoom(&g, x, 100.0);
        assert_eq!(s.beat_w, s.beat_w_max());
        s.zoom(&g, x, -300.0);
        assert_eq!(s.beat_w, Studio::BEAT_W_MIN);
        assert!(s.scroll.0 >= 0.0 && s.scroll.0 <= s.max_scroll(&g).0);
    }

    #[test]
    fn center_of_lands_on_what_it_names() {
        let s = studio();
        // Tall enough that every row is in view.
        let g = s.geometry(1440.0, 1100.0);
        let mut targets = vec![Hit::Tab(s.tab), Hit::Ruler(960 * 8)];
        targets.extend(DrawerTab::ALL.map(Hit::Tab));
        for row in &s.tree().rows {
            targets.push(Hit::Row(row.kind));
            match row.kind {
                RowKind::Folder(f) => targets.extend([Hit::Chevron(f), Hit::FolderBus(f)]),
                RowKind::Track(t) => {
                    targets.extend([Hit::Mute(t), Hit::Solo(t)]);
                    let project = s.project();
                    for clip in &project.tracks[&t].clips {
                        targets.push(Hit::Clip(t, clip.id));
                    }
                }
            }
        }
        for target in targets {
            let (x, y) = s
                .center_of(&g, target)
                .unwrap_or_else(|| panic!("{target:?} should be on screen"));
            let got = s.hit(&g, x, y);
            match (target, got) {
                (Hit::Ruler(_), Hit::Ruler(_)) => {}
                _ => assert_eq!(got, target),
            }
        }
    }

    #[test]
    fn center_of_is_none_for_a_row_scrolled_out() {
        let mut s = studio();
        let g = s.geometry(1440.0, 640.0);
        s.scroll_by(&g, 0.0, -10_000.0, false);
        let first = s.tree().rows[0].kind;
        assert_eq!(s.center_of(&g, Hit::Row(first)), None);
    }

    #[test]
    fn hover_row_names_the_row_whatever_part_is_under_the_pointer() {
        let mut s = studio();
        let keys = track_named(&s, "Keys");
        s.hover = Hit::Mute(keys);
        assert_eq!(s.hover_row(), Some(RowKind::Track(keys)));
        s.hover = Hit::Tab(s.tab);
        assert_eq!(s.hover_row(), None);
    }

    #[test]
    fn rows_scroll_together_in_both_columns() {
        let mut s = studio();
        let g = s.geometry(1440.0, 640.0);
        assert!(
            s.max_scroll(&g).1 > 0.0,
            "the sample is taller than this window"
        );
        s.scroll_by(&g, 0.0, -50.0, false);
        let row = *s.tree().rows.first().expect("a row");
        assert_eq!(s.row_y(&g, &row), g.column.y - 50.0);
    }

    #[test]
    fn a_taller_window_needs_no_vertical_scroll() {
        let s = studio();
        let g = s.geometry(1440.0, 1200.0);
        assert_eq!(s.max_scroll(&g).1, 0.0);
    }

    #[test]
    fn shutting_the_drawer_keeps_the_scroll_inside_the_longer_view() {
        let mut s = studio();
        let g = s.geometry(1440.0, 640.0);
        s.scroll_by(&g, 0.0, -10_000.0, false);
        let before = s.scroll.1;
        s.press(&g, Hit::Tab(s.tab));
        assert!(!s.drawer_open);
        assert!(s.scroll.1 < before);
        let shut = s.geometry(1440.0, 640.0);
        assert!(s.scroll.1 <= s.max_scroll(&shut).1);
    }

    #[test]
    fn the_tabs_switch_and_open_the_drawer() {
        let mut s = studio();
        let g = geometry(&s);
        s.press(&g, Hit::Tab(s.tab));
        assert!(!s.drawer_open);
        let g = geometry(&s);
        let t = g.tab(DrawerTab::Mixer);
        let hit = s.hit(&g, t.x + 2.0, t.y + 2.0);
        assert_eq!(hit, Hit::Tab(DrawerTab::Mixer));
        s.press(&g, hit);
        assert!(s.drawer_open);
        assert_eq!(s.tab, DrawerTab::Mixer);
        s.press(&g, hit);
        assert!(!s.drawer_open);
        s.press(&g, hit);
        assert!(s.drawer_open);
        // The toggle's place at the bar's right end is not a button: the tabs are the only way.
        let end = g.drawer_bar;
        assert_eq!(s.hit(&g, end.right() - 2.0, end.y + 2.0), Hit::None);
    }

    #[test]
    fn the_drawer_buttons_do_not_overlap() {
        let g = Geometry::new(1440.0, 860.0, true);
        assert!(g.tab(DrawerTab::Roll).right() <= g.tab(DrawerTab::Mixer).x);
        assert!(g.tab(DrawerTab::Mixer).right() < g.roll_switch(false).x);
        assert_eq!(Geometry::TAB_W, 28.0);
        assert_eq!(Geometry::TAB_H, 24.0);
        assert_eq!(g.roll_switch(false).x - g.drawer_bar.x, 82.0);
        assert_eq!(g.roll_length().x - g.drawer_bar.x, 156.0);
        assert_eq!(g.roll_length().right() - g.drawer_bar.x, 184.0);
        assert_eq!(g.roll_map().x - g.drawer_bar.x, 148.0);
    }

    #[test]
    fn a_folders_line_is_neutral_until_the_pointer_is_in_it_or_it_is_selected() {
        let mut s = studio();
        let g = geometry(&s);
        let bar = *s.tree().bars.first().expect("a bar");
        assert_eq!(s.line_state(&bar), LineState::Rest);
        // Over a track in that folder (hover the track's M button too).
        let drums = track_named(&s, "Drums");
        s.hover = Hit::Mute(drums);
        let drums_row = s
            .tree()
            .row_of(RowKind::Track(drums))
            .copied()
            .expect("a row");
        let inner = s
            .tree()
            .bars
            .iter()
            .find(|b| Some(b.folder) == drums_row.parent)
            .copied()
            .expect("a bar");
        assert_eq!(s.line_state(&inner), LineState::Hover);
        // The outer folder's line stays neutral: only the nearest folder lights up.
        let outer = s.tree().bars.iter().find(|b| b.depth < inner.depth);
        if let Some(outer) = outer {
            assert_eq!(s.line_state(outer), LineState::Rest);
        }
        s.press(&g, Hit::Row(RowKind::Folder(bar.folder)));
        assert_eq!(s.line_state(&bar), LineState::Selected);
    }

    #[test]
    fn roll_is_centred_on_the_notes_and_never_runs_off_the_keyboard() {
        let note = |pitch| Note {
            id: NoteId(1),
            start: 0,
            length: 10,
            pitch,
            velocity: 100,
            muted: false,
            ai: false,
        };
        assert_eq!(roll_top_pitch(&[note(60), note(66)], 24), 63 + 12);
        assert_eq!(roll_top_pitch(&[note(0)], 24), 23);
        assert_eq!(roll_top_pitch(&[note(127)], 24), 127);
        assert_eq!(roll_top_pitch(&[], 24), 72);
    }

    #[test]
    fn the_end_of_the_song_leaves_room_after_the_last_section() {
        let s = studio();
        let tm = s.project().time_map();
        assert!(s.end_tick() > tm.bar_start(21));
    }

    fn click(s: &mut Studio, hit: Hit) {
        let g = geometry(s);
        let (x, y) = s
            .center_of(&g, hit)
            .unwrap_or_else(|| panic!("not visible: {hit:?}"));
        assert_eq!(s.hit(&g, x, y), hit);
        assert!(s.press(&g, hit));
        assert_eq!(s.error, None);
    }

    fn choose(s: &mut Studio, action: Action) {
        let i = s
            .popup
            .as_ref()
            .unwrap()
            .layout(geometry(s).size)
            .cells
            .iter()
            .position(|c| c.live() && c.action.as_ref() == Some(&action))
            .unwrap();
        click(s, Hit::Item(i));
    }

    #[test]
    fn the_history_opens_the_one_line_of_each_operation_and_of_a_permission_change() {
        let mut s = studio();
        let id = track_named(&s, "Drums");
        click(&mut s, Hit::Mute(id));
        click(&mut s, Hit::Bar(BarButton::History));
        choose(&mut s, Action::StepInfo(1));
        let g = geometry(&s);
        let layout = s.popup.as_ref().unwrap().layout(g.size);
        let cli = layout
            .cells
            .iter()
            .find(|c| matches!(c.look, crate::popup::Look::Cli { .. }))
            .expect("the one line is open");
        assert!(cli.text.starts_with("$ xirius track.mute"), "{}", cli.text);
        assert!(cli.text.lines().last().unwrap().starts_with("取り消し"));
        // Pressing the button again closes it.
        choose(&mut s, Action::StepInfo(1));
        let layout = s.popup.as_ref().unwrap().layout(g.size);
        assert!(
            !layout
                .cells
                .iter()
                .any(|c| matches!(c.look, crate::popup::Look::Cli { .. }))
        );

        // A change of the AI's permission is a line of the history too.
        s.ai_act(crate::ai::Act::Level(xirius_core::AiPermission::Read));
        let notes = s.popup.as_ref().unwrap().notes.clone();
        assert_eq!(notes.len(), 1);
        choose(&mut s, Action::StepInfo(notes[0].n));
        let layout = s.popup.as_ref().unwrap().layout(g.size);
        let cli = layout
            .cells
            .iter()
            .find(|c| matches!(c.look, crate::popup::Look::Cli { .. }))
            .unwrap();
        assert_eq!(
            cli.text,
            "$ xirius ai.permission.set --level read\n→ 読み取りのみ"
        );
    }

    #[test]
    fn history_buttons_and_jumps_update_core_and_the_open_popup() {
        let mut s = studio();
        // Undo here must land exactly on the state the test kept; the
        // instrument-input arm that would follow the selection is not
        // what this test checks (REC-024).
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
        );
        let id = track_named(&s, "Drums");
        click(&mut s, Hit::Mute(id));
        click(&mut s, Hit::Arm(id));
        click(&mut s, Hit::Bar(BarButton::History));
        choose(&mut s, Action::Undo);
        assert!(s.project().tracks[&id].mute);
        assert!(!s.project().tracks[&id].armed);
        assert!(s.popup.as_ref().unwrap().history.last().unwrap().undone);
        choose(&mut s, Action::Redo);
        assert!(s.project().tracks[&id].armed);
        choose(&mut s, Action::Jump(0));
        assert!(!s.project().tracks[&id].mute);
        assert!(!s.project().tracks[&id].armed);
    }

    #[test]
    fn grid_triplet_and_snap_flow_through_the_popup_to_core() {
        let mut s = studio();
        click(&mut s, Hit::Bar(BarButton::Grid));
        choose(&mut s, Action::GridValue("1/8"));
        choose(&mut s, Action::Triplet);
        choose(&mut s, Action::Snap(false));
        assert_eq!(s.song.grid(), "1/8t");
        assert!(!s.song.snap());
        assert_eq!(s.bar_view().grid, "1/8t");
        assert!(!s.popup.as_ref().unwrap().snap);
    }

    #[test]
    fn color_output_and_record_are_undoable_without_changing_the_roll() {
        let mut s = studio();
        let id = track_named(&s, "Drums");
        let clip = s.selected_clip;
        let old = s.project().tracks[&id].color.clone();
        click(&mut s, Hit::TrackColor(id));
        choose(&mut s, Action::Color(Some(4)));
        assert_eq!(
            s.project().tracks[&id].color,
            Some(crate::song::hex(TrackColor::new(4)))
        );
        let g = geometry(&s);
        s.undo(&g);
        assert_eq!(s.project().tracks[&id].color, old);
        click(&mut s, Hit::Widen(id));
        assert!(s.tree.row_of(RowKind::Track(id)).unwrap().tall);
        click(&mut s, Hit::Output(id));
        choose(&mut s, Action::Output("master".into()));
        assert_eq!(s.project().tracks[&id].output, Some(Output::Master));
        click(&mut s, Hit::Arm(id));
        assert!(s.project().tracks[&id].armed);
        assert_eq!(s.selected_clip, clip);
        let g = geometry(&s);
        let row = s.tree.row_of(RowKind::Track(id)).unwrap();
        let keys = RowParts::of(row).keys.unwrap();
        // The keys button opens the instrument's window (UI-098).
        assert_eq!(
            s.hit(
                &g,
                g.column.x + keys.x + keys.w / 2.0,
                s.row_y(&g, row) + keys.y + keys.h / 2.0
            ),
            Hit::Keys(id)
        );
    }

    #[test]
    fn browser_tabs_views_search_and_panels_keep_their_own_state() {
        let mut s = studio();
        click(&mut s, Hit::Browser(BrowserHit::ViewButton));
        choose(&mut s, Action::View(browser::View::Vendor));
        assert_eq!(s.browser.view(), browser::View::Vendor);
        click(&mut s, Hit::Rail(RailTab::Effects));
        assert_eq!(s.browser.view(), browser::View::Kind);
        click(&mut s, Hit::Browser(BrowserHit::Search));
        let g = geometry(&s);
        assert!(s.typed(&g, "r", Some("reverb")));
        assert!(
            s.browser
                .library()
                .iter()
                .all(|i| i.name.to_lowercase().contains("reverb"))
        );
        click(&mut s, Hit::Rail(RailTab::Files));
        assert!(!s.browser.typing);
        let before = geometry(&s);
        click(&mut s, Hit::ToggleArrange);
        click(&mut s, Hit::ToggleTempo);
        let after = geometry(&s);
        assert_eq!(after.section_band.h, 0.0);
        assert_eq!(after.tempo.h, 0.0);
        assert!(after.column.h > before.column.h);
        assert_eq!(s.song.steps(), 0);
    }

    #[test]
    fn the_column_head_has_the_mock_buttons_side_by_side_inside_it() {
        let s = studio();
        let g = geometry(&s);
        let rects: Vec<Rect> = Hit::HEAD.iter().map(|h| g.toggle(*h).unwrap()).collect();
        for (r, h) in rects.iter().zip(Hit::HEAD) {
            assert!(r.x >= g.head.x && r.right() <= g.head.right(), "{h:?}");
            assert_eq!(s.hit(&g, r.x + r.w / 2.0, r.y + r.h / 2.0), h);
        }
        // + at the left; the toggles, the zoom buttons and the switch at the right.
        for w in rects.windows(2) {
            assert!(w[0].right() <= w[1].x);
        }
        assert_eq!(rects[6].right(), g.head.right() - Space::X1.px());
        // The ‹ keeps a wider gap before the six at the right, in the narrowest head.
        let g = Geometry::with_head(1440.0, 900.0, g.panels, Layout::TRACK_HEAD_W);
        let fold = g.toggle(Hit::Fold).unwrap();
        let first = g.toggle(Hit::ToggleArrange).unwrap();
        assert!(g.toggle(Hit::AddTrack).unwrap().right() < fold.x);
        assert!(
            fold.right() + Space::X2.px() <= first.x,
            "{fold:?} {first:?}"
        );
    }

    #[test]
    fn the_follow_switch_is_the_last_head_button_and_turns_following_off_and_on() {
        let mut s = studio();
        let g = geometry(&s);
        assert!(s.follow.on, "the default follows");
        let zoom = g.toggle(Hit::ZoomIn).unwrap();
        let follow = g.toggle(Hit::Follow).unwrap();
        assert!(zoom.right() <= follow.x, "the switch sits beside the zoom");
        click(&mut s, Hit::Follow);
        assert!(!s.follow.on);
        assert_eq!(
            s.notice.as_ref().map(|n| n.text.as_str()),
            Some("自動スクロール: 切")
        );
        click(&mut s, Hit::Follow);
        assert!(s.follow.on);
    }

    #[test]
    fn the_plus_adds_a_track_of_the_chosen_kind_at_the_end_in_one_step() {
        let mut s = studio();
        for (kind, n) in [("audio", 1), ("instrument", 2), ("folder", 3), ("bus", 4)] {
            let tracks = s.project().tracks.len();
            let rows = s.tree.rows.len();
            click(&mut s, Hit::AddTrack);
            assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::AddTrack);
            choose(&mut s, Action::AddTrack(kind));
            assert!(s.popup.is_none());
            assert_eq!(s.song.steps(), n, "{kind}");
            assert_eq!(s.tree.rows.len(), rows + 1, "{kind}");
            let added = if kind == "folder" { 0 } else { 1 };
            assert_eq!(s.project().tracks.len(), tracks + added, "{kind}");
        }
        // ファイルから… asks the OS's file window for files (UI-017).
        click(&mut s, Hit::AddTrack);
        choose(&mut s, Action::FromFile);
        assert!(s.popup.is_none());
        assert_eq!(s.file_request, Some(FileRequest::Audio));
        assert_eq!(s.song.steps(), 4);
    }

    #[test]
    fn the_zoom_buttons_widen_and_narrow_the_beats_and_keep_the_left_edge() {
        let mut s = studio();
        s.scroll.0 = 64.0;
        let left = s.px_to_ticks(s.scroll.0);
        let w = s.beat_w;
        click(&mut s, Hit::ZoomIn);
        assert!(s.beat_w > w);
        assert!((s.px_to_ticks(s.scroll.0) - left).abs() <= 1, "{left}");
        click(&mut s, Hit::ZoomOut);
        click(&mut s, Hit::ZoomOut);
        assert!(s.beat_w < w);
        assert_eq!(s.song.steps(), 0);
    }

    #[test]
    fn popup_blocks_clicks_underneath_and_escape_dismisses_it() {
        let mut s = studio();
        let id = track_named(&s, "Drums");
        click(&mut s, Hit::Bar(BarButton::Grid));
        s.press(&geometry(&s), Hit::Mute(id));
        assert!(s.popup.is_none());
        assert!(!s.project().tracks[&id].mute);
        click(&mut s, Hit::Bar(BarButton::SongName));
        choose(&mut s, Action::ShowVersions);
        assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Versions);
        assert!(s.key(&geometry(&s), "escape", false, false));
        assert!(s.popup.is_none());
    }

    #[test]
    fn the_song_menus_song_settings_opens_the_form_of_the_command_with_the_values_now() {
        let mut s = studio();
        let g = geometry(&s);
        let now = s.project().settings;
        click(&mut s, Hit::Bar(BarButton::SongName));
        choose(&mut s, Action::SongSettings);
        assert!(s.popup.is_none(), "the menu closes");
        let form = s.palette.as_ref().unwrap().form.as_ref().unwrap();
        assert_eq!(form.command, "project.settings");
        assert_eq!(
            form.args(),
            serde_json::json!({
                "sample_rate": now.sample_rate.to_string(),
                "bit_depth": now.bit_depth.to_string(),
                "media_folder": s.project().media.folder.clone(),
                "copy_imports": s.project().media.copy_imports.to_string()
            })
        );
        // The value typed in is the song's from then on.
        s.palette
            .as_mut()
            .unwrap()
            .replace_text(Some(0..100), "44100");
        let apply = s
            .palette
            .as_ref()
            .unwrap()
            .layout(g.size)
            .rows
            .last()
            .unwrap()
            .1;
        s.palette_press(&g, apply.x + 10.0, apply.y + 10.0);
        assert!(s.palette.is_none(), "{:?}", s.error);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(s.project().settings.sample_rate, 44100);
        assert_eq!(s.project().settings.bit_depth, now.bit_depth);
    }

    #[test]
    fn save_versions_restore_and_open_as_song_round_trip_through_core() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("ui3-files-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let saved = root.join("song");
        let copy = root.join("copy");
        let mut s = studio();
        let id = track_named(&s, "Drums");
        click(&mut s, Hit::Bar(BarButton::SongName));
        choose(&mut s, Action::Save);
        assert_eq!(s.file_request.take(), Some(FileRequest::SaveAs));
        s.finish_file_request_sync(FileRequest::SaveAs, &saved);
        assert_eq!(s.error, None);
        assert!(s.song.is_saved());
        click(&mut s, Hit::Bar(BarButton::Saved));
        choose(&mut s, Action::NameVersion);
        s.typed(&geometry(&s), "a", Some("初稿"));
        s.typed(&geometry(&s), "enter", None);
        assert_eq!(s.error, None);
        assert!(s.song.versions().iter().any(|v| v.name == "初稿"));
        s.key(&geometry(&s), "escape", false, false);
        click(&mut s, Hit::Mute(id));
        s.key(&geometry(&s), "s", true, false);
        s.wait_for_save();
        assert!(s.song.is_saved());
        click(&mut s, Hit::Bar(BarButton::Saved));
        let i = s
            .popup
            .as_ref()
            .unwrap()
            .versions
            .iter()
            .position(|v| v.name == "初稿")
            .unwrap();
        choose(&mut s, Action::SelectVersion(i));
        assert!(
            s.popup
                .as_ref()
                .unwrap()
                .detail
                .as_ref()
                .unwrap()
                .changed
                .contains(&id)
        );
        choose(&mut s, Action::Restore);
        assert!(!s.project().tracks[&id].mute);
        s.undo(&geometry(&s));
        assert!(s.project().tracks[&id].mute);
        click(&mut s, Hit::Bar(BarButton::Saved));
        choose(&mut s, Action::SelectVersion(i));
        choose(&mut s, Action::OpenAsSong);
        let request = s.file_request.take().unwrap();
        s.finish_file_request_sync(request, &copy);
        assert_eq!(s.error, None);
        assert!(!s.project().tracks[&id].mute);
        s.finish_file_request_sync(FileRequest::Open, &saved);
        assert_eq!(s.error, None);
        assert!(s.project().tracks[&id].mute);
        drop(s);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Studio Pro's Record Panel, Import File and Save New Version keys (KEY-012) do what the
    /// record button, 「ファイルから…」 and the saved versions' 「名前を付けて保存」 do.
    #[test]
    fn key_012_the_record_panel_import_and_new_version_keys_work_as_their_buttons() {
        use crate::tools::Mods;
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("key-012-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut s = studio();
        let g = geometry(&s);
        let all = Mods {
            ctrl: true,
            shift: true,
            alt: true,
        };

        assert!(!s.rec_panel.open);
        assert!(s.mapped_key(&g, "r", None, Mods::alt_shift()));
        assert!(s.rec_panel.open);
        assert!(s.mapped_key(&g, "r", None, Mods::alt_shift()));
        assert!(!s.rec_panel.open);

        assert!(s.mapped_key(&g, "o", None, Mods::ctrl_shift()));
        assert_eq!(s.file_request.take(), Some(FileRequest::Audio));
        assert_eq!(s.song.steps(), 0);

        // A song not saved yet has no versions.
        assert!(!s.mapped_key(&g, "s", None, all));
        assert!(s.popup.is_none());
        s.finish_file_request_sync(FileRequest::SaveAs, &root.join("song"));
        assert_eq!(s.error, None);
        let g = geometry(&s);
        assert!(s.mapped_key(&g, "s", None, all));
        let p = s.popup.as_ref().unwrap();
        assert_eq!(p.kind, PopupKind::Versions);
        assert_eq!(p.naming.as_deref(), Some(""));
        s.typed(&g, "a", Some("二稿"));
        s.typed(&g, "enter", None);
        assert_eq!(s.error, None);
        assert!(s.song.versions().iter().any(|v| v.name == "二稿"));
        drop(s);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_bar_in_view_is_listed_once_in_order() {
        let s = studio();
        let g = geometry(&s);
        let bars = s.visible_bars(&g);
        assert_eq!(bars.first().map(|b| b.0), Some(1));
        assert!(
            bars.windows(2)
                .all(|w| w[0].0 + 1 == w[1].0 && w[0].1 < w[1].1)
        );
    }
}
