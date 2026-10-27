//! The command line: a suite run on a target from a terminal, without a database, a sign-in or a
//! UI. It runs wherever the binary runs (Linux, macOS), next to agents anywhere, and writes the
//! run's report (pacod-bench-report/1) to a file that can be carried to another installation
//! and uploaded in pacod-gw's console.
//!
//! A target file (TOML) names the target and where its secrets are, never the secrets:
//!
//! ```toml
//! name = "lite on the laptop"
//! product = "pacod_gw_lite"          # pacod_gw, pacod_gw_lite, third_party
//! [external]                          # a URL and a key; or [managed], below
//! base_url = "http://127.0.0.1:18280"
//! model = "openai/gpt-mock"
//! key_env = "BENCH_TARGET_KEY"
//! paid = false
//! mock = { family = "openai", delay_ms = 200, chunk_delay_ms = 20, chunks = 20 }
//! # [managed]
//! # admin_url = "http://127.0.0.1:18081"
//! # admin_token_env = "BENCH_GW_ADMIN_TOKEN"
//! # mock_url = "http://gw-load-mock:9000"
//! # data_plane = "http://127.0.0.1:18080"
//! [declared]
//! layout = "lite"
//! arch = "arm64"
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail, ensure};
use clap::Args;
use pacod_bench_engine::agents::{AgentClient, apply_placement, parse_agents};
use pacod_bench_engine::controller::{self, CellResult, RunEvent, RunInput, Target};
use pacod_bench_engine::gw::GwAdmin;
use pacod_bench_engine::model::{MockProfile, Placement, TargetKind};
use pacod_bench_engine::plan::{CellKind, Suite, expand, with_declared_mock};
use pacod_bench_engine::report::{RunInfo, encode};
use pacod_bench_engine::requests::Names;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};

use crate::config::{check_url, secret};

#[derive(Args)]
pub struct RunArgs {
    /// The suite (YAML or JSON).
    #[arg(long)]
    suite: PathBuf,
    /// The target (TOML; see `pacod-bench run --help` in docs/runbook.md).
    #[arg(long)]
    target: PathBuf,
    /// The agents, `name=URL,…`; BENCH_AGENTS when left out. Their token is BENCH_AGENT_TOKEN.
    #[arg(long)]
    agents: Option<String>,
    /// Where each agent stands, `name=in_stack|same_host|remote,…`; BENCH_AGENT_PLACEMENT.
    #[arg(long)]
    placement: Option<String>,
    /// The mock as the agents reach it, for the baseline cells; BENCH_BASELINE_MOCK_URL.
    #[arg(long)]
    baseline_mock_url: Option<String>,
    /// The report (pacod-bench-report/1).
    #[arg(long, default_value = "bench-report.json")]
    out: PathBuf,
    /// How long a cell waits for a busy agent.
    #[arg(long, default_value_t = 300)]
    agent_wait_s: u64,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// A report file.
    file: PathBuf,
}

