# Tarafold v0.1 — Design

## 1. Overview

Tarafold is a Rust workspace that builds one binary, `tarafold`. The numerical engine is
a set of library crates with no knowledge of the CLI, HTTP, or UI. The CLI drives the
engine end to end. A Vite + React + D3 workspace is compiled to static assets, embedded
into the binary, and used both by `tarafold serve` and the self-contained `report.html`.

```
            ┌────────────────────────── tarafold (single binary) ──────────────────────────┐
 CSV ──────▶│ tarafold-cli ──▶ tarafold-engine (pipeline orchestrator)                       │
            │                    │                                                          │
            │   ingest ─▶ profile ─▶ relate ─▶ discover ─▶ validate ─▶ optimize ─▶ report    │
            │   (io)      (profile)  (relate)   (discover)  (validate)  (optimize)  (report)  │
            │                    ▲        shared: core, expr                                 │
            │ tarafold-server (axum, loopback) ── embedded web/dist (rust-embed)             │
            └───────────────────────────────────────────────────────────────────────────────┘
                     │                                         │
               report.json / report.html / *.csv        browser workspace (D3)
```

Why these choices:

| Decision | Choice | Reason |
|---|---|---|
| Engine language | Rust | Single static binary, native speed, no Julia/Python runtime. |
| Symbolic regression | In-house (sparse basis + GP) | PySR needs Julia at runtime. PySR is kept as a dev-only reference in the benchmark. |
| CSV | `csv` crate + own columnar `Dataset` | Numeric-only v0.1. Polars' Rust API adds build time and binary size and changes often. Ingest sits behind a trait so Polars/Parquet can be added later. |
| Linear algebra | `faer` | Pure Rust, fast, no BLAS/LAPACK system deps. |
| Parallelism | `rayon` | Data-parallel evaluation and island-parallel search. |
| RNG | `rand_chacha::ChaCha8Rng` | Portable, seedable, reproducible across platforms. |
| Simplification / equivalence | `egg` (e-graphs) | Rewrite-based simplification and equivalence checks. |
| Server | `axum` + `tokio` (feature `serve`) | Small API, loopback only. |
| Asset embedding | `rust-embed` | UI ships inside the binary. |
| JSON Schema | `schemars` | Schema generated from the same Rust types that write the report. |
| Frontend | Vite + React + TypeScript + D3 | The UI is a static viewer plus a local API client; no SSR needed, so Next.js adds weight without benefit. |
| License | Apache-2.0 | Permissive, includes a patent grant. |

## 2. Workspace layout

```
Cargo.toml                    workspace, shared lints, profiles
crates/
  tarafold-core/              Dataset, Column, Split, Config, Evidence, errors, Rng helpers
  tarafold-io/                CSV ingest (sniffing, limits), CSV export (injection-safe)
  tarafold-expr/              Expr tree, eval, forward-mode AD, parse/print (infix, LaTeX, JSON), egg simplify
  tarafold-profile/           column statistics, histograms, outlier flags
  tarafold-relate/            correlations, MI, dCor, ridge, GBDT, permutation importance, H-statistic, screening
  tarafold-discover/          sparse basis regression, GP search, constant fitting, Pareto front, selection
  tarafold-validate/          metrics, bootstrap, stability, residuals, applicability domain, grading, evidence
  tarafold-optimize/          inverse search: DE + local refinement, 1-D root finding, constraints, diversity
  tarafold-synth/             equation registry, data generator, truth.json
  tarafold-bench/             bench runner, metrics, baseline comparison
  tarafold-report/            report types (schemars), JSON writer, HTML assembler
  tarafold-engine/            Pipeline, stage graph, cancellation, progress events
  tarafold-server/            axum API + embedded UI (feature-gated)
  tarafold-cli/               the `tarafold` binary (clap)
web/                          Vite + React + TS + D3 workspace
bench/baseline.json           committed aggregate metrics for regression checks
bench/reference/              dev-only PySR comparison (uv script, not shipped)
fuzz/                         cargo-fuzz targets (CSV, expression parser, constraint parser)
docs/                         user guide, algorithm notes, report schema docs
```

Dependency rule: `core ← expr ← {profile, relate, discover, validate, optimize, synth}
← report ← engine ← {server, bench} ← cli`. No crate below `engine` depends on I/O beyond
`io`. `server` and `cli` are the only crates that may print or open sockets.

## 3. Core data model (`tarafold-core`)

```rust
pub struct Dataset {
    pub columns: Vec<Column>,          // numeric only, same length
    pub n_rows: usize,
    pub source: SourceInfo,            // path, sha256, delimiter, excluded columns + reasons
}
pub struct Column { pub name: String, pub values: Vec<f64>, pub flags: ColumnFlags }

pub struct Split { pub train: Vec<u32>, pub val: Vec<u32>, pub test: Vec<u32>, pub strategy: SplitStrategy }
pub enum SplitStrategy { Random { seed: u64 }, Ordered }   // Ordered = time-ordered data, no shuffling

pub enum Evidence { Association, Predictive, FunctionalFit, InterventionalFit } // no Causal in v0.1 (R9.2)
pub enum DataSource { Observational, Experimental { designed: Vec<String> }, Simulation { designed: Vec<String> } }

pub struct Weights(Option<Vec<f64>>);  // None = unweighted; normalized to mean 1 over used rows
pub enum VarKind { Continuous, Integer, Step(f64) } // for controllables (R8.9)

pub struct RunConfig { /* every tunable; serde + TOML; defaults in one place */ }
```

`RunConfig` precedence: CLI flags > `--config` file > defaults. The resolved config is
written as `run.toml` so any run can be reproduced exactly.

Row indices are `u32` (10 M row cap). All numeric work is `f64`.

## 4. Expressions (`tarafold-expr`)

- **Representation:** flat postfix `Vec<Node>` (`Const(idx)`, `Var(idx)`, `Unary(op)`, `Binary(op)`)
  with a separate `Vec<f64>` of constants. Postfix is cache-friendly, cheap to mutate by
  subtree ranges, and simple to evaluate.
