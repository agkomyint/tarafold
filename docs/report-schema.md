# report.json reference

`report.json` is the complete record of a run. Its JSON Schema is generated from the Rust
types and printed by `tarafold schema`; the web UI's TypeScript types
(`web/src/report/schema.ts`) are generated from the same schema with
`pnpm --dir web gen:types`.

The schema has its own version, `schema_version` (currently `0.1.0`), separate from the tool
version. Non-finite numbers (an undefined R², an unbounded interval) are written as `null`;
every number in the schema also allows `null`.

## Top level

| Field | Contents |
|---|---|
| `schema_version`, `tarafold_version` | Versions |
| `run` | `id` (input hash + seed), `started_unix`, `status` (`completed` or `interrupted`), `engine`, the fully resolved `config`, and per-stage timings |
| `dataset` | Source path, SHA-256, delimiter, rows read and dropped, excluded columns with reasons, per-column summary and profile, `data_source`, `weighted` |
| `targets[]` | One entry per target (below) |
| `optimize` | `null`, or the goals, method, controllables, bounds, baseline, ranked `candidates[]`, `reached`, `binding` bounds, `conflict`, `redundant_objectives`, evidence, and the disclaimer |
| `suggest` | `null`, or the mode, exploit/explore split, bounds, competing `structures`, `suggestions[]`, and the note |
| `warnings[]` | `code`, `message`, `target` |
| `notes[]` | Interpretation notes (observational or interventional wording) |

## targets[]

| Field | Contents |
|---|---|
| `target`, `features` | Target column and the features passed to discovery |
| `relate` | Correlations, mutual information, distance correlation, baselines, permutation importance, interactions (H²), collinear pairs, leakage suspects, feature correlation matrix |
| `screening[]` | Per feature: kept or not, score, reason |
| `discover` | `front[]` (each with `id`, `infix`, `infix_exact`, `latex`, JSON `tree`, `complexity`, `constants`, `variables`, `strategy`, and train/val/test metrics), `selected_id`, strategy log, evaluations, rows used |
| `validate` | `reliability` (grade and checks), `evidence`, `evidence_basis`, 90% interval half-width and test coverage, residual SD, `bootstrap` (constant intervals and ensemble constants), `stability`, residual diagnostics, training `domain` |

`infix_exact` parses back to the identical expression (`tarafold_expr::parse`). `tree` is the
form the browser evaluates: `{"const": 2.5}`, `{"var": "price"}`, or
`{"op": "mul", "args": [...]}` with `op` one of `add sub mul div pow neg exp log sqrt square
abs sin cos tanh inv`.

## Candidates

Each candidate has `inputs`, `predictions` per goal target, `goals_met` (in goal order),
`joint_probability` (share of bootstrap ensemble members for which every hard goal holds),
`interval`, `in_domain`, `domain_distance`, change from baseline (absolute and percent),
`sensitivity` (∂ŷ/∂x), `elasticity`, and `marks` (`best`, `knee`, `best:<goal>`,
`preferred`, `most_likely`).

## Compatibility

Readers should ignore unknown fields. New optional fields can appear in minor schema
versions; removing or renaming a field raises the major version.
