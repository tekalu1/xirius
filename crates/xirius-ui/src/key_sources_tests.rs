use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::KeyMode;

use super::*;
use crate::instrument::window::Part;
use crate::popup::PopupKind;
use crate::studio::{Geometry, Hit, Studio};

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

/// The sample song with a compressor on Bass, its window open.
fn studio() -> (Studio, Geometry, TrackId, TrackId, PluginId) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let bass = track(&s, "Bass");
    let drums = track(&s, "Drums");
    let comp = builtin(&mut s, bass, "compressor");
    s.instruments.open.clear();
    s.show_instrument(bass, comp);
    (s, g, drums, bass, comp)
}

fn add(s: &mut Studio, comp: PluginId, from: TrackId) {
    assert!(s.run_command(
        "plugin.sidechain.add",
        json!({ "id": comp.to_string(), "from": from.to_string() })
    ));
}

fn sources(s: &Studio, comp: PluginId) -> Vec<xirius_core::model::KeySource> {
    let p = s.project();
    p.tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == comp)
        .and_then(|x| x.sidechain.as_ref())
        .map(|sc| sc.sources.clone())
        .unwrap_or_default()
}

fn seat_press(s: &mut Studio, g: &Geometry, comp: PluginId, part: Part) {
    let (x, y) = s.instrument_center(g, comp, part).expect("in sight");
    assert!(s.instrument_down(g, x, y));
}

/// The index of the list's cell a reader names `spoken`.
fn cell(s: &Studio, g: &Geometry, spoken: &str) -> usize {
    s.popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .iter()
        .position(|c| c.spoken == spoken)
        .unwrap_or_else(|| panic!("no {spoken}"))
}

fn open(s: &Studio) -> Option<PopupKind> {
    s.popup.as_ref().map(|p| p.kind)
}

#[test]
fn with_no_source_the_seat_opens_the_list_and_with_one_turns_the_key() {
    let (mut s, g, drums, _, comp) = studio();
    seat_press(&mut s, &g, comp, Part::Sidechain);
    assert_eq!(open(&s), Some(PopupKind::KeySources(comp)));
    s.popup = None;
    add(&mut s, comp, drums);
    let on = |s: &Studio| {
        let p = s.project();
        p.tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|x| x.id == comp)
            .unwrap()
            .sidechain
            .as_ref()
            .unwrap()
            .on
    };
    seat_press(&mut s, &g, comp, Part::Sidechain);
    assert!(!on(&s));
    assert_eq!(open(&s), None);
    assert!(s.run_command("history.undo", json!({})));
    assert!(on(&s));
    // the name and its chevron open the list
    seat_press(&mut s, &g, comp, Part::Sources);
    assert_eq!(open(&s), Some(PopupKind::KeySources(comp)));
}

#[test]
fn the_list_has_the_tracks_then_the_buses_without_its_own_channel() {
    let (mut s, _, drums, bass, comp) = studio();
    assert!(s.run_command("track.add.bus", json!({})));
    let list = s.pick_view(PopupKind::KeySources(comp)).unwrap();
    assert!(list.keyed);
    let heads: Vec<&str> = list.groups.iter().map(|g| g.heading.as_str()).collect();
    assert_eq!(heads, ["トラック", "バス"]);
    for row in &list.groups[0].rows {
        let key = row.key.unwrap();
        assert_ne!(key.track, bass);
        assert!(!key.track.is_master());
    }
    assert!(
        list.groups[0]
            .rows
            .iter()
            .any(|r| r.key.unwrap().track == drums)
    );
    assert_eq!(list.empty, "一致するチャンネルはありません");
}