- **Evaluation:** column-wise over row chunks of 1,024: each node produces a chunk vector, so
  one pass per node instead of per-row interpretation. Chunks are processed in parallel; any
  reduction (loss sum) uses fixed chunk boundaries and pairwise summation in chunk order, so
  results are identical for any thread count.
- **Protected semantics:** `log(x)`, `sqrt(x)` are defined on `x > 0` / `x ≥ 0`; `÷` by |x| < 1e-12
  and `pow` with negative base and non-integer exponent yield NaN. Any NaN/inf on training
  rows discards the candidate (R6.8). No silent `abs()` wrapping, so reported equations mean
  what they say.
- **Derivatives:** forward-mode dual numbers over the same evaluator give ∂f/∂constants
  (for Levenberg–Marquardt) and ∂f/∂x (for optimization and sensitivity).
- **Printing:** infix with minimal parentheses, LaTeX, JSON tree (for the UI evaluator).
- **Code generation (`codegen` module, R16):** one backend per target behind a `Codegen` trait:
  Python (`math` only), JavaScript (`Math` only), SQL (ANSI `EXP`/`LN`/`POWER`/`SQRT`, with
  `CASE WHEN` guards so domain errors give `NULL`), Excel (named cells, `IF` guards giving
  `#NUM!`), LaTeX, JSON. Protected operations are emitted as small guard helpers so exported
  code keeps engine semantics. Identifiers are sanitized deterministically (non-alphanumerics →
  `_`, leading digit → `v_` prefix, collisions get `_2`, `_3`, and reserved words get `_` suffix);
  the mapping goes in the header comment. Python and JS output is executed against the golden
  vectors in CI (Python and Node are available on CI runners; neither is a runtime dependency).
- **Parsing:** a small Pratt parser for infix expressions (used by tests, `--equation`
  overrides, and `truth.json`). Never `eval`.
- **Simplification:** convert to an `egg` `RecExpr`, run a bounded rule set (commutativity,
  associativity, identity/annihilator, constant folding, `x*x → square(x)`,
  `exp(log x) → x` for x>0, power merging), extract the lowest-complexity term. Iteration and
  node limits keep it under ~5 ms per expression.
- **Complexity:** weighted node count (default weight 1 for all nodes; `exp/log/pow` weight 2).
  Configurable.

## 5. Pipeline (`tarafold-engine`)

```rust
pub struct Pipeline { config: RunConfig, events: Sender<ProgressEvent>, cancel: CancelToken }
impl Pipeline {
    pub fn run(&self, data: Dataset) -> Result<Report, EngineError>;
}
pub trait Stage { type In; type Out: Serialize; fn run(&self, ctx: &Ctx, input: Self::In) -> Result<Self::Out, StageError>; }
```

Each stage returns a typed, serializable artifact that goes into the report. Stages check
`cancel` at loop boundaries; on cancellation the engine assembles a partial report with
`status: interrupted` (R2.9). Timings and row counts per stage go into `report.run.stages`.

### Stage 1 — Ingest (`tarafold-io`)

1. Sniff delimiter (`, ; \t |`) from the first 64 KB by consistency of field counts; sniff
   decimal separator (`.` vs `,` when delimiter is `;`).
2. Stream records; enforce file size, row, and column limits before allocating (R3.6).
3. Per column: parse with `fast-float`; missing tokens → NaN; numeric if ≥ 95% of non-empty
   cells parse (R3.2).
4. Exclude non-numeric, constant, and duplicate columns, with reasons (R3.5).
5. Compute SHA-256 while streaming (R3.8). The input is any `Read`; path `-` means stdin, which
   is buffered to a temp file only if a second pass is needed (it is not for v0.1) (R3.9).
6. If `--weights` is set, validate the weight column (finite, ≥ 0, not all zero), remove it from
   features, and normalize weights to mean 1 over used rows (R3.10).

### Stage 2 — Profile (`tarafold-profile`)

Single pass per column for moments (Welford), then a sort-based pass for quantiles and
unique count. Histograms use 50 equal-width bins between the 0.5% and 99.5% quantiles with
overflow bins. Outliers: robust z = 0.6745·(x − median)/MAD > 5. Flags: integer-valued (all
values within 1e-9 of an integer), bounded-unit (all in [0,1]), all-positive.

### Stage 3 — Split

Default random 60/20/20 with the run seed; `--split ordered` keeps row order for time-ordered
data, where random splits leak information. The test split is held out until Stage 6.

### Multiple targets (R17)

The engine runs ingest, profile, and split once. For each target in the order given, it runs
stages 4–7 with that target and records the results under `report.targets[i]`. Per-target
stages are independent, so they reuse the same seed derivation `(run_seed, target_index)` and
stay deterministic. Other targets stay in the feature pool unless `--exclude-other-targets`;
when a selected equation uses another target, the report adds a `uses_other_target` warning.
Bootstrap resample indices in Stage 6 are drawn once per run and shared by all targets, so
ensemble member *b* of every target was fit on the same rows. That keeps errors that targets
share visible in joint goal probabilities (R18.7). Optimization across targets is joint
(Stage 7, multi-goal).

### Row weights

When weights `w` are set, every loss in the pipeline is the weighted version: weighted MSE in
GP, weighted least squares in OLS refits and elastic net (coordinate descent with weighted
residuals), weighted Levenberg–Marquardt, weighted R² (`1 − Σw r² / Σw (y − ȳ_w)²`), and
weighted bootstrap (resampling probability ∝ w is *not* used; rows are resampled uniformly and
keep their weights). Ridge and GBDT baselines are fit with the same weights. Reports include
both weighted and unweighted metrics (R6.10).

### Stage 4 — Relate (`tarafold-relate`)

