//! Local HTTP server for the Tarafold web UI (`tarafold serve`, design §8, R10.3, R14.4).
//!
//! Security model: binds to loopback by default; every `/api/*` request needs the random
//! 128-bit token as `Authorization: Bearer`; `Host` must name the bound address and `Origin`,
//! when present, must match it (DNS-rebinding defense); request bodies are limited to 1 MB;
//! one pipeline run at a time. The server never makes outbound connections.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use tarafold_core::config::{OptimizeConfig, SuggestConfig, SuggestMode};
use tarafold_core::{Dataset, RunConfig};
use tarafold_engine::EngineError;
use tarafold_report::Report;
use tarafold_report::html::{DataSample, UI_HTML};
use tokio::sync::broadcast;
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::BroadcastStream;

/// Request body limit (R14.4).
pub const BODY_LIMIT: usize = 1024 * 1024;
/// Rows of data sent to the UI.
const SAMPLE_ROWS: usize = 20_000;

/// Server errors.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// Could not bind or serve.
    #[error("server error: {0}")]
    Io(#[from] std::io::Error),
    /// Could not get random bytes for the token.
    #[error("could not generate an access token: {0}")]
    Random(String),
}

/// What the server serves.
pub struct ServeOptions {
    /// Address to bind (default `127.0.0.1:0`, port 0 picks a free port).
    pub addr: SocketAddr,
    /// The report shown at start.
    pub report: Report,
    /// The data the report was built from (needed for re-runs and charts).
    pub data: Option<Dataset>,
    /// Access token; generated when `None`.
    pub token: Option<String>,
}

/// Progress event sent over `GET /api/events`.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    /// `started`, `finished`, or `failed`.
    pub kind: &'static str,
    /// Human-readable message.
    pub message: String,
}

struct Inner {
    report: Report,
    data: Option<Dataset>,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<Inner>>,
    /// Held while a pipeline job runs (one at a time).
    busy: Arc<tokio::sync::Mutex<()>>,
    token: Arc<String>,
    hosts: Arc<Vec<String>>,
    events: broadcast::Sender<Progress>,
}

/// A running server.
pub struct Running {
    /// Bound address.
    pub addr: SocketAddr,
    /// Access token.
    pub token: String,
    handle: std::thread::JoinHandle<Result<(), ServerError>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Running {
    /// URL to open, with the token in the fragment (never sent to the server or logged).
    pub fn url(&self) -> String {
        format!("http://{}/#token={}", self.addr, self.token)
    }

    /// Block until the server stops.
    pub fn wait(self) -> Result<(), ServerError> {
        self.handle.join().unwrap_or_else(|_| {
            Err(ServerError::Io(std::io::Error::other(
                "server thread failed",
            )))
        })
    }

    /// Stop the server and wait for it.
    pub fn stop(mut self) -> Result<(), ServerError> {
        if let Some(s) = self.shutdown.take() {
            let _ = s.send(());
        }
        self.wait()
    }
}

fn new_token() -> Result<String, ServerError> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| ServerError::Random(e.to_string()))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Constant-time comparison of the presented token.
fn token_ok(given: &str, want: &str) -> bool {
    given.len() == want.len()
        && given
            .bytes()
            .zip(want.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// Start serving on a background thread. Returns once the socket is bound.
pub fn start(opts: ServeOptions) -> Result<Running, ServerError> {
    let token = match opts.token {
        Some(t) => t,
        None => new_token()?,
    };
    let std_listener = std::net::TcpListener::bind(opts.addr)?;
    std_listener.set_nonblocking(true)?;
    let addr = std_listener.local_addr()?;
    let mut hosts = vec![addr.to_string()];
    if addr.ip().is_loopback() {
        hosts.push(format!("localhost:{}", addr.port()));
    }
    let (events, _) = broadcast::channel(64);
    let state = AppState {
        inner: Arc::new(Mutex::new(Inner {
            report: opts.report,
            data: opts.data,
        })),
        busy: Arc::new(tokio::sync::Mutex::new(())),
        token: Arc::new(token.clone()),
        hosts: Arc::new(hosts),
        events,
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handle = std::thread::spawn(move || -> Result<(), ServerError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_io()
            .enable_time()
            .build()?;
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(std_listener)?;
            axum::serve(listener, router(state))
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await?;
            Ok(())
        })
    });
    Ok(Running {
        addr,
        token,
        handle,
        shutdown: Some(tx),
    })
}

fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/report", get(get_report))
        .route("/api/data", get(get_data))
        .route("/api/events", get(get_events))
        .route("/api/optimize", post(post_optimize))
        .route("/api/suggest", post(post_suggest))
        .route("/api/evaluate", post(post_evaluate))
        .route("/api/run", post(post_run))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .merge(api)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(state.clone(), check_host))
        .with_state(state)
}

fn json_error(status: StatusCode, msg: impl Into<String>) -> Response {
    let body = serde_json::json!({ "error": msg.into() }).to_string();
    (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        body,
    )
        .into_response()
}

/// `Host` must be the bound address; `Origin`, when sent, must be the same origin.
async fn check_host(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let h = req.headers();
    let host = h
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !s.hosts.iter().any(|x| x == host) {
        return json_error(StatusCode::FORBIDDEN, "unexpected Host header");
    }
    if let Some(origin) = h.get(header::ORIGIN) {
        let o = origin.to_str().unwrap_or("");
        if !s.hosts.iter().any(|x| o == format!("http://{x}")) {
            return json_error(StatusCode::FORBIDDEN, "cross-origin request refused");
        }
    }
    let mut resp = next.run(req).await;
    let hs = resp.headers_mut();
    hs.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    hs.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    hs.insert("x-frame-options", HeaderValue::from_static("DENY"));
    hs.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

async fn require_token(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !token_ok(given, &s.token) {
        return json_error(StatusCode::UNAUTHORIZED, "missing or wrong access token");
    }
    next.run(req).await
}

async fn index() -> Response {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8")),
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(
                    "default-src 'self' 'unsafe-inline' data: blob:; connect-src 'self'; frame-ancestors 'none'",
                ),
            ),
        ],
        UI_HTML,
    )
        .into_response()
}

