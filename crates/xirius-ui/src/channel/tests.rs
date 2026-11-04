use serde_json::json;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::rack::{RackNode, SplitMode};

use super::*;
use crate::instrument::Pick as Add;
use crate::studio::{Geometry, Studio};

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.instruments.open.clear();
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}

fn add(s: &mut Studio, t: TrackId, name: &str) -> PluginId {
    assert!(s.run_command("plugin.add", json!({"track":t,"name":name})));
    s.project().tracks[&t].inserts.last().unwrap().id
}

/// Vocal with EQ, then a splitter: Comp in one branch, Reverb at -6 dB in the other.
fn vocal(s: &mut Studio) -> (TrackId, NodeId, [PluginId; 3], [NodeId; 2]) {
    let t = track(s, "Vocal");
    let eq = add(s, t, "EQ");
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
    (t, sp, [eq, comp, verb], b)
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn card(v: &View, pick: Pick) -> flow::Card {
    v.flow
        .cards
        .iter()
        .find(|c| c.pick == pick)
        .cloned()
        .unwrap()
}

fn column(v: &View, id: NodeId) -> Column {
    v.flow.columns.iter().find(|c| c.id == id).cloned().unwrap()
}

fn split(s: &Studio, t: TrackId, id: NodeId) -> Split {
    split_of(&s.project(), t, id).unwrap()
}

fn press(s: &mut Studio, g: &Geometry, (x, y): (f32, f32)) {
    assert!(s.channel_press(g, x, y, 1));
    s.channel_up(g);
}

#[test]
fn it_opens_for_a_track_and_a_pinned_one_stays() {
    let (mut s, _) = studio();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    s.open_channel(vocal);
    assert_eq!(s.channel.as_ref().unwrap().track, vocal);
    s.open_channel(keys);
    assert_eq!(s.channel.as_ref().unwrap().track, keys);
    s.channel.as_mut().unwrap().pinned = true;
    s.channel.as_mut().unwrap().top = false;
    s.open_channel(vocal);
    let w = s.channel.as_ref().unwrap();
    assert_eq!(w.track, keys, "the pin keeps it");
    assert!(w.top, "and brings it up");
}

#[test]
fn the_mixer_and_a_device_window_open_it() {
    let (mut s, _) = studio();
    let vocal = track(&s, "Vocal");
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    let g = s.geometry(1600.0, 1000.0);
    assert!(s.mixer_press(&g, crate::mixer::MixerHit::Editor(vocal)));
    assert_eq!(s.channel.as_ref().map(|w| w.track), Some(vocal));
}

#[test]
fn it_sits_in_the_middle_and_wraps_its_tools_when_narrow() {
    let (mut s, g) = studio();
    vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    assert!(v.r.w >= MIN_W);
    assert_eq!(v.r.x, (g.size.0 - v.r.w) / 2.0);
    let ys: Vec<f32> = v.tools.iter().map(|(_, r)| r.y).collect();
    assert!(ys.iter().all(|y| *y == ys[0]), "one row: {ys:?}");
    assert_eq!(v.toolbar.h, TOOL_H);

    let sp = s
        .channel_view(&g)
        .unwrap()
        .flow
        .cards
        .iter()
        .find_map(|c| match c.pick {
            Pick::Split(id) => Some(id),
            Pick::Plugin(_) => None,
        });
    assert!(s.run_command("rack.branch.add", json!({"split": sp.unwrap()})));
    let narrow = s.geometry(360.0, 800.0);
    let v = s.channel_view(&narrow).unwrap();
    assert_eq!(v.r.w, 360.0 - 2.0 * SIDE);
    let y = |tool| v.tools.iter().find(|(t, _)| *t == tool).unwrap().1;
    assert!(y(Tool::Macros).y > y(Tool::Insert).y, "the macros go under");
    assert_eq!(y(Tool::Macros).x, v.r.x + TOOL_PAD);
    assert!(v.toolbar.h > TOOL_H);
    // a flow wider than the window shows from its left
    assert!(v.flow.w > v.body.w);
    let left = v
        .flow
        .ios
        .iter()
        .map(|io| io.rect.x)
        .fold(f32::MAX, f32::min);
    assert!(left >= v.body.x, "{left} {}", v.body.x);
}

#[test]
fn only_an_instrument_track_has_the_instrument_button() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    s.open_channel(keys);
    let v = s.channel_view(&g).unwrap();
    assert!(v.tools.iter().any(|(t, _)| *t == Tool::Instrument));
    vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    assert!(!v.tools.iter().any(|(t, _)| *t == Tool::Instrument));
}

