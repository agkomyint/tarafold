# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/). The `report.json` schema is versioned separately.

## [Unreleased]

## [0.1.0-rc.1] - 2026-11-18

Release candidate for v0.1.0: every phase of the v0.1 spec is in, and the full benchmark
meets all v0.1 acceptance thresholds ([docs/benchmarks.md](docs/benchmarks.md)).

### Added

- Visual workspace (`web/`, Vite + React + D3): Overview, Explore, Relationships, Equations,
  Target, Next experiments, and Reliability views; brushing scatter plots that stay fast at
  50,000 points, partial dependence and response surfaces, parallel-coordinate filtering of
  trade-off sets, what-if sliders, keyboard navigation, data tables for every chart, and no
  serious axe-core findings.
- `report.html`: the workspace as one offline file with the report and a 20,000-row data
  sample embedded; written by `run`, `optimize`, `suggest`, and `tarafold report <dir>`;
  `--open` opens it.
- `tarafold serve <report-dir|csv>`: the workspace with live goal editing, suggestions, and
  re-runs. Loopback only by default, random access token, Host/Origin checks, 1 MB body limit.
- `tarafold export --to python|js|sql|excel|latex|json`: standalone code with protected
  semantics and a header (version, run, grade, evidence, training ranges, renamed columns).
  Python and JavaScript are tested against the engine's golden vectors.
- Row weights are now used by every fit (`--weights` was validated but ignored before).
- `--time-budget`, Ctrl-C (keeps what was found and writes the report, marked
  `interrupted`), `--max-evals`, `--threads`, `-v`/`-vv`, `--log-file`.
- `--engine` is recorded in the config and report; `bench --compare` lists engines side by side.
- Benchmark: 25 known equations (312 cases, new heteroscedastic T9 tier with weighting gain),
  `--suite multi` (T11, known Pareto fronts), and a harder closed loop for `--suite active`
  (T12) where suggestions reach the goal in about half the experiments of random sampling.
- Fuzz targets (weekly on nightly) and seeded parser robustness tests; CI browser tests,
  binary size and speed checks; docs: user guide, algorithms, report schema, benchmarks.

### Changed

- Trade-off sets are trimmed to 20 by farthest-point selection instead of crowding distance
  (better spread with 3+ objectives); redundant objectives are judged on the whole first front.
- Release binaries embed the web UI; the release workflow smoke-checks each binary.
- Dependabot no longer proposes bumping the MSRV toolchain action.

### Fixed

- Constant fitting stalled when one constant's derivative is undefined on some rows (for
  example the exponent in `x^3` with negative `x`); that constant is now held while the
  others are fitted.
- The heteroscedasticity warning no longer suggests `--weights` when weights are in use.

### Benchmark (full suite, 312 cases)

- Correct structure 95%, R² > 0.999 on fresh data 97%, inverse success 99.1%.
- 90% interval coverage 0.904 on noisy cases; `high` grade for 96% of correct structures and
  6 of 15 wrong ones (close approximations of noisy exponentials).

## [0.1.0-alpha.2] - 2026-11-17

Second preview: Phases 5 and 6 of the v0.1 spec. The visual workspace, local server, and equation
export are not included yet.

### Added

- Reliability: bootstrap confidence intervals for constants and ensemble prediction intervals,
  structural stability by resampled rediscovery, residual diagnostics; grades can reach `high`.
- `--data-source experimental|simulation` and `--designed` for the `interventional_fit` level.
- Optimization: `--constrain` (linear), `--max-change`, `--integer`, `--step`.
- Multi-goal optimization: `--require`, `--prefer max:x[:w]`, `--robust`, `--lexicographic`,
  chained target equations, NSGA-II / NSGA-III trade-off sets with knee, best, preferred and
  most-likely marks, conflict reports, redundant-objective detection.
- `--suggest N` and `tarafold suggest <report-dir>`: next experiments (explore / exploit /
  balanced) written to `suggestions.csv` in the input's column layout.
- `tarafold optimize <report-dir>`: new goals on a saved report without rediscovery.
- `--operators`, cleaner simplifier, exact gradients for constant fitting, shared golden
  evaluator vectors, schema validation of `report.json`.
- Benchmark: two seeds per case (144 cases incl. new T10 discrete-input tier) and
  `--suite active` (closed-loop experiment selection).

### Known limitations

- In the closed-loop benchmark, suggestions do not yet need fewer experiments than random
  sampling (median 35 vs 35 to recover the equation).
- Inverse success on discrete-input datasets (T10) is 88.9%, below the 90% target.

### Benchmark (full suite, 144 cases)

- Correct structure 92%, R² > 0.999 on fresh data 92%, inverse success 97.8%.
- 90% interval coverage 0.894 on noisy cases; `high` grade for 92% of correct structures and
  17% of wrong ones.
## [0.1.0-alpha.1] - 2026-11-17

First preview. Phases 1–4 of the v0.1 spec; the visual workspace, multi-goal optimization,
experiment suggestions, bootstrap reliability, and the local server are not included yet.

### Added

- `tarafold run`: CSV ingest (delimiter/decimal sniffing, missing values, exclusions, stdin),
  column profiling, relationship analysis, equation discovery, validation on held-out data,
  and single-goal target-seeking with bounds and fixed values. Writes `report.json`
  (schema 0.1.0), `equations.csv`, `candidates.csv`, and `run.toml`.
- Equation discovery: sparse basis regression (linear, log-log, log-target), subset search,
  Levenberg–Marquardt constant refits, and genetic-programming symbolic regression
  (island model, multi-gene linear scaling, thread-count-independent results).
- `tarafold relate`: correlations, KSG mutual information, distance correlation, ridge and
  gradient-boosted baselines, permutation importance, Friedman H², VIF, leakage checks.
- `tarafold profile`, `tarafold synth` (10 known-equation datasets), `tarafold bench`
  (smoke/full suites, baselines, regression checks), `tarafold schema`.
- Evidence levels and reliability grades; the grade is capped at `medium` until stability
  checks land.

### Benchmark (full suite, 66 cases)

- Correct structure 95%, R² > 0.999 on fresh data 97%, noiseless T1–T5/T8 exact 14/14.
- Optimization candidates that hit the goal under the true equation: 99.4%.

### Changed

- v0.1 spec widened for general use: experimental/simulation data sources with an
  `interventional_fit` evidence level, equation export (Python, JS, SQL, Excel, LaTeX, JSON),
  row weights, integer and stepped controllables, multiple targets, and stdin input.
  Categorical columns and lagged features moved to v0.2.
- Multi-goal optimization: hard and soft goals, up to 10 objectives (NSGA-II/III), chained
  targets, joint goal probability, conflict reporting.
- Next-experiment suggestions (`--suggest`, `tarafold suggest`) with a closed-loop benchmark.
- Pluggable engines (`--engine classical|quantum|hybrid`, `bench --compare`): classical in
  v0.1; quantum and hybrid planned as experimental (v0.5).

### Added

- v0.1 spec: requirements, design, and implementation tasks.
- Open-source project files, CI and release workflows.
