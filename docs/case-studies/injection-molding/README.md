# Case study: injection molding, defects versus energy

**Field:** plastics manufacturing. **Shows:** a hard quality limit with a cost objective,
a whole-number input, an approximate formula, and the robust option.

## The situation

A production log of 1,200 shots records melt temperature, injection pressure, cooling time
(whole seconds), the defect rate, and energy per batch. Quality requires a defect rate of
at most 2%; within that, energy should be as low as possible.

Hidden truth (3% noise on both measurements):
`defect_rate = 0.3 · e^(−0.15·cooling) + 0.00002 · (melt − 235)²` and
`energy_kwh = 0.002 · melt · cooling + 0.01 · pressure`. Pressure does not affect defects.

## Steps

```sh
python make_data.py work
tarafold run work/molding.csv --target defect_rate,energy_kwh --exclude-other-targets \
  --control melt_temp_C,pressure_bar,cooling_time_s --integer cooling_time_s \
  --require "defect_rate<=0.02" --prefer "min:energy_kwh" --out work/report
```

## What Tarafold found

| Target | Selected equation | Test R² | Grade |
|---|---|---|---|
| energy_kwh | `0.862 + 0.002 · cooling · melt` | 0.992 | high |
| defect_rate | `1.466 − 0.1455·ln(cooling) + 2.016e-5·melt² + 0.00485·cooling − 0.00947·melt` | 0.997 | high |

- The energy formula is right. The small pressure term (0.5–1.2 kWh) was folded into the
  constant 0.862; at 3% noise it is hard to separate.
- The defect formula is an **approximation**: a log and a linear term stand in for the
  exponential decay with cooling time, and the melt-temperature parabola is recovered
  (`2.016e-5·melt² − 0.00947·melt` has its minimum at 235 °C, the true optimum).

## Settings

| # | melt (°C) | pressure | cooling (s) | predicted defects | true defects | true energy |
|---|---|---|---|---|---|---|
| 1 | 223.2 | 84 | 19 | 0.0200 | 0.0201 | 9.33 |
| 2 | 218.6 | 51 | 20 | 0.0200 | 0.0203 | 9.26 |
| 3 | 215.5 | 118 | 21 | 0.0200 | 0.0205 | 10.23 |

The true optimum is 223.5 °C, 19 s, low pressure, at 8.99 kWh; candidate 1 has the right
temperature and cooling time. All candidates sit exactly on the limit and miss it by up to
0.0005 under the truth. Tarafold showed why: their joint probability of meeting the limit
was only 0.47–0.62.

## Asking for confidence

`--robust 0.9` (keep only settings that meet the limit in 90% of the bootstrap ensemble)
returned no candidate (exit code 5) and named `defect_rate<=0.02` as the binding goal: the
ensemble is not confident anywhere near the cheapest settings. A practical alternative is a
safety margin:

```sh
tarafold optimize work/report --control melt_temp_C,pressure_bar,cooling_time_s \
  --integer cooling_time_s --require "defect_rate<=0.018" --prefer "min:energy_kwh"
```

All three candidates then meet the real 2% limit under the true law (0.0181–0.0186), at
9.5–12 kWh.

## Takeaways

- Goals on the boundary are fragile; joint probability tells you how fragile.
- An approximate formula still finds good settings inside the data's range.
- A margin costs little here: tightening the limit from 2.0% to 1.8% raised energy from 9.33
  to 9.54 kWh for the cheapest candidate.
