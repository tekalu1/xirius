//! The right-click menus of the scenes (CTX-001 …): one shared set of parts, and
//! one file per group of scenes that says which rows a scene has.
//!
//! A row is a command of core's table: its name on the screen is the command's short
//! name, its key is the one the palette shows, and choosing it runs the command by
//! the path of the keys and the palette. A scene's file only picks the commands and
//! the arguments that fit the thing that was pressed. The menu itself is the
//! pop-over the other menus use (`PopupKind::Context`): the same rows, gaps between
//! groups instead of lines, dim rows that stay in their place.

use serde_json::Value;
use xirius_core::auto::AutoTarget;
use xirius_core::id::{FolderId, PluginId, TrackId};
use xirius_core::time::Tick;
use xirius_design::{Layout, Space, TrackColor};

use crate::palette::{Row as PaletteRow, Way, scope_of};
use crate::popup::{Action, Cell, Look, PopupKind};
use crate::studio::{Geometry, Rect, Studio};

pub mod ask;
mod depth;
mod range_lane;
use depth::flatten;
pub mod auto;
pub mod browser;
pub mod clip;
pub mod export;
pub mod fields;
pub(crate) mod key_more;
pub mod marker;
pub mod mixed;
pub mod mixer;
mod next_prev;
mod pool;
mod preset;
pub mod roll;
pub mod ruler;
pub mod screen_cmd;
mod screen_cmd_rows;
pub mod search;
mod section;
pub mod take;
pub mod tempo;
pub mod timeline;
pub mod tools;
pub mod track;

/// The widest a menu grows to.
const WIDEST: f32 = 360.0;
/// The squares of a colour row, as the colour pop-over has them.
use crate::popup::{SWATCH, SWATCH_COLUMNS, SWATCH_GAP};
/// The room left of a row's text, and right of its end (as the rows of `popup`).
const SIDE: f32 = 16.0;
/// What a row of the menu does when it is chosen.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    /// A little room between two groups (no line).
    Gap,
    /// Rows one level down; they take the place of this menu.
    Sub(Vec<Item>),
    /// A command with its arguments.
    Cmd(&'static str, Value),
    /// A command that has a key: the key's own path.
    Key(&'static str),
    /// A change of the screen that is not a command.
    Do(Act),
    /// Back to the menu above.
    Back,
    /// A command that takes a name: the menu turns into a name box (command, its
    /// arguments, the argument the name goes to, the name it starts with).
    Ask(&'static str, Value, &'static str, String),
    /// The same with a number: the box takes digits and the argument is the number.
    AskNumber(&'static str, Value, &'static str, String),
    /// A field of an audio clip's value at the head of its menu (its kind, the clips, the
    /// value they share).
    Field(fields::Kind, Value, Option<f64>),
}

/// The changes of the screen the menus make that no command makes.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Tool(&'static str),
    /// Moves the playhead.
    Seek(Tick),
    /// Opens the editor of the selected clip.
    Open,
    /// Shows a track's lane of a target, making the lane when the track has none.
    Lane(TrackId, String),
    /// The rows inside a folder become the selection.
    SelectIn(FolderId),
    /// Opens or closes every folder.
    Folders(bool),
    /// Opens the export panel with these tracks as the stems.
    Stems(Vec<TrackId>),
    /// Opens the export panel with a folder's bus as one stem named after the folder.
    FolderStem(FolderId),
    /// Starts playing from a song position.
    PlayFrom(Tick),
    /// Opens the export panel for the mixdown of the loop.
    ExportLoop,
    ExportSpan(Tick, Tick),
    /// Opens the box that types the value of an automation point, at a window point.
    PointValue(TrackId, AutoTarget, Tick, f32, f32),
    /// Deletes the selected automation points.
    DeletePoints,
    /// Hides a track's lane.
    HideLane(TrackId, AutoTarget),
    /// The roll's notes of one pitch become the selection.
    RollPitch(u8),
    /// A controller lane of the roll shows another controller.
    RollLaneKind(usize, crate::roll::LaneKind),
    /// Hides a controller lane of the roll.
    RollLaneHide(usize),
    /// Shows a channel's row in the track column.
    Reveal(TrackId),
    /// Scrolls the mixer to a channel's strip.
    ShowStrip(TrackId),
    /// The channels that play into a bus become the selection.
    SelectSources(TrackId),
    /// Opens the window of an effect.
    OpenPlugin(TrackId, PluginId),
    /// Loads a preset into an effect.
    Preset(PluginId, String),
    /// Reloads the effects that went down.
    /// Starts the process of this plugin again (it went down).
    RestartPlugin(PluginId),
    /// Opens the list of effects under a channel's ＋ row.
    PickInsert(TrackId),
    /// Puts an effect used lately (its name or `builtin:` id) at the end of a channel.
    InsertRecent(TrackId, String),
    /// Asks for a preset file to read into an effect (false) or to write it to (true).
    PresetFile(PluginId, bool),
    /// A row of the browser's menus.
    Browse(browser::BrowseAct),
    /// A row of the menu of an export.
    Export(export::ExportAct),
    /// A part waits for a knob of the controller (the target as `track.volume:<id>`).
    MidiLearn(String),
    /// Gives up waiting for a knob.
    MidiLearnCancel,
    /// Takes the knob over from the part that had it.
    MidiLearnConfirm,
    /// Puts the transform frame round the stretch of a lane or of the tempo row (AUT-010).
    RangeFrame(crate::arrange::range_trim::RangePlace, Tick, Tick),
    /// Opens the export panel on the MIDI files of these instrument tracks, over the span or the
    /// whole song (UI-281).
    MidiExport(Vec<TrackId>, Option<(Tick, Tick)>),
}

