//! The start screen (UI-350): what the window shows when it opens. A song from the last time
//! does not open by itself; the song a crash interrupted does (PRJ-012).
//!
//! Under the bar with only the logo: 「新規」「開く…」「DAWproject を開く…」 at the left, and
//! 「最近の曲」 at the right, a [`PickList`] with its search, newest first, the sample last with
//! its 「サンプル」 tag. Its top row is chosen at first; Enter or a double press opens it. The
//! first time there is only the sample: no search, and 新規 has the keys. A window whose content
//! is narrower than [`NARROW`] stacks it all in one column.
//!
//! Cancelling a song's opening comes back here with its row still chosen: the screen stays
//! until a song is open (`song_replaced_with` ends it).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use xirius_design::{FontSize, Layout as L, Radius, Space, Weight};
use xirius_session::recent::RecentSong;

use crate::draw::{Align, Pen};
use crate::pick_list::{PickKey, PickList, PickRow};
use crate::popup::{Action, Cell, Look};
use crate::studio::{FileRequest, Geometry, Hit, Rect, Studio};
use crate::tools::Mods;

/// A window narrower than this: one column, the buttons in a row without their keys.
pub const NARROW: f32 = 520.0;
/// The room over the content, under the bar.
const TOP: f32 = 40.0;
/// The widest the two columns get.
const CONTENT_MAX: f32 = 680.0;
/// The left column (the buttons).
const LEFT_W: f32 = 184.0;
const BUTTON_H: f32 = 32.0;
/// A button's sides.
const BUTTON_PAD: f32 = 10.0;
/// The heading over the list, with the search at its right.
const HEAD_H: f32 = 28.0;
const SEARCH_W: f32 = 180.0;
const SEARCH_H: f32 = 26.0;
/// The buttons over the list in a narrow window.
const NARROW_GAP: f32 = 20.0;
/// The sample's tag.
const TAG_H: f32 = 16.0;
const TAG_PAD: f32 = 6.0;
/// The key map's commands the start screen takes: a new song, opening one, the settings and
/// quitting (UI-350).
pub const KEYS: [&str; 4] = ["project.new", "project.open", "view.settings", "app.quit"];

pub const SAMPLE_TAG: &str = "サンプル";
pub const RECENT: &str = "最近の曲";
/// What the empty search says.
pub const SEARCH: &str = "検索";
/// What a screen reader calls the search (the mock's label).
pub const SEARCH_NAME: &str = "最近の曲を検索";
/// Two presses on the same row this close together open it.
const DOUBLE: Duration = Duration::from_millis(500);

/// A button of the left column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    New,
    Open,
    Dawproject,
}

impl Button {
    pub const ALL: [Button; 3] = [Button::New, Button::Open, Button::Dawproject];

    pub fn label(self) -> &'static str {
        match self {
            Button::New => "新規",
            Button::Open => "開く…",
            Button::Dawproject => "DAWproject を開く…",
        }
    }

    pub fn keys(self) -> &'static str {
        match self {
            Button::New => "Ctrl+N",
            Button::Open => "Ctrl+O",
            Button::Dawproject => "",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Button::New => "plus",
            Button::Open => "folder-open",
            Button::Dawproject => "open",
        }
    }
}

/// What has the keys on the start screen, in the order Tab walks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Button(Button),
    Search,
    List,
}

/// A part of the start screen under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    Button(Button),
    /// 「オーディオデバイスなし」: its menu.
    Device,
    Search,
    /// A row of the list, counting the rows the search lets through.
    Row(usize),
}

/// The start screen's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Home {
    pub pick: PickList,
    pub focus: Focus,
    /// The songs the rows were built from (the list the keeper read last).
    songs: Vec<RecentSong>,
    /// Built at least once.
    built: bool,
    /// The person moved the keys: the first choice (the top row, or 新規) is theirs now.
    touched: bool,
    /// The last press on a row: when, and which.
    click: Option<(Instant, usize)>,
}

