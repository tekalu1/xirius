use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::rack::RackNode;

use super::super::flow::Pick;
use super::*;
use crate::studio::DrawerTab;

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

/// Vocal with EQ, then a splitter: Comp in one branch, Reverb in the other.
fn vocal(s: &mut Studio) -> (TrackId, NodeId, [PluginId; 3]) {
    let t = track(s, "Vocal");
    let eq = add(s, t, "EQ");
    let comp = add(s, t, "Comp");
    let verb = add(s, t, "Reverb");
    assert!(s.run_command("rack.split.add", json!({"track":t})));
    let (sp, b) = match s.project().tracks[&t].rack.as_ref().unwrap().fx.last() {
        Some(RackNode::Split(x)) => (x.id, [x.branches[0].id, x.branches[1].id]),
        other => panic!("{other:?}"),
    };
    assert!(s.run_command("rack.node.move", json!({"id":comp,"to":b[0]})));
    assert!(s.run_command("rack.node.move", json!({"id":verb,"to":b[1]})));
    (t, sp, [eq, comp, verb])
}

fn flow_of(s: &Studio, g: &Geometry, t: TrackId) -> Flow {
    let strip = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .unwrap();
    s.strip_flow(&strip, &s.project().tracks[&t])
}

fn ms(base: Instant, n: u64) -> Instant {
    base + Duration::from_millis(n)
}

/// The middle of `spot`'s line.
fn on_line(s: &Studio, g: &Geometry, t: TrackId, spot: Spot) -> (f32, f32) {
    let f = flow_of(s, g, t);
    let seam = f.seam(&s.project().tracks[&t], spot).unwrap();
    let p = place(&f, &seam).unwrap();
    ((p.left + p.right) / 2.0, p.y)
}

fn cand(track: TrackId, spot: Spot, mid: bool) -> Cand {
    Cand {
        track,
        spot,
        at: (0.0, 0.0),
        mid,
    }
}

fn some_spot() -> (TrackId, Spot) {
    let (s, _) = studio();
    (
        track(&s, "Guitar"),
        Spot::After(Pick::Plugin(PluginId(900_001))),
    )
}

// ---- the pointer's way to the line (a clock of our own) ----

#[test]
fn a_slow_pointer_shows_the_line_at_once() {
    let (t, spot) = some_spot();
    let b = Instant::now();
    let mut l = Live::default();
    l.feed(b, 10.0, 100.0);
    l.feed(ms(b, 30), 10.0, 103.0);
    assert!(l.slow());
    l.pointer(ms(b, 30), Some(cand(t, spot, true)), None, false);
    assert_eq!(l.spot_on(t), Some(spot));
    // it grows from the first frame, to the full line by Fast
    assert!(l.look(ms(b, 30), false).unwrap().grow < 0.05);
    assert!((l.look(ms(b, 30 + 130), false).unwrap().grow - 1.0).abs() < 1e-3);
}

#[test]
fn a_fast_pointer_that_stops_shows_it_after_the_settle_time() {
    let (t, spot) = some_spot();
    let b = Instant::now();
    let mut l = Live::default();
    l.feed(b, 10.0, 100.0);
    l.feed(ms(b, 20), 10.0, 160.0);
    assert!(!l.slow());
    l.pointer(ms(b, 20), Some(cand(t, spot, true)), None, false);
    assert_eq!(l.spot_on(t), None);
    assert!(l.wait_pending());
    let again = |_: TrackId, _: Spot| Some(cand(t, spot, true));
    l.advance(ms(b, 20 + SETTLE_MS - 1), false, again, None);
    assert_eq!(l.spot_on(t), None);
    l.advance(ms(b, 20 + SETTLE_MS), false, again, None);
    assert_eq!(l.spot_on(t), Some(spot));
}

