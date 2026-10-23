//! The MIDI output ports the output list of an instrument track offers (ENG-028).

use std::cell::RefCell;

use xirius_core::model::{MidiOut, Track};

use crate::popup::Choice;

thread_local! {
    static TEST_PORTS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// What a choice's `to` starts with when it names a MIDI port.
pub const PREFIX: &str = "midi:";

/// Lists `ports` instead of this PC's MIDI output ports (`None`: this PC's again).
pub fn set_test_ports(ports: Option<Vec<String>>) {
    TEST_PORTS.with(|t| *t.borrow_mut() = ports);
}

/// The MIDI output ports to list. With a device, this PC's by name (no port is
/// opened); without one, none.
pub fn output_ports(device: bool) -> Vec<String> {
    if let Some(ports) = TEST_PORTS.with(|t| t.borrow().clone()) {
        return ports;
    }
    if !device {
        return Vec::new();
    }
    xirius_engine::midi_out::midi_out_ports().unwrap_or_default()
}

/// The rows for the ports: each port, and the one the track names even when this
/// PC has no such port now, so the row still shows what was chosen.
pub fn port_choices(mut ports: Vec<String>, current: Option<&MidiOut>) -> Vec<Choice> {
    if let Some(m) = current
        && !ports.contains(&m.port)
    {
        ports.push(m.port.clone());
    }
    ports
        .into_iter()
        .map(|n| Choice {
            to: format!("{PREFIX}{n}"),
            label: n,
        })
        .collect()
}

/// What the strip's output field says of a track that plays to a MIDI port: the
/// port, and the channel when it is not the first.
pub fn label(track: &Track) -> Option<String> {
    let m = track.midi_out.as_ref()?;
    Some(if m.channel == 1 {
        m.port.clone()
    } else {
        format!("{} {}", m.port, m.channel)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::MixerHit;
    use crate::popup::{Action, PopupKind};
    use crate::studio::{DrawerTab, Geometry, Hit, Studio};

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        let g = s.geometry(1600.0, 1000.0);
        (s, g)
    }

    fn track(s: &Studio, name: &str) -> xirius_core::id::TrackId {
        s.project()
            .tracks
            .values()
            .find(|t| t.name == name)
            .unwrap()
            .id
    }

    /// Presses the output field of `t` where the screen shows it.
    fn press_output(s: &mut Studio, g: &Geometry, t: xirius_core::id::TrackId) -> bool {
        let (x, y) = s.mixer_center(g, MixerHit::Output(t)).expect("a field");
        assert_eq!(s.hit(g, x, y), Hit::Mixer(MixerHit::Output(t)));
        s.press(g, Hit::Mixer(MixerHit::Output(t)))
    }

    fn labels(s: &Studio) -> Vec<String> {
        let p = s.popup.as_ref().unwrap();
        p.choices().iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn an_instrument_output_lists_the_ports_and_a_pick_sets_it() {
        set_test_ports(Some(vec!["Loop A".into(), "Loop B".into()]));
        let (mut s, g) = studio();
        let keys = track(&s, "Keys");
        assert!(press_output(&mut s, &g, keys));
        let ls = labels(&s);
        assert!(ls.ends_with(&["Loop A".into(), "Loop B".into()]), "{ls:?}");
        assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Output(keys));

        let before = s.song().steps();
        s.popup_action(&g, Action::Output("midi:Loop B".into()));
        assert_eq!(s.song().steps(), before + 1);
        let shown = {
            let p = s.project();
            assert_eq!(p.tracks[&keys].midi_out.as_ref().unwrap().port, "Loop B");
            crate::draw::output_label(&p, &p.tracks[&keys])
        };
        assert_eq!(shown, "Loop B");

        // The port is gone from this PC: the list still shows it, lit.
        set_test_ports(Some(vec!["Loop A".into()]));
        assert!(press_output(&mut s, &g, keys));
        assert!(labels(&s).contains(&"Loop B".to_string()));
        assert_eq!(s.popup.as_ref().unwrap().current(), "midi:Loop B");

        // The master takes the track back.
        s.popup_action(&g, Action::Output("master".into()));
        assert_eq!(s.project().tracks[&keys].midi_out, None);
        set_test_ports(None);
    }

    #[test]
    fn an_audio_track_lists_no_port_and_without_a_device_none_is_listed() {
        set_test_ports(Some(vec!["Loop A".into()]));
        let (mut s, g) = studio();
        let vocal = track(&s, "Vocal");
        assert!(press_output(&mut s, &g, vocal));
        assert!(!labels(&s).contains(&"Loop A".to_string()));
        set_test_ports(None);
        assert!(output_ports(false).is_empty());
        s.popup = None;
        let keys = track(&s, "Keys");
        assert!(press_output(&mut s, &g, keys));
        assert!(
            s.popup
                .as_ref()
                .unwrap()
                .choices()
                .iter()
                .all(|c| !c.to.starts_with(PREFIX))
        );
    }

    #[test]
    fn the_label_adds_a_channel_other_than_the_first() {
        let mut t = Studio::sample()
            .project()
            .tracks
            .values()
            .next()
            .unwrap()
            .as_ref()
            .clone();
        assert_eq!(label(&t), None);
        t.midi_out = Some(MidiOut {
            port: "Synth".into(),
            channel: 1,
        });
        assert_eq!(label(&t).as_deref(), Some("Synth"));
        t.midi_out.as_mut().unwrap().channel = 10;
        assert_eq!(label(&t).as_deref(), Some("Synth 10"));
    }
}
