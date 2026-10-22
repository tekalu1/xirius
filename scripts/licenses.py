#!/usr/bin/env python3
"""Xirius licenses (LIC-007): THIRD-PARTY-LICENSES.txt, the Rust libraries in the app and their
licence texts, made by cargo-about from about.toml and about.hbs, then the code of others that
is copied into the repository and built into the app (LIC-021): cargo-about lists crates only.

  python scripts/licenses.py           write THIRD-PARTY-LICENSES.txt
  python scripts/licenses.py --check   exit 1 when the file is not what the lock file gives,
                                      or a folder under vendor/ or third_party/ is in no list

The only network it uses is cargo's own fetch of the crates (cargo fetch --locked); cargo-about
runs --frozen, so it reads the licence files of the fetched crates and asks nothing else.
Needs cargo-about 0.9 (cargo install cargo-about --locked --features cli).
Standard library only (Python 3.11+).
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD-PARTY-LICENSES.txt"

# Code of others copied into the repository and built into the app (LIC-021): its folder, the
# name and version the app lists, its licence (SPDX) and the file of its licence text. The
# version is "-" when the copy has none of its own.
VENDORED = [
    ("crates/xirius-stretch/vendor/signalsmith-stretch", "signalsmith-stretch", "1.3.1", "MIT", "LICENSE.txt"),
    ("crates/xirius-stretch/vendor/signalsmith-linear", "signalsmith-linear", "-", "MIT", "LICENSE.txt"),
    # Bitwig's example project (bitwig-5.0-readme.xml) and the reference files; the shots of the
    # DAWproject screen read them, so they are built into the app.
    ("crates/xirius-core/src/dawproject/testdata/reference", "dawproject", "-", "MIT", "LICENSE"),
]

# Folders under vendor/ or third_party/ that the app is not built with, and why.
NOT_SHIPPED = {
    "third_party/vst3sdk": "the VST3 SDK, to build the test plugins; the app uses the vst3 crate",
}

# The first words of a licence text, so a copy is not listed under another licence.
LICENSE_HEADS = {"MIT": "MIT License"}

VENDOR_DIRS = ("vendor", "third_party")


def generate() -> str:
    subprocess.run(["cargo", "fetch", "--locked"], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "licenses.txt"
        subprocess.run(
            [
                "cargo", "about", "generate", "--workspace", "--frozen", "--fail",
                "-c", str(ROOT / "about.toml"), "-o", str(out), str(ROOT / "about.hbs"),
            ],
            cwd=ROOT,
            check=True,
        )
        text = out.read_text(encoding="utf-8")
    return tidy(text + vendored_text())


def shipped_folders(paths: list[str]) -> set[str]:
    """What is right under a vendor/ or third_party/ folder, of the repository's `paths` (files
    and submodules): its folders and submodules, and its files but a note about them (*.md)."""
    folders = set()
    for path in paths:
        parts = path.split("/")
        for i, part in enumerate(parts[:-1]):
            if part in VENDOR_DIRS:
                entry = parts[: i + 2]
                if len(parts) > i + 2 or not entry[-1].lower().endswith(".md"):
                    folders.add("/".join(entry))
                break
    return folders


def unlisted(paths: list[str], vendored=VENDORED, not_shipped=NOT_SHIPPED) -> list[str]:
    """The folders under vendor/ or third_party/ that are in neither list."""
    known = {v[0] for v in vendored} | set(not_shipped)
    return sorted(shipped_folders(paths) - known)


def tracked(root: Path = ROOT) -> list[str]:
    """The files and submodules of the repository."""
    out = subprocess.run(["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True)
    return [p for p in out.stdout.decode("utf-8").split("\0") if p]


def vendored_text(root: Path = ROOT, vendored=VENDORED) -> str:
    """The section of THIRD-PARTY-LICENSES.txt for the copied code: a line each, then the texts."""
    missing = unlisted(tracked(root), vendored)
    if missing:
        raise SystemExit(
            "licenses: add these to VENDORED or NOT_SHIPPED in scripts/licenses.py: " + ", ".join(missing)
        )
    lines = ["", "Xirius に組み込まれた、Rust のクレートでないコード（ソースを写して一緒にビルドするもの）と、そのライセンス", ""]
    texts = []
    for folder, name, version, license, file in vendored:
        text = (root / folder / file).read_text(encoding="utf-8")
        head = LICENSE_HEADS.get(license)
        if head is None or not text.lstrip().startswith(head):
            raise SystemExit(f"licenses: {folder}/{file} is not the text of {license}")
        lines.append(f"vendor {name} {version} {license}")
        texts += ["", f"==== {license}", f"used {name} {version}", "", text]
    return "\n" + "\n".join(lines + texts) + "\n"


def tidy(text: str) -> str:
    """One kind of line end, no spaces at the ends of lines, no run of blank lines."""
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    out: list[str] = []
    for line in lines:
        if line == "" and out and out[-1] == "":
            continue
        out.append(line)
    text = "\n".join(out).strip("\n") + "\n"
    # A path of this PC must not ship.
    if re.search(r"[A-Za-z]:\\Users|\.cargo[\\/]registry", text):
        raise SystemExit("licenses: the text holds a local path")
    return text


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="compare, do not write")
    args = parser.parse_args()
    text = generate()
    if args.check:
        now = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if now != text:
            print(f"licenses: {OUT.name} is out of date; run python scripts/licenses.py", file=sys.stderr)
            return 1
        print(f"licenses: {OUT.name} is up to date")
        return 0
    OUT.write_bytes(text.encode("utf-8"))
    crates = sum(1 for line in text.splitlines() if line.startswith("crate "))
    print(f"licenses: wrote {OUT.name} ({crates} crates)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
