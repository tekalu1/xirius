#!/usr/bin/env python3
"""Build the contents of the new public repository from a commit of this one.

The public repository starts from one commit of the released contents, without this
repository's history. This script makes that tree:

    python scripts/public-export.py --list                 # what is carried and dropped
    python scripts/public-export.py --out ../xirius-public # write the files
    python scripts/public-export.py --out ../xirius-public --gitleaks gitleaks.exe --commit

It reads the files of the commit (`--rev`, HEAD by default) from git's object store, so
uncommitted changes are not carried and this repository is never changed: it does not
check out, stage, stash or write anything inside it.

`scripts/public-export.toml` says what is dropped ([[exclude]]), what must not be in the
carried files ([[forbid]], with those of `public-export.private.toml` beside it, which is
itself not carried) and which files must be there ([expect]). The export stops
(exit 1) when a carried file matches a [[forbid]] rule or an expected file is missing.

`--commit` makes the output a git repository with a single commit, written as
`tekalu <82334772+tekalu1@users.noreply.github.com>` with a Signed-off-by line. The
submodules stay submodules (the same commit of the same URL). Nothing is pushed.

Standard library only.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
import zipfile
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_CONFIG = HERE / "public-export.toml"
AUTHOR = "tekalu <82334772+tekalu1@users.noreply.github.com>"


@dataclass
class Entry:
    mode: str
    kind: str  # blob, commit (submodule) or tree
    sha: str
    path: str


@dataclass
class Rule:
    name: str
    regex: re.Pattern[str]
    allow: list[re.Pattern[str]] = field(default_factory=list)
    allow_match: re.Pattern[str] | None = None


@dataclass
class Config:
    excludes: list[tuple[str, re.Pattern[str], str]]
    rules: list[Rule]
    expect: list[str]
    skip: list[str] = field(default_factory=list)


def glob_regex(pattern: str) -> re.Pattern[str]:
    """A git-style glob on a path from the root. A trailing `/` takes the whole folder."""
    folder = pattern.endswith("/")
    pattern = pattern.rstrip("/")
    out = []
    i = 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif pattern[i] == "*":
            out.append("[^/]*")
            i += 1
        elif pattern[i] == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(pattern[i]))
            i += 1
    tail = "(?:/.*)?" if folder else ""
    return re.compile("^" + "".join(out) + tail + "$")


def load_config(path: Path) -> Config:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    private = path.with_name(path.stem + ".private.toml")
    if private.is_file():
        more = tomllib.loads(private.read_text(encoding="utf-8")).get("forbid", [])
        data.setdefault("forbid", []).extend(more)
    excludes = []
    for e in data.get("exclude", []):
        excludes.append((e["pattern"], glob_regex(e["pattern"]), " ".join(e["reason"].split())))
    rules = []
    for r in data.get("forbid", []):
        rules.append(
            Rule(
                name=r["name"],
                regex=re.compile(r["regex"]),
                allow=[glob_regex(a) for a in r.get("allow", [])],
                allow_match=re.compile(r["allow_match"]) if r.get("allow_match") else None,
            )
        )
    return Config(
        excludes,
        rules,
        list(data.get("expect", {}).get("files", [])),
        list(data.get("scan", {}).get("skip", [])),
    )


def git(repo: Path, *args: str, data: bytes | None = None) -> bytes:
    r = subprocess.run(
        ["git", "-C", str(repo), *args],
        input=data,
        capture_output=True,
        check=False,
    )
    if r.returncode != 0:
        raise SystemExit(f"git {' '.join(args)}: {r.stderr.decode(errors='replace').strip()}")
    return r.stdout


def tree(repo: Path, rev: str) -> list[Entry]:
    out = git(repo, "ls-tree", "-r", "-z", "--full-tree", rev)
    entries = []
    for rec in out.split(b"\0"):
        if not rec:
            continue
        meta, path = rec.split(b"\t", 1)
        mode, kind, sha = meta.decode().split()
        entries.append(Entry(mode, kind, sha, path.decode("utf-8")))
    return entries


def blobs(repo: Path, shas: list[str]) -> dict[str, bytes]:
    """Read many blobs with one `git cat-file --batch`."""
    out = git(repo, "cat-file", "--batch", data="".join(s + "\n" for s in shas).encode())
    found: dict[str, bytes] = {}
    pos = 0
    while pos < len(out):
        nl = out.index(b"\n", pos)
        sha, _kind, size = out[pos:nl].decode().split()
        start = nl + 1
        found[sha] = out[start : start + int(size)]
        pos = start + int(size) + 1
    return found


def excluded(cfg: Config, path: str) -> str | None:
    for pattern, rx, _reason in cfg.excludes:
        if rx.match(path):
            return pattern
    return None


def texts(path: str, data: bytes) -> list[tuple[str, str]]:
    """The text to look through: the file itself, or the members of a zip (a .dawproject)."""
    if data.startswith(b"PK\x03\x04"):
        found = []
        try:
            with zipfile.ZipFile(io.BytesIO(data)) as z:
                for name in z.namelist():
                    body = z.read(name)
                    if b"\0" not in body:
                        found.append((f"{path}!{name}", body.decode("utf-8", errors="replace")))
        except zipfile.BadZipFile:
            return []
        return found
    if b"\0" in data[:8192]:
        return []
    return [(path, data.decode("utf-8", errors="replace"))]


def scan(cfg: Config, files: dict[str, bytes]) -> list[str]:
    hits = []
    for path in sorted(files):
        if path in cfg.skip:
            continue
        for where, text in texts(path, files[path]):
            for rule in cfg.rules:
                if any(a.match(path) for a in rule.allow):
                    continue
                for n, line in enumerate(text.splitlines(), 1):
                    for m in rule.regex.finditer(line):
                        if rule.allow_match and rule.allow_match.search(m.group(0)):
                            continue
                        hits.append(f"{where}:{n}: {rule.name}: {m.group(0)}")
    return hits


def dangling(cfg: Config, files: dict[str, bytes]) -> list[str]:
    """Carried files that name a dropped folder (links that will not resolve)."""
    prefixes = [p.rstrip("/") + "/" for p, _, _ in cfg.excludes if p.endswith("/") and "*" not in p]
    found = []
    for path in sorted(files):
        if path in cfg.skip:
            continue
        for where, text in texts(path, files[path]):
            count = sum(text.count(p) for p in prefixes)
            if count:
                found.append(f"{where}: {count}")
    return found


def inside(child: Path, parent: Path) -> bool:
    try:
        child.resolve().relative_to(parent.resolve())
        return True
    except ValueError:
        return False


def version(files: dict[str, bytes]) -> str:
    data = tomllib.loads(files["Cargo.toml"].decode("utf-8"))
    return data["workspace"]["package"]["version"]


def run_gitleaks(exe: str, out: Path) -> int:
    with tempfile.TemporaryDirectory() as tmp:
        report = Path(tmp) / "gitleaks.json"
        cmd = [exe, "dir", str(out), "--no-banner", "--redact", "--exit-code", "1"]
        cmd += ["--report-format", "json", "--report-path", str(report)]
        if (out / ".gitleaks.toml").is_file():
            cmd += ["--config", str(out / ".gitleaks.toml")]
        r = subprocess.run(cmd, capture_output=True, text=True, check=False)
        findings = json.loads(report.read_text(encoding="utf-8") or "[]") if report.exists() else []
    for f in findings:
        print(f"gitleaks: {f.get('File')}:{f.get('StartLine')}: {f.get('RuleID')}")
    print(f"gitleaks: {len(findings)} finding(s), exit {r.returncode}")
    if r.returncode not in (0, 1):
        print(r.stderr.strip(), file=sys.stderr)
    return r.returncode


def commit(out: Path, entries: list[Entry], ver: str, author: str) -> None:
    name, email = re.fullmatch(r"(.+?)\s*<(.+)>", author).groups()

    def g(*args: str) -> None:
        r = subprocess.run(["git", "-C", str(out), *args], capture_output=True, text=True)
        if r.returncode != 0:
            raise SystemExit(f"git {' '.join(args)}: {r.stderr.strip()}")

    g("init", "-q", "-b", "main")
    g("config", "core.autocrlf", "false")
    g("config", "user.name", name)
    g("config", "user.email", email)
    g("add", "-A")
    for e in entries:
        if e.kind == "commit":
            g("update-index", "--add", "--cacheinfo", f"160000,{e.sha},{e.path}")
        elif e.mode == "100755":
            g("update-index", "--chmod=+x", e.path)
    g("commit", "-q", "-s", "-m", f"Xirius {ver}")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repo", type=Path, default=HERE.parent, help="repository to read (default: this one)")
    ap.add_argument("--rev", default="HEAD", help="commit to read (default HEAD)")
    ap.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    ap.add_argument("--out", type=Path, help="empty folder outside the repository to write to")
    ap.add_argument("--list", action="store_true", help="print what is carried and dropped, write nothing")
    ap.add_argument("--gitleaks", help="gitleaks executable to scan the output with")
    ap.add_argument("--commit", action="store_true", help="make the output a repository with one commit")
    ap.add_argument("--author", default=AUTHOR, help=f"author of that commit (default {AUTHOR})")
    a = ap.parse_args(argv)

    if not a.list and a.out is None:
        ap.error("give --out <dir> or --list")
    repo = Path(git(a.repo, "rev-parse", "--show-toplevel").decode().strip())
    cfg = load_config(a.config)
    rev = git(repo, "rev-parse", "--verify", f"{a.rev}^{{commit}}").decode().strip()
    entries = tree(repo, rev)

    carried = [e for e in entries if excluded(cfg, e.path) is None]
    dropped = [(e, excluded(cfg, e.path)) for e in entries if excluded(cfg, e.path) is not None]
    for e in carried:
        if e.mode == "120000":
            raise SystemExit(f"{e.path}: symbolic links are not carried; replace it first")

    files = blobs(repo, [e.sha for e in carried if e.kind == "blob"])
    by_path = {e.path: files[e.sha] for e in carried if e.kind == "blob"}

    print(f"commit {rev}")
    print(f"carried: {len(carried)} ({sum(1 for e in carried if e.kind == 'commit')} submodule)")
    print(f"dropped: {len(dropped)}")
    for pattern, _rx, reason in cfg.excludes:
        n = sum(1 for _, p in dropped if p == pattern)
        print(f"  {pattern} ({n}): {reason}")
    if a.list:
        for e in carried:
            print(f"carry {e.path}")
        for e, p in dropped:
            print(f"drop  {e.path}  [{p}]")

    status = 0
    missing = [f for f in cfg.expect if f not in by_path]
    for f in missing:
        print(f"missing: {f} (expected in the public repository)")
        status = 1
    hits = scan(cfg, by_path)
    for h in hits:
        print(f"forbidden: {h}")
    if hits:
        status = 1
    links = dangling(cfg, by_path)
    if links:
        print("note: these carried files name a dropped folder:")
        for line in links:
            print(f"  {line}")

    if a.list:
        return status
    out = a.out
    if inside(out, repo):
        raise SystemExit(f"{out} is inside the repository; give a folder outside it")
    if out.exists() and any(out.iterdir()):
        raise SystemExit(f"{out} is not empty; give an empty or new folder")
    if status:
        print("stopped: nothing written")
        return status
    for path, data in by_path.items():
        dest = out / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
    for e in carried:
        if e.kind == "commit":
            (out / e.path).mkdir(parents=True, exist_ok=True)
    print(f"wrote {len(by_path)} files to {out}")

    if a.gitleaks and run_gitleaks(a.gitleaks, out) != 0:
        print("stopped: gitleaks found something (no commit made)")
        return 1
    if a.commit:
        commit(out, carried, version(by_path), a.author)
        print(f"committed in {out} as {a.author}")
    return 0


if __name__ == "__main__":
    os.environ.setdefault("PYTHONUTF8", "1")
    sys.exit(main())
