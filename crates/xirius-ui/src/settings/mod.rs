//! The settings screen (SET-001〜007, KEY-026): a modal over the window with the sections down
//! its left side. What is changed there is in effect at once, with no OK and no Cancel, and
//! is not a step of the song's history (`docs/ui-approved.md`, 設定の画面).
//!
//! Every change goes through the same commands as the CLI and the AI (`prefs.set`,
//! `keymap.set`, `mousemap.set` ...), so what a command changes shows here on the next frame.
//!
//! The screen is a list built without a window ([`scene`]): the same list is painted,
//! hit-tested and read by the tests.

pub(crate) mod audio;
pub mod field;
mod keys;
mod macros;
mod mouse;
mod panes;
pub(crate) mod parts;
pub mod scene;
pub mod shots;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_design::{FontSize, Layout, Radius, Weight};
use xirius_prefs::catalog::{self, Cat};
use xirius_prefs::{Chord, Settings};

use crate::draw::{Align, Pen};
use crate::dropdown::{Menu, Pick};
use crate::paint::Colors;
use crate::studio::{Geometry, Rect, Studio};
use crate::tools::Mods;

pub use field::Field;
use scene::{B, Scene};

/// The sections, down the left side, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    General,
    Audio,
    Midi,
    Plugins,
    Edit,
    Keys,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::General,
        Section::Audio,
        Section::Midi,
        Section::Plugins,
        Section::Edit,
        Section::Keys,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::General => "一般",
            Section::Audio => "オーディオ",
            Section::Midi => "MIDI",
            Section::Plugins => "プラグイン",
            Section::Edit => "編集",
            Section::Keys => "ショートカット",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Section::General => "gear",
            Section::Audio => "wave",
            Section::Midi => "midi",
            Section::Plugins => "plug",
            Section::Edit => "pencil",
            Section::Keys => "kbd",
        }
    }
}

/// The two halves of ショートカット.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sub {
    Keyboard,
    Mouse,
}

/// The text fields of the screen.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldId {
    /// A number kept in a preference.
    Pref(String),
    /// The search over the shortcuts, by name.
    Search,
    /// The name of the macro that is open.
    MacroName,
    /// The search over the commands a macro can take.
    MacroPick,
}

/// What a press on a part of the screen does.
#[derive(Clone, Debug, PartialEq)]
pub enum Do {
    Nav(Section),
    Close,
    Sub(Sub),
    /// `prefs.set <key> <value>`.
    Pref(String, String),
    /// Takes a folder out of a list.
    ListRemove(String, String),
    /// Asks for a folder to add to a list.
    AddFolder(String),
    /// Frees a controller's knob or pad (`midimap.unset --target`).
    MidiUnset(String),
    Menu(MenuKind),
    Field(FieldId),
    /// Listens for the input a sound comes in on.
    Detect,
    Input(usize),
    /// Scans for plugins again (`plugin.rescan`).
    Rescan,
    /// Stops the scan that runs, keeping the bundles it opened.
    StopScan,
    /// 「テスト音」: the tone once, left then right (UI-260).
    Tone,
    /// Opens the unusable bundle at the path again.
    Retry(std::path::PathBuf),
    /// Shows only the plugins that cannot be read.
    BadOnly(bool),
    /// The shortcut search: by key (true) or by name.
    KeyMode(bool),
    /// A key of the keyboard: search by it.
    KbKey(String),
    /// A modifier of the keyboard: held for the search.
    KbMod(u8),
    Legend(Cat),
    /// Waits for the key to give a command (`from` is the key it replaces).
    Capture {
        ctx: String,
        cmd: String,
        from: Option<Chord>,
    },
    /// Two commands on one key: the other takes the key this one had before.
    Swap {
        ctx: String,
        cmd: String,
        key: Chord,
    },
    /// Two commands on one key: the other lets it go.
    Unlink {
        ctx: String,
        cmd: String,
        key: Chord,
    },
    Restore {
        ctx: String,
        cmd: String,
    },
    ClearQuery,
    Area(String),
    /// The tool the mouse table is for (the lanes have two: the selecting one and another).
    MouseTool(String),
    Zone(String),
    /// A hold of a tool back to what it does at first: (context, modifier).
    HoldRestore(String, String),
    /// Takes a hold away: (context, modifier).
    HoldRemove(String, String),
    /// Opens or closes the editor of a macro.
    MacroOpen(String),
    /// Makes a macro with a name not used yet.
    MacroNew,
    MacroDelete(String),
    /// Takes a step out of the open macro.
    MacroRemove(usize),
    /// Moves a step down (true) or up.
    MacroMove(usize, bool),
    /// Opens or closes the list of commands to add.
    MacroPick,
    /// Adds a command as the last step of the open macro.
    MacroAdd(String),
    MacroRun(String),
    ToastUndo,
    ToastClose,
    Nothing,
}

/// Which menu a dropdown opens.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuKind {
    Pref(String),
    Preset,
    More,
    MouseCell {
        ctx: String,
        modifier: String,
    },
    /// The modifier of a hold: (context, modifier).
    HoldMod(String, String),
    /// The tool a hold switches to: (context, modifier).
    HoldTo(String, String),
    /// Adds a hold for a tool of the area.
    HoldAdd(String),
}

/// A file the screen asks the window for (the window has the system's file dialogs).
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    Folder(String),
    ImportStudioPro,
    ImportToml,
    Export(String),
}

