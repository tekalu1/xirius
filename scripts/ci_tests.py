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
import subprocess
import sys

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


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--list", action="store_true", help="print the commands, run nothing")
    parser.add_argument(
        "--plugins", action="store_true", help="the tests with a real ARA plugin instead"
    )
    args = parser.parse_args()
    if args.plugins and not args.list:
        missing = [v for v in sorted(PLUGIN_TESTS.values()) if not os.environ.get(v)]
        if missing:
            print("set " + ", ".join(missing) + " to the plugins' files", file=sys.stderr)
            return 1
    failed = []
    for command in plugin_commands() if args.plugins else commands():
        print("$ " + " ".join(command), flush=True)
        if args.list:
            continue
        if subprocess.run(command).returncode != 0:
            failed.append(" ".join(command))
    if failed:
        print("failed:\n  " + "\n  ".join(failed), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
