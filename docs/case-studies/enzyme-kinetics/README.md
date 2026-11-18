# Case study: enzyme kinetics from five lab tables

**Field:** biochemistry lab. **Shows:** preparing multi-table data, decoy columns, a law that
is not in Tarafold's test set, goal-seeking on designed experiments, honest grading.

## The situation

A lab measured reaction rates in 1,500 runs. The export is five tables, as from a lab
information system:

| Table | Columns |
|---|---|
| `runs.csv` | run_id, date_index, operator_id, batch_id |
| `samples.csv` | run_id, enzyme_mg_ml, substrate_mM, ph |
| `conditions.csv` | run_id, temperature_K, humidity_pct, stir_rpm |
| `readings.csv` | run_id, instrument_id, raw_rate (rows shuffled) |
| `instruments.csv` | instrument_id, offset, gain |

Three instruments read with different offsets and gains. The question: what law governs
the rate, and which conditions give a rate of 2.0?

Hidden truth (Michaelis–Menten with an Arrhenius term, 3% noise):
`rate = 4.2e8 · enzyme · substrate / (2.5 + substrate) · e^(−6000/T)`.
pH, humidity, stirring, operator, batch, and date have no effect.

## Steps

```sh
python make_data.py          # writes tables/*.csv
python join.py               # joins on run_id, calibrates rate = (raw − offset) / gain
tarafold run experiment.csv --target rate --data-source experimental \
  --control enzyme_mg_ml,substrate_mM,temperature_K --goal 2.0 \
  --bounds enzyme_mg_ml=0.1..1.0 --suggest 8 --out report --open
```

Tarafold reads one numeric table, so the join and the calibration happen first
([join.py](join.py)). That step is part of real work and stays visible in the case study.

## What Tarafold found

- **Inputs:** it used only enzyme, substrate, and temperature. All six decoys were left out.
- **Equation:** `7.69e-88 · enzyme · substrate^0.739 · T^38.2 · e^(−0.331·√substrate − 0.0612·T)`,
  test R² 0.998, 90% intervals covering 95% of test rows.
- **Grade `medium`**, evidence `interventional_fit` (designed data). The grade is not `high`
  because rediscoveries on resampled data agreed only 5 times out of 10: the data does not
  settle the exact form.
- **The form is an approximation.** `T^38 · e^(−0.061·T)` follows `e^(−6000/T)` closely over
  280–330 K, and `substrate^0.74 · e^(−0.33·√substrate)` follows the saturation curve. Enzyme
  enters linearly, as in the truth.

## Goal: rate = 2.0

Five candidates, checked against the hidden law:

| # | enzyme | substrate | T (K) | predicted | true | error |
|---|---|---|---|---|---|---|
| 1 | 1.00 | 10.4 | 316.5 | 2.000 | 1.981 | −1.0% |
| 2 | 0.99 | 7.3 | 318.1 | 2.000 | 2.000 | 0.0% |
| 3 | 0.82 | 9.1 | 320.4 | 2.000 | 1.988 | −0.6% |
| 4 | 0.90 | 14.2 | 317.5 | 2.000 | 1.988 | −0.6% |
| 5 | 0.68 | 12.0 | 322.6 | 2.000 | 1.985 | −0.8% |

## Next experiments

Four suggestions aim at the goal. Four go to low substrate and high temperature, where the
competing equations on the front disagree most. Those are the runs that would reveal the
exact saturation shape and lift the grade.

## Takeaways

- Data preparation (joins, calibration) is done before Tarafold, in a script you can review.
- A good fit is not proof of the law: the grade said `medium`, and the suggestions say how
  to find out.
- Goal-seeking was accurate even though the formula was an approximation, because the
  candidates lie inside the measured range.