impl Default for Home {
    fn default() -> Home {
        Home {
            pick: PickList::of(Vec::new()),
            focus: Focus::Button(Button::New),
            songs: Vec::new(),
            built: false,
            touched: false,
            click: None,
        }
    }
}

impl Home {
    /// Any song to list besides the sample? Without one there is no search.
    pub fn has_songs(&self) -> bool {
        !self.songs.is_empty()
    }

    /// The rows from `songs`: the songs newest first, then the sample. The cursor stays on
    /// the row it was on.
    fn build(&mut self, songs: &[RecentSong], now: u64) {
        let kept = self
            .pick
            .nav()
            .get(self.pick.selected)
            .map(|r| r.act.clone());
        let mut rows: Vec<PickRow> = songs
            .iter()
            .map(|s| {
                let name = if s.draft {
                    format!("{}（保存前）", s.name)
                } else {
                    s.name.clone()
                };
                PickRow::new(name)
                    .hint(crate::popup::ago(now, s.at_ms))
                    .icon("file")
                    .act(Action::Pick(s.dir.to_string_lossy().into_owned()))
            })
            .collect();
        rows.push(
            PickRow::new(crate::song::SAMPLE_NAME)
                .icon("file")
                .act(Action::OpenSample),
        );
        self.songs = songs.to_vec();
        let mut list = PickList::of(rows);
        list.query = std::mem::take(&mut self.pick.query);
        list.marked = std::mem::take(&mut self.pick.marked);
        list.offset = self.pick.offset;
        list.selected = kept
            .flatten()
            .and_then(|act| list.nav().iter().position(|r| r.act.as_ref() == Some(&act)))
            .unwrap_or(0);
        self.pick = list;
        if !self.has_songs() {
            self.pick.query.clear();
            self.pick.marked.clear();
        }
        if !self.touched {
            self.focus = if self.has_songs() {
                Focus::List
            } else {
                Focus::Button(Button::New)
            };
            self.pick.selected = 0;
        }
        self.built = true;
    }

    /// The places Tab walks, in order (no search the first time).
    fn order(&self) -> Vec<Focus> {
        let mut out: Vec<Focus> = Button::ALL.into_iter().map(Focus::Button).collect();
        if self.has_songs() {
            out.push(Focus::Search);
        }
        out.push(Focus::List);
        out
    }

    /// What the cursor's row opens.
    fn chosen(&self) -> Option<Action> {
        self.pick
            .nav()
            .get(self.pick.selected)
            .and_then(|r| r.act.clone())
    }
}

/// Where the start screen's parts are.
#[derive(Clone, Debug, PartialEq)]
pub struct HomeLayout {
    pub buttons: [Rect; 3],
    /// The buttons say their keys (not in a narrow window).
    pub keys: bool,
    pub no_device: Option<Rect>,
    pub heading: Rect,
    /// The list's cells: the rows that show (or what says nothing matches).
    pub cells: Vec<Cell>,
    /// The search box at the heading's right (with songs to search).
    pub search: Option<Rect>,
    /// Each row that shows, by its place among the rows the search lets through.
    pub rows: Vec<(usize, Rect)>,
    /// The window size the list is measured at (its scroll keeps to it).
    pub list_size: (f32, f32),
}

