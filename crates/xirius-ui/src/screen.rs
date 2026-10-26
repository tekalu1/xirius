//! The real screen: the top bar that is also the title bar, the track column,
//! the arrangement and the drawer.
//!
//! The pictures of the three regions are [`crate::draw`]; what is where and
//! what a press does is [`crate::studio`]. This file wires them to GPUI: it
//! paints the top bar, turns mouse, wheel and key events into
//! calls on the [`Studio`], and runs the window (and, with `--shot`, the
//! screenshot run of [`crate::shot`]).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Bounds, Context, DragMoveEvent, ElementInputHandler, EntityInputHandler, ExternalPaths,
    FocusHandle, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    PlatformInput, Point, Render, ScrollDelta, ScrollWheelEvent, TitlebarOptions, TouchPhase,
    UTF16Selection, Window, WindowBounds, WindowControlArea, WindowDecorations, WindowOptions,
    canvas, div, point, prelude::*, px, size, svg,
};
use xirius_design::{FontSize, Layout, Weight};

use xirius_session::output::OutputMode;

use crate::app::Place;
use crate::assets::{Assets, LOGO_PATH};
use crate::draw::{Pen, paint_overlay, paint_roll};
use crate::paint::Colors;
use crate::park::{PARK_AT, park};
use crate::shot::{self, Act, Capture, Outcome, Play};
use crate::studio::{FileRequest, Geometry, Hit, Studio};
use crate::theme::{THEME, ui_font};
use crate::tools::Mods;

pub const WINDOW_TITLE: &str = "Xirius";

/// Wheel "lines" are this many pixels (only for real mouse wheels).
const LINE_PX: f32 = 20.0;
/// One notch of the wheel zooms by one step.
pub(crate) const ZOOM_NOTCH_PX: f32 = 60.0;

pub struct Options {
    /// Logical size of the window.
    pub window: (f32, f32),
    pub place: Place,
    /// Run the screenshot script and write the pictures here.
    pub shots: Option<PathBuf>,
    /// Which script: one of [`crate::SCRIPTS`].
    pub script: &'static [shot::Shot],
    pub roll_bench: Option<PathBuf>,
    /// Measure 64 strips playing in the mixer (UI-024) and write the numbers here.
    pub mixer_bench: Option<PathBuf>,
    pub light: bool,
    /// Where the transport plays to.
    pub output: OutputMode,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            window: (1440.0, 860.0),
            place: Place::Centered,
            shots: None,
            script: shot::SCRIPT,
            roll_bench: None,
            mixer_bench: None,
            light: false,
            output: OutputMode::Device,
        }
    }
}

/// The body's size for a window: everything between the top bar and the bottom bar.
fn body_size(window: &Window) -> (f32, f32) {
    let s = window.viewport_size();
    (
        f32::from(s.width),
        (f32::from(s.height) - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H).max(0.0),
    )
}

pub(crate) fn geometry(studio: &Studio, window: &Window) -> Geometry {
    let (w, h) = body_size(window);
    studio.geometry(w, h)
}

/// A point in the window as a point in the body.
fn in_body(p: Point<gpui::Pixels>) -> (f32, f32) {
    (f32::from(p.x), f32::from(p.y) - Layout::TOP_BAR_H)
}

fn mods(m: Modifiers) -> Mods {
    Mods {
        ctrl: m.control || m.platform,
        shift: m.shift,
        alt: m.alt,
    }
}

struct Screen {
    pending_save_input: Vec<PlatformInput>,
    studio: Rc<RefCell<Studio>>,
    colors: Rc<Colors>,
    focus: FocusHandle,
    background: gpui::Entity<crate::roll::layer::Background>,
    /// The theme a script asked for (`--light`); otherwise `ui.theme` decides (SET-002).
    fixed_theme: Option<xirius_design::Theme>,
    /// This window's handle once the tools' cursors are set up on it (`pointer_os`), and how
    /// many frames have looked for it.
    pointer_window: Option<isize>,
    pointer_looks: u8,
    /// The MCP server an outside AI reaches the song through; it stops with
    /// the window.
    _ai_server: Option<xirius_mcp::background::Background>,
    /// Files from outside are being dragged over the window.
    files_dragged: bool,
    /// Whether Caps Lock was on at the last change of the modifiers this window saw; forgotten
    /// while the window is in the background, so a Caps Lock pressed in another app does not
    /// count (KEY-012).
    last_caps: Option<bool>,
    _activation: Option<gpui::Subscription>,
}

impl Screen {
    /// Before a short save needs a veil, input waits as it did with a synchronous save.
    fn queue_save_input(&mut self, input: PlatformInput) -> bool {
        let studio = self.studio.borrow();
        let Some(save) = &studio.saving else {
            return false;
        };
        let release = match &input {
            PlatformInput::MouseUp(up) => self.pending_save_input.iter().any(|event| matches!(event, PlatformInput::MouseDown(down) if down.button == up.button)),
            PlatformInput::KeyUp(up) => self.pending_save_input.iter().any(|event| matches!(event, PlatformInput::KeyDown(down) if down.keystroke.key == up.keystroke.key)),
            _ => false,
        };
        if save.defers_input() || release {
            self.pending_save_input.push(input);
        }
        true
    }
    fn right_pressed(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let mut s = self.studio.borrow_mut();
        if s.input_blocked() {
            return;
        }
        let g = geometry(&s, window);
        let (x, y) = in_body(ev.position);
        if s.preparing.is_some() || s.recovery.is_some() || s.settings.is_some() {
            return;
        }
        if s.palette.is_some() {
            if s.close_palette() {
                cx.notify();
            }
            return;
        }
        if s.popup.is_none() && s.over_instrument(&g, x, y) {
            return;
        }
        if s.cpu_context(&g, x, y)
            || s.lib_context(&g, x, y)
            || s.scene_context(&g, x, y)
            || (s.roll_input(&g).is_none() && s.context_press(&g, x, y))
        {
            cx.notify();
        }
    }
    fn moved(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let mut studio = self.studio.borrow_mut();
        if ev.pressed_button.is_none() {
            // A release outside the window that did not come here.
            studio.pointer_held = false;
        }
        if studio.opening.is_some() {
            let g = geometry(&studio, window);
            let (x, y) = in_body(ev.position);
            let open = studio.opening.as_mut().unwrap();
            let full = crate::opening_view::name_rect(&g).contains(x, y);
            let cancel = crate::opening_view::cancel_rect(&g).contains(x, y);
            if open.full_name != full || open.cancel_hover != cancel {
                open.full_name = full;
                open.cancel_hover = cancel;
                cx.notify();
            }
            return;
        }
        if studio.saving.is_some() {
            return;
        }
        let g = geometry(&studio, window);
        let (x, y) = in_body(ev.position);
        if studio.preparing.is_some() {
            if studio.prepare_hover(&g, x, y) {
                cx.notify();
            }
            return;
        }
        if studio.recovery.is_some() {
            if studio.recovery_hover(&g, x, y) {
                cx.notify();
            }
            return;
        }
        if studio.settings.is_some() {
            if studio.settings_hover(x, y) {
                cx.notify();
            }
            return;
        }
        if studio.palette.is_some() {
            if studio.palette_hover(&g, x, y) {
                cx.notify();
            }
            return;
        }
        let pointer_tip = crate::hover_tip::pointer_tip(&studio, &g);
        studio.input_map.pointer = (x, y);
        if pointer_tip != crate::hover_tip::pointer_tip(&studio, &g) {
            cx.notify();
        }
        if studio.bar_move(y, ev.modifiers.shift) {
            cx.notify();
            return;
        }
        if studio.zoom_box_move(x, y)
            || studio.mixer_bar_move(&g, x)
            || studio.edge_move(&g, y)
            || studio.side_move(&g, x)
            || studio.pan_move(&g, x, y)
            || studio.ruler_drag_move(&g, x, y)
        {
            cx.notify();
            return;
        }
        if studio.edge_hover(&g, x, y) | studio.side_hover(&g, x, y) {
            cx.notify();
        }
        if studio.mixer.drag.is_some() {
            if studio.drag_move(y, ev.modifiers.shift) {
                cx.notify();
            }
            return;
        }
        if studio.name_drag_move(x, y) || studio.fx_move(x, y) || studio.lib_move(x, y) {
            cx.notify();
            return;
        }
        if studio.instrument_moved(&g, x, y, ev.modifiers.shift) {
            cx.notify();
            return;
        }
        if let Some(ctx) = studio.roll_input(&g) {
            let out = studio.roll.moved(&ctx, x, y, mods(ev.modifiers));
            if studio.apply_roll(&g, out) {
                cx.notify();
            }
        }
        if studio.arrange_move(&g, x, y, mods(ev.modifiers)) {
            cx.notify();
        }
        let hit = studio.hit(&g, x, y);
        if hit != studio.hover {
            studio.hover = hit;
            cx.notify();
        }
    }

