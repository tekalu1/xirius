//! The loop band of the arrangement's ruler and of the roll's, under the same rules
//! (2026-11-05): a press let go on the band turns the loop on or off and keeps its range, a
//! drag moves it or one of its ends, a drag in the band's row outside it makes a new range,
//! a press below the row moves the playhead; the right press has the same menus in both,
//! NumPad/ turns the loop on or off, and a screen reader hears the band as a Button.

use serde_json::json;
use xirius_core::time::Tick;

use super::tests::{choose, labels};
use crate::arrange::Spot;
use crate::roll::{Ctx, Out};
use crate::ruler::{Band, LOOP_H, LoopGrip};
use crate::studio::{Geometry, Studio};
use crate::tools::{Mods, SnapMode};

const BAR: Tick = 3840;

/// The arrangement alone, snapping to bars, with a loop over bars 5 to 8 that does not repeat.
fn arrangement() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.snap_mode = SnapMode::Bar;
    s.song
        .run(
            "prefs.set",
            json!({"key":"edit.snap.arrange.mode","value":"bar"}),
        )
        .unwrap();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    set_loop(&mut s, 4 * BAR, 8 * BAR);
    (s, g)
}

/// The sample with its roll open, the same loop.
fn with_roll() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 820.0);
    s.resized(&g);
    assert!(s.roll_ctx(&g).is_some(), "the roll is open");
    set_loop(&mut s, 4 * BAR, 8 * BAR);
    (s, g)
}

fn set_loop(s: &mut Studio, start: Tick, end: Tick) {
    s.song
        .run("loop.set", json!({ "start": start, "end": end }))
        .unwrap();
    if s.transport.looping {
        let p = s.project();
        s.transport.set_looping(&p, false);
    }
}

fn loop_now(s: &Studio) -> Option<(Tick, Tick)> {
    s.project().loop_range.map(|l| (l.start, l.end))
}

/// The middle of the band's row of the arrangement's ruler at song tick `t`.
fn band_at(s: &Studio, g: &Geometry, t: Tick) -> (f32, f32) {
    (s.tick_x(g, t), g.ruler.y + LOOP_H / 2.0)
}

/// A press in the arrangement as the window presses: what the arrangement does not take
/// goes to the press of what is under it (the ruler seeks).
fn arrange_down(s: &mut Studio, g: &Geometry, at: (f32, f32)) {
    if !s.arrange_down(g, at.0, at.1, Mods::NONE) {
        let hit = s.hit(g, at.0, at.1);
        s.press(g, hit);
    }
}

fn arrange_drag(s: &mut Studio, g: &Geometry, from: (f32, f32), to: (f32, f32)) {
    arrange_down(s, g, from);
    s.arrange_move(g, to.0, to.1, Mods::NONE);
    s.arrange_up(g, to.0, to.1, Mods::NONE);
}

fn arrange_click(s: &mut Studio, g: &Geometry, at: (f32, f32)) {
    arrange_drag(s, g, at, at);
}

fn ctx(s: &Studio, g: &Geometry) -> Ctx {
    s.roll_ctx(g).unwrap()
}

/// The roll's ruler at song tick `t`, `dy` below its top.
fn roll_at(s: &Studio, g: &Geometry, t: Tick, dy: f32) -> (f32, f32) {
    let c = ctx(s, g);
    let f = s.roll.frame(c.body);
    (s.roll.ruler_scale(&c).x(t), f.ruler.y + dy)
}

/// The loop grip of the roll's ruler under `at`.
fn roll_grip(s: &mut Studio, g: &Geometry, at: (f32, f32)) -> Option<LoopGrip> {
    let c = ctx(s, g);
    s.roll
        .ruler_menu_at(&c, at.0, at.1)
        .and_then(|(grip, _)| grip)
}

fn apply(s: &mut Studio, g: &Geometry, out: Out) {
    s.apply_roll(g, out);
    assert!(s.error.is_none(), "{:?}", s.error);
}