#[test]
fn a_fast_pointer_going_through_never_shows_it() {
    let (t, spot) = some_spot();
    let b = Instant::now();
    let mut l = Live::default();
    l.feed(b, 10.0, 100.0);
    l.feed(ms(b, 20), 10.0, 160.0);
    l.pointer(ms(b, 20), Some(cand(t, spot, true)), None, false);
    // it has gone on before the time is up
    l.feed(ms(b, 60), 10.0, 240.0);
    l.pointer(ms(b, 60), None, None, false);
    assert!(!l.wait_pending());
    l.advance(ms(b, 400), false, |_, _| Some(cand(t, spot, true)), None);
    assert_eq!(l.spot_on(t), None);
    assert!(l.shown.is_none());
}

#[test]
fn the_line_stays_a_grace_after_the_pointer_leaves_and_then_fades() {
    let (t, spot) = some_spot();
    let b = Instant::now();
    let mut l = Live::default();
    l.feed(b, 10.0, 100.0);
    l.pointer(b, Some(cand(t, spot, true)), None, false);
    l.feed(ms(b, 200), 40.0, 100.0);
    l.pointer(ms(b, 200), None, None, false);
    assert!(l.leave_pending());
    l.advance(ms(b, 200 + GRACE_MS - 1), false, |_, _| None, None);
    assert_eq!(l.spot_on(t), Some(spot));
    l.advance(ms(b, 200 + GRACE_MS + 1), false, |_, _| None, None);
    // it is going: not to be pressed, still on the screen
    assert_eq!(l.spot_on(t), None);
    assert!(l.look(ms(b, 200 + GRACE_MS + 1), false).unwrap().going);
    l.advance(ms(b, 200 + GRACE_MS + 90), false, |_, _| None, None);
    assert!(l.shown.is_none());
}

#[test]
fn the_pointer_coming_back_in_the_grace_keeps_the_line() {
    let (t, spot) = some_spot();
    let b = Instant::now();
    let mut l = Live::default();
    l.pointer(b, Some(cand(t, spot, true)), None, false);
    l.pointer(ms(b, 200), None, None, false);
    l.pointer(ms(b, 230), Some(cand(t, spot, true)), None, false);
    assert!(!l.leave_pending());
    l.advance(ms(b, 400), false, |_, _| None, None);
    assert_eq!(l.spot_on(t), Some(spot));
}

#[test]
fn slow_travel_along_the_rows_keeps_the_line_and_it_slides_to_the_next() {
    let (t, spot) = some_spot();
    let b = Instant::now();
    let mut l = Live::default();
    l.pointer(b, Some(cand(t, spot, true)), None, false);
    l.pointer(ms(b, 100), None, None, true);
    assert!(!l.leave_pending());
    l.advance(ms(b, 900), false, |_, _| None, None);
    assert_eq!(l.spot_on(t), Some(spot));
    let next = Spot::Before(Pick::Plugin(PluginId(900_002)));
    l.feed(ms(b, 1000), 10.0, 100.0);
    let mut c = cand(t, next, true);
    c.at = (0.0, 22.0);
    l.pointer(ms(b, 1000), Some(c), Some((0.0, 0.0)), false);
    assert_eq!(l.spot_on(t), Some(next));
    let look = l.look(ms(b, 1000), false).unwrap();
    assert!((look.off.1 + 22.0).abs() < 1e-3);
    assert_eq!(l.look(ms(b, 1200), false).unwrap().off, (0.0, 0.0));
}

#[test]
fn the_power_columns_start_no_line() {
    let (t, spot) = some_spot();
    let mut l = Live::default();
    l.pointer(Instant::now(), Some(cand(t, spot, false)), None, false);
    assert!(l.shown.is_none() && !l.wait_pending());
}

#[test]
fn reduced_motion_shows_and_moves_at_once_but_keeps_the_grace() {
    let (t, spot) = some_spot();
    let b = Instant::now();
    let mut l = Live::default();
    l.pointer(b, Some(cand(t, spot, true)), None, false);
    let look = l.look(b, true).unwrap();
    assert_eq!(look.grow, 1.0);
    assert_eq!(look.off, (0.0, 0.0));
    l.pointer(ms(b, 100), None, None, false);
    l.advance(ms(b, 100 + GRACE_MS - 1), true, |_, _| None, None);
    assert_eq!(l.spot_on(t), Some(spot), "the grace is kept");
    l.advance(ms(b, 100 + GRACE_MS + 1), true, |_, _| None, None);
    assert!(l.shown.is_none(), "it goes at once");
}

