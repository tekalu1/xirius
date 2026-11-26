//! The integration tests of xirius-engine, in one test executable (ARC-052): one file per area,
//! each a module here. One executable links once, where each file linked one of its own; nextest
//! still runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-engine -E 'binary(it) &
//! test(/^sandbox::/)'` or `cargo test -p xirius-engine --test it sandbox::`.
//!
//! Kept as executables of their own, beside this folder, are the tests that set variables the
//! program or the fixture plugin reads (`editor_open_window`, `handshake`,
//! `prepare_no_timeout`): a plain `cargo test` would run them in one process with the others.
//! So are the tests that install `assert_no_alloc`'s allocator for the whole executable
//! (`anticipation`, `disk_stream`, `no_alloc`, `note_route`, `pitch_correction`, `timing_rt`):
//! an executable has one global allocator, and here it would check every other test too.

#[path = "../crash_records/mod.rs"]
mod crash_records;

mod ara_host;
mod automation_time;
mod clock;
mod count_in;
mod crash_reports;
mod isolation_benchmark;
mod master_order;
mod midi_out;
mod mix;
mod monitor;
mod multi;
mod no_lock;
mod no_split_bits;
mod pad_output_engine;
mod parallel;
mod pdc;
mod prefetch;
mod record;
mod record_crash;
mod retirement;
mod rf64_real;
mod sandbox;
mod sandbox_clap;
mod sidechain;
mod split;
mod split_keys;
mod transport;
mod unlaunchable_host;
mod version_info;
mod vst3;