    /// The pointer left the window without a last move: nothing is under it,
    /// so the hover goes (a button pressed last would stay lit otherwise).
    fn exited(&mut self, _ev: &MouseExitEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let mut studio = self.studio.borrow_mut();
        if studio.hover != Hit::None {
            studio.hover = Hit::None;
            cx.notify();
        }
    }

    fn pressed(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.studio.borrow_mut().pointer_held = true;
        if self.queue_save_input(PlatformInput::MouseDown(ev.clone())) {
            let mut s = self.studio.borrow_mut();
            let g = geometry(&s, window);
            let (x, y) = in_body(ev.position);
            s.saving_press(&g, x, y);
            cx.notify();
            return;
        }
        window.focus(&self.focus, cx);
        let mut studio = self.studio.borrow_mut();
        let g = geometry(&studio, window);
        let (x, y) = in_body(ev.position);
        if studio.unsaved_press(&g, x, y)
            || studio.opening_press(&g, x, y)
            || studio.saving_press(&g, x, y)
        {
            cx.notify();
            return;
        }
        studio.input_map.browser_focus = studio.browser.open && g.panel.contains(x, y);
        if studio.prepare_press(&g, x, y)
            || studio.recovery_press(&g, x, y)
            || studio.toast_press(&g, x, y)
            || studio.job_stop_press(&g, x, y)
        {
            drop(studio);
            self.files(cx);
            cx.notify();
            return;
        }
        if studio.settings.is_none() && studio.notice_press(&g, x, y) {
            cx.notify();
            return;
        }
        if studio.settings.is_some() {
            if studio.settings_press(&g, x, y) {
                cx.notify();
            }
            drop(studio);
            self.settings_files(cx);
            return;
        }
        if studio.palette.is_some() {
            studio.palette_press(&g, x, y);
            cx.notify();
            drop(studio);
            self.files(cx);
            return;
        }
        if studio.cpu_down(&g, x, y, ev.click_count as u32) {
            cx.notify();
            return;
        }
        if studio.name_press(&g, x, y, ev.click_count as u32) {
            cx.notify();
            return;
        }
        if studio.bar_down(&g, x, y, ev.click_count as u32) {
            cx.notify();
            return;
        }
        if studio.mixer_bar_down(&g, x, y) {
            cx.notify();
            return;
        }
        if studio.zoom_box_down(&g, x, y, mods(ev.modifiers)) {
            cx.notify();
            return;
        }
        if studio.edge_down(&g, x, y, ev.click_count as u32)
            || studio.side_down(&g, x, y, ev.click_count as u32)
        {
            cx.notify();
            return;
        }
        studio.name_drag_down(&g, x, y);
        if studio.chain_down(&g, x, y) {
            cx.notify();
            drop(studio);
            self.files(cx);
            return;
        }
        if studio.popup.is_none() && studio.instrument_down(&g, x, y) {
            cx.notify();
            return;
        }
        studio.ruler_drag_down(&g, x, y, mods(ev.modifiers));
        if studio.roll_press(&g, x, y, mods(ev.modifiers), ev.click_count as u32) {
            cx.notify();
            return;
        }
        if ev.click_count == 2 && studio.arrange_open(&g, x, y, mods(ev.modifiers)) {
            cx.notify();
            return;
        }
        studio.arrange.clicks = ev.click_count as u32;
        if studio.arrange_down(&g, x, y, mods(ev.modifiers)) {
            if ev.click_count == 2 {
                studio.arrange_open(&g, x, y, mods(ev.modifiers));
            }
            cx.notify();
            return;
        }
        let hit = studio.hit(&g, x, y);
        let menu = studio.popup.is_some();
        // A file row is held before its press: the press closes the band under the row
        // selected before, and the rows below it move up.
        let file_row = matches!(
            hit,
            Hit::Browser(
                crate::browser::BrowserHit::File(_)
                    | crate::browser::BrowserHit::Pool(_)
                    | crate::browser::BrowserHit::PoolCount(_)
            )
        );
        let held = !menu && file_row && studio.lib_grab(&g, hit, x, y, ev.click_count);
        let pressed = studio.press(&g, hit);
        let grabbed =
            held || (!menu && !file_row && studio.lib_grab(&g, hit, x, y, ev.click_count));
        if studio.drag_begin(&g, hit, y) || pressed || grabbed {
            cx.notify();
        }
        drop(studio);
        self.files(cx);
    }

    /// The middle button holds the view under it (EDT-068).
    fn middle_pressed(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let mut studio = self.studio.borrow_mut();
        studio.pointer_held = true;
        if studio.input_blocked() {
            return;
        }
        let g = geometry(&studio, window);
        let (x, y) = in_body(ev.position);
        if studio.settings.is_none() && studio.middle_down(&g, x, y) {
            cx.notify();
        }
    }

    fn middle_released(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let mut studio = self.studio.borrow_mut();
        studio.pointer_held = false;
        if studio.input_blocked() {
            return;
        }
        let g = geometry(&studio, window);
        if studio.middle_up(&g) {
            cx.notify();
        }
    }

    fn wheel(&mut self, ev: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.queue_save_input(PlatformInput::ScrollWheel(ev.clone())) {
            return;
        }
        let mut studio = self.studio.borrow_mut();
        let g = geometry(&studio, window);
        let d = ev.delta.pixel_delta(px(LINE_PX));
        if studio.window_wheel(
            &g,
            in_body(ev.position),
            f32::from(d.x),
            f32::from(d.y),
            mods(ev.modifiers),
            ev.modifiers.control,
        ) {
            cx.notify();
        }
    }

    /// 「Xirius について」 from the app's menu (macOS's menu bar; LIC-015).
    fn about_menu(
        &mut self,
        _: &crate::app_menu::AboutXirius,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut studio = self.studio.borrow_mut();
        let g = geometry(&studio, window);
        if studio.about_from_menu(&g) {
            cx.notify();
        }
    }

