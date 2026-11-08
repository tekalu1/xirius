//! Explicitly unavailable until native NSView hosting is implemented.
use crate::{Error, Plugin, error};
use std::{path::Path, time::Duration};
pub const OFFSCREEN_ENV: &str = "XIRIUS_EDITOR_OFFSCREEN";
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

#[derive(Clone, Copy, Debug)]
pub struct Capture {
    pub width: u32,
    pub height: u32,
    /// Distinct pixel colours; 1 means a blank (often black) capture.
    pub distinct_colors: usize,
}

impl Placement {
    pub fn shown(owner: Option<isize>) -> Self {
        Self {
            owner,
            top: true,
            ..Self::default()
        }
    }
    pub fn offscreen() -> Self {
        Self::default()
    }
}
/// No native editor can be constructed on this platform yet.
pub enum Editor {}
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

    pub fn open_editor(&self, _title: &str) -> Result<Editor, Error> {
        Err(unavailable())
    }
    pub fn open_editor_at(&self, _title: &str, _place: Placement) -> Result<Editor, Error> {
        Err(unavailable())
    }
}
fn unavailable() -> Error {
    error("VST3 editor windows are not available on macOS yet (NSView hosting requires stage 3)")
}
pub fn capture_window(_handle: isize, _path: &Path) -> Result<Capture, Error> {
    Err(unavailable())
}
impl Editor {
    pub fn uses_native_frame(&self) -> bool {
        match *self {}
    }
    pub fn refresh_bar(&self) {
        match *self {}
    }
    pub fn set_bar(&self, _bar: crate::EditorBar) {
        match *self {}
    }
    pub fn take_actions(&self) -> Vec<crate::EditorAction> {
        match *self {}
    }
    pub fn take_play_requests(&self) -> u32 {
        match *self {}
    }
    pub fn pump(&mut self, _duration: Duration) {
        match *self {}
    }
    pub fn pump_own(&mut self, _duration: Duration) {
        match *self {}
    }
    pub fn is_visible(&self) -> bool {
        match *self {}
    }
    pub fn size(&self) -> (i32, i32) {
        match *self {}
    }
    pub fn content_scale(&self) -> Option<f32> {
        match *self {}
    }
    pub fn set_content_scale(&mut self, _factor: f32) -> bool {
        match *self {}
    }
    pub fn drag_to(&mut self, _w: i32, _h: i32) -> (i32, i32) {
        match *self {}
    }
    pub fn took_foreground(&self) -> bool {
        match *self {}
    }
    pub fn resize_requests(&self) -> u32 {
        match *self {}
    }
    pub fn can_resize(&self) -> bool {
        match *self {}
    }
    pub fn capture_bmp(&self, _path: &Path) -> Result<Capture, Error> {
        match *self {}
    }
    pub fn hwnd(&self) -> isize {
        match *self {}
    }
    pub fn viewport_hwnd(&self) -> isize {
        match *self {}
    }
    pub fn set_caption(&self, _caption: Caption) {
        match *self {}
    }
    pub fn caption(&self) -> Option<Caption> {
        match *self {}
    }
    pub fn dark_frame(&self) -> bool {
        match *self {}
    }
    pub fn raise(&self) {
        match *self {}
    }
    pub fn origin(&self) -> (i32, i32) {
        match *self {}
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        match *self {}
    }
}
