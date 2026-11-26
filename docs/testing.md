# Running tests

Install cargo-nextest **0.9.151** (for example, `cargo install -j6 cargo-nextest --version 0.9.151 --locked`),
then run `python scripts/ci_tests.py`.
This is the local and CI entrypoint. It prepares normal executables and plugin fixtures,
runs the window-free workspace tests, checks the small default-feature configurations,
runs doctests, and checks the complete context-menu catalog. Cargo builds use six jobs.
`--list` shows the selected workspace test targets; `--legacy` runs the original Cargo
package groups for comparison. Neither mode runs ignored tests by default.

A crate's integration tests are modules of one test executable, `tests/it/main.rs`, one file
per area (ARC-052). The crate links once instead of once per file. Tests that open windows, set
a variable another test would read, or use their own harness keep executables of their own
(listed in each `main.rs`). A test's name is `<file>::<test>`:
`cargo test -p xirius-core --test it recovery::`, or with nextest
`-E 'binary(it) & test(/^recovery::/)'`. nextest still runs each test in a process of its own.

The workspace build combines the UI dev-dependencies' `test-io` and the engine's
`assert_no_alloc/warn_debug`. Default-feature checks separately exercise core edit hashes,
background history writes, session playback/audition/MIDI capture, CLI/MCP song flow, and
prefs persistence without that workspace union.
These are additional builds, not part of a claim that every configuration builds once.

`--plugins` remains the explicit real-ARA check: set the plugin paths listed in
`docs/ara-verification.md`. Tests opening real windows remain excluded by the lists in
`scripts/ci_tests.py` and must run on a desktop of known size before release.
`--windows --tag v<version>` runs the window tests a release needs a record of
(`WINDOW_TESTS` in `scripts/release_ci_gate.py`; for now the `xirius-ui` `e2e_window` save
and open run) on a clean checkout, and on a pass makes the annotated release tag whose message
names the commit and the result. The release workflow stops a tag without that record
(`docs/release-ci-gate.md`).
MP3 tests list encoder-dependent cases as ignored when LAME is absent. Explicitly executing
one of those cases with a nonempty name substring filter, `--exact`, or `--ignored` without
the encoder fails; it cannot be reported as a successful test. Listing remains successful.
Build LAME with `scripts/build-lame.ps1` or `scripts/build-lame.sh` to exercise those cases.
Windows and macOS CI each build LAME once with those scripts and share the library with
all four test shards through `XIRIUS_LAME`. The check job requires the encoder-dependent
session and export card cases to pass in the combined JUnit results.

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

## Adding a feature

Some tests are made from the tables the code already keeps: a new row gets its checks without a
new test, and a row left without them fails the run, naming the row.

- **A command that goes into the history** (`CommandDef::history`): add a step for it to
  `core::every_history_command_undoes_and_redoes` (`crates/xirius-core/tests/it/core.rs`). A
  registered history command that no step runs fails it as `not covered by this test: [<name>]`.
  Each step checks that the song stays valid, that the command makes one step, and that undo and
  redo give back the songs before and after. The song is saved half way and at the end, the
  session dropped and the folder opened again: every state a step made reads back from the files
  as it was in memory (a field the model does not write or read fails as `state <n>, made by
  <command>: [<path of the field>]`), project.json is the song, and the first state and the last
  come back by jumping. A command's own test then checks what the command does, and needs no
  save-and-reopen of its own; it keeps one only for a song the steps do not make (an old file, a
  field that is not written).
- **A command or an argument**: nothing to add for the command line and MCP.
  `args::tests::every_argument_of_every_command_reads_as_the_mcp_call_does`
  (`crates/xirius-cli/src/args.rs`) types every argument of every registered command on the
  command line in each way it can be typed (`--name value`, `--name=value`, each alias, a bare
  word, a flag alone, an ID list named once per ID) and reads it as the JSON an MCP client sends;
  a difference fails as `<command> --<arg> (<way>): <line> reads as …, the MCP call … as …`. A new
  `ArgType` does not build until it has sample values in `samples()` there and an index in
  `interfaces::the_script_has_a_call_of_every_argument_type`, which then fails until
  `interfaces.rs`'s script has a call with it. A test that a command is reachable from both
  entrances is not needed.