/// What 「取り消す」 on the toast does.
#[derive(Clone, Debug, PartialEq)]
pub enum Undo {
    Preset(String),
    /// The keymap as it was, in the form `keymap.import` reads.
    Keymap(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub text: String,
    pub undo: Option<Undo>,
    pub since: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenMenu {
    pub kind: MenuKind,
    pub menu: Menu,
    pub hover: Option<usize>,
}

/// How long the toast stays.
pub const TOAST: Duration = Duration::from_secs(5);
/// How long 「入力を検出」 listens.
pub const DETECT: Duration = Duration::from_millis(1600);

/// The open settings screen.
#[derive(Clone, Debug)]
pub struct SettingsUi {
    pub section: Section,
    pub sub: Sub,
    /// How far the body is scrolled down, in px.
    pub scroll: f32,
    pub pointer: Option<(f32, f32)>,
    pub menu: Option<OpenMenu>,
    /// The text field that has the keyboard.
    pub field: Option<Field>,
    /// The shortcut search.
    pub query: String,
    pub key_mode: bool,
    pub q_key: Option<Chord>,
    /// The modifiers held (or pressed on the drawn keyboard) for the search.
    pub held: u8,
    pub clicked_mods: u8,
    /// The macro whose editor is open.
    pub macro_open: Option<String>,
    /// The list of commands to add to it is open.
    pub macro_pick: bool,
    pub legend: Option<Cat>,
    pub capture: Option<(String, String, Option<Chord>)>,
    /// The key a row had before it was given another, for 入れ替え.
    pub prev: HashMap<(String, String), Chord>,
    pub area: String,
    pub tool: String,
    pub zone: String,
    /// Why a hold was not moved: (context, modifier, text).
    pub hold_error: Option<(String, String, String)>,
    /// The names of the parts the controller's knobs are given to, by track id.
    pub parts: HashMap<String, String>,
    pub toast: Option<Toast>,
    pub detect: Option<Instant>,
    /// The input a sound was heard on.
    pub hot_input: Option<usize>,
    pub io: xirius_session::audio_settings::Snapshot,
    /// The test tone sounds, and the output's level, left and right (UI-260).
    pub tone: bool,
    pub out: [f32; 2],
    /// What the plugin page shows; the studio fills it from the scan each frame it is built.
    pub plugins: panes::PluginList,
    pub bad_only: bool,
    pub ask: Option<Ask>,
    /// When the screen opened (the meters move from here).
    pub opened: Instant,
    pub now: Instant,
}

impl SettingsUi {
    pub fn new(section: Section) -> SettingsUi {
        let now = Instant::now();
        SettingsUi {
            section,
            sub: Sub::Keyboard,
            scroll: 0.0,
            pointer: None,
            menu: None,
            field: None,
            query: String::new(),
            key_mode: true,
            q_key: None,
            held: 0,
            clicked_mods: 0,
            macro_open: None,
            macro_pick: false,
            legend: None,
            capture: None,
            prev: HashMap::new(),
            area: "roll".into(),
            tool: "select".into(),
            zone: "note-body".into(),
            hold_error: None,
            parts: HashMap::new(),
            toast: None,
            detect: None,
            hot_input: None,
            io: Default::default(),
            tone: false,
            out: [0.0; 2],
            plugins: panes::PluginList::default(),
            bad_only: false,
            ask: None,
            opened: now,
            now,
        }
    }

    /// The seconds since the screen opened (what the meters move by).
    pub fn t(&self) -> f32 {
        self.now.duration_since(self.opened).as_secs_f32()
    }

    /// Moves the clock to `now`; true when something on the screen changed or moves.
    pub fn step(&mut self, now: Instant) -> bool {
        self.now = now;
        let mut moving = self.section == Section::Audio || self.section == Section::Midi;
        if let Some(t) = &self.toast
            && now.duration_since(t.since) >= TOAST
        {
            self.toast = None;
            moving = true;
        }
        if let Some(at) = self.detect {
            if now.duration_since(at) >= DETECT {
                self.detect = None;
                // The session reports which inputs actually received sound.
            }
            moving = true;
        }
        moving || self.toast.is_some()
    }

    pub fn tell(&mut self, text: String, undo: Option<Undo>) {
        self.toast = Some(Toast {
            text,
            undo,
            since: self.now,
        });
    }

    /// Goes to a section, from the top.
    pub fn go(&mut self, section: Section) {
        self.section = section;
        self.scroll = 0.0;
        self.menu = None;
        self.field = None;
        self.capture = None;
    }
}

// ------------------------------------------------------------------- frame

/// Where the modal sits in a body of `size`.
pub fn frame(size: (f32, f32)) -> Rect {
    let w = 1040f32.min(size.0 - 48.0).max(320.0);
    let h = 700f32.min(size.1 - 48.0).max(240.0);
    Rect::new(
        ((size.0 - w) / 2.0).round(),
        ((size.1 - h) / 2.0).round(),
        w,
        h,
    )
}

pub const NAV_W: f32 = 200.0;
/// The header's height: the title, and the search on ショートカット.
pub const HEAD_H: f32 = 52.0;
/// The body's sides.
pub const PAD_X: f32 = 24.0;

/// The colours the mock gives the modal: its face, the nav and the wells in it.
pub struct Faces {
    pub bg: gpui::Hsla,
    pub nav: gpui::Hsla,
    pub well: gpui::Hsla,
    pub nav_sel: gpui::Hsla,
    pub nav_hover: gpui::Hsla,
}

pub fn faces(c: &Colors) -> Faces {
    match c.theme {
        xirius_design::Theme::Dark => Faces {
            bg: c.surface_1,
            nav: c.paper,
            well: c.surface_2,
            nav_sel: c.sel,
            nav_hover: c.hover,
        },
        xirius_design::Theme::Light => Faces {
            bg: c.paper,
            nav: c.surface_1,
            well: c.surface_2,
            nav_sel: c.surface_3,
            nav_hover: c.surface_2,
        },
    }
}

/// The plugins that cannot be read (the count by プラグイン in the nav).
pub fn bad_count(ui: &SettingsUi) -> usize {
    ui.plugins.bad_count()
}

/// The whole screen as a list, for a body of `size`.
pub fn build(
    ui: &SettingsUi,
    s: &Settings,
    registry: &xirius_core::command::Registry,
    c: &Colors,
    size: (f32, f32),
) -> Scene {
    let screen = Rect::new(0.0, 0.0, size.0, size.1);
    // The pointer is on the menu, not under it.
    let pointer = ui.pointer.filter(|(x, y)| {
        ui.menu
            .as_ref()
            .is_none_or(|m| !m.menu.layout.rect.contains(*x, *y))
    });
    let mut b = B::new(c, screen, pointer);
    let f = frame(size);
    let fc = faces(c);
    // Hit testing reads the last matching item: controls override the panel, and the panel
    // catches its empty space before the veil can close the settings.
    b.item(screen, Do::Close, None);
    b.item(f, Do::Nothing, None);
    b.fill(screen, c.veil);
    b.shadow(f);
    b.round(f, fc.bg, Radius::M);
    b.push_mask(f);
    nav(&mut b, ui, &fc, f);
    let main = Rect::new(f.x + NAV_W, f.y, f.w - NAV_W, f.h);
    header(&mut b, ui, s, main);
    let body = Rect::new(main.x, main.y + HEAD_H, main.w, main.h - HEAD_H);
    b.scene.body = body;
    b.push_mask(body);
    let x = body.x + PAD_X;
    let w = body.w - PAD_X * 2.0;
    let top = body.y - ui.scroll;
    let end = match ui.section {
        Section::General => panes::general(&mut b, s, x, top, w),
        Section::Audio => audio::audio(&mut b, ui, s, x, top, w),
        Section::Midi => audio::midi(&mut b, ui, s, x, top, w),
        Section::Plugins => panes::plugins(&mut b, ui, s, x, top, w),
        Section::Edit => panes::edit(&mut b, ui, s, x, top, w),
        Section::Keys => match ui.sub {
            Sub::Keyboard => keys::keys(&mut b, ui, s, registry, x, top, w),
            Sub::Mouse => mouse::mouse(&mut b, ui, s, x, top, w),
        },
    };
    b.pop_mask();
    b.scene.scroll_max = (end + ui.scroll + 20.0 - body.bottom()).max(0.0);
    if let Some(t) = &ui.toast {
        toast(&mut b, t, f);
    }
    b.pop_mask();
    b.scene.frame = f;
    b.finish()
}

fn nav(b: &mut B, ui: &SettingsUi, fc: &Faces, f: Rect) {
    let c = b.c;
    let r = Rect::new(f.x, f.y, NAV_W, f.h);
    b.fill(r, fc.nav);
    b.item(r, Do::Nothing, None);
    let x = r.x + 8.0;
    let w = NAV_W - 16.0;
    let head = Rect::new(x + 10.0, r.y + 14.0 + 2.0, w - 10.0, 22.0);
    b.text(
        "設定",
        FontSize::Lead,
        Weight::Semibold,
        c.ink_strong,
        head,
        Align::Left,
    );
    let mut y = head.bottom() + 12.0;
    let bad = bad_count(ui);
    for s in Section::ALL {
        let it = Rect::new(x, y, w, 32.0);
        let on = ui.section == s;
        if on {
            b.round(it, fc.nav_sel, Radius::S);
        } else if b.hot(it) {
            b.round(it, fc.nav_hover, Radius::S);
        }
        let ink = if on { c.ink_strong } else { c.ink };
        b.icon(s.icon(), parts::icon_at(it.x + 10.0, it), ink);
        b.text(
            s.label(),
            FontSize::Small,
            if on { Weight::Medium } else { Weight::Regular },
            ink,
            Rect::new(it.x + 36.0, it.y, it.w - 46.0, it.h),
            Align::Left,
        );
        if s == Section::Plugins && bad > 0 {
            let n = bad.to_string();
            let nw = scene::width(&n, FontSize::Micro);
            let nr = Rect::new(it.right() - 10.0 - nw, it.y, nw, it.h);
            b.digits(
                &n,
                FontSize::Micro,
                Weight::Regular,
                c.ink_alert,
                nr,
                Align::Right,
            );
            b.icon(
                "alert",
                Rect::new(nr.x - 14.0, it.y + (it.h - 12.0) / 2.0, 12.0, 12.0),
                c.ink_alert,
            );
        }
        b.item(it, Do::Nav(s), None);
        y += 34.0;
    }
}

fn header(b: &mut B, ui: &SettingsUi, s: &Settings, main: Rect) {
    let c = b.c;
    let h = Rect::new(main.x, main.y, main.w, HEAD_H);
    let mid = h.y + 12.0 + (HEAD_H - 20.0) / 2.0 - 4.0;
    let title = ui.section.label();
    let tr = Rect::new(h.x + PAD_X, mid - 12.0, 200.0, 24.0);
    let tw = b.text(
        title,
        FontSize::Lead,
        Weight::Semibold,
        c.ink_strong,
        tr,
        Align::Left,
    );
    // The close button, at the end.
    let close = Rect::new(
        h.right() - 12.0 - Layout::ICON_BUTTON,
        mid - Layout::ICON_BUTTON / 2.0,
        Layout::ICON_BUTTON,
        Layout::ICON_BUTTON,
    );
    parts::icon_button(b, close, "close", Do::Close, Some("閉じる（Esc）"));
    let mut x = tr.x + tw + 12.0;
    match ui.section {
        Section::Keys => {
            let seg = parts::seg(
                b,
                x,
                mid - 14.0,
                &[
                    (
                        "キーボード",
                        Do::Sub(Sub::Keyboard),
                        ui.sub == Sub::Keyboard,
                    ),
                    ("マウス", Do::Sub(Sub::Mouse), ui.sub == Sub::Mouse),
                ],
                None,
            );
            x = seg.right() + 12.0;
            if ui.sub == Sub::Keyboard {
                keys::header(b, ui, s, x, mid, close.x - 8.0);
            }
        }
        Section::Plugins => {
            if let Some(scan) = &ui.plugins.scan {
                // How far the scan is, in place of 「再スキャン」 (another cannot start), and
                // under the title the bar and the bundle it opens (PLG-059).
                scan_head(b, scan, tr.x, h, close.x - 8.0, mid);
            } else {
                let label = "再スキャン";
                let w = scene::width(label, FontSize::Small) + 16.0 + 6.0 + 20.0;
                let r = Rect::new(close.x - 8.0 - w, mid - 14.0, w, 28.0);
                parts::button(b, r, "sync", label, Do::Rescan);
            }
        }
        _ => {}
    }
}

/// The plugin page's head while a scan runs: 「6 / 12」 at the end of the title's line, and
/// under the title the bar and the bundle it opens, cut at the front to fit.
fn scan_head(b: &mut B, scan: &panes::ScanView, x: f32, h: Rect, right: f32, mid: f32) {
    let c = b.c;
    if scan.total > 0 && !scan.stopping {
        b.digits(
            &format!("{} / {}", scan.done.min(scan.total), scan.total),
            FontSize::Body,
            Weight::Semibold,
            c.ink_strong,
            Rect::new(right - 120.0, mid - 12.0, 120.0, 24.0),
            Align::Right,
        );
    }
    let bar = Rect::new(x, h.y + 42.0, SCAN_BAR_W, 4.0);
    b.round(bar, c.surface_3, Radius::Pill);
    if scan.total > 0 {
        let done = scan.done.min(scan.total) as f32 / scan.total as f32;
        if done > 0.0 {
            b.round(
                Rect::new(bar.x, bar.y, bar.w * done, bar.h),
                c.fill,
                Radius::Pill,
            );
        }
    }
    let fx = bar.right() + 12.0;
    let room = right - fx;
    if !scan.file.is_empty() && room > 40.0 {
        b.text(
            &fit_front(&scan.file, FontSize::Caption, room),
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(fx, bar.y - 6.0, room, 16.0),
            Align::Left,
        );
    }
}

/// The scan's bar under the plugin page's title.
const SCAN_BAR_W: f32 = 180.0;

/// `s` as wide as `w` at most: the front goes, behind 「…」, so a path keeps its file's name.
pub(crate) fn fit_front(s: &str, size: FontSize, w: f32) -> String {
    if scene::width(s, size) <= w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    (1..chars.len())
        .map(|i| format!("…{}", chars[i..].iter().collect::<String>()))
        .find(|t| scene::width(t, size) <= w)
        .unwrap_or_else(|| "…".to_string())
}

/// `s` as wide as `w` at most: the end goes, behind 「…」.
pub(crate) fn fit_back(s: &str, size: FontSize, w: f32) -> String {
    if scene::width(s, size) <= w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    (1..chars.len())
        .rev()
        .map(|i| format!("{}…", chars[..i].iter().collect::<String>()))
        .find(|t| scene::width(t, size) <= w)
        .unwrap_or_else(|| "…".to_string())
}

fn toast(b: &mut B, t: &Toast, f: Rect) {
    let c = b.c;
    let tw = scene::width(&t.text, FontSize::Small);
    let undo_w = if t.undo.is_some() {
        scene::width("取り消す", FontSize::Small) + 16.0 + 4.0 + 16.0
    } else {
        0.0
    };
    let w = 12.0 + tw + 10.0 + undo_w + if undo_w > 0.0 { 10.0 } else { 0.0 } + 24.0 + 6.0;
    let r = Rect::new(f.x + (f.w - w) / 2.0, f.bottom() - 16.0 - 36.0, w, 36.0);
    b.shadow(r);
    b.round(r, c.surface_3, Radius::S);
    b.item(r, Do::Nothing, None);
    b.text(
        &t.text,
        FontSize::Small,
        Weight::Regular,
        c.ink_strong,
        Rect::new(r.x + 12.0, r.y, tw + 2.0, r.h),
        Align::Left,
    );
    let mut x = r.x + 12.0 + tw + 10.0;
    if t.undo.is_some() {
        let u = Rect::new(x, r.y + 4.0, undo_w, 28.0);
        parts::button(b, u, "undo", "取り消す", Do::ToastUndo);
        x = u.right() + 10.0;
    }
    parts::icon_button(
        b,
        Rect::new(x, r.y + 6.0, 24.0, 24.0),
        "close",
        Do::ToastClose,
        None,
    );
}

/// Paints the screen and what floats on it (a menu, a tip).
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let Some(ui) = &studio.settings else {
        return;
    };
    let scene = studio.settings_scene_with(g, pen.c);
    scene::paint(pen, &scene.ops);
    if let Some(m) = &ui.menu {
        crate::dropdown::paint_menu(pen, &m.menu, m.hover);
    } else if let Some((r, tip)) = &scene.tip {
        parts::paint_tip(pen, *r, tip, g.size);
    }
}

// ------------------------------------------------------------ the studio

/// The theme `ui.theme` asks for; `auto` follows `system`.
pub fn theme(s: &Settings, system: xirius_design::Theme) -> xirius_design::Theme {
    match s.prefs.text("ui.theme").as_str() {
        "light" => xirius_design::Theme::Light,
        "dark" => xirius_design::Theme::Dark,
        _ => system,
    }
}

/// Looks for `name` among the menu's picks.
fn menu_value(m: &OpenMenu, i: usize) -> Option<String> {
    m.menu.value(i).map(str::to_string)
}

fn bits(m: Mods) -> u8 {
    let mut b = 0;
    if m.ctrl {
        b |= xirius_prefs::chord::MOD;
    }
    if m.alt {
        b |= xirius_prefs::chord::ALT;
    }
    if m.shift {
        b |= xirius_prefs::chord::SHIFT;
    }
    b
}

impl Studio {
    /// Opens the settings on `section` (the rail's gear, Ctrl+, and the palette open 一般).
    pub fn open_settings(&mut self, section: Section) {
        self.popup = None;
        self.palette = None;
        self.error = None;
        self.browser.typing = false;
        self.poll_io();
        // The plugin page reads the scan's list; it is read from the cache once.
        self.plugin_catalog();
        match &mut self.settings {
            Some(ui) => ui.go(section),
            None => self.settings = Some(SettingsUi::new(section)),
        }
    }

