//! The top bar and the bottom bar as geometry and a picture.
//!
//! The top bar is also the window's title bar: its empty stretches drag the
//! window and the Windows caption buttons sit at its right end. The bottom bar,
//! under the drawer, has the transport and the CPU (UI-300). Everything between
//! is painted with a [`Pen`] from the rectangles of [`BarLayout`], so where a
//! button sits and what a press on it means is tested without a window.
//!
//! Coordinates are local to each bar: y runs 0..`Layout::TOP_BAR_H` in the top
//! bar and 0..`Layout::BOTTOM_BAR_H` in the bottom bar
//! ([`BarButton::in_bottom`]).

use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::assets::LOGO_ASPECT;
use crate::browser::text_width;
use crate::draw::{Align, Pen};
use crate::studio::Rect;
use crate::tools::{SnapMode, Tool};

/// A button of the bar that opens something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarButton {
    /// The logo: opens 「Xirius について」 (LIC-015). The narrow bar has none.
    Logo,
    /// The song's name: opens the song menu.
    SongName,
    /// The saved indicator: opens the version list.
    Saved,
    /// The grid value: opens the grid pop-over.
    Grid,
    /// The snap switch left of the grid value (UI-302): the tilted magnet, cut by a slash
    /// while off.
    Snap,
    /// Undo and redo in one: opens the history.
    History,
    /// The command palette: every operation by its name (UI-087).
    Palette,
    /// The AI's stars: opens the AI's permission and how to reach it (AI-003).
    Ai,
    Export,
    /// A tool button: picks the tool for the arrangement and the roll.
    Tool(Tool),
    /// The ▾ right of the arrow: lists the tools the arrow can turn into, and in the roll the
    /// arrow's mode (UI-128, UI-286).
    AltTool,
    /// The pencil's shape, right of the pencil: lists the shapes (UI-128).
    Shape,
    /// The length the pencil draws with, right of the grid while it is the note before's: a
    /// press goes back to the grid's value (EDT-131).
    DrawLength,
    /// The transport (UI-009): back to the start, play and pause, stop,
    /// record, loop and the metronome (REC-003).
    ToStart,
    Play,
    Stop,
    Record,
    Loop,
    /// The pre-roll switch, beside the metronome (REC-007): the same as the panel's.
    PreRoll,
    Metronome,
    /// The tempo readout: dragged, scrolled, stepped with the arrows or typed
    /// (UI-119).
    Tempo,
    /// The meter readout: opens the list of meters and the two fields.
    Meter,
    /// At the right of the bar: shows and hides the record panel (REC-007).
    RecPanel,
    /// The CPU of the song: opens the list of plugins, the heaviest first (ENG-008).
    Cpu,
    /// The ripple switch, between the tools and the grid as in Studio One (EDT-135): the
    /// arrangement only, lit while clip edits close up and push what follows.
    Ripple,
}

impl BarButton {
    /// Whether the button is in the bottom bar (UI-300): the transport, the position, the
    /// tempo, the meter and the CPU.
    pub fn in_bottom(self) -> bool {
        matches!(
            self,
            BarButton::ToStart
                | BarButton::Play
                | BarButton::Stop
                | BarButton::Record
                | BarButton::Loop
                | BarButton::PreRoll
                | BarButton::Metronome
                | BarButton::Tempo
                | BarButton::Meter
                | BarButton::Cpu
        )
    }
}

/// The icon of a tool's button.
pub fn tool_icon(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "select",
        Tool::Range => "range",
        Tool::Cut => "cut",
        Tool::Eraser => "eraser",
        Tool::Draw => "pencil",
        Tool::Mute => "mute",
        Tool::Audition => "listen",
    }
}

/// The ▾ beside the arrow, and the shape button beside the pencil.
const ALT_W: f32 = 16.0;
const SHAPE_W: f32 = 36.0;

/// The icon of a pencil shape.
pub fn shape_icon(shape: xirius_core::shape::Shape) -> &'static str {
    use xirius_core::shape::Shape;
    match shape {
        Shape::Free => "shape-free",
        Shape::Line => "shape-line",
        Shape::Parabola => "shape-parabola",
        Shape::Square => "shape-square",
        Shape::Triangle => "shape-triangle",
        Shape::Saw => "shape-saw",
        Shape::Sine => "shape-sine",
        Shape::Transform => "shape-transform",
    }
}

pub const NAME_MAX_W: f32 = 150.0;
pub const SAVED_W: f32 = 92.0;
/// The grid value with its ▾ (UI-302): wide enough for every value but the triplets, which
/// widen it.
const GRID_W: f32 = 72.0;
/// Inside the grid value: the room left of the text, and the ▾ with the room around it.
const GRID_PAD_L: f32 = 10.0;
const GRID_PAD_R: f32 = 6.0;
const CHEVRON: f32 = 14.0;
const GRID_TEXT_GAP: f32 = 6.0;
/// Between the snap switch and the grid value, as in the mock.
const SNAP_GAP: f32 = 2.0;
/// The length chip right of the grid: a pencil and "1/2".
const LENGTH_W: f32 = 64.0;
/// The CPU meter: "CPU" and "100%" in the caption size over a thin well (UI-301).
const CPU_W: f32 = 76.0;
/// Inside the CPU meter (UI-301): the row of "CPU" and the number, then the well.
const CPU_PAD_X: f32 = 6.0;
const CPU_TEXT_H: f32 = 11.0;
const CPU_GAP: f32 = 4.0;
const CPU_WELL_H: f32 = 4.0;

/// How much of the well the load fills; past 100% it stays full.
fn cpu_fill_w(percent: u32, well_w: f32) -> f32 {
    (well_w * percent.min(100) as f32 / 100.0).round()
}
/// The song name as short as it gets when the editing group needs the room.
const NAME_MIN_W: f32 = 64.0;
/// The position (bar.beat.tick) and the time beside it, in one strip with
/// the tempo and the meter (as in the mock).
/// How long the record button shakes when pressed with nothing armed.
pub const SHAKE: std::time::Duration = std::time::Duration::from_millis(320);
/// How far the shake swings at first, in px.
const SHAKE_PX: f32 = 3.0;
/// How long the record button stays lit or dark while the song plays up to the punch range.
const BLINK: std::time::Duration = std::time::Duration::from_millis(300);

/// The sideways offset of the record button `since` the press with nothing armed: three swings
/// that die away. None when the motion is reduced or the shake is over.
pub fn shake_offset(since: Option<std::time::Duration>, reduced: bool) -> f32 {
    let Some(t) = since.filter(|t| *t < SHAKE && !reduced) else {
        return 0.0;
    };
    let k = t.as_secs_f32() / SHAKE.as_secs_f32();
    (k * 3.0 * std::f32::consts::TAU).sin() * SHAKE_PX * (1.0 - k)
}

