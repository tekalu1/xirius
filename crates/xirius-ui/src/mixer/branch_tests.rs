//! A splitter's branches on the strip, as approved on 2026-11-06 (MIX-045): the ＋ at the end
//! of the splitter's row, the pan on a wide strip's level row, a muted branch with no hook and
//! the level row typed and stepped as a device's knob is (UI-324).

use super::*;
use crate::branch_value::{Focus, Place, Which};
use crate::mixer::flow::Kind;

fn strip(t: TrackId, node: NodeId, which: Which) -> Focus {
    Focus {
        track: t,
        node,
        which,
        place: Place::Strip,
    }
}

/// A press as the screen gives it: the keys, then the branch values, then the mixer.
fn click(s: &mut Studio, g: &Geometry, (x, y): (f32, f32), clicks: u32, m: Mods) {
    s.press_keys(g, x, y);
    if !s.value_press(g, x, y, clicks, m) {
        let hit = s.hit(g, x, y);
        s.press(g, hit);
    }
    s.level_up();
}

fn mods(shift: bool) -> Mods {
    Mods {
        shift,
        ..Mods::default()
    }
}

fn branches(s: &Studio, t: TrackId, sp: NodeId) -> usize {
    crate::channel::split_of(&s.project(), t, sp)
        .unwrap()
        .branches
        .len()
}

#[test]
fn mix_045_the_splitters_plus_shows_at_116_under_the_pointer_and_always_when_wide() {
    let (mut s, g) = studio();
    let (t, sp, _, _) = vocal(&mut s);
    let add = MixerHit::AddBranch(t, sp);
    // at 116 it waits for the pointer or a pick
    assert_eq!(s.mixer_center(&g, add), None);
    s.hover = Hit::Mixer(MixerHit::Fold(t, sp));
    let (x, y) = s.mixer_center(&g, add).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(add));
    s.hover = Hit::None;
    s.mixer.chosen = Some((t, Pick::Split(sp)));
    assert!(s.mixer_center(&g, add).is_some());
    s.mixer.chosen = None;
    // on a wide strip it is always there
    s.mixer.wide.insert(t);
    let (x, y) = s.mixer_center(&g, add).unwrap();
    for n in 3..10 {
        assert!(s.press(&g, Hit::Mixer(add)));
        assert_eq!(branches(&s, t, sp), n);
    }
    assert_eq!(s.mixer.chosen, Some((t, Pick::Split(sp))));
    // the screen reader has it while it shows
    let marks = crate::a11y::marks(&s, &g);
    let m = marks
        .iter()
        .find(|m| m.key == format!("flow-{t}-{add:?}"))
        .unwrap();
    assert_eq!(m.node.label(), Some("分岐を追加"));
    let _ = (x, y);
    s.undo(&g);
    assert_eq!(branches(&s, t, sp), 8);
}

#[test]
fn mix_045_a_channel_splits_row_has_no_plus() {
    let (mut s, g) = studio();
    let (t, sp, _, _) = vocal(&mut s);
    assert!(s.run_command("rack.split.mode", json!({"id": sp, "mode": "channel"})));
    s.mixer.wide.insert(t);
    s.mixer.chosen = Some((t, Pick::Split(sp)));
    assert_eq!(s.mixer_center(&g, MixerHit::AddBranch(t, sp)), None);
}

