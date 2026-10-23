"""Tests of scripts/release_ci_gate.py: python scripts/test_release_ci_gate.py

They never reach GitHub: the runs and jobs are written here, and the clock is a counter.
"""

import importlib.util
import re
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("release_ci_gate", HERE / "release_ci_gate.py")
gate = importlib.util.module_from_spec(spec)
sys.modules["release_ci_gate"] = gate
spec.loader.exec_module(gate)

REPO = "tekalu1/xirius"
SHA = "a" * 40
WID = 7


def run(**over):
    r = {
        "id": 1,
        "run_attempt": 1,
        "head_sha": SHA,
        "workflow_id": WID,
        "path": gate.WORKFLOW,
        "event": "push",
        "head_branch": "main",
        "status": "completed",
        "conclusion": "success",
        "repository": {"full_name": REPO},
        "head_repository": {"full_name": REPO},
    }
    r.update(over)
    return r


def jobs(conclusion="success", status="completed", names=None):
    return [
        {"name": n, "status": status, "conclusion": conclusion}
        for n in (names if names is not None else gate.REQUIRED_JOBS)
    ]


class MatchTests(unittest.TestCase):
    def test_only_main_pushes_of_the_same_commit_and_workflow_count(self):
        good = run()
        others = [
            run(head_sha="b" * 40),
            run(workflow_id=8),
            run(event="pull_request"),
            run(event="workflow_dispatch"),
            run(head_branch="wip"),
            run(path=".github/workflows/plan.yml"),
            run(head_repository={"full_name": "someone/xirius"}),
        ]
        self.assertEqual(gate.matching_runs([good, *others], SHA, WID, REPO), [good])
        # The path GitHub gives may carry the ref after an @.
        at = run(path=gate.WORKFLOW + "@refs/heads/main")
        self.assertEqual(gate.matching_runs([at], SHA, WID, REPO), [at])


class DecideTests(unittest.TestCase):
    def decide(self, runs, by_id):
        return gate.decide(runs, lambda r: by_id[r["id"]])

    def test_a_green_run_with_every_required_job_passes(self):
        state, why = self.decide([run()], {1: jobs()})
        self.assertEqual(state, "pass", why)

    def test_no_run_is_missing(self):
        self.assertEqual(self.decide([], {})[0], "missing")

    def test_a_red_run_or_job_stops_even_while_it_is_going(self):
        self.assertEqual(self.decide([run(conclusion="failure")], {1: jobs()})[0], "red")
        going = run(status="in_progress", conclusion=None)
        one_red = jobs()
        one_red[0]["conclusion"] = "failure"
        self.assertEqual(self.decide([going], {1: one_red})[0], "red")
        for c in ("timed_out", "action_required", "startup_failure"):
            self.assertEqual(self.decide([run(conclusion=c)], {1: jobs()})[0], "red", c)

    def test_a_going_run_is_waited_for(self):
        going = run(status="in_progress", conclusion=None)
        self.assertEqual(
            self.decide([going], {1: jobs(conclusion=None, status="in_progress")})[0], "wait"
        )
        queued = run(status="queued", conclusion=None)
        self.assertEqual(self.decide([queued], {1: []})[0], "wait")

    def test_cancelled_skipped_and_short_runs_are_not_a_yes(self):
        for c in ("cancelled", "skipped", "neutral"):
            self.assertEqual(self.decide([run(conclusion=c)], {1: jobs()})[0], "unusable", c)
        # The run says success, but a job was skipped, missing or there twice.
        skipped = jobs()
        skipped[1]["conclusion"] = "skipped"
        self.assertEqual(self.decide([run()], {1: skipped})[0], "unusable")
        short = jobs(names=gate.REQUIRED_JOBS[:-1])
        self.assertEqual(self.decide([run()], {1: short})[0], "unusable")
        twice = jobs(names=[*gate.REQUIRED_JOBS, gate.REQUIRED_JOBS[0]])
        self.assertEqual(self.decide([run()], {1: twice})[0], "unusable")

    def test_one_red_run_of_the_commit_is_enough_and_a_going_one_is_waited_for(self):
        green, red = run(id=1), run(id=2, conclusion="failure")
        self.assertEqual(self.decide([green, red], {1: jobs(), 2: jobs()})[0], "red")
        going = run(id=2, status="in_progress", conclusion=None)
        self.assertEqual(
            self.decide([green, going], {1: jobs(), 2: jobs(None, "in_progress")})[0], "wait"
        )
        cancelled = run(id=2, conclusion="cancelled")
        self.assertEqual(self.decide([green, cancelled], {1: jobs(), 2: jobs()})[0], "pass")


