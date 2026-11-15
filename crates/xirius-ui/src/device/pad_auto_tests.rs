//! A Drum Kit pad's automation (UI-358): the lane is named for the pad and its sound, its
//! values read in the knob's units, and the target list holds the pads that have a sound,
//! one group each, searchable by the pad's name.

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::id::{PluginId, TrackId};
use xirius_params::pad_key;

use super::pad_select_tests::{body, centre, fill, kit, plugin, window};
use crate::arrange::{lane_name, value_text};
use crate::popup::PopupKind;
use crate::studio::{Rect, Studio};

/// The voice parameters' names, as the pad's face reads them.
const NAMES: [(&str, &str); 13] = [
    ("attack", "アタック"),
    ("decay", "ディケイ"),
    ("sustain", "サステイン"),
    ("release", "リリース"),
    ("ftype", "フィルターの型"),
    ("cutoff", "カットオフ"),
    ("resonance", "レゾナンス"),
    ("drive", "ドライブ"),
    ("transpose", "トランスポーズ"),
    ("fine", "微調整"),
    ("volume", "ボリューム"),
    ("pan", "パン"),
    ("velocity", "ベロシティの効き"),
];

fn target(id: PluginId, pad: usize, name: &str) -> AutoTarget {
    AutoTarget::Param(id, pad_key(pad, name))
}

fn name_of(s: &Studio, t: TrackId, target: &AutoTarget) -> String {
    let p = s.project();
    lane_name(&p, &p.tracks[&t], target)
}

#[test]
fn a_pads_lane_is_named_for_the_pad_its_sound_and_the_knob() {
    let (mut s, _, t, id) = window("Drum Kit");
    fill(&mut s, id, 0);
    assert_eq!(
        name_of(&s, t, &target(id, 0, "volume")),
        "A1 Kick · ボリューム"
    );
    for (key, word) in NAMES {
        assert_eq!(
            name_of(&s, t, &target(id, 0, key)),
            format!("A1 Kick · {word}")
        );
    }
    // A pad with no sound has only its name.
    assert_eq!(name_of(&s, t, &target(id, 4, "volume")), "A5 · ボリューム");
}

#[test]
fn a_lane_of_the_old_key_loads_as_it_was_and_reads_the_same() {
    let (mut s, _, t, id) = window("Drum Kit");
    fill(&mut s, id, 0);
    // The key the song kept before: no new one, no change to the song.
    let old: AutoTarget = format!("param:{id}:A1.volume").parse().unwrap();
    assert_eq!(old, target(id, 0, "volume"));
    let args = json!({ "track": t.to_string(), "target": old.to_string() });
    assert!(s.run_command("auto.lane.add", args), "{:?}", s.error);
    let lanes = &s.project().tracks[&t].automation;
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].target.to_string(), format!("param:{id}:A1.volume"));
    assert_eq!(name_of(&s, t, &old), "A1 Kick · ボリューム");
}

#[test]
fn a_pads_values_read_in_the_knobs_units() {
    let (s, _, t, id) = window("Drum Kit");
    let _ = (&s, t);
    let spec = |name: &str| {
        xirius_params::DRUM_KIT
            .resolve(&pad_key(0, name))
            .unwrap()
            .spec
    };
    let at = |name: &str, plain: f64| (target(id, 0, name), spec(name).to_norm(plain));
    let (k, v) = at("volume", -6.0);
    assert_eq!(value_text(&k, v), "-6.0 dB");
    let (k, v) = at("transpose", 7.0);
    assert_eq!(value_text(&k, v), "+7 st");
    let (k, v) = at("fine", -25.0);
    assert_eq!(value_text(&k, v), "-25 cent");
    let (k, v) = at("decay", 500.0);
    assert_eq!(value_text(&k, v), "500 ms");
    let (k, v) = at("sustain", 40.0);
    assert_eq!(value_text(&k, v), "40 %");
    let (k, v) = at("pan", -50.0);
    assert!(value_text(&k, v).starts_with('L'), "{}", value_text(&k, v));
    // The box asks for the same unit, and what is typed comes back as the lane's 0..1.
    let (k, v) = at("volume", -6.0);
    assert_eq!(crate::autovalue::unit(&k), "dB");
    assert_eq!(crate::autovalue::text(&k, v), "-6");
    let typed = crate::autovalue::parse(&k, "-6").unwrap();
    assert!((typed - v).abs() < 1e-3, "{typed} for {v}");
}

fn open_list(s: &mut Studio, t: TrackId) -> crate::pick_list::PickList {
    s.open_popup(
        PopupKind::AutomationTargets(t, false),
        Rect::new(100.0, 100.0, 100.0, 28.0),
    );
    s.popup.as_ref().unwrap().pick.clone().expect("a list")
}

#[test]
fn the_target_list_groups_each_pad_with_a_sound_under_its_name() {
    let (mut s, _, t, id) = window("Drum Kit");
    let _ = id;
    fill(&mut s, id, 0);
    fill(&mut s, id, 1);
    let list = open_list(&mut s, t);
    let heads: Vec<&str> = list.groups.iter().map(|g| g.heading.as_str()).collect();
    assert!(heads.contains(&"Drum Kit · A1 Kick"), "{heads:?}");
    assert!(heads.contains(&"Drum Kit · A2 Kick"), "{heads:?}");
    assert!(!heads.iter().any(|h| h.contains("A3")), "{heads:?}");
    let group = list
        .groups
        .iter()
        .find(|g| g.heading == "Drum Kit · A1 Kick")
        .unwrap();
    let labels: Vec<&str> = group.rows.iter().map(|r| r.label.as_str()).collect();
    let want: Vec<&str> = NAMES.iter().map(|n| n.1).collect();
    assert_eq!(labels, want);
    // A row of it picks the pad's lane.
    assert_eq!(group.rows[10].to(), target(id, 0, "volume").to_string());
}

#[test]
fn the_target_list_finds_a_pad_by_its_name_or_its_sound() {
    let (mut s, _, t, id) = window("Drum Kit");
    fill(&mut s, id, 0);
    fill(&mut s, id, 1);
    let mut list = open_list(&mut s, t);
    let shown =
        |l: &crate::pick_list::PickList| l.visible().iter().map(|r| r.to()).collect::<Vec<_>>();
    list.field.value = "A2 ボリューム".into();
    assert_eq!(shown(&list), [target(id, 1, "volume").to_string()]);
    list.field.value = "a1 kick".into();
    assert_eq!(shown(&list).len(), 13);
    list.field.value = "kick カットオフ".into();
    assert_eq!(shown(&list).len(), 2);
}

#[test]
fn a_right_press_on_a_pads_knob_offers_its_lane_and_makes_it() {
    let (mut s, g, t, id) = kit();
    let (x, y) = centre(&s, &g, t, "volume");
    assert!(s.instrument_right(&g, x, y));
    assert!(crate::context::tests::choose(
        &mut s,
        &g,
        "オートメーションを表示"
    ));
    let shown = xirius_params::PAD_NAMES[s.instruments.open[0].pick];
    let lanes: Vec<String> = s.project().tracks[&t]
        .automation
        .iter()
        .map(|l| l.target.to_string())
        .collect();
    assert_eq!(lanes, [format!("param:{id}:{shown}.volume")]);
    let _ = (body(&s, &g, t), plugin(&s, t));
}
