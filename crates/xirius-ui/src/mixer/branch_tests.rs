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
