//! The plugin's editor in a top-level window of our own (`IPlugView` on an HWND).
//!
//! The window is opened without activation and without a taskbar button, so it does not take
//! the foreground from whoever uses the desktop. Whether the plugin grabbed the foreground
//! anyway is reported in `Editor::took_foreground`. [`Placement`] says where it goes: at the
//! bottom of the z-order (tools), above the DAW's window that owns it (a person asked for it),
//! or off every screen (tests and screenshots, [`OFFSCREEN_ENV`]).
//!
//! A view that can resize gets a sizing border: a drag asks the view for the nearest size it
//! accepts (`checkSizeConstraint`) and the new size reaches it through `onSize`. The window's
//! DPI goes to the view as a content scale (`IPlugViewContentScaleSupport`), when it opens and
//! when the window moves to a monitor with another scale.

use std::cell::Cell;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use vst3::Steinberg::Vst::{IEditControllerTrait, ViewType};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kPlatformTypeHWND, kResultFalse,
    kResultOk, tresult,
};
use vst3::{Class, ComPtr, ComWrapper};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmGetWindowAttribute,
    DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect,
    GetForegroundWindow, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, HWND_BOTTOM, HWND_TOP,
    IsWindow, IsWindowVisible, MINMAXINFO, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW,
    SIZE_MINIMIZED, SM_CXVIRTUALSCREEN, SM_XVIRTUALSCREEN, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SendMessageW, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, WM_CLOSE, WM_DPICHANGED, WM_GETMINMAXINFO, WM_SIZE, WM_SIZING,
    WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT, WMSZ_LEFT, WMSZ_TOP, WMSZ_TOPLEFT, WMSZ_TOPRIGHT,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};

use crate::plugin::{Plugin, Shared};
use crate::{Error, error};

const STYLE: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP
    | WS_SYSMENU
    | windows_sys::Win32::UI::WindowsAndMessaging::WS_CLIPCHILDREN;
/// For a view that can resize: a sizing border and a maximize box.
const RESIZABLE: u32 = STYLE | WS_THICKFRAME | WS_MAXIMIZEBOX;
const EX_STYLE: u32 = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
fn frame_style(place: Placement) -> u32 {
    // Requested editors allow keyboard focus when clicked. Opening still uses NOACTIVATE;
    // tools and offscreen windows also retain the non-activating window style.
    if place.top {
        WS_EX_TOOLWINDOW
    } else {
        EX_STYLE
    }
}
#[cfg(test)]
mod key_focus_tests {
    use super::*;
    #[test]
    fn requested_frames_allow_click_focus_and_tools_remain_non_activating() {
        let requested = Placement {
            top: true,
            ..Placement::default()
        };
        assert_eq!(frame_style(requested) & WS_EX_NOACTIVATE, 0);
        assert_ne!(frame_style(Placement::default()) & WS_EX_NOACTIVATE, 0);
        assert_ne!(frame_style(Placement::offscreen()) & WS_EX_NOACTIVATE, 0);
    }
}
/// `PW_CLIENTONLY | PW_RENDERFULLCONTENT`: the client area, including DirectComposition and
/// GPU content where the system can get it.
const PRINT_FLAGS: u32 = 3;

/// Set (to anything but `0`), editors open off every screen: for tests and screenshots, which
/// must not show windows to whoever uses the desktop. A plugin host process inherits it.
pub const OFFSCREEN_ENV: &str = "XIRIUS_EDITOR_OFFSCREEN";

fn offscreen_wanted() -> bool {
    std::env::var_os(OFFSCREEN_ENV).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Where an editor's window opens ([`Plugin::open_editor_at`]). It never takes the
/// foreground; this only decides where it is and what it stays above.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    /// Snapshot of the main window and its monitor; never an HWND ownership link.
    pub area: Option<crate::EditorArea>,
    /// The window that owns it (an `HWND`): the editor stays above that window and is
    /// destroyed with it.
    pub owner: Option<isize>,
    /// The window's top-left corner on the screen. `None` centres it over the owner, or puts
    /// it at 40, 40 without one.
    pub at: Option<(i32, i32)>,
    /// Shown above the other windows; otherwise at the bottom of the z-order, behind them.
    pub top: bool,
    /// The caption's colours, given before the window is first shown. `None` leaves the
    /// system's own.
    pub caption: Option<Caption>,
}

/// The colours the system draws an editor window's caption in, so it keeps the app's own
/// look: the dark or light frame, the bar's colour and its title's (`0xRRGGBB`). Windows 11
/// takes all three; Windows 10 takes only the dark or light frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caption {
    pub dark: bool,
    pub fill: u32,
    pub text: u32,
}

/// A `COLORREF` (`0x00BBGGRR`) of `0xRRGGBB`.
fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16 & 0xff)
}

impl Caption {
    /// Gives `hwnd` these colours; whether the system took all three (an older Windows takes
    /// only the dark or light frame, and the rest is left).
    fn apply(self, hwnd: HWND) -> bool {
        let dark = i32::from(self.dark);
        let fill = colorref(self.fill);
        let text = colorref(self.text);
        // SAFETY: our own window; each value is a 4-byte integer, as the attribute says.
        let done = unsafe {
            [
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    &dark as *const i32 as *const _,
                    4,
                ),
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_CAPTION_COLOR as u32,
                    &fill as *const u32 as *const _,
                    4,
                ),
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_TEXT_COLOR as u32,
                    &text as *const u32 as *const _,
                    4,
                ),
            ]
        };
        done.iter().all(|r| *r == 0)
    }

    /// Whether the system draws `hwnd`'s frame dark (the one attribute it can be asked for;
    /// the colours can only be set).
    fn dark_frame(hwnd: HWND) -> bool {
        let mut dark = 0i32;
        // SAFETY: our own window; the out value is a 4-byte integer.
        let r = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                &mut dark as *mut i32 as *mut _,
                4,
            )
        };
        r == 0 && dark != 0
    }
}

