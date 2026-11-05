//! Splitters nested in splitters on the strip (MIX-044): shut at the narrow width, one level
//! opened at a time, all open on a wide strip, and the person's own kept apart.

use super::*;
use crate::mixer::{STRIP_W, WIDE_W};
use crate::tools::Mods;

/// Vocal's splitter with a splitter in its first branch and another in that one's first.
fn nest(s: &mut Studio) -> (TrackId, [NodeId; 3]) {
    fn first(nodes: &[RackNode], id: NodeId) -> Option<NodeId> {
        nodes.iter().find_map(|n| match n {
            RackNode::Split(x) if x.id == id => Some(x.branches[0].id),
            RackNode::Split(x) => x.branches.iter().find_map(|b| first(&b.fx, id)),
            RackNode::Plugin(_) => None,
        })
    }
    let (t, outer, _) = vocal(s);
    let mut ids = vec![outer];
    for _ in 0..2 {
        let rack = s.project().tracks[&t].rack.clone().unwrap();
        let branch = first(&rack.fx, *ids.last().unwrap()).unwrap();
        let before = splits(s, t);
        let at = format!("branch:{branch}:0");
        assert!(s.run_command("rack.split.add", json!({"track":t,"at":at})));
        let new = splits(s, t).into_iter().find(|x| !before.contains(x));
        ids.push(new.unwrap());
    }
    (t, [ids[0], ids[1], ids[2]])
}

