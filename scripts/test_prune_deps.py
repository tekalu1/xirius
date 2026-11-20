"""Destructive cleanup boundaries and retention rules, using disposable fixtures only."""
from contextlib import redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import prune_deps as cleanup


class PruneTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.repo = Path(self.folder.name) / "checkout"
        self.deps = self.repo / "target/debug/deps"
        self.deps.mkdir(parents=True)
        self.detect_running = cleanup.running_artifacts
        self.busy = patch.object(cleanup, "running_artifacts", return_value=[])
        self.busy.start()
        self.addCleanup(self.busy.stop)
        output = redirect_stdout(io.StringIO())
        output.__enter__()
        self.addCleanup(output.__exit__, None, None, None)

    def artifact(self, stem="unit", generation=1, extension=".rlib", timestamp=1):
        path = self.deps / f"{stem}-{generation:016x}{extension}"
        path.write_bytes(b"fixture")
        os.utime(path, ns=(timestamp * 1_000_000_000,) * 2)
        return path

    def test_preview_never_removes_candidates(self):
        old = self.artifact()
        new = self.artifact(generation=2, timestamp=2)
        cleanup.prune(self.repo)
        self.assertTrue(old.exists())
        self.assertTrue(new.exists())

    def test_apply_keeps_latest_and_does_not_cross_names_or_extensions(self):
        old = self.artifact()
        new = self.artifact(generation=2, timestamp=2)
        other = self.artifact(stem="another")
        pdb = self.artifact(extension=".pdb")
        self.assertEqual(cleanup.prune(self.repo, True), 1)
        self.assertFalse(old.exists())
        self.assertTrue(all(path.exists() for path in (new, other, pdb)))

    def test_equal_newest_timestamps_are_all_retained(self):
        old = self.artifact()
        tied = [self.artifact(generation=n, timestamp=2) for n in (2, 3)]
        cleanup.prune(self.repo, True)
        self.assertFalse(old.exists())
        self.assertTrue(all(path.exists() for path in tied))

    def test_unix_executables_without_extensions_are_grouped(self):
        old = self.artifact(extension="")
        new = self.artifact(generation=2, extension="", timestamp=2)
        cleanup.prune(self.repo, True)
        self.assertFalse(old.exists())
        self.assertTrue(new.exists())

    def test_import_libraries_are_grouped_but_object_fragments_are_left(self):
        old = self.artifact(extension=".dll.lib")
        new = self.artifact(generation=2, extension=".dll.lib", timestamp=2)
        fragment = self.artifact(extension=".fragment.rcgu.o")
        self.artifact(generation=2, extension=".fragment.rcgu.o", timestamp=2)
        cleanup.prune(self.repo, True)
        self.assertFalse(old.exists())
        self.assertTrue(new.exists())
        self.assertTrue(fragment.exists())

    def test_unhashed_files_and_directories_and_neighbor_checkout_survive(self):
        plain = self.deps / "unit.rcgu.o"
        plain.write_bytes(b"keep")
        directory = self.deps / "unit-0000000000000001.rlib"
        directory.mkdir()
        (directory / "child").write_bytes(b"keep")
        self.artifact(generation=2, timestamp=2)
        neighbor = self.repo.parent / "neighbor/target/debug/deps"
        neighbor.mkdir(parents=True)
        sentinel = neighbor / "unit-0000000000000001.rlib"
        sentinel.write_bytes(b"keep")
        cleanup.prune(self.repo, True)
        self.assertTrue(plain.exists())
        self.assertTrue((directory / "child").exists())
        self.assertTrue(sentinel.exists())

    def test_changed_candidate_is_not_removed(self):
        old = self.artifact()
        self.artifact(generation=2, timestamp=2)
        candidates = cleanup.candidates(self.deps)
        old.write_bytes(b"changed after discovery")
        with patch.object(cleanup, "candidates", return_value=candidates):
            with self.assertRaisesRegex(RuntimeError, "changed"):
                cleanup.prune(self.repo, True)
        self.assertTrue(old.exists())

    def test_running_program_prevents_removal(self):
        old = self.artifact()
        self.artifact(generation=2, timestamp=2)
        with patch.object(cleanup, "running_artifacts", return_value=[123]):
            with self.assertRaisesRegex(RuntimeError, "in use"):
                cleanup.prune(self.repo, True)
        self.assertTrue(old.exists())

    def test_external_target_is_rejected_before_pruning(self):
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": str(self.repo.parent / "outside")}):
            with patch.object(sys, "argv", ["prune_deps.py", "--apply"]):
                with patch.object(cleanup, "prune") as prune, redirect_stderr(io.StringIO()):
                    self.assertEqual(cleanup.main(), 1)
        prune.assert_not_called()

    def test_process_detection_only_matches_this_build_directory(self):
        debug = self.deps.parent / "音"
        owned = str(debug / "deps/test.exe")
        neighbor = str(debug.with_name("neighbor") / "deps/test.exe")
        pid = os.getpid() + 100000
        if os.name == "nt":
            output = json.dumps([
                {"ProcessId": pid, "ExecutablePath": owned, "CommandLine": None},
                {"ProcessId": pid + 1, "ExecutablePath": neighbor, "CommandLine": None},
                {"ProcessId": os.getpid(), "ExecutablePath": None, "CommandLine": owned},
            ], ensure_ascii=False)
        else:
            output = f"{pid} {owned}\n{pid + 1} {neighbor}\n{os.getpid()} {owned}\n"
        with patch.object(cleanup.subprocess, "check_output", return_value=output):
            self.assertEqual(self.detect_running(debug), [pid])

    def test_lock_excludes_a_second_process(self):
        (self.deps.parent / ".cargo-lock").write_bytes(b"existing lock file")
        script = "from pathlib import Path\nimport sys\nimport prune_deps\ntry:\n    with prune_deps.build_lock(Path(sys.argv[1])): pass\nexcept RuntimeError:\n    sys.exit(0)\nsys.exit(1)"
        env = os.environ.copy()
        env["PYTHONPATH"] = str(Path(__file__).resolve().parent)
        with cleanup.build_lock(self.deps.parent):
            result = subprocess.run([sys.executable, "-c", script, str(self.deps.parent)],
                                    env=env, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_reparse_points_are_rejected(self):
        class Info:
            st_mode = 0o040755
            st_file_attributes = 0x400
        with patch.object(Path, "lstat", return_value=Info()):
            with self.assertRaisesRegex(ValueError, "reparse"):
                cleanup.ordinary(self.deps, directory=True)

    def test_target_link_cannot_reach_another_directory(self):
        self.deps.rmdir()
        self.deps.parent.rmdir()
        (self.repo / "target").rmdir()
        outside = self.repo.parent / "outside"
        (outside / "debug/deps").mkdir(parents=True)
        sentinel = outside / "debug/deps/keep"
        sentinel.write_bytes(b"keep")
        try:
            if os.name == "nt":
                import _winapi
                _winapi.CreateJunction(str(outside), str(self.repo / "target"))
            else:
                (self.repo / "target").symlink_to(outside, target_is_directory=True)
        except OSError as error:
            self.skipTest(f"Directory links unavailable: {error}")
        with self.assertRaisesRegex(ValueError, "link|reparse"):
            cleanup.prune(self.repo, True)
        self.assertTrue(sentinel.exists())


if __name__ == "__main__":
    unittest.main()
