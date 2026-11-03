use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use super::*;
use crate::instrument::window::Part;
use crate::mixer::{MixerHit, STRIP_GAP, WIDE_W};
use crate::studio::DrawerTab;
use crate::tools::Mods;

fn studio(w: f32) -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    s.instruments.open.clear();
    let g = s.geometry(w, 1000.0);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}

fn add(s: &mut Studio, t: TrackId, name: &str) -> PluginId {
    assert!(s.run_command("plugin.add", json!({"track":t,"name":name})));
    let id = s.project().tracks[&t].inserts.last().unwrap().id;
    s.instruments.open.clear();
    id
}

/// A press and a let go on a row's name, as the pointer does it.
fn click_row(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId) {
    let (x, y) = s.mixer_center(g, MixerHit::Open(t, id)).unwrap();
    s.chosen_press(g, x, y);
    assert!(s.chain_down(g, x, y));
    assert!(s.fx_up(g, x, y, false));
}

fn docked_plugin(s: &Studio) -> Option<PluginId> {
    s.docked().map(|i| s.instruments.open[i].plugin)
}

fn press(s: &mut Studio, g: &Geometry, id: PluginId, part: Part, clicks: u32) {
    let (x, y) = s.instrument_center(g, id, part).unwrap();
    assert!(s.instrument_press(g, x, y, clicks, Mods::NONE));
}

/// Vocal with an EQ picked: the EQ's face beside the strips.
fn picked(w: f32) -> (Studio, Geometry, TrackId, PluginId) {
    let (mut s, g) = studio(w);
    let t = if w < 1000.0 {
        s.mixer_strips(&g)[0].track
    } else {
        track(&s, "Vocal")
    };
    let eq = add(&mut s, t, "EQ");
    click_row(&mut s, &g, t, eq);
    (s, g, t, eq)
}

#[test]
fn a_picked_device_shows_its_face_right_of_the_strips() {
    let (s, g, t, eq) = picked(1600.0);
    assert_eq!(docked_plugin(&s), Some(eq));
    assert!(!s.instruments.open.iter().any(|w| !w.docked));
    let split = s.mixer_split(&g).unwrap();
    assert!(!split.folded);
    let body = g.drawer_body;
    assert_eq!(split.mixer.x, body.x);
    assert_eq!(split.panel.right(), body.right());
    assert_eq!(split.mixer.w + split.grip.w + split.panel.w, body.w);
    assert_eq!(s.mixer_body(&g), split.mixer);
    // the strips keep left of the grip, the master at its edge
    let strips = s.mixer_strips(&g);
    assert!(strips.iter().all(|x| x.rect.right() <= split.grip.x));
    assert_eq!(
        strips.last().unwrap().rect.right(),
        split.mixer.right() - STRIP_GAP
    );
    // the face is the device's window, where the drawer puts it
    let i = s.docked().unwrap();
    assert_eq!(s.instruments.frame(i, &g), split.panel);
    assert_eq!(s.instruments.open[i].track, t);
    // a press on the face's knob reaches the device, not the mixer
    let (x, y) = s.instrument_center(&g, eq, Part::Knob(0)).unwrap();
    assert!(split.panel.contains(x, y));
    assert_eq!(s.mixer_hit(&g, x, y), None);
}

#[test]
fn another_row_takes_the_face_and_a_window_of_its_own_keeps_it() {
    let (mut s, g, t, eq) = picked(1600.0);
    let comp = add(&mut s, t, "Compressor");
    click_row(&mut s, &g, t, comp);
    assert_eq!(docked_plugin(&s), Some(comp));
    assert_eq!(s.instruments.open.len(), 1);
    // the keys move the pick and the face follows
    assert!(s.field_keys(&g, "up", None, Mods::NONE));
    assert_eq!(docked_plugin(&s), Some(eq));
    // a device in a window of its own stays there
    s.show_instrument(t, comp);
    assert!(s.field_keys(&g, "down", None, Mods::NONE));
    assert_eq!(docked_plugin(&s), Some(eq));
}