#[test]
fn mix_045_the_pan_is_on_a_wide_strips_level_row_only() {
    let (mut s, g) = studio();
    let (t, _, b, [_, _, verb]) = vocal(&mut s);
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    let pan = MixerHit::BranchPan(t, b[1]);
    assert_eq!(s.mixer_center(&g, pan), None, "not at 116");
    s.mixer.wide.insert(t);
    let (x, y) = s.mixer_center(&g, pan).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(pan));
    let r = s.mixer_rect(&g, pan).unwrap();
    assert_eq!(r.w, flow::SUB_PAN);
    // a drag right, then typed as R35, then Delete puts it back to the middle
    s.press_keys(&g, x, y);
    assert!(s.value_press(&g, x, y, 1, Mods::default()));
    assert!(s.level_move(x + r.w.max(44.0) / 4.0, false));
    assert!(s.level_up());
    assert_eq!(branch(&s, t, b[1]).1, 0.5);
    click(&mut s, &g, (x, y), 2, Mods::default());
    s.ime_replace(None, "R35", false);
    assert!(key(&mut s, &g, "enter", false));
    assert_eq!(branch(&s, t, b[1]).1, 0.35);
    assert_eq!(s.mixer.value_focus, Some(strip(t, b[1], Which::Pan)));
    assert!(key(&mut s, &g, "delete", false));
    assert_eq!(branch(&s, t, b[1]).1, 0.0);
    let marks = crate::a11y::marks(&s, &g);
    let m = marks
        .iter()
        .find(|m| m.key == format!("flow-{t}-{pan:?}"))
        .unwrap();
    assert_eq!(m.node.label(), Some("Reverb の分岐のパン"));
}

#[test]
fn mix_045_the_level_row_is_typed_and_stepped_as_a_knob() {
    let (mut s, g) = studio();
    let (t, _, b, [_, _, verb]) = vocal(&mut s);
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    let (x, y) = s.mixer_center(&g, MixerHit::Level(t, b[1])).unwrap();
    // a double press types it
    click(&mut s, &g, (x, y), 2, Mods::default());
    let e = s.mixer.value_entry.clone().unwrap();
    assert_eq!(e.focus, strip(t, b[1], Which::Gain));
    s.ime_replace(None, "-3", false);
    assert!(key(&mut s, &g, "enter", false));
    assert_eq!(branch(&s, t, b[1]).0, -3.0);
    // the arrows and PgUp/PgDn
    assert!(key(&mut s, &g, "left", false));
    assert_eq!(branch(&s, t, b[1]).0, -4.0);
    assert!(s.field_keys(&g, "right", None, mods(true)));
    assert_eq!(branch(&s, t, b[1]).0, -3.9);
    assert!(key(&mut s, &g, "pageup", false));
    assert_eq!(branch(&s, t, b[1]).0, 2.1);
    // M mutes; a right press offers the field and 0 dB
    assert!(key(&mut s, &g, "m", false));
    assert!(branch(&s, t, b[1]).2);
    assert!(s.scene_context(&g, x, y));
    let rows: Vec<String> = s.context_rows().into_iter().map(|r| r.label).collect();
    assert_eq!(rows, ["値を入力…", "0 dB に戻す"]);
    assert!(s.context_pick(&g, 1));
    assert_eq!(branch(&s, t, b[1]).0, 0.0);
    // Esc lets the keys go
    s.mixer.value_focus = Some(strip(t, b[1], Which::Gain));
    assert!(key(&mut s, &g, "enter", false));
    s.ime_replace(None, "-40", false);
    assert!(key(&mut s, &g, "escape", false));
    assert_eq!(branch(&s, t, b[1]).0, 0.0);
}

#[test]
fn mix_045_a_muted_branch_has_no_hook_and_all_muted_leave_a_stub() {
    let (mut s, g) = studio();
    let (t, sp, b, _) = vocal(&mut s);
    let hooks = |s: &Studio| {
        let f = flow_of(s, &g, t);
        let (l, row) = f.split(sp).cloned().unwrap();
        assert!(matches!(l.kind, Kind::Split { open: true, .. }));
        let x = flow::dot(row, &l);
        let x = x.x + x.w / 2.0;
        flow::pieces(&f)
            .into_iter()
            .filter(|p| p.first().is_some_and(|q| (q.from().0 - x).abs() < 0.5))
            .map(|p| (p.len(), p.last().map(|q| q.to())))
            .collect::<Vec<_>>()
    };
    assert_eq!(hooks(&s).len(), 2);
    assert!(s.run_command("rack.branch.set", json!({"id": b[0], "mute": "on"})));
    assert_eq!(hooks(&s).len(), 1);
    assert!(s.run_command("rack.branch.set", json!({"id": b[1], "mute": "on"})));
    let stub = hooks(&s);
    assert_eq!(stub.len(), 1);
    assert_eq!(stub[0].0, 1, "a short straight stub");
}

