//! The drawer's top edge (UI-095): dragged, it sets how high the drawer is; a
//! double-click gives back the height it opens with, as a splitter does in most
//! editors. No line marks it: over it the pointer becomes the row-resize cursor.
//!
//! The height belongs to the app, not to the song. Each tab keeps its own (the
//! mixer needs more room than the roll), and both are kept in `view.json` next to
//! the plugin cache, so the next window opens with them.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_design::Layout;

use crate::studio::{DrawerTab, Geometry, Panels, Rect, Studio};

/// How far the edge reaches above the gap (into the arrangement) and below it
/// (into the drawer's tab strip, above its buttons).
const GRAB_OUT: f32 = 3.0;
const GRAB_IN: f32 = 3.0;
/// The least the open drawer is: the roll keeps its strip and a few rows of
/// keys; the mixer the height its strips had in the smallest window.
pub const ROLL_MIN_H: f32 = 180.0;
pub const MIXER_MIN_H: f32 = Layout::DRAWER_H;
/// What the arrangement keeps under its header: about two rows.
pub const ARRANGE_MIN_H: f32 = 120.0;
/// The file's key.
const KEY: &str = "drawer_h";

/// The lowest and highest the open drawer can be in a body `h` high.
pub fn limits(h: f32, panels: Panels) -> (f32, f32) {
    let gap = Layout::REGION_GAP;
    let least = if panels.mixer {
        MIXER_MIN_H
    } else {
        ROLL_MIN_H
    };
    let band = if panels.arrange {
        Layout::SECTION_BAND_H
    } else {
        0.0
    };
    let tempo = if panels.tempo {
        Layout::TEMPO_LANE_H
    } else {
        0.0
    };
    // Gaps above the header, under it, under the lanes and under the drawer.
    let under_header = h - gap * 4.0 - Layout::RULER_H - band - tempo;
    let most = under_header - ARRANGE_MIN_H;
    // In a window too low for both, the rows give way first, then the drawer:
    // it never covers the header.
    let room = under_header.max(Layout::DRAWER_BAR_H);
    (least.min(room), most.max(least).min(room))
}

/// `want` kept inside [`limits`].
pub fn fit(want: f32, h: f32, panels: Panels) -> f32 {
    let (lo, hi) = limits(h, panels);
    want.clamp(lo, hi)
}