fn roll_drag(s: &mut Studio, g: &Geometry, from: (f32, f32), to: (f32, f32)) {
    let c = ctx(s, g);
    let out = s.roll.down(&c, from.0, from.1, Mods::NONE, 1);
    apply(s, g, out);
    let c = ctx(s, g);
    let out = s.roll.moved(&c, to.0, to.1, Mods::NONE);
    apply(s, g, out);
    let c = ctx(s, g);
    let out = s.roll.up(&c, to.0, to.1, Mods::NONE);
    apply(s, g, out);
}

fn roll_click(s: &mut Studio, g: &Geometry, at: (f32, f32)) {
    roll_drag(s, g, at, at);
}

#[test]
fn a_press_let_go_on_the_arrangement_band_turns_the_loop_on_and_off_and_keeps_its_range() {
    let (mut s, g) = arrangement();
    let steps = s.song.steps();
    let body = band_at(&s, &g, 6 * BAR);
    assert_eq!(
        s.arrange_spot(&g, body.0, body.1),
        Spot::Loop(LoopGrip::Body)
    );
    arrange_click(&mut s, &g, body);
    assert!(s.transport.looping);
    assert_eq!(loop_now(&s), Some((4 * BAR, 8 * BAR)));
    arrange_click(&mut s, &g, body);
    assert!(!s.transport.looping, "off again");
    assert_eq!(
        loop_now(&s),
        Some((4 * BAR, 8 * BAR)),
        "an off band keeps its range"
    );
    assert_eq!(s.song.steps(), steps, "no step to undo");
    // A tremble within the drag's slop is still a press.
    arrange_drag(&mut s, &g, body, (body.0 + 2.0, body.1));
    assert!(s.transport.looping);
}

#[test]
fn a_drag_on_the_arrangement_band_moves_it_or_its_ends_and_turns_nothing() {
    let (mut s, g) = arrangement();
    let bar = s.ticks_to_px(BAR);
    // The body moves the loop, keeping its length.
    let body = band_at(&s, &g, 6 * BAR);
    arrange_drag(&mut s, &g, body, (body.0 + bar * 2.0, body.1));
    assert_eq!(loop_now(&s), Some((6 * BAR, 10 * BAR)));
    assert!(!s.transport.looping);
    // The ends are within 4 px either side: 3 px inside the end grabs it.
    let end = band_at(&s, &g, 10 * BAR);
    let inside = (end.0 - 3.0, end.1);
    assert_eq!(
        s.arrange_spot(&g, inside.0, inside.1),
        Spot::Loop(LoopGrip::End)
    );
    arrange_drag(&mut s, &g, inside, (inside.0 + bar, inside.1));
    assert_eq!(loop_now(&s), Some((6 * BAR, 11 * BAR)));
    let start = band_at(&s, &g, 6 * BAR);
    let outside = (start.0 - 3.0, start.1);
    assert_eq!(
        s.arrange_spot(&g, outside.0, outside.1),
        Spot::Loop(LoopGrip::Start)
    );
    arrange_drag(&mut s, &g, outside, (outside.0 - bar, outside.1));
    assert_eq!(loop_now(&s), Some((5 * BAR, 11 * BAR)));
    // A press let go on an end turns nothing.
    let end = band_at(&s, &g, 11 * BAR);
    arrange_click(&mut s, &g, end);
    assert!(!s.transport.looping);
    assert_eq!(loop_now(&s), Some((5 * BAR, 11 * BAR)));
}

#[test]
fn the_arrangement_band_row_outside_the_band_makes_a_range_and_below_it_moves_the_playhead() {
    let (mut s, g) = arrangement();
    // A drag in the band's row outside the band: a new range.
    let (a, b) = (band_at(&s, &g, 12 * BAR), band_at(&s, &g, 14 * BAR));
    arrange_drag(&mut s, &g, a, b);
    assert_eq!(loop_now(&s), Some((12 * BAR, 14 * BAR)));
    assert!(!s.transport.looping);
    // A press let go there moves the playhead and leaves the loop.
    let out = band_at(&s, &g, 2 * BAR);
    arrange_click(&mut s, &g, out);
    assert!(!s.transport.looping);
    assert_eq!(loop_now(&s), Some((12 * BAR, 14 * BAR)));
    assert!((s.playhead - 2 * BAR).abs() < BAR / 4, "{}", s.playhead);
    // Below the band's row, even under the band, a press moves the playhead.
    let below = (s.tick_x(&g, 13 * BAR), g.ruler.y + g.ruler.h - 4.0);
    arrange_click(&mut s, &g, below);
    assert!(!s.transport.looping);
    assert!((s.playhead - 13 * BAR).abs() < BAR / 4, "{}", s.playhead);
}

