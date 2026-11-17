//! `tarafold` command-line interface.

mod summary;

use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tarafold_core::{DataSource, RunConfig, SplitStrategy};
use tarafold_engine::EngineError;
use tarafold_io::{IngestOptions, IoError, read_csv};

const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\ncommit: ",
    env!("TARAFOLD_GIT_COMMIT"),
    "\ntarget: ",
    env!("TARAFOLD_BUILD_TARGET"),
    "\nreport schema: 0.1.0"
);

/// Tarafold: unfold the mathematics hidden in numerical data.
#[derive(Parser)]
#[command(name = "tarafold", version, long_version = LONG_VERSION, about, max_term_width = 100)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the full pipeline: discover equations for a target, validate them, and optionally
    /// find inputs that reach a goal.
    Run(Box<RunArgs>),
    /// Generate a synthetic dataset from a known equation.
    Synth(SynthArgs),
    /// Run the known-equation benchmark and score discovery against the truth.
    Bench(BenchArgs),
    /// Print the JSON Schema of report.json.
    Schema,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Human,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum Engine {
    Classical,
    Quantum,
    Hybrid,
}

#[derive(Clone, Copy, ValueEnum)]
enum SourceArg {
    Observational,
    Experimental,
    Simulation,
}

#[derive(Clone, Copy, ValueEnum)]
enum SplitArg {
    Random,
    Ordered,
}

#[derive(Args)]
struct RunArgs {
    /// CSV file, or `-` for stdin.
    input: String,
    /// Target column(s), comma-separated.
    #[arg(long, short, value_delimiter = ',', required = true)]
    target: Vec<String>,
    /// Desired target value; enables optimization.
    #[arg(long, allow_negative_numbers = true)]
    goal: Option<f64>,
    /// Controllable columns, comma-separated (default: all variables in the equation).
    #[arg(long, value_delimiter = ',')]
    control: Vec<String>,
    /// Bounds for a controllable, e.g. `price=10..50` (repeatable).
    #[arg(long, value_name = "COL=LO..HI", allow_negative_numbers = true)]
    bounds: Vec<String>,
    /// Fix a non-controllable at a value, e.g. `rainfall=120` (repeatable).
    #[arg(long, value_name = "COL=VALUE", allow_negative_numbers = true)]
    fix: Vec<String>,
    /// Number of optimization candidates.
    #[arg(long)]
    candidates: Option<usize>,
    /// Goal tolerance as a fraction of the target's observed range.
    #[arg(long)]
    tolerance: Option<f64>,
    /// Row-weight column.
    #[arg(long)]
    weights: Option<String>,
    /// How the data was produced.
    #[arg(long, value_enum)]
    data_source: Option<SourceArg>,
    /// Split strategy (`ordered` for time-ordered data).
    #[arg(long, value_enum)]
    split: Option<SplitArg>,
    /// Override the selected equation by front id.
    #[arg(long)]
    select: Option<usize>,
    /// Maximum features passed to discovery.
    #[arg(long)]
    max_features: Option<usize>,
    /// Seed for all randomness.
    #[arg(long)]
    seed: Option<u64>,
    /// TOML config file (flags override it).
    #[arg(long)]
    config: Option<PathBuf>,
    /// Computation engine. Only `classical` is available in this build.
    #[arg(long, value_enum, default_value = "classical")]
    engine: Engine,
    /// Output directory.
    #[arg(long, default_value = "tarafold-out")]
    out: PathBuf,
    /// Output format on stdout.
    #[arg(long, value_enum, default_value = "human")]
    format: Format,
}

#[derive(Clone, Copy, ValueEnum)]
enum SuiteArg {
    Smoke,
    Full,
}

#[derive(Args)]
struct BenchArgs {
    /// Which cases to run.
    #[arg(long, value_enum, default_value = "smoke")]
    suite: SuiteArg,
    /// Baseline file to compare against; exits 1 on regression.
    #[arg(long)]
    baseline: Option<PathBuf>,
    /// Write this run's aggregate into the baseline file (keeps other suites).
    #[arg(long)]
    write_baseline: Option<PathBuf>,
    /// Output directory for bench.json and bench.md.
    #[arg(long, default_value = "bench-out")]
    out: PathBuf,
}

#[derive(Args)]
struct SynthArgs {
    /// Registry name (see --list).
    name: Option<String>,
    /// List available datasets.
    #[arg(long)]
    list: bool,
    /// Rows to generate.
    #[arg(long, default_value_t = 2000)]
    rows: usize,
    /// Noise std as a fraction of the target's std.
    #[arg(long, default_value_t = 0.0)]
    noise: f64,
    /// Pure-noise distractor columns to add.
    #[arg(long, default_value_t = 0)]
    distractors: usize,
    /// Seed.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Output directory (writes data.csv and truth.json).
    #[arg(long, default_value = "synth-out")]
    out: PathBuf,
}

/// Error with an exit code (R2.6).
struct Failure {
    code: u8,
    error: anyhow::Error,
}

