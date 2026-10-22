//! The alternative tool (the one the arrow turns into while Ctrl is held), changed through the
//! top bar as the window does it, and then used with Ctrl in the arrangement. The presses go
//! through the window's handlers in the window's order (`screen.rs`).

use super::*;
use crate::arrange::probe::{clip_mid, track};
use crate::studio::Hit;
use crate::topbar::BarButton;

fn win_mods(s: &mut Studio, g: &Geometry, m: Mods) {
    if let Some(ctx) = s.roll_ctx(g) {
        let out = s.roll.mods_changed(&ctx, m);
        s.apply_roll(g, out);
    }
    s.arrange_mods(m);
}

fn win_move(s: &mut Studio, g: &Geometry, x: f32, y: f32, m: Mods) {
    s.input_map.pointer = (x, y);
    if s.bar_move(y, m.shift) {
        return;
    }
    if s.zoom_box_move(x, y)
        || s.mixer_bar_move(g, x)
        || s.edge_move(g, y)
        || s.side_move(g, x)
        || s.pan_move(g, x, y)
        || s.ruler_drag_move(g, x, y)
    {
        return;
    }
    if let Some(ctx) = s.roll_input(g) {
        let out = s.roll.moved(&ctx, x, y, m);
        s.apply_roll(g, out);
    }
    s.arrange_move(g, x, y, m);
    let hit = s.hit(g, x, y);
    s.hover = hit;
}

fn win_down(s: &mut Studio, g: &Geometry, x: f32, y: f32, m: Mods) {
    if s.recovery_press(g, x, y)
        || s.toast_press(g, x, y)
        || s.job_stop_press(g, x, y)
        || s.notice_press(g, x, y)
        || s.cpu_down(g, x, y, 1)
        || s.name_press(g, x, y, 1)
        || s.bar_down(g, x, y, 1)
        || s.mixer_bar_down(g, x, y)
        || s.zoom_box_down(g, x, y, m)
        || s.edge_down(g, x, y, 1)
        || s.side_down(g, x, y, 1)
    {
        return;
    }
    s.name_drag_down(g, x, y);
    if s.chain_down(g, x, y) || (s.popup.is_none() && s.instrument_down(g, x, y)) {
        return;
    }
    s.ruler_drag_down(g, x, y, m);
    if s.roll_press(g, x, y, m, 1) {
        return;
    }
    s.arrange.clicks = 1;
    if s.arrange_down(g, x, y, m) {
        return;
    }
    let hit = s.hit(g, x, y);
    s.press(g, hit);
}

fn win_up(s: &mut Studio, g: &Geometry, x: f32, y: f32, m: Mods) {
    if s.bar_up() || s.edge_up() | s.side_up() || s.mixer_bar_up() || s.zoom_box_up(g, x, y) {
        return;
    }
    s.ruler_drag_up();
    if s.name_drag_up(g, x, y)
        || s.fx_up(g, x, y, m.alt)
        || s.lib_up(g, x, y)
        || s.instrument_up()
        || s.drag_end()
    {
        return;
    }
    if let Some(ctx) = s.roll_ctx(g) {
        let out = s.roll.up(&ctx, x, y, m);
        s.apply_roll(g, out);
    }
    s.arrange_up(g, x, y, m);
}

fn click(s: &mut Studio, g: &Geometry, (x, y): (f32, f32)) {
    win_move(s, g, x, y, Mods::NONE);
    win_down(s, g, x, y, Mods::NONE);
    win_up(s, g, x, y, Mods::NONE);
}

/// The right press of the window (`right_pressed`).
fn right_click(s: &mut Studio, g: &Geometry, (x, y): (f32, f32)) {
    win_move(s, g, x, y, Mods::NONE);
    assert!(
        s.cpu_context(g, x, y)
            || s.lib_context(g, x, y)
            || s.scene_context(g, x, y)
            || (s.roll_input(g).is_none() && s.context_press(g, x, y))
    );
    assert!(s.popup.is_some(), "no menu");
}

fn press_bar(s: &mut Studio, g: &Geometry, b: BarButton) {
    let p = s.center_of(g, Hit::Bar(b)).expect("on the bar");
    assert_eq!(s.hit(g, p.0, p.1), Hit::Bar(b));
    click(s, g, p);
}

