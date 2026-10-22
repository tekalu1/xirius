//! `--script about` (and `--light`): the star and wordmark heading, and the software rows.

use crate::shot::{Act, Shot, Target};
use crate::topbar::BarButton;

pub const ABOUT: &[Shot] = &[
    Shot {
        name: "about-01-wordmark",
        acts: &[Act::Press(Target::Top(BarButton::Logo))],
    },
    // UI-303: the GitHub mark's balloon, its name and where it goes, under the mark.
    Shot {
        name: "about-02-github",
        acts: &[Act::Hover(Target::PopupTip("GitHub のリポジトリ"))],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::about::AboutLook;
    use crate::popup::Look;
    use crate::studio::{Hit, Studio};

    #[test]
    fn the_wordmark_shot_opens_the_heading_and_the_next_shot_finds_github() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        assert_eq!(ABOUT[0].name, "about-01-wordmark");
        for shot in ABOUT {
            for act in shot.acts {
                match *act {
                    Act::Press(target) => {
                        let hit = crate::shot::resolve(&s, target).unwrap();
                        assert!(s.press(&g, hit));
                    }
                    Act::Hover(target) => {
                        let Hit::Item(at) = crate::shot::resolve(&s, target).unwrap() else {
                            panic!("the GitHub button");
                        };
                        let layout = s.popup.as_ref().unwrap().layout(g.size);
                        assert_eq!(layout.cells[at].icon, Some("github"));
                    }
                    _ => panic!("the about shots need only a press and a hover"),
                }
            }
            let layout = s.popup.as_ref().unwrap().layout(g.size);
            let title = layout
                .cells
                .iter()
                .find(|c| c.look == Look::About(AboutLook::Title))
                .unwrap();
            assert_eq!(title.text, "Xirius");
        }
    }
}