#[test]
fn a_card_dragged_into_a_branch_moves_there() {
    let (mut s, g) = studio();
    let (t, _, [eq, comp, _], b) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let (x, y) = mid(card(&v, Pick::Plugin(eq)).rect);
    let slot = *v
        .flow
        .slots
        .iter()
        .find(|sl| sl.chain == Chain::Branch(b[0]) && sl.index == 0)
        .unwrap();
    assert!(s.channel_press(&g, x, y, 1));
    s.channel_moved(&g, x + 10.0, y);
    let (mx, my) = mid(slot.mark);
    s.channel_moved(&g, mx, my);
    match &s.channel.as_ref().unwrap().grab {
        Some(Grab::Card {
            moved: true, to, ..
        }) => assert_eq!(*to, Some(slot)),
        other => panic!("{other:?}"),
    }
    assert!(s.channel_up(&g));
    let sp = s.project().tracks[&t].rack.clone().unwrap().fx;
    match &sp[..] {
        [RackNode::Split(x)] => {
            let ids: Vec<_> = x.branches[0]
                .fx
                .iter()
                .map(|n| match n {
                    RackNode::Plugin(p) => *p,
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(ids, vec![eq, comp]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_card_let_go_where_it_was_stays() {
    let (mut s, g) = studio();
    let (t, _, [eq, ..], _) = vocal(&mut s);
    let before = s.project().tracks[&t].rack.clone();
    let v = s.channel_view(&g).unwrap();
    let (x, y) = mid(card(&v, Pick::Plugin(eq)).rect);
    assert!(s.channel_press(&g, x, y, 1));
    s.channel_moved(&g, x + 2.0, y);
    s.channel_up(&g);
    assert_eq!(s.project().tracks[&t].rack, before);
    assert_eq!(s.channel.as_ref().unwrap().chosen, Some(Pick::Plugin(eq)));
}

#[test]
fn a_splitter_does_not_go_into_its_own_branches() {
    let (mut s, g) = studio();
    let (_, sp, _, b) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    for slot in v
        .flow
        .slots
        .iter()
        .filter(|sl| sl.chain == Chain::Branch(b[1]))
    {
        let (x, y) = mid(slot.mark);
        let got = slot_near(&v.flow, Pick::Split(sp), x, y);
        assert!(
            !matches!(
                got,
                Some(Slot {
                    chain: Chain::Branch(_),
                    ..
                })
            ),
            "{got:?}"
        );
    }
}

#[test]
fn a_drop_counts_the_card_it_leaves() {
    let slot = |index| Slot {
        chain: Chain::Top,
        index,
        mark: Rect::new(0.0, 0.0, 1.0, 1.0),
    };
    let from = Some((Chain::Top, 1));
    assert_eq!(drop_index(from, &slot(1)), None);
    assert_eq!(drop_index(from, &slot(2)), None);
    assert_eq!(drop_index(from, &slot(0)), Some(0));
    assert_eq!(drop_index(from, &slot(3)), Some(2));
    assert_eq!(
        drop_index(Some((Chain::Layer(NodeId(999)), 0)), &slot(0)),
        Some(0)
    );
}

#[test]
fn a_gap_adds_the_effect_in_its_place() {
    let (mut s, g) = studio();
    let (t, sp, [_, comp, _], b) = vocal(&mut s);
    // every branch's last part has its ＋ after it
    let v = s.channel_view(&g).unwrap();
    assert!(v.flow.gaps.iter().any(|gp| gp.chain == Chain::Branch(b[0])
        && gp.index == 1
        && gp.after == Some(Pick::Plugin(comp))));
    // an empty branch's ＋ adds its first
    assert!(s.run_command("rack.branch.add", json!({"split": sp})));
    let third = split(&s, t, sp).branches[2].id;
    let v = s.channel_view(&g).unwrap();
    let (i, gap) = v
        .flow
        .gaps
        .iter()
        .enumerate()
        .find(|(_, gp)| gp.chain == Chain::Branch(third) && gp.index == 0 && gp.after.is_none())
        .map(|(i, gp)| (i, *gp))
        .expect("the empty branch's gap");
    assert_eq!(
        part_at(&v, mid(gap.button).0, mid(gap.button).1),
        Some(Part::Gap(i))
    );
    press(&mut s, &g, mid(gap.button));
    assert_eq!(s.mixer.insert_into, Some((t, Chain::Branch(third), 0)));
    assert!(s.popup.is_some());
    s.popup = None;
    let ids = |s: &Studio| -> Vec<PluginId> {
        s.project().tracks[&t]
            .inserts
            .iter()
            .map(|p| p.id)
            .collect()
    };
    let new = |s: &Studio, before: &[PluginId]| {
        ids(s).into_iter().find(|id| !before.contains(id)).unwrap()
    };
    let before = ids(&s);
    assert!(s.add_effect_into(t, Chain::Branch(third), 0, &Add::built_in("eq")));
    let added = new(&s, &before);
    assert_eq!(
        split(&s, t, sp).branches[2].fx,
        vec![RackNode::Plugin(added)]
    );
    // into a branch with parts, at its head
    let before = ids(&s);
    assert!(s.add_effect_into(t, Chain::Branch(b[0]), 0, &Add::built_in("eq")));
    let first = new(&s, &before);
    assert_eq!(
        split(&s, t, sp).branches[0].fx,
        vec![RackNode::Plugin(first), RackNode::Plugin(comp)]
    );
    // one step undoes it
    assert!(s.undo(&g));
    assert_eq!(
        split(&s, t, sp).branches[0].fx,
        vec![RackNode::Plugin(comp)]
    );
}

#[test]
fn a_branch_head_has_a_plus_on_its_line_that_adds_its_first() {
    let (mut s, g) = studio();
    let (t, _, [_, comp, _], b) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let col = column(&v, b[0]);
    let (i, gap) = v
        .flow
        .gaps
        .iter()
        .enumerate()
        .find(|(_, gp)| gp.chain == Chain::Branch(b[0]) && gp.index == 0)
        .map(|(i, gp)| (i, *gp))
        .expect("the head's ＋");
    assert_eq!(gap.after, None);
    // In the head's middle, on the line, clear of the level, its value, M and ×.
    let (bx, by) = mid(gap.button);
    assert!(col.head.contains(bx, by));
    assert!((bx - head_mid(col.head)).abs() < 0.5);
    let hp = head_parts(&col);
    for r in [hp.num, hp.level, hp.value, hp.mute]
        .into_iter()
        .chain(hp.remove)
    {
        assert!(
            r.right() <= gap.button.x || r.x >= gap.button.right(),
            "{r:?} {:?}",
            gap.button
        );
    }
    assert_eq!(part_at(&v, bx, by), Some(Part::Gap(i)));
    // A touch on the branch's first card shows it; the press opens the list into the head.
    s.channel.as_mut().unwrap().chosen = Some(Pick::Plugin(comp));
    assert!(v.flow.beside(Pick::Plugin(comp), &gap));
    press(&mut s, &g, (bx, by));
    assert_eq!(s.mixer.insert_into, Some((t, Chain::Branch(b[0]), 0)));
    assert!(matches!(
        s.popup.as_ref().map(|p| p.kind),
        Some(crate::popup::PopupKind::Insert(_))
    ));
}

#[test]
fn a_head_sets_the_level_mutes_and_takes_the_branch_away() {
    let (mut s, g) = studio();
    let (t, sp, _, b) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let col = column(&v, b[1]);
    let hp = head_parts(&col);
    // the head shows its parts under the pointer, and keeps -6.0 at rest
    s.channel_hover(&g, mid(col.rect).0, mid(col.rect).1);
    assert!(head_shown(s.channel.as_ref().unwrap(), &col, &v.flow, None));
    s.channel_hover(&g, 2.0, 2.0);
    assert!(!head_shown(
        s.channel.as_ref().unwrap(),
        &col,
        &v.flow,
        None
    ));

    assert!(s.channel_press(&g, hp.level.right() - 1.0, mid(hp.level).1, 1));
    assert!(s.mixer.level_drag.is_some());
    assert!(s.level_up());
    assert!(split(&s, t, sp).branches[1].gain_db > -6.0);

    press(&mut s, &g, mid(hp.mute));
    assert!(split(&s, t, sp).branches[1].mute);

    assert!(s.run_command("rack.branch.add", json!({"split": sp})));
    let v = s.channel_view(&g).unwrap();
    let third = split(&s, t, sp).branches[2].id;
    let r = head_parts(&column(&v, third)).remove.unwrap();
    press(&mut s, &g, mid(r));
    assert_eq!(split(&s, t, sp).branches.len(), 2);
}

#[test]
fn a_channel_split_names_its_branches_l_and_r_and_keeps_them() {
    let (mut s, g) = studio();
    let (_, sp, _, b) = vocal(&mut s);
    assert!(s.run_command("rack.split.mode", json!({"id": sp, "mode": "channel"})));
    let v = s.channel_view(&g).unwrap();
    assert_eq!(col_label(&column(&v, b[0])), "L");
    let hp = head_parts(&column(&v, b[1]));
    assert_eq!(hp.label, "R");
    assert!(hp.remove.is_none());
}

#[test]
fn a_crossover_dragged_sets_its_frequency_once() {
    let (mut s, g) = studio();
    let (t, sp, _, _) = vocal(&mut s);
    assert!(s.run_command("rack.split.mode", json!({"id": sp, "mode": "frequency"})));
    let from = split(&s, t, sp).crossovers[0];
    let v = s.channel_view(&g).unwrap();
    let (x, y) = mid(v.flow.crosses[0].rect);
    assert_eq!(part_at(&v, x, y), Some(Part::Cross(sp, 0)));
    assert!(s.channel_press(&g, x, y, 1));
    s.channel_moved(&g, x + 12.0, y);
    s.channel_moved(&g, x + 24.0, y);
    let steps = s.project().tracks.len();
    s.channel_up(&g);
    let now = split(&s, t, sp).crossovers[0];
    assert!((now - (from * 2.0).round()).abs() < 1.0, "{from} → {now}");
    assert_eq!(s.project().tracks.len(), steps);
}

#[test]
fn a_crossover_stays_between_its_neighbours() {
    let from = [200.0, 2000.0, 8000.0];
    assert_eq!(crossed(&from, 1, 240.0)[1], (8000.0f64 / 1.05).round());
    assert_eq!(crossed(&from, 1, -240.0)[1], (200.0f64 * 1.05).round());
    assert_eq!(crossed(&from, 0, -480.0)[0], 20.0);
}

#[test]
fn the_splitter_button_adds_one_after_the_chosen_card() {
    let (mut s, g) = studio();
    let (t, sp, [eq, ..], _) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    press(&mut s, &g, mid(card(&v, Pick::Plugin(eq)).rect));
    let tool = v
        .tools
        .iter()
        .find(|(x, _)| *x == Tool::Splitter)
        .unwrap()
        .1;
    press(&mut s, &g, mid(tool));
    let fx = s.project().tracks[&t].rack.clone().unwrap().fx;
    assert!(
        matches!(&fx[..], [RackNode::Plugin(a), RackNode::Split(x), RackNode::Split(y)]
        if *a == eq && x.id != sp && y.id == sp),
        "{fx:?}"
    );
}

#[test]
fn the_dot_bypasses_and_the_menu_renames() {
    let (mut s, g) = studio();
    let (t, _, [eq, ..], _) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let c = card(&v, Pick::Plugin(eq));
    let cp = card_parts(&c);
    press(&mut s, &g, mid(cp.dot.unwrap()));
    let p = |s: &Studio| {
        s.project().tracks[&t]
            .inserts
            .iter()
            .find(|p| p.id == eq)
            .cloned()
            .unwrap()
    };
    assert!(p(&s).bypass);
    press(&mut s, &g, mid(cp.more));
    assert!(s.ctx.is_some(), "the card's menu");
    assert!(s.run_command("plugin.rename", json!({"id": eq, "name": "Bright"})));
    assert_eq!(p(&s).name, "Bright");
}

#[test]
fn the_mode_pill_lists_the_three_modes() {
    let (mut s, g) = studio();
    let (_, sp, _, _) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let pill = card_parts(&card(&v, Pick::Split(sp))).mode.unwrap();
    assert_eq!(part_at(&v, mid(pill).0, mid(pill).1), Some(Part::Mode(sp)));
    press(&mut s, &g, mid(pill));
    let popup = s.popup.as_ref().expect("the mode's list");
    assert!(crate::pick_list::is(popup.kind), "a PickList (UI-320)");
    let rows: Vec<_> = crate::pick_list::found(popup).cloned().collect();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].on, "a new splitter is normal");
    let act = rows[1].act.clone().unwrap();
    assert_eq!(crate::operations::of_action(&act), "rack.split.mode");
    assert!(s.popup_action(&g, act));
    assert!(s.popup.is_none());
    let mode = super::split_of(&s.project(), track(&s, "Vocal"), sp).map(|x| x.mode);
    assert_eq!(mode, Some(SplitMode::Channel));
}

#[test]
fn a_built_in_opens_in_its_window_over_the_editor() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    s.run_command(
        "plugin.add",
        json!({ "track": t, "name": "eq", "kind": "effect", "format": "builtin", "uid": "eq" }),
    );
    let eq = s.project().tracks[&t].inserts.last().unwrap().id;
    s.open_channel(t);
    let v = s.channel_view(&g).unwrap();
    let (x, y) = mid(card(&v, Pick::Plugin(eq)).rect);
    assert!(s.channel_press(&g, x, y, 2));
    assert!(s.instruments.open.iter().any(|w| w.plugin == eq));
    assert!(!s.channel.as_ref().unwrap().top);
}

#[test]
fn the_close_and_the_bar_drag_work() {
    let (mut s, g) = studio();
    vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let (x, y) = mid(v.bar.name);
    assert!(s.channel_press(&g, x, y, 1));
    s.channel_moved(&g, x + 40.0, y + 30.0);
    s.channel_up(&g);
    let after = s.channel_view(&g).unwrap();
    assert_eq!((after.r.x, after.r.y), (v.r.x + 40.0, v.r.y + 30.0));
    press(&mut s, &g, mid(after.bar.close));
    assert!(s.channel.is_none());
}

#[test]
fn the_wheel_scrolls_a_tall_flow() {
    let (mut s, _) = studio();
    vocal(&mut s);
    let g = s.geometry(1600.0, 360.0);
    let v = s.channel_view(&g).unwrap();
    assert!(v.flow.h > v.body.h);
    let (x, y) = mid(v.body);
    assert!(s.channel_wheel(&g, x, y, 0.0, -40.0));
    assert_eq!(s.channel.as_ref().unwrap().scroll.1, 40.0);
    assert!(s.channel_wheel(&g, x, y, 0.0, -4000.0));
    assert_eq!(s.channel.as_ref().unwrap().scroll.1, v.flow.h - v.body.h);
}

#[test]
fn a_reader_hears_the_window_its_cards_and_its_heads() {
    let (mut s, g) = studio();
    vocal(&mut s);
    let marks = marks(&s, &g);
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].node.label(), Some("チャンネルエディター Vocal"));
    let labels: Vec<String> = marks[0]
        .children
        .iter()
        .map(|m| m.node.label().unwrap_or_default().to_string())
        .collect();
    for want in [
        "Insert",
        "Splitter",
        "Macro Controls",
        POPOUT_LABEL,
        "EQ",
        "EQ の電源",
        "スプリッター",
        "分け方",
        "分岐 1 の音量",
        "分岐 2 をミュート",
        "分岐 2 を外す",
    ] {
        assert!(labels.iter().any(|l| l == want), "{want} in {labels:?}");
    }
}

#[test]
fn a_reader_hears_a_layer_by_its_instrument() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    for name in ["Piano", "Strings", "Pad"] {
        assert!(s.run_command("rack.layer.add", json!({"track":t,"name":name})));
    }
    s.open_channel(t);
    let labels: Vec<String> = marks(&s, &g)[0]
        .children
        .iter()
        .map(|m| m.node.label().unwrap_or_default().to_string())
        .collect();
    assert!(
        labels.iter().any(|l| l == "レイヤー Pad の音量"),
        "{labels:?}"
    );
}

