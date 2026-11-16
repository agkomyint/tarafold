# Tarafold v0.1 — Requirements

## Introduction

Tarafold is a numerical discovery engine. Given a CSV of numerical observations, it
recovers interpretable equations `y = f(x1, …, xn)` and finds feasible input
combinations that bring `y` close to a user-chosen target `y*` under constraints.

v0.1 ships as **one self-contained binary** (`tarafold`) written in Rust. The CLI is the
primary interface: one command runs the whole pipeline end to end and writes a
machine-readable report plus a self-contained HTML report. A visual workspace (D3) is
embedded in the binary and served locally on demand.

**Primary success criterion:** on synthetic datasets generated from known equations,
Tarafold, without being told the equation, recovers an approximation that is accurate on
held-out data, and proposes inputs that, when evaluated with the *true* equation, land
within tolerance of the target.

### Terminology

| Term | Meaning |
|---|---|
| Dataset | The parsed CSV: numeric columns, rows. |
| Target | The column `y` the user wants to explain and steer. |
| Feature | Any non-target numeric column used as a candidate input. |
| Controllable | A feature the user says they can change. Others are held fixed. |
| Candidate equation | A symbolic expression produced by discovery, with fitted constants. |
| Pareto front | Candidate equations that are not beaten on both accuracy and complexity. |
| Selected equation | The candidate chosen by the model-selection rule. |
| Applicability domain | The region of input space supported by training data. |
| Evidence level | One of `association`, `predictive`, `functional_fit`, `interventional_fit`. v0.1 never emits `causal`. |
| Data source | How the data was produced, declared by the user: `observational` (default), `experimental`, or `simulation`. |
| Designed column | A column whose values were set deliberately by the experimenter or simulation, not merely observed. |
| Row weight | A non-negative per-row weight, e.g. inverse measurement variance. |
| Goal | A condition on a target: exact, threshold, range, maximize, or minimize. |
| Hard / soft goal | Hard goals (`--require`) must hold; soft goals (`--prefer`) are traded off. |
| Objective | A maximize or minimize goal. |
| Trade-off set | The Pareto front of candidates when there are two or more objectives. |
| Suggestion | Proposed input settings for the next experiment, with the reason it was chosen. |
| Run | One execution of the pipeline, identified by a run ID and fully described by its config. |

---

## Requirements

### R1 — Single binary distribution

**User story:** As a user, I want one executable with no runtime dependencies, so that I can
run Tarafold on any machine without installing Python, Julia, or Node.

1. THE `tarafold` binary SHALL run on Windows x86_64, Linux x86_64/aarch64 (static musl), and macOS x86_64/aarch64 without other runtime installs.
2. THE binary SHALL embed the visual workspace assets; serving the UI SHALL NOT require Node.js or network access.
3. THE binary SHALL make no outbound network requests and SHALL collect no telemetry.
4. WHEN `tarafold --version` runs, THE system SHALL print the version, git commit, build target, and report-schema version.

### R2 — End-to-end CLI

**User story:** As a user, I want one command that runs the whole pipeline, so that I get
discoveries and target-reaching inputs without a UI.

1. THE CLI SHALL provide `tarafold run <csv|-> --target <col>` that executes ingest → profile → relate → discover → validate → (optionally) optimize → report.
2. WHEN `--goal`, `--require`, or `--prefer` is given, THE system SHALL run the optimization stage; WHEN `--suggest <N>` is given, THE system SHALL run the experiment-suggestion stage; WHEN neither is given, THE system SHALL skip these stages and say so in the report.
3. THE CLI SHALL accept `--control <cols>`, `--fix <col=value>`, `--constrain <expr>`, `--baseline <row|median|values>`, `--integer <cols>`, `--step <col=size>`, `--weights <col>`, `--data-source <observational|experimental|simulation>`, `--designed <cols>`, `--require <goal>`, `--prefer <goal>`, `--robust <p>`, `--suggest <N>`, `--suggest-mode <explore|exploit|balanced>`, `--suggest-expand <pct>`, `--seed`, `--time-budget`, `--max-evals`, `--threads`, `--out <dir>`, `--config <file.toml>`, and `--format <human|json>`.
4. THE CLI SHALL expose each stage as a subcommand: `profile`, `relate`, `discover`, `validate`, `optimize`, `suggest`, plus `synth`, `bench`, `serve`, `schema`, `report`, `export`.
5. WHEN `run` completes, THE system SHALL write `report.json`, `report.html`, `equations.csv`, `candidates.csv`, and `run.toml` (the fully resolved config) to `--out`.
6. THE CLI SHALL use exit codes: `0` success, `1` internal error, `2` invalid usage/config, `3` invalid data, `4` completed but no candidate met validation thresholds, `5` completed but no feasible optimization candidate.
7. WHEN `--format json` is set, THE CLI SHALL write only JSON to stdout and diagnostics to stderr.
8. THE CLI SHALL show progress on stderr when attached to a TTY and SHALL be silent otherwise unless `--verbose`.
9. WHEN interrupted (Ctrl-C), THE system SHALL stop at the next checkpoint and write a partial report marked `status: "interrupted"`.

