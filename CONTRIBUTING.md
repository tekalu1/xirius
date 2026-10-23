# Contributing to Xirius

Xirius is licensed under `MIT OR Apache-2.0`. A contribution is accepted under the same terms
and needs a sign-off, as described below. There is no CLA.

## Sign-off (Developer Certificate of Origin)

Every commit carries a `Signed-off-by` line with the name you are known by and the address you
commit with. The name does not have to be your legal name: your own name, or a handle you use
in public, is accepted, as long as the line is yours and the address is the commit's author or
committer address. Git adds it for you:

```sh
git commit -s -m "UI-012: describe the change"
```

To stop forgetting it, install the hooks of this repository once per clone:

```sh
git config core.hooksPath .githooks
```

- `.githooks/commit-msg` adds the line to every commit message that has none. `git commit -s`
  and the hook together leave one line, not two.
- `.githooks/pre-push` looks for leaks in the commits being pushed (see below) and stops the
  push when it finds one.

Setting `core.hooksPath` means `.git/hooks` is no longer read in that clone.

The line certifies the Developer Certificate of Origin, version 1.1:

```
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.


Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

The address on the `Signed-off-by` line must be the commit's author or committer address.
Merge commits and commits by bots are not checked. To add a missing sign-off:

```sh
git rebase --signoff origin/main     # every commit of the branch
git commit --amend -s --no-edit      # only the last commit
```

## The check

`python scripts/dco.py` lists the commits of `origin/main..HEAD` that lack a sign-off and exits
with 1 when there is any. The `dco` workflow runs it on every pull request over the commits the
pull request adds. `python scripts/test_dco.py` tests the check itself.

The sign-off is required: a pull request with a commit that is not signed off is not merged.
Commits made in GitHub's web editor are signed off too (the repository asks for it), so
the check passes on them.

## Leaks

The repository is public, and so is every commit pushed to it. Two checks look for secrets
before they get there:

- **On your PC**, `.githooks/pre-push` (run by `scripts/pre_push.py`, Python's standard library
  only) looks at the commits the push would add that no remote has yet:
  - [gitleaks](https://github.com/gitleaks/gitleaks) over those commits, with `.gitleaks.toml`.
    When `gitleaks` is not on `PATH`, the hook says so and goes on without it.
  - The terms in `.git/info/sensitive-terms`, a file of your own that is never committed: one
    term per line, `#` starts a comment, case does not matter. Names, addresses, host names or
    paths that must not leave your PC go there. The hook looks for them in the added lines
    and the file paths, the commit messages, the author and committer of each commit, the
    messages of annotated tags and the names of the refs.

  Any hit stops the push and names the commit and the file. Remove the term from the commits
  (amend or rebase) and push again. `git push --no-verify` skips the hook; don't, unless the
  hit is wrong.
- **On GitHub**, the `gitleaks` job of the `rust` workflow scans the whole history on every
  push and pull request, with gitleaks's own release pinned by version and SHA-256.

## Before sending a change

The commit message starts with the id of the plan item in `docs/plan.toml` that the change
carries (`UI-012: …`). A change from outside does not need one; the id is added when the change
is taken in.

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -p xirius-design-lint`
- `cargo deny check`
- `python scripts/plan.py check` when `docs/plan.toml` changes

Dependencies must be under a permissive license (`deny.toml`). GPL, AGPL and LGPL code, JUCE and
Tracktion are not used.

## Security problems

Do not open a public issue for a vulnerability; report it privately as `SECURITY.md` describes.

## Making a release

The version is `version` of `[workspace.package]` in `Cargo.toml`, and the tag of its release
is `v` and the version (`v1.0.0`); the update check of the app reads that tag.

1. Change the version in `Cargo.toml` on `main` (with `Cargo.lock`) and merge it.
2. Push the tag `v<version>` on that commit of `main`. The `release` workflow checks the tag
   against the version, builds the Windows files with `scripts/release.py`, attaches them to a
   new release and publishes it.

To make the same files on your PC (Windows, with Visual Studio's C++ tools and
[NSIS 3](https://nsis.sourceforge.io/)):

```
python scripts/release.py --tag v1.0.0 --makensis "C:\path\to\makensis.exe"
```

It writes `target/dist/`: `Xirius-<version>-win-x64-setup.exe` (the installer, per user, no
administrator rights), `Xirius-<version>-win-x64.zip`, `lame-3.100.tar.gz` (the source of the
LGPL `lame_enc.dll` that ships beside the program) and `SHA256SUMS.txt`. Packaging the same
programs again gives the same bytes (`--no-build` reuses `target/release`). `python
scripts/test_release.py` tests the script without building.