/// One row of a menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    /// The key, as the palette writes it.
    pub key: String,
    pub enabled: bool,
    /// Switched on (a check at the end).
    pub on: bool,
    /// After it runs, the clips it made are the selection.
    pub selects: bool,
    pub reason: String,
    /// A colour of the palette, drawn as a square (the colour rows).
    pub swatch: Option<usize>,
    /// A colour square before the label (the track's colour on its 「色」 row).
    pub mark: Option<usize>,
    pub run: Run,
}

impl Item {
    fn new(label: &str, run: Run) -> Item {
        Item {
            label: label.to_string(),
            key: String::new(),
            enabled: true,
            on: false,
            selects: false,
            reason: String::new(),
            swatch: None,
            mark: None,
            run,
        }
    }

    /// A row for a change of the screen that no command makes, with the key it has.
    pub fn act(label: &str, key: &str, act: Act) -> Item {
        Item {
            key: key.to_string(),
            ..Item::new(label, Run::Do(act))
        }
    }

    pub fn gap() -> Item {
        Item::new("", Run::Gap)
    }

    /// The first row of a menu one level down.
    pub fn back() -> Item {
        Item::new("戻る", Run::Back)
    }

    /// A row that opens `rows` (a chevron at its end).
    pub fn sub(label: &str, rows: Vec<Item>) -> Item {
        let enabled = rows.iter().any(|r| r.enabled && !matches!(r.run, Run::Gap));
        Item {
            enabled,
            ..Item::new(label, Run::Sub(rows))
        }
    }

    /// The rows of a colour: `none` is the first row's name (「トラックの色」 for a clip),
    /// then the palette. `args` has the things the command acts on; each row adds `color`.
    pub fn colors(none: &str, name: &'static str, args: Value, enabled: bool) -> Item {
        Item::sub("色", Item::color_rows(none, name, args)).enabled(enabled)
    }

    /// A colour row that shows `current`: a square before its label, and a check on that colour
    /// among the squares inside.
    pub fn current(mut self, current: usize) -> Item {
        self.mark = Some(current);
        if let Run::Sub(rows) = &mut self.run {
            for row in rows {
                row.on = row.swatch == Some(current);
            }
        }
        self
    }

    /// The rows inside a colour row (see `colors`).
    pub fn color_rows(none: &str, name: &'static str, args: Value) -> Vec<Item> {
        let with = |color: String| {
            let mut a = args.clone();
            a["color"] = Value::String(color);
            Run::Cmd(name, a)
        };
        let mut rows = vec![Item::new(none, with("none".to_string()))];
        rows.push(Item::gap());
        for c in TrackColor::all() {
            rows.push(Item {
                swatch: Some(c.index()),
                ..Item::new("", with(crate::song::hex(c)))
            });
        }
        rows
    }

    /// A row that turns the menu into a name box (see `Run::Ask`).
    pub fn ask(
        label: &str,
        name: &'static str,
        args: Value,
        arg: &'static str,
        initial: &str,
    ) -> Item {
        Item::new(label, Run::Ask(name, args, arg, initial.to_string()))
    }

    /// A row that turns the menu into a box for a number (see `Run::AskNumber`).
    pub fn ask_number(
        label: &str,
        name: &'static str,
        args: Value,
        arg: &'static str,
        initial: &str,
    ) -> Item {
        Item::new(label, Run::AskNumber(name, args, arg, initial.to_string()))
    }