impl HomeLayout {
    pub fn of(home: &Home, g: &Geometry, no_device: bool) -> HomeLayout {
        // At the top under the bar, as the approved mock has it (UI-350).
        let narrow = g.size.0 < NARROW;
        let (top, side, bottom) = if narrow {
            (Space::X4.px(), Space::X3.px(), Space::X4.px())
        } else {
            (TOP, Space::X5.px(), Space::X5.px())
        };
        let z = Rect::new(
            side,
            top,
            (g.size.0 - 2.0 * side).max(0.0),
            (g.size.1 - top - bottom).max(0.0),
        );
        let w = z.w.min(CONTENT_MAX);
        let x0 = z.x + (z.w - w) / 2.0;
        let gap = Space::X1.px();
        let line = |x: f32, y: f32| {
            let lw = crate::start::no_device::width();
            Rect::new(x, y, lw, crate::start::no_device::HEIGHT)
        };

        // The buttons, and the line under them.
        let mut buttons = [Rect::default(); 3];
        let (list_x, list_w, list_y0, device_y);
        if narrow {
            let (mut x, mut y) = (x0, z.y);
            for (i, b) in Button::ALL.into_iter().enumerate() {
                let bw = button_width(b, false);
                if x > x0 && x + bw > x0 + w {
                    x = x0;
                    y += BUTTON_H + gap;
                }
                buttons[i] = Rect::new(x, y, bw.min(w), BUTTON_H);
                x += bw + gap;
            }
            let mut h = y + BUTTON_H;
            device_y = h + gap;
            if no_device {
                h = device_y + crate::start::no_device::HEIGHT;
            }
            list_x = x0;
            list_w = w;
            list_y0 = h + NARROW_GAP;
        } else {
            let mut y = z.y;
            for (i, r) in buttons.iter_mut().enumerate() {
                *r = Rect::new(x0, y, LEFT_W, BUTTON_H);
                // 新規 stands apart from the other two.
                y += BUTTON_H + if i == 0 { gap + Space::X2.px() } else { gap };
            }
            device_y = buttons[2].bottom() + Space::X3.px();
            list_x = x0 + LEFT_W + Space::X6.px();
            list_w = (x0 + w - list_x).max(0.0);
            list_y0 = z.y;
        }
        let no_device = no_device.then(|| line(x0, device_y));

        // The heading with the search at its right, and the list under them: as tall as
        // the room left, at most the list's own most.
        let heading = Rect::new(list_x, list_y0, list_w, HEAD_H);
        let search = home.has_songs().then(|| {
            let sx = if narrow {
                list_x + crate::browser::text_width(RECENT, FontSize::Caption) + Space::X3.px()
            } else {
                heading.right() - SEARCH_W
            };
            let sx = sx.max(list_x);
            Rect::new(
                sx,
                heading.y + (HEAD_H - SEARCH_H) / 2.0,
                (heading.right() - sx).max(0.0),
                SEARCH_H,
            )
        });
        let pad = L::POPUP_PAD;
        let row_h = L::MENU_ROW_H;
        let list_top = heading.bottom() + gap;
        let room = (z.bottom() - list_top).max(row_h);
        // `cells` puts its search box over the rows, pads its box and keeps `MAX_H` and the
        // window's edge: the size it is measured at gives the rows the room asked for.
        let list_size = (list_w + 2.0 * pad, room + 12.0 + 2.0 * pad + row_h);
        let cells: Vec<Cell> = crate::pick_list::cells(&home.pick, list_w + 2.0 * pad, list_size)
            .into_iter()
            .filter(|c| !matches!(c.look, Look::Input))
            .map(|mut c| {
                c.rect.x += list_x - pad;
                c.rect.y += list_top - pad - row_h;
                c
            })
            .collect();
        let first = home.pick.offset_at(list_size);
        let rows = cells
            .iter()
            .filter(|c| matches!(c.look, Look::Row))
            .enumerate()
            .map(|(k, c)| (first + k, c.rect))
            .collect();
        HomeLayout {
            buttons,
            keys: !narrow,
            no_device,
            heading,
            cells,
            search,
            rows,
            list_size,
        }
    }

    /// The list's rows (and what shows when nothing matches): where the wheel scrolls it and
    /// what a screen reader calls the list.
    pub fn list(&self) -> Rect {
        let top = self.heading.bottom();
        let bottom = self
            .cells
            .iter()
            .map(|c| c.rect.bottom())
            .fold(top, f32::max);
        Rect::new(self.heading.x, top, self.heading.w, bottom - top)
    }

    pub fn button(&self, b: Button) -> Rect {
        self.buttons[Button::ALL.iter().position(|x| *x == b).unwrap_or(0)]
    }

