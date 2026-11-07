//! The branches of a splitter in the channel editor, as approved on 2026-11-06 (MIX-045): the
//! ＋ right of the columns, the ✖ that always deletes, a muted branch's cut line and the head's
//! level and pan typed and stepped as a device's knob is (UI-324).

use serde_json::json;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::rack::{RackNode, SplitMode};
use xirius_design::LinePiece;

use super::*;
use crate::branch_value::{Focus, Place, Which};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.instruments.open.clear();
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn add(s: &mut Studio, t: TrackId, name: &str) -> PluginId {
    assert!(s.run_command("plugin.add", json!({"track":t,"name":name})));
    s.project().tracks[&t].inserts.last().unwrap().id
}

/// Vocal with EQ, then a splitter: Comp in one branch, Reverb at -6 dB in the other.
fn vocal(s: &mut Studio) -> (TrackId, NodeId, [NodeId; 2]) {
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .id;
    add(s, t, "EQ");
    let comp = add(s, t, "Comp");
    let verb = add(s, t, "Reverb");
    assert!(s.run_command("rack.split.add", json!({"track":t})));
    let (sp, b) = match &s.project().tracks[&t].rack.as_ref().unwrap().fx[..] {
        [.., RackNode::Split(x)] => (x.id, [x.branches[0].id, x.branches[1].id]),
        other => panic!("{other:?}"),
    };
    assert!(s.run_command("rack.node.move", json!({"id":comp,"to":b[0]})));
    assert!(s.run_command("rack.node.move", json!({"id":verb,"to":b[1]})));
    assert!(s.run_command("rack.branch.set", json!({"id":b[1],"gain":-6.0})));
    s.open_channel(t);
    (t, sp, b)
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn column(v: &View, id: NodeId) -> Column {
    v.flow.columns.iter().find(|c| c.id == id).cloned().unwrap()
}

fn split(s: &Studio, t: TrackId, id: NodeId) -> Option<Split> {
    split_of(&s.project(), t, id)
}

fn press(s: &mut Studio, g: &Geometry, (x, y): (f32, f32)) {
    assert!(s.channel_press(g, x, y, 1));
    s.channel_up(g);
}

/// A press as the screen gives it: the keys first, then the branch values, then the editor.
fn click(s: &mut Studio, g: &Geometry, (x, y): (f32, f32), clicks: u32, m: Mods) {
    s.press_keys(g, x, y);
    if !s.value_press(g, x, y, clicks, m) {
        s.channel_press(g, x, y, clicks);
    }
    s.level_up();
    s.channel_up(g);
}

fn key(s: &mut Studio, g: &Geometry, key: &str, m: Mods) -> bool {
    s.field_keys(g, key, None, m)
}

fn levels(s: &Studio, t: TrackId, n: NodeId) -> (f64, f64, bool) {
    s.project().tracks[&t]
        .rack
        .as_ref()
        .unwrap()
        .levels(n)
        .unwrap()
}

fn editor(t: TrackId, node: NodeId, which: Which) -> Focus {
    Focus {
        track: t,
        node,
        which,
        place: Place::Editor,
    }
}

/// Whether a straight line passes `(x, y)` up and down.
fn line_at(v: &View, x: f32, y: f32) -> bool {
    v.flow.links.iter().flat_map(|l| l.pieces.iter()).any(|p| {
        let (a, b) = (p.from(), p.to());
        matches!(p, LinePiece::Straight { .. })
            && (a.0 - x).abs() < 0.5
            && (b.0 - x).abs() < 0.5
            && a.1.min(b.1) <= y
            && y <= a.1.max(b.1)
    })
}

fn labels(s: &Studio, g: &Geometry) -> Vec<String> {
    marks(s, g)[0]
        .children
        .iter()
        .map(|m| m.node.label().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn mix_045_the_plus_right_of_the_columns_adds_a_branch_with_no_limit() {
    let (mut s, g) = studio();
    let (t, sp, _) = vocal(&mut s);
    for n in 2..9 {
        let v = s.channel_view(&g).unwrap();
        assert_eq!(v.flow.adds.len(), 1);
        let add = v.flow.adds[0].clone();
        assert_eq!(add.split, sp);
        assert_eq!((add.rect.w, add.rect.h), (flow::ADD_BTN, flow::ADD_BTN));
        // right of the last column, level with the heads, as the mock's
        let last = v
            .flow
            .columns
            .iter()
            .filter(|c| matches!(c.kind, flow::ColKind::Branch { .. }))
            .max_by(|a, b| a.rect.x.total_cmp(&b.rect.x))
            .unwrap();
        assert_eq!(add.rect.x, last.rect.right() + flow::COL_GAP + 4.0);
        assert_eq!(add.rect.y, last.head.y + 1.0);
        assert_eq!(
            part_at(&v, mid(add.rect).0, mid(add.rect).1),
            Some(Part::AddBranch(sp))
        );
        assert_eq!(split(&s, t, sp).unwrap().branches.len(), n);
        press(&mut s, &g, mid(add.rect));
        assert_eq!(s.channel.as_ref().unwrap().chosen, Some(Pick::Split(sp)));
    }
    // Every branch keeps its column, its head and its ✖.
    let v = s.channel_view(&g).unwrap();
    let sp_now = split(&s, t, sp).unwrap();
    assert_eq!(sp_now.branches.len(), 9);
    for b in &sp_now.branches {
        assert!(head_parts(&column(&v, b.id)).remove.is_some());
    }
    // one undo takes one branch back
    s.undo(&g);
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 8);
}

#[test]
fn mix_045_the_plus_has_its_name_its_tip_and_its_menu() {
    let (mut s, g) = studio();
    let (t, sp, b) = vocal(&mut s);
    let labels = labels(&s, &g);
    for want in [
        "分岐を追加",
        "分岐 1 を削除",
        "分岐 2 を削除",
        "分岐 1 のパン",
        "分岐 2 のパン",
    ] {
        assert!(labels.iter().any(|l| l == want), "{want} in {labels:?}");
    }
    let v = s.channel_view(&g).unwrap();
    let (x, y) = mid(v.flow.adds[0].rect);
    s.channel_hover(&g, x, y);
    assert_eq!(
        crate::hover_tip::pointer_tip(&s, &g).map(|(c, _)| c),
        Some("rack.branch.add")
    );
    assert_eq!(
        xirius_core::commands::labels::label("rack.branch.add"),
        Some("分岐を追加")
    );
    assert_eq!(
        xirius_core::commands::labels::label("rack.branch.remove"),
        Some("分岐を削除")
    );
    let r = head_parts(&column(&v, b[1])).remove.unwrap();
    s.channel_hover(&g, mid(r).0, mid(r).1);
    assert_eq!(
        crate::hover_tip::pointer_tip(&s, &g).map(|(c, _)| c),
        Some("rack.branch.remove")
    );
    // A right press on the ＋ offers 「分岐を追加」; so does the splitter's menu.
    assert!(s.channel_right(&g, x, y));
    let rows: Vec<String> = s.context_rows().into_iter().map(|r| r.label).collect();
    assert_eq!(rows, ["分岐を追加"]);
    assert!(s.context_pick(&g, 0));
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 3);
}

#[test]
fn mix_045_a_channel_split_has_l_and_r_and_no_plus_or_cross() {
    let (mut s, g) = studio();
    let (t, sp, b) = vocal(&mut s);
    assert!(s.run_command("rack.branch.add", json!({"split": sp})));
    assert!(s.run_command("rack.split.mode", json!({"id": sp, "mode": "channel"})));
    // three branches go down to two; the undo brings the third back
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 2);
    let v = s.channel_view(&g).unwrap();
    assert!(v.flow.adds.is_empty());
    assert_eq!(col_label(&column(&v, b[0])), "L");
    let hp = head_parts(&column(&v, b[1]));
    assert_eq!(hp.label, "R");
    assert!(hp.remove.is_none());
    let labels = labels(&s, &g);
    assert!(!labels.iter().any(|l| l == "分岐を追加"), "{labels:?}");
    // the splitter's menu has no 「分岐を追加」
    let card = v
        .flow
        .cards
        .iter()
        .find(|c| c.pick == Pick::Split(sp))
        .unwrap()
        .rect;
    assert!(s.channel_right(&g, mid(card).0, mid(card).1));
    let rows: Vec<String> = s.context_rows().into_iter().map(|r| r.label).collect();
    assert!(!rows.iter().any(|r| r == "分岐を追加"), "{rows:?}");
    s.popup = None;
    s.undo(&g);
    let back = split(&s, t, sp).unwrap();
    assert_eq!((back.mode, back.branches.len()), (SplitMode::Normal, 3));
}