    pub fn on(mut self, on: bool) -> Item {
        self.on = on;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Item {
        self.enabled = enabled;
        self
    }

    /// The clips the command makes become the selection.
    pub fn selecting(mut self) -> Item {
        self.selects = true;
        self
    }

    pub fn labelled(mut self, label: &str) -> Item {
        self.label = label.to_string();
        self
    }
}

/// A row as the pop-over draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: String,
    pub key: String,
    pub enabled: bool,
    pub on: bool,
    pub sub: bool,
    pub gap: bool,
    pub swatch: Option<usize>,
    pub mark: Option<usize>,
    pub reason: String,
    /// The field of an audio clip's value, when it is one.
    pub field: Option<fields::Shown>,
}

/// The menu that is open.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub search: Option<search::Search>,
    pub items: Vec<Item>,
    /// The menus above this one (a row opened it).
    pub trail: Vec<Vec<Item>>,
    /// The name box, while the menu asks for a name.
    pub ask: Option<ask::Ask>,
    /// The field of a clip's value that has the keys.
    pub field: Option<fields::Typing>,
    /// How far a wheel has turned since the last notch over a field (UI-313).
    pub turn: fields::WheelTurn,
    /// When the wheel last moved this menu's list (UI-313): a turn within 400 ms of it keeps
    /// moving the list, even over a field.
    pub scrolled_at: Option<std::time::Instant>,
}

impl State {
    pub fn rows(&self) -> Vec<Row> {
        self.items
            .iter()
            .enumerate()
            .map(|(n, i)| Row {
                label: i.label.clone(),
                key: i.key.clone(),
                enabled: i.enabled,
                on: i.on,
                sub: matches!(i.run, Run::Sub(_)),
                gap: matches!(i.run, Run::Gap),
                swatch: i.swatch,
                mark: i.mark,
                field: match &i.run {
                    Run::Field(kind, _, value) => Some(fields::Shown::of(
                        *kind,
                        *value,
                        self.field.as_ref().filter(|t| t.row == n),
                    )),
                    _ => None,
                },
                reason: if i.enabled {
                    String::new()
                } else if !i.reason.is_empty() {
                    i.reason.clone()
                } else {
                    crate::disabled_reason::label(&i.label).into()
                },
            })
            .collect()
    }
}

/// How wide the menu is: the widest row and its key, between the two limits.
pub fn width(rows: &[Row]) -> f32 {
    let text = |s: &str, ascii: f32, wide: f32| -> f32 {
        s.chars()
            .map(|c| if c.is_ascii() { ascii } else { wide })
            .sum()
    };
    let widest = rows
        .iter()
        .map(|r| {
            if r.field.is_some() {
                return fields::ROW_W;
            }
            let end = if r.sub {
                Layout::ICON_SIZE
            } else {
                text(&r.key, 6.5, 12.0)
            };
            text(&r.label, 7.5, 13.5) + end + if end > 0.0 { SIDE } else { 0.0 }
        })
        .fold(0.0f32, f32::max);
    (widest + Layout::POPUP_PAD * 2.0 + SIDE * 2.0).clamp(Layout::MENU_W, WIDEST)
}

/// The rows of the pop-over, laid out from its top-left corner.
pub fn cells(rows: &[Row], w: f32) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    let mut cells = Vec::new();
    let mut y = pad;
    let mut column = 0;
    for (i, row) in rows.iter().enumerate() {
        if row.gap {
            // No line between groups (design rules): room.
            y += Space::X2.px();
            continue;
        }
        if let Some(color) = row.swatch {
            let step = SWATCH + SWATCH_GAP;
            let r = Rect::new(pad + column as f32 * step, y, SWATCH, SWATCH);
            cells.push(
                Cell::new(r, Look::Swatch(Some(color)))
                    .on(row.on)
                    .act(Action::Ctx(i)),
            );
            column += 1;
            let last = rows.get(i + 1).is_none_or(|n| n.swatch.is_none());
            if column == SWATCH_COLUMNS || last {
                column = 0;
                y += step;
            }
            continue;
        }
        let r = Rect::new(pad, y, w - pad * 2.0, Layout::MENU_ROW_H);
        if let Some(field) = &row.field {
            let mut row_cells = fields::cells(r, i, field, row.enabled);
            if !row.enabled {
                for c in &mut row_cells {
                    c.tip = row.reason.clone();
                }
            }
            cells.extend(row_cells);
            y += fields::ROW_H;
            continue;
        }
        let mut cell = Cell::new(r, Look::Row)
            .text(row.label.clone())
            .mark(row.mark);
        if row.sub {
            cell = cell.icon("chevron-right");
        } else {
            cell = cell.hint(row.key.clone()).on(row.on);
        }
        cell = if row.enabled {
            cell.act(Action::Ctx(i))
        } else {
            cell.off().tip(&row.reason)
        };
        cells.push(cell);
        y += Layout::MENU_ROW_H;
    }
    cells
}

