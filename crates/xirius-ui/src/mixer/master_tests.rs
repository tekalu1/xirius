//! The master pinned to the right edge, with the channels scrolling beside it (the feel
//! pass, item 08), pressed and dragged the way the window does.

use super::*;

const H: f32 = 1000.0;

/// The sample song with the mixer open, in a window too narrow for every channel.
fn narrow() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(700.0, H);
    (s, g)
}

fn master(s: &Studio, g: &Geometry) -> StripLayout {
    s.mixer_strips(g)
        .into_iter()
        .find(|x| x.track.is_master())
        .expect("the master is on screen")
}

#[test]
fn the_master_is_the_last_strip_and_does_not_move_when_the_channels_scroll() {
    let (mut s, g) = narrow();
    let first = master(&s, &g).rect;
    assert_eq!(first.right(), g.drawer_body.right() - STRIP_GAP);
    assert!(s.mixer_strips(&g).last().unwrap().track.is_master());
    let max = s.mixer_max_scroll(&g);
    assert!(max > 0.0, "the channels do not fit beside the master");
    for scroll in [0.0, max / 2.0, max] {
        s.mixer.scroll = scroll;
        assert_eq!(master(&s, &g).rect, first, "scrolled to {scroll}");
    }
}

#[test]
fn a_channel_never_covers_the_master_and_the_last_one_ends_beside_it() {
    let (mut s, g) = narrow();
    let view = s.mixer_view(&g);
    let at = master(&s, &g).rect;
    assert!(view.right() <= at.x, "the view stops before the master");
    s.mixer.scroll = s.mixer_max_scroll(&g);
    let channels: Vec<_> = s
        .mixer_strips(&g)
        .into_iter()
        .filter(|x| !x.track.is_master())
        .collect();
    let last = channels.last().unwrap().rect;
    assert!(
        (last.right() + STRIP_GAP - view.right()).abs() < 0.01,
        "scrolled to the end, the last channel sits at the view's edge"
    );
    // Part of a channel reaches past the view when scrolled between two strips; the
    // point in the master still answers for the master.
    s.mixer.scroll = (STRIP_W + STRIP_GAP) / 2.0;
    let reach = s
        .mixer_strips(&g)
        .into_iter()
        .filter(|x| !x.track.is_master())
        .find(|x| x.rect.right() > view.right())
        .expect("a channel reaches under the master");
    let (x, y) = (at.x + at.w / 2.0, reach.rect.y + reach.rect.h / 2.0);
    assert!(reach.rect.contains(x, y) || reach.rect.right() > at.x);
    assert_eq!(
        s.mixer_hit(&g, x, y).map(MixerHit::track),
        Some(TrackId::MASTER)
    );
    // And between the view's edge and the master, nothing answers for the channel.
    let (gx, gy) = (view.right() + MASTER_GAP / 2.0, y);
    assert!(
        s.mixer_hit(&g, gx, gy)
            .is_none_or(|h| h.track().is_master() || view.contains(gx, gy))
    );
}

#[test]
fn the_master_and_the_channels_are_equally_tall() {
    let (s, g) = narrow();
    let strips = s.mixer_strips(&g);
    let m = strips.iter().find(|x| x.track.is_master()).unwrap();
    assert!(
        strips
            .iter()
            .all(|x| x.rect.y == m.rect.y && x.rect.h == m.rect.h)
    );
    assert!(
        m.rect.bottom() + BAR_GAP + BAR_H <= g.drawer_body.bottom(),
        "the bar has its room under the strips"
    );
}

#[test]
fn the_bar_shows_only_when_the_channels_do_not_fit() {
    let (mut s, g) = narrow();
    assert!(s.mixer_bar(&g).is_some());
    let wide = s.geometry(1900.0, H);
    assert_eq!(s.mixer_max_scroll(&wide), 0.0);
    assert!(s.mixer_bar(&wide).is_none());
    // The room stays: the strips are as tall as with the bar.
    assert_eq!(master(&s, &wide).rect.h, master(&s, &g).rect.h);
    s.tab = DrawerTab::Roll;
    assert!(s.mixer_bar(&g).is_none(), "no bar while the roll is shown");
}

