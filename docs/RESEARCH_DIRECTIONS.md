# Research directions: what tarafold needs to help find new things

*A proposal for the tarafold team, written after a long hands-on attempt to use v0.1.0-alpha.1/2 to
make a new scientific discovery. Everything below comes from runs we actually did. Numbers are
quoted as measured, including the failures. Nothing here has been peer reviewed.*

## 1. Summary

Tarafold is already good at its stated job: recovering a compact equation from numeric data and
saying how far to trust it. In our trials it **rediscovered known laws reliably** and its
validation caught weak fits. It did **not** produce a result we could call new, and the reasons are
mostly missing *workflow* features, not search quality. Closing four gaps would move it from "fits
equations" to "helps a scientist find out whether an equation is new, why it holds, and where it
breaks":

1. Know what is already known (novelty check).
2. Respect physics (units and dimensional analysis).
3. Detect where a law bends (regime and crossover diagnostics).
4. Close the loop with simulators and experiments (active discovery that beats random sampling).

## 2. What we tried and what happened

| Trial | Data | Outcome |
|---|---|---|
| Cluster SZ Y-M (RealSR, 100k rows) | public simulation | Recovered the self-similar `Y ~ M_gas * M^(2/3)` form. Rediscovery. |
| Exoplanets, language statistics | public archives | Only known or weak relations. Well-mined data gives well-known answers. |
| Schelling segregation jamming threshold | 16k to 160k own simulations, 6 variants | Compact empirical law `tau* ~ 1/2 + b v^(2/3) n^(-1/3)` (R^2 0.95 over 409 cells). **The n-exponent drifted from -0.29 to -0.45 as n grew**, so it is a crossover, not a power law. A static derivation was tested and refuted (R^2 0.22). Not claimable as new. |
| Life-like cellular automata, all 262,144 rules | own simulation | Mean-field map explains 73.6%. Tarafold's best correction reached ~0.79 held-out, same as plain linear regression. No compact law. |
| 15 World Bank indicators x 140 countries, shuffled-target control | public | Shuffled targets scored R^2 <= 0 (good). Real fits were known relations (Preston curve, rain-forest). 16 of 91 income-controlled pairs survived Bonferroni, mostly trivial. |
| Simulated chemical plant (3,000 batches) | own simulation | Recovered Arrhenius-type structure, R^2 0.986. Recommended settings within 1.2 yield points of the true optimum, **but the 90% interval (87 to 97) missed the true value 84.8** because the optimizer pushed to the edge of the data where selectivity falls. |

Lessons:

- Fitting was never the bottleneck. **Deciding whether a result is new, physical and general was**,
  and it all happened by hand, outside the tool.
- The most informative failure (the drifting exponent) was found only because we manually refit in
  size windows. The tool reported one good-looking formula and a single reliability grade.
- Several fits were near-tautologies (life expectancy vs child mortality). The tool flagged
  correlated columns but not definitional ones.
- Optimization recommended points where the model is least supported and reported intervals that
  did not cover the truth there.
- In the repo's own closed-loop benchmark, suggested experiments needed **35 runs, the same as
  random sampling** (CHANGELOG, Known limitations). Active learning is the feature most likely to
  matter for discovery and it currently gives no advantage.

## 3. Proposed features, in priority order

### P1. Novelty and rediscovery check

**What:** After discovery, compare the selected equation (and the top front entries) to a catalog of
known formulas and report `known`, `variant of known`, or `no match`. Start with a bundled catalog
(Feynman-style equations, standard scalings such as power laws, Arrhenius, Michaelis-Menten,
logistic, Preston-type curves) matched up to constants and symbolic equivalence. Add an optional,
user-initiated literature query hook (title/abstract search by variable names and form), never
automatic network access.

**Why:** Every one of our "candidates" took hours of manual searching to classify as a
rediscovery. A discovery tool must answer "is this known?" itself. A `rediscovery` label is also
honest output that stops teams from over-claiming.

**Potential:** Turns the report from "here is a fit" into "here is what is new". Also gives the
benchmark a second axis: novelty detection precision.

**Sketch:** `report.json` gains `targets[].novelty { status, matches[], catalog_version }`. Extend
`bench` with cases whose truth is intentionally outside the catalog.

### P2. Units and dimensional analysis