#[test]
fn a_split_mode_has_its_words() {
    assert_eq!(mode_text(SplitMode::Normal), "ノーマル");
    assert_eq!(mode_text(SplitMode::Frequency), "周波数分割");
}

/// Keys as a multi-instrument of three layers: Piano, Strings and Pad.
fn multi(s: &mut Studio) -> (TrackId, Vec<xirius_core::rack::Layer>) {
    let t = track(s, "Keys");
    for name in ["Piano", "Strings", "Pad"] {
        assert!(s.run_command("rack.layer.add", json!({"track":t,"name":name})));
    }
    s.open_channel(t);
    let layers = keys::layers(&s.project().tracks[&t]).to_vec();
    (t, layers)
}

#[test]
fn a_multi_instrument_has_the_key_figure_when_the_window_is_wide() {
    let (mut s, g) = studio();
    let (_, layers) = multi(&mut s);
    let v = s.channel_view(&g).unwrap();
    assert!(v.r.w >= keys::LEAST_W, "{}", v.r.w);
    let k = v.keys.expect("the figure");
    assert_eq!(k.boxes.len(), layers.len());
    assert!(k.panel.y >= v.body.bottom() - 0.5);
    assert!(k.panel.bottom() <= v.r.bottom() + 0.5);
    // A full layer's box takes the whole map.
    assert!((k.boxes[0].rect.w - k.map.w).abs() < 0.5);
    let narrow = s.geometry(700.0, 1000.0);
    assert!(s.channel_view(&narrow).unwrap().keys.is_none());
    // A track of one instrument has none.
    let vocal = track(&s, "Vocal");
    s.channel = Some(ChannelWin::new(vocal));
    assert!(s.channel_view(&g).unwrap().keys.is_none());
}

