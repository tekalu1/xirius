#!/usr/bin/env python3
"""DCO check (LIC-008): every commit in a range carries a `Signed-off-by:` line.

    python scripts/dco.py [RANGE]        # default: origin/main..HEAD

A sign-off is `Signed-off-by: Name <address>`. As in the DCO app of GitHub, its address must be
the commit's author or committer address. Merge commits and commits by bots are not checked.
Exit status 1 lists the commits that fail.
"""

import re
import subprocess
import sys

SIGN_OFF = re.compile(r"^Signed-off-by:\s+(?P<name>\S.*?)\s+<(?P<mail>[^<>\s]+@[^<>\s]+)>\s*$", re.M)
RECORD = "\x1e"
FIELD = "\x1f"


def judge(message, author_mail, committer_mail):
    """None when the commit is signed off properly, otherwise why not."""
    found = [m.group("mail").lower() for m in SIGN_OFF.finditer(message)]
    if not found:
        return "no Signed-off-by line"
    own = {author_mail.lower(), committer_mail.lower()}
    if not own & set(found):
        return "Signed-off-by is neither the author's nor the committer's address"
    return None


def commits(rng, cwd=None):
    out = subprocess.run(
        ["git", "log", "--no-merges", f"--format={RECORD}%H{FIELD}%ae{FIELD}%ce{FIELD}%an{FIELD}%B", rng],
        capture_output=True,
        text=True,
        encoding="utf-8",
        cwd=cwd,
        check=True,
    ).stdout
    for record in out.split(RECORD):
        if not record.strip():
            continue
        sha, author_mail, committer_mail, author, message = record.split(FIELD, 4)
        yield sha, author_mail, committer_mail, author, message


def check(rng, cwd=None):
    failures = []
    for sha, author_mail, committer_mail, author, message in commits(rng, cwd):
        if author.endswith("[bot]"):
            continue
        why = judge(message, author_mail, committer_mail)
        if why:
            failures.append((sha, message.strip().splitlines()[0] if message.strip() else "", why))
    return failures


def main(argv):
    rng = argv[1] if len(argv) > 1 else "origin/main..HEAD"
    failures = check(rng)
    for sha, subject, why in failures:
        print(f"{sha[:10]} {subject}: {why}")
    if failures:
        print(f"DCO: {len(failures)} commit(s) are not signed off. Add the line with `git commit -s`.")
        return 1
    print(f"DCO: every commit in {rng} is signed off")
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.exit(main(sys.argv))