**What:** Optional units per column (`--units temp=K,pressure=Pa,...` or a sidecar file).
(a) Reject candidate equations that add quantities of different dimensions. (b) A Buckingham-pi
mode that finds dimensionless groups first, then searches over the groups. (c) Report the
dimensions of fitted constants (a constant with dimensions suggests a missing variable).

**Why:** Dimensional consistency is the cheapest strong prior in physics. It prunes the search space
massively and makes results interpretable. Our best fits (`1.508 * life_exp^1.099 * pop^-0.08 ...`)
mixed incommensurable quantities with no way to tell.

**Potential:** Fewer spurious equations, better extrapolation, and real physical insight. This is
where symbolic regression has produced its most credible results (dimensionless combinations).

### P3. Regime, crossover and scaling diagnostics

**What:**
- **Local-exponent scan:** for each input, fit the log-log slope in sliding windows and report
  whether it is constant. Flag drift beyond bootstrap CI. In the Schelling data this would have
  shown n-exponent -0.29 -> -0.35 -> -0.45 immediately.
- **Piecewise/crossover forms:** try `a + b x^p` -> `a + b x^q` transitions (smooth broken power
  laws, `(1 + (x/x0)^s)` forms) and report the crossover scale.
- **Scaling-collapse mode:** given a control parameter and system size, search for exponents that
  collapse curves (finite-size scaling), reporting critical points and exponents with CIs.

**Why:** Many real laws are asymptotic or have regimes. A single global formula can look excellent
over the sampled range and be wrong outside it. Phase transitions and critical phenomena are a rich
area where simulation data is cheap and formulas are poorly known.

**Potential:** The most direct route to genuinely new empirical results: crossover scales,
effective exponents, and collapse functions for models people have simulated for decades.

### P4. Active discovery against a simulator