| Output | Algorithm | Notes |
|---|---|---|
| Linear association | Pearson, Spearman (rank then Pearson) | All pairs. O(n·p²). |
| Nonlinear association | Mutual information, KSG estimator (k = 3) with a k-d tree (`kiddo`) | Feature–target, ≤ 5,000-row subsample. Reported in nats and as normalized `1 − exp(−2·MI)`. |
| Dependence | Distance correlation | Feature–target, same subsample. O(m²) memory-bounded by subsample. |
| Linear baseline | Ridge regression, λ by validation-set search over a log grid | Standardized features. Gives a "linear-is-enough" reference. |
| Nonlinear baseline | Histogram GBDT (in-house: 256 bins, depth ≤ 6, learning rate 0.05, early stopping on validation) | Deterministic. Estimates how much signal any model can find; the equation is compared against it. |
| Importance | Permutation importance on GBDT, validation set, 10 repeats → mean ± std | |
| Interactions | Friedman's H² for pairs among top-k (k = 8) features using GBDT partial dependence on a 500-row sample | |
| Collinearity | |r| > 0.95, VIF from ridge-free OLS on standardized features | |
| Leakage suspects | |r(x, y)| > 0.999 | Flagged, not removed. |

**Feature screening for discovery (R5.7):** rank by max(normalized permutation importance,
normalized MI). Keep features above 1% of the top score, up to `max_features` (12). When
two kept features are collinear (|r| > 0.98), keep the one with higher importance and record
the other as an alternate. Every decision is written to `relate.screening`.

### Stage 5 — Discover (`tarafold-discover`)

Two strategies run in sequence; both feed one Pareto archive.

**5a. Sparse basis regression (fast, deterministic; FFX/SINDy-style).**

1. Build a basis library from screened features: `x`, `x²`, `x³`, `1/x`, `√x`, `log x`
   (where valid on all rows), pairwise products `xᵢxⱼ`, and ratios `xᵢ/xⱼ` (where denominator
   is never near zero). Cap library at 400 terms by MI pre-ranking.
2. Standardize the library; run an elastic-net path (coordinate descent, α ∈ {0.95, 0.5},
   100 λ values).
3. For each distinct support along the path, refit unpenalized OLS on the raw basis → candidate.
4. **Log-log variant:** if `y` and all screened features are strictly positive, fit
   `log y = c₀ + Σ cᵢ log xᵢ` with the same sparse path. This recovers multiplicative and
   power laws (`revenue = visitors × conversion × price`, Cobb–Douglas) exactly, which are
   common in business data and hard for additive models.
5. **Log-target variant:** if `y > 0`, fit `log y` on the linear library to catch exponentials.

**5b. Genetic-programming symbolic regression.**

- **Populations:** 16 islands × 100 expressions; initial populations from ramped half-and-half
  random trees (depth 2–5) plus all 5a candidates converted to trees.
- **Evolution:** regularized evolution. Each step: tournament (size 10, picking the best by
  penalized loss with p = 0.9), mutate or crossover, replace the oldest member.
- **Mutations:** change constant (multiplicative/additive jitter), change operator, change
  variable, insert node, delete node, replace subtree with random, crossover (subtree swap),
  simplify, re-fit constants.
- **Loss:** normalized MSE on the training subsample (MSE / Var(y)), i.e. 1 − R².
- **Parsimony:** adaptive: penalty per complexity level ∝ how many population members
  already occupy that level, so search keeps exploring underpopulated sizes (same idea as PySR).
- **Constant fitting:** Levenberg–Marquardt with forward-mode Jacobian, ≤ 20 iterations, on
  every new expression with probability 0.2 and always before entering the hall of fame.
- **Hall of fame:** best expression per complexity level across islands.
- **Migration:** every 50 steps per island, each island receives its ring neighbor's top 5
  and 2 random hall-of-fame members.
- **Determinism:** search runs in epochs. In each epoch every island evolves independently
  with its own `ChaCha8Rng` seeded from `(run_seed, island_id, epoch)`. Migration and
  hall-of-fame merges happen between epochs in island-index order, with ties broken by
  `(complexity, loss, canonical string)`. Output is identical across thread counts when
  bounded by `--max-evals`. A `--time-budget` stops at an epoch boundary and is
  reproducible only if the same number of epochs completes.
- **Budget:** default `max_evals = 2_000_000` expression evaluations on ≤ 5,000 rows, with
  `--time-budget` default 90 s.

**5c. Pareto front and selection (R6.4, R6.7).**

1. Merge 5a and 5b candidates; simplify; dedupe by canonical form.
2. Compute validation loss; keep the Pareto front over (complexity, validation loss), max 20.
3. Selection: let `L*` be the best validation loss. Eligible = front members with
   `L ≤ L* + max(0.1·L*, 1e-6)`. Among eligible, sorted by complexity, pick the one with the
   largest score `−Δlog(L)/Δcomplexity` relative to its predecessor on the front. If only one
   is eligible, pick it. The user can override with `--select <id>`.

### Stage 6 — Validate (`tarafold-validate`)

| Check | Method |
|---|---|
| Accuracy | R², RMSE, MAE, NRMSE (RMSE / std(y)) on train/val/test for every front member. |
| Constant uncertainty | Bootstrap 200× on train ∪ val: refit constants of the fixed structure with LM; 2.5/97.5 percentiles. |
| Prediction intervals | Bootstrap ensemble predictions + residual quantiles (from validation residuals) → 90% interval. Coverage on test is reported. |
| Structural stability | 20 bootstrap resamples × reduced search (10% budget); fraction whose selected equation is equivalent to the original (e-graph equivalence, else numeric equivalence on 1,000 in-domain points with max rel. error < 1e-3). |
| Residual diagnostics | Spearman(|residual|, prediction) for heteroscedasticity; Pearson(residual, prediction); top 20 residual rows. |
| Baseline comparison | Test R² vs. ridge and GBDT. |
| Applicability domain | Per-feature [min, max] on train plus Mahalanobis distance (regularized covariance) with threshold at the 99th percentile of training distances. |

**Reliability grade (R7.7):**

- `high`: test R² ≥ 0.95, stability ≥ 0.7, test R² ≥ best baseline − 0.02, n_train ≥ 20 × n_constants, no leakage suspect among used variables.
- `medium`: test R² ≥ 0.8 and stability ≥ 0.4.
- `low`: otherwise. Every grade lists the conditions that passed and failed.

