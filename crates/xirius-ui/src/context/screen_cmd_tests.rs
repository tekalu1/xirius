//! OPS-006 / CTX-001: the screen-only rows run through the named commands. A row of a menu and
//! the same command typed as a binding do the same thing, and every row without a command is
//! listed here, so a new row has to be classified.

use std::path::PathBuf;

use serde_json::json;

use super::browser::{BrowseAct, FxTarget};
use super::export::{ExportAct, Row};
use super::screen_cmd::act_command;
use super::tests::studio;
use super::{Act, Item};
use crate::arrange::probe::track;
use crate::roll::LaneKind;
use crate::studio::{DrawerTab, Geometry, Rect, Studio};
use crate::tree::RowKind;
use xirius_core::auto::AutoTarget;
use xirius_core::id::{FolderId, PluginId, TrackId};
use xirius_core::model::CtrlKind;

/// Every kind of act, and whether the command it names is a screen command (`screen_ops`) rather
/// than one of the song. No wildcard: a new act fails to compile until it is put on one side.
fn on_screen(act: &Act) -> bool {
    match act {
        Act::Open
        | Act::DeletePoints
        | Act::SelectIn(_)
        | Act::HideLane(..)
        | Act::RangeFrame(..)
        | Act::Tool(_)
        | Act::Seek(_) => false,
        Act::RollPitch(_)
        | Act::RollLaneKind(..)
        | Act::RollLaneHide(_)
        | Act::Reveal(_)
        | Act::ShowStrip(_)
        | Act::SelectSources(_)
        | Act::OpenPlugin(..)
        | Act::Lane(..)
        | Act::Folders(_)
        | Act::Stems(_)
        | Act::FolderStem(_)
        | Act::PlayFrom(_)
        | Act::ExportLoop
        | Act::ExportSpan(..)
        | Act::MidiExport(..)
        | Act::PointValue(..)
        | Act::Preset(..)
        | Act::RestartPlugin(_)
        | Act::PickInsert(_)
        | Act::InsertRecent(..)
        | Act::PresetFile(..)
        | Act::Browse(_)
        | Act::MidiLearn(_)
        | Act::MidiLearnCancel
        | Act::MidiLearnConfirm
        | Act::Export(_) => true,
    }
}

fn samples(t: TrackId, p: PluginId, f: FolderId) -> Vec<Act> {
    let path = || PathBuf::from("sounds/kick.wav");
    vec![
        Act::Open,
        Act::Tool("tool.draw"),
        Act::Seek(1920),
        Act::PlayFrom(960),
        Act::Lane(t, "volume".into()),
        Act::HideLane(t, AutoTarget::Volume),
        Act::SelectIn(f),
        Act::Folders(true),
        Act::Folders(false),
        Act::Stems(vec![t]),
        Act::FolderStem(f),
        Act::ExportLoop,
        Act::ExportSpan(0, 3840),
        Act::MidiExport(vec![t], None),
        Act::MidiExport(vec![t], Some((0, 3840))),
        Act::PointValue(t, AutoTarget::Volume, 960, 10.0, 10.0),
        Act::DeletePoints,
        Act::RangeFrame(Some((t, AutoTarget::Volume)), 0, 1920),
        Act::RangeFrame(None, 0, 1920),
        Act::RollPitch(60),
        Act::RollLaneKind(1, LaneKind::Ctrl(CtrlKind::Cc(74))),
        Act::RollLaneKind(0, LaneKind::Velocity),
        Act::RollLaneHide(2),
        Act::MidiLearn(format!("track.volume:{t}")),
        Act::MidiLearn(format!("track.pan:{t}")),
        Act::MidiLearnCancel,
        Act::MidiLearnConfirm,
        Act::Reveal(t),
        Act::ShowStrip(t),
        Act::SelectSources(t),
        Act::OpenPlugin(t, p),
        Act::Preset(p, "Warm".into()),
        Act::RestartPlugin(p),
        Act::PickInsert(t),
        Act::InsertRecent(t, "EQ".into()),
        Act::PresetFile(p, false),
        Act::PresetFile(p, true),
        Act::Browse(BrowseAct::UseNew("Sampler")),
        Act::Browse(BrowseAct::UseHere("Sampler")),
        Act::Browse(BrowseAct::Favourite("Sampler", true)),
        Act::Browse(BrowseAct::Favourite("Sampler", false)),
        Act::Browse(BrowseAct::FavouriteFile(path(), true)),
        Act::Browse(BrowseAct::Hide("Dexed", true)),
        Act::Browse(BrowseAct::Hide("Dexed", false)),
        Act::Browse(BrowseAct::Fx(FxTarget::Channel, "EQ")),
        Act::Browse(BrowseAct::Fx(FxTarget::NewBus, "EQ")),
        Act::Browse(BrowseAct::Fx(FxTarget::Master, "EQ")),
        Act::Browse(BrowseAct::Groups(true)),
        Act::Browse(BrowseAct::ShowHidden(true)),
        Act::Browse(BrowseAct::Rescan),
        Act::Browse(BrowseAct::PutNew(path())),
        Act::Browse(BrowseAct::PutHere(path())),
        Act::Browse(BrowseAct::AddPlace(path())),
        Act::Browse(BrowseAct::RemovePlace("Samples".into())),
        Act::Browse(BrowseAct::Reveal(path())),
        Act::Browse(BrowseAct::Cleanup),
        Act::Export(ExportAct {
            job: "j1".into(),
            row: Row::Listen,
            anchor: Rect::default(),
            file: None,
        }),
        Act::Export(ExportAct {
            job: "j1".into(),
            row: Row::Rewrite,
            anchor: Rect::default(),
            file: Some("C:/x/Bass.wav".into()),
        }),
        Act::Export(ExportAct {
            job: "j1".into(),
            row: Row::Trash,
            anchor: Rect::default(),
            file: None,
        }),
    ]
}

