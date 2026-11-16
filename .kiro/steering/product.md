# Product: Tarafold

Tarafold is an open-source numerical discovery engine shipped as one binary. Given a CSV of
numerical observations, it recovers interpretable equations `y = f(x1..xn)` and finds inputs
that bring `y` close to a target under constraints.

- The CLI (`tarafold run`) is the primary interface. The D3 workspace is an embedded viewer and explorer.
- Honesty over impressiveness: every equation carries a reliability grade and an evidence
  level (`association`, `predictive`, `functional_fit`, `interventional_fit`). Never claim causation.
  For observational data, never write "causes", "drives", "will increase/decrease", or "leads to".
  `interventional_fit` requires a user-declared experimental or simulation data source, and its
  wording must say it rests on that declaration and holds inside the data domain only.
- General-purpose: nothing in the engine is specific to one field (business, physics, etc.).
  Domain examples belong in the synthetic registry and docs, not in engine logic.
- Success is measured by the synthetic benchmark (`tarafold bench`), not by demos.
  A change that lowers a bench metric needs a written justification in the PR.
- No telemetry, no network calls, no foundation models.
- Current spec: `.kiro/specs/tarafold-v0-1/`.
