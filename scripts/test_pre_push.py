"""Tests of scripts/pre_push.py: python scripts/test_pre_push.py

They make throwaway repositories with the real git (a remote and a clone) in a temporary
folder, with a git config of their own; gitleaks is never run (a stand-in is passed).
"""

import contextlib
import importlib.util
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("pre_push", HERE / "pre_push.py")
pp = importlib.util.module_from_spec(spec)
sys.modules["pre_push"] = pp
spec.loader.exec_module(pp)


class Parsing(unittest.TestCase):
    def test_terms_skip_comments_and_blank_lines(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "terms")
            Path(p).write_text("﻿# names\n\n  Example Corp  \nhost-01\n", encoding="utf-8")
            self.assertEqual(pp.read_terms(p), [(3, "Example Corp"), (4, "host-01")])
            self.assertEqual(pp.read_terms(os.path.join(d, "none")), [])

    def test_deletions_are_not_looked_at(self):
        lines = (
            f"refs/heads/a {'1' * 40} refs/heads/a {pp.ZERO}\n"
            f"(delete) {pp.ZERO} refs/heads/b {'2' * 40}\n"
        )
        self.assertEqual(pp.updates(lines), [("refs/heads/a", "1" * 40, "refs/heads/a", pp.ZERO)])

    def test_the_range_leaves_out_what_a_remote_has(self):
        self.assertEqual(pp.rev_args("1" * 40, pp.ZERO, lambda _: True),
                         ["1" * 40, "--not", "--remotes"])
        self.assertEqual(pp.rev_args("1" * 40, "2" * 40, lambda _: True),
                         ["1" * 40, "--not", "2" * 40, "--remotes"])
        # A remote commit this clone has never fetched cannot be named to git log.
        self.assertEqual(pp.rev_args("1" * 40, "2" * 40, lambda _: False),
                         ["1" * 40, "--not", "--remotes"])

    def test_the_log_is_read_by_part(self):
        log = (
            f"{pp.MARK}commit {'a' * 40}\n"
            f"{pp.MARK}author Someone <someone@example.com>\n"
            f"{pp.MARK}committer Someone <someone@example.com>\n"
            "Add the mixer\n\nFor ACME.\n"
            f"{pp.MARK}end\n"
            "\n"
            "diff --git a/src/acme.rs b/src/acme.rs\n"
            "--- /dev/null\n"
            "+++ b/src/acme.rs\n"
            "@@ -0,0 +1 @@\n"
            "+// acme was here\n"
            "-// ACME removed\n"
        )
        hits = pp.scan_log(log, [(1, "acme"), (2, "someone@example.com")])
        where = sorted({(h.where, h.term) for h in hits})
        self.assertEqual(where, [
            (f"commit {'a' * 12} (author)", "someone@example.com"),
            (f"commit {'a' * 12} (committer)", "someone@example.com"),
            (f"commit {'a' * 12} (message)", "acme"),
            (f"commit {'a' * 12} (path src/acme.rs)", "acme"),
            (f"commit {'a' * 12} (src/acme.rs)", "acme"),
        ])


