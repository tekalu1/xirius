//! MIDI keyboards plugged in and pulled out while the window is open (ENG-001, ENG-038).
//!
//! A thread of its own lists this PC's MIDI input ports by name every [`EVERY`] (a list opens
//! no port) and hands the list over when it changed; [`Studio::idle`] takes it. The studio
//! says the change in one line, and an open port list shows the ports there are now.
//!
//! Nothing in the song changes. Outside a recording, an added port opens for live MIDI.
//! Recording owns its input until the take ends; any pending open resumes afterwards.
//! A track that names a disconnected port keeps that name and its input-list entry.
//!
//! A screen without a device (tests, shots) watches nothing; the tests hand lists in.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use crate::studio::Studio;
#[cfg(test)]
use crate::tree::RowKind;
use xirius_core::model::TrackKind;

/// How often the thread lists the ports.
pub const EVERY: Duration = Duration::from_secs(1);

/// This PC's MIDI ports, as the studio last heard of them.
#[derive(Debug, Default)]
pub struct Ports {
    /// Lists from the watching thread, while it runs.
    rx: Option<Receiver<Vec<String>>>,
    /// The last list; `None` before the first, which is taken without a word.
    known: Option<Vec<String>>,
}

impl Ports {
    /// The ports as last listed (none before the first list).
    pub fn known(&self) -> &[String] {
        self.known.as_deref().unwrap_or_default()
    }
}

/// The ports `now` has that `before` had not, and those it lost, each in its list's order.
pub fn change(before: &[String], now: &[String]) -> (Vec<String>, Vec<String>) {
    let added = now.iter().filter(|p| !before.contains(p)).cloned();
    let removed = before.iter().filter(|p| !now.contains(p)).cloned();
    (added.collect(), removed.collect())
}

/// Lists the ports with `list` every `every` on a thread of its own, and sends each list that
/// differs from the one before (the first too). The thread ends once the receiver is gone.
pub fn spawn(
    list: impl Fn() -> Vec<String> + Send + 'static,
    every: Duration,
) -> Option<Receiver<Vec<String>>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("xirius-midi-watch".into())
        .spawn(move || {
            let mut last = None;
            loop {
                let now = list();
                if last.as_ref() != Some(&now) {
                    if tx.send(now.clone()).is_err() {
                        return;
                    }
                    last = Some(now);
                }
                std::thread::sleep(every);
            }
        })
        .ok()?;
    Some(rx)
}

/// The ports named in Japanese quotes, one after another (「A」「B」).
fn quoted(ports: &[String]) -> String {
    ports.iter().map(|p| format!("「{p}」")).collect()
}

impl Studio {
    /// Starts watching this PC's MIDI ports, with a device only.
    pub fn watch_midi_ports(&mut self) {
        if self.transport.mode == xirius_session::output::OutputMode::Device {
            if let Err(error) = self
                .song
                .io
                // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .enumerate_later()
            {
                self.error = Some(error);
            }
            let list = || xirius_engine::midi_in::midi_ports().unwrap_or_default();
            self.midi_ports.rx = spawn(list, EVERY);
        }
    }

    /// Watches the lists `rx` hands over, in place of this PC's.
    pub fn watch_midi_ports_from(&mut self, rx: Receiver<Vec<String>>) {
        self.midi_ports.rx = Some(rx);
    }

