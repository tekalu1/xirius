//! The integration tests of xirius-ara, in one test executable (ARC-052): one file per area, each
//! a module here. One executable links once, where each file linked one of its own; nextest still
//! runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-ara -E 'binary(it) &
//! test(/^service::/)'` or `cargo test -p xirius-ara --test it service::`.
//!
//! Kept as executables of their own, beside this folder: `contain`, whose faults go to the
//! process's last-chance filter, and `test_plugin`, which `scripts/ci_tests.py` runs by its
//! target's name with a real plugin (`PLUGIN_TESTS`).

mod api_copy;
mod clap;
mod service;
