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
one of those cases with a nonempty name substring filter, `--exact`, or `--ignored` without
the encoder fails; it cannot be reported as a successful test. Listing remains successful.
Build LAME with `scripts/build-lame.ps1` or `scripts/build-lame.sh` to exercise those cases.

For a focused UI test, first run `python scripts/ci_tests.py --prepare`. Reload tests require
the host beside the test build, with the same package version and wire protocol; tests never
invoke Cargo to create it. Version/protocol checks do not detect an older host from the same
version: rerun preparation after engine or host changes. This command prepares debug artifacts;
they cannot be used for release tests, which need matching release artifacts.
For restricted local environments, `XIRIUS_TEST_TRASH_DIR` lets export-action tests use an
injected file-moving callback. The directory must be on the same volume as the test exports
under the checkout's `target/`, because the callback uses `std::fs::rename`. Without it,
Windows CI retains the real recycle-bin check. Keep signing-test temporary directories outside any Git checkout; the
production key-generation guard still refuses paths inside a repository.

Every test run through nextest (the `default` profile and `ci`, which inherits it) has a
per-test timeout in `.config/nextest.toml`: a test running over 60 seconds is printed as SLOW,
and one still running after 5 minutes is killed with its child processes and fails as TIMEOUT,
by name. The `cargo test` runs (`--default-features`, `--doctest`, `--legacy`, `--plugins`)
do not go through nextest and have no such limit.

## Native preset popup (Windows desktop)

After `python scripts/ci_tests.py --prepare`, run
`cargo test -p xirius-ui --test plugin_preset_front_window -- --nocapture`.
The `_window` suffix excludes this test from CI. It opens the fixture editor in a
dedicated host, off every display with silent output, and sends `BM_CLICK` to its
preset button. Both pin states must put the popup above the editor in native window
order and at `WindowFromPoint` below the button. It also checks placement, selection,
closure, and the editor's unchanged owner, pin state and order relative to the main
window. Each run uses a temporary folder for data and removes it afterwards.
An additional case gives the editor `WS_EX_TOPMOST`; this is independent of the
pin button, which retains an editor rather than making it always on top.
Two more cases temporarily attach the input queues and activate the editor before
`BM_CLICK`, asserting `GetForegroundWindow` first. Run this manual test only on an
unused desktop: these cases deliberately change the foreground window, without
injecting keyboard or mouse input. Both pin states use ordinary non-topmost editors.
The scenario writes `front.json`, including notification, creation, first paint callback,
and request-to-popup observation times. The paint callback is not a GPU presentation timestamp.

## What a test may leave

A test writes nothing to the person's Xirius folders (`%LOCALAPPDATA%\Xirius` and
`%APPDATA%\Xirius` on Windows; `~/Library/Application Support/Xirius`, `~/Library/Caches/Xirius`
and `~/Library/Logs/Xirius` on macOS) and leaves nothing in the temporary folder. It keeps its
files in a folder it removes also when it fails (`tempfile::TempDir`, or `shot::TestFolder` in
the UI crate, which waits for a closed song's history and cleanup, for the recent-song lists kept in it, for
work on other threads that still writes in it, and for files a program that is ending still
holds, and fails the test when it cannot remove the folder; a shot script a UI test plays can keep
its files in the test's folder, `shot::TestFolder::lent`), and names the folders of what it starts: a test that starts a plugin host
sets `XIRIUS_CRASH_DIR` to a folder of its own before it starts a thread (each host makes its
crash records as it starts, whether it falls over or not; `shot::CrashRecords` in the UI crate,
`crash_records::CrashRecords` in the engine's and the session's tests). That folder is under
`target/`, as the plugin copies below: a host still ending may hold its records after the test,
and a later test removes the old ones. One that opens the
browser or scans plugins with a song of its own names its files too (`XIRIUS_BROWSER_FILE`,
`XIRIUS_PLUGIN_CACHE`, or the scan options of its song; a song a UI test makes with the default
options keeps its plugin list in its `shot::TestFolder`, scans no installed folder, and stops a
scan still running before it goes); and one that bounces or exports from an
unsaved song points the temporary folder at one of its own (`shot::TestTemp`). A background job
reads as ended before it writes its last record and lets go of its `<job>.lock` in the song's
`exports` folder: a test that ends on seeing it ended waits for that lock to go first. A plugin a test loads into its own process cannot be removed on Windows
until the process ends; it is copied under `target/` (`xirius_test_vst3::copies_dir`).

`scripts/ci_tests.py` runs the tests with a temporary folder of the run's own, and with the
variables the programs find the person's folders by pointed at an empty folder of the run's own,
as on a PC where Xirius never ran: `LOCALAPPDATA` and `APPDATA` on Windows; `HOME` on macOS, with
`CARGO_HOME` and `RUSTUP_HOME` left naming cargo's and rustup's folders; `XDG_CONFIG_HOME` and
`XDG_CACHE_HOME` elsewhere. A test cannot reach the person's folders that way, and a file it makes
there is not missed because the same file was already on the PC (an empty `Crashes`, a lock
file, a plugin list). Anything in the Xirius folders of that stand-in after the run, anything new,
changed or gone in the person's Xirius folders (listed before and after), and anything left in
the run's temporary folder fails the run by name (`scripts/leftovers.py`). So does a name new
directly in the checkout's `target/` after the run, other than what cargo and nextest keep there
(`debug`, `release`, `doc`, `package`, `tmp`, `nextest` and cargo's own files): a test that
works under `target/` removes what it made there. When an Xirius that is not this
checkout's runs meanwhile (the installed app, a run in another checkout), what changed in the
person's Xirius folders is printed as a warning instead; the stand-in and the temporary folder
always count. Each new or written path, and each name left in the temporary folder, is printed
with the time it was last written in UTC (`(written 2026-11-16T04:19:32Z)`): the tests that the
run's log shows running at that time are the ones to look at first. A run outside `ci_tests.py` (`cargo nextest run`, `cargo test`) uses the person's
folders: the tests still write nothing there, but nothing checks it.

## CI stages

Each OS builds one nextest archive, including normal executables and test plugins. Four
shards run individual tests with `slice:1/4` through `slice:4/4`; this distributes test counts,
not elapsed time. Source checkout and archive must have the same revision. The current CI
uses the same checkout layout on build and execution runners because some tests reference
source paths embedded at compile time.

To catch files that are present in a build directory but absent from the test archive,
create an archive with `python scripts/ci_tests.py --archive tests.tar.zst`, then run the
affected test from that archive with `cargo nextest run -P ci --archive-file tests.tar.zst
--workspace-remap <checkout> --extract-to <empty-directory> -E 'test(<test-name>)'`.
Use an empty extraction directory outside the build tree and remove it after the run.
An extracted archive does not hide absolute build paths embedded in a test binary while the
original build tree still exists. Check for build-time `CARGO_TARGET_TMPDIR` references in a
new test, and use the separate CI shard jobs to confirm that no build-only path is required.

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
