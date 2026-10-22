"""Tests of scripts/public-export.py: python scripts/test_public_export.py"""

import importlib.util
import io
import subprocess
import sys
import tempfile
import unittest
import zipfile
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("public_export", HERE / "public-export.py")
pe = importlib.util.module_from_spec(spec)
sys.modules["public_export"] = pe
spec.loader.exec_module(pe)

CONFIG = """
[[exclude]]
pattern = "temporary/"
reason = "internal"

[[exclude]]
pattern = "**/*.secret"
reason = "private"

[[forbid]]
name = "personal absolute path"
regex = '''(?i)\\b[A-Z]:[\\\\/]+(dev|users)[\\\\/]+\\S+'''
allow = ["docs/allowed.md"]

[[forbid]]
name = "email address"
regex = '''[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\\.[A-Za-z0-9-]+)*\\.[A-Za-z]{2,}'''
allow_match = '''@example\\.com$'''
allow = []

[expect]
files = ["Cargo.toml", "README.md"]
"""

SUBMODULE = "1111111111111111111111111111111111111111"


def git(repo, *args):
    return subprocess.run(
        ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@example.com", *args],
        capture_output=True,
        text=True,
        check=True,
    ).stdout


class GlobTests(unittest.TestCase):
    def test_globs(self):
        self.assertTrue(pe.glob_regex("temporary/").match("temporary/a/b.png"))
        self.assertTrue(pe.glob_regex("temporary/").match("temporary"))
        self.assertFalse(pe.glob_regex("temporary/").match("temporary-file.md"))
        self.assertTrue(pe.glob_regex("**/*.secret").match("a.secret"))
        self.assertTrue(pe.glob_regex("**/*.secret").match("x/y/a.secret"))
        self.assertFalse(pe.glob_regex("docs/*.md").match("docs/a/b.md"))
        self.assertTrue(pe.glob_regex("docs/?.md").match("docs/a.md"))


class ExportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.repo = base / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "core.autocrlf", "false")
        files = {
            "Cargo.toml": '[workspace.package]\nversion = "1.2.3"\n',
            "README.md": "see temporary/a.md\nmail dev@example.com\n",
            "docs/allowed.md": "a path like C:\\Users\\name is described here\n",
            "temporary/a.md": "D:/dev/private\n",
            "keys/a.secret": "x\n",
            "run.sh": "#!/bin/sh\n",
        }
        for path, text in files.items():
            p = self.repo / path
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(text.encode())
        git(self.repo, "add", "-A")
        git(self.repo, "update-index", "--chmod=+x", "run.sh")
        git(self.repo, "update-index", "--add", "--cacheinfo", f"160000,{SUBMODULE},third_party/sdk")
        git(self.repo, "commit", "-q", "-m", "one")
        self.config = base / "export.toml"
        self.config.write_text(CONFIG, encoding="utf-8")
        self.out = base / "out"

    def tearDown(self):
        self.tmp.cleanup()

    def run_export(self, *args):
        buf = io.StringIO()
        with redirect_stdout(buf):
            code = pe.main(["--repo", str(self.repo), "--config", str(self.config), *args])
        return code, buf.getvalue()

    def commit_file(self, path, data: bytes):
        (self.repo / path).parent.mkdir(parents=True, exist_ok=True)
        (self.repo / path).write_bytes(data)
        git(self.repo, "add", path)
        git(self.repo, "commit", "-q", "-m", "more")

    def test_list_drops_by_pattern_and_names_dangling_links(self):
        code, out = self.run_export("--list")
        self.assertEqual(code, 0, out)
        self.assertIn("drop  temporary/a.md  [temporary/]", out)
        self.assertIn("drop  keys/a.secret  [**/*.secret]", out)
        self.assertIn("carry README.md", out)
        self.assertIn("carry third_party/sdk", out)
        self.assertIn("README.md: 1", out)
        self.assertFalse(self.out.exists())

    def test_writes_the_carried_files_and_leaves_the_repository_alone(self):
        head = git(self.repo, "rev-parse", "HEAD")
        (self.repo / "README.md").write_text("changed, not committed\n", encoding="utf-8")
        status = git(self.repo, "status", "--porcelain")
        code, out = self.run_export("--out", str(self.out))
        self.assertEqual(code, 0, out)
        self.assertEqual((self.out / "README.md").read_bytes(), b"see temporary/a.md\nmail dev@example.com\n")
        self.assertFalse((self.out / "temporary").exists())
        self.assertFalse((self.out / "keys").exists())
        self.assertTrue((self.out / "third_party/sdk").is_dir())
        self.assertEqual(git(self.repo, "rev-parse", "HEAD"), head)
        self.assertEqual(git(self.repo, "status", "--porcelain"), status)

    def test_a_forbidden_match_stops_before_writing(self):
        self.commit_file("src/a.rs", b'const P: &str = "D:\\\\dev\\\\me\\\\x.wav";\n')
        code, out = self.run_export("--out", str(self.out))
        self.assertEqual(code, 1)
        self.assertIn("forbidden: src/a.rs:1: personal absolute path", out)
        self.assertFalse(self.out.exists())

    def test_an_email_outside_the_allowed_domains_is_forbidden(self):
        self.commit_file("b.txt", b"ask me@private.jp\n")
        code, out = self.run_export("--list")
        self.assertEqual(code, 1)
        self.assertIn("forbidden: b.txt:1: email address: me@private.jp", out)

    def test_zip_members_are_looked_through(self):
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr("project.xml", '<File path="D:\\dev\\me\\a.wav"/>')
        self.commit_file("t/song.dawproject", buf.getvalue())
        code, out = self.run_export("--list")
        self.assertEqual(code, 1)
        self.assertIn("t/song.dawproject!project.xml:1: personal absolute path", out)

    def test_the_private_rules_beside_the_config_are_added(self):
        self.commit_file("c.txt", b"made by someone\n")
        self.assertEqual(self.run_export("--list")[0], 0)
        private = self.config.with_name("export.private.toml")
        private.write_text('[[forbid]]\nname = "name"\nregex = "someone"\nallow = []\n', encoding="utf-8")
        code, out = self.run_export("--list")
        self.assertEqual(code, 1)
        self.assertIn("forbidden: c.txt:1: name: someone", out)

    def test_skipped_files_are_not_looked_through(self):
        self.commit_file("rules.txt", b"ask me@private.jp\n")
        with open(self.config, "a", encoding="utf-8") as f:
            f.write('\n[scan]\nskip = ["rules.txt"]\n')
        self.assertEqual(self.run_export("--list")[0], 0)

    def test_a_missing_expected_file_stops(self):
        git(self.repo, "rm", "-q", "README.md")
        git(self.repo, "commit", "-q", "-m", "drop readme")
        code, out = self.run_export("--list")
        self.assertEqual(code, 1)
        self.assertIn("missing: README.md", out)

    def test_refuses_a_folder_inside_the_repository_or_not_empty(self):
        with self.assertRaises(SystemExit):
            self.run_export("--out", str(self.repo / "export"))
        self.out.mkdir()
        (self.out / "x").write_text("x", encoding="utf-8")
        with self.assertRaises(SystemExit):
            self.run_export("--out", str(self.out))

    def test_commit_makes_one_signed_off_commit_with_the_submodule(self):
        code, out = self.run_export("--out", str(self.out), "--commit")
        self.assertEqual(code, 0, out)
        log = subprocess.run(
            ["git", "-C", str(self.out), "log", "--format=%an <%ae>%n%B"], capture_output=True, text=True, check=True
        ).stdout
        self.assertTrue(log.startswith("tekalu <82334772+tekalu1@users.noreply.github.com>\nXirius 1.2.3\n"), log)
        self.assertIn("Signed-off-by: tekalu <82334772+tekalu1@users.noreply.github.com>", log)
        count = subprocess.run(
            ["git", "-C", str(self.out), "rev-list", "--count", "HEAD"], capture_output=True, text=True, check=True
        ).stdout
        self.assertEqual(count.strip(), "1")
        tree = subprocess.run(
            ["git", "-C", str(self.out), "ls-tree", "-r", "HEAD"], capture_output=True, text=True, check=True
        ).stdout
        self.assertIn(f"160000 commit {SUBMODULE}\tthird_party/sdk", tree)
        self.assertIn("100755 blob", next(line for line in tree.splitlines() if line.endswith("run.sh")))
        self.assertNotIn("temporary", tree)


class RealRepositoryTests(unittest.TestCase):
    def test_this_repository_exports_clean(self):
        buf = io.StringIO()
        with redirect_stdout(buf):
            code = pe.main(["--list"])
        lines = [line for line in buf.getvalue().splitlines() if line.startswith(("forbidden:", "missing:"))]
        self.assertEqual(lines, [])
        self.assertEqual(code, 0)


if __name__ == "__main__":
    sys.exit(unittest.main())
