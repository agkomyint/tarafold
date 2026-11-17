# Tarafold

**Unfolding the mathematics hidden in data.**

Tarafold is an open-source numerical discovery engine. Give it a CSV of numerical
observations and it:

1. profiles every variable,
2. finds linear, nonlinear, and interaction relationships with a target,
3. recovers interpretable candidate equations `y = f(x1, …, xn)`,
4. validates them on held-out data and grades their reliability,
5. finds input combinations that bring the target close to a value you choose, within your constraints.

It ships as **one binary** with no Python, Julia, or Node runtime. The CLI runs the whole
pipeline, and an embedded visual workspace lets you explore the results.

> **Status:** pre-alpha. The end-to-end pipeline works (`synth` → `run` → equations →
> validation → target-seeking). Genetic-programming search, the benchmark command, the visual
> workspace, multi-goal optimization, and experiment suggestions are in progress.
> The v0.1 spec is in [`.kiro/specs/tarafold-v0-1/`](.kiro/specs/tarafold-v0-1/).

### Try it now

```sh
cargo build --release -p tarafold-cli
./target/release/tarafold synth --list
./target/release/tarafold synth business-direct --rows 2000 --noise 0.05 --out demo
./target/release/tarafold run demo/data.csv --target revenue --goal 50000 --control spend,price
cargo run --release -p tarafold-engine --example synthetic_suite   # score all datasets vs. truth
```

The commands below describe the full v0.1 target interface; not all flags exist yet.

## Quick look

```sh
# Generate a dataset from a known equation (to see whether Tarafold finds it)
tarafold synth business-revenue --rows 2000 --noise 0.01 --out demo/

# Discover the equation and find inputs that reach revenue = 1,000,000
tarafold run demo/data.csv --target revenue --goal 1000000 \
  --control spend,price --bounds price=10..50 --constrain "spend <= 60000" \
  --out demo/out

# Explore visually (local only, 127.0.0.1)
tarafold serve demo/out
```

Output in `demo/out/`: `report.json` (versioned schema, see `tarafold schema`),
`report.html` (single self-contained file, works offline), `equations.csv`,
`candidates.csv`, and `run.toml` (the exact config, for reproducing the run).

Use the discovered equation in your own tools:

```sh
tarafold export demo/out --to python    # also: js, sql, excel, latex, json
```

Other options for general use: read from stdin (`-`), several targets (`--target a,b`),
row weights (`--weights w`), whole-number or stepped inputs (`--integer machines`,
`--step hours=0.5`), and `--data-source experimental|simulation` for designed data.

### Several goals at once

```sh
tarafold run plant.csv --target yield,defect_rate,energy_cost \
  --control melt_temp,pressure,cooling_time_s \
  --require "defect_rate<=0.01" --prefer "max:yield:3" --prefer "min:energy_cost"
```

Hard goals (`--require`) must hold; soft goals (`--prefer`) are traded off. With two or
more maximize/minimize goals (up to 10), Tarafold returns the trade-off set and marks the
balanced choice, the best per goal, and the option most likely to meet every goal. If the
goals can't all be met, it names the goals that conflict.

### What to measure next

```sh
tarafold run trials.csv --target yield --data-source experimental \
  --control nitrogen,irrigation --prefer "max:yield" --suggest 8
```

`--suggest` proposes the next experiments: where the model is least certain, where
competing equations disagree, or where goals are most likely to be met
(`--suggest-mode explore|exploit|balanced`). It writes `suggestions.csv` in the same format
as your data. Run the experiments, fill in the results, append the rows, and run again.

## Where it fits

Tarafold works on any table of numbers where a compact formula might exist: lab experiments,
engineering simulations (as a cheap surrogate model), process settings in manufacturing,
energy efficiency curves, crop response, pricing and marketing funnels. It is a poor fit
when no compact formula exists (images, text, hundreds of weak effects), with very few rows,
or when hidden confounders drive observational data. In those cases the report says the
formula loses to the black-box baseline.

## What Tarafold does and does not claim

Tarafold fits equations to data. With observational data, a good fit means the equation
describes how the variables move together. It does not show that changing an input in
the real world produces the predicted outcome. When you declare that inputs were set
deliberately (`--data-source experimental` or `simulation`), results can reach the
`interventional_fit` level, and the report notes that this rests on your declaration.
Every result carries:

- an **evidence level**: `association`, `predictive`, `functional_fit`, or `interventional_fit` (never `causal` in v0.1),
- a **reliability grade** (`high` / `medium` / `low`) with the checks that passed and failed,
- an **in-domain flag** for optimization results that rely on extrapolation.

## How it works

| Stage | Methods |
|---|---|
| Relate | Pearson, Spearman, mutual information (KSG), distance correlation, ridge and gradient-boosted baselines, permutation importance, Friedman's H-statistic |
| Discover | Sparse basis regression (FFX/SINDy-style, including log-log for power laws), then genetic-programming symbolic regression with Levenberg–Marquardt constant fitting and e-graph simplification |
| Validate | Train/validation/test split, bootstrap intervals, structural stability, residual diagnostics, applicability domain |
| Optimize | Exact 1-D root finding, differential evolution with local refinement, linear constraints, NSGA-II/III for multiple objectives, diversity selection, ensemble uncertainty and joint goal probability |
| Suggest | Ensemble over competing equations and bootstrap refits; uncertainty, equation-disagreement, and goal-likelihood scores; diverse batches |

Details: [design document](.kiro/specs/tarafold-v0-1/design.md).

## Measuring discovery

Tarafold is judged on synthetic data generated from known equations. `tarafold bench`
reports symbolic recovery rate, held-out accuracy, extrapolation accuracy, variable
selection, interval coverage, and whether proposed inputs actually reach the goal when
checked against the true equation.

## Building from source

Requirements: Rust 1.99+; for the UI, Node 22+ and pnpm.

```sh
pnpm --dir web install && pnpm --dir web build   # optional: embeds the UI
cargo build --release -p tarafold-cli
./target/release/tarafold --version
```

Without the UI build, the binary still works and `serve` shows how to build the UI.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: [SECURITY.md](SECURITY.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