**What:** `tarafold run --simulator "<command>"` where the command takes a row of inputs and
prints the target. The loop proposes experiments, calls the command, refits, and stops when the
structure is stable. First, fix the known weakness: suggestions must beat random sampling in the
closed-loop benchmark (target: median runs-to-recover clearly below random's 35). Candidate
approaches: disagreement across the bootstrap ensemble, discrimination between the top competing
equations (choose the point where they differ most), and boundary-seeking sampling near regime
changes found by P3.

**Why:** Simulation is the only cheap source of genuinely new data for most teams. All of our
discovery attempts relied on writing a simulator in Rust by hand, sweeping parameters, and
waiting. A built-in loop makes that a one-line workflow and makes sampling efficient.

**Potential:** Automates the exploration that scientists do manually, and at the right points
(where candidate equations disagree) is where new structure is revealed.

### P5. Built-in null controls and multiple-testing control

**What:** A `--null-control N` option that reruns discovery on shuffled targets N times and
reports the distribution of held-out R^2 and complexity. For multi-target or many-pair sweeps,
report adjusted significance (Bonferroni/FDR). Add a confounder option: `--control income` to
partial out a column before searching.

**Why:** Cross-field data produced nice-looking formulas. Only our hand-built permutation control
and income partialling showed which were meaningful. Tarafold should make the honest baseline the
default.

**Potential:** Prevents false discoveries and gives users defensible evidence.

### P6. Baseline-plus-correction mode

**What:** `--baseline "<expression>"` (a user's known theory, for example the mean-field map) and
the tool models the residual, reporting how much the correction adds beyond the baseline and
whether a compact correction exists.

**Why:** Scientists rarely want to rediscover the known part. In the cellular-automata run the
mean-field baseline explained 73.6% and the interesting question was purely about the remainder.

**Potential:** Targets exactly the gap between theory and data, where new physics lives.

### P7. Dynamics and structured inputs

**What:** Time-series and ODE discovery (SINDy-style sparse regression of derivatives, with the
same validation), and native handling of bit/categorical feature sets (for example rule tables)
with set-valued features such as counts, sums, and positions.

**Why:** Much of science is differential equations; cellular automata, population models and
chemical kinetics are not algebraic y = f(x).

**Potential:** Opens the large class of dynamical systems.

### P8. Edge-aware optimization and honest intervals

**What:** Penalize or flag recommendations far from observed data (the report already has
`domain_distance`). Widen intervals as a function of distance, and recommend a confirmation run
when a candidate sits at a bound or outside the central support. Add a check: for any candidate
whose predicted gain exceeds the best observed, require a stated validation experiment.

**Why:** In the plant simulation the recommendation was good, but the prediction was 7 points too
high and the interval missed. For industrial users this matters more than the formula.

**Potential:** Trustworthy recommendations, which is the main commercial value.

## 4. Benchmark additions

The current benchmark scores recovery of known equations. To measure *discovery* ability, add:

- **T11 regime tier:** known crossovers and broken power laws; score crossover-scale error and
  whether drift is flagged.
- **T12 tautology and confounder tier:** definitional pairs and hidden common drivers; score
  whether the tool avoids or flags them.
- **Novelty tier:** equations outside the catalog; score false `known` labels.
- **Closed-loop tier:** already exists (`--suite active`); make beating random a gate.
- **Extrapolation tier:** score interval coverage at the edge of the domain, not just inside it.

## 5. Why this is worth doing

- The hard, valuable part of discovery is judging novelty, validity and domain of validity. A
  tool that automates that, with honest labels, is useful even when the answer is "not new".
- Rediscovery is a feature as well as a limit: a tool that rederives known laws from raw data and
  labels them correctly earns the trust that makes its occasional new result believable.
- Cheap simulators exist in nearly every field (statistical physics, ecology, epidemiology,
  chemical engineering). Combining P3 and P4 lets a researcher point tarafold at a simulator and
  ask where its behaviour is not yet described by a known formula.
- The commercial and scientific uses share the same core: reliable intervals, edge awareness, and
  null controls.

## 6. Suggested first milestone

A small, testable slice that delivers value fast:

1. `--null-control` (P5) and local-exponent scan (P3): both reuse the existing pipeline and are
   mostly reporting code.
2. A catalog of about 50 known forms with matching (P1), reported as `novelty.status`.
3. Make the closed-loop benchmark beat random (P4), as the gating success measure.

Success would be: running tarafold on the Schelling data automatically reports "n-exponent drifts
from -0.29 to -0.45; crossover near n ~ 100 to 300; no catalog match", rather than a single
confident formula.

## 7. Caveats

- This proposal is based on one extended session by one user with an AI assistant; priorities
  may differ for your roadmap.
- None of our candidate results was confirmed as new. They are listed to show what the tool made
  easy and what it did not.
- Data and scripts from the trials are outside this repository (a local `tarafold-data` folder);
  they can be shared on request.

## 8. Blind rediscovery test (extrapolation)

12 laws that are not in the built-in registry, 2,000 rows each, 1% noise, `--data-source simulation`,
one seed. Score: R^2 of the **selected** equation on fresh points where the first input is pushed
up to 1.4x beyond its training maximum. Script and data: `tarafold-data/blind/` (not in this repo).

| Law | Selected equation (abridged) | Extrapolation R^2 |
|---|---|---|
| Kepler `a^1.5` | `1.001 * a^1.5` | 0.9999 |
| Arrhenius `A exp(-E/T)` | `0.9987 * A * exp(-E/T)` | 1.0000 |
| Poiseuille `r^4 dp/(eta l)` | `0.39 * r^4.011 * dp/eta/l` | 1.0000 |
| Michaelis-Menten | `V * exp(-log(1 + Km/S))` (equivalent form) | 1.0000 |
| Van der Waals, Planck, relativistic KE, logistic, large-angle pendulum | accurate numeric forms, structure not always the textbook one | >= 0.9999 |
| Hill, n = 2.5 | complexity 29, picks an overfit form | **0.560** (a complexity-12 front entry scores 0.959) |
| Lennard-Jones | complexity 35, constants up to 6.6e6 | **-120.9** |
| Driven resonance | complexity 37 | **-188.9** |

Reading: 9 of 12 extrapolate well. All three failures have a sharp feature (steep power, singular
or resonant response) and the selected equation is far more complex than its simpler front
neighbours, with huge constants. In Hill, a simpler front member extrapolates much better than the
one the selection rule chose.

Caveats: R^2 can hide a structurally different but numerically close formula (pendulum and
relativistic KE are series-like approximations, not the true form). Only the first input is
extrapolated, noise is a single 1%, and there is one seed per law. Treat as indicative.

Implications for the roadmap: (a) selection should be **extrapolation-aware** (leave-range-out
validation or a penalty on constant magnitude), because the current rule can pick the worst-behaved
front member; (b) the structural equivalence check (tasks.md 2.2) should be used on these cases;
(c) add a sharp-feature tier to the benchmark (resonance, LJ, Hill with larger n).
