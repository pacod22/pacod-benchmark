//! One benchmark run, cell by cell.
//!
//! A run goes through these steps:
//! 1. It asks every agent whether it is there, speaks this contract, is free, and allows each
//!    cell's share of the load.
//! 2. On a gateway it manages, it records the gateway's fingerprint, makes sure of the mocks,
//!    and sets up the scratch organisation, its workspaces and grants, then waits until every
//!    instance has applied them.
//! 3. For each cell: it writes the cell's mock profile and makes the cell's key (managed
//!    targets), waits for the instances again, checks that no other calls are in flight at the
//!    gateway, waits for the agents to be free, prepares them all at once (each probes the
//!    target with one call), starts them together, and collects their seconds once a second,
//!    with the gateway's own figures every two. Then it stops the agents, deletes the key, and
//!    reports the cell.
//! 4. It takes the scene down, keys first, whatever happened.
//!
//! Nothing is written to the gateway while a cell's load runs. Agents run on a lease that every
//! report renews: if the benchmark goes away, they stop on their own.
//!
//! The run reports what happens as [`RunEvent`]s on a channel; whoever started it keeps them
//! (a database, a file) and shows them.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::future::join_all;
use pacod_gw_load::contract::{AuthHeader, CONTRACT, Info, Prepare, Profile, RequestSpec, RunState};
use ring::digest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::agents::AgentClient;
use crate::collect::{AgentPart, Collector, Flag, GatewaySample, Histograms, Point, Summary, state_name};
use crate::gw::{GwAdmin, GwError};
use crate::model::{CellConfig, Interface, Placement};
use crate::plan::{CellKind, DRAIN_S, Plan, PlannedCell};
use crate::requests::{self, Names};
use crate::scene::{self, MOCK_SECRET, Mocks, Scene};
use crate::split::{self, Share};

/// How often the agents are asked for their seconds.
pub const POLL_EVERY: Duration = Duration::from_secs(1);
/// How often the gateway's own figures are read.
pub const SAMPLE_EVERY: Duration = Duration::from_secs(2);
/// The agents' lease, in seconds.
pub const LEASE_S: u64 = 30;
/// Connections an agent opens before the start, at most.
pub const WARMUP_MAX: u32 = 200;
pub const CPU_WARN: f64 = 85.0;
pub const LAG_WARN_MS: f64 = 50.0;
/// How long the instances may take to apply a write before the cell goes ahead anyway.
pub const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Fewer steady seconds than this make a cell's figures shaky.
pub const SHORT_STEADY_S: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Preparing,
    Running,
    Cleaning,
    Finished,
    Cancelled,
    Error,
    /// The benchmark stopped during the run; the agents stopped when their lease ran out.
    Interrupted,
}

impl RunStatus {
    pub fn finished(self) -> bool {
        matches!(self, RunStatus::Finished | RunStatus::Cancelled | RunStatus::Error | RunStatus::Interrupted)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Queued => "queued",
            RunStatus::Preparing => "preparing",
            RunStatus::Running => "running",
            RunStatus::Cleaning => "cleaning",
            RunStatus::Finished => "finished",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Error => "error",
            RunStatus::Interrupted => "interrupted",
        }
    }

    pub fn parse(text: &str) -> Option<RunStatus> {
        serde_json::from_value(Value::String(text.to_owned())).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellStatus {
    Pending,
    Preparing,
    Running,
    Draining,
    Done,
    Skipped,
    Error,
    Cancelled,
}

impl CellStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CellStatus::Pending => "pending",
            CellStatus::Preparing => "preparing",
            CellStatus::Running => "running",
            CellStatus::Draining => "draining",
            CellStatus::Done => "done",
            CellStatus::Skipped => "skipped",
            CellStatus::Error => "error",
            CellStatus::Cancelled => "cancelled",
        }
    }
}

/// What else went on at the gateway around a cell.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Noise {
    /// No other calls were in flight at the gateway just before the cell.
    pub quiet_before: Option<bool>,
    /// Calls in flight at the gateway beyond the agents' own, the median of the steady window's
    /// samples.
    pub excess_in_flight: Option<f64>,
    pub config_head_before: Option<i64>,
    pub config_head_after: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellResult {
    pub index: u32,
    pub rep: u32,
    pub kind: CellKind,
    pub status: CellStatus,
    #[serde(default)]
    pub error: String,
    pub axes: BTreeMap<String, Value>,
    pub config: CellConfig,
    pub comparison_key: String,
    pub baseline_key: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    /// The key's id (managed), or a hint of a pasted key: never the key.
    #[serde(default)]
    pub key_hint: String,
    #[serde(default)]
    pub agents: Vec<AgentPart>,
    /// The whole cell, and its steady window.
    pub summary: Option<Summary>,
    pub steady: Option<Summary>,
    /// The steady window's histograms.
    #[serde(default)]
    pub histograms: Option<Histograms>,
    /// The seconds and the gateway's samples; lists leave them out.
    #[serde(default)]
    pub points: Vec<Point>,
    #[serde(default)]
    pub gateway: Vec<GatewaySample>,
    #[serde(default)]
    pub flags: Vec<Flag>,
    #[serde(default)]
    pub noise: Noise,
}

