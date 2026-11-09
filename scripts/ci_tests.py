"""Run the workspace's tests that do not open windows (the CI's test step).

The tests that open real windows (drive the app with `--script`, move windows, open plugin
editors) need a desktop of a known size and a person-free screen. They are run on a PC
before a release, not on CI: a CI runner's desktop is smaller and its timing differs, so they
fail there for reasons that are not the app's. Everything else runs here.

    python scripts/ci_tests.py            # run
    python scripts/ci_tests.py --list     # print the cargo commands only
    python scripts/ci_tests.py --plugins  # the tests with a real ARA plugin, before a release

The tests with a real ARA plugin are `#[ignore]`d, so a run without one never passes them by
skipping: each needs its variable set to the plugin's file (`docs/ara-verification.md`), and
fails when it is not.

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
from pathlib import Path

import context_catalog

# The integration tests that open windows, by package.
WINDOW_TESTS = {
    "xirius-ui": {"crash_entry"},  # and every `*_window` / `*_windows` target
    "xirius-session": {
        "eng067_recapture",
        "eng070_window_moves",
        "eng071_place_moves",
        "plugin_release_thread",
    },
    "xirius-vst3": {"editor_caption", "editor_resize", "vendor_editor_windows"},
}
# Unit tests inside a library that open windows.
WINDOW_UNIT_TESTS = ["mixer::editor_tests"]
# The integration tests that need a real ARA plugin, by package and target: the variable that
# names the plugin's file.
PLUGIN_TESTS = {
    ("xirius-ara", "test_plugin"): "XIRIUS_ARA_TEST_PLUGIN",
    ("xirius-session", "ara_real"): "XIRIUS_ARA_REAL_PLUGIN",
    ("xirius-ui", "ara_real_window"): "XIRIUS_ARA_REAL_PLUGIN",
}


def opens_windows(package, target):
    if target in WINDOW_TESTS.get(package, set()):
        return True
    return package == "xirius-ui" and (target.endswith("_window") or target.endswith("_windows"))


def commands():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            check=True,
            capture_output=True,
            encoding="utf-8",
        ).stdout
    )
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
        ["cargo", "test", "-p", package, "--test", target, "--", "--ignored", "--nocapture"]
        for (package, target) in sorted(PLUGIN_TESTS)
    ]


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
    # --tests unifies the same dev features as the workspace test build, including test DLLs.
    run(["cargo", "build", "--locked", "--workspace", "--bins", "--tests", "-j", "6"])


def default_feature_commands():
    # Exercise ordinary dependencies without the UI's test-io and engine's warn_debug union.
    return [
        ["cargo", "test", "--locked", "-j", "6", "-p", "xirius-core", "--test", "state_hash", "--test", "history_writer"],
        ["cargo", "test", "--locked", "-j", "6", "-p", "xirius-session", "--test", "empty_instrument"],
        ["cargo", "test", "--locked", "-j", "6", "-p", "xirius-cli", "--test", "song_flow", "--test", "prefs_cli"],
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


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--plugins", action="store_true")
    parser.add_argument("--prepare", action="store_true", help="build required executables and fixtures")
    parser.add_argument("--archive", type=Path, help="build an archive for test shards")
    parser.add_argument("--run-archive", type=Path, help="run an archive without compiling")
    parser.add_argument("--partition", help="nextest slice:m/n")
    parser.add_argument("--coverage-dir", type=Path)
    parser.add_argument("--check-coverage", type=Path, help="check the combined observations")
    parser.add_argument("--expected-shards", type=int, default=0)
    parser.add_argument("--doctest", action="store_true")
    parser.add_argument("--default-features", action="store_true")
    parser.add_argument("--legacy", action="store_true", help="run the original Cargo test groups")
    args = parser.parse_args()
    if args.check_coverage and args.expected_shards:
        return int(not check_shards(args.check_coverage, args.expected_shards))
    if args.check_coverage:
        return int(not context_catalog.check(args.check_coverage, Path(__file__).resolve().parent.parent))
    if args.plugins:
        if not args.list:
            missing = sorted({v for v in PLUGIN_TESTS.values() if not os.environ.get(v)})
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
        run(["cargo", "nextest", "archive", "--locked", "-j", "6", *nextest_targets(),
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
            run(command, environment)
            if args.partition:
                (coverage / "success.txt").write_text(args.partition + "\n", encoding="utf-8")
            if not args.run_archive:
                for command in default_feature_commands():
                    run(command, environment)
                run(["cargo", "test", "--locked", "-j", "6", "--workspace", "--doc"], environment)
        if not (args.plugins or args.doctest or args.default_features or args.partition):
            if not context_catalog.check(coverage, Path(__file__).resolve().parent.parent):
                return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (subprocess.CalledProcessError, RuntimeError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