    /// The plugin page as the last scan and the one that runs stand.
    fn plugin_page(&self) -> panes::PluginList {
        let scan = &self.mixer.scan;
        let mut list = match &self.mixer.catalog {
            Some(c) => panes::PluginList::of(c),
            None => panes::PluginList::default(),
        };
        list.scan = scan.running().then(|| panes::ScanView {
            done: scan.done,
            total: scan.total,
            stopping: scan.stopping,
            file: scan.file.clone(),
        });
        list.retrying = self.browser.retrying.clone();
        list.error = scan.error.clone();
        list.spin = self.browser.spin;
        list
    }

    pub fn close_settings(&mut self) -> bool {
        self.settings.take().is_some()
    }

    /// The screen as a list, as it stands (the colours do not move a part).
    pub fn settings_scene(&self, g: &Geometry) -> Scene {
        let theme = self.with_settings(|s| theme(s, crate::theme::THEME));
        self.settings_scene_with(g, &Colors::new(theme))
    }

    /// The screen as a list, in the colours `c`.
    pub fn settings_scene_with(&self, g: &Geometry, c: &Colors) -> Scene {
        let Some(ui) = &self.settings else {
            return Scene::default();
        };
        let shared = self.song.settings().clone();
        let s = shared.lock().unwrap_or_else(|e| e.into_inner());
        let mut ui = ui.clone();
        ui.io = self
            .song
            .io
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .view
            .clone();
        ui.parts = self
            .project()
            .tracks
            .iter()
            .map(|(id, t)| (id.to_string(), t.name.clone()))
            .collect();
        ui.plugins = self.plugin_page();
        ui.tone = self.rig_tone;
        ui.out = self.rig_out;
        build(&ui, &s, self.song.registry(), c, g.size)
    }

