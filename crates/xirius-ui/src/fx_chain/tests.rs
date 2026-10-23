use super::*;
use crate::studio::DrawerTab;
use xirius_core::auto::AutoTarget;

fn setup() -> (Studio, Geometry, TrackId, TrackId) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(1600.0, 1000.0);
    let p = s.project();
    let a = p.tracks.values().find(|t| t.name == "Guitar").unwrap().id;
    let b = p.tracks.values().find(|t| t.name == "Bass").unwrap().id;
    (s, g, a, b)
}
fn first_row(s: &Studio, g: &Geometry, t: TrackId) -> Rect {
    s.mixer_strips(g)
        .into_iter()
        .find(|l| l.track == t)
        .unwrap()
        .inserts
}
fn close(s: &mut Studio) {
    s.instruments.open.clear();
    s.popup = None;
}
fn grab(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId) -> (f32, f32) {
    s.mixer.set_insert_offset(t, 0);
    let r = s.mixer_rect(g, MixerHit::Open(t, id)).unwrap();
    let at = (r.x + 4.0, r.y + 11.0);
    assert!(s.chain_down(g, at.0, at.1));
    at
}

#[test]
fn insert_drag_moves_and_alt_copies_through_the_same_pointer_handlers() {
    let (mut s, g, a, b) = setup();
    let pick = Pick::built_in("EQ");
    s.add_effect(a, 0, &pick);
    close(&mut s);
    let id = s.project().tracks[&a]
        .inserts
        .iter()
        .find(|p| p.kind == PluginKind::Effect)
        .unwrap()
        .id;
    grab(&mut s, &g, a, id);
    let dest = first_row(&s, &g, b);
    s.fx_move(dest.x + 20.0, dest.y + 10.0);
    s.fx_up(&g, dest.x + 20.0, dest.y + 10.0, true);
    let copy = s.project().tracks[&b]
        .inserts
        .iter()
        .find(|p| p.name == "EQ")
        .unwrap()
        .id;
    assert_ne!(copy, id);
    assert!(s.project().tracks[&a].inserts.iter().any(|p| p.id == id));
    grab(&mut s, &g, a, id);
    let dest = first_row(&s, &g, TrackId::MASTER);
    s.fx_move(dest.x + 20.0, dest.y + 10.0);
    s.fx_up(&g, dest.x + 20.0, dest.y + 10.0, false);
    assert!(!s.project().tracks[&a].inserts.iter().any(|p| p.id == id));
    assert!(
        s.project().tracks[&TrackId::MASTER]
            .inserts
            .iter()
            .any(|p| p.id == id)
    );
}
#[test]
fn click_opens_and_remove_button_removes_the_insert() {
    let (mut s, g, a, _) = setup();
    s.add_effect(a, 0, &Pick::built_in("EQ"));
    close(&mut s);
    let id = s.project().tracks[&a].inserts[0].id;
    let at = grab(&mut s, &g, a, id);
    s.fx_up(&g, at.0, at.1, false);
    assert!(s.instruments.is_open(id));
    close(&mut s);
    let row = list_row(first_row(&s, &g, a), 0);
    let r = remove_part(row);
    assert!(s.chain_down(&g, r.x + 8.0, r.y + 11.0));
    assert!(!s.project().tracks[&a].inserts.iter().any(|p| p.id == id));
}
#[test]
fn effect_picker_search_enter_and_drag_open_the_editor() {
    let (mut s, g, a, _) = setup();
    let r = first_row(&s, &g, a);
    s.open_popup(PopupKind::Insert(a), r);
    assert!(s.fx_typed("e", Some("EQ")));
    assert!(s.fx_typed("enter", None));
    assert!(
        s.project().tracks[&a]
            .inserts
            .iter()
            .any(|p| p.name == "EQ")
    );
    assert!(!s.instruments.open.is_empty());
    close(&mut s);
    s.open_popup(PopupKind::Insert(a), r);
    let p = s.popup.as_ref().unwrap();
    let c = p
        .layout(g.size)
        .cells
        .into_iter()
        .find(|c| matches!(&c.action,Some(Action::Pick(to)) if to=="builtin:delay"))
        .unwrap();
    let from = (c.rect.x + 20.0, c.rect.y + 12.0);
    assert!(s.chain_down(&g, from.0, from.1));
    let dest = first_row(&s, &g, TrackId::MASTER);
    s.fx_move(dest.x + 20.0, dest.y + 10.0);
    s.fx_up(&g, dest.x + 20.0, dest.y + 10.0, false);
    assert!(
        s.project().tracks[&TrackId::MASTER]
            .inserts
            .iter()
            .any(|p| p.name == "Delay")
    );
}
#[test]
fn recent_effects_are_unique_and_keep_the_last_eight() {
    let (mut s, _, _, _) = setup();
    for i in 0..10 {
        assert!(s.add_effect(TrackId::MASTER, 0, &Pick::built_in(&format!("Effect{i}"))));
        close(&mut s);
    }
    assert_eq!(s.recent_effects().len(), 8);
    for name in ["EQ", "Delay", "EQ"] {
        s.add_effect(TrackId::MASTER, 0, &Pick::built_in(name));
        close(&mut s);
    }
    let recent = s.recent_effects();
    assert_eq!(recent.len(), 8);
    assert_eq!(recent[0].label, "EQ");
    assert_eq!(recent.iter().filter(|r| r.label == "EQ").count(), 1);
    assert_eq!(s.insert_list()[0].label, "EQ");
}
#[test]
fn many_inserts_scroll_without_moving_the_fader() {
    let (mut s, _, a, _) = setup();
    for _ in 0..64 {
        assert!(s.run_command("plugin.add", json!({"track":a.to_string(),"name":"EQ"})));
    }
    let g = s.geometry(1600.0, 1000.0);
    let before = s
        .mixer_strips(&g)
        .into_iter()
        .find(|r| r.track == a)
        .unwrap();
    for _ in 0..100 {
        s.mixer_wheel(
            &g,
            before.inserts.x + 30.0,
            before.inserts.y + 10.0,
            0.0,
            -1.0,
        );
    }
    let after = s
        .mixer_strips(&g)
        .into_iter()
        .find(|r| r.track == a)
        .unwrap();
    assert_eq!(before, after);
    assert!(s.mixer_rect(&g, MixerHit::AddInsert(a)).is_some());
    assert!(s.mixer.offset(a, List::Inserts) > 60);
}