#[test]
fn mix_045_the_cross_deletes_and_at_two_takes_the_splitter_off() {
    let (mut s, g) = studio();
    let (t, sp, b) = vocal(&mut s);
    assert!(s.run_command("rack.branch.add", json!({"split": sp})));
    let third = split(&s, t, sp).unwrap().branches[2].id;
    // three: the branch goes, with an undo in the notice
    let v = s.channel_view(&g).unwrap();
    press(
        &mut s,
        &g,
        mid(head_parts(&column(&v, third)).remove.unwrap()),
    );
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 2);
    assert_eq!(s.notice_text(), Some("分岐を削除しました"));
    let button = s.notice.as_ref().and_then(|n| n.button.as_ref()).unwrap();
    assert_eq!(button.label, "元に戻す");
    // two: the splitter goes and Comp stays in series where it was
    let v = s.channel_view(&g).unwrap();
    press(
        &mut s,
        &g,
        mid(head_parts(&column(&v, b[1])).remove.unwrap()),
    );
    assert!(split(&s, t, sp).is_none());
    let names: Vec<String> = s.project().tracks[&t]
        .inserts
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert_eq!(names, ["EQ", "Comp"]);
    let fx = s.project().tracks[&t]
        .rack
        .clone()
        .map(|r| r.fx)
        .unwrap_or_default();
    assert!(
        fx.iter().all(|n| matches!(n, RackNode::Plugin(_))),
        "{fx:?}"
    );
    assert_eq!(s.notice_text(), Some("スプリッターを外しました"));
    // one undo each brings them back
    s.undo(&g);
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 2);
    s.undo(&g);
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 3);
}