#[test]
fn a_press_let_go_on_the_roll_band_turns_the_loop_on_and_off_and_the_roll_mirrors_the_song() {
    let (mut s, g) = with_roll();
    let body = roll_at(&s, &g, 6 * BAR, 5.0);
    assert_eq!(roll_grip(&mut s, &g, body), Some(LoopGrip::Body));
    let steps = s.song.steps();
    roll_click(&mut s, &g, body);
    assert!(s.transport.looping);
    assert_eq!(loop_now(&s), Some((4 * BAR, 8 * BAR)));
    roll_click(&mut s, &g, body);
    assert!(!s.transport.looping);
    assert_eq!(s.song.steps(), steps);
    // The roll's band is the song's loop: set elsewhere, the roll's band is there.
    set_loop(&mut s, 9 * BAR, 10 * BAR);
    let body = roll_at(&s, &g, 9 * BAR + BAR / 2, 5.0);
    assert_eq!(roll_grip(&mut s, &g, body), Some(LoopGrip::Body));
    roll_click(&mut s, &g, body);
    assert!(s.transport.looping);
}

#[test]
fn a_drag_on_the_roll_band_moves_it_or_its_ends_and_outside_it_makes_a_range() {
    let (mut s, g) = with_roll();
    let c = ctx(&s, &g);
    let bar = s.roll.ruler_scale(&c).x(BAR) - s.roll.ruler_scale(&c).x(0);
    let body = roll_at(&s, &g, 6 * BAR, 5.0);
    roll_drag(&mut s, &g, body, (body.0 + bar, body.1));
    assert_eq!(loop_now(&s), Some((5 * BAR, 9 * BAR)));
    assert!(!s.transport.looping, "a drag turns nothing");
    let end = roll_at(&s, &g, 9 * BAR, 5.0);
    let inside = (end.0 - 3.0, end.1);
    roll_drag(&mut s, &g, inside, (inside.0 + bar, inside.1));
    assert_eq!(loop_now(&s), Some((5 * BAR, 10 * BAR)));
    // Outside the band: a new range; the press there moves the playhead.
    let (a, b) = (
        roll_at(&s, &g, 10 * BAR + BAR / 4, 5.0),
        roll_at(&s, &g, 11 * BAR, 5.0),
    );
    roll_drag(&mut s, &g, a, b);
    assert_eq!(loop_now(&s), Some((10 * BAR + BAR / 4, 11 * BAR)));
    assert!(!s.transport.looping);
    // Below the row, under the band too: the playhead.
    let below = roll_at(&s, &g, 10 * BAR + BAR / 2, 20.0);
    roll_click(&mut s, &g, below);
    assert_eq!(s.playhead, 10 * BAR + BAR / 2);
    assert!(!s.transport.looping);
}

#[test]
fn the_right_press_on_either_ruler_has_the_same_menus() {
    let (mut s, g) = with_roll();
    // On the loop.
    let arr = band_at(&s, &g, 6 * BAR);
    assert!(s.timeline_context(&g, arr.0, arr.1));
    let arrange_loop = labels(&s);
    s.popup = None;
    let roll = roll_at(&s, &g, 6 * BAR, 5.0);
    assert!(s.roll_context(&g, roll.0, roll.1));
    assert_eq!(labels(&s), arrange_loop);
    assert_eq!(arrange_loop[0], "編集");
    s.popup = None;
    // Elsewhere on the ruler: below the band's row, and in the row outside the band.
    for (t, dy) in [(10 * BAR, 20.0), (10 * BAR, 5.0)] {
        let arr = (s.tick_x(&g, t), g.ruler.y + dy);
        assert!(s.timeline_context(&g, arr.0, arr.1));
        let arrange_ruler = labels(&s);
        s.popup = None;
        let roll = roll_at(&s, &g, t, dy);
        assert!(s.roll_context(&g, roll.0, roll.1));
        assert_eq!(labels(&s), arrange_ruler);
        assert_eq!(arrange_ruler[0], "時間");
        s.popup = None;
    }
    // The roll's rows act at the roll's tick: its loop's end lands on bar 11.
    let roll = roll_at(&s, &g, 10 * BAR, 20.0);
    assert!(s.roll_context(&g, roll.0, roll.1));
    assert!(choose(&mut s, &g, "ループの終わりをここに"));
    assert_eq!(loop_now(&s), Some((4 * BAR, 10 * BAR)));
    // From the roll's band, its menu's 「ループ」 turns the loop on.
    let roll = roll_at(&s, &g, 6 * BAR, 5.0);
    set_loop(&mut s, 4 * BAR, 8 * BAR);
    assert!(s.roll_context(&g, roll.0, roll.1));
    assert!(choose(&mut s, &g, "ループ"));
    assert!(s.transport.looping);
}

