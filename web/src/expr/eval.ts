// Browser evaluator for equation trees from report.json. It must match the engine's protected
// semantics exactly; src/expr/eval.test.ts checks it against crates/tarafold-expr/tests/golden.json.
import type { Tree } from "../report/schema";

const DIV_EPS = 1e-12;

function unary(op: string, a: number): number {
  switch (op) {
    case "neg":
      return -a;
    case "exp":
      return Math.exp(a);
    case "log":
      return a > 0 ? Math.log(a) : NaN;
    case "sqrt":
      return a >= 0 ? Math.sqrt(a) : NaN;
    case "square":
      return a * a;
    case "abs":
      return Math.abs(a);
    case "sin":
      return Math.sin(a);
    case "cos":
      return Math.cos(a);
    case "tanh":
      return Math.tanh(a);
    case "inv":
      return Math.abs(a) >= DIV_EPS ? 1 / a : NaN;
    default:
      return NaN;
  }
}

/** `powf` semantics: NaN for a negative base with a non-integer exponent; `1^y = 1`. */
function pow(a: number, b: number): number {
  if (a < 0 && !Number.isInteger(b)) return NaN;
  if (a === 1) return 1;
  return Math.pow(a, b);
}

function binary(op: string, a: number, b: number): number {
  switch (op) {
    case "add":
      return a + b;
    case "sub":
      return a - b;
    case "mul":
      return a * b;
    case "div":
      return Math.abs(b) >= DIV_EPS ? a / b : NaN;
    case "pow":
      return pow(a, b);
    default:
      return NaN;
  }
}

/** Evaluate a tree with variables looked up by name. Missing variables give NaN. */
export function evalTree(tree: Tree, env: Record<string, number>): number {
  if ("const" in tree) return tree.const ?? NaN;
  if ("var" in tree) return env[tree.var] ?? NaN;
  const args = tree.args.map((t) => evalTree(t, env));
  if (args.length === 1) return unary(tree.op, args[0]!);
  if (args.length === 2) return binary(tree.op, args[0]!, args[1]!);
  return NaN;
}

/** Variable names used by a tree, in first-use order. */
export function treeVariables(tree: Tree, out: string[] = []): string[] {
  if ("var" in tree) {
    if (!out.includes(tree.var)) out.push(tree.var);
  } else if ("args" in tree) {
    for (const a of tree.args) treeVariables(a, out);
  }
  return out;
}

/**
 * Compile a tree to a function of a value array, where `vars[i]` names `x[i]`. Faster than
 * `evalTree` for charts that evaluate thousands of points.
 */
export function compileTree(tree: Tree, vars: string[]): (x: ArrayLike<number>) => number {
  const build = (t: Tree): ((x: ArrayLike<number>) => number) => {
    if ("const" in t) {
      const c = t.const ?? NaN;
      return () => c;
    }
    if ("var" in t) {
      const i = vars.indexOf(t.var);
      return i < 0 ? () => NaN : (x) => x[i] ?? NaN;
    }
    const fs = t.args.map(build);
    const op = t.op;
    if (fs.length === 1) {
      const f = fs[0]!;
      return (x) => unary(op, f(x));
    }
    if (fs.length === 2) {
      const f = fs[0]!;
      const g = fs[1]!;
      return (x) => binary(op, f(x), g(x));
    }
    return () => NaN;
  };
  return build(tree);
}
