//! The integration tests of xirius-plugins, in one test executable (ARC-052): one file per area,
//! each a module here. One executable links once, where each file linked one of its own; nextest
//! still runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-plugins -E 'binary(it) &
//! test(/^scan::/)'` or `cargo test -p xirius-plugins --test it scan::`.
//!
//! Kept as an executable of its own, beside this folder: `ara_containment`, which installs an
//! exception handler in the process and faults on purpose.

mod scan;
mod scan_clap;
mod scan_vst3_macos;
mod version_info;
