//! The right press on the instrument row (CTX-040).

use xirius_core::auto::AutoTarget;

use super::*;
use crate::mixer::MixerHit;
use crate::popup::{Action, PopupKind};
use crate::studio::{DrawerTab, Geometry};
use crate::tools::Mods;
use crate::tree::RowKind;
use xirius_core::id::PluginId;

/// The sample song with the mixer open and Keys playing the built-in sampler.
fn studio() -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    // the strips' flow shows the instrument and an effect under it
    s.drawer_size.mixer = Some(600.0);
    let g = s.geometry(1600.0, 1000.0);
    let t = keys(&s);
    assert!(s.set_instrument(t, &Pick::built_in(DEFAULT)));
    let id = current(&s.project().tracks[&t]).unwrap().id;
    (s, g, t, id)
}

fn keys(s: &Studio) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .map(|t| t.id)
        .unwrap()
}

/// A right press on the instrument row.
fn right_press(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId) {
    let (x, y) = s.mixer_center(g, MixerHit::Open(t, id)).expect("on screen");
    assert!(s.context_press(g, x, y));
}

fn pick(s: &mut Studio, g: &Geometry, to: &str) {
    let label = match to {
        "open" => "画面を開く",
        "preset" => "プリセット",
        "replace" => "差し替える…",
        "reload" => "再起動",
        "auto" => "オートメーションを表示",
        "remove" => "プラグインを削除",
        "process" => "動かす場所",
        "Default" => "Default",
        _ => "",
    };
    let label = if label.is_empty() {
        crate::context::tests::all_items(s)
            .into_iter()
            .find_map(|item| {
                matches!(&item.run, crate::context::Run::Legacy(_, _, action)
                if **action == Action::Pick(to.into()))
                .then(|| item.label.clone())
            })
            .expect("legacy operation retained")
    } else {
        label.to_owned()
    };
    assert!(crate::context::tests::choose(s, g, &label));
}

#[test]
fn the_instrument_row_has_its_own_menu_with_keys_and_groups() {
    let (mut s, g, t, id) = studio();
    right_press(&mut s, &g, t, id);
    assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Context);
    let rows = crate::context::tests::all_rows(&s);
    for (label, key, enabled) in [
        ("画面を開く", "Shift+F11", true),
        ("プリセット", "", true),
        ("差し替える…", "", true),
        ("再起動", "", true),
        ("オートメーションを表示", "A", true),
        ("コピー", "Ctrl+C", false),
        ("プラグインを削除", "Del", true),
    ] {
        let row = rows.iter().find(|r| r.label == label).expect(label);
        assert_eq!((&*row.key, row.enabled), (key, enabled), "{label}");
    }
    // An effect keeps the plugin menu.
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "EQ" })
    ));
    let eq = s.project().tracks[&t].inserts[1].id;
    s.popup = None;
    right_press(&mut s, &g, t, eq);
    assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Context);
}

#[test]
fn open_presets_and_replace_go_where_the_row_says() {
    let (mut s, g, t, id) = studio();
    right_press(&mut s, &g, t, id);
    pick(&mut s, &g, "open");
    assert!(s.instruments.is_open(id));
    // Opening it again keeps it open.
    right_press(&mut s, &g, t, id);
    pick(&mut s, &g, "open");
    assert!(s.instruments.is_open(id));
    // プリセット opens the list in the same place, the current one on.
    right_press(&mut s, &g, t, id);
    let anchor = s.popup.as_ref().unwrap().anchor;
    pick(&mut s, &g, "preset");
    let popup = s.popup.as_ref().expect("the presets");
    assert_eq!(popup.kind, PopupKind::Context);
    assert_eq!(popup.anchor, anchor);
    // The Sampler has only its Default (PLG-064), then the saves under it.
    assert!(
        s.context_rows()
            .iter()
            .any(|r| r.label == "Default" && r.on)
    );
    let labels: Vec<String> = s
        .context_rows()
        .into_iter()
        .filter(|r| !r.gap && r.label != "戻る")
        .map(|r| r.label)
        .collect();
    assert_eq!(
        labels,
        [
            "Default",
            "上書き保存",
            "名前を付けて保存…",
            "ブラウザーに表示"
        ]
    );
    pick(&mut s, &g, "Default");
    assert_eq!(window::preset_of(&plugin(&s, t)), Some("Default"));
    // 差し替える… is the instrument picker.
    right_press(&mut s, &g, t, id);
    pick(&mut s, &g, "replace");
    assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Instrument(t));
    // 再起動 starts the plugins again.
    s.popup = None;
    let before = s.health.restarts;
    right_press(&mut s, &g, t, id);
    pick(&mut s, &g, "reload");
    assert_eq!(s.health.restarts, before + 1);
    assert!(s.popup.is_none());
}

