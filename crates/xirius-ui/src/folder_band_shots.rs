//! ARR-003: a folded folder in the lanes, pressed through its chevron: the clips of its tracks
//! at any depth lie as bands, and an open folder draws none.
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::Studio;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "band01-open",
        acts: &[Act::Reset, Act::Stage(Play::Studio(stage), 0)],
    },
    Shot {
        name: "band02-percussion-folded",
        acts: &[
            Act::Press(Target::Chevron("Percussion")),
            Act::Stage(Play::Studio(stage), 1),
        ],
    },
    Shot {
        name: "band03-rhythm-folded",
        acts: &[
            Act::Press(Target::Chevron("Rhythm")),
            Act::Stage(Play::Studio(stage), 2),
        ],
    },
];

fn stage(s: &mut Studio, n: u8) {
    let rhythm = crate::arrange::probe::folder(s, "Rhythm");
    let perc = crate::arrange::probe::folder(s, "Percussion");
    match n {
        0 => {
            // The arrangement alone, tall enough for every row.
            s.drawer_open = false;
            assert!(!s.is_collapsed(rhythm) && !s.is_collapsed(perc));
        }
        1 => {
            assert!(s.is_collapsed(perc), "the chevron folded the folder");
            assert!(!crate::arrange::folder_band::spans(&s.project(), perc).is_empty());
        }
        _ => {
            assert!(s.is_collapsed(rhythm));
            let all = crate::arrange::folder_band::spans(&s.project(), rhythm);
            let inner = crate::arrange::folder_band::spans(&s.project(), perc);
            assert!(
                inner
                    .iter()
                    .all(|(a, b)| all.iter().any(|(x, y)| x <= a && b <= y))
            );
        }
    }
}
