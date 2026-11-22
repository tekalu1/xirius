//! Physical screen coordinates, passed by value across the GUI boundary (UI-337).
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO,
    MonitorFromRect, MonitorFromWindow,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::{
    HiDpi::GetDpiForWindow,
    WindowsAndMessaging::{GetClientRect, GetWindowRect},
};

pub const EDITOR_TITLE_HEIGHT: f32 = 36.0;

#[derive(Clone, Copy, Debug)]
pub struct EditorFrame {
    pub bounds: [i32; 4],
    title: [i32; 4],
}

impl EditorFrame {
    pub fn new(bounds: [i32; 4], title_height: i32) -> Self {
        let [l, t, r, b] = bounds;
        Self {
            bounds,
            title: [l, t, r, b.min(t.saturating_add(title_height))],
        }
    }

    fn title_points(self) -> [[i32; 4]; 3] {
        let [l, t, r, b] = self.title;
        let y = t + (b - t) / 2;
        let inset = ((b - t) / 2).min((r - l) / 2);
        [r - 1 - inset, l + inset, l + (r - l) / 2].map(|x| [x, y, x + 1, y + 1])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorArea {
    /// Used only to read z order; never assigned as an owner or parent.
    pub main_window: isize,
    pub main: [i32; 4],
    pub work: [i32; 4],
    pub restore: Option<(i32, i32)>,
    pub replacing: bool,
    pub retiring: Option<isize>,
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
                retiring: None,
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
        let occupied = visible_editors_except(self.main_window, external, self.retiring);
        self.place_new(size, saved, saved.and_then(monitor_work), &occupied, gap)
    }

    #[cfg(any(target_os = "windows", test))]
    fn place_new(
        self,
        size: (i32, i32),
        saved: Option<(i32, i32)>,
        saved_work: Option<[i32; 4]>,
        occupied: &[EditorFrame],
        gap: i32,
    ) -> (i32, i32) {
        // An editor opened on its own can reuse its saved monitor. Additional editors
        // look for room on the monitor from which the user opens them.
        let work = if self.replacing {
            Some(self.work)
        } else {
            saved_work.filter(|work| occupied.is_empty() || *work == self.work)
        };
        let origin = self.fit(size, saved, work);
        place_editor(
            origin,
            size,
            work.unwrap_or(self.work),
            occupied,
            gap,
            self.replacing || (saved.is_some() && work.is_some()),
        )
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

fn intersects([l, t, r, b]: [i32; 4], [x, y, right, bottom]: [i32; 4]) -> bool {
    l < right && t < bottom && r > x && b > y
}

/// A saved or inherited origin wins when the whole frame fits there.
pub fn place_editor(
    origin: (i32, i32),
    (width, height): (i32, i32),
    [left, top, right, bottom]: [i32; 4],
    occupied: &[EditorFrame],
    gap: i32,
    prefer_origin: bool,
) -> (i32, i32) {
    let max_x = right.saturating_sub(width).max(left);
    let max_y = bottom.saturating_sub(height).max(top);
    let clamp = |(x, y): (i32, i32)| (x.clamp(left, max_x), y.clamp(top, max_y));
    let origin = clamp(origin);
    let windows: Vec<_> = occupied
        .iter()
        .copied()
        .filter(|frame| intersects(frame.bounds, [left, top, right, bottom]))
        .collect();
    let occupied: Vec<_> = windows.iter().map(|frame| frame.bounds).collect();
    let frame_at = |(x, y): (i32, i32)| [x, y, x.saturating_add(width), y.saturating_add(height)];
    let clear = |point| {
        occupied
            .iter()
            .all(|&old| !intersects(frame_at(point), old))
    };
    if occupied.is_empty() || (prefer_origin && clear(origin)) {
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
    let preferred = if prefer_origin {
        origin
    } else {
        clamp((l.saturating_add(offset), t.saturating_add(offset)))
    };
    // Each title rules out a rectangle of origins. Checking its edges and the
    // work-area edges finds the nearest position in every remaining region.
    let candidates = |rectangles: &[[i32; 4]]| {
        let mut xs = vec![left, max_x, preferred.0];
        let mut ys = vec![top, max_y, preferred.1];
        for &[l, t, r, b] in rectangles {
            xs.extend([l.saturating_sub(width), r]);
            ys.extend([t.saturating_sub(height), b]);
        }
        xs.retain(|x| (left..=max_x).contains(x));
        ys.retain(|y| (top..=max_y).contains(y));
        xs.sort_unstable();
        xs.dedup();
        ys.sort_unstable();
        ys.dedup();
        xs.iter()
            .flat_map(|&x| ys.iter().map(move |&y| (x, y)))
            .collect::<Vec<_>>()
    };
    let distance = |(x, y): (i32, i32)| -> u64 {
        u64::from(x.abs_diff(preferred.0)) + u64::from(y.abs_diff(preferred.1))
    };
    let titles: Vec<_> = windows.iter().map(|old| old.title).collect();
    if let Some(point) = candidates(&titles)
        .into_iter()
        .filter(|&point| {
            titles
                .iter()
                .all(|&title| !intersects(frame_at(point), title))
        })
        .min_by_key(|&point| distance(point))
    {
        return point;
    }
    // A nearly screen-sized editor can make full bars impossible. Keep as many
    // close buttons and title points reachable as the work area permits.
    let probes: Vec<_> = windows.iter().flat_map(|old| old.title_points()).collect();
    candidates(&probes)
        .into_iter()
        .min_by_key(|&point| {
            let frame = frame_at(point);
            let hidden = |rect| intersects(frame, rect);
            let (titles, _) = probes.as_chunks::<3>();
            let closes = titles.iter().filter(|p| hidden(p[0])).count();
            let total = probes.iter().filter(|&&p| hidden(p)).count();
            (closes, total, distance(point))
        })
        .unwrap_or(preferred)
}

/// Visible independent Studio and host windows, excluding loading placeholders and
/// the editor that will close once its replacement is open.
#[cfg(target_os = "windows")]
pub fn visible_editors_except(
    main: isize,
    external: &[u64],
    retiring: Option<isize>,
) -> Vec<EditorFrame> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GW_HWNDNEXT, GW_OWNER, GetTopWindow, GetWindow, GetWindowTextW, IsWindowVisible,
    };
    let frame = |hwnd| {
        let mut r = RECT::default();
        let mut client = RECT::default();
        let mut at = POINT::default();
        // SAFETY: both calls are read-only on a native handle.
        unsafe {
            if IsWindowVisible(hwnd) == 0 || GetWindowRect(hwnd, &mut r) == 0 {
                return None;
            }
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            let height = (EDITOR_TITLE_HEIGHT * scale).round() as i32;
            let mut frame = EditorFrame::new([r.left, r.top, r.right, r.bottom], height);
            if GetClientRect(hwnd, &mut client) != 0 && ClientToScreen(hwnd, &mut at) != 0 {
                frame.title = [at.x, at.y, at.x + client.right, r.bottom.min(at.y + height)];
            }
            Some(frame)
        }
    };
    let mut bounds: Vec<_> = external
        .iter()
        .map(|&id| id as isize)
        .filter(|&hwnd| Some(hwnd) != retiring)
        .filter_map(|hwnd| frame(hwnd as HWND))
        .collect();
    if main == 0 {
        return bounds;
    }
    // SAFETY: get the OS z order and read the title only for windows owned by Studio.
    unsafe {
        let mut hwnd = GetTopWindow(std::ptr::null_mut());
        while !hwnd.is_null() {
            if Some(hwnd as isize) != retiring
                && GetWindow(hwnd, GW_OWNER) == main as HWND
                && let Some(r) = frame(hwnd)
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
        retiring: None,
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
    fn a_saved_origin_wins_only_while_the_full_frame_is_clear() {
        let place = |origin| {
            place_editor(
                origin,
                (300, 170),
                [0, 0, 1200, 800],
                &[EditorFrame::new([0, 0, 400, 700], 36)],
                10,
                true,
            )
        };
        assert_eq!(place((650, 200)), (650, 200));
        assert_eq!(place((399, 200)), (410, 0));
    }

    #[test]
    fn placement_checks_sides_in_order_and_avoids_every_editor() {
        let work = [0, 0, 1200, 800];
        let center = [400, 200, 800, 550];
        let right = [810, 200, 1200, 550];
        let left = [0, 200, 390, 550];
        let below = [0, 560, 1200, 800];
        let place = |occupied: &[[i32; 4]]| {
            let frames: Vec<_> = occupied.iter().map(|&r| EditorFrame::new(r, 36)).collect();
            place_editor((400, 200), (320, 170), work, &frames, 10, false)
        };
        assert_eq!(place(&[center]), (810, 200));
        assert_eq!(place(&[center, right]), (70, 200));
        assert_eq!(place(&[center, right, left]), (400, 560));
        assert_eq!(place(&[center, right, left, below]), (400, 20));
        assert_eq!(place(&[[1200, 0, 1500, 500]]), (400, 200));
        assert_eq!(
            place_editor(
                (0, 0),
                (320, 170),
                [0, 0, 640, 170],
                &[EditorFrame::new([0, 0, 320, 170], 36)],
                10,
                false
            ),
            (320, 0)
        );
    }

    #[test]
    fn a_cascade_at_the_bottom_keeps_the_older_close_button_exposed() {
        let old = EditorFrame::new([200, 100, 700, 600], 36);
        let (x, y) = place_editor((200, 100), (500, 500), [0, 0, 750, 600], &[old], 12, false);
        assert!(x + 500 <= old.title_points()[0][0] || y >= old.title[3]);
    }

    #[test]
    fn overlap_avoids_the_titles_of_every_window() {
        let frames = [
            EditorFrame::new([0, 0, 400, 700], 63),
            EditorFrame::new([400, 0, 800, 700], 63),
            EditorFrame::new([425, 126, 800, 500], 63),
        ];
        for prefer_origin in [false, true] {
            let (x, y) = place_editor(
                (400, 126),
                (600, 500),
                [0, 0, 800, 700],
                &frames,
                42,
                prefer_origin,
            );
            assert_eq!(y, 189);
            assert!(
                frames
                    .iter()
                    .all(|old| !intersects([x, y, x + 600, y + 500], old.title))
            );
        }
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
            AREA.place_new(
                size,
                saved,
                saved_work,
                &[EditorFrame::new([2300, 200, 2800, 500], 36)],
                10
            ),
            (2810, 200)
        );
    }
}