**Evidence level (R9.1, R9.4):**

- `association`: correlation/MI results.
- `predictive`: a model or equation with test R² ≥ 0.5 that beats the mean predictor.
- `functional_fit`: selected equation graded `medium` or `high`.
- `interventional_fit`: as `functional_fit`, plus data source `experimental` or `simulation`
  and every controllable in the designed set. The report records that this rests on the user's
  declaration (`evidence_basis: "user_declared_design"`) and applies inside the applicability
  domain only.

For `experimental` data the designed set defaults to the controllables; for `simulation` it
defaults to all features. A warning is added when a designed column correlates with another
designed column at |r| > 0.5, since that suggests the design was not independent.

### Stage 7 — Optimize (`tarafold-optimize`)

**Problem.** Controllable vector `x_c` with bounds `[l, u]`; non-controllables fixed at
baseline `b`; linear constraints `A·x_c ≤ c`; optional max-change limits (turned into bounds).
Minimize:

```
J(x_c) = ((f(x) − y*) / s_y)²                 # reach the target
       + α · Σ ((x_c,i − b_i) / s_i)²          # prefer small changes   (α = 1e-3)
       + β · max(0, D_M(x) − D_99)²            # stay in the data       (β = 1)
```

with `s` = training standard deviations and `D_M` the Mahalanobis distance.

**Algorithm.**

1. **One controllable:** exact. Scan 1,000 grid points over `[l, u]`, find sign changes of
   `f(x) − y*`, refine each with Brent's method. Returns all roots, not one.
2. **Several controllables:** differential evolution (DE/rand/1/bin, population 15·d,
   F = 0.7, CR = 0.9, seeded), bounds by reflection, linear constraints by an increasing
   penalty. Then refine the best 20 distinct points with projected Gauss–Newton on the
   target residual using ∂f/∂x from AD.
3. **Feasibility:** every candidate is checked exactly against bounds and constraints; violators
   are dropped.
4. **Ensemble scoring:** each candidate is evaluated with the bootstrap ensemble from Stage 6 to
   get mean prediction and a 90% interval.
5. **Ranking key:** `(feasible, in_domain, |ŷ_mean − y*| + κ·ŷ_std, change size)` with κ = 1.
6. **Diversity:** greedy selection; accept a candidate only if its standardized distance to all
   accepted ones exceeds 0.25. Return up to N = 5.
7. **No solution:** report the closest achievable value and the binding bounds/constraints
   (those active at the optimum within 1e-6) (R8.6).
8. **Sensitivity:** ∂f/∂xᵢ and elasticity (∂f/∂xᵢ · xᵢ/f) at each candidate (R8.8).

**Integer and step controllables (R8.9).** Bounds are first rounded inward to the nearest valid
values. Work is done in grid units `k = (x − l)/step` (step = 1 for integers).

- *One controllable:* if the grid has ≤ 100,000 points, evaluate all of them exactly and keep
  the best; otherwise run the continuous root finder and check the two grid points around each root.
- *Several controllables:* DE runs on continuous relaxations; each trial vector is rounded to
  the grid before evaluation, so the population only scores valid points. Gauss–Newton
  refinement is replaced by a local search over the ±1 grid neighborhood of each candidate
  (coordinate moves, accepted while `J` improves). Mixed continuous/discrete problems refine
  continuous coordinates with Gauss–Newton while holding discrete ones fixed, alternating
  with the discrete neighborhood search for up to 5 rounds.
- Every returned candidate is checked to lie exactly on its grid (within 1e-9 · step).

**Multi-goal optimization (R18).**

*Goals.* Parsed by the same dedicated grammar family as constraints:
`goal := [max:|min:] col | col (= | >= | <=) number | col = number..number`, with an optional
`:weight` suffix for soft goals. Each goal resolves to a target column with a selected equation.

*Consistent predictions across targets.* Build a dependency graph: target `a → b` when b's
selected equation uses a. Topologically sort it (cycle → exit code 2 naming the cycle). At a
candidate input, evaluate targets in that order and substitute each predicted value into the
equations that use it. Derivatives follow the chain rule through forward-mode AD. Ensemble
member *b* of each target is composed only with member *b* of its inputs.

*Formulation.* With hard goals `H`, soft non-objective goals `S`, and objectives `O`:

- Each hard goal becomes a constraint `g_h(x) ≤ 0` on the scaled violation, e.g. `(v − ŷ)/s_y`
  for `ŷ ≥ v`, `|ŷ − v|/s_y − tol` for exact goals.
- Each soft non-objective goal adds `w · max(0, g(x))²` to the scalar penalty `P(x)`, together
  with the small-change and domain terms from `J` above.
- Each objective `o` gives `±ŷ_o(x)/s_o` (minimized).

*Algorithm by number of objectives |O|:*

| |O| | Method |
|---|---|
| 0 | Minimize `P(x)` subject to `H`, using the single-goal machinery (DE + refinement; 1-D exact when one controllable). |
| 1 | Minimize `objective + P(x)` subject to `H`, same machinery. |
| 2–3 | NSGA-II (population 100·|O|, SBX crossover η = 15, polynomial mutation η = 20, seeded), objectives `O ∪ {P}` merged as `o + P` per objective so penalties do not create an extra axis. |
| 4–10 | NSGA-III with Das–Dennis reference directions (two-layer for |O| ≥ 6), same operators. |
| > 10 | Rejected with exit code 2 and a suggestion to turn objectives into thresholds. |

Constraints use Deb's feasibility rules (feasible beats infeasible; among infeasible, smaller
total violation wins). Integer and step controllables use the same rounding-before-evaluation
approach as above. All randomness is seeded per generation from the run seed, and populations
are processed in index order, so results are deterministic.

*Post-processing.*

1. Re-check every front member exactly against bounds, constraints, grids, and hard goals.
2. Score with the ensemble: per target mean and 90% interval; joint probability of all hard
   goals = fraction of ensemble members *b* for which every hard goal holds (R18.7).
