"""Preview old hashed artifacts in this checkout's target/debug/deps; --apply removes them.

Run only after builds, tests and applications using this checkout have stopped. Different
feature configurations can use different hashes: removing one can require a later rebuild.
No dependencies outside the standard library.
"""
import argparse
from collections import defaultdict
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

HASHED = re.compile(r"^(.+)-[0-9a-f]{16}(\.dll\.(?:lib|exp)|\.[^.]+)?$")


def ordinary(path, directory=False):
    info = path.lstat()
    if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
        raise ValueError(f"Refusing a link or reparse point: {path.name}")
    if not (stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode)):
        raise ValueError(f"Unexpected file type: {path.name}")
    return info


def deps_directory(repo):
    """Never accept a target outside the script's own checkout, or follow a directory link."""
    for path in (repo, repo / "target", repo / "target/debug", repo / "target/debug/deps"):
        ordinary(path, directory=True)
    return repo / "target/debug/deps"


def identity(info):
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns


def candidates(deps):
    groups = defaultdict(list)
    for path in deps.iterdir():
        match = HASHED.fullmatch(path.name)
        if not match:
            continue
        try:
            info = ordinary(path)
        except ValueError:
            continue
        groups[match.groups()].append((path, identity(info)))
    result = []
    for entries in groups.values():
        newest = max(info[3] for _, info in entries)
        result.extend((path, info) for path, info in entries if info[3] < newest)
    return sorted(result, key=lambda entry: (entry[1][3], entry[0].name))


@contextmanager
def build_lock(debug):
    """Hold the same directory lock as Cargo, without waiting for an active build."""
    path = debug / ".cargo-lock"
    if path.exists() or path.is_symlink():
        ordinary(path)
    with path.open("a+b") as handle:
        handle.seek(0)
        try:
            if os.name == "nt":
                import msvcrt
                msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            raise RuntimeError("Cargo is using this build directory; try after it finishes") from error
        try:
            yield
        finally:
            if os.name == "nt":
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(handle, fcntl.LOCK_UN)


def running_artifacts(debug):
    """Catch existing test/app processes too; callers must not start new ones meanwhile."""
    if os.name == "nt":
        command = ["powershell", "-NoProfile", "-Command",
                   "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); "
                   "Get-CimInstance Win32_Process | Select-Object ProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress"]
        output = subprocess.check_output(command, text=True, encoding="utf-8", errors="replace", timeout=30)
        records = json.loads(output or "[]")
        if isinstance(records, dict):
            records = [records]
        rows = [(row["ProcessId"], (row.get("ExecutablePath") or "") + " " +
                 (row.get("CommandLine") or "")) for row in records]
    else:
        output = subprocess.check_output(["ps", "-axo", "pid=,command="], text=True, timeout=30)
        rows = [(int(parts[0]), parts[1]) for line in output.splitlines()
                if len(parts := line.strip().split(None, 1)) == 2]
    prefix = str(debug.absolute()).replace("\\", "/").casefold() + "/"
    return [pid for pid, command in rows if pid != os.getpid() and
            prefix in command.replace("\\", "/").casefold()]


def prune(repo, apply=False):
    deps = deps_directory(repo)
    with build_lock(deps.parent):
        busy = running_artifacts(deps.parent)
        if busy:
            raise RuntimeError("Build artifacts are in use by process(es): " + ", ".join(map(str, busy)))
        old = candidates(deps)
        size = sum(info[2] for _, info in old)
        print(f"{'Removing' if apply else 'Would remove'} {len(old)} old artifacts ({size:,} bytes)")
        if not apply:
            print("Preview only. Stop this checkout's builds, tests and app before using --apply.")
            return 0
        removed = 0
        for path, expected in old:
            deps_directory(repo)
            if identity(ordinary(path)) != expected:
                raise RuntimeError(f"Artifact changed during cleanup: {path.name}")
            path.unlink()
            removed += 1
        print(f"Removed {removed} artifacts. Newest timestamps (including ties) were kept.")
        return removed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="remove the previewed old generations")
    args = parser.parse_args()
    repo = Path(__file__).absolute().parent.parent
    try:
        configured = os.environ.get("CARGO_TARGET_DIR")
        if configured and Path(configured).resolve() != (repo / "target").resolve():
            raise ValueError("Unset CARGO_TARGET_DIR: only this checkout's target is supported")
        if not (repo / "target").exists():
            print("No target directory; nothing to remove.")
            return 0
        prune(repo, args.apply)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Cleanup stopped: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
