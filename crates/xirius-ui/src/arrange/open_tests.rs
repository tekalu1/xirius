//! Opening a clip's editor: a double click on it, or F2 (EDT-042).

use super::probe::{clip_mid, track};
use super::*;
use crate::studio::DrawerTab;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    (s, g)
}

fn alt() -> Mods {
    Mods {
        alt: true,
        ..Mods::default()
    }
}

/// The two presses of a double click, as the window sends them: the arrange
/// takes each press and the second one also opens.
fn double_click(s: &mut Studio, g: &Geometry, at: (f32, f32), mods: Mods) {
    for n in 1..=2 {
        s.arrange_down(g, at.0, at.1, mods);
        if n == 2 {
            s.arrange_open(g, at.0, at.1, mods);
        }
        s.arrange_up(g, at.0, at.1, mods);
    }
}

#[test]
fn a_double_click_on_a_midi_clip_opens_the_roll_with_it() {
    let (mut s, g) = studio();
    assert!(!s.drawer_open);
    let drums = track(&s, "Drums");
    let at = clip_mid(&s, &g, "Drums", 0);
    double_click(&mut s, &g, at, Mods::default());
    assert!(s.drawer_open, "a shut drawer opens");
    assert_eq!(s.tab, DrawerTab::Roll);
    let want = s.project().tracks[&drums].clips[0].id;
    assert_eq!(s.selected_clip, Some((drums, want)));
    assert_eq!(s.roll_target().map(|t| t.clip.id), Some(want));
}

#[test]
fn the_mixer_tab_is_switched_to_the_roll() {
    let (mut s, g) = studio();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(g.size.0, g.size.1);
    let at = clip_mid(&s, &g, "Keys", 0);
    double_click(&mut s, &g, at, Mods::default());
    assert!(s.drawer_open);
    assert_eq!(s.tab, DrawerTab::Roll);
    assert!(s.roll_target().is_some());
}

#[test]
fn alt_zooms_the_roll_to_the_clip() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let at = clip_mid(&s, &g, "Keys", 0);
    let len = s.project().tracks[&keys].clips[0].length;
    s.roll.scroll.0 = 500.0;
    double_click(&mut s, &g, at, alt());
    let g2 = s.geometry(g.size.0, g.size.1);
    let ctx = s.roll_ctx(&g2).expect("the roll is open");
    let grid = s.roll.view(&ctx).grid;
    let beats = len as f32 / s.project().ppq as f32;
    let want = (grid.w / beats).clamp(crate::roll::BEAT_PX_MIN, crate::roll::BEAT_PX_MAX);
    assert!((s.roll.beat_px - want).abs() < 0.01, "{}", s.roll.beat_px);
    assert_eq!(s.roll.scroll.0, 0.0);

    // without Alt the zoom stays where it was
    s.roll.beat_px = 77.0;
    double_click(&mut s, &g, at, Mods::default());
    assert_eq!(s.roll.beat_px, 77.0);
}

/// The window slides a shut drawer open (UI-011). The roll is fitted to the
/// clip and its notes in the drawer it opens to, not in the first frame of the
/// slide, where the drawer has no height yet.
#[test]
fn the_roll_opens_on_the_clip_while_the_drawer_slides_open() {
    let open = |slides: bool| {
        let (mut s, g) = studio();
        s.motion.enabled.set(slides);
        s.roll.scroll = (500.0, 0.0);
        let at = clip_mid(&s, &g, "Keys", 0);
        double_click(&mut s, &g, at, alt());
        assert!(s.drawer_open);
        (s.roll.beat_px, s.roll.scroll)
    };
    let still = open(false);
    assert_eq!(still.1.0, 0.0);
    assert!(still.1.1 > 0.0, "the notes are brought into view");
    assert_eq!(open(true), still);
}

#[test]
fn an_audio_clip_is_selected_and_the_drawer_stays_as_it_was() {
    let (mut s, g) = studio();
    s.tab = DrawerTab::Mixer;
    let vocal = track(&s, "Vocal");
    let at = clip_mid(&s, &g, "Vocal", 0);
    double_click(&mut s, &g, at, Mods::default());
    let clip = s.project().tracks[&vocal].clips[0].id;
    assert_eq!(s.selected_clip, Some((vocal, clip)));
    assert!(!s.drawer_open);
    assert_eq!(s.tab, DrawerTab::Mixer);
}

#[test]
fn f2_opens_the_selected_clip_and_alt_f2_zooms_to_it() {
    let (mut s, g) = studio();
    let (t, c) = s.selected_clip.expect("the sample selects a clip");
    assert!(s.arrange_key(&g, "f2", Mods::default()));
    assert!(s.drawer_open);
    assert_eq!(s.tab, DrawerTab::Roll);
    assert_eq!(s.roll_target().map(|r| r.clip.id), Some(c));
    assert_eq!(s.selected_clip, Some((t, c)));

    s.roll.beat_px = 77.0;
    assert!(s.arrange_key(&g, "f2", alt()));
    assert_ne!(s.roll.beat_px, 77.0);
}

#[test]
fn the_cut_tool_splits_and_does_not_open() {
    let (mut s, g) = studio();
    s.tool = Tool::Cut;
    let at = clip_mid(&s, &g, "Drums", 0);
    s.arrange_down(&g, at.0, at.1, Mods::default());
    assert!(!s.arrange_open(&g, at.0, at.1, Mods::default()));
    assert!(!s.drawer_open);
}
