use serde_json::json;
use xirius_core::id::TrackId;

use super::*;
use crate::mixer::List;
use crate::studio::{DrawerTab, Geometry};

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
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

/// Keys as two layers with an effect in the second, then a reverb on the track.
fn keys(s: &mut Studio) -> (TrackId, [PluginId; 4]) {
    let t = track(s, "Keys");
    for name in ["Piano", "Pad"] {
        assert!(s.run_command("rack.layer.add", json!({"track":t,"name":name})));
    }
    let (layer, a, b) = match &s.project().tracks[&t].rack.as_ref().unwrap().source {
        Some(Source::Multi { layers, .. }) => {
            (layers[1].id, layers[0].instrument, layers[1].instrument)
        }
        _ => unreachable!(),
    };
    let chorus = add(s, t, "Chorus");
    assert!(s.run_command("rack.node.move", json!({"id":chorus,"to":layer})));
    let reverb = add(s, t, "Reverb");
    (t, [a, b, chorus, reverb])
}

/// Vocal with EQ, then a splitter: Comp in one branch, Reverb at -6 dB in the other.
fn vocal(s: &mut Studio) -> (TrackId, NodeId, [PluginId; 3]) {
    let t = track(s, "Vocal");
    let eq = add(s, t, "EQ");
    let comp = add(s, t, "Comp");
    let verb = add(s, t, "Reverb");
    assert!(s.run_command("rack.split.add", json!({"track":t})));
    let (sp, b) = match &s.project().tracks[&t].rack.as_ref().unwrap().fx[..] {
        [
            RackNode::Plugin(_),
            RackNode::Plugin(_),
            RackNode::Plugin(_),
            RackNode::Split(x),
        ] => (x.id, [x.branches[0].id, x.branches[1].id]),
        other => panic!("{other:?}"),
    };
    assert!(s.run_command("rack.node.move", json!({"id":comp,"to":b[0]})));
    assert!(s.run_command("rack.node.move", json!({"id":verb,"to":b[1]})));
    assert!(s.run_command("rack.branch.set", json!({"id":b[1],"gain":-6.0})));
    (t, sp, [eq, comp, verb])
}

fn rows(s: &Studio, t: TrackId) -> Vec<Line> {
    lines(&s.project().tracks[&t], &s.mixer.folded)
        .into_iter()
        .filter(|l| !matches!(l.kind, Kind::Head | Kind::Add))
        .collect()
}

fn flow_of(s: &Studio, g: &Geometry, t: TrackId) -> Flow {
    let strip = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .unwrap();
    s.strip_flow(&strip, &s.project().tracks[&t])
}

#[test]
fn a_multi_lists_its_layers_with_their_own_effects_stepped_in_under_them() {
    let (mut s, _) = studio();
    let (t, [piano, pad, chorus, reverb]) = keys(&mut s);
    let got: Vec<_> = rows(&s, t)
        .iter()
        .map(|l| (l.area, l.kind, l.depth, l.under))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                Area::Instrument,
                Kind::Instrument {
                    id: piano,
                    layer: Some(0)
                },
                0,
                None
            ),
            (
                Area::Instrument,
                Kind::Instrument {
                    id: pad,
                    layer: Some(1)
                },
                0,
                None
            ),
            (Area::Instrument, Kind::Effect(chorus), 1, Some(pad)),
            (Area::Insert, Kind::Effect(reverb), 0, None),
        ]
    );
    let audio = lines(&s.project().tracks[&track(&s, "Guitar")], &HashSet::new());
    assert!(audio.iter().all(|l| l.area == Area::Insert));
}

