//! The integration tests of xirius-ui that open no window, in one test executable (ARC-052): one
//! file per area, each a module here. One executable links once, where each file linked one of
//! its own; nextest still runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-ui -E 'binary(it) &
//! test(/^routing::/)'` or `cargo test -p xirius-ui --test it routing::`.
//!
//! Kept as executables of their own, beside this folder: the tests that open real windows (the
//! `*_window` and `*_windows` files, `crash_entry`), which `scripts/ci_tests.py` leaves out of CI
//! and the release runs by their target's name; and `scan`, which sets a variable the program
//! reads, where a plain `cargo test` would run it in one process with the others.

mod context_layout;
mod drag_areas;
mod glyph_cover;
mod gui_subsystem;
mod patched_doc;
mod routing;
mod source_rules;
mod version_info;
mod window_sandbox;
