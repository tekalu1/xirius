# Running tests

Install cargo-nextest **0.9.151** (for example, `cargo install -j6 cargo-nextest --version 0.9.151 --locked`),
then run `python scripts/ci_tests.py`.
This is the local and CI entrypoint. It prepares normal executables and plugin fixtures,
runs the window-free workspace tests, checks the small default-feature configurations,
runs doctests, and checks the complete context-menu catalog. Cargo builds use six jobs.
`--list` shows the selected workspace test targets; `--legacy` runs the original Cargo
package groups for comparison. Neither mode runs ignored tests by default.

The workspace build combines the UI dev-dependencies' `test-io` and the engine's
`assert_no_alloc/warn_debug`. Default-feature checks separately exercise core edit hashes,
background history writes, session playback/audition/MIDI capture, CLI/MCP song flow, and
prefs persistence without that workspace union.
These are additional builds, not part of a claim that every configuration builds once.

`--plugins` remains the explicit real-ARA check: set the plugin paths listed in
`docs/ara-verification.md`. Tests opening real windows remain excluded by the lists in
`scripts/ci_tests.py` and must run on a desktop of known size before release.
MP3 tests list encoder-dependent cases as ignored when LAME is absent. Explicitly executing
one of those cases without the encoder fails; it cannot be reported as a successful test.
Build LAME with `scripts/build-lame.ps1` or `scripts/build-lame.sh` to exercise those cases.

For a focused UI test, first run `python scripts/ci_tests.py --prepare`. Reload tests require
the host beside the test build, with the same package version and wire protocol; tests never
invoke Cargo to create it. For restricted local environments, `XIRIUS_TEST_TRASH_DIR` lets
export-action tests use an injected file-moving callback. Without it, CI retains the real
recycle-bin check. Keep signing-test temporary directories outside any Git checkout; the
production key-generation guard still refuses paths inside a repository.

## CI stages

Each OS builds one nextest archive, including normal executables and test plugins. Four
shards run individual tests with `slice:1/4` through `slice:4/4`; this distributes test counts,
not elapsed time. Source checkout and archive must have the same revision. The current CI
uses the same checkout layout on build and execution runners because some tests reference
source paths embedded at compile time.

Each shard publishes separate JUnit and context observations. The aggregate requires every
build and shard to succeed and all four observation artifacts to be present before checking
the complete catalog. Equal filenames in different shards are preserved. The stable
`check (windows-latest)` and `check (macos-latest)` job names remain release gates.

The build job restores Rust cache on branches and PRs but saves only on `refs/heads/main`.
Its explicit shared key is `check`; `workflow_dispatch` still accepts `use_cache=false`.
Cache restore/save, build/archive, archive upload/download, and shard execution are separate
Actions steps. The runner also prints preparation and archive-command elapsed times.

References: [custom harness contract](https://nexte.st/docs/design/custom-test-harnesses/),
[archive requirements](https://nexte.st/docs/ci-features/archiving/),
[partitioning](https://nexte.st/docs/ci-features/partitioning/).