- **A built-in instrument or effect** (`xirius_params::BUILTINS`): nothing to add for what every
  device must do. `stock::every` (`crates/xirius-engine/src/stock/every.rs`) checks each one:
  silence in is silence out at its defaults, with every value at 0 and at 1, with and without a
  file; notes without its files (or with a file that was not found) make no sound and do not
  panic; a sound that plays out and a note let go before its file ends leave no voice sounding
  and fall under -120 dB within eight seconds. A failure names the row, as `<uid>, <case>: left
  frame 812 is 0.003`; a built-in in the list without a processor in `stock::create` fails as
  `<uid>, …: no processor`. The probe tests (`stock::probe`) take the list from there too. The
  device's own tests then check its sound, not silence or letting go.
- **A row of a right-press menu** (a rule in `crates/xirius-ui/tests/context_menus/mapping.json`
  or `supplemental.json`): run the menu tests with `XIRIUS_WRITE_MENUS=1` and read the change
  to `tests/context_menus/menus.txt` (each menu, opened from the sample song, with its labels in
  order, the rule of each row and `(dim)`). `context::placement::every_row`
  (`crates/xirius-ui/src/context/every_row_tests.rs`) then presses the row: it must run a
  registered command with arguments it takes, be at most one step that one undo takes back, and
  be exactly one step when its command goes into the history (a row that sets what already
  stands is in `SETS_WHAT_STANDS` with the reason). A failure names the rule and its place in
  the table, as `other-17:002 (編集 > 削除) 「削除」 in other-17 (a clip and a point): one press
  made 2 steps that one undo does not take back together`; a menu that no longer matches the file fails with the line and its menu; a rule
  that no menu opened shows fails as `<rule> (…): not in any menu of <target> opened here`, and
  a new target as `<target>: neither in TARGETS nor in NOT_OPENED`. Open the target in
  `TARGETS` (a state of the sample song that shows the row) or give the reason in `ABSENT` /
  `NOT_OPENED`. A menu's own test then checks what its rows do, their keys and the selection;
  it needs no list of labels, no `enabled` of a row of the file and no "one step, one undo"
  that only compares the song.
- **A script of pictures** (a row of `SCRIPTS` in `crates/xirius-ui/src/lib.rs`): nothing to
  add for its steps. `every_script_tests` (`crates/xirius-ui/src/every_script_tests.rs`) plays
  every script without a window at the window's size (1440×860), from the sample song, shot after
  shot, with the frames the window draws between shots: each press, hover, drag and wheel turn
  must find its target on the screen, and no step may panic or leave an error. A failure names
  the script, the shot and the step, as `ripple / ripple-01-off / step 1 (Stage(Laid, 0)):
  panicked: the clip is on screen`; a target the window would miss without a word fails as `…
  (Press(Track("Keys"))): its target is off the screen`. A step only the window plays (a
  `Play::Window` stage, the piano roll's ruler, a plugin's window) ends its shot there, until a
  shot starts again from the sample song. A `pub const …: &[Shot]` list that is not in
  `SCRIPTS` fails as `lists of shots not registered in SCRIPTS`; a script this test cannot play
  goes in `NOT_PLAYED` with the reason. What a shot leaves is checked from the `ENDS` table beside
  its script (`devices_shots::tests::ENDS`), one row per shot; a row whose shot no longer plays
  to its end fails as `rows of ENDS whose shots were not played to their ends`. A script's own
  test then checks what its pictures show, not that it is listed or that its targets exist.
- **A design rule, a lint case or a color** (`crates/xirius-design-lint`, `crates/xirius-design`):
  a lint case is a line in a fixture, not a test. Add it to
  `crates/xirius-design-lint/tests/fixtures/violations/*.rs` with `//~ <rule>` at the end of the
  line that must fail (`//~^` for the line above; a rule hit twice on one line is marked twice),
  or to `fixtures/clean/*.rs` when it must pass. Cases that must not see each other's `use` lines
  or names are cut apart with a `// ----` line; `// ---- crates/xirius-ui/src/draw.rs` checks the
  part below as that file. `fixtures::*` checks every fixture against its marks and fails with
  every difference as `<file>:<line>: <rule> …`; a rule with no failing fixture fails as
  `見本の無い規則`. A new token, face or pairing goes into `text_rows` or `graphic_rows`
  (`crates/xirius-design/tests/it/contrast.rs`), which hold every text and graphic against its
  face at 4.5:1 and 3:1. Then run `XIRIUS_WRITE_CONTRAST=1 cargo nextest run -p xirius-design
  contrast` and read the change to `crates/xirius-design/tests/contrast.txt` (each row with its
  two colors and ratio). A changed color fails `contrast::the_contrast_table_matches_its_snapshot`
  with the first row that moved, as `tests/contrast.txt:<line>`.
