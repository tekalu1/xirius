use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::rack::RackNode;

use super::*;
use crate::instrument::Pick as Effect;
use crate::mixer::flow::{COARSE_H, Flow};
use crate::mixer::{STRIP_GAP, WIDE_W};
use crate::studio::DrawerTab;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    let g = s.geometry(1600.0, 1000.0);
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
    s.project().tracks[&t].inserts.last().unwrap().id
}

fn names(s: &Studio, t: TrackId) -> Vec<String> {
    s.project().tracks[&t]
        .inserts
        .iter()
        .map(|p| p.name.clone())
        .collect()
}

/// Vocal with EQ, then a splitter: Comp in one branch, Reverb in the other.
fn vocal(s: &mut Studio) -> (TrackId, NodeId, [NodeId; 2], [PluginId; 3]) {
    let t = track(s, "Vocal");
    let eq = add(s, t, "EQ");
    let comp = add(s, t, "Comp");
    let verb = add(s, t, "Reverb");
    assert!(s.run_command("rack.split.add", json!({"track":t})));
    let (sp, b) = match s.project().tracks[&t].rack.as_ref().unwrap().fx.last() {
        Some(RackNode::Split(x)) => (x.id, [x.branches[0].id, x.branches[1].id]),
        other => panic!("{other:?}"),
    };
    assert!(s.run_command("rack.node.move", json!({"id":comp,"to":b[0]})));
    assert!(s.run_command("rack.node.move", json!({"id":verb,"to":b[1]})));
    (t, sp, b, [eq, comp, verb])
}

fn flow_of(s: &Studio, g: &Geometry, t: TrackId) -> Flow {
    let strip = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .unwrap();
    s.strip_flow(&strip, &s.project().tracks[&t])
}

fn branch(s: &Studio, t: TrackId, b: NodeId) -> (f64, f64, bool) {
    s.project().tracks[&t]
        .rack
        .as_ref()
        .unwrap()
        .levels(b)
        .unwrap()
}

fn key(s: &mut Studio, g: &Geometry, key: &str, alt: bool) -> bool {
    let mods = Mods {
        alt,
        ..Mods::default()
    };
    s.field_keys(g, key, None, mods)
}

/// A press and a let go on a row's name, as the pointer does it.
fn click_row(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId) {
    let (x, y) = s.mixer_center(g, MixerHit::Open(t, id)).unwrap();
    s.press_keys(g, x, y);
    assert!(s.chain_down(g, x, y));
    assert!(s.fx_up(g, x, y, false));
}

#[test]
fn a_press_picks_a_row_and_a_press_on_the_picked_row_opens_it() {
    let (mut s, g) = studio();
    let t = track(&s, "Guitar");
    let eq = add(&mut s, t, "EQ");
    s.instruments.open.clear();
    click_row(&mut s, &g, t, eq);
    assert_eq!(s.mixer.chosen, Some((t, Pick::Plugin(eq))));
    // its face shows beside the strips (MIX-046), not as a window
    assert_eq!(s.docked().map(|i| s.instruments.open[i].plugin), Some(eq));
    click_row(&mut s, &g, t, eq);
    assert!(s.instruments.is_open(eq));
    assert_eq!(s.docked(), None);
    s.instruments.open.clear();
    // a press elsewhere lets the row go
    let fader = s.mixer_center(&g, MixerHit::Fader(t)).unwrap();
    s.chosen_press(&g, fader.0, fader.1);
    assert_eq!(s.mixer.chosen, None);
}

