"""Tests of scripts/release.py: python scripts/test_release.py

They do not build: they check the version and tag, the installer's file lists, the zip and the
checksums on small files.
"""

import importlib.util
import json
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("release", HERE / "release.py")
rel = importlib.util.module_from_spec(spec)
sys.modules["release"] = rel
spec.loader.exec_module(rel)


class VersionTests(unittest.TestCase):
    def test_the_tag_must_be_v_and_the_version(self):
        self.assertIsNone(rel.check_tag("v1.0.0", "1.0.0"))
        self.assertIsNone(rel.check_tag("refs/tags/v1.0.0", "1.0.0"))
        self.assertIsNotNone(rel.check_tag("1.0.0", "1.0.0"))
        self.assertIsNotNone(rel.check_tag("v1.0.1", "1.0.0"))
        self.assertIsNotNone(rel.check_tag("v1.0.0-rc.1", "1.0.0"))
        self.assertIsNone(rel.check_tag("v1.1.0-rc.1", "1.1.0-rc.1"))

    def test_the_workspace_version_is_read_and_must_be_semver(self):
        with tempfile.TemporaryDirectory() as tmp:
            toml = Path(tmp) / "Cargo.toml"
            toml.write_text('[workspace.package]\nversion = "1.2.3"\n', encoding="utf-8")
            self.assertEqual(rel.workspace_version(toml), "1.2.3")
            toml.write_text('[workspace.package]\nversion = "1.2"\n', encoding="utf-8")
            with self.assertRaises(SystemExit):
                rel.workspace_version(toml)
        self.assertRegex(rel.workspace_version(), rel.SEMVER)

    def test_windows_gets_four_numbers(self):
        self.assertEqual(rel.version4("1.2.3"), "1.2.3.0")
        self.assertEqual(rel.version4("1.2.3-beta.1"), "1.2.3.0")

    def test_the_names_of_the_files(self):
        self.assertEqual(rel.base_name("1.0.0"), "Xirius-1.0.0-win-x64")

    def test_check_tag_only_exits_by_the_tag(self):
        version = rel.workspace_version()
        self.assertEqual(rel.main(["--tag", f"v{version}", "--check-tag"]), 0)
        self.assertEqual(rel.main(["--tag", f"v{version}.9", "--check-tag"]), 1)

    def test_the_lame_checksum_is_the_one_build_lame_checks(self):
        self.assertEqual(rel.lame_sha256(), "ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e")


class ListTests(unittest.TestCase):
    def test_the_installer_installs_and_removes_the_same_files(self):
        install, uninstall = rel.nsis_lists(["a.exe", "licenses/X.txt", "licenses/sub/Y.txt", "NOTICE"])
        self.assertEqual(
            install.splitlines(),
            [
                'SetOutPath "$INSTDIR"',
                'File "${STAGE}\\a.exe"',
                'IfErrors update_failed',
                'File "${STAGE}\\NOTICE"',
                'IfErrors update_failed',
                'SetOutPath "$INSTDIR\\licenses"',
                'File "${STAGE}\\licenses\\X.txt"',
                'IfErrors update_failed',
                'SetOutPath "$INSTDIR\\licenses\\sub"',
                'File "${STAGE}\\licenses\\sub\\Y.txt"',
                'IfErrors update_failed',
                'SetOutPath "$INSTDIR"',
            ],
        )
        self.assertEqual(
            uninstall.splitlines(),
            [
                'Delete "$INSTDIR\\a.exe"',
                'Delete "$INSTDIR\\NOTICE"',
                'Delete "$INSTDIR\\licenses\\X.txt"',
                'Delete "$INSTDIR\\licenses\\sub\\Y.txt"',
                'SetOutPath "$TEMP"',
                'RMDir "$INSTDIR\\licenses\\sub"',
                'RMDir "$INSTDIR\\licenses"',
            ],
        )

    def test_names_are_escaped_or_refused(self):
        install, _ = rel.nsis_lists(["a$b.txt"])
        self.assertIn('File "${STAGE}\\a$$b.txt"', install)
        with self.assertRaises(ValueError):
            rel.nsis_lists(['a"b.txt'])


class ZipTests(unittest.TestCase):
    def test_the_zip_is_the_same_for_the_same_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            folder = base / "Xirius-1.0.0-win-x64"
            (folder / "licenses").mkdir(parents=True)
            (folder / "a.exe").write_bytes(b"MZ")
            (folder / "licenses" / "L.txt").write_bytes(b"text\n")
            files = rel.files_of(folder)
            self.assertEqual(files, ["a.exe", "licenses/L.txt"])
            when = (2026, 10, 15, 1, 2, 4)
            rel.write_zip(base / "one.zip", folder, files, when)
            rel.write_zip(base / "two.zip", folder, files, when)
            self.assertEqual((base / "one.zip").read_bytes(), (base / "two.zip").read_bytes())
            with zipfile.ZipFile(base / "one.zip") as z:
                self.assertEqual(z.namelist(), ["Xirius-1.0.0-win-x64/a.exe", "Xirius-1.0.0-win-x64/licenses/L.txt"])
                self.assertEqual(z.read("Xirius-1.0.0-win-x64/licenses/L.txt"), b"text\n")

    def test_the_zip_time_is_utc_and_not_before_1980(self):
        self.assertEqual(rel.zip_time(1_792_000_000), (2026, 10, 14, 17, 46, 40))
        self.assertEqual(rel.zip_time(0)[0], 1980)
        self.assertGreater(rel.commit_seconds(), 1_700_000_000)

    def test_sums_are_in_the_form_sha256sum_reads(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            (out / "b.zip").write_bytes(b"")
            (out / "a.exe").write_bytes(b"abc")
            sums = rel.write_sums(out, ["b.zip", "a.exe"]).read_text(encoding="ascii")
            self.assertEqual(
                sums,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  a.exe\n"
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  b.zip\n",
            )


class ManifestTests(unittest.TestCase):
    """UI-282: the manifest the app checks (version, file, SHA-256, channel, min_version)."""

    def test_the_manifest_names_the_version_the_file_the_hash_and_the_channel(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            for version, channel in [("1.0.1", "stable"), ("1.1.0-beta.2", "beta")]:
                name = f"Xirius-{version}-win-x64-setup.exe"
                (out / name).write_bytes(b"abc")
                path = rel.write_manifest(out, version, name, "1.0.0")
                self.assertEqual(path.name, rel.manifest_name(version))
                manifest = json.loads(path.read_text(encoding="utf-8"))
                self.assertEqual(
                    set(manifest),
                    {"version", "file", "sha256", "channel", "min_version"},
                )
                self.assertEqual(manifest["version"], version)
                self.assertEqual(manifest["file"], name)
                self.assertEqual(manifest["sha256"], rel.sha256(out / name))
                self.assertEqual(manifest["channel"], channel)
                self.assertEqual(manifest["min_version"], "1.0.0")
                self.assertEqual(rel.channel(version), channel)

    def test_the_minimum_version_must_be_semver(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            name = "Xirius-1.0.1-win-x64-setup.exe"
            (out / name).write_bytes(b"abc")
            with self.assertRaises(SystemExit):
                rel.write_manifest(out, "1.0.1", name, "latest")


if __name__ == "__main__":
    sys.exit(unittest.main())
