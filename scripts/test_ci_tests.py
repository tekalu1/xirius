"""Test the shared test selection and fail-closed shard aggregation."""
import contextlib
import io
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path
from unittest.mock import patch

import ci_tests
import leftovers


class SelectionTests(unittest.TestCase):
    def test_window_exclusions_and_real_plugin_entrypoints_are_preserved(self):
        for package, targets in ci_tests.WINDOW_TESTS.items():
            for target in targets:
                self.assertTrue(ci_tests.opens_windows(package, target))
        self.assertTrue(ci_tests.opens_windows("xirius-ui", "new_window"))
        self.assertTrue(ci_tests.opens_windows("xirius-ui", "new_windows"))
        self.assertFalse(ci_tests.opens_windows("xirius-session", "empty_instrument"))
        self.assertEqual(len(ci_tests.plugin_commands()), 3)
        for command in ci_tests.plugin_commands():
            self.assertIn("--ignored", command)
        self.assertIn("not test(mixer::editor_tests)", ci_tests.test_filter())

    def test_workspace_targets_are_the_union_of_legacy_integration_targets(self):
        commands = [["cargo", "test", "--test", "a", "--test", "b"],
                    ["cargo", "test", "--test", "b", "--test", "c"]]
        with patch.object(ci_tests, "commands", return_value=commands):
            self.assertEqual(ci_tests.nextest_targets(), ["--workspace", "--lib", "--bins",
                             "--test", "a", "--test", "b", "--test", "c"])


class CoverageTests(unittest.TestCase):
    def test_missing_failed_or_cancelled_shard_cannot_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(RuntimeError):
                ci_tests.check_shards(tmp, 4)
            directory = Path(tmp) / "coverage-windows-latest-1"
            directory.mkdir()
            (directory / "1-0.json").write_text("{}")
            with self.assertRaises(RuntimeError):
                ci_tests.check_shards(tmp, 1)
            (directory / "success.txt").write_text("slice:2/4")
            with self.assertRaises(RuntimeError):
                ci_tests.check_shards(tmp, 1)
            (directory / "success.txt").write_text("slice:1/1")
            with patch.object(ci_tests.context_catalog, "check", return_value=False):
                self.assertFalse(ci_tests.check_shards(tmp, 1))

    def test_existing_observations_are_rejected_before_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "success.txt").write_text("slice:1/4")
            with patch("sys.argv", ["ci_tests.py", "--partition", "slice:1/4", "--coverage-dir", tmp]):
                with self.assertRaisesRegex(RuntimeError, "must be empty"):
                    ci_tests.main()

    def test_equal_names_from_different_shards_are_not_overwritten(self):
        with tempfile.TemporaryDirectory() as tmp:
            for shard in range(1, 5):
                directory = Path(tmp) / f"coverage-windows-latest-{shard}"
                directory.mkdir()
                (directory / "success.txt").write_text(f"slice:{shard}/4")
                (directory / "1-0.json").write_text(json.dumps({"shard": shard}))
            def check(directory, root):
                records = [json.loads(p.read_text()) for p in Path(directory).glob("*.json")]
                self.assertEqual(sorted(r["shard"] for r in records), [1, 2, 3, 4])
                return True
            with patch.object(ci_tests.context_catalog, "check", side_effect=check):
                self.assertTrue(ci_tests.check_shards(tmp, 4))

    def test_release_checks_depend_on_build_and_all_shards(self):
        workflow = (Path(__file__).resolve().parent.parent / ".github/workflows/rust.yml").read_text(encoding="utf-8")
        aggregate = workflow.split("  check:\n", 1)[1].split("  deny:", 1)[0]
        self.assertIn("needs: [build, test]", aggregate)
        self.assertIn("if: always()", aggregate)
        self.assertIn('assert os.environ["BUILD_RESULT"] == "success"', aggregate)
        self.assertIn('assert os.environ["TEST_RESULT"] == "success"', aggregate)
        self.assertIn("--expected-shards 4", aggregate)
        self.assertIn("save-if: ${{ github.ref == 'refs/heads/main' }}", workflow)


