import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import dco  # noqa: E402


def git(cwd, *args, mail="dev@example.com"):
    subprocess.run(
        ["git", "-c", "user.name=Dev", "-c", f"user.email={mail}", *args],
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )


class JudgeTests(unittest.TestCase):
    def test_signed(self):
        msg = "x: y\n\nSigned-off-by: Dev <dev@example.com>\n"
        self.assertIsNone(dco.judge(msg, "dev@example.com", "dev@example.com"))

    def test_unsigned(self):
        self.assertIn("no Signed-off-by", dco.judge("x: y\n", "a@b.c", "a@b.c"))

    def test_trailer_among_others(self):
        msg = "x: y\n\nCo-Authored-By: C <c@d.e>\nSigned-off-by: Dev <DEV@example.com>\n"
        self.assertIsNone(dco.judge(msg, "dev@example.com", "bot@example.com"))

    def test_other_address(self):
        msg = "x: y\n\nSigned-off-by: Else <else@example.com>\n"
        self.assertIn("neither", dco.judge(msg, "dev@example.com", "dev@example.com"))

    def test_a_co_author_is_not_a_sign_off(self):
        msg = "x: y\n\nCo-Authored-By: Dev <dev@example.com>\n"
        self.assertIn("no Signed-off-by", dco.judge(msg, "dev@example.com", "dev@example.com"))

    def test_japanese_name(self):
        msg = "x: y\n\nSigned-off-by: 開発者 <dev@example.com>\n"
        self.assertIsNone(dco.judge(msg, "dev@example.com", "dev@example.com"))


class RangeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.cwd = self.tmp.name
        git(self.cwd, "init", "-q", "-b", "main")
        (Path(self.cwd) / "a").write_text("1", encoding="utf-8")
        git(self.cwd, "add", "a")
        git(self.cwd, "commit", "-q", "-m", "first, before the rule")

    def tearDown(self):
        self.tmp.cleanup()

    def add(self, name, *flags, message="change"):
        (Path(self.cwd) / name).write_text(name, encoding="utf-8")
        git(self.cwd, "add", name)
        git(self.cwd, "commit", "-q", *flags, "-m", message)

    def test_signed_commits_pass_and_an_unsigned_one_fails(self):
        self.add("b", "-s", message="signed")
        self.assertEqual(dco.check("HEAD~1..HEAD", self.cwd), [])
        self.add("c", message="日本語の件名")
        failures = dco.check("HEAD~2..HEAD", self.cwd)
        self.assertEqual([f[1] for f in failures], ["日本語の件名"])

    def test_commits_before_the_range_are_left_alone(self):
        self.add("b", "-s")
        self.assertEqual(dco.check("HEAD~1..HEAD", self.cwd), [])
        everything = dco.check("HEAD", self.cwd)
        self.assertEqual([f[1] for f in everything], ["first, before the rule"])

    def test_a_merge_commit_is_not_checked(self):
        git(self.cwd, "checkout", "-q", "-b", "topic")
        self.add("b", "-s")
        git(self.cwd, "checkout", "-q", "main")
        self.add("c", "-s")
        git(self.cwd, "merge", "-q", "--no-ff", "-m", "merge topic", "topic")
        self.assertEqual(dco.check("HEAD~2..HEAD", self.cwd), [])


if __name__ == "__main__":
    unittest.main()
