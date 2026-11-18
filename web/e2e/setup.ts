import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";

export const OUT = resolve(process.cwd(), "test-results", "fixture");

export function binary(): string {
  if (process.env.TARAFOLD_BIN) return process.env.TARAFOLD_BIN;
  const exe = process.platform === "win32" ? "tarafold.exe" : "tarafold";
  return resolve(process.cwd(), "..", "target", "release", exe);
}

/** Build a multi-target, multi-goal report with suggestions from a synthetic dataset. */
export default function setup() {
  const bin = binary();
  if (!existsSync(bin)) throw new Error(`tarafold binary not found at ${bin}; set TARAFOLD_BIN`);
  rmSync(OUT, { recursive: true, force: true });
  mkdirSync(OUT, { recursive: true });
  const synth = resolve(OUT, "synth");
  execFileSync(bin, ["synth", "business-revenue", "--rows", "1200", "--noise", "0.02", "--out", synth]);
  // Exit code 5 (goal missed) still writes a report; anything else is a failure.
  try {
    execFileSync(bin, [
      "run",
      resolve(synth, "data.csv"),
      "--target",
      "visitors,conversion,orders,revenue",
      "--control",
      "spend,price",
      "--require",
      "revenue>=40000",
      "--prefer",
      "min:visitors",
      "--prefer",
      "max:orders",
      "--prefer",
      "max:revenue",
      "--suggest",
      "6",
      "--stability",
      "2",
      "--out",
      resolve(OUT, "report"),
    ]);
  } catch (e) {
    const status = (e as { status?: number }).status;
    if (status !== 5 && status !== 4) throw e;
  }
}