impl CellResult {
    pub fn of(c: &PlannedCell) -> CellResult {
        CellResult {
            index: c.index,
            rep: c.rep,
            kind: c.kind,
            status: CellStatus::Pending,
            error: String::new(),
            axes: c.axes.clone(),
            config: c.config.clone(),
            comparison_key: c.comparison_key.clone(),
            baseline_key: c.baseline_key.clone(),
            started_at: None,
            finished_at: None,
            key_hint: String::new(),
            agents: Vec::new(),
            summary: None,
            steady: None,
            histograms: None,
            points: Vec::new(),
            gateway: Vec::new(),
            flags: Vec::new(),
            noise: Noise::default(),
        }
    }
}

/// What happens during a run, in order.
#[derive(Debug, Clone)]
pub enum RunEvent {
    Status {
        status: RunStatus,
        error: Option<String>,
    },
    /// The target as the gateway describes itself: `{status, cache}`.
    Observed(Value),
    /// What the run has made on the gateway so far: kept so that a benchmark that stops can take
    /// it down later.
    Scene(Scene),
    Cell {
        index: u32,
        rep: u32,
        status: CellStatus,
        agents: Vec<AgentPart>,
    },
    Point {
        index: u32,
        rep: u32,
        point: Box<Point>,
    },
    CellDone(Box<CellResult>),
    Warning(String),
}

/// Where a run's calls go.
pub enum Target {
    /// A pacod-gw whose management API the benchmark uses.
    Managed {
        gw: Arc<GwAdmin>,
        /// The mock as the gateway reaches it.
        mock_url: String,
        /// The data plane as the agents reach it; none: each agent's own target.
        data_plane: Option<String>,
    },
    /// A URL and a key; nothing is set up.
    External {
        base_url: String,
        key: String,
        names: Names,
        /// Calls may reach a paid provider: one attempt each.
        paid: bool,
    },
}

pub struct RunInput {
    pub id: String,
    /// Six characters of the id, for the names of what the run makes.
    pub short: String,
    pub target: Target,
    pub agents: Vec<AgentClient>,
    pub plan: Plan,
    /// The mock as the agents reach it, for the baseline cells; none: they are skipped.
    pub baseline_url: Option<String>,
    /// How long a cell waits for a busy agent before it is skipped.
    pub agent_wait: Duration,
}

pub struct Outcome {
    pub status: RunStatus,
    pub error: Option<String>,
    pub scene: Scene,
    pub leftovers: Vec<String>,
}

enum RunError {
    Cancelled,
    Failed(String),
}

impl From<GwError> for RunError {
    fn from(e: GwError) -> Self {
        RunError::Failed(e.to_string())
    }
}

enum CellError {
    Cancelled,
    Skipped(Flag),
    Failed(String),
}

impl From<GwError> for CellError {
    fn from(e: GwError) -> Self {
        CellError::Failed(e.to_string())
    }
}

/// What a run says about a pasted key: a gateway key's id part (`pgw_<id>_…`), which is not
/// secret, or the start of its SHA-256.
pub fn key_hint(secret: &str) -> String {
    let parts: Vec<&str> = secret.split('_').collect();
    if parts.len() >= 3 && parts[0] == "pgw" && !parts[1].is_empty() {
        return format!("pgw_{}_…", parts[1]);
    }
    let d = digest::digest(&digest::SHA256, secret.as_bytes());
    format!("sha256:{}", &hex::encode(d)[..8])
}

struct Runner {
    input: RunInput,
    events: mpsc::UnboundedSender<RunEvent>,
    cancel: watch::Receiver<bool>,
    scene: Scene,
    mocks: Mocks,
    /// The key of the cell that runs, until it is deleted.
    cell_key: Option<String>,
}

/// Runs `input` to its end, or until `cancel` turns true.
pub async fn run(input: RunInput, events: mpsc::UnboundedSender<RunEvent>, cancel: watch::Receiver<bool>) -> Outcome {
    let mut r = Runner { input, events, cancel, scene: Scene::default(), mocks: Mocks::default(), cell_key: None };
    r.send(RunEvent::Status { status: RunStatus::Preparing, error: None });
    let (mut status, mut error) = match r.execute().await {
        Ok(()) => (RunStatus::Finished, None),
        Err(RunError::Cancelled) => (RunStatus::Cancelled, None),
        Err(RunError::Failed(e)) => (RunStatus::Error, Some(e)),
    };
    let mut leftovers = Vec::new();
    if let Target::Managed { gw, .. } = &r.input.target
        && !r.scene.ledger.is_empty()
    {
        r.send(RunEvent::Status { status: RunStatus::Cleaning, error: None });
        // The teardown ignores the cancel: it is what makes cancelling safe.
        leftovers = scene::teardown(gw, &r.scene.ledger, &r.scene.orgs()).await;
        if !leftovers.is_empty() && status == RunStatus::Finished {
            r.send(RunEvent::Warning(format!(
                "{} objects the run made could not be deleted; they are listed as left over",
                leftovers.len()
            )));
        }
    }
    if status == RunStatus::Finished && r.cancelled() {
        status = RunStatus::Cancelled;
        error = None;
    }
    r.send(RunEvent::Status { status, error: error.clone() });
    Outcome { status, error, scene: r.scene.clone(), leftovers }
}