/// Every splitter of `t`'s rack.
fn splits(s: &Studio, t: TrackId) -> Vec<NodeId> {
    fn walk(nodes: &[RackNode], out: &mut Vec<NodeId>) {
        for n in nodes {
            if let RackNode::Split(x) = n {
                out.push(x.id);
                for b in &x.branches {
                    walk(&b.fx, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&s.project().tracks[&t].rack.as_ref().unwrap().fx, &mut out);
    out
}

/// The splitter rows `t`'s strip shows, and whether each is open.
fn shown(s: &Studio, g: &Geometry, t: TrackId) -> Vec<(NodeId, bool)> {
    flow_of(s, g, t)
        .rows
        .iter()
        .filter_map(|(l, _)| match l.kind {
            Kind::Split { id, open, .. } => Some((id, open)),
            _ => None,
        })
        .collect()
}

fn width(s: &Studio, g: &Geometry, t: TrackId) -> f32 {
    s.mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .unwrap()
        .rect
        .w
}

fn fold(s: &mut Studio, g: &Geometry, t: TrackId, id: NodeId) {
    let hit = MixerHit::Fold(t, id);
    let (x, y) = s.mixer_center(g, hit).unwrap();
    assert_eq!(s.hit(g, x, y), Hit::Mixer(hit));
    assert!(s.press(g, Hit::Mixer(hit)));
}

/// The button by the strip's name: wide, and narrow again.
fn widen(s: &mut Studio, g: &Geometry, t: TrackId) {
    let hit = MixerHit::Widen(t);
    let (x, y) = s.mixer_center(g, hit).unwrap();
    assert_eq!(s.hit(g, x, y), Hit::Mixer(hit));
    assert!(s.press(g, Hit::Mixer(hit)));
}

#[test]
fn at_the_narrow_width_a_nested_splitter_starts_shut_and_opens_one_level_at_a_time() {
    let (mut s, g) = studio();
    let (t, [outer, inner, deepest]) = nest(&mut s);
    assert_eq!(width(&s, &g, t), STRIP_W);
    // The top splitter is open; the one in its branch is shut, with its count of branches.
    assert_eq!(shown(&s, &g, t), vec![(outer, true), (inner, false)]);
    let flow = flow_of(&s, &g, t);
    let (row, _) = flow
        .rows
        .iter()
        .find(|(l, _)| matches!(l.kind, Kind::Split { id, .. } if id == inner))
        .unwrap();
    assert_eq!(
        row.kind,
        Kind::Split {
            id: inner,
            branches: 2,
            open: false
        }
    );
    assert_eq!(row.depth, 1, "the indent is as it was");
    // A press opens one level: the splitter in it starts shut again.
    fold(&mut s, &g, t, inner);
    assert_eq!(
        shown(&s, &g, t),
        vec![(outer, true), (inner, true), (deepest, false)]
    );
    fold(&mut s, &g, t, deepest);
    assert_eq!(
        shown(&s, &g, t),
        vec![(outer, true), (inner, true), (deepest, true)]
    );
    // Pressed again, it shuts.
    fold(&mut s, &g, t, inner);
    assert_eq!(shown(&s, &g, t), vec![(outer, true), (inner, false)]);
}

#[test]
fn widened_every_splitter_shows_and_at_the_narrow_width_again_the_person_s_own_stay() {
    let (mut s, g) = studio();
    let (t, [outer, inner, deepest]) = nest(&mut s);
    let all = vec![(outer, true), (inner, true), (deepest, true)];
    widen(&mut s, &g, t);
    assert_eq!(width(&s, &g, t), WIDE_W);
    assert_eq!(shown(&s, &g, t), all);
    // Narrow again with nothing pressed: as it started.
    widen(&mut s, &g, t);
    assert_eq!(width(&s, &g, t), STRIP_W);
    assert_eq!(shown(&s, &g, t), vec![(outer, true), (inner, false)]);
    // Opened by the person at the narrow width: open there after the wide strip.
    fold(&mut s, &g, t, inner);
    widen(&mut s, &g, t);
    assert_eq!(shown(&s, &g, t), all);
    widen(&mut s, &g, t);
    assert_eq!(
        shown(&s, &g, t),
        vec![(outer, true), (inner, true), (deepest, false)]
    );
    // Shut by the person at the narrow width: shut there after the wide strip, though the
    // wide strip shows it open.
    fold(&mut s, &g, t, inner);
    widen(&mut s, &g, t);
    assert_eq!(shown(&s, &g, t), all);
    widen(&mut s, &g, t);
    assert_eq!(shown(&s, &g, t), vec![(outer, true), (inner, false)]);
    // The top splitter shut stays shut at both widths, as before.
    fold(&mut s, &g, t, outer);
    assert_eq!(shown(&s, &g, t), vec![(outer, false)]);
    widen(&mut s, &g, t);
    assert_eq!(shown(&s, &g, t), vec![(outer, false)]);
}

#[test]
fn enter_on_a_picked_nested_splitter_opens_it_as_the_press_does() {
    let (mut s, g) = studio();
    let (t, [outer, inner, deepest]) = nest(&mut s);
    s.mixer.chosen = Some((t, Pick::Split(inner)));
    assert!(s.flow_key(&g, "enter", None, Mods::NONE));
    assert_eq!(
        shown(&s, &g, t),
        vec![(outer, true), (inner, true), (deepest, false)]
    );
    // ↓ goes on to the splitter it showed.
    assert!(s.flow_key(&g, "down", None, Mods::NONE));
    assert_eq!(s.mixer.chosen, Some((t, Pick::Split(deepest))));
}

#[test]
fn a_song_opened_again_starts_its_nested_splitters_shut() {
    let (mut s, g) = studio();
    let (t, [outer, inner, _]) = nest(&mut s);
    fold(&mut s, &g, t, inner);
    assert!(s.mixer.folds.knows(inner));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nest");
    s.song.save_as(&path).unwrap();
    assert!(s.request_open_file(crate::studio::FileRequest::Open, &path));
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while s.opening.is_some() {
        assert!(std::time::Instant::now() < end, "the song never opened");
        s.poll_opening();
        std::thread::yield_now();
    }
    // What the person opened is the view's, not the song's.
    assert!(!s.mixer.folds.knows(inner));
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(1600.0, 1000.0);
    let t = track(&s, "Vocal");
    assert_eq!(shown(&s, &g, t), vec![(outer, true), (inner, false)]);
}

#[test]
fn the_screen_reader_hears_a_nested_splitter_shut_then_open() {
    let (mut s, g) = studio();
    let (t, [_, inner, _]) = nest(&mut s);
    let expanded = |s: &Studio, g: &Geometry| {
        let key = format!("flow-{t}-{:?}", MixerHit::Fold(t, inner));
        crate::a11y::marks(s, g)
            .iter()
            .find(|m| m.key == key)
            .and_then(|m| m.node.is_expanded())
    };
    assert_eq!(expanded(&s, &g), Some(false));
    fold(&mut s, &g, t, inner);
    assert_eq!(expanded(&s, &g), Some(true));
    fold(&mut s, &g, t, inner);
    assert_eq!(expanded(&s, &g), Some(false));
    widen(&mut s, &g, t);
    assert_eq!(expanded(&s, &g), Some(true), "the wide strip shows it open");
}
