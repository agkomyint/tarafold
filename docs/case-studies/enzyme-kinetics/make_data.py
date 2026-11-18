"""Demo data: an enzyme-kinetics lab study stored as 5 related tables.

The hidden law (not in Tarafold's registry, Tarafold is never told it):
    rate = 4.2e8 * enzyme * substrate / (2.5 + substrate) * exp(-6000 / temperature)
i.e. Michaelis-Menten kinetics (Km = 2.5 mM) with an Arrhenius temperature term.

Tables
    runs.csv         run_id, date_index, operator_id, batch_id
    samples.csv      run_id, enzyme_mg_ml, substrate_mM, ph
    conditions.csv   run_id, temperature_K, humidity_pct, stir_rpm
    readings.csv     run_id, instrument_id, raw_rate        (instrument-biased, noisy)
    instruments.csv  instrument_id, offset, gain            (calibration)

join.py merges them into one analysis table. pH, humidity, stirring, operator and batch
have no effect in the hidden law: they are distractors.
"""

import csv
import math
import random
from pathlib import Path

random.seed(2026)
out = Path(__file__).parent / "tables"
out.mkdir(exist_ok=True)
N = 1500

instruments = [
    {"instrument_id": 1, "offset": 0.05, "gain": 1.00},
    {"instrument_id": 2, "offset": -0.02, "gain": 1.08},
    {"instrument_id": 3, "offset": 0.10, "gain": 0.95},
]

runs, samples, conditions, readings = [], [], [], []
for i in range(1, N + 1):
    e = random.uniform(0.1, 2.0)
    s = random.uniform(0.1, 20.0)
    t = random.uniform(280.0, 330.0)
    true_rate = 4.2e8 * e * s / (2.5 + s) * math.exp(-6000.0 / t)
    rate = true_rate * (1 + random.gauss(0, 0.03))  # 3% measurement noise
    inst = random.choice(instruments)
    raw = inst["offset"] + inst["gain"] * rate
    runs.append({"run_id": i, "date_index": i // 20, "operator_id": random.randint(1, 4),
                 "batch_id": random.randint(1, 12)})
    samples.append({"run_id": i, "enzyme_mg_ml": round(e, 4), "substrate_mM": round(s, 4),
                    "ph": round(random.uniform(6.5, 8.0), 2)})
    conditions.append({"run_id": i, "temperature_K": round(t, 2),
                       "humidity_pct": round(random.uniform(30, 60), 1),
                       "stir_rpm": random.choice([200, 400, 600])})
    readings.append({"run_id": i, "instrument_id": inst["instrument_id"], "raw_rate": round(raw, 6)})

random.shuffle(readings)  # tables are not in the same order, like real exports

for name, rows in [("runs", runs), ("samples", samples), ("conditions", conditions),
                   ("readings", readings), ("instruments", instruments)]:
    with open(out / f"{name}.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)
print(f"wrote 5 tables with {N} runs to {out}")
