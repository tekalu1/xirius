#!/usr/bin/env python3
"""The leak check of a push (LIC-041): `.githooks/pre-push` runs this.

    git config core.hooksPath .githooks      # once per clone (CONTRIBUTING.md)

Git gives the hook the remote's name and URL as arguments and one line per ref on stdin:
`<local ref> <local sha> <remote ref> <remote sha>`. The commits looked at are the ones the
push adds that no remote-tracking branch has yet (`<local sha> --not <remote sha> --remotes`).
Over them:

  - gitleaks (`gitleaks git --log-opts=<the range> --config .gitleaks.toml`), when `gitleaks` is
    on PATH; without it the hook says so and goes on;
  - the terms of `.git/info/sensitive-terms` (never committed: one term per line, `#` starts a
    comment, case does not matter) in the added lines and the paths of the diffs, the commit
    messages, the authors and committers, the messages of annotated tags and the ref names.

Any hit exits 1, and the push stops. Standard library only.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
from dataclasses import dataclass

ZERO = "0" * 40
TERMS = "info/sensitive-terms"
# The separators of the log below; neither is in a commit message or a diff that git prints.
MARK = "\x01"


@dataclass(frozen=True)
class Hit:
    where: str  # "commit 1a2b3c4d5e6f (src/x.rs)", "ref refs/heads/x", ...
    term: str
    line: int  # of the term in the terms file


def git(*args: str, check: bool = True) -> str:
    p = subprocess.run(["git", *args], capture_output=True, encoding="utf-8", errors="replace")
    if check and p.returncode != 0:
        raise SystemExit(f"pre-push: git {' '.join(args)} failed: {p.stderr.strip()}")
    return p.stdout


def read_terms(path: str) -> list[tuple[int, str]]:
    """(line, term) of a terms file; [] when there is none."""
    try:
        with open(path, encoding="utf-8-sig") as f:
            lines = f.read().splitlines()
    except FileNotFoundError:
        return []
    out = []
    for n, raw in enumerate(lines, 1):
        term = raw.strip()
        if term and not term.startswith("#"):
            out.append((n, term))
    return out


def updates(stdin_text: str) -> list[tuple[str, str, str, str]]:
    """The ref lines of the push, without the deletions."""
    out = []
    for raw in stdin_text.splitlines():
        parts = raw.split()
        if len(parts) != 4:
            continue
        local_ref, local_sha, remote_ref, remote_sha = parts
        if set(local_sha) == {"0"}:
            continue  # a deletion adds nothing
        out.append((local_ref, local_sha, remote_ref, remote_sha))
    return out


def rev_args(local_sha: str, remote_sha: str, known) -> list[str]:
    """The `git log` arguments of the commits the push adds."""
    args = [local_sha, "--not"]
    if set(remote_sha) != {"0"} and known(remote_sha):
        args.append(remote_sha)
    args.append("--remotes")
    return args


def find(text: str, terms: list[tuple[int, str]], where: str) -> list[Hit]:
    low = text.lower()
    return [Hit(where, t, n) for n, t in terms if t.lower() in low]


def scan_log(log: str, terms: list[tuple[int, str]]) -> list[Hit]:
    """The hits in the output of `git log` with LOG_FORMAT and the diff (-p -U0)."""
    hits: list[Hit] = []
    sha = ""
    path = ""
    in_message = False
    for line in log.splitlines():
        if line.startswith(MARK + "commit "):
            sha = line[len(MARK + "commit "):].strip()
            path = ""
            continue
        if line.startswith(MARK + "author ") or line.startswith(MARK + "committer "):
            who = line[1:].split(" ", 1)
            hits += find(who[1] if len(who) > 1 else "", terms, f"commit {sha[:12]} ({who[0]})")
            if who[0] == "committer":
                in_message = True
            continue
        if in_message:
            end = line.endswith(MARK + "end")
            hits += find(line[: -len(MARK + "end")] if end else line, terms,
                         f"commit {sha[:12]} (message)")
            if end:
                in_message = False
            continue
        if line.startswith("+++ "):
            path = line[4:]
            if path.startswith("b/"):
                path = path[2:]
            hits += find(path, terms, f"commit {sha[:12]} (path {path})")
        elif line.startswith("+"):
            hits += find(line[1:], terms, f"commit {sha[:12]} ({path})")
        elif line.startswith("rename to ") or line.startswith("copy to "):
            hits += find(line, terms, f"commit {sha[:12]} ({line})")
    return hits


LOG_FORMAT = (
    f"--format={MARK}commit %H%n{MARK}author %an <%ae>%n{MARK}committer %cn <%ce>%n%B{MARK}end"
)


def log_of(args: list[str]) -> str:
    return git("log", LOG_FORMAT, "-p", "-U0", "--no-color", "--no-ext-diff", "--no-renames",
               "--diff-merges=first-parent", *args)


def known(sha: str) -> bool:
    return subprocess.run(["git", "cat-file", "-e", sha + "^{commit}"],
                          capture_output=True).returncode == 0


def tag_text(sha: str) -> str:
    """The message of an annotated tag; '' for anything else."""
    kind = git("cat-file", "-t", sha, check=False).strip()
    return git("cat-file", "tag", sha) if kind == "tag" else ""


def gitleaks(exe: str, root: str, args: list[str]) -> int:
    cmd = [exe, "git", "--log-opts=" + " ".join(args), "--redact", "--verbose", "--no-banner",
           "--exit-code", "1"]
    config = os.path.join(root, ".gitleaks.toml")
    if os.path.isfile(config):
        cmd += ["--config", config]
    return subprocess.run(cmd + [root]).returncode


def main(argv: list[str], stdin_text: str, which=shutil.which, leaks=gitleaks) -> int:
    root = git("rev-parse", "--show-toplevel").strip()
    terms_path = git("rev-parse", "--git-path", TERMS).strip()
    if not os.path.isabs(terms_path):
        terms_path = os.path.join(os.getcwd(), terms_path)
    terms = read_terms(terms_path)
    exe = which("gitleaks")
    if exe is None:
        print("pre-push: warning: gitleaks is not on PATH; the push is not checked for "
              "secrets here (CI still runs it)", file=sys.stderr)

    hits: list[Hit] = []
    stop = False
    for local_ref, local_sha, remote_ref, remote_sha in updates(stdin_text):
        for ref in {local_ref, remote_ref}:
            hits += find(ref, terms, f"ref {ref}")
        hits += find(tag_text(local_sha), terms, f"tag {local_ref}")
        args = rev_args(local_sha, remote_sha, known)
        if not git("rev-list", "-n", "1", *args).strip():
            continue  # nothing new
        if terms:
            hits += scan_log(log_of(args), terms)
        if exe is not None and leaks(exe, root, args) != 0:
            print(f"pre-push: gitleaks found a secret in {local_ref}", file=sys.stderr)
            stop = True

    if not terms:
        print(f"pre-push: no terms in {terms_path}, so none were looked for", file=sys.stderr)
    seen = set()
    for h in hits:
        if (h.where, h.term) in seen:
            continue
        seen.add((h.where, h.term))
        print(f"pre-push: {h.where}: has the term on line {h.line} of {terms_path} "
              f"({h.term!r})", file=sys.stderr)
    if hits or stop:
        print("pre-push: the push is stopped. Take it out of the commits (amend or rebase) and "
              "push again (CONTRIBUTING.md, Leaks).", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    for s in (sys.stdout, sys.stderr):
        s.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main(sys.argv[1:], sys.stdin.read()))