/// Where the heights are kept: `XIRIUS_VIEW_FILE`, otherwise `view.json` next to
/// the plugin cache (`%LOCALAPPDATA%\Xirius` on Windows): the heights belong to the PC, so
/// they stay out of the settings (`%APPDATA%\Xirius`).
pub fn view_path() -> PathBuf {
    std::env::var_os("XIRIUS_VIEW_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| xirius_plugins::default_cache_path().with_file_name("view.json"))
}

/// A drag of the edge: where it started and how high the drawer was then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeDrag {
    from_y: f32,
    from_h: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawerSize {
    /// The height asked for with each tab; None is the height it opens with.
    pub roll: Option<f32>,
    pub mixer: Option<f32>,
    pub drag: Option<EdgeDrag>,
    /// The pointer is over the edge.
    pub hover: bool,
    /// Where a change is written; None keeps it in this window only.
    pub file: Option<PathBuf>,
}

impl DrawerSize {
    pub fn want(&self, tab: DrawerTab) -> Option<f32> {
        match tab {
            DrawerTab::Roll => self.roll,
            DrawerTab::Mixer => self.mixer,
        }
    }

    fn slot(&mut self, tab: DrawerTab) -> &mut Option<f32> {
        match tab {
            DrawerTab::Roll => &mut self.roll,
            DrawerTab::Mixer => &mut self.mixer,
        }
    }

    /// The heights kept in `path`, written back there when they change. A
    /// missing or broken file is the heights the drawer opens with.
    pub fn load(path: &Path) -> DrawerSize {
        let kept = read(path);
        let get = |tab: &str| {
            kept.get(KEY)
                .and_then(|v| v.get(tab))
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .filter(|v| v.is_finite() && *v > 0.0)
        };
        DrawerSize {
            roll: get("roll"),
            mixer: get("mixer"),
            file: Some(path.to_path_buf()),
            ..DrawerSize::default()
        }
    }

    /// Writes the heights to the file, keeping whatever else it holds.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        let mut kept = read(path);
        kept.insert(
            KEY.into(),
            json!({ "roll": self.roll, "mixer": self.mixer }),
        );
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&Value::Object(kept))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

fn read(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

impl Studio {
    /// The band that grabs the drawer's top edge: the gap above it and a little
    /// on either side. None while the drawer is shut.
    pub fn drawer_edge(&self, g: &Geometry) -> Option<Rect> {
        let gap = Layout::REGION_GAP;
        self.drawer_open.then(|| {
            Rect::new(
                g.drawer.x,
                g.drawer.y - gap - GRAB_OUT,
                g.drawer.w,
                gap + GRAB_OUT + GRAB_IN,
            )
        })
    }

    /// The pointer at (x, y) would take the edge: nothing floats over it.
    pub fn on_drawer_edge(&self, g: &Geometry, x: f32, y: f32) -> bool {
        self.popup.is_none()
            && self.palette.is_none()
            && self.drawer_edge(g).is_some_and(|r| r.contains(x, y))
    }

    /// A press: on the edge it starts a drag, and a double-click gives the tab
    /// back the height it opens with. True when the edge took the press.
    pub fn edge_down(&mut self, g: &Geometry, x: f32, y: f32, clicks: u32) -> bool {
        if !self.on_drawer_edge(g, x, y) {
            return false;
        }
        if clicks >= 2 {
            self.drawer_size.drag = None;
            let slot = self.drawer_size.slot(self.tab);
            if slot.take().is_some() {
                self.edge_changed(g);
                self.keep_drawer_size();
            }
            return true;
        }
        self.drawer_size.drag = Some(EdgeDrag {
            from_y: y,
            from_h: g.drawer.h,
        });
        true
    }

    /// The pointer moves with the edge held: the drawer follows it, inside
    /// [`limits`]. False when the edge is not held.
    pub fn edge_move(&mut self, g: &Geometry, y: f32) -> bool {
        let Some(d) = self.drawer_size.drag else {
            return false;
        };
        let h = fit(d.from_h + d.from_y - y, g.size.1, g.panels).round();
        if h != g.drawer.h {
            *self.drawer_size.slot(self.tab) = Some(h);
            self.edge_changed(g);
        }
        true
    }

    /// The button comes up: the drag ends and the height is kept.
    pub fn edge_up(&mut self) -> bool {
        if self.drawer_size.drag.take().is_none() {
            return false;
        }
        self.keep_drawer_size();
        true
    }

    /// Notes whether the pointer is over the edge (for its cursor). True when
    /// that changed.
    pub fn edge_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let busy = self.arrange.drag.is_some() || self.roll.drag.is_some() || self.pan.is_some();
        let on = !busy && self.on_drawer_edge(g, x, y);
        let changed = on != self.drawer_size.hover;
        self.drawer_size.hover = on;
        changed
    }

    /// The pointer shows the row-resize cursor: over the edge or dragging it.
    pub fn edge_cursor(&self) -> bool {
        self.drawer_size.drag.is_some() || (self.drawer_size.hover && self.drawer_open)
    }

    fn edge_changed(&mut self, g: &Geometry) {
        let g = self.geometry(g.size.0, g.size.1);
        self.resized(&g);
    }

    fn keep_drawer_size(&self) {
        if let Err(e) = self.drawer_size.save() {
            eprintln!("could not keep the drawer's height: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1440.0;
    const H: f32 = 820.0;

    fn studio() -> Studio {
        Studio::sample()
    }

    fn edge_y(s: &Studio, g: &Geometry) -> f32 {
        let r = s.drawer_edge(g).expect("an open drawer");
        r.y + r.h / 2.0
    }

    /// Presses the edge's middle, moves by `dy` and lets go.
    fn drag(s: &mut Studio, dy: f32) {
        let g = s.geometry(W, H);
        let y = edge_y(s, &g);
        assert!(s.edge_down(&g, W / 2.0, y, 1));
        // In steps, as a pointer does; each move sees the geometry it made.
        for i in 1..=4 {
            let g = s.geometry(W, H);
            assert!(s.edge_move(&g, y + dy * i as f32 / 4.0));
        }
        assert!(s.edge_up());
    }

    #[test]
    fn dragging_the_edge_up_makes_the_drawer_taller_and_the_rows_shorter() {
        let mut s = studio();
        let before = s.geometry(W, H);
        assert_eq!(before.drawer.h, Layout::DRAWER_H);
        drag(&mut s, -100.0);
        let after = s.geometry(W, H);
        assert_eq!(after.drawer.h, Layout::DRAWER_H + 100.0);
        assert_eq!(after.lanes.h, before.lanes.h - 100.0);
        assert_eq!(after.drawer_body.h, before.drawer_body.h + 100.0);
        // The roll gets the room.
        let roll = |g: &Geometry| s.roll.frame(g.drawer_body).grid.h;
        assert!(roll(&after) > roll(&before));
        // Down again.
        drag(&mut s, 60.0);
        assert_eq!(s.geometry(W, H).drawer.h, Layout::DRAWER_H + 40.0);
    }

    #[test]
    fn the_drawer_stops_at_its_least_and_the_arrangement_keeps_two_rows() {
        let mut s = studio();
        drag(&mut s, 2000.0);
        let g = s.geometry(W, H);
        assert_eq!(g.drawer.h, ROLL_MIN_H);
        drag(&mut s, -2000.0);
        let g = s.geometry(W, H);
        assert_eq!(g.lanes.h, ARRANGE_MIN_H);
        assert_eq!(g.drawer.h, limits(H, g.panels).1);
        // The header and the regions still sit in order inside the window.
        assert!(g.ruler.bottom() < g.lanes.y);
        assert!(g.lanes.bottom() < g.drawer.y);
        assert!(g.drawer.bottom() <= H);
    }

    #[test]
    fn the_least_holds_the_roll_and_the_mixer() {
        let mut s = studio();
        drag(&mut s, 2000.0);
        let g = s.geometry(W, H);
        let frame = s.roll.frame(g.drawer_body);
        assert!(frame.grid.h >= 4.0 * crate::roll::ROW_H_MIN, "{frame:?}");
        // The mixer keeps its own height, never less than its strips need.
        s.tab = DrawerTab::Mixer;
        let auto = s.geometry(W, H).drawer.h;
        drag(&mut s, 2000.0);
        assert_eq!(s.geometry(W, H).drawer.h, MIXER_MIN_H);
        drag(&mut s, -50.0);
        assert_eq!(s.geometry(W, H).drawer.h, MIXER_MIN_H + 50.0);
        assert!(auto >= MIXER_MIN_H);
        // Each tab has its own height.
        s.tab = DrawerTab::Roll;
        assert_eq!(s.geometry(W, H).drawer.h, ROLL_MIN_H);
    }

    #[test]
    fn a_smaller_window_squeezes_the_drawer_and_gives_the_height_back_after() {
        let mut s = studio();
        drag(&mut s, -300.0);
        let tall = s.geometry(W, H).drawer.h;
        assert_eq!(tall, Layout::DRAWER_H + 300.0);
        // The smallest window the app allows (960 x 600, less the two bars), and
        // lower still: the drawer gives way before it covers the header.
        let small_h = 600.0 - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H;
        for tab in DrawerTab::ALL {
            s.tab = tab;
            for (w, h) in [(960.0, small_h), (960.0, 400.0), (800.0, 300.0)] {
                let g = s.geometry(w, h);
                assert!(
                    g.drawer.y >= 0.0 && g.drawer.bottom() <= h,
                    "{tab:?} {h} {g:?}"
                );
                assert!(g.lanes.h >= 0.0 && g.lanes.bottom() <= g.drawer.y);
                if h == small_h {
                    assert!(g.lanes.h >= ARRANGE_MIN_H, "{tab:?} {g:?}");
                }
            }
        }
        s.tab = DrawerTab::Roll;
        assert!(s.geometry(960.0, small_h).drawer.h < tall);
        // Back to the larger window: the height asked for comes back.
        assert_eq!(s.geometry(W, H).drawer.h, tall);
    }

    #[test]
    fn a_double_click_gives_the_height_the_drawer_opens_with() {
        let mut s = studio();
        drag(&mut s, -120.0);
        let g = s.geometry(W, H);
        let y = edge_y(&s, &g);
        assert!(s.edge_down(&g, 300.0, y, 2));
        assert_eq!(s.drawer_size.roll, None);
        assert_eq!(s.geometry(W, H).drawer.h, Layout::DRAWER_H);
        assert!(!s.edge_up());
    }

    #[test]
    fn the_edge_is_the_gap_and_needs_an_open_drawer_and_nothing_over_it() {
        let mut s = studio();
        let g = s.geometry(W, H);
        let y = edge_y(&s, &g);
        assert!(s.on_drawer_edge(&g, 600.0, y));
        // Not the rows above it, nor the tabs below it.
        assert!(!s.on_drawer_edge(&g, 600.0, g.lanes.bottom() - 8.0));
        let tab = g.tab(DrawerTab::Roll);
        assert!(!s.on_drawer_edge(&g, tab.x + 2.0, tab.y + 1.0));
        assert!(s.edge_hover(&g, 600.0, y));
        assert!(s.edge_cursor());
        assert!(s.edge_hover(&g, 600.0, g.lanes.y + 20.0));
        assert!(!s.edge_cursor());
        s.drawer_open = false;
        let shut = s.geometry(W, H);
        assert!(!s.on_drawer_edge(&shut, 600.0, edge_y(&studio(), &g)));
        assert!(!s.edge_down(&shut, 600.0, shut.drawer.y - 3.0, 1));
        s.drawer_open = true;
        s.open_popup(
            crate::popup::PopupKind::Grid,
            Rect::new(600.0, 10.0, 10.0, 10.0),
        );
        assert!(!s.on_drawer_edge(&g, 600.0, y));
    }

    #[test]
    fn a_press_without_a_move_keeps_the_height_it_opens_with() {
        let mut s = studio();
        let g = s.geometry(W, H);
        let y = edge_y(&s, &g);
        assert!(s.edge_down(&g, 600.0, y, 1));
        assert!(s.edge_move(&g, y));
        assert!(s.edge_up());
        assert_eq!(s.drawer_size.roll, None);
    }

    #[test]
    fn the_heights_are_kept_and_the_next_window_opens_with_them() {
        let dir_tmp = tempfile::Builder::new()
            .prefix("xirius-view-")
            .tempdir()
            .unwrap();
        let dir = dir_tmp.path().join("view");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("view.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"other": 1}"#).unwrap();
        let mut s = studio();
        s.drawer_size = DrawerSize::load(&path);
        assert_eq!(s.drawer_size.roll, None);
        drag(&mut s, -80.0);
        s.tab = DrawerTab::Mixer;
        drag(&mut s, 2000.0);
        // Another window.
        let mut next = studio();
        next.drawer_size = DrawerSize::load(&path);
        assert_eq!(next.geometry(W, H).drawer.h, Layout::DRAWER_H + 80.0);
        next.tab = DrawerTab::Mixer;
        assert_eq!(next.geometry(W, H).drawer.h, MIXER_MIN_H);
        // What else the file held is still there.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"other\""), "{text}");
        // A broken file is the default heights.
        std::fs::write(&path, "{").unwrap();
        assert_eq!(DrawerSize::load(&path).roll, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_file_nothing_is_written() {
        let mut s = studio();
        assert_eq!(s.drawer_size.file, None);
        drag(&mut s, -40.0);
        assert!(s.drawer_size.save().is_ok());
    }
}
