"""Merge the 5 lab tables into one analysis table for Tarafold.

Steps a lab would do before modelling:
  1. join runs + samples + conditions + readings on run_id
  2. join instruments on instrument_id and calibrate: rate = (raw_rate - offset) / gain
  3. keep every candidate input (including ones that may not matter) and the target
"""

import csv
from pathlib import Path

here = Path(__file__).parent
t = here / "tables"


def load(name, key):
    with open(t / f"{name}.csv", newline="") as f:
        return {row[key]: row for row in csv.DictReader(f)}


runs = load("runs", "run_id")
samples = load("samples", "run_id")
conditions = load("conditions", "run_id")
readings = load("readings", "run_id")
instruments = load("instruments", "instrument_id")

fields = ["enzyme_mg_ml", "substrate_mM", "temperature_K", "ph", "humidity_pct", "stir_rpm",
          "operator_id", "batch_id", "date_index", "rate"]
rows = []
for run_id, r in runs.items():
    s, c, m = samples[run_id], conditions[run_id], readings[run_id]
    inst = instruments[m["instrument_id"]]
    rate = (float(m["raw_rate"]) - float(inst["offset"])) / float(inst["gain"])
    rows.append({
        "enzyme_mg_ml": s["enzyme_mg_ml"], "substrate_mM": s["substrate_mM"],
        "temperature_K": c["temperature_K"], "ph": s["ph"], "humidity_pct": c["humidity_pct"],
        "stir_rpm": c["stir_rpm"], "operator_id": r["operator_id"], "batch_id": r["batch_id"],
        "date_index": r["date_index"], "rate": f"{rate:.6f}",
    })

with open(here / "experiment.csv", "w", newline="") as f:
    w = csv.DictWriter(f, fieldnames=fields)
    w.writeheader()
    w.writerows(rows)
print(f"joined {len(rows)} runs x {len(fields)} columns -> {here / 'experiment.csv'}")