#[test]
fn filtered_picker_does_not_scroll_past_its_only_result() {
    let (mut s, g, a, _) = setup();
    s.use_plugins(xirius_plugins::Catalog::default());
    let r = first_row(&s, &g, a);
    s.open_popup(PopupKind::Insert(a), r);
    s.popup
        .as_mut()
        .unwrap()
        .pick
        .as_mut()
        .unwrap()
        .groups
        .last_mut()
        .unwrap()
        .rows
        .extend((0..30).map(|i| {
            crate::pick_list::PickRow::new(format!("Other {i}"))
                .act(Action::Pick("builtin:eq".into()))
        }));
    s.fx_typed("e", Some("EQ"));
    assert_eq!(s.popup.as_ref().unwrap().max_offset(g.size), 0);
    let result = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .into_iter()
        .filter(|c| c.action.is_some() && !c.foot)
        .count();
    assert_eq!(result, 1);
    s.fx_typed("enter", None);
    assert!(
        s.project().tracks[&a]
            .inserts
            .iter()
            .any(|p| p.name == "EQ")
    );
}

#[test]
fn hidden_mixer_cannot_accept_a_drop_and_unrelated_input_keeps_enter() {
    let (mut s, g, a, _) = setup();
    let r = first_row(&s, &g, a);
    let pos = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    s.tab = DrawerTab::Roll;
    assert_eq!(s.fx_target(&g, pos.0, pos.1), None);
    s.browser.tab = crate::browser::RailTab::Effects;
    s.browser.open = true;
    s.browser.selected_item = Some("EQ");
    s.mixer.browser_fx_focus = true;
    s.open_popup(PopupKind::Versions, Rect::new(0.0, 0.0, 10.0, 10.0));
    assert!(!s.fx_typed("enter", None));
    s.popup = None;
    s.browser.typing = true;
    assert!(!s.fx_typed("enter", None));
    assert!(s.project().tracks[&a].inserts.is_empty());
    // Neither a pop-up nor typing: Enter puts the focused effect on the
    // chosen track.
    s.browser.typing = false;
    s.selected_row = Some(crate::tree::RowKind::Track(a));
    assert!(s.fx_typed("enter", None));
    assert!(
        s.project().tracks[&a]
            .inserts
            .iter()
            .any(|p| p.name == "EQ")
    );
}
#[test]
fn expanded_header_uses_the_chain_and_accepts_a_drop() {
    let (mut s, g, a, _) = setup();
    s.press(&g, Hit::Widen(a));
    s.tab = DrawerTab::Roll;
    let g = s.geometry(1600.0, 1000.0);
    let r = s.fx_header_rect(&g, a).unwrap();
    s.scroll.1 += (r.bottom() - g.column.bottom() + 12.0).max(0.0);
    let r = s.fx_header_rect(&g, a).unwrap();
    s.mixer.fx_drag = Some(Drag {
        source: Source::Effect(Pick::built_in("EQ")),
        from: (0.0, 0.0),
        at: (r.x, r.y),
        moved: true,
        click: None,
    });
    s.fx_up(&g, r.x + 20.0, r.y + 10.0, false);
    close(&mut s);
    s.mixer.set_insert_offset(a, 0);
    let got = s.fx_header_hit(&g, r.x + 24.0, r.y + 10.0);
    assert!(
        matches!(got,Some(MixerHit::Open(t,_)) if t==a),
        "{r:?} column={:?} got={got:?}",
        g.column
    );
}
#[test]
fn preset_file_buttons_use_the_target_plugin() {
    let (mut s, g, a, _) = setup();
    s.add_effect(a, 0, &Pick::built_in("EQ"));
    close(&mut s);
    s.mixer.set_insert_offset(a, 0);
    let id = s.project().tracks[&a].inserts[0].id;
    let r = crate::fx_preset::part(list_row(first_row(&s, &g, a), 0));
    assert!(s.chain_down(&g, r.x + 8.0, r.y + 11.0));
    let panel = s.mixer.preset_panel.unwrap();
    assert!(s.chain_down(&g, panel.rect.x + 20.0, panel.rect.y + 38.0));
    assert_eq!(
        s.file_request,
        Some(crate::studio::FileRequest::PresetSave(id))
    );
}
#[test]
fn automation_mark_ignores_empty_lanes_but_keeps_hidden_points() {
    let (mut s, _, a, _) = setup();
    s.run_command(
        "auto.lane.add",
        json!({"track":a.to_string(),"target":"volume"}),
    );
    assert!(!crate::auto_mark::present(
        &s.project().tracks[&a],
        &AutoTarget::Volume
    ));
    s.run_command(
        "auto.point.add",
        json!({"track":a.to_string(),"target":"volume","at":0,"value":-6}),
    );
    s.run_command(
        "auto.lane.hide",
        json!({"track":a.to_string(),"target":"volume"}),
    );
    assert!(crate::auto_mark::present(
        &s.project().tracks[&a],
        &AutoTarget::Volume
    ));
    assert!(!crate::auto_mark::present(
        &s.project().tracks[&a],
        &AutoTarget::Pan
    ));
}