#[test]
fn a_row_held_shows_the_line_without_the_plus_or_a_wait() {
    let (t, _) = some_spot();
    let b = Instant::now();
    let mut l = Live::default();
    l.dragging(b, Some(cand(t, Spot::Drop(2), false)), None);
    assert!(l.live().is_some_and(|s| !s.plus && s.drag));
    l.end_drag(ms(b, 60), false);
    assert_eq!(l.spot_on(t), None);
}

#[test]
fn a_press_finds_the_line_a_little_beyond_where_it_is_found_before_it_shows() {
    let p = Place {
        left: 4.0,
        right: 100.0,
        y: 50.0,
        from: 6.0,
        to: 98.0,
    };
    // before it shows: 5 above and below
    assert_eq!(zone(&p, false, false, 50.0, 55.0), Some(true));
    assert_eq!(zone(&p, false, false, 50.0, 56.0), None);
    // once it shows: 8
    assert_eq!(zone(&p, false, true, 50.0, 58.0), Some(true));
    assert_eq!(zone(&p, false, true, 50.0, 59.0), None);
    // the power mark's column and the close's: no start there, and 3 once it shows
    assert_eq!(zone(&p, false, false, 10.0, 50.0), None);
    assert_eq!(zone(&p, false, true, 10.0, 53.0), Some(false));
    assert_eq!(zone(&p, false, true, 10.0, 54.0), None);
    assert_eq!(zone(&p, false, true, 95.0, 53.0), Some(false));
    // outside the area's sides
    assert_eq!(zone(&p, false, true, 3.0, 50.0), None);
}

// ---- where the lines are ----

#[test]
fn a_track_with_no_device_has_no_line() {
    let (s, g) = studio();
    let t = track(&s, "Vocal");
    assert!(flow_of(&s, &g, t).seams().is_empty());
}

#[test]
fn a_chain_has_a_line_above_its_first_row_and_under_each_row() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let b = add(&mut s, t, "Comp");
    let f = flow_of(&s, &g, t);
    let seams = f.seams();
    let spots: Vec<_> = seams.iter().map(|x| x.spot).collect();
    assert_eq!(
        spots,
        [
            Spot::Before(Pick::Plugin(a)),
            Spot::After(Pick::Plugin(a)),
            Spot::After(Pick::Plugin(b)),
        ]
    );
    // rows are where they were: the lines lie on their edges
    let (_, ra) = f.plugin(a).unwrap();
    let (_, rb) = f.plugin(b).unwrap();
    assert_eq!(seams[0].y, ra.y);
    assert_eq!(seams[1].y, rb.y);
    assert_eq!(seams[2].y, rb.bottom());
}

#[test]
fn the_first_and_the_last_line_with_their_plus_are_inside_the_insert_panel() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    add(&mut s, t, "EQ");
    let f = flow_of(&s, &g, t);
    let panel = f.insert_panel().unwrap();
    let (d, _) = plus_size(false);
    let seams = f.seams();
    assert!(seams.last().unwrap().y + d / 2.0 <= panel.bottom());
    assert!(seams[0].y - d / 2.0 >= panel.y);
}

#[test]
fn a_branch_has_a_line_above_its_first_row_and_the_last_one_inside_the_panel() {
    let (mut s, g) = studio();
    let (t, _, [_, comp, verb]) = vocal(&mut s);
    let f = flow_of(&s, &g, t);
    let spots: Vec<_> = f.seams().iter().map(|x| (x.spot, x.depth)).collect();
    assert!(spots.contains(&(Spot::Before(Pick::Plugin(comp)), 1)));
    assert!(spots.contains(&(Spot::After(Pick::Plugin(comp)), 1)));
    assert!(spots.contains(&(Spot::After(Pick::Plugin(verb)), 1)));
    let panel = f.insert_panel().unwrap();
    let last = f.seams().pop().unwrap();
    let (d, _) = plus_size(false);
    assert!(last.y + d / 2.0 <= panel.bottom());
}