impl Studio {
    /// A row that runs `name` with `args`. Its name and key are the command's; it is dim
    /// when the command would change nothing now.
    pub(crate) fn ctx_cmd(&mut self, name: &'static str, args: Value) -> Item {
        let label = self.song.registry().label(name).unwrap_or(name).to_string();
        let enabled = self.song.check(name, args.clone()).unwrap_or(false);
        Item {
            key: self.menu_key(name),
            enabled,
            ..Item::new(&label, Run::Cmd(name, args))
        }
    }

    /// A row that runs `name` with `args` under its own name (a way of running the command:
    /// 50%, +12). It has no key, since the key runs the command another way.
    pub(crate) fn ctx_as(&mut self, label: &str, name: &'static str, args: Value) -> Item {
        let result = self.song.check(name, args.clone());
        let enabled = result.as_ref().copied().unwrap_or(false);
        let reason = if enabled {
            String::new()
        } else {
            result.err().map_or_else(
                || crate::disabled_reason::command(self, name).into(),
                |e| e.to_string(),
            )
        };
        Item {
            enabled,
            reason,
            ..Item::new(label, Run::Cmd(name, args))
        }
    }

    /// A row that runs a command which changes a setting or the view, not the song. `check`
    /// reports those as changing nothing, so the row is always enabled.
    pub(crate) fn ctx_setting(&mut self, label: &str, name: &'static str, args: Value) -> Item {
        Item::new(label, Run::Cmd(name, args))
    }

    /// A row that runs a command by its key's path (what the keys and the palette run).
    pub(crate) fn ctx_key(&mut self, g: &Geometry, name: &'static str) -> Item {
        let label = self.song.registry().label(name).unwrap_or(name).to_string();
        let (key, enabled) = match scope_of(name) {
            Some(scope) => (self.menu_key(name), self.key_enabled(g, name, scope)),
            None => (String::new(), false),
        };
        Item {
            key,
            enabled,
            reason: if enabled {
                String::new()
            } else {
                crate::disabled_reason::command(self, name).into()
            },
            ..Item::new(&label, Run::Key(name))
        }
    }

    /// A row for a command the session runs (a bounce, a sound read from the file), whose
    /// check is its own: `enabled` as told, with `reason` when not. `key` is the name the key
    /// is bound to (it may carry arguments).
    pub(crate) fn ctx_session(
        &self,
        label: &str,
        name: &'static str,
        key: &str,
        args: Value,
        enabled: bool,
        reason: &str,
    ) -> Item {
        Item {
            key: self.menu_key(key),
            enabled,
            reason: if enabled {
                String::new()
            } else {
                reason.to_string()
            },
            ..Item::new(label, Run::Cmd(name, args))
        }
    }

    /// A row for a change of the screen.
    pub(crate) fn ctx_do(&self, name: &'static str, act: Act) -> Item {
        let label = self.song.registry().label(name).unwrap_or(name).to_string();
        Item {
            key: self.menu_key(name),
            ..Item::new(&label, Run::Do(act))
        }
    }

    /// Opens a menu of `items` at a point of the body. The gap at the end of a group is
    /// dropped when nothing follows it.
    pub(crate) fn open_context(&mut self, items: Vec<Item>, x: f32, y: f32) {
        let mut items = items;
        for item in &mut items {
            if let Run::Sub(rows) = &mut item.run {
                *rows = flatten(std::mem::take(rows), "");
            }
        }
        while matches!(items.last().map(|i| &i.run), Some(Run::Gap)) {
            items.pop();
        }
        // The transposition's field takes the keys when the menu opens (UI-313, P3-5 of review
        // 15): the caret and its ring are there at once, the search box keeps none of the IME's
        // composition, and a menu without it gives the keys to its first value's field. A
        // destructible field such as the tempo is not picked first on an audio clip.
        let first = items
            .iter()
            .position(|i| i.enabled && matches!(i.run, Run::Field(fields::Kind::Semis, ..)))
            .or_else(|| {
                items
                    .iter()
                    .position(|i| i.enabled && matches!(i.run, Run::Field(..)))
            });
        self.ctx = Some(State {
            search: search::Search::of(&items),
            items,
            trail: Vec::new(),
            ask: None,
            field: first.map(|row| fields::Typing {
                row,
                text: String::new(),
                fresh: true,
                marked: String::new(),
            }),
            turn: fields::WheelTurn::default(),
            scrolled_at: None,
        });
        self.popup = Some(crate::popup::Popup::new(
            PopupKind::Context,
            Rect::new(x, y, 0.0, 0.0),
        ));
        self.refresh_popup();
    }

