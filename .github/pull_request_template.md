## What and why

<!-- What this changes and why. Name the plan item ids from docs/plan.toml (e.g. UI-012). -->

Plan items:

## How it was checked

<!-- The checks in CONTRIBUTING.md you ran, and anything you tried by hand. -->

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo run -p xirius-design-lint` (when the screen changed)
- [ ] `python scripts/plan.py check` (when `docs/plan.toml` changed)

## Developer Certificate of Origin

Every commit of this pull request must end with a `Signed-off-by:` line with your name and
email (`git commit -s`), certifying the Developer Certificate of Origin 1.1 quoted in
`CONTRIBUTING.md`. Pull requests with a commit that is not signed off are not merged; the
`dco` check fails on them.

- [ ] Every commit is signed off (`Signed-off-by: Your Name <you@example.com>`).
