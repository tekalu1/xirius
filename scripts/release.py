#!/usr/bin/env python3
"""Xirius release (LIC-019, LIC-020): the Windows files of one release, made on this PC.

  python scripts/release.py                    build, then write target/dist/
  python scripts/release.py --tag v1.0.0       the same, after checking the tag is the version
  python scripts/release.py --sign-key <file>  the same, and sign the manifest with that key
  python scripts/release.py --min-version 1.0.1
                                               the oldest Xirius that may install it
  python scripts/release.py --tag v1.0.0 --check-tag
                                               only check the tag (exit 1 when it differs)

The version is the one of [workspace.package] in Cargo.toml; a release's tag is `v<version>`
(the update check reads the tag of the latest release, crates/xirius-session/src/update.rs).
`--sign-key` is the private key `xirius-update-sign keygen` makes (UI-282); the manifest is
signed by `scripts/update_sign.py` (Python's standard library only), and
`Xirius-<version>-win-x64-update.json.sig` is written beside it. `--min-version` is the oldest
Xirius that may install the version (the manifest's `min_version`; default `1.0.0`).

What it writes to target/dist/ (--out):
  Xirius-<version>-win-x64-setup.exe   the installer (NSIS, for the current user)
  Xirius-<version>-win-x64-update.json the manifest: version, installer, SHA-256, channel
  Xirius-<version>-win-x64-update.json.sig
                                       its ed25519 signature, with --sign-key
  Xirius-<version>-win-x64.zip         the same files in a folder, without an installer
  lame-3.100.tar.gz                    the source of lame_enc.dll (LGPL), as downloaded
  SHA256SUMS.txt                       `<sha256>  <name>` of the setup, the zip and the tarball

Needs: the Rust toolchain, Visual Studio's C++ tools (scripts/build-lame.ps1 builds the
DLL when target/lame/x64 has none) and NSIS 3 (--makensis, the MAKENSIS variable, or
makensis on PATH). Standard library only (Python 3.11+).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
import zipfile
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ARCH = "x64"
LAME_VERSION = "3.100"

# The programs of the app, from `cargo build --release`. xirius-ui starts xirius-engine, which
# starts xirius-plugin-host; xirius and the session look for xirius-plugin-scan beside themselves.
PROGRAMS = ["xirius-ui", "xirius", "xirius-engine", "xirius-plugin-host", "xirius-plugin-scan"]
PACKAGES = ["xirius-ui", "xirius-cli", "xirius-engine", "xirius-plugins"]

# Files of the repository put at the top of the folder.
TOP = ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "THIRD-PARTY-LICENSES.txt"]

# Licence texts put in licenses/ (name there, path in the repository): what is in the app and
# is not a Rust library of THIRD-PARTY-LICENSES.txt.
LICENSES = [
    ("OFL-Inter.txt", "assets/fonts/OFL-Inter.txt"),
    ("OFL-NotoSansJP.txt", "assets/fonts/OFL-NotoSansJP.txt"),
    ("octicons-LICENSE.txt", "crates/xirius-ui/assets/octicons/LICENSE"),
    ("signalsmith-stretch-LICENSE.txt", "crates/xirius-stretch/vendor/signalsmith-stretch/LICENSE.txt"),
    ("signalsmith-linear-LICENSE.txt", "crates/xirius-stretch/vendor/signalsmith-linear/LICENSE.txt"),
]

SEMVER = re.compile(r"^(\d+)\.(\d+)\.(\d+)(-[0-9A-Za-z.-]+)?$")


def workspace_version(cargo_toml: Path = ROOT / "Cargo.toml") -> str:
    with open(cargo_toml, "rb") as f:
        version = tomllib.load(f)["workspace"]["package"]["version"]
    if not SEMVER.match(version):
        raise SystemExit(f"release: the version in Cargo.toml is not x.y.z: {version!r}")
    return version


def check_tag(tag: str, version: str) -> str | None:
    """Why `tag` is not the tag of `version`, or None."""
    if tag.startswith("refs/tags/"):
        tag = tag[len("refs/tags/"):]
    if tag != f"v{version}":
        return f"the tag {tag!r} is not v{version}, the version in Cargo.toml"
    return None


def version4(version: str) -> str:
    """The four numbers Windows keeps in a file's version: 1.2.3-beta.1 -> 1.2.3.0."""
    m = SEMVER.match(version)
    if not m:
        raise ValueError(version)
    return f"{m[1]}.{m[2]}.{m[3]}.0"


