# Tarafold user guide

Tarafold reads a CSV of numbers, finds equations that describe a target column, checks how far
to trust them, finds inputs that bring the target to a goal, and proposes the next
experiments. Everything runs locally in one binary. Nothing is sent over the network.

- [Install](#install)
- [Your first run](#your-first-run)
- [Reading the results](#reading-the-results)
- [Goals and optimization](#goals-and-optimization)
- [Next experiments](#next-experiments)
- [Using an equation elsewhere](#using-an-equation-elsewhere)
- [The visual workspace](#the-visual-workspace)
- [Reproducibility and budgets](#reproducibility-and-budgets)
- [Command reference](#command-reference)
- [Exit codes](#exit-codes)
- [Limits](#limits)

## Install

Download the archive for your platform from GitHub Releases, check it against `SHA256SUMS`,
and put `tarafold` on your `PATH`. Provenance can be verified with
`gh attestation verify <archive> --repo agkomyint/tarafold`.

From source (Rust 1.99 or newer; Node 22+ and pnpm for the UI):

```sh
pnpm --dir web install && pnpm --dir web build
cargo build --release -p tarafold-cli
```

## Your first run

```sh
tarafold synth business-direct --rows 2000 --noise 0.05 --out demo
tarafold run demo/data.csv --target revenue --out demo/out --open
```

The input must have a header row and at least 30 complete rows. Delimiters (`,` `;` tab) and
decimal commas are detected. Non-numeric columns are left out and listed with the reason.
Use `-` to read from stdin.

`demo/out/` then contains:

| File | Contents |
|---|---|
| `report.json` | Everything, in a versioned schema (`tarafold schema` prints it) |
| `report.html` | The interactive report as one file; opens offline |
| `equations.csv` | Every equation on the accuracy/complexity front |
| `candidates.csv` | Optimization candidates (when goals were given) |
| `suggestions.csv` | Suggested experiments, in the input's column layout (with `--suggest`) |
| `run.toml` | The exact configuration; `--config run.toml` repeats the run |

## Reading the results

For each target Tarafold reports a **front** of equations from simplest to most accurate and
selects one: the simplest equation within 10% of the best validation loss that still
predicts held-out ranges of each input (the extrapolation guard; turn it off with
`--no-extrapolation-guard`, or pick another with `--select <id>`).

The selected equation carries:

- a **reliability grade**, `high`, `medium`, or `low`, with the checks behind it: held-out
  accuracy, comparison with flexible baseline models, residual patterns, interval coverage,
  and structural stability (how often a rediscovery on resampled data finds the same
  equation; `--stability N`, default 10);
- an **evidence level**: `association`, `predictive`, `functional_fit`, or
  `interventional_fit`.

On observational data, equations describe how variables move together in this data. They do
not show what happens if you change an input. `interventional_fit` is possible only when you
declare the data as designed (`--data-source experimental` or `simulation`, optionally
`--designed a,b` for the columns you set), and the report says the result rests on that
declaration and holds inside the data domain only.

`tarafold profile data.csv` and `tarafold relate data.csv -t y` show the column statistics
and the relationship screening on their own.

## Goals and optimization

One exact goal:

```sh
tarafold run data.csv -t revenue --goal 50000 --control spend,price --bounds price=10..50
```

Several goals, including trade-offs:

```sh
tarafold run plant.csv --target yield,defect_rate,energy_cost \
  --control melt_temp,pressure --require "defect_rate<=0.01" \
  --prefer "max:yield:3" --prefer "min:energy_cost"
```

- `--require` is a hard goal: `col>=v`, `col<=v`, `col=v`, `col=a..b`.
- `--prefer` is an objective or soft goal: `max:col[:weight]`, `min:col[:weight]`, or a
  threshold with a weight. Up to 10 objectives. Goals are on target columns.
- `--constrain "a + b <= 100"` adds linear constraints on controllables.
- `--integer machines`, `--step hours=0.5` keep inputs on a grid.
- `--max-change price=20%` limits the move from the baseline (the medians, or `--fix`).
- `--robust 0.9` keeps candidates whose probability of meeting every hard goal, across the
  bootstrap ensemble, is at least 0.9.

A target may use earlier targets as inputs (`--target orders,revenue` lets `revenue` use
`orders`), and goals are evaluated through those chains. With two or more objectives the
result is a trade-off set of up to 20 candidates marked `knee`, `best:<goal>`, `preferred`,
and `most_likely`. When goals cannot all hold, the report names the smallest conflicting set
and the best value each goal can reach while the others hold (exit code 5).

Candidates outside the data domain are marked; they are extrapolations.

Rerun optimization on a saved report without rediscovering equations:

```sh
tarafold optimize demo/out --goal 60000 --control spend,price
```

## Next experiments

```sh
tarafold run trials.csv -t yield --data-source experimental --control n,water \
  --prefer max:yield --suggest 8 --suggest-mode balanced
tarafold suggest demo/out -n 8 --mode explore --expand 10%
```

`explore` picks points where the equations are least certain or where competing equations
disagree; `exploit` picks points likely to meet the goals; `balanced` splits the batch.
`--suggest-expand 10%` allows points beyond the observed range (marked outside the data
domain). `suggestions.csv` has the input's columns with the targets left empty: run the
experiments, fill in the results, append the rows to your data, and run again.

## Using an equation elsewhere

```sh
tarafold export demo/out --to python        # js, sql, excel, latex, json
tarafold export demo/out --to sql --target revenue --id 3 --out revenue.sql
```

Exports are standalone (Python uses only `math`, JavaScript only `Math`). A header states the
Tarafold version, run id, grade, evidence level, and each input's training range. Column
names that are not valid identifiers are renamed and the mapping is listed. Protected
operations keep their meaning: division by a value near zero, `log` of a non-positive value,
and `sqrt` of a negative value give `NaN` in Python and JavaScript, `NULL` in SQL, and
`#NUM!` in Excel. The Python and JavaScript output is tested against the engine on shared
test vectors to 1e-9 relative error.

## The visual workspace

`report.html` works from disk with no network access. `tarafold serve <report-dir>` (or
`tarafold serve data.csv -t y`) opens the same workspace with live controls: change goals,
bounds, and constraints and rerun optimization, ask for new suggestions, or rerun discovery
for other targets. The server listens on `127.0.0.1` only by default and prints a URL with a
random access token; requests without it, from another origin, or with a foreign `Host`
header are refused.

Views: Overview (dataset, warnings, histograms), Explore (brushing scatter, scatter matrix,
correlations), Relationships (importance, interactions, screening, baselines), Equations
(front, equation card, residuals, partial dependence, response surface), Target (goal
builder, what-if sliders, candidates, trade-offs, sensitivity), Next experiments, and
Reliability. Every chart has a data-table alternative and the views work with the keyboard.

## Reproducibility and budgets

All randomness comes from `--seed` (default 42). With the same data, seed, and settings,
results are identical with any `--threads` on the same platform (math libraries can differ
slightly between operating systems). `--max-evals N` sets the
equation-search budget (default 240,000 evaluations).

`--time-budget 90s` stops the search after that long, and Ctrl-C stops it at the next search
epoch (well under a second); both keep the best equations found so far and still write every
output file. A stopped search depends
on machine speed, so the report says so, skips the stability check, and Ctrl-C marks the run
`interrupted`. A second Ctrl-C quits without writing.

`-v` prints stage timings, `-vv` also the configuration, and `--log-file run.log` keeps a
copy. Logs never contain data values.

## Command reference

| Command | Purpose |
|---|---|
| `run <csv>` | Full pipeline |
| `optimize <dir>` | New goals on a saved report |
| `suggest <dir>` | New suggestions on a saved report |
| `export <dir> --to <lang>` | Equation as code |
| `report <dir>` | Rebuild `report.html` (`--data <csv>` to embed the data sample) |
| `serve <dir\|csv>` | Interactive workspace on localhost |
| `profile <csv>`, `relate <csv> -t y` | Column statistics, relationship screening |
| `synth <name>` / `synth --list` | Datasets from known equations |
| `bench --suite smoke\|full\|active\|multi` | Score Tarafold against known equations |
| `schema` | JSON Schema of `report.json` |

`tarafold <command> --help` lists every flag. Flags override `--config` files.

`--engine classical` is the only engine in this build; `quantum` and `hybrid` are reserved
names for planned engines and exit with code 2.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Internal error |
| 2 | Invalid usage or configuration |
| 3 | The data cannot support the analysis (too few rows, no usable columns) |
| 4 | The selected equation was graded `low` (outputs are still written) |
| 5 | A goal was not reached or goals conflict (outputs are still written) |

## Limits

- Equations are searched over `+ − × ÷ ^` and the unary operators in `--operators`
  (default `exp log sqrt square inv`; also `abs sin cos tanh`). Laws outside that space are
  approximated, and the grade and baseline comparison show it.
- Up to 10 million rows; discovery subsamples training rows beyond `max_rows` (5,000 by
  default) and says so in the warnings.
- Hidden confounders in observational data can make a well-fitting equation misleading.
  Tarafold reports association, not cause.
- Reports built with `--weights` cannot be reused by `optimize`/`suggest`; rerun `run`.
