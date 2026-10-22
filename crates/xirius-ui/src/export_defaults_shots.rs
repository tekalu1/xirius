//! EXP-012: the export panel's values that differ from the song's own are blue, and the
//! balloon says what the song has. Real window presses and a hover.
use crate::{
    shot::Shot,
    shot::{Act, Target},
    topbar::BarButton,
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "default01-as-the-song",
        acts: &[Act::Reset, Act::Press(Target::Top(BarButton::Export))],
    },
    Shot {
        name: "default02-rate-moved-balloon",
        acts: &[
            Act::Press(Target::PopupText("44.1")),
            Act::Hover(Target::PopupText("44.1")),
        ],
    },
    Shot {
        name: "default03-three-moved",
        acts: &[
            Act::Press(Target::PopupText("16")),
            Act::Press(Target::PopupText("FLAC")),
            Act::Hover(Target::PopupText("FLAC")),
        ],
    },
];
