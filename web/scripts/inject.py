"""Dev helper: build report.html from web/dist-single and an existing report, without
rebuilding the Rust binary (same injection as `tarafold report`).

Usage: python web/scripts/inject.py <report-dir> <data.csv> [max-rows]
"""

import csv
import json
import sys
from pathlib import Path

ui = (Path(__file__).resolve().parents[1] / "dist-single" / "index.html").read_text(encoding="utf-8")
report_dir, data_csv = Path(sys.argv[1]), Path(sys.argv[2])
max_rows = int(sys.argv[3]) if len(sys.argv) > 3 else 20000
report = json.loads((report_dir / "report.json").read_text(encoding="utf-8"))
numeric = [c["name"] for c in report["dataset"]["columns"]]
with open(data_csv, newline="", encoding="utf-8") as f:
    rows = list(csv.DictReader(f))
n = len(rows)
k = min(n, max_rows)
idx = [i * n // k for i in range(k)]
sample = {
    "columns": numeric,
    "values": [[float(rows[i][c]) if rows[i][c] not in ("", None) else None for i in idx] for c in numeric],
    "rows_total": n,
    "sampled": k < n,
}
esc = lambda v: json.dumps(v, separators=(",", ":")).replace("<", "\\u003c")
tags = (f'<script type="application/json" id="tarafold-report">{esc(report)}</script>\n'
        f'<script type="application/json" id="tarafold-data">{esc(sample)}</script>')
marker = "<!--TARAFOLD_DATA-->"
i = ui.rfind(marker)
html = ui[:i] + tags + ui[i + len(marker):]
(report_dir / "report.html").write_text(html, encoding="utf-8")
print(f"wrote {report_dir / 'report.html'} ({k} rows)")
