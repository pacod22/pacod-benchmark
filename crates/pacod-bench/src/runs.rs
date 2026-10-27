//! The run queue: runs start in the order they were asked for, one at a time.
//!
//! One at a time because the benchmark's mocks on a gateway are shared by every run there, and
//! two runs would measure each other. A run that waits is `queued`; the worker starts the
//! oldest one as soon as none is active. While a run goes, its events are written to the
//! database (the cells' seconds every few seconds) and sent to whoever follows it.
//!
//! A pasted key of an external target is held in memory only, from the start request until the
//! run begins: it is never written anywhere.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pacod_bench_engine::agents::AgentClient;
use pacod_bench_engine::controller::{self, CellResult, RunEvent, RunInput, RunStatus, Target};
use pacod_bench_engine::gw::GwAdmin;
use pacod_bench_engine::model::TargetKind;
use pacod_bench_engine::plan::{CellKind, PlannedCell};
use pacod_bench_engine::requests::Names;
use pacod_bench_engine::scene::{self, Scene};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{Notify, broadcast, mpsc, watch};

use crate::config::Config;
use crate::store::{RunRow, Store};

/// How often a running cell's seconds are written to the database.
const SAVE_EVERY: Duration = Duration::from_secs(5);

struct Active {
    id: String,
    cancel: watch::Sender<bool>,
    events: broadcast::Sender<Value>,
}

/// Counters for `/metrics`.
#[derive(Default)]
pub struct Counters {
    pub runs_started: AtomicU64,
    pub runs_finished: AtomicU64,
    pub runs_failed: AtomicU64,
    pub cells_done: AtomicU64,
    pub cells_failed: AtomicU64,
}

pub struct Runs {
    store: Store,
    config: Arc<Config>,
    http: reqwest::Client,
    gateways: Arc<BTreeMap<String, Arc<GwAdmin>>>,
    keys: Mutex<HashMap<String, String>>,
    active: Mutex<Option<Active>>,
    wake: Notify,
    pub counters: Counters,
}

impl Runs {
    pub fn new(
        store: Store,
        config: Arc<Config>,
        http: reqwest::Client,
        gateways: Arc<BTreeMap<String, Arc<GwAdmin>>>,
    ) -> Runs {
        Runs {
            store,
            config,
            http,
            gateways,
            keys: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            wake: Notify::new(),
            counters: Counters::default(),
        }
    }

    /// Holds a pasted key for a queued run, and wakes the worker.
    pub fn queued(&self, run_id: &str, key: Option<String>) {
        if let Some(k) = key {
            self.keys.lock().insert(run_id.to_owned(), k);
        }
        self.wake.notify_one();
    }

    pub fn active_id(&self) -> Option<String> {
        self.active.lock().as_ref().map(|a| a.id.clone())
    }

    /// Cancels the active run, if it is this one; false otherwise.
    pub fn cancel_active(&self, run_id: &str) -> bool {
        match self.active.lock().as_ref() {
            Some(a) if a.id == run_id => {
                let _ = a.cancel.send(true);
                true
            }
            _ => false,
        }
    }

    pub fn forget_key(&self, run_id: &str) {
        self.keys.lock().remove(run_id);
    }

    /// The live events of the active run, if it is this one.
    pub fn subscribe(&self, run_id: &str) -> Option<broadcast::Receiver<Value>> {
        self.active.lock().as_ref().filter(|a| a.id == run_id).map(|a| a.events.subscribe())
    }

    /// Takes down what interrupted runs left on their gateways, then starts the worker.
    pub fn spawn(self: Arc<Self>, interrupted: Vec<RunRow>, mut stop: watch::Receiver<bool>) {
        tokio::spawn(async move {
            for run in interrupted {
                self.clean(&run).await;
            }
            loop {
                match self.store.next_queued().await {
                    Ok(Some(run)) => {
                        self.execute(run).await;
                        continue;
                    }
                    Ok(None) => {}
                    Err(e) => tracing::warn!(error = %e, "reading the run queue failed"),
                }
                tokio::select! {
                    _ = self.wake.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                    _ = stop.wait_for(|s| *s) => return,
                }
            }
        });
    }

    /// Deletes what a run left on its gateway; returns what is still left.
    pub async fn clean(&self, run: &RunRow) -> Vec<String> {
        let scene: Scene = serde_json::from_value(run.scene.clone()).unwrap_or_default();
        let mut left: Vec<String> = serde_json::from_value(run.leftovers.clone()).unwrap_or_default();
        for p in &scene.ledger {
            if !left.contains(p) {
                left.push(p.clone());
            }
        }
        let gw = run.target_id.as_ref().and_then(|t| self.gateways.get(t));
        let (Some(gw), false) = (gw, left.is_empty()) else { return left };
        let still = scene::teardown(gw, &left, &scene.orgs()).await;
        if let Err(e) = self.store.set_leftovers(&run.id, &still).await {
            tracing::warn!(run_id = %run.id, error = %e, "recording a run's leftovers failed");
        }
        let _ = self.store.set_scene(&run.id, &json!(Scene { ledger: vec![], ..scene })).await;
        if !still.is_empty() {
            tracing::warn!(run_id = %run.id, left = still.len(), "objects of an interrupted run could not be deleted");
        }
        still
    }