- **A default key, a preset or a mouse default** (`xirius_prefs::catalog`, `catalog::PRESETS`,
  `mousemap::DEFAULTS`): run `XIRIUS_WRITE_KEYS=1 cargo nextest run -p xirius-prefs
  default_keys` and read the change to `crates/xirius-prefs/tests/default-keys.txt` (every key
  of every preset with its context, its Windows and Mac names and its command, then every
  modifier of every mouse context with its action). A new preset is in it with no line to add.
  A changed table fails `default_keys::every_presets_keys_and_the_mouses_defaults_match_their_snapshot`
  with each line that went and came, as `tests/default-keys.txt:<line>: 消えた  keys ableton
  global F9 …` and `表の <line> 行め: 増えた  …`. A test then needs no copy of a default key; it
  checks what a key does, or a rule the table must keep (`session::no_preset_leaves_more_clashes_than_studio_pro`).
- **A preference** (`xirius_prefs::prefs::PREFS`): a row added there is in
  `settings::tests::every_setting_is_there_the_next_launch` with no line to add. It sets every
  value of a choice, both ends of a number and the opposite of a switch, through the settings
  screen when a page shows it (its switch or segment, its menu, its folder list) and through the
  `prefs.set` the screen runs when none does, then relaunches and reads it back; a last round puts
  every one back. A family (`….`) needs a name in `MEMBERS` and a free text a value in `TEXTS`,
  or it fails as `<key>: no name to try it with in MEMBERS` / `<key>: no value to try (a text
  needs one in TEXTS)`. A preference the studio works by goes in `lives` with what shows it, and
  must then be offered on the screen. A failure names the row, the value, the round and the way,
  as `audio.count-in = 8 (round 0, prefs.set): the studio has 0`, `edit.snap.roll.mode = bar
  (round 0, prefs.set): the next launch reads "adaptive"` or `edit.cursor.crosshair = true (round
  0): at once the studio does not work by it`.

## Build artifacts

The `dev` and `test` profiles use `debug = "line-tables-only"` and keep Cargo's default
incremental compilation. Backtraces retain file and line information. Inspecting local
variables and types in a debugger requires full debug information. The first build
after switching profiles rebuilds artifacts for the new settings. Artifacts from earlier
builds stay on disk, and incremental compilation can add new generations as you edit.

To preview older hashed artifacts in this checkout, run `python scripts/prune_deps.py`.
After stopping this checkout's builds, tests and app, run `python scripts/prune_deps.py --apply`
to remove them. This uses no extra tools or packages. It only visits `target/debug/deps`
beside this script's checkout and refuses an external `CARGO_TARGET_DIR`, directory links
or reparse points. It does not recurse into subdirectories or visit other checkouts.

Files with a 16-digit hash are grouped by name and extension. Every file with the newest
modification time is kept, including ties; only older files are removed. Unhashed files,
directories and links are left alone. The script holds Cargo's build lock, refuses detected
running artifacts, and checks each candidate again before removing it. Do not start builds,
tests or the app while it runs: a process check cannot prevent a new program from starting.

Different feature configurations can have different hashes, so an older artifact may still
be useful and require rebuilding later. For that reason cleanup is explicit, rather than
automatically appended to `ci_tests.py`. It leaves incremental directories and release
artifacts untouched. The cleanup boundaries and retention rules run in CI with
`python scripts/test_prune_deps.py`.

## Native preset popup (Windows desktop)

