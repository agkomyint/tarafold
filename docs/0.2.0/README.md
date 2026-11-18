# Tarafold 0.2.0: plan

Status: proposal. Nothing here is built yet. Every item names the problem it fixes, how we
would know it worked (a benchmark number), and roughly how big it is.

Sources for this plan: what v0.1 measured (`docs/benchmarks.md`), the gaps found while
building the case studies and the public-data runs (Kepler from NASA exoplanets, E = mc²
from AME2020), and the roadmap in `.kiro/specs/tarafold-v0-1/design.md` §13.

## The questions this plan answers

### Should Tarafold add more prediction systems?

Not as the main output. Tarafold's value is an equation a person can read and check. Many
tools already give black-box predictions, and they do it well. What is worth adding:

- **Equation + correction.** When a compact equation explains most of the data but not all,
  model what is left (the residual) separately and report how much it adds. The user keeps
  the readable equation and learns how much is unexplained. This also covers "I already have
  a theory, find what it misses" (`--baseline "<expression>"`).
- **A usable fallback when no formula exists.** The gradient-boosted baseline already runs
  in every analysis. Make it exportable and usable for optimization when the equation is
  graded `low`, clearly labelled as a black box.

### Should it use more compute?

Some, where compute is the bottleneck, and only with a measured payoff. In v0.1 most misses
are not caused by too little search:

| Miss | Cause | Fix |
|---|---|---|
| Noisy exponentials replaced by approximations (T5 64% structure) | Approximations fit the noise just as well | Better selection and units, not more search |
| Star-mass exponent −0.463 instead of −0.5 (Kepler) | Errors in the input values pull exponents toward zero | Errors-in-variables fitting |
| Raw exoplanet periods needed a manual log transform | Large values dominate squared errors | Automatic scale choice |
| Lennard-Jones, resonance (research notes) | Sharp features, search does not find them | More search + new operators: compute helps here |

So: add an effort setting that scales the search, and measure what it buys.

- `--effort quick|default|thorough|max`: scales GP evaluations (×0.25 to ×20), islands,
  restarts, and stability reruns. `max` is for overnight runs on a workstation.
- Publish a budget curve in the benchmark (structure recovery against evaluations), so users
  can see where more compute stops helping. A first version exists as
  `crates/tarafold-discover/examples/gp_budget.rs`.
- No GPU work in 0.2. Evaluation is already parallel on CPU cores; GPU kernels would add a
  large dependency for an unproven gain.

### How do we make accuracy higher?

In order of expected impact:

1. **Units and dimensional analysis.** Optional units per column (`--units mass=kg,v=m/s`).
   Reject equations that add quantities of different dimensions; optionally search over
   dimensionless groups (Buckingham π). This is the strongest known prior in physics and
   prunes most wrong candidates.
   *Measure:* structure recovery on T5 and T7 with 10% noise; target T5 ≥ 80% (now 64%).
2. **Automatic scale choice.** When the target spans orders of magnitude, also fit in log
   space and compare on relative error, so users do not need to transform by hand.
   *Measure:* Kepler on raw exoplanet data recovered without a manual log step.
3. **Errors in inputs.** An option to account for measurement error in the inputs
   (orthogonal-distance or total least squares), so exponents are not biased toward zero.
   *Measure:* star-mass exponent within 0.02 of −0.5 on the exoplanet data.
4. **Better grade calibration.** v0.1 grades 40% of wrong structures `high` (6 of 15, all
   close approximations). Add a check that compares the selected equation with simpler
   neighbours on held-out ranges, and use units when given.
   *Measure:* `high` given to wrong structures ≤ 20% while correct ones stay ≥ 90%.
5. **Sharp-feature operators.** Rational and saturating forms (`x^n/(k^n+x^n)`), and an
   option to search with user-given building blocks.
   *Measure:* a new sharp-feature tier (Hill n = 2.5, Lennard-Jones, resonance).

### How do we handle larger data?

v0.1 reads CSV into memory and discovers on at most 5,000 training rows by default.

- **Parquet and streaming CSV input**, so 10–100 million rows load without converting.
- **Smarter subsampling:** stratified over the input space (cover the edges, not only the
  dense middle), and repeated on several subsamples to check the equation does not change.
- **Use all rows where it is cheap:** constant refits, validation, and intervals on the full
  data, even when the search runs on a subsample.
- *Measure:* 10M rows × 20 columns in under 10 minutes on 8 cores; recovery unchanged.

### How do we handle more columns?