#[test]
fn every_act_names_a_registered_command_and_gives_arguments_it_declares() {
    let (s, _) = studio();
    let t = track(&s, "Vocal");
    let f = *s.project().folders.keys().next().expect("a folder");
    for act in samples(t, PluginId(1), f) {
        let (name, args) = act_command(&act);
        let def = s
            .song
            .registry()
            .get(name)
            .unwrap_or_else(|| panic!("{name} is not in the table ({act:?})"));
        if on_screen(&act) {
            assert!(!def.history, "{name} is a screen command");
        }
        let args = args.as_object().expect("an object");
        for key in args.keys() {
            assert!(
                def.args.iter().any(|a| a.name == key),
                "{name} has no argument {key}"
            );
        }
        for a in def.args.iter().filter(|a| a.required) {
            assert!(args.contains_key(a.name), "{name} lacks {}", a.name);
        }
    }
}

fn run_row(s: &mut Studio, g: &Geometry, act: Act) -> bool {
    s.context_run(g, Item::act("row", "", act))
}

fn view_of(s: &Studio) -> (Vec<RowKind>, Option<RowKind>, f32) {
    (s.arrange.tracks.clone(), s.selected_row, s.mixer.scroll)
}

#[test]
fn a_menu_row_and_the_typed_command_show_the_same_channel() {
    let (mut a, g) = studio();
    let (mut b, _) = studio();
    let t = track(&a, "Guitar");
    for (act, line) in [
        (Act::Reveal(t), format!("track.reveal --id {t}")),
        (Act::Reveal(t), format!("track.reveal --id {t} --in tracks")),
    ] {
        assert!(run_row(&mut a, &g, act));
        assert!(b.run_binding(&g, &line), "{line}");
        assert_eq!(view_of(&a), view_of(&b), "{line}");
        assert_eq!(a.selected_row, Some(RowKind::Track(t)));
    }
    // The mixer's strip: the same scroll either way.
    for s in [&mut a, &mut b] {
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
    }
    assert!(run_row(&mut a, &g, Act::ShowStrip(t)));
    assert!(b.run_binding(&g, &format!("track.reveal --id {t} --in mixer")));
    assert_eq!(view_of(&a), view_of(&b));
}

#[test]
fn the_sources_of_a_bus_are_selected_by_the_row_and_by_the_command() {
    let (mut a, g) = studio();
    let (mut b, _) = studio();
    for s in [&mut a, &mut b] {
        let v = track(s, "Vocal");
        assert!(s.run_command(
            "send.add.bus",
            json!({"ids": [v.to_string()], "name": "Space"})
        ));
    }
    let bus = track(&a, "Space");
    assert!(run_row(&mut a, &g, Act::SelectSources(bus)));
    assert!(b.run_binding(&g, &format!("select.sources --id {bus}")));
    assert_eq!(view_of(&a), view_of(&b));
    assert_eq!(a.arrange.tracks, [RowKind::Track(track(&a, "Vocal"))]);
    // A bus nobody plays into has nothing to select.
    let t = track(&a, "Guitar");
    assert!(!b.run_binding(&g, &format!("select.sources --id {t}")));
}

#[test]
fn the_dock_opens_closes_and_switches_by_name() {
    let (mut s, g) = studio();
    assert!(!s.drawer_open);
    assert!(s.run_binding(&g, "view.dock --tab mixer"));
    assert!(s.drawer_open && s.tab == DrawerTab::Mixer);
    assert!(s.run_binding(&g, "view.dock --tab roll --state open"));
    assert!(
        s.drawer_open && s.tab == DrawerTab::Roll,
        "the other tab opens"
    );
    assert!(s.run_binding(&g, "view.dock --tab mixer --state closed"));
    assert!(s.drawer_open, "another tab's close leaves it open");
    assert!(s.run_binding(&g, "view.dock --tab roll --state closed"));
    assert!(!s.drawer_open);
    assert!(!s.run_binding(&g, "view.dock --tab nowhere"));
}

#[test]
fn a_lane_is_named_the_way_the_song_names_it() {
    use crate::roll::{COMMON, NUMBERED};
    let (s, g) = studio();
    let mut s = s;
    let _ = &g;
    let mut all: Vec<LaneKind> = COMMON.to_vec();
    all.extend(NUMBERED.iter().map(|n| LaneKind::Ctrl(CtrlKind::Cc(*n))));
    for k in all {
        let (_, args) = act_command(&Act::RollLaneKind(0, k));
        let said = args["lane"].as_str().unwrap();
        let back = if said == "velocity" {
            Some(LaneKind::Velocity)
        } else {
            CtrlKind::parse(said).map(LaneKind::Ctrl)
        };
        assert_eq!(back, Some(k), "{said}");
    }
    // Without a roll open, the lane commands find nothing and say so instead of failing.
    s.drawer_open = false;
    assert!(!s.run_binding(&g, "cc.lane.hide --index 3"));
}

#[test]
fn the_send_rows_of_the_mixer_use_the_send_level_commands() {
    let (s, _) = studio();
    for name in ["send.level.set", "send.level.reset", "send.set"] {
        assert!(s.song.registry().get(name).is_some(), "{name}");
    }
}
