//! Where a recording takes its sound and notes from (REC-001, OPEN-07, ENG-028).
//!
//! The mixer's input row of an audio track names channels of the input
//! device (`track.input.set`): `In 1` is mono, `In 1+2` a stereo pair. No
//! name records `In 1+2` (`In 1` from a mono device). Every armed audio track
//! records its own channels from the same device, at the same time; the
//! session opens the device itself.
//!
//! Armed instrument tracks record from every MIDI input port of this PC,
//! opened for each recording and let go when it ends.
//!
//! A screen without a device (tests, screenshots) records a test tone and a
//! few notes instead, so no microphone and no MIDI device is ever opened
//! there.

use xirius_core::id::Node;
use xirius_core::model::{Project, Track, TrackKind};
use xirius_engine::midi_in::MidirInput;
use xirius_engine::record::{AudioInput, MidiInput, MockInput, MockMidi};

/// What the input row says for a track without a named input.
pub const DEFAULT_INPUT: &str = "既定（In 1+2）";

/// Channels the input row lists at most (the engine's limit).
const MAX_CHANNELS: usize = xirius_engine::record::MAX_INPUT_CHANNELS;

/// How many channels the device's default input has, as the input row lists
/// them. Reading its format does not open it; no device is none.
pub fn device_channels() -> usize {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.default_input_config().ok())
        .map_or(0, |c| usize::from(c.channels()))
}

/// The inputs of `channels` channels, as a track names them: each pair
/// first, then its two channels on their own (`In 1+2`, `In 1`, `In 2`, …).
pub fn channel_names(channels: usize) -> Vec<String> {
    let n = channels.min(MAX_CHANNELS);
    let mut out = Vec::new();
    let mut c = 1;
    while c <= n {
        if c < n {
            out.push(format!("In {}+{}", c, c + 1));
            out.push(format!("In {c}"));
            out.push(format!("In {}", c + 1));
        } else {
            out.push(format!("In {c}"));
        }
        c += 2;
    }
    out
}

/// The armed tracks of `p` a recording takes, top to bottom, as the session
/// picks them: the audio tracks, then the instrument tracks.
pub fn armed(p: &Project) -> (Vec<&Track>, Vec<&Track>) {
    let mut column = Vec::new();
    for n in &p.root {
        match n {
            Node::Track(t) => column.push(*t),
            Node::Folder(f) => column.extend(p.tracks_in(*f)),
        }
    }
    let armed = |kind: TrackKind| {
        column
            .iter()
            .filter_map(|id| p.tracks.get(id))
            .filter(|t| t.armed && t.kind == kind)
            .map(|t| &**t)
            .collect::<Vec<_>>()
    };
    (armed(TrackKind::Audio), armed(TrackKind::Instrument))
}

/// The test tone a screen without a device records (two channels).
pub fn test_input(rate: u32) -> Box<dyn AudioInput> {
    Box::new(MockInput::sine(f64::from(rate), 220.0, 0.3))
}

/// A few notes played along, for a screen without a device: C, E, G and C on
/// the beats, eight times over. The first is one beat in: the stand-in counts its
/// frames from the first block after it is attached, and the engine can render that
/// block before the transport starts, where a note is (rightly) not part of the take.
/// A note on the very frame the take opens is pinned by the transport tests, which
/// place it themselves.
pub fn test_midi(rate: u32) -> Box<dyn MidiInput> {
    let beat = u64::from(rate) / 4;
    let mut events = Vec::new();
    for bar in 0..8u64 {
        for (i, pitch) in [60u8, 64, 67, 72].into_iter().enumerate() {
            let at = (bar * 4 + i as u64 + 1) * beat;
            events.push((at, [0x90, pitch, 96]));
            events.push((at + beat * 3 / 4, [0x80, pitch, 0]));
        }
    }
    Box::new(MockMidi::new(events))
}

/// The MIDI input of a recording: every port of this PC with a device, the
/// test notes without one. An error says why there is none (no port); the
/// session then leaves the instrument tracks out and says so.
pub fn midi_input(device: bool, rate: u32) -> Result<Box<dyn MidiInput>, String> {
    if !device {
        return Ok(test_midi(rate));
    }
    MidirInput::open(None).map(|m| Box::new(m) as Box<dyn MidiInput>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channels_are_listed_in_pairs_then_one_by_one() {
        assert_eq!(channel_names(0), Vec::<String>::new());
        assert_eq!(channel_names(1), ["In 1"]);
        assert_eq!(channel_names(2), ["In 1+2", "In 1", "In 2"]);
        assert_eq!(channel_names(3), ["In 1+2", "In 1", "In 2", "In 3"]);
        assert_eq!(channel_names(64).len(), MAX_CHANNELS / 2 * 3);
    }

    #[test]
    fn every_name_is_one_the_session_reads() {
        for (i, name) in channel_names(4).iter().enumerate() {
            let ch = xirius_session::record::input_channels(Some(name), 4).unwrap();
            assert_eq!(ch.len(), if i % 3 == 0 { 2 } else { 1 }, "{name}");
        }
    }
}
