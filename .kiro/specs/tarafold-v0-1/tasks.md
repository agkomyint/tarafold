# Tarafold v0.1 — Implementation Tasks

Order: a thin end-to-end pipeline first (Phase 1), then a benchmark to measure it
(Phase 2), then deeper capabilities, each measured by the benchmark. Every task lists the
requirements it covers and a concrete "done when".

Conventions for every task: code passes `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`; public items have doc comments; no `unwrap`/`expect` outside tests.

---

## Phase 0 — Repository foundation

- [x] 0.1 Create the Cargo workspace with all crates from design §2 as empty libraries plus the `tarafold` binary in `tarafold-cli`. Set edition 2024, `rust-version = "1.99"`, `license = "Apache-2.0"`, shared `[workspace.lints]`, release profile from design §11.
  - Done when: `cargo build --workspace` and `cargo run -p tarafold-cli -- --version` work. _R1.4, R15.1_
- [ ] 0.2 Add `deny.toml`, `rustfmt.toml`, and verify `.github/workflows/ci.yml` runs green on all three OSes.
  - Done when: CI passes on a PR. _R14.5, R15.3_
- [ ] 0.3 Scaffold `web/` (Vite + React + TS + D3, ESLint, Vitest) with `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build`, and a single-file build target.
  - Done when: both builds produce `web/dist/` and `web/dist-single/index.html`. _R10.2_

## Phase 1 — Walking skeleton (end to end, minimal)

- [x] 1.1 `tarafold-core`: `Dataset`, `Column`, `Split` (random + ordered), `RunConfig` with defaults + TOML load/merge, `Evidence`, error types, seeded RNG helper.
  - Done when: unit tests cover config precedence and split reproducibility. _R2.3, R7.1_
- [x] 1.2 `tarafold-io`: CSV ingest with delimiter/decimal sniffing, missing tokens, numeric detection, exclusions with reasons, limits, SHA-256.
  - Done when: tests cover `;` + decimal comma files, mixed columns, limits, < 30 rows error, stdin input (`-`), and invalid weight columns. _R3.1–R3.10_
- [x] 1.3 `tarafold-expr`: postfix `Expr`, chunked deterministic evaluator, protected ops, infix/LaTeX/JSON printing, Pratt parser.
  - Done when: proptest `parse(print(e)) == e` passes; golden vectors file exists. _R6.5, R6.8_
- [x] 1.4 `tarafold-synth`: registry with 6 seed equations (one each from T1, T2, T4, T5, T6, T8) and generator writing CSV + `truth.json`; `tarafold synth`.
  - Done when: `tarafold synth business-revenue --rows 2000 --out d` writes both files reproducibly. _R11.1_
- [x] 1.5 `tarafold-discover` (5a only): basis library, elastic-net path, OLS refit, log-log and log-target variants, Pareto front, selection rule.
  - Done when: noiseless T1, T4, and T8 revenue are recovered symbolically. _R6.1, R6.4, R6.7_
- [x] 1.6 `tarafold-validate` (minimal): R², RMSE, MAE, NRMSE on all splits; applicability domain (ranges + Mahalanobis).
  - Done when: metrics match a reference implementation within 1e-12 on fixtures. _R7.2, R7.8_
- [x] 1.7 `tarafold-optimize` (minimal): bounds, fixed non-controllables at medians, 1-D Brent roots, DE for d > 1, feasibility check, top-N by error.
  - Done when: on T8 revenue, candidates evaluated with the true function are within 1% of goal. _R8.1, R8.2, R8.4_
- [x] 1.8 `tarafold-report` + `tarafold-engine`: report types with `schemars` (using the `targets[]` shape and top-level `optimize`/`suggest` sections from the start, even with one target and one goal), pipeline wiring of stages 1, 3, 5a, 6, 7, JSON writer, `equations.csv`, `candidates.csv`, `run.toml`.
  - Done when: `tarafold schema` prints a schema that validates the produced report. _R10.1, R2.5_
  - Status: done; the CLI end-to-end test validates report.json against `tarafold schema` (non-finite numbers are `null` and allowed by the schema).
- [x] 1.9 `tarafold-cli`: `run` with flags from design §7 (subset: target, goal, control, bounds, seed, out, format), human summary, JSON mode, exit codes.
  - Done when: `assert_cmd` integration test runs synth → run end to end and checks files and exit codes. _R2.1, R2.2, R2.6, R2.7_

**Milestone M1:** `tarafold synth` → `tarafold run --goal` works end to end on business data.

## Phase 2 — Measure it

- [x] 2.1 `tarafold-bench`: runner over registry subsets, all metrics in design §6.2, per-dataset and aggregate JSON + Markdown table.
  - Done when: `tarafold bench --suite smoke` runs and prints the table. _R11.3_