impl Runner {
    fn send(&self, e: RunEvent) {
        // A run goes on whether or not anyone listens.
        let _ = self.events.send(e);
    }

    fn cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    fn gw(&self) -> Option<Arc<GwAdmin>> {
        match &self.input.target {
            Target::Managed { gw, .. } => Some(gw.clone()),
            Target::External { .. } => None,
        }
    }

    async fn execute(&mut self) -> Result<(), RunError> {
        self.check_agents().await?;
        if self.cancelled() {
            return Err(RunError::Cancelled);
        }
        if self.gw().is_some() {
            let result = self.setup().await;
            self.send(RunEvent::Scene(self.scene.clone()));
            result?;
        }
        if self.input.baseline_url.is_none() && self.input.plan.cells.iter().any(|c| c.kind == CellKind::Baseline) {
            self.send(RunEvent::Warning(
                "no mock the agents can reach is configured (BENCH_BASELINE_MOCK_URL): the baseline cells are skipped"
                    .into(),
            ));
        }
        self.send(RunEvent::Status { status: RunStatus::Running, error: None });
        let cells = self.input.plan.cells.clone();
        for c in &cells {
            if self.cancelled() {
                return Err(RunError::Cancelled);
            }
            let result = self.cell(c).await;
            let cancelled = result.status == CellStatus::Cancelled;
            self.send(RunEvent::CellDone(Box::new(result)));
            if cancelled {
                return Err(RunError::Cancelled);
            }
        }
        Ok(())
    }

    /// Every agent there, of this contract, free, and allowing every cell's share.
    async fn check_agents(&self) -> Result<(), RunError> {
        let agents = &self.input.agents;
        if agents.is_empty() {
            return Err(RunError::Failed("the run has no agents".into()));
        }
        let infos = join_all(agents.iter().map(AgentClient::info)).await;
        let mut checked = Vec::new();
        for (a, info) in agents.iter().zip(infos) {
            let info = info.map_err(|e| RunError::Failed(e.to_string()))?;
            if info.contract != CONTRACT {
                return Err(RunError::Failed(format!(
                    "agent {} speaks contract {}, this benchmark {CONTRACT}",
                    a.name(),
                    info.contract
                )));
            }
            if let Some(b) = &info.busy {
                return Err(RunError::Failed(format!(
                    "agent {} is busy with {} ({})",
                    a.name(),
                    b.run_id,
                    state_name(b.state)
                )));
            }
            checked.push(info);
        }
        for c in &self.input.plan.cells {
            let budget = budget(&c.config, agents.len()).map_err(RunError::Failed)?;
            let shares = split::shares(&c.config.load, agents.len(), budget);
            for ((a, info), share) in agents.iter().zip(&checked).zip(&shares) {
                if let Some(problem) = fits(a.name(), info, share, c.config.load.duration_s()) {
                    return Err(RunError::Failed(format!("cell {}: {problem}", c.index)));
                }
            }
        }
        Ok(())
    }

    /// The fingerprint, the mocks, the scratch organisation.
    async fn setup(&mut self) -> Result<(), RunError> {
        let Target::Managed { gw, mock_url, .. } = &self.input.target else { return Ok(()) };
        let gw = gw.clone();
        let status = gw.status().await.map_err(|e| match e.status() {
            Some(401 | 403) => RunError::Failed("the gateway refuses the benchmark's admin token".into()),
            _ => RunError::Failed(e.to_string()),
        })?;
        let cache = gw.cache_view().await.unwrap_or(Value::Null);
        self.send(RunEvent::Observed(json!({ "status": status, "cache": cache })));

        let targets: Vec<&PlannedCell> = self.input.plan.cells.iter().filter(|c| c.kind == CellKind::Target).collect();
        let mut needs = Vec::new();
        let mut profiles = BTreeMap::new();
        let mut variants = Vec::new();
        for c in &targets {
            let need = (c.config.family(), c.config.request.interface == Interface::Embeddings);
            if !needs.contains(&need) {
                needs.push(need);
            }
            profiles.entry(c.config.family()).or_insert_with(|| c.config.mock.spec());
            if !variants.contains(&c.config.workspace) {
                variants.push(c.config.workspace);
            }
        }
        self.mocks = scene::ensure_mocks(&gw, mock_url, &needs, &profiles).await?;
        let models: Vec<String> = self.mocks.models.iter().map(|m| m.model_id.clone()).collect();
        let name = format!("{}{}", crate::gw::SCRATCH_ORG_PREFIX, self.input.short);
        scene::scratch(&gw, &mut self.scene, &name, &variants, &models).await?;
        if !gw.settle(SETTLE_TIMEOUT).await? {
            self.send(RunEvent::Warning("the gateway's instances had not applied the scene after 30 s".into()));
        }
        Ok(())
    }

