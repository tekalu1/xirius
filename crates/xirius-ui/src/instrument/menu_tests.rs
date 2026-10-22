//! The right press on the instrument row (CTX-040).

use xirius_core::auto::AutoTarget;

use super::menu::Menu;
use super::*;
use crate::mixer::MixerHit;
use crate::popup::{Action, Cell, PopupKind};
use crate::studio::{DrawerTab, Geometry};
use crate::tools::Mods;
use crate::tree::RowKind;
use xirius_core::id::PluginId;

/// The sample song with the mixer open and Keys playing the built-in sampler.
fn studio() -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
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

/// The rows of the open menu as they are drawn.
fn rows(s: &Studio, g: &Geometry) -> Vec<Cell> {
    s.popup.as_ref().expect("a menu").layout(g.size).cells
}

fn pick(s: &mut Studio, g: &Geometry, to: &str) {
    s.popup_action(g, Action::Pick(to.into()));
}

#[test]
fn the_instrument_row_has_its_own_menu_with_keys_and_groups() {
    let (mut s, g, t, id) = studio();
    right_press(&mut s, &g, t, id);
    let popup = s.popup.as_ref().unwrap();
    assert_eq!(popup.kind, PopupKind::InstMenu(Menu::Row(t, id)));
    let cells = rows(&s, &g);
    let shown: Vec<(&str, &str, bool)> = cells
        .iter()
        .map(|c| (c.text.as_str(), c.hint.as_str(), c.enabled))
        .collect();
    assert_eq!(
        shown,
        [
            ("画面を開く", "Shift+F11", true),
            ("プリセット", "", true),
            ("差し替える…", "", true),
            ("再起動", "", true),
            ("オートメーションを表示", "A", true),
            ("コピー", "Ctrl+C", false),
            ("プラグインを削除", "Del", true),
        ]
    );
    // Space, not lines, between the four groups.
    let gaps: Vec<f32> = cells
        .windows(2)
        .map(|w| w[1].rect.y - w[0].rect.bottom())
        .collect();
    assert_eq!(gaps.iter().filter(|d| **d > 0.5).count(), 3);
    // An effect keeps the plugin menu.
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "EQ" })
    ));
    let eq = s.project().tracks[&t].inserts[1].id;
    s.popup = None;
    right_press(&mut s, &g, t, eq);
    assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Plugin(t, eq));
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
    assert_eq!(popup.kind, PopupKind::InstMenu(Menu::Preset(t, id)));
    assert_eq!(popup.anchor, anchor);
    assert_eq!(popup.current, "初期");
    pick(&mut s, &g, "強め");
    assert_eq!(window::preset_of(&plugin(&s, t)), "強め");
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
    assert_eq!(popup.kind, PopupKind::InstMenu(Menu::Params(t, id)));
    let names: Vec<&str> = popup.choices.iter().map(|c| c.label.as_str()).collect();
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
    let cells = rows(&s, &g);
    let preset = cells.iter().find(|c| c.text == "プリセット").unwrap();
    assert!(!preset.enabled);
    // It keeps nothing yet: no parameter to follow.
    pick(&mut s, &g, "auto");
    let cells = rows(&s, &g);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].text, "パラメーターがありません");
    assert!(!cells[0].enabled);
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
