# Contributing to Tarafold

Thanks for helping. This file covers how to build, test, and get a change merged.

## Setup

- Rust 1.99 or newer (`rustup`), with `rustfmt` and `clippy`.
- For the UI: Node 22+ and pnpm.

```sh
cargo build --workspace
cargo test --workspace
pnpm --dir web install
```

## Before opening a PR

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p tarafold-cli --release -- bench --suite smoke --baseline bench/baseline.json
pnpm --dir web lint && pnpm --dir web typecheck && pnpm --dir web test
```

## Rules for changes

- **Benchmark first.** Changes to discovery, validation, or optimization must include smoke-bench
  results before and after. A regression needs a written justification.
- **Determinism.** Randomness comes only from the run seed via `ChaCha8Rng`. Results must not depend on thread count.
- **No panics on user data.** Library crates return errors; `unwrap`/`expect` are denied outside tests.
- **Honest wording.** User-facing text lives in `tarafold-report` templates. Do not describe
  observational fits with causal language; a test enforces this.
- **Dependencies.** New crates need a reason in the PR and must pass `cargo deny check`.
- **Report schema.** Changes to `report.json` bump the schema version (SemVer) and update `docs/`.
- **Scope.** Keep PRs focused. Open an issue first for new features or algorithms.

## Commit and PR conventions

- Conventional Commits (`feat:`, `fix:`, `perf:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`).
- Add a line to `CHANGELOG.md` under *Unreleased* for user-visible changes.
- By contributing you agree your contribution is licensed under Apache-2.0
  (see section 5 of the [LICENSE](LICENSE)). Sign off commits (`git commit -s`) per the
  [Developer Certificate of Origin](https://developercertificate.org/).

## Reporting a discovery miss

If Tarafold fails to recover an equation you know, open a "Discovery miss" issue with the
generating equation, sampling ranges, row count, noise, and the command you ran. These become
benchmark cases.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