- [ ] 2.2 Symbolic equivalence (rounded-constant comparison + numeric equivalence); used by bench and later by stability.
  - Done when: tests show `x*(y+1)` ≡ `x*y + x` and `2.5x^0.7` ≢ `2.5x^0.8`. _R11.3_
  - Status: numeric equivalence (exact) and refit-constants structure check are implemented in the bench; e-graph equivalence still to do.
- [x] 2.3 Commit `bench/baseline.json`; `--baseline` comparison with thresholds; smoke bench in CI.
  - Done when: an intentional regression fails CI. _R11.4, R11.5_
- [ ] 2.4 Expand registry to ≥ 25 equations across T1–T12 with noise, distractor, and collinear variants.
  - Done when: `tarafold synth --list` shows ≥ 25 entries with tiers. _R11.2_
- [ ] 2.5 `bench/reference/pysr_compare.py` (uv script) producing the same metrics file format.
  - Done when: a side-by-side table can be produced locally. _(dev only)_

## Phase 3 — Symbolic regression engine

- [x] 3.1 Forward-mode AD over the evaluator (∂/∂constants, ∂/∂x).
  - Done when: proptest vs. central finite differences passes. _R8.8_
  - Status: ∂/∂x and ∂/∂constants (forward mode over columns) done; LM uses exact Jacobians.
- [x] 3.2 Levenberg–Marquardt constant fitting.
  - Done when: recovers constants of T4/T5 structures from perturbed starts. _R6.2_
- [x] 3.3 GP islands: init, mutations, crossover, tournament, regularized evolution, adaptive parsimony, hall of fame, migration, epoch-based determinism, budgets, cancellation.
  - Done when: determinism test (threads 1 vs 8) is byte-identical; T2, T3, T5 noiseless recovered. _R6.2, R6.9_
- [x] 3.4 `egg` simplification and canonical dedupe on the front.
  - Done when: proptest shows simplification preserves values; front has no equivalent duplicates. _R6.6_
  - Decision: a deterministic rule-based simplifier (bottom-up to a fixed point) replaces `egg` for v0.1: no dependency, values preserved including protected semantics (randomized test over 4,000 expressions).
- [x] 3.5 Operator configuration (`--operators`) and complexity weights.
  - Done when: restricting operators changes the search space as tested. _R6.3_
- [x] 3.6 Re-run full bench; update baseline.
  - Done when: noiseless T1–T5/T8 symbolic recovery ≥ 70%. _Success criterion_
  - Result: 14/14 noiseless T1–T5/T8 cases exact; full suite 95% correct structure (63/66), 99.4% inverse success. GP uses multi-gene linear scaling (least-squares coefficients for top-level additive terms).
- [ ] 3.7 Row weights through elastic net, OLS refit, GP loss, LM, metrics, and baselines; T9 registry entries.
  - Done when: weighting gain > 1.2 on T9 and unweighted runs are byte-identical to before. _R3.10, R6.10_
- [ ] 3.8 Code generation backends (Python, JS, SQL, Excel, LaTeX, JSON) with identifier sanitizing and guard helpers; `tarafold export`.
  - Done when: Python and JS exports match golden vectors within 1e-9 in CI; SQL/Excel pass string tests. _R16.1–R16.5_

## Phase 4 — Relationships

- [x] 4.1 Profile stage (moments, quantiles, histograms, MAD outliers, flags); `tarafold profile`.
  - Done when: 1 M × 50 profile < 30 s on a 4-core machine. _R4.1–R4.3, R13.2_
- [x] 4.2 Pearson/Spearman matrices, KSG MI (kiddo), distance correlation on subsample.
  - Done when: MI matches the analytic value for correlated Gaussians within 5%. _R5.1, R5.2_
- [x] 4.3 Ridge baseline and in-house histogram GBDT with early stopping.
  - Done when: GBDT R² on T6 ≥ 0.95; deterministic across threads. _R5.3_
- [x] 4.4 Permutation importance, Friedman H², VIF/collinearity, leakage suspects.
  - Done when: H² ranks the true interacting pair first on T6. _R5.4–R5.6, R9.7_
- [x] 4.5 Feature screening feeding discovery, with recorded reasons; `tarafold relate`.
  - Done when: distractor variants reach variable-selection precision ≥ 0.9. _R5.7_
  - Result: profiling 1M × 50 in 1.2 s; variable precision 1.00 on the full bench. Screening deliberately never drops a feature for weak evidence alone (dropping cost 3 gravity cases at 200 rows); it ranks, enforces `max_features`, keeps user-named controllables, and deprioritizes near-duplicates. Importance and MI are counted only above their noise levels.

## Phase 5 — Reliability

- [x] 5.1 Bootstrap constant CIs and ensemble prediction intervals; coverage on test.
  - Done when: coverage on 10% noise is within 0.80–0.97. _R7.3_