#[test]
fn mix_045_the_notices_undo_brings_the_branch_back() {
    let (mut s, g) = studio();
    let (t, sp, _) = vocal(&mut s);
    assert!(s.run_command("rack.branch.add", json!({"split": sp})));
    let third = split(&s, t, sp).unwrap().branches[2].id;
    let v = s.channel_view(&g).unwrap();
    let at = mid(head_parts(&column(&v, third)).remove.unwrap());
    click(&mut s, &g, at, 1, Mods::default());
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 2);
    let text = s.notice_text().expect("a notice").to_string();
    let label = s.notice_button().expect("a button").to_string();
    let r = crate::notice::button_rect(crate::notice::place(&s, &g, &text, Some(&label)), &label);
    assert!(s.notice_press(&g, r.x + r.w / 2.0, r.y + r.h / 2.0));
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 3);
}

#[test]
fn mix_045_a_muted_branch_cuts_its_line_and_keeps_its_name() {
    let (mut s, g) = studio();
    let (t, sp, b) = vocal(&mut s);
    assert!(s.run_command("rack.branch.add", json!({"split": sp})));
    let empty = split(&s, t, sp).unwrap().branches[2].id;
    let cut = |s: &Studio, n: NodeId| {
        let v = s.channel_view(&g).unwrap();
        let col = column(&v, n);
        let cx = col.head.x + col.head.w / 2.0;
        let full = split(s, t, sp)
            .unwrap()
            .branches
            .iter()
            .any(|x| x.id == n && !x.fx.is_empty());
        let m = col.head.bottom()
            + if full {
                flow::STEP / 2.0
            } else {
                flow::CARD_H / 2.0
            };
        (
            line_at(&v, cx, m - flow::MUTE_CUT / 2.0 - 1.0),
            line_at(&v, cx, m),
            line_at(&v, cx, m + flow::MUTE_CUT / 2.0 + 1.0),
        )
    };
    for n in [b[1], empty] {
        assert_eq!(cut(&s, n), (true, true, true), "on");
        assert!(s.run_command("rack.branch.set", json!({"id": n, "mute": "on"})));
        // a gap of MUTE_CUT under the head; the line goes on after it
        assert_eq!(cut(&s, n), (true, false, true), "muted");
    }
    assert_eq!(cut(&s, b[0]), (true, true, true), "the others stay");
    // At rest the head keeps the yellow M; the name is not dimmed (the same ink).
    let labels = labels(&s, &g);
    assert!(
        labels.iter().any(|l| l == "分岐 3 をミュート"),
        "{labels:?}"
    );
    assert!(labels.iter().any(|l| l == "分岐 2 の音量"), "{labels:?}");
}

