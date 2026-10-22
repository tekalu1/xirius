use crate::studio::Studio;
use xirius_engine::record::MidiInput;
use xirius_session::output::OutputMode;

pub type Opener = Box<dyn FnMut() -> Result<Box<dyn MidiInput>, String>>;

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
        opener: impl FnMut() -> Result<Box<dyn MidiInput>, String> + 'static,
    ) {
        self.live_midi.opener = Some(Box::new(opener));
    }
    pub(crate) fn can_open_live_midi(&self) -> bool {
        self.transport.mode == OutputMode::Device || self.live_midi.opener.is_some()
    }
    pub(crate) fn open_midi_port(&mut self) -> Result<Box<dyn MidiInput>, String> {
        if let Some(open) = &mut self.live_midi.opener {
            return open();
        }
        if self.transport.mode != OutputMode::Device {
            return Err("MIDI入力がありません".into());
        }
        xirius_engine::midi_in::MidirInput::open(None)
            .map(|input| Box::new(input) as Box<dyn MidiInput>)
    }
}