#[derive(Args)]
pub struct PlanArgs {
    /// The suite (YAML or JSON).
    #[arg(long)]
    suite: PathBuf,
    /// `managed` or `external`.
    #[arg(long, default_value = "managed")]
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetFile {
    name: String,
    #[serde(default = "default_product")]
    product: String,
    #[serde(default)]
    external: Option<ExternalFile>,
    #[serde(default)]
    managed: Option<ManagedFile>,
    #[serde(default)]
    declared: toml::Table,
}

fn default_product() -> String {
    "pacod_gw".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalFile {
    base_url: String,
    model: String,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    upstream: String,
    key_env: String,
    #[serde(default = "default_true")]
    paid: bool,
    #[serde(default)]
    mock: Option<MockProfile>,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedFile {
    admin_url: String,
    admin_token_env: String,
    mock_url: String,
    #[serde(default)]
    data_plane: Option<String>,
}

fn read_suite(path: &PathBuf) -> anyhow::Result<Suite> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Suite::parse(&text).map_err(anyhow::Error::msg)
}

pub fn plan(a: PlanArgs) -> anyhow::Result<()> {
    let suite = read_suite(&a.suite)?;
    let kind = match a.kind.as_str() {
        "managed" => TargetKind::Managed,
        "external" => TargetKind::External,
        other => bail!("--kind: {other:?} is neither managed nor external"),
    };
    let plan = expand(&suite, kind, 256).map_err(anyhow::Error::msg)?;
    println!("{}", serde_json::to_string_pretty(&plan)?);
    Ok(())
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub async fn run(a: RunArgs) -> anyhow::Result<()> {
    let mut suite = read_suite(&a.suite)?;
    let text = std::fs::read_to_string(&a.target).with_context(|| format!("reading {}", a.target.display()))?;
    let t: TargetFile = toml::from_str(&text).with_context(|| format!("{}", a.target.display()))?;
    let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).build()?;
    let mut notes = Vec::new();
    let (target, kind, definition) = match (t.external, t.managed) {
        (Some(e), None) => {
            check_url(&e.base_url).context("external.base_url")?;
            let key = env(&e.key_env)
                .filter(|k| !k.trim().is_empty())
                .with_context(|| format!("{} is not set", e.key_env))?;
            if e.paid {
                ensure!(
                    suite.base.load.max_calls.is_some(),
                    "a target that may reach a paid provider needs load.max_calls (or paid = false in the target file)"
                );
            }
            notes.extend(with_declared_mock(&mut suite, e.mock));
            let definition = json!({ "base_url": e.base_url, "model": e.model, "provider": e.provider, "upstream": e.upstream, "paid": e.paid, "mock": e.mock });
            let names = Names { model: e.model, provider: e.provider, upstream: e.upstream };
            (
                Target::External { base_url: e.base_url.trim_end_matches('/').to_owned(), key, names, paid: e.paid },
                TargetKind::External,
                definition,
            )
        }
        (None, Some(m)) => {
            check_url(&m.admin_url).context("managed.admin_url")?;
            check_url(&m.mock_url).context("managed.mock_url")?;
            let token = env(&m.admin_token_env)
                .filter(|k| !k.trim().is_empty())
                .with_context(|| format!("{} is not set", m.admin_token_env))?;
            let gw = Arc::new(GwAdmin::new(&m.admin_url, &token, http.clone()));
            let definition = json!({ "admin_url": m.admin_url, "mock_url": m.mock_url, "data_plane": m.data_plane });
            (Target::Managed { gw, mock_url: m.mock_url, data_plane: m.data_plane }, TargetKind::Managed, definition)
        }
        _ => bail!("the target file has [external] or [managed], one of them"),
    };
    let plan = expand(&suite, kind, 256).map_err(anyhow::Error::msg)?;

    let agents_text = a.agents.or_else(|| env("BENCH_AGENTS")).context("--agents (or BENCH_AGENTS) is not set")?;
    let mut agents = parse_agents(&agents_text).map_err(anyhow::Error::msg)?;
    apply_placement(&a.placement.or_else(|| env("BENCH_AGENT_PLACEMENT")).unwrap_or_default(), &mut agents)
        .map_err(anyhow::Error::msg)?;
    ensure!(!agents.is_empty(), "no agents");
    let lookup = |name: &str| env(name);
    let token =
        secret(&lookup, "BENCH_AGENT_TOKEN")?.context("BENCH_AGENT_TOKEN (or BENCH_AGENT_TOKEN_FILE) is not set")?;
    let clients: Vec<AgentClient> = agents.iter().map(|r| AgentClient::new(r.clone(), &token, http.clone())).collect();

    let id = format!("cli_{}", uuid::Uuid::now_v7().simple());
    let short = crate::runs::short(&id);
    let baseline_url = a.baseline_mock_url.or_else(|| env("BENCH_BASELINE_MOCK_URL"));
    let total = plan.cells.len();
    eprintln!(
        "{} cells ({} baselines), about {} s, {} calls; {}",
        total,
        plan.estimate.baseline_cells,
        plan.estimate.duration_s,
        plan.estimate.calls,
        notes.join("; ")
    );
    let input = RunInput {
        id: id.clone(),
        short,
        target,
        agents: clients,
        plan: plan.clone(),
        baseline_url,
        agent_wait: Duration::from_secs(a.agent_wait_s),
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (cancel_tx, cancel) = watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        eprintln!("cancelling; the scene is taken down first");
        let _ = cancel_tx.send(true);
    });
    let task = tokio::spawn(controller::run(input, tx, cancel));
    let mut cells: Vec<CellResult> = Vec::new();
    let mut observed = Value::Null;
    while let Some(e) = rx.recv().await {
        match e {
            RunEvent::CellDone(c) => {
                eprintln!("{}", line(&c, total));
                cells.push(*c);
            }
            RunEvent::Observed(v) => observed = v,
            RunEvent::Warning(w) => eprintln!("warning: {w}"),
            RunEvent::Status { status, .. } if !status.finished() => eprintln!("run {}", status.as_str()),
            _ => {}
        }
    }
    let outcome = task.await.context("the run's task failed")?;
    let declared = serde_json::to_value(&t.declared)?;
    let placements: Vec<(String, Placement)> = agents.iter().map(|a| (a.name.clone(), a.placement)).collect();
    let gw_commit = crate::reports::gw_commit();
    let who = env("USER").unwrap_or_default();
    let info = RunInfo {
        id: &id,
        name: &suite.name,
        description: &suite.description,
        created_at: chrono::Utc::now(),
        version: env!("CARGO_PKG_VERSION"),
        gw_commit: &gw_commit,
        run_by: (!who.is_empty()).then(|| (who.clone(), String::new())),
        target_label: &t.name,
        target_kind: kind,
        product: &t.product,
        observed: (!observed.is_null()).then_some(&observed),
        declared: &declared,
        declared_mock: definition.get("mock").filter(|m| !m.is_null()),
        agents: &placements,
        mock_shared: kind == TargetKind::Managed && declared["mock_dedicated"] != true,
        suite: serde_json::to_value(&suite)?,
    };
    eprintln!(
        "run {}{}",
        outcome.status.as_str(),
        outcome.error.as_ref().map(|e| format!(": {e}")).unwrap_or_default()
    );
    match encode(&info, &cells) {
        Ok(bytes) => {
            std::fs::write(&a.out, &bytes).with_context(|| format!("writing {}", a.out.display()))?;
            eprintln!(
                "report in {} (pacod-bench-report/1): publish it, or upload it in pacod-gw's console",
                a.out.display()
            );
        }
        Err(e) => eprintln!("no report: {e}"),
    }
    ensure!(
        outcome.leftovers.is_empty(),
        "{} objects the run made could not be deleted: {:?}",
        outcome.leftovers.len(),
        outcome.leftovers
    );
    Ok(())
}

/// `pacod-bench validate`: a report file, read and checked as the gateway reads it.
pub fn validate(a: ValidateArgs) -> anyhow::Result<()> {
    let bytes = std::fs::read(&a.file).with_context(|| format!("reading {}", a.file.display()))?;
    match pacod_gw_bench_report::parse(&bytes) {
        Ok(r) => {
            let class = match r.measurement.class {
                pacod_gw_bench_report::Class::Behaviour => "behaviour",
                pacod_gw_bench_report::Class::Capacity => "capacity",
            };
            println!(
                "{}: a valid report of {} cells ({} bytes), measured as {class}",
                a.file.display(),
                r.cells.len(),
                bytes.len()
            );
            Ok(())
        }
        Err(problems) => {
            for p in &problems {
                println!("{}: {}", if p.path.is_empty() { "(report)" } else { &p.path }, p.message);
            }
            bail!("{} problems", problems.len())
        }
    }
}

/// One line a cell: what it ran and what it measured.
fn line(c: &CellResult, total: usize) -> String {
    let what = if c.kind == CellKind::Baseline {
        "baseline".to_owned()
    } else {
        let axes: Vec<String> = c.axes.iter().map(|(k, v)| format!("{k}={v}")).collect();
        if axes.is_empty() { "base".into() } else { axes.join(" ") }
    };
    let figures = match &c.steady {
        Some(s) => {
            let gw = s.gw_delay.map_or("n/a".to_owned(), |g| format!("{:.1} ms", g.p99));
            format!(
                "{} ok, {} failed, {:.1}/s, total p50 {:.0} ms p99 {:.0} ms, gateway p99 {gw}",
                s.ok, s.failed, s.rps_mean, s.total.p50, s.total.p99
            )
        }
        None => c.error.clone(),
    };
    let flags: Vec<&str> = c.flags.iter().map(|f| f.code.as_str()).collect();
    format!(
        "cell {}/{total} {what}: {} — {figures}{}",
        c.index + 1,
        c.status.as_str(),
        if flags.is_empty() { String::new() } else { format!(" [{}]", flags.join(", ")) }
    )
}