    /// The rows of the menu that is open.
    pub fn context_rows(&self) -> Vec<Row> {
        self.ctx.as_ref().map(State::rows).unwrap_or_default()
    }

    /// Whether a context menu is open.
    pub fn context_open(&self) -> bool {
        self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Context)
    }

    /// Chooses row `i` of the menu that is open. A row with rows below opens them in
    /// the menu's place; any other closes the menu and runs.
    pub(crate) fn context_pick(&mut self, g: &Geometry, i: usize) -> bool {
        let Some(item) = self.ctx.as_ref().and_then(|s| s.items.get(i)).cloned() else {
            return false;
        };
        if !item.enabled {
            return false;
        }
        if let Run::Sub(rows) = item.run.clone() {
            let st = self.ctx.as_mut().expect("the menu is open");
            let mut next = vec![Item::back(), Item::gap()];
            next.extend(rows);
            st.trail.push(std::mem::replace(&mut st.items, next));
            st.search = search::Search::of(&st.items);
            st.field = None;
            self.refresh_popup();
            return true;
        }
        if item.run == Run::Back {
            let st = self.ctx.as_mut().expect("the menu is open");
            if let Some(up) = st.trail.pop() {
                st.items = up;
                st.search = search::Search::of(&st.items);
                st.field = None;
            }
            self.refresh_popup();
            return true;
        }
        if matches!(item.run, Run::Ask(..) | Run::AskNumber(..)) {
            self.ask_open(item.run);
            return true;
        }
        if matches!(item.run, Run::Field(..)) {
            return self.field_focus(i);
        }
        self.ctx = None;
        self.popup = None;
        self.context_run(g, item)
    }

    /// Chooses the row named `label` of the menu that is open (the screenshot scripts and
    /// the tests press rows by name).
    pub(crate) fn context_choose(&mut self, g: &Geometry, label: &str) -> bool {
        let Some(i) = self
            .context_rows()
            .iter()
            .position(|r| !r.gap && r.label == label)
        else {
            return false;
        };
        self.context_pick(g, i)
    }

    /// Runs a row.
    fn context_run(&mut self, g: &Geometry, item: Item) -> bool {
        match item.run {
            Run::Gap
            | Run::Sub(_)
            | Run::Back
            | Run::Ask(..)
            | Run::AskNumber(..)
            | Run::Field(..) => false,
            Run::Cmd(name, args) => {
                let Some(data) = self.arrange_run(name, args) else {
                    return false;
                };
                if item.selects {
                    self.select_made(&data);
                }
                self.section_followed(name, &data);
                self.rebuild(g);
                if let Some(ctx) = self.roll_ctx(g) {
                    self.roll.sync(&ctx);
                }
                true
            }
            Run::Key(name) => {
                let Some(scope) = scope_of(name) else {
                    return false;
                };
                let row = PaletteRow {
                    name,
                    label: item.label,
                    key: None,
                    reason: String::new(),
                    icon: "",
                    enabled: true,
                    way: Way::Key(scope),
                };
                self.run_row(g, &row)
            }
            // The point's value box hangs where the menu was, and a row of an export's menu keeps
            // the place its panel hung: the two rows that carry a window point.
            Run::Do(Act::PointValue(t, target, at, x, y)) => self
                .auto_ref(t, &target, at)
                .is_some_and(|point| self.auto_value_open(point, x, y)),
            // A mixed selection loses its clips too (CTX-007), as the Delete key does.
            Run::Do(Act::DeletePoints) => {
                let used = self.delete_points_and_clips();
                self.rebuild(g);
                used
            }
            Run::Do(Act::Export(act)) => self.export_run_row(act),
            // Every other row runs the command it stands for, as the typed command does.
            Run::Do(act) => {
                let (name, args) = screen_cmd::act_command(&act);
                let args = args.as_object().cloned().unwrap_or_default();
                self.run_parsed(g, name.to_string(), args)
            }
        }
    }

    /// The right press at a point of the body: opens the menu of what is there. False when
    /// no scene claims the point.
    pub(crate) fn scene_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.browser.typing = false;
        self.input_map.browser_focus = self.browser.open && g.panel.contains(x, y);
        self.input_map.context_delete = None;
        self.input_map.context_clipboard = None;
        if self.context_open() {
            self.popup = None;
            self.ctx = None;
        }
        if self.export_context(g, x, y) {
            return true;
        }
        if self.roll_input(g).is_some_and(|c| c.body.contains(x, y)) {
            return self.roll_context(g, x, y);
        }
        if self.mixer_shown() && g.drawer_body.contains(x, y) {
            return self.mixer_context(g, x, y);
        }
        self.timeline_context(g, x, y)
    }

    /// Shift+F10 and the menu key: the menu of what is selected where the keys go, opened where
    /// a press on its middle would open it: the clip in the arrangement, the first selected
    /// note in the piano roll, the selected track's strip in the mixer.
    pub(crate) fn key_context(&mut self, g: &Geometry) -> bool {
        // The song-files tab with the keys: the menu of the row the keys are on (BRW-004).
        if self.input_context(g) == "browser" && self.pool_shown() {
            return self.pool_key_menu(g);
        }
        let at = match self.input_context(g) {
            "roll" => self.key_context_note(g),
            "mixer" => self.key_context_strip(g),
            _ => self
                .selected_clip
                .or_else(|| self.arrange.clips.iter().next().copied())
                .and_then(|(t, c)| self.center_of(g, crate::studio::Hit::Clip(t, c))),
        };
        let Some((x, y)) = at else {
            return false;
        };
        self.popup = None;
        self.scene_context(g, x, y)
    }

    /// The middle of the earliest selected note, when the roll shows it.
    fn key_context_note(&self, g: &Geometry) -> Option<(f32, f32)> {
        let ctx = self.roll_input(g)?;
        let v = self.roll.view(&ctx);
        let n = self
            .roll
            .selected_notes(&ctx)
            .into_iter()
            .min_by_key(|n| (n.start, n.pitch))?;
        let at = (
            (v.x(n.start).max(ctx.body.x) + v.x(n.start + n.length).min(ctx.body.right())) / 2.0,
            v.y(n.pitch) + v.row_h / 2.0,
        );
        ctx.body.contains(at.0, at.1).then_some(at)
    }

    /// The middle of the selected track's name in the mixer, when it shows.
    fn key_context_strip(&self, g: &Geometry) -> Option<(f32, f32)> {
        let Some(crate::tree::RowKind::Track(t)) = self.selected_row else {
            return None;
        };
        let r = self
            .mixer_strips(g)
            .into_iter()
            .find(|s| s.track == t)?
            .name;
        let at = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        g.drawer_body.contains(at.0, at.1).then_some(at)
    }

    /// The song position under a window x, on the grid.
    pub(crate) fn ctx_tick(&self, g: &Geometry, x: f32) -> Tick {
        self.snap_tick(self.tick_at(g, x).max(0), false)
    }
}

