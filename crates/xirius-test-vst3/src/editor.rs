//! A resizable editor view for host tests (PLG-027): a child window painted with GDI that
//! keeps a size constraint and reports what the host told it in its window text, so a test in
//! the host can read the size and the content scale back with `GetWindowTextW`.
//!
//! It holds a label and a native text field (for the keyboard-routing tests of PLG-048), so a
//! host can type into a plugin's own control and check which keys come back to it. The label
//! shows the delay the instance has now, the process it runs in and which instance of that
//! process it is, so a picture of the window tells which instance it belongs to and what was
//! moved in it.
//!
//! The constraint: at least 200 × 150, at most 1600 × 1200, and the width a multiple of 10.

use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DEFAULT_GUI_FONT, DeleteObject, EndPaint, FillRect,
    GetStockObject, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect,
    GetWindowLongPtrW, KillTimer, RegisterClassExW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    SendMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, UnregisterClassW,
    WM_ERASEBKGND, WM_PAINT, WM_SETFONT, WM_TIMER, WNDCLASSEXW, WS_BORDER, WS_CHILD,
    WS_CLIPCHILDREN, WS_VISIBLE,
};

/// The timer that brings the label up to date.
const SHOW_TIMER: usize = 1;

/// What the label shows, kept on the view's window (`GWLP_USERDATA`) while it lives.
struct Shown {
    delay: Arc<AtomicU32>,
    number: u32,
    label: HWND,
}

impl Shown {
    fn show(&self) {
        let d = self.delay.load(Ordering::Relaxed);
        let text = wide(&format!(
            "delay {d} ({:.2})  pid {}  instance {}",
            f64::from(d) / 256.0,
            std::process::id(),
            self.number
        ));
        // SAFETY: the label is a child of the live view window.
        unsafe { SetWindowTextW(self.label, text.as_ptr()) };
    }
}

pub const MIN: (i32, i32) = (200, 150);
pub const MAX: (i32, i32) = (1600, 1200);
pub const STEP: i32 = 10;
/// The size at a content scale of 1.
pub const BASE: (i32, i32) = (400, 300);

/// What `checkSizeConstraint` does to a size.
pub fn constrain(w: i32, h: i32) -> (i32, i32) {
    let w = (w.clamp(MIN.0, MAX.0) / STEP) * STEP;
    (w, h.clamp(MIN.1, MAX.1))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn module() -> HINSTANCE {
    let mut m: HINSTANCE = std::ptr::null_mut();
    // SAFETY: looks up the module that contains this function, without a reference.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            module as *const () as *const u16,
            &mut m,
        )
    };
    m
}

fn class_name() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| wide("XiriusTestVst3View"))
}

unsafe extern "system" fn view_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_TIMER if wp == SHOW_TIMER => {
            // SAFETY: set in `attached` to a live `Shown`, cleared in `removed` before it goes.
            let shown = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Shown;
            if !shown.is_null() {
                // SAFETY: as above.
                unsafe { (*shown).show() };
            }
            0
        }
        WM_PAINT => {
            // SAFETY: a standard paint cycle on the window this procedure was called for.
            unsafe {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rc);
                // Four bands, so a capture that is not blank has more than one colour.
                let colors = [
                    0x00_30_20_18u32,
                    0x00_80_50_20,
                    0x00_20_a0_e0,
                    0x00_f0_f0_f0,
                ];
                let band = (rc.bottom - rc.top).max(4) / 4;
                for (i, c) in colors.iter().enumerate() {
                    let r = RECT {
                        left: rc.left,
                        top: rc.top + band * i as i32,
                        right: rc.right,
                        bottom: if i == 3 {
                            rc.bottom
                        } else {
                            rc.top + band * (i as i32 + 1)
                        },
                    };
                    let brush = CreateSolidBrush(*c);
                    FillRect(dc, &r, brush);
                    DeleteObject(brush);
                }
                EndPaint(hwnd, &ps);
            }
            0
        }
        // SAFETY: the default procedure for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// A label and a text field, children of the view's window (destroyed with it). Returns the
/// label.
fn controls(parent: HWND) -> HWND {
    let style = WS_CHILD | WS_VISIBLE;
    // SAFETY: ordinary child controls of a live window on this GUI thread.
    unsafe {
        let child = |class: &str, text: &str, style: u32, y: i32, h: i32| {
            CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                style,
                20,
                y,
                320,
                h,
                parent,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        let label = child("STATIC", "Xirius Test Latency", style, 20, 24);
        let text = child("EDIT", "", style | WS_BORDER, 60, 28);
        let font = GetStockObject(DEFAULT_GUI_FONT);
        for h in [label, text] {
            if !h.is_null() {
                SendMessageW(h, WM_SETFONT, font as usize, 1);
            }
        }
        label
    }
}

pub struct View {
    delay: Arc<AtomicU32>,
    number: u32,
    hwnd: Cell<usize>,
    size: Cell<(i32, i32)>,
    scale: Cell<f32>,
    frame: Cell<Option<ComPtr<IPlugFrame>>>,
    /// This object as an `IPlugView`, without a reference (for `resizeView`).
    me: Cell<usize>,
}

impl Class for View {
    type Interfaces = (IPlugView, IPlugViewContentScaleSupport);
}

/// A new view, as `IEditController::createView` returns it (one owned reference).
pub fn create(delay: Arc<AtomicU32>, number: u32) -> *mut IPlugView {
    let view = ComWrapper::new(View {
        delay,
        number,
        hwnd: Cell::new(0),
        size: Cell::new(BASE),
        scale: Cell::new(1.0),
        frame: Cell::new(None),
        me: Cell::new(0),
    });
    let ptr = view.to_com_ptr::<IPlugView>().unwrap().into_raw();
    view.me.set(ptr as usize);
    ptr
}

impl View {
    /// Moves the child window to the current size and writes the report into its text.
    fn sync(&self) {
        let hwnd = self.hwnd.get() as HWND;
        if hwnd.is_null() {
            return;
        }
        let (w, h) = self.size.get();
        let text = wide(&format!("size={w}x{h} scale={:.2}", self.scale.get()));
        // SAFETY: our own child window.
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                w,
                h,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            SetWindowTextW(hwnd, text.as_ptr());
        }
    }
}

impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, ty: FIDString) -> tresult {
        if ty.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host passes a NUL-terminated platform name.
        let ty = unsafe { std::ffi::CStr::from_ptr(ty) };
        if ty.to_bytes() == b"HWND" {
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn attached(&self, parent: *mut std::ffi::c_void, _: FIDString) -> tresult {
        if parent.is_null() || self.hwnd.get() != 0 {
            return kResultFalse;
        }
        let instance = module();
        // SAFETY: registers our class once per load of this module (a second call fails
        // harmlessly), then creates a child of the host's window.
        let hwnd = unsafe {
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(view_proc);
            wc.hInstance = instance;
            wc.lpszClassName = class_name().as_ptr();
            RegisterClassExW(&wc);
            let (w, h) = self.size.get();
            CreateWindowExW(
                0,
                class_name().as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                w,
                h,
                parent as HWND,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return kResultFalse;
        }
        self.hwnd.set(hwnd as usize);
        let shown = Box::new(Shown {
            delay: self.delay.clone(),
            number: self.number,
            label: controls(hwnd),
        });
        shown.show();
        // SAFETY: our own window; `removed` takes the box back before destroying it.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(shown) as isize);
            SetTimer(hwnd, SHOW_TIMER, 100, None);
        }
        self.sync();
        kResultOk
    }

    unsafe fn removed(&self) -> tresult {
        let hwnd = self.hwnd.replace(0) as HWND;
        if !hwnd.is_null() {
            // SAFETY: our own child window; the class goes once no window of it is left, so
            // the module can unload without leaving a procedure behind.
            unsafe {
                KillTimer(hwnd, SHOW_TIMER);
                let shown = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Shown;
                if !shown.is_null() {
                    drop(Box::from_raw(shown));
                }
                DestroyWindow(hwnd);
                UnregisterClassW(class_name().as_ptr(), module());
            }
        }
        kResultOk
    }

    unsafe fn onWheel(&self, _: f32) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyDown(&self, _: char16, _: int16, _: int16) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyUp(&self, _: char16, _: int16, _: int16) -> tresult {
        kResultFalse
    }

    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        let (w, h) = self.size.get();
        // SAFETY: checked output rect.
        unsafe {
            *size = ViewRect {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            }
        };
        kResultOk
    }

    unsafe fn onSize(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked input rect.
        let r = unsafe { *size };
        self.size.set((r.right - r.left, r.bottom - r.top));
        self.sync();
        kResultOk
    }

    unsafe fn onFocus(&self, _: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        // SAFETY: the host passes a live frame or null; we keep our own reference.
        self.frame
            .set(unsafe { vst3::ComRef::from_raw(frame) }.map(|f| f.to_com_ptr()));
        kResultOk
    }

    unsafe fn canResize(&self) -> tresult {
        kResultOk
    }

    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked in/out rect.
        unsafe {
            let r = &mut *rect;
            let (w, h) = constrain(r.right - r.left, r.bottom - r.top);
            r.right = r.left + w;
            r.bottom = r.top + h;
        }
        kResultOk
    }
}

impl IPlugViewContentScaleSupportTrait for View {
    unsafe fn setContentScaleFactor(&self, factor: f32) -> tresult {
        if !(0.5..=4.0).contains(&factor) {
            return kInvalidArgument;
        }
        let old = self.scale.replace(factor);
        if (old - factor).abs() < 1e-4 {
            self.sync();
            return kResultOk;
        }
        // Scales its own size and asks the host for the new one, as plugins do.
        let (w, h) = self.size.get();
        let (w, h) = constrain(
            (w as f32 * factor / old).round() as i32,
            (h as f32 * factor / old).round() as i32,
        );
        let frame = self.frame.take();
        if let Some(f) = &frame {
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            };
            // SAFETY: our own interface pointer, alive for the call; the host calls onSize back.
            unsafe { f.resizeView(self.me.get() as *mut IPlugView, &mut rect) };
        } else {
            self.size.set((w, h));
        }
        self.frame.set(frame);
        self.sync();
        kResultOk
    }
}