#[test]
fn a_box_pressed_chooses_its_layer() {
    let (mut s, g) = studio();
    let (t, layers) = multi(&mut s);
    let id = layers[2].id.to_string();
    assert!(s.run_command(
        "rack.layer.set",
        json!({"id": id, "keys": "60-72", "velocity": "80-127"})
    ));
    let v = s.channel_view(&g).unwrap();
    let k = v.keys.unwrap();
    assert_eq!(k.chosen, 0);
    let b = &k.boxes[2];
    press(
        &mut s,
        &g,
        (b.rect.x + b.rect.w / 2.0, b.rect.y + b.rect.h / 2.0),
    );
    assert_eq!(
        s.channel.as_ref().unwrap().chosen,
        Some(Pick::Plugin(layers[2].instrument))
    );
    assert_eq!(s.channel_view(&g).unwrap().keys.unwrap().chosen, 2);
    // A card in a layer's chain chooses that layer too.
    let eq = add(&mut s, t, "EQ");
    assert!(s.run_command("rack.node.move", json!({"id":eq,"to":layers[1].id})));
    s.channel.as_mut().unwrap().chosen = Some(Pick::Plugin(eq));
    assert_eq!(s.channel_view(&g).unwrap().keys.unwrap().chosen, 1);
}

#[test]
fn a_side_dragged_sets_the_layer_in_one_step() {
    let (mut s, g) = studio();
    let (t, layers) = multi(&mut s);
    let k = s.channel_view(&g).unwrap().keys.unwrap();
    let b = k.boxes[0].clone();
    let high = keys::grip(&b, keys::Edge::High);
    let (x, y) = mid(high);
    assert!(s.channel_press(&g, x, y, 1));
    s.channel_moved(&g, keys::key_x(k.map, k.span, 80.0), y);
    s.channel_moved(&g, keys::key_x(k.map, k.span, 60.0), y);
    // The box follows the pointer before it is let go; the layer does not change yet.
    let held = s.channel_view(&g).unwrap().keys.unwrap();
    assert_eq!(held.boxes[0].ranges.0, (0, 59));
    assert_eq!(keys::layers(&s.project().tracks[&t])[0].keys, (0, 127));
    s.channel_up(&g);
    assert_eq!(keys::layers(&s.project().tracks[&t])[0].keys, (0, 59));
    let loud = keys::grip(
        &s.channel_view(&g).unwrap().keys.unwrap().boxes[0],
        keys::Edge::Loud,
    );
    let (x, y) = mid(loud);
    assert!(s.channel_press(&g, x, y, 1));
    s.channel_moved(&g, x, keys::vel_y(k.map, 100.0));
    s.channel_up(&g);
    let l = keys::layers(&s.project().tracks[&t])[0].clone();
    assert_eq!((l.keys, l.velocity), ((0, 59), (1, 100)));
    s.undo(&g);
    assert_eq!(keys::layers(&s.project().tracks[&t])[0].velocity, (1, 127));
    assert_eq!(keys::layers(&s.project().tracks[&t])[0].keys, (0, 59));
    assert_eq!(layers[0].id, keys::layers(&s.project().tracks[&t])[0].id);
}