    async fn cell(&mut self, c: &PlannedCell) -> CellResult {
        let mut res = CellResult::of(c);
        res.started_at = Some(Utc::now());
        self.send(RunEvent::Cell { index: c.index, rep: c.rep, status: CellStatus::Preparing, agents: vec![] });
        match self.cell_inner(c, &mut res).await {
            Ok(()) => {}
            Err(CellError::Cancelled) => res.status = CellStatus::Cancelled,
            Err(CellError::Skipped(flag)) => {
                res.status = CellStatus::Skipped;
                res.error = flag.message.clone();
                res.flags.push(flag);
            }
            Err(CellError::Failed(e)) => {
                res.status = CellStatus::Error;
                res.error = e;
            }
        }
        // The key goes first, whatever happened, so that it cannot be used once the cell is over.
        if let (Some(gw), Some(key)) = (self.gw(), self.cell_key.take()) {
            if let Err(e) = scene::delete_key(&gw, &mut self.scene, &key).await {
                res.flags.push(Flag::new("key_left", format!("the cell's key could not be deleted: {e}")));
            }
            self.send(RunEvent::Scene(self.scene.clone()));
        }
        res.finished_at = Some(Utc::now());
        res
    }

    /// What the cell's calls are: the base URL (none: each agent's own target), the key, the
    /// request, and where the key goes.
    async fn calls(
        &mut self,
        c: &PlannedCell,
        res: &mut CellResult,
    ) -> Result<(Option<String>, String, RequestSpec, AuthHeader), CellError> {
        let cfg = &c.config;
        let embeddings = cfg.request.interface == Interface::Embeddings;
        if c.kind == CellKind::Baseline {
            let Some(url) = &self.input.baseline_url else {
                return Err(CellError::Skipped(Flag::new(
                    "no_baseline_mock",
                    "no mock the agents can reach is configured",
                )));
            };
            let (spec, auth) = requests::baseline(cfg, scene::upstream_of(cfg.family(), embeddings));
            let base = format!("{}/x-mock/{}", url.trim_end_matches('/'), cfg.mock.spec());
            return Ok((Some(base), MOCK_SECRET.to_owned(), spec, auth));
        }
        match &self.input.target {
            Target::External { base_url, key, names, paid } => {
                res.key_hint = key_hint(key);
                Ok((
                    Some(base_url.clone()),
                    key.clone(),
                    requests::build(cfg, names, *paid),
                    requests::auth_header(cfg.request.interface),
                ))
            }
            Target::Managed { gw, mock_url, data_plane } => {
                let (gw, mock_url, data_plane) = (gw.clone(), mock_url.clone(), data_plane.clone());
                scene::set_profile(&gw, &mut self.mocks, &mock_url, cfg.family(), &cfg.mock.spec()).await?;
                let model = self
                    .mocks
                    .model(cfg.family(), embeddings)
                    .cloned()
                    .ok_or_else(|| CellError::Failed(format!("no {} mock model was set up", cfg.family())))?;
                let ws = self
                    .scene
                    .workspace(&cfg.workspace)
                    .ok_or_else(|| CellError::Failed("the cell's workspace was not set up".into()))?
                    .to_owned();
                let drain = DRAIN_S.min(cfg.load.timeout_s);
                let ends = Utc::now() + chrono::Duration::seconds((cfg.load.duration_s() + drain + 120) as i64);
                let name = format!("bench-{}-c{}-r{}", self.input.short, c.index, c.rep);
                let made = scene::cell_key(&gw, &mut self.scene, &ws, cfg, &model.model_id, &name, ends).await?;
                self.cell_key = Some(made.id.clone());
                res.key_hint = made.id.clone();
                self.send(RunEvent::Scene(self.scene.clone()));
                if !gw.settle(SETTLE_TIMEOUT).await? {
                    res.flags.push(Flag::new(
                        "settle_timeout",
                        "the gateway's instances had not applied the cell's key after 30 s",
                    ));
                }
                let names = Names { model: model.model, provider: model.provider, upstream: model.upstream };
                Ok((
                    data_plane,
                    made.secret,
                    requests::build(cfg, &names, false),
                    requests::auth_header(cfg.request.interface),
                ))
            }
        }
    }

