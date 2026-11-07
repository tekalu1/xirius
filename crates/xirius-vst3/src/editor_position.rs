//! Physical screen coordinates, passed by value across the GUI boundary (UI-337).
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromRect,
    MonitorFromWindow,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorArea {
    /// Used only to read z order; never assigned as an owner or parent.
    pub main_window: isize,
    pub main: [i32; 4],
    pub work: [i32; 4],
    pub restore: Option<(i32, i32)>,
}

impl EditorArea {
    /// Read only the app's own HWND, before queuing the snapshot to a plugin.
    pub fn from_window(hwnd: isize) -> Option<Self> {
        let mut r = RECT::default();
        // SAFETY: queries only; invalid handles fail without dereferencing them.
        unsafe {
            if GetWindowRect(hwnd as HWND, &mut r) == 0 {
                return None;
            }
            let monitor = MonitorFromWindow(hwnd as HWND, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return None;
            }
            let w = info.rcWork;
            Some(Self {
                main_window: hwnd,
                main: [r.left, r.top, r.right, r.bottom],
                work: [w.left, w.top, w.right, w.bottom],
                restore: None,
            })
        }
    }

    pub(crate) fn origin(self, size: (i32, i32), saved: Option<(i32, i32)>) -> (i32, i32) {
        let work = saved.and_then(monitor_work);
        self.fit(size, saved, work)
    }

    fn fit(
        self,
        (w, h): (i32, i32),
        saved: Option<(i32, i32)>,
        saved_work: Option<[i32; 4]>,
    ) -> (i32, i32) {
        let [l, t, r, b] = saved_work.unwrap_or(self.work);
        let [ml, mt, mr, mb] = self.main;
        let (x, y) = saved.filter(|_| saved_work.is_some()).unwrap_or((
            ml.saturating_add((mr.saturating_sub(ml).saturating_sub(w)) / 2),
            mt.saturating_add((mb.saturating_sub(mt).saturating_sub(h)) / 3),
        ));
        (
            x.clamp(l, r.saturating_sub(w).max(l)),
            y.clamp(t, b.saturating_sub(h).max(t)),
        )
    }
}

fn monitor_work((x, y): (i32, i32)) -> Option<[i32; 4]> {
    let r = RECT {
        left: x,
        top: y,
        right: x.saturating_add(1),
        bottom: y.saturating_add(1),
    };
    // SAFETY: query on a local rectangle, no window handles or plugin calls.
    unsafe {
        let monitor = MonitorFromRect(&r, MONITOR_DEFAULTTONULL);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let w = info.rcWork;
        Some([w.left, w.top, w.right, w.bottom])
    }
}

#[derive(Default)]
pub(crate) struct Position {
    pub area: Option<EditorArea>,
    pub saved: Option<(i32, i32)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    const AREA: EditorArea = EditorArea {
        main_window: 0,
        main: [2000, 100, 3200, 1000],
        work: [1920, 0, 3840, 1040],
        restore: None,
    };
    #[test]
    fn restores_the_instance_position_and_clamps_to_its_monitor() {
        assert_eq!(
            AREA.fit((500, 300), Some((-1200, 90)), Some([-1920, 0, 0, 1040])),
            (-1200, 90)
        );
        assert_eq!(
            AREA.fit((500, 300), Some((3800, 1000)), Some(AREA.work)),
            (3340, 740)
        );
    }
    #[test]
    fn removed_monitor_returns_to_the_current_main_monitor() {
        assert_eq!(AREA.fit((500, 300), Some((-1200, 90)), None), (2350, 300));
        assert_eq!(AREA.fit((5000, 3000), None, None), (1920, 0));
    }
}
