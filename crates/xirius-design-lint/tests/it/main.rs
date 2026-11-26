//! The integration tests of xirius-design-lint, in one test executable (ARC-052): one file per
//! area, each a module here. One executable links once, where each file linked one of its own;
//! nextest still runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-design-lint -E 'binary(it) &
//! test(/^tokens::/)'` or `cargo test -p xirius-design-lint --test it tokens::`.

mod behavior;
mod fixtures;
mod interaction_coverage;
mod license_bans;
mod policy_docs;
mod tokens;
mod trademarks;
mod ui_io_inventory;
mod vst2_policy;