impl Placement {
    /// For a person who asked for the window: above the other windows, over `owner` when there
    /// is one. Off every screen instead while [`OFFSCREEN_ENV`] is set.
    pub fn shown(owner: Option<isize>) -> Placement {
        if offscreen_wanted() {
            return Placement {
                owner,
                area: owner.and_then(crate::EditorArea::from_window),
                ..Placement::offscreen()
            };
        }
        Placement {
            owner,
            at: None,
            top: true,
            caption: None,
            area: owner.and_then(crate::EditorArea::from_window),
        }
    }

    /// To the right of every screen, so the window draws without being seen.
    pub fn offscreen() -> Placement {
        // SAFETY: plain queries.
        let (x, w) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
            )
        };
        Placement {
            owner: None,
            at: Some((x + w + 200, 0)),
            top: false,
            caption: None,
            area: None,
        }
    }

    /// The top-left corner for a window of `w` × `h`.
    fn origin(&self, w: i32, h: i32) -> (i32, i32) {
        if let Some(at) = self.at {
            return at;
        }
        let Some(owner) = self.owner else {
            return (40, 40);
        };
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: a query on a handle the caller gave; a stale one fails.
        if unsafe { GetWindowRect(owner as HWND, &mut r) } == 0 {
            return (40, 40);
        }
        let x = r.left + ((r.right - r.left) - w) / 2;
        let y = r.top + ((r.bottom - r.top) - h) / 3;
        (x.max(r.left), y.max(r.top))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// What the window procedure and the frame share about one editor window. It lives in a box
/// owned by the [`Editor`]; the window's `GWLP_USERDATA` points at it.
struct WindowState {
    flashing: Cell<bool>,
    presets: std::cell::RefCell<Option<crate::editor_presets::Popup>>,
    native_frame: bool,
    viewport: HWND,
    chrome: crate::editor_chrome::Controls,
    shared: Arc<Shared>,
    chrome_scale: Cell<f32>,
    chrome_caption: Cell<Option<Caption>>,
    /// The attached view (no reference of its own: the `Editor` holds one).
    view: *mut IPlugView,
    style: u32,
    /// The window procedure passes sizes to the view only once it is attached.
    live: Cell<bool>,
    /// Set while the frame sizes the window for the view, which calls `onSize` itself.
    quiet: Cell<bool>,
    size: Cell<ViewRect>,
    /// The content scale passed to the view, when it takes one.
    scale: Cell<Option<f32>>,
}

impl WindowState {
    fn touched(&self, hwnd: HWND) {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        let mut actions = self
            .shared
            .editor_actions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        actions.retain(|a| !matches!(a, crate::EditorAction::EditorTouched { .. }));
        actions.push(crate::EditorAction::EditorTouched {
            window: hwnd as u64,
            time,
        });
    }
    fn view(&self) -> vst3::ComRef<'_, IPlugView> {
        // SAFETY: the editor keeps the view alive while the window points at this state.
        unsafe { vst3::ComRef::from_raw(self.view) }.expect("a view")
    }

    /// Frame size: the outer window minus its client area.
    fn frame_size(&self) -> (i32, i32) {
        let mut r = RECT::default();
        if self.native_frame {
            // SAFETY: a local rectangle, no window or plugin call.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::AdjustWindowRectEx(
                    &mut r, self.style, 0, EX_STYLE,
                );
            }
        }
        (
            r.right - r.left,
            r.bottom - r.top + crate::editor_chrome::height(self.chrome_scale.get()),
        )
    }

    fn outer_size(&self, size: ViewRect) -> (i32, i32) {
        (
            (size.right - size.left)
                .max((crate::editor_chrome::MIN_WIDTH * self.chrome_scale.get()).round() as i32)
                + self.frame_size().0,
            size.bottom - size.top + self.frame_size().1,
        )
    }

    fn layout(&self, width: i32, height: i32) {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetParent;
        let mut outer = RECT::default();
        // SAFETY: the viewport and its frame are owned on this GUI thread.
        unsafe {
            GetClientRect(GetParent(self.viewport), &mut outer);
            SetWindowPos(
                self.viewport,
                std::ptr::null_mut(),
                ((outer.right - width) / 2).max(0),
                crate::editor_chrome::height(self.chrome_scale.get()),
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let bar = self
            .shared
            .editor_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        self.chrome
            .layout(outer.right, self.chrome_scale.get(), &bar);
    }

    /// Tells the view its new size and keeps it.
    fn on_size(&self, w: i32, h: i32) {
        self.layout(w, h);
        let mut r = rect(w, h);
        // SAFETY: the attached view and a rect owned here.
        unsafe { self.view().onSize(&mut r) };
        self.size.set(r);
    }

    /// Passes `factor` to the view; false when it takes no content scale.
    fn set_scale(&self, factor: f32) -> bool {
        let Some(css) = self.view().cast::<IPlugViewContentScaleSupport>() else {
            return false;
        };
        // SAFETY: a plain call on the attached view; it may call `resizeView` back.
        unsafe { css.setContentScaleFactor(factor) };
        self.scale.set(Some(factor));
        true
    }
}

fn rect(w: i32, h: i32) -> ViewRect {
    ViewRect {
        left: 0,
        top: 0,
        right: w,
        bottom: h,
    }
}

/// The state of one of our windows, once the editor set it.
///
/// # Safety
/// `hwnd` must be a window of our class on this thread.
unsafe fn state<'a>(hwnd: HWND) -> Option<&'a WindowState> {
    // SAFETY: the pointer is 0 or the editor's live box (cleared before it is freed).
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState).as_ref() }
}