impl<E: Into<anyhow::Error>> From<E> for Failure {
    fn from(e: E) -> Self {
        let error = e.into();
        let code = if let Some(e) = error.downcast_ref::<EngineError>() {
            match e {
                EngineError::Config(_) => 2,
                EngineError::Data(_) => 3,
            }
        } else if let Some(e) = error.downcast_ref::<IoError>() {
            match e {
                IoError::Read(_) => 1,
                IoError::MissingColumn(_) => 2,
                _ => 3,
            }
        } else {
            1
        };
        Self { code, error }
    }
}

fn usage(msg: impl Into<String>) -> Failure {
    Failure {
        code: 2,
        error: anyhow!(msg.into()),
    }
}

fn parse_kv(s: &str) -> Result<(String, &str), Failure> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| usage(format!("expected COL=VALUE, got `{s}`")))?;
    Ok((k.trim().to_string(), v.trim()))
}

fn num(s: &str, what: &str) -> Result<f64, Failure> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| usage(format!("invalid number `{s}` in {what}")))
}

fn build_config(a: &RunArgs) -> Result<RunConfig, Failure> {
    let mut cfg = match &a.config {
        Some(p) => {
            let text = fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
            toml::from_str(&text)
                .map_err(|e| usage(format!("invalid config {}: {e}", p.display())))?
        }
        None => RunConfig::default(),
    };
    cfg.targets = a
        .target
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if let Some(v) = a.seed {
        cfg.seed = v;
    }
    if let Some(g) = a.goal {
        cfg.optimize.goal = Some(g);
    }
    if !a.control.is_empty() {
        cfg.optimize.controllables = a.control.iter().map(|c| c.trim().to_string()).collect();
    }
    for b in &a.bounds {
        let (k, v) = parse_kv(b)?;
        let (lo, hi) = v
            .split_once("..")
            .ok_or_else(|| usage(format!("expected COL=LO..HI, got `{b}`")))?;
        let (lo, hi) = (num(lo, "--bounds")?, num(hi, "--bounds")?);
        if lo > hi {
            return Err(usage(format!("lower bound exceeds upper bound in `{b}`")));
        }
        cfg.optimize.bounds.insert(k, [lo, hi]);
    }
    for f in &a.fix {
        let (k, v) = parse_kv(f)?;
        cfg.optimize.fixed.insert(k, num(v, "--fix")?);
    }
    if let Some(n) = a.candidates {
        cfg.optimize.n_candidates = n.clamp(1, 50);
    }
    if let Some(t) = a.tolerance {
        if !(t > 0.0 && t < 1.0) {
            return Err(usage("--tolerance must be between 0 and 1"));
        }
        cfg.optimize.tolerance = t;
    }
    if let Some(s) = a.data_source {
        cfg.data_source = match s {
            SourceArg::Observational => DataSource::Observational,
            SourceArg::Experimental => DataSource::Experimental,
            SourceArg::Simulation => DataSource::Simulation,
        };
    }
    if let Some(s) = a.split {
        cfg.split.strategy = match s {
            SplitArg::Random => SplitStrategy::Random,
            SplitArg::Ordered => SplitStrategy::Ordered,
        };
    }
    if let Some(s) = a.select {
        cfg.discover.select = Some(s);
    }
    if let Some(m) = a.max_features {
        cfg.discover.max_features = m.max(1);
    }
    Ok(cfg)
}