    fn input(&self, run: &RunRow, cells: Vec<PlannedCell>) -> Result<RunInput, String> {
        let names: Vec<String> = serde_json::from_value(run.agents["names"].clone()).unwrap_or_default();
        let token = self.config.agent_token.clone().ok_or("no agent token is configured (BENCH_AGENT_TOKEN)")?;
        let mut agents = Vec::new();
        for name in &names {
            let a = self
                .config
                .agents
                .iter()
                .find(|a| &a.name == name)
                .ok_or_else(|| format!("the agent {name} is no longer configured"))?;
            agents.push(AgentClient::new(a.clone(), &token, self.http.clone()));
        }
        let def = &run.target["definition"];
        let target = match run.target["kind"].as_str() {
            Some("managed") => {
                let id = run.target_id.clone().unwrap_or_default();
                let gw = self.gateways.get(&id).cloned().ok_or("the gateway is no longer configured")?;
                let g = self
                    .config
                    .gateways
                    .iter()
                    .find(|g| format!("cfg-{}", g.name) == id)
                    .ok_or("the gateway is no longer configured")?;
                Target::Managed { gw, mock_url: g.mock_url.clone(), data_plane: g.data_plane.clone() }
            }
            _ => {
                let key = self
                    .keys
                    .lock()
                    .remove(&run.id)
                    .ok_or("the pasted key was held in memory only, and the benchmark restarted since")?;
                Target::External {
                    base_url: def["base_url"].as_str().unwrap_or("").to_owned(),
                    key,
                    names: Names {
                        model: def["model"].as_str().unwrap_or("").to_owned(),
                        provider: def["provider"].as_str().unwrap_or("").to_owned(),
                        upstream: def["upstream"].as_str().unwrap_or("").to_owned(),
                    },
                    paid: def["paid"].as_bool().unwrap_or(true),
                }
            }
        };
        Ok(RunInput {
            id: run.id.clone(),
            short: short(&run.id),
            target,
            agents,
            plan: pacod_bench_engine::plan::Plan { cells, pruned: vec![], estimate: Default::default(), notes: vec![] },
            baseline_url: self.config.baseline_mock_url.clone(),
            agent_wait: Duration::from_secs(self.config.agent_wait_s),
        })
    }

    async fn planned(&self, run_id: &str) -> Result<Vec<PlannedCell>, String> {
        let rows = self.store.cells(run_id).await.map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|r| {
                Ok(PlannedCell {
                    index: r.idx as u32,
                    rep: r.rep as u32,
                    kind: if r.kind == "baseline" { CellKind::Baseline } else { CellKind::Target },
                    axes: serde_json::from_value(r.axes).map_err(|e| e.to_string())?,
                    config: serde_json::from_value(r.config).map_err(|e| e.to_string())?,
                    comparison_key: r.comparison_key,
                    baseline_key: r.baseline_key,
                })
            })
            .collect()
    }

    async fn execute(&self, run: RunRow) {
        let (cancel, cancel_rx) = watch::channel(false);
        let (events, _) = broadcast::channel(1024);
        *self.active.lock() = Some(Active { id: run.id.clone(), cancel, events: events.clone() });
        self.counters.runs_started.fetch_add(1, Ordering::Relaxed);
        tracing::info!(run_id = %run.id, target = %run.target["name"], "run starting");

        let prepared = match self.planned(&run.id).await {
            Ok(cells) => self.input(&run, cells),
            Err(e) => Err(e),
        };
        let input = match prepared {
            Ok(i) => i,
            Err(e) => {
                let _ = self.store.set_status(&run.id, "error", Some(&e)).await;
                let _ = events.send(json!({ "event": "status", "status": "error", "error": e }));
                self.counters.runs_failed.fetch_add(1, Ordering::Relaxed);
                *self.active.lock() = None;
                return;
            }
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(controller::run(input, tx, cancel_rx));
        let mut recorder = Recorder {
            store: &self.store,
            run: &run,
            events: &events,
            points: HashMap::new(),
            last_save: tokio::time::Instant::now(),
            counters: &self.counters,
        };
        while let Some(e) = rx.recv().await {
            recorder.record(e).await;
        }
        let outcome = match task.await {
            Ok(o) => o,
            Err(e) => {
                tracing::error!(run_id = %run.id, error = %e, "the run's task failed");
                let _ = self.store.set_status(&run.id, "error", Some("the run's task failed")).await;
                *self.active.lock() = None;
                return;
            }
        };
        let _ = self.store.set_leftovers(&run.id, &outcome.leftovers).await;
        let _ = self.store.set_scene(&run.id, &json!(outcome.scene)).await;
        let counter =
            if outcome.status == RunStatus::Error { &self.counters.runs_failed } else { &self.counters.runs_finished };
        counter.fetch_add(1, Ordering::Relaxed);
        tracing::info!(run_id = %run.id, status = outcome.status.as_str(), error = ?outcome.error, "run ended");
        let _ = events.send(json!({ "event": "done", "status": outcome.status, "leftovers": outcome.leftovers.len() }));
        *self.active.lock() = None;
    }
}

