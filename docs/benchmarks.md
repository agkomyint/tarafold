# Benchmark results

Tarafold is scored on data generated from known equations (`tarafold synth --list`). The
numbers below are from the v0.1.0 release candidate on Windows x86-64, default settings.
Reproduce them with:

```sh
tarafold bench --suite full --out bench-out      # ~25 min
tarafold bench --suite active --out bench-out    # closed-loop experiment selection (T12)
tarafold bench --suite multi --out bench-out     # multi-goal problems with known fronts (T11)
python bench/thresholds.py bench-out/bench.json bench-out/active.json bench-out/multi.json
```

## Full suite (312 cases)

25 equations × {200, 2,000} rows × noise {0, 1%, 10%} of std(y) × 2 data seeds, plus a
5-distractor variant of the interaction dataset. Heteroscedastic T9 cases run with their
`weight` column as row weights.

| tier | cases | correct structure | R² > 0.999 on fresh data | inverse success |
|---|---|---|---|---|
| T1 linear | 24 | 100% | 100% | 100% |
| T2 polynomial | 36 | 100% | 100% | 100% |
| T3 rational | 24 | 100% | 100% | 100% |
| T4 power law | 24 | 100% | 100% | 99.2% |
| T5 exp/log | 36 | 64% | 81% | 96.2% |
| T6 interactions, distractors | 36 | 100% | 100% | 100% |
| T7 physics | 72 | 100% | 100% | 100% |
| T8 business | 24 | 92% | 96% | 99.2% |
| T9 weighted | 24 | 100% | 100% | 100% |
| T10 discrete inputs | 12 | 100% | 100% | 90.7% |
| **all** | 312 | **95%** | **97%** | **99.1%** |

- *Correct structure*: after refitting the constants on noiseless data, the selected
  equation matches the truth within 1e-3 relative error.
- *Inverse success*: share of optimization candidates within tolerance that also reach the
  goal under the true equation (discrete candidates must be on their grid).
- Mean 90%-interval coverage on noisy cases: 0.904.
- Grade calibration: `high` is given to 96% of correct structures and to 40% (6 of 15) of
  wrong ones. The wrong ones graded `high` are close approximations of exponentials on noisy
  data (for example `1.19·x1 + 0.015·x1·x2²` for `x1·exp(x2/10)` at 10% noise) that pass
  every in-range check. A grade describes how reliable an equation is inside the data's
  range, not proof of the true law.
- Weighting gain on noisy T9 cases (RMSE against the truth, unweighted ÷ weighted): median 1.22.

Where it misses: exponentials with noise (T5 `exp-log`, `decay`, `exp-growth`) are often
replaced by an algebraic approximation with the same in-range accuracy, and
`business-direct` at 200 noisy rows finds a near-equivalent form of `price·exp(−0.03·price)`.

## Closed loop (T12)

Start from 30 rows in the lowest 40% of each input's range. The goal lies at the true value at
85% of each range. Each round adds 5 experiments chosen by `--suggest` (balanced), uniform
random sampling, or a Latin hypercube; the "experiment" is the true equation plus 5% noise.
A goal counts as reached when one of the first 3 optimization candidates is within 2% of the
target's range under the true equation. 5 datasets × 3 seeds, at most 40 added experiments.

| method | median experiments to goal | mean | mean experiments to recover the equation |
|---|---|---|---|
| suggest | 5 | 5.7 | 34.3 |
| random | 10 | 10.3 | 35.3 |
| Latin hypercube | 5 | 9.3 | 35.7 |

Suggestions reach the goal in about half the experiments random sampling needs. They do not
recover the equation meaningfully faster than the baselines.

## Multi-goal (T11)

Every objective is discovered from 1,000 noiseless rows, then all are minimized jointly. The
returned candidates (up to 20) are scored with the true functions. *HV ratio* compares their
hypervolume with that of the same number of well-spread points on the true front; *vs dense
front* compares with the densely sampled true front, which a finite set cannot match.

| problem | objectives | HV ratio | HV vs dense front | goal truth rate |
|---|---|---|---|---|
| convex | 2 | 1.000 | 0.981 | — |
| concave | 2 | 0.988 | 0.938 | — |
| convex with `f1 ≤ 0.25` | 2 | 1.000 | 0.974 | 1.00 |
| DTLZ2 | 3 | 0.838 | 0.689 | — |
| DTLZ2 | 5 | 0.912 | 0.600 | — |

The DTLZ2 objectives are products of sines and cosines; Tarafold fits them with
approximations of R² 0.97–1.00, which is the main loss on those problems.

## v0.1 acceptance thresholds

All pass (`bench/thresholds.py`):

| Threshold | Result |
|---|---|
| Noiseless T1–T5, T8: symbolic recovery ≥ 70% | 100% |
| Noiseless T1–T5, T8: R² > 0.999 ≥ 90% | 100% |
| 10% noise: median oracle gap ≤ 0.02 | 0.000 |
| 5 distractors: variable precision ≥ 0.9 | 1.00 |
| Inverse success on accurate equations ≥ 90% | 99.5% |
| 10% noise: interval coverage in [0.80, 0.97] | 0.903 |
| T9 weighting gain > 1.2 | 1.22 |
| T10 inverse success ≥ 90%, on grid | 90.7% |
| Export fidelity (Python, JS) ≤ 1e-9 | passes (golden-vector tests) |
| T11 HV ratio ≥ 0.9 (2–3 objectives, mean) / ≥ 0.75 (5) | 0.957 / 0.912 |
| T11 goal truth rate ≥ 0.9; calibration error ≤ 0.1 | 1.00; 0.00 |
| T12 median to goal ≤ 60% of random; not worse than LHS | 50%; mean 5.7 vs 9.3 |

Two definitions were set while building the suites and are stated here so the numbers can be
read correctly: the T11 HV ratio is normalized by the same number of well-spread true-front
points (the design's dense-front ratio is in the table too), and the T11 calibration check
has only one goal-constrained problem, where every candidate reported probability 1 and met
its goal, so it is a weak test.