    pub fn rect(&self, spot: Spot) -> Option<Rect> {
        match spot {
            Spot::Button(b) => Some(self.button(b)),
            Spot::Device => self.no_device,
            Spot::Search => self.search,
            Spot::Row(i) => self.rows.iter().find(|(k, _)| *k == i).map(|(_, r)| *r),
        }
    }

    pub fn at(&self, x: f32, y: f32) -> Option<Spot> {
        Button::ALL
            .into_iter()
            .map(Spot::Button)
            .chain(self.no_device.map(|_| Spot::Device))
            .chain(self.search.map(|_| Spot::Search))
            .chain(self.rows.iter().map(|(i, _)| Spot::Row(*i)))
            .find(|s| self.rect(*s).is_some_and(|r| r.contains(x, y)))
    }
}

/// A button's width in a narrow window: its icon and its name, padded.
fn button_width(b: Button, keys: bool) -> f32 {
    let pad = BUTTON_PAD;
    let mut w = pad
        + L::ICON_SIZE
        + Space::X2.px()
        + crate::browser::text_width(b.label(), FontSize::Small)
        + pad;
    if keys && !b.keys().is_empty() {
        w += Space::X3.px() + crate::browser::text_width(b.keys(), FontSize::Caption);
    }
    w
}

impl Studio {
    /// 「閉じる」 (Ctrl+W, UI-350): the song leaves the way it does for a new one — kept,
    /// saved, and asked about when its plugin state could not be kept — and the start screen
    /// shows once the empty song is in. Nothing to close while the start screen shows.
    pub(crate) fn close_to_home(&mut self) -> bool {
        if self.home_shown() {
            return false;
        }
        self.popup = None;
        if self.request_open(crate::opening::Request::New) {
            self.after_open(|s| {
                s.home = Some(Home::default());
                s.poll_io();
            });
        }
        true
    }

    /// The start screen shows: the window has not opened a song yet (UI-350).
    pub fn home_shown(&self) -> bool {
        self.home.is_some() && self.start_shown()
    }

    pub fn home_layout(&self, g: &Geometry) -> Option<HomeLayout> {
        let home = self.home.as_ref().filter(|_| self.start_shown())?;
        Some(HomeLayout::of(home, g, !self.has_audio_device()))
    }

    pub fn home_at(&self, g: &Geometry, x: f32, y: f32) -> Option<Spot> {
        self.home_layout(g)?.at(x, y)
    }

    pub fn home_rect(&self, g: &Geometry, spot: Spot) -> Option<Rect> {
        self.home_layout(g)?.rect(spot)
    }

    /// A wheel turn on the start screen: over the list it scrolls the rows a row a notch.
    pub(crate) fn home_wheel(&mut self, g: &Geometry, x: f32, y: f32, notch: f32) -> bool {
        let Some(l) = self.home_layout(g) else {
            return false;
        };
        if notch == 0.0 || !l.list().contains(x, y) {
            return false;
        }
        let Some(home) = self.home.as_mut() else {
            return false;
        };
        // A list that showed its chosen row scrolls on from there.
        home.pick.settle(l.list_size);
        let max = home.pick.max_offset(l.list_size);
        let offset = &mut home.pick.offset;
        *offset = if notch < 0.0 {
            (*offset + 1).min(max)
        } else {
            offset.saturating_sub(1)
        };
        true
    }

    /// Once a frame: the rows follow the keeper's list of recent songs.
    pub(crate) fn home_tick(&mut self) -> bool {
        // The start screen lies over an empty song. Once that song gains
        // anything (a drop, an outside edit), the song shows and the start
        // screen is over: undoing back to empty must not bring it back.
        if self.home.is_some() && !self.start_shown() {
            self.home = None;
            return true;
        }
        if !self.home_shown() {
            return false;
        }
        let listed = self.keep.listed().to_vec();
        let Some(home) = self.home.as_mut() else {
            return false;
        };
        if home.built && home.songs == listed {
            return false;
        }
        home.build(&listed, xirius_core::store::now_ms());
        true
    }