### R3 — Data ingestion

**User story:** As a user, I want to upload messy real CSVs and be told exactly what was used.

1. THE system SHALL read CSV with a header row; delimiter, quote character, and decimal separator SHALL be auto-detected and overridable.
2. THE system SHALL treat columns that parse as finite numbers in ≥ 95% of non-empty cells as numeric; other columns SHALL be excluded and listed with the reason.
3. THE system SHALL treat empty cells, `NA`, `NaN`, `null`, and unparseable cells in numeric columns as missing.
4. THE system SHALL drop rows with missing values in the target or in any used feature, and SHALL report counts per column.
5. THE system SHALL exclude constant columns and exact duplicate columns from features, and report them.
6. THE system SHALL enforce configurable limits (default: 2 GB file, 10 M rows, 500 columns) and fail with exit code 3 when exceeded.
7. IF fewer than 30 usable rows remain, THEN THE system SHALL fail with exit code 3 and a message stating the count.
8. THE system SHALL record a SHA-256 hash of the input file in the report.
9. WHEN the input path is `-`, THE system SHALL read CSV from stdin, apply the same limits, and record `source: "stdin"` with the hash of the bytes read.
10. WHEN `--weights <col>` is given, THE system SHALL use that column as row weights, exclude it from features, and fail with exit code 3 if any used weight is negative, non-finite, or all weights are zero.

### R4 — Profiling

**User story:** As a user, I want to see what each variable looks like before trusting any equation.

1. FOR each numeric column, THE system SHALL compute count, missing, min, max, mean, std, quantiles (1, 5, 25, 50, 75, 95, 99), skewness, number of unique values, sign (all-positive / mixed), and a 50-bin histogram.
2. THE system SHALL flag outliers using a robust z-score (median/MAD) above 5 and report counts; outliers SHALL NOT be removed unless `--drop-outliers` is set.
3. THE system SHALL flag likely integer-valued and likely bounded (e.g. a rate in [0,1]) columns.

### R5 — Relationship discovery

**User story:** As a user, I want to know which variables relate to the target, including nonlinear and interaction effects.