const FRAME_FLASH_TIMER: usize = 1;

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT, ScreenToClient};
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_PAINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BN_DOUBLECLICKED, GetDlgItem, GetMessagePos, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT,
        HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, MA_ACTIVATEANDEAT,
        WM_COMMAND, WM_DRAWITEM, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST,
        WindowFromPoint,
    };
    // SAFETY: the style belongs to this window; a fallback keeps its native frame.
    let native =
        unsafe { GetWindowLongPtrW(hwnd, windows_sys::Win32::UI::WindowsAndMessaging::GWL_STYLE) }
            as u32
            & windows_sys::Win32::UI::WindowsAndMessaging::WS_CAPTION
            != 0;
    if msg == WM_NCCALCSIZE && !native {
        return 0;
    }
    // SAFETY: our class's pointer, cleared before its state is released.
    if let Some(st) = unsafe { state(hwnd) } {
        if msg == WM_MOUSEACTIVATE && (lp as u32 >> 16) == WM_LBUTTONDOWN {
            // The second press is consumed during Windows' mouse activation, before a
            // BN_CLICKED can reopen the list after its normal deactivation.
            let list = st.presets.borrow();
            if let Some(list) = list.as_ref().filter(|list| list.shown()) {
                // SAFETY: query the current mouse message and this frame's band control.
                let on_button = unsafe {
                    let pos = GetMessagePos();
                    let point = windows_sys::Win32::Foundation::POINT {
                        x: pos as i16 as i32,
                        y: (pos >> 16) as i16 as i32,
                    };
                    WindowFromPoint(point) == GetDlgItem(hwnd, crate::editor_chrome::PRESET as i32)
                };
                if on_button {
                    list.dismiss();
                    return MA_ACTIVATEANDEAT as LRESULT;
                }
            }
        }
        use windows_sys::Win32::UI::WindowsAndMessaging::{WA_CLICKACTIVE, WM_ACTIVATE};
        // Closing an owned popup can reactivate a window without anyone touching it.
        if msg == WM_ACTIVATE && wp & 0xffff == WA_CLICKACTIVE as usize {
            st.touched(hwnd);
        }
        use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
        use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, WM_TIMER};
        if msg == WM_TIMER && wp == FRAME_FLASH_TIMER {
            st.flashing.set(false);
            // SAFETY: the timer and paint belong to this frame.
            unsafe {
                KillTimer(hwnd, FRAME_FLASH_TIMER);
                InvalidateRect(
                    hwnd,
                    &crate::editor_chrome::focus_bounds(hwnd, st.chrome_scale.get()),
                    0,
                );
            }
            return 0;
        }
        if msg == WM_PAINT || msg == windows_sys::Win32::UI::WindowsAndMessaging::WM_PRINTCLIENT {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: balanced paint scope on the frame.
            unsafe {
                let printing = msg != WM_PAINT;
                let dc = if printing {
                    wp as _
                } else {
                    BeginPaint(hwnd, &mut ps)
                };
                let bar = st
                    .shared
                    .editor_bar
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                crate::editor_chrome::paint(
                    hwnd,
                    dc,
                    &bar,
                    st.chrome_caption.get(),
                    st.chrome_scale.get(),
                    None,
                );
                if st.flashing.get() {
                    crate::editor_chrome::paint_focus(
                        hwnd,
                        dc,
                        st.chrome_caption.get(),
                        st.chrome_scale.get(),
                    );
                }
                if !printing {
                    EndPaint(hwnd, &ps);
                }
            }
            return 0;
        }
        if msg == WM_DRAWITEM && lp != 0 {
            // SAFETY: Windows supplies a DRAWITEMSTRUCT for a child owner-drawn button.
            let item = unsafe { &*(lp as *const windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT) };
            let bar = st
                .shared
                .editor_bar
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            crate::editor_chrome::paint(
                hwnd,
                item.hDC,
                &bar,
                st.chrome_caption.get(),
                st.chrome_scale.get(),
                Some((
                    item.CtlID as usize,
                    item.itemState & windows_sys::Win32::UI::Controls::ODS_DISABLED == 0,
                )),
            );
            return 1;
        }
        if msg == WM_COMMAND {
            let id = wp & 0xffff;
            let event = (wp >> 16) as u32;
            // An owner-drawn button can report BN_DOUBLECLICKED when the list was
            // dismissed and the button is pressed again within the double-click interval.
            if id == crate::editor_chrome::PRESET && matches!(event, 0 | BN_DOUBLECLICKED) {
                st.touched(hwnd);
                let mut popup = st.presets.borrow_mut();
                if let Some(list) = popup.as_ref()
                    && list.shown()
                {
                    list.dismiss();
                    return 0;
                }
                // SAFETY: the native control belongs to this frame.
                let button = unsafe { GetDlgItem(hwnd, crate::editor_chrome::PRESET as i32) };
                *popup = crate::editor_presets::Popup::new(
                    hwnd,
                    button,
                    st.shared.clone(),
                    st.chrome_scale.get(),
                    st.chrome_caption.get(),
                );
                return 0;
            }
            if event != 0 {
                // SAFETY: non-click notifications keep their normal default handling.
                return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
            }
            let bar = st
                .shared
                .editor_bar
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(action) = crate::editor_chrome::command(hwnd, id, &bar) {
                st.touched(hwnd);
                if matches!(action, crate::EditorAction::Channel) {
                    allow_song_foreground(bar.stack_base);
                }
                let mut actions = st
                    .shared
                    .editor_actions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if actions.len() < 64 {
                    actions.push(action);
                }
            }
            return 0;
        }
        if msg == WM_NCHITTEST && !native {
            let mut p = windows_sys::Win32::Foundation::POINT {
                x: (lp as u16) as i16 as i32,
                y: ((lp >> 16) as u16) as i16 as i32,
            };
            let mut r = RECT::default();
            unsafe {
                ScreenToClient(hwnd, &mut p);
                GetClientRect(hwnd, &mut r);
            }
            let edge = (5.0 * st.chrome_scale.get()).round() as i32;
            let left = p.x < edge;
            let right = p.x >= r.right - edge;
            let top = p.y < edge;
            let bottom = p.y >= r.bottom - edge;
            let hit = if st.style & WS_THICKFRAME != 0 {
                match (left, right, top, bottom) {
                    (true, _, true, _) => HTTOPLEFT,
                    (_, true, true, _) => HTTOPRIGHT,
                    (true, _, _, true) => HTBOTTOMLEFT,
                    (_, true, _, true) => HTBOTTOMRIGHT,
                    (true, _, _, _) => HTLEFT,
                    (_, true, _, _) => HTRIGHT,
                    (_, _, true, _) => HTTOP,
                    (_, _, _, true) => HTBOTTOM,
                    _ if p.y < st.frame_size().1 => HTCAPTION,
                    _ => HTCLIENT,
                }
            } else if p.y < st.frame_size().1 {
                HTCAPTION
            } else {
                HTCLIENT
            };
            return hit as LRESULT;
        }
    }
    if crate::editor_keys::handle(hwnd, msg, wp, lp) {
        return 0;
    }
    if msg == WM_CLOSE {
        // The owner closes the editor by dropping it; the close box only hides the window.
        // SAFETY: `hwnd` is the window this procedure was called for.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        return 0;
    }
    // SAFETY: our window; the state is cleared before it is freed.
    if let Some(st) = unsafe { state(hwnd) }.filter(|s| s.live.get())
        && let Some(result) = sizing(hwnd, st, msg, wp, lp)
    {
        return result;
    }
    // SAFETY: the default procedure for everything else.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Lets the song process activate a window requested by a click in the native editor.