#[test]
fn numpad_slash_turns_the_loop_on_and_off_from_either_view() {
    let (mut s, g) = with_roll();
    s.focus_arrange(true);
    assert!(s.mapped_key(&g, "NumPad/", Some("/"), Mods::NONE));
    assert!(s.transport.looping);
    s.focus_arrange(false);
    assert!(s.mapped_key(&g, "NumPad/", Some("/"), Mods::NONE));
    assert!(!s.transport.looping);
    assert_eq!(loop_now(&s), Some((4 * BAR, 8 * BAR)));
}

#[test]
fn a_screen_reader_hears_the_band_as_a_button_with_its_bars_and_state() {
    let (mut s, g) = with_roll();
    let find = |s: &Studio, key: &str| {
        crate::a11y::marks(s, &g)
            .into_iter()
            .find(|m| m.key == key)
            .unwrap_or_else(|| panic!("{key}"))
    };
    for key in ["loop-band", "roll-loop-band"] {
        let m = find(&s, key);
        assert_eq!(m.node.role(), gpui::accesskit::Role::Button);
        assert_eq!(m.node.label(), Some("ループ 5 小節〜8 小節 オフ"));
        assert_eq!(m.node.toggled(), Some(gpui::accesskit::Toggled::False));
    }
    let hit = find(&s, "roll-loop-band").press.expect("it presses");
    assert!(crate::a11y::press(&mut s, &g, hit));
    assert!(s.transport.looping);
    let m = find(&s, "loop-band");
    assert_eq!(m.node.label(), Some("ループ 5 小節〜8 小節 オン"));
    assert_eq!(m.node.toggled(), Some(gpui::accesskit::Toggled::True));
}

#[test]
fn the_numbers_take_the_ink_for_a_fill_only_over_a_filled_band() {
    let band = |lit: bool, hover: Option<LoopGrip>| Band {
        x0: 10.0,
        x1: 50.0,
        lit,
        held: false,
        hover,
        punch: false,
    };
    assert_eq!(band(false, None).over(), None, "an off band keeps its look");
    assert_eq!(band(false, Some(LoopGrip::New)).over(), None);
    assert_eq!(band(true, None).over(), Some((10.0, 50.0)));
    assert_eq!(band(false, Some(LoopGrip::Body)).over(), Some((10.0, 50.0)));
}

/// The song playing to a silent output.
fn play_silent(s: &mut Studio, g: &Geometry) {
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    assert!(s.edit_key(g, "space", Mods::NONE));
    assert!(s.transport.playing, "{:?}", s.error);
}

/// Both bands as they would be painted now: the arrangement's and the roll's.
fn bands(s: &Studio, g: &Geometry) -> [Band; 2] {
    let p = s.project();
    let (a, _) = crate::arrange::paint::band(s, &p, g).expect("the arrangement's band");
    let (r, _) = crate::roll::ruler::band(s, &s.roll, &ctx(s, g)).expect("the roll's band");
    [a, r]
}