3. With `--robust p`, drop members below `p` (R18.8).
4. Reduce to at most 20 by crowding distance (NSGA-II) or reference-direction niches (NSGA-III).
5. Mark: *knee* (maximum distance to the hyperplane through the per-objective extremes after
   normalization), *best per objective*, *most likely to meet all goals*, and, when weights or
   `--priority lexicographic` are given, the *preferred* candidate (weighted Tchebycheff on
   normalized objectives, or lexicographic with 1% tolerance per level) (R18.5, R18.6).
6. Redundant objectives: Spearman rank correlation > 0.9 across the final set (R18.10).

*Infeasibility (R18.9).* If no feasible candidate exists, find a minimal conflicting subset by
deletion filtering: drop one hard goal at a time and re-run a reduced-budget search; a goal is
kept in the subset only if removing it restores feasibility. Then, for each goal in the subset,
report its best achievable value with the other hard goals held. Cost is bounded by
`|H| + |subset|` reduced searches at 10% budget each.

### Stage 7b — Suggest next experiments (`tarafold-optimize::suggest`, R19)

Suggestions reuse the composed equations, ensemble, domain model, and constraint handling.
The search space is the controllables (or the designed columns for experimental and
simulation data), with bounds widened by `--suggest-expand` and labeled outside-domain where
they exceed observed ranges.

*Uncertainty model.* The ensemble combines two sources:

- **Constant uncertainty:** bootstrap refits of the selected equation (Stage 6).
- **Structure uncertainty:** the top 5 Pareto-front equations by validation loss within 2× the
  best, each with its bootstrap refits, weighted by `exp(−n_val · L / 2)` normalized
  (a BIC-like weight from validation loss).

*Acquisition scores* at a point `x`:

- **Explore:** weighted ensemble variance of every target's prediction, normalized by `s_y²`
  and summed over targets. The structure part of the variance is reported separately as
  *equation discrimination*, the points where competing equations disagree most
  (Box–Hill-style discrimination).
- **Exploit:** with hard goals only, the ensemble probability that all hard goals hold. With
  objectives, the ensemble probability that `x` would be non-dominated against the current
  observed (or predicted) front, computed per ensemble member. This is a sample-based
  probability of improvement that needs no Gaussian assumption.
- **Balanced:** a batch of N is split `⌈N/2⌉` exploit and `⌊N/2⌋` explore (explore when no goals,
  R19.3). The split is reported.

*Batch selection.* Maximize the acquisition with DE (multi-start, seeded). Pick greedily: after
choosing a point, multiply the acquisition of nearby points by the local penalty
`1 − exp(−d²/(2r²))` with `r = 0.15` in standardized units. Reject points within 0.15 of
existing rows or earlier picks (R19.4). Exact grids and constraints apply as in optimization.

*Output.* Each suggestion records values, mode, reason code (`uncertainty`,
`equation_discrimination`, `goal_likelihood`, `improvement`), the acquisition value, per-target
predictions with intervals, and joint goal probability. `suggestions.csv` copies the input
header: suggested columns are filled, non-controllables are set to their fixed values, and
target columns are left blank, so the user fills in results and appends the rows (R19.7).

*Out of scope for v0.1:* Gaussian-process surrogates and expected hypervolume improvement.
The ensemble approach is chosen because it uses the discovered equations directly and has no
kernel hyperparameters to tune. GP surrogates are on the roadmap as an alternative.

`--constrain` expressions are parsed by a dedicated grammar
(`linear_expr (<= | >= | =) number`), resolved against column names, and rejected if
non-linear (R14.2).

### Stage 8 — Report (`tarafold-report`)

`report.json` top-level shape (schema version `0.1.0`):

```
{
  "schema_version", "tarafold_version", "run": { id, started_at, config, seed, status, stages[] },
  "dataset": { source, n_rows, columns[], excluded[], missing, weights, data_source },
  "profile": { columns[] },
  "targets": [ {
    "target",
    "relate": { pearson, spearman, mi, dcor, baselines, importance, interactions, collinearity, leakage, screening },
    "discover": { front[], selected_id, strategies, budget_used },
    "validate": { per_candidate[], selected: { bootstrap, stability, residuals, coverage, grade, evidence, evidence_basis }, domain }
  } ],
  "dependencies": { edges[], order[] },
  "optimize": null | { goals[], method, controllables, kinds, bounds, constraints, baseline,
                       candidates[ { id, inputs, predictions{}, goals_met{}, joint_probability, marks[] } ],
                       redundant_objectives[], conflict: null | { subset[], best_achievable{} }, binding, disclaimer },
  "suggest": null | { mode, split, ensemble: { structures[], weights[] },
                      suggestions[ { id, inputs, mode, reason, acquisition, predictions{}, joint_probability, in_domain } ], note },
  "warnings": [ { code, message, target, evidence_impact } ]
}
```

`suggestions.csv` is written alongside `candidates.csv` when `--suggest` is set.

`report.html` is built from the single-file UI bundle with the report JSON injected into a
`<script type="application/json" id="tarafold-report">` tag. JSON is escaped for `</script>`.
CSV exports prefix text cells that start with `= + - @` with `'` (R14.3).

All user-facing text comes from templates in one module, with separate template sets for
observational and interventional evidence. A unit test fails if any observational template
contains banned causal phrasing (`causes`, `drives`, `will increase`, `will decrease`, `leads to`),
and if any interventional template omits the "user-declared design" and "inside the data
domain" qualifiers (R9.2, R9.6).

## 6. Synthetic datasets and benchmark

### 6.1 Registry (`tarafold-synth`)

Each entry: name, tier, truth expression (infix), variable sampling distributions, valid
domain, controllables, and an example goal.

