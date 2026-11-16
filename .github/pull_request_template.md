## What and why

<!-- What does this change and why? Link the issue or spec task (e.g. tasks.md 3.3). -->

## Benchmark impact

<!-- Required for changes to discover/validate/optimize/relate. Paste smoke-bench before/after. -->

| Metric | Before | After |
|---|---|---|
| Symbolic recovery | | |
| Accuracy solution | | |
| Inverse success | | |

## Checklist

- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test` pass
- [ ] Web lint/typecheck/test pass (if `web/` changed)
- [ ] No new dependency, or reason given and `cargo deny check` passes
- [ ] Report schema version bumped (if `report.json` changed)
- [ ] User-facing text avoids causal claims about observational fits
- [ ] `CHANGELOG.md` updated (if user-visible)
- [ ] Commits signed off (DCO)
