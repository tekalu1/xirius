//! A same-process, editor-owned preset list. No window or focus belongs to the song process.
use crate::{Caption, EditorAction, PresetRow, plugin::Shared};
use std::cell::{Cell, RefCell};
use std::sync::{Arc, Once};
use windows_sys::Win32::UI::Controls::EM_SETCUEBANNER;
use windows_sys::Win32::UI::{Input::KeyboardAndMouse::*, Shell::*};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
use xirius_design::{Font, FontSize, Layout, Pair, Space, Surface, Theme, ink, surface};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
fn color(rgb: u32) -> u32 {
    ((rgb & 255) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 255)
}

struct State {
    shared: Arc<Shared>,
    rows: Vec<PresetRow>,
    query: RefCell<String>,
    cursor: Cell<usize>,
    scroll: Cell<usize>,
    edit: Cell<HWND>,
    scale: f32,
    theme: Theme,
    font: HFONT,
    brush: HBRUSH,
    field_brush: HBRUSH,
}

impl State {
    fn px(&self, n: f32) -> i32 {
        (n * self.scale).round() as i32
    }
    fn visible(&self) -> Vec<usize> {
        let query = self.query.borrow().to_lowercase();
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.footer || r.label.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect()
    }
    fn body_count(&self, visible: &[usize]) -> usize {
        visible.partition_point(|&i| !self.rows[i].footer)
    }
    fn capacity(&self) -> usize {
        10
    }
    fn row_y(&self, visible: usize, body: usize) -> Option<i32> {
        let shown = body.min(self.capacity()).max(2);
        let row = if visible < body {
            if visible < self.scroll.get() || visible >= self.scroll.get() + self.capacity() {
                return None;
            }
            visible - self.scroll.get()
        } else {
            shown + visible - body
        };
        Some(self.px(Layout::POPUP_PAD
            + Layout::MENU_ROW_H * (row + 1) as f32
            + if visible >= body { Space::X2.px() } else { 0.0 }))
    }
    fn choose(&self, hwnd: HWND) {
        if let Some(&i) = self.visible().get(self.cursor.get())
            && self.rows[i].enabled
        {
            let base = self
                .shared
                .editor_bar
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .stack_base;
            crate::editor::allow_song_foreground(base);
            let mut actions = self
                .shared
                .editor_actions
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if actions.len() < 64 {
                actions.push(EditorAction::PresetPick(self.rows[i].key.clone()));
            }
            drop(actions);
            // SAFETY: this popup belongs to the current GUI thread.
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
    fn step(&self, down: bool) {
        let rows = self.visible();
        if rows.is_empty() {
            return;
        }
        for n in 1..=rows.len() {
            let i = if down {
                (self.cursor.get() + n) % rows.len()
            } else {
                (self.cursor.get() + rows.len() - n) % rows.len()
            };
            if self.rows[rows[i]].enabled {
                self.cursor.set(i);
                break;
            }
        }
        let i = self.cursor.get();
        if i < self.body_count(&rows) {
            if i < self.scroll.get() {
                self.scroll.set(i);
            }
            if i >= self.scroll.get() + self.capacity() {
                self.scroll.set(i + 1 - self.capacity());
            }
        }
    }
}

pub(crate) struct Popup {
    hwnd: HWND,
    state: Box<State>,
}

impl Popup {
    pub fn shown(&self) -> bool {
        // SAFETY: the window is alive until this object is dropped.
        unsafe { IsWindowVisible(self.hwnd) != 0 }
    }
    pub fn dismiss(&self) {
        // SAFETY: owned window on its GUI thread.
        unsafe {
            ShowWindow(self.hwnd, SW_HIDE);
        }
    }
    pub fn new(
        owner: HWND,
        button: HWND,
        shared: Arc<Shared>,
        scale: f32,
        caption: Option<Caption>,
    ) -> Option<Self> {
        static CLASS: Once = Once::new();
        CLASS.call_once(|| {
            let name = wide("XiriusPresetList");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                lpszClassName: name.as_ptr(),
                ..Default::default()
            };
            // SAFETY: registration copies the class name; callback is static.
            unsafe {
                RegisterClassW(&class);
            }
        });
        let rows = shared
            .editor_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .menu
            .clone();
        let theme = if caption.is_none_or(|c| c.dark) {
            Theme::Dark
        } else {
            Theme::Light
        };
        // SAFETY: GDI resources belong to the popup and are released after its window.
        let (font, brush, field_brush) = unsafe {
            (
                CreateFontW(
                    -(FontSize::Small.px() * scale).round() as i32,
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    wide(Font::NotoSansJp.family()).as_ptr(),
                ),
                CreateSolidBrush(color(
                    Surface::Paper
                        .strong_text()
                        .resolve(theme)
                        .surface
                        .to_rgb_u32(),
                )),
                CreateSolidBrush(color(
                    Surface::Field
                        .strong_text()
                        .resolve(theme)
                        .surface
                        .to_rgb_u32(),
                )),
            )
        };
        let cursor = rows
            .iter()
            .position(|r| r.selected && r.enabled)
            .or_else(|| rows.iter().position(|r| r.enabled))
            .unwrap_or(0);
        let state = Box::new(State {
            shared,
            rows,
            query: RefCell::new(String::new()),
            cursor: Cell::new(cursor),
            scroll: Cell::new(cursor.saturating_sub(9)),
            edit: Cell::new(std::ptr::null_mut()),
            scale,
            theme,
            font,
            brush,
            field_brush,
        });
        let width = state.px(Layout::MENU_W);
        let footer = state.rows.iter().filter(|r| r.footer).count();
        let height = state.px(Layout::POPUP_PAD * 2.0
            + Space::X2.px()
            + Layout::MENU_ROW_H
                * ((state.rows.len() - footer).min(state.capacity()).max(2) + footer + 1) as f32);
        let mut rect = RECT::default();
        // SAFETY: queries and window creation on the editor's GUI thread. Owner is same-process.
        let hwnd = unsafe {
            GetWindowRect(button, &mut rect);
            let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(monitor, &mut info);
            let parked = GetWindowLongW(owner, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE != 0;
            let x = if parked {
                rect.left
            } else {
                rect.left
                    .min(info.rcWork.right - width)
                    .max(info.rcWork.left)
            };
            let y = if rect.bottom + height <= info.rcWork.bottom {
                rect.bottom
            } else {
                rect.top - height
            }
            .max(info.rcWork.top);
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                wide("XiriusPresetList").as_ptr(),
                wide("Presets").as_ptr(),
                WS_POPUP | WS_CLIPCHILDREN,
                x,
                y,
                width,
                height,
                owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: unselected objects, no window was created.
            unsafe {
                DeleteObject(font);
                DeleteObject(brush);
                DeleteObject(field_brush);
            }
            return None;
        }
        let popup = Self { hwnd, state };
        // SAFETY: the stable boxed state outlives the window and its child EDIT.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*popup.state as *const State as isize);
            let pad = popup.state.px(Layout::POPUP_PAD);
            let edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
                pad,
                pad,
                width - pad * 2,
                popup.state.px(Layout::MENU_ROW_H),
                hwnd,
                1 as _,
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            popup.state.edit.set(edit);
            SendMessageW(edit, WM_SETFONT, font as usize, 1);
            SendMessageW(edit, EM_SETCUEBANNER, 1, wide("検索…").as_ptr() as isize);
            SetWindowSubclass(edit, Some(edit_proc), 1, hwnd as usize);
            let parked = GetWindowLongW(owner, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE != 0;
            ShowWindow(hwnd, if parked { SW_SHOWNOACTIVATE } else { SW_SHOW });
            if !parked {
                SetForegroundWindow(hwnd);
                SetFocus(edit);
            }
        }
        Some(popup)
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        // SAFETY: clear the borrowed state before destroying windows, then release GDI resources.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            DestroyWindow(self.hwnd);
            DeleteObject(self.state.font);
            DeleteObject(self.state.brush);
            DeleteObject(self.state.field_brush);
        }
    }
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    parent: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN && matches!(wp as u16, VK_UP | VK_DOWN | VK_RETURN | VK_ESCAPE) {
        // SAFETY: the subclass data is the live parent popup handle.
        return unsafe { SendMessageW(parent as HWND, msg, wp, lp) };
    }
    if msg == WM_CHAR && matches!(wp, 13 | 27) {
        return 0;
    }
    // SAFETY: all unhandled messages follow the native EDIT procedure.
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: state is boxed by Popup; cleared before its window is destroyed.
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State };
    if state.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    let s = unsafe { &*state };
    // SAFETY: every handle is owned by this GUI thread; GDI objects remain alive during painting.
    unsafe {
        match msg {
            WM_ACTIVATE if wp & 0xffff == WA_INACTIVE as usize => {
                if IsWindowVisible(hwnd) != 0 {
                    ShowWindow(hwnd, SW_HIDE);
                }
                return 0;
            }
            WM_CLOSE => {
                ShowWindow(hwnd, SW_HIDE);
                return 0;
            }
            WM_KEYDOWN => {
                match wp as u16 {
                    VK_ESCAPE => {
                        let empty = s.query.borrow().is_empty();
                        if empty {
                            ShowWindow(hwnd, SW_HIDE);
                        } else {
                            SetWindowTextW(s.edit.get(), wide("").as_ptr());
                        }
                    }
                    VK_DOWN => s.step(true),
                    VK_UP => s.step(false),
                    VK_RETURN => s.choose(hwnd),
                    _ => {}
                }
                InvalidateRect(hwnd, std::ptr::null(), 0);
                return 0;
            }
            WM_COMMAND if wp >> 16 == EN_CHANGE as usize => {
                let mut text = vec![0u16; GetWindowTextLengthW(s.edit.get()) as usize + 1];
                let n = GetWindowTextW(s.edit.get(), text.as_mut_ptr(), text.len() as i32);
                *s.query.borrow_mut() = String::from_utf16_lossy(&text[..n as usize]);
                s.cursor.set(
                    s.visible()
                        .iter()
                        .position(|&i| s.rows[i].enabled)
                        .unwrap_or(0),
                );
                s.scroll.set(0);
                InvalidateRect(hwnd, std::ptr::null(), 0);
                return 0;
            }
            WM_MOUSEWHEEL => {
                let delta = (wp >> 16) as i16;
                let max = s.body_count(&s.visible()).saturating_sub(s.capacity());
                s.scroll.set(if delta > 0 {
                    s.scroll.get().saturating_sub(3)
                } else {
                    (s.scroll.get() + 3).min(max)
                });
                InvalidateRect(hwnd, std::ptr::null(), 0);
                return 0;
            }
            WM_MOUSEMOVE | WM_LBUTTONUP => {
                let y = (lp >> 16) as i16 as i32;
                let visible = s.visible();
                let body = s.body_count(&visible);
                for (i, &row) in visible.iter().enumerate() {
                    if s.rows[row].enabled
                        && s.row_y(i, body)
                            .is_some_and(|top| y >= top && y < top + s.px(Layout::MENU_ROW_H))
                    {
                        s.cursor.set(i);
                        if msg == WM_LBUTTONUP {
                            s.choose(hwnd);
                        }
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                        break;
                    }
                }
                return 0;
            }
            WM_CTLCOLOREDIT => {
                let pair = Surface::Field.strong_text().resolve(s.theme);
                SetTextColor(wp as HDC, color(pair.ink.to_rgb_u32()));
                SetBkColor(wp as HDC, color(pair.surface.to_rgb_u32()));
                return s.field_brush as isize;
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut bounds = RECT::default();
                GetClientRect(hwnd, &mut bounds);
                FillRect(dc, &bounds, s.brush);
                let old = SelectObject(dc, s.font);
                SetBkMode(dc, TRANSPARENT as i32);
                let visible = s.visible();
                let body = s.body_count(&visible);
                if body == 0 && !s.query.borrow().is_empty() {
                    let pair = Surface::Paper.text().resolve(s.theme);
                    SetTextColor(dc, color(pair.ink.to_rgb_u32()));
                    let mut empty = RECT {
                        left: s.px(Layout::POPUP_PAD + Space::X2.px()),
                        top: s.px(Layout::POPUP_PAD + Layout::MENU_ROW_H),
                        right: bounds.right,
                        bottom: s.px(Layout::POPUP_PAD + Layout::MENU_ROW_H * 3.0),
                    };
                    DrawTextW(
                        dc,
                        wide("見つかりません").as_ptr(),
                        -1,
                        &mut empty,
                        DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
                    );
                }
                for (i, &index) in visible.iter().enumerate() {
                    let Some(y) = s.row_y(i, body) else {
                        continue;
                    };
                    let row = &s.rows[index];
                    let face = if i == s.cursor.get() && row.enabled {
                        Surface::Hover
                    } else {
                        Surface::Paper
                    };
                    let pair = if row.enabled {
                        face.strong_text().resolve(s.theme)
                    } else {
                        Pair::<surface::Paper, ink::Disabled>::new().resolve(s.theme)
                    };
                    let mut rect = RECT {
                        left: s.px(Layout::POPUP_PAD),
                        top: y,
                        right: bounds.right - s.px(Layout::POPUP_PAD),
                        bottom: y + s.px(Layout::MENU_ROW_H),
                    };
                    let brush = CreateSolidBrush(color(pair.surface.to_rgb_u32()));
                    FillRect(dc, &rect, brush);
                    DeleteObject(brush);
                    SetTextColor(dc, color(pair.ink.to_rgb_u32()));
                    rect.left += s.px(Space::X2.px());
                    let label = if row.selected {
                        format!("✓ {}", row.label)
                    } else {
                        format!("   {}", row.label)
                    };
                    DrawTextW(
                        dc,
                        wide(&label).as_ptr(),
                        -1,
                        &mut rect,
                        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                    );
                }
                SelectObject(dc, old);
                EndPaint(hwnd, &ps);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