class FlakyTests(unittest.TestCase):
    JUNIT = """<testsuites><testsuite name="a">
      <testcase name="t::ok" classname="a"/>
      <testcase name="t::flaky" classname="a"><flakyFailure message="m"/></testcase>
      <testcase name="t::bad" classname="a"><failure message="m"/><rerunFailure message="m"/></testcase>
    </testsuite></testsuites>"""

    def test_only_the_tests_that_passed_on_a_retry_are_flaky(self):
        with tempfile.TemporaryDirectory() as tmp:
            junit = Path(tmp) / "junit.xml"
            junit.write_text(self.JUNIT, encoding="utf-8")
            self.assertEqual(ci_tests.flaky_tests(junit), ["a t::flaky"])

    def test_the_summary_lists_each_test_once(self):
        with tempfile.TemporaryDirectory() as tmp:
            first = Path(tmp) / "one" / "junit.xml"
            second = Path(tmp) / "two" / "junit.xml"
            for junit in (first, second):
                junit.parent.mkdir()
                junit.write_text(self.JUNIT, encoding="utf-8")
            summary = Path(tmp) / "summary.md"
            # The workflow command it prints is for the job log; a test must not emit one.
            printed = io.StringIO()
            with patch.dict("os.environ", {"GITHUB_STEP_SUMMARY": str(summary)}), contextlib.redirect_stdout(printed):
                self.assertEqual(ci_tests.report_flaky([first, second], "x"), ["a t::flaky"])
            self.assertEqual(printed.getvalue().count("::warning"), 1)
            self.assertEqual(summary.read_text(encoding="utf-8").count("`a t::flaky`"), 1)

    def test_the_workflow_reports_the_retried_tests_of_every_shard(self):
        workflow = (Path(__file__).resolve().parent.parent / ".github/workflows/rust.yml").read_text(encoding="utf-8")
        aggregate = workflow.split("  check:\n", 1)[1].split("  deny:", 1)[0]
        self.assertIn("--flaky-summary junit", aggregate)
        self.assertIn("pattern: junit-${{ matrix.os }}-*", aggregate)