The search grows very fast with the number of inputs. v0.1 passes at most `max_features`
columns to discovery.

- **Better screening** for 100–1,000 columns: group near-duplicate columns, rank by
  interaction strength as well as single-column importance, and report what was left out.
- **Column groups and roles:** let users mark columns as "condition", "measurement", or
  "identifier", so identifiers and timestamps are never used as inputs.
- **Lagged columns** for time series (`--lags temperature:1,2,3`), with ordered splits
  enforced. Biology and process data usually depend on what happened earlier.
- **Categorical columns** as indicators (low cardinality), with per-group constants.
- **Multi-table input:** read several CSVs with declared keys and join them, so the
  preparation step in the enzyme case study is part of the run and recorded in the report.
- *Measure:* variable precision ≥ 0.9 with 200 distractor columns (now tested with 5).

### How do we detect novelty?

v0.1 cannot tell whether a found equation is already known. This is the biggest gap
between "fits an equation" and "helps discover something".

1. **Known-law catalog.** Ship a catalog of about 200 known forms (Feynman equations,
   Arrhenius, Michaelis–Menten, Hill, logistic, power laws, Kepler, Stokes, and so on).
   After discovery, match the selected and front equations up to constants and report
   `known: <name>`, `variant of: <name>`, or `no match`. Fully offline; no network calls.
2. **Null controls.** `--null-control 20` reruns discovery on shuffled targets and reports
   how good a fit pure chance produces. A result only counts if it clearly beats that.
3. **Definitional-relation check.** Flag inputs that are probably computed from the target
   (the E = mc² run showed the leakage check works; extend it to ratios, sums, and products
   of columns).
4. **Regime detection.** Fit exponents in windows of each input and report when they drift.
   A law that changes form across the range (a crossover) is more interesting than a single
   global fit, and v0.1 reports only one formula.
5. **Novelty benchmark tier:** laws outside the catalog, scored on false "known" labels;
   regime tier scored on detected crossovers.

A `no match` label is not proof of novelty, and the report will say so. It is a reason to
look closer, run the null control, and check the literature.

## Also in 0.2

- **Progress events** from the engine, so the UI and other apps can show where a long run is.
- **Simulator loop:** `--simulator "<command>"` calls a user's simulator for each suggested
  experiment, so closed-loop discovery is one command.
- **macOS builds back** in CI and releases (paused in 0.1.0 because hosted macOS runners did
  not start jobs).
- **Stable engine API** for building applications, published on crates.io.
- **Case studies 6 and 7** from public data (exoplanets, AME2020).

## Priorities

0.2.0 focuses on **more data** and **longer, deeper runs**. More rows lower the effect of
noise on constants and selection; longer searches find forms the default budget misses.
Neither fixes bias from errors in the inputs or the approximation problem on noisy data,
so the accuracy items stay in the plan at lower priority.

| Priority | Item | Size | Gate metric |
|---|---|---|---|
| 1 | Larger data: Parquet, streaming CSV, full-data refits, stratified subsampling | large | 10M rows x 20 columns under 10 min on 8 cores; recovery unchanged |
| 1 | Longer compute: --effort, checkpoints, resumable runs, budget curve | medium | budget curve published; max improves T5 structure recovery |
| 1 | More columns: screening for 100–1,000 columns, column roles | medium | variable precision >= 0.9 with 200 distractors |
| 2 | Progress events (needed for long runs) | small | UI and CLI show stage and search progress |
| 2 | Grade calibration | medium | wrong-structure `high` <= 20% |
| 2 | Units + dimensional analysis | large | T5 structure >= 80% at 10% noise |
| 2 | Known-law catalog + null controls | medium | novelty tier: <= 5% false "known" |
| 3 | Automatic scale choice, errors in inputs | medium | Kepler raw data recovered; exponent within 0.02 |
| 3 | Lags, categoricals, multi-table input | medium | time-series tier; case study rerun with one command |
| 3 | Regime detection, sharp-feature operators, simulator loop | large | regime and sharp-feature tiers |
| — | Python package | — | not in 0.2.0 |

The task list is in [tasks.md](tasks.md).

Rule for every item, as in v0.1: no aggregate benchmark metric may drop by more than 3
points, and every change is measured with `tarafold bench` before it ships.

## What 0.2 will still not do

- Prove cause and effect from observational data.
- Replace domain experts: deciding that an equation is a real, new law still needs a person,
  new experiments, and independent replication.
- Find laws in data that do not contain them (effects below the noise, unmeasured variables,
  ranges never sampled).