#[test]
fn the_picked_row_has_a_level_row_whose_bar_and_m_move_its_branch() {
    let (mut s, g) = studio();
    let (t, _, b, [_, _, verb]) = vocal(&mut s);
    assert!(flow_of(&s, &g, t).level().is_none());
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    let flow = flow_of(&s, &g, t);
    let (l, sub) = *flow.level().unwrap();
    let (_, under) = *flow.picked(Pick::Plugin(verb)).unwrap();
    assert!((under.bottom()..=under.bottom() + 1.0).contains(&sub.y));
    assert_eq!(l.target.map(|x| (x.id, x.layer)), Some((b[1], false)));
    // a press at the bar's right end and a drag to its middle: one step of the history
    let bar = s.mixer_rect(&g, MixerHit::Level(t, b[1])).unwrap();
    let hit = s.hit(&g, bar.right() - 1.0, bar.y + bar.h / 2.0);
    assert_eq!(hit, Hit::Mixer(MixerHit::Level(t, b[1])));
    assert!(s.level_down(&g, hit, bar.right() - 1.0));
    assert!(s.level_move(bar.x + bar.w / 2.0, false));
    let want = (flow::level_db(0.5) * 10.0).round() / 10.0;
    assert_eq!(branch(&s, t, b[1]).0, 0.0);
    assert!(s.level_up());
    assert_eq!(branch(&s, t, b[1]).0, want);
    assert!(s.run_command("history.undo", json!({})));
    assert_eq!(branch(&s, t, b[1]).0, 0.0);
    // the level row's M
    let m = MixerHit::RowMute(t, b[1]);
    let (x, y) = s.mixer_center(&g, m).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(m));
    assert!(s.press(&g, Hit::Mixer(m)));
    assert!(branch(&s, t, b[1]).2);
}

#[test]
fn a_row_with_a_level_shows_its_m_under_the_pointer() {
    let (mut s, g) = studio();
    let (t, _, b, [eq, comp, _]) = vocal(&mut s);
    let m = MixerHit::RowMute(t, b[0]);
    assert!(s.mixer_center(&g, m).is_none());
    s.hover = Hit::Mixer(MixerHit::Open(t, comp));
    let (x, y) = s.mixer_center(&g, m).unwrap();
    assert!(s.press(&g, Hit::Mixer(m)));
    assert!(branch(&s, t, b[0]).2);
    // a row that heads no branch has no M
    s.hover = Hit::Mixer(MixerHit::Open(t, eq));
    assert!(flow_of(&s, &g, t).plugin(eq).unwrap().0.target.is_none());
    assert_ne!(s.hit(&g, x, y), Hit::Mixer(m));
}

#[test]
fn the_plus_after_a_row_puts_the_effect_right_after_it_and_picks_it() {
    let (mut s, g) = studio();
    let t = track(&s, "Guitar");
    let eq = add(&mut s, t, "EQ");
    add(&mut s, t, "Delay");
    s.instruments.open.clear();
    s.mixer.chosen = Some((t, Pick::Plugin(eq)));
    let after = MixerHit::After(t, Pick::Plugin(eq));
    let (x, y) = s.mixer_center(&g, after).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(after));
    assert!(s.press(&g, Hit::Mixer(after)));
    assert!(matches!(
        s.popup.as_ref().map(|p| p.kind),
        Some(PopupKind::Insert(x)) if x == t
    ));
    assert_eq!(s.mixer.insert_after, Some((t, Pick::Plugin(eq))));
    s.popup = None;
    assert!(s.add_effect_after(t, Pick::Plugin(eq), &Effect::built_in("Comp")));
    assert_eq!(names(&s, t), ["EQ", "Comp", "Delay"]);
    let comp = s.project().tracks[&t].inserts[1].id;
    assert_eq!(s.mixer.chosen, Some((t, Pick::Plugin(comp))));
}

#[test]
fn the_plus_after_a_row_in_a_branch_lands_in_that_branch() {
    let (mut s, _) = studio();
    let (t, _, b, [_, comp, _]) = vocal(&mut s);
    assert!(s.add_effect_after(t, Pick::Plugin(comp), &Effect::built_in("Delay")));
    let rack = s.project().tracks[&t].rack.clone().unwrap();
    let Some(RackNode::Split(sp)) = rack.fx.last() else {
        panic!()
    };
    assert_eq!(sp.branches[0].id, b[0]);
    assert_eq!(sp.branches[0].fx.len(), 2);
}