1. FOR every feature pair and feature–target pair, THE system SHALL compute Pearson and Spearman correlation.
2. FOR every feature–target pair, THE system SHALL compute mutual information (KSG estimator) and distance correlation, on a subsample of at most 5,000 rows.
3. THE system SHALL fit a baseline ridge regression and a gradient-boosted tree model on the training split and report their held-out R² as reference points.
4. THE system SHALL report permutation importance of each feature for the tree model, with a confidence interval from repeated permutations.
5. THE system SHALL estimate pairwise interaction strength (Friedman's H-statistic) for the top-k features (default k = 8).
6. THE system SHALL flag multicollinearity when |Pearson| > 0.95 or variance inflation factor > 10.
7. THE system SHALL select a feature subset for symbolic discovery (default max 12) and record why each feature was kept or dropped.

### R6 — Equation discovery

**User story:** As a user, I want interpretable candidate equations ranked by accuracy and simplicity.

1. THE system SHALL run a sparse basis regression stage (FFX/SINDy-style) that produces candidate linear-in-parameters equations.
2. THE system SHALL run a genetic-programming symbolic regression stage with fitted constants, seeded with the sparse-regression candidates.
3. THE default operator set SHALL be `+ − × ÷ pow exp log sqrt square abs`; users SHALL be able to restrict or extend it from the supported list (`sin cos tanh inv max min`).
4. THE system SHALL produce a Pareto front of (complexity, validation loss) with at most 20 equations.
5. EACH candidate SHALL include: infix string, LaTeX, JSON expression tree, constants, complexity, train/validation metrics, and the variables used.
6. THE system SHALL algebraically simplify candidates and merge equivalent ones before reporting.
7. THE system SHALL select one equation by the rule in the design (largest drop in log-loss per complexity among candidates within 10% of the best validation loss); users SHALL be able to override by ID.
8. Expressions SHALL be evaluated with protected semantics; any candidate producing non-finite values on training data SHALL be discarded.
9. WITH the same input, config, and seed, THE system SHALL produce identical equations regardless of thread count, when `--max-evals` (not `--time-budget`) bounds the search.
10. WHEN row weights are given, THE system SHALL minimize weighted loss in every discovery strategy and constant fit, and report weighted and unweighted metrics.

### R7 — Validation and reliability

**User story:** As a user, I want to know how far to trust each equation.

1. THE system SHALL split rows into train/validation/test (default 60/20/20, seeded); the test split SHALL be used only for final reporting.
2. FOR each Pareto candidate, THE system SHALL report R², RMSE, MAE, and normalized RMSE on train, validation, and test.
3. FOR the selected equation, THE system SHALL compute bootstrap (default 200 resamples) confidence intervals for constants and for predictions.
4. THE system SHALL report structural stability: the fraction of bootstrap refits (default 20, reduced search budget) that recover an equivalent structure.
5. THE system SHALL report residual diagnostics: residual vs. prediction correlation, heteroscedasticity indicator, and largest residuals by row.
6. THE system SHALL compare the selected equation's test R² against the baseline models and state whether it is within 0.02 of the best baseline.
7. THE system SHALL assign a reliability grade (`high`, `medium`, `low`) by the rule in the design, and list the reasons.
8. THE system SHALL model the applicability domain (per-feature ranges plus Mahalanobis distance to training data) and expose a function to test whether a point is inside it.

### R8 — Target-seeking optimization

**User story:** As a user, I want to set a target value and see which controllable inputs could reach it under my constraints.

1. THE user SHALL choose controllable features; non-controllable features SHALL be fixed at the baseline (default: training medians).
2. THE user SHALL be able to set bounds per controllable feature; default bounds SHALL be the observed min and max.
3. THE user SHALL be able to add linear constraints (e.g. `spend_a + spend_b <= 50000`) and `--max-change <col=pct>`.
4. THE system SHALL find up to N (default 5) diverse candidate input combinations minimizing `|f(x) − y*|`, with secondary objectives of small change from baseline and staying inside the applicability domain.
5. EACH optimization candidate SHALL report: input values, change from baseline (absolute and %), predicted y with a prediction interval, distance from y*, in-domain flag, and constraint satisfaction.
6. IF no candidate reaches within tolerance (default 1% of target range), THEN THE system SHALL report the closest achievable value and which constraint or bound is binding.
7. Candidates outside the applicability domain SHALL be visibly labeled as extrapolation and SHALL NOT rank above in-domain candidates with comparable error.
8. THE system SHALL perform sensitivity analysis: local partial derivatives of the selected equation at each candidate.
9. THE user SHALL be able to mark controllables as integer (`--integer`) or as multiples of a step (`--step col=0.5`); every returned candidate SHALL satisfy these exactly, and bounds SHALL be rounded inward to valid values.

### R9 — Honest claims (correlation vs. prediction vs. causation)

**User story:** As a user, I must not be misled into thinking a change will cause an outcome,
and when my data comes from designed experiments or simulations I want results that say so.

1. EVERY relationship and equation in the report SHALL carry an evidence level: `association`, `predictive`, `functional_fit`, or `interventional_fit`.
2. v0.1 SHALL NOT emit the evidence level `causal`, and text about observational data SHALL NOT use causal verbs ("causes", "drives", "will increase").
3. THE user SHALL declare the data source with `--data-source` (default `observational`) and, for `experimental`, the designed columns with `--designed` (default: the controllables). For `simulation`, all features default to designed.
4. THE system SHALL assign `interventional_fit` only when the data source is `experimental` or `simulation`, every controllable is a designed column, and the selected equation is graded `medium` or `high`. Otherwise the highest level SHALL be `functional_fit`.
5. FOR observational data, optimization results SHALL be presented as "inputs for which the fitted equation predicts y ≈ y*", with a fixed disclaimer that observational fits do not establish that intervening produces the outcome.
6. FOR `interventional_fit` results, the report SHALL state that the claim depends on the user's declaration of how the data was produced, and that it holds only inside the applicability domain.
7. THE report SHALL list detected threats to interpretation: multicollinearity, extrapolation, low stability, small sample, and target leakage suspects (features with |r| > 0.999 to the target).

### R10 — Report and visual workspace

**User story:** As a user, I want to explore the discoveries visually.

1. `report.json` SHALL conform to a versioned JSON Schema printed by `tarafold schema`.
2. `report.html` SHALL be a single self-contained file (inline JS/CSS/data) that opens offline.
3. `tarafold serve <report-dir|csv>` SHALL serve the workspace on `127.0.0.1` with a random free port by default, and print the URL.
4. THE workspace SHALL show: dataset overview, per-variable histograms, a scatter-matrix/2D scatter explorer, correlation and MI heatmap, importance and interaction charts, the Pareto front, each equation with predicted-vs-actual and residual plots, partial-dependence curves of the selected equation, a 2D response surface for any two features, optimization controls, candidate results, trade-off charts for multi-goal results (scatter for 2–3 objectives, parallel coordinates with live filtering for more), a goal checklist per candidate, a suggested-experiments table and plot against existing data, and reliability indicators.
5. WHEN served against a CSV, THE workspace SHALL let users change target, controllables, goal, and constraints and re-run stages via a local HTTP API.
6. THE workspace SHALL render reliability grade and evidence level next to every equation and candidate.
7. THE workspace SHALL be usable with keyboard only and meet WCAG 2.1 AA color contrast; charts SHALL use a colorblind-safe palette.

### R11 — Synthetic data and benchmark

**User story:** As a maintainer, I want to measure objectively whether Tarafold discovers known equations.

1. `tarafold synth <name>` SHALL generate datasets from a built-in registry of known equations with configurable rows, noise level, distractor features, collinear features, and seed; it SHALL write the CSV and a `truth.json` with the ground-truth equation.
2. THE registry SHALL include at least 25 equations across tiers defined in the design (linear, polynomial, rational, power-law, exponential/log, interactions, physics, business-style, heteroscedastic with known weights, integer controllables, multi-goal problems with known trade-off fronts).
3. `tarafold bench` SHALL run the registry (or a named subset) through the pipeline and output per-dataset and aggregate metrics defined in the design, including a closed-loop suite (`--suite active`) that measures how many suggested experiments are needed to reach goals or recover equations, compared with random sampling.
4. THE bench SHALL compare results against a committed baseline file and exit non-zero when an aggregate metric regresses past its threshold.
5. A reduced "smoke" bench SHALL run in CI in under 10 minutes.

### R12 — Library and extensibility

**User story:** As a developer, I want to use the engine without the CLI.

1. THE engine SHALL be a set of Rust library crates with no dependency on the CLI, HTTP server, or UI.
2. EACH stage SHALL be callable independently with typed inputs and outputs and SHALL be serializable with serde.
3. THE public API SHALL be documented with rustdoc; every public item SHALL have a doc comment.

### R13 — Performance

1. ON a 4-core laptop, `tarafold run` on 10,000 rows × 10 features with default settings SHALL finish within 120 s.
2. Profiling SHALL handle 1 M rows × 50 columns within 30 s and 4 GB RAM.
3. Discovery SHALL subsample to at most 5,000 training rows by default, and report that it did.

### R14 — Security and supply chain

1. THE parser SHALL treat input files as untrusted: no evaluation of cell content, bounded memory, and fuzz-tested parsing.
2. Constraint and `--constrain` expressions SHALL be parsed by a dedicated grammar, never evaluated as code.
3. Exported CSVs SHALL neutralize cells beginning with `= + - @` when they are text.
4. THE local server SHALL bind to loopback by default, require a per-session random token for API calls, and reject cross-origin requests.
5. CI SHALL run `cargo deny` (licenses, advisories, sources) and fail on known vulnerabilities.
6. Releases SHALL include SHA-256 checksums, an SBOM (CycloneDX), and GitHub build-provenance attestations.

### R15 — Open-source project hygiene

1. THE project SHALL be licensed Apache-2.0, with a `LICENSE` file and SPDX identifiers in crate manifests.
2. THE repo SHALL contain README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, CHANGELOG, issue and PR templates.
3. CI SHALL run format, clippy (deny warnings), tests, docs build, web lint/typecheck/build, and the smoke bench on Linux, Windows, and macOS.
4. Releases SHALL follow SemVer; the report schema SHALL carry its own version and follow SemVer separately.

### R16 — Equation export

**User story:** As a user, I want to use a discovered equation in my own tools without Tarafold.

1. `tarafold export <report-dir> --to <python|js|sql|excel|latex|json>` SHALL emit the selected equation (or `--id <n>`) as standalone code: a Python function using only `math`, a JavaScript function using only `Math`, a SQL expression (ANSI with `EXP`, `LN`, `POWER`, `SQRT`), an Excel formula referencing named cells, LaTeX, or the JSON tree.
2. Exported code SHALL include a header comment with the Tarafold version, run ID, reliability grade, evidence level, and the training range of each variable.
3. Exported code SHALL reproduce the engine's values within 1e-9 relative error on the golden vectors (SQL and Excel are checked by string tests).
4. Variable names that are not valid identifiers in the target language SHALL be sanitized deterministically, and the mapping SHALL be included in the header.
5. Protected operations SHALL keep their semantics in exported code: domain errors produce NaN/NULL/`#NUM!` rather than a silently different value.

### R17 — Multiple targets

**User story:** As a user with several outcome columns, I want each explained in one run.

1. `--target` SHALL accept a comma-separated list; THE system SHALL run relate → discover → validate (→ optimize) once per target and share ingest, profile, and split.
2. Other targets SHALL remain available as features for each target unless `--exclude-other-targets` is set, and the report SHALL note when one target's selected equation uses another target.
3. `--goal` SHALL accept `col=value` pairs for multiple targets; all goals across targets SHALL be optimized jointly over shared controllables as defined in R18.
4. THE exit code SHALL be the most severe across targets.

### R18 — Multi-goal optimization

**User story:** As a user with several outcomes, I want inputs that satisfy all my hard
requirements and show the best trade-offs between the things I want to maximize or minimize.

1. A goal SHALL be one of: exact (`col=v`), threshold (`col>=v`, `col<=v`), range (`col=a..b`), maximize (`max:col`), or minimize (`min:col`). Exact, threshold, and range goals MAY also be soft.
2. `--require <goal>` SHALL add a hard goal and `--prefer <goal>[:weight]` a soft goal; `--goal` SHALL remain shorthand for `--require` with an exact goal. Any number of hard goals SHALL be supported; at most 10 max/min objectives SHALL be accepted, with exit code 2 above that.
3. WHEN one target's selected equation uses another target, THE system SHALL compose the equations so that every prediction at a candidate input is consistent; IF the dependencies form a cycle, THEN THE system SHALL fail with exit code 2 naming the cycle.
4. WITH zero or one max/min objective, THE system SHALL return up to N diverse candidates that satisfy all hard goals, ranked per R8.4–R8.7 extended to all goals.
5. WITH two or more max/min objectives, THE system SHALL return a trade-off set (Pareto front) of up to 20 diverse candidates satisfying all hard goals, and SHALL mark the knee point, the best candidate per objective, and the candidate with the highest probability of meeting every goal.
6. WHEN priorities are given (`--prefer max:a:3`) or `--priority lexicographic` is set, THE system SHALL also return the single candidate that best matches the weights or order.
7. FOR each candidate, THE system SHALL report the predicted value and interval of every target, which goals are met, and the joint probability that all hard goals are met, estimated from a bootstrap ensemble refit on shared resamples across targets.
8. WHEN `--robust <p>` is set, THE system SHALL accept only candidates whose joint probability of meeting all hard goals is at least `p`.
9. IF no candidate satisfies all hard goals, THEN THE system SHALL report a minimal conflicting subset of goals, the best achievable value of each goal when the others hold, and exit with code 5.
10. THE system SHALL report pairs of objectives whose values are rank-correlated above 0.9 across the trade-off set as redundant and suggest merging one into a threshold.

### R19 — Next-experiment suggestions

**User story:** As a researcher, I want Tarafold to tell me which experiments to run next, so
that each round of new data most reduces uncertainty or most likely reaches my goals.

1. `tarafold run … --suggest <N>` and `tarafold suggest <report-dir> -n <N>` SHALL return N proposed input settings over the controllable (or designed) columns, respecting bounds, constraints, integer and step rules, and fixed values.
2. `--suggest-mode explore` SHALL prefer settings where the model is least certain, including where competing equations on the Pareto front disagree most; `exploit` SHALL prefer settings most likely to meet the goals or improve the objectives; `balanced` (default) SHALL mix both within one batch and report the split.
3. WHEN no goals are given, THE system SHALL use `explore` and say so.
4. Suggestions in one batch SHALL be pairwise diverse (standardized distance ≥ 0.15) and SHALL NOT duplicate existing rows.
5. `--suggest-expand <pct>` SHALL allow suggestions up to that percentage beyond the observed range of each controllable (default 0); such suggestions SHALL be labeled as outside the data domain.
6. EACH suggestion SHALL include: input values, the reason it was chosen (uncertainty reduction, equation discrimination, or goal likelihood), predicted values with intervals, and the joint goal probability when goals exist.
7. THE system SHALL write `suggestions.csv` with the same columns as the input, suggested values filled in, and target columns empty, so results can be filled in and appended to the dataset for the next run.
8. THE report SHALL state that suggestions are proposals of where to measure next, not predictions that the goal will be reached.
9. Suggestions SHALL be deterministic for the same input, config, and seed.