- [x] 5.2 Structural stability with reduced-budget refits.
  - Done when: stability ≥ 0.7 on noiseless recovered equations and lower on pure-noise targets. _R7.4_
- [x] 5.3 Residual diagnostics, baseline comparison, reliability grade with reasons.
  - Done when: a pure-noise target is graded `low`. _R7.5–R7.7_
- [x] 5.4 Evidence levels, warnings list, text templates (observational and interventional sets), banned-causal-phrase test, optimization disclaimer.
  - Done when: the template test fails if a banned phrase is added to an observational template or a qualifier is removed from an interventional one. _R9.1, R9.2, R9.5–R9.7_
- [x] 5.5 `--data-source` and `--designed`: `interventional_fit` rule, `evidence_basis`, correlated-design warning.
  - Done when: the same T8 data gets `functional_fit` as observational and `interventional_fit` as simulation; a non-designed controllable caps it at `functional_fit`. _R9.3, R9.4_
  - Result (full bench, 132 cases): 90% interval coverage 0.894 on noisy cases; `high` given to 92% of correct structures and 20% (2/10) of wrong ones (noisy cases with R² > 0.99). Stability uses 10 rediscoveries in the CLI, 3 in the bench, each on ≤ 1,000 resampled rows at 10% GP budget.

## Phase 6 — Optimization, multi-goal, and experiment suggestions

- [ ] 6.1 Constraint grammar (`--constrain`), max-change, baseline options, `--fix`.
  - Done when: non-linear constraints are rejected with a clear error; fuzz target added. _R8.3, R14.2_
- [ ] 6.2 Projected Gauss–Newton refinement, domain penalty, ensemble ranking, diversity selection.
  - Done when: returned candidates are pairwise distinct (standardized distance > 0.25). _R8.4, R8.5, R8.7_
- [ ] 6.3 Infeasible-goal handling with binding constraints; sensitivity and elasticities.
  - Done when: an unreachable goal returns exit code 5 with the closest value and binding bound. _R8.6, R8.8, R2.6_
- [ ] 6.4 `tarafold optimize <report-dir>` reuses a saved front without re-discovery.
  - Done when: optimize on a saved report takes < 2 s for T8. _R2.4_
- [ ] 6.5 Integer and step controllables: inward bound rounding, exhaustive 1-D grid, rounded DE, grid-neighborhood search, mixed alternation; T10 registry entry.
  - Done when: inverse success ≥ 90% on T10 and every candidate is on-grid. _R8.9_
- [ ] 6.6 Multiple targets: shared ingest/profile/split, per-target stages and seeds, `targets[]` report shape, `--goal a=..,b=..`, `--exclude-other-targets`, `uses_other_target` warning, combined exit code.
  - Done when: `--target orders,revenue` on T8 recovers both equations and flags that `revenue` uses `orders`. _R17.1–R17.4_
- [ ] 6.7 Goal grammar (`--require`, `--prefer`, weights, `--goal` shorthand), target dependency graph, cycle detection, composed evaluation and AD through chains, shared bootstrap resamples across targets; goal-parser fuzz target.
  - Done when: on T8, changing `price` in a candidate changes predicted `orders`, `revenue`, and `profit` consistently; a cyclic dependency exits with code 2 naming the cycle. _R18.1–R18.3, R17.3_
- [ ] 6.8 Constrained single-objective path (0–1 objectives) with hard goals, soft-goal penalties, joint goal probability, and `--robust`.
  - Done when: `--require "profit>=X" --prefer "min:spend"` on T8 returns candidates that meet the goal under the true functions ≥ 90% of the time. _R18.4, R18.7, R18.8_
- [ ] 6.9 NSGA-II (2–3 objectives) and NSGA-III (4–10), Deb feasibility rules, grid-aware variation, deterministic seeding, reduction to ≤ 20, knee / best-per-objective / most-likely / preferred marks, redundancy detection; T11 registry entries with true fronts.
  - Done when: T11 hypervolume ratio ≥ 0.9 (2–3 objectives) and ≥ 0.75 (5 objectives); determinism test passes. _R18.5, R18.6, R18.10_
- [ ] 6.10 Infeasibility analysis: deletion-filter minimal conflicting subset and best achievable value per conflicting goal; exit code 5.
  - Done when: two contradictory hard goals on T8 are reported as the conflicting pair with correct best-achievable values. _R18.9_
- [ ] 6.11 Suggestion ensemble: constant + structure ensemble with validation-loss weights; explore, exploit, and balanced acquisition; equation-discrimination component.
  - Done when: on a T2 dataset where two front equations agree on observed rows, `explore` places suggestions where they disagree most. _R19.2, R19.3_
