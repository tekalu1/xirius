//! The integration tests of xirius-mcp, in one test executable (ARC-052): one file per area, each
//! a module here. One executable links once, where each file linked one of its own; nextest still
//! runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-mcp -E 'binary(it) &
//! test(/^receipts::/)'` or `cargo test -p xirius-mcp --test it receipts::`.

mod async_receipt;
mod http_events;
mod mcp;
mod other_save;
mod prj025_stamp;
mod receipts;
mod result_permissions;
mod results;
mod run_events;
mod screen_tools;
