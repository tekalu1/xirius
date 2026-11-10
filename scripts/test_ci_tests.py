"""Test the shared test selection and fail-closed shard aggregation."""
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import ci_tests


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
            with patch.dict("os.environ", {"GITHUB_STEP_SUMMARY": str(summary)}):
                self.assertEqual(ci_tests.report_flaky([first, second], "x"), ["a t::flaky"])
            self.assertEqual(summary.read_text(encoding="utf-8").count("`a t::flaky`"), 1)

    def test_the_workflow_reports_the_retried_tests_of_every_shard(self):
        workflow = (Path(__file__).resolve().parent.parent / ".github/workflows/rust.yml").read_text(encoding="utf-8")
        aggregate = workflow.split("  check:\n", 1)[1].split("  deny:", 1)[0]
        self.assertIn("--flaky-summary junit", aggregate)
        self.assertIn("pattern: junit-${{ matrix.os }}-*", aggregate)


if __name__ == "__main__":
    unittest.main()