#[test]
fn an_insert_name_keeps_the_room_it_has_when_the_pointer_is_on_the_row() {
    // The remove mark is 16 wide and shows under the pointer only. The name is written to the end
    // of the row; the mark lies over its tail, and the preset mark does not show over a name that
    // needs its room ("Compressor" is not cut to "Compr…").
    let row = Rect::new(10.0, 20.0, 104.0, crate::mixer::ROW_H);
    let name = name_rect(row);
    assert_eq!(name.right() + 2.0, row.right());
    assert!(name.right() > remove_part(row).x);
    assert!(crate::fx_chain::preset_mark_fits(row, "EQ"));
    assert!(!crate::fx_chain::preset_mark_fits(row, "Compressor"));
    assert_eq!(name.x, bypass_part(row).right());
    assert!(name.w >= crate::browser::text_width("Compressor", xirius_design::FontSize::Caption));
}

#[test]
fn ui_297_the_effects_list_shows_the_same_thumb_when_more_rows_than_show() {
    let (mut s, g, a, _) = setup();
    s.use_plugins(xirius_plugins::Catalog::default());
    let r = first_row(&s, &g, a);
    s.open_popup(PopupKind::Insert(a), r);
    s.popup
        .as_mut()
        .unwrap()
        .pick
        .as_mut()
        .unwrap()
        .groups
        .last_mut()
        .unwrap()
        .rows
        .extend((0..300).map(|i| {
            crate::pick_list::PickRow::new(format!("Other {i}"))
                .act(Action::Pick("builtin:eq".into()))
        }));
    let thumb = |s: &Studio| {
        s.popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .into_iter()
            .find(|c| c.look == crate::popup::Look::Thumb)
    };
    let p = s.popup.as_ref().unwrap();
    let n = crate::pick_list::found(p).count();
    // The rows' part keeps its height at any scroll (UI-338).
    let pick = p.pick.as_ref().unwrap();
    let (h, shown) = (pick.body_h(g.size), pick.capacity(g.size) as f32);
    let at_top = thumb(&s).expect("a thumb").rect;
    assert_eq!(at_top.w, 4.0);
    assert_eq!(at_top.h, (h * shown / n as f32).max(16.0));
    assert!(
        thumb(&s).unwrap().action.is_none(),
        "it only shows where the list is"
    );
    s.fx_typed("e", Some("EQ"));
    assert!(thumb(&s).is_none());
}
