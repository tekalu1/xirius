//! The integration tests of xirius-prefs, in one test executable (ARC-052): one file per area,
//! each a module here. One executable links once, where each file linked one of its own; nextest
//! still runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-prefs -E 'binary(it) &
//! test(/^mvp::/)'` or `cargo test -p xirius-prefs --test it mvp::`.

mod default_keys;
mod macros;
mod mvp;
mod session;