pub(crate) fn allow_song_foreground(stack_base: u64) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, GetWindowThreadProcessId,
    };
    // SAFETY: grant the process owning the song window permission, without sending it
    // a synchronous message or connecting the two processes' input queues.
    unsafe {
        let mut process = 0;
        if GetWindowThreadProcessId(stack_base as HWND, &mut process) != 0 {
            AllowSetForegroundWindow(process);
        }
    }
}

/// The messages that resize the view: a drag on the border (`WM_SIZING`), the new size
/// (`WM_SIZE`), the smallest size (`WM_GETMINMAXINFO`) and a move to a monitor with another
/// scale (`WM_DPICHANGED`). `None` leaves the message to the default procedure.
fn sizing(hwnd: HWND, st: &WindowState, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    let resizable = st.style & WS_THICKFRAME != 0;
    match msg {
        WM_SIZING if resizable && lp != 0 => {
            // SAFETY: the system passes the dragged outer rect, in screen coordinates.
            let r = unsafe { &mut *(lp as *mut RECT) };
            let (bw, bh) = st.frame_size();
            let mut want = rect(
                (r.right - r.left - bw)
                    .max((crate::editor_chrome::MIN_WIDTH * st.chrome_scale.get()).ceil() as i32),
                r.bottom - r.top - bh,
            );
            // SAFETY: the attached view and a rect owned here.
            if unsafe { st.view().checkSizeConstraint(&mut want) } != kResultOk {
                return None;
            }
            let (w, h) = (want.right - want.left + bw, want.bottom - want.top + bh);
            // The dragged edge moves; the opposite one stays.
            let edge = wp as u32;
            if matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT) {
                r.left = r.right - w;
            } else {
                r.right = r.left + w;
            }
            if matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
                r.top = r.bottom - h;
            } else {
                r.bottom = r.top + h;
            }
            Some(1)
        }
        WM_GETMINMAXINFO if resizable && lp != 0 => {
            let mut least = rect(
                (crate::editor_chrome::MIN_WIDTH * st.chrome_scale.get()).ceil() as i32,
                1,
            );
            // SAFETY: the attached view and a rect owned here.
            if unsafe { st.view().checkSizeConstraint(&mut least) } != kResultOk {
                return None;
            }
            let (bw, bh) = st.frame_size();
            // SAFETY: the system passes a writable MINMAXINFO.
            let info = unsafe { &mut *(lp as *mut MINMAXINFO) };
            info.ptMinTrackSize.x = least.right - least.left + bw;
            info.ptMinTrackSize.y = least.bottom - least.top + bh;
            Some(0)
        }
        WM_SIZE if !st.quiet.get() && wp as u32 != SIZE_MINIMIZED => {
            let (w, h) = (
                (lp & 0xffff) as i32,
                (((lp >> 16) & 0xffff) as i32
                    - crate::editor_chrome::height(st.chrome_scale.get()))
                .max(1),
            );
            let now = st.size.get();
            if resizable && (now.right - now.left, now.bottom - now.top) != (w, h) {
                st.on_size(w, h);
            } else {
                st.layout(now.right - now.left, now.bottom - now.top);
            }
            Some(0)
        }
        WM_DPICHANGED => {
            let factor = (wp & 0xffff) as f32 / 96.0;
            st.chrome_scale.set(factor);
            // The view picks its own size for the new scale (through `resizeView`); the window
            // only moves to where the system suggests. A view without a content scale keeps
            // its size.
            st.set_scale(factor);
            let size = st.size.get();
            let (width, height) = st.outer_size(size);
            st.quiet.set(true);
            if lp != 0 {
                // SAFETY: the system passes the suggested outer rect.
                let r = unsafe { &*(lp as *const RECT) };
                // SAFETY: our own window.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        r.left,
                        r.top,
                        width,
                        height,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
            }
            st.layout(size.right - size.left, size.bottom - size.top);
            st.quiet.set(false);
            Some(0)
        }
        _ => None,
    }
}

/// The window class, registered on first use.
fn class_name() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| {
        let name = wide("XiriusPluginEditor");
        // SAFETY: a zeroed WNDCLASSEXW with the required fields set. Windows copies the
        // class name during registration.
        unsafe {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(window_proc);
            wc.hInstance = GetModuleHandleW(std::ptr::null());
            wc.lpszClassName = name.as_ptr();
            RegisterClassExW(&wc);
        }
        name
    })
}

// A vendor forwards keys it did not consume to its parent. Keep that path through
// our viewport to the frame; text input consumed by the vendor never arrives here.
unsafe extern "system" fn viewport_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetParent, SendMessageW, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS,
    };
    if matches!(msg, WM_KEYDOWN | WM_KEYUP | WM_KILLFOCUS) {
        // SAFETY: this viewport and its frame are created on the same GUI thread.
        return unsafe { SendMessageW(GetParent(hwnd), msg, wp, lp) };
    }
    // SAFETY: unhandled messages keep the standard child-window behavior.
    unsafe { windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, msg, wp, lp) }
}