/// Review M4: under auto-punch both bands are red; a press still shows: full red with the
/// ink for a fill while the loop is on, a faint red with the plain ink while it is off.
#[test]
fn auto_punch_shows_the_loop_on_and_off_on_both_bands() {
    let (mut s, g) = with_roll();
    s.song.set("record.punch", json!({"on": true})).unwrap();
    s.song
        .set("record.preroll", json!({"on": true, "bars": 1}))
        .unwrap();
    let c = crate::paint::Colors::new(xirius_design::Theme::ALL[0]);
    let p = s.project();
    let (_, rolls) = crate::arrange::paint::band(&s, &p, &g).unwrap();
    let (_, roll_rolls) = crate::roll::ruler::band(&s, &s.roll, &ctx(&s, &g)).unwrap();
    assert!(
        rolls[0].is_some() && roll_rolls[0].is_some(),
        "the pre-roll beside both"
    );
    for b in bands(&s, &g) {
        assert!(b.punch && !b.lit);
        assert_eq!(b.over(), None, "off: the numbers keep their ink");
        assert_eq!(b.face(&c), c.alert.opacity(0.3));
    }
    let at = band_at(&s, &g, 6 * BAR);
    arrange_click(&mut s, &g, at);
    assert!(
        s.transport.looping,
        "the press turns the loop on under punch too"
    );
    s.arrange.hover = Spot::None;
    for b in bands(&s, &g) {
        assert!(b.punch && b.lit);
        assert_eq!(b.over(), Some((b.x0, b.x1)));
        assert_eq!(b.face(&c), c.alert);
    }
    let at = roll_at(&s, &g, 6 * BAR, 5.0);
    roll_click(&mut s, &g, at);
    assert!(!s.transport.looping);
    s.roll.ruler_hover = None;
    for b in bands(&s, &g) {
        assert_eq!(b.face(&c), c.alert.opacity(0.3));
    }
    assert_eq!(loop_now(&s), Some((4 * BAR, 8 * BAR)));
}

/// Review M5: the bar numbers, the times and the seconds all sit in the band's row and take
/// the ink for a fill where they cross the band's fill; a bar's (and its seconds) blue.
#[test]
fn every_number_on_the_ruler_sits_in_the_band_row() {
    use crate::ruler::{band_row, number_ink, row_parts};
    use crate::studio::Rect;
    let ruler = Rect::new(100.0, 40.0, 600.0, 28.0);
    let row = band_row(ruler);
    assert_eq!((row.y, row.h), (ruler.y, LOOP_H));
    assert_eq!(row_parts(ruler, None), vec![(row, false)]);
    let parts = row_parts(ruler, Some((200.0, 300.0)));
    assert_eq!(
        parts,
        vec![
            (Rect::new(100.0, 40.0, 100.0, LOOP_H), false),
            (Rect::new(300.0, 40.0, 400.0, LOOP_H), false),
            (Rect::new(200.0, 40.0, 100.0, LOOP_H), true),
        ]
    );
    // A band past the ruler's left edge leaves no stretch there.
    assert_eq!(row_parts(ruler, Some((50.0, 150.0))).len(), 2);
    for theme in xirius_design::Theme::ALL {
        let c = crate::paint::Colors::new(theme);
        assert_eq!(number_ink(&c, true, false), c.ink_blue);
        assert_eq!(number_ink(&c, false, false), c.ink);
        assert_eq!(number_ink(&c, true, true), c.on_fill);
        assert_eq!(number_ink(&c, false, true), c.on_fill);
    }
}