#[test]
fn a_side_does_not_pass_the_other() {
    let map = Rect::new(0.0, 0.0, 128.0, 127.0);
    let from = ((40, 50), (20, 90));
    assert_eq!(
        keys::dragged(from, keys::Edge::Low, map, (0.0, 128.0), 100.0, 0.0),
        ((50, 50), (20, 90))
    );
    assert_eq!(
        keys::dragged(from, keys::Edge::High, map, (0.0, 128.0), 0.0, 0.0),
        ((40, 40), (20, 90))
    );
    assert_eq!(
        keys::dragged(from, keys::Edge::Loud, map, (0.0, 128.0), 0.0, 127.0),
        ((40, 50), (20, 20))
    );
    assert_eq!(
        keys::dragged(from, keys::Edge::Soft, map, (0.0, 128.0), 0.0, 0.0),
        ((40, 50), (90, 90))
    );
    assert_eq!(
        keys::dragged(from, keys::Edge::Soft, map, (0.0, 128.0), 0.0, 200.0),
        ((40, 50), (1, 90))
    );
    assert_eq!(
        keys::dragged(from, keys::Edge::High, map, (0.0, 128.0), 999.0, 0.0),
        ((40, 127), (20, 90))
    );
}

#[test]
fn the_figure_shows_the_octaves_the_layers_take() {
    let (mut s, _) = studio();
    let (t, layers) = multi(&mut s);
    assert_eq!(keys::span(&layers), (0.0, 128.0));
    for (l, keys) in layers.iter().zip(["E0-B1", "C2-C6", "C2-C6"]) {
        assert!(s.run_command(
            "rack.layer.set",
            json!({"id": l.id.to_string(), "keys": keys})
        ));
    }
    // C0 to C6, as the mock shows them.
    let layers = keys::layers(&s.project().tracks[&t]).to_vec();
    assert_eq!(keys::span(&layers), (24.0, 73.0));
    // Six octaves at the least, and no key past 127.
    let mut high = layers.clone();
    for l in &mut high {
        l.keys = (120, 125);
    }
    assert_eq!(keys::span(&high), (48.0, 80.0));
    let board = Rect::new(0.0, 0.0, 730.0, 64.0);
    let (whites, blacks) = keys::key_rects(board, (24.0, 73.0));
    assert_eq!((whites.len(), blacks.len()), (43, 30));
    assert_eq!(whites[0], (24, Rect::new(0.0, 0.0, 15.0, 64.0)));
    assert!((whites.last().unwrap().1.right() - 730.0).abs() < 0.01);
}