#[test]
fn mix_045_a_heads_level_is_typed_and_stepped_as_a_knob() {
    let (mut s, g) = studio();
    let (t, _, b) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let hp = head_parts(&column(&v, b[1]));
    let gain = editor(t, b[1], Which::Gain);
    let none = Mods::default();
    // a double press types it, with what it holds chosen
    click(&mut s, &g, mid(hp.value), 2, none);
    let e = s.mixer.value_entry.clone().unwrap();
    assert_eq!((e.focus, e.field.text().as_str()), (gain, "-6.0"));
    s.ime_replace(None, "-3 dB", false);
    assert!(key(&mut s, &g, "enter", none));
    assert_eq!(levels(&s, t, b[1]).0, -3.0);
    assert!(s.mixer.value_entry.is_none());
    assert_eq!(s.mixer.value_focus, Some(gain));
    // Enter opens it too; Esc keeps the old value
    assert!(key(&mut s, &g, "enter", none));
    s.ime_replace(None, "-20", false);
    assert!(key(&mut s, &g, "escape", none));
    assert_eq!(levels(&s, t, b[1]).0, -3.0);
    // -inf is silence; Tab puts it in and goes to the pan
    assert!(key(&mut s, &g, "enter", none));
    s.ime_replace(None, "-inf", false);
    assert!(key(&mut s, &g, "tab", none));
    assert_eq!(levels(&s, t, b[1]).0, xirius_core::model::MIN_DB);
    assert_eq!(s.mixer.value_focus, Some(editor(t, b[1], Which::Pan)));
    s.mixer.value_focus = Some(gain);
    // Delete puts it back to 0 dB; the arrows move a dB, with Shift a tenth; PgUp/PgDn 6 dB
    assert!(key(&mut s, &g, "delete", none));
    assert_eq!(levels(&s, t, b[1]).0, 0.0);
    assert!(key(&mut s, &g, "left", none));
    assert_eq!(levels(&s, t, b[1]).0, -1.0);
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    assert!(key(&mut s, &g, "right", shift));
    assert_eq!(levels(&s, t, b[1]).0, -0.9);
    assert!(key(&mut s, &g, "pagedown", none));
    assert_eq!(levels(&s, t, b[1]).0, -6.9);
    assert!(key(&mut s, &g, "pageup", none));
    assert_eq!(levels(&s, t, b[1]).0, -0.9);
    // M mutes
    assert!(key(&mut s, &g, "m", none));
    assert!(levels(&s, t, b[1]).2);
    // Alt and a press put it back to 0 dB
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    click(&mut s, &g, mid(hp.value), 1, alt);
    assert_eq!(levels(&s, t, b[1]).0, 0.0);
    // A press away puts what is typed in and lets the keys go.
    click(&mut s, &g, mid(hp.value), 2, none);
    s.ime_replace(None, "-12", false);
    s.press_keys(&g, 4.0, 4.0);
    assert_eq!(levels(&s, t, b[1]).0, -12.0);
    assert_eq!(s.mixer.value_focus, None);
    assert!(s.mixer.value_entry.is_none());
}

#[test]
fn mix_045_a_heads_pan_is_dragged_typed_and_put_back() {
    let (mut s, g) = studio();
    let (t, _, b) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let hp = head_parts(&column(&v, b[0]));
    let pan = hp.pan.unwrap();
    assert_eq!(pan.w, PAN_W);
    let (x, y) = mid(pan);
    assert_eq!(part_at(&v, x, y), Some(Part::Pan(b[0])));
    // a drag right: the whole sweep is twice its width (the mock's), one step
    s.press_keys(&g, x, y);
    assert!(s.value_press(&g, x, y, 1, Mods::default()));
    assert!(s.level_move(x + pan.w.max(44.0) / 4.0, false));
    assert!(s.level_up());
    assert_eq!(levels(&s, t, b[0]).1, 0.5);
    s.undo(&g);
    assert_eq!(levels(&s, t, b[0]).1, 0.0);
    // typed as L20 or R35
    click(&mut s, &g, (x, y), 2, Mods::default());
    assert_eq!(s.mixer.value_entry.as_ref().unwrap().field.text(), "C");
    s.ime_replace(None, "L20", false);
    assert!(key(&mut s, &g, "enter", Mods::default()));
    assert_eq!(levels(&s, t, b[0]).1, -0.2);
    // the menu's 「既定に戻す」
    assert!(s.channel_right(&g, x, y));
    let rows: Vec<String> = s.context_rows().into_iter().map(|r| r.label).collect();
    assert_eq!(rows, ["値を入力…", "既定に戻す"]);
    assert!(s.context_pick(&g, 1));
    assert_eq!(levels(&s, t, b[0]).1, 0.0);
    // the level's menu, and its 「値を入力…」 opens the field
    let (lx, ly) = mid(hp.level);
    assert!(s.channel_right(&g, lx, ly));
    let rows: Vec<String> = s.context_rows().into_iter().map(|r| r.label).collect();
    assert_eq!(rows, ["値を入力…", "0 dB に戻す"]);
    assert!(s.context_pick(&g, 0));
    assert_eq!(
        s.mixer.value_entry.as_ref().map(|e| e.focus),
        Some(editor(t, b[0], Which::Gain))
    );
}

