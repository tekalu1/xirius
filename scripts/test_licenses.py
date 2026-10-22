"""Tests of scripts/licenses.py: python scripts/test_licenses.py

They do not run cargo-about: they check that the code copied under vendor/ and third_party/ is
listed with its licence text (LIC-021).
"""

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("licenses", HERE / "licenses.py")
lic = importlib.util.module_from_spec(spec)
sys.modules["licenses"] = lic
spec.loader.exec_module(lic)

MIT = "MIT License\n\nCopyright (c) 2020 Someone\n\nPermission is hereby granted.\n"


class VendoredTests(unittest.TestCase):
    def test_every_folder_under_vendor_or_third_party_is_in_a_list(self):
        paths = [
            "crates/a/vendor/README.md",
            "crates/a/vendor/lib-one/LICENSE.txt",
            "crates/a/vendor/lib-one/one.h",
            "crates/a/vendor/loose.h",
            "crates/b/src/vendor.rs",
            "third_party/sdk",
            "src/main.rs",
        ]
        self.assertEqual(
            lic.shipped_folders(paths),
            {"crates/a/vendor/lib-one", "crates/a/vendor/loose.h", "third_party/sdk"},
        )
        vendored = [("crates/a/vendor/lib-one", "lib-one", "1.0", "MIT", "LICENSE.txt")]
        self.assertEqual(
            lic.unlisted(paths, vendored, {"third_party/sdk": "not built in"}),
            ["crates/a/vendor/loose.h"],
        )
        self.assertEqual(
            lic.unlisted(paths, vendored, {}), ["crates/a/vendor/loose.h", "third_party/sdk"]
        )

    def test_the_repositorys_copies_are_all_listed(self):
        self.assertEqual(lic.unlisted(lic.tracked()), [])
        for folder, *_ in lic.VENDORED:
            self.assertTrue((lic.ROOT / folder).is_dir(), folder)

    def test_the_section_lists_each_copy_and_its_whole_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            folder = root / "c" / "vendor" / "lib-one"
            folder.mkdir(parents=True)
            (folder / "LICENSE.txt").write_text(MIT, encoding="utf-8")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            vendored = [("c/vendor/lib-one", "lib-one", "1.0", "MIT", "LICENSE.txt")]
            text = lic.vendored_text(root, vendored)
            self.assertIn("\nvendor lib-one 1.0 MIT\n", text)
            self.assertIn("\n==== MIT\nused lib-one 1.0\n\n" + MIT, text)
            # A copy in no list stops it.
            other = root / "c" / "vendor" / "lib-two"
            other.mkdir()
            (other / "two.h").write_text("int two;\n", encoding="utf-8")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            with self.assertRaises(SystemExit) as e:
                lic.vendored_text(root, vendored)
            self.assertIn("c/vendor/lib-two", str(e.exception))
            # So does a text that is not the licence it is listed under.
            subprocess.run(["git", "rm", "-q", "-r", "--cached", "c/vendor/lib-two"], cwd=root, check=True)
            (folder / "LICENSE.txt").write_text("Apache License\n", encoding="utf-8")
            with self.assertRaises(SystemExit):
                lic.vendored_text(root, vendored)

    def test_the_shipped_file_has_the_section(self):
        text = (lic.ROOT / "THIRD-PARTY-LICENSES.txt").read_text(encoding="utf-8")
        for folder, name, version, license, file in lic.VENDORED:
            self.assertTrue(f"\nvendor {name} {version} {license}\n" in text, name)
            whole = lic.tidy((lic.ROOT / folder / file).read_text(encoding="utf-8")).strip()
            self.assertTrue(f"\nused {name} {version}\n\n{whole}" in text, name)


if __name__ == "__main__":
    unittest.main()