    /// About three seconds of the gateway's calls in flight before the load: none expected.
    async fn quiet_check(&self, gw: &GwAdmin, res: &mut CellResult) {
        let mut busy = 0;
        let mut seen = 0;
        for i in 0..3 {
            if i > 0 {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            if let Ok(live) = gw.live().await {
                seen += 1;
                if live["in_flight"].as_i64().unwrap_or(0) > 0 {
                    busy += 1;
                }
            }
        }
        if seen > 0 {
            res.noise.quiet_before = Some(busy < 2);
            if busy >= 2 {
                res.flags.push(Flag::new("other_traffic", "other calls were in flight at the gateway before the cell"));
            }
        }
        res.noise.config_head_before = gw.status().await.ok().and_then(|s| s["config_head"].as_i64());
    }

    /// The agents' answers to `/v1/info` once none is busy; skipped after `agent_wait`.
    async fn wait_free(&mut self) -> Result<Vec<Info>, CellError> {
        let deadline = Instant::now() + self.input.agent_wait;
        loop {
            let infos = join_all(self.input.agents.iter().map(AgentClient::info)).await;
            let mut out = Vec::new();
            let mut busy = Vec::new();
            for (a, info) in self.input.agents.iter().zip(infos) {
                let info = info.map_err(|e| CellError::Failed(e.to_string()))?;
                if let Some(b) = &info.busy {
                    busy.push(format!("agent {} is busy with {} ({})", a.name(), b.run_id, state_name(b.state)));
                }
                out.push(info);
            }
            if busy.is_empty() {
                return Ok(out);
            }
            if Instant::now() >= deadline {
                return Err(CellError::Skipped(Flag::new("agent_busy_skipped", busy.join("; "))));
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                _ = self.cancel.changed() => {}
            }
            if self.cancelled() {
                return Err(CellError::Cancelled);
            }
        }
    }

    async fn cell_inner(&mut self, c: &PlannedCell, res: &mut CellResult) -> Result<(), CellError> {
        let cfg = c.config.clone();
        let (base_url, key, spec, auth) = self.calls(c, res).await?;
        let infos = self.wait_free().await?;
        // Once the agents are free: another controller's load before that is no noise of this
        // cell's.
        if let Some(gw) = self.gw() {
            self.quiet_check(&gw, res).await;
        }
        if self.cancelled() {
            return Err(CellError::Cancelled);
        }
        let n = self.input.agents.len();
        let shares = split::shares(&cfg.load, n, budget(&cfg, n).map_err(CellError::Failed)?);
        let run_id = format!("b{}-c{}-r{}", self.input.short, c.index, c.rep);
        let parts: Vec<AgentPart> = self
            .input
            .agents
            .iter()
            .zip(&infos)
            .zip(&shares)
            .map(|((a, info), share)| AgentPart {
                name: a.name().to_owned(),
                placement: a.agent.placement,
                version: info.version.clone(),
                target: info.target.clone(),
                cpus: info.cpus,
                share: Some(share.clone()),
                state: "pending".into(),
                ..AgentPart::default()
            })
            .collect();
        res.agents = parts.clone();

        // Prepared all at once: one slow prepare must not let another's lease run out.
        let plans: Vec<Prepare> = shares
            .iter()
            .enumerate()
            .map(|(i, share)| {
                let warm = share.max_in_flight.unwrap_or(share.peak().ceil() as u32);
                Prepare {
                    contract: CONTRACT,
                    run_id: run_id.clone(),
                    part: i as u32,
                    parts: n as u32,
                    key: key.clone(),
                    auth_header: auth,
                    base_url: base_url.clone(),
                    request: spec.clone(),
                    profile: Profile {
                        mode: cfg.load.mode,
                        stages: share.stages.clone(),
                        max_in_flight: share.max_in_flight,
                        timeout_s: cfg.load.timeout_s,
                        max_calls: share.max_calls,
                        drain_s: DRAIN_S,
                        http: cfg.request.http,
                        warmup_connections: warm.min(WARMUP_MAX),
                    },
                    lease_s: LEASE_S,
                }
            })
            .collect();
        let timeout = Duration::from_secs(cfg.load.timeout_s + 30);
        let agents = self.input.agents.clone();
        let prepared = join_all(agents.iter().zip(&plans).map(|(a, p)| a.prepare(p, timeout))).await;
        let mut problems = Vec::new();
        for (i, result) in prepared.into_iter().enumerate() {
            let part = &mut res.agents[i];
            match result {
                Err(e) => {
                    part.state = "failed".into();
                    part.error = e.to_string();
                    problems.push(e.to_string());
                }
                Ok(p) => {
                    part.state = state_name(p.state).into();
                    let ok = p.state == RunState::Prepared
                        && p.probe.error.is_none()
                        && p.probe.status.is_some_and(|s| (200..300).contains(&s));
                    if !ok {
                        let said = p.probe.error.clone().unwrap_or_else(|| format!("answered {:?}", p.probe.status));
                        let excerpt: String = p.probe.body_excerpt.chars().take(200).collect();
                        let excerpt = if excerpt.is_empty() { String::new() } else { format!(": {excerpt}") };
                        problems.push(format!("agent {}: the call before the load {said}{excerpt}", part.name));
                    }
                    part.probe = Some(p.probe);
                }
            }
        }
        if !problems.is_empty() {
            stop(&agents, &run_id, &[]).await;
            return Err(CellError::Failed(problems.join("; ")));
        }
        let started = join_all(agents.iter().map(|a| a.start(&run_id))).await;
        if let Some(e) = started.into_iter().find_map(Result::err) {
            stop(&agents, &run_id, &[]).await;
            return Err(CellError::Failed(e.to_string()));
        }
        for p in &mut res.agents {
            p.state = "running".into();
        }
        res.status = CellStatus::Running;
        self.send(RunEvent::Cell {
            index: c.index,
            rep: c.rep,
            status: CellStatus::Running,
            agents: res.agents.clone(),
        });

        let window = (cfg.load.warmup_s, cfg.load.duration_s());
        let mut col = Collector::new(res.agents.clone(), window);
        let cancelled = self.follow(c, &run_id, &mut col, res).await;
        let finished: Vec<bool> = (0..n).map(|i| col.finished(i)).collect();
        stop(&agents, &run_id, &finished).await;
        col.emit(true, Instant::now());
        res.agents = col.parts.clone();
        res.summary = Some(col.summary());
        res.steady = Some(col.steady_summary());
        res.histograms = Some(col.steady.hist.clone());
        res.points = col.points.clone();
        res.flags.extend(col.flags.clone());
        self.noise(c, res).await;
        flags(res, &col);
        if col.all_lost() {
            return Err(CellError::Failed("no agent answered any more; their leases stop the load".into()));
        }
        res.status = if cancelled { CellStatus::Cancelled } else { CellStatus::Done };
        if cancelled {
            return Err(CellError::Cancelled);
        }
        Ok(())
    }

    /// Collects the agents' seconds until all have ended; true when the run was cancelled.
    async fn follow(&mut self, c: &PlannedCell, run_id: &str, col: &mut Collector, res: &mut CellResult) -> bool {
        let agents = self.input.agents.clone();
        let gw = self.gw();
        let load = &c.config.load;
        let give_up = Instant::now() + Duration::from_secs(load.duration_s() + DRAIN_S + 60);
        let t0 = Instant::now();
        let mut next_sample = t0;
        let mut cancel_sent: Option<Instant> = None;
        let mut draining = false;
        loop {
            if self.cancelled() && cancel_sent.is_none() {
                cancel_sent = Some(Instant::now() + Duration::from_secs(15));
                let finished: Vec<bool> = (0..agents.len()).map(|i| col.finished(i)).collect();
                stop(&agents, run_id, &finished).await;
            }
            let active = col.active();
            let reports = join_all(active.iter().map(|&i| agents[i].report(run_id, col.last_t(i)))).await;
            let now = Instant::now();
            for (&i, rep) in active.iter().zip(reports) {
                match rep {
                    Ok(r) => col.take(i, r, now),
                    Err(e) => {
                        col.failed(i, &e.to_string());
                    }
                }
            }
            if !draining && col.any_draining() {
                draining = true;
                res.status = CellStatus::Draining;
                self.send(RunEvent::Cell {
                    index: c.index,
                    rep: c.rep,
                    status: CellStatus::Draining,
                    agents: col.parts.clone(),
                });
            }
            if let Some(gw) = &gw
                && Instant::now() >= next_sample
            {
                next_sample += SAMPLE_EVERY;
                if let Ok(live) = gw.live().await {
                    let agents_in_flight = if c.kind == CellKind::Baseline { 0 } else { col.agents_in_flight() };
                    let sample = GatewaySample {
                        t: (t0.elapsed().as_millis() as f64 / 100.0).round() / 10.0,
                        in_flight: live["in_flight"].as_i64().unwrap_or(0),
                        agents_in_flight,
                        rejected_per_second: live["rejected_per_second"].as_f64().unwrap_or(0.0),
                        spool_bytes: live["spool_bytes"].as_i64().unwrap_or(0),
                        events_dropped: live["events_dropped"].as_i64().unwrap_or(0),
                    };
                    col.gw_in_flight = Some(sample.in_flight);
                    res.gateway.push(sample);
                }
            }
            let done = col.done();
            for point in col.emit(done, Instant::now()) {
                self.send(RunEvent::Point { index: c.index, rep: c.rep, point: Box::new(point) });
            }
            if done {
                break;
            }
            let now = Instant::now();
            if cancel_sent.is_some_and(|deadline| now > deadline) {
                self.send(RunEvent::Warning("the agents did not confirm the cancel in time".into()));
                break;
            }
            if now > give_up {
                res.flags.push(Flag::new("agents_late", "the agents did not end the cell in time; it was cancelled"));
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(POLL_EVERY) => {}
                _ = self.cancel.changed(), if cancel_sent.is_none() => {}
            }
        }
        cancel_sent.is_some()
    }

    /// The gateway's other traffic and configuration changes during the cell.
    async fn noise(&self, c: &PlannedCell, res: &mut CellResult) {
        let Some(gw) = self.gw() else { return };
        res.noise.config_head_after = gw.status().await.ok().and_then(|s| s["config_head"].as_i64());
        if let (Some(before), Some(after)) = (res.noise.config_head_before, res.noise.config_head_after)
            && after != before
        {
            res.flags.push(Flag::new(
                "config_changed_during_cell",
                format!("the gateway's configuration changed during the cell (version {before} to {after})"),
            ));
        }
        let (from, to) = (c.config.load.warmup_s as f64, c.config.load.duration_s() as f64);
        let steady: Vec<&GatewaySample> = res.gateway.iter().filter(|s| s.t >= from && s.t < to).collect();
        if steady.is_empty() {
            return;
        }
        let mut excess: Vec<f64> = steady.iter().map(|s| s.in_flight as f64 - s.agents_in_flight as f64).collect();
        excess.sort_by(f64::total_cmp);
        let median = excess[excess.len() / 2];
        res.noise.excess_in_flight = Some(median);
        // The agents' own calls in flight, against which a few more at the gateway are timing.
        let agents = steady.iter().map(|s| s.agents_in_flight as f64).sum::<f64>() / steady.len() as f64;
        let tolerance = (0.05 * agents).max(2.0);
        if median > tolerance {
            res.flags.push(Flag::new(
                "other_traffic",
                format!("about {median:.0} calls beyond the cell's own were in flight at the gateway"),
            ));
        }
    }
}

/// What one call through a target says, before any run: a wrong key, a model the gateway
/// cannot reach, a mock outside its egress allow-list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub ok: bool,
    pub status: Option<u16>,
    pub ttfb_ms: f64,
    pub total_ms: f64,
    pub error: String,
    /// The start of an answer that was not a success.
    pub excerpt: String,
    /// The gateway's instances applied the probe's scene in time.
    pub settled: Option<bool>,
    /// What the probe could not delete on the gateway.
    pub leftovers: Vec<String>,
}