    /// A press on the start screen.
    pub fn home_press(&mut self, g: &Geometry, spot: Spot) -> bool {
        self.home_tick();
        match spot {
            Spot::Button(b) => {
                if let Some(home) = self.home.as_mut() {
                    home.focus = Focus::Button(b);
                    home.touched = true;
                }
                self.home_button(b);
            }
            Spot::Device => {
                if let Some(r) = self.home_rect(g, spot) {
                    self.open_popup(crate::popup::PopupKind::NoDevice, r);
                }
            }
            Spot::Search => {
                if let Some(home) = self.home.as_mut() {
                    home.focus = Focus::Search;
                    home.touched = true;
                }
            }
            Spot::Row(i) => {
                let now = Instant::now();
                let Some(home) = self.home.as_mut() else {
                    return false;
                };
                let twice = home
                    .click
                    .is_some_and(|(at, k)| k == i && now.duration_since(at) < DOUBLE);
                home.pick.selected = i;
                home.focus = Focus::List;
                home.touched = true;
                home.click = (!twice).then_some((now, i));
                if twice {
                    self.home_open_chosen();
                }
            }
        }
        true
    }

    fn home_button(&mut self, b: Button) {
        match b {
            Button::New => self.start_leaving(crate::saving::Leave::New),
            Button::Open => self.file_request = Some(FileRequest::Open),
            Button::Dawproject => self.file_request = Some(FileRequest::OpenDawproject),
        }
    }

    /// Opens the cursor's row: its song, or the sample.
    fn home_open_chosen(&mut self) {
        let Some(act) = self.home.as_ref().and_then(Home::chosen) else {
            return;
        };
        match act {
            Action::Pick(dir) => self.start_file_request(FileRequest::Open, &PathBuf::from(dir)),
            Action::OpenSample => self.start_leaving(crate::saving::Leave::Sample),
            _ => {}
        }
    }

    /// The start screen's keys: Tab walks 新規 → 開く… → DAWproject を開く… → 検索 → 一覧,
    /// ↑↓ Home End walk the list, Enter opens or presses, Space presses a button, Esc clears
    /// the search. Letters reach the search through the IME (`pick_typing`). Ctrl's keys go
    /// on to the key map (Ctrl+N, Ctrl+O).
    pub(crate) fn home_key(&mut self, g: &Geometry, key: &str, mods: Mods) -> bool {
        // The window's own asks (the recovery, the unsaved song, a file dialog) keep the keys
        // whatever order the screen hands them out in.
        if !self.home_shown()
            || self.popup.is_some()
            || self.settings.is_some()
            || self.palette.is_some()
            || self.recovery.is_some()
            || self.unsaved.is_some()
            || self.file_request.is_some()
            || self.input_blocked()
            || mods.ctrl
            || mods.alt
        {
            return false;
        }
        self.home_tick();
        let no_device = !self.has_audio_device();
        let Some(home) = self.home.as_mut() else {
            return false;
        };
        let size = HomeLayout::of(home, g, no_device).list_size;
        match key {
            "tab" => {
                let order = home.order();
                let at = order.iter().position(|f| *f == home.focus).unwrap_or(0);
                let n = order.len();
                let next = if mods.shift { at + n - 1 } else { at + 1 } % n;
                home.focus = order[next];
                home.touched = true;
            }
            "enter" => match home.focus {
                Focus::Button(b) => {
                    home.touched = true;
                    self.home_button(b);
                }
                Focus::Search | Focus::List => {
                    if let PickKey::Run(_) = home.pick.key("enter", None, size) {
                        self.home_open_chosen();
                    }
                }
            },
            "space" => match home.focus {
                Focus::Button(b) => self.home_button(b),
                _ => return false,
            },
            "up" | "down" => match home.focus {
                Focus::Button(b) => {
                    let i = Button::ALL.iter().position(|x| *x == b).unwrap_or(0);
                    let j = if key == "down" {
                        (i + 1).min(2)
                    } else {
                        i.saturating_sub(1)
                    };
                    home.focus = Focus::Button(Button::ALL[j]);
                    home.touched = true;
                }
                Focus::Search | Focus::List => {
                    home.pick.key(key, None, size);
                    home.focus = Focus::List;
                    home.touched = true;
                }
            },
            "home" | "end" if matches!(home.focus, Focus::Search | Focus::List) => {
                let n = home.pick.nav().len();
                home.pick.settle(size);
                home.pick.selected = if key == "home" {
                    0
                } else {
                    n.saturating_sub(1)
                };
                crate::pick_list::keep_on_screen(&mut home.pick, size);
                home.focus = Focus::List;
                home.touched = true;
            }
            "backspace" if home.has_songs() && !home.pick.query.is_empty() => {
                home.pick.key("backspace", None, size);
                home.focus = Focus::Search;
            }
            "escape" if !home.pick.query.is_empty() => {
                home.pick.key("escape", None, size);
            }
            _ => return false,
        }
        true
    }