    /// The theme the preference asks for, `system` being the system's (SET-002).
    pub fn wanted_theme(&self, system: xirius_design::Theme) -> xirius_design::Theme {
        self.with_settings(|s| theme(s, system))
    }

    /// Runs one settings command; a refusal is said on the toast.
    fn settings_run(&mut self, command: &str, args: Value) -> Option<String> {
        if self.transport.recording
            && matches!(
                args["key"].as_str(),
                Some("audio.driver" | "audio.device" | "audio.buffer")
            )
        {
            if let Some(ui) = &mut self.settings {
                ui.tell("録音中はオーディオデバイスを変更できません".into(), None);
            }
            return None;
        }
        let command = if command == "prefs.set" {
            match args["key"].as_str().unwrap_or("").split('.').next() {
                Some("audio") => "audio.set",
                Some("midi") => "midi.set",
                _ => command,
            }
        } else {
            command
        };
        match self.song.set(command, args) {
            Ok(m) => Some(m),
            Err(e) => {
                if let Some(ui) = &mut self.settings {
                    ui.tell(e, None);
                }
                None
            }
        }
    }

    fn with_settings<T>(&self, f: impl FnOnce(&Settings) -> T) -> T {
        let shared = self.song.settings().clone();
        let s = shared.lock().unwrap_or_else(|e| e.into_inner());
        f(&s)
    }