fn plugin(s: &Studio, t: TrackId) -> Plugin {
    current(&s.project().tracks[&t]).cloned().unwrap()
}

#[test]
fn automation_shows_a_lane_of_the_picked_parameter_in_one_step() {
    let (mut s, g, t, id) = studio();
    right_press(&mut s, &g, t, id);
    pick(&mut s, &g, "auto");
    let popup = s.popup.as_ref().expect("the parameters");
    assert_eq!(popup.kind, PopupKind::Context);
    let cs: Vec<_> = s
        .context_rows()
        .into_iter()
        .filter(|r| !r.gap && r.label != "戻る")
        .collect();
    let names: Vec<&str> = cs.iter().map(|c| c.label.as_str()).collect();
    let def = plugin(&s, t).builtin().unwrap();
    let want: Vec<&str> = def.params.iter().map(|p| p.label).collect();
    assert_eq!(names, want);
    let before = s.song().steps();
    pick(&mut s, &g, "cutoff");
    let target = AutoTarget::Param(id, "cutoff".into());
    let lane = |s: &Studio| {
        s.project().tracks[&t]
            .automation
            .iter()
            .find(|l| l.target == target)
            .map(|l| l.hidden)
    };
    assert_eq!(lane(&s), Some(false));
    assert!(s.arrange.open.contains(&t), "the lanes show");
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(s.error, None);
    // Shown already: nothing more to undo.
    assert!(s.show_param_lane(t, id, "cutoff"));
    assert_eq!(s.song().steps(), before + 1);
    // A parameter the instrument does not have is not added.
    assert!(!s.show_param_lane(t, id, "Volume"));
    assert!(s.undo(&g));
    assert_eq!(lane(&s), None);
}

#[test]
fn remove_leaves_the_row_to_pick_an_instrument() {
    let (mut s, g, t, id) = studio();
    right_press(&mut s, &g, t, id);
    pick(&mut s, &g, "remove");
    let track = &s.project().tracks[&t];
    assert!(current(track).is_none());
    assert_eq!(gap(track), 1);
    assert!(s.mixer_center(&g, MixerHit::AddInstrument(t)).is_some());
    assert!(s.undo(&g));
    assert_eq!(plugin(&s, t).id, id);
}

#[test]
fn another_makers_instrument_has_no_presets_here() {
    let (mut s, g, t, _) = studio();
    let pad = Pick {
        name: "Pad Synth".into(),
        format: "vst3".into(),
        uid: "FEDCBA9876543210FEDCBA9876543210".into(),
    };
    assert!(s.set_instrument(t, &pad));
    let id = plugin(&s, t).id;
    right_press(&mut s, &g, t, id);
    let cells = crate::context::tests::all_rows(&s);
    let preset = cells.iter().find(|c| c.label == "プリセット").unwrap();
    assert!(!preset.enabled);
    // It keeps nothing yet: no parameter to follow.
    pick(&mut s, &g, "auto");
    let cells = crate::context::tests::all_rows(&s);
    let empty = cells
        .iter()
        .find(|r| r.label == "パラメーターがありません")
        .unwrap();
    assert!(!empty.enabled);
}

#[test]
fn shift_f11_opens_the_selected_tracks_instrument() {
    let (mut s, g, t, id) = studio();
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    s.selected_row = None;
    assert!(!s.instrument_key("f11", shift), "no track selected");
    assert!(!s.instruments.is_open(id));
    s.selected_row = Some(RowKind::Track(t));
    assert!(!s.instrument_key("f11", Mods::default()));
    assert!(s.edit_key(&g, "f11", shift));
    assert!(s.instruments.is_open(id));
    assert!(s.edit_key(&g, "f11", shift));
    assert!(!s.instruments.is_open(id), "again shuts it");
}

#[test]
fn an_external_instrument_opens_the_effective_process_choices_from_its_row() {
    let (mut s, g, t, _) = studio();
    let pick = Pick {
        name: "Fixture".into(),
        format: "vst3".into(),
        uid: "00112233445566778899AABBCCDDEEFF".into(),
    };
    assert!(s.set_instrument(t, &pick));
    let id = current(&s.project().tracks[&t]).unwrap().id;
    let key = xirius_prefs::isolation::key(&pick.format, &pick.uid);
    s.song
        .set("prefs.set", json!({"key":key,"value":"dedicated"}))
        .unwrap();
    right_press(&mut s, &g, t, id);
    assert!(s.context_path("動かす場所").is_some());
    assert!(s.context_choose(&g, "機器"));
    let marks = crate::a11y::marks(&s, &g);
    assert!(marks.iter().any(|m| m.node.label() == Some("動かす場所")));
    self::pick(&mut s, &g, "process");
    assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Context);
    assert!(s.context_rows().iter().any(|r| r.label == "専用" && r.on));
}