    /// The start screen's list takes what is typed (it has songs to search).
    pub(crate) fn home_typing(&self) -> Option<&PickList> {
        let home = self.home.as_ref().filter(|_| self.home_shown())?;
        (home.has_songs()
            && self.popup.is_none()
            && self.settings.is_none()
            && self.palette.is_none())
        .then_some(&home.pick)
    }

    /// The start screen's search box, as the IME writes in it: typing moves the keys there.
    pub(crate) fn home_typing_mut(&mut self) -> Option<&mut PickList> {
        self.home_typing()?;
        let home = self.home.as_mut()?;
        home.focus = Focus::Search;
        home.touched = true;
        Some(&mut home.pick)
    }

    /// The start screen's search with nothing typed: it waits for keys, nobody is busy in it.
    pub(crate) fn home_typing_idle(&self) -> bool {
        self.home_typing()
            .is_some_and(|p| p.query.is_empty() && p.marked.is_empty())
    }
}

/// The start screen over the body.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let (Some(home), Some(l)) = (studio.home.as_ref(), studio.home_layout(g)) else {
        return;
    };
    let c = pen.c;
    pen.fill(Rect::new(0.0, 0.0, g.size.0, g.size.1), c.paper);
    let hover = match studio.hover {
        Hit::Home(spot) => Some(spot),
        _ => None,
    };
    for b in Button::ALL {
        let r = l.button(b);
        let hot = hover == Some(Spot::Button(b));
        paint_button(pen, b, r, l.keys, hot);
        if home.focus == Focus::Button(b) {
            pen.outline(r, c.ring, Radius::S, 2.0);
        }
    }
    if let Some(r) = l.no_device {
        crate::start::no_device::paint(pen, r, hover == Some(Spot::Device));
    }
    pen.text(
        RECENT,
        FontSize::Caption,
        Weight::Semibold,
        c.ink_strong,
        l.heading,
        Align::Left,
    );
    if let Some(r) = l.search {
        paint_search(pen, home, r);
    }
    let rows = home.pick.visible();
    for cell in &l.cells {
        let hot = match cell.look {
            Look::Row => l
                .rows
                .iter()
                .find(|(_, r)| *r == cell.rect)
                .is_some_and(|(i, _)| hover == Some(Spot::Row(*i))),
            _ => false,
        };
        crate::popup::paint_cell(pen, cell, hot);
    }
    for (i, r) in &l.rows {
        if rows
            .get(*i)
            .is_some_and(|row| row.act == Some(Action::OpenSample))
        {
            paint_tag(pen, *r, crate::song::SAMPLE_NAME);
        }
    }
    if let Some(r) = l.search
        && home.focus == Focus::Search
    {
        pen.outline(r, c.ring, Radius::Clip, 2.0);
    }
    if home.focus == Focus::List
        && let Some((_, r)) = l.rows.iter().find(|(i, _)| *i == home.pick.selected)
    {
        pen.outline(*r, c.ring, Radius::S, 2.0);
    }
}