#[test]
fn the_keys_move_pick_open_and_take_away_the_picked_row() {
    let (mut s, g) = studio();
    let t = track(&s, "Guitar");
    let eq = add(&mut s, t, "EQ");
    let delay = add(&mut s, t, "Delay");
    s.instruments.open.clear();
    // nothing picked: the keys go on
    assert!(!key(&mut s, &g, "down", false));
    s.mixer.chosen = Some((t, Pick::Plugin(eq)));
    assert!(key(&mut s, &g, "down", false));
    assert_eq!(s.mixer.chosen, Some((t, Pick::Plugin(delay))));
    assert!(key(&mut s, &g, "up", true));
    assert_eq!(names(&s, t), ["Delay", "EQ"]);
    assert_eq!(s.mixer.chosen, Some((t, Pick::Plugin(delay))));
    assert!(key(&mut s, &g, "space", false));
    assert!(s.project().tracks[&t].inserts[0].bypass);
    assert!(key(&mut s, &g, "enter", false));
    assert!(s.instruments.is_open(delay));
    s.instruments.open.clear();
    assert!(key(&mut s, &g, "insert", false));
    assert!(matches!(
        s.popup.as_ref().map(|p| p.kind),
        Some(PopupKind::Insert(_))
    ));
    s.popup = None;
    assert!(key(&mut s, &g, "delete", false));
    assert_eq!(names(&s, t), ["EQ"]);
    assert_eq!(s.mixer.chosen, Some((t, Pick::Plugin(eq))));
    assert!(key(&mut s, &g, "escape", false));
    assert_eq!(s.mixer.chosen, None);
}

#[test]
fn alt_left_and_right_move_the_level_and_m_mutes_the_branch() {
    let (mut s, g) = studio();
    let (t, sp, b, [eq, comp, _]) = vocal(&mut s);
    s.mixer.chosen = Some((t, Pick::Plugin(eq)));
    assert!(key(&mut s, &g, "down", false));
    assert_eq!(s.mixer.chosen, Some((t, Pick::Split(sp))));
    assert!(key(&mut s, &g, "down", false));
    assert_eq!(s.mixer.chosen, Some((t, Pick::Plugin(comp))));
    assert!(key(&mut s, &g, "left", true));
    assert!(key(&mut s, &g, "left", true));
    assert_eq!(branch(&s, t, b[0]).0, -2.0);
    assert!(key(&mut s, &g, "right", true));
    assert_eq!(branch(&s, t, b[0]).0, -1.0);
    assert!(key(&mut s, &g, "m", false));
    assert!(branch(&s, t, b[0]).2);
    // a splitter does not go with Delete
    s.mixer.chosen = Some((t, Pick::Split(sp)));
    assert!(key(&mut s, &g, "delete", false));
    assert!(s.error.is_some());
    assert!(flow::has_split(&s.project().tracks[&t], sp));
}

#[test]
fn a_touch_makes_the_rows_32_high() {
    let (mut s, g) = studio();
    let t = track(&s, "Guitar");
    let eq = add(&mut s, t, "EQ");
    let fine = flow_of(&s, &g, t).plugin(eq).unwrap().1.h;
    assert!(fine < COARSE_H);
    // a touch's press is hit at the height drawn; the rows grow once it is let go
    let row = flow_of(&s, &g, t).plugin(eq).unwrap().1;
    s.mixer.coarse_next = Some(true);
    s.press_keys(&g, row.x + 4.0, row.y + row.h - 2.0);
    assert_eq!(flow_of(&s, &g, t).plugin(eq).unwrap().1, row);
    s.coarse_up();
    assert!(s.mixer.coarse);
    assert_eq!(flow_of(&s, &g, t).plugin(eq).unwrap().1.h, COARSE_H);
}