#[test]
fn a_reader_hears_each_layers_range_and_its_sides() {
    let (mut s, g) = studio();
    let (_, layers) = multi(&mut s);
    assert!(s.run_command(
        "rack.layer.set",
        json!({"id": layers[0].id.to_string(), "keys": "C2-C6", "velocity": "1-100"})
    ));
    let marks = marks(&s, &g);
    let find = |label: &str| {
        marks[0]
            .children
            .iter()
            .find(|m| m.node.label() == Some(label))
            .map(|m| m.node.clone())
            .unwrap_or_else(|| panic!("{label}"))
    };
    assert_eq!(find("Piano の範囲").value(), Some("C2 – C6 · 1 – 100"));
    assert_eq!(find("Piano の下の鍵").value(), Some("C2"));
    assert_eq!(find("Piano のベロシティの上").numeric_value(), Some(100.0));
}

#[test]
fn a_pinned_editor_whose_track_goes_closes_and_others_open_again() {
    let (mut s, _) = studio();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    s.open_channel(vocal);
    s.channel.as_mut().unwrap().pinned = true;
    assert!(s.run_command("track.delete", json!({"ids":[vocal]})));
    assert!(s.channel.is_none(), "its track went");
    s.open_channel(keys);
    assert_eq!(s.channel.as_ref().map(|w| w.track), Some(keys));
}

