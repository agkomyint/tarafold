# Repository structure

```
crates/tarafold-core      shared types, config, errors
crates/tarafold-io        CSV ingest/export
crates/tarafold-expr      expressions: eval, AD, parse/print, simplify
crates/tarafold-profile   column statistics
crates/tarafold-relate    correlations, MI, baselines, importance, interactions, screening
crates/tarafold-discover  sparse basis regression + GP symbolic regression + selection
crates/tarafold-validate  metrics, bootstrap, stability, domain, grading
crates/tarafold-optimize  target-seeking inverse search
crates/tarafold-synth     known-equation dataset registry
crates/tarafold-bench     benchmark runner and metrics
crates/tarafold-report    report schema, JSON/HTML output, user-facing text
crates/tarafold-engine    pipeline orchestration
crates/tarafold-server    local HTTP API + embedded UI
crates/tarafold-cli       the `tarafold` binary
web/                      visual workspace
bench/                    committed baseline + dev-only PySR reference
fuzz/                     cargo-fuzz targets
docs/                     user and algorithm docs
```

Dependency direction: core ← expr ← stage crates ← report ← engine ← server/bench ← cli.
Do not add edges against this direction.
