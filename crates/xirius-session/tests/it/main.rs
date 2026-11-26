//! The integration tests of xirius-session, in one test executable (ARC-052): one file per area,
//! each a module here. One executable links once, where each file linked one of its own; nextest
//! still runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-session -E 'binary(it) &
//! test(/^bounce::/)'` or `cargo test -p xirius-session --test it bounce::`.
//!
//! Kept as executables of their own, beside this folder: the tests that open windows (named in
//! `scripts/ci_tests.py`), the real ARA plugin's test (`ara_real`, run with `--ignored`), the
//! default-feature check (`empty_instrument`), `export_mp3` (`harness = false`), `ara_crash`
//! (it faults on purpose under the process's crash filter), and the tests that set a variable
//! the program or the fixture plugin reads (a plain `cargo test` would run them in one process
//! with the others).

#[path = "../crash_records/mod.rs"]
mod crash_records;
#[path = "../support/mod.rs"]
mod support;

mod app;
mod ara;
mod ara_clips;
mod ara_three;
mod audio_settings;
mod aut016_steps;
mod aut019_master;
mod automation_modes;
mod automation_pdc;
mod automation_send;
mod bind_all;
mod bounce;
mod builtin;
mod chain_preset;
mod child_time;
mod clap;
mod cleanup;
mod clip_audio;
mod clip_loop;
mod clip_transpose;
mod clipfx_render;
mod controllers;
mod drivers;
mod drum_live;
mod drum_live_passive;
mod eng064_starting;
mod eng065_late;
mod eng068_latency_edit;
mod eng071_offline_renders;
mod eng078_keys;
mod export_actions;
mod export_dawproject;
mod export_folder;
mod export_jobs;
mod export_midi;
mod export_muted;
mod export_selection;
mod export_stale;
mod export_watch;
mod ext;
mod file_ops;
mod file_reveal;
mod heavy_vst3_open;
mod import;
mod isolation;
mod latency;
mod listen;
mod live;
mod live_policy;
mod many_inserts;
mod media;
mod midi_import;
mod midi_out;
mod mix039_owned_edit;
mod mix049_output_key;
mod mix051_note_source;
mod native_pedal;
mod pad_output_render;
mod plugin_crash_isolation;
mod plugin_process_preferences;
mod plugin_state_pull;
mod plugin_timing;
mod pool;
mod presets;
mod processed;
mod rack_keys;
mod rack_layers_sound;
mod rack_sound;
mod record;
mod relocate;
mod sample_folder;
mod session;
mod streaming;
mod track_transpose;
mod volatile_probe;
mod watch;
mod wavfile;
mod xpreset;
