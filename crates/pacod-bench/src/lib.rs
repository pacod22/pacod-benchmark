//! pacod-benchmark's service: the API the UI calls, the run queue and the database, and the
//! command line that runs a suite without them. `main.rs` is the binary around it.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use tokio::sync::watch;

pub mod agents;
pub mod api;
pub mod cli;
pub mod config;
pub mod gateways;
pub mod identity;
pub mod reports;
pub mod runs;
pub mod schedules;
pub mod secrets;
pub mod settings;
pub mod store;

use agents::Agents;
use api::{AppState, AuthMode};
use config::{Auth, Config};
use gateways::Gateways;
use runs::Runs;
use settings::Settings;
use store::Store;

/// The database, with a few tries: it may start at the same time.
pub async fn connect(url: &str) -> anyhow::Result<Store> {
    let mut wait = Duration::from_millis(500);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match Store::connect(url).await {
            Ok(s) => return Ok(s),
            Err(e) if attempt < 12 => {
                tracing::warn!(error = %e, attempt, "the database does not answer yet");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(5));
            }
            Err(e) => return Err(e).context("connecting to the database (DATABASE_URL)"),
        }
    }
}

/// The service, put together and started, but not listening yet.
pub struct App {
    pub api: Router,
    pub metrics: Router,
    pub runs: Arc<Runs>,
}

/// Migrates the database, writes the configured gateways as targets and opens the tokens of
/// those added through the UI, starts following pacod-id and the run queue (after taking down
/// what interrupted runs left, which needs their gateways).
pub async fn app(config: Arc<Config>, stop: watch::Receiver<bool>) -> anyhow::Result<App> {
    let store = connect(&config.database_url).await?;
    store.migrate().await.context("migrating the database")?;

    let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).build()?;
    match &config.keys {
        Some(keys) => tracing::info!(
            kek_id = %keys.current().id,
            kek_check = %keys.current().check_value(),
            "gateways may be added through the UI; their tokens are sealed under BENCH_KEK"
        ),
        None => tracing::info!("BENCH_KEK is not set: no gateway is added through the UI"),
    }
    let gateways = Arc::new(Gateways::load(&config, &store, &http).await?);
    let agents = Arc::new(Agents::load(&config, &store).await?);
    let settings = Arc::new(Settings::load(config.clone(), &store).await?);

    let auth = match &config.auth {
        Auth::Oidc(s) => AuthMode::Oidc(identity::Identity::start(s.clone(), stop.clone())),
        Auth::Dev { token } => {
            tracing::warn!("BENCH_AUTH=dev: one static token stands for a benchmark admin; never outside development");
            AuthMode::Dev { token: token.clone() }
        }
    };
    let (ended_tx, ended) = tokio::sync::mpsc::unbounded_channel();
    let runs =
        Arc::new(Runs::new(store.clone(), config.clone(), http.clone(), gateways.clone(), agents.clone(), ended_tx));
    let interrupted = store.recover().await?;
    if !interrupted.is_empty() {
        tracing::warn!(
            runs = interrupted.len(),
            "runs were going when the benchmark stopped; their scenes are taken down"
        );
    }
    runs.clone().spawn(interrupted, stop.clone());

    let state = AppState { config, store, runs: runs.clone(), auth: Arc::new(auth), http, gateways, agents, settings };
    schedules::spawn(state.clone(), ended, stop);
    let metrics = Router::new().route("/metrics", get(metrics)).with_state(runs.clone());
    Ok(App { api: api::router(state), metrics, runs })
}

