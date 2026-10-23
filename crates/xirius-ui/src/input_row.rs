//! The mixer's input row (REC-001): what its well says and what its list holds.
//!
//! An audio track without a named input records the device's first pair
//! (`In 1+2`, `In 1` from a mono device). A strip is narrow, so the well names
//! that pair, as the output well names where the strip plays, and the list
//! says it is the default (既定（In 1+2）, lit).
//!
//! An instrument track names a MIDI input port of this PC; none is every port.
//! Its well then says `MIDI` (mock v4) and the list すべての入力, lit. The ports
//! are only listed, never opened: the recording opens the one it takes. A
//! screen without a device lists none.
//!
//! The window reads how many channels the device has once as it opens, on a
//! thread of its own ([`Studio::read_input_channels`]): only its format, so the
//! device is not opened, and a mono device's well says `In 1` from the start.

use std::cell::{Cell, RefCell};
#[cfg(not(test))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;

use xirius_core::model::{Track, TrackKind};

use crate::popup::Choice;

/// The list's row of every MIDI port, an instrument track without a named port.
pub const ALL_PORTS: &str = "すべての入力";

/// What the well of such a track says (short enough for a strip).
pub const ANY_MIDI: &str = "MIDI";

/// The channels of the device's default input last read (2 until then).
#[cfg(not(test))]
static CHANNELS: AtomicUsize = AtomicUsize::new(2);

#[cfg(test)]
thread_local! {
    /// Each test keeps its own, so one that reads a mono device leaves the others at 2.
    static CHANNELS: Cell<usize> = const { Cell::new(2) };
}

thread_local! {
    /// Port names the shots and the tests list instead of this PC's.
    static TEST_PORTS: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    /// The channels the shots and the tests read instead of the device's.
    static TEST_CHANNELS: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Keeps how many channels the device has, read as the window opens and when
/// the list opens, so the well can name the default without asking the device
/// every frame.
pub fn note_channels(n: usize) {
    #[cfg(not(test))]
    CHANNELS.store(n, Ordering::Relaxed);
    #[cfg(test)]
    CHANNELS.with(|c| c.set(n));
}

/// How many channels were last noted.
pub fn noted_channels() -> usize {
    #[cfg(not(test))]
    return CHANNELS.load(Ordering::Relaxed);
    #[cfg(test)]
    return CHANNELS.with(Cell::get);
}

/// Reads `n` channels instead of the device's (`None`: the device's again).
pub fn set_test_channels(n: Option<usize>) {
    TEST_CHANNELS.with(|t| t.set(n));
}

/// The channels the list offers: the device's with one (its format is read,
/// the device is not opened), the test tone's two without one.
pub fn device_channels(device: bool) -> usize {
    if let Some(n) = TEST_CHANNELS.with(Cell::get) {
        return n;
    }
    if device {
        crate::input::device_channels()
    } else {
        2
    }
}

/// What the well and the list say when the device has no input.
pub const NO_INPUT: &str = "入力なし";

/// The input a track without a name records.
pub fn default_name() -> &'static str {
    match noted_channels() {
        0 => NO_INPUT,
        1 => "In 1",
        _ => "In 1+2",
    }
}

/// The list's row of the default.
pub fn default_label() -> String {
    format!("既定（{}）", default_name())
}

/// What the well says: the named input, else the default's channels or `MIDI`.
pub fn label(track: &Track) -> &str {
    match (track.input.as_deref(), track.kind) {
        (Some(name), _) => name,
        (None, TrackKind::Instrument) => ANY_MIDI,
        (None, _) => default_name(),
    }
}

/// Lists `ports` instead of this PC's MIDI ports (`None`: this PC's again).
pub fn set_test_ports(ports: Option<Vec<String>>) {
    TEST_PORTS.with(|t| *t.borrow_mut() = ports);
}

/// The MIDI input ports to list. With a device, this PC's by name (no port is
/// opened); without one, none.
pub fn midi_ports(device: bool) -> Vec<String> {
    if let Some(ports) = TEST_PORTS.with(|t| t.borrow().clone()) {
        return ports;
    }
    if !device {
        return Vec::new();
    }
    xirius_engine::midi_in::midi_ports().unwrap_or_default()
}

