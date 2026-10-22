//! The right edges of the browser's panel and of the track head (UI-048): dragged,
//! each sets how wide it is; a double-click gives back the width it opens with, as
//! the drawer's top edge does (UI-095). No line marks them: over one the pointer
//! becomes the column-resize cursor.
//!
//! Every panel can be resized (the drawer by its top edge, these two by their right
//! edges); [`PANELS`] lists them and a test keeps a new region of the screen from
//! being added as a panel of one fixed size.
//!
//! The widths belong to the app, not to the song: they are kept in `view.json` next
//! to the drawer's heights.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_design::Layout;

use crate::studio::{Geometry, Panels, Rect, Studio};

/// How far a grab reaches into the panel and into the column beside the gap.
const GRAB: f32 = 2.0;
/// The browser keeps at least what its search row and a name need; at most about
/// twice what it opens with.
pub const BROWSER_MIN_W: f32 = 160.0;
pub const BROWSER_MAX_W: f32 = 480.0;
/// The head can be as wide as the longest names and the deepest folders ask for,
/// and no wider than this.
pub const HEAD_MAX_W: f32 = 420.0;
/// What the arrangement keeps beside the panels.
pub const ARRANGE_MIN_W: f32 = 320.0;
/// The width of the track list folded to a strip: the colours of the tracks and a ›.
pub const FOLDED_W: f32 = 24.0;
const BROWSER_KEY: &str = "browser_w";
const HEAD_KEY: &str = "head_w";
const FOLDED_KEY: &str = "tracklist_folded";

/// A panel of the main screen and how it is resized. The test of this file reads
/// the fields of [`Geometry`] and asks that each is listed here or in [`PARTS`].
pub struct PanelSize {
    /// The field of [`Geometry`] that is the panel.
    pub field: &'static str,
    pub resized_by: Edge,
}

/// Which edge is dragged to resize a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The top edge (the height).
    Top,
    /// The right edge (the width).
    Right,
}

/// The panels of the main screen: every one has an edge that resizes it.
pub const PANELS: &[PanelSize] = &[
    PanelSize {
        field: "panel",
        resized_by: Edge::Right,
    },
    PanelSize {
        field: "column",
        resized_by: Edge::Right,
    },
    PanelSize {
        field: "drawer",
        resized_by: Edge::Top,
    },
];