/// The host side of the view: the plugin asks it to resize the window.
pub struct PlugFrame {
    hwnd: usize,
    /// The editor's `WindowState` (it outlives the view's attachment).
    state: usize,
    resizes: std::sync::atomic::AtomicU32,
}

impl Class for PlugFrame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for PlugFrame {
    unsafe fn resizeView(&self, view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if view.is_null() || new_size.is_null() {
            return kResultFalse;
        }
        // SAFETY: the editor keeps its state alive while the view is attached to this frame.
        let st = unsafe { &*(self.state as *const WindowState) };
        // SAFETY: checked for null; the plugin passes its live view and a rect.
        unsafe {
            let (w, h) = st.outer_size(*new_size);
            // The window's own WM_SIZE stays quiet: the view hears of its size once, here.
            st.quiet.set(true);
            SetWindowPos(
                self.hwnd as HWND,
                std::ptr::null_mut(),
                0,
                0,
                w,
                h,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            st.quiet.set(false);
            st.layout(
                (*new_size).right - (*new_size).left,
                (*new_size).bottom - (*new_size).top,
            );
            if let Some(v) = vst3::ComRef::from_raw(view) {
                v.onSize(new_size);
            }
            st.size.set(*new_size);
        }
        self.resizes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        kResultOk
    }
}

/// What `Editor::capture_bmp` saw.
#[derive(Clone, Copy, Debug)]
pub struct Capture {
    pub width: u32,
    pub height: u32,
    /// Distinct pixel colours; 1 means a blank (often black) capture.
    pub distinct_colors: usize,
}

/// An open editor. Dropping it detaches the view and destroys the window.
pub struct Editor {
    hwnd: HWND,
    keys: Box<crate::editor_keys::Keys>,
    view: ComPtr<IPlugView>,
    frame: ComWrapper<PlugFrame>,
    state: Box<WindowState>,
    took_foreground: bool,
    /// The caption colours the system took (`None`: none given, or an older Windows).
    caption: Cell<Option<Caption>>,
    /// The editor runs the thread's messages itself when it opens and closes (no GUI loop
    /// runs them).
    pumps: bool,
    _shared: Arc<Shared>,
    _apartment: crate::owner::Apartment,
}

impl Plugin {
    pub fn set_editor_bar(&self, bar: crate::EditorBar) {
        *self
            .shared()
            .editor_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = bar;
    }

