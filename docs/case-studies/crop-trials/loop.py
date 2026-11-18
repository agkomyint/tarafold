"""Crop field trials run as a closed loop (case study 4).

Hidden response (Tarafold is not told it):
    yield_t_ha = 9 * nitrogen / (60 + nitrogen) * (1 - exp(-0.004 * water_mm))
with 4% plot-to-plot noise. Past trials covered only low nitrogen (0-100 kg/ha) and low
irrigation (100-400 mm). The goal, 6.5 t/ha, needs settings outside that region.

Each season: Tarafold fits the data, proposes inputs for the goal and 6 new plots
(`--suggest 6`); the "field" returns the true yield plus noise; the rows are appended.

Usage: python loop.py <tarafold-binary> <work-dir> [seasons]
"""

import csv
import json
import math
import random
import subprocess
import sys
from pathlib import Path

tf, work = sys.argv[1], Path(sys.argv[2])
seasons = int(sys.argv[3]) if len(sys.argv) > 3 else 4
work.mkdir(parents=True, exist_ok=True)
rng = random.Random(11)


def true_yield(n, w):
    return 9 * n / (60 + n) * (1 - math.exp(-0.004 * w))


def field(n, w):
    return true_yield(n, w) * (1 + rng.gauss(0, 0.04))


data = work / "trials.csv"
rows = []
for _ in range(30):
    n, w = rng.uniform(0, 100), rng.uniform(100, 400)
    rows.append({"nitrogen_kg_ha": round(n, 1), "water_mm": round(w, 0),
                 "yield_t_ha": round(field(n, w), 3)})


def save():
    with open(data, "w", newline="") as f:
        wr = csv.DictWriter(f, fieldnames=["nitrogen_kg_ha", "water_mm", "yield_t_ha"])
        wr.writeheader()
        wr.writerows(rows)


print("| season | plots | selected equation | grade | best candidate: N, water | true yield there |")
print("|---|---|---|---|---|---|")
for season in range(1, seasons + 1):
    save()
    out = work / f"season{season}"
    subprocess.run([tf, "run", str(data), "--target", "yield_t_ha",
                    "--data-source", "experimental", "--control", "nitrogen_kg_ha,water_mm",
                    "--bounds", "nitrogen_kg_ha=0..250", "--bounds", "water_mm=100..800",
                    "--goal", "6.5", "--suggest", "6", "--suggest-mode", "balanced",
                    "--out", str(out)], capture_output=True)
    rep = json.loads((out / "report.json").read_text(encoding="utf-8"))
    t = rep["targets"][0]
    eq = next(e for e in t["discover"]["front"] if e["id"] == t["discover"]["selected_id"])
    c = rep["optimize"]["candidates"][0]
    n, w = c["inputs"]["nitrogen_kg_ha"], c["inputs"]["water_mm"]
    print(f"| {season} | {len(rows)} | `{eq['infix']}` | {t['validate']['reliability']['grade']} "
          f"| {n:.0f}, {w:.0f} | {true_yield(n, w):.2f} |")
    for s in rep["suggest"]["suggestions"]:
        sn, sw = s["inputs"]["nitrogen_kg_ha"], s["inputs"]["water_mm"]
        rows.append({"nitrogen_kg_ha": round(sn, 1), "water_mm": round(sw, 0),
                     "yield_t_ha": round(field(sn, sw), 3)})
save()
print(f"\nfinal data: {data} ({len(rows)} plots)")