#[test]
fn the_face_opens_as_a_window_from_its_bar() {
    let (mut s, g, _, eq) = picked(1600.0);
    press(&mut s, &g, eq, Part::Pin, 1);
    assert_eq!(s.docked(), None);
    assert!(s.instruments.is_open(eq));
    assert!(!s.instruments.open[0].docked);
    assert_eq!(s.mixer_body(&g), g.drawer_body);

    // a double click on the bar does it too; a single one leaves it
    let (mut s, g, _, eq) = picked(1600.0);
    let head = s.instrument_center(&g, eq, Part::Head);
    let (x, y) = head.unwrap_or_else(|| {
        let i = s.docked().unwrap();
        let r = s.instruments.frame(i, &g);
        (r.x + r.w / 2.0, r.y + 4.0)
    });
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert_eq!(docked_plugin(&s), Some(eq));
    assert!(s.instruments.grab.is_none(), "the face does not move");
    assert!(s.instrument_press(&g, x, y, 2, Mods::NONE));
    assert_eq!(s.docked(), None);
    assert!(s.instruments.is_open(eq));

    // a second press on the picked row opens it as a window as before
    let (mut s, g, t, eq) = picked(1600.0);
    click_row(&mut s, &g, t, eq);
    assert_eq!(s.docked(), None);
    assert!(s.instruments.is_open(eq));
}

#[test]
fn the_close_takes_the_face_away_and_the_strips_have_the_drawer() {
    let (mut s, g, _, eq) = picked(1600.0);
    press(&mut s, &g, eq, Part::Close, 1);
    assert_eq!(s.docked(), None);
    assert!(!s.instruments.is_open(eq));
    assert_eq!(s.mixer_split(&g), None);
    assert_eq!(s.mixer_body(&g), g.drawer_body);
}

#[test]
fn the_face_shows_only_with_the_mixer() {
    let (mut s, g, _, eq) = picked(1600.0);
    let panel = s.mixer_split(&g).unwrap().panel;
    s.tab = DrawerTab::Roll;
    let roll = s.geometry(1600.0, 1000.0);
    assert_eq!(s.docked(), None);
    let i = s.instruments.docked().unwrap();
    assert_eq!(s.instruments.frame(i, &roll).w, 0.0);
    let (x, y) = (panel.x + panel.w / 2.0, panel.y + panel.h / 2.0);
    assert_eq!(s.instruments.at(&s.project(), &roll, x, y), None);
    s.tab = DrawerTab::Mixer;
    assert_eq!(docked_plugin(&s), Some(eq));
    // a window of it opened while the mixer is away comes out of the drawer
    s.tab = DrawerTab::Roll;
    let t = s.instruments.open[i].track;
    assert!(s.open_plugin(t, eq));
    assert_eq!(s.instruments.docked(), None);
    assert!(s.instruments.is_open(eq));
}

#[test]
fn the_grip_sets_the_face_width_by_the_pointer_and_by_sixteen_with_the_keys() {
    let (mut s, g, _, _) = picked(1600.0);
    let split = s.mixer_split(&g).unwrap();
    let (x, y) = (
        split.grip.x + split.grip.w / 2.0,
        split.grip.y + split.grip.h / 2.0,
    );
    assert!(s.grip_down(&g, x, y));
    assert!(s.grip_move(&g, x - 40.0));
    assert!(s.grip_up());
    let wider = s.mixer_split(&g).unwrap();
    assert_eq!(wider.panel.w, split.panel.w + 40.0);
    assert_eq!(wider.mixer.w, split.mixer.w - 40.0);
    // the grip was pressed last: ←→ move it by 16
    assert!(s.field_keys(&g, "left", None, Mods::NONE));
    assert_eq!(s.mixer_split(&g).unwrap().panel.w, wider.panel.w + STEP);
    assert!(s.field_keys(&g, "right", None, Mods::NONE));
    assert!(s.field_keys(&g, "right", None, Mods::NONE));
    assert_eq!(s.mixer_split(&g).unwrap().panel.w, wider.panel.w - STEP);
    // the next face takes the width it was left at
    assert_eq!(s.instruments.dock_w, Some(wider.panel.w - STEP));
    // a press elsewhere lets the keys go
    let r = s.mixer_strips(&g)[0].rect;
    assert!(!s.grip_down(&g, r.x + 4.0, r.y + 4.0));
    assert!(!s.mixer.grip_focus);
}

