//! pacod-benchmark's service: the API the UI calls, the run queue and the database, and the
//! command line that runs a suite without them. `main.rs` is the binary around it.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use pacod_bench_engine::gw::GwAdmin;
use serde_json::json;
use tokio::sync::watch;

pub mod api;
pub mod cli;
pub mod config;
pub mod identity;
pub mod reports;
pub mod runs;
pub mod store;

use api::{AppState, AuthMode};
use config::{Auth, Config};
use runs::Runs;
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

/// Migrates the database, writes the configured gateways as targets, starts following pacod-id
/// and the run queue (after taking down what interrupted runs left).
pub async fn app(config: Arc<Config>, stop: watch::Receiver<bool>) -> anyhow::Result<App> {
    let store = connect(&config.database_url).await?;
    store.migrate().await.context("migrating the database")?;

    let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).build()?;
    let mut gateways = BTreeMap::new();
    for g in &config.gateways {
        let definition = json!({ "admin_url": g.admin_url, "mock_url": g.mock_url, "data_plane": g.data_plane });
        store.upsert_config_target(&g.name, &definition, &g.declared).await?;
        gateways.insert(format!("cfg-{}", g.name), Arc::new(GwAdmin::new(&g.admin_url, &g.token, http.clone())));
    }
    store.forget_config_targets(&config.gateways.iter().map(|g| g.name.clone()).collect::<Vec<_>>()).await?;
    let gateways = Arc::new(gateways);

    let auth = match &config.auth {
        Auth::Oidc(s) => AuthMode::Oidc(identity::Identity::start(s.clone(), stop.clone())),
        Auth::Dev { token } => {
            tracing::warn!("BENCH_AUTH=dev: one static token stands for a benchmark admin; never outside development");
            AuthMode::Dev { token: token.clone() }
        }
    };
    let runs = Arc::new(Runs::new(store.clone(), config.clone(), http.clone(), gateways.clone()));
    let interrupted = store.recover().await?;
    if !interrupted.is_empty() {
        tracing::warn!(
            runs = interrupted.len(),
            "runs were going when the benchmark stopped; their scenes are taken down"
        );
    }
    runs.clone().spawn(interrupted, stop);

    let state = AppState { config, store, runs: runs.clone(), auth: Arc::new(auth), http, gateways };
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
    tracing::info!(listen = %config.listen, gateways = config.gateways.len(), agents = config.agents.len(), "pacod-bench serving");
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