    /// Takes the newest list the watching thread handed over. True when something on screen
    /// changed.
    pub(crate) fn poll_midi_ports(&mut self) -> bool {
        let Some(rx) = &self.midi_ports.rx else {
            return false;
        };
        let mut newest = None;
        loop {
            match rx.try_recv() {
                Ok(list) => newest = Some(list),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.midi_ports.rx = None;
                    break;
                }
            }
        }
        newest.is_some_and(|now| self.midi_ports_now(now))
    }

    /// The ports are `now`: says what was plugged in or pulled out, in one line, and shows
    /// them in an open port list. The first list is taken without a word. True when
    /// something on screen changed.
    pub fn midi_ports_now(&mut self, now: Vec<String>) -> bool {
        self.song
            .io
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .view
            .ports = now.clone();
        if self.can_open_live_midi() && self.midi_ports.known() != now {
            self.release_midi();
            self.invalidate_midi_open();
            self.live_midi.resume = !now.is_empty();
            if !now.is_empty() && !self.transport.recording && !self.transport.pending_record {
                self.try_live_midi_open();
            }
        }
        let Some(before) = self.midi_ports.known.replace(now.clone()) else {
            return false;
        };
        let (added, removed) = change(&before, &now);
        if added.is_empty() && removed.is_empty() {
            return false;
        }
        if let Some(text) = self.port_line(&added, &removed) {
            self.tell(text);
        }
        self.refresh_port_list(now);
        true
    }

    /// The line a change says: what was plugged in, else what was pulled out.
    fn port_line(&self, added: &[String], removed: &[String]) -> Option<String> {
        if !added.is_empty() {
            Some(format!("{} 接続", quoted(added)))
        } else {
            Some(format!("{} 切断", quoted(removed)))
        }
    }

    /// An open port list of an instrument track lists `now`, and still the port its track names.
    fn refresh_port_list(&mut self, now: Vec<String>) {
        let Some(crate::popup::PopupKind::Input(t)) = self.popup.as_ref().map(|p| p.kind) else {
            return;
        };
        let current = {
            let p = self.project();
            match p.tracks.get(&t) {
                Some(track) if track.kind == TrackKind::Instrument => track.input.clone(),
                _ => return,
            }
        };
        // The list is built again from the watcher's `now` (UI-320): what was
        // typed, the cursor and the scroll stay.
        let lit = current.clone().unwrap_or_else(|| "none".into());
        let list = crate::pick_list::PickList::of_choices(
            crate::input_row::port_choices(now, current),
            &lit,
            "input",
            |to| crate::popup::Action::Input(to.to_string()),
        )
        .named("一致する入力はありません");
        if let Some(pick) = self.popup.as_mut().and_then(|p| p.pick.as_mut()) {
            pick.refill(list);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_row::{ALL_PORTS, set_test_ports};
    use crate::mixer::MixerHit;
    use crate::popup::Action;
    use crate::studio::{DrawerTab, Geometry, Hit};
    use xirius_core::id::TrackId;

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        s.settle_pool();
        let g = s.geometry(1600.0, 1000.0);
        (s, g)
    }

    fn track(s: &Studio, name: &str) -> TrackId {
        let p = s.project();
        p.tracks.values().find(|t| t.name == name).unwrap().id
    }

    fn ports(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn labels(s: &Studio) -> Vec<String> {
        let p = s.popup.as_ref().unwrap();
        p.choices().iter().map(|c| c.label.clone()).collect()
    }

    /// Polls as the window's timer does until a change shows.
    fn until_changed(s: &mut Studio) {
        let begun = std::time::Instant::now();
        while !s.idle().changed {
            assert!(begun.elapsed() < Duration::from_secs(60), "no change came");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_changes_are_the_ports_added_and_those_lost() {
        let (a, r) = change(&ports(&["A", "B"]), &ports(&["B", "C", "D"]));
        assert_eq!((a, r), (ports(&["C", "D"]), ports(&["A"])));
        let (a, r) = change(&ports(&["A"]), &ports(&["A"]));
        assert!(a.is_empty() && r.is_empty());
    }

    #[test]
    fn the_thread_sends_only_lists_that_changed() {
        let lists = std::sync::Mutex::new(vec![
            ports(&[]),
            ports(&[]),
            ports(&["KeyStep 37"]),
            ports(&["KeyStep 37"]),
            ports(&[]),
            ports(&["A"]),
        ]);
        let rx = spawn(
            move || {
                let mut l = lists.lock().unwrap();
                if l.len() > 1 {
                    l.remove(0)
                } else {
                    l[0].clone()
                }
            },
            Duration::from_millis(10),
        )
        .unwrap();
        let got: Vec<_> = (0..4)
            .map(|_| rx.recv_timeout(Duration::from_secs(10)).unwrap())
            .collect();
        assert_eq!(
            got,
            [
                ports(&[]),
                ports(&["KeyStep 37"]),
                ports(&[]),
                ports(&["A"])
            ]
        );
        // The list stays: nothing more comes.
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn plugging_in_a_keyboard_says_so_and_the_selected_instrument_takes_it() {
        let (mut s, _) = studio();
        let keys = track(&s, "Keys");
        s.selected_row = Some(RowKind::Track(keys));
        let (tx, rx) = std::sync::mpsc::channel();
        s.watch_midi_ports_from(rx);
        // The first list, as the window opens: nothing is said.
        tx.send(ports(&[])).unwrap();
        tx.send(ports(&[])).unwrap();
        let begun = std::time::Instant::now();
        while s.midi_ports.known.is_none() {
            assert!(begun.elapsed() < Duration::from_secs(60));
            s.idle();
        }
        assert_eq!(s.notice_text(), None);
        let steps = s.song.steps();
        tx.send(ports(&["KeyStep 37"])).unwrap();
        until_changed(&mut s);
        assert_eq!(s.notice_text(), Some("「KeyStep 37」 接続"));
        assert_eq!(s.midi_ports.known(), ports(&["KeyStep 37"]));
        // No setting: Keys still takes every port, and the song is as it was.
        assert_eq!(s.project().tracks[&keys].input, None);
        assert_eq!(s.song.steps(), steps);
        // A list the same as the one before says nothing again.
        assert!(!s.midi_ports_now(ports(&["KeyStep 37"])));
    }

    #[test]
    fn a_keyboard_plugged_in_while_recording_is_for_the_next_take() {
        let (mut s, _) = studio();
        s.midi_ports_now(ports(&[]));
        s.transport.recording = true;
        assert!(s.midi_ports_now(ports(&["KeyStep 37"])));
        assert_eq!(s.notice_text(), Some("「KeyStep 37」 接続"));
        assert!(s.midi_ports_now(ports(&[])));
        assert_eq!(s.notice_text(), Some("「KeyStep 37」 切断"));
        s.transport.recording = false;
    }

    #[test]
    fn a_port_pulled_out_leaves_the_song_that_names_it_as_it_is() {
        set_test_ports(Some(ports(&["KeyStep 37", "loopMIDI Port"])));
        let (mut s, g) = studio();
        let keys = track(&s, "Keys");
        s.midi_ports_now(ports(&["KeyStep 37", "loopMIDI Port"]));
        // Keys names the KeyStep; its list is open.
        let (x, y) = s.mixer_center(&g, MixerHit::Input(keys)).unwrap();
        assert_eq!(s.hit(&g, x, y), Hit::Mixer(MixerHit::Input(keys)));
        assert!(s.press(&g, Hit::Mixer(MixerHit::Input(keys))));
        s.popup_action(&g, Action::Input("KeyStep 37".into()));
        assert!(s.press(&g, Hit::Mixer(MixerHit::Input(keys))));
        assert_eq!(labels(&s), [ALL_PORTS, "KeyStep 37", "loopMIDI Port"]);
        let steps = s.song.steps();
        let saved = s.project().tracks[&keys].clone();
        // Pulled out: said, the open list is made again, and the song keeps the name.
        assert!(s.midi_ports_now(ports(&["loopMIDI Port"])));
        assert_eq!(s.notice_text(), Some("「KeyStep 37」 切断"));
        assert_eq!(labels(&s), [ALL_PORTS, "loopMIDI Port", "KeyStep 37"]);
        assert_eq!(s.popup.as_ref().unwrap().current(), "KeyStep 37");
        assert_eq!(s.song.steps(), steps);
        assert_eq!(*s.project().tracks[&keys], *saved);
        assert_eq!(
            crate::input_row::label(&s.project().tracks[&keys]),
            "KeyStep 37"
        );
        // Plugged in again: the list has it in its place, and Keys takes it once more.
        s.selected_row = Some(RowKind::Track(keys));
        assert!(s.midi_ports_now(ports(&["KeyStep 37", "loopMIDI Port"])));
        assert_eq!(labels(&s), [ALL_PORTS, "KeyStep 37", "loopMIDI Port"]);
        assert_eq!(s.notice_text(), Some("「KeyStep 37」 接続"));
        set_test_ports(None);
    }

    #[test]
    fn an_instrument_naming_another_port_is_not_told_it_takes_a_new_one() {
        let (mut s, _) = studio();
        let keys = track(&s, "Keys");
        s.selected_row = Some(RowKind::Track(keys));
        assert!(s.run_command(
            "track.input.set",
            serde_json::json!({ "ids": [keys.to_string()], "input": "loopMIDI Port" }),
        ));
        s.midi_ports_now(ports(&["loopMIDI Port"]));
        assert!(s.midi_ports_now(ports(&["loopMIDI Port", "KeyStep 37"])));
        assert_eq!(s.notice_text(), Some("「KeyStep 37」 接続"));
    }

    #[test]
    fn without_a_device_no_port_is_watched() {
        let (mut s, _) = studio();
        s.watch_midi_ports();
        assert!(s.midi_ports.rx.is_none());
        assert!(!s.idle().changed);
    }
}