#[test]
fn the_widen_button_widens_its_strip_and_the_next_moves_along() {
    let (mut s, g) = studio();
    let strips = s.mixer_strips(&g);
    let (a, b) = (strips[0].track, strips[1].track);
    let narrow = strips[0].rect.w;
    let (x, y) = s.mixer_center(&g, MixerHit::Widen(a)).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(MixerHit::Widen(a)));
    assert!(s.press(&g, Hit::Mixer(MixerHit::Widen(a))));
    let strips = s.mixer_strips(&g);
    assert_eq!(strips[0].rect.w, WIDE_W);
    assert_eq!(strips[1].track, b);
    assert_eq!(strips[1].rect.x, strips[0].rect.x + WIDE_W + STRIP_GAP);
    assert_eq!(s.strip_offset(&g, b), Some(WIDE_W + STRIP_GAP));
    assert!(s.press(&g, Hit::Mixer(MixerHit::Widen(a))));
    assert_eq!(s.mixer_strips(&g)[0].rect.w, narrow);
}

#[test]
fn the_screen_reader_names_the_m_the_plus_the_level_and_the_name_buttons() {
    let (mut s, g) = studio();
    let (t, _, b, [_, comp, verb]) = vocal(&mut s);
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    s.hover = Hit::Mixer(MixerHit::Open(t, comp));
    let marks = crate::a11y::marks(&s, &g);
    let find = |hit: MixerHit| {
        marks
            .iter()
            .find(|m| m.key == format!("flow-{t}-{hit:?}"))
            .unwrap_or_else(|| panic!("no {hit:?}"))
    };
    let m = find(MixerHit::RowMute(t, b[0]));
    assert_eq!(m.node.label(), Some("Comp の分岐をミュート"));
    assert_eq!(m.node.toggled(), Some(accesskit::Toggled::False));
    let level = find(MixerHit::Level(t, b[1]));
    assert_eq!(level.node.role(), accesskit::Role::Slider);
    assert_eq!(level.node.label(), Some("Reverb の分岐の音量"));
    assert_eq!(level.node.numeric_value(), Some(0.0));
    let after = find(MixerHit::After(t, Pick::Plugin(verb)));
    assert_eq!(after.node.label(), Some("Reverb の後ろに挿す"));
    let editor = find(MixerHit::Editor(t));
    assert_eq!(editor.node.label(), Some("チャンネルエディターを開く"));
    let widen = find(MixerHit::Widen(t));
    assert_eq!(widen.node.label(), Some("広げる"));
    assert_eq!(widen.node.toggled(), Some(accesskit::Toggled::False));
}

#[test]
fn a_press_on_the_arrangement_gives_the_keys_back() {
    let (mut s, g) = studio();
    let t = track(&s, "Guitar");
    let eq = add(&mut s, t, "EQ");
    s.instruments.open.clear();
    click_row(&mut s, &g, t, eq);
    assert_eq!(s.mixer.chosen, Some((t, Pick::Plugin(eq))));
    // the screen begins every press with this (`screen.rs`, `pressed`)
    let (x, y) = (g.lanes.x + g.lanes.w / 2.0, g.lanes.y + 4.0);
    assert!(!matches!(s.hit(&g, x, y), Hit::Mixer(_)));
    s.press_keys(&g, x, y);
    assert_eq!(s.mixer.chosen, None);
    for k in ["space", "delete", "enter", "m", "insert", "down"] {
        assert!(!key(&mut s, &g, k, false), "{k} goes on to the keymap");
    }
    assert!(!s.project().tracks[&t].inserts[0].bypass);
    assert_eq!(names(&s, t), ["EQ"]);
}

#[test]
fn esc_while_a_knob_is_awaited_ends_the_wait_first() {
    let (mut s, g) = studio();
    let t = track(&s, "Guitar");
    let eq = add(&mut s, t, "EQ");
    s.instruments.open.clear();
    click_row(&mut s, &g, t, eq);
    // a part waits for a knob (as `midimap.learn` leaves it with an input open)
    let target = format!("track.volume:{t}");
    s.live_midi.assign.learning = Some(xirius_prefs::midimap::Target::parse(&target).unwrap());
    assert!(
        !key(&mut s, &g, "escape", false),
        "the keymap's Esc ends it"
    );
    assert_eq!(s.mixer.chosen, Some((t, Pick::Plugin(eq))));
}