/// One call through `target` by `agent`, as a cell's probe makes it: on a managed gateway with a
/// scene of its own (`bench-chk<short>`: the OpenAI mock, a workspace, a key), taken down after.
/// The agent prepares a one-call run, which makes the probe, and cancels it.
pub async fn probe(target: &Target, agent: &AgentClient, short: &str) -> Result<ProbeResult, String> {
    let mut cfg: CellConfig = serde_json::from_value(json!({
        "load": { "mode": "concurrency", "stages": [{ "duration_s": 1, "target": 1 }], "timeout_s": 30, "max_calls": 2 }
    }))
    .expect("a probe's configuration");
    let kind = match target {
        Target::Managed { .. } => crate::model::TargetKind::Managed,
        Target::External { .. } => crate::model::TargetKind::External,
    };
    cfg.check(kind)?;
    let mut scene = Scene::default();
    let mut settled = None;
    let result = async {
        let (base_url, key, spec) = match target {
            Target::External { base_url, key, names, paid } => {
                (Some(base_url.clone()), key.clone(), requests::build(&cfg, names, *paid))
            }
            Target::Managed { gw, mock_url, data_plane } => {
                let profiles = BTreeMap::from([(cfg.mock.family, cfg.mock.spec())]);
                let mocks = scene::ensure_mocks(gw, mock_url, &[(cfg.mock.family, false)], &profiles)
                    .await
                    .map_err(|e| e.to_string())?;
                let model = mocks.model(cfg.mock.family, false).cloned().ok_or("the mock model was not set up")?;
                let name = format!("{}chk{short}", crate::gw::SCRATCH_ORG_PREFIX);
                scene::scratch(gw, &mut scene, &name, &[cfg.workspace], std::slice::from_ref(&model.model_id))
                    .await
                    .map_err(|e| e.to_string())?;
                let ws = scene.workspaces[0].1.clone();
                let ends = Utc::now() + chrono::Duration::seconds(120);
                let key = scene::cell_key(gw, &mut scene, &ws, &cfg, &model.model_id, &name, ends)
                    .await
                    .map_err(|e| e.to_string())?;
                settled = Some(gw.settle(SETTLE_TIMEOUT).await.map_err(|e| e.to_string())?);
                let names = Names { model: model.model, provider: model.provider, upstream: model.upstream };
                (data_plane.clone(), key.secret, requests::build(&cfg, &names, false))
            }
        };
        let info = agent.info().await.map_err(|e| e.to_string())?;
        if let Some(b) = info.busy {
            return Err(format!("agent {} is busy with {} ({})", agent.name(), b.run_id, state_name(b.state)));
        }
        let run_id = format!("b{short}-probe");
        let plan = Prepare {
            contract: CONTRACT,
            run_id: run_id.clone(),
            part: 0,
            parts: 1,
            key,
            auth_header: requests::auth_header(cfg.request.interface),
            base_url,
            request: spec,
            profile: Profile {
                mode: cfg.load.mode,
                stages: cfg.load.stages.clone(),
                max_in_flight: None,
                timeout_s: cfg.load.timeout_s,
                max_calls: Some(1),
                drain_s: 1,
                http: cfg.request.http,
                warmup_connections: 0,
            },
            lease_s: LEASE_S,
        };
        let prepared = agent.prepare(&plan, Duration::from_secs(60)).await;
        let _ = agent.cancel(&run_id).await;
        let p = prepared.map_err(|e| e.to_string())?.probe;
        Ok(ProbeResult {
            ok: p.error.is_none() && p.status.is_some_and(|s| (200..300).contains(&s)),
            status: p.status,
            ttfb_ms: p.ttfb_ms,
            total_ms: p.total_ms,
            error: p.error.unwrap_or_default(),
            excerpt: p.body_excerpt.chars().take(300).collect(),
            settled,
            leftovers: vec![],
        })
    }
    .await;
    let leftovers = match target {
        Target::Managed { gw, .. } if !scene.ledger.is_empty() => {
            scene::teardown(gw, &scene.ledger, &scene.orgs()).await
        }
        _ => vec![],
    };
    result.map(|r| ProbeResult { leftovers, ..r })
}

