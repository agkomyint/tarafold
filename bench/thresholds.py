# /// script
# requires-python = ">=3.10"
# ///
"""Check the v0.1 acceptance thresholds (design §6.2) on a full-suite bench.json.

Usage: python bench/thresholds.py bench-out/bench.json [active.json] [multi.json]
Dev-only helper; prints one line per threshold and exits 1 if any fails.
"""

import json
import statistics
import sys


def main() -> int:
    b = json.load(open(sys.argv[1], encoding="utf-8"))
    cases = [c for c in b["cases"] if not c.get("error")]
    rows = []

    def check(name, value, ok):
        rows.append((name, value, ok))

    core = [c for c in cases if c["tier"] in ("T1", "T2", "T3", "T4", "T5", "T8") and c["case"]["noise"] == 0]
    check("noiseless T1-T5,T8 symbolic recovery >= 70%", sum(c["exact"] for c in core) / len(core), sum(c["exact"] for c in core) / len(core) >= 0.70)
    acc = sum(c["accuracy_solution"] for c in core) / len(core)
    check("noiseless T1-T5,T8 accuracy solution >= 90%", acc, acc >= 0.90)
    noisy = [c["oracle_gap"] for c in cases if c["case"]["noise"] == 0.1 and c["oracle_gap"] is not None]
    gap = statistics.median(noisy)
    check("10% noise median oracle gap <= 0.02", gap, gap <= 0.02)
    distr = [c for c in cases if c["case"]["distractors"] >= 5]
    prec = sum(c["var_precision"] for c in distr) / len(distr)
    check("5 distractors variable precision >= 0.9", prec, prec >= 0.9)
    good = [c for c in cases if c["accuracy_solution"]]
    ok = sum(c["inverse_ok"] for c in good)
    tot = sum(c["inverse_total"] for c in good)
    check("inverse success on accurate equations >= 90%", ok / tot, ok / tot >= 0.9)
    cov = [c["coverage_90"] for c in cases if c["case"]["noise"] == 0.1 and c["coverage_90"] is not None]
    m = statistics.mean(cov)
    check("10% noise interval coverage in [0.80, 0.97]", m, 0.80 <= m <= 0.97)
    gains = [c["weighting_gain"] for c in cases if c.get("weighting_gain") is not None]
    g = statistics.median(gains) if gains else float("nan")
    check("T9 weighting gain > 1.2", g, g > 1.2)
    t10 = [c for c in cases if c["tier"] == "T10"]
    ok10 = sum(c["inverse_ok"] for c in t10)
    tot10 = sum(c["inverse_total"] for c in t10)
    check("T10 inverse success >= 90% (on grid)", ok10 / tot10, ok10 / tot10 >= 0.9)
    if len(sys.argv) > 2:
        a = json.load(open(sys.argv[2], encoding="utf-8"))
        r = a["median_to_goal"]["suggest"] / a["median_to_goal"]["random"]
        check("T12 suggest median to goal <= 60% of random", r, r <= 0.6)
        check("T12 suggest mean to goal <= LHS", a["mean_to_goal"]["suggest"] - a["mean_to_goal"]["lhs"], a["mean_to_goal"]["suggest"] <= a["mean_to_goal"]["lhs"])
    if len(sys.argv) > 3:
        mm = json.load(open(sys.argv[3], encoding="utf-8"))
        check("T11 HV ratio (2-3 objectives) >= 0.9", mm["hv_ratio_2_3"], mm["hv_ratio_2_3"] >= 0.9)
        check("T11 HV ratio (5 objectives) >= 0.75", mm["hv_ratio_5"], mm["hv_ratio_5"] >= 0.75)
        check("T11 goal truth rate >= 0.9", mm["goal_truth_rate"], mm["goal_truth_rate"] >= 0.9)
        check("T11 calibration error <= 0.1", mm["calibration_error"], mm["calibration_error"] <= 0.1)
    for name, value, ok in rows:
        print(f"{'PASS' if ok else 'FAIL'}  {name}: {value:.4f}")
    return 0 if all(ok for _, _, ok in rows) else 1


if __name__ == "__main__":
    sys.exit(main())
