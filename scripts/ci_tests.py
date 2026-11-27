"""Run the workspace's tests that do not open windows (the CI's test step).

The tests that open real windows (drive the app with `--script`, move windows, open plugin
editors) need a desktop of a known size and a person-free screen. They are run on a PC
before a release, not on CI: a CI runner's desktop is smaller and its timing differs, so they
fail there for reasons that are not the app's. Everything else runs here.

    python scripts/ci_tests.py            # run
    python scripts/ci_tests.py --list     # print the cargo commands only
    python scripts/ci_tests.py --plugins  # the tests with real plugins, before a release
    python scripts/ci_tests.py --windows --tag v1.0.1
                                          # the window tests a release needs, and its tag

The tests with a real plugin (an ARA plugin, an installed VST3 effect and instrument) are
`#[ignore]`d, so a run without one never passes them by skipping: each needs its variables set
to the plugins' files (`docs/ara-verification.md`, `docs/testing.md`), and fails when they are
not. Without a real plugin, the same paths run on the test plugins (`xirius-test-vst3`,
`xirius-test-clap`).

`--windows` runs the window tests a release needs a record of (`release_ci_gate.WINDOW_TESTS`)
on the checkout's commit, which must have no changes. On a pass it prints the record and writes
it to `target/window-tests/<commit>.txt`; with `--tag v<version>` it also makes the annotated
tag whose message holds it (not pushed). The release workflow stops a tag without a passing
record of its own commit (`docs/release-ci-gate.md`).

The tests run with a temporary folder of the run's own and with the person's folders pointed at
an empty folder of the run's own (as on a PC where Xirius never ran); a test that writes to the
Xirius folders, there or the person's, or leaves a file in the temporary folder fails the run by
name (`leftovers.py`).

Standard library only.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

import context_catalog
import leftovers
import plan
import release_ci_gate

# The integration tests that open windows, by package.
WINDOW_TESTS = {
    "xirius-ui": {"crash_entry"},  # and every `*_window` / `*_windows` target
    "xirius-session": {
        "eng067_recapture",
        "eng070_window_moves",
        "eng071_place_moves",
    },
    "xirius-vst3": {"editor_caption", "editor_resize", "vendor_editor_windows"},
}
# Unit tests inside a library that open windows.
WINDOW_UNIT_TESTS = ["mixer::editor_tests", "independent::window_tests"]
# The tests that need a real plugin, by package, target ("lib" for a library's unit tests) and
# test-name filter (None for the whole target): the variables that name the plugins' files.
PLUGIN_TESTS = {
    ("xirius-ara", "test_plugin", None): ("XIRIUS_ARA_TEST_PLUGIN",),
    ("xirius-session", "ara_real", None): ("XIRIUS_ARA_REAL_PLUGIN",),
    ("xirius-ui", "ara_real_window", None): ("XIRIUS_ARA_REAL_PLUGIN",),
    ("xirius-vst3", "it", "real_plugins::"): ("XIRIUS_REAL_VST3_EFFECT", "XIRIUS_REAL_VST3_INSTRUMENT"),
    ("xirius-vst3", "vendor_editor_windows", None): ("XIRIUS_REAL_VST3_EFFECT", "XIRIUS_REAL_VST3_INSTRUMENT"),
    ("xirius-session", "it", "ext::installed_"): ("XIRIUS_REAL_VST3_INSTRUMENT",),
    ("xirius-ui", "lib", "mixer::editor_tests::installed_"): ("XIRIUS_REAL_VST3_INSTRUMENT",),
}


def opens_windows(package, target):
    if target in WINDOW_TESTS.get(package, set()):
        return True
    return package == "xirius-ui" and (target.endswith("_window") or target.endswith("_windows"))


def missing_window_tests(meta):
    """The names in WINDOW_TESTS that are no test target: a renamed or removed file."""
    targets = {
        (p["name"], t["name"]) for p in meta["packages"] for t in p["targets"] if "test" in t["kind"]
    }
    return sorted(f"{p} {t}" for p, names in WINDOW_TESTS.items() for t in names if (p, t) not in targets)


def commands():
    meta = metadata()
    missing = missing_window_tests(meta)
    if missing:
        raise RuntimeError("WINDOW_TESTS names no test target (take it off): " + ", ".join(missing))
    members = set(meta["workspace_members"])
    runs = []
    for package in meta["packages"]:
        if package["id"] not in members:
            continue
        name = package["name"]
        tests = [
            t["name"]
            for t in package["targets"]
            if "test" in t["kind"] and not opens_windows(name, t["name"])
        ]
        if tests:
            targets = [arg for test in sorted(tests) for arg in ("--test", test)]
            runs.append(["cargo", "test", "--no-fail-fast", "-p", name, *targets])
    skip = [arg for unit in WINDOW_UNIT_TESTS for arg in ("--skip", unit)]
    return [
        ["cargo", "test", "--no-fail-fast", "--workspace", "--lib", "--bins", "--", *skip],
        *runs,
        ["cargo", "test", "--workspace", "--doc"],
    ]


def plugin_commands():
    return [
        ["cargo", "test", "-p", package, *(["--lib"] if target == "lib" else ["--test", target]), "--",
         *([name] if name else []), "--ignored", "--nocapture"]
        for (package, target, name) in sorted(PLUGIN_TESTS, key=lambda k: (k[0], k[1], k[2] or ""))
    ]


def plugin_variables():
    return sorted({v for names in PLUGIN_TESTS.values() for v in names})


NEXTEST_VERSION = "0.9.151"


def nextest_targets():
    names = sorted({c[i + 1] for c in commands() for i, arg in enumerate(c) if arg == "--test"})
    return ["--workspace", "--lib", "--bins", *[v for n in names for v in ("--test", n)]]


def test_filter():
    return " & ".join(f"not test({unit})" for unit in WINDOW_UNIT_TESTS)


def run(command, environment=None):
    print("$ " + " ".join(map(str, command)), flush=True)
    start = time.monotonic()
    result = subprocess.run(command, env=environment)
    print(f"elapsed: {time.monotonic() - start:.3f}s", flush=True)
    if result.returncode:
        raise subprocess.CalledProcessError(result.returncode, command)


def nextest_version():
    version = subprocess.check_output(["cargo", "nextest", "--version"], text=True)
    if version.split()[1] != NEXTEST_VERSION:
        raise RuntimeError(f"install cargo-nextest {NEXTEST_VERSION}")


def prepare():
    # --tests unifies the same dev features as the workspace test build. The test targets link
    # the fixture plugins' rlibs while the host loads their cdylibs, which Cargo makes only for a
    # library built as a root: --lib makes them in the same build. Building the fixtures on their
    # own (`-p xirius-test-clap --lib`) resolves other features and rewrites their unhashed
    # rlibs, so `nextest archive` rebuilt them and every crate above them a second time.
    run(["cargo", "build", "--locked", "--workspace", "--lib", "--bins", "--tests", "-j", "6"])


JUNIT = Path("target/nextest/ci/junit.xml")

MP3_CASES = {
    ("xirius-session::export_mp3", "each_bitrate_writes_a_constant_bitrate_file"),
    ("xirius-session::export_mp3", "a_song_above_48k_is_written_at_48k_or_44k1"),
    ("xirius-session::export_mp3", "id3_tags_name_the_song_and_the_track"),
    ("xirius-session::export_mp3", "loudness_and_true_peak_are_those_of_the_mp3"),
    ("xirius-ui", "export::tests::an_mp3_card_shows_its_bitrate_and_loudness_over_true_peak"),
}


def listed_tests(listing):
    """`<binary> <test>` for each test of `cargo nextest list --message-format json`."""
    return {f"{binary} {name}" for binary, suite in listing["rust-suites"].items() for name in suite["testcases"]}


def missing_plan_tests(plan_data, listed):
    """The tests the items of docs/plan.toml name in `tests` that nextest does not list."""
    return [f"{eid}: {name}" for eid, name in plan.nextest_names(plan_data) if name not in listed]


def check_plan_tests():
    """Fail on a test docs/plan.toml names that is not there. Run on Windows, where every test
    builds (some are Windows-only), after prepare(), so the listing compiles nothing."""
    listing = subprocess.check_output(
        ["cargo", "nextest", "list", "--locked", "--workspace", "--lib", "--bins", "--tests",
         "--run-ignored", "all", "--message-format", "json"],
        text=True,
    )
    missing = missing_plan_tests(plan.load_plan(), listed_tests(json.loads(listing)))
    if missing:
        raise RuntimeError("docs/plan.toml names tests that are not there (renamed or removed?): " + ", ".join(missing))


def require_mp3_cases(junits):
    counts = dict.fromkeys(MP3_CASES, 0)
    for junit in junits:
        for case in ET.parse(junit).iter("testcase"):
            key = (case.get("classname"), case.get("name"))
            if key in counts:
                if any(case.find(tag) is not None for tag in ("skipped", "failure", "error")):
                    raise RuntimeError(f"MP3 test did not pass: {' '.join(key)}")
                counts[key] += 1
    for key, count in sorted(counts.items()):
        if count != 1:
            raise RuntimeError(f"expected one MP3 result for {' '.join(key)}, got {count}")
    print(f"JUnit confirms {len(counts)} encoder-dependent MP3 tests passed")


def flaky_tests(junit):
    """The tests of a nextest junit file that failed and then passed on a retry."""
    found = []
    for case in ET.parse(junit).iter("testcase"):
        if case.find("flakyFailure") is not None or case.find("flakyError") is not None:
            found.append(f"{case.get('classname')} {case.get('name')}")
    return sorted(found)


def report_flaky(junits, label):
    """Names the retried tests as warnings and in the job summary (GITHUB_STEP_SUMMARY)."""
    found = sorted({t for junit in junits for t in flaky_tests(junit)})
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    for test in found:
        print(f"::warning title=Flaky test ({label})::{test} failed, then passed on a retry")
    if summary and found:
        with open(summary, "a", encoding="utf-8") as out:
            out.write(f"### Flaky tests ({label}): {len(found)}\n\n")
            out.write("Failed first, passed on a retry. A real bug can hide here: look at each.\n\n")
            out.writelines(f"- `{test}`\n" for test in found)
    return found


def default_feature_commands():
    # Exercise ordinary dependencies without the UI's test-io and engine's warn_debug union.
    return [
        ["cargo", "test", "--locked", "-j", "6", "-p", "xirius-core", "--test", "it", "--", "state_hash::",
         "history_writer::"],
        ["cargo", "test", "--locked", "-j", "6", "-p", "xirius-session", "--test", "empty_instrument"],
        ["cargo", "test", "--locked", "-j", "6", "-p", "xirius-cli", "--test", "it", "--", "song_flow::",
         "prefs_cli::"],
    ]


def check_shards(directory, count):
    with tempfile.TemporaryDirectory(prefix="xirius-context-merged-") as merged:
        for shard in range(1, count + 1):
            matches = list(Path(directory).glob(f"coverage-*-{shard}"))
            if len(matches) != 1 or not (matches[0] / "success.txt").is_file():
                raise RuntimeError(f"missing successful coverage shard {shard}")
            if (matches[0] / "success.txt").read_text(encoding="utf-8").strip() != f"slice:{shard}/{count}":
                raise RuntimeError(f"wrong coverage partition {shard}")
            files = list(matches[0].glob("*.json"))
            if not files:
                raise RuntimeError(f"empty coverage shard {shard}")
            for file in files:
                shutil.copyfile(file, Path(merged) / f"{shard}-{file.name}")
        return context_catalog.check(merged, Path(__file__).resolve().parent.parent)


# ------------------------------------------------------------------ the release's window tests

WINDOW_RECORDS = Path("target/window-tests")


def window_commands():
    """The window tests a release needs a record of (release_ci_gate.WINDOW_TESTS)."""
    return [["cargo", "nextest", "run", "--locked", "--no-fail-fast", "-p", package, "--test", target]
            for package, target in release_ci_gate.WINDOW_TESTS]


def all_window_commands(meta):
    """Every test that opens a window (`--windows --all`), one at a time: for a run on a PC
    before a release, with no record."""
    members = set(meta["workspace_members"])
    runs = []
    for package in sorted(meta["packages"], key=lambda p: p["name"]):
        if package["id"] not in members:
            continue
        name = package["name"]
        tests = sorted(t["name"] for t in package["targets"]
                       if "test" in t["kind"] and opens_windows(name, t["name"]))
        if tests:
            runs.append(["cargo", "nextest", "run", "--locked", "--no-fail-fast", "--test-threads", "1",
                         "-p", name, *[v for t in tests for v in ("--test", t)]])
    units = " | ".join(f"test({unit})" for unit in WINDOW_UNIT_TESTS)
    runs.append(["cargo", "nextest", "run", "--locked", "--no-fail-fast", "--test-threads", "1",
                 "-p", "xirius-ui", "--lib", "-E", units])
    return runs


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, encoding="utf-8").stdout.strip()


def clean_head():
    """The commit the record names: the tests must be of it, with nothing changed beside it."""
    if git("status", "--porcelain", "--untracked-files=no"):
        raise RuntimeError("commit or stash the changes first: the record names the commit")
    return git("rev-parse", "HEAD")


def windows(args):
    """Runs the window tests a release needs; on a pass, prints their record and, with --tag,
    makes the annotated tag that holds it (ARC-051, docs/release-ci-gate.md)."""
    if args.tag:
        run([sys.executable, str(Path(__file__).resolve().parent / "release.py"), "--tag", args.tag,
             "--check-tag"])
        if git("tag", "--list", args.tag):
            raise RuntimeError(f"the tag {args.tag} is already here")
    sha = clean_head()
    nextest_version()
    environment = os.environ.copy()
    environment["CARGO_BUILD_JOBS"] = "6"
    with leftovers.Watch(Path(__file__).resolve().parent.parent, environment) as watch:
        for command in window_commands():
            run(command, watch.environment)
        failures = watch.finish()
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    if clean_head() != sha:
        raise RuntimeError("HEAD moved while the window tests ran")
    record = release_ci_gate.window_record(sha, datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))
    WINDOW_RECORDS.mkdir(parents=True, exist_ok=True)
    (WINDOW_RECORDS / f"{sha}.txt").write_text(record + "\n", encoding="utf-8")
    print(record)
    if args.tag:
        git("tag", "-a", args.tag, sha, "-m", f"Xirius {args.tag[1:]}\n\n{record}")
        print(f"made the tag {args.tag} on {sha[:12]}: git push origin {args.tag}")
    return 0


def metadata():
    return json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"], check=True,
                                     capture_output=True, encoding="utf-8").stdout)


# The variables the programs find installed plugins by: `--windows --all` runs as on a PC with
# none, and the real plugins are `--plugins`'s.
PLUGIN_FOLDERS = ("CommonProgramFiles", "CommonProgramW6432", "CommonProgramFiles(x86)")
PLUGIN_PATHS = ("XIRIUS_VST3_PATH", "CLAP_PATH", "XIRIUS_TEST_VST3_INSTRUMENT", "XIRIUS_ARA_REAL_PLUGIN",
                "XIRIUS_ARA_REAL_COMMON_FILES", "XIRIUS_REAL_VST3_EFFECT", "XIRIUS_REAL_VST3_INSTRUMENT")


def windows_all(args):
    """Runs every window test with the person's folders and plugins out of reach (`leftovers.py`)."""
    selected = all_window_commands(metadata())
    if args.list:
        for command in selected:
            print(" ".join(command))
        return 0
    nextest_version()
    # The programs the tests start beside their build (the plugin and ARA hosts), as for CI.
    prepare()
    environment = os.environ.copy()
    environment["CARGO_BUILD_JOBS"] = "6"
    for name in PLUGIN_PATHS:
        environment.pop(name, None)
    with tempfile.TemporaryDirectory(prefix="xirius-no-plugins-") as nothing, \
            leftovers.Watch(Path(__file__).resolve().parent.parent, environment) as watch:
        if os.name == "nt":
            watch.environment.update({name: nothing for name in PLUGIN_FOLDERS})
        failed = []
        for command in selected:
            try:
                run(command, watch.environment)
            except subprocess.CalledProcessError as error:
                failed.append(" ".join(error.cmd))
        failures = watch.finish()
    for line in failures + [f"failed: {c}" for c in failed]:
        print(line, file=sys.stderr)
    return int(bool(failures or failed))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--plugins", action="store_true")
    parser.add_argument("--windows", action="store_true", help="the window tests a release needs a record of")
    parser.add_argument("--tag", help="with --windows: make this annotated release tag holding the record")
    parser.add_argument("--all", action="store_true", help="with --windows: every window test, with no record")
    parser.add_argument("--prepare", action="store_true", help="build required executables and fixtures")
    parser.add_argument("--archive", type=Path, help="build an archive for test shards")
    parser.add_argument("--run-archive", type=Path, help="run an archive without compiling")
    parser.add_argument("--partition", help="nextest slice:m/n")
    parser.add_argument("--coverage-dir", type=Path)
    parser.add_argument("--check-coverage", type=Path, help="check the combined observations")
    parser.add_argument("--expected-shards", type=int, default=0)
    parser.add_argument("--flaky-summary", type=Path, help="list the retried tests of junit files under this directory")
    parser.add_argument("--check-mp3-junit", type=Path, help="require all encoder-dependent MP3 cases in these JUnit files")
    parser.add_argument("--doctest", action="store_true")
    parser.add_argument("--default-features", action="store_true")
    parser.add_argument("--legacy", action="store_true", help="run the original Cargo test groups")
    args = parser.parse_args()
    if args.tag and not args.windows:
        parser.error("--tag needs --windows")
    if args.all and (args.tag or not args.windows):
        parser.error("--all goes with --windows, without --tag")
    if args.windows and args.all:
        return windows_all(args)
    if args.windows:
        if args.list:
            for command in window_commands():
                print(" ".join(command))
            return 0
        return windows(args)
    if args.check_mp3_junit:
        require_mp3_cases(sorted(args.check_mp3_junit.rglob("junit.xml")))
        return 0
    if args.flaky_summary:
        report_flaky(sorted(args.flaky_summary.rglob("junit.xml")), "all shards")
        return 0
    if args.check_coverage and args.expected_shards:
        return int(not check_shards(args.check_coverage, args.expected_shards))
    if args.check_coverage:
        return int(not context_catalog.check(args.check_coverage, Path(__file__).resolve().parent.parent))
    if args.plugins:
        if not args.list:
            missing = [v for v in plugin_variables() if not os.environ.get(v)]
            if missing:
                parser.error("set " + ", ".join(missing) + " to the plugins' files")
        selected = plugin_commands()
    elif args.doctest:
        selected = [["cargo", "test", "--locked", "-j", "6", "--workspace", "--doc"]]
    elif args.default_features:
        selected = default_feature_commands()
    elif args.legacy:
        selected = commands()
    else:
        selected = None
    if args.list:
        for command in selected or [["cargo", "nextest", "run", *nextest_targets(), "-E", test_filter()]]:
            print(" ".join(map(str, command)))
        return 0
    if args.prepare:
        prepare()
        return 0
    if args.archive:
        nextest_version()
        prepare()
        if sys.platform == "win32":
            check_plan_tests()
        run(["cargo", "nextest", "archive", "--locked", "--build-jobs", "6", *nextest_targets(),
             "--archive-file", str(args.archive)])
        return 0
    if args.partition and not args.coverage_dir:
        parser.error("a partition needs --coverage-dir for later aggregation")
    with tempfile.TemporaryDirectory(prefix="xirius-context-catalog-") as temporary:
        coverage = args.coverage_dir or Path(temporary)
        coverage.mkdir(parents=True, exist_ok=True)
        if any(coverage.iterdir()):
            raise RuntimeError("coverage directory must be empty for this run")
        environment = os.environ.copy()
        environment["CARGO_BUILD_JOBS"] = "6"
        if not args.plugins:
            environment["XIRIUS_CONTEXT_COVERAGE_DIR"] = str(coverage.resolve())
        # What the tests write outside their own folders fails the run (`leftovers.py`).
        with leftovers.Watch(Path(__file__).resolve().parent.parent, environment) as watch:
            run_tests(args, selected, coverage, watch.environment)
            failures = watch.finish()
        if failures:
            print("\n".join(failures), file=sys.stderr)
            return 1
        if not (args.plugins or args.doctest or args.default_features or args.partition):
            if not context_catalog.check(coverage, Path(__file__).resolve().parent.parent):
                return 1
    return 0


def run_tests(args, selected, coverage, environment):
    if selected is not None:
        if args.legacy:
            prepare()
        for command in selected:
            run(command, environment)
    else:
        nextest_version()
        command = ["cargo", "nextest", "run", "--no-fail-fast", "-P", "ci", "-E", test_filter()]
        if args.run_archive:
            command += ["--archive-file", str(args.run_archive), "--workspace-remap", str(Path.cwd()),
                        "--extract-to", str(Path.cwd())]
        else:
            prepare()
            command += ["--locked", *nextest_targets()]
        if args.partition:
            command += ["--partition", args.partition]
        try:
            run(command, environment)
        finally:
            if JUNIT.is_file():
                report_flaky([JUNIT], args.partition or "all")
        if args.partition:
            (coverage / "success.txt").write_text(args.partition + "\n", encoding="utf-8")
        if not args.run_archive:
            for command in default_feature_commands():
                run(command, environment)
            run(["cargo", "test", "--locked", "-j", "6", "--workspace", "--doc"], environment)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (subprocess.CalledProcessError, RuntimeError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
