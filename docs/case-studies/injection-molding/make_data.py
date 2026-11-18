"""Synthetic production log of an injection-molding line (case study 3).

Hidden relationships (Tarafold is not told them):
    defect_rate = 0.3 * exp(-0.15 * cooling_time_s) + 0.00002 * (melt_temp_C - 235)^2
    energy_kwh  = 0.002 * melt_temp_C * cooling_time_s + 0.01 * pressure_bar
cooling_time_s is a whole number of seconds. pressure_bar does not affect defects.
Noise: 3% on both measurements. 1,200 shots.

Usage: python make_data.py <out-dir>
"""

import csv
import math
import random
import sys
from pathlib import Path

random.seed(7)
out = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
out.mkdir(parents=True, exist_ok=True)


def defect(m, p, c):
    return 0.3 * math.exp(-0.15 * c) + 0.00002 * (m - 235) ** 2


def energy(m, p, c):
    return 0.002 * m * c + 0.01 * p


with open(out / "molding.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["melt_temp_C", "pressure_bar", "cooling_time_s", "defect_rate", "energy_kwh"])
    for _ in range(1200):
        m = random.uniform(200, 270)
        p = random.uniform(50, 120)
        c = random.randint(5, 30)
        w.writerow([round(m, 2), round(p, 2), c,
                    round(defect(m, p, c) * (1 + random.gauss(0, 0.03)), 6),
                    round(energy(m, p, c) * (1 + random.gauss(0, 0.03)), 5)])
print(f"wrote {out / 'molding.csv'}")