#[test]
fn mix_045_an_empty_branch_is_picked_and_its_level_and_pan_are_typed() {
    let (mut s, g) = studio();
    let (t, sp, _, _) = vocal(&mut s);
    assert!(s.press(&g, Hit::Mixer(MixerHit::AddBranch(t, sp))));
    let new = crate::channel::split_of(&s.project(), t, sp)
        .unwrap()
        .branches[2]
        .id;
    s.mixer.chosen = None;
    // at 116 a press on the empty row picks it and the level row opens under it
    let row = MixerHit::Branch(t, new);
    let at = s.mixer_center(&g, row).unwrap();
    assert_eq!(s.hit(&g, at.0, at.1), Hit::Mixer(row));
    click(&mut s, &g, at, 1, Mods::default());
    assert_eq!(s.mixer.chosen, Some((t, Pick::Branch(new))));
    let level = s.mixer_center(&g, MixerHit::Level(t, new)).unwrap();
    assert_eq!(s.mixer_center(&g, MixerHit::BranchPan(t, new)), None);
    click(&mut s, &g, level, 2, Mods::default());
    s.ime_replace(None, "-6", false);
    assert!(key(&mut s, &g, "enter", false));
    assert_eq!(branch(&s, t, new).0, -6.0);
    // on a wide strip the pan is there too
    s.mixer.wide.insert(t);
    let pan = s.mixer_center(&g, MixerHit::BranchPan(t, new)).unwrap();
    click(&mut s, &g, pan, 2, Mods::default());
    s.ime_replace(None, "L20", false);
    assert!(key(&mut s, &g, "enter", false));
    assert_eq!(branch(&s, t, new).1, -0.2);
    // the level row speaks for the empty branch; the undo takes the pan, then the level, back
    let marks = crate::a11y::marks(&s, &g);
    let level = MixerHit::Level(t, new);
    let m = marks
        .iter()
        .find(|m| m.key == format!("flow-{t}-{level:?}"))
        .unwrap();
    assert_eq!(m.node.label(), Some("分岐 3 の分岐の音量"));
    s.undo(&g);
    assert_eq!(branch(&s, t, new), (-6.0, 0.0, false));
    s.undo(&g);
    assert_eq!(branch(&s, t, new), (0.0, 0.0, false));
    assert_eq!(s.mixer.chosen, Some((t, Pick::Branch(new))));
}

#[test]
fn mix_045_a_value_lets_the_keys_go_when_they_move_on_or_it_goes_out_of_sight() {
    let (mut s, g) = studio();
    let (t, _, b, [_, comp, verb]) = vocal(&mut s);
    s.mixer.wide.insert(t);
    let before = branch(&s, t, b[1]);
    let untouched = |s: &mut Studio| {
        for k in ["left", "right", "up", "pageup", "pagedown", "delete", "m"] {
            assert!(!s.value_key(k, Mods::default()), "{k}");
        }
        assert_eq!(s.mixer.value_focus, None);
    };
    // the round on to the next place, or back to the one before, from the level and the pan
    for way in ["next", "prev"] {
        for hit in [MixerHit::Level(t, b[1]), MixerHit::BranchPan(t, b[1])] {
            s.mixer.chosen = Some((t, Pick::Plugin(verb)));
            let at = s.mixer_center(&g, hit).unwrap();
            click(&mut s, &g, at, 1, Mods::default());
            assert!(s.mixer.value_focus.is_some());
            assert!(round(&mut s, &g, way), "{way}");
            untouched(&mut s);
            assert_eq!(branch(&s, t, b[1]), before, "{way} {hit:?}");
        }
    }
    // F6 brings the browser's instruments up: the keys go with it
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    let at = s.mixer_center(&g, MixerHit::Level(t, b[1])).unwrap();
    click(&mut s, &g, at, 1, Mods::default());
    assert!(s.mixer.value_focus.is_some());
    assert!(f6(&mut s, &g));
    untouched(&mut s);
    assert_eq!(branch(&s, t, b[1]), before);
    // what is typed goes in as the keys move on
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    let at = s.mixer_center(&g, MixerHit::Level(t, b[1])).unwrap();
    click(&mut s, &g, at, 2, Mods::default());
    s.ime_replace(None, "-9", false);
    assert!(round(&mut s, &g, "next"));
    assert_eq!(s.mixer.value_entry, None);
    assert_eq!(branch(&s, t, b[1]).0, -9.0);
    s.undo(&g);
    // another row picked, or the mixer put away: the level row is gone and so are its keys
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    click(&mut s, &g, at, 1, Mods::default());
    s.mixer.chosen = Some((t, Pick::Plugin(comp)));
    untouched(&mut s);
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    click(&mut s, &g, at, 1, Mods::default());
    s.drawer_open = false;
    untouched(&mut s);
    assert_eq!(branch(&s, t, b[1]), before);
}

