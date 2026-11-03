use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use super::*;
use crate::popup::{Action, PopupKind};
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

fn builtin(s: &mut Studio, t: TrackId, uid: &str) -> PluginId {
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": uid, "kind": "effect", "format": "builtin", "uid": uid }),
    ));
    s.project().tracks[&t].inserts.last().unwrap().id
}

/// Bass with a compressor the kick (Drums) can key.
fn bass_comp(s: &mut Studio) -> (TrackId, TrackId, PluginId) {
    let bass = track(s, "Bass");
    let drums = track(s, "Drums");
    let comp = builtin(s, bass, "compressor");
    (drums, bass, comp)
}

fn send_list(s: &mut Studio, t: TrackId) -> crate::pick_list::PickList {
    s.pick_view(PopupKind::Send(t)).unwrap()
}

#[test]
fn the_send_plus_lists_the_plug_ins_that_take_a_key_under_the_buses() {
    let (mut s, _) = studio();
    let (drums, bass, comp) = bass_comp(&mut s);
    // an EQ takes no key and is not listed
    builtin(&mut s, bass, "eq");
    let list = send_list(&mut s, drums);
    assert_eq!(list.groups[0].heading, "バス");
    let sc = &list.groups[1];
    assert_eq!(sc.heading, "サイドチェーン");
    let row = sc
        .rows
        .iter()
        .find(|r| r.label == "Bass › compressor")
        .unwrap();
    assert_eq!(row.icon, Some("sc"));
    assert!(!row.on && row.enabled);
    assert_eq!(row.act, Some(Action::Pick(format!("{PICK}{comp}"))));
    assert_eq!(sc.rows.len(), 1);
    // the strip's own plug-ins are not listed on it
    assert!(
        send_list(&mut s, bass)
            .groups
            .iter()
            .all(|g| g.heading != "サイドチェーン")
    );
}

#[test]
fn a_pick_keys_the_plug_in_in_one_step_and_a_second_takes_it_away() {
    let (mut s, g) = studio();
    let (drums, bass, comp) = bass_comp(&mut s);
    let pick = format!("{PICK}{comp}");
    s.mixer_pick(PopupKind::Send(drums), &pick);
    let keys = key_sends(&s.project(), drums);
    assert_eq!(keys.len(), 1);
    let k = &keys[0];
    assert_eq!(
        (k.owner, k.plugin, k.pre, k.level_db),
        (bass, comp, false, 0.0)
    );
    assert!(k.on);
    // its row is after the strip's sends, with the bar at its right end
    let row = s.mixer_rect(&g, MixerHit::KeySend(drums, k.id)).unwrap();
    let bar = s.mixer_rect(&g, MixerHit::KeyLevel(drums, k.id)).unwrap();
    assert!(bar.x > row.x && bar.right() <= row.right() + 0.5);
    let (x, y) = s.mixer_center(&g, MixerHit::KeyLevel(drums, k.id)).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(MixerHit::KeyLevel(drums, k.id)));
    let add = s.mixer_rect(&g, MixerHit::AddSend(drums)).unwrap();
    assert!(add.y > row.y);
    // the list checks it now
    let list = send_list(&mut s, drums);
    assert!(list.groups[1].rows[0].on);
    assert!(s.run_command("history.undo", json!({})));
    assert!(key_sends(&s.project(), drums).is_empty());
    assert!(s.run_command("history.redo", json!({})));
    s.mixer_pick(PopupKind::Send(drums), &pick);
    assert!(key_sends(&s.project(), drums).is_empty());
}

#[test]
fn a_key_given_to_a_receiver_turned_off_turns_it_on() {
    let (mut s, _) = studio();
    let (drums, bass, comp) = bass_comp(&mut s);
    let keys = track(&s, "Keys");
    assert!(s.run_command(
        "plugin.sidechain.add",
        json!({ "id": comp.to_string(), "from": keys.to_string() })
    ));
    assert!(s.run_command(
        "plugin.sidechain.enable",
        json!({ "id": comp.to_string(), "state": "off" })
    ));
    s.mixer_pick(PopupKind::Send(drums), &format!("{PICK}{comp}"));
    let p = s.project();
    let x = p.tracks[&bass]
        .inserts
        .iter()
        .find(|x| x.id == comp)
        .unwrap();
    assert!(x.sidechain.as_ref().unwrap().on);
}

#[test]
fn a_receiver_a_key_would_loop_back_to_is_dim_with_the_loop_mark() {
    let (mut s, _) = studio();
    let (drums, bass, comp) = bass_comp(&mut s);
    let drum_comp = builtin(&mut s, drums, "compressor");
    s.mixer_pick(PopupKind::Send(drums), &format!("{PICK}{comp}"));
    // Bass waits on Drums now: Bass keying the drums' compressor would loop
    let list = send_list(&mut s, bass);
    let row = list.groups[1]
        .rows
        .iter()
        .find(|r| r.label == "Drums › compressor")
        .unwrap();
    assert!(!row.enabled);
    assert_eq!(row.icon, Some("loop"));
    assert!(!s.project().sidechain_ok(drum_comp, bass));
}

