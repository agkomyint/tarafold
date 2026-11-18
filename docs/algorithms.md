# How Tarafold works

This is a summary of the methods. The full design, with the reasoning behind each choice, is
in [`.kiro/specs/tarafold-v0-1/design.md`](../.kiro/specs/tarafold-v0-1/design.md).

## Pipeline

1. **Ingest.** CSV with delimiter and decimal sniffing, numeric detection, exclusion reasons
   (non-numeric, constant, duplicate), complete-case rows, SHA-256 of the input, optional
   row weights normalized to mean 1.
2. **Profile.** Moments, quantiles, histograms, MAD outliers, integer/unit/positive flags.
3. **Split.** Train/validation/test (60/20/20), random or ordered, from the run seed.
4. **Relate.** Pearson, Spearman, KSG mutual information, distance correlation, a ridge and an
   in-house histogram gradient-boosting baseline, permutation importance, Friedman H²,
   collinearity (VIF), leakage suspects. Screening ranks features and enforces
   `max_features`; it never drops a feature for weak evidence alone and always keeps the
   user's controllables.
5. **Discover.** Two engines feed one candidate pool:
   - *Sparse basis regression*: a library of transformed terms (powers, logs, products,
     ratios) searched by elastic-net paths and exhaustive/forward subset search, in three
     forms: linear, log-log (power laws), and log-target (exponential forms). Constants are
     then refit in the original units by Levenberg–Marquardt with exact forward-mode
     Jacobians.
   - *Genetic programming*: 8 islands × 64 expressions, regularized evolution (tournament,
     replace oldest), per-island parsimony pressure, multi-gene linear scaling (least-squares
     coefficients for the top-level additive terms), ring migration between epochs. Islands
     evolve in parallel inside an epoch with RNG streams fixed by (seed, island, epoch), so
     results do not depend on the thread count.
   - Expressions are simplified by a deterministic rule set that preserves values, including
     the protected semantics below. The Pareto front over (complexity, validation loss) is
     kept; selection takes the largest drop in log-loss per unit of complexity among
     equations within 10% of the best, after an extrapolation guard that refits each
     candidate with the lowest or highest 20% of every input held out and drops candidates
     whose held-out error is more than twice the best.
   - With row weights every fit minimizes `Σ w (y − f)²`: weighted least squares (columns
     scaled by √w), weighted LM residuals, and weighted GP fitness.
6. **Validate.** Metrics on all splits; 200 bootstrap refits of the constants give constant
   intervals and an ensemble; 90% prediction intervals combine the ensemble spread with the
   residual SD; structural stability reruns discovery 10 times on resampled rows at 10% of the
   budget and counts equivalent results; residual diagnostics; an applicability domain from
   ranges plus Mahalanobis distance. The grade combines these checks; the evidence level
   follows from the data source and the grade.
7. **Optimize.** One goal: exact 1-D root finding or differential evolution with Newton
   projection; with constraints or discrete inputs, an exact-penalty DE plus pattern search.
   Several goals: constrained single-objective search for 0–1 objectives, NSGA-II for 2–3,
   reference-direction NSGA-III for 4–10, Deb feasibility rules, a well-spread trade-off set
   of up to 20 (best point per objective first, then farthest-point selection), knee and
   per-goal marks, joint goal probability over the bootstrap ensemble, redundancy detection
   (rank correlation above 0.9 across the first front), and a deletion-filter search for the
   smallest conflicting goal set.
8. **Suggest.** An ensemble of the selected equation's bootstrap refits and up to 5 competing
   front equations weighted by likelihood ratio. Explore scores ensemble variance and
   equation disagreement; exploit scores the probability of meeting the goals or improving
   the trade-off. Batches are chosen greedily with a local penalty (radius 0.15 in
   standardized units) and kept away from existing rows.
9. **Report.** JSON with a versioned schema, CSVs (text cells starting with `= + - @` are
   prefixed with `'`), and a single-file HTML report with the data sample embedded.

## Protected semantics

Domain errors give NaN instead of a silently different value: `a / b` and `1 / b` need
`|b| ≥ 1e-12`; `log(a)` needs `a > 0`; `sqrt(a)` needs `a ≥ 0`; `a ^ b` with `a < 0` needs an
integer `b`. The browser evaluator and exported code follow the same rules, checked against
the shared vectors in `crates/tarafold-expr/tests/golden.json`.

## Equivalence

Two equations are equivalent when they are equal after simplification, or when their values
agree within 1e-3 relative error at sample points once constants are rounded to 3
significant figures (`tarafold_expr::equiv`). The benchmark applies the same rule on fresh
in-domain data from the generating distribution.

## Determinism

All randomness goes through ChaCha8 streams derived from the run seed. Reductions use a
fixed order. A run bounded by `--max-evals` is byte-identical for any thread count on the same
platform (math libraries can differ slightly between operating systems); a run stopped by
`--time-budget` or Ctrl-C is not, and says so.
