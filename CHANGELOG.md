# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/). The `report.json` schema is versioned separately.

## [Unreleased]

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
