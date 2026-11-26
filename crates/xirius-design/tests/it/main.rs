//! The integration tests of xirius-design, in one test executable (ARC-052): one file per area,
//! each a module here. One executable links once, where each file linked one of its own; nextest
//! still runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-design -E 'binary(it) &
//! test(/^contrast::/)'` or `cargo test -p xirius-design --test it contrast::`.

mod api_surface;
mod contrast;
mod surface_steps;
