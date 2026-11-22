//! Physical screen coordinates, passed by value across the GUI boundary (UI-337).
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, RECT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromRect,
    MonitorFromWindow,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorArea {
    /// Used only to read z order; never assigned as an owner or parent.
    pub main_window: isize,
    pub main: [i32; 4],
    pub work: [i32; 4],
    pub restore: Option<(i32, i32)>,
    pub replacing: bool,
}

impl EditorArea {
    #[cfg(not(target_os = "windows"))]
    pub fn from_window(_handle: isize) -> Option<Self> {
        None
    }

    /// Read only the app's own HWND, before queuing the snapshot to a plugin.
    #[cfg(target_os = "windows")]
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
                replacing: false,
            })
        }
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn origin(
        self,
        size: (i32, i32),
        saved: Option<(i32, i32)>,
        external: &[u64],
        gap: i32,
    ) -> (i32, i32) {
        if self.replacing {
            return self.fit(size, saved, Some(self.work));
        }
        let occupied = visible_editors(self.main_window, external);
        self.place_new(size, saved, saved.and_then(monitor_work), &occupied, gap)
    }

    #[cfg(any(target_os = "windows", test))]
    fn place_new(
        self,
        size: (i32, i32),
        saved: Option<(i32, i32)>,
        saved_work: Option<[i32; 4]>,
        occupied: &[[i32; 4]],
        gap: i32,
    ) -> (i32, i32) {
        // An editor opened on its own can reuse its saved monitor. Additional editors
        // look for room on the monitor from which the user opens them.
        let work = saved_work.filter(|work| occupied.is_empty() || *work == self.work);
        let origin = self.fit(size, saved, work);
        place_beside(origin, size, work.unwrap_or(self.work), occupied, gap)
    }

    #[cfg(any(target_os = "windows", test))]
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

#[cfg(target_os = "windows")]
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

/// Places a new window around the open windows on its monitor, keeping its title bar
/// reachable even on monitors too small for the windows to fit side by side.
pub fn place_beside(
    origin: (i32, i32),
    (width, height): (i32, i32),
    [left, top, right, bottom]: [i32; 4],
    occupied: &[[i32; 4]],
    gap: i32,
) -> (i32, i32) {
    let max_x = right.saturating_sub(width).max(left);
    let max_y = bottom.saturating_sub(height).max(top);
    let clamp = |(x, y): (i32, i32)| (x.clamp(left, max_x), y.clamp(top, max_y));
    let origin = clamp(origin);
    let occupied: Vec<_> = occupied
        .iter()
        .copied()
        .filter(|&[l, t, r, b]| r > left && l < right && b > top && t < bottom)
        .collect();
    let clear = |(x, y): (i32, i32)| {
        occupied.iter().all(|&[l, t, r, b]| {
            x >= r || y >= b || x.saturating_add(width) <= l || y.saturating_add(height) <= t
        })
    };
    if occupied.is_empty() {
        return origin;
    }
    for direction in 0..4 {
        for &[l, t, r, b] in &occupied {
            let candidate = match direction {
                0 => (r.saturating_add(gap), t.clamp(top, max_y)),
                1 => (
                    l.saturating_sub(gap).saturating_sub(width),
                    t.clamp(top, max_y),
                ),
                2 => (l.clamp(left, max_x), b.saturating_add(gap)),
                _ => (
                    l.clamp(left, max_x),
                    t.saturating_sub(gap).saturating_sub(height),
                ),
            };
            if clamp(candidate) == candidate && clear(candidate) {
                return candidate;
            }
        }
    }
    if clear(origin) {
        return origin;
    }
    // A gap may be too small for a margin, or offset from the adjacent editor.
    // Any empty rectangle can slide left and up to these edges.
    for x in std::iter::once(left).chain(occupied.iter().map(|r| r[2])) {
        for y in std::iter::once(top).chain(occupied.iter().map(|r| r[3])) {
            if clamp((x, y)) == (x, y) && clear((x, y)) {
                return (x, y);
            }
        }
    }
    let [l, t, _, _] = occupied[0];
    let offset = gap.saturating_mul(3);
    let shifted = clamp((l.saturating_add(offset), t.saturating_add(offset)));
    if shifted == (l, t) {
        clamp((l.saturating_sub(offset), t.saturating_sub(offset)))
    } else {
        shifted
    }
}

/// Visible, independent Studio windows and the external top-level editors reported by
/// the host. The loading placeholders and temporary preset forms have their own policy.
#[cfg(target_os = "windows")]
pub fn visible_editors(main: isize, external: &[u64]) -> Vec<[i32; 4]> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GW_HWNDNEXT, GW_OWNER, GetTopWindow, GetWindow, GetWindowTextW, IsWindowVisible,
    };
    let rect = |hwnd| {
        let mut r = RECT::default();
        // SAFETY: both calls are read-only on a native handle.
        (unsafe { IsWindowVisible(hwnd) != 0 && GetWindowRect(hwnd, &mut r) != 0 })
            .then_some([r.left, r.top, r.right, r.bottom])
    };
    let mut bounds: Vec<_> = external.iter().filter_map(|&id| rect(id as HWND)).collect();
    if main == 0 {
        return bounds;
    }
    // SAFETY: get the OS z order and read the title only for windows owned by Studio.
    unsafe {
        let mut hwnd = GetTopWindow(std::ptr::null_mut());
        while !hwnd.is_null() {
            if GetWindow(hwnd, GW_OWNER) == main as HWND
                && let Some(r) = rect(hwnd)
            {
                let mut title = [0u16; 64];
                let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) as usize;
                let title = String::from_utf16_lossy(&title[..len]);
                if title
                    .strip_prefix("Xirius · ")
                    .is_some_and(|name| !name.contains(" · "))
                {
                    bounds.push(r);
                }
            }
            hwnd = GetWindow(hwnd, GW_HWNDNEXT);
        }
    }
    bounds
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
        replacing: false,
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
    fn placement_checks_sides_in_order_and_avoids_every_editor() {
        let work = [0, 0, 1200, 800];
        let center = [400, 200, 800, 550];
        let right = [810, 200, 1200, 550];
        let left = [0, 200, 390, 550];
        let below = [0, 560, 1200, 800];
        let place =
            |occupied: &[[i32; 4]]| place_beside((400, 200), (320, 170), work, occupied, 10);
        assert_eq!(place(&[center]), (810, 200));
        assert_eq!(place(&[center, right]), (70, 200));
        assert_eq!(place(&[center, right, left]), (400, 560));
        assert_eq!(place(&[center, right, left, below]), (400, 20));
        assert_eq!(place(&[[1200, 0, 1500, 500]]), (400, 200));
        assert_eq!(
            place_beside(
                (0, 0),
                (320, 170),
                [0, 0, 640, 170],
                &[[0, 0, 320, 170]],
                10
            ),
            (320, 0)
        );
    }

    #[test]
    fn removed_monitor_returns_to_the_current_main_monitor() {
        assert_eq!(AREA.fit((500, 300), Some((-1200, 90)), None), (2350, 300));
        assert_eq!(AREA.fit((5000, 3000), None, None), (1920, 0));
    }

    #[test]
    fn an_additional_editor_stays_on_the_source_monitor() {
        let saved = Some((-1200, 90));
        let saved_work = Some([-1920, 0, 0, 1040]);
        let size = (500, 300);
        assert_eq!(
            AREA.place_new(size, saved, saved_work, &[], 10),
            (-1200, 90)
        );
        assert_eq!(
            AREA.place_new(size, saved, saved_work, &[[2300, 200, 2800, 500]], 10),
            (2810, 200)
        );
    }
}
