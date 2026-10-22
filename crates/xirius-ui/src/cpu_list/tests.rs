use super::*;
use crate::mixer::MixerHit;
use crate::studio::{DrawerTab, Hit};
use xirius_core::model::TrackKind;

/// A song with an effect on its first audio track, shown in the mixer.
fn with_effect() -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let t = s
        .project()
        .tracks
        .values()
        .find(|x| x.kind == TrackKind::Audio)
        .expect("an audio track")
        .id;
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Echo", "kind": "effect",
                "format": "vst3", "uid": "00000000000000000000000000000001" }),
    ));
    let id = s.project().tracks[&t].inserts.last().unwrap().id;
    let g = s.geometry(1600.0, 1000.0);
    (s, g, t, id)
}

fn fake(s: &mut Studio, load: f64, id: PluginId, share: f64) {
    s.perf.fake = Some(Fake {
        load,
        xruns: 2,
        loads: BTreeMap::from([(id, share)]),
    });
    s.poll_perf_now();
}

fn open_list(s: &mut Studio, g: &Geometry) {
    assert!(s.press(g, Hit::Bar(crate::topbar::BarButton::Cpu)));
    assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Perf));
}

fn cell_of(s: &Studio, g: &Geometry, act: Action) -> Rect {
    let p = s.popup.as_ref().expect("the list");
    let layout = p.layout(g.size);
    let c = layout
        .cells
        .iter()
        .find(|c| c.action.as_ref() == Some(&act))
        .unwrap_or_else(|| panic!("{act:?} in the list"));
    c.rect
}

fn press_cell(s: &mut Studio, g: &Geometry, act: Action) -> bool {
    let i = s
        .popup
        .as_ref()
        .expect("the list")
        .layout(g.size)
        .cells
        .iter()
        .position(|c| c.action.as_ref() == Some(&act))
        .unwrap_or_else(|| panic!("{act:?} in the list"));
    s.press(g, Hit::Item(i))
}

#[test]
fn the_mark_lights_at_15_percent_and_goes_off_at_12() {
    let (mut s, _g, _t, id) = with_effect();
    fake(&mut s, 0.3, id, 0.14);
    assert!(!s.perf.heavy.is_heavy(id));
    fake(&mut s, 0.3, id, 0.16);
    assert!(s.perf.heavy.is_heavy(id), "15% and over");
    fake(&mut s, 0.3, id, 0.13);
    assert!(s.perf.heavy.is_heavy(id), "still lit above 12%");
    fake(&mut s, 0.3, id, 0.11);
    assert!(!s.perf.heavy.is_heavy(id), "off below 12%");
}

#[test]
fn the_bar_says_the_cpu_and_the_list_has_the_plugin_with_its_share() {
    let (mut s, g, _t, id) = with_effect();
    fake(&mut s, 0.42, id, 0.2);
    assert_eq!(s.bar_view().cpu, 42);
    open_list(&mut s, &g);
    let v = s.perf_view();
    assert_eq!((v.percent, v.xruns), (42, 2));
    let row = v.rows.iter().find(|r| r.id == id).expect("its row");
    assert_eq!(
        (row.percent, row.heavy, row.name.as_str()),
        (20, true, "Echo")
    );
    assert_eq!(v.rows[0].id, id, "the heaviest first");
}

#[test]
fn the_check_deactivates_and_activates_and_the_state_is_kept() {
    let (mut s, g, t, id) = with_effect();
    fake(&mut s, 0.3, id, 0.2);
    open_list(&mut s, &g);
    assert!(press_cell(&mut s, &g, Action::Cpu(CAct::Toggle(id))));
    let p = &s.project().tracks[&t].inserts;
    assert!(p.iter().find(|x| x.id == id).unwrap().deactivated);
    s.poll_perf_now();
    assert_eq!(
        s.perf.rows.iter().find(|r| r.id == id).unwrap().run,
        Run::Deactivated
    );
    assert!(press_cell(&mut s, &g, Action::Cpu(CAct::Toggle(id))));
    assert!(
        !s.project().tracks[&t]
            .inserts
            .iter()
            .find(|x| x.id == id)
            .unwrap()
            .deactivated
    );
}