| Tier | Examples |
|---|---|
| T1 linear | `3x₁ − 2x₂ + 5`; 5-var linear with one zero coefficient |
| T2 polynomial | `x₁² + 2x₁x₂ − x₂`; `x³ − x` |
| T3 rational | `x₁ / (1 + x₂)`; `(x₁ + x₂)/(x₁ x₂)` |
| T4 power law | `2.5 x₁^0.7 x₂^0.3` (Cobb–Douglas); `x₁^1.5 / x₂` |
| T5 exp/log | `10 e^{−0.5 x₁} + log x₂`; `x₁ e^{x₂/10}` |
| T6 interactions + distractors | `x₁x₂ + x₃` with 5 noise columns; `sin`-free saturating `x₁/(x₁ + x₂)` |
| T7 physics (Feynman subset) | `G m₁ m₂ / r²`, `½ m v²`, `2π √(L/g)`, `P V / (n R)`, `q₁ q₂ / (4π ε r²)` |
| T8 business | `visitors = 120 · spend^0.6`; `conv = 0.08 · e^{−0.03·price}`; `orders = visitors · conv`; `revenue = orders · price`; `profit = revenue − 0.4·revenue − spend` (multi-column causal chain, so intermediate columns are present as features too; also used for multi-target runs) |
| T9 weighted | Heteroscedastic versions of T1/T4 where noise std varies per row and a `weight = 1/σ²` column is provided; checks that weighting improves constant accuracy |
| T10 integer controls | `output = 40 · machines^0.8 · hours^0.5` with `machines` integer in 1..20 and `hours` in steps of 0.5; checks R8.9 |
| T11 multi-goal | Known-front problems built from closed-form targets over shared inputs: 2-objective (convex and concave fronts), 3- and 5-objective DTLZ2-style targets, plus T8 with `profit>=…`, `max:revenue`, `min:spend`. Each entry ships its true Pareto front sampled at 10,000 points. |
| T12 active loop | T4, T5, T7, T8, T11 entries run as closed loops: start with 20 rows, ask for 5 suggestions per round, "run the experiment" by evaluating the true equation plus noise, append, repeat up to 10 rounds. |

At least 25 equations. Variants per equation: rows ∈ {200, 2,000}, Gaussian noise with std
∈ {0, 0.01, 0.1} × std(y), distractors ∈ {0, 5}, and a collinear-duplicate variant (feature
plus 1% noise copy).

### 6.2 Metrics (`tarafold-bench`)

| Metric | Definition |
|---|---|
| Symbolic recovery | Selected equation equivalent to truth: e-graph equality after rounding constants to 3 significant figures, or numeric equivalence (max rel. error < 1e-3 on 1,000 in-domain points) using the same variable set. |
| Accuracy solution | Test R² > 0.999 (noiseless) — same definition as SRBench. |
| Oracle gap | Oracle R² (truth on noisy test) − selected test R². |
| Extrapolation R² | R² on points sampled from the domain widened by 50% on each side. |
| Variable selection | Precision / recall of used variables vs. truth variables. |
| Complexity ratio | complexity(selected) / complexity(truth). |
| Inverse success | Fraction of returned optimization candidates with `|f_true(x) − y*| ≤ 1%` of y range, for goals sampled inside the achievable range. |
| Weighting gain | On T9: relative constant error unweighted ÷ weighted (target > 1). |
| Export fidelity | Max relative error of exported Python/JS vs. engine on golden vectors. |
| Hypervolume ratio | On T11: hypervolume of the returned feasible set (evaluated with the true functions) ÷ hypervolume of the true front, using a shared reference point. |
| Goal truth rate | Fraction of returned multi-goal candidates whose hard goals all hold under the true functions. |
| Joint-probability calibration | Among candidates with reported joint probability in bins of 0.1, the observed fraction meeting all goals under truth + noise. |
| Experiments to goal | On T12: median number of added rows until a candidate meets all goals under truth, vs. random and Latin-hypercube sampling with the same budget. |
| Experiments to recovery | On T12: median number of added rows until the selected equation is symbolically equivalent to truth, vs. the same baselines. |
| Interval coverage | Share of test points inside 90% prediction intervals (target 0.85–0.95). |
| Cost | Wall time, peak RSS, evaluations used. |

**v0.1 acceptance thresholds** (aggregate, default settings, 2,000 rows):

- Noiseless T1–T5, T8: symbolic recovery ≥ 70%, accuracy solution ≥ 90%.
- 10% noise, all tiers: median oracle gap ≤ 0.02.
- With 5 distractors: variable-selection precision ≥ 0.9.
- Inverse success ≥ 90% on equations recovered at accuracy-solution level.
- Interval coverage between 0.80 and 0.97 on 10% noise.
- Weighting gain > 1.2 on T9; inverse success ≥ 90% on T10 with all candidates on-grid.
- Export fidelity ≤ 1e-9 relative error for Python and JS.
- T11: hypervolume ratio ≥ 0.9 for 2–3 objectives and ≥ 0.75 for 5 objectives; goal truth rate ≥ 0.9; calibration error ≤ 0.1 per bin.
- T12: suggestions reach the goal in ≤ 60% of the experiments random sampling needs (median), and never worse than Latin-hypercube sampling on aggregate.
- No regression > 3 percentage points vs. `bench/baseline.json` on any aggregate metric.

**Smoke bench (CI):** 8 equations, noiseless + 10% noise, 500 rows, `--max-evals 200000`,
plus one 2-objective T11 problem and one 3-round T12 loop, target < 10 min on GitHub-hosted runners.
The full active suite runs with `tarafold bench --suite active`.

**Reference comparison (dev only):** `bench/reference/pysr_compare.py` runs PySR on the same
CSVs via `uv run` and writes the same metrics, so we can track the gap to the state of the
art. It is never a runtime dependency.

## 7. CLI (`tarafold-cli`)