/// `pacod-bench serve`: listens until `shutdown` resolves, then cancels a run still going and
/// waits for its scene to be taken down.
pub async fn serve(config: Config, shutdown: impl Future<Output = ()> + Send + 'static) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let (stop_tx, stop) = watch::channel(false);
    let app = app(config.clone(), stop).await?;
    if let Some(addr) = &config.metrics_listen {
        let listener = tokio::net::TcpListener::bind(addr.as_str()).await.with_context(|| format!("binding {addr}"))?;
        let metrics = app.metrics.clone();
        tokio::spawn(async move { axum::serve(listener, metrics).await });
    }
    let listener = tokio::net::TcpListener::bind(config.listen.as_str())
        .await
        .with_context(|| format!("binding {}", config.listen))?;
    tracing::info!(listen = %config.listen, "pacod-bench serving");
    axum::serve(listener, app.api).with_graceful_shutdown(shutdown).await?;

    let runs = app.runs;
    if let Some(id) = runs.active_id() {
        tracing::info!(run_id = %id, "cancelling the active run");
        runs.cancel_active(&id);
        let until = tokio::time::Instant::now() + Duration::from_secs(60);
        while runs.active_id().is_some() && tokio::time::Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    let _ = stop_tx.send(true);
    tracing::info!("pacod-bench stopped");
    Ok(())
}

/// `pacod-bench rewrap-secrets`: wraps the data key of every sealed gateway token again under the
/// current key-encryption key, after a rotation (the old key in `BENCH_KEK_OLD`). The tokens
/// themselves are not encrypted again, and a running service goes on with the ones it opened.
/// What it did: tokens moved, tokens already under the current key.
pub async fn rewrap_secrets(env: config::Env<'_>) -> anyhow::Result<(usize, usize)> {
    let database_url = config::secret(env, "DATABASE_URL")?.context("DATABASE_URL is not set")?;
    let keys =
        secrets::Keys::from_env(env)?.context("BENCH_KEK is not set: there is no key to wrap the tokens under")?;
    let store = connect(&database_url).await?;
    store.migrate().await.context("migrating the database")?;
    let (mut moved, mut current, mut failed) = (0, 0, Vec::new());
    for (id, admin_url, sealed) in store.sealed_tokens().await? {
        let context = secrets::token_context(&id, &admin_url);
        let result = serde_json::from_value::<secrets::Sealed>(sealed)
            .map_err(|e| e.to_string())
            .and_then(|s| secrets::rewrap(keys.all(), keys.current(), &context, &s).map_err(|e| e.to_string()));
        match result {
            Ok(Some(new)) => {
                store.set_sealed(&id, &serde_json::to_value(&new)?).await?;
                moved += 1;
            }
            Ok(None) => current += 1,
            Err(e) => {
                tracing::error!(target_id = %id, error = %e, "a gateway's token cannot be wrapped again");
                failed.push(id);
            }
        }
    }
    tracing::info!(moved, current, failed = failed.len(), kek_id = %keys.current().id, "tokens wrapped again");
    anyhow::ensure!(failed.is_empty(), "the tokens of {} cannot be opened with the configured keys", failed.join(", "));
    Ok((moved, current))
}

/// Prometheus' text format: the job `pacod-bench` (platform/prometheus/bench.yml).
async fn metrics(State(runs): State<Arc<Runs>>) -> String {
    let c = &runs.counters;
    let get = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
    format!(
        "# HELP bench_runs_started_total Runs started.\n# TYPE bench_runs_started_total counter\nbench_runs_started_total {}\n\
         # HELP bench_runs_ended_total Runs ended, by outcome.\n# TYPE bench_runs_ended_total counter\n\
         bench_runs_ended_total{{outcome=\"ok\"}} {}\nbench_runs_ended_total{{outcome=\"error\"}} {}\n\
         # HELP bench_cells_ended_total Cells ended, by outcome.\n# TYPE bench_cells_ended_total counter\n\
         bench_cells_ended_total{{outcome=\"ok\"}} {}\nbench_cells_ended_total{{outcome=\"error\"}} {}\n\
         # HELP bench_run_active Whether a run is going.\n# TYPE bench_run_active gauge\nbench_run_active {}\n",
        get(&c.runs_started),
        get(&c.runs_finished),
        get(&c.runs_failed),
        get(&c.cells_done),
        get(&c.cells_failed),
        u8::from(runs.active_id().is_some()),
    )
}