#[test]
fn a_splitter_is_one_row_that_opens_on_its_branches_and_folds_away() {
    let (mut s, g) = studio();
    let (t, sp, [eq, comp, verb]) = vocal(&mut s);
    let got: Vec<_> = rows(&s, t)
        .iter()
        .map(|l| (l.kind, l.depth, l.heads, l.level))
        .collect();
    assert_eq!(
        got,
        vec![
            (Kind::Effect(eq), 0, None, None),
            (
                Kind::Split {
                    id: sp,
                    branches: 2,
                    open: true
                },
                0,
                None,
                None
            ),
            (Kind::Effect(comp), 1, Some(sp), None),
            (Kind::Effect(verb), 1, Some(sp), Some((-6.0, false))),
        ]
    );
    // the hooks from the splitter to each branch keep the structure line's rules
    let flow = flow_of(&s, &g, t);
    let hooks = pieces(&flow);
    assert_eq!(hooks.len(), 2);
    assert!(hooks.iter().all(|p| StructureLine::lawful(p)));

    let fold = MixerHit::Fold(t, sp);
    let (x, y) = s.mixer_center(&g, fold).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(fold));
    assert!(s.press(&g, Hit::Mixer(fold)));
    assert!(s.mixer.folded.contains(&sp));
    assert_eq!(
        rows(&s, t).iter().map(|l| l.kind).collect::<Vec<_>>(),
        vec![
            Kind::Effect(eq),
            Kind::Split {
                id: sp,
                branches: 2,
                open: false
            }
        ]
    );
    assert!(pieces(&flow_of(&s, &g, t)).is_empty());
    // a splitter that is gone is forgotten
    assert!(s.run_command("rack.flatten", json!({"track":t})));
    assert!(!s.mixer.folded.contains(&sp));
}

#[test]
fn the_dot_bypasses_and_the_name_opens() {
    let (mut s, g) = studio();
    let t = track(&s, "Bass");
    let delay = add(&mut s, t, "Delay");
    let bypass = MixerHit::Bypass(t, delay);
    let (x, y) = s.mixer_center(&g, bypass).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(bypass));
    assert!(s.press(&g, Hit::Mixer(bypass)));
    assert!(
        s.project().tracks[&t]
            .inserts
            .iter()
            .any(|p| p.id == delay && p.bypass)
    );
    let open = s.mixer_rect(&g, MixerHit::Open(t, delay)).unwrap();
    assert_eq!(
        s.hit(&g, open.x + open.w / 2.0, open.y + open.h / 2.0),
        Hit::Mixer(MixerHit::Open(t, delay))
    );
}

#[test]
fn the_instrument_area_adds_a_layer_once_there_is_an_instrument() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    assert!(s.mixer_rect(&g, MixerHit::AddInstrument(t)).is_some());
    assert!(s.mixer_rect(&g, MixerHit::AddLayer(t)).is_none());
    keys(&mut s);
    assert!(s.mixer_rect(&g, MixerHit::AddInstrument(t)).is_none());
    let flow = flow_of(&s, &g, t);
    let add = flow.add(Area::Instrument).unwrap();
    assert_eq!(
        hit(
            &s,
            &s.project().tracks[&t],
            &flow,
            add.x + add.w / 2.0,
            add.y + 4.0
        ),
        MixerHit::AddLayer(t)
    );
}

#[test]
fn an_effect_let_go_on_a_strip_lands_by_the_row_under_it() {
    let (mut s, g) = studio();
    let t = track(&s, "Bass");
    add(&mut s, t, "EQ");
    let delay = add(&mut s, t, "Delay");
    let track = s.project().tracks[&t].clone();
    let flow = flow_of(&s, &g, t);
    let (_, r) = flow.plugin(delay).unwrap();
    // the upper half of Delay (the second insert) is before it
    assert_eq!(drop_index(&track, &flow, r.y + 2.0), 1);
    assert_eq!(drop_index(&track, &flow, r.bottom() - 2.0), 2);
    let head = flow
        .rows
        .iter()
        .find(|(l, _)| l.area == Area::Insert && l.kind == Kind::Head)
        .unwrap()
        .1;
    assert_eq!(drop_index(&track, &flow, head.y + 2.0), 0);
    assert_eq!(drop_index(&track, &flow, flow.rows[0].1.y + 2.0), 0);
}