/// The search at the heading's right: what is typed, or 「検索」, and the caret while it has
/// the keys.
fn paint_search(pen: &mut Pen, home: &Home, r: Rect) {
    let c = pen.c;
    pen.round(r, c.surface_2, Radius::Clip);
    let pad = Space::X2.px();
    let icon = Rect::new(
        r.x + pad,
        r.y + (r.h - L::ICON_SIZE) / 2.0,
        L::ICON_SIZE,
        L::ICON_SIZE,
    );
    pen.icon("search", icon, c.ink);
    let tx = icon.right() + Space::X1.px();
    let text = format!("{}{}", home.pick.query, home.pick.marked);
    let room = Rect::new(tx, r.y, (r.right() - pad - tx).max(0.0), r.h);
    let mut w = 0.0;
    pen.mask(room, |pen| {
        if text.is_empty() {
            pen.text(
                SEARCH,
                FontSize::Small,
                Weight::Regular,
                c.ink_on_toggle,
                room,
                Align::Left,
            );
        } else {
            w = pen.text(
                &text,
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                room,
                Align::Left,
            );
        }
    });
    if home.focus == Focus::Search {
        let caret_h = r.h - 2.0 * Space::X2.px();
        pen.fill(
            Rect::new(
                (tx + w + 1.0).min(room.right()),
                r.y + (r.h - caret_h) / 2.0,
                1.5,
                caret_h,
            ),
            c.ink_strong,
        );
    }
}

/// A button of the left column: 新規 filled, the others on the paper until the pointer is
/// over them.
fn paint_button(pen: &mut Pen, b: Button, r: Rect, keys: bool, hot: bool) {
    let c = pen.c;
    let primary = b == Button::New;
    let (face, ink, soft) = match (primary, hot) {
        (true, false) => (Some(c.fill), c.on_fill, c.on_fill),
        (true, true) => (Some(c.fill_hover), c.on_fill, c.on_fill),
        (false, false) => (None, c.ink_strong, c.ink),
        (false, true) => (Some(c.hover), c.ink_strong, c.ink),
    };
    if let Some(face) = face {
        pen.round(r, face, Radius::S);
    }
    let pad = BUTTON_PAD;
    let icon = Rect::new(
        r.x + pad,
        r.y + (r.h - L::ICON_SIZE) / 2.0,
        L::ICON_SIZE,
        L::ICON_SIZE,
    );
    pen.icon(b.icon(), icon, ink);
    let x = icon.right() + Space::X2.px();
    pen.text(
        b.label(),
        FontSize::Small,
        Weight::Medium,
        ink,
        Rect::new(x, r.y, (r.right() - x).max(0.0), r.h),
        Align::Left,
    );
    if keys && !b.keys().is_empty() {
        pen.text(
            b.keys(),
            FontSize::Caption,
            Weight::Regular,
            soft,
            Rect::new(r.x, r.y, r.w - pad, r.h),
            Align::Right,
        );
    }
}

/// The 「サンプル」 tag after the sample's name in its row.
fn paint_tag(pen: &mut Pen, r: Rect, name: &str) {
    let c = pen.c;
    let pad = Space::X2.px();
    let x =
        r.x + pad + L::ICON_SIZE + pad + pen.measure(name, FontSize::Small, Weight::Medium) + pad;
    let w = pen.measure(SAMPLE_TAG, FontSize::Micro, Weight::Medium) + 2.0 * TAG_PAD;
    if x + w > r.right() - pad {
        return;
    }
    let tag = Rect::new(x, r.y + (r.h - TAG_H) / 2.0, w, TAG_H);
    pen.round(tag, c.surface_3, Radius::Clip);
    pen.text(
        SAMPLE_TAG,
        FontSize::Micro,
        Weight::Medium,
        c.ink,
        tag,
        Align::Center,
    );
}

#[cfg(test)]
mod tests;
