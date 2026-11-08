//! MIX-040: giving a macro knob its targets from the parts' menus, the knob's own menu, the
//! list of targets and 学習, and a controller's sweep of a macro made one step.

use std::time::Instant;

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::controls::ControlId;
use xirius_core::id::{PluginId, TrackId};

use super::assign::{ADD, LEARN, REMOVE};
use crate::context::mixer::MACRO_ASSIGN;
use crate::instrument::window::Part;
use crate::mixer::MixerHit;
use crate::popup::{Action, PopupKind};
use crate::studio::{Geometry, Rect, Studio};

/// The sample song with a Delay on Keys, its window open.
fn delay() -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    s.instruments.open.clear();
    let g = s.geometry(1600.0, 1000.0);
    let t = keys(&s);
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Delay" }),
    ));
    let id = s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "Delay")
        .unwrap()
        .id;
    s.show_instrument(t, id);
    (s, g, t, id)
}

fn keys(s: &Studio) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn labels(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| !r.gap)
        .map(|r| r.label)
        .collect()
}

fn row_enabled(s: &Studio, label: &str) -> bool {
    crate::context::tests::all_rows(s)
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", labels(s)))
        .enabled
}

/// Chooses the row that starts with `label`.
fn pick(s: &mut Studio, g: &Geometry, label: &str) {
    let name = crate::context::tests::all_labels(s)
        .into_iter()
        .find(|name| name.starts_with(label))
        .expect("operation");
    assert!(crate::context::tests::choose(s, g, &name));
}

fn maps(s: &Studio, t: TrackId, k: u8) -> Vec<AutoTarget> {
    s.project().tracks[&t]
        .controls
        .get(ControlId::Knob(k))
        .maps
        .iter()
        .map(|m| m.target.clone())
        .collect()
}

/// Opens knob `k`'s menu in the macro window.
fn knob_menu(s: &mut Studio, g: &Geometry, t: TrackId, k: u8) {
    s.open_macros(t);
    let (x, y) = mid(s.macro_view(g).unwrap().knobs[usize::from(k)]);
    assert!(s.macro_right(g, x, y));
    assert!(s.context_open());
}

#[test]
fn a_device_knob_gives_itself_to_a_macro_in_one_step() {
    let (mut s, g, t, id) = delay();
    let (x, y) = s.instrument_center(&g, id, Part::Knob(0)).unwrap();
    assert!(s.instrument_right(&g, x, y));
    let all = crate::context::tests::all_labels(&s);
    let at = all.iter().position(|l| l == MACRO_ASSIGN).expect("the row");
    assert_eq!(all[at - 1], "オートメーションを表示");
    let n = s.song().steps();
    pick(&mut s, &g, MACRO_ASSIGN);
    assert_eq!(labels(&s)[1..3], ["Macro 1", "Macro 2"]);
    pick(&mut s, &g, "Macro 3");
    assert_eq!(s.song().steps(), n + 1);
    let [AutoTarget::Param(p, _)] = maps(&s, t, 2)[..] else {
        panic!("{:?}", maps(&s, t, 2));
    };
    assert_eq!(p, id);
    // the knob's owner is on, the rest wait: one target, one macro
    assert!(s.instrument_right(&g, x, y));
    pick(&mut s, &g, MACRO_ASSIGN);
    // (an unnamed knob takes its target's name)
    let rows: Vec<_> = s
        .context_rows()
        .into_iter()
        .filter(|r| !r.gap && r.label != "戻る")
        .collect();
    assert_eq!(rows.len(), 8);
    assert!(rows[2].on && rows[2].enabled, "{:?}", rows[2].label);
    assert!(!rows[0].enabled && !rows[7].enabled);
    let own = rows[2].label.clone();
    pick(&mut s, &g, &own);
    assert!(maps(&s, t, 2).is_empty(), "its owner's row takes it back");
}

#[test]
fn the_knob_menu_has_add_learn_remove_and_its_lane() {
    let (mut s, g, t, _) = delay();
    knob_menu(&mut s, &g, t, 7);
    let all = crate::context::tests::all_labels(&s);
    assert_eq!(all[..4], [ADD, LEARN, REMOVE, "名前を変更"]);
    assert!(all.contains(&"オートメーションを表示".to_string()));
    assert!(all.contains(&"MIDI 学習".to_string()));
    assert!(!row_enabled(&s, LEARN), "nothing was touched yet");
    assert!(!row_enabled(&s, REMOVE), "Macro 8 has nothing");
    // the channel editor's row opens the same menu, and its list hangs under the row's knob
    s.macro_pad = None;
    s.popup = None;
    s.open_channel(t);
    let v = s.channel_view(&g).unwrap();
    let tool = v
        .tools
        .iter()
        .find(|(x, _)| *x == crate::channel::Tool::Macros)
        .unwrap()
        .1;
    let (x, y) = mid(tool);
    assert!(s.channel_press(&g, x, y, 1));
    s.channel_up(&g);
    let cell = s.channel_view(&g).unwrap().macros.unwrap().cells[7];
    let (x, y) = mid(cell);
    assert!(s.channel_right(&g, x, y));
    assert_eq!(labels(&s)[..3], [ADD, LEARN, REMOVE]);
    pick(&mut s, &g, ADD);
    assert_eq!(
        s.popup.as_ref().map(|p| p.kind),
        Some(PopupKind::MacroTargets(t, 7))
    );
}