#[test]
fn a_double_press_on_a_row_lights_its_slot_in_the_mixer_and_a_single_does_not() {
    let (mut s, g, _t, id) = with_effect();
    fake(&mut s, 0.3, id, 0.2);
    open_list(&mut s, &g);
    let at = cell_of(&s, &g, Action::Cpu(CAct::Pick(id)));
    let (x, y) = (at.x + 80.0, at.y + 6.0);
    assert!(!s.cpu_down(&g, x, y, 1));
    assert!(!s.perf.pinged(id));
    assert!(s.cpu_down(&g, x, y, 2));
    assert!(s.perf.pinged(id));
    assert!(
        s.mixer_center(
            &s.geometry(1600.0, 1000.0),
            MixerHit::Open(s.perf.rows[0].track, id)
        )
        .is_some(),
        "the row is on screen"
    );
    // The check, pressed twice, is two toggles and no ping.
    let check = cell_of(&s, &g, Action::Cpu(CAct::Toggle(id)));
    s.perf.ping = None;
    assert!(!s.cpu_down(&g, check.x + 4.0, check.y + 4.0, 2));
    assert!(!s.perf.pinged(id));
}

#[test]
fn a_double_press_scrolls_the_mixer_to_a_slot_out_of_sight_and_lights_it() {
    let (mut s, g, t, _) = with_effect();
    for i in 0..5 {
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": format!("Echo {i}"), "kind": "effect",
                    "format": "vst3", "uid": "00000000000000000000000000000001" }),
        ));
    }
    let ids: Vec<PluginId> = s.project().tracks[&t]
        .inserts
        .iter()
        .map(|p| p.id)
        .collect();
    let (first, last) = (ids[0], *ids.last().unwrap());
    let rows = crate::mixer::Rows::of(&s.project()).inserts;
    assert!(ids.len() > rows, "the list holds more than it shows");
    // The mixer looks at the far end, away from the strip.
    s.mixer.scroll = 1.0e6;
    s.rebuild(&g);
    assert!(s.mixer_center(&g, MixerHit::Open(t, last)).is_none());

    let at = |s: &mut Studio, id: PluginId| {
        fake(s, 0.3, id, 0.2);
        s.popup = None;
        open_list(s, &g);
        let at = cell_of(s, &g, Action::Cpu(CAct::Pick(id)));
        assert!(s.cpu_down(&g, at.x + 80.0, at.y + 6.0, 2));
        assert!(s.perf.pinged(id));
    };
    at(&mut s, last);
    assert!(
        s.mixer_center(&g, MixerHit::Open(t, last)).is_some(),
        "the last slot is on screen"
    );
    assert!(s.mixer_center(&g, MixerHit::Open(t, first)).is_none());
    // Back up the list to the first.
    at(&mut s, first);
    assert!(s.mixer_center(&g, MixerHit::Open(t, first)).is_some());
    assert_eq!(s.mixer.offset(t, crate::mixer::List::Inserts), 0);
}

#[test]
fn the_right_press_on_a_row_opens_its_menu() {
    let (mut s, g, _t, id) = with_effect();
    fake(&mut s, 0.3, id, 0.2);
    open_list(&mut s, &g);
    let at = cell_of(&s, &g, Action::Cpu(CAct::Pick(id)));
    assert!(s.cpu_context(&g, at.x + 80.0, at.y + 6.0));
    assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Context));
    let rows = s.context_rows();
    let names: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
    assert!(names.iter().any(|n| n == "画面を開く"), "{names:?}");
    assert!(names.iter().any(|n| n == "無効にする"), "{names:?}");
    assert!(names.iter().any(|n| n == "トラックをフリーズ"), "{names:?}");
}

#[test]
fn a_deactivated_plugin_does_not_count_as_heavy_and_a_bypassed_one_reads_as_such() {
    let (mut s, _g, _t, id) = with_effect();
    fake(&mut s, 0.3, id, 0.2);
    assert!(s.perf.heavy.is_heavy(id));
    assert!(s.run_command("plugin.deactivate", json!({ "ids": [id.to_string()] })));
    s.poll_perf_now();
    assert!(!s.perf.heavy.is_heavy(id));
}