    pub fn take_editor_actions(&self) -> Vec<crate::EditorAction> {
        std::mem::take(
            &mut *self
                .shared()
                .editor_actions
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }

    pub fn set_editor_area(&self, area: crate::EditorArea) {
        self.shared()
            .editor_position
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .area = Some(area);
    }

    /// Opens the plugin's editor in a new window titled `title`, at the bottom of the z-order
    /// (off every screen while [`OFFSCREEN_ENV`] is set). Call it on the thread that will
    /// pump messages (`Editor::pump`).
    pub fn open_editor(&self, title: &str) -> Result<Editor, Error> {
        let place = if offscreen_wanted() {
            Placement::offscreen()
        } else {
            Placement::default()
        };
        self.open(title, place, true)
    }

    /// Opens the plugin's editor in a new window titled `title`, where `place` says, from a
    /// thread whose GUI loop runs the window's messages: nothing is pumped here, so the GUI's
    /// handlers never run inside the caller's.
    pub fn open_editor_at(&self, title: &str, place: Placement) -> Result<Editor, Error> {
        self.open(title, place, false)
    }

    fn open(&self, title: &str, place: Placement, pump: bool) -> Result<Editor, Error> {
        self.open_frame(title, place, pump, false)
    }

    fn open_frame(
        &self,
        title: &str,
        place: Placement,
        pump: bool,
        native_frame: bool,
    ) -> Result<Editor, Error> {
        let started = Instant::now();
        let trace = std::env::var_os("XIRIUS_PLUGIN_HOST_LOG").is_some();
        let controller = self
            .shared()
            .controller
            .as_ref()
            .ok_or_else(|| error("the plugin has no edit controller"))?;
        let apartment = crate::owner::Apartment::new().map_err(error)?;
        // SAFETY: createView returns a reference we own, or null.
        let view = unsafe { ComPtr::from_raw(controller.createView(ViewType::kEditor)) }
            .ok_or_else(|| error("the plugin has no editor view"))?;
        if trace {
            eprintln!("plugin editor: createView in {:?}", started.elapsed());
        }
        // SAFETY: plain queries.
        if unsafe { view.isPlatformTypeSupported(kPlatformTypeHWND) } != kResultOk {
            return Err(error("the editor does not support HWND"));
        }
        let mut size = ViewRect {
            left: 0,
            top: 0,
            right: 400,
            bottom: 300,
        };
        // SAFETY: `size` is owned here. Plugins without a size keep the default.
        unsafe { view.getSize(&mut size) };
        // SAFETY: a plain query.
        let style = if unsafe { view.canResize() } == kResultOk {
            RESIZABLE
        } else {
            STYLE
        };
        let style = if native_frame {
            style | windows_sys::Win32::UI::WindowsAndMessaging::WS_CAPTION
        } else {
            style
        };
        let scale =
            unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() }.max(96) as f32 / 96.0;
        let (w, h) = (
            (size.right - size.left).max((crate::editor_chrome::MIN_WIDTH * scale).round() as i32),
            size.bottom - size.top + crate::editor_chrome::height(scale),
        );
        let external = self
            .shared()
            .editor_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stack
            .clone();
        let (x, y) = {
            let mut position = self
                .shared()
                .editor_position
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if place.area.is_some() {
                position.area = place.area;
            }
            if place.top && !offscreen_wanted() {
                position.area.map_or_else(
                    || place.origin(w, h),
                    |area| {
                        area.origin(
                            (w, h),
                            place.at.or(area.restore).or(position.saved),
                            &external,
                            (12.0 * scale).round() as i32,
                        )
                    },
                )
            } else {
                place.origin(w, h)
            }
        };
        let owner = place.owner.unwrap_or(0) as HWND;
        let t = wide(title);
        let before = unsafe { GetForegroundWindow() };
        // SAFETY: a window of our registered class; all strings are NUL-terminated.
        let hwnd = unsafe {
            CreateWindowExW(
                frame_style(place),
                class_name().as_ptr(),
                t.as_ptr(),
                style,
                x,
                y,
                w,
                h,
                owner,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(error(format!(
                "CreateWindowEx failed ({})",
                std::io::Error::last_os_error()
            )));
        }
        // SAFETY: this frame alone; older Windows simply ignore the unsupported attribute.
        unsafe {
            use windows_sys::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
            let corner = DWMWCP_ROUND;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &corner as *const _ as _,
                std::mem::size_of_val(&corner) as u32,
            );
        }
        // Before the window is first shown, so its caption is never seen in another colour.
        let caption = place.caption.filter(|c| c.apply(hwnd));
        let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
        // SAFETY: the plug-in viewport is a child on the SAME GUI thread as this frame.
        let viewport = unsafe {
            CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::WS_CHILD
                    | windows_sys::Win32::UI::WindowsAndMessaging::WS_VISIBLE
                    | windows_sys::Win32::UI::WindowsAndMessaging::WS_CLIPCHILDREN,
                0,
                crate::editor_chrome::height(scale),
                size.right - size.left,
                size.bottom - size.top,
                hwnd,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        if viewport.is_null() {
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(error("cannot create plug-in viewport"));
        }
        // SAFETY: subclass only our own child on this GUI thread; no borrowed callback data.
        if unsafe {
            windows_sys::Win32::UI::Shell::SetWindowSubclass(viewport, Some(viewport_proc), 1, 0)
        } == 0
        {
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(error("cannot route plug-in viewport keys"));
        }
        let state = Box::new(WindowState {
            flashing: Cell::new(false),
            presets: Default::default(),
            native_frame,
            viewport,
            chrome: crate::editor_chrome::Controls::new(hwnd),
            shared: self.shared().clone(),
            chrome_scale: Cell::new(scale),
            chrome_caption: Cell::new(place.caption),
            view: view.as_ptr(),
            style,
            live: Cell::new(false),
            quiet: Cell::new(false),
            size: Cell::new(size),
            scale: Cell::new(None),
        });
        // SAFETY: our window; the editor clears this before the box is freed.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const WindowState as isize) };
        let Some(keys) = crate::editor_keys::Keys::install(hwnd, place.top) else {
            // SAFETY: this is our unattached window.
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(error("cannot install editor keyboard routing"));
        };
        let frame = ComWrapper::new(PlugFrame {
            hwnd: hwnd as usize,
            state: &*state as *const WindowState as usize,
            resizes: Default::default(),
        });
        let frame_ptr = frame.as_com_ref::<IPlugFrame>().unwrap().as_ptr();
        // SAFETY: the frame and the window outlive the view's attachment (see Drop).
        let attached = unsafe {
            view.setFrame(frame_ptr);
            view.attached(viewport, kPlatformTypeHWND)
        };
        if trace {
            eprintln!("plugin editor: attached in {:?}", started.elapsed());
        }
        let mut editor = Editor {
            hwnd,
            keys,
            view,
            frame,
            state,
            took_foreground: false,
            caption: Cell::new(caption),
            pumps: pump,
            _shared: self.shared().clone(),
            _apartment: apartment,
        };
        if attached != kResultOk {
            drop(editor);
            if !native_frame {
                return self.open_frame(title, place, pump, true);
            }
            return Err(error("IPlugView::attached failed"));
        }
        // SAFETY: the plugin may have changed its size while attaching.
        unsafe {
            let mut now = editor.state.size.get();
            editor.view.getSize(&mut now);
            editor.state.size.set(now);
        }
        editor.state.live.set(true);
        // SAFETY: a plain query on our window (96 in a process that is not DPI aware).
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        editor.state.set_scale(dpi.max(96) as f32 / 96.0);
        let now = editor.state.size.get();
        let (width, height) = editor.state.outer_size(now);
        // Attachment and the monitor's DPI can change the view size. Clamp the final
        // frame, including its bar, rather than only the pre-attachment estimate.
        let final_origin = if place.top && !offscreen_wanted() {
            let position = self
                .shared()
                .editor_position
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            position.area.map(|area| {
                area.origin(
                    (width, height),
                    place.at.or(area.restore).or(position.saved),
                    &external,
                    (12.0 * dpi.max(96) as f32 / 96.0).round() as i32,
                )
            })
        } else {
            None
        };
        editor.state.quiet.set(true);
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                final_origin.map_or(0, |p| p.0),
                final_origin.map_or(0, |p| p.1),
                width,
                height,
                SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | if final_origin.is_none() {
                        SWP_NOMOVE
                    } else {
                        0
                    },
            );
        }
        editor.state.quiet.set(false);
        editor
            .state
            .layout(now.right - now.left, now.bottom - now.top);
        // Keep the native frame hidden until its view is attached and laid out. The
        // studio can show a loading window while the plug-in blocks in attached().
        let z = if place.top { HWND_TOP } else { HWND_BOTTOM };
        // SAFETY: the frame and its attached view belong to this GUI thread.
        unsafe {
            SetWindowPos(
                hwnd,
                z,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        };
        editor.state.touched(hwnd);
        // A GUI's own loop runs the messages; pumping here would run its handlers inside the
        // caller's.
        if pump {
            editor.pump(Duration::from_millis(50));
        }
        let after = unsafe { GetForegroundWindow() };
        editor.took_foreground = after != before && after == hwnd;
        Ok(editor)
    }
}

impl Editor {
    /// True when the common frame had to retain the OS caption for this product.
    pub fn uses_native_frame(&self) -> bool {
        self.state.native_frame
    }
    pub fn refresh_bar(&self) {
        let bar = self
            ._shared
            .editor_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let scale = self.state.chrome_scale.get();
        let mut bounds = RECT::default();
        // SAFETY: this frame and its controls belong to the host GUI thread.
        unsafe {
            GetClientRect(self.hwnd, &mut bounds);
        }
        self.state.chrome.layout(bounds.right, scale, &bar);
        bounds.bottom = crate::editor_chrome::height(scale);
        // SAFETY: the rectangle includes the chrome controls and ends above the viewport.
        unsafe {
            windows_sys::Win32::Graphics::Gdi::RedrawWindow(
                self.hwnd,
                &bounds,
                std::ptr::null_mut(),
                windows_sys::Win32::Graphics::Gdi::RDW_INVALIDATE
                    | windows_sys::Win32::Graphics::Gdi::RDW_ALLCHILDREN,
            );
        }
    }