#[test]
fn the_list_gives_and_takes_back_and_shows_the_other_knobs_targets() {
    let (mut s, g, t, _) = delay();
    knob_menu(&mut s, &g, t, 7);
    pick(&mut s, &g, ADD);
    let popup = s.popup.as_ref().expect("the list");
    assert_eq!(popup.kind, PopupKind::MacroTargets(t, 7));
    assert!(crate::pick_list::is(popup.kind), "a PickList (UI-320)");
    let volume = AutoTarget::Volume.to_string();
    let want = Action::MacroTarget(t, 7, volume.clone());
    let rows: Vec<_> = crate::pick_list::found(popup).cloned().collect();
    let row = rows
        .iter()
        .find(|r| r.act.as_ref() == Some(&want))
        .expect("the volume");
    assert!(!row.on);
    assert_eq!(crate::operations::of_action(&want), "control.assign");
    let n = s.song().steps();
    assert!(s.popup_action(&g, want.clone()));
    assert!(s.popup.is_none());
    assert_eq!(maps(&s, t, 7), [AutoTarget::Volume]);
    assert_eq!(s.song().steps(), n + 1);
    // open again, its own is on, and choosing it lets it go
    assert!(s.pick_macro(&g, t, 7));
    let rows: Vec<_> = crate::pick_list::found(s.popup.as_ref().unwrap())
        .cloned()
        .collect();
    assert!(rows.iter().any(|r| r.on && r.act.as_ref() == Some(&want)));
    assert!(s.popup_action(&g, want));
    assert!(maps(&s, t, 7).is_empty());
    // another knob's target waits, with that knob's name
    assert!(s.run_command(
        "control.assign",
        json!({ "track": t.to_string(), "control": "k1", "target": volume }),
    ));
    assert!(s.pick_macro(&g, t, 7));
    let rows: Vec<_> = crate::pick_list::found(s.popup.as_ref().unwrap())
        .cloned()
        .collect();
    let taken = rows.iter().find(|r| r.label == "ボリューム").unwrap();
    let k1 = s.project().tracks[&t].control_label(ControlId::Knob(0));
    assert_eq!(taken.hint, k1, "the owner's name");
    assert!(!taken.enabled && taken.act.is_none());
}

#[test]
fn learn_takes_the_parameter_the_device_moved_last() {
    let (mut s, g, t, id) = delay();
    assert!(s.run_command(
        "plugin.param.set",
        json!({ "id": id.to_string(), "param": "mix", "value": 0.3 }),
    ));
    knob_menu(&mut s, &g, t, 4);
    let learn = labels(&s)
        .into_iter()
        .find(|l| l.starts_with(LEARN))
        .unwrap();
    assert_ne!(learn, LEARN, "it names what it takes: {learn}");
    let n = s.song().steps();
    pick(&mut s, &g, LEARN);
    assert_eq!(s.song().steps(), n + 1);
    assert_eq!(maps(&s, t, 4), [AutoTarget::Param(id, "mix".into())]);
    // taken by Macro 5, it is not offered to another knob
    knob_menu(&mut s, &g, t, 5);
    assert!(!row_enabled(&s, LEARN));
    // and Macro 5's menu takes it back
    knob_menu(&mut s, &g, t, 4);
    assert!(row_enabled(&s, REMOVE));
    pick(&mut s, &g, REMOVE);
    pick(&mut s, &g, "Delay");
    assert!(maps(&s, t, 4).is_empty());
}

#[test]
fn a_controller_sweep_of_a_macro_is_one_step() {
    let (mut s, _, t, _) = delay();
    let hit = MixerHit::Control(t, ControlId::Knob(3));
    let n = s.song().steps();
    for cc in [10, 40, 80, 127] {
        assert!(s.knob_move(hit, cc));
    }
    assert_eq!(s.song().steps(), n, "nothing is written while it moves");
    s.mixer.knob_at = Some(Instant::now() - crate::mixer::knob::QUIET);
    assert!(s.knob_quiet());
    assert_eq!(s.song().steps(), n + 1);
    let v = s.project().tracks[&t]
        .controls
        .get(ControlId::Knob(3))
        .value;
    assert!((v - 1.0).abs() < 1e-9, "{v}");
}
