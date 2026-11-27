//! The integration tests of xirius-vst3, in one test executable (ARC-052): one file per area, each
//! a module here. One executable links once, where each file linked one of its own; nextest still
//! runs every test in a process of its own.
//!
//! A test's name is `<file>::<test>`: `cargo nextest run -p xirius-vst3 -E 'binary(it) &
//! test(/^sdk_pin::/)'` or `cargo test -p xirius-vst3 --test it sdk_pin::`.
//!
//! Kept as executables of their own, beside this folder: the tests that open editor windows
//! (`editor_caption`, `editor_resize`, `vendor_editor_windows`), which `scripts/ci_tests.py`
//! leaves out of CI by their target's name; `owner_thread`, which installs `assert_no_alloc`'s
//! allocator for its whole executable; `trash_filter_probe`, which sets the process's unhandled
//! exception filter and is run alone; and the tests that count the fixture plugin's users in the
//! process (`module_sharing`, `residency_timer`, `macos_bundle`), where a plain `cargo test`
//! would load the same fixture from the other files' tests at the same time.

mod concurrent_load;
mod gain;
mod note_names;
mod real_plugins;
mod sdk_pin;
mod sdk_samples;
mod zero_bus;
