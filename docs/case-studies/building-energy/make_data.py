"""Daily heating-season meter readings for a portfolio of office buildings (case study 5).

Hidden relationship (Tarafold is not told it):
    energy_kwh = 30 + 0.012 * floor_area_m2 * (setpoint_C - outdoor_C) + 0.08 * occupancy
Observational: facility managers raise the setpoint on cold days, so setpoint and outdoor
temperature are correlated, as in real building logs. 2% meter noise. 1,500 building-days.

Usage: python make_data.py <out-dir>
"""

import csv
import random
import sys
from pathlib import Path

random.seed(5)
out = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
out.mkdir(parents=True, exist_ok=True)
buildings = [random.uniform(500, 5000) for _ in range(40)]

with open(out / "meters.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["building_id", "floor_area_m2", "outdoor_C", "setpoint_C", "occupancy", "energy_kwh"])
    for _ in range(1500):
        b = random.randrange(len(buildings))
        area = buildings[b]
        outdoor = random.uniform(-8, 14)
        setpoint = 20 + 0.12 * (10 - outdoor) + random.gauss(0, 0.6)
        occ = random.randint(0, 300)
        e = 30 + 0.012 * area * (setpoint - outdoor) + 0.08 * occ
        w.writerow([b + 1, round(area, 0), round(outdoor, 1), round(setpoint, 1), occ,
                    round(e * (1 + random.gauss(0, 0.02)), 2)])
print(f"wrote {out / 'meters.csv'}")