class Push(unittest.TestCase):
    """The hook over real repositories."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        config = base / "gitconfig"
        config.write_text("[init]\n\tdefaultBranch = main\n[core]\n\tautocrlf = false\n",
                          encoding="utf-8")
        self.env = {
            **os.environ,
            "GIT_CONFIG_GLOBAL": str(config),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_AUTHOR_NAME": "Dev",
            "GIT_AUTHOR_EMAIL": "dev@example.org",
            "GIT_COMMITTER_NAME": "Dev",
            "GIT_COMMITTER_EMAIL": "dev@example.org",
        }
        self.old_env = dict(os.environ)
        os.environ.clear()
        os.environ.update(self.env)
        self.remote = base / "remote.git"
        self.clone = base / "clone"
        self.git("init", "--bare", "-q", str(self.remote), cwd=base)
        self.git("clone", "-q", str(self.remote), str(self.clone), cwd=base)
        self.old_cwd = os.getcwd()
        os.chdir(self.clone)
        self.commit("README.md", "hello\n", "Start")
        self.git("push", "-q", "origin", "main")

    def tearDown(self):
        os.chdir(self.old_cwd)
        os.environ.clear()
        os.environ.update(self.old_env)
        self.tmp.cleanup()

    def git(self, *args, cwd=None):
        return subprocess.run(["git", *args], cwd=cwd or self.clone, env=self.env, check=True,
                              capture_output=True, encoding="utf-8").stdout.strip()

    def commit(self, name, text, message):
        (self.clone / name).write_text(text, encoding="utf-8")
        self.git("add", name)
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD")

    def terms(self, *terms):
        path = self.git("rev-parse", "--git-path", pp.TERMS)
        path = Path(path) if os.path.isabs(path) else self.clone / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("".join(t + "\n" for t in terms), encoding="utf-8")

    def push(self, ref="refs/heads/main", remote_sha=None, leaks=lambda *_: 0, which=None):
        local = self.git("rev-parse", ref)
        if remote_sha is None:
            remote_sha = self.git("rev-parse", "origin/main")
        line = f"{ref} {local} {ref} {remote_sha}\n"
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = pp.main(["origin", str(self.remote)], line,
                           which=which or (lambda _: None), leaks=leaks)
        return code, err.getvalue()

    def test_a_clean_push_goes_and_says_gitleaks_is_missing(self):
        self.terms("Example Corp")
        self.commit("a.txt", "nothing here\n", "Add a")
        code, err = self.push()
        self.assertEqual(code, 0, err)
        self.assertIn("gitleaks is not on PATH", err)

    def test_a_term_in_an_added_line_stops_it(self):
        self.terms("# a comment", "example corp")
        sha = self.commit("a.txt", "made for Example Corp\n", "Add a")
        code, err = self.push()
        self.assertEqual(code, 1)
        self.assertIn(f"commit {sha[:12]} (a.txt)", err)
        self.assertIn("line 2", err)

    def test_a_term_in_the_message_the_author_or_a_path_stops_it(self):
        self.terms("ticket-42")
        self.commit("a.txt", "x\n", "Fix ticket-42")
        self.assertEqual(self.push()[0], 1)
        self.git("reset", "-q", "--hard", "origin/main")
        self.terms("dev@example.org")
        self.commit("b.txt", "x\n", "Add b")
        code, err = self.push()
        self.assertEqual(code, 1)
        self.assertIn("(author)", err)
        self.git("reset", "-q", "--hard", "origin/main")
        self.terms("secret-plan")
        self.commit("secret-plan.txt", "x\n", "Add c")
        self.assertEqual(self.push()[0], 1)

    def test_what_the_remote_already_has_is_not_looked_at_again(self):
        self.commit("a.txt", "Example Corp\n", "Add a")
        self.git("push", "-q", "origin", "main")
        self.terms("Example Corp")
        self.commit("b.txt", "fine\n", "Add b")
        code, err = self.push()
        self.assertEqual(code, 0, err)

    def test_a_new_branch_is_looked_at_from_where_the_remotes_end(self):
        self.terms("Example Corp")
        self.git("switch", "-q", "-c", "topic")
        self.commit("a.txt", "Example Corp\n", "Add a")
        code, _ = self.push("refs/heads/topic", remote_sha=pp.ZERO)
        self.assertEqual(code, 1)

    def test_a_branch_name_stops_it(self):
        self.terms("example-corp")
        self.git("switch", "-q", "-c", "for-example-corp")
        code, err = self.push("refs/heads/for-example-corp", remote_sha=pp.ZERO)
        self.assertEqual(code, 1)
        self.assertIn("ref refs/heads/for-example-corp", err)

    def test_gitleaks_runs_over_the_new_commits_and_its_finding_stops_it(self):
        self.commit("a.txt", "x\n", "Add a")
        seen = []

        def leaks(exe, root, args):
            seen.append((exe, args))
            return 1

        code, err = self.push(leaks=leaks, which=lambda _: "gitleaks")
        self.assertEqual(code, 1)
        self.assertIn("gitleaks found a secret", err)
        self.assertEqual(seen[0][1][1:], ["--not", self.git("rev-parse", "origin/main"),
                                          "--remotes"])
        # Nothing new: gitleaks is not run.
        self.git("push", "-q", "origin", "main")
        seen.clear()
        self.assertEqual(self.push(leaks=leaks, which=lambda _: "gitleaks")[0], 0)
        self.assertEqual(seen, [])


if __name__ == "__main__":
    sys.exit(unittest.main())
