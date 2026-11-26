//! UI-182 / UI-183: a double click on the empty lane of an instrument track makes a bar of empty
//! part; a double click on the empty part of a track's head brings that channel up in the
//! mixer; only the name text renames. UI-379: a double click anywhere in a clip's time on its
//! row opens it, its clips locked by the automation or not, and never lays a part over it.
//!
//! The presses go the way the window sends them (`screen.rs`: the name, then the arrangement's
//! open, then its press, then the row), so the click path under test is the real one.

use super::probe::track;
use super::*;
use crate::studio::{DrawerTab, RowParts};
use crate::tools::Mods;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    (s, g)
}

/// One press of the window's body, in the order `screen.rs` takes them.
fn press(s: &mut Studio, g: &Geometry, (x, y): (f32, f32), clicks: u32) {
    if s.name_press(g, x, y, clicks) {
        return;
    }
    if clicks == 2 && s.arrange_open(g, x, y, Mods::NONE) {
        return;
    }
    if s.arrange_down(g, x, y, Mods::NONE) {
        if clicks == 2 {
            s.arrange_open(g, x, y, Mods::NONE);
        }
        return;
    }
    let hit = s.hit(g, x, y);
    s.press(g, hit);
}

fn double_click(s: &mut Studio, g: &Geometry, at: (f32, f32)) {
    for clicks in 1..=2 {
        press(s, g, at, clicks);
        s.arrange_up(g, at.0, at.1, Mods::NONE);
    }
}

fn bar(s: &Studio) -> Tick {
    s.project().time_map().bar_start(2)
}

/// The point of the lane of `name` at `tick`.
fn lane_at(s: &Studio, g: &Geometry, name: &str, tick: Tick) -> (f32, f32) {
    let kind = RowKind::Track(track(s, name));
    let row = *s.tree().row_of(kind).expect("row");
    (
        g.lanes.x - s.scroll.0 + s.ticks_to_px(tick),
        s.row_y(g, &row) + row.h / 2.0,
    )
}

fn clips(s: &Studio, name: &str) -> Vec<(Tick, Tick, String)> {
    let mut v: Vec<_> = s.project().tracks[&track(s, name)]
        .clips
        .iter()
        .map(|c| (c.start, c.length, c.name.clone()))
        .collect();
    v.sort();
    v
}

#[test]
fn a_double_click_on_the_empty_lane_of_an_instrument_makes_a_bar_and_selects_it() {
    let (mut s, g) = studio();
    let bass = track(&s, "Bass");
    let before = clips(&s, "Bass");
    let steps = s.song.steps();
    // The middle of bar 2 (bars 1 to 4 of the Bass are empty).
    let at = lane_at(&s, &g, "Bass", bar(&s) + bar(&s) / 2);
    double_click(&mut s, &g, at);

    let after = clips(&s, "Bass");
    assert_eq!(after.len(), before.len() + 1);
    assert!(
        after.contains(&(bar(&s), bar(&s), "Bass".to_string())),
        "one bar from the head of the bar, named as the track: {after:?}"
    );
    let made = s.project().tracks[&bass]
        .clips
        .iter()
        .find(|c| c.start == bar(&s))
        .unwrap()
        .id;
    assert_eq!(s.selected_clip, Some((bass, made)));
    assert_eq!(s.arrange.clips.len(), 1);
    assert_eq!(s.song.steps(), steps + 1, "one step of the history");
    assert!(!s.drawer_open, "made and selected, not opened");
    assert!(s.error.is_none());
}

#[test]
fn the_new_part_is_cut_before_the_next_one_and_starts_after_the_one_before() {
    let (mut s, g) = studio();
    let bass = track(&s, "Bass").to_string();
    let beat = bar(&s) / 4;
    // A part of one beat, from the third beat of bar 2.
    assert!(
        s.arrange_run(
            "clip.add.midi",
            json!({"track": bass, "at": bar(&s) + 2 * beat, "length": beat, "name": "x"}),
        )
        .is_some()
    );

    // Before it: from the head of the bar to the part.
    let at = lane_at(&s, &g, "Bass", bar(&s) + beat);
    double_click(&mut s, &g, at);
    assert!(clips(&s, "Bass").contains(&(bar(&s), 2 * beat, "Bass".to_string())));

    // After it: from where it ends to the end of the bar.
    let at = lane_at(&s, &g, "Bass", bar(&s) + 3 * beat + beat / 2);
    double_click(&mut s, &g, at);
    assert!(clips(&s, "Bass").contains(&(bar(&s) + 3 * beat, beat, "Bass".to_string())));
}

#[test]
fn a_double_click_on_the_empty_lane_of_an_audio_track_makes_nothing() {
    let (mut s, g) = studio();
    let before = clips(&s, "Vocal");
    let steps = s.song.steps();
    let at = lane_at(&s, &g, "Vocal", bar(&s) * 2 + 10);
    double_click(&mut s, &g, at);
    assert_eq!(clips(&s, "Vocal"), before);
    assert_eq!(s.song.steps(), steps);
    assert!(!s.drawer_open);
    assert!(s.error.is_none());
}