    pub fn set_bar(&self, bar: crate::EditorBar) {
        let mut old = self
            ._shared
            .editor_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // A raise updates the stacking protocol even when the chrome stays the same.
        old.stack.clone_from(&bar.stack);
        old.stack_base = bar.stack_base;
        old.raise_ticket = bar.raise_ticket;
        if *old != bar {
            *old = bar;
            drop(old);
            self.refresh_bar();
        }
    }

    pub fn take_actions(&self) -> Vec<crate::EditorAction> {
        std::mem::take(
            &mut *self
                ._shared
                .editor_actions
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }
    /// Plain Space presses from the editor frame, consumed by its host. Text fields keep Space.
    pub fn take_play_requests(&self) -> u32 {
        self.keys.take()
    }
    /// Runs this thread's message loop for `duration`, so the editor can draw and run timers.
    pub fn pump(&mut self, duration: Duration) {
        let end = Instant::now() + duration;
        loop {
            // SAFETY: a standard PeekMessage loop on a zeroed MSG.
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if Instant::now() >= end {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// As [`Editor::pump`], for this window and its children only: a caller inside another
    /// window's message handler lets the plugin draw without that window's messages coming
    /// back into it.
    pub fn pump_own(&mut self, duration: Duration) {
        let end = Instant::now() + duration;
        loop {
            // SAFETY: a PeekMessage loop on a zeroed MSG, filtered to our own window.
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, self.hwnd, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if Instant::now() >= end {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// False once the user closed the window with its close box (which only hides it); the
    /// owner then drops the editor.
    pub fn is_visible(&self) -> bool {
        // SAFETY: a plain query on our window.
        unsafe { IsWindowVisible(self.hwnd) != 0 }
    }

    /// The view's size in pixels.
    pub fn size(&self) -> (i32, i32) {
        let s = self.state.size.get();
        (s.right - s.left, s.bottom - s.top)
    }

    /// The content scale the view was given, or `None` when it takes none
    /// (`IPlugViewContentScaleSupport`).
    pub fn content_scale(&self) -> Option<f32> {
        self.state.scale.get()
    }

    /// Passes a content scale to the view, as when the window moves to a monitor with another
    /// scale. False when the view takes none.
    pub fn set_content_scale(&mut self, factor: f32) -> bool {
        self.state.set_scale(factor)
    }

    /// Sizes the view's area to about `w` × `h` the way a drag of the bottom-right corner
    /// does (the view's constraint applies), and returns the size the view took. A view that
    /// cannot resize keeps its size.
    pub fn drag_to(&mut self, w: i32, h: i32) -> (i32, i32) {
        if self.state.style & WS_THICKFRAME != 0 {
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            let (bw, bh) = self.state.frame_size();
            // SAFETY: messages to our own window, with a rect that outlives the call.
            unsafe {
                GetWindowRect(self.hwnd, &mut r);
                r.right = r.left + w + bw;
                r.bottom = r.top + h + bh;
                SendMessageW(
                    self.hwnd,
                    WM_SIZING,
                    WMSZ_BOTTOMRIGHT as WPARAM,
                    &mut r as *mut RECT as LPARAM,
                );
                SetWindowPos(
                    self.hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        self.size()
    }

    /// Whether the window became the foreground window when it opened. The window is created
    /// so this should not happen; a plugin that activates itself would cause it.
    pub fn took_foreground(&self) -> bool {
        self.took_foreground
    }

    /// Resize requests the plugin made through the frame.
    pub fn resize_requests(&self) -> u32 {
        self.frame
            .resizes
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn can_resize(&self) -> bool {
        // SAFETY: a plain query.
        unsafe { self.view.canResize() == kResultOk }
    }

    /// Renders the client area with `PrintWindow` (no screen grab, so other windows on top do
    /// not matter) and writes it to `path` as a 32-bit BMP.
    pub fn capture_bmp(&self, path: &Path) -> Result<Capture, Error> {
        capture_window(self.hwnd as isize, path)
    }

    /// The window's handle (an `HWND`).
    pub fn hwnd(&self) -> isize {
        self.hwnd as isize
    }

    /// Parent of the plug-in view, excluding the common title bar.
    pub fn viewport_hwnd(&self) -> isize {
        self.state.viewport as isize
    }

    /// Gives the window's caption these colours (the theme was changed).
    pub fn set_caption(&self, caption: Caption) {
        self.caption
            .set(caption.apply(self.hwnd).then_some(caption));
        self.state.chrome_caption.set(Some(caption));
        self.refresh_bar();
    }

    /// The caption colours the system took for the window; `None` when none were given or it
    /// took none (an older Windows).
    pub fn caption(&self) -> Option<Caption> {
        self.caption.get()
    }

    /// Whether the system draws the window's frame dark, asked of the system itself.
    pub fn dark_frame(&self) -> bool {
        Caption::dark_frame(self.hwnd)
    }

    /// Brings a hidden or buried window back above the others, without activating it.
    pub fn raise(&self) {
        self.raise_from(0);
    }

    /// Restores the stack and, for an explicit open, takes the foreground from the
    /// window that requested it if that window is still active.
    pub fn raise_from(&self, foreground_from: u64) {
        let bar = self
            ._shared
            .editor_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        // SAFETY: queries do not send messages to another GUI thread. Only our own frame moves.
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GW_HWNDNEXT, GW_HWNDPREV, GW_OWNER, GWL_EXSTYLE, GetWindow, SetForegroundWindow,
                WS_EX_TOPMOST,
            };
            let mut above = Vec::new();
            let has_base = IsWindow(bar.stack_base as HWND) != 0;
            let mut cursor = if has_base {
                GetWindow(bar.stack_base as HWND, GW_HWNDPREV)
            } else {
                windows_sys::Win32::UI::WindowsAndMessaging::GetTopWindow(std::ptr::null_mut())
            };
            while !cursor.is_null() && above.len() < 4096 {
                above.push(cursor as u64);
                cursor = GetWindow(cursor, if has_base { GW_HWNDPREV } else { GW_HWNDNEXT });
            }
            let rank = bar
                .stack
                .iter()
                .position(|&h| h == self.hwnd as u64)
                .unwrap_or(0);
            let foreground = GetForegroundWindow();
            let ceiling = (foreground_from == 0
                && has_base
                && GetWindow(foreground, GW_OWNER) == bar.stack_base as HWND)
                .then(|| above.iter().position(|&h| h == foreground as u64))
                .flatten();
            let after = bar.stack[..rank]
                .iter()
                .rev()
                .find(|h| {
                    above
                        .iter()
                        .take(ceiling.unwrap_or(above.len()))
                        .any(|w| w == *h)
                })
                .copied()
                .or_else(|| ceiling.map(|_| foreground as u64))
                .or_else(|| {
                    // A background GUI's HWND_TOP can leave its frame below the active
                    // window. Insert after a concrete normal window above the base.
                    if !has_base {
                        return None;
                    }
                    above.iter().copied().find(|&h| {
                        h != self.hwnd as u64
                            && !bar.stack.contains(&h)
                            && GetWindowLongPtrW(h as HWND, GWL_EXSTYLE) & WS_EX_TOPMOST as isize
                                == 0
                    })
                })
                .map_or(HWND_TOP, |h| h as HWND);
            SetWindowPos(
                self.hwnd,
                after,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            if foreground_from != 0
                && GetForegroundWindow() as u64 == foreground_from
                && GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) & WS_EX_NOACTIVATE as isize == 0
            {
                SetForegroundWindow(self.hwnd);
                if windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(
                    self.hwnd,
                    FRAME_FLASH_TIMER,
                    420,
                    None,
                ) != 0
                {
                    self.state.flashing.set(true);
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                        self.hwnd,
                        &crate::editor_chrome::focus_bounds(
                            self.hwnd,
                            self.state.chrome_scale.get(),
                        ),
                        0,
                    );
                }
            }
        };
        self._shared
            .editor_actions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(crate::EditorAction::EditorRaised(bar.raise_ticket));
    }

    /// The window's top-left corner on the screen.
    pub fn origin(&self) -> (i32, i32) {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: a plain query on our window.
        unsafe { GetWindowRect(self.hwnd, &mut r) };
        (r.left, r.top)
    }
}

/// Renders the client area of any window of this process or another (`hwnd`) with
/// `PrintWindow` and writes it to `path` as a 32-bit BMP: tests use it to see that a window
/// they did not open themselves really draws.
pub fn capture_window(hwnd: isize, path: &Path) -> Result<Capture, Error> {
    let hwnd = hwnd as HWND;
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: GDI calls on a live window with objects released before returning.
    let pixels = unsafe {
        if IsWindow(hwnd) == 0 {
            return Err(error("no such window"));
        }
        GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        if w <= 0 || h <= 0 {
            return Err(error("the window has no client area"));
        }
        {
            let screen = GetDC(hwnd);
            let dc = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(dc, bmp);
            let mut class = [0u16; 64];
            let count = windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW(
                hwnd,
                class.as_mut_ptr(),
                class.len() as i32,
            );
            let native = String::from_utf16_lossy(&class[..count as usize]);
            // Full-content DWM capture can return stale/black child buttons offscreen.
            // Our GDI frame implements WM_PRINTCLIENT and prints synchronously instead.
            let flags = if native == "XiriusPluginEditor" {
                1
            } else {
                PRINT_FLAGS
            };
            let printed = PrintWindow(hwnd, dc, flags) != 0;
            SelectObject(dc, old);
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut px = vec![0u32; (w * h) as usize];
            let lines = GetDIBits(
                dc,
                bmp,
                0,
                h as u32,
                px.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );
            DeleteObject(bmp);
            DeleteDC(dc);
            ReleaseDC(hwnd, screen);
            if !printed || lines != h {
                return Err(error("PrintWindow or GetDIBits failed"));
            }
            (w as u32, h as u32, px)
        }
    };
    let (w, h, px) = pixels;
    write_bmp(path, w, h, &px).map_err(|e| error(format!("{}: {e}", path.display())))?;
    let distinct = px
        .iter()
        .map(|p| p & 0x00ff_ffff)
        .collect::<HashSet<_>>()
        .len();
    Ok(Capture {
        width: w,
        height: h,
        distinct_colors: distinct,
    })
}

/// A top-down 32-bit BMP.
fn write_bmp(path: &Path, w: u32, h: u32, px: &[u32]) -> std::io::Result<()> {
    let data = (w * h * 4) as usize;
    let mut out = Vec::with_capacity(54 + data);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for p in px {
        out.extend_from_slice(&p.to_le_bytes());
    }
    std::fs::write(path, out)
}

impl Drop for Editor {
    fn drop(&mut self) {
        if !offscreen_wanted() {
            self._shared
                .editor_position
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .saved = Some(self.origin());
        }
        // SAFETY: detach in the SDK's order, then destroy our window. The window forgets the
        // state first, so no message reaches a view that is going.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            self.view.removed();
            self.view.setFrame(std::ptr::null_mut());
            crate::editor_keys::Keys::remove(self.hwnd);
            DestroyWindow(self.hwnd);
        }
        if self.pumps {
            self.pump(Duration::from_millis(10));
        }
    }
}