impl crate::studio::Studio {
    /// Reads how many channels the device has, on a thread of its own, so the
    /// window opens without waiting for the driver. Without a device nothing is
    /// read: the test tone has two.
    pub fn read_input_channels(&mut self) {
        if self.transport.mode == xirius_session::output::OutputMode::Device {
            self.read_input_channels_with(crate::input::device_channels);
        }
    }

    /// [`Self::read_input_channels`] with `read` in place of the device.
    pub fn read_input_channels_with(&mut self, read: impl FnOnce() -> usize + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("xirius-input-channels".into())
            .spawn(move || {
                let _ = tx.send(read());
            });
        if spawned.is_ok() {
            self.input_channels = Some(rx);
        }
    }

    /// Takes the channels read, once they are. True when the well's default
    /// changed, so a still window draws again.
    pub(crate) fn poll_input_channels(&mut self) -> bool {
        let Some(rx) = &self.input_channels else {
            return false;
        };
        let read = rx.try_recv();
        if read != Err(std::sync::mpsc::TryRecvError::Empty) {
            self.input_channels = None;
        }
        // A thread that ended without a count, or a device that could not be read, leaves the
        // default: that a device has no input is known from its list of inputs (`poll_io`).
        let Ok(n) = read else {
            return false;
        };
        if n == 0 {
            return false;
        }
        let before = default_name();
        note_channels(n);
        before != default_name()
    }
}

/// What reads the channels hands over, kept by the studio until it is taken.
pub type ChannelsRead = Option<Receiver<usize>>;