/// `name`'s first clip and where it is drawn.
fn first_clip(s: &Studio, g: &Geometry, name: &str) -> (TrackId, ClipId, Rect) {
    let t = track(s, name);
    let p = s.project();
    let c = p.tracks[&t]
        .clips
        .iter()
        .min_by_key(|c| c.start)
        .expect("a clip");
    let row = *s.tree().row_of(RowKind::Track(t)).expect("row");
    (t, c.id, s.clip_rect(g, &row, c))
}

fn stacks(s: &Studio, t: TrackId) -> usize {
    xirius_core::takes::stacks(&s.project().tracks[&t]).len()
}

/// The lanes of `t` open, its clips locked as the setting has them by default.
fn lock(s: &mut Studio, g: &Geometry, t: TrackId) {
    s.toggle_lanes(g, t);
    s.rebuild(g);
    assert!(s.clips_locked(t), "the lanes lock the clips by default");
}

/// Down the whole row (the clip, and the bands above and below it that are the row's space),
/// locked by the automation or not: the clip opens in the roll, even from the mixer's tab, and
/// nothing is laid over it. Opened, a locked clip stays locked.
#[test]
fn a_double_click_anywhere_down_a_clip_opens_it_locked_or_not() {
    for locked in [false, true] {
        let (mut s, g) = studio();
        let drums = track(&s, "Drums");
        if locked {
            lock(&mut s, &g, drums);
        }
        let before = clips(&s, "Drums");
        let takes = stacks(&s, drums);
        let h = s.tree().row_of(RowKind::Track(drums)).expect("row").h;
        for across in [0.1, 0.5, 0.9] {
            for down in 0..h as u32 {
                // From the mixer's tab once the drawer is open: the roll takes its place.
                if s.drawer_open {
                    s.tab = DrawerTab::Mixer;
                }
                // The drawer the first one opens may move the lanes: measured each time.
                let g = s.geometry(g.size.0, g.size.1);
                let (_, clip, r) = first_clip(&s, &g, "Drums");
                let row = *s.tree().row_of(RowKind::Track(drums)).expect("row");
                let at = (r.x + r.w * across, s.row_y(&g, &row) + down as f32 + 0.5);
                // The low edge of a clip that moves is the extended arrow's: its presses cut the
                // clip there (and join it back by the cut), which is undone for the next point.
                let smart = !locked
                    && s.roll.extended
                    && at.1 >= r.bottom() - Arrange::CUT_BAND
                    && at.1 < r.bottom();
                let steps = s.song.steps();
                s.selected_clip = None;
                double_click(&mut s, &g, at);
                let case = format!("locked {locked}, {across} across, {down} px down");
                assert_eq!(stacks(&s, drums), takes, "{case}: no part is laid over it");
                assert!(s.drawer_open, "{case}");
                assert_eq!(s.tab, DrawerTab::Roll, "{case}");
                assert!(s.error.is_none(), "{case}: {:?}", s.error);
                if smart {
                    for _ in 0..2 {
                        if clips(&s, "Drums") != before {
                            s.song.undo();
                        }
                    }
                    s.rebuild(&g);
                } else {
                    assert_eq!(s.song.steps(), steps, "{case}");
                    assert_eq!(s.selected_clip, Some((drums, clip)), "{case}");
                }
                assert_eq!(clips(&s, "Drums"), before, "{case}");
            }
        }
        if locked {
            // Opened, it is still locked: a drag does not move it.
            let g = s.geometry(g.size.0, g.size.1);
            let (_, _, r) = first_clip(&s, &g, "Drums");
            let from = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            super::probe::hold(&mut s, &g, from, (from.0 + 120.0, from.1), Mods::NONE);
            s.arrange_up(&g, from.0 + 120.0, from.1, Mods::NONE);
            assert_eq!(clips(&s, "Drums"), before, "the lock still holds the clip");
        }
    }
}

#[test]
fn make_part_makes_nothing_where_a_clip_already_is() {
    let (mut s, g) = studio();
    let (bass, _, r) = first_clip(&s, &g, "Bass");
    let before = clips(&s, "Bass");
    let steps = s.song.steps();
    for x in [r.x + 1.0, r.x + r.w / 2.0, r.right() - 1.0] {
        assert!(s.make_part(&g, bass, x), "the press is answered");
    }
    assert_eq!(clips(&s, "Bass"), before);
    assert_eq!(s.song.steps(), steps, "no clip.add.midi");
    // Just before it the lane is empty: a part is made there, up to the clip.
    assert!(s.make_part(&g, bass, r.x - 2.0));
    assert_eq!(clips(&s, "Bass").len(), before.len() + 1);
    assert_eq!(s.song.steps(), steps + 1);
}