#[test]
fn the_bar_follows_the_pointer_and_lets_go_in_one_step() {
    let (mut s, g) = studio();
    let (drums, _, comp) = bass_comp(&mut s);
    s.mixer_pick(PopupKind::Send(drums), &format!("{PICK}{comp}"));
    let id = key_sends(&s.project(), drums)[0].id;
    let zone = s.mixer_rect(&g, MixerHit::KeyLevel(drums, id)).unwrap();
    let bar = key_bar(zone);
    let hit = Hit::Mixer(MixerHit::KeyLevel(drums, id));
    // a press at the left end is the floor, the pointer half along the middle
    assert!(s.level_down(&g, hit, bar.x));
    assert_eq!(s.key_level(drums, id), Some(KEY_MIN_DB));
    s.level_move(bar.x + bar.w / 2.0, false);
    let mid = bar_db(0.5);
    assert_eq!(s.key_level(drums, id), Some(mid));
    // nothing is written until the bar is let go
    assert_eq!(key_sends(&s.project(), drums)[0].level_db, 0.0);
    assert!(s.level_up());
    assert_eq!(key_sends(&s.project(), drums)[0].level_db, mid);
    assert!(s.run_command("history.undo", json!({})));
    assert_eq!(key_sends(&s.project(), drums)[0].level_db, 0.0);
}

#[test]
fn the_arrows_move_the_bar_pressed_last_by_a_db_and_with_shift_a_fifth() {
    let (mut s, g) = studio();
    let (drums, _, comp) = bass_comp(&mut s);
    s.mixer_pick(PopupKind::Send(drums), &format!("{PICK}{comp}"));
    let id = key_sends(&s.project(), drums)[0].id;
    assert!(s.press(&g, Hit::Mixer(MixerHit::KeyLevel(drums, id))));
    assert_eq!(s.mixer.key_focus, Some((drums, id)));
    let level = |s: &Studio| key_sends(&s.project(), drums)[0].level_db;
    assert!(s.key_bar_key("left", Mods::default()));
    assert_eq!(level(&s), -1.0);
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    for _ in 0..5 {
        assert!(s.key_bar_key("right", shift));
    }
    assert_eq!(level(&s), 0.0);
    // held at the top
    for _ in 0..8 {
        s.key_bar_key("right", Mods::default());
    }
    assert_eq!(level(&s), KEY_MAX_DB);
    assert!(s.key_bar_key("escape", Mods::default()));
    assert_eq!(s.mixer.key_focus, None);
    assert!(!s.key_bar_key("left", Mods::default()));
}

#[test]
fn a_press_on_a_key_send_row_opens_the_receiver() {
    let (mut s, g) = studio();
    let (drums, _, comp) = bass_comp(&mut s);
    s.mixer_pick(PopupKind::Send(drums), &format!("{PICK}{comp}"));
    let id = key_sends(&s.project(), drums)[0].id;
    s.instruments.open.clear();
    assert!(s.press(&g, Hit::Mixer(MixerHit::KeySend(drums, id))));
    assert!(s.instruments.is_open(comp));
}

#[test]
fn a_row_under_the_pointer_lights_its_partner() {
    let (mut s, _) = studio();
    let (drums, bass, comp) = bass_comp(&mut s);
    s.mixer_pick(PopupKind::Send(drums), &format!("{PICK}{comp}"));
    let id = key_sends(&s.project(), drums)[0].id;
    s.hover = Hit::Mixer(MixerHit::KeySend(drums, id));
    assert_eq!(
        s.mixer_partners(),
        Partners {
            sends: vec![(drums, id)],
            plugins: vec![comp],
        }
    );
    s.hover = Hit::Mixer(MixerHit::Open(bass, comp));
    assert_eq!(s.mixer_partners().sends, vec![(drums, id)]);
    s.hover = Hit::None;
    assert_eq!(s.mixer_partners(), Partners::default());
}

#[test]
fn the_screen_reader_names_the_key_send_and_its_level() {
    let (mut s, g) = studio();
    let (drums, _, comp) = bass_comp(&mut s);
    s.mixer_pick(PopupKind::Send(drums), &format!("{PICK}{comp}"));
    let id = key_sends(&s.project(), drums)[0].id;
    assert!(s.press(&g, Hit::Mixer(MixerHit::KeyLevel(drums, id))));
    assert!(s.key_bar_key("left", Mods::default()));
    let marks = crate::a11y::marks(&s, &g);
    let find = |hit: MixerHit| {
        marks
            .iter()
            .find(|m| m.key == format!("flow-{drums}-{hit:?}"))
            .unwrap_or_else(|| panic!("no {hit:?}"))
    };
    let row = find(MixerHit::KeySend(drums, id));
    assert_eq!(row.node.role(), accesskit::Role::Button);
    assert_eq!(
        row.node.label(),
        Some("サイドチェーンのセンド：Bass › compressor")
    );
    let bar = find(MixerHit::KeyLevel(drums, id));
    assert_eq!(bar.node.role(), accesskit::Role::Slider);
    assert_eq!(bar.node.label(), Some("Bass › compressor へのセンドの量"));
    assert_eq!(bar.node.numeric_value(), Some(-1.0));
    assert_eq!(bar.node.value(), Some("-1.0 dB"));
    assert_eq!(bar.node.min_numeric_value(), Some(KEY_MIN_DB));
}

#[test]
fn the_mixer_watches_the_receivers_only_while_it_shows() {
    let (mut s, _) = studio();
    let (drums, _, comp) = bass_comp(&mut s);
    s.mixer_pick(PopupKind::Send(drums), &format!("{PICK}{comp}"));
    let p = s.project();
    assert_eq!(s.gr_ids(&p), vec![comp]);
    s.poll_gr(&p, &[]);
    assert_eq!(s.mixer.gr_watched, vec![comp]);
    s.drawer_open = false;
    // what it let go is still read once, to be unwatched
    assert_eq!(s.gr_ids(&p), vec![comp]);
    s.poll_gr(&p, &[]);
    assert!(s.mixer.gr_watched.is_empty());
    assert!(s.gr_ids(&p).is_empty());
}