/// The calls a cell's agents may make after their probes; an error when too few are left.
fn budget(cfg: &CellConfig, agents: usize) -> Result<Option<u64>, String> {
    let Some(max) = cfg.load.max_calls else { return Ok(None) };
    let left = max.saturating_sub(agents as u64);
    if left < agents as u64 {
        return Err(format!(
            "{max} calls leave too few for {agents} agents after the {agents} calls that check the target first"
        ));
    }
    Ok(Some(left))
}

/// Whether an agent's caps allow its share.
fn fits(name: &str, info: &Info, share: &Share, duration: u64) -> Option<String> {
    let caps = &info.caps;
    let peak = share.peak();
    if duration > caps.max_duration_s {
        return Some(format!("agent {name} runs at most {} s", caps.max_duration_s));
    }
    match share.max_in_flight {
        None if peak > f64::from(caps.max_concurrency) => {
            Some(format!("agent {name} makes at most {} calls at a time, its share is {peak}", caps.max_concurrency))
        }
        Some(_) if peak > caps.max_rate => {
            Some(format!("agent {name} makes at most {} calls a second, its share is {peak}", caps.max_rate))
        }
        Some(m) if m > caps.max_concurrency => {
            Some(format!("agent {name} holds at most {} calls at a time", caps.max_concurrency))
        }
        _ => None,
    }
}