```
tarafold run <csv|-> --target <col>[,<col>...] [--goal <v> | --goal a=1,b=2] [--control a,b] [--fix c=1] \
    [--require "a>=100"]... [--prefer "max:b:3"]... [--priority weighted|lexicographic] [--robust 0.9] \
    [--suggest 8] [--suggest-mode explore|exploit|balanced] [--suggest-expand 10%] \
    [--constrain "a + b <= 5000"] [--bounds a=0..100] [--max-change a=20%] \
    [--integer a] [--step b=0.5] [--weights w] \
    [--data-source observational|experimental|simulation] [--designed a,b] [--exclude-other-targets] \
    [--baseline median|row:<i>|a=1,b=2] [--split random|ordered] [--select <id>] \
    [--seed 42] [--max-evals N] [--time-budget 90s] [--threads N] \
    [--config run.toml] [--out ./tarafold-out] [--format human|json] [--open]
tarafold export <report-dir> --to python|js|sql|excel|latex|json [--target <col>] [--id <n>] [--out file]
tarafold profile <csv>
tarafold relate <csv> --target <col>
tarafold discover <csv> --target <col>
tarafold validate <report-dir>            # recompute validation for a saved front
tarafold optimize <report-dir> [--goal|--require|--prefer ...] --control ...
tarafold suggest <report-dir> -n 8 [--suggest-mode ...] [--require|--prefer ...] [--out suggestions.csv]
tarafold synth <name|--list> [--rows] [--noise] [--distractors] [--seed] --out <dir>
tarafold bench [--suite smoke|full|active] [--baseline bench/baseline.json] [--write-baseline]
tarafold serve <report-dir|csv> [--port 0] [--host 127.0.0.1] [--no-open]
tarafold schema                            # print report JSON Schema
tarafold report <report-dir> --html        # rebuild report.html
```

Human output after `run` (stderr progress, stdout summary):

```
Dataset   data.csv  2,000 rows × 7 numeric columns (1 excluded: "region", non-numeric)
Target    revenue
Selected  revenue = orders × price                        R²(test) 0.9999  grade: high
Front     5 candidates  (see report.html)
Goal      revenue = 1,000,000  →  3 feasible candidates (in data domain)
  #1  spend 41,200 (+18%)  price 34.10 (−3%)   ŷ = 1,000,310 [987k, 1.013M]
Note      Fits on observational data describe association, not the effect of intervening.
Wrote     ./tarafold-out/report.html
```

Exit codes per R2.6. Errors map: `ConfigError → 2`, `DataError → 3`, `NoValidCandidate → 4`,
`NoFeasibleCandidate → 5`, everything else → 1.

## 8. Server and workspace

**Server (`tarafold-server`, feature `serve`, on by default in release builds).**

| Endpoint | Purpose |
|---|---|
| `GET /` and assets | Embedded UI. |
| `GET /api/report` | Current report. |
| `POST /api/run` | Re-run pipeline with a new config (target, controllables, budget). Runs in a background task; progress via SSE `GET /api/events`. |
| `POST /api/optimize` | Re-run only Stage 7 with new goals/constraints (fast path for sliders). |
| `POST /api/suggest` | Re-run only Stage 7b with new mode, batch size, or goals. |
| `POST /api/evaluate` | Evaluate the selected equation and ensemble at given points. |

Security (R14.4): binds `127.0.0.1` by default; a random 128-bit token is generated at start,
printed in the URL fragment, and required as `Authorization: Bearer` on `/api/*`; `Host` and
`Origin` headers must match the bound address (DNS-rebinding defense); request body limit 1 MB;
one pipeline run at a time.

**Workspace (`web/`).** Views:

1. **Overview** — dataset card, excluded columns, per-variable histograms with flags.
2. **Explore** — scatter matrix (canvas for > 5k points), 2D scatter with brushing, correlation/MI heatmap.
3. **Relationships** — importance bars with error bars, interaction heatmap, screening table.
4. **Equations** — Pareto front scatter (click to select), equation card (LaTeX via KaTeX, constants ± CI, grade, evidence), predicted-vs-actual, residual plots, partial-dependence curves, 2D response surface heatmap for any two variables.
5. **Target** — goal builder (require/prefer, weights, priority), controllable toggles, bound sliders, constraint editor, candidate table with goal checklist and joint probability, trade-off view (2D/3D scatter for 2–3 objectives; parallel coordinates with brush filters for more), knee and best-per-objective marks, redundant-objective notes, conflict explanation when infeasible, in-domain badges, sensitivity bars.
6. **Next experiments** — mode and batch-size controls, suggestions table with reasons, suggestions plotted over existing data in the scatter views, equation-disagreement map for two chosen inputs, `suggestions.csv` download.
7. **Reliability** — grade checklist, stability, coverage, baseline comparison, warnings.

The browser evaluates equations locally from the JSON tree for interactive sliders. A shared
golden-vector file (`crates/tarafold-expr/tests/golden.json`) is checked by both Rust and
TypeScript tests so the two evaluators cannot drift.

Accessibility: keyboard navigation for all controls, ARIA labels on charts, data tables as
alternatives to every chart, Okabe–Ito palette.

## 9. Error handling and logging

- Library crates: `thiserror` enums per crate; no panics on user data (enforced by clippy
  `unwrap_used`/`expect_used` = deny outside tests).
- CLI: `anyhow` for context, mapped to exit codes.
- Logging: `tracing`; `-v/-vv` for levels; `--log-file`. No data values are logged above `debug`.

## 10. Testing strategy

| Layer | Tests |
|---|---|
| Unit | Every stage and algorithm (e.g. KSG MI vs. analytic Gaussian MI, ridge vs. closed form). |
| Property (`proptest`) | Simplification preserves values; AD derivative ≈ finite differences; parse(print(e)) == e; CSV round-trip. |
| Golden | Rust/TS evaluator vectors; report snapshots (`insta`) on fixed seeds. |
| Determinism | Same seed, `--threads 1` vs `--threads 8` → byte-identical `discover`, `optimize`, and `suggest` output. |
| Integration | `tarafold run` on synthetic CSVs via `assert_cmd`; exit codes; JSON-only stdout. |
| Schema | `report.json` from integration runs validates against `tarafold schema`. |
| Fuzz | `cargo-fuzz` targets for CSV ingest, infix parser, constraint parser, goal parser (run weekly in CI, 10 min each). |
| Bench | Smoke suite in CI; full suite on demand and before release. |
| Web | Vitest for components and the evaluator; Playwright smoke test against `tarafold serve` on a synthetic report. |

## 11. Build, release, and size

- Release profile: `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "abort"` for
  the binary. Target binary size < 40 MB.