class LoopTests(unittest.TestCase):
    def loop(self, answers, wait=40 * 60, grace=5 * 60, interval=30, tries=5):
        """Runs the loop over `answers` (a state or an ApiError per ask); returns the result
        and how many times it asked."""
        now = [0.0]
        asked = [0]

        def look():
            a = answers[min(asked[0], len(answers) - 1)]
            asked[0] += 1
            if isinstance(a, Exception):
                raise a
            return a, a

        def sleep(s):
            now[0] += s

        ok, why = gate.gate(look, wait, grace, interval, tries,
                            clock=lambda: now[0], sleep=sleep, say=lambda *_: None)
        return ok, why, asked[0]

    def test_it_waits_for_a_going_run_and_then_passes(self):
        ok, _, asked = self.loop(["wait", "wait", "pass"])
        self.assertTrue(ok)
        self.assertEqual(asked, 3)

    def test_red_stops_at_once(self):
        ok, _, asked = self.loop(["red"])
        self.assertFalse(ok)
        self.assertEqual(asked, 1)

    def test_a_run_that_never_ends_stops_at_the_limit(self):
        ok, why, asked = self.loop(["wait"])
        self.assertFalse(ok)
        self.assertIn("40 minutes", why)
        self.assertEqual(asked, 40 * 60 // 30 + 1)

    def test_no_run_is_waited_for_a_while_and_then_stops(self):
        ok, why, asked = self.loop(["missing"])
        self.assertFalse(ok)
        self.assertIn("5 minutes", why)
        self.assertEqual(asked, 5 * 60 // 30 + 1)
        # A run that shows up within the grace is followed.
        ok, _, _ = self.loop(["missing", "missing", "wait", "pass"])
        self.assertTrue(ok)

    def test_api_failures_are_tried_again_and_then_stop(self):
        err = gate.ApiError("GET x: timed out")
        ok, _, _ = self.loop([err, err, "pass"])
        self.assertTrue(ok)
        ok, why, asked = self.loop([err], tries=3)
        self.assertFalse(ok)
        self.assertIn("3 times", why)
        self.assertEqual(asked, 3)


class WorkflowTests(unittest.TestCase):
    """The required jobs are rust.yml's jobs, and the release waits on rust.yml."""

    def test_rust_yml_has_the_required_jobs(self):
        text = (HERE.parent / gate.WORKFLOW).read_text(encoding="utf-8")
        self.assertRegex(text, r"(?m)^  push:\n    branches: \[main\]")
        names = set(re.findall(r"(?m)^    name: (.+)$", text))
        oses = re.search(r"(?m)^        os: \[(.*)\]$", text)
        self.assertIsNotNone(oses, "the check job's matrix of OSes")
        expanded = set()
        for n in names:
            if "${{ matrix.os }}" in n:
                expanded |= {n.replace("${{ matrix.os }}", o.strip()) for o in oses[1].split(",")}
            else:
                expanded.add(n)
        self.assertEqual(expanded, set(gate.REQUIRED_JOBS))

    def test_the_release_runs_the_gate_before_signing_and_publishing(self):
        text = (HERE.parent / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn("python scripts/release_ci_gate.py", text)
        self.assertRegex(text, r"(?ms)^  sign:\n    needs: \[build, ci-gate\]")
        self.assertRegex(text, r"(?ms)^  publish:\n    needs: \[build, ci-gate, sign\]")
        self.assertRegex(text, r"--wait-minutes 40\b")


if __name__ == "__main__":
    sys.exit(unittest.main())
