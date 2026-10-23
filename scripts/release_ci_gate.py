#!/usr/bin/env python3
"""Does main's CI say yes to the commit of a release tag? (LIC-039)

    python scripts/release_ci_gate.py --repo tekalu1/xirius --sha <commit> --tag v1.0.1

The release workflow runs this before it signs and publishes (docs/release-ci-gate.md). It
reads, with GET requests only, the runs of `.github/workflows/rust.yml` that a push to `main`
started on the same commit, and:

  - exits 0 when such a run's latest attempt finished `success` with every job `success` and
    each of REQUIRED_JOBS there exactly once;
  - exits 1 at once when a run or a job is red (`failure`, `timed_out`, `action_required`,
    `startup_failure`), even while the rest of the run is still going;
  - waits while a run is going (up to --wait-minutes), and while there is no run yet (up to
    --missing-grace-minutes: the tag and main are often pushed together);
  - exits 1 for everything else: no run, `cancelled` / `skipped` / `neutral`, a job missing
    or twice, the wait running out, the API failing again and again.

The token is the job's GITHUB_TOKEN (`actions: read`, `contents: read`); nothing is written to
GitHub. With GITHUB_OUTPUT set, `sha=<commit>` is written there on success.

Standard library only.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

WORKFLOW = ".github/workflows/rust.yml"
BRANCH = "main"
# The names of rust.yml's jobs as GitHub shows them (`check (${{ matrix.os }})` per OS).
# scripts/test_release_ci_gate.py reads rust.yml and checks they are still there.
REQUIRED_JOBS = ["check (windows-latest)", "cargo-deny", "gitleaks"]

RED = {"failure", "timed_out", "action_required", "startup_failure"}
API = "https://api.github.com"


class ApiError(Exception):
    pass


# ------------------------------------------------------------------ the rule

def matching_runs(runs: list[dict], sha: str, workflow_id: int, repo: str) -> list[dict]:
    """The runs that are main's CI of `sha`: same commit, rust.yml, a push to main, this repo."""
    out = []
    for r in runs:
        if r.get("head_sha") != sha or r.get("workflow_id") != workflow_id:
            continue
        if r.get("event") != "push" or r.get("head_branch") != BRANCH:
            continue
        path = str(r.get("path") or "").split("@")[0]
        if path != WORKFLOW:
            continue
        if (r.get("repository") or {}).get("full_name") != repo:
            continue
        if (r.get("head_repository") or {}).get("full_name") != repo:
            continue
        out.append(r)
    return out


def run_verdict(run: dict, jobs: list[dict]) -> tuple[str, str]:
    """`red`, `going`, `success` or `unusable`, and why, for one run's latest attempt."""
    name = f"run {run.get('id')} (attempt {run.get('run_attempt', 1)})"
    if run.get("conclusion") in RED:
        return "red", f"{name} is {run['conclusion']}"
    for j in jobs:
        if j.get("conclusion") in RED:
            return "red", f"{name}: job {j.get('name')!r} is {j['conclusion']}"
    if run.get("status") != "completed":
        return "going", f"{name} is {run.get('status')}"
    if run.get("conclusion") != "success":
        return "unusable", f"{name} is {run.get('conclusion')}"
    for j in jobs:
        if j.get("status") != "completed" or j.get("conclusion") != "success":
            return "unusable", f"{name}: job {j.get('name')!r} is {j.get('conclusion') or j.get('status')}"
    names = [j.get("name") for j in jobs]
    for required in REQUIRED_JOBS:
        n = names.count(required)
        if n != 1:
            return "unusable", f"{name}: job {required!r} is there {n} times, not once"
    return "success", f"{name} is success with {', '.join(REQUIRED_JOBS)}"


def decide(runs: list[dict], jobs_of) -> tuple[str, str]:
    """`pass`, `red`, `wait`, `missing` or `unusable` over the matching runs.

    `jobs_of(run)` gives the jobs of the run's latest attempt. One red run is enough to stop;
    a success counts only when no other run of the commit is still going.
    """
    if not runs:
        return "missing", f"no run of {WORKFLOW} from a push to {BRANCH} on this commit yet"
    verdicts = [run_verdict(r, jobs_of(r)) for r in runs]
    for v, why in verdicts:
        if v == "red":
            return "red", why
    for v, why in verdicts:
        if v == "going":
            return "wait", why
    for v, why in verdicts:
        if v == "success":
            return "pass", why
    return "unusable", "; ".join(why for _, why in verdicts)