- The web bundle is built first (`pnpm --dir web build`) and embedded via `rust-embed`. If
  `web/dist` is missing, a placeholder page is embedded and `serve` prints how to build the UI,
  so `cargo build` works without Node.
- Targets: `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
  `x86_64-apple-darwin`, `aarch64-apple-darwin`.
- Release artifacts: archives, `SHA256SUMS`, CycloneDX SBOM, GitHub build-provenance attestations.

## 12. Risks and limitations

| Risk | Impact | Mitigation |
|---|---|---|
| In-house SR weaker than PySR | Lower recovery rate | Sparse-basis + log-log stages catch common forms cheaply; PySR reference comparison tracks the gap; GP improvements are measured by the bench. |
| Search cost grows with features | Slow or poor results on wide data | Screening to ≤ 12 features, subsampling, evaluation budget, epoch-level time limits. |
| Non-identifiability (collinear inputs, equivalent forms) | Multiple equally good equations | Report alternates, stability score, collinearity warnings; never present one equation as "the truth". |
| Overfitting noise | Spurious complex equations | Validation-based selection, parsimony, baseline comparison, held-out test, stability. |
| Extrapolation in optimization | Recommended inputs outside data support | Applicability domain penalty, labeling, ranking below in-domain. |
| Causal misreading | Users act on correlations | Evidence levels, disclaimers, banned-phrase test, no `causal` level in v0.1. |
| Wrong `--data-source` declaration | Observational data labeled `interventional_fit` | Label records it as user-declared; correlated-design warning; docs explain what qualifies as designed. |
| Exported code drifts from engine | Users get different numbers in their tools | Golden-vector execution tests for Python/JS; guard helpers for protected ops. |
| Float non-determinism under parallelism | Irreproducible runs | Epoch-synchronous islands, per-island RNG, fixed-order reductions, determinism test. |
| Time-series data with random splits | Over-optimistic validation | `--split ordered`; warning when target autocorrelation at lag 1 > 0.8. |
| Penalty-based constraint handling misses feasible regions | Missed solutions | Exact feasibility check, multiple DE restarts, report binding constraints. |
| Binary size and compile time (axum, egg, faer) | Slower CI, bigger downloads | Feature flags, LTO, strip; measure size in CI. |
| Rust/TS evaluator drift | UI shows numbers that differ from the report | Shared golden vectors tested in both. |
| Errors compound through chained equations | Joint predictions worse than each fit alone | Shared-resample ensembles carry errors through the chain; joint-probability calibration is benchmarked. |
| Many objectives make fronts hard to use | Users get hundreds of near-equal options | NSGA-III niches, cap of 20 returned, knee/preferred marks, redundancy detection, cap of 10 objectives. |
| Suggestions explore where the equations are wrong in shape, not just uncertain | Wasted experiments | Structure ensemble across competing equations; benchmark against random and Latin-hypercube sampling; `explore` reason codes shown to users. |
| Suggested settings unsafe in the real world | Harm in labs or plants | Suggestions stay inside bounds/constraints and observed ranges by default; widening needs `--suggest-expand` and is labeled. |

**Out of scope for v0.1:** categorical variables, lagged features and differential/dynamical
equations, causal discovery, units/dimensional analysis, Gaussian-process surrogates and
expected hypervolume improvement, Parquet input, Python bindings, GPU, hosted service and accounts.

## 13. Roadmap after v0.1

- v0.2: Categorical columns as 0/1 indicators (low cardinality) with per-group constants;
  lagged features (`--lags x:1,2`) with ordered splits enforced; Parquet input; units-aware
  search (units in header, dimensional consistency as a constraint); Python bindings
  (PyO3 + maturin, `pip install tarafold`).
- v0.3: Causal assistance with explicit assumptions (DAG input, constraint-based discovery as hypotheses); suggestions designed specifically to test whether an intervention works (randomized designs); Gaussian-process surrogates as an alternative uncertainty model for suggestions.
- v0.4: Dynamical systems (SINDy on time derivatives); integrations with lab automation for fully closed loops.
- v0.5 (experimental): quantum and hybrid engines behind `--engine` (R20); see §14.

## 14. Quantum and hybrid engines (future, R20)

v0.1 defines the engine interface and the `--engine` flag; only `classical` ships.

**Engine interface.** `trait Engine { fn discover(&self, ctx, data) -> DiscoverOutput;
fn optimize(&self, ctx, problem) -> OptimizeOutput; fn capabilities(&self) -> Caps; }`. The
classical implementation wraps the stages in §5. Output types are shared, so Stage 6
validation, grading, evidence levels, and the report are identical for all engines.

**Where quantum methods could plausibly fit** (each is a combinatorial sub-problem that can be
written as a QUBO, i.e. minimizing `xᵀQx` over binary `x`):

| Sub-problem | QUBO form | Engine |
|---|---|---|
| Sparse basis selection (5a): pick ≤ k of 400 basis terms | binary inclusion variables; Q from the Gram matrix and `Xᵀy`, plus a cardinality penalty | quantum annealing or QAOA |
| Integer / step controllables in optimization (R8.9) | binary encoding of grid levels; Q from a local quadratic model of `J` | quantum annealing or QAOA |
| Diverse batch selection for suggestions (R19.4) | maximize acquisition minus pairwise similarity | quantum annealing or QAOA |

- **`quantum`:** solves those QUBOs on a quantum backend (a local state-vector simulator by
  default, remote hardware only with an opt-in feature and provider flag); continuous parts
  (constant fitting, validation) stay classical, because they cannot run otherwise.
- **`hybrid`:** classical GP search and constant fitting, with QUBO sub-problems sent to the
  quantum backend, and a classical solve of the same QUBO kept as a fallback and reference.
- **Reference solver:** every QUBO is also solved classically (simulated annealing / exact for
  small sizes), so `bench --compare` measures quantum against the same formulation, not against
  a different algorithm.

**Expectations.** At v0.1 problem sizes (≤ 400 binary variables), classical solvers are
expected to match or beat current quantum hardware. The point of R20 is to measure that
honestly as hardware improves; a quantum engine becomes a recommended default only if the
comparison bench shows better metrics at equal or lower cost.
