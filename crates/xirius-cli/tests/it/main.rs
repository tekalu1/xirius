//! The integration tests of xirius-cli, in one test executable (ARC-052): one file per area, each
//! a module here. One executable links once, where each file linked one of its own; nextest still
//! runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-cli -E 'binary(it) &
//! test(/^song_flow::/)'` or `cargo test -p xirius-cli --test it song_flow::`. The tests start
//! the programs as children and give each child its own variables, so none is kept apart.

mod audio_settings;
mod bind_warning;
mod cli;
mod command_docs;
mod command_host;
mod command_visual;
mod import_cli;
mod interfaces;
mod job_cancel;
mod mcp_session;
mod places_cli;
mod prefs_cli;
mod record_cli;
mod recovery_cli;
mod session_cli;
mod song_flow;
mod two_processes;
mod version_info;