#[test]
fn the_thumb_is_as_long_as_the_view_is_of_the_channels_and_follows_the_scroll() {
    let (mut s, g) = narrow();
    let (groove, thumb) = s.mixer_bar(&g).unwrap();
    assert_eq!(thumb.x, groove.x, "at the start the thumb is at the left");
    let view = s.mixer_view(&g);
    let max = s.mixer_max_scroll(&g);
    let want = groove.w * view.w / (view.w + max);
    assert!((thumb.w - want).abs() < 0.01 || thumb.w == BAR_MIN);
    s.mixer.scroll = max;
    let (_, thumb) = s.mixer_bar(&g).unwrap();
    assert!((thumb.right() - groove.right()).abs() < 0.01);
}

#[test]
fn dragging_the_thumb_scrolls_the_channels_and_letting_go_ends_it() {
    let (mut s, g) = narrow();
    let (groove, thumb) = s.mixer_bar(&g).unwrap();
    let (x, y) = (thumb.x + thumb.w / 2.0, thumb.y + thumb.h / 2.0);
    assert!(s.mixer_bar_down(&g, x, y));
    assert_eq!(s.mixer.scroll, 0.0, "pressing on the thumb moves nothing");
    assert!(s.mixer_bar_move(&g, x + 40.0));
    let span = groove.w - thumb.w;
    let want = 40.0 / span * s.mixer_max_scroll(&g);
    assert!(
        (s.mixer.scroll - want).abs() < 0.5,
        "{} {want}",
        s.mixer.scroll
    );
    // Past the end, it stops at the end.
    assert!(s.mixer_bar_move(&g, x + 10_000.0));
    assert_eq!(s.mixer.scroll, s.mixer_max_scroll(&g));
    assert!(s.mixer_bar_move(&g, x - 10_000.0));
    assert_eq!(s.mixer.scroll, 0.0);
    assert!(s.mixer_bar_up());
    assert!(!s.mixer_bar_move(&g, x + 40.0), "it is let go");
    assert!(!s.mixer_bar_up());
}

#[test]
fn a_press_on_the_groove_brings_the_thumb_under_the_pointer() {
    let (mut s, g) = narrow();
    let (groove, thumb) = s.mixer_bar(&g).unwrap();
    let (x, y) = (groove.right() - 2.0, groove.y + groove.h / 2.0);
    assert!(!thumb.contains(x, y));
    assert!(s.mixer_bar_down(&g, x, y));
    assert_eq!(s.mixer.scroll, s.mixer_max_scroll(&g));
    let (_, now) = s.mixer_bar(&g).unwrap();
    assert!(now.contains(x, y), "the thumb is under the pointer");
    assert!(s.mixer_bar_up());
}

#[test]
fn a_press_off_the_bar_does_not_take_it() {
    let (mut s, g) = narrow();
    let (groove, _) = s.mixer_bar(&g).unwrap();
    assert!(!s.mixer_bar_down(&g, groove.x + 20.0, groove.y - 60.0));
    assert!(s.mixer.bar_grab.is_none());
    let wide = s.geometry(1900.0, H);
    assert!(!s.mixer_bar_down(&wide, 100.0, groove.y));
}

#[test]
fn the_wheel_scrolls_the_channels_from_anywhere_but_the_lists() {
    let (mut s, g) = narrow();
    let at = master(&s, &g).rect;
    // On the master's name, not on one of its lists.
    let (x, y) = (at.x + at.w / 2.0, at.y + 8.0);
    assert!(s.mixer_wheel(&g, x, y, 0.0, -30.0));
    assert_eq!(s.mixer.scroll, 30.0);
    assert!(s.mixer_wheel(&g, x, y, 0.0, 10.0));
    assert_eq!(s.mixer.scroll, 20.0);
    assert!(s.mixer_wheel(&g, x, y, -10_000.0, 0.0));
    assert_eq!(s.mixer.scroll, s.mixer_max_scroll(&g));
}
