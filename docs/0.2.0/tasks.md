# Tarafold 0.2.0: tasks

Order: measure first, then larger data and longer runs (priority 1), then the rest. Every
task has a "done when" that a test or `tarafold bench` can check. Conventions from v0.1
apply: `cargo fmt`, `clippy -D warnings`, `cargo test`, no `unwrap` on user data, results
reproducible for a fixed seed and `--max-evals`, and no aggregate bench metric dropping by
more than 3 points.

Current limits these tasks lift (v0.1.0): CSV only, read fully into memory (10M-row cap);
discovery on at most 5,000 training rows (`discover.max_rows`); GP fitness on 800 rows
(`gp.rows`); 240,000 GP evaluations (`gp.max_evals`); bootstrap on at most 2,000 rows;
stability reruns on at most 1,000 rows; no progress output during discovery.

## Phase A: measure where data and compute help

- [ ] A.1 Scaling bench: `tarafold bench --suite scaling` runs a fixed set of entries (one per
  tier, plus T5 and the hard cases) at rows {1k, 10k, 100k, 1M} and noise {1%, 10%}.
  Records structure, constant error, R², wall time, peak memory.
  - Done when: the suite runs and writes `scaling.md`; results committed to `docs/benchmarks.md`.
- [ ] A.2 Budget curve: the same entries at GP budgets {60k, 240k, 1M, 5M, 20M} evaluations.
  - Done when: a structure-vs-budget table is in `docs/benchmarks.md`, showing where returns stop.
- [ ] A.3 Hard-case tier in the registry: Hill (n = 2.5), Lennard-Jones, driven resonance,
  noisy `decay` and `exp-growth`, and a 50-column distractor variant.
  - Done when: `tarafold synth --list` shows them and the full bench includes them.

## Phase B: larger data (priority 1)

- [ ] B.1 Streaming CSV ingest: read in chunks, column-wise storage, bounded memory; keep
  delimiter/decimal sniffing and exclusion reasons.
  - Done when: 10M rows × 20 columns load with peak memory under 2.5 GB, and ingest tests pass unchanged.
- [ ] B.2 Parquet input (`tarafold run data.parquet`), numeric columns, row groups streamed.
  New dependency justified in the PR and passing `cargo deny`.
  - Done when: the same dataset as CSV and Parquet gives a byte-identical `report.json` apart from the source fields.
- [ ] B.3 Raise the row cap (configurable, default 100M) with a clear error on memory limits.
  - Done when: a 100M-row synthetic file is rejected or processed with a clear message, never a crash.
- [ ] B.4 Stratified subsampling for discovery: cover the edges of every input, not only the
  dense middle (for example quantile bins per input, then uniform within bins).
  - Done when: on skewed-input datasets, extrapolation R² improves and no aggregate metric drops.
- [ ] B.5 Full-data refits: constants, validation metrics, intervals, and the extrapolation
  guard use all rows (chunked evaluation) even when the search ran on a subsample.
  - Done when: on 1M-row noisy cases, constant error falls versus v0.1 and the scaling bench shows it.
- [ ] B.6 Subsample agreement: rerun discovery on 3 disjoint subsamples of large data and
  report whether they agree (replaces stability reruns when rows ≥ 100k).
  - Done when: the report has `subsample_agreement`, and the grade uses it.
- [ ] B.7 Scale the internal caps with the data: `gp.rows`, bootstrap rows, and stability rows
  grow with available rows and the effort level (B.x and C.1 together).
  - Done when: settings are recorded in `run.toml` and documented.
- [ ] B.8 Speed: profile ingest, relate (MI, GBDT), and evaluation on 10M rows; parallelize
  the hot loops with fixed chunk order.
  - Done when: 10M × 20 `run` under 10 minutes on 8 cores, and the thread-count determinism test passes.

## Phase C: longer compute (priority 1)