fn write(dir: &Path, name: &str, contents: &str) -> Result<(), Failure> {
    let p = dir.join(name);
    fs::write(&p, contents).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

fn cmd_run(a: &RunArgs) -> Result<u8, Failure> {
    match a.engine {
        Engine::Classical => {}
        Engine::Quantum | Engine::Hybrid => {
            return Err(usage(
                "engine not available in this build: only `classical` is implemented; \
                 quantum and hybrid engines are planned (see requirement R20)",
            ));
        }
    }
    let cfg = build_config(a)?;
    let tty = std::io::stderr().is_terminal();
    let mut opts = IngestOptions {
        required: cfg.targets.clone(),
        weights: a.weights.clone(),
        ..Default::default()
    };
    opts.required
        .extend(cfg.optimize.controllables.iter().cloned());
    let data = if a.input == "-" {
        let mut buf = Vec::new();
        std::io::stdin()
            .lock()
            .read_to_end(&mut buf)
            .context("reading stdin")?;
        read_csv(buf.as_slice(), "stdin", &opts)?
    } else {
        let f = fs::File::open(&a.input).with_context(|| format!("opening {}", a.input))?;
        read_csv(f, &a.input, &opts)?
    };
    if tty {
        eprintln!(
            "Read {} rows × {} numeric columns from {}",
            data.n_rows,
            data.columns.len(),
            data.source.path
        );
    }
    let (report, outcome) = tarafold_engine::run(&data, &cfg)?;

    fs::create_dir_all(&a.out).with_context(|| format!("creating {}", a.out.display()))?;
    let json = report.to_json()?;
    write(&a.out, "report.json", &json)?;
    write(&a.out, "equations.csv", &report.equations_csv()?)?;
    write(&a.out, "candidates.csv", &report.candidates_csv()?)?;
    let toml_text =
        toml::to_string_pretty(&cfg).map_err(|e| anyhow!("serializing run.toml: {e}"))?;
    write(&a.out, "run.toml", &toml_text)?;

    match a.format {
        Format::Json => println!("{json}"),
        Format::Human => print!("{}", summary::render(&report, &a.out)),
    }
    Ok(if outcome.goal_missed {
        5
    } else if outcome.low_grade {
        4
    } else {
        0
    })
}

fn cmd_bench(a: &BenchArgs) -> Result<u8, Failure> {
    let suite = match a.suite {
        SuiteArg::Smoke => tarafold_bench::Suite::Smoke,
        SuiteArg::Full => tarafold_bench::Suite::Full,
    };
    let tty = std::io::stderr().is_terminal();
    let report = tarafold_bench::run(suite, |i, n, r| {
        if tty {
            eprintln!(
                "[{i}/{n}] {} rows={} noise={} -> {}",
                r.case.dataset,
                r.case.rows,
                r.case.noise,
                if r.structure { "structure ok" } else { "miss" }
            );
        }
    });
    fs::create_dir_all(&a.out).with_context(|| format!("creating {}", a.out.display()))?;
    let json = serde_json::to_string_pretty(&report).map_err(|e| anyhow!(e.to_string()))?;
    write(&a.out, "bench.json", &json)?;
    let md = tarafold_bench::markdown(&report);
    write(&a.out, "bench.md", &md)?;
    print!("{md}");

    let read_baselines = |p: &Path| -> Result<serde_json::Map<String, serde_json::Value>, Failure> {
        if !p.exists() {
            return Ok(serde_json::Map::new());
        }
        let text = fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        serde_json::from_str(&text)
            .map_err(|e| usage(format!("invalid baseline {}: {e}", p.display())))
    };
    let mut code = 0;
    if let Some(p) = &a.baseline {
        let all = read_baselines(p)?;
        match all.get(suite.key()) {
            None => eprintln!(
                "note: {} has no `{}` baseline; skipping comparison",
                p.display(),
                suite.key()
            ),
            Some(v) => {
                let base: tarafold_bench::Aggregate = serde_json::from_value(v.clone())
                    .map_err(|e| usage(format!("invalid baseline entry: {e}")))?;
                let regressions = tarafold_bench::compare(&report.aggregate, &base);
                if regressions.is_empty() {
                    println!("\nNo regressions against {}.", p.display());
                } else {
                    println!("\nRegressions against {}:", p.display());
                    for r in &regressions {
                        println!(
                            "  {}: baseline {:.4}, now {:.4} (allowed change {:.2})",
                            r.metric, r.baseline, r.current, r.allowed
                        );
                    }
                    code = 1;
                }
            }
        }
    }
    if let Some(p) = &a.write_baseline {
        let mut all = read_baselines(p)?;
        let v = serde_json::to_value(&report.aggregate).map_err(|e| anyhow!(e.to_string()))?;
        all.insert(suite.key().into(), v);
        if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&all).map_err(|e| anyhow!(e.to_string()))?;
        fs::write(p, text + "\n").with_context(|| format!("writing {}", p.display()))?;
        println!("Wrote `{}` baseline to {}", suite.key(), p.display());
    }
    Ok(code)
}

fn cmd_synth(a: &SynthArgs) -> Result<u8, Failure> {
    if a.list {
        for e in tarafold_synth::registry() {
            println!("{:18} {:4} {}", e.name, e.tier, e.description);
        }
        return Ok(0);
    }
    let Some(name) = &a.name else {
        return Err(usage("give a dataset name or --list"));
    };
    let entry = tarafold_synth::find(name).map_err(|e| usage(e.to_string()))?;
    let g = tarafold_synth::generate(&entry, a.rows, a.noise, a.distractors, a.seed)
        .map_err(|e| usage(e.to_string()))?;
    fs::create_dir_all(&a.out).with_context(|| format!("creating {}", a.out.display()))?;
    write(
        &a.out,
        "data.csv",
        &g.to_csv().map_err(|e| anyhow!(e.to_string()))?,
    )?;
    let truth = serde_json::to_string_pretty(&g.truth).map_err(|e| anyhow!(e.to_string()))?;
    write(&a.out, "truth.json", &truth)?;
    println!(
        "Wrote {} rows of `{}` to {}\nTruth: {} = {}\nTry:   tarafold run {} --target {} --goal {} --control {}",
        a.rows,
        name,
        a.out.join("data.csv").display(),
        g.truth.target,
        g.truth.equation,
        a.out.join("data.csv").display(),
        g.truth.target,
        g.truth.example_goal,
        g.truth.controllables.join(",")
    );
    Ok(0)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Run(a) => cmd_run(a),
        Command::Synth(a) => cmd_synth(a),
        Command::Bench(a) => cmd_bench(a),
        Command::Schema => tarafold_report::schema_json()
            .map_err(Failure::from)
            .map(|s| {
                println!("{s}");
                0
            }),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(f) => {
            eprintln!("error: {:#}", f.error);
            ExitCode::from(f.code)
        }
    }
}
