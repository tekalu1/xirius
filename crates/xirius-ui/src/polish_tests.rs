use crate::{
    mixer::STRIP_GAP,
    studio::{DrawerTab, Studio},
};
use xirius_design::Space;

#[test]
fn the_roll_fills_the_1280_dock_and_channels_adjoin_buses() {
    let mut s = Studio::sample();
    let g = s.geometry(1280.0, 816.0);
    let f = s.roll.frame(g.drawer_body);
    assert_eq!(f.grid.right(), g.drawer_body.right());
    assert_eq!(f.keys.x, g.drawer_body.x);
    s.command(|song| song.add_track("bus"));
    assert!(s.error.is_none());
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(1280.0, 816.0);
    let strips = s.mixer_strips(&g);
    assert!(
        strips
            .iter()
            .any(|r| s.project().tracks[&r.track].kind == xirius_core::model::TrackKind::Bus)
    );
    // The master stands apart at the right edge; the channels adjoin one another.
    let channels: Vec<_> = strips.iter().filter(|r| !r.track.is_master()).collect();
    for pair in channels.windows(2) {
        assert!((pair[1].rect.x - pair[0].rect.right() - STRIP_GAP).abs() < 0.1);
    }
}

#[test]
fn the_last_track_has_room_below_it_at_960() {
    let mut s = Studio::sample();
    let g = s.geometry(960.0, 696.0);
    s.scroll_by(&g, 0.0, -10000.0, false);
    let row = s.tree().rows.last().unwrap();
    let bottom = s.row_y(&g, row) + row.full_h();
    assert!(g.lanes.bottom() - bottom >= Space::X6.px() - 0.1);
}

#[test]
fn floating_surfaces_and_all_four_track_switches_have_design_tokens() {
    for theme in xirius_design::Theme::ALL {
        let c = crate::paint::Colors::new(theme);
        assert!(c.float.iter().all(|s| f32::from(s.blur_radius) >= 6.0));
        assert_ne!(c.arm_on.0, c.monitor_on.0);
        for toggle in xirius_design::Toggle::ALL {
            let pair = toggle.on(theme);
            assert!(xirius_design::contrast_ratio(pair.fill, pair.label) >= 4.5);
        }
    }
}