#[test]
fn mix_045_the_strips_pan_takes_pageup_and_pagedown_and_stops_at_the_ends() {
    let (mut s, g) = studio();
    let (t, _, b, [_, _, verb]) = vocal(&mut s);
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    s.mixer.wide.insert(t);
    let at = s.mixer_center(&g, MixerHit::BranchPan(t, b[1])).unwrap();
    click(&mut s, &g, at, 1, Mods::default());
    for want in [0.2, 0.4, 0.6, 0.8, 1.0, 1.0] {
        assert!(key(&mut s, &g, "pageup", false));
        assert_eq!(branch(&s, t, b[1]).1, want);
    }
    for _ in 0..12 {
        assert!(key(&mut s, &g, "pagedown", false));
    }
    assert_eq!(branch(&s, t, b[1]).1, -1.0);
    assert_eq!(branch(&s, t, b[1]).0, 0.0, "the level stays");
}

#[test]
fn mix_045_a_channel_splits_empty_rows_are_l_and_r() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    assert!(s.run_command("rack.split.add", json!({"track": t, "mode": "channel"})));
    let b: Vec<NodeId> = match s.project().tracks[&t].rack.as_ref().unwrap().fx.last() {
        Some(RackNode::Split(x)) => x.branches.iter().map(|b| b.id).collect(),
        other => panic!("{other:?}"),
    };
    // the rows paint and speak the names the editor gives its columns
    let f = flow_of(&s, &g, t);
    let track = s.project().tracks[&t].clone();
    let names: Vec<String> = f
        .rows
        .iter()
        .filter(|(l, _)| matches!(l.kind, Kind::Empty { channel: true, .. }))
        .map(|(l, _)| flow::row_name(&track, l))
        .collect();
    assert_eq!(names, ["分岐 L", "分岐 R"]);
    // picked, each opens its level row, and the reader hears the same name
    for (id, name) in b.iter().zip(["分岐 L", "分岐 R"]) {
        s.mixer.chosen = Some((t, flow::Pick::Branch(*id)));
        let marks = crate::a11y::marks(&s, &g);
        let level = MixerHit::Level(t, *id);
        let m = marks
            .iter()
            .find(|m| m.key == format!("flow-{t}-{level:?}"))
            .unwrap();
        assert_eq!(
            m.node.label(),
            Some(format!("{name} の分岐の音量").as_str())
        );
    }
    // a normal split's stay numbered
    assert_eq!(flow::empty_name(1, false), "分岐 2");
}

/// F6 as the screen takes it: the value does not keep it, and its binding shows the browser's
/// instruments.
fn f6(s: &mut Studio, g: &Geometry) -> bool {
    assert!(!s.field_keys(g, "f6", None, Mods::default()));
    let args = json!({ "tab": "instruments" });
    s.key_more(g, "browser.tab", args.as_object().unwrap()) == Some(true)
}

/// The keys go round the places (「次の領域へ」 / 「前の領域へ」), as the key or the palette runs it.
fn round(s: &mut Studio, g: &Geometry, way: &str) -> bool {
    let args = json!({ "way": way });
    s.key_more(g, "view.focus", args.as_object().unwrap()) == Some(true)
}
