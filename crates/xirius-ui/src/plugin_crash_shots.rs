use crate::{
    shot::{Act, Part, Play, Shot, Target},
    studio::{Geometry, Studio},
    topbar::BarButton,
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "crash01-insert",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(crate::plugin_reload_shots::prepare), 0),
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Studio(crash), 0),
        ],
    },
    Shot {
        name: "crash02-card",
        acts: &[
            Act::Press(Target::Strip(Part::Insert, "Vocal")),
            Act::Stage(Play::Laid(card), 0),
        ],
    },
];

pub(crate) fn crash(s: &mut Studio, _: u8) {
    let id = crate::plugin_reload_shots::id(s);
    // The first two failures restart automatically. The third leaves the existing manual
    // recovery actions available, so these scenes exercise that final state.
    for attempt in 0..3 {
        crate::plugin_reload_shots::ready(s);
        let host = s.transport.host().unwrap();
        let key = host
            .plugins()
            .param_titles(id)
            .into_iter()
            .find(|(_, title)| title == "Crash")
            .unwrap()
            .0
            .to_string();
        let link = host.plugins().link(id).unwrap();
        let server = host.plugins().server_of(id).unwrap().clone();
        let old_pid = server.pid();
        let before = server.status(&link).crashes;
        assert!(server.automate(&link, &key, 1.0, 0));
        assert!(server.automate(&link, &key, 0.0, 1));
        crate::shot::run_for(s, 15.0, |_| server.status(&link).crashes > before);
        assert!(
            server.status(&link).crashes > before,
            "attempt={attempt}, before={before}, status={:?}, pid={:?}, playing={}",
            server.status(&link),
            server.pid(),
            s.transport.playing
        );
        if attempt < 2 {
            crate::shot::run_for(s, 15.0, |s| {
                s.transport.host().is_some_and(|h| {
                    h.plugins()
                        .server_of(id)
                        .is_some_and(|next| next.pid().is_some() && next.pid() != old_pid)
                })
            });
        }
    }
    crate::shot::run_for(s, 15.0, |s| s.health.crashed(id));
    s.poll_plugins_now();
    assert!(s.health.fake.is_none());
    assert!(s.health.crashed(id));
    let report = s
        .transport
        .host()
        .unwrap()
        .plugins()
        .reports()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert_eq!(report.status.exit_code, Some(86));
}

pub(crate) fn card(s: &mut Studio, g: &Geometry, _: u8) {
    let popup = s.popup.as_ref().unwrap();
    let layout = popup.layout(g.size);
    let text = layout
        .cells
        .iter()
        .filter(|c| !c.text.is_empty())
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(text, ["Xirius Test Latency", "再起動", "外す"]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::MixerHit;
    #[test]
    fn a_real_fixture_exit_opens_only_the_name_and_two_actions_from_its_insert_row() {
        let _records = crate::shot::CrashRecords::new();
        let mut s = Studio::sample();
        crate::plugin_reload_shots::prepare(&mut s, 0);
        assert!(s.toggle_play());
        crash(&mut s, 0);
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Vocal")
            .unwrap()
            .id;
        let id = crate::plugin_reload_shots::id(&s);
        let g = s.geometry(1600.0, 900.0);
        let (x, y) = s.mixer_center(&g, MixerHit::Open(track, id)).unwrap();
        assert!(s.chain_down(&g, x, y));
        assert!(s.fx_up(&g, x, y, false));
        card(&mut s, &g, 0);
        s.transport.pause();
    }
}