#[test]
fn another_song_closes_the_editor_and_the_mixers_picks() {
    let (mut s, _) = studio();
    let (t, sp, ..) = vocal(&mut s);
    s.open_channel(t);
    s.mixer.chosen = Some((t, crate::mixer::flow::Pick::Split(sp)));
    s.new_song_sync();
    assert!(s.channel.is_none());
    assert!(s.mixer.chosen.is_none());
}

#[test]
fn the_chosen_card_shows_the_plus_before_and_after_it() {
    let (mut s, g) = studio();
    let (_, _, [eq, comp, _], b) = vocal(&mut s);
    let v = s.channel_view(&g).unwrap();
    let f = &v.flow;
    let beside = |pick: Pick| -> Vec<(Chain, usize)> {
        let mut v: Vec<_> = f
            .gaps
            .iter()
            .filter(|gp| f.beside(pick, gp))
            .map(|gp| (gp.chain, gp.index))
            .collect();
        v.sort_by_key(|x| x.1);
        v.dedup();
        v
    };
    // Comp heads its branch: the ＋ in the branch's head and the one after it
    assert_eq!(
        beside(Pick::Plugin(comp)),
        [(Chain::Branch(b[0]), 0), (Chain::Branch(b[0]), 1)]
    );
    let eq_card = card(&v, Pick::Plugin(eq));
    let (chain, k) = eq_card.chain.unwrap();
    let eq_gaps = beside(Pick::Plugin(eq));
    assert!(eq_gaps.contains(&(chain, k)), "{eq_gaps:?}");
    assert!(eq_gaps.contains(&(chain, k + 1)), "{eq_gaps:?}");
}
