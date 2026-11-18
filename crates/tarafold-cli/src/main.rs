//! `tarafold` command-line interface.

mod summary;

use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tarafold_core::config::{OptimizeConfig, SuggestConfig, SuggestMode};
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
    /// More detail on stderr (-v: stage timings; -vv: also configuration).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    /// Also write log messages to this file.
    #[arg(long, global = true, value_name = "FILE")]
    log_file: Option<PathBuf>,
    /// Worker threads (default: all cores). Results do not depend on it.
    #[arg(long, global = true)]
    threads: Option<usize>,
    #[command(subcommand)]
    command: Command,
}

/// Minimal logger for `-v` and `--log-file`. It logs settings and timings, never data values.
struct Log {
    level: u8,
    file: Option<std::sync::Mutex<fs::File>>,
}

impl Log {
    fn msg(&self, level: u8, text: &str) {
        if self.level >= level {
            eprintln!("{text}");
        }
        if let Some(f) = &self.file
            && let Ok(mut f) = f.lock()
        {
            use std::io::Write as _;
            let _ = writeln!(f, "{text}");
        }
    }
}

static LOG: std::sync::OnceLock<Log> = std::sync::OnceLock::new();

fn log(level: u8, text: &str) {
    if let Some(l) = LOG.get() {
        l.msg(level, text);
    }
}

/// Parse `90s`, `5m`, `1h`, or plain seconds.
fn parse_duration(s: &str) -> Result<std::time::Duration, Failure> {
    let s = s.trim();
    let (num_part, mult) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1.0),
        Some('m') => (&s[..s.len() - 1], 60.0),
        Some('h') => (&s[..s.len() - 1], 3600.0),
        _ => (s, 1.0),
    };
    let v = num(num_part, "--time-budget")? * mult;
    if v <= 0.0 {
        return Err(usage("--time-budget must be positive"));
    }
    Ok(std::time::Duration::from_secs_f64(v))
}

/// First Ctrl-C stops the search and still writes the report; a second one exits.
fn install_ctrl_c() {
    let _ = ctrlc::set_handler(|| {
        if tarafold_core::stop::interrupted() {
            eprintln!("\nStopped.");
            std::process::exit(130);
        }
        tarafold_core::stop::interrupt();
        eprintln!(
            "\nStopping: keeping the equations found so far and writing the report (Ctrl-C again to quit)."
        );
    });
}

#[derive(Subcommand)]
enum Command {
    /// Run the full pipeline: discover equations for a target, validate them, and optionally
    /// find inputs that reach a goal.
    Run(Box<RunArgs>),
    /// Generate a synthetic dataset from a known equation.
    Synth(SynthArgs),
    /// Re-run optimization on a saved report with new goals, without re-running discovery.
    Optimize(Box<OptimizeArgs>),
    /// Suggest next experiments from a saved report, without re-running discovery.
    Suggest(Box<SuggestArgs>),
    /// Run the known-equation benchmark and score discovery against the truth.
    Bench(BenchArgs),
    /// Profile every numeric column: statistics, histograms, outliers, flags.
    Profile(ProfileArgs),
    /// Show how each feature relates to a target and which would be passed to discovery.
    Relate(RelateArgs),
    /// Print the JSON Schema of report.json.
    Schema,
    /// Export an equation as standalone code (Python, JavaScript, SQL, Excel, LaTeX, JSON).
    Export(ExportArgs),
    /// Rebuild report.html for a saved report.
    Report(ReportArgs),
    /// Open the interactive workspace in the browser, served from this machine only.
    #[cfg(feature = "serve")]
    Serve(ServeArgs),
}

/// `tarafold serve <report-dir|csv>`.
#[cfg(feature = "serve")]
#[derive(Args)]
struct ServeArgs {
    /// A report directory from `tarafold run`, or a CSV file (then --target is required).
    input: PathBuf,
    /// Target column(s) when serving a CSV.
    #[arg(long, short, value_delimiter = ',')]
    target: Vec<String>,
    /// The CSV a report was built from (default: the path recorded in the report).
    #[arg(long)]
    data: Option<String>,
    /// Port (0 picks a free one).
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Address to bind. Anything other than loopback exposes the server to your network.
    #[arg(long, default_value = "127.0.0.1")]
    host: std::net::IpAddr,
    /// Do not open the browser.
    #[arg(long)]
    no_open: bool,
}