    fn key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if !matches!(k.key.as_str(), "space" | "escape")
            && self.queue_save_input(PlatformInput::KeyDown(ev.clone()))
        {
            return;
        }
        let blocked_key = {
            let mut s = self.studio.borrow_mut();
            s.unsaved_key(&k.key) || s.opening_key(&k.key) || s.saving_key(&k.key)
        };
        if blocked_key {
            cx.notify();
            return;
        }
        // A veil over a job that stops the person: Esc cancels it before anything else (UI-061).
        if k.key == "escape" && crate::wait::cancel() {
            cx.notify();
            return;
        }
        let native = crate::native_key::take();
        let key = native
            .as_ref()
            .and_then(|key| key.key.as_deref())
            .unwrap_or(&k.key);
        let code = native.as_ref().and_then(|key| key.code.as_deref());
        let character = native
            .as_ref()
            .and_then(|key| key.character.as_deref())
            .or(k.key_char.as_deref());
        let mut studio = self.studio.borrow_mut();
        let g = geometry(&studio, window);
        let m = mods(k.modifiers);
        // The song's instruments are opening: only Esc (後で読む) does anything (ENG-064).
        if studio.prepare_key(&k.key) {
            cx.notify();
            return;
        }
        // The card after a crash waits for an answer before any key does anything.
        if studio.recovery_key(&g, &k.key) {
            cx.notify();
            return;
        }
        // So does 「使われていない録音を整理」 (BRW-004).
        if studio.cleanup_key(&g, &k.key) {
            cx.notify();
            return;
        }
        // Ctrl+V in a list's search box (the settings' too) pastes (UI-338).
        let picking = studio.pick_typing().is_some() && studio.ask_field().is_none();
        if picking && m.ctrl && !m.alt && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                studio.pick_paste(&text);
            }
            cx.notify();
            return;
        }
        // The settings take every key while open, and Ctrl+, opens them
        // (SET-001); their fields' letters come through the IME too.
        let logical = character.filter(|c| {
            c.chars().count() == 1
                && !c
                    .chars()
                    .any(|ch| ch.is_alphanumeric() || ch.is_whitespace() || ch.is_control())
        });
        let capture_mods = if logical.is_some() {
            Mods { shift: false, ..m }
        } else {
            m
        };
        if studio.settings.is_some() && studio.settings_key(logical.unwrap_or(key), capture_mods) {
            cx.notify();
            drop(studio);
            self.settings_files(cx);
            return;
        }
        // The palette's letters do not come with the key: the IME hands
        // them on (`EntityInputHandler` below).
        if m.ctrl
            && k.key == "c"
            && let Some(text) = studio
                .palette
                .as_ref()
                .and_then(|p| p.selected_result_text())
        {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            return;
        }
        if studio.palette.is_some() && studio.palette_key(&g, &k.key, m) {
            cx.notify();
            drop(studio);
            self.files(cx);
            return;
        }
        if studio.field_keys(&g, &k.key, k.key_char.as_deref(), m) {
            cx.notify();
            return;
        }
        // A list's search box (UI-338): its letters come through the IME
        // (`EntityInputHandler` below), so the key brings none; a letter is
        // still the list's, never a note or a shortcut.
        if picking
            && !m.ctrl
            && !m.alt
            && k.key_char
                .as_deref()
                .is_some_and(|c| !c.is_empty() && !c.chars().any(char::is_control))
        {
            cx.notify();
            return;
        }
        let keypad = key != k.key;
        let ch = if picking { None } else { k.key_char.as_deref() };
        let typed = !keypad && !m.ctrl && studio.typed(&g, &k.key, ch);
        let played = !keypad
            && !typed
            && !m.ctrl
            && !m.alt
            && !m.shift
            && studio.pc_key_down(&k.key, ev.is_held);
        if typed || played || studio.mapped_key_code(&g, key, character, m, code) {
            cx.notify();
        }
        drop(studio);
        self.files(cx);
    }

    /// A key came up: a PC key that played a note lets it go (UI-074).
    fn key_up(&mut self, ev: &KeyUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.queue_save_input(PlatformInput::KeyUp(ev.clone())) {
            return;
        }
        let mut studio = self.studio.borrow_mut();
        // Space let go during a drag: the edge scroll goes back to its pace (EDT-046).
        if studio.drag_space(&ev.keystroke.key, false)
            | studio.pc_key_up(&ev.keystroke.key)
            | studio.key_sound_up()
        {
            cx.notify();
        }
    }

    fn released(&mut self, ev: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.studio.borrow_mut().pointer_held = false;
        if self.queue_save_input(PlatformInput::MouseUp(ev.clone())) {
            return;
        }
        let mut studio = self.studio.borrow_mut();
        if studio.input_blocked() {
            return;
        }
        if studio.bar_up() || studio.edge_up() | studio.side_up() {
            cx.notify();
            return;
        }
        if studio.palette.is_some()
            || studio.settings.is_some()
            || studio.recovery.is_some()
            || studio.preparing.is_some()
        {
            return;
        }
        let g = geometry(&studio, window);
        let (x, y) = in_body(ev.position);
        if studio.mixer_bar_up() {
            cx.notify();
            return;
        }
        if studio.zoom_box_up(&g, x, y) {
            cx.notify();
            return;
        }
        studio.ruler_drag_up();
        if ev.modifiers.alt && studio.replace_file_up(&g, x, y) {
            cx.notify();
            return;
        }
        if studio.name_drag_up(&g, x, y)
            || studio.fx_up(&g, x, y, ev.modifiers.alt)
            || studio.lib_up(&g, x, y)
        {
            cx.notify();
            return;
        }
        if studio.instrument_up() {
            cx.notify();
            return;
        }
        if studio.drag_end() {
            cx.notify();
            return;
        }
        if let Some(ctx) = studio.roll_ctx(&g) {
            let out = studio.roll.up(&ctx, x, y, mods(ev.modifiers));
            if studio.apply_roll(&g, out) {
                cx.notify();
            }
        }
        if studio.arrange_up(&g, x, y, mods(ev.modifiers)) {
            cx.notify();
        }
    }

    fn modifiers(
        &mut self,
        ev: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let caps = ev.capslock.on;
        let caps_pressed = self.last_caps.is_some_and(|was| was != caps);
        self.last_caps = Some(caps);
        let mut studio = self.studio.borrow_mut();
        if studio.input_blocked() {
            return;
        }
        if studio.recovery.is_some() || studio.preparing.is_some() {
            return;
        }
        if studio.settings.is_some() {
            if studio.settings_mods(mods(ev.modifiers)) {
                cx.notify();
            }
            return;
        }
        if studio.palette.is_some() {
            return;
        }
        let g = geometry(&studio, window);
        // Caps Lock reaches the window only as a change of its light.
        if caps_pressed && studio.mapped_key(&g, "capslock", None, mods(ev.modifiers)) {
            cx.notify();
        }
        if let Some(ctx) = studio.roll_ctx(&g) {
            let out = studio.roll.mods_changed(&ctx, mods(ev.modifiers));
            if studio.apply_roll(&g, out) {
                cx.notify();
            }
        }
        if studio.arrange_mods(mods(ev.modifiers)) {
            cx.notify();
        }
    }

    /// Files dragged over the window.
    fn files_over(
        &mut self,
        ev: &DragMoveEvent<ExternalPaths>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files_dragged = true;
        let mut studio = self.studio.borrow_mut();
        if studio.input_blocked() {
            return;
        }
        let named = studio.drag_names(ev.drag(cx).paths());
        if studio.drop_hover(true) | named {
            cx.notify();
        }
    }

    /// Files dropped on the window (UI-076).
    fn files_dropped(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files_dragged = false;
        let mut studio = self.studio.borrow_mut();
        if studio.input_blocked() {
            return;
        }
        let g = geometry(&studio, window);
        let (x, y) = in_body(window.mouse_position());
        if studio.drop_files(&g, x, y, paths.paths()) {
            cx.notify();
        }
    }

    fn files(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.studio.borrow_mut().file_request.take() else {
            return;
        };
        if request == FileRequest::SaveAs {
            let destination = self.studio.borrow_mut().save_test_destination.take();
            if let Some(path) = destination {
                self.studio.borrow_mut().start_file_request(request, &path);
                return;
            }
        }
        if matches!(
            request,
            FileRequest::PresetOpen(_) | FileRequest::PresetSave(_)
        ) {
            let file = self.studio.borrow_mut().mixer.preset_file.take();
            if let Some(file) = file {
                self.studio.borrow_mut().start_file_request(request, &file);
                cx.notify();
                return;
            }
        }
        // A script answers the request itself (`finish_file_request`): the system's window
        // would wait for a person.
        if !self.studio.borrow().os_files {
            self.studio.borrow_mut().file_request = Some(request);
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
            return self.pick(request, cx);
        }
        let directory = self
            .studio
            .borrow()
            .song()
            .dir()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let preset_open = matches!(request, FileRequest::PresetOpen(_));
        let preset_save = matches!(request, FileRequest::PresetSave(_));
        let dawproject_open = request == FileRequest::OpenDawproject;
        let files = preset_open || dawproject_open;
        let open = request == FileRequest::Open || files;
        let existing = open.then(|| {
            cx.prompt_for_paths(gpui::PathPromptOptions {
                files,
                directories: !files,
                multiple: false,
                prompt: Some(
                    if preset_open {
                        "プリセットを読み込む"
                    } else if dawproject_open {
                        "DAWproject を開く"
                    } else {
                        "曲を開く"
                    }
                    .into(),
                ),
            })
        });
        let dawproject_name = (request == FileRequest::ExportDawproject)
            .then(|| xirius_session::dawproject::file_name(&self.studio.borrow().project()));
        // The song's own folder, where `export.dawproject` writes when no place is named.
        let directory = match &dawproject_name {
            Some(_) => self.studio.borrow().song().dir().unwrap_or(directory),
            None => directory,
        };
        let new = (!open).then(|| {
            cx.prompt_for_new_path(
                &directory,
                Some(if preset_save {
                    "Preset.xirius-preset"
                } else if let Some(name) = &dawproject_name {
                    name
                } else {
                    "新しい曲"
                }),
            )
        });
        cx.spawn(async move |this, cx| {
            let result = if let Some(picker) = existing {
                picker.await.map(|r| r.map(|p| p.and_then(|mut p| p.pop())))
            } else {
                new.unwrap().await
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(path))) => {
                        this.studio.borrow_mut().start_file_request(request, &path)
                    }
                    Ok(Ok(None)) => {}
                    _ => {
                        this.studio.borrow_mut().error =
                            Some("ファイルを選択できませんでした".into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The folder or file the settings asked for, from the system's dialogs.
    fn settings_files(&mut self, cx: &mut Context<Self>) {
        use crate::settings::Ask;
        let Some(ask) = self.studio.borrow_mut().settings_take_ask() else {
            return;
        };
        let pick = |directories: bool, prompt: &str| {
            cx.prompt_for_paths(gpui::PathPromptOptions {
                files: !directories,
                directories,
                multiple: false,
                prompt: Some(prompt.to_string().into()),
            })
        };
        let existing = match &ask {
            Ask::Folder(_) => Some(pick(true, "フォルダーを追加")),
            Ask::ImportStudioPro | Ask::ImportToml => Some(pick(false, "読み込む")),
            Ask::Export(_) => None,
        };
        let new = match &ask {
            Ask::Export(format) => {
                let name = if format == "toml" {
                    "keymap.toml"
                } else {
                    "keymap.html"
                };
                let dir = dirs_home();
                Some(cx.prompt_for_new_path(&dir, Some(name)))
            }
            _ => None,
        };
        cx.spawn(async move |this, cx| {
            let result = if let Some(picker) = existing {
                picker.await.map(|r| r.map(|p| p.and_then(|mut p| p.pop())))
            } else {
                new.unwrap().await
            };
            let _ = this.update(cx, |this, cx| {
                if let Ok(Ok(Some(path))) = result {
                    this.studio.borrow_mut().settings_answer(ask, &path);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks the OS's file window for audio files or for a folder (UI-017, BRW-005).
    fn pick(&mut self, request: FileRequest, cx: &mut Context<Self>) {
        let files = matches!(request, FileRequest::Audio | FileRequest::Locate(_));
        let prompt = match request {
            FileRequest::Audio => "取り込む",
            FileRequest::Locate(_) => "場所を指定",
            FileRequest::LocateFolder => "探すフォルダー",
            FileRequest::MediaFolder => "置き場所を選ぶ",
            _ => "場所に追加",
        };
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files,
            directories: !files,
            multiple: request == FileRequest::Audio,
            prompt: Some(prompt.into()),
        });
        cx.spawn(async move |this, cx| {
            let result = picker.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(paths))) => this.studio.borrow_mut().finish_picked(request, &paths),
                    Ok(Ok(None)) => {}
                    _ => {
                        this.studio.borrow_mut().error =
                            Some("ファイルを選択できませんでした".into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Hands each run of the AI to the studio, on the window's thread.
    fn listen(mut jobs: xirius_mcp::Jobs, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while let Some(job) = jobs.recv().await {
                let gone = this.update(cx, |this, cx| {
                    this.studio.borrow_mut().ai_job(job);
                    cx.notify();
                });
                if gone.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

/// Asks for the plugins between frames too, every [`crate::health::POLL`]: a minimized window
/// draws none (PLG-053, [`crate::idle`]). A plugin that went down blinks the taskbar button
/// once, unless a script runs the window.
fn watch(window: &Window, cx: &mut Context<Screen>, attention: bool) {
    cx.spawn_in(window, async move |this, cx| {
        loop {
            cx.background_executor().timer(crate::health::POLL).await;
            let alive = this.update_in(cx, |this, window, cx| {
                let Ok(mut studio) = this.studio.try_borrow_mut() else {
                    return;
                };
                let mut idle = studio.idle();
                if !studio.live_midi.assign.commands.is_empty() {
                    let g = geometry(&studio, window);
                    idle.changed |= studio.run_midi_commands(&g);
                }
                if studio.has_asked() {
                    idle.changed = true;
                }
                idle.changed |= studio.poll_other_change(|s| geometry(s, window));
                drop(studio);
                if idle.went_down && attention {
                    window.request_attention();
                }
                if idle.changed {
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        }
    })
    .detach();
}

fn top_bar(studio: Rc<RefCell<Studio>>, window: &Window, c: Rc<Colors>) -> gpui::Div {
    let layout = studio
        .borrow()
        .bar_layout(f32::from(window.viewport_size().width));
    let mut bar = div()
        .relative()
        .w_full()
        .h(px(Layout::TOP_BAR_H))
        .flex_none()
        .bg(c.surface_0);
    for (x0, x1) in &layout.drag {
        bar = bar.child(
            div()
                .absolute()
                .left(px(*x0))
                .top(px(0.0))
                .w(px(x1 - x0))
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        );
    }
    let logo = layout.logo;
    // The logo's face, under the mark (the bar's canvas would cover it).
    let v = studio.borrow().bar_view();
    let face = {
        let b = crate::topbar::BarButton::Logo;
        if v.open == Some(b) {
            Some(c.sel)
        } else if v.hover == Some(b) {
            Some(c.hover)
        } else {
            None
        }
    };
    if let Some(face) = face {
        let r = layout.rect(crate::topbar::BarButton::Logo);
        bar = bar.child(
            div()
                .absolute()
                .left(px(r.x))
                .top(px(r.y))
                .w(px(r.w))
                .h(px(r.h))
                .rounded(px(xirius_design::Radius::S.px()))
                .bg(face),
        );
    }
    bar = bar.child(
        svg()
            .absolute()
            .left(px(logo.x))
            .top(px(logo.y))
            .w(px(logo.w))
            .h(px(logo.h))
            .path(LOGO_PATH)
            .text_color(c.ink_strong),
    );
    // A newer version is out (UI-262).
    if v.update
        && let Some(d) = layout.update_dot()
    {
        bar = bar.child(
            div()
                .absolute()
                .left(px(d.x))
                .top(px(d.y))
                .w(px(d.w))
                .h(px(d.h))
                .rounded(px(xirius_design::Radius::Pill.px()))
                .bg(c.fill),
        );
    }
    if cfg!(windows) {
        let side = crate::caption::icon_side();
        for (i, (area, path)) in crate::caption::buttons(window.is_maximized())
            .into_iter()
            .enumerate()
        {
            // The close button's hover face is the danger fill: its glyph takes
            // the ink on that fill (UI-309).
            let is_close = i == 2;
            let hover = if is_close { c.danger } else { c.surface_3 };
            let hover_ink = if is_close { c.on_fill } else { c.ink };
            let group = gpui::SharedString::from(format!("caption-{i}"));
            bar = bar.child(
                div()
                    .group(group.clone())
                    .absolute()
                    .right(px((2 - i) as f32 * Layout::CAPTION_BUTTON_W))
                    .top(px(0.0))
                    .w(px(Layout::CAPTION_BUTTON_W))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .window_control_area(area)
                    .hover(move |s| s.bg(hover))
                    .child(
                        svg()
                            .w(px(side))
                            .h(px(side))
                            .path(path)
                            .text_color(c.ink)
                            .group_hover(group, move |s| s.text_color(hover_ink)),
                    ),
            );
        }
    }
    bar.child(
        canvas(
            |_, _, _| (),
            move |b, (), window, cx| {
                let studio = studio.borrow();
                let mut pen = Pen {
                    window,
                    cx,
                    c: &c,
                    origin: (f32::from(b.origin.x), f32::from(b.origin.y)),
                };
                crate::topbar::paint(&mut pen, &layout, &studio.bar_view());
                if bars_veiled(&studio) {
                    // The veil of the palette covers the bar too (UI-087), as
                    // the settings' and the recovery card's do.
                    let veil = pen.c.veil;
                    pen.fill(
                        crate::studio::Rect::new(0.0, 0.0, layout.width, Layout::TOP_BAR_H),
                        veil,
                    );
                }
            },
        )
        .size_full(),
    )
}

/// Whether a veil covers the bars: the palette's, the settings', the recovery card's.
fn bars_veiled(studio: &Studio) -> bool {
    studio
        .opening
        .as_ref()
        .is_some_and(|open| open.visible(crate::wait::now()))
        || studio.palette.is_some()
        || studio.settings.is_some()
        || studio.recovery.is_some()
        || studio.preparing.is_some()
}

/// The bottom bar under the drawer (UI-300): the CPU and the transport.
fn bottom_bar(studio: Rc<RefCell<Studio>>, window: &Window, c: Rc<Colors>) -> gpui::Div {
    let layout = studio
        .borrow()
        .bar_layout(f32::from(window.viewport_size().width));
    div()
        .relative()
        .w_full()
        .h(px(Layout::BOTTOM_BAR_H))
        .flex_none()
        .bg(c.surface_0)
        .child(
            canvas(
                |_, _, _| (),
                move |b, (), window, cx| {
                    let studio = studio.borrow();
                    let mut pen = Pen {
                        window,
                        cx,
                        c: &c,
                        origin: (f32::from(b.origin.x), f32::from(b.origin.y)),
                    };
                    crate::topbar::paint_bottom(&mut pen, &layout, &studio.bar_view());
                    if bars_veiled(&studio) {
                        let veil = pen.c.veil;
                        pen.fill(
                            crate::studio::Rect::new(0.0, 0.0, layout.width, Layout::BOTTOM_BAR_H),
                            veil,
                        );
                    }
                },
            )
            .size_full(),
        )
}

/// The settings' field that has the keyboard, if one has.
fn settings_field(studio: &Studio) -> Option<&crate::settings::field::Field> {
    studio
        .input_map
        .name
        .as_ref()
        .map(|n| &n.field)
        .or_else(|| studio.settings.as_ref()?.field.as_ref())
}

/// The field that takes the text: a menu's name box, or the settings' field.
fn typing_field(studio: &Studio) -> Option<&crate::settings::field::Field> {
    studio.ask_field().or_else(|| settings_field(studio))
}

/// A list's search box, as a field, when it takes the text (UI-338): over
/// the settings' field, under a menu's name box and the rename box.
fn pick_field(studio: &Studio) -> Option<crate::settings::field::Field> {
    if studio.ask_field().is_some() || studio.input_map.name.is_some() {
        return None;
    }
    studio.pick_typing().map(|p| p.field())
}

/// Where the keymap is written to first: the home folder.
fn dirs_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The palette's search field, as GPUI's IME sees it (UI-087), and the
/// settings' fields (SET-001). Registered only while one of them is open;
/// each call goes to [`crate::palette::Palette`] or the settings' field.
impl EntityInputHandler for Screen {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let studio = self.studio.borrow();
        if let Some(f) = pick_field(&studio) {
            let (text, real) = f.text_for_range(range);
            *adjusted = Some(real);
            return Some(text);
        }
        if let Some(f) = typing_field(&studio) {
            let (text, real) = f.text_for_range(range);
            *adjusted = Some(real);
            return Some(text);
        }
        let (text, real) = studio.palette.as_ref()?.text_for_range(range);
        *adjusted = Some(real);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let studio = self.studio.borrow();
        if let Some(f) = pick_field(&studio) {
            return Some(UTF16Selection {
                range: f.selected_range(),
                reversed: false,
            });
        }
        if let Some(f) = typing_field(&studio) {
            return Some(UTF16Selection {
                range: f.selected_range(),
                reversed: false,
            });
        }
        Some(UTF16Selection {
            range: studio.palette.as_ref()?.selected_range(),
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let studio = self.studio.borrow();
        if let Some(f) = pick_field(&studio) {
            return f.marked_range();
        }
        if let Some(f) = typing_field(&studio) {
            return f.marked_range();
        }
        studio.palette.as_ref()?.marked_range()
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let mut studio = self.studio.borrow_mut();
        if pick_field(&studio).is_some() {
            studio.pick_unmark();
        } else if studio.ask_field().is_some() {
            studio.ask_unmark();
        } else if let Some(name) = &mut studio.input_map.name {
            name.field.unmark();
        } else if settings_field(&studio).is_some() {
            studio.settings_unmark();
        } else if let Some(p) = studio.palette.as_mut() {
            p.unmark();
        }
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut studio = self.studio.borrow_mut();
        if studio.input_blocked() {
            return;
        }
        if pick_field(&studio).is_some() {
            studio.pick_text(range, text, false);
        } else if studio.ask_field().is_some() {
            studio.ask_text(range, text, false);
        } else if let Some(name) = &mut studio.input_map.name {
            name.field.replace_text(range, text);
        } else if settings_field(&studio).is_some() {
            studio.settings_text(range, text, false);
        } else if let Some(p) = studio.palette.as_mut() {
            p.replace_text(range, text);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut studio = self.studio.borrow_mut();
        if studio.input_blocked() {
            return;
        }
        if pick_field(&studio).is_some() {
            studio.pick_text(range, text, true);
        } else if studio.ask_field().is_some() {
            studio.ask_text(range, text, true);
        } else if let Some(name) = &mut studio.input_map.name {
            name.field.mark_text(range, text);
        } else if settings_field(&studio).is_some() {
            studio.settings_text(range, text, true);
        } else if let Some(p) = studio.palette.as_mut() {
            p.mark_text(range, text);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let studio = self.studio.borrow();
        let g = studio.geometry(
            f32::from(element_bounds.size.width),
            f32::from(element_bounds.size.height),
        );
        if let Some(f) = pick_field(&studio) {
            // The search box is the list's first cell (UI-338).
            let at = match studio.popup.as_ref().filter(|p| p.pick.is_some()) {
                Some(p) => p.layout(g.size).cells.first()?.rect,
                None => {
                    studio
                        .settings
                        .as_ref()?
                        .menu
                        .as_ref()?
                        .menu
                        .layout
                        .cells
                        .first()?
                        .rect
                }
            };
            let mut pen = Pen {
                window,
                cx,
                c: &self.colors,
                origin: (0.0, 0.0),
            };
            let r = f.text_bounds(at, range, |t| {
                pen.measure(t, FontSize::Small, Weight::Regular)
            });
            return Some(Bounds::new(
                point(
                    element_bounds.origin.x + px(r.x),
                    element_bounds.origin.y + px(r.y),
                ),
                size(px(r.w), px(r.h)),
            ));
        }
        if let Some(f) = studio.ask_field() {
            let at = studio.popup.as_ref()?.layout(g.size).cells.first()?.rect;
            let mut pen = Pen {
                window,
                cx,
                c: &self.colors,
                origin: (0.0, 0.0),
            };
            let r = f.text_bounds(at, range, |t| {
                pen.measure(t, FontSize::Small, Weight::Regular)
            });
            return Some(Bounds::new(
                point(
                    element_bounds.origin.x + px(r.x),
                    element_bounds.origin.y + px(r.y),
                ),
                size(px(r.w), px(r.h)),
            ));
        }
        if let Some(f) = settings_field(&studio) {
            let at = studio
                .name_rect(&g)
                .or_else(|| studio.settings_scene(&g).field)?;
            let mut pen = Pen {
                window,
                cx,
                c: &self.colors,
                origin: (0.0, 0.0),
            };
            let r = f.text_bounds(at, range, |t| {
                pen.measure(t, FontSize::Small, Weight::Regular)
            });
            return Some(Bounds::new(
                point(
                    element_bounds.origin.x + px(r.x),
                    element_bounds.origin.y + px(r.y),
                ),
                size(px(r.w), px(r.h)),
            ));
        }
        let p = studio.palette.as_ref()?;
        let search = p.layout(g.size).search;
        let mut pen = Pen {
            window,
            cx,
            c: &self.colors,
            origin: (0.0, 0.0),
        };
        let r = p.text_bounds(search, range, |t| {
            pen.measure(t, FontSize::Lead, Weight::Regular)
        });
        Some(Bounds::new(
            point(
                element_bounds.origin.x + px(r.x),
                element_bounds.origin.y + px(r.y),
            ),
            size(px(r.w), px(r.h)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        // The caret does not move by the pointer: it stays at the end.
        None
    }
}

impl Screen {
    /// Asks the system for the tool's own cursor over the body (`None`: the system's shape).
    /// The window is found by its title the first few frames, then kept.
    #[cfg(windows)]
    fn tool_cursor(&mut self, shape: Option<crate::pointer::Shape>) {
        if self.pointer_window.is_none() && self.pointer_looks < 20 {
            self.pointer_looks += 1;
            self.pointer_window = crate::park::own_handle(WINDOW_TITLE)
                .filter(|hwnd| crate::pointer_os::install(*hwnd));
        }
        if let Some(hwnd) = self.pointer_window {
            crate::pointer_os::show(hwnd, shape);
        }
    }
}

impl Render for Screen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The theme follows `ui.theme` at once (SET-002); `auto` follows the system.
        let theme = self.fixed_theme.unwrap_or_else(|| {
            let system = match window.appearance() {
                gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => {
                    xirius_design::Theme::Dark
                }
                _ => xirius_design::Theme::Light,
            };
            self.studio.borrow().wanted_theme(system)
        });
        self.studio.borrow_mut().set_caption_theme(theme);
        if theme != self.colors.theme {
            self.colors = Rc::new(Colors::new(theme));
            let colors = self.colors.clone();
            self.background
                .update(cx, |view, cx| view.set_colors(colors, cx));
        }
        let c = self.colors.clone();
        {
            let mut studio = self.studio.borrow_mut();
            // "Animation effects" can be turned off while the app runs (UI-055).
            if let Some(reduced) = studio
                .motion_watch
                .as_mut()
                .and_then(|w| w.poll(std::time::Instant::now(), crate::motion_watch::os_reduced))
            {
                studio.ai.motion.reduced = reduced;
            }
            let g = geometry(&studio, window);
            studio.resized(&g);
            if !studio.input_blocked() {
                studio.run_midi_commands(&g);
            }
            // Screen commands asked for from the command line or the AI (KEY-012).
            if !studio.input_blocked() {
                studio.run_asked(&g);
            }
            match studio.window_ask.take() {
                Some(crate::context::key_more::WindowAsk::Fullscreen) => {
                    window.toggle_fullscreen();
                }
                Some(crate::context::key_more::WindowAsk::Quit) if studio.request_close() => {
                    window.defer(cx, |window, _| window.remove_window());
                }
                Some(crate::context::key_more::WindowAsk::Quit) | None => {}
            }
            if studio.tick()
                || studio.motion.moving()
                || crate::wait::pending()
                || crate::repaint_shots::still_frames()
            {
                window.request_animation_frame();
            }
            studio.follow_playhead(&g);
            if studio.drag_edge_frame(&g) {
                window.request_animation_frame();
            }
            // Files dragged out of the window again leave no light behind.
            if self.files_dragged && !cx.has_active_drag() {
                self.files_dragged = false;
                studio.drop_hover(false);
            }
            if let Some(text) = studio.ai.copy.take() {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
        }
        if self.studio.borrow().saving.is_none() {
            let inputs = std::mem::take(&mut self.pending_save_input);
            if !inputs.is_empty() {
                window.defer(cx, move |window, cx| {
                    for input in inputs {
                        window.dispatch_event(input, cx);
                    }
                });
            }
        }
        let g = geometry(&self.studio.borrow(), window);
        self.background.update(cx, |view, cx| view.sync(g, cx));
        let mut background_style = gpui::StyleRefinement::default();
        background_style.size.width = Some(px(g.size.0).into());
        background_style.size.height = Some(px(g.size.1).into());
        let bar = top_bar(self.studio.clone(), window, c.clone());
        let bottom_nodes = crate::a11y::bottom_bar_nodes(&self.studio.borrow(), &g);
        let bottom = bottom_bar(self.studio.clone(), window, c.clone())
            .id("bottom-bar")
            .when(self.studio.borrow().settings.is_none(), |bar| {
                let nodes = bottom_nodes;
                bar.role(gpui::Role::Toolbar)
                    .aria_label("トランスポート")
                    .a11y_synthetic_children(move |tree| {
                        for (i, node) in nodes {
                            tree.push_child(tree.synthetic_node_id(("bottom-bar", i)), node);
                        }
                    })
            });
        let studio = self.studio.clone();
        let colors = self.colors.clone();
        let (entity, focus) = (cx.entity(), self.focus.clone());
        let pointer = {
            let studio = self.studio.borrow();
            crate::pointer::of_studio(&studio, &g)
        };
        #[cfg(windows)]
        self.tool_cursor(pointer.shape);
        let cursor = pointer.style;
        // The cached song overview, then the roll and what floats over both
        // (the playhead, the mixer, pop-overs, the error line) in one canvas
        // on top.
        let settings_nodes = crate::a11y::settings_nodes(&self.studio.borrow(), &g);
        let has_settings = self.studio.borrow().settings.is_some();
        let about_nodes = crate::a11y::about_nodes(&self.studio.borrow(), &g);
        let has_about = !has_settings && !about_nodes.is_empty();
        let track_rows = crate::a11y::track_rows(&self.studio.borrow(), &g);
        let marks = crate::a11y::marks(&self.studio.borrow(), &g);
        let body = div()
            .id("studio-body")
            .when(has_settings, |body| {
                body.role(gpui::Role::Dialog)
                    .aria_label("設定")
                    .a11y_synthetic_children(move |tree| {
                        for (i, node) in settings_nodes {
                            tree.push_child(tree.synthetic_node_id(("settings", i)), node);
                        }
                    })
            })
            .when(has_about, |body| {
                body.role(gpui::Role::Dialog)
                    .aria_label("Xirius について")
                    .a11y_synthetic_children(move |tree| {
                        for (i, node) in about_nodes {
                            tree.push_child(tree.synthetic_node_id(("about", i)), node);
                        }
                    })
            })
            .when(
                !has_settings && !has_about && !track_rows.is_empty(),
                |body| {
                    body.role(gpui::Role::Tree)
                        .aria_label("トラック")
                        .a11y_synthetic_children(move |tree| {
                            // Several rows can be selected at once (UI-340).
                            tree.parent_node().set_multiselectable();
                            for (i, node) in track_rows {
                                tree.push_child(tree.synthetic_node_id(("track-row", i)), node);
                            }
                        })
                },
            )
            .relative()
            .cursor(cursor)
            .flex_1()
            .w_full()
            .min_h(px(0.0))
            .child(self.background.clone().cached(background_style))
            .child(
                canvas(
                    |_, _, _| (),
                    move |b, (), window, cx| {
                        let studio = studio.borrow();
                        let mut pen = Pen {
                            window,
                            cx,
                            c: &colors,
                            origin: (f32::from(b.origin.x), f32::from(b.origin.y)),
                        };
                        let g = studio.geometry(f32::from(b.size.width), f32::from(b.size.height));
                        if g.drawer_body.h > 0.0 && studio.tab == crate::studio::DrawerTab::Roll {
                            pen.mask(g.drawer_body, |pen| paint_roll(pen, &studio, g.drawer_body));
                        }
                        paint_overlay(&mut pen, &studio, &g);
                        // While the palette is open its field takes text,
                        // and the IME is on (it is off for the shortcuts).
                        let typing = studio.settings.as_ref().is_some_and(|s| s.field.is_some());
                        if studio.palette.is_some()
                            || typing
                            || studio.pick_typing().is_some()
                            || studio.ask_field().is_some()
                            || studio.input_map.name.is_some()
                        {
                            window.handle_input(&focus, ElementInputHandler::new(b, entity), cx);
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c.surface_0)
            .text_color(c.ink)
            .font(ui_font(Weight::Regular, false))
            .text_size(px(FontSize::Small.px()))
            .track_focus(&self.focus)
            .on_mouse_move(cx.listener(Self::moved))
            .on_mouse_exit(cx.listener(Self::exited))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::pressed))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::right_pressed))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::middle_pressed))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::middle_released))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::middle_released))
            .on_scroll_wheel(cx.listener(Self::wheel))
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(Self::about_menu))
            .on_key_up(cx.listener(Self::key_up))
            .on_drag_move(cx.listener(Self::files_over))
            .on_drop(cx.listener(Self::files_dropped))
            .on_modifiers_changed(cx.listener(Self::modifiers))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::released))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::released))
            .child(bar)
            .child(body)
            .child(bottom)
            .child(crate::opening_view::accessible(self.studio.clone(), &g))
            .child(crate::unsaved::accessible(self.studio.clone(), &g))
            .children(marks.into_iter().map(|m| mark(m, self.studio.clone())))
    }
}

/// An empty box over a control that is only painted, carrying its accessible node (UI-298):
/// it takes no pointer, and a screen reader's press on a count of takes opens the take rows.
fn mark(m: crate::a11y::Mark, studio: Rc<RefCell<Studio>>) -> gpui::Stateful<gpui::Div> {
    let n = &m.node;
    let b = n.bounds().unwrap_or_default();
    let mut el = div()
        .id(gpui::SharedString::from(format!("a11y-{}", m.key)))
        .absolute()
        .left(px(b.x0 as f32))
        .top(px(b.y0 as f32))
        .w(px(b.width() as f32))
        .h(px(b.height() as f32))
        .role(n.role());
    if let Some(label) = n.label() {
        el = el.aria_label(label.to_string());
    }
    if let Some(value) = n.value() {
        el = el.aria_value(value.to_string());
    }
    if let Some(open) = n.is_expanded() {
        el = el.aria_expanded(open);
    }
    if let Some(selected) = n.is_selected() {
        el = el.aria_selected(selected);
    }
    if let Some(toggled) = n.toggled() {
        el = el.aria_toggled(toggled);
    }
    el = el.children(m.children.into_iter().map(|mut child| {
        if let Some(r) = child.node.bounds() {
            child.node.set_bounds(gpui::accesskit::Rect::new(
                r.x0 - b.x0,
                r.y0 - b.y0,
                r.x1 - b.x0,
                r.y1 - b.y0,
            ));
        }
        mark(child, studio.clone())
    }));
    if let Some(track) = m.fold {
        el = el.on_a11y_action(gpui::accesskit::Action::Click, move |_, window, _| {
            if crate::a11y::fold(&mut studio.borrow_mut(), track) {
                window.refresh();
            }
        });
    } else if let Some(track) = m.pick {
        el = el.on_a11y_action(gpui::accesskit::Action::Click, move |_, window, _| {
            let g = geometry(&studio.borrow(), window);
            if crate::a11y::pick(&mut studio.borrow_mut(), &g, track) {
                window.refresh();
            }
        });
    } else if let Some(hit) = m.press {
        el = el.on_a11y_action(gpui::accesskit::Action::Click, move |_, window, _| {
            let g = geometry(&studio.borrow(), window);
            if crate::a11y::press(&mut studio.borrow_mut(), &g, hit) {
                window.refresh();
            }
        });
    }
    el
}

// ------------------------------------------------------- screenshot driver

enum Phase {
    /// Frames to wait before the first step (fonts, first layout).
    Warm(u32),
    /// Frames to wait after a step before taking the picture.
    Settle(u32),
    Capturing(Capture),
}

struct Driver {
    todo: VecDeque<&'static shot::Shot>,
    current: Option<&'static shot::Shot>,
    phase: Phase,
    dir: PathBuf,
    saved: usize,
    failed: usize,
}

struct Run {
    studio: Rc<RefCell<Studio>>,
    place: Place,
    parked: bool,
    driver: Option<Driver>,
    roll_bench: Option<crate::roll::probe::Bench>,
    mixer_bench: Option<crate::mixer::bench::Bench>,
}

const WARM_FRAMES: u32 = 40;
const SETTLE_FRAMES: u32 = 8;

fn modifiers(ctrl: bool) -> Modifiers {
    Modifiers {
        control: ctrl,
        ..Modifiers::default()
    }
}

/// Sends one act through the window's input path.
fn perform(act: &Act, run: &Run, window: &mut Window, cx: &mut App) {
    let aim = |target| {
        let studio = run.studio.borrow();
        let hit = shot::resolve(&studio, target)?;
        let g = geometry(&studio, window);
        let (x, y) = studio.center_of(&g, hit)?;
        Some(point(px(x), px(y + Layout::TOP_BAR_H)))
    };
    let mouse_move = |at| {
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at,
            pressed_button: None,
            modifiers: Modifiers::default(),
        })
    };
    match *act {
        Act::Stage(play, stage) => match play {
            Play::Studio(f) => f(&mut run.studio.borrow_mut(), stage),
            Play::Laid(f) => {
                let mut s = run.studio.borrow_mut();
                let g = geometry(&s, window);
                f(&mut s, &g, stage);
            }
            Play::Window(f) => f(stage, &run.studio, window, cx),
        },
        Act::RollRuler(stage) => crate::rollruler_shots::play(stage, &run.studio, window, cx),
        Act::Import(stage) => {
            let mut s = run.studio.borrow_mut();
            let g = geometry(&s, window);
            crate::import_shots::stage(&mut s, &g, stage);
        }
        Act::ClipEdit(stage) => {
            let mut s = run.studio.borrow_mut();
            let g = geometry(&s, window);
            crate::clipedit_shots::stage(&mut s, &g, stage);
        }
        Act::Plugin(stage) => {
            let shot = run
                .driver
                .as_ref()
                .and_then(|d| Some((d.dir.as_path(), d.current?.name)));
            crate::plugin_shots::play(stage, &run.studio, shot, window, cx);
        }
        Act::Settings(stage) => {
            let mut s = run.studio.borrow_mut();
            let g = geometry(&s, window);
            crate::settings::shots::stage(&mut s, &g, stage);
        }
        Act::Reset => {
            let mut s = run.studio.borrow_mut();
            let mode = s.transport.mode;
            *s = Studio::sample();
            s.transport.mode = mode;
            if run.roll_bench.is_some() {
                crate::roll::probe::Bench::prepare(&mut s);
            }
            window.refresh();
        }
        Act::Hover(target) => match aim(target) {
            Some(at) => {
                window.dispatch_event(mouse_move(at), cx);
            }
            None => eprintln!("cannot aim at {target:?}"),
        },
        Act::Press(target) => match aim(target) {
            Some(at) => {
                window.dispatch_event(mouse_move(at), cx);
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }),
                    cx,
                );
            }
            None => eprintln!("cannot aim at {target:?}"),
        },
        Act::Wheel { at, dx, dy, ctrl } => match aim(at) {
            Some(at) => {
                window.dispatch_event(mouse_move(at), cx);
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: at,
                        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
                        modifiers: modifiers(ctrl),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            }
            None => eprintln!("cannot aim at {at:?}"),
        },
        Act::Drag { at, dx, dy } => match aim(at) {
            Some(at) => {
                let to = point(at.x + px(dx), at.y + px(dy));
                window.dispatch_event(mouse_move(at), cx);
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: at,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: to,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: to,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }),
                    cx,
                );
            }
            None => eprintln!("cannot aim at {at:?}"),
        },
        Act::Key(source) => match Keystroke::parse(source) {
            Ok(keystroke) => {
                let m = keystroke.modifiers;
                let letter = keystroke
                    .key_char
                    .clone()
                    .or_else(|| (keystroke.key.chars().count() == 1).then(|| keystroke.key.clone()))
                    .filter(|_| !m.control && !m.alt && !m.platform);
                window.dispatch_event(
                    PlatformInput::KeyDown(KeyDownEvent {
                        keystroke,
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
                // The letter follows the key, as Windows sends WM_CHAR after
                // it; it reaches only an open palette's field, the one place
                // that takes text through the IME's door.
                if let (Some(letter), Some(p)) = (letter, run.studio.borrow_mut().palette.as_mut())
                {
                    p.replace_text(None, &letter);
                }
            }
            Err(e) => eprintln!("bad key {source}: {e}"),
        },
    }
}

/// Runs at the start of every frame and schedules itself for the next.
fn step(run: &Rc<RefCell<Run>>, window: &mut Window, cx: &mut App) {
    {
        let mut r = run.borrow_mut();
        if !r.parked && r.place == Place::Parked {
            r.parked = park(WINDOW_TITLE, PARK_AT, PARK_AT);
        }
    }
    let mut quit = false;
    let (events, finished) = {
        let mut r = run.borrow_mut();
        let studio = r.studio.clone();
        if let Some(bench) = &mut r.roll_bench {
            let mut s = studio.borrow_mut();
            let g = geometry(&s, window);
            bench.tick(&mut s, &g)
        } else if let Some(bench) = &mut r.mixer_bench {
            let mut s = studio.borrow_mut();
            let g = geometry(&s, window);
            (Vec::new(), bench.tick(&mut s, &g))
        } else {
            (Vec::new(), false)
        }
    };
    let had_events = !events.is_empty();
    for event in events {
        window.dispatch_event(event, cx);
    }
    quit |= finished;
    let studio = run.borrow().studio.clone();
    #[cfg(feature = "loading-probe")]
    let opening_ready = crate::opening_shots::ready(&studio);
    #[cfg(not(feature = "loading-probe"))]
    let opening_ready = studio.borrow().opening.is_none();
    let capture_ready = opening_ready
        && crate::saving_shots::ready(&studio, window, cx)
        && crate::otherproc_shots::ready(&studio, window, cx);
    // Handlers borrow the studio, so nothing here is held while dispatching.
    let acts: Option<&'static shot::Shot> = {
        let mut r = run.borrow_mut();
        match r.driver.take() {
            None => None,
            Some(mut d) => {
                let mut next = None;
                match &mut d.phase {
                    Phase::Warm(n) | Phase::Settle(n) if *n > 0 => *n -= 1,
                    Phase::Warm(_) => next = d.todo.pop_front(),
                    Phase::Settle(_) if capture_ready => {
                        let name = d.current.map_or("shot", |s| s.name);
                        let path = d.dir.join(format!("{name}.bmp"));
                        d.phase = Phase::Capturing(Capture::start(WINDOW_TITLE, path));
                    }
                    Phase::Settle(_) => {}
                    Phase::Capturing(capture) => match capture.outcome() {
                        Outcome::Pending => {}
                        done => {
                            if done == Outcome::Saved {
                                d.saved += 1;
                            } else {
                                d.failed += 1;
                            }
                            next = d.todo.pop_front();
                            if next.is_none() {
                                println!(
                                    "screenshots: {} saved, {} failed, in {}",
                                    d.saved,
                                    d.failed,
                                    d.dir.display()
                                );
                                quit = true;
                            }
                        }
                    },
                }
                if let Some(shot) = next {
                    d.current = Some(shot);
                    d.phase = Phase::Settle(SETTLE_FRAMES);
                }
                r.driver = Some(d);
                next
            }
        }
    };
    if let Some(shot) = acts {
        for act in shot.acts {
            let r = run.borrow();
            perform(act, &r, window, cx);
        }
    }
    // Frames being measured are asked for by the screen itself, as in use: a `refresh` would
    // paint every cached view again.
    if crate::repaint_shots::still_frames() {
    } else if !had_events || run.borrow().roll_bench.is_none() {
        window.refresh();
    }
    if quit {
        cx.quit();
        return;
    }
    let next = run.clone();
    window.on_next_frame(move |window, cx| step(&next, window, cx));
}

/// Opens the window and runs until it is closed (or the script ends).
pub fn run(opts: Options) {
    if let Err(e) = crate::crash_reports::install() {
        eprintln!("could not initialize crash records: {e}");
    }
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            if let Err(e) = crate::fonts::load(cx) {
                eprintln!("could not load the bundled fonts: {e}");
            }
            crate::app_menu::install(cx);
            let (w, h) = opts.window;
            let bounds = match opts.place {
                Place::At(x, y) => Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
                // Parked: opened centred, then moved away by `step` (see park.rs).
                Place::Centered | Place::Parked => Bounds::centered(None, size(px(w), px(h)), cx),
            };
            let measured = opts.roll_bench.is_some() || opts.mixer_bench.is_some();
            let initial = if opts.shots.is_some() || measured {
                Studio::sample()
            } else {
                Studio::new(crate::song::Song::empty())
            };
            let studio = Rc::new(RefCell::new(initial));
            // A script or a measurement never sounds on the speakers.
            studio.borrow_mut().transport.mode = if opts.shots.is_some() || measured {
                OutputMode::Silent { paced: true }
            } else {
                opts.output
            };
            if opts.roll_bench.is_some() {
                crate::roll::probe::Bench::prepare(&mut studio.borrow_mut());
            }
            if opts.mixer_bench.is_some()
                && let Err(e) = crate::mixer::bench::prepare(&mut studio.borrow_mut())
            {
                eprintln!("could not make the 64 strips: {e}");
            }
            // A script keeps the mock's list, so its pictures do not depend on this PC.
            if opts.shots.is_none() && !measured {
                let mut s = studio.borrow_mut();
                s.use_installed_plugins();
                s.os_files = true;
                // The places, favourites and hidden things kept from the last run (BRW-005).
                s.browser.start_prefs(xirius_session::places::path());
                // Off the window's thread: the device's channels (its format only) and,
                // from then on, the MIDI ports plugged in and out (ENG-001).
                s.read_input_channels();
                s.watch_midi_ports();
                // Whether a newer version is out, once a day (UI-262).
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                s.check_for_update(
                    xirius_session::update::UpdateCheck::in_dir(
                        &xirius_session::recent::root(),
                        xirius_session::update::curl(),
                    ),
                    now_ms,
                );
            }
            let driver = opts.shots.as_ref().map(|dir| {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    eprintln!("could not create {}: {e}", dir.display());
                }
                Driver {
                    todo: opts.script.iter().collect(),
                    current: None,
                    phase: Phase::Warm(WARM_FRAMES),
                    dir: dir.clone(),
                    saved: 0,
                    failed: 0,
                }
            });
            let run = Rc::new(RefCell::new(Run {
                studio: studio.clone(),
                place: opts.place,
                parked: false,
                driver,
                roll_bench: opts.roll_bench.clone().map(crate::roll::probe::Bench::new),
                mixer_bench: opts
                    .mixer_bench
                    .clone()
                    .map(crate::mixer::bench::Bench::new),
            }));
            let scripted = opts.shots.is_some() || measured;
            // What the last window moved when it closed: said once (REC-016).
            if !scripted {
                studio.borrow_mut().take_left(&crate::notice::left_path());
                // The settings the last window left (SET-001); a script keeps
                // the defaults in memory.
                studio
                    .borrow_mut()
                    .song
                    .load_settings(&xirius_prefs::config_dir());
                // The song is kept as it is edited, and the one a crash interrupted
                // comes back (PRJ-001, PRJ-012).
                studio
                    .borrow_mut()
                    .start_keeping(crate::keep::Keep::at(crate::keep::root()));
                // The cleanups the last window stopped when it closed run again (REC-026).
                studio.borrow_mut().tidy_later(&crate::notice::later_path());
                // A UIA test opens a page this way (no OS input): XIRIUS_OPEN_SECTION=audio
                // puts the audio settings up on the fixture's simulated devices. After
                // `start_keeping`, which can swap the song (and its io) for the one the
                // last window left.
                let open = std::env::var("XIRIUS_OPEN_SECTION").ok();
                if open.as_deref() == Some("audio") {
                    let mut s = studio.borrow_mut();
                    crate::audioset_shots::fixture(&mut s);
                    s.open_settings(crate::settings::Section::Audio);
                    // An input is chosen so the radios' selection is real (UI-080).
                    let _ = s
                        .song
                        .run("audio.input.select", serde_json::json!({ "channel": 1 }));
                } else if open.as_deref() == Some("midi") {
                    studio
                        .borrow_mut()
                        .open_settings(crate::settings::Section::Midi);
                }
            }
            // The drawer's heights the last window left (UI-095). A script
            // keeps its own unless it names a file.
            if !scripted || std::env::var_os("XIRIUS_VIEW_FILE").is_some() {
                let path = crate::drawer_edge::view_path();
                let mut studio = studio.borrow_mut();
                studio.drawer_size = crate::drawer_edge::DrawerSize::load(&path);
                studio.side_size = crate::side_edge::SideSize::load(&path);
            }
            // Closing the window closes the song: the recordings nothing
            // names any more go to the recycle bin (REC-016). A script or a
            // measurement leaves them.
            let closing = (!scripted).then(|| studio.clone());
            cx.on_window_closed(move |cx, _| {
                if cx.windows().is_empty() {
                    let mut recent_since = std::time::Instant::now();
                    if let Some(studio) = &closing {
                        match studio.try_borrow_mut() {
                            Ok(mut s) => {
                                recent_since = s.keep.recent_close_since().unwrap_or(recent_since);
                                // This song's cleanup joins those of the songs closed
                                // before (REC-026), and its folder is let go: a close is
                                // not a crash.
                                if let Err(e) = s.close_window_song() {
                                    eprintln!("注意: {e}");
                                }
                                // UI-282: the checked installer starts once this window is
                                // gone; it waits for Xirius to close, then puts the new
                                // version in and starts Xirius again.
                                if let Some(install) = s.take_pending_install() {
                                    let spawn = s.update.spawn.clone();
                                    if let Err(e) = spawn(&install) {
                                        eprintln!("注意: {e}");
                                    }
                                }
                                // The cleanups are waited for a while; those not over then
                                // run again at the next start.
                                let until = std::time::Instant::now() + crate::tidy::CLOSE_WAIT;
                                let open_cleaned = s.wait_open_close(crate::tidy::CLOSE_WAIT);
                                let n = open_cleaned
                                    + s.wait_tidy(
                                        until.saturating_duration_since(std::time::Instant::now()),
                                        &crate::notice::later_path(),
                                        &mut |e| eprintln!("注意: {e}"),
                                    );
                                if n > 0 {
                                    eprintln!(
                                        "使われていない録音のファイル {n} 個をごみ箱に移動しました"
                                    );
                                    // Said again by the next window.
                                    let left = crate::notice::left_path();
                                    if let Err(e) = crate::notice::leave(&left, n) {
                                        eprintln!("could not write {}: {e}", left.display());
                                    }
                                }
                            }
                            Err(_) => eprintln!("注意: 更新の予約を読めませんでした"),
                        }
                    }
                    // Keep the process alive while accepted browser snapshots are saved.
                    // The foreground executor continues pumping; no UI thread joins I/O.
                    cx.spawn(async move |cx| {
                        let mut recent_done = false;
                        loop {
                            if !recent_done
                                && let Some(errors) = crate::recent_worker::finish(recent_since)
                            {
                                for error in errors {
                                    eprintln!("recent list close: {error}");
                                }
                                recent_done = true;
                            }
                            if recent_done
                                && let Some(failures) = crate::browser_prefs_save::finish()
                            {
                                for error in failures {
                                    eprintln!("注意: {error}");
                                }
                                cx.update(|cx| {
                                    if cx.windows().is_empty() {
                                        cx.quit();
                                    }
                                });
                                break;
                            }
                            cx.background_executor().timer(crate::health::POLL).await;
                        }
                    })
                    .detach();
                }
            })
            .detach();
            studio.borrow().motion.enabled.set(true);
            // The AI reaches the song over MCP (AI-019); a script or a
            // measurement keeps no server and the OS's motion setting out.
            let served = if scripted {
                None
            } else {
                let mut s = studio.borrow_mut();
                s.ai.motion.reduced = crate::motion_watch::os_reduced();
                s.motion_watch = Some(Default::default());
                crate::ai::serve::start(&mut s)
            };
            let parked = opts.place == Place::Parked;
            let handle = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(WINDOW_TITLE.into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(
                            px(Layout::TRAFFIC_LIGHT_X),
                            px(Layout::TRAFFIC_LIGHT_Y),
                        )),
                    }),
                    window_decorations: Some(WindowDecorations::Client),
                    // A window in the background keeps its frame rate.
                    inactive_frame_interval: None,
                    // Shown without taking the focus (SW_SHOWNOACTIVATE).
                    focus: false,
                    // The body keeps 520 px between the two bars (UI-300).
                    window_min_size: Some(size(px(960.0), px(600.0))),
                    ..WindowOptions::default()
                },
                move |window, cx| {
                    let close_studio = studio.clone();
                    window.on_window_should_close(cx, move |window, _| {
                        let ready = close_studio.borrow_mut().request_close();
                        if !ready {
                            window.request_animation_frame();
                        }
                        ready
                    });
                    if scripted || parked {
                        let first = run.clone();
                        window.on_next_frame(move |window, cx| step(&first, window, cx));
                    }
                    let focus = cx.focus_handle();
                    crate::native_key::install();
                    window.focus(&focus, cx);
                    // A script's pictures keep the theme it asked for.
                    let fixed_theme = (opts.light || scripted).then_some(if opts.light {
                        xirius_design::Theme::Light
                    } else {
                        THEME
                    });
                    let colors = Rc::new(Colors::new(fixed_theme.unwrap_or(THEME)));
                    let background = cx.new(|_| {
                        crate::roll::layer::Background::new(studio.clone(), colors.clone())
                    });
                    cx.new(|cx| {
                        let _ai_server = served.map(|(server, jobs)| {
                            Screen::listen(jobs, cx);
                            server
                        });
                        watch(window, cx, !scripted);
                        let _activation =
                            Some(cx.observe_window_activation(window, |this, window, _| {
                                if !window.is_window_active() {
                                    this.last_caps = None;
                                }
                            }));
                        Screen {
                            pending_save_input: Vec::new(),
                            studio,
                            colors,
                            focus,
                            background,
                            fixed_theme,
                            pointer_window: None,
                            pointer_looks: 0,
                            _ai_server,
                            files_dragged: false,
                            last_caps: None,
                            _activation,
                        }
                    })
                },
            );
            if let Err(e) = handle {
                eprintln!("could not open the window: {e:#}");
                cx.quit();
            }
        });
    // A file still auditioned is let go while its threads run: left to the end of the process,
    // its output would wait for a thread the OS has already ended, and the exit would abort.
    crate::audition::close();
}
