use crate::{
    shot::{Act, Part, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "recover01-crashed",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(crate::plugin_reload_shots::prepare), 0),
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Studio(crate::plugin_crash_shots::crash), 0),
            Act::Press(Target::Strip(Part::Insert, "Vocal")),
            Act::Stage(Play::Laid(crate::plugin_crash_shots::card), 0),
        ],
    },
    Shot {
        name: "recover02-restarted",
        acts: &[
            Act::Press(Target::PopupText("再起動")),
            Act::Stage(Play::Studio(crate::plugin_reload_shots::restored), 0),
        ],
    },
    Shot {
        name: "recover03-crashed-again",
        acts: &[
            Act::Stage(Play::Studio(crate::plugin_crash_shots::crash), 0),
            Act::Press(Target::Strip(Part::Insert, "Vocal")),
            Act::Stage(Play::Laid(crate::plugin_crash_shots::card), 0),
        ],
    },
    Shot {
        name: "recover04-removed",
        acts: &[
            Act::Press(Target::PopupText("外す")),
            Act::Stage(Play::Studio(removed), 0),
            Act::Press(Target::Top(BarButton::Stop)),
        ],
    },
];

fn removed(s: &mut Studio, _: u8) {
    assert!(s.popup.is_none());
    assert!(
        s.project()
            .tracks
            .values()
            .find(|t| t.name == "Vocal")
            .unwrap()
            .inserts
            .is_empty()
    );
    crate::shot::run_for(s, 5.0, |s| {
        s.transport.host().unwrap().plugins().reports().is_empty()
    });
    assert!(s.transport.host().unwrap().plugins().reports().is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Geometry;
    use crate::{health::PlugAct, mixer::MixerHit, popup::Action};

    fn open(s: &mut Studio, g: &Geometry) {
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Vocal")
            .unwrap()
            .id;
        let id = crate::plugin_reload_shots::id(s);
        let (x, y) = s.mixer_center(g, MixerHit::Open(t, id)).unwrap();
        assert!(s.chain_down(g, x, y));
        assert!(s.fx_up(g, x, y, false));
        crate::plugin_crash_shots::card(s, g, 0);
    }

    fn click(s: &mut Studio, g: &Geometry, action: PlugAct) {
        let layout = s.popup.as_ref().unwrap().layout(g.size);
        let rect = layout
            .cells
            .iter()
            .find(|c| c.action == Some(Action::Plugin(action)))
            .unwrap()
            .rect;
        let (x, y) = (rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
        assert!(s.press(g, s.hit(g, x, y)));
    }

    #[test]
    fn restart_restores_the_live_fixture_value_and_remove_empties_the_slot_in_one_undo() {
        let mut s = Studio::sample();
        crate::plugin_reload_shots::prepare(&mut s, 0);
        assert!(s.toggle_play());
        crate::plugin_crash_shots::crash(&mut s, 0);
        let project = s.project();
        let steps = s.song().steps();
        let g = s.geometry(1600.0, 900.0);
        open(&mut s, &g);
        click(&mut s, &g, PlugAct::Restart);
        assert!(s.popup.is_none());
        crate::plugin_reload_shots::restored(&mut s, 0);
        assert_eq!(*s.project(), *project);
        assert_eq!(s.song().steps(), steps);
        crate::plugin_crash_shots::crash(&mut s, 0);
        open(&mut s, &g);
        click(&mut s, &g, PlugAct::Remove);
        removed(&mut s, 0);
        assert_eq!(s.song().steps(), steps + 1);
        assert!(s.run_command("history.undo", serde_json::json!({})));
        assert_eq!(*s.project(), *project);
        s.transport.pause();
    }
}