/// The list of an instrument track: every port, then each port. A port the song
/// names that is not here now stays in the list, so the row still shows it.
pub fn port_choices(mut ports: Vec<String>, current: Option<String>) -> Vec<Choice> {
    if let Some(current) = current
        && !ports.contains(&current)
    {
        ports.push(current);
    }
    let mut out = vec![Choice {
        to: "none".into(),
        label: ALL_PORTS.into(),
    }];
    out.extend(ports.into_iter().map(|n| Choice {
        to: n.clone(),
        label: n,
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::MixerHit;
    use crate::popup::{Action, PopupKind};
    use crate::studio::{DrawerTab, Geometry, Hit, Studio};

    /// The sample song with the mixer open.
    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        let g = s.geometry(1600.0, 1000.0);
        (s, g)
    }

    /// Presses the input well of `t` where the screen shows it.
    fn press_input(s: &mut Studio, g: &Geometry, t: xirius_core::id::TrackId) -> bool {
        let (x, y) = s.mixer_center(g, MixerHit::Input(t)).expect("a button");
        assert_eq!(s.hit(g, x, y), Hit::Mixer(MixerHit::Input(t)));
        s.press(g, Hit::Mixer(MixerHit::Input(t)))
    }

    fn track(s: &Studio, name: &str) -> xirius_core::id::TrackId {
        let p = s.project();
        p.tracks.values().find(|t| t.name == name).unwrap().id
    }

    fn labels(s: &Studio) -> Vec<String> {
        let p = s.popup.as_ref().unwrap();
        p.choices().iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn the_well_names_the_default_pair_and_the_list_says_it_is_the_default() {
        let (s, _) = studio();
        let p = s.project();
        let vocal = p.tracks.values().find(|t| t.name == "Vocal").unwrap();
        assert_eq!(vocal.input, None);
        // Short enough for the well: no 既定（ cut off at its end.
        assert_eq!(label(vocal), "In 1+2");
        assert!(crate::browser::text_width(label(vocal), xirius_design::FontSize::Caption) < 60.0);
        assert_eq!(default_label(), "既定（In 1+2）");
        let mut named = (**vocal).clone();
        named.input = Some("In 2".into());
        assert_eq!(label(&named), "In 2");
    }

    #[test]
    fn an_instrument_well_lists_the_midi_ports_and_sets_one() {
        set_test_ports(Some(vec!["KeyStep 37".into(), "loopMIDI Port".into()]));
        let (mut s, g) = studio();
        let keys = track(&s, "Keys");
        assert_eq!(label(&s.project().tracks[&keys]), ANY_MIDI);
        assert!(press_input(&mut s, &g, keys));
        let popup = s.popup.as_ref().expect("the port list");
        assert_eq!(popup.kind, PopupKind::Input(keys));
        assert_eq!(popup.current(), "none");
        assert_eq!(labels(&s), [ALL_PORTS, "KeyStep 37", "loopMIDI Port"]);
        let before = s.song().steps();
        s.popup_action(&g, Action::Input("loopMIDI Port".into()));
        assert_eq!(s.song().steps(), before + 1);
        let p = s.project();
        assert_eq!(p.tracks[&keys].input.as_deref(), Some("loopMIDI Port"));
        assert_eq!(label(&p.tracks[&keys]), "loopMIDI Port");
        drop(p);
        // The port is gone from this PC: the list still shows it, lit.
        set_test_ports(Some(vec!["KeyStep 37".into()]));
        assert!(press_input(&mut s, &g, keys));
        assert_eq!(labels(&s), [ALL_PORTS, "KeyStep 37", "loopMIDI Port"]);
        assert_eq!(s.popup.as_ref().unwrap().current(), "loopMIDI Port");
        s.popup_action(&g, Action::Input("none".into()));
        assert_eq!(s.project().tracks[&keys].input, None);
        set_test_ports(None);
    }

    #[test]
    fn without_a_device_no_port_is_listed() {
        set_test_ports(None);
        assert!(midi_ports(false).is_empty());
        let (mut s, g) = studio();
        let keys = track(&s, "Keys");
        assert!(press_input(&mut s, &g, keys));
        assert_eq!(labels(&s), [ALL_PORTS]);
    }

    #[test]
    fn a_mono_device_read_as_the_window_opens_names_in_1_from_the_start() {
        let (mut s, g) = studio();
        let vocal = track(&s, "Vocal");
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        s.read_input_channels_with(move || {
            // The driver answers late: the window does not wait for it.
            let _ = rx.recv();
            1
        });
        assert!(!s.idle().changed);
        assert_eq!(label(&s.project().tracks[&vocal]), "In 1+2");
        tx.send(()).unwrap();
        let begun = std::time::Instant::now();
        while !s.idle().changed {
            assert!(begun.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(s.input_channels.is_none());
        assert_eq!(label(&s.project().tracks[&vocal]), "In 1");
        assert_eq!(default_label(), "既定（In 1）");
        // The list says the same, read as a mono device.
        set_test_channels(Some(1));
        assert!(press_input(&mut s, &g, vocal));
        assert_eq!(labels(&s), ["既定（In 1）", "In 1"]);
        set_test_channels(None);
    }

    #[test]
    fn a_stereo_or_missing_device_keeps_in_1_plus_2() {
        for n in [2, 0] {
            let (mut s, _) = studio();
            s.read_input_channels_with(move || n);
            let begun = std::time::Instant::now();
            while s.input_channels.is_some() {
                assert!(begun.elapsed() < std::time::Duration::from_secs(10));
                assert!(!s.idle().changed, "{n} channels change nothing");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(default_name(), "In 1+2", "{n} channels");
        }
    }

    #[test]
    fn without_a_device_nothing_is_read() {
        let (mut s, _) = studio();
        assert_ne!(s.transport.mode, xirius_session::output::OutputMode::Device);
        s.read_input_channels();
        assert!(s.input_channels.is_none());
    }

    #[test]
    fn a_port_the_song_names_stays_in_the_list() {
        let to = |c: Vec<Choice>| c.into_iter().map(|c| c.to).collect::<Vec<_>>();
        assert_eq!(
            to(port_choices(vec!["A".into()], Some("B".into()))),
            ["none", "A", "B"]
        );
        assert_eq!(
            to(port_choices(vec!["A".into()], Some("A".into()))),
            ["none", "A"]
        );
    }
}