#[cfg(test)]
mod auto_tests;
#[cfg(test)]
mod browser_names_tests;
#[cfg(test)]
mod browser_tests;
#[cfg(test)]
mod clip_tests;
#[cfg(test)]
mod drum_tests;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod fields_tests;
#[cfg(test)]
mod finish_tests;
#[cfg(test)]
mod folder_stem_tests;
#[cfg(test)]
mod key_more_tests;
#[cfg(test)]
mod marker_tests;
#[cfg(test)]
mod midi_export_tests;
#[cfg(test)]
mod mixed_tests;
#[cfg(test)]
mod mixer_tests;
#[cfg(test)]
mod names_tests;
#[cfg(test)]
mod next_prev_tests;
#[cfg(test)]
mod preset_tests;
#[cfg(test)]
mod range_frame_tests;
#[cfg(test)]
mod roll_lane_tests;
#[cfg(test)]
mod roll_tests;
#[cfg(test)]
mod ruler_tests;
#[cfg(test)]
mod scene_names_tests;
#[cfg(test)]
mod screen_cmd_tests;
#[cfg(test)]
mod screen_rows_tests;
#[cfg(test)]
mod section_tests;
#[cfg(test)]
mod tempo_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod track_tests;
#[cfg(test)]
mod transpose_lag_tests;