/// The other regions of [`Geometry`]: not panels, with the reason.
pub const PARTS: &[(&str, &str)] = &[
    ("size", "the window's size"),
    ("panels", "which regions are shown"),
    ("rail", "the strip of buttons that opens the panel"),
    (
        "rec",
        "the strip of record settings across the top: one row of fields, as high as they are",
    ),
    ("head", "the corner above the track column, as wide as it"),
    ("ruler", "a row of the arrangement's header"),
    ("section_band", "a row of the arrangement's header"),
    ("tempo", "a row of the arrangement's header"),
    (
        "lanes",
        "the arrangement, which takes what the panels leave",
    ),
    ("drawer_bar", "the tab strip of the drawer"),
    ("drawer_body", "the content of the drawer"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Browser,
    Head,
}

/// A drag of an edge: where it started and how wide the panel was then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SideDrag {
    side: Side,
    from_x: f32,
    from_w: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SideSize {
    /// The widths asked for; None is the width it opens with.
    pub browser: Option<f32>,
    pub head: Option<f32>,
    /// The track list is a strip (UI-066). Kept with the widths: it is the app's, not the song's.
    pub folded: bool,
    pub drag: Option<SideDrag>,
    /// The pointer is over this edge.
    pub hover: Option<Side>,
    /// Where a change is written; None keeps it in this window only.
    pub file: Option<PathBuf>,
}

/// What is left for the arrangement beside the rail, the gaps and `panels_w` of
/// panels, less the room it keeps.
fn room(w: f32, browser_open: bool, panels_w: f32) -> f32 {
    let gaps = if browser_open { 5.0 } else { 4.0 };
    w - Layout::REGION_GAP * gaps - Layout::RAIL_W - ARRANGE_MIN_W - panels_w
}

/// The least and most the open browser can be in a window `w` wide whose track head
/// is `head_w`.
pub fn browser_limits(w: f32, head_w: f32) -> (f32, f32) {
    let most = room(w, true, head_w).min(BROWSER_MAX_W);
    (BROWSER_MIN_W, most.max(BROWSER_MIN_W))
}

/// The least and most the track head can be: as wide as its rows need (`content`)
/// and up to [`HEAD_MAX_W`], leaving the arrangement its room beside the browser's
/// panel (`browser_w`, zero when it is shut).
pub fn head_limits(w: f32, browser_w: f32, content: f32) -> (f32, f32) {
    let most = room(w, browser_w > 0.0, browser_w).min(HEAD_MAX_W);
    (content, most.max(content))
}

impl SideSize {
    /// The width of the browser's panel in a window `w` wide, with the head `head_w`.
    pub fn browser_w(&self, w: f32, panels: Panels, head_w: f32) -> f32 {
        let (lo, hi) = browser_limits(w, head_w);
        if panels.browser {
            self.browser.unwrap_or(Layout::BROWSER_W).clamp(lo, hi)
        } else {
            Layout::BROWSER_W
        }
    }

    /// The width of the track head: `content` (what its rows need) or what was asked
    /// for, if more.
    pub fn head_w(&self, w: f32, panels: Panels, content: f32) -> f32 {
        if self.folded {
            return FOLDED_W;
        }
        let browser_w = if panels.browser {
            let (lo, hi) = browser_limits(w, content);
            self.browser.unwrap_or(Layout::BROWSER_W).clamp(lo, hi)
        } else {
            0.0
        };
        let (lo, hi) = head_limits(w, browser_w, content);
        self.head.map_or(lo, |want| want.clamp(lo, hi))
    }

    /// The widths kept in `path`, written back there when they change. A missing
    /// or broken file is the widths the panels open with.
    pub fn load(path: &Path) -> SideSize {
        let kept = read(path);
        let get = |key: &str| {
            kept.get(key)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .filter(|v| v.is_finite() && *v > 0.0)
        };
        SideSize {
            browser: get(BROWSER_KEY),
            head: get(HEAD_KEY),
            folded: kept
                .get(FOLDED_KEY)
                .and_then(Value::as_bool)
                .unwrap_or(false),
            file: Some(path.to_path_buf()),
            ..SideSize::default()
        }
    }

    /// Writes the widths to the file, keeping whatever else it holds (the drawer's
    /// heights).
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        let mut kept = read(path);
        kept.insert(BROWSER_KEY.into(), json!(self.browser));
        kept.insert(HEAD_KEY.into(), json!(self.head));
        kept.insert(FOLDED_KEY.into(), json!(self.folded));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&Value::Object(kept))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    fn slot(&mut self, side: Side) -> &mut Option<f32> {
        match side {
            Side::Browser => &mut self.browser,
            Side::Head => &mut self.head,
        }
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
    /// The band that grabs the right edge of `side`: the gap beside it and a little
    /// on either side. None while that panel is not shown.
    pub fn side_edge(&self, g: &Geometry, side: Side) -> Option<Rect> {
        let gap = Layout::REGION_GAP;
        let r = match side {
            Side::Browser => g.panels.browser.then_some(g.panel)?,
            // A strip has no width to set: it opens by a press, not by a drag.
            Side::Head => (!g.panels.folded).then_some(g.column)?,
        };
        let top = match side {
            Side::Browser => r.y,
            Side::Head => g.head.y,
        };
        Some(Rect::new(
            r.right() - GRAB,
            top,
            gap + GRAB * 2.0,
            (r.bottom() - top).max(0.0),
        ))
    }

    /// The edge at (x, y), when nothing floats over it.
    pub fn side_at(&self, g: &Geometry, x: f32, y: f32) -> Option<Side> {
        if self.popup.is_some() || self.palette.is_some() {
            return None;
        }
        [Side::Browser, Side::Head]
            .into_iter()
            .find(|s| self.side_edge(g, *s).is_some_and(|r| r.contains(x, y)))
    }

    /// A press: on an edge it starts a drag, and a double-click gives that panel
    /// the width it opens with. True when an edge took the press.
    pub fn side_down(&mut self, g: &Geometry, x: f32, y: f32, clicks: u32) -> bool {
        let Some(side) = self.side_at(g, x, y) else {
            return false;
        };
        if clicks >= 2 {
            self.side_size.drag = None;
            if self.side_size.slot(side).take().is_some() {
                self.side_changed(g);
                self.keep_side_size();
            }
            return true;
        }
        let from_w = match side {
            Side::Browser => g.panel.w,
            Side::Head => g.column.w,
        };
        self.side_size.drag = Some(SideDrag {
            side,
            from_x: x,
            from_w,
        });
        true
    }

    /// The pointer moves with an edge held: the panel follows it, inside its
    /// limits. False when no edge is held.
    pub fn side_move(&mut self, g: &Geometry, x: f32) -> bool {
        let Some(d) = self.side_size.drag else {
            return false;
        };
        let want = (d.from_w + x - d.from_x).round();
        let now = match d.side {
            Side::Browser => g.panel.w,
            Side::Head => g.column.w,
        };
        if want != now {
            *self.side_size.slot(d.side) = Some(want);
            self.side_changed(g);
        }
        true
    }

    /// The button comes up: the drag ends and the width is kept.
    pub fn side_up(&mut self) -> bool {
        if self.side_size.drag.take().is_none() {
            return false;
        }
        self.keep_side_size();
        true
    }

    /// Notes which edge the pointer is over (for its cursor). True when that changed.
    pub fn side_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let busy = self.arrange.drag.is_some() || self.roll.drag.is_some() || self.pan.is_some();
        let on = if busy { None } else { self.side_at(g, x, y) };
        let changed = on != self.side_size.hover;
        self.side_size.hover = on;
        changed
    }

    /// The pointer shows the column-resize cursor: over an edge or dragging one.
    pub fn side_cursor(&self) -> bool {
        self.side_size.drag.is_some() || self.side_size.hover.is_some()
    }

    /// UI-066: folds the track list to a strip, or opens it again. The rows keep their heights,
    /// so what is beside them stays where it was; the arrangement takes the room.
    pub fn toggle_tracklist(&mut self, g: &Geometry) {
        self.side_size.folded = !self.side_size.folded;
        self.side_size.drag = None;
        self.side_size.hover = None;
        let now = self.geometry(g.size.0, g.size.1);
        self.resized(&now);
        self.keep_side_size();
    }

    fn side_changed(&mut self, g: &Geometry) {
        // The width asked for is what the panel shows: the limits hold it, and it
        // is kept as it was clamped so that a drag past a limit does not pile up.
        let now = self.geometry(g.size.0, g.size.1);
        if let Some(d) = self.side_size.drag {
            let shown = match d.side {
                Side::Browser => now.panel.w,
                Side::Head => now.column.w,
            };
            *self.side_size.slot(d.side) = Some(shown);
        }
        self.resized(&now);
    }

    fn keep_side_size(&self) {
        if let Err(e) = self.side_size.save() {
            eprintln!("could not keep the panels' widths: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawer_edge;

    const W: f32 = 1440.0;
    const H: f32 = 820.0;

    fn studio() -> Studio {
        let mut s = Studio::sample();
        s.browser.open = true;
        s
    }

    fn mid(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// Presses the middle of `side`'s edge, moves by `dx` in steps and lets go.
    fn drag(s: &mut Studio, side: Side, dx: f32) {
        let g = s.geometry(W, H);
        let (x, y) = mid(s.side_edge(&g, side).expect("a shown panel"));
        assert!(s.side_down(&g, x, y, 1));
        for i in 1..=4 {
            let g = s.geometry(W, H);
            assert!(s.side_move(&g, x + dx * i as f32 / 4.0));
        }
        assert!(s.side_up());
    }

    #[test]
    fn dragging_the_browsers_edge_sets_its_width_and_the_arrangement_gives_way() {
        let mut s = studio();
        let before = s.geometry(W, H);
        assert_eq!(before.panel.w, Layout::BROWSER_W);
        drag(&mut s, Side::Browser, 80.0);
        let after = s.geometry(W, H);
        assert_eq!(after.panel.w, Layout::BROWSER_W + 80.0);
        assert_eq!(after.column.x, before.column.x + 80.0);
        assert_eq!(after.lanes.w, before.lanes.w - 80.0);
        drag(&mut s, Side::Browser, -30.0);
        assert_eq!(s.geometry(W, H).panel.w, Layout::BROWSER_W + 50.0);
    }

    #[test]
    fn dragging_the_heads_edge_sets_its_width() {
        let mut s = studio();
        let before = s.geometry(W, H);
        drag(&mut s, Side::Head, 60.0);
        let after = s.geometry(W, H);
        assert_eq!(after.column.w, before.column.w + 60.0);
        assert_eq!(after.head.w, after.column.w);
        assert_eq!(after.lanes.x, before.lanes.x + 60.0);
        // Not narrower than its rows ask.
        drag(&mut s, Side::Head, -1000.0);
        assert_eq!(s.geometry(W, H).column.w, before.column.w);
    }

    #[test]
    fn each_panel_stops_at_its_limits_and_the_arrangement_keeps_its_room() {
        let mut s = studio();
        drag(&mut s, Side::Browser, -1000.0);
        assert_eq!(s.geometry(W, H).panel.w, BROWSER_MIN_W);
        drag(&mut s, Side::Browser, 2000.0);
        assert_eq!(s.geometry(W, H).panel.w, BROWSER_MAX_W);
        drag(&mut s, Side::Head, 2000.0);
        let g = s.geometry(W, H);
        assert!(g.lanes.w >= ARRANGE_MIN_W, "{g:?}");
        assert!(g.column.w <= HEAD_MAX_W);
        // The smallest window: both panels and the arrangement still sit in it.
        let small = s.geometry(960.0, 600.0 - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H);
        assert!(small.lanes.w >= ARRANGE_MIN_W, "{small:?}");
        assert!(small.lanes.right() <= 960.0);
        assert!(small.panel.right() <= small.column.x);
    }

    #[test]
    fn a_double_click_gives_the_width_the_panel_opens_with() {
        let mut s = studio();
        drag(&mut s, Side::Browser, 70.0);
        let g = s.geometry(W, H);
        let (x, y) = mid(s.side_edge(&g, Side::Browser).unwrap());
        assert!(s.side_down(&g, x, y, 2));
        assert_eq!(s.side_size.browser, None);
        assert_eq!(s.geometry(W, H).panel.w, Layout::BROWSER_W);
        assert!(!s.side_up());
    }

    #[test]
    fn the_edge_needs_its_panel_shown_and_nothing_over_it() {
        let mut s = studio();
        let g = s.geometry(W, H);
        let (x, y) = mid(s.side_edge(&g, Side::Browser).unwrap());
        assert_eq!(s.side_at(&g, x, y), Some(Side::Browser));
        assert!(s.side_hover(&g, x, y));
        assert!(s.side_cursor());
        // The body of the panel and of the rows is not an edge.
        assert_eq!(s.side_at(&g, g.panel.x + 20.0, y), None);
        assert_eq!(s.side_at(&g, g.lanes.x + 40.0, y), None);
        assert!(s.side_hover(&g, g.lanes.x + 40.0, y));
        assert!(!s.side_cursor());
        s.open_popup(
            crate::popup::PopupKind::Grid,
            Rect::new(600.0, 10.0, 10.0, 10.0),
        );
        assert_eq!(s.side_at(&g, x, y), None);
        s.popup = None;
        s.browser.open = false;
        let shut = s.geometry(W, H);
        assert_eq!(s.side_edge(&shut, Side::Browser), None);
        assert_eq!(shut.panel.w, 0.0);
        // The head still has its edge.
        assert!(s.side_edge(&shut, Side::Head).is_some());
    }

    #[test]
    fn the_widths_are_kept_for_the_next_window_beside_the_drawers_height() {
        let dir_tmp = tempfile::Builder::new()
            .prefix("xirius-side-edge-")
            .tempdir()
            .unwrap();
        let dir = dir_tmp.path().join("side-edge");
        let file = dir.join("view.json");
        let mut s = studio();
        s.drawer_size.file = Some(file.clone());
        s.side_size.file = Some(file.clone());
        drag(&mut s, Side::Browser, 40.0);
        drag(&mut s, Side::Head, 25.0);
        s.drawer_size.roll = Some(300.0);
        s.drawer_size.save().unwrap();
        let again = SideSize::load(&file);
        assert_eq!(again.browser, Some(Layout::BROWSER_W + 40.0));
        assert!(again.head.is_some());
        // Each file writer keeps what the other wrote.
        assert_eq!(drawer_edge::DrawerSize::load(&file).roll, Some(300.0));
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(SideSize::load(Path::new("no/such/view.json")).browser, None);
    }

    #[test]
    fn every_region_of_the_geometry_is_a_panel_with_an_edge_or_a_part_with_a_reason() {
        let src = include_str!("studio.rs");
        let body = src
            .split("pub struct Geometry {")
            .nth(1)
            .and_then(|t| t.split("\n}\n").next())
            .expect("the Geometry struct");
        let fields: Vec<&str> = body
            .lines()
            .filter_map(|l| l.trim().strip_prefix("pub "))
            .filter_map(|l| l.split(':').next())
            .collect();
        assert!(fields.len() >= 10, "{fields:?}");
        for f in &fields {
            let panel = PANELS.iter().any(|p| p.field == *f);
            let part = PARTS.iter().any(|(name, _)| name == f);
            assert!(
                panel ^ part,
                "`Geometry::{f}` is a new region: list it in side_edge::PANELS with the edge \
                 that resizes it (a panel is never of one fixed size, UI-048), or in \
                 side_edge::PARTS with the reason it is not a panel"
            );
        }
        for p in PANELS {
            assert!(fields.contains(&p.field), "{}", p.field);
        }
        for (name, why) in PARTS {
            assert!(fields.contains(name) && !why.is_empty(), "{name}");
        }
    }

    #[test]
    fn each_listed_panel_changes_size_when_its_edge_is_dragged() {
        for p in PANELS {
            let mut s = studio();
            let size = |g: &Geometry| match p.field {
                "panel" => (g.panel.w, g.panel.h),
                "column" => (g.column.w, g.column.h),
                "drawer" => (g.drawer.w, g.drawer.h),
                other => panic!("`{other}`: say here how its size is read"),
            };
            let before = size(&s.geometry(W, H));
            match (p.field, p.resized_by) {
                ("panel", Edge::Right) => drag(&mut s, Side::Browser, 50.0),
                ("column", Edge::Right) => drag(&mut s, Side::Head, 50.0),
                ("drawer", Edge::Top) => {
                    let g = s.geometry(W, H);
                    let r = s.drawer_edge(&g).unwrap();
                    let (x, y) = mid(r);
                    assert!(s.edge_down(&g, x, y, 1));
                    let g = s.geometry(W, H);
                    assert!(s.edge_move(&g, y - 50.0));
                    assert!(s.edge_up());
                }
                other => panic!("`{other:?}`: how is it dragged?"),
            }
            assert_ne!(size(&s.geometry(W, H)), before, "{}", p.field);
        }
    }
}