class LeftoverTests(unittest.TestCase):
    def watch(self, tmp, processes=()):
        place = Path(tmp) / "place"
        (place / "Crashes").mkdir(parents=True, exist_ok=True)
        (place / "prefs.toml").write_text("a")
        checkout = Path(tmp) / "checkout"
        checkout.mkdir(exist_ok=True)
        real = leftovers.foreign_xirius
        patcher = patch.object(leftovers, "foreign_xirius",
                               side_effect=lambda c, t, p=None: real(c, t, list(processes)))
        patcher.start()
        self.addCleanup(patcher.stop)
        return place, leftovers.Watch(checkout, {"PATH": "x"}, places=[place], every=60)

    def test_the_places_named_are_the_programs_default_folders(self):
        windows = leftovers.user_places({"LOCALAPPDATA": "L", "APPDATA": "R"}, "win32")
        self.assertEqual(windows, [Path("L/Xirius"), Path("R/Xirius")])
        mac = leftovers.user_places({"HOME": "/h"}, "darwin")
        self.assertEqual(mac, [Path("/h/Library/Application Support/Xirius"), Path("/h/Library/Caches/Xirius"),
                               Path("/h/Library/Logs/Xirius")])
        self.assertEqual(leftovers.user_places({"HOME": "/h"}, "linux"), [Path("/h/.config/xirius"),
                                                                          Path("/h/.cache/xirius")])

    def test_the_stand_in_points_only_the_variables_the_programs_find_the_folders_by(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            windows = leftovers.stand_in(root / "w", {"LOCALAPPDATA": "L", "APPDATA": "R", "PATH": "x"}, "win32")
            self.assertEqual(windows, {"LOCALAPPDATA": str(root / "w" / "Local"),
                                       "APPDATA": str(root / "w" / "Roaming")})
            self.assertEqual(leftovers.user_places(windows, "win32"),
                             [root / "w" / "Local" / "Xirius", root / "w" / "Roaming" / "Xirius"])
            mac = leftovers.stand_in(root / "m", {"HOME": "/h", "RUSTUP_HOME": "/r"}, "darwin")
            self.assertEqual(mac, {"HOME": str(root / "m"), "CARGO_HOME": str(Path("/h/.cargo")),
                                   "RUSTUP_HOME": str(Path("/r"))})
            self.assertEqual(leftovers.user_places(mac, "darwin"),
                             [root / "m" / "Library/Application Support/Xirius",
                              root / "m" / "Library/Caches/Xirius", root / "m" / "Library/Logs/Xirius"])
            linux = leftovers.stand_in(root / "x", {"HOME": "/h"}, "linux")
            self.assertEqual(leftovers.user_places({"HOME": "/h", **linux}, "linux"),
                             [root / "x" / ".config" / "xirius", root / "x" / ".cache" / "xirius"])
            for folder, platform in ((root / "w", "win32"), (root / "m", "darwin"), (root / "x", "linux")):
                environment = {"HOME": "/h", **leftovers.stand_in(folder, {"HOME": "/h"}, platform)}
                for place in leftovers.user_places(environment, platform):
                    self.assertTrue(place.parent.is_dir(), place)
                    self.assertFalse(place.exists(), place)

    def test_the_tests_run_with_the_persons_folders_pointed_at_an_empty_folder_of_the_runs_own(self):
        with tempfile.TemporaryDirectory() as tmp:
            place, watch = self.watch(tmp)
            with watch:
                ran_with = leftovers.user_places(watch.environment)
                self.assertTrue(ran_with)
                self.assertEqual(ran_with, watch.stand_in_places)
                for folder in ran_with:
                    self.assertTrue(folder.is_relative_to(watch.stand_in), folder)
                    self.assertFalse(folder.exists(), folder)
                self.assertEqual(watch.finish(), [])
            self.assertFalse(watch.stand_in.exists())

    def test_what_the_tests_make_in_the_empty_folders_fails_by_name_even_beside_another_xirius(self):
        installed = ("Xirius.exe", str(Path(tempfile.gettempdir()) / "elsewhere" / "Xirius.exe"))
        for processes in ((), (installed,)):
            with tempfile.TemporaryDirectory() as tmp:
                place, watch = self.watch(tmp, processes)
                with watch, contextlib.redirect_stdout(io.StringIO()):
                    first, last = watch.stand_in_places[0], watch.stand_in_places[-1]
                    (first / "Crashes").mkdir(parents=True)
                    last.mkdir(parents=True, exist_ok=True)
                    (last / "browser.json.lock").write_text("")
                    failures = "\n".join(watch.finish())
                self.assertIn("+ " + str(first / "Crashes"), failures)
                self.assertIn("+ " + str(last / "browser.json.lock"), failures)
                self.assertFalse(watch.stand_in.exists())

    def test_a_run_that_leaves_nothing_passes_and_removes_its_temporary_folder(self):
        with tempfile.TemporaryDirectory() as tmp:
            place, watch = self.watch(tmp)
            with watch:
                self.assertEqual(watch.environment["TMP"], str(watch.temporary))
                self.assertEqual(watch.environment["TMPDIR"], str(watch.temporary))
                self.assertEqual(watch.environment["PATH"], "x")
                (watch.temporary / "a-test").mkdir()
                (watch.temporary / "a-test" / "f").write_text("x")
                shutil.rmtree(watch.temporary / "a-test")
                self.assertEqual(watch.finish(), [])
            self.assertFalse(watch.temporary.exists())

    def test_a_file_written_in_the_places_or_left_in_the_temporary_folder_fails_by_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            place, watch = self.watch(tmp)
            with watch, contextlib.redirect_stdout(io.StringIO()):
                (place / "Crashes" / "1-2.dmp").write_text("x")
                (place / "prefs.toml").write_text("bb")
                (watch.temporary / "xirius-x-1").mkdir()
                (watch.temporary / "xirius-x-1" / "f").write_text("x")
                failures = "\n".join(watch.finish())
            self.assertIn("+ " + str(place / "Crashes" / "1-2.dmp"), failures)
            self.assertIn("~ " + str(place / "prefs.toml"), failures)
            self.assertIn("xirius-x-1" + os.sep + " (1 inside)", failures)
            self.assertFalse(watch.temporary.exists())

    def test_each_line_names_when_the_path_was_last_written_to_find_the_test_in_the_log(self):
        written = datetime(2026, 11, 16, 4, 19, 32, tzinfo=timezone.utc).timestamp()
        stamp = " (written 2026-11-16T04:19:32Z)"
        with tempfile.TemporaryDirectory() as tmp:
            place, watch = self.watch(tmp)
            (place / "gone.json").write_text("x")
            with watch, contextlib.redirect_stdout(io.StringIO()):
                made = watch.stand_in_places[0] / "Crashes"
                made.mkdir(parents=True)
                new = place / "Crashes" / "1-2.dmp"
                new.write_text("x")
                (place / "prefs.toml").write_text("bb")
                (place / "gone.json").unlink()
                (watch.temporary / "left").write_text("x")
                (watch.temporary / "xirius-x-1").mkdir()
                for path in (made, new, place / "prefs.toml", watch.temporary / "left", watch.temporary / "xirius-x-1"):
                    os.utime(path, (written, written))
                failures = watch.finish()
            self.assertIn("  + " + str(made) + stamp, failures)
            self.assertIn("  + " + str(new) + stamp, failures)
            self.assertIn("  ~ " + str(place / "prefs.toml") + stamp, failures)
            self.assertIn("  - " + str(place / "gone.json"), failures)
            self.assertIn("  left" + stamp, failures)
            self.assertIn("  xirius-x-1" + os.sep + " (0 inside)" + stamp, failures)
            # The folder a new file went into is still not a change of its own.
            self.assertFalse([line for line in failures if line.startswith("  ~ " + str(place / "Crashes"))])

    def test_the_shared_list_of_running_jobs_counts_only_for_what_is_left_in_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            place, watch = self.watch(tmp)
            with watch:
                self.assertTrue((watch.temporary / "Xirius" / "jobs").is_dir())
                self.assertEqual(watch.finish(), [])
            place, watch = self.watch(tmp)
            with watch, contextlib.redirect_stdout(io.StringIO()):
                (watch.temporary / "Xirius" / "jobs" / "7-1.json").write_text("x")
                (watch.temporary / "Xirius" / "Bounces").mkdir()
                failures = "\n".join(watch.finish())
            self.assertIn("Xirius" + os.sep + "jobs" + os.sep + "7-1.json", failures)
            self.assertIn("Xirius" + os.sep + "Bounces" + os.sep + " (0 inside)", failures)
            self.assertNotIn("Xirius" + os.sep + " (", failures)

    def test_beside_another_xirius_the_places_warn_but_the_temporary_folder_still_counts(self):
        with tempfile.TemporaryDirectory() as tmp:
            installed = str(Path(tmp) / "elsewhere" / "Xirius.exe")
            place, watch = self.watch(tmp, [("Xirius.exe", installed)])
            printed = io.StringIO()
            with watch, contextlib.redirect_stdout(printed):
                (place / "Crashes" / "1-2.dmp").write_text("x")
                self.assertEqual(watch.finish(), [])
            self.assertIn("warning", printed.getvalue())
            self.assertIn("1-2.dmp", printed.getvalue())
            place, watch = self.watch(tmp, [("Xirius.exe", installed)])
            with watch, contextlib.redirect_stdout(io.StringIO()):
                (watch.temporary / "left").write_text("x")
                self.assertEqual(len(watch.finish()), 2)

    def test_a_name_made_directly_in_the_checkouts_target_fails_by_name_but_the_builds_do_not(self):
        written = datetime(2026, 11, 16, 4, 19, 32, tzinfo=timezone.utc).timestamp()
        stamp = " (written 2026-11-16T04:19:32Z)"
        with tempfile.TemporaryDirectory() as tmp:
            place, watch = self.watch(tmp)
            target = watch.checkout / "target"
            (target / "debug").mkdir(parents=True)
            (target / "ctx-audio-old").mkdir()
            with watch, contextlib.redirect_stdout(io.StringIO()):
                (target / "ctx2-reveal-1").mkdir()
                (target / "ctx2-reveal-1" / "song.xirius").write_text("x")
                (target / "pdc.wav").write_text("x")
                for own in ("nextest", "tmp", "release"):
                    (target / own).mkdir()
                (target / ".rustc_info.json").write_text("{}")
                (target / "debug" / "xirius-test-crash-records").mkdir()
                (target / "ctx-audio-old" / "tone.wav").write_text("x")
                for path in (target / "ctx2-reveal-1", target / "pdc.wav"):
                    os.utime(path, (written, written))
                failures = watch.finish()
            self.assertIn("  ctx2-reveal-1" + os.sep + stamp, failures)
            self.assertIn("  pdc.wav" + stamp, failures)
            self.assertEqual(len([line for line in failures if line.startswith("  ")]), 2, failures)
            self.assertIn(str(target), failures[0])

    def test_a_checkout_with_no_target_before_the_run_counts_only_the_names_made_besides_the_builds(self):
        with tempfile.TemporaryDirectory() as tmp:
            place, watch = self.watch(tmp)
            target = watch.checkout / "target"
            with watch:
                # The archive a shard runs is unpacked into the checkout during the run.
                (target / "debug" / "deps").mkdir(parents=True)
                (target / "nextest").mkdir()
                self.assertEqual(watch.finish(), [])
            place, watch = self.watch(tmp)
            with watch, contextlib.redirect_stdout(io.StringIO()):
                (target / "export-actions-ui-1").mkdir()
                failures = "\n".join(watch.finish())
            self.assertIn("export-actions-ui-1" + os.sep, failures)

    def test_a_watch_that_cannot_start_removes_its_temporary_folder(self):
        with tempfile.TemporaryDirectory() as tmp:
            place, watch = self.watch(tmp)
            with patch.object(leftovers, "listing", side_effect=OSError("x")), self.assertRaises(OSError):
                with watch:
                    pass
            self.assertFalse(watch.temporary.exists())

    def test_this_checkouts_programs_and_the_ones_the_tests_copied_are_not_another_xirius(self):
        with tempfile.TemporaryDirectory() as tmp:
            checkout, temporary = Path(tmp) / "c", Path(tmp) / "t"
            processes = [("xirius-plugin-host.exe", str(checkout / "target" / "debug" / "xirius-plugin-host.exe")),
                         ("xirius-copy.exe", str(temporary / "a" / "xirius-copy.exe")),
                         ("other.exe", str(Path(tmp) / "other.exe")),
                         ("xirius-engine", None),
                         ("Xirius.exe", str(Path(tmp) / "app" / "Xirius.exe"))]
            self.assertEqual(leftovers.foreign_xirius(checkout, temporary, processes),
                             [str(Path(tmp) / "app" / "Xirius.exe")])

    def test_the_tests_run_inside_the_watch_and_what_they_leave_fails_the_run(self):
        seen = []

        def run(command, environment=None):
            seen.append((command[:3], environment))
            if command[:2] == ["cargo", "test"]:
                Path(environment["TMP"], "left-by-a-test").mkdir()

        printed = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp, patch("sys.argv", ["ci_tests.py", "--doctest"]), \
                patch.object(ci_tests, "run", side_effect=run), \
                patch.object(ci_tests.leftovers, "user_places", return_value=[Path(tmp) / "none"]), \
                contextlib.redirect_stdout(printed), contextlib.redirect_stderr(printed):
            self.assertEqual(ci_tests.main(), 1)
        self.assertEqual(len(seen), 1)
        temporary = seen[0][1]["TMP"]
        self.assertEqual(seen[0][1]["TEMP"], temporary)
        self.assertIn("left-by-a-test", printed.getvalue())
        self.assertFalse(Path(temporary).exists())

    def test_the_tests_cannot_reach_the_persons_folders_and_what_they_make_instead_fails_the_run(self):
        seen = []
        real = leftovers.user_places

        def run(command, environment=None):
            seen.append(environment)
            (real(environment)[0] / "Crashes").mkdir(parents=True)

        printed = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp, patch("sys.argv", ["ci_tests.py", "--doctest"]),                 patch.object(ci_tests, "run", side_effect=run),                 patch.object(ci_tests.leftovers, "user_places",
                             side_effect=lambda environ=None, platform=None:
                             [Path(tmp) / "person"] if environ is None else real(environ, platform)),                 contextlib.redirect_stdout(printed), contextlib.redirect_stderr(printed):
            self.assertEqual(ci_tests.main(), 1)
            self.assertFalse((Path(tmp) / "person").exists())
        made = real(seen[0])[0] / "Crashes"
        self.assertNotIn(made.parent, real())
        self.assertIn("+ " + str(made), printed.getvalue())
        self.assertFalse(made.exists())

    def test_what_failing_tests_leave_is_named_too(self):
        seen = []

        def run(command, environment=None):
            seen.append(environment)
            Path(environment["TMP"], "left-by-a-failing-test").mkdir()
            raise subprocess.CalledProcessError(1, command)

        printed = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp, patch("sys.argv", ["ci_tests.py", "--doctest"]), \
                patch.object(ci_tests, "run", side_effect=run), \
                patch.object(ci_tests.leftovers, "user_places", return_value=[Path(tmp) / "none"]), \
                contextlib.redirect_stdout(printed), contextlib.redirect_stderr(printed):
            with self.assertRaises(subprocess.CalledProcessError):
                ci_tests.main()
        self.assertIn("left-by-a-failing-test", printed.getvalue())
        self.assertFalse(Path(seen[0]["TMP"]).exists())


if __name__ == "__main__":
    unittest.main()
