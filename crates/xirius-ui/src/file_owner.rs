//! An explicit owner for asynchronous native preset dialogs (UI-337).
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use std::num::NonZeroIsize;

pub(crate) struct Owner(NonZeroIsize);
impl Owner {
    pub fn new(hwnd: isize) -> Option<Self> {
        // SAFETY: a query; a stale editor handle fails without calling its window procedure.
        if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(hwnd as _) } == 0 {
            return None;
        }
        NonZeroIsize::new(hwnd).map(Self)
    }
}
impl HasWindowHandle for Owner {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the dialog copies this native handle when constructed; it does not retain this wrapper.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(self.0)))
        })
    }
}
impl HasDisplayHandle for Owner {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: the Windows display handle contains no borrowed data.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Windows(WindowsDisplayHandle::new()))
        })
    }
}