def base_name(version: str) -> str:
    return f"Xirius-{version}-win-{ARCH}"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def lame_sha256(script: Path = ROOT / "scripts" / "build-lame.ps1") -> str:
    m = re.search(r"\$Sha256 = '([0-9a-f]{64})'", script.read_text(encoding="utf-8"))
    if not m:
        raise SystemExit("release: no $Sha256 in scripts/build-lame.ps1")
    return m[1]


def run(cmd: list[str], **kw) -> None:
    print("release: " + " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True, **kw)


def build() -> Path:
    cmd = ["cargo", "build", "--release", "--locked"]
    for p in PACKAGES:
        cmd += ["-p", p]
    for b in PROGRAMS:
        cmd += ["--bin", b]
    run(cmd, cwd=ROOT)
    return ROOT / "target" / "release"


def lame(work: Path) -> Path:
    """The folder with lame_enc.dll, LAME-COPYING.txt and LAME-SOURCE.txt, built when missing."""
    out = work / ARCH
    if not (out / "lame_enc.dll").is_file():
        run(
            [
                "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
                "-File", str(ROOT / "scripts" / "build-lame.ps1"),
                "-Arch", ARCH, "-Work", str(work), "-NoInstall",
            ],
            cwd=ROOT,
            # Windows PowerShell 5.1 started from PowerShell 7 (as on the CI runner) inherits
            # PSModulePath and then cannot load its own modules (Get-FileHash is missing).
            env={k: v for k, v in os.environ.items() if k.upper() != "PSMODULEPATH"},
        )
    for name in ("lame_enc.dll", "LAME-COPYING.txt", "LAME-SOURCE.txt"):
        if not (out / name).is_file():
            raise SystemExit(f"release: {out / name} is missing")
    return out


def stage(folder: Path, programs: Path, lame_dir: Path, tarball_name: str) -> list[str]:
    """Puts the files of the release in `folder` (which must not exist); returns their paths."""
    folder.mkdir(parents=True)
    (folder / "licenses").mkdir()
    for name in PROGRAMS:
        shutil.copy2(programs / f"{name}.exe", folder / f"{name}.exe")
    shutil.copy2(lame_dir / "lame_enc.dll", folder / "lame_enc.dll")
    for name in TOP:
        shutil.copyfile(ROOT / name, folder / name)
    for name, path in LICENSES:
        shutil.copyfile(ROOT / path, folder / "licenses" / name)
    shutil.copyfile(lame_dir / "LAME-COPYING.txt", folder / "licenses" / "LAME-COPYING.txt")
    source = (lame_dir / "LAME-SOURCE.txt").read_text(encoding="ascii").rstrip("\r\n").splitlines()
    source.append(f"The same tarball is attached to every release of Xirius as {tarball_name}.")
    (folder / "licenses" / "LAME-SOURCE.txt").write_bytes(("\r\n".join(source) + "\r\n").encode("ascii"))
    files = files_of(folder)
    # The installer keeps the time of each file: the time of HEAD, as the zip.
    when = commit_seconds()
    for f in files:
        os.utime(folder / f, (when, when))
    return files


def files_of(folder: Path) -> list[str]:
    return sorted(p.relative_to(folder).as_posix() for p in folder.rglob("*") if p.is_file())


def nsis_quote(text: str) -> str:
    if '"' in text or "\n" in text:
        raise ValueError(f"cannot be put in the installer: {text!r}")
    return text.replace("$", "$$")


def nsis_lists(files: list[str]) -> tuple[str, str]:
    """The File lines the installer includes and the Delete lines its uninstaller does."""
    def win(path: str) -> str:
        return nsis_quote(path.replace("/", "\\"))

    install, uninstall = [], []
    folders: dict[str, list[str]] = {}
    for f in files:
        folders.setdefault(f.rpartition("/")[0], []).append(f)
    for folder in sorted(folders):
        install.append(f'SetOutPath "$INSTDIR\\{win(folder)}"' if folder else 'SetOutPath "$INSTDIR"')
        for f in folders[folder]:
            install.append(f'File "${{STAGE}}\\{win(f)}"')
            # A write that fails (a scanner's lock, a full disk) stops at once; the section
            # puts Xirius back and leaves the note (UI-282).
            install.append("IfErrors update_failed")
            uninstall.append(f'Delete "$INSTDIR\\{win(f)}"')
    install.append('SetOutPath "$INSTDIR"')
    # A folder cannot be removed while it is the uninstaller's working folder.
    uninstall.append('SetOutPath "$TEMP"')
    for folder in sorted((f for f in folders if f), key=lambda f: -f.count("/")):
        uninstall.append(f'RMDir "$INSTDIR\\{win(folder)}"')
    return "\n".join(install) + "\n", "\n".join(uninstall) + "\n"


def commit_seconds() -> int:
    """The time of HEAD, the time every file of the release is given (the same files, the same zip)."""
    return int(
        subprocess.run(
            ["git", "-C", str(ROOT), "log", "-1", "--format=%ct"], capture_output=True, text=True, check=True
        ).stdout.strip()
    )


def zip_time(seconds: int) -> tuple[int, int, int, int, int, int]:
    t = datetime.fromtimestamp(seconds, timezone.utc)
    return (max(t.year, 1980), t.month, t.day, t.hour, t.minute, t.second)


def write_zip(zip_path: Path, folder: Path, files: list[str], when) -> None:
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for f in files:
            info = zipfile.ZipInfo(f"{folder.name}/{f}", date_time=when)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            info.create_system = 0
            z.writestr(info, (folder / f).read_bytes())


def find_makensis(given: str | None) -> str:
    for c in (given, os.environ.get("MAKENSIS"), shutil.which("makensis")):
        if c and Path(c).is_file():
            return c
    raise SystemExit("release: NSIS not found: give --makensis <makensis.exe> or set MAKENSIS")


def write_sums(out: Path, names: list[str]) -> Path:
    sums = out / "SHA256SUMS.txt"
    sums.write_bytes("".join(f"{sha256(out / n)}  {n}\n" for n in sorted(names)).encode("ascii"))
    return sums


def manifest_name(version: str) -> str:
    return f"Xirius-{version}-win-x64-update.json"


def channel(version: str) -> str:
    """A version with a pre-release part is the beta channel (UI-282)."""
    m = SEMVER.match(version)
    return "beta" if m and m[4] else "stable"


def write_manifest(out: Path, version: str, setup_name: str, min_version: str) -> Path:
    """The manifest the app checks and the signature covers (UI-282)."""
    if not SEMVER.match(min_version):
        raise SystemExit(f"release: --min-version is not x.y.z: {min_version!r}")
    manifest = {
        "version": version,
        "file": setup_name,
        "sha256": sha256(out / setup_name),
        "channel": channel(version),
        "min_version": min_version,
    }
    path = out / manifest_name(version)
    # The exact bytes are what is signed; the app reads the JSON, so order does not matter.
    path.write_bytes(
        (json.dumps(manifest, separators=(",", ":"), sort_keys=True) + "\n").encode("utf-8")
    )
    return path


def sign(manifest: Path, key: Path, setup: Path, version: str) -> Path:
    """Signs `manifest` with `key` (UI-282), writing `<manifest>.sig` beside it.

    The signer is `scripts/update_sign.py` (Python's standard library only); the release job
    runs the same file and never a program built by the build job.
    """
    if not key.is_file():
        raise SystemExit(f"release: the key file is missing: {key}")
    signature = manifest.with_name(manifest.name + ".sig")
    run(
        [
            sys.executable, "-I", "-X", "utf8", str(ROOT / "scripts" / "update_sign.py"),
            "sign", "--key", str(key), "--manifest", str(manifest),
            "--out", str(signature), "--setup", str(setup), "--version", version,
        ],
        cwd=ROOT,
    )
    return signature


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--tag", help="the tag being released (v1.0.0 or refs/tags/v1.0.0)")
    parser.add_argument("--check-tag", action="store_true", help="only check --tag against the version")
    parser.add_argument("--out", type=Path, default=ROOT / "target" / "dist")
    parser.add_argument("--makensis", help="makensis.exe of NSIS 3")
    parser.add_argument("--sign-key", type=Path,
                        help="the private key of the update signature (xirius-update-sign keygen)")
    parser.add_argument("--min-version", default="1.0.0",
                        help="the oldest Xirius that may install this (the manifest's min_version)")
    parser.add_argument("--lame-work", type=Path, default=ROOT / "target" / "lame",
                        help="the -Work folder of build-lame.ps1 (the tarball and <arch>/)")
    parser.add_argument("--no-build", action="store_true", help="use target/release as it is")
    args = parser.parse_args(argv)

    version = workspace_version()
    if args.tag:
        why = check_tag(args.tag, version)
        if why:
            print(f"release: {why}", file=sys.stderr)
            return 1
        print(f"release: the tag is v{version}")
    if args.check_tag:
        if not args.tag:
            parser.error("--check-tag needs --tag")
        return 0
    if os.name != "nt":
        raise SystemExit("release: the Windows files are made on Windows")

    makensis = find_makensis(args.makensis)
    out = args.out.resolve()
    name = base_name(version)
    if out.exists() and any(out.iterdir()):
        raise SystemExit(f"release: {out} is not empty; remove it first")
    out.mkdir(parents=True, exist_ok=True)

    programs = ROOT / "target" / "release" if args.no_build else build()
    lame_dir = lame(args.lame_work)
    tarball = args.lame_work / f"lame-{LAME_VERSION}.tar.gz"
    if sha256(tarball) != lame_sha256():
        raise SystemExit(f"release: {tarball} is not the tarball build-lame.ps1 checks")

    work = out / "work"
    folder = work / name
    files = stage(folder, programs, lame_dir, tarball.name)
    install, uninstall = nsis_lists(files)
    (work / "install-files.nsh").write_text(install, encoding="utf-8", newline="\n")
    (work / "uninstall-files.nsh").write_text(uninstall, encoding="utf-8", newline="\n")

    zip_name = f"{name}.zip"
    write_zip(out / zip_name, folder, files, zip_time(commit_seconds()))
    setup_name = f"{name}-setup.exe"
    run(
        [
            makensis, "-V2", "-INPUTCHARSET", "UTF8",
            f"-DVERSION={version}", f"-DVERSION4={version4(version)}",
            f"-DSTAGE={folder}", f"-DLISTS={work}", f"-DOUTFILE={out / setup_name}",
            str(ROOT / "packaging" / "windows" / "xirius.nsi"),
        ],
        cwd=ROOT,
    )
    shutil.copyfile(tarball, out / tarball.name)
    sums = write_sums(out, [setup_name, zip_name, tarball.name])
    manifest = write_manifest(out, version, setup_name, args.min_version)
    signature = sign(manifest, args.sign_key.resolve(), out / setup_name, version) if args.sign_key else None

    print(f"release: {len(files)} files in {name}")
    for n in (setup_name, manifest.name, zip_name, tarball.name, sums.name):
        print(f"  {out / n}  {(out / n).stat().st_size:,} bytes")
    if signature:
        print(f"  {signature}  {(signature).stat().st_size:,} bytes")
    else:
        print("release: 署名なし（--sign-key を渡すと .sig を作ります）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
