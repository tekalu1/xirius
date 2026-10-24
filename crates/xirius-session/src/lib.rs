//! The session layer: the song in `xirius-core` on one side, the audio engine on the other.
//!
//! - [`build`] makes the engine's mix from the song.
//! - [`host`] keeps a running engine in step with the song as commands change it.
//! - [`export`] renders the song to WAV, FLAC or MP3 ([`mp3`]) files offline; [`jobs`] keeps the record of
//!   each export in the song folder, where every process finds it.
//! - [`commands`] adds the export, transport and plugin commands to core's command table, and
//!   [`ext`] answers them.
//! - [`record`] records the armed tracks into the song (the record button and `record.*`);
//!   [`cleanup`] puts the recordings nothing uses any more in the recycle bin.
//! - [`watch`] measures, trims and monitors the input of the armed tracks before and while they
//!   record.
//! - [`plugins`] opens the VST3 plugins a song names; [`output`] is where a player's sound goes.

pub mod app;
pub mod audio_settings;
pub mod audition;
pub mod automation;
pub mod automation_timing;
pub mod autowrite;
pub mod beat;
pub mod bounce;
pub mod build;
pub mod clap_plugins;
pub mod cleanup;
pub mod clip_audio;
pub mod commands;
pub mod dawproject;
pub mod decode;
pub mod export;
mod export_paths;
pub mod export_preview;
pub mod export_selection;
pub mod export_stale;
pub mod export_watch;
pub mod ext;
pub mod file_ops;
pub mod graph;
pub mod host;
pub mod import;
pub mod io_store;
pub mod jobs;
pub mod listen;
pub mod live;
pub mod midi_export;
pub mod midi_import;
pub mod midi_out;
pub mod midi_settings;
pub mod mp3;
pub mod os;
pub mod output;
pub mod peaks;
pub mod perf;
pub mod places;
pub mod plugins;
pub mod pool;
pub mod preset;
pub mod processed;
pub mod punch;
pub mod rack_row;
pub mod recent;
pub mod record;
pub mod reveal;
pub mod samples;
pub mod smf;
pub mod starting;
pub mod synth;
pub mod tempo;
pub mod track_preset;
pub mod update;
pub mod watch;
pub mod wav;
pub mod xpreset;