#[test]
fn a_channel_the_key_would_loop_back_from_shows_the_loop_and_takes_nothing() {
    let (mut s, g, drums, bass, comp) = studio();
    // Drums waits on Bass: Drums keying Bass's compressor would loop
    let drum_comp = builtin(&mut s, drums, "compressor");
    add(&mut s, drum_comp, bass);
    s.instruments.open.clear();
    s.show_instrument(bass, comp);
    seat_press(&mut s, &g, comp, Part::Sources);
    let list = s.popup.as_ref().unwrap().pick.clone().unwrap();
    let row = list.groups[0]
        .rows
        .iter()
        .find(|r| r.label == "Drums")
        .unwrap();
    assert!(!row.enabled && row.key.unwrap().looped);
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    assert!(
        layout
            .cells
            .iter()
            .any(|c| c.spoken == "ループ（輪になるので選べません）")
    );
    // the row itself is fainter, and a screen reader hears it cannot be picked
    let drums_row = layout
        .cells
        .iter()
        .find(|c| matches!(c.look, Look::Key(KLook::Row { .. })) && c.text == "Drums")
        .unwrap();
    assert!(!drums_row.enabled);
    fn find<'a>(marks: &'a [crate::a11y::Mark], label: &str) -> Option<&'a crate::a11y::Mark> {
        marks.iter().find_map(|m| {
            (m.node.role() == accesskit::Role::Group && m.node.label() == Some(label))
                .then_some(m)
                .or_else(|| find(&m.children, label))
        })
    }
    let marks = crate::a11y::marks(&s, &g);
    // The bridge to the window has no "off": the words say it (review 3 中6).
    let drums = find(&marks, "Drums（輪になるので選べません）").unwrap();
    assert!(drums.node.is_disabled());
    assert!(find(&marks, "Drums").is_none());
    // a row that can be picked is not said to be off
    let free = list.groups[0].rows.iter().find(|r| r.enabled).unwrap();
    assert!(!find(&marks, &free.label).unwrap().node.is_disabled());
    let send = &layout.cells[cell(&s, &g, "Drums センド（輪になるので選べません）")];
    assert!(!send.live());
    assert!(!s.press(
        &g,
        Hit::Item(cell(&s, &g, "Drums センド（輪になるので選べません）"))
    ));
    assert!(sources(&s, comp).is_empty());
}

#[test]
fn each_box_is_one_step_and_the_list_stays_open() {
    let (mut s, g, _, _, comp) = studio();
    seat_press(&mut s, &g, comp, Part::Sources);
    let mode = |s: &Studio| sources(s, comp).first().map(|k| k.mode);
    let press = |s: &mut Studio, what: &str| {
        let i = cell(s, &g, &format!("Drums {what}"));
        assert!(s.press(&g, Hit::Item(i)));
        assert_eq!(open(s), Some(PopupKind::KeySources(comp)));
    };
    press(&mut s, "センド");
    assert!(matches!(mode(&s), Some(KeyMode::Send { pre: false, .. })));
    // the amount shows beside the box now
    assert!(
        s.popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .iter()
            .any(|c| c.spoken == "Drums センドの量" && c.text == "0.0 dB")
    );
    press(&mut s, "プリフェーダー");
    assert!(matches!(mode(&s), Some(KeyMode::Send { pre: true, .. })));
    press(&mut s, "出力");
    assert_eq!(mode(&s), Some(KeyMode::Output));
    // with the output, the position takes nothing
    let pre = cell(&s, &g, "Drums プリフェーダー");
    assert!(!s.popup.as_ref().unwrap().layout(g.size).cells[pre].live());
    press(&mut s, "センド");
    assert!(matches!(mode(&s), Some(KeyMode::Send { pre: false, .. })));
    press(&mut s, "センド");
    assert_eq!(mode(&s), None);
    // one undo for each
    for want in ["send", "output", "pre", "send"] {
        assert!(s.run_command("history.undo", json!({})));
        let got = match mode(&s) {
            Some(KeyMode::Output) => "output",
            Some(KeyMode::Send { pre: true, .. }) => "pre",
            Some(KeyMode::Send { .. }) => "send",
            None => "none",
        };
        assert_eq!(got, want);
    }
}

#[test]
fn enter_gives_the_cursors_channel_a_send() {
    let (mut s, g, _, _, comp) = studio();
    seat_press(&mut s, &g, comp, Part::Sources);
    let first = s.popup.as_ref().unwrap().pick.as_ref().unwrap().groups[0].rows[0]
        .key
        .unwrap()
        .track;
    assert!(s.pick_typed(&g, "enter", None));
    let keys = sources(&s, comp);
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].track, first);
    assert_eq!(open(&s), Some(PopupKind::KeySources(comp)));
}

