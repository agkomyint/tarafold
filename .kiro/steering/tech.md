# Tech conventions

- Rust stable (installed locally: 1.99), edition 2024, `rust-version = "1.99"`. Use the existing local toolchain; do not install other toolchains.
- Frontend: Vite + React + TypeScript + D3 in `web/`, managed with pnpm (local: pnpm 11, Node 25).
- Key crates: `csv`, `fast-float`, `faer`, `rayon`, `rand_chacha`, `egg`, `kiddo`, `serde`, `schemars`,
  `clap`, `thiserror`, `anyhow`, `tracing`, `axum`/`tokio` (server feature only), `rust-embed`.
  Adding a new dependency needs a reason in the PR and must pass `cargo deny`.
- Determinism: all randomness goes through `ChaCha8Rng` derived from the run seed. Reductions use
  fixed chunk order. Results must not depend on thread count when bounded by `--max-evals`.
- No `unwrap`/`expect`/`panic!` on user data paths. Library crates return `thiserror` errors.
- Library crates below `tarafold-engine` never print, read env vars, or open sockets.
- User-facing wording lives in `tarafold-report` templates only.

## Commands

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p tarafold-cli -- bench --suite smoke
pnpm --dir web install; pnpm --dir web lint; pnpm --dir web typecheck; pnpm --dir web test; pnpm --dir web build
```