#[test]
fn the_line_is_a_hit_only_while_it_shows() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let spot = Spot::After(Pick::Plugin(a));
    let (x, y) = on_line(&s, &g, t, spot);
    let seam = Hit::Mixer(MixerHit::Seam(t, spot));
    assert_ne!(s.hit(&g, x, y), seam);
    s.seam_show(t, spot, true);
    assert_eq!(s.hit(&g, x, y), seam);
    // 8 px off the line still, 9 not
    assert_eq!(s.hit(&g, x, y + 8.0), seam);
    assert_ne!(s.hit(&g, x, y + 9.0), seam);
}

#[test]
fn a_press_on_the_line_opens_the_list_there_and_the_line_stays_while_it_is_open() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let spot = Spot::After(Pick::Plugin(a));
    s.seam_show(t, spot, true);
    assert!(s.press(&g, Hit::Mixer(MixerHit::Seam(t, spot))));
    assert!(s.popup.is_some());
    assert_eq!(s.mixer.insert_after, Some((t, Pick::Plugin(a))));
    // the pointer goes away: the line stays under the open list
    let (x, y) = on_line(&s, &g, t, spot);
    let now = Instant::now();
    s.seam_moved(&g, x + 300.0, y + 300.0, now);
    s.seam_tick(&g, now + Duration::from_secs(2));
    assert_eq!(s.mixer.seam.spot_on(t), Some(spot));
}

#[test]
fn the_line_above_the_first_row_opens_the_list_for_a_place_before_it() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    add(&mut s, t, "Comp");
    let spot = Spot::Before(Pick::Plugin(a));
    s.seam_show(t, spot, true);
    assert!(s.press(&g, Hit::Mixer(MixerHit::Seam(t, spot))));
    assert_eq!(
        s.mixer.insert_into,
        Some((t, crate::channel::flow::Chain::Top, 0))
    );
}

#[test]
fn the_pointer_moving_slowly_shows_the_line_and_the_rows_light_nothing() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let spot = Spot::After(Pick::Plugin(a));
    let (x, y) = on_line(&s, &g, t, spot);
    let b = Instant::now();
    s.seam_moved(&g, x, y + 10.0, b);
    s.seam_moved(&g, x, y + 4.0, ms(b, 30));
    assert_eq!(s.mixer.seam.spot_on(t), Some(spot));
    assert!(on_show(&s, t));
}

/// The pointer's speed reaches the line through the window's moves and frames: a fast pointer
/// shows it only once it stops on it (the timing itself is the `Live` tests' above).
#[test]
fn a_fast_pointer_shows_the_line_only_when_it_stops_on_it() {
    struct Row {
        name: &'static str,
        /// (px from the line, ms) the pointer is at.
        moves: &'static [(f32, u64)],
        /// (ms, shows) frames after the moves.
        frames: &'static [(u64, bool)],
    }
    let rows = [
        Row {
            name: "going across",
            moves: &[(-40.0, 0), (2.0, 30), (80.0, 60)],
            frames: &[(400, false)],
        },
        Row {
            name: "stopping on it",
            moves: &[(-40.0, 0), (1.0, 30)],
            frames: &[(100, false), (30 + SETTLE_MS + 1, true)],
        },
    ];
    let mut failures = Vec::new();
    for row in &rows {
        let (mut s, g) = studio();
        let t = track(&s, "Vocal");
        let a = add(&mut s, t, "EQ");
        let spot = Spot::After(Pick::Plugin(a));
        let (x, y) = on_line(&s, &g, t, spot);
        let b = Instant::now();
        for &(dy, at) in row.moves {
            s.seam_moved(&g, x, y + dy, ms(b, at));
            if s.mixer.seam.spot_on(t).is_some() {
                failures.push(format!("{}: shown while moving at {at} ms", row.name));
            }
        }
        for &(at, shows) in row.frames {
            s.seam_tick(&g, ms(b, at));
            let got = s.mixer.seam.spot_on(t);
            if got != shows.then_some(spot) {
                failures.push(format!("{}: at {at} ms got {got:?}", row.name));
            }
        }
    }
    assert!(failures.is_empty(), "fast pointer: {failures:#?}");
}