# ------------------------------------------------------------------ GitHub

def make_get(token: str | None, timeout: float):
    def get(path: str, query: dict | None = None):
        url = API + path
        if query:
            url += "?" + urllib.parse.urlencode(query)
        req = urllib.request.Request(url, method="GET")
        req.add_header("Accept", "application/vnd.github+json")
        req.add_header("X-GitHub-Api-Version", "2022-11-28")
        req.add_header("User-Agent", "xirius-release-ci-gate")
        if token:
            req.add_header("Authorization", f"Bearer {token}")
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as e:
            raise ApiError(f"GET {path}: {e}") from e

    return get


def workflow_id(get, repo: str) -> int:
    w = get(f"/repos/{repo}/actions/workflows/{WORKFLOW.rsplit('/', 1)[1]}")
    if w.get("path") != WORKFLOW:
        raise SystemExit(f"release-ci-gate: the workflow is {w.get('path')!r}, not {WORKFLOW}")
    if w.get("state") != "active":
        raise SystemExit(f"release-ci-gate: {WORKFLOW} is {w.get('state')}, not active")
    return int(w["id"])


def look(get, repo: str, wid: int, sha: str) -> tuple[str, str]:
    answer = get(
        f"/repos/{repo}/actions/workflows/{wid}/runs",
        {"head_sha": sha, "event": "push", "branch": BRANCH, "per_page": 100},
    )
    runs = matching_runs(answer.get("workflow_runs") or [], sha, wid, repo)

    def jobs_of(run: dict) -> list[dict]:
        a = get(f"/repos/{repo}/actions/runs/{run['id']}/jobs", {"filter": "latest", "per_page": 100})
        return a.get("jobs") or []

    return decide(runs, jobs_of)


# ------------------------------------------------------------------ the loop

def gate(look_once, wait_s: float, grace_s: float, interval_s: float, api_tries: int,
         clock=time.monotonic, sleep=time.sleep, say=print) -> tuple[bool, str]:
    """Asks until there is an answer; (passed, why)."""
    start = clock()
    failures = 0
    while True:
        try:
            state, why = look_once()
            failures = 0
        except ApiError as e:
            failures += 1
            if failures >= api_tries:
                return False, f"the API failed {failures} times in a row: {e}"
            say(f"release-ci-gate: {e} (try {failures} of {api_tries})")
            sleep(interval_s)
            continue
        elapsed = clock() - start
        if state == "pass":
            return True, why
        if state in ("red", "unusable"):
            return False, why
        if state == "missing" and elapsed >= grace_s:
            return False, f"{why} after {grace_s / 60:.0f} minutes"
        if state == "wait" and elapsed >= wait_s:
            return False, f"{why} after {wait_s / 60:.0f} minutes"
        say(f"release-ci-gate: {why}; asking again in {interval_s:.0f} s")
        sleep(interval_s)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--repo", required=True, help="owner/name")
    p.add_argument("--sha", required=True, help="the commit the tag points to")
    p.add_argument("--tag", default="", help="only for the messages")
    p.add_argument("--wait-minutes", type=float, default=40)
    p.add_argument("--missing-grace-minutes", type=float, default=5)
    p.add_argument("--interval-seconds", type=float, default=30)
    p.add_argument("--request-timeout-seconds", type=float, default=20)
    p.add_argument("--api-tries", type=int, default=5)
    args = p.parse_args(argv)
    if len(args.sha) != 40 or any(c not in "0123456789abcdef" for c in args.sha):
        p.error(f"--sha is not a full commit id: {args.sha!r}")

    get = make_get(os.environ.get("GITHUB_TOKEN"), args.request_timeout_seconds)
    try:
        wid = workflow_id(get, args.repo)
    except ApiError as e:
        print(f"::error::release-ci-gate: {e}", file=sys.stderr)
        return 1
    ok, why = gate(
        lambda: look(get, args.repo, wid, args.sha),
        args.wait_minutes * 60, args.missing_grace_minutes * 60,
        args.interval_seconds, args.api_tries,
    )
    label = f"{args.tag} ({args.sha[:12]})" if args.tag else args.sha[:12]
    if not ok:
        print(f"::error::main の CI が {label} を通していません: {why}（docs/release-ci-gate.md）",
              file=sys.stderr)
        return 1
    print(f"release-ci-gate: {label}: {why}")
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as f:
            f.write(f"sha={args.sha}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
