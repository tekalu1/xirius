use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use serde_json::json;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "tempo01-before",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(prepare), 0),
            Act::Press(Target::Bar(5)),
            Act::Press(Target::Top(BarButton::Metronome)),
        ],
    },
    Shot {
        name: "tempo02-after",
        acts: &[
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Studio(after), 0),
        ],
    },
    Shot {
        name: "tempo03-stop",
        acts: &[Act::Press(Target::Top(BarButton::Stop))],
    },
];
fn prepare(s: &mut Studio, _: u8) {
    s.drawer_open = false;
    assert!(s.run_command("tempo.set", json!({"at":0,"bpm":120})));
    assert!(s.run_command("tempo.set", json!({"at":19200,"bpm":180})));
    assert!(s.run_command("timesig.set", json!({"bar":6,"num":3,"den":8})));
    s.beat_w = 100.0;
    s.scroll.0 = s.ticks_to_px(15360);
    s.seek(15360);
    assert_eq!(s.bar_view().meter, "4/4");
}
fn after(s: &mut Studio, _: u8) {
    crate::shot::run_for(s, 8.0, |s| s.playhead >= 20640);
    assert!(s.playhead >= 20640, "{:?}", s.error);
    assert_eq!(s.bar_view().meter, "3/8");
    assert!(s.transport.metronome);
    assert_eq!(s.project().time_map().to_bbt(s.playhead).0, 7);
}
