# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/). The `report.json` schema is versioned separately.

## [Unreleased]

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
