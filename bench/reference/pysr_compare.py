# /// script
# requires-python = ">=3.10"
# dependencies = ["pysr>=1.0", "numpy", "pandas"]
# ///
"""Dev-only reference: run PySR on the same synthetic cases as `tarafold bench` and write the
same per-case metrics, so the two can be compared side by side. Never a runtime dependency.

    uv run bench/reference/pysr_compare.py --tarafold target/release/tarafold --suite smoke

PySR installs a Julia runtime on first use (several hundred MB). Output: bench-out/pysr.json
and a Markdown table next to it, with the fields `tarafold bench` uses for the same cases
(`exact`, `r2_fresh`, `var_precision`, `seconds`).
"""

import argparse
import json
import subprocess
import tempfile
import time
from pathlib import Path

import numpy as np
import pandas as pd

SMOKE = ["linear", "polynomial", "cobb-douglas", "interaction", "kinetic-energy", "gravity",
         "business-revenue", "business-direct", "production"]


def synth(bin_: str, name: str, rows: int, noise: float, seed: int, out: Path) -> dict:
    subprocess.run([bin_, "synth", name, "--rows", str(rows), "--noise", str(noise),
                    "--seed", str(seed), "--out", str(out)], check=True, capture_output=True)
    return json.loads((out / "truth.json").read_text())


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tarafold", required=True)
    ap.add_argument("--suite", choices=["smoke", "full"], default="smoke")
    ap.add_argument("--out", default="bench-out")
    ap.add_argument("--niterations", type=int, default=40)
    a = ap.parse_args()

    from pysr import PySRRegressor  # imported late: it starts Julia

    listing = subprocess.run([a.tarafold, "synth", "--list"], check=True, capture_output=True,
                             text=True).stdout.splitlines()
    names = [l.split()[0] for l in listing if l.strip()]
    if a.suite == "smoke":
        names = [n for n in names if n in SMOKE]
        grid = [(500, 0.0), (500, 0.1)]
    else:
        grid = [(r, n) for r in (200, 2000) for n in (0.0, 0.01, 0.1)]
    results = []
    with tempfile.TemporaryDirectory() as tmp:
        for name in names:
            for rows, noise in grid:
                d = Path(tmp) / f"{name}-{rows}-{noise}"
                truth = synth(a.tarafold, name, rows, noise, 1, d)
                df = pd.read_csv(d / "data.csv")
                feats = [c for c in truth["features"] if c != "weight"]
                X, y = df[feats].to_numpy(), df[truth["target"]].to_numpy()
                model = PySRRegressor(
                    niterations=a.niterations,
                    binary_operators=["+", "-", "*", "/", "^"],
                    unary_operators=["exp", "log", "sqrt", "square"],
                    random_state=1, deterministic=True, parallelism="serial",
                    progress=False, verbosity=0,
                )
                t0 = time.time()
                model.fit(X, y, variable_names=feats)
                secs = time.time() - t0
                fresh = Path(tmp) / f"{name}-fresh"
                synth(a.tarafold, name, 1000, 0.0, 1001, fresh)
                fd = pd.read_csv(fresh / "data.csv")
                pred = model.predict(fd[feats].to_numpy())
                true = fd[truth["target"]].to_numpy()
                scale = max(np.mean(np.abs(true)) * 1e-2, 1e-12)
                rel = np.max(np.abs(pred - true) / np.maximum(np.abs(true), scale))
                r2 = 1 - np.sum((pred - true) ** 2) / np.sum((true - true.mean()) ** 2)
                used = {v for v in feats if v in str(model.sympy())}
                want = set(truth["equation_variables"])
                results.append({
                    "dataset": name, "rows": rows, "noise": noise,
                    "equation": str(model.sympy()), "exact": bool(rel < 1e-3),
                    "r2_fresh": float(r2),
                    "var_precision": len(used & want) / len(used) if used else 0.0,
                    "seconds": secs,
                })
                print(f"{name} rows={rows} noise={noise}: {results[-1]['equation']}")
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "pysr.json").write_text(json.dumps(results, indent=2))
    lines = ["| dataset | rows | noise | exact | R² fresh | seconds | equation |", "|---|---|---|---|---|---|---|"]
    for r in results:
        lines.append(f"| {r['dataset']} | {r['rows']} | {r['noise']} | {'yes' if r['exact'] else 'no'} | "
                     f"{r['r2_fresh']:.6f} | {r['seconds']:.1f} | `{r['equation'][:60]}` |")
    (out / "pysr.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