#[test]
fn mix_045_a_layer_head_has_no_pan() {
    let (mut s, g) = studio();
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    for name in ["Piano", "Strings"] {
        assert!(s.run_command("rack.layer.add", json!({"track":t,"name":name})));
    }
    s.open_channel(t);
    let v = s.channel_view(&g).unwrap();
    assert!(!v.flow.columns.is_empty());
    for col in &v.flow.columns {
        assert!(head_parts(col).pan.is_none());
    }
    assert!(v.flow.adds.is_empty());
}

#[test]
fn mix_045_sixty_four_branches_lay_out_without_a_stall() {
    let (mut s, g) = studio();
    let (t, sp, _) = vocal(&mut s);
    for _ in 2..64 {
        assert!(s.run_command("rack.branch.add", json!({"split": sp})));
    }
    let start = std::time::Instant::now();
    let v = s.channel_view(&g).unwrap();
    let marks = marks(&s, &g);
    let took = start.elapsed();
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 64);
    assert_eq!(
        v.flow
            .columns
            .iter()
            .filter(|c| matches!(c.kind, flow::ColKind::Branch { .. }))
            .count(),
        64
    );
    assert!(!marks.is_empty());
    // a debug build's layout and reader tree of a 64-wide flow stay well under a frame's worth
    // of a second
    assert!(took.as_millis() < 500, "{took:?}");
    // the strip lays out the 64 too
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    let g = s.geometry(1600.0, 1000.0);
    let start = std::time::Instant::now();
    let strip = s
        .mixer_strips(&g)
        .into_iter()
        .find(|x| x.track == t)
        .unwrap();
    let flow = s.strip_flow(&strip, &s.project().tracks[&t]);
    assert!(start.elapsed().as_millis() < 500);
    assert!(flow.rows.len() > 64);
}

#[test]
fn mix_045_a_heads_value_lets_the_keys_go_when_they_move_on_or_the_editor_shuts() {
    let (mut s, g) = studio();
    let (t, _, b) = vocal(&mut s);
    let before = levels(&s, t, b[0]);
    let untouched = |s: &mut Studio| {
        for k in ["left", "right", "up", "pageup", "pagedown", "delete", "m"] {
            assert!(!s.value_key(k, Mods::default()), "{k}");
        }
        assert_eq!(s.mixer.value_focus, None);
    };
    for way in ["next", "prev"] {
        let v = s.channel_view(&g).unwrap();
        let hp = head_parts(&column(&v, b[0]));
        for r in [hp.level, hp.pan.unwrap()] {
            click(&mut s, &g, mid(r), 1, Mods::default());
            assert!(s.mixer.value_focus.is_some());
            assert!(round(&mut s, &g, way), "{way}");
            untouched(&mut s);
            assert_eq!(levels(&s, t, b[0]), before, "{way}");
        }
    }
    // F6 brings the browser's instruments up: the keys go with it
    let v = s.channel_view(&g).unwrap();
    let hp = head_parts(&column(&v, b[0]));
    click(&mut s, &g, mid(hp.pan.unwrap()), 1, Mods::default());
    assert!(s.mixer.value_focus.is_some());
    assert!(!key(&mut s, &g, "f6", Mods::default()));
    let args = json!({ "tab": "instruments" });
    assert_eq!(
        s.key_more(&g, "browser.tab", args.as_object().unwrap()),
        Some(true)
    );
    untouched(&mut s);
    assert_eq!(levels(&s, t, b[0]), before);
    // typed, then F6 through the field's keys to the browser: what is typed goes in as one
    // step, the field shuts and the keys go
    click(&mut s, &g, mid(hp.value), 2, Mods::default());
    assert!(s.mixer.value_entry.is_some());
    let steps = s.song.steps();
    s.ime_replace(None, "-9", false);
    assert!(!s.field_keys(&g, "f6", None, Mods::default()));
    assert_eq!(
        s.key_more(&g, "browser.tab", args.as_object().unwrap()),
        Some(true)
    );
    assert!(s.mixer.value_entry.is_none());
    untouched(&mut s);
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(levels(&s, t, b[0]).0, -9.0);
    s.undo(&g);
    assert_eq!(levels(&s, t, b[0]), before);
    // typed, and the editor shuts: what is typed goes in and the keys go
    click(&mut s, &g, mid(hp.value), 2, Mods::default());
    s.ime_replace(None, "-4", false);
    let open = s.channel.take();
    assert!(!s.value_key("enter", Mods::default()));
    assert!(s.mixer.value_entry.is_none());
    untouched(&mut s);
    assert_eq!(levels(&s, t, b[0]).0, -4.0);
    s.undo(&g);
    s.channel = open;
    // the editor shut: its heads take no keys
    click(&mut s, &g, mid(hp.level), 1, Mods::default());
    assert!(s.mixer.value_focus.is_some());
    s.channel = None;
    untouched(&mut s);
    assert_eq!(levels(&s, t, b[0]), before);
}