- [ ] C.1 `--effort quick|default|thorough|max`: scales GP evaluations (×0.25, ×1, ×5, ×20),
  islands, population, restarts, stability reruns, and optimizer budgets. `--max-evals` still
  overrides. Recorded in the report and `run.toml`.
  - Done when: each level runs on the smoke suite; `max` improves T5 structure recovery on the full bench.
- [ ] C.2 Independent restarts: run the GP several times with different seeds (derived from the
  run seed) and merge the fronts, instead of one long run.
  - Done when: at equal total budget, restarts are at least as good as one run on the budget curve.
- [ ] C.3 Checkpoints and resume: save the search state every N epochs;
  `tarafold run --resume <out-dir>` continues a stopped or timed-out run.
  - Done when: an interrupted then resumed run gives the same front as an uninterrupted one with the same budget.
- [ ] C.4 `--time-budget` by stage: spread a wall-clock budget over discovery, stability, and
  optimization instead of discovery only.
  - Done when: a 2-hour budget run finishes all stages and reports how time was spent.
- [ ] C.5 Progress events from the engine: stage, epoch, evaluations, best loss, ETA; shown in
  the CLI (stderr) and sent over `/api/events` in `tarafold serve`.
  - Done when: a 30-minute run shows progress in both, and JSON stdout stays clean.

## Phase D: more columns (priority 1)

- [ ] D.1 Screening for 100–1,000 columns: group near-duplicates (|r| > 0.98), rank groups,
  include interaction strength, and record every column left out with its reason.
  - Done when: variable precision ≥ 0.9 and recall ≥ 0.9 with 200 distractor columns.
- [ ] D.2 Column roles: `--roles id=run_id,time=day,ignore=notes`; identifiers and
  timestamps are never inputs; the Data view shows roles.
  - Done when: an identifier column with a hidden correlation to the target is never selected.
- [ ] D.3 Larger `max_features` with `--effort`: more inputs passed to search when the budget allows.
  - Done when: the 50-column hard case recovers the true inputs at `thorough`.

## Phase E: accuracy and trust (priority 2)

- [ ] E.1 Grade calibration: compare the selected equation with simpler front neighbours on
  held-out ranges; use subsample agreement (B.6) on large data.
  - Done when: `high` given to wrong structures ≤ 20% and to correct ones ≥ 90% on the full bench.
- [ ] E.2 Units and dimensional analysis (`--units col=unit,...`): reject dimensionally
  inconsistent candidates; report units of fitted constants.
  - Done when: T5 and T7 structure at 10% noise improve; T5 ≥ 80%.
- [ ] E.3 Known-law catalog (~200 forms) with matching up to constants: `novelty.status` =
  `known: <name>` | `variant of: <name>` | `no match`. Offline only.
  - Done when: a novelty tier shows ≤ 5% false "known" and ≥ 90% correct "known".
- [ ] E.4 Null controls: `--null-control N` reruns on shuffled targets and reports the chance level.
  - Done when: the report shows the chance distribution and a pure-noise target is flagged.

## Phase F: later in 0.2 if time allows (priority 3)

- [ ] F.1 Automatic scale choice (fit in log space for wide-range targets).
- [ ] F.2 Errors-in-inputs fitting (orthogonal distance).
- [ ] F.3 Lagged and categorical columns.
- [ ] F.4 Multi-table input with declared keys.
- [ ] F.5 Regime detection (exponent drift across an input's range).
- [ ] F.6 Sharp-feature operators (Hill-type, rational).
- [ ] F.7 Simulator loop (`--simulator "<command>"`).
- [ ] F.8 macOS builds back in CI and releases.

Not in 0.2.0: Python package.

## Release

- [ ] R.1 Full, active, multi, and scaling suites meet their gates; new baselines committed.
- [ ] R.2 Docs: user guide (large data, effort, resume, roles), benchmarks, CHANGELOG.
- [ ] R.3 Tag `v0.2.0-rc.1`, verify downloaded binaries, then `v0.2.0`.