- [ ] 6.12 Batch selection with local penalties, de-duplication against existing rows, `--suggest-expand` with domain labels, reason codes, `suggestions.csv` in input format, report note; `tarafold suggest` and `run --suggest`.
  - Done when: filling in `suggestions.csv` with true values and appending it to the input CSV produces a valid next run with no column edits. _R19.1, R19.4–R19.9_
- [ ] 6.13 Closed-loop bench (`--suite active`, T12) comparing against random and Latin-hypercube sampling; add one loop to the smoke suite.
  - Done when: T12 thresholds in design §6.2 are met and the comparison table is in the bench output. _R11.3_

## Phase 7 — Report and workspace

- [ ] 7.1 Typed report loader in `web/` generated from the JSON Schema; TS evaluator passing golden vectors.
  - Done when: Rust and TS golden tests both pass in CI. _R10.1_
- [ ] 7.2 Overview and Explore views (histograms, scatter matrix with canvas, brushing, heatmaps).
  - Done when: 50k-point scatter stays interactive (> 30 fps pan/brush). _R10.4_
- [ ] 7.3 Relationships and Equations views (importance, interactions, Pareto, equation card with KaTeX, residuals, partial dependence, 2D response surface).
  - Done when: clicking a front point updates all equation charts. _R10.4, R10.6_
- [ ] 7.4 Target, Next-experiments, and Reliability views (goal builder, controllables, bounds, constraint editor, candidates with goal checklist, trade-off scatter and parallel coordinates with brushing, conflict explanation, suggestions table and overlay, badges, checklist).
  - Done when: grade and evidence appear next to every equation and candidate, and a 5-objective front can be filtered live. _R10.4, R10.6_
- [ ] 7.5 `report.html` assembly (single-file bundle + injected JSON, `</script>` escaping); `tarafold report --html`; `--open`.
  - Done when: report opens offline from disk in Chrome, Firefox, Safari. _R10.2_
- [ ] 7.6 Accessibility pass: keyboard navigation, ARIA, table alternatives, contrast check.
  - Done when: axe-core in Playwright reports no serious violations. _R10.7_

## Phase 8 — Local server

- [ ] 8.1 `tarafold-server`: embedded assets, `/api/report`, token auth, Host/Origin checks, body limits.
  - Done when: requests without token or with foreign Origin get 401/403 in tests. _R10.3, R14.4_
- [ ] 8.2 `/api/run` with SSE progress, `/api/optimize`, `/api/suggest`, `/api/evaluate`; UI wired to re-run.
  - Done when: changing goals or suggestion mode in the UI updates results without a full re-run. _R10.5_
- [ ] 8.3 `tarafold serve <csv|report-dir>`.
  - Done when: Playwright smoke test passes against `tarafold serve`. _R10.3_

## Phase 9 — Hardening and release

- [ ] 9.1 Remaining CLI surface: all subcommands, `--config`, `--threads`, `--time-budget`, Ctrl-C partial reports, progress bars, `-v`, `--log-file`.
  - Done when: Ctrl-C during discovery writes a report with `status: interrupted`. _R2.3, R2.4, R2.8, R2.9_
- [ ] 9.2 Fuzz targets (CSV, infix parser, constraint parser) and weekly fuzz workflow; CSV export injection guard.
  - Done when: 10 min fuzzing per target finds no crashes. _R14.1, R14.3_
- [ ] 9.3 Performance pass against R13 with `criterion` benches; binary size check in CI (< 40 MB).
  - Done when: 10k × 10 `run` < 120 s on 4 cores. _R13.1, R13.3_
- [ ] 9.4 Docs: user guide, algorithm notes, report schema reference, rustdoc on all public items.
  - Done when: `cargo doc` with `-D missing_docs` passes. _R12.1–R12.3_
- [ ] 9.5 Release workflow dry run on a `v0.1.0-rc.1` tag: 5 targets, checksums, SBOM, attestations.
  - Done when: downloaded binaries run `tarafold bench --suite smoke` on each OS with no other runtime installed, the UI is served from the embedded assets with networking disabled, and a test asserts the binary opens no outbound connections. _R1.1–R1.3, R14.6, R15.2_
- [ ] 9.6 Full bench meets all v0.1 acceptance thresholds in design §6.2; update CHANGELOG; tag `v0.1.0`.
  - Done when: thresholds met and release published. _Success criterion, R15.4_
- [ ] 9.7 Engine interface and `--engine` flag (classical only), engine recorded in report and `run.toml`, `bench --compare` running each available engine side by side.
  - Done when: `--engine quantum` exits with code 2 and a clear message; `bench --compare classical` produces the comparison table. _R20.1–R20.4, R20.6_
  - Future (v0.5, not v0.1): QUBO formulations, local simulator backend, opt-in remote-hardware feature. _R20.5_