    /// A press while the settings are open. True when the screen changed.
    pub fn settings_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(ui) = &mut self.settings else {
            return false;
        };
        ui.pointer = Some((x, y));
        if let Some(m) = ui.menu.take() {
            if m.menu.layout.rect.contains(x, y) {
                if let Some(v) = m.menu.at(x, y).and_then(|i| menu_value(&m, i)) {
                    self.settings_pick(&m.kind, &v);
                } else {
                    // A heading or a dim row: the menu stays.
                    if let Some(ui) = &mut self.settings {
                        ui.menu = Some(m);
                    }
                }
            }
            return true;
        }
        let scene = self.settings_scene(g);
        let Some(item) = scene.item_at(x, y).cloned() else {
            return false;
        };
        if let Some(ui) = &mut self.settings {
            // A press elsewhere puts in what was typed.
            // (Adding a command keeps the list of commands to search in.)
            let picking = matches!(item.act, Do::MacroAdd(_))
                && ui
                    .field
                    .as_ref()
                    .is_some_and(|f| f.id == FieldId::MacroPick);
            if ui.field.is_some() && !matches!(item.act, Do::Field(_)) && !picking {
                self.settings_commit_field();
            }
        }
        self.settings_do(g, item.act, item.r);
        true
    }

    /// The pointer moved. True when the screen should be painted again.
    pub fn settings_hover(&mut self, x: f32, y: f32) -> bool {
        let Some(ui) = &mut self.settings else {
            return false;
        };
        ui.pointer = Some((x, y));
        if let Some(m) = &mut ui.menu {
            let h = m.menu.at(x, y);
            if h != m.hover {
                m.hover = h;
            }
        }
        true
    }

    pub fn settings_wheel(&mut self, g: &Geometry, x: f32, y: f32, dy: f32) -> bool {
        let scene = self.settings_scene(g);
        let Some(ui) = &mut self.settings else {
            return false;
        };
        if ui.menu.is_some() {
            return true;
        }
        if scene.body.contains(x, y) {
            ui.scroll = (ui.scroll - dy).clamp(0.0, scene.scroll_max);
        }
        true
    }

    /// Does what a part of the screen does; `at` is where it was pressed (a menu opens there).
    pub fn settings_do(&mut self, g: &Geometry, act: Do, at: Rect) {
        let Some(ui) = &mut self.settings else {
            return;
        };
        match act {
            Do::Nav(s) => ui.go(s),
            Do::Close => {
                self.settings = None;
            }
            Do::Sub(s) => {
                ui.sub = s;
                ui.scroll = 0.0;
                ui.capture = None;
                ui.field = None;
            }
            Do::Pref(key, value) => {
                self.settings_run("prefs.set", json!({"key": key, "value": value}));
                // What the pointer shows follows the switch at once, with no press in between.
                self.apply_edit_prefs();
            }
            Do::ListRemove(key, item) => {
                self.settings_run(
                    "prefs.set",
                    json!({"key": key, "value": item, "remove": true}),
                );
            }
            Do::AddFolder(key) => ui.ask = Some(Ask::Folder(key)),
            Do::MidiUnset(target) => {
                self.settings_run("midimap.unset", json!({"target": target}));
            }
            Do::Menu(kind) => {
                let menu = self.settings_menu(g, &kind, at);
                if let Some(ui) = &mut self.settings {
                    ui.menu = menu.map(|menu| OpenMenu {
                        kind,
                        menu,
                        hover: None,
                    });
                }
            }
            Do::Field(id) => {
                let text = match &id {
                    FieldId::Pref(key) => self.with_settings(|s| {
                        let n = s.prefs.number(key);
                        parts::number_text(n)
                    }),
                    FieldId::Search => ui.query.clone(),
                    FieldId::MacroName => ui.macro_open.clone().unwrap_or_default(),
                    FieldId::MacroPick => String::new(),
                };
                if let Some(ui) = &mut self.settings {
                    if id == FieldId::Search {
                        ui.key_mode = false;
                    }
                    ui.field = Some(Field::new(id, text));
                }
            }
            Do::Detect => {
                ui.detect = Some(ui.now);
                ui.hot_input = None;
                self.settings_run("audio.input.detect", json!({"async": true}));
            }
            Do::Input(n) => {
                self.settings_run("audio.input.select", json!({"channel": n + 1}));
            }
            Do::Rescan => {
                ui.plugins.error = None;
                if let Err(e) = self.start_plugin_scan() {
                    self.error = Some(e);
                }
            }
            Do::StopScan => self.stop_plugin_scan(),
            Do::Tone => {
                self.play_test_tone();
                // The bar under the modal is not seen: a tone that did not start is said
                // in a toast instead.
                if let Some(e) = self.error.take()
                    && let Some(ui) = &mut self.settings
                {
                    ui.tell(e, None);
                }
            }
            // The one bundle is opened again; the rest of the list stays (`plugin.rescan --path`).
            Do::Retry(path) => {
                if let Some(at) = self.browser.bad.iter().position(|b| b.path == path)
                    && let Err(e) = self.retry_plugin(at)
                {
                    self.error = Some(e);
                }
            }
            Do::BadOnly(on) => ui.bad_only = on,
            Do::KeyMode(on) => {
                ui.key_mode = on;
                ui.query.clear();
                ui.q_key = None;
                ui.field = if on {
                    None
                } else {
                    Some(Field::new(FieldId::Search, String::new()))
                };
            }
            Do::KbKey(name) => {
                let k = Chord::new(ui.held | ui.clicked_mods, &name);
                ui.key_mode = true;
                ui.field = None;
                ui.query.clear();
                ui.q_key = if ui.q_key.as_ref() == Some(&k) {
                    None
                } else {
                    Some(k)
                };
            }
            Do::KbMod(bit) => {
                ui.clicked_mods ^= bit;
                if let Some(k) = &mut ui.q_key {
                    k.mods = ui.clicked_mods | ui.held;
                }
            }
            Do::Legend(cat) => {
                ui.legend = if ui.legend == Some(cat) {
                    None
                } else {
                    Some(cat)
                };
            }
            Do::Capture { ctx, cmd, from } => {
                ui.field = None;
                ui.capture = Some((ctx, cmd, from));
            }
            Do::Swap { ctx, cmd, key } => {
                let from = ui.prev.get(&(ctx.clone(), cmd.clone())).cloned();
                let args = match from {
                    Some(f) if f != key => json!({"key": key.to_string(), "command": cmd,
                        "context": ctx, "from": f.to_string(), "swap": true}),
                    _ => json!({"key": key.to_string(), "command": cmd, "context": ctx,
                        "replace": true}),
                };
                self.settings_run("keymap.set", args);
            }
            Do::Unlink { ctx, cmd, key } => {
                self.settings_run(
                    "keymap.set",
                    json!({"key": key.to_string(), "command": cmd, "context": ctx,
                        "replace": true}),
                );
            }
            Do::Restore { ctx, cmd } => {
                ui.prev.remove(&(ctx.clone(), cmd.clone()));
                self.settings_run("keymap.reset", json!({"context": ctx, "command": cmd}));
            }
            Do::ClearQuery => {
                ui.query.clear();
                ui.q_key = None;
                ui.clicked_mods = 0;
                if let Some(f) = &mut ui.field {
                    f.set(String::new());
                }
            }
            Do::Area(a) => {
                ui.hold_error = None;
                ui.zone = mouse::first_zone(&a).to_string();
                ui.tool = mouse::table_tools(&a)
                    .first()
                    .map_or("select", |t| t.name)
                    .to_string();
                ui.area = a;
            }
            Do::Zone(z) => ui.zone = z,
            Do::MouseTool(t) => ui.tool = t,
            Do::HoldRestore(ctx, modifier) => {
                ui.hold_error = None;
                self.settings_run(
                    "mousemap.reset",
                    json!({"context": ctx, "modifier": modifier}),
                );
            }
            Do::HoldRemove(ctx, modifier) => {
                ui.hold_error = None;
                self.settings_run(
                    "mousemap.unset",
                    json!({"context": ctx, "modifier": modifier}),
                );
            }
            Do::MacroOpen(name) => {
                ui.field = None;
                ui.macro_pick = false;
                ui.macro_open = if ui.macro_open.as_ref() == Some(&name) {
                    None
                } else {
                    Some(name)
                };
            }
            Do::MacroNew => {
                let name = self.with_settings(|s| {
                    (1..)
                        .map(|n| format!("マクロ {n}"))
                        .find(|n| s.macros.get(n).is_none())
                        .unwrap_or_default()
                });
                if self
                    .settings_run("macro.set", json!({"name": name, "steps": []}))
                    .is_some()
                    && let Some(ui) = &mut self.settings
                {
                    ui.macro_open = Some(name.clone());
                    ui.macro_pick = false;
                    ui.field = Some(Field::new(FieldId::MacroName, name));
                }
            }
            Do::MacroDelete(name) => {
                if self
                    .settings_run("macro.remove", json!({"name": name}))
                    .is_some()
                    && let Some(ui) = &mut self.settings
                {
                    ui.macro_open = None;
                    ui.macro_pick = false;
                    ui.field = None;
                }
            }
            Do::MacroRemove(i) => self.settings_macro_steps(|v| {
                if i < v.len() {
                    v.remove(i);
                }
            }),
            Do::MacroMove(i, down) => self.settings_macro_steps(|v| {
                let j = if down { i + 1 } else { i.wrapping_sub(1) };
                if i < v.len() && j < v.len() {
                    v.swap(i, j);
                }
            }),
            Do::MacroPick => {
                ui.macro_pick = !ui.macro_pick;
                ui.field = ui
                    .macro_pick
                    .then(|| Field::new(FieldId::MacroPick, String::new()));
            }
            Do::MacroAdd(command) => {
                self.settings_macro_steps(|v| v.push(command));
                if let Some(f) = self.settings.as_mut().and_then(|ui| ui.field.as_mut())
                    && f.id == FieldId::MacroPick
                {
                    f.set(String::new());
                }
            }
            Do::MacroRun(name) => {
                self.run_macro(g, &name);
                let said = match self.error.take() {
                    Some(e) => e,
                    None => format!("マクロ「{name}」を実行しました"),
                };
                if let Some(ui) = &mut self.settings {
                    ui.tell(said, None);
                }
            }
            Do::ToastUndo => {
                let undo = ui.toast.take().and_then(|t| t.undo);
                match undo {
                    Some(Undo::Preset(name)) => {
                        self.settings_run("keymap.preset", json!({"name": name}));
                    }
                    Some(Undo::Keymap(toml)) => self.settings_import_text(&toml),
                    None => {}
                }
            }
            Do::ToastClose => ui.toast = None,
            Do::Nothing => {}
        }
    }