fn pick_row(s: &mut Studio, g: &Geometry, label: &str) {
    let i = s
        .context_rows()
        .iter()
        .position(|r| !r.gap && r.label == label)
        .unwrap_or_else(|| panic!("no row {label}"));
    let p = s.center_of(g, Hit::Item(i)).expect("the row on screen");
    click(s, g, p);
    assert!(s.popup.is_none(), "the menu stays open");
    s.settings_reload();
}

/// The tool a Ctrl press on the Keys clip acts as, after the pointer came there: the one
/// its drag holds, or the one that cut the clip in two.
fn ctrl_press_on_clip(s: &mut Studio, g: &Geometry) -> Option<Tool> {
    let t = track(s, "Keys");
    let clips = s.project().tracks[&t].clips.len();
    let (x, y) = clip_mid(s, g, "Keys", 0);
    win_move(s, g, x, y, Mods::NONE);
    win_mods(s, g, Mods::ctrl());
    win_move(s, g, x, y, Mods::ctrl());
    win_down(s, g, x, y, Mods::ctrl());
    let tool = s.arrange.drag.as_ref().map(|d| d.tool);
    win_up(s, g, x, y, Mods::ctrl());
    win_mods(s, g, Mods::NONE);
    tool.or((s.project().tracks[&t].clips.len() > clips).then_some(Tool::Cut))
}

/// The sample song with the roll open in the drawer, and the settings in a folder of the test's.
fn setup(dir: &std::path::Path) -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.song.load_settings(dir);
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Roll;
    let g = s.geometry(1440.0, 820.0);
    s.resized(&g);
    (s, g)
}

/// A press in the roll's empty grid: the roll is the editor in use.
fn use_the_roll(s: &mut Studio, g: &Geometry) {
    let body = s.roll_ctx(g).expect("the roll").body;
    click(s, g, (body.x + 12.0, body.y + 12.0));
    assert_eq!(s.tool_area(), Area::Roll);
}

#[test]
fn after_the_roll_a_right_press_in_the_arrangement_gives_the_bar_the_arrangements_tool() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, g) = setup(dir.path());
    use_the_roll(&mut s, &g);
    // The clip's menu, closed again: the keys and the bar are the arrangement's now.
    let at = clip_mid(&s, &g, "Keys", 0);
    right_click(&mut s, &g, at);
    assert!(s.mapped_key(&g, "escape", None, Mods::NONE));
    assert!(s.arrange.focus);
    assert_eq!(
        s.tool_area(),
        Area::Arrange,
        "the bar still stands for the roll"
    );
    assert!(s.bar_layout(1440.0).alt.w > 0.0, "no ▾ in the arrangement");

    let before = s.bar_view().alt;
    press_bar(&mut s, &g, BarButton::Tool(Tool::Select));
    s.settings_reload();
    let shown = s.bar_view().alt;
    assert_ne!(shown, before, "pressing the arrow again changed nothing");
    assert_eq!(ctrl_press_on_clip(&mut s, &g), Some(shown));
}

#[test]
fn the_alternative_tool_picked_from_the_bar_is_the_one_ctrl_gives_in_the_arrangement() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, g) = setup(dir.path());
    use_the_roll(&mut s, &g);
    // Back to the arrangement by a press on its empty lane.
    let t = track(&s, "Keys");
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let lane = (
        g.lanes.x + g.lanes.w - 20.0,
        s.row_y(&g, &row) + row.h / 2.0,
    );
    click(&mut s, &g, lane);
    assert_eq!(s.tool_area(), Area::Arrange);

    // The ▾ menu.
    press_bar(&mut s, &g, BarButton::AltTool);
    pick_row(&mut s, &g, Tool::Mute.label());
    assert_eq!(s.bar_view().alt, Tool::Mute);
    assert_eq!(ctrl_press_on_clip(&mut s, &g), Some(Tool::Mute));
    s.undo(&g);

    // The arrow pressed again goes on from there.
    press_bar(&mut s, &g, BarButton::Tool(Tool::Select));
    s.settings_reload();
    let shown = s.bar_view().alt;
    assert_ne!(shown, Tool::Mute);
    assert_eq!(ctrl_press_on_clip(&mut s, &g), Some(shown));

    // The settings write the same place (`tool.alt` with the area).
    s.song
        .set(
            "tool.alt",
            serde_json::json!({ "tool": "erase", "area": "arrange" }),
        )
        .unwrap();
    assert_eq!(s.bar_view().alt, Tool::Eraser);
    assert_eq!(ctrl_press_on_clip(&mut s, &g), Some(Tool::Eraser));
}