#[test]
fn mix_045_a_heads_pan_takes_pageup_and_pagedown_and_stops_at_the_ends() {
    let (mut s, g) = studio();
    let (t, _, b) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let pan = head_parts(&column(&v, b[1])).pan.unwrap();
    click(&mut s, &g, mid(pan), 1, Mods::default());
    for want in [-0.2, -0.4, -0.6, -0.8, -1.0, -1.0] {
        assert!(key(&mut s, &g, "pagedown", Mods::default()));
        assert_eq!(levels(&s, t, b[1]).1, want);
    }
    for _ in 0..12 {
        assert!(key(&mut s, &g, "pageup", Mods::default()));
    }
    assert_eq!(levels(&s, t, b[1]).1, 1.0);
    assert_eq!(levels(&s, t, b[1]).0, -6.0, "the level stays");
}

/// The press a screen reader's action on the mark `key` makes, as the screen wires it.
fn reader_press(s: &mut Studio, g: &Geometry, key: &str) -> bool {
    fn find(marks: &[crate::a11y::Mark], key: &str) -> Option<crate::studio::Hit> {
        marks.iter().find_map(|m| {
            if m.key == key {
                m.press
            } else {
                find(&m.children, key)
            }
        })
    }
    let hit = find(&crate::a11y::marks(s, g), key).expect("the mark has an action");
    crate::a11y::press(s, g, hit)
}

#[test]
fn mix_045_a_reader_presses_the_plus_and_a_heads_mute() {
    let (mut s, g) = studio();
    let (t, sp, b) = vocal(&mut s);
    assert!(reader_press(
        &mut s,
        &g,
        &format!("channel-add-branch-{sp}")
    ));
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 3);
    assert_eq!(s.channel.as_ref().unwrap().chosen, Some(Pick::Split(sp)));
    assert!(reader_press(
        &mut s,
        &g,
        &format!("channel-add-branch-{sp}")
    ));
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 4);
    s.undo(&g);
    assert_eq!(split(&s, t, sp).unwrap().branches.len(), 3);
    assert!(reader_press(&mut s, &g, &format!("channel-mute-{}", b[0])));
    assert!(levels(&s, t, b[0]).2);
}

#[test]
fn mix_045_empty_branch_names_remain_visible_without_the_controls() {
    let (mut s, g) = studio();
    let (t, sp, _) = vocal(&mut s);
    assert!(s.run_command("rack.branch.add", json!({"split":sp})));
    s.channel.as_mut().unwrap().pointer = None;
    s.channel.as_mut().unwrap().chosen = None;
    let v = s.channel_view(&g).unwrap();
    for b in split(&s, t, sp).unwrap().branches {
        let col = column(&v, b.id);
        assert!(!head_shown(
            s.channel.as_ref().unwrap(),
            &col,
            &v.flow,
            None
        ));
        let (name, rect) = paint::resting_branch_name(&col).unwrap();
        assert_eq!(name, format!("分岐 {}", col.n));
        assert!(rect.right() < col.head.x + col.head.w / 2.0);
        assert!(rect.right() < head_parts(&col).keep.x);
    }
}

/// The keys go round the places (「次の領域へ」 / 「前の領域へ」), as the key or the palette runs it.
fn round(s: &mut Studio, g: &Geometry, way: &str) -> bool {
    let args = json!({ "way": way });
    s.key_more(g, "view.focus", args.as_object().unwrap()) == Some(true)
}