The CI-safe `xirius-engine --test editor_open_window` cases open parked fixture editors.
They check that the list belongs to the editor host, has the editor as owner, filters
searches, clears search before closing on Escape, and delivers the selected key over IPC.
The placement case waits for each host acknowledgement and checks native window order.
UI unit tests exercise placement sequencing, stale acknowledgements and a stalled host.
These message tests make no claim about foreground activation or physical input.

On an **unused Windows desktop**, after `python scripts/ci_tests.py --prepare`, run:

```powershell
python scripts/test_plugin_presets.py --work <temporary-folder> --out <results-folder>
```

The script checks running Xirius processes with `Get-CimInstance Win32_Process` before
opening anything and refuses to overlap another run. It launches the checkout's debug
UI with isolated data and silent output, loads the repository's fixture in two dedicated
hosts, and uses `SendInput` for mouse clicks, drags, and keyboard input. It leaves Windows'
foreground rules intact: no `AttachThreadInput`, `BM_CLICK`, or topmost windows.
`result.json` records each assertion and the corresponding desktop BMP; `app.log` records
fixture setup and shutdown. A failure is an error, not a skipped check. Temporary user data
is removed on exit; the result folder is retained for review.

Use a desktop that can fit the 1040 × 640 logical-pixel main window and both fixture editors
without covering their controls. The checks cover foreground activation, search and two-stage
Escape, repeat-button, editor-body and outside-click dismissal, two editors with a list open,
pinned editors with built-in/channel windows, and most-recently-touched stacking. They check
that clicking the exposed part of either studio palette keeps its foreground focus and restores
the external stack. A browser drop creates and selects an instrument track and opens its editor
in the foreground. The suite also checks preset selection and the owners of the save form and
native file dialog. Use separate results folders for `--case A`, which checks channel placement
and stacking beside an unpinned editor, and `--case G`, which checks track insertion at a seam,
opening a built-in editor in the foreground and restoring the original track order with one undo.
Also run `--case cold` with a separate results folder. It starts without a plugin host and
drags the fixture from the browser. The fixture delays attaching its view by two seconds so
the test can detect a prematurely visible native frame. It then checks the editor content,
foreground window and window order without any further input.
With `--case device-kinds` and its own results folder, open instruments and FX from
the mixer. The case checks coexistence during and after loading, non-overlapping
placement on one monitor, repeat-click foreground and stacking, same-kind replacement
at the previous position, and the effect of pinning and unpinning. It also checks
clearance including the window frames when a full strip is free, that adding an
instrument from the Channel Editor keeps the FX window open above the studio, and
that raising the instrument from its card preserves the fixture's painted view.
The case starts a separate app session for window placement. It measures physical frame
coordinates after activation, including five Reverb reopens, Drum Kit → Sampler replacement,
Reverb beside a pinned EQ, and Limiter auto-opened from an insert gap beside Sampler.
Run this portion alone with `--case device-placement` or the Windows test
`foreground_device_windows_respect_clear_space_and_keep_their_frame_origin`.
Check installed-plugin compatibility in a separate manual pass.
macOS has no external plugin editor (NSView host) yet and is outside this desktop test's scope.

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

The fixture plugins are built in the same `cargo build` as the workspace (`--lib` makes their
cdylibs). Built on their own they get other features and rewrite their unhashed rlibs, and the
archive then compiles them and every crate above them a second time.

The default-feature checks and the doctests run in their own job per OS (`side`, cache key
`side`), beside the build instead of before the archive.

Each shard publishes separate JUnit and context observations. The aggregate requires every
build, the side job and every shard to succeed and all four observation artifacts to be present before checking
the complete catalog. Equal filenames in different shards are preserved. The stable
`check (windows-latest)` and `check (macos-latest)` job names remain release gates.

The build job restores Rust cache on branches and PRs but saves only on `refs/heads/main`.
Its explicit shared key is `check`; `workflow_dispatch` still accepts `use_cache=false`.
Cache restore/save, build/archive, archive upload/download, and shard execution are separate
Actions steps. The runner also prints preparation and archive-command elapsed times.

References: [custom harness contract](https://nexte.st/docs/design/custom-test-harnesses/),
[archive requirements](https://nexte.st/docs/ci-features/archiving/),
[partitioning](https://nexte.st/docs/ci-features/partitioning/).