fn lock(s: &AppState) -> std::sync::MutexGuard<'_, Inner> {
    s.inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn json_ok<T: Serialize>(v: &T) -> Response {
    match serde_json::to_string(v) {
        Ok(body) => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            body,
        )
            .into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_report(State(s): State<AppState>) -> Response {
    json_ok(&lock(&s).report)
}

async fn get_data(State(s): State<AppState>) -> Response {
    let g = lock(&s);
    match &g.data {
        Some(d) => json_ok(&DataSample::from_dataset(d, SAMPLE_ROWS)),
        None => json_ok(&serde_json::Value::Null),
    }
}

async fn get_events(State(s): State<AppState>) -> impl IntoResponse {
    let stream = BroadcastStream::new(s.events.subscribe()).filter_map(|m| {
        let m = m.ok()?;
        Some(Ok::<_, std::convert::Infallible>(
            Event::default().data(serde_json::to_string(&m).unwrap_or_default()),
        ))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Goals and inputs from the UI (mirrors the CLI goal flags).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct GoalRequest {
    goal: Option<f64>,
    require: Vec<String>,
    prefer: Vec<String>,
    control: Vec<String>,
    bounds: std::collections::BTreeMap<String, [f64; 2]>,
    constrain: Vec<String>,
    robust: Option<f64>,
    lexicographic: bool,
    // Suggestion settings (`/api/suggest` only).
    n: Option<usize>,
    mode: Option<String>,
    expand: Option<f64>,
}

fn optimize_config(saved: &Report, g: &GoalRequest) -> Result<OptimizeConfig, String> {
    let prev = &saved.run.config.optimize;
    let mut o = OptimizeConfig {
        integer: prev.integer.clone(),
        steps: prev.steps.clone(),
        fixed: prev.fixed.clone(),
        ..Default::default()
    };
    o.goal = g.goal.filter(|v| v.is_finite());
    o.require = g
        .require
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    o.prefer = g
        .prefer
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    o.constraints = g
        .constrain
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    o.controllables = if g.control.is_empty() {
        prev.controllables.clone()
    } else {
        g.control.clone()
    };
    for (k, b) in &g.bounds {
        if !(b[0].is_finite() && b[1].is_finite() && b[0] <= b[1]) {
            return Err(format!("invalid bounds for `{k}`"));
        }
        o.bounds.insert(k.clone(), *b);
    }
    // Discrete kinds and fixed values only apply to controllables that are still chosen.
    o.integer.retain(|n| o.controllables.contains(n));
    o.steps.retain(|n, _| o.controllables.contains(n));
    if let Some(r) = g.robust {
        if !(0.0..=1.0).contains(&r) {
            return Err("robust must be between 0 and 1".into());
        }
        o.robust = Some(r);
    }
    o.lexicographic = g.lexicographic;
    Ok(o)
}

fn engine_error(e: &EngineError) -> Response {
    json_error(StatusCode::BAD_REQUEST, e.to_string())
}

/// Re-run Stage 7 or 7b on the current report in a blocking task.
async fn rerun(s: AppState, g: GoalRequest, only_suggest: bool) -> Response {
    let Ok(_guard) = s.busy.clone().try_lock_owned() else {
        return json_error(StatusCode::CONFLICT, "another run is in progress");
    };
    let (saved, data) = {
        let g = lock(&s);
        (g.report.clone(), g.data.clone())
    };
    let Some(data) = data else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "the data is not loaded; start `tarafold serve` with --data <csv>",
        );
    };
    let opt = match optimize_config(&saved, &g) {
        Ok(o) => o,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, e),
    };
    if !only_suggest && opt.goal.is_none() && opt.require.is_empty() && opt.prefer.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "give at least one goal");
    }
    let sug = if only_suggest {
        let mode = match g.mode.as_deref().unwrap_or("balanced") {
            "explore" => SuggestMode::Explore,
            "exploit" => SuggestMode::Exploit,
            "balanced" => SuggestMode::Balanced,
            m => return json_error(StatusCode::BAD_REQUEST, format!("unknown mode `{m}`")),
        };
        SuggestConfig {
            n: g.n.unwrap_or(8).clamp(1, 100),
            mode,
            expand: g.expand.unwrap_or(0.0).clamp(0.0, 10.0),
        }
    } else {
        saved.run.config.suggest.clone()
    };
    let res = tokio::task::spawn_blocking(move || {
        tarafold_engine::rerun_from_report(&saved, &data, opt, sug, only_suggest)
    })
    .await;
    match res {
        Ok(Ok((report, _))) => {
            let mut g = lock(&s);
            // `suggest` keeps earlier optimization results.
            g.report = report;
            json_ok(&g.report)
        }
        Ok(Err(e)) => engine_error(&e),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn post_optimize(State(s): State<AppState>, body: axum::Json<GoalRequest>) -> Response {
    rerun(s, body.0, false).await
}

async fn post_suggest(State(s): State<AppState>, body: axum::Json<GoalRequest>) -> Response {
    rerun(s, body.0, true).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvaluateRequest {
    #[serde(default)]
    target: Option<String>,
    points: Vec<std::collections::BTreeMap<String, f64>>,
}

#[derive(Debug, Serialize)]
struct Evaluated {
    target: String,
    equation: String,
    predictions: Vec<f64>,
    /// 5th and 95th percentile of the bootstrap ensemble at each point.
    ensemble_range: Vec<[f64; 2]>,
}

/// Evaluate the selected equation (and its bootstrap ensemble) at given points.
async fn post_evaluate(State(s): State<AppState>, body: axum::Json<EvaluateRequest>) -> Response {
    let req = body.0;
    if req.points.len() > 10_000 {
        return json_error(StatusCode::BAD_REQUEST, "at most 10,000 points");
    }
    let g = lock(&s);
    let t = match &req.target {
        Some(n) => g.report.targets.iter().find(|t| &t.target == n),
        None => g.report.targets.first(),
    };
    let Some(t) = t else {
        return json_error(StatusCode::BAD_REQUEST, "unknown target");
    };
    let Some(eq) = t
        .discover
        .front
        .iter()
        .find(|e| e.id == t.discover.selected_id)
    else {
        return json_error(StatusCode::BAD_REQUEST, "no selected equation");
    };
    let expr = match tarafold_report::export::equation_expr(eq) {
        Ok(e) => e,
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.0),
    };
    let members: Vec<_> = t
        .validate
        .bootstrap
        .ensemble
        .iter()
        .map(|c| expr.with_constants(c))
        .collect();
    let mut predictions = Vec::with_capacity(req.points.len());
    let mut ranges = Vec::with_capacity(req.points.len());
    for p in &req.points {
        let x: Vec<f64> = eq
            .variables
            .iter()
            .map(|v| p.get(v).copied().unwrap_or(f64::NAN))
            .collect();
        predictions.push(expr.eval_point(&x));
        let mut m: Vec<f64> = members
            .iter()
            .map(|e| e.eval_point(&x))
            .filter(|v| v.is_finite())
            .collect();
        m.sort_by(f64::total_cmp);
        let q = |f: f64| {
            if m.is_empty() {
                f64::NAN
            } else {
                m[((m.len() - 1) as f64 * f).round() as usize]
            }
        };
        ranges.push([q(0.05), q(0.95)]);
    }
    json_ok(&Evaluated {
        target: t.target.clone(),
        equation: eq.infix.clone(),
        predictions,
        ensemble_range: ranges,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RunRequest {
    targets: Vec<String>,
    max_evals: Option<usize>,
    seed: Option<u64>,
}

/// Re-run the whole pipeline with new targets or budget; progress goes to `/api/events`.
async fn post_run(State(s): State<AppState>, body: axum::Json<RunRequest>) -> Response {
    let req = body.0;
    let Ok(_guard) = s.busy.clone().try_lock_owned() else {
        return json_error(StatusCode::CONFLICT, "another run is in progress");
    };
    let (mut cfg, data): (RunConfig, Option<Dataset>) = {
        let g = lock(&s);
        (g.report.run.config.clone(), g.data.clone())
    };
    let Some(data) = data else {
        return json_error(StatusCode::BAD_REQUEST, "the data is not loaded");
    };
    if !req.targets.is_empty() {
        cfg.targets = req.targets.iter().map(|t| t.trim().to_string()).collect();
        if cfg.targets.iter().any(|t| data.index_of(t).is_err()) {
            return json_error(StatusCode::BAD_REQUEST, "a target is not a numeric column");
        }
        // Goals and suggestions refer to the old targets; drop them on a new target set.
        cfg.optimize = OptimizeConfig::default();
        cfg.suggest = SuggestConfig::default();
    }
    if let Some(m) = req.max_evals {
        cfg.discover.gp.max_evals = m.clamp(1_000, 50_000_000);
    }
    if let Some(seed) = req.seed {
        cfg.seed = seed;
    }
    let tx = s.events.clone();
    let _ = tx.send(Progress {
        kind: "started",
        message: format!("Running discovery for {}", cfg.targets.join(", ")),
    });
    let res = tokio::task::spawn_blocking(move || tarafold_engine::run(&data, &cfg)).await;
    match res {
        Ok(Ok((report, _))) => {
            let _ = tx.send(Progress {
                kind: "finished",
                message: "Finished".into(),
            });
            let mut g = lock(&s);
            g.report = report;
            json_ok(&g.report)
        }
        Ok(Err(e)) => {
            let _ = tx.send(Progress {
                kind: "failed",
                message: e.to_string(),
            });
            engine_error(&e)
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests;