#[test]
fn the_amount_follows_the_pointer_and_lets_go_in_one_step() {
    let (mut s, g, drums, _, comp) = studio();
    add(&mut s, comp, drums);
    seat_press(&mut s, &g, comp, Part::Sources);
    let i = cell(&s, &g, "Drums センドの量");
    let bar = s.popup.as_ref().unwrap().layout(g.size).cells[i].rect;
    assert!(s.press(&g, Hit::Item(i)));
    assert!(s.level_down(&g, Hit::Item(i), bar.x));
    s.level_move(bar.x + bar.w / 2.0, false);
    let mid = crate::mixer::keys::bar_db(0.5);
    // the list shows the held amount, the song is not written yet
    let shown = &s.popup.as_ref().unwrap().layout(g.size).cells[i];
    assert_eq!(shown.text, amount_words(mid));
    assert_eq!(sources(&s, comp)[0].mode.send().unwrap().1, 0.0);
    assert!(s.level_up());
    assert_eq!(sources(&s, comp)[0].mode.send().unwrap().1, mid);
    assert_eq!(open(&s), Some(PopupKind::KeySources(comp)));
    assert!(s.run_command("history.undo", json!({})));
    assert_eq!(sources(&s, comp)[0].mode.send().unwrap().1, 0.0);
}

#[test]
fn the_reader_names_the_seat_and_each_part_of_a_row() {
    let (mut s, g, drums, _, comp) = studio();
    add(&mut s, comp, drums);
    let marks = crate::a11y::marks(&s, &g);
    let find = |marks: &[crate::a11y::Mark], key: String| {
        marks
            .iter()
            .find(|m| m.key == key)
            .unwrap_or_else(|| panic!("no {key}"))
            .node
            .clone()
    };
    let seat = find(&marks, format!("device-{comp}-sidechain"));
    assert_eq!(seat.role(), accesskit::Role::Button);
    assert_eq!(seat.label(), Some("サイドチェーン"));
    assert_eq!(seat.toggled(), Some(accesskit::Toggled::True));
    let list = find(&marks, format!("device-{comp}-sources"));
    assert_eq!(list.label(), Some("サイドチェーンのソース Drums"));
    assert_eq!(list.is_expanded(), Some(false));

    seat_press(&mut s, &g, comp, Part::Sources);
    let marks = crate::a11y::marks(&s, &g);
    let named = |label: &str| {
        marks
            .iter()
            .find(|m| m.node.label() == Some(label))
            .unwrap_or_else(|| panic!("no {label}"))
            .node
            .clone()
    };
    let row = named("Drums");
    assert_eq!(row.role(), accesskit::Role::Group);
    let send = named("Drums センド");
    assert_eq!(send.role(), accesskit::Role::CheckBox);
    assert_eq!(send.toggled(), Some(accesskit::Toggled::True));
    let out = named("Drums 出力");
    assert_eq!(out.toggled(), Some(accesskit::Toggled::False));
    let pre = named("Drums プリフェーダー");
    assert_eq!(pre.role(), accesskit::Role::Button);
    assert_eq!(pre.toggled(), Some(accesskit::Toggled::False));
    let amount = named("Drums センドの量");
    assert_eq!(amount.role(), accesskit::Role::Slider);
    assert_eq!(amount.numeric_value(), Some(0.0));
    assert_eq!(amount.value(), Some("0.0 dB"));
    assert_eq!(amount.max_numeric_value(), Some(6.0));
}

#[test]
fn the_seat_names_the_first_source_and_counts_the_rest() {
    let (mut s, _, drums, _, comp) = studio();
    let keys = track(&s, "Keys");
    let name = |s: &Studio| {
        let p = s.project();
        let x = p
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|x| x.id == comp)
            .unwrap()
            .clone();
        crate::instrument::window::source_of(&p, &x)
    };
    assert_eq!(name(&s), "");
    add(&mut s, comp, drums);
    assert_eq!(name(&s), "Drums");
    add(&mut s, comp, keys);
    assert_eq!(name(&s), "Drums +1");
}

#[test]
fn the_amount_reads_with_its_sign_and_a_tenth() {
    assert_eq!(amount_words(1.5), "+1.5 dB");
    assert_eq!(amount_words(0.0), "0.0 dB");
    assert_eq!(amount_words(-0.0), "0.0 dB");
    assert_eq!(amount_words(-12.0), "-12.0 dB");
    assert_eq!(amount_words(-0.04), "0.0 dB");
}
