use crate::studio::Studio;
use xirius_engine::record::MidiInput;
use xirius_session::output::OutputMode;

pub type Opener = Box<dyn FnMut() -> Result<Box<dyn MidiInput>, String> + Send>;

pub struct Opening {
    generation: u64,
    work: xirius_session::background::Job<Opened>,
}
struct Opened {
    opener: Option<Opener>,
    result: Result<Box<dyn MidiInput>, String>,
}

pub(crate) fn retire(input: Box<dyn MidiInput>) {
    let _ = xirius_session::background::Job::spawn("live-midi-retire", move || {
        drop(input);
        Ok(())
    });
}

impl Studio {
    pub(crate) fn midi_open_failed(&mut self, error: String) {
        if self.error.is_none() || self.error == self.live_midi.opening_error {
            self.error = Some(error.clone());
        }
        self.live_midi.opening_error = Some(error.clone());
        self.live_midi.resume = true;
        self.live_midi.retry_at =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(250));
    }
    pub fn midi_opener(
        &mut self,
        opener: impl FnMut() -> Result<Box<dyn MidiInput>, String> + Send + 'static,
    ) {
        self.invalidate_midi_open();
        if let Some(previous) = self.live_midi.opener.replace(Box::new(opener)) {
            let _ = xirius_session::background::Job::spawn("midi-opener-retire", move || {
                drop(previous);
                Ok(())
            });
        }
    }
    pub(crate) fn can_open_live_midi(&self) -> bool {
        self.transport.mode == OutputMode::Device
            || self.live_midi.opener.is_some()
            || self.live_midi.opening.is_some()
    }

    pub(crate) fn invalidate_midi_open(&mut self) {
        self.live_midi.generation = self
            .live_midi
            .generation
            .checked_add(1)
            .expect("MIDI generations exhausted");
        if let Some(input) = self.live_midi.input.take() {
            retire(input);
        }
    }

    pub(crate) fn open_midi_port(&mut self) -> Option<Result<Box<dyn MidiInput>, String>> {
        if self.live_midi.opening.is_none() {
            if !self.can_open_live_midi() {
                return Some(Err("MIDI入力がありません".into()));
            }
            let mut opener = self.live_midi.opener.take();
            let work = match xirius_session::background::Job::spawn("live-midi-open", move || {
                let result = match &mut opener {
                    Some(open) => open(),
                    None => xirius_engine::midi_in::MidirInput::open(None)
                        .map(|input| Box::new(input) as Box<dyn MidiInput>),
                };
                Ok(Opened { opener, result })
            }) {
                Ok(work) => work,
                Err(error) => return Some(Err(error)),
            };
            self.live_midi.opening = Some(Opening {
                generation: self.live_midi.generation,
                work,
            });
        }
        let opening = self.live_midi.opening.as_ref().unwrap();
        let result = opening.work.poll()?;
        let current = opening.generation == self.live_midi.generation;
        self.live_midi.opening = None;
        let mut opened = match result {
            Ok(opened) => opened,
            Err(error) => return current.then_some(Err(error)),
        };
        if self.live_midi.opener.is_none() {
            self.live_midi.opener = opened.opener.take();
        }
        if let Some(previous) = opened.opener.take() {
            let _ = xirius_session::background::Job::spawn("midi-opener-retire", move || {
                drop(previous);
                Ok(())
            });
        }
        if !current {
            let _ = xirius_session::background::Job::spawn("stale-live-midi", move || {
                drop(opened);
                Ok(())
            });
            return None;
        }
        Some(opened.result)
    }

    pub(crate) fn poll_stale_midi_open(&mut self) {
        if self
            .live_midi
            .opening
            .as_ref()
            .is_some_and(|open| open.generation != self.live_midi.generation)
        {
            let _ = self.open_midi_port();
        }
    }

    pub(crate) fn try_live_midi_open(&mut self) -> bool {
        match self.open_midi_port() {
            Some(Ok(input)) => {
                self.live_midi_input(input);
                true
            }
            Some(Err(error)) => {
                let previous = self.error.clone();
                self.midi_open_failed(error);
                self.error != previous
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::{Duration, Instant};

    struct Midi {
        ports: Vec<String>,
        dropped: mpsc::Sender<std::thread::ThreadId>,
    }
    impl MidiInput for Midi {
        fn ports(&self) -> Vec<String> {
            self.ports.clone()
        }
        fn read(&mut self, _: usize, _: &mut dyn FnMut(usize, [u8; 3])) {}
    }
    impl Drop for Midi {
        fn drop(&mut self) {
            let _ = self.dropped.send(std::thread::current().id());
        }
    }

    #[test]
    fn hotplug_rec_cancel_and_new_ports_share_one_start_and_retire_stale_input_off_ui() {
        let mut s = Studio::sample();
        let release = crate::browser_io_tests::Release::new();
        let gate = release.0.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (entered_tx, entered) = mpsc::channel();
        let (dropped_tx, dropped) = mpsc::channel();
        let ui = std::thread::current().id();
        s.midi_opener(move || {
            let n = count.fetch_add(1, Ordering::SeqCst);
            assert_ne!(std::thread::current().id(), ui);
            if n == 0 {
                entered_tx.send(()).unwrap();
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |done| !*done).unwrap());
            }
            Ok(Box::new(Midi {
                ports: vec![if n == 0 { "Old" } else { "New" }.into()],
                dropped: dropped_tx.clone(),
            }))
        });
        s.midi_ports_now(vec!["Old".into()]);
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.kind == xirius_core::model::TrackKind::Instrument)
            .unwrap()
            .id;
        s.song.toggle_arm(track).unwrap();
        s.toggle_record();
        assert!(s.transport.pending_record);
        for _ in 0..16 {
            s.idle();
            s.tick();
            s.geometry(880.0, 600.0);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        s.midi_ports_now(vec!["New".into()]);
        s.transport_key("enter");
        s.poll_live_midi();
        assert!(!s.transport.pending_record && !s.transport.recording);
        release.release();
        let deadline = Instant::now() + Duration::from_secs(30);
        while s.live_midi.input.is_none() {
            s.poll_live_midi();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(s.live_midi.input.as_ref().unwrap().ports(), ["New"]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_ne!(dropped.recv_timeout(Duration::from_secs(20)).unwrap(), ui);
        s.midi_ports_now(Vec::new());
        assert_ne!(dropped.recv_timeout(Duration::from_secs(20)).unwrap(), ui);
        assert!(!s.transport.recording);
    }
}