    /// Puts a whole keymap back from its text (取り消す after 「すべてプリセットに戻す」).
    fn settings_import_text(&mut self, toml: &str) {
        let dir = std::env::temp_dir().join(format!("xirius-keymap-{}", std::process::id()));
        let path = dir.join("keymap.toml");
        let wrote = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, toml));
        if wrote.is_ok() {
            self.settings_run(
                "keymap.import",
                json!({"source": "toml", "path": path.to_string_lossy()}),
            );
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    /// The menu a dropdown opens, under `at`.
    fn settings_menu(&self, g: &Geometry, kind: &MenuKind, at: Rect) -> Option<Menu> {
        let bounds = Rect::new(0.0, 0.0, g.size.0, g.size.1);
        let plain = |choices: Vec<(String, String)>, current: &str| {
            let widest = choices
                .iter()
                .map(|(_, l)| scene::width(l, FontSize::Small))
                .fold(0.0, f32::max);
            let rows: Vec<(String, String, bool)> =
                choices.into_iter().map(|(v, l)| (v, l, true)).collect();
            Menu::new(at, &rows, current, widest, bounds)
        };
        self.with_settings(|s| match kind {
            MenuKind::Pref(key) => {
                let choices = if key == "audio.device" {
                    self.song
                        .io
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .view
                        .devices
                        .iter()
                        .map(|d| (d.name.clone(), d.name.clone()))
                        .collect()
                } else {
                    panes::choices(key)
                };
                Some(plain(choices, &s.prefs.text(key)))
            }
            MenuKind::MouseCell { ctx, modifier } => {
                let current = s.mousemap.get(ctx, modifier);
                Some(plain(mouse::choices(ctx), &current))
            }
            MenuKind::HoldMod(_, modifier) => {
                let rows: Vec<(String, String)> = xirius_prefs::mousemap::HOLD_MODS
                    .iter()
                    .map(|m| (m.to_string(), mouse::mods_label(m)))
                    .collect();
                Some(plain(rows, modifier))
            }
            MenuKind::HoldAdd(area) => {
                let rows: Vec<(String, String)> = mouse::addable_tools(s, area)
                    .into_iter()
                    .map(|t| (t.name.to_string(), t.label.to_string()))
                    .collect();
                Some(plain(rows, ""))
            }
            MenuKind::HoldTo(ctx, modifier) => {
                let tool = ctx.split('.').nth(1).unwrap_or("");
                let rows: Vec<(String, String)> = xirius_prefs::mousemap::TOOLS
                    .iter()
                    .filter(|t| t.name != tool)
                    .map(|t| (format!("tool:{}", t.name), t.label.to_string()))
                    .collect();
                Some(plain(rows, &s.mousemap.get(ctx, modifier)))
            }
            MenuKind::Preset => {
                let mut picks = vec![Pick::heading("プリセット")];
                for name in catalog::PRESET_NAMES {
                    let label = catalog::preset(name).map(|p| p.label).unwrap_or(name);
                    let n = if *name == s.keymap.preset {
                        0
                    } else {
                        s.keymap.rows_changed_by(name)
                    };
                    let mut p = Pick::row(name, label).on(*name == s.keymap.preset);
                    if n > 0 {
                        p = p.hint(format!("{n} 件を変更"));
                    }
                    picks.push(p);
                }
                Some(Menu::rich(at, &picks, 232.0, bounds))
            }
            MenuKind::More => {
                let changed = s.keymap.changed_rows() > 0;
                let picks = vec![
                    Pick::heading("読み込み"),
                    Pick::row(
                        "import-studio-pro",
                        "Studio Pro のキーボードショートカット…",
                    )
                    .icon("open"),
                    Pick::row("import-toml", "ファイル…").icon("file"),
                    Pick::heading("書き出し"),
                    Pick::row("export-toml", "TOML").icon("export"),
                    Pick::row("export-json", "JSON").icon("export"),
                    Pick::row("export-html", "一覧（印刷用 HTML）").icon("export"),
                    Pick::gap(),
                    Pick::row("reset-all", "すべてプリセットに戻す")
                        .icon("restore")
                        .enabled(changed),
                ];
                Some(Menu::rich(at, &picks, 260.0, bounds))
            }
        })
    }

    /// A row of an open menu was picked.
    fn settings_pick(&mut self, kind: &MenuKind, v: &str) {
        match kind {
            MenuKind::Pref(key) => {
                self.settings_run("prefs.set", json!({"key": key, "value": v}));
            }
            MenuKind::MouseCell { ctx, modifier } => {
                self.settings_run(
                    "mousemap.set",
                    json!({"context": ctx, "modifier": modifier, "action": v}),
                );
            }
            MenuKind::HoldMod(ctx, modifier) => {
                if v != modifier {
                    let to = self.with_settings(|s| s.mousemap.get(ctx, modifier));
                    let args = json!({"context": ctx, "modifier": v, "action": to,
                        "move-from": modifier});
                    // A modifier the tool already has is refused: the row says why.
                    let err = self.song.set("mousemap.set", args).err();
                    if let Some(ui) = &mut self.settings {
                        ui.hold_error = err.map(|e| (ctx.clone(), modifier.clone(), e));
                    }
                }
            }
            MenuKind::HoldAdd(area) => {
                let added = self.with_settings(|s| mouse::first_free_hold(s, area, v));
                if let Some((ctx, m, to)) = added {
                    self.settings_run(
                        "mousemap.set",
                        json!({"context": ctx, "modifier": m, "action": to}),
                    );
                }
            }
            MenuKind::HoldTo(ctx, modifier) => {
                self.settings_run(
                    "mousemap.set",
                    json!({"context": ctx, "modifier": modifier, "action": v}),
                );
            }
            MenuKind::Preset => {
                let old = self.with_settings(|s| s.keymap.preset.clone());
                if v != old
                    && self
                        .settings_run("keymap.preset", json!({"name": v}))
                        .is_some()
                {
                    let n = self.with_settings(|s| s.keymap.changed_rows());
                    let label = catalog::preset(v).map(|p| p.label).unwrap_or(v);
                    let text = if n > 0 {
                        format!("「{label}」を適用しました（変更済みの {n} 件はそのまま）")
                    } else {
                        format!("「{label}」を適用しました")
                    };
                    if let Some(ui) = &mut self.settings {
                        ui.tell(text, Some(Undo::Preset(old)));
                    }
                }
            }
            MenuKind::More => match v {
                "import-studio-pro" => self.settings_ask(Ask::ImportStudioPro),
                "import-toml" => self.settings_ask(Ask::ImportToml),
                "export-toml" => self.settings_ask(Ask::Export("toml".into())),
                "export-json" => self.settings_ask(Ask::Export("json".into())),
                "export-html" => self.settings_ask(Ask::Export("html".into())),
                "reset-all" => {
                    let (old, n, preset) = self.with_settings(|s| {
                        (
                            s.keymap.to_toml(),
                            s.keymap.changed_rows(),
                            s.keymap.preset.clone(),
                        )
                    });
                    if self
                        .settings_run("keymap.preset", json!({"name": preset, "keep": "none"}))
                        .is_some()
                        && let Some(ui) = &mut self.settings
                    {
                        ui.prev.clear();
                        ui.tell(
                            format!("{n} 件をプリセットに戻しました"),
                            Some(Undo::Keymap(old)),
                        );
                    }
                }
                _ => {}
            },
        }
    }

    fn settings_ask(&mut self, ask: Ask) {
        if let Some(ui) = &mut self.settings {
            ui.ask = Some(ask);
        }
    }

    /// The window picked a file or a folder for what the screen asked.
    pub fn settings_answer(&mut self, ask: Ask, path: &std::path::Path) {
        let p = path.to_string_lossy().to_string();
        let said = match ask {
            Ask::Folder(key) => {
                self.settings_run("prefs.set", json!({"key": key, "value": p, "add": true}));
                None
            }
            Ask::ImportStudioPro => {
                self.settings_run("keymap.import", json!({"source": "studio-pro", "path": p}))
            }
            Ask::ImportToml => {
                self.settings_run("keymap.import", json!({"source": "toml", "path": p}))
            }
            Ask::Export(format) => {
                self.settings_run("keymap.export", json!({"format": format, "to": p}))
            }
        };
        if let Some(m) = said
            && let Some(ui) = &mut self.settings
        {
            ui.tell(m, None);
        }
    }

    /// Changes the steps of the open macro with `f` and saves it.
    fn settings_macro_steps(&mut self, f: impl FnOnce(&mut Vec<String>)) {
        let Some(name) = self.settings.as_ref().and_then(|ui| ui.macro_open.clone()) else {
            return;
        };
        let Some(mut steps) = self.with_settings(|s| s.macros.get(&name).map(|m| m.steps.clone()))
        else {
            return;
        };
        f(&mut steps);
        let steps: Vec<Value> = steps.iter().map(|c| json!({"command": c})).collect();
        self.settings_run("macro.set", json!({"name": name, "steps": steps}));
    }

    /// Enter in the list of commands: the first of what is listed is added.
    fn settings_macro_enter(&mut self) {
        let Some(ui) = &self.settings else {
            return;
        };
        let Some(name) = ui.macro_open.clone() else {
            return;
        };
        let query = ui.field.as_ref().map(Field::text).unwrap_or_default();
        let first = self.with_settings(|s| {
            let first_ctx = s
                .macros
                .get(&name)
                .map(|m| xirius_prefs::macro_ext::context_of(&m.steps))
                .unwrap_or("global");
            let cands = xirius_prefs::macro_ext::candidates(self.song.registry());
            macros::pick_list(&cands, &query, first_ctx)
                .first()
                .map(|c| c.command.to_string())
        });
        if let Some(command) = first {
            self.settings_macro_steps(|v| v.push(command));
            if let Some(f) = self.settings.as_mut().and_then(|ui| ui.field.as_mut()) {
                f.set(String::new());
            }
        }
    }

    /// Puts in what a number field holds.
    fn settings_commit_field(&mut self) {
        let Some(ui) = &mut self.settings else {
            return;
        };
        let Some(f) = ui.field.take() else {
            return;
        };
        let text = f.value.trim().to_string();
        match &f.id {
            FieldId::Pref(key) if !text.is_empty() => {
                self.settings_run("prefs.set", json!({"key": key, "value": text}));
            }
            FieldId::MacroName => {
                let from = self.settings.as_ref().and_then(|ui| ui.macro_open.clone());
                if let Some(from) = from
                    && !text.is_empty()
                    && text != from
                    && self
                        .settings_run("macro.rename", json!({"name": from, "new-name": text}))
                        .is_some()
                    && let Some(ui) = &mut self.settings
                {
                    ui.macro_open = Some(text);
                }
            }
            _ => {}
        }
    }

    /// A key while the settings are open (all are theirs), or Ctrl+, to open them.
    pub fn settings_key(&mut self, key: &str, mods: Mods) -> bool {
        let comma = key == "," && mods.ctrl && !mods.alt && !mods.shift;
        let Some(ui) = &mut self.settings else {
            if comma {
                self.open_settings(Section::General);
                return true;
            }
            return false;
        };
        // A key for a command.
        if let Some((ctx, cmd, from)) = ui.capture.clone() {
            if key == "escape" {
                ui.capture = None;
                return true;
            }
            if parts::is_modifier(key) {
                return true;
            }
            let Some(chord) = Chord::from_event(key, bits(mods)) else {
                return true;
            };
            ui.capture = None;
            if let Some(f) = &from {
                ui.prev.insert((ctx.clone(), cmd.clone()), f.clone());
            } else {
                let before = self.with_settings(|s| s.keymap.keys_of(&ctx, &cmd));
                if let (Some(ui), Some(k)) = (&mut self.settings, before.first()) {
                    ui.prev.insert((ctx.clone(), cmd.clone()), k.clone());
                }
            }
            let mut args = json!({"key": chord.to_string(), "command": cmd, "context": ctx});
            if let Some(f) = from {
                args["from"] = Value::String(f.to_string());
            }
            self.settings_run("keymap.set", args);
            return true;
        }
        if let Some(m) = &mut ui.menu {
            match key {
                "escape" => ui.menu = None,
                "up" | "down" => {
                    let n = m.menu.layout.cells.len();
                    let live: Vec<usize> =
                        (0..n).filter(|i| m.menu.layout.cells[*i].live()).collect();
                    if !live.is_empty() {
                        let at = m.hover.and_then(|h| live.iter().position(|i| *i == h));
                        let next = match (key, at) {
                            ("down", Some(p)) => (p + 1).min(live.len() - 1),
                            ("up", Some(p)) => p.saturating_sub(1),
                            ("up", None) => live.len() - 1,
                            _ => 0,
                        };
                        m.hover = Some(live[next]);
                    }
                }
                "enter" => {
                    let m = ui.menu.take().expect("open");
                    if let Some(v) = m.hover.and_then(|i| menu_value(&m, i)) {
                        self.settings_pick(&m.kind, &v);
                    }
                }
                _ => {}
            }
            return true;
        }
        if let Some(f) = &mut ui.field {
            if !f.marked.is_empty() {
                return true;
            }
            match key {
                "escape" => {
                    if f.id == FieldId::Search && !f.text().is_empty() {
                        f.set(String::new());
                        ui.query.clear();
                    } else {
                        ui.field = None;
                    }
                }
                "enter" if f.id == FieldId::MacroPick => self.settings_macro_enter(),
                "enter" | "tab" => self.settings_commit_field(),
                "backspace" => {
                    f.backspace();
                    if f.id == FieldId::Search {
                        ui.query = f.text();
                    }
                }
                _ => {}
            }
            return true;
        }
        if ui.section == Section::Audio && matches!(key, "up" | "down" | "left" | "right") {
            let io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
            let n = io
                .view
                .devices
                .iter()
                .find(|d| d.name == io.view.device)
                .map_or(0, |d| d.inputs.len());
            let current = io.view.selected.unwrap_or(0);
            let next = if key == "up" || key == "left" {
                current.saturating_sub(1)
            } else {
                (current + 1).min(n.saturating_sub(1))
            };
            drop(io);
            if n > 0 {
                self.settings_run("audio.input.select", json!({"channel": next + 1}));
            }
            return true;
        }
        // The PC keys play on the MIDI page, and its first row shows what they played (UI-263).
        if ui.section == Section::Midi
            && !mods.ctrl
            && !mods.alt
            && !mods.shift
            && self.settings_pc_key(key)
        {
            return true;
        }
        let Some(ui) = &mut self.settings else {
            return true;
        };
        // The search by key: the key pressed is looked for.
        if ui.section == Section::Keys && ui.sub == Sub::Keyboard && ui.key_mode {
            if key == "escape" && ui.q_key.is_some() {
                ui.q_key = None;
                return true;
            }
            if key != "escape"
                && !parts::is_modifier(key)
                && !comma
                && let Some(chord) = Chord::from_event(key, bits(mods) | ui.clicked_mods)
            {
                ui.q_key = Some(chord);
                return true;
            }
        }
        if key == "escape" || comma {
            self.settings = None;
            return true;
        }
        true
    }

    /// The IME put text in the field (`marked`: it is still composing).
    pub fn settings_text(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        marked: bool,
    ) {
        let Some(ui) = &mut self.settings else {
            return;
        };
        let Some(f) = &mut ui.field else {
            return;
        };
        if marked {
            f.mark_text(range, text);
        } else {
            f.replace_text(range, text);
        }
        if f.id == FieldId::Search {
            ui.query = f.text();
        }
    }

    pub fn settings_unmark(&mut self) {
        if let Some(f) = self.settings.as_mut().and_then(|ui| ui.field.as_mut()) {
            f.unmark();
        }
    }

    /// The modifiers held changed (the drawn keyboard shows them).
    pub fn settings_mods(&mut self, mods: Mods) -> bool {
        let Some(ui) = &mut self.settings else {
            return false;
        };
        let b = bits(mods);
        if b == ui.held {
            return false;
        }
        ui.held = b;
        true
    }

    /// The clock moved (a frame or the idle poll); true when the screen moves.
    pub fn settings_tick(&mut self) -> bool {
        self.poll_io();
        match &mut self.settings {
            Some(ui) => ui.step(Instant::now()),
            None => false,
        }
    }

    /// Reads the settings files again when another program (the CLI) wrote them.
    /// True when the screen shows something new.
    pub fn settings_reload(&mut self) -> bool {
        let shared = self.song.settings().clone();
        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
        let changed = s.reload_if_changed();
        drop(s);
        self.apply_edit_prefs() || changed
    }

    /// What the window has to ask for (a folder, a file to read or write).
    pub fn settings_take_ask(&mut self) -> Option<Ask> {
        self.settings.as_mut()?.ask.take()
    }
}