#[test]
fn a_narrow_mixer_folds_to_bands_and_keeps_the_strip_in_front() {
    let (mut s, g, t, _) = picked(1600.0);
    // the face is pulled wide until less than 240 is left for the strips
    let split = s.mixer_split(&g).unwrap();
    let (x, y) = (split.grip.x + 1.0, split.grip.y + split.grip.h / 2.0);
    assert!(s.grip_down(&g, x, y));
    s.grip_move(&g, x - (split.mixer.w - FOLD_W) - 8.0);
    s.grip_up();
    let folded = s.mixer_split(&g).unwrap();
    assert!(folded.folded);
    let n = s.project().tracks.len();
    assert_eq!(folded.mixer.w, folded_w(n));
    let strips = s.mixer_strips(&g);
    assert_eq!(strips.len(), 1);
    assert_eq!(strips[0].track, t);
    assert_eq!(strips[0].rect.w, WIDE_W);
    // every other channel is a band, none scrolled away, and the master stays at the right
    let bands = s.mixer_bands(&g);
    assert_eq!(bands.len(), n - 1);
    assert_eq!(s.mixer_max_scroll(&g), 0.0);
    let (_, master) = bands.iter().find(|(x, _)| x.is_master()).unwrap();
    assert_eq!(master.right(), folded.mixer.right() - STRIP_GAP);
    assert!(
        bands
            .iter()
            .all(|(_, r)| r.x >= folded.mixer.x && r.right() <= folded.mixer.right())
    );
    assert!(bands.iter().all(|(_, r)| r.w == BAND_W));
    assert!(bands.iter().any(|(x, _)| x.is_master()));
    assert!(bands.iter().all(|(x, _)| *x != t));
    // a band's press brings its channel in front
    let (other, r) = *bands.iter().find(|(x, _)| !x.is_master()).unwrap();
    let hit = s.mixer_hit(&g, r.x + r.w / 2.0, r.y + r.h / 2.0).unwrap();
    assert_eq!(hit, MixerHit::Strip(other));
    assert!(s.mixer_press(&g, hit));
    let strips = s.mixer_strips(&g);
    assert_eq!(strips.len(), 1);
    assert_eq!(strips[0].track, other);
    assert_eq!(strips[0].rect.w, WIDE_W);
    assert!(s.mixer_strips(&g).iter().all(|x| x.track != t));
    // the face is the same device
    assert!(s.docked().is_some());
}

/// The window at its least (960×600): the face sits beside the strips, not under them, and
/// pulled wide it folds every channel to a band beside it, all inside the drawer (MIX-046).
#[test]
fn at_the_least_window_the_face_sits_beside_the_strips_and_folds_them_all() {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    // the drawer as tall as the least window lets it be, so the row to press is in view
    s.drawer_size.mixer = Some(600.0);
    s.instruments.open.clear();
    let (w, h) = crate::studio::WINDOW_MIN;
    let g = s.geometry(w, h);
    let body = g.drawer_body;
    assert!(body.h > 0.0 && body.w >= STACK_W, "{body:?}");
    // a channel with no instrument area, so the row is in sight in the short drawer
    let t = s
        .mixer_strips(&g)
        .iter()
        .map(|x| x.track)
        .find(|t| s.project().tracks[t].kind != xirius_core::model::TrackKind::Instrument)
        .unwrap();
    let eq = add(&mut s, t, "EQ");
    click_row(&mut s, &g, t, eq);
    let inside = |r: Rect| {
        r.x >= body.x - 0.5
            && r.right() <= body.right() + 0.5
            && r.y >= body.y - 0.5
            && r.bottom() <= body.bottom() + 0.5
    };
    let split = s.mixer_split(&g).unwrap();
    assert!(!split.stacked);
    assert!(inside(split.mixer) && inside(split.panel), "{split:?}");
    assert!(split.grip.w == GRIP_W && split.grip.x >= split.mixer.right());

    let i = s.docked().unwrap();
    s.instruments.dock_w = None;
    s.instruments.open[i].body.0 = widest(body);
    let split = s.mixer_split(&g).unwrap();
    assert!(split.folded && !split.stacked);
    assert!(inside(split.mixer) && inside(split.panel), "{split:?}");
    assert!(split.panel.w >= FOLD_W, "{split:?}");
    assert_eq!(s.instruments.rect(i, &g), split.panel);
    let strips = s.mixer_strips(&g);
    assert_eq!(strips.len(), 1);
    assert_eq!(strips[0].rect.w, WIDE_W);
    let bands = s.mixer_bands(&g);
    assert_eq!(strips.len() + bands.len(), s.project().tracks.len());
    assert_eq!(s.mixer_max_scroll(&g), 0.0);
    assert!(
        strips
            .iter()
            .map(|x| x.rect)
            .chain(bands.iter().map(|(_, r)| *r))
            .all(|r| r.x >= split.mixer.x && r.right() <= split.mixer.right() && inside(r))
    );
    let (_, master) = bands.iter().find(|(x, _)| x.is_master()).unwrap();
    assert_eq!(master.right(), split.mixer.right() - STRIP_GAP);
}

