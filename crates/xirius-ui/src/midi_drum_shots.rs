use crate::{
    midi_auto_shots as midi,
    shot::{Act, Play, Shot, Target},
    studio::Studio,
};
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "drum01-selected",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(prepare), 0),
            Act::Press(Target::Track("Drums")),
        ],
    },
    Shot {
        name: "drum02-connected",
        acts: &[Act::Stage(Play::Studio(plug), 0)],
    },
    Shot {
        name: "drum03-unplugged",
        acts: &[Act::Stage(Play::Studio(unplug), 0)],
    },
];
fn prepare(s: &mut Studio, _: u8) {
    midi::setup(s, "Drums", "Drum Kit", 36);
}
fn plug(s: &mut Studio, _: u8) {
    midi::connect(s, "Drums");
}
fn unplug(s: &mut Studio, _: u8) {
    midi::disconnect(s, "Drums");
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{studio::Hit, tree::RowKind};
    use std::sync::atomic::Ordering;
    #[test]
    fn hotplug_and_replug_play_a_kick_on_the_selected_unarmed_drum_kit() {
        let mut s = Studio::sample();
        let opened = midi::setup(&mut s, "Drums", "Drum Kit", 36);
        let t = midi::track(&s, "Drums");
        let g = s.geometry(1600.0, 1000.0);
        assert!(s.press(&g, Hit::Row(RowKind::Track(t))));
        let steps = s.song.steps();
        midi::connect(&mut s, "Drums");
        assert_eq!(opened.load(Ordering::Relaxed), 1);
        assert_eq!(
            crate::instrument::current(&s.project().tracks[&t])
                .unwrap()
                .uid,
            "Drum Kit"
        );
        midi::disconnect(&mut s, "Drums");
        s.mixer.meters.clear();
        midi::connect(&mut s, "Drums");
        assert_eq!(opened.load(Ordering::Relaxed), 2);
        assert_eq!(s.song.steps(), steps);
        midi::disconnect(&mut s, "Drums");
        s.transport.reset();
    }
}