/// `tarafold report <report-dir>`.
#[derive(Args)]
struct ReportArgs {
    /// Directory with report.json from an earlier `tarafold run`.
    report_dir: PathBuf,
    /// Write report.html (the default and only output for now).
    #[arg(long, default_value_t = true)]
    html: bool,
    /// The CSV the report was built from (default: the path recorded in the report).
    #[arg(long)]
    data: Option<String>,
    /// Do not embed a data sample.
    #[arg(long)]
    no_data: bool,
    /// Open report.html in the default browser.
    #[arg(long)]
    open: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum ExportTo {
    Python,
    Js,
    Sql,
    Excel,
    Latex,
    Json,
}

/// `tarafold export <report-dir>`.
#[derive(Args)]
struct ExportArgs {
    /// Directory with report.json from an earlier `tarafold run`.
    report_dir: PathBuf,
    /// Output language.
    #[arg(long, value_enum)]
    to: ExportTo,
    /// Target to export (default: the first target).
    #[arg(long, short)]
    target: Option<String>,
    /// Front equation id (default: the selected equation).
    #[arg(long)]
    id: Option<usize>,
    /// Write to this file instead of stdout.
    #[arg(long)]
    out: Option<PathBuf>,
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

/// Goal, input, and constraint flags shared by `run`, `optimize`, and `suggest`.
#[derive(Args)]
struct GoalArgs {
    /// Desired target value; enables optimization. `a=1,b=2` sets exact goals on several targets.
    #[arg(long, allow_negative_numbers = true)]
    goal: Option<String>,
    /// Hard goal, repeatable: `col>=v`, `col<=v`, `col=v`, `col=a..b`.
    #[arg(long, value_name = "GOAL", allow_negative_numbers = true)]
    require: Vec<String>,
    /// Soft goal or objective, repeatable: `max:col[:w]`, `min:col[:w]`, or a threshold with `:w`.
    #[arg(long, value_name = "GOAL", allow_negative_numbers = true)]
    prefer: Vec<String>,
    /// Keep only candidates whose joint probability of meeting all hard goals is at least this.
    #[arg(long)]
    robust: Option<f64>,
    /// Pick the preferred trade-off by objective order instead of weights.
    #[arg(long)]
    lexicographic: bool,
    /// Linear constraint, repeatable, e.g. `spend_a + spend_b <= 50000`.
    #[arg(long, value_name = "EXPR", allow_negative_numbers = true)]
    constrain: Vec<String>,
    /// Whole-number controllables, comma-separated.
    #[arg(long, value_delimiter = ',')]
    integer: Vec<String>,
    /// Stepped controllable, repeatable: `hours=0.5`.
    #[arg(long, value_name = "COL=STEP")]
    step: Vec<String>,
    /// Largest change from baseline, repeatable: `price=20%` or `price=0.2`.
    #[arg(long, value_name = "COL=PCT")]
    max_change: Vec<String>,
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
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Explore,
    Exploit,
    Balanced,
}

/// `tarafold optimize <report-dir>`.
#[derive(Args)]
struct OptimizeArgs {
    /// Directory with report.json from an earlier `tarafold run`.
    report_dir: PathBuf,
    #[command(flatten)]
    g: GoalArgs,
    /// The CSV the report was built from (default: the path recorded in the report).
    #[arg(long)]
    data: Option<String>,
    /// Output directory (default: the report directory).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Output format on stdout.
    #[arg(long, value_enum, default_value = "human")]
    format: Format,
}

/// `tarafold suggest <report-dir>`.
#[derive(Args)]
struct SuggestArgs {
    /// Directory with report.json from an earlier `tarafold run`.
    report_dir: PathBuf,
    /// Number of suggestions.
    #[arg(short, long, default_value_t = 8)]
    n: usize,
    /// How suggestions are chosen.
    #[arg(long, value_enum, default_value = "balanced")]
    mode: ModeArg,
    /// Allow suggestions this far beyond the observed range (e.g. `10%`).
    #[arg(long)]
    expand: Option<String>,
    #[command(flatten)]
    g: GoalArgs,
    /// The CSV the report was built from (default: the path recorded in the report).
    #[arg(long)]
    data: Option<String>,
    /// Output directory (default: the report directory).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Output format on stdout.
    #[arg(long, value_enum, default_value = "human")]
    format: Format,
}

#[derive(Args)]
struct RunArgs {
    /// CSV file, or `-` for stdin.
    input: String,
    /// Target column(s), comma-separated. A target may use targets listed before it as inputs.
    #[arg(long, short, value_delimiter = ',', required = true)]
    target: Vec<String>,
    #[command(flatten)]
    g: GoalArgs,
    /// Do not use other targets as inputs for any target.
    #[arg(long)]
    exclude_other_targets: bool,
    /// Suggest this many next experiments (writes suggestions.csv).
    #[arg(long, value_name = "N")]
    suggest: Option<usize>,
    /// How suggestions are chosen.
    #[arg(long, value_enum)]
    suggest_mode: Option<ModeArg>,
    /// Allow suggestions this far beyond the observed range (e.g. `10%` or `0.1`).
    #[arg(long)]
    suggest_expand: Option<String>,
    /// Row-weight column.
    #[arg(long)]
    weights: Option<String>,
    /// How the data was produced.
    #[arg(long, value_enum)]
    data_source: Option<SourceArg>,
    /// Columns set deliberately in the experiment or simulation (default: controllables for
    /// experimental data, all features for simulation data).
    #[arg(long, value_delimiter = ',')]
    designed: Vec<String>,
    /// Bootstrap resamples for intervals (0 disables).
    #[arg(long)]
    bootstrap: Option<usize>,
    /// Resampled rediscoveries for structural stability (0 disables and caps the grade).
    #[arg(long)]
    stability: Option<usize>,
    /// Split strategy (`ordered` for time-ordered data).
    #[arg(long, value_enum)]
    split: Option<SplitArg>,
    /// Override the selected equation by front id.
    #[arg(long)]
    select: Option<usize>,
    /// Pick the equation by validation loss and complexity only, without the held-out-range check.
    #[arg(long)]
    no_extrapolation_guard: bool,
    /// Maximum features passed to discovery.
    #[arg(long)]
    max_features: Option<usize>,
    /// Unary operators for equation search, comma-separated
    /// (default: exp,log,sqrt,square,inv; also abs,sin,cos,tanh).
    #[arg(long, value_delimiter = ',')]
    operators: Vec<String>,
    /// Seed for all randomness.
    #[arg(long)]
    seed: Option<u64>,
    /// Equation-search budget in expression evaluations (default 240000).
    #[arg(long)]
    max_evals: Option<usize>,
    /// Stop equation search after this long (e.g. `90s`, `5m`) and keep what was found.
    /// A stopped search depends on machine speed; `--max-evals` keeps runs reproducible.
    #[arg(long)]
    time_budget: Option<String>,
    #[arg(long)]
    config: Option<PathBuf>,
    /// Computation engine (default `classical`, the only one in this build).
    #[arg(long, value_enum)]
    engine: Option<Engine>,
    /// Output directory.
    #[arg(long, default_value = "tarafold-out")]
    out: PathBuf,
    /// Open report.html in the default browser when done.
    #[arg(long)]
    open: bool,
    /// Output format on stdout.
    #[arg(long, value_enum, default_value = "human")]
    format: Format,
}

#[derive(Args)]
struct ProfileArgs {
    /// CSV file, or `-` for stdin.
    input: String,
    /// Output format on stdout.
    #[arg(long, value_enum, default_value = "human")]
    format: Format,
}

#[derive(Args)]
struct RelateArgs {
    /// CSV file, or `-` for stdin.
    input: String,
    /// Target column.
    #[arg(long, short)]
    target: String,
    /// Maximum features passed to discovery.
    #[arg(long)]
    max_features: Option<usize>,
    /// Seed.
    #[arg(long)]
    seed: Option<u64>,
    /// Output format on stdout.
    #[arg(long, value_enum, default_value = "human")]
    format: Format,
}

#[derive(Clone, Copy, ValueEnum)]
enum SuiteArg {
    Smoke,
    Full,
    /// Closed-loop experiment selection (suggest vs random vs Latin hypercube).
    Active,
    /// Multi-goal problems with known Pareto fronts (T11).
    Multi,
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
    /// Compare engines side by side, e.g. `classical,quantum,hybrid` (R20). Engines missing
    /// from this build are listed as unavailable.
    #[arg(long, value_delimiter = ',')]
    compare: Vec<String>,
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

/// Apply goal, input, and constraint flags to the optimization settings.
fn apply_goals(a: &GoalArgs, opt: &mut OptimizeConfig) -> Result<(), Failure> {
    if let Some(g) = &a.goal {
        let g = g.trim();
        match g.parse::<f64>() {
            Ok(v) if v.is_finite() => opt.goal = Some(v),
            _ if g.contains('=') => {
                // `a=1,b=2`: exact goals on several targets (R17.3).
                opt.require.extend(
                    g.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                );
            }
            _ => {
                return Err(usage(format!(
                    "invalid --goal `{g}`: give a number or col=value pairs"
                )));
            }
        }
    }
    opt.require.extend(a.require.iter().cloned());
    opt.prefer.extend(a.prefer.iter().cloned());
    if let Some(r) = a.robust {
        if !(0.0..=1.0).contains(&r) {
            return Err(usage("--robust must be between 0 and 1"));
        }
        opt.robust = Some(r);
    }
    if a.lexicographic {
        opt.lexicographic = true;
    }
    opt.constraints.extend(a.constrain.iter().cloned());
    opt.integer
        .extend(a.integer.iter().map(|s| s.trim().to_string()));
    for s in &a.step {
        let (k, v) = parse_kv(s)?;
        opt.steps.insert(k, num(v, "--step")?);
    }
    for s in &a.max_change {
        let (k, v) = parse_kv(s)?;
        let frac = match v.strip_suffix('%') {
            Some(p) => num(p, "--max-change")? / 100.0,
            None => num(v, "--max-change")?,
        };
        if frac < 0.0 {
            return Err(usage("--max-change must be non-negative"));
        }
        opt.max_change.insert(k, frac);
    }
    if !a.control.is_empty() {
        opt.controllables = a.control.iter().map(|c| c.trim().to_string()).collect();
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
        opt.bounds.insert(k, [lo, hi]);
    }
    for f in &a.fix {
        let (k, v) = parse_kv(f)?;
        opt.fixed.insert(k, num(v, "--fix")?);
    }
    if let Some(n) = a.candidates {
        opt.n_candidates = n.clamp(1, 50);
    }
    if let Some(t) = a.tolerance {
        if !(t > 0.0 && t < 1.0) {
            return Err(usage("--tolerance must be between 0 and 1"));
        }
        opt.tolerance = t;
    }
    Ok(())
}

fn parse_fraction(s: &str, what: &str) -> Result<f64, Failure> {
    let s = s.trim();
    let v = match s.strip_suffix('%') {
        Some(p) => num(p, what)? / 100.0,
        None => num(s, what)?,
    };
    if v < 0.0 {
        return Err(usage(format!("{what} must be non-negative")));
    }
    Ok(v)
}

fn mode_of(m: ModeArg) -> SuggestMode {
    match m {
        ModeArg::Explore => SuggestMode::Explore,
        ModeArg::Exploit => SuggestMode::Exploit,
        ModeArg::Balanced => SuggestMode::Balanced,
    }
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
    if let Some(m) = a.max_evals {
        cfg.discover.gp.max_evals = m.clamp(1_000, 1_000_000_000);
    }
    if a.exclude_other_targets {
        cfg.exclude_other_targets = true;
    }
    if let Some(e) = a.engine {
        cfg.engine = match e {
            Engine::Classical => "classical",
            Engine::Quantum => "quantum",
            Engine::Hybrid => "hybrid",
        }
        .into();
    }
    tarafold_engine::check_engine(&cfg.engine)?;
    apply_goals(&a.g, &mut cfg.optimize)?;
    if let Some(n) = a.suggest {
        cfg.suggest.n = n.min(100);
    }
    if let Some(m) = a.suggest_mode {
        cfg.suggest.mode = mode_of(m);
    }
    if let Some(x) = &a.suggest_expand {
        cfg.suggest.expand = parse_fraction(x, "--suggest-expand")?;
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
    if a.no_extrapolation_guard {
        cfg.discover.extrapolation_guard = false;
    }
    if let Some(s) = a.select {
        cfg.discover.select = Some(s);
    }
    if let Some(m) = a.max_features {
        cfg.discover.max_features = m.max(1);
    }
    if !a.designed.is_empty() {
        cfg.designed = a.designed.iter().map(|s| s.trim().to_string()).collect();
    }
    if let Some(b) = a.bootstrap {
        cfg.validate.bootstrap = b.min(10_000);
    }
    if let Some(s) = a.stability {
        cfg.validate.stability = s.min(200);
    }
    if !a.operators.is_empty() {
        cfg.discover.gp.operators = a.operators.iter().map(|s| s.trim().to_string()).collect();
    }
    Ok(cfg)
}

fn write(dir: &Path, name: &str, contents: &str) -> Result<(), Failure> {
    let p = dir.join(name);
    fs::write(&p, contents).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

fn cmd_run(a: &RunArgs) -> Result<u8, Failure> {
    let cfg = build_config(a)?;
    let tty = std::io::stderr().is_terminal();
    let mut opts = IngestOptions {
        required: cfg.targets.clone(),
        weights: a.weights.clone(),
        ..Default::default()
    };
    opts.required
        .extend(cfg.optimize.controllables.iter().cloned());
    let data = load(&a.input, &opts)?;
    if tty {
        eprintln!(
            "Read {} rows × {} numeric columns from {}",
            data.n_rows,
            data.columns.len(),
            data.source.path
        );
    }
    let (report, outcome) = {
        let budget = a.time_budget.as_deref().map(parse_duration).transpose()?;
        tarafold_core::stop::set_deadline(budget.map(|d| std::time::Instant::now() + d));
        install_ctrl_c();
        log(
            2,
            &format!(
                "configuration:\n{}",
                toml::to_string_pretty(&cfg).unwrap_or_default()
            ),
        );
        tarafold_engine::run(&data, &cfg)?
    };
    for s in &report.run.stages {
        log(1, &format!("stage {:<28} {:>8} ms", s.name, s.millis));
    }
    if report.run.status == tarafold_report::RunStatus::Interrupted {
        eprintln!("Run interrupted; writing what was found so far.");
    }

    fs::create_dir_all(&a.out).with_context(|| format!("creating {}", a.out.display()))?;
    let json = report.to_json()?;
    write(&a.out, "report.json", &json)?;
    write(&a.out, "equations.csv", &report.equations_csv()?)?;
    write(&a.out, "candidates.csv", &report.candidates_csv()?)?;
    let toml_text =
        toml::to_string_pretty(&cfg).map_err(|e| anyhow!("serializing run.toml: {e}"))?;
    write(&a.out, "run.toml", &toml_text)?;
    write_suggestions(&a.out, &report, &data)?;
    write_html(&a.out, &report, Some(&data))?;
    if a.open {
        open_in_browser(&a.out.join("report.html"));
    }

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
        SuiteArg::Multi => {
            let tty = std::io::stderr().is_terminal();
            let r = tarafold_bench::multi::run_multi(1, |x| {
                if tty {
                    eprintln!(
                        "{}: HV ratio {:.3}{}",
                        x.problem,
                        x.hypervolume_ratio,
                        x.error
                            .as_deref()
                            .map(|e| format!(" ({e})"))
                            .unwrap_or_default()
                    );
                }
            });
            fs::create_dir_all(&a.out).with_context(|| format!("creating {}", a.out.display()))?;
            let json = serde_json::to_string_pretty(&r).map_err(|e| anyhow!(e.to_string()))?;
            write(&a.out, "multi.json", &json)?;
            let md = tarafold_bench::multi::markdown(&r);
            write(&a.out, "multi.md", &md)?;
            print!("{md}");
            return Ok(0);
        }
        SuiteArg::Active => {
            let tty = std::io::stderr().is_terminal();
            let r = tarafold_bench::active::run_active(&[1, 2, 3], |l| {
                if tty {
                    eprintln!(
                        "{} {:?} seed {}: goal after {:?}, recovery after {:?}",
                        l.dataset,
                        l.method,
                        l.seed,
                        l.experiments_to_goal,
                        l.experiments_to_recovery
                    );
                }
            });
            fs::create_dir_all(&a.out).with_context(|| format!("creating {}", a.out.display()))?;
            let json = serde_json::to_string_pretty(&r).map_err(|e| anyhow!(e.to_string()))?;
            write(&a.out, "active.json", &json)?;
            let md = tarafold_bench::active::markdown(&r);
            write(&a.out, "active.md", &md)?;
            print!("{md}");
            return Ok(0);
        }
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
    if !a.compare.is_empty() {
        let mut t = String::from(
            "\n## Engine comparison\n\n| engine | status | cases | structure | R²>0.999 | inverse | seconds |\n|---|---|---|---|---|---|---|\n",
        );
        for e in &a.compare {
            let e = e.trim();
            if tarafold_engine::check_engine(e).is_ok() {
                let g = &report.aggregate;
                t.push_str(&format!(
                    "| {e} | ran | {} | {:.0}% | {:.0}% | {:.1}% | {:.0} |\n",
                    g.cases,
                    100.0 * g.structure_rate,
                    100.0 * g.accuracy_rate,
                    100.0 * g.inverse_success,
                    g.seconds
                ));
            } else {
                t.push_str(&format!(
                    "| {e} | not available in this build | — | — | — | — | — |\n"
                ));
            }
        }
        write(&a.out, "compare.md", &t)?;
        print!("{t}");
    }

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

fn load(input: &str, opts: &IngestOptions) -> Result<tarafold_core::Dataset, Failure> {
    Ok(if input == "-" {
        let mut buf = Vec::new();
        std::io::stdin()
            .lock()
            .read_to_end(&mut buf)
            .context("reading stdin")?;
        read_csv(buf.as_slice(), "stdin", opts)?
    } else {
        let f = fs::File::open(input).with_context(|| format!("opening {input}"))?;
        read_csv(f, input, opts)?
    })
}

fn cmd_profile(a: &ProfileArgs) -> Result<u8, Failure> {
    let data = load(&a.input, &IngestOptions::default())?;
    let prof = tarafold_profile::profile(&data);
    match a.format {
        Format::Json => {
            let s = serde_json::to_string_pretty(&prof).map_err(|e| anyhow!(e.to_string()))?;
            println!("{s}");
        }
        Format::Human => {
            println!(
                "{} rows × {} numeric columns from {}",
                data.n_rows,
                data.columns.len(),
                data.source.path
            );
            for e in &data.source.excluded {
                println!("  excluded {} ({})", e.name, e.reason);
            }
            println!(
                "{:<20} {:>12} {:>12} {:>12} {:>12} {:>7} {:>8} {:>8}  flags",
                "column", "min", "median", "max", "std", "skew", "unique", "outliers"
            );
            for p in &prof {
                let mut flags = Vec::new();
                if p.integer_valued {
                    flags.push("integer");
                }
                if p.bounded_unit {
                    flags.push("in [0,1]");
                }
                if p.all_positive {
                    flags.push("positive");
                }
                println!(
                    "{:<20} {:>12.5} {:>12.5} {:>12.5} {:>12.5} {:>7.2} {:>8} {:>8}  {}",
                    p.name,
                    p.min,
                    p.quantiles.p50,
                    p.max,
                    p.std,
                    p.skewness,
                    p.unique,
                    p.outliers,
                    flags.join(", ")
                );
            }
        }
    }
    Ok(0)
}

fn cmd_relate(a: &RelateArgs) -> Result<u8, Failure> {
    let opts = IngestOptions {
        required: vec![a.target.clone()],
        ..Default::default()
    };
    let data = load(&a.input, &opts)?;
    let mut cfg = RunConfig {
        targets: vec![a.target.clone()],
        ..Default::default()
    };
    if let Some(s) = a.seed {
        cfg.seed = s;
    }
    if let Some(m) = a.max_features {
        cfg.discover.max_features = m.max(1);
    }
    let (rep, screening) = tarafold_engine::relate(&data, &cfg, &a.target)?;
    match a.format {
        Format::Json => {
            let v =
                serde_json::json!({ "target": a.target, "relate": rep, "screening": screening });
            let s = serde_json::to_string_pretty(&v).map_err(|e| anyhow!(e.to_string()))?;
            println!("{s}");
        }
        Format::Human => {
            println!("Target {}  ({} rows)", a.target, data.n_rows);
            for b in &rep.baselines {
                println!(
                    "Baseline {:<6} R²(val) {:.4}  R²(test) {:.4}  [{}]",
                    b.model,
                    b.val_r2,
                    b.test_r2.unwrap_or(f64::NAN),
                    b.detail
                );
            }
            println!(
                "\n{:<20} {:>8} {:>9} {:>8} {:>8} {:>11} {:>8}  discovery",
                "feature", "pearson", "spearman", "MI r²", "dCor", "importance", "VIF"
            );
            for (f, s) in rep.features.iter().zip(&screening) {
                let verdict = if s.kept {
                    "kept".to_string()
                } else {
                    format!("dropped: {}", s.reason)
                };
                println!(
                    "{:<20} {:>8.3} {:>9.3} {:>8.3} {:>8.3} {:>11.4} {:>8.1}  {verdict}",
                    f.feature,
                    f.pearson,
                    f.spearman,
                    f.mi_r2,
                    f.distance_correlation,
                    f.importance,
                    f.vif.min(1e6)
                );
            }
            if !rep.interactions.is_empty() {
                println!("\nInteractions (Friedman H², strongest first):");
                for i in rep.interactions.iter().take(5) {
                    println!("  {} × {}  H² = {:.3}", i.a, i.b, i.h2);
                }
            }
            for p in &rep.collinear {
                println!("Collinear: {} ~ {} (r = {:.3})", p.a, p.b, p.r);
            }
            for l in &rep.leakage {
                println!("Possible leakage: `{l}` has |r| > 0.999 with the target");
            }
            println!(
                "\nThese measure association in this data, not the effect of changing a feature."
            );
        }
    }
    Ok(0)
}

/// Columns for suggestions.csv: the dataset's numeric columns, then excluded columns (empty).
fn write_suggestions(
    dir: &Path,
    report: &tarafold_report::Report,
    data: &tarafold_core::Dataset,
) -> Result<(), Failure> {
    let Some(s) = &report.suggest else {
        return Ok(());
    };
    let mut columns = data.names();
    columns.extend(data.source.excluded.iter().map(|e| e.name.clone()));
    let rows: Vec<std::collections::BTreeMap<String, f64>> =
        s.suggestions.iter().map(|x| x.row.clone()).collect();
    write(
        dir,
        "suggestions.csv",
        &report.suggestions_csv(&columns, &rows)?,
    )
}

/// Rows of the input data embedded in report.html for charts.
const HTML_SAMPLE_ROWS: usize = 20_000;

fn write_html(
    dir: &Path,
    report: &tarafold_report::Report,
    data: Option<&tarafold_core::Dataset>,
) -> Result<(), Failure> {
    use tarafold_report::html::{DataSample, report_html};
    let sample = data.map(|d| DataSample::from_dataset(d, HTML_SAMPLE_ROWS));
    write(dir, "report.html", &report_html(report, sample.as_ref())?)
}

/// Open a file with the system's default handler. Failures are reported, not fatal.
fn open_in_browser(path: &Path) {
    let r = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path).spawn()
    };
    if let Err(e) = r {
        eprintln!("note: could not open {}: {e}", path.display());
    }
}

fn cmd_report(a: &ReportArgs) -> Result<u8, Failure> {
    let report = read_report(&a.report_dir)?;
    let data = if a.no_data {
        None
    } else {
        let path = a
            .data
            .clone()
            .unwrap_or_else(|| report.dataset.source.path.clone());
        let opts = IngestOptions {
            required: report.run.config.targets.clone(),
            ..Default::default()
        };
        match load(&path, &opts) {
            Ok(d) => Some(d),
            Err(f) => {
                eprintln!(
                    "note: data not embedded ({:#}); pass --data <csv> to include it",
                    f.error
                );
                None
            }
        }
    };
    write_html(&a.report_dir, &report, data.as_ref())?;
    let p = a.report_dir.join("report.html");
    println!("Wrote {}", p.display());
    if a.open {
        open_in_browser(&p);
    }
    Ok(0)
}

#[cfg(feature = "serve")]
fn cmd_serve(a: &ServeArgs) -> Result<u8, Failure> {
    let (report, data) = if a.input.is_dir() {
        let report = read_report(&a.input)?;
        let path = a
            .data
            .clone()
            .unwrap_or_else(|| report.dataset.source.path.clone());
        let opts = IngestOptions {
            required: report.run.config.targets.clone(),
            ..Default::default()
        };
        let data = match load(&path, &opts) {
            Ok(d) => Some(d),
            Err(f) => {
                eprintln!(
                    "note: data not loaded ({:#}); charts and re-runs need --data <csv>",
                    f.error
                );
                None
            }
        };
        (report, data)
    } else {
        if a.target.is_empty() {
            return Err(usage("serving a CSV needs --target"));
        }
        let input = a.input.to_string_lossy().to_string();
        let opts = IngestOptions {
            required: a.target.clone(),
            ..Default::default()
        };
        let data = load(&input, &opts)?;
        let cfg = RunConfig {
            targets: a.target.iter().map(|t| t.trim().to_string()).collect(),
            ..Default::default()
        };
        eprintln!("Running discovery for {} ...", cfg.targets.join(", "));
        let (report, _) = tarafold_engine::run(&data, &cfg)?;
        (report, Some(data))
    };
    if !a.host.is_loopback() {
        eprintln!(
            "warning: binding {} makes the workspace reachable from your network; anyone with the URL can use it",
            a.host
        );
    }
    let running = tarafold_server::start(tarafold_server::ServeOptions {
        addr: std::net::SocketAddr::new(a.host, a.port),
        report,
        data,
        token: None,
    })
    .map_err(|e| anyhow!(e.to_string()))?;
    let url = running.url();
    println!("Tarafold workspace: {url}");
    println!("The token in the URL is required; keep it private. Press Ctrl-C to stop.");
    if !a.no_open {
        open_url(&url);
    }
    running.wait().map_err(|e| anyhow!(e.to_string()))?;
    Ok(0)
}

#[cfg(feature = "serve")]
fn open_url(url: &str) {
    let r = if cfg!(target_os = "windows") {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = r {
        eprintln!("note: could not open a browser ({e}); open the URL above");
    }
}

fn read_report(report_dir: &Path) -> Result<tarafold_report::Report, Failure> {
    let rp = report_dir.join("report.json");
    let text = fs::read_to_string(&rp).with_context(|| format!("reading {}", rp.display()))?;
    tarafold_report::Report::from_json(&text)
        .map_err(|e| usage(format!("{} is not a Tarafold report: {e}", rp.display())))
}

fn cmd_export(a: &ExportArgs) -> Result<u8, Failure> {
    use tarafold_report::export::{ExportFormat, export};
    let report = read_report(&a.report_dir)?;
    let fmt = match a.to {
        ExportTo::Python => ExportFormat::Python,
        ExportTo::Js => ExportFormat::Js,
        ExportTo::Sql => ExportFormat::Sql,
        ExportTo::Excel => ExportFormat::Excel,
        ExportTo::Latex => ExportFormat::Latex,
        ExportTo::Json => ExportFormat::Json,
    };
    let code = export(&report, a.target.as_deref(), a.id, fmt).map_err(|e| usage(e.0))?;
    match &a.out {
        Some(p) => {
            fs::write(p, &code).with_context(|| format!("writing {}", p.display()))?;
            eprintln!("Wrote {}", p.display());
        }
        None => print!("{code}"),
    }
    Ok(0)
}

/// `tarafold optimize|suggest <report-dir>`: reload the report and data, apply new goals, rerun
/// optimization and/or suggestions.
fn cmd_saved(
    report_dir: &Path,
    g: &GoalArgs,
    data_path: Option<&str>,
    out: Option<&Path>,
    format: Format,
    suggest: Option<SuggestConfig>,
) -> Result<u8, Failure> {
    let saved = read_report(report_dir)?;
    if saved.dataset.weighted {
        return Err(usage(
            "this report used row weights; re-run `tarafold run` with --weights instead",
        ));
    }
    let path = data_path
        .map(str::to_string)
        .unwrap_or_else(|| saved.dataset.source.path.clone());
    let opts = IngestOptions {
        required: saved.run.config.targets.clone(),
        ..Default::default()
    };
    let data = load(&path, &opts)?;
    let mut opt = OptimizeConfig::default();
    apply_goals(g, &mut opt)?;
    let wants_opt = opt.goal.is_some() || !opt.require.is_empty() || !opt.prefer.is_empty();
    if suggest.is_none() && !wants_opt {
        return Err(usage("give a goal: --goal, --require, or --prefer"));
    }
    // `suggest` uses the goals to aim suggestions but does not run the optimizer.
    let only_suggest = suggest.is_some();
    let sug = suggest.unwrap_or_default();
    let (report, outcome) =
        tarafold_engine::rerun_from_report(&saved, &data, opt, sug, only_suggest)?;
    let out = out.unwrap_or(report_dir);
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let json = report.to_json()?;
    write(out, "report.json", &json)?;
    write(out, "candidates.csv", &report.candidates_csv()?)?;
    write_suggestions(out, &report, &data)?;
    write_html(out, &report, Some(&data))?;
    match format {
        Format::Json => println!("{json}"),
        Format::Human => {
            // `suggest` keeps earlier optimization results in report.json but only shows suggestions.
            let mut shown = report.clone();
            if only_suggest {
                shown.optimize = None;
            }
            print!("{}", summary::render(&shown, out));
        }
    }
    Ok(if outcome.goal_missed && !only_suggest {
        5
    } else {
        0
    })
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
    let file = match &cli.log_file {
        Some(p) => match fs::File::create(p) {
            Ok(f) => Some(std::sync::Mutex::new(f)),
            Err(e) => {
                eprintln!("error: cannot open log file {}: {e}", p.display());
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let _ = LOG.set(Log {
        level: cli.verbose,
        file,
    });
    if let Some(n) = cli.threads
        && let Err(e) = rayon::ThreadPoolBuilder::new()
            .num_threads(n.max(1))
            .build_global()
    {
        eprintln!("warning: --threads ignored: {e}");
    }
    let result = match &cli.command {
        Command::Run(a) => cmd_run(a),
        Command::Synth(a) => cmd_synth(a),
        Command::Bench(a) => cmd_bench(a),
        Command::Optimize(a) => cmd_saved(
            &a.report_dir,
            &a.g,
            a.data.as_deref(),
            a.out.as_deref(),
            a.format,
            None,
        ),
        Command::Suggest(a) => {
            let expand = match &a.expand {
                Some(x) => parse_fraction(x, "--expand"),
                None => Ok(0.0),
            };
            match expand {
                Ok(expand) => cmd_saved(
                    &a.report_dir,
                    &a.g,
                    a.data.as_deref(),
                    a.out.as_deref(),
                    a.format,
                    Some(SuggestConfig {
                        n: a.n.clamp(1, 100),
                        mode: mode_of(a.mode),
                        expand,
                    }),
                ),
                Err(e) => Err(e),
            }
        }
        Command::Profile(a) => cmd_profile(a),
        Command::Relate(a) => cmd_relate(a),
        Command::Export(a) => cmd_export(a),
        Command::Report(a) => cmd_report(a),
        #[cfg(feature = "serve")]
        Command::Serve(a) => cmd_serve(a),
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