/// The head of `name`'s row: a point at `x` from the row's left edge and `y` from its top.
fn head(s: &Studio, g: &Geometry, name: &str, x: f32, y: f32) -> (f32, f32) {
    let row = *s
        .tree()
        .row_of(RowKind::Track(track(s, name)))
        .expect("row");
    (g.column.x + x, s.row_y(g, &row) + y)
}

#[test]
fn a_double_click_on_the_empty_head_opens_the_mixer_on_that_channel_and_lights_it() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    let parts = RowParts::of(&row);
    // On the name's line, right of its text.
    let at = head(
        &s,
        &g,
        "Vocal",
        parts.name_right - 4.0,
        RowParts::LINE_1_Y + 6.0,
    );
    assert!(!s.drawer_open);
    double_click(&mut s, &g, at);

    assert!(s.input_map.name.is_none(), "no rename");
    assert!(s.drawer_open);
    assert_eq!(s.tab, DrawerTab::Mixer);
    assert_eq!(s.selected_row, Some(RowKind::Track(vocal)));
    assert_eq!(s.arrange.tracks, vec![RowKind::Track(vocal)]);
    let now = xirius_core::store::now_ms();
    assert!(s.mixer.flash_level(vocal, now) > 0.0, "the strip is lit");
    // The channel is in view.
    let g = s.settled_geometry(g.size.0, g.size.1);
    let view = s.mixer_view(&g);
    let strip = s
        .mixer_strips(&g)
        .into_iter()
        .find(|x| x.track == vocal)
        .expect("the strip shows");
    assert!(strip.rect.x >= view.x && strip.rect.right() <= view.right() + 0.5);
}

#[test]
fn the_second_double_click_on_the_same_head_keeps_the_mixer_open() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let row = *s.tree().row_of(RowKind::Track(keys)).unwrap();
    let parts = RowParts::of(&row);
    let at = head(
        &s,
        &g,
        "Keys",
        parts.name_right - 4.0,
        RowParts::LINE_1_Y + 6.0,
    );
    double_click(&mut s, &g, at);
    let g = s.geometry(g.size.0, g.size.1);
    let at = head(
        &s,
        &g,
        "Keys",
        parts.name_right - 4.0,
        RowParts::LINE_1_Y + 6.0,
    );
    double_click(&mut s, &g, at);
    assert!(s.mixer_shown());
    assert_eq!(s.selected_row, Some(RowKind::Track(keys)));
}

#[test]
fn only_the_name_text_renames() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    let parts = RowParts::of(&row);
    let on_text = head(
        &s,
        &g,
        "Vocal",
        parts.name_x + 3.0,
        RowParts::LINE_1_Y + 3.0,
    );
    double_click(&mut s, &g, on_text);
    assert_eq!(s.input_map.name.as_ref().map(|n| n.track), Some(vocal));
    assert!(!s.drawer_open, "the mixer is not opened by a rename");
    s.name_key("escape", Mods::NONE);

    // The same line, far right of the text, is the head's empty part.
    let band = parts.name_right - parts.name_x;
    assert!(band > 80.0, "a band wide enough to tell the two apart");
    let beside = head(
        &s,
        &g,
        "Vocal",
        parts.name_right - 4.0,
        RowParts::LINE_1_Y + 3.0,
    );
    double_click(&mut s, &g, beside);
    assert!(s.input_map.name.is_none());
    assert!(s.mixer_shown());
}

#[test]
fn a_double_click_on_a_button_of_the_head_does_neither() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let at = s.center_of(&g, Hit::Mute(vocal)).expect("the mute button");
    assert!(!s.name_press(&g, at.0, at.1, 2));
    assert!(s.input_map.name.is_none());
    assert!(!s.drawer_open);
}

#[test]
fn the_command_does_the_same_from_the_command_line() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(s.run_binding(&g, &format!("mixer.focus --id {vocal}")));
    assert!(s.mixer_shown());
    assert_eq!(s.selected_row, Some(RowKind::Track(vocal)));
    assert!(s.mixer.flash_level(vocal, xirius_core::store::now_ms()) > 0.0);
    // A track that is not in the song finds nothing to do.
    s.error = None;
    assert!(!s.run_binding(&g, &format!("mixer.focus --id {}", TrackId(99_999))));
}

#[test]
fn the_light_holds_for_a_moment_then_fades_and_goes() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let other = track(&s, "Keys");
    assert!(s.run_binding(&g, &format!("mixer.focus --id {vocal}")));
    // Measured from the moment it was lit.
    let t0 = xirius_core::store::now_ms();
    let lit = |t: u64| s.mixer.flash_level(vocal, t0 + t);
    assert!(lit(100) > 0.9);
    assert!(lit(600) < lit(400) && lit(600) > 0.0);
    assert_eq!(lit(crate::mixer::FLASH_MS + 5_000), 0.0);
    assert_eq!(s.mixer.flash_level(other, t0), 0.0, "only that channel");
    assert!(!s.mixer.flash_expire(t0));
    assert!(s.mixer.flash_expire(t0 + crate::mixer::FLASH_MS + 5_000));
    assert_eq!(s.mixer.flash_level(vocal, t0), 0.0);
}
