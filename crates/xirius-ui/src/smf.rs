//! Reading a standard MIDI file (UI-076): the reader is the session's, which the CLI uses too.

pub use xirius_session::smf::{SmfNote, read};

#[cfg(test)]
pub mod tests {
    pub use xirius_session::smf::build::three_notes;
}
