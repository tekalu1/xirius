//! `--script fill-ink` (UI-309): faces painted in the primary fill, and the ink on them. The track
//! head's lit switches (arrange, tempo, follow), the stopped play button under the pointer, and a
//! fader's cap under the pointer. Take it once as it is and once with `--light`: in the light
//! theme the lit switches' icons were black. Silent; the pointer is the studio's own, no OS input.
//! The other faces fixed with these have scenes already: the window's close button under the
//! pointer is `--script caption` (caption-04-hover-close), the settings' detect button
//! `--script audioset` (a01-audio), a pinged plug-in's row `--script cpu-ping`.
use crate::shot::{Act, Part, Shot, Target};
use crate::studio::DrawerTab;
use crate::topbar::BarButton;

pub const SHOTS: &[Shot] = &[
    // Arrange, tempo and follow are on in the sample; the pointer on the arrange switch.
    Shot {
        name: "fi01-lit-switches",
        acts: &[Act::Reset, Act::Hover(Target::ArrangeToggle)],
    },
    // Stopped, the play button under the pointer wears the primary hover face.
    Shot {
        name: "fi02-play-hover",
        acts: &[Act::Hover(Target::Top(BarButton::Play))],
    },
    // The cap of the fader under the pointer, and the grip on it.
    Shot {
        name: "fi03-fader-hover",
        acts: &[
            Act::Press(Target::Tab(DrawerTab::Mixer)),
            Act::Hover(Target::Strip(Part::Fader, "Keys")),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::toggle_colors;
    use crate::mixer::MixerHit;
    use crate::paint::Colors;
    use crate::studio::{Hit, Studio};
    use xirius_design::{Layout, Theme};

    /// Each shot played without a window, and `check` on the studio after it.
    fn play(check: impl Fn(&str, &Studio)) {
        let (w, h) = (1440.0, 860.0 - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H);
        let mut s = Studio::sample();
        for shot in SHOTS {
            for act in shot.acts {
                let g = s.geometry(w, h);
                match *act {
                    Act::Reset => s = Studio::sample(),
                    Act::Hover(t) | Act::Press(t) => {
                        let hit = crate::shot::resolve(&s, t)
                            .unwrap_or_else(|| panic!("{}: {t:?}", shot.name));
                        let (x, y) = s
                            .center_of(&g, hit)
                            .unwrap_or_else(|| panic!("{}: {t:?} off screen", shot.name));
                        assert_eq!(s.hit(&g, x, y), hit, "{}: {t:?}", shot.name);
                        if matches!(act, Act::Press(_)) {
                            s.press(&g, hit);
                        } else {
                            s.hover = hit;
                        }
                    }
                    _ => unreachable!("{}", shot.name),
                }
                assert_eq!(s.error, None, "{}", shot.name);
            }
            check(shot.name, &s);
        }
    }

    #[test]
    fn the_script_is_listed() {
        let listed = crate::SCRIPTS.iter().find(|(name, _)| *name == "fill-ink");
        let names = |shots: &[Shot]| shots.iter().map(|s| s.name).collect::<Vec<_>>();
        assert_eq!(listed.map(|(_, s)| names(s)), Some(names(SHOTS)));
    }

    #[test]
    fn each_shot_shows_a_fill_face_with_the_ink_on_it() {
        play(|name, s| match name {
            "fi01-lit-switches" => {
                assert!(s.arrange_visible && s.tempo_visible && s.follow.on);
                assert_eq!(s.hover, Hit::ToggleArrange);
            }
            "fi02-play-hover" => {
                assert!(!s.transport.playing);
                assert_eq!(s.bar_view().hover, Some(BarButton::Play));
            }
            "fi03-fader-hover" => {
                let Hit::Mixer(MixerHit::Fader(_)) = s.hover else {
                    panic!("the pointer is on {:?}", s.hover);
                };
            }
            _ => panic!("{name}"),
        });
        for theme in Theme::ALL {
            let c = Colors::new(theme);
            assert_eq!(toggle_colors(&c, true, true), (c.fill, c.on_fill));
            assert_eq!(c.primary_hover, (c.fill_hover, c.on_fill));
        }
    }
}
