import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import type { Tree } from "../report/schema";
import { compileTree, evalTree, treeVariables } from "./eval";

interface Case {
  expr: string;
  variables: string[];
  points: number[][];
  expected: (number | null)[];
  tree: Tree;
}

const golden = JSON.parse(
  readFileSync(resolve(process.cwd(), "../crates/tarafold-expr/tests/golden.json"), "utf8"),
) as { tolerance: number; cases: Case[] };

describe("browser evaluator matches the engine golden vectors", () => {
  it("has trees for every case", () => {
    expect(golden.cases.length).toBeGreaterThan(10);
    for (const c of golden.cases) expect(c.tree, c.expr).toBeDefined();
  });

  for (const c of golden.cases) {
    it(c.expr, () => {
      const compiled = compileTree(c.tree, c.variables);
      c.points.forEach((p, i) => {
        const env = Object.fromEntries(c.variables.map((v, j) => [v, p[j]!]));
        const want = c.expected[i];
        for (const got of [evalTree(c.tree, env), compiled(p)]) {
          if (want === null || want === undefined) {
            expect(Number.isFinite(got), `${c.expr} at ${p}: ${got}`).toBe(false);
          } else {
            expect(Math.abs(got - want), `${c.expr} at ${p}: ${got} vs ${want}`).toBeLessThanOrEqual(
              golden.tolerance * Math.max(Math.abs(want), 1),
            );
          }
        }
      });
    });
  }

  it("lists variables", () => {
    const t: Tree = { op: "mul", args: [{ var: "b" }, { op: "add", args: [{ var: "a" }, { var: "b" }] }] };
    expect(treeVariables(t)).toEqual(["b", "a"]);
  });
});