#[test]
fn the_pointer_leaving_the_window_gives_the_line_its_grace_then_takes_it() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let spot = Spot::After(Pick::Plugin(a));
    let (x, y) = on_line(&s, &g, t, spot);
    let b = Instant::now();
    s.seam_moved(&g, x, y, b);
    assert_eq!(s.mixer.seam.spot_on(t), Some(spot));
    s.seam_left(ms(b, 100));
    s.seam_tick(&g, ms(b, 100 + GRACE_MS + 1));
    assert_eq!(s.mixer.seam.spot_on(t), None);
    s.seam_tick(&g, ms(b, 100 + GRACE_MS + 200));
    assert!(s.mixer.seam.shown.is_none());
}

#[test]
fn the_line_goes_when_its_row_does() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let spot = Spot::After(Pick::Plugin(a));
    s.seam_show(t, spot, true);
    assert!(s.run_command("plugin.remove", json!({"ids":[a]})));
    s.seam_tick(&g, Instant::now());
    assert!(s.mixer.seam.shown.is_none());
}

#[test]
fn a_row_held_and_moved_shows_the_line_where_it_will_land() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let b = add(&mut s, t, "Comp");
    add(&mut s, t, "Delay");
    s.instruments.open.clear();
    let (x, y) = s.mixer_center(&g, MixerHit::Open(t, a)).unwrap();
    s.press_keys(&g, x, y);
    assert!(s.chain_down(&g, x, y));
    let (_, y2) = s.mixer_center(&g, MixerHit::Open(t, b)).unwrap();
    assert!(s.fx_move(x, y2 + 6.0));
    s.seam_moved(&g, x, y2 + 6.0, Instant::now());
    let live = s.mixer.seam.live().copied().unwrap();
    assert!(live.drag && !live.plus);
    assert!(matches!(live.spot, Spot::Drop(_)));
    assert!(s.fx_up(&g, x, y2 + 6.0, false));
}

#[test]
fn the_screen_reader_names_the_line_above_the_first_row() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let marks = crate::a11y::marks(&s, &g);
    let key = |spot: Spot| format!("flow-{t}-{:?}", MixerHit::Seam(t, spot));
    let before = marks
        .iter()
        .find(|m| m.key == key(Spot::Before(Pick::Plugin(a))))
        .expect("the line above the first row");
    assert_eq!(before.node.label(), Some("EQ の前に挿す"));
    let after = marks
        .iter()
        .find(|m| m.key == key(Spot::After(Pick::Plugin(a))))
        .unwrap();
    assert_eq!(after.node.label(), Some("EQ の後ろに挿す"));
}

#[test]
fn the_line_has_no_tooltip() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    let spot = Spot::After(Pick::Plugin(a));
    s.seam_show(t, spot, true);
    let (x, y) = on_line(&s, &g, t, spot);
    s.hover = s.hit(&g, x, y);
    assert!(matches!(s.hover, Hit::Mixer(MixerHit::Seam(..))));
    assert_eq!(crate::hover_tip::command(s.hover), None);
    assert_eq!(crate::hover_tip::text(&s, s.hover), None);
}

#[test]
fn a_touch_shows_the_line_below_the_picked_row_with_a_larger_plus() {
    let (mut s, _) = studio();
    let t = track(&s, "Vocal");
    let a = add(&mut s, t, "EQ");
    s.mixer.coarse = true;
    s.mixer.chosen = Some((t, Pick::Plugin(a)));
    let p = s.project();
    let look = visible(&s, &p.tracks[&t], Instant::now()).unwrap();
    assert_eq!(look.spot, Spot::After(Pick::Plugin(a)));
    assert_eq!(look.grow, 1.0);
    assert!(plus_size(true).0 > plus_size(false).0);
}