/// Cancels every agent that may still be going.
async fn stop(agents: &[AgentClient], run_id: &str, finished: &[bool]) {
    let going = agents.iter().enumerate().filter(|(i, _)| !finished.get(*i).copied().unwrap_or(false));
    join_all(going.map(|(_, a)| a.cancel(run_id))).await;
}

/// What a reader of the cell's figures should know, from the agents and the calls.
fn flags(res: &mut CellResult, col: &Collector) {
    for p in &res.agents {
        if p.cpu_max >= CPU_WARN {
            res.flags.push(Flag::new(
                "agent_cpu_high",
                format!(
                    "agent {} used up to {:.0}% of its {} CPUs: its figures may describe the agent rather than the gateway",
                    p.name, p.cpu_max, p.cpus
                ),
            ));
        }
    }
    if col.whole.lag_max > LAG_WARN_MS {
        res.flags.push(Flag::new(
            "agent_lag",
            format!(
                "the agents started calls up to {:.0} ms late: they are short of CPU, or the rate is more than they can drive",
                col.whole.lag_max
            ),
        ));
    }
    if col.whole.skipped > 0 {
        res.flags.push(Flag::new(
            "skipped_calls",
            format!("{} calls were not started because max_in_flight calls were running", col.whole.skipped),
        ));
    }
    for (code, why) in [
        ("503", "the gateway was overloaded, or a provider unavailable"),
        ("429", "a rate limit or a concurrency cap"),
        ("402", "a quota"),
    ] {
        if let Some(n) = col.whole.status.get(code).filter(|n| **n > 0) {
            res.flags
                .push(Flag::new(&format!("refused_{code}"), format!("{n} calls were refused with {code} ({why})")));
        }
    }
    if col.whole.errors.timeout > 0 {
        res.flags
            .push(Flag::new("timeouts", format!("{} calls took longer than the timeout", col.whole.errors.timeout)));
    }
    let dropped = res.gateway.iter().map(|s| s.events_dropped).max().unwrap_or(0);
    let first = res.gateway.first().map_or(0, |s| s.events_dropped);
    if dropped > first {
        res.flags.push(Flag::new(
            "events_dropped",
            "the gateway dropped usage events during the cell: analytics did not keep up",
        ));
    }
    if res.agents.iter().any(|p| p.placement == Placement::InStack) {
        res.flags.push(Flag::new(
            "in_stack_agent",
            "an agent ran in the gateway's stack: its figures show behaviour, not capacity",
        ));
    }
    if res.steady.as_ref().is_some_and(|s| s.duration_s < SHORT_STEADY_S) {
        res.flags.push(Flag::new("short_steady_window", "fewer than 5 steady seconds were measured"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_hint_never_shows_the_secret() {
        assert_eq!(key_hint("pgw_dev_abc123secret"), "pgw_dev_…");
        let h = key_hint("sk-something-secret");
        assert!(h.starts_with("sha256:") && h.len() == 15);
    }

    #[test]
    fn the_budget_leaves_a_call_for_each_agent_after_the_probes() {
        let suite = crate::plan::Suite::parse(
            "name: x\nbase:\n  load: { stages: [{ duration_s: 1, target: 1 }], max_calls: 3 }\n",
        )
        .unwrap();
        assert_eq!(budget(&suite.base, 1).unwrap(), Some(2));
        assert!(budget(&suite.base, 2).is_err());
    }
}