#[test]
fn a_long_flow_scrolls_in_its_place_and_brings_a_plugin_into_view() {
    let (mut s, _) = studio();
    s.drawer_size.mixer = None;
    let g = s.geometry(1600.0, 1000.0);
    let t = track(&s, "Guitar");
    let first = add(&mut s, t, "EQ");
    for _ in 0..30 {
        add(&mut s, t, "Comp");
    }
    let last = s.project().tracks[&t].inserts.last().unwrap().id;
    let strip = s
        .mixer_strips(&g)
        .into_iter()
        .find(|x| x.track == t)
        .unwrap();
    let max = s.flow_max(&strip, &s.project().tracks[&t]);
    assert!(max > 0);
    for _ in 0..max + 5 {
        s.mixer_wheel(
            &g,
            strip.inserts.x + 30.0,
            strip.inserts.y + 10.0,
            0.0,
            -1.0,
        );
    }
    assert_eq!(s.mixer.offset(t, List::Flow), max);
    assert_eq!(
        s.mixer_strips(&g).into_iter().find(|x| x.track == t),
        Some(strip)
    );
    assert!(s.mixer_rect(&g, MixerHit::Open(t, last)).is_some());
    assert!(s.mixer_rect(&g, MixerHit::Open(t, first)).is_none());
    s.reveal_in_flow(&g, t, first);
    assert!(s.mixer.offset(t, List::Flow) < max);
    assert!(s.mixer_rect(&g, MixerHit::Open(t, first)).is_some());
}

#[test]
fn the_screen_reader_names_the_splitter_and_the_plus_of_each_area() {
    let (mut s, g) = studio();
    let (t, sp, _) = vocal(&mut s);
    let (k, _) = keys(&mut s);
    let marks = crate::a11y::marks(&s, &g);
    let find = |key: String| {
        marks
            .iter()
            .find(|m| m.key == key)
            .unwrap_or_else(|| panic!("no {key}"))
    };
    let split = find(format!("flow-{t}-{:?}", MixerHit::Fold(t, sp)));
    assert_eq!(split.node.label(), Some("スプリッター"));
    assert_eq!(split.node.is_expanded(), Some(true));
    let plus = find(format!("flow-{t}-{:?}", MixerHit::AddInsert(t)));
    assert_eq!(plus.node.label(), Some("プラグインを追加"));
    let layer = find(format!("flow-{k}-{:?}", MixerHit::AddLayer(k)));
    assert_eq!(layer.node.label(), Some("レイヤーを追加"));
    s.mixer.folded.insert(sp);
    let marks = crate::a11y::marks(&s, &g);
    let split = marks
        .iter()
        .find(|m| m.key == format!("flow-{t}-{:?}", MixerHit::Fold(t, sp)))
        .unwrap();
    assert_eq!(split.node.is_expanded(), Some(false));
}

#[test]
fn at_the_narrow_width_a_hovered_or_picked_branch_row_keeps_its_whole_name() {
    let (mut s, g) = studio();
    let (t, _, [_, comp, verb]) = vocal(&mut s);
    let room = |s: &Studio, id: PluginId| {
        let flow = flow_of(s, &g, t);
        let (l, r) = flow.plugin(id).unwrap();
        assert!(r.w <= crate::mixer::STRIP_W);
        let mute = shows_mute(s, t, l);
        let level = if mute || picked(s, t, l) {
            0.0
        } else {
            l.level.map_or(0.0, |v| {
                crate::browser::text_width(&level_text(v), FontSize::Micro)
            })
        };
        name_part(*r, l, false, level, false, mute).w
    };
    let fits = |w: f32, name: &str| w >= crate::browser::text_width(name, FontSize::Small);
    // under the pointer the M takes the close's place
    s.hover = crate::studio::Hit::Mixer(MixerHit::Open(t, comp));
    assert!(shows_mute(
        &s,
        t,
        &flow_of(&s, &g, t).plugin(comp).unwrap().0.clone()
    ));
    assert!(fits(room(&s, comp), "Comp"), "{}", room(&s, comp));
    let flow = flow_of(&s, &g, t);
    let (l, r) = flow.plugin(comp).unwrap();
    assert_eq!(mute_part(*r, l).right(), remove_part(item(*r, l)).right());
    // the picked row leaves its level to the row under it
    s.hover = crate::studio::Hit::None;
    s.mixer.chosen = Some((t, Pick::Plugin(verb)));
    assert!(fits(room(&s, verb), "Reverb"), "{}", room(&s, verb));
}