/// Six characters of a run id, for the names of what it makes on a gateway.
pub fn short(id: &str) -> String {
    let tail: String = id.chars().rev().filter(char::is_ascii_alphanumeric).take(6).collect();
    tail.chars().rev().collect::<String>().to_ascii_lowercase()
}

/// A cell row without its seconds, for lists.
pub fn cell_view_row(c: &crate::store::CellRow) -> Value {
    let mut v = json!(c);
    if let Some(o) = v.as_object_mut() {
        o.remove("points");
    }
    v
}

/// A cell's result without its seconds, for lists and events.
pub fn cell_view(c: &CellResult) -> Value {
    let mut v = json!(c);
    if let Some(o) = v.as_object_mut() {
        o.remove("points");
        o.remove("gateway");
    }
    v
}

struct Recorder<'a> {
    store: &'a Store,
    run: &'a RunRow,
    events: &'a broadcast::Sender<Value>,
    points: HashMap<u32, Vec<Value>>,
    last_save: tokio::time::Instant,
    counters: &'a Counters,
}

impl Recorder<'_> {
    async fn record(&mut self, e: RunEvent) {
        let id = &self.run.id;
        let out = match e {
            RunEvent::Status { status, error } => {
                if let Err(err) = self.store.set_status(id, status.as_str(), error.as_deref()).await {
                    tracing::warn!(run_id = %id, error = %err, "recording a run's status failed");
                }
                json!({ "event": "status", "status": status, "error": error })
            }
            RunEvent::Observed(v) => {
                let _ = self.store.set_run_observed(id, &v).await;
                if let Some(t) = &self.run.target_id {
                    let _ = self.store.set_observed(t, &v).await;
                }
                json!({ "event": "observed", "observed": v })
            }
            RunEvent::Scene(scene) => {
                let _ = self.store.set_scene(id, &json!(scene)).await;
                return;
            }
            RunEvent::Cell { index, rep, status, agents } => {
                let _ = self.store.set_cell_status(id, index as i32, status.as_str()).await;
                json!({ "event": "cell", "index": index, "rep": rep, "status": status, "agents": agents })
            }
            RunEvent::Point { index, point, .. } => {
                let p = json!(point);
                self.points.entry(index).or_default().push(p.clone());
                if self.last_save.elapsed() >= SAVE_EVERY {
                    self.last_save = tokio::time::Instant::now();
                    if let Some(all) = self.points.get(&index) {
                        let _ = self.store.set_cell_points(id, index as i32, &json!(all)).await;
                    }
                }
                json!({ "event": "point", "index": index, "point": p })
            }
            RunEvent::CellDone(c) => {
                let counter = match c.status {
                    controller::CellStatus::Error => &self.counters.cells_failed,
                    _ => &self.counters.cells_done,
                };
                counter.fetch_add(1, Ordering::Relaxed);
                self.points.remove(&c.index);
                if let Err(err) = self.store.finish_cell(id, &c).await {
                    tracing::warn!(run_id = %id, error = %err, "recording a cell failed");
                }
                json!({ "event": "cell_done", "cell": cell_view(&c) })
            }
            RunEvent::Warning(w) => {
                let _ = self.store.add_warning(id, &w).await;
                json!({ "event": "warning", "warning": w })
            }
        };
        let _ = self.events.send(out);
    }
}

/// The target as a run keeps it: no secret.
pub fn target_snapshot(t: &crate::store::TargetRow) -> Value {
    json!({ "id": t.id, "name": t.name, "kind": t.kind, "product": t.product, "definition": t.definition, "declared": t.declared })
}

pub fn target_kind(kind: &str) -> TargetKind {
    if kind == "managed" { TargetKind::Managed } else { TargetKind::External }
}

/// The agents as a run keeps them: names, URLs and where they stand.
pub fn agents_snapshot(config: &Config, names: &[String]) -> Value {
    let list: Vec<Value> = config
        .agents
        .iter()
        .filter(|a| names.contains(&a.name))
        .map(|a| json!({ "name": a.name, "url": a.url, "placement": a.placement }))
        .collect();
    json!({ "names": names, "agents": list })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_id_is_six_lower_case_characters_of_the_end() {
        assert_eq!(short("run_0192F3A4B5C6D7E8"), "c6d7e8");
        assert_eq!(short("ab"), "ab");
    }
}