/// Review M6 and L1: both bands tell a press from a drag by the straight distance, 3 px for a
/// mouse and 10 px for a finger or a pen.
#[test]
fn both_bands_tell_a_press_from_a_drag_alike_and_a_finger_may_tremble_further() {
    let (mut s, g) = with_roll();
    let body = band_at(&s, &g, 6 * BAR);
    let shift = |p: (f32, f32), d: (f32, f32)| (p.0 + d.0, p.1 + d.1);
    // 2 px aslant (2.8 px) is a press in both; 2.5 px aslant (3.5 px) a drag in both.
    arrange_drag(&mut s, &g, body, shift(body, (2.0, 2.0)));
    assert!(s.transport.looping);
    arrange_drag(&mut s, &g, body, shift(body, (2.5, 2.5)));
    assert!(
        s.transport.looping,
        "a drag back onto the bar turns nothing"
    );
    let rb = roll_at(&s, &g, 6 * BAR, 5.0);
    roll_drag(&mut s, &g, rb, shift(rb, (2.0, 2.0)));
    assert!(!s.transport.looping);
    roll_drag(&mut s, &g, rb, shift(rb, (2.5, 2.5)));
    assert!(!s.transport.looping);
    assert_eq!(loop_now(&s), Some((4 * BAR, 8 * BAR)));
    // A finger: 8 px is still a press on either band.
    s.coarse_press = true;
    arrange_drag(&mut s, &g, body, shift(body, (6.0, 5.0)));
    assert!(s.transport.looping);
    assert!(s.roll_press(&g, rb.0, rb.1, Mods::NONE, 1));
    let c = ctx(&s, &g);
    let out = s.roll.moved(&c, rb.0 + 6.0, rb.1 + 5.0, Mods::NONE);
    apply(&mut s, &g, out);
    let c = ctx(&s, &g);
    let out = s.roll.up(&c, rb.0 + 6.0, rb.1 + 5.0, Mods::NONE);
    apply(&mut s, &g, out);
    assert!(!s.transport.looping);
    // Past 10 px it moves the band.
    let far = shift(body, (s.beat_w * 8.0, 0.0));
    arrange_drag(&mut s, &g, body, far);
    assert!(!s.transport.looping);
    assert_ne!(loop_now(&s), Some((4 * BAR, 8 * BAR)));
}

/// Review L9: a band pulled and brought back, or an end pulled onto the other, leaves the
/// loop as it was and turns nothing; a press while the song plays turns the loop on and off.
#[test]
fn a_band_brought_back_or_pulled_to_nothing_changes_nothing_and_a_press_works_while_playing() {
    let (mut s, g) = with_roll();
    let steps = s.song.steps();
    let body = band_at(&s, &g, 6 * BAR);
    arrange_down(&mut s, &g, body);
    s.arrange_move(&g, body.0 + 80.0, body.1, Mods::NONE);
    s.arrange_move(&g, body.0, body.1, Mods::NONE);
    s.arrange_up(&g, body.0, body.1, Mods::NONE);
    assert!(!s.transport.looping);
    let rb = roll_at(&s, &g, 6 * BAR, 5.0);
    let c = ctx(&s, &g);
    let out = s.roll.down(&c, rb.0, rb.1, Mods::NONE, 1);
    apply(&mut s, &g, out);
    let c = ctx(&s, &g);
    let out = s.roll.moved(&c, rb.0 + 80.0, rb.1, Mods::NONE);
    apply(&mut s, &g, out);
    let c = ctx(&s, &g);
    let out = s.roll.up(&c, rb.0, rb.1, Mods::NONE);
    apply(&mut s, &g, out);
    assert!(!s.transport.looping);
    // The end onto the start: no loop of no length.
    let (end, start) = (band_at(&s, &g, 8 * BAR), band_at(&s, &g, 4 * BAR));
    arrange_drag(&mut s, &g, end, start);
    assert_eq!(loop_now(&s), Some((4 * BAR, 8 * BAR)));
    assert_eq!(s.song.steps(), steps);
    // While the song plays.
    play_silent(&mut s, &g);
    arrange_click(&mut s, &g, body);
    assert!(s.transport.looping && s.transport.playing);
    roll_click(&mut s, &g, rb);
    assert!(!s.transport.looping && s.transport.playing);
    assert_eq!(s.song.steps(), steps);
}

/// Review L4: under a menu neither band is offered to a screen reader.
#[test]
fn a_screen_reader_is_offered_neither_band_under_a_menu() {
    let (mut s, g) = with_roll();
    let rb = roll_at(&s, &g, 6 * BAR, 5.0);
    assert!(s.roll_context(&g, rb.0, rb.1));
    assert!(s.popup.is_some(), "the menu is open");
    let keys: Vec<_> = crate::a11y::marks(&s, &g)
        .into_iter()
        .map(|m| m.key)
        .collect();
    assert!(
        !keys
            .iter()
            .any(|k| k == "loop-band" || k == "roll-loop-band")
    );
}