#[test]
fn a_small_screen_folds_the_strips_as_the_face_opens() {
    let (s, g, t, _) = picked(760.0);
    let split = s.mixer_split(&g).unwrap();
    assert!(split.folded);
    assert!(split.mixer.w >= FOLDED_MIN.min(g.drawer_body.w - GRIP_W));
    let strips = s.mixer_strips(&g);
    assert!(strips.iter().all(|x| x.track == t && x.rect.w == WIDE_W));
    assert!(
        s.mixer_bands(&g)
            .iter()
            .all(|(_, r)| r.right() <= split.grip.x)
    );
}

#[test]
fn the_face_the_grip_and_the_bands_have_names() {
    let (mut s, g, t, _) = picked(1600.0);
    let marks = crate::a11y::marks(&s, &g);
    let find = |label: &str| {
        marks
            .iter()
            .find(|m| m.node.label() == Some(label))
            .unwrap_or_else(|| panic!("no mark named {label}"))
    };
    assert_eq!(
        find("窓で開く: Vocal › EQ").node.role(),
        accesskit::Role::Button
    );
    assert_eq!(
        find("閉じる: Vocal › EQ").node.role(),
        accesskit::Role::Button
    );
    let grip = find(GRIP_LABEL);
    assert_eq!(grip.node.role(), accesskit::Role::Splitter);
    let w = s.mixer_split(&g).unwrap().panel.w;
    assert_eq!(grip.node.numeric_value(), Some(f64::from(w.round())));
    // folded, each band is a button named for its channel that brings it in front
    s.instruments.dock_w = None;
    let i = s.docked().unwrap();
    s.instruments.open[i].body.0 = widest(g.drawer_body);
    let marks = crate::a11y::marks(&s, &g);
    let bands = s.mixer_bands(&g);
    assert!(!bands.is_empty());
    for (b, _) in bands {
        let name = s.project().tracks[&b].shown_name().to_string();
        let m = marks.iter().find(|m| m.key == format!("band-{b}")).unwrap();
        assert_eq!(m.node.label(), Some(name.as_str()));
        assert_eq!(m.press, Some(Hit::Mixer(MixerHit::Strip(b))));
    }
    assert_ne!(s.mixer_lead(), None);
    assert_eq!(s.mixer_lead(), Some(t));
}

#[test]
fn a_narrow_drawer_has_the_face_under_the_strips_without_the_grip() {
    let (mut s, g, t, eq) = picked(380.0);
    let split = s.mixer_split(&g).unwrap();
    let body = g.drawer_body;
    assert!(body.w < STACK_W);
    assert!(split.stacked && !split.folded);
    assert_eq!((split.mixer.x, split.mixer.w), (body.x, body.w));
    assert_eq!((split.panel.x, split.panel.w), (body.x, body.w));
    assert_eq!(split.panel.bottom(), body.bottom());
    assert!(split.mixer.bottom() < split.panel.y);
    assert!(split.panel.h <= body.h * 0.5);
    assert!(s.mixer_strips(&g).iter().any(|x| x.track == t));
    assert!(
        s.mixer_strips(&g)
            .iter()
            .all(|x| x.rect.bottom() <= split.panel.y)
    );
    // the face is the device's, with its bar; there is no grip to press or name
    let i = s.docked().unwrap();
    assert_eq!(s.instruments.frame(i, &g), split.panel);
    assert!(s.instrument_center(&g, eq, Part::Pin).is_some());
    let marks = crate::a11y::marks(&s, &g);
    assert!(marks.iter().all(|m| m.key != "dock-grip"));
    let (x, y) = (split.panel.x + 4.0, split.panel.y - GRIP_W / 2.0);
    assert!(!s.grip_down(&g, x, y));
}