/// The logo's dot when a newer version is out.
const UPDATE_DOT: f32 = 7.0;

const POS_W: f32 = 84.0;
const SECS_W: f32 = 60.0;
const BPM_W: f32 = 52.0;
const METER_W: f32 = 40.0;

/// Width of the Windows caption buttons.
pub const fn caption_w() -> f32 {
    if cfg!(windows) {
        3.0 * Layout::CAPTION_BUTTON_W
    } else {
        0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BarLayout {
    pub width: f32,
    pub logo: Rect,
    pub name: Rect,
    pub saved: Rect,
    /// The transport, the position, the tempo and the meter, in the bottom bar's coordinates.
    pub skip: Rect,
    pub play: Rect,
    pub stop: Rect,
    pub record: Rect,
    pub looped: Rect,
    pub preroll: Rect,
    pub metronome: Rect,
    pub time: Rect,
    pub bpm: Rect,
    pub meter: Rect,
    pub tools: Vec<(Tool, Rect)>,
    /// The ▾ after the arrow and the pencil's shape. Both are empty when the bar leaves them out.
    pub alt: Rect,
    pub shape: Rect,
    pub grid: Rect,
    /// The snap switch, left of the grid value (UI-302).
    pub snap: Rect,
    /// The ripple switch, left of the snap switch: empty in the roll's bar.
    pub ripple: Rect,
    /// The pencil's length, right of the grid: empty unless `of_length` made room for it.
    pub length: Rect,
    pub rec_panel: Rect,
    /// At the left of the bottom bar, in its coordinates.
    pub cpu: Rect,
    pub history: Rect,
    pub palette: Rect,
    pub ai: Rect,
    pub export: Rect,
    /// The stretches `(x0, x1)` of the top bar that drag the window.
    pub drag: Vec<(f32, f32)>,
}

impl BarLayout {
    /// The bar of the arrangement (it has the ripple switch).
    pub fn new(width: f32, song_name: &str) -> BarLayout {
        BarLayout::of(width, song_name, true)
    }

    /// `arrange`: whether the arrangement's ripple switch stands beside the grid (the roll's
    /// edits do not ripple, so it has none). Both areas have the ▾ after the arrow (UI-286).
    pub fn of(width: f32, song_name: &str, arrange: bool) -> BarLayout {
        BarLayout::of_length(width, song_name, arrange, false)
    }

    /// `of`, with the pencil's length chip right of the grid when `length` (the song's name
    /// makes way for it).
    pub fn of_length(width: f32, song_name: &str, arrange: bool, length: bool) -> BarLayout {
        Self::of_save(width, song_name, arrange, length, false, "1/16")
    }

    /// `grid_text`: what the grid value says, which sets its width (`grid_w`).
    pub fn of_save(
        width: f32,
        song_name: &str,
        arrange: bool,
        length: bool,
        failed: bool,
        grid_text: &str,
    ) -> BarLayout {
        let h = Layout::TOP_BAR_H;
        let b = Layout::ICON_BUTTON;
        let pad = if cfg!(target_os = "macos") {
            Layout::TRAFFIC_LIGHT_ROOM
        } else {
            Space::X3.px()
        };
        let y = (h - b) / 2.0;
        let gap = Space::X1.px();
        // The top bar, from the left: the logo, the song's name and the saved state.
        let logo = Rect::new(
            pad,
            (h - Layout::LOGO_H) / 2.0,
            Layout::LOGO_H * LOGO_ASPECT,
            Layout::LOGO_H,
        );
        let name_w =
            (text_width(song_name, FontSize::Body) + Space::X3.px() + Layout::ICON_SIZE + 12.0)
                .min(NAME_MAX_W);
        let mut name = Rect::new(logo.right() + Space::X4.px(), y, name_w, b);
        let mut saved = Rect::new(
            name.right() + Space::X1.px(),
            y,
            if failed {
                SAVED_W + Space::X4.px() * 2.0
            } else {
                SAVED_W
            },
            b,
        );

        // From the right: export, the AI, the palette, history and the record panel close
        // together, as in the mock.
        let right_pad = if cfg!(windows) { Space::X2.px() } else { pad };
        let export = Rect::new(width - caption_w() - right_pad - b, y, b, b);
        let ai = Rect::new(export.x - gap - b, y, b, b);
        let palette = Rect::new(ai.x - gap - b, y, b, b);
        let history = Rect::new(palette.x - gap - b, y, b, b);
        let rec_panel = Rect::new(history.x - gap - b, y, b, b);

        // Left of them the editing group: the tools with the ▾ and the shape, the ripple
        // switch (the arrangement's: the roll's edits do not ripple), the grid and the
        // pencil's length.
        let mut x = rec_panel.x - Space::X2.px();
        let length = if length {
            x -= LENGTH_W;
            let r = Rect::new(x, y, LENGTH_W, b);
            x -= gap;
            r
        } else {
            Rect::default()
        };
        let grid_w = grid_w(grid_text);
        x -= grid_w;
        let grid = Rect::new(x, y, grid_w, b);
        x -= SNAP_GAP + b;
        let snap = Rect::new(x, y, b, b);
        let ripple = if arrange {
            x -= gap + b;
            Rect::new(x, y, b, b)
        } else {
            Rect::default()
        };
        let tools_w = Tool::ALL.len() as f32 * 24.0 + ALT_W + SHAPE_W;
        let mut tx = x - Space::X2.px() - tools_w;
        // A long name gives way to the group, and then the saved state's words (its icon
        // stays).
        let mut short_by = saved.right() + Space::X2.px() - tx;
        if short_by > 0.0 {
            let cut = short_by.min((name.w - NAME_MIN_W).max(0.0));
            name.w -= cut;
            saved.x -= cut;
            short_by -= cut;
            if short_by > 0.0 && saved.w > b {
                saved.w = b;
            }
        }
        let mut tools = Vec::new();
        let mut alt_rect = Rect::default();
        let mut shape_rect = Rect::default();
        for tool in Tool::ALL {
            tools.push((tool, Rect::new(tx, y, 24.0, b)));
            tx += 24.0;
            if tool == Tool::Select {
                alt_rect = Rect::new(tx, y, ALT_W, b);
                tx += ALT_W;
            }
            if tool == Tool::Draw {
                shape_rect = Rect::new(tx, y, SHAPE_W, b);
                tx += SHAPE_W;
            }
        }

        // The bottom bar: the CPU at the left, the transport and the position centred on the
        // window.
        let by = (Layout::BOTTOM_BAR_H - b) / 2.0;
        let cpu = Rect::new(Space::X2.px(), by, CPU_W, b);
        let play_w = b + Space::X1.px();
        let widths = [
            b,
            play_w,
            b,
            b,
            b,
            b,
            b,
            Space::X2.px() + POS_W + Space::X1.px() + SECS_W,
            BPM_W,
            METER_W,
        ];
        let total: f32 = widths.iter().sum::<f32>() + gap * (widths.len() - 1) as f32;
        let mut x = ((width - total) / 2.0).max(cpu.right() + Space::X2.px());
        let mut place = |w: f32| {
            let r = if w == play_w {
                Rect::new(x, by - 2.0, w, w)
            } else {
                Rect::new(x, by, w, b)
            };
            x += w + gap;
            r
        };
        let skip = place(b);
        let play = place(play_w);
        let stop = place(b);
        let record = place(b);
        let looped = place(b);
        let preroll = place(b);
        let metronome = place(b);
        let time = place(widths[7]);
        let bpm = place(widths[8]);
        let meter = place(widths[9]);

        let mut layout = BarLayout {
            width,
            logo,
            name,
            saved,
            skip,
            play,
            stop,
            record,
            looped,
            preroll,
            metronome,
            time,
            bpm,
            meter,
            tools,
            alt: alt_rect,
            shape: shape_rect,
            grid,
            snap,
            ripple,
            length,
            rec_panel,
            cpu,
            history,
            palette,
            ai,
            export,
            drag: Vec::new(),
        };
        layout.drag = layout.drag_spans();
        layout
    }

    /// Where the top bar's buttons end up in one line: used to find the gaps.
    fn blocks(&self) -> Vec<(f32, f32)> {
        let logo = self.rect(BarButton::Logo);
        let tools = self.tools.first().map_or(self.ripple.x, |(_, r)| r.x);
        vec![
            (logo.x, logo.right()),
            (self.name.x, self.saved.right()),
            (tools, self.export.right()),
        ]
    }

    fn drag_spans(&self) -> Vec<(f32, f32)> {
        let end = self.width - caption_w();
        let mut spans = Vec::new();
        let mut at = 0.0;
        for (x0, x1) in self.blocks() {
            if x0 > at {
                spans.push((at, x0));
            }
            at = at.max(x1);
        }
        if end > at {
            spans.push((at, end));
        }
        spans
    }

    /// The dot at the logo's top right that says a newer version is out (UI-262).
    pub fn update_dot(&self) -> Option<Rect> {
        (self.logo.w > 0.0).then(|| {
            Rect::new(
                self.logo.right() - 1.0,
                self.logo.y - 4.0,
                UPDATE_DOT,
                UPDATE_DOT,
            )
        })
    }

    pub fn rect(&self, button: BarButton) -> Rect {
        match button {
            // As tall as the other buttons, a little wider than the mark.
            BarButton::Logo if self.logo.w > 0.0 => {
                let b = Layout::ICON_BUTTON;
                let x = self.logo.x - Space::X1.px();
                Rect::new(
                    x,
                    (Layout::TOP_BAR_H - b) / 2.0,
                    self.logo.w + Space::X1.px() * 2.0,
                    b,
                )
            }
            BarButton::Logo => Rect::default(),
            BarButton::SongName => self.name,
            BarButton::Saved => self.saved,
            BarButton::Grid => self.grid,
            BarButton::Snap => self.snap,
            BarButton::DrawLength => self.length,
            BarButton::History => self.history,
            BarButton::Palette => self.palette,
            BarButton::Ai => self.ai,
            BarButton::Export => self.export,
            BarButton::ToStart => self.skip,
            BarButton::Play => self.play,
            BarButton::Stop => self.stop,
            BarButton::Record => self.record,
            BarButton::Loop => self.looped,
            BarButton::PreRoll => self.preroll,
            BarButton::Metronome => self.metronome,
            BarButton::Tempo => self.bpm,
            BarButton::Meter => self.meter,
            BarButton::AltTool => self.alt,
            BarButton::Shape => self.shape,
            BarButton::RecPanel => self.rec_panel,
            BarButton::Cpu => self.cpu,
            BarButton::Ripple => self.ripple,
            BarButton::Tool(t) => self
                .tools
                .iter()
                .find(|(tool, _)| *tool == t)
                .map_or(Rect::default(), |(_, r)| *r),
        }
    }

    pub const BUTTONS: [BarButton; 24] = [
        BarButton::Logo,
        BarButton::SongName,
        BarButton::Saved,
        BarButton::ToStart,
        BarButton::Play,
        BarButton::Stop,
        BarButton::Record,
        BarButton::Loop,
        BarButton::PreRoll,
        BarButton::Metronome,
        BarButton::Tempo,
        BarButton::Meter,
        BarButton::AltTool,
        BarButton::Shape,
        BarButton::Snap,
        BarButton::Grid,
        BarButton::DrawLength,
        BarButton::RecPanel,
        BarButton::Cpu,
        BarButton::History,
        BarButton::Palette,
        BarButton::Ai,
        BarButton::Export,
        BarButton::Ripple,
    ];

    /// The top bar's button at a point of the top bar.
    pub fn hit(&self, x: f32, y: f32) -> Option<BarButton> {
        Self::BUTTONS
            .into_iter()
            .filter(|b| !b.in_bottom())
            .chain(self.tools.iter().map(|(t, _)| BarButton::Tool(*t)))
            .find(|b| self.rect(*b).contains(x, y))
    }

    /// The bottom bar's button at a point of the bottom bar.
    pub fn hit_bottom(&self, x: f32, y: f32) -> Option<BarButton> {
        Self::BUTTONS
            .into_iter()
            .filter(|b| b.in_bottom())
            .find(|b| self.rect(*b).contains(x, y))
    }
}

/// What the tempo readout is in the middle of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TempoMode {
    #[default]
    Idle,
    /// It has the arrow keys.
    Focus,
    /// The pointer is down on it and moves the value.
    Drag,
    /// The value is typed; `fresh` while the old value is still selected.
    Edit { fresh: bool },
}

/// What the bar shows.
#[derive(Clone, Debug, PartialEq)]
pub struct BarView {
    pub song_name: String,
    /// Whether the song's record is up to date (PRJ-010).
    pub save: crate::keep::SaveMark,
    pub save_turn: f32,
    /// The session's grid value (`1/16`, `1/8t`).
    pub grid: String,
    /// What the grid button says, which depends on the snap mode.
    pub grid_text: String,
    /// The length the pencil draws with (a note value or beats), shown right of the grid while
    /// the roll takes it from the note before (EDT-131).
    pub draw_length: Option<String>,
    pub snap: bool,
    /// The picked tool, lit in the bar.
    pub tool: Tool,
    /// The tool the arrow turns into while Ctrl is held, underlined in the bar.
    pub alt: Tool,
    /// Whether the roll's arrow is in its extended mode (UI-286): the arrow icon is filled.
    pub arrow_extended: bool,
    /// The shape the pencil draws (UI-128).
    pub shape: xirius_core::shape::Shape,
    /// The tool in effect while modifiers are held, lit in place of the picked one.
    pub held: Option<Tool>,
    /// The position as bar.beat.tick.
    pub time: String,
    /// The position as time, beside it (`m:ss.sss`).
    pub seconds: String,
    pub bpm: String,
    pub meter: String,
    /// What is being done to the tempo readout (UI-119).
    pub tempo: TempoMode,
    pub playing: bool,
    /// A recording runs: the record button is lit, the count-in included.
    pub recording: bool,
    /// The beats of the count-in still to click (REC-007). The button counts them; the position
    /// stays white until the recording itself starts.
    pub pre_count: Option<u32>,
    /// How far the record button is shaken sideways, in px (pressed with nothing armed).
    pub record_shake: f32,
    /// The song plays up to the punch range: the record button blinks.
    pub pre_roll: bool,
    /// The pre-roll switch is on (REC-007): its button beside the metronome is lit.
    pub pre_roll_on: bool,
    /// The record panel is shown: its button at the right of the bar is lit.
    pub rec_panel: bool,
    /// The song's CPU in percent, and whether blocks were missed (ENG-008).
    pub cpu: u32,
    pub looping: bool,
    pub metronome: bool,
    /// The setting `edit.ripple` is on (EDT-135): the ripple switch is lit.
    pub ripple: bool,
    /// The button whose pop-over is open.
    pub open: Option<BarButton>,
    pub hover: Option<BarButton>,
    pub can_export: bool,
    pub export_stale: bool,
    /// A newer version is out: the logo has a dot (UI-262).
    pub update: bool,
    pub motion: std::rc::Rc<crate::view_motion::ViewMotion>,
    pub motion_setting: xirius_design::Motion,
    /// The AI's button: its permission and its stars (AI-003, AI-036).
    pub ai: crate::ai::BarAi,
}

impl BarView {
    /// A take is being recorded (not just counted in): the position turns red with the record
    /// button's face.
    pub fn recording_now(&self) -> bool {
        self.recording && self.pre_count.is_none()
    }
}

/// The position as the bar writes it: `1.1.000`, the tick always in three digits.
pub fn bar_beat_tick(map: &xirius_core::time::TimeMap, tick: xirius_core::time::Tick) -> String {
    let (bar, beat, t) = map.to_bbt(tick);
    format!("{bar}.{beat}.{t:03}")
}

/// The grid value as the bar writes it: `1/16`, `1/8` + 3連.
pub fn grid_label(grid: &str) -> String {
    match grid.strip_suffix('t') {
        Some(base) => format!("{base}・3連"),
        None => grid.to_string(),
    }
}

/// What the grid button says: the value in the list for クオンタイズ, 小節,
/// `fr` for frames, and for 自動 the note value it found (`step` ticks).
pub fn grid_text(mode: SnapMode, grid: &str, step: xirius_core::time::Tick, ppq: u32) -> String {
    match mode {
        SnapMode::Quantize => grid_label(grid),
        SnapMode::Bar => "小節".to_string(),
        SnapMode::Frames => "fr".to_string(),
        SnapMode::Auto => format!("1/{}", (i64::from(ppq) * 4 / step.max(1)).max(1)),
    }
}

/// The grid value's width for what it says: the text, then the ▾.
fn grid_w(text: &str) -> f32 {
    (GRID_PAD_L + text_width(text, FontSize::Small) + GRID_TEXT_GAP + CHEVRON + GRID_PAD_R)
        .ceil()
        .max(GRID_W)
}

/// The face under a bar button: lit while its pop-over is open, lighter under the pointer.
fn face_of(view: &BarView, c: &crate::paint::Colors, button: BarButton) -> Option<gpui::Hsla> {
    if view.open == Some(button) {
        Some(c.sel)
    } else if view.hover == Some(button) {
        Some(c.hover)
    } else {
        None
    }
}

/// The icon's square in the middle of a button.
fn icon_at(r: Rect) -> Rect {
    Rect::new(
        r.x + (r.w - Layout::ICON_SIZE) / 2.0,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    )
}

/// The top bar.
pub fn paint(pen: &mut Pen, layout: &BarLayout, view: &BarView) {
    let c = pen.c;
    let face = |button: BarButton| face_of(view, c, button);

    // The song name and the saved indicator.
    if let Some(f) = face(BarButton::SongName) {
        pen.round(layout.name, f, Radius::S);
    }
    let n = layout.name;
    let pad = Space::X2.px();
    let name_r = Rect::new(
        n.x + pad,
        n.y,
        n.w - pad - Layout::ICON_SIZE - Space::X1.px(),
        n.h,
    );
    pen.mask(name_r, |pen| {
        pen.text(
            &view.song_name,
            FontSize::Body,
            Weight::Medium,
            pen.c.ink_strong,
            name_r,
            Align::Left,
        );
    });
    pen.icon(
        "chevron-down",
        Rect::new(
            n.right() - Layout::ICON_SIZE - Space::X1.px(),
            n.y + (n.h - Layout::ICON_SIZE) / 2.0,
            Layout::ICON_SIZE,
            Layout::ICON_SIZE,
        ),
        c.ink,
    );
    let s = layout.saved;
    if let Some(f) = face(BarButton::Saved) {
        pen.round(s, f, Radius::S);
    }
    let (icon, label, ink) = match view.save {
        crate::keep::SaveMark::Saved => ("saved", "保存済み", c.ok),
        crate::keep::SaveMark::Saving => ("sync", "保存中", c.ink),
        crate::keep::SaveMark::Unsaved => ("dot", "未保存", c.ink),
        crate::keep::SaveMark::Failed => ("alert", "保存できません", c.alert),
    };
    pen.icon_turned(
        icon,
        Rect::new(
            s.x + pad,
            s.y + (s.h - Layout::ICON_SIZE) / 2.0,
            Layout::ICON_SIZE,
            Layout::ICON_SIZE,
        ),
        ink,
        if view.save == crate::keep::SaveMark::Saving {
            view.save_turn
        } else {
            0.0
        },
    );
    if s.w > Layout::ICON_BUTTON {
        pen.text(
            label,
            FontSize::Caption,
            Weight::Medium,
            ink,
            Rect::new(
                s.x + pad + Layout::ICON_SIZE + Space::X1.px(),
                s.y,
                s.w,
                s.h,
            ),
            Align::Left,
        );
    }

    // The ripple switch, lit while it is on.
    if layout.ripple.w > 0.0 {
        let r = layout.ripple;
        let f = pen.transition(
            &view.motion,
            view.motion_setting,
            r,
            "transport-Ripple",
            view.ripple,
            face(BarButton::Ripple).unwrap_or(c.surface_1),
            c.fill,
        );
        pen.round(r, f, Radius::S);
        pen.icon(
            "ripple",
            icon_at(r),
            if view.ripple {
                c.on_fill
            } else {
                c.ink_on_toggle
            },
        );
    }
    let lit = view.held.unwrap_or(view.tool);
    for (tool, r) in &layout.tools {
        let picked = *tool == lit;
        let off = if view.hover == Some(BarButton::Tool(*tool)) {
            c.hover
        } else {
            c.surface_1
        };
        let face = pen.transition(
            &view.motion,
            view.motion_setting,
            *r,
            &format!("tool-{tool:?}"),
            picked,
            off,
            c.sel,
        );
        pen.round(*r, face, Radius::S);
        let icon = if *tool == Tool::Select && view.arrow_extended {
            "select-extended"
        } else {
            tool_icon(*tool)
        };
        pen.icon(
            icon,
            icon_at(*r),
            if picked {
                c.ink_strong
            } else {
                c.ink_on_toggle
            },
        );
        // The alternative tool of the arrow: underlined, thicker while it is the one in effect.
        if *tool == view.alt && *tool != Tool::Select {
            let thick = if view.held == Some(*tool) { 3.0 } else { 2.0 };
            pen.round(
                Rect::new(r.x + 5.0, r.bottom() - 3.0 - thick + 1.0, r.w - 10.0, thick),
                c.fill,
                Radius::Pill,
            );
        }
    }

    // The ▾ of the arrow's alternative tool, and the pencil's shape with its ▾.
    if layout.alt.w > 0.0 {
        let r = layout.alt;
        if let Some(f) = face(BarButton::AltTool) {
            pen.round(r, f, Radius::S);
        }
        pen.icon("chevron-down", icon_at(r), c.ink_on_toggle);
    }
    if layout.shape.w > 0.0 {
        let r = layout.shape;
        if let Some(f) = face(BarButton::Shape) {
            pen.round(r, f, Radius::S);
        }
        let ink = if lit == Tool::Draw {
            c.ink_strong
        } else {
            c.ink_on_toggle
        };
        let mark = Layout::ICON_SIZE;
        pen.icon(
            shape_icon(view.shape),
            Rect::new(r.x + Space::X1.px(), r.y + (r.h - mark) / 2.0, mark, mark),
            ink,
        );
        pen.icon(
            "chevron-down",
            Rect::new(
                r.right() - mark + 2.0,
                r.y + (r.h - mark) / 2.0 + 2.0,
                mark - 4.0,
                mark - 4.0,
            ),
            c.ink_on_toggle,
        );
    }

    // The snap switch (UI-302): the selection's face while on; the magnet is cut while off.
    let r = layout.snap;
    let f = pen.transition(
        &view.motion,
        view.motion_setting,
        r,
        "transport-Snap",
        view.snap,
        face(BarButton::Snap).unwrap_or(c.surface_1),
        c.sel,
    );
    pen.round(r, f, Radius::S);
    pen.icon(
        if view.snap { "magnet" } else { "magnet-off" },
        icon_at(r),
        if view.snap {
            c.ink_strong
        } else {
            c.ink_on_toggle
        },
    );

    // The grid value, then the ▾ of its menu.
    let g = layout.grid;
    let g_face = face(BarButton::Grid).unwrap_or(c.surface_2);
    pen.round(g, g_face, Radius::S);
    let text_r = Rect::new(
        g.x + GRID_PAD_L,
        g.y,
        g.w - GRID_PAD_L - GRID_TEXT_GAP - CHEVRON - GRID_PAD_R,
        g.h,
    );
    pen.mask(text_r, |pen| {
        pen.digits(
            &view.grid_text,
            FontSize::Small,
            Weight::Semibold,
            pen.c.ink_strong,
            text_r,
            Align::Left,
        );
    });
    pen.icon(
        "chevron-down",
        Rect::new(
            g.right() - GRID_PAD_R - CHEVRON,
            g.y + (g.h - CHEVRON) / 2.0,
            CHEVRON,
            CHEVRON,
        ),
        c.ink,
    );

    // The pencil's length, where it differs from the grid's value.
    if let Some(text) = &view.draw_length
        && layout.length.w > 0.0
    {
        let r = layout.length;
        pen.round(r, face(BarButton::DrawLength).unwrap_or(c.sel), Radius::S);
        pen.icon(
            "pencil",
            Rect::new(
                r.x + pad,
                r.y + (r.h - Layout::ICON_SIZE) / 2.0,
                Layout::ICON_SIZE,
                Layout::ICON_SIZE,
            ),
            c.ink_strong,
        );
        pen.digits(
            text,
            FontSize::Caption,
            Weight::Medium,
            c.ink_strong,
            Rect::new(
                r.x + pad + Layout::ICON_SIZE + Space::X1.px(),
                r.y,
                r.w,
                r.h,
            ),
            Align::Left,
        );
    }

    // The record panel's button, then history, the palette and export: icon only.
    let panel_face = pen.transition(
        &view.motion,
        view.motion_setting,
        layout.rec_panel,
        "transport-RecPanel",
        view.rec_panel,
        face(BarButton::RecPanel).unwrap_or(c.surface_1),
        c.fill,
    );
    pen.round(layout.rec_panel, panel_face, Radius::S);
    pen.icon(
        "sliders",
        icon_at(layout.rec_panel),
        if view.rec_panel {
            c.on_fill
        } else {
            c.ink_strong
        },
    );
    let h_face = face(BarButton::History);
    if let Some(f) = h_face {
        pen.round(layout.history, f, Radius::S);
    }
    pen.icon("history", icon_at(layout.history), c.ink_strong);
    if let Some(f) = face(BarButton::Palette) {
        pen.round(layout.palette, f, Radius::S);
    }
    pen.icon("prompt", icon_at(layout.palette), c.ink_strong);
    if let Some(f) = face(BarButton::Ai) {
        pen.round(layout.ai, f, Radius::S);
    }
    crate::ai::paint_bar_button(pen, icon_at(layout.ai), &view.ai);
    let e_face = if view.can_export {
        face(BarButton::Export).unwrap_or(c.surface_2)
    } else {
        c.surface_0
    };
    pen.round(layout.export, e_face, Radius::S);
    if view.export_stale {
        pen.round(
            Rect::new(layout.export.right() - 6.0, layout.export.y + 2.0, 5.0, 5.0),
            c.stale,
            Radius::Pill,
        );
    }
    pen.icon(
        "export",
        icon_at(layout.export),
        if view.can_export {
            c.ink_strong
        } else {
            c.ink_on_toggle
        },
    );
}

/// The bottom bar (UI-300): the CPU at the left, then the transport and one strip of the
/// position, the tempo and the meter.
pub fn paint_bottom(pen: &mut Pen, layout: &BarLayout, view: &BarView) {
    let c = pen.c;
    let face = |button: BarButton| face_of(view, c, button);
    {
        // A meter (UI-301): "CPU" and the number over a thin well filled to the load. However
        // heavy, it keeps one colour and one shape; the list it opens says what is heavy.
        let r = layout.cpu;
        let cpu_face = face(BarButton::Cpu).unwrap_or(c.surface_1);
        pen.round(r, cpu_face, Radius::S);
        let inner = Rect::new(
            r.x + CPU_PAD_X,
            r.y + (r.h - CPU_TEXT_H - CPU_GAP - CPU_WELL_H) / 2.0,
            r.w - 2.0 * CPU_PAD_X,
            CPU_TEXT_H,
        );
        pen.text(
            "CPU",
            FontSize::Caption,
            Weight::Medium,
            c.ink,
            inner,
            Align::Left,
        );
        pen.digits(
            &format!("{}%", view.cpu),
            FontSize::Caption,
            Weight::Medium,
            c.ink_strong,
            inner,
            Align::Right,
        );
        let well = Rect::new(inner.x, inner.bottom() + CPU_GAP, inner.w, CPU_WELL_H);
        pen.round(well, c.well, Radius::Pill);
        let w = cpu_fill_w(view.cpu, well.w);
        if w > 0.0 {
            pen.round(Rect::new(well.x, well.y, w, well.h), c.fill, Radius::Pill);
        }
    }

    // The transport: what is on is lit in its own colour.
    let lit = |on: bool, color| if on { color } else { c.ink_on_toggle };
    for (b, name, ink) in [
        (BarButton::ToStart, "skip-back", c.ink_on_toggle),
        (BarButton::Stop, "stop", c.ink_on_toggle),
        (BarButton::Record, "record", lit(view.recording, c.alert)),
        (BarButton::Loop, "loop", lit(view.looping, c.ink_blue)),
        (
            BarButton::PreRoll,
            "preroll",
            lit(view.pre_roll_on, c.ink_blue),
        ),
        (
            BarButton::Metronome,
            "metronome",
            lit(view.metronome, c.ink_blue),
        ),
    ] {
        let mut r = layout.rect(b);
        if b == BarButton::Record {
            r.x += view.record_shake;
        }
        // While recording, the record button sits on the selection face; it blinks while the
        // song plays up to the punch range.
        let on = match b {
            BarButton::Record => {
                view.recording
                    && !(view.pre_roll
                        && (std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis())
                            / BLINK.as_millis())
                            % 2
                            == 1)
            }
            BarButton::Loop => view.looping,
            BarButton::PreRoll => view.pre_roll_on,
            BarButton::Metronome => view.metronome,
            _ => false,
        };
        let lit = if b == BarButton::Record {
            c.arm_on.0
        } else {
            c.fill
        };
        let f = pen.transition(
            &view.motion,
            view.motion_setting,
            r,
            &format!("transport-{b:?}"),
            on,
            face(b).unwrap_or(c.surface_1),
            lit,
        );
        pen.round(r, f, Radius::S);
        // The count-in counts the beats left in the record button, where the dot is.
        if let (BarButton::Record, Some(left)) = (b, view.pre_count) {
            pen.digits(
                &left.to_string(),
                FontSize::Body,
                Weight::Medium,
                c.on_fill,
                r,
                Align::Center,
            );
        } else {
            pen.icon(name, icon_at(r), if on { c.on_fill } else { ink });
        }
    }
    // Under the pointer the stopped play button already wears the primary
    // hover face, so its icon takes the ink on that face (UI-309).
    let play_off = if view.hover == Some(BarButton::Play) {
        c.primary_hover
    } else {
        (c.surface_3, c.ink)
    };
    let (play_face, play_ink) = pen.transition_pair(
        &view.motion,
        view.motion_setting,
        layout.play,
        "transport-play",
        view.playing,
        play_off,
        c.primary,
    );
    pen.round(layout.play, play_face, Radius::Pill);
    pen.icon("play", icon_at(layout.play), play_ink);
    // One strip: bar.beat.tick, the time beside it, the tempo and the meter.
    let t = layout.time;
    let end = if layout.meter.w > 0.0 {
        layout.meter.right()
    } else {
        layout.bpm.right()
    };
    pen.round(Rect::new(t.x, t.y, end - t.x, t.h), c.surface_2, Radius::S);
    let pos_w = (t.w - Space::X2.px()).min(POS_W);
    pen.digits(
        &view.time,
        FontSize::Lead,
        Weight::Medium,
        if view.recording_now() {
            c.ink_alert
        } else {
            c.ink_strong
        },
        Rect::new(t.x + Space::X2.px(), t.y, pos_w, t.h),
        Align::Left,
    );
    let secs_x = t.x + Space::X2.px() + POS_W + Space::X1.px();
    if t.right() >= secs_x + SECS_W {
        pen.digits(
            &view.seconds,
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(secs_x, t.y, SECS_W, t.h),
            Align::Left,
        );
    }
    for (b, r, text) in [
        (BarButton::Tempo, layout.bpm, &view.bpm),
        (BarButton::Meter, layout.meter, &view.meter),
    ] {
        if r.w == 0.0 {
            continue;
        }
        // The readout is a control: a face under the pointer, a lit one while
        // it is dragged or its list is open, a frame while it has the keys.
        let face = Rect::new(r.x, r.y + 3.0, r.w, r.h - 6.0);
        let editing = b == BarButton::Tempo && matches!(view.tempo, TempoMode::Edit { .. });
        if editing {
            pen.round(face, c.surface_1, Radius::S);
        } else if view.open == Some(b) || (b == BarButton::Tempo && view.tempo == TempoMode::Drag) {
            pen.round(face, c.sel, Radius::S);
        } else if view.hover == Some(b) {
            pen.round(face, c.hover, Radius::S);
        }
        if b == BarButton::Tempo && view.tempo != TempoMode::Idle && view.tempo != TempoMode::Drag {
            crate::bar_tempo::frame(pen, face, 1.5, c.ring);
        }
        if let (true, TempoMode::Edit { fresh }) = (editing, view.tempo) {
            let w = crate::browser::text_width(text, FontSize::Small);
            let x = r.x + (r.w - w) / 2.0;
            if fresh {
                pen.round(
                    Rect::new(x - 2.0, face.y + 3.0, w + 4.0, face.h - 6.0),
                    c.fill,
                    Radius::Xs,
                );
            }
            pen.digits(
                text,
                FontSize::Small,
                Weight::Medium,
                if fresh { c.on_fill } else { c.ink_strong },
                r,
                Align::Center,
            );
            if !fresh {
                pen.fill(
                    Rect::new(x + w + 1.0, face.y + 5.0, 1.5, face.h - 10.0),
                    c.ink_strong,
                );
            }
            continue;
        }
        pen.digits(
            text,
            FontSize::Small,
            Weight::Medium,
            if b == BarButton::Tempo && view.tempo == TempoMode::Drag {
                c.ink_strong
            } else {
                c.ink
            },
            r,
            Align::Center,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_icon_is_loaded_and_audition_is_the_speaker_not_the_ear() {
        let loaded: Vec<&str> = crate::assets::all_icons().map(|(n, _)| *n).collect();
        for tool in Tool::ALL {
            assert!(loaded.contains(&tool_icon(tool)), "{tool:?}");
        }
        // The ear is the track head's monitor button; the tool is the mock's speaker.
        assert_eq!(tool_icon(Tool::Audition), "listen");
    }

    fn overlap(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    /// The top bar's parts, in its coordinates.
    fn top(l: &BarLayout) -> Vec<(&'static str, Rect)> {
        let mut parts = vec![
            ("logo", l.logo),
            ("name", l.name),
            ("saved", l.saved),
            ("grid", l.grid),
            ("snap", l.snap),
            ("length", l.length),
            ("rec-panel", l.rec_panel),
            ("history", l.history),
            ("palette", l.palette),
            ("ai", l.ai),
            ("export", l.export),
            ("alt", l.alt),
            ("shape", l.shape),
            ("ripple", l.ripple),
        ];
        parts.extend(l.tools.iter().map(|(t, r)| (tool_icon(*t), *r)));
        parts.retain(|(_, r)| r.w > 0.0);
        parts
    }

    /// The bottom bar's parts, in its coordinates (UI-300).
    fn bottom(l: &BarLayout) -> Vec<(&'static str, Rect)> {
        vec![
            ("cpu", l.cpu),
            ("skip", l.skip),
            ("play", l.play),
            ("stop", l.stop),
            ("record", l.record),
            ("loop", l.looped),
            ("preroll", l.preroll),
            ("metronome", l.metronome),
            ("time", l.time),
            ("bpm", l.bpm),
            ("meter", l.meter),
        ]
    }

    fn check(parts: &[(&'static str, Rect)], right: f32, h: f32, w: f32) {
        for (i, (an, a)) in parts.iter().enumerate() {
            assert!(a.x >= 0.0 && a.right() <= right + 0.5, "{an} at {w}");
            assert!(a.y >= 0.0 && a.bottom() <= h, "{an} at {w}: {a:?}");
            for (bn, b) in &parts[i + 1..] {
                assert!(!overlap(*a, *b), "{an} overlaps {bn} at {w}");
            }
        }
    }

    #[test]
    fn nothing_overlaps_and_everything_stays_in_the_bar_at_the_smallest_and_a_wide_window() {
        for w in [960.0, 1120.0, 1280.0, 1440.0, 2200.0] {
            for (arrange, length) in [(true, false), (false, true)] {
                let l = BarLayout::of_save(w, "夜の散歩", arrange, length, false, "1/16・3連");
                check(&top(&l), w - caption_w(), Layout::TOP_BAR_H, w);
                check(&bottom(&l), w, Layout::BOTTOM_BAR_H, w);
            }
        }
    }

    #[test]
    fn nothing_overlaps_at_any_width_from_960_to_2200() {
        let mut w = 960.0;
        while w <= 2200.0 {
            let l = BarLayout::new(w, &"とても長い曲の名前".repeat(6));
            check(&top(&l), w - caption_w(), Layout::TOP_BAR_H, w);
            check(&bottom(&l), w, Layout::BOTTOM_BAR_H, w);
            w += 4.0;
        }
    }

    /// UI-300: the top bar keeps every part at the smallest window; nothing drops out.
    #[test]
    fn the_smallest_window_keeps_every_part_of_the_top_bar() {
        let l = BarLayout::of_save(960.0, "夜の散歩", true, true, false, "1/16・3連");
        assert!(l.logo.w > 0.0);
        assert_eq!(l.tools.len(), Tool::ALL.len());
        for r in [
            l.alt,
            l.shape,
            l.ripple,
            l.snap,
            l.grid,
            l.length,
            l.rec_panel,
        ] {
            assert!(r.w > 0.0, "{r:?}");
        }
        assert!(l.name.w >= NAME_MIN_W);
    }

    #[test]
    fn the_transport_is_centred_in_the_bottom_bar_and_the_cpu_is_at_its_left() {
        for w in [960.0, 1440.0, 2200.0] {
            let l = BarLayout::new(w, "曲");
            let mid = (l.skip.x + l.meter.right()) / 2.0;
            assert!((mid - w / 2.0).abs() < 0.5, "{mid} at {w}");
            assert_eq!(l.cpu.x, Space::X2.px());
            assert!(l.cpu.right() < l.skip.x);
        }
    }

    #[test]
    fn grid_sits_right_of_the_tools_and_history_and_export_at_the_far_right() {
        let l = BarLayout::new(1440.0, "曲");
        let last_tool = l.tools.last().unwrap().1;
        assert!(l.grid.x >= last_tool.right());
        assert!(l.history.x > l.grid.right());
        assert!(l.palette.x > l.history.x);
        assert!(l.ai.x > l.palette.x);
        assert!(l.export.x > l.ai.x);
        assert!(l.export.right() <= 1440.0 - caption_w());
    }

    #[test]
    fn the_buttons_are_found_by_a_point_and_the_rest_of_the_bar_is_not() {
        let l = BarLayout::new(1920.0, "曲");
        for b in BarLayout::BUTTONS {
            let r = l.rect(b);
            // The length chip only stands in the roll's bar with room made for it.
            if r.w <= 0.0 {
                continue;
            }
            let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            if b.in_bottom() {
                assert_eq!(l.hit_bottom(x, y), Some(b));
            } else {
                assert_eq!(l.hit(x, y), Some(b));
            }
        }
        assert_eq!(l.hit_bottom(l.time.x + 4.0, l.time.y + 4.0), None);
        assert_eq!(l.hit(5.0, 5.0), None);
        // A point of one bar does not find the other bar's button there.
        let play = l.play;
        assert_eq!(l.hit(play.x + 4.0, play.y + 4.0), None);
    }

    #[test]
    fn every_tool_has_a_button_at_every_width() {
        for w in [960.0, 1440.0] {
            let l = BarLayout::new(w, "曲");
            assert_eq!(l.tools.len(), Tool::ALL.len());
            for t in Tool::ALL {
                let r = l.rect(BarButton::Tool(t));
                assert_eq!(
                    l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
                    Some(BarButton::Tool(t))
                );
            }
        }
    }

    #[test]
    fn no_button_lies_in_a_drag_stretch() {
        let l = BarLayout::new(1440.0, "夜の散歩");
        assert!(!l.drag.is_empty());
        for b in BarLayout::BUTTONS {
            if b.in_bottom() {
                continue;
            }
            let r = l.rect(b);
            for (x0, x1) in &l.drag {
                assert!(r.right() <= *x0 || r.x >= *x1, "{b:?} in {x0}..{x1}");
            }
        }
        for (_, r) in &l.tools {
            for (x0, x1) in &l.drag {
                assert!(r.right() <= *x0 || r.x >= *x1, "{r:?} in {x0}..{x1}");
            }
        }
        let last = l.drag.last().unwrap();
        assert!((last.1 - (1440.0 - caption_w())).abs() < 0.5);
    }

    #[test]
    fn a_long_name_is_cut_short_instead_of_pushing_the_bar() {
        let short = BarLayout::new(1440.0, "曲");
        let long = BarLayout::new(1440.0, &"とても長い曲の名前".repeat(6));
        assert!(long.name.w <= NAME_MAX_W);
        assert!(long.name.w > short.name.w);
        // At the smallest window the name gives way to the editing group.
        let small = BarLayout::new(960.0, &"とても長い曲の名前".repeat(6));
        assert!(small.name.w < long.name.w && small.name.w >= NAME_MIN_W);
        assert!(small.saved.right() < small.tools[0].1.x);
    }

    #[test]
    fn there_is_no_text_on_the_export_button() {
        let l = BarLayout::new(1440.0, "曲");
        assert_eq!(l.export.w, Layout::ICON_BUTTON);
    }

    #[test]
    fn the_position_writes_the_tick_in_three_digits_as_in_the_mock() {
        let p = xirius_core::model::Project::new("曲");
        let map = p.time_map();
        assert_eq!(bar_beat_tick(&map, 0), "1.1.000");
        let ppq = xirius_core::time::Tick::from(p.ppq);
        assert_eq!(bar_beat_tick(&map, ppq * 4 * 4 + ppq + 5), "5.2.005");
    }

    #[test]
    fn the_position_tempo_and_meter_sit_in_one_strip_at_every_width() {
        for w in [960.0, 1440.0] {
            let l = BarLayout::new(w, "夜の散歩");
            assert!(l.time.w >= POS_W + SECS_W);
            assert!(l.bpm.x - l.time.right() <= Space::X1.px() + 0.5);
            assert!(l.meter.x - l.bpm.right() <= Space::X1.px() + 0.5);
            assert!(l.meter.w > 0.0);
        }
    }

    /// UI-302: the snap switch is a 28 x 28 button just left of the grid value, and the value
    /// is at least 64 wide and holds every label with its ▾.
    #[test]
    fn the_cpu_meter_fills_its_well_with_the_load_and_stays_full_past_it() {
        let l = BarLayout::of_save(960.0, "夜の散歩", true, false, false, "1/16");
        let well = l.cpu.w - 2.0 * CPU_PAD_X;
        assert_eq!(cpu_fill_w(0, well), 0.0);
        assert_eq!(cpu_fill_w(50, well), (well / 2.0).round());
        assert_eq!(cpu_fill_w(100, well), well);
        assert_eq!(cpu_fill_w(180, well), well, "an overload keeps the shape");
        // The row and the well fit in the button, and "CPU" and the widest number do not meet.
        assert!(CPU_TEXT_H + CPU_GAP + CPU_WELL_H + 8.0 <= l.cpu.h);
        assert!(
            text_width("CPU", FontSize::Caption) + 3.0 + text_width("100%", FontSize::Caption)
                <= well
        );
    }

    #[test]
    fn the_snap_switch_stands_left_of_the_grid_value() {
        let l = BarLayout::new(1440.0, "曲");
        assert_eq!(
            (l.snap.w, l.snap.h),
            (Layout::ICON_BUTTON, Layout::ICON_BUTTON)
        );
        assert_eq!(l.grid.x - l.snap.right(), SNAP_GAP);
        assert!(l.ripple.right() < l.snap.x);
        assert_eq!(l.hit(l.snap.x + 4.0, l.snap.y + 4.0), Some(BarButton::Snap));
        for text in ["1/4", "1/16", "1/128", "小節", "fr"] {
            assert_eq!(grid_w(text), GRID_W, "{text}");
        }
        assert!(grid_w("1/16") >= 64.0);
        let wide = grid_w("1/16・3連");
        assert!(wide > GRID_W);
        let room = wide - GRID_PAD_L - GRID_TEXT_GAP - CHEVRON - GRID_PAD_R;
        assert!(room >= text_width("1/16・3連", FontSize::Small));
    }

    #[test]
    fn the_grid_label_names_triplets() {
        assert_eq!(grid_label("1/16"), "1/16");
        assert_eq!(grid_label("1/8t"), "1/8・3連");
    }

    #[test]
    fn the_grid_text_follows_the_snap_mode() {
        assert_eq!(grid_text(SnapMode::Quantize, "1/8t", 320, 960), "1/8・3連");
        assert_eq!(grid_text(SnapMode::Bar, "1/16", 3840, 960), "小節");
        assert_eq!(grid_text(SnapMode::Frames, "1/16", 64, 960), "fr");
        assert_eq!(grid_text(SnapMode::Auto, "1/16", 120, 960), "1/32");
    }
}
