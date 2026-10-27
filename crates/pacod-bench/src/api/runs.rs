//! Runs: start, follow, cancel, read.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use futures_util::stream::{self, Stream, StreamExt};
use pacod_bench_engine::controller::{CellResult, RunStatus, key_hint};
use pacod_bench_engine::gw::GwAdmin;
use pacod_bench_engine::measure::classify;
use pacod_bench_engine::plan::{CellKind, Suite};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;

use super::suites::{SuiteInput, plan_on, read_suite};
use super::{ApiError, ApiResult, AppState, Principal};
use crate::reports::{self, mock_shared};
use crate::runs::{agents_snapshot, cell_view_row, target_snapshot};
use crate::store::{Move, NewCell, NewPublication, NewRun, RunRow, new_id};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/runs", get(list).post(start))
        .route("/queue", get(queue))
        .route("/runs/{id}", get(one).delete(remove))
        .route("/runs/{id}/move", post(move_run))
        .route("/runs/{id}/rerun", post(rerun))
        .route("/runs/{id}/cells/{idx}", get(cell))
        .route("/runs/{id}/cancel", post(cancel))
        .route("/runs/{id}/events", get(events))
        .route("/runs/{id}/report", get(report))
        .route("/runs/{id}/publish", post(publish))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    limit: Option<i64>,
    /// The `created_at` of the last run of the previous page.
    #[serde(default)]
    before: Option<DateTime<Utc>>,
}

/// A run as lists show it: without what only its page shows.
fn slim(r: &RunRow) -> Value {
    let mut v = json!(r);
    let o = v.as_object_mut().expect("a run is an object");
    o.remove("suite");
    o.remove("scene");
    o.insert("estimate".into(), r.plan["estimate"].clone());
    o.remove("plan");
    v
}

async fn list(State(st): State<AppState>, _p: Principal, Query(q): Query<ListQuery>) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = st.store.runs(limit, q.before).await?;
    let next = (rows.len() as i64 == limit).then(|| rows.last().map(|r| r.created_at)).flatten();
    let items: Vec<Value> = rows.iter().map(slim).collect();
    Ok(Json(json!({ "items": items, "next_before": next })))
}

/// The run that goes, and the queued runs in the order they start.
async fn queue_view(st: &AppState) -> ApiResult<Value> {
    let rows = st.store.queue().await?;
    let active = rows.iter().find(|r| r.status != "queued").map(slim);
    let queued: Vec<Value> = rows.iter().filter(|r| r.status == "queued").map(slim).collect();
    Ok(json!({ "active": active, "queued": queued }))
}

async fn queue(State(st): State<AppState>, _p: Principal) -> ApiResult<Json<Value>> {
    Ok(Json(queue_view(&st).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveInput {
    to: Move,
}

/// A queued run moved one place up or down, or to the front of the queue.
async fn move_run(
    State(st): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(m): Json<MoveInput>,
) -> ApiResult<Json<Value>> {
    p.require("bench.runs.start")?;
    if !st.store.move_queued(&id, m.to).await? {
        return Err(ApiError::conflict("the run is not queued"));
    }
    Ok(Json(queue_view(&st).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartInput {
    #[serde(default)]
    suite_id: Option<String>,
    #[serde(default)]
    suite: Option<SuiteInput>,
    target_id: String,
    /// The agents to use; all configured ones when left out.
    #[serde(default)]
    agents: Option<Vec<String>>,
    #[serde(default)]
    label: String,
    /// An external target's key: held in memory until the run starts, never stored.
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    acknowledge_cost: bool,
}

async fn start(
    State(st): State<AppState>,
    p: Principal,
    Json(input): Json<StartInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require("bench.runs.start")?;
    let (suite, suite_id): (Suite, Option<String>) = match (&input.suite_id, &input.suite) {
        (Some(id), None) => {
            let row = st.store.suite(id).await?.ok_or_else(|| ApiError::not_found("suite"))?;
            (serde_json::from_value(row.document).map_err(|e| ApiError::invalid(e.to_string()))?, Some(row.id))
        }
        (None, Some(s)) => (read_suite(s)?.0, None),
        _ => return Err(ApiError::invalid("send suite_id or suite, one of them")),
    };
    let row = enqueue(
        &st,
        Enqueue {
            suite,
            suite_id,
            target_id: input.target_id,
            agents: input.agents,
            label: input.label,
            key: input.key,
            acknowledge_cost: input.acknowledge_cost,
            by_id: p.id.clone(),
            by_name: p.display.clone(),
            schedule_id: None,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!(row))))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RerunInput {
    /// The run's own label when left out.
    #[serde(default)]
    label: Option<String>,
    /// An external target's key, again: it was never kept.
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    acknowledge_cost: bool,
}

/// The run again: its copy of the suite, on its target, with its agents, under the settings of
/// now. Its cells have the same comparison keys, so the two line up in a comparison.
async fn rerun(
    State(st): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    body: Option<Json<RerunInput>>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require("bench.runs.start")?;
    let input = body.map(|Json(b)| b).unwrap_or_default();
    let run = st.store.run(&id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    let suite: Suite = serde_json::from_value(run.suite.clone())
        .map_err(|e| ApiError::invalid(format!("the run's copy of its suite does not read: {e}")))?;
    let gone = || ApiError::invalid(format!("the target {} is gone", run.target["name"].as_str().unwrap_or("")));
    let target_id = run.target_id.clone().ok_or_else(gone)?;
    if st.store.target(&target_id).await?.is_none() {
        return Err(gone());
    }
    let agents: Vec<String> = serde_json::from_value(run.agents["names"].clone()).unwrap_or_default();
    if let Some(name) = agents.iter().find(|a| st.agents.get(a).is_none()) {
        return Err(ApiError::invalid(format!("the agent {name} is gone")));
    }
    let row = enqueue(
        &st,
        Enqueue {
            suite,
            suite_id: run.suite_id.clone(),
            target_id,
            agents: Some(agents),
            label: input.label.unwrap_or_else(|| run.label.clone()),
            key: input.key,
            acknowledge_cost: input.acknowledge_cost,
            by_id: p.id.clone(),
            by_name: p.display.clone(),
            schedule_id: None,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!(row))))
}

/// A run to queue, from whatever asked for it: the API, a re-run, a schedule.
pub(crate) struct Enqueue {
    pub suite: Suite,
    pub suite_id: Option<String>,
    pub target_id: String,
    /// The agents to use; the settings' default ones, else every one, when left out.
    pub agents: Option<Vec<String>>,
    pub label: String,
    /// An external target's key: held in memory until the run starts, never stored.
    pub key: Option<String>,
    pub acknowledge_cost: bool,
    pub by_id: String,
    pub by_name: String,
    /// The schedule that asks for the run, if one does.
    pub schedule_id: Option<String>,
}

/// Checks a run (its plan on the target, its agents, an external target's key and cost) and
/// queues it, with copies of its suite, target, agents and the settings of now.
pub(crate) async fn enqueue(st: &AppState, input: Enqueue) -> ApiResult<RunRow> {
    if input.label.len() > 80 {
        return Err(ApiError::invalid("the label is at most 80 characters"));
    }
    let suite = input.suite;
    let target = st.store.target(&input.target_id).await?.ok_or_else(|| ApiError::not_found("target"))?;
    if target.kind == "managed" {
        st.gateways.get(&target.id).map_err(ApiError::conflict)?;
    }
    let plan = plan_on(st, &suite, &target)?;

    let configured = st.agents.names();
    if configured.is_empty() || st.config.agent_token.is_none() {
        return Err(ApiError::invalid(
            "no load agents are configured (BENCH_AGENTS or the Agents page, and BENCH_AGENT_TOKEN)",
        ));
    }
    let settings = st.settings.effective();
    let agents = input.agents.clone().unwrap_or_else(|| {
        if settings.default_agents.is_empty() { configured.clone() } else { settings.default_agents.clone() }
    });
    if agents.is_empty() || agents.len() > 50 {
        return Err(ApiError::invalid("a run has 1 to 50 agents"));
    }
    if let Some(unknown) = agents.iter().find(|a| !configured.contains(a)) {
        return Err(ApiError::invalid(format!("unknown agent {unknown}")));
    }
    let mut key = None;
    if target.kind == "external" {
        let k = input.key.as_deref().map(str::trim).filter(|k| !k.is_empty());
        let k = k.ok_or_else(|| ApiError::invalid("an external target needs its key for this run"))?;
        if k.len() > 4096 || k.chars().any(|c| c.is_control()) {
            return Err(ApiError::invalid("the key is not a valid header value"));
        }
        if target.definition["paid"].as_bool().unwrap_or(true) {
            if !input.acknowledge_cost {
                return Err(ApiError::invalid("confirm that the calls may cost money"));
            }
            if plan.cells.iter().any(|c| c.kind == CellKind::Target && c.config.load.max_calls.is_none()) {
                return Err(ApiError::invalid(
                    "a target that may reach a paid provider needs load.max_calls in every cell",
                ));
            }
        }
        key = Some(k.to_owned());
    } else if input.key.is_some() {
        return Err(ApiError::invalid("a managed gateway's keys are the benchmark's own: send no key"));
    }

    let id = new_id("run");
    let cells: Vec<NewCell> = plan
        .cells
        .iter()
        .map(|c| NewCell {
            idx: c.index as i32,
            rep: c.rep as i32,
            kind: if c.kind == CellKind::Baseline { "baseline".into() } else { "target".into() },
            axes: json!(c.axes),
            config: json!(c.config),
            comparison_key: c.comparison_key.clone(),
            baseline_key: c.baseline_key.clone(),
        })
        .collect();
    let row = st
        .store
        .insert_run(
            &NewRun {
                id: id.clone(),
                label: input.label.clone(),
                suite_id: input.suite_id,
                suite: json!(suite),
                target_id: target.id.clone(),
                target: target_snapshot(&target),
                agents: agents_snapshot(&st.agents, &agents),
                plan: json!({ "estimate": plan.estimate, "pruned": plan.pruned, "notes": plan.notes }),
                started_by: input.by_id,
                started_by_name: input.by_name.clone(),
                key_hint: key.as_deref().map(key_hint).unwrap_or_default(),
                settings: json!(settings),
                schedule_id: input.schedule_id,
            },
            &cells,
        )
        .await?;
    st.runs.queued(&id, key);
    tracing::info!(run_id = %id, by = %input.by_name, target = %target.name, cells = cells.len(), "run queued");
    Ok(row)
}

async fn one(State(st): State<AppState>, _p: Principal, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let run = st.store.run(&id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    let rows = st.store.cells(&id).await?;
    let results: Vec<CellResult> =
        rows.iter().filter_map(|r| r.result.clone()).filter_map(|v| serde_json::from_value(v).ok()).collect();
    let measurement = classify(&results, mock_shared(&run));
    let cells: Vec<Value> = rows.iter().map(cell_view_row).collect();
    let publications = st.store.publications(&id).await?;
    Ok(Json(json!({ "run": run, "cells": cells, "publications": publications, "measurement": measurement })))
}

/// The run's report (`pacod-bench-report/1`), as a file: the same bytes a publish sends.
async fn report(State(st): State<AppState>, _p: Principal, Path(id): Path<String>) -> ApiResult<Response> {
    let run = st.store.run(&id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    if !RunStatus::parse(&run.status).is_some_and(RunStatus::finished) {
        return Err(ApiError::conflict("a run has a report once it has ended"));
    }
    let bytes = reports::report(&st.store, &run).await.map_err(ApiError::invalid)?;
    let disposition = format!("attachment; filename=\"pacod-bench-{id}.json\"");
    Ok(([(header::CONTENT_TYPE, "application/json".to_owned()), (header::CONTENT_DISPOSITION, disposition)], bytes)
        .into_response())
}

/// Where reports go: the gateway the settings name, else `BENCH_PUBLISH_URL`, else the
/// configuration's first gateway, else the one gateway the benchmark manages when there is one
/// only; its client and console.
pub(super) fn destination(st: &AppState) -> Option<(String, Arc<GwAdmin>, Option<String>)> {
    if let Some(id) = st.settings.effective().publish_target_id {
        let g = st.gateways.get(&id).ok()?;
        let console = g.console_url.clone().or_else(|| st.config.console_url.clone());
        return Some((g.admin_url.clone(), g.gw.clone(), console));
    }
    if let Some(p) = &st.config.publish {
        let gw = Arc::new(GwAdmin::new(&p.url, &p.token, st.http.clone()));
        return Some((p.url.clone(), gw, st.config.console_url.clone()));
    }
    let g = st.gateways.only()?;
    let console = g.console_url.clone().or_else(|| st.config.console_url.clone());
    Some((g.admin_url.clone(), g.gw.clone(), console))
}

/// Sends the run's report to pacod-gw ([`destination`]), and records where it went, or why it
/// did not.
async fn publish(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    p.require("bench.reports.publish")?;
    let run = st.store.run(&id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    Ok(Json(publish_run(&st, &run, &p.display).await?))
}

/// Sends a finished run's report to [`destination`] and records where it went, or why it did
/// not; for a caller of the API or a schedule.
pub(crate) async fn publish_run(st: &AppState, run: &RunRow, by: &str) -> ApiResult<Value> {
    let (url, gw, console_url) = destination(st).ok_or_else(|| {
        ApiError::invalid(
            "no gateway to publish to is configured (BENCH_PUBLISH_URL, or a gateway the benchmark manages)",
        )
    })?;
    if !RunStatus::parse(&run.status).is_some_and(RunStatus::finished) {
        return Err(ApiError::conflict("a run is published once it has ended"));
    }
    let bytes = reports::report(&st.store, run).await.map_err(ApiError::invalid)?;
    let digest = hex::encode(ring::digest::digest(&ring::digest::SHA256, &bytes));
    let record = |status: &'static str, gw_id: String, error: String| {
        let (store, run_id, url, digest, by) =
            (st.store.clone(), run.id.clone(), url.clone(), digest.clone(), by.to_owned());
        async move {
            store
                .insert_publication(&NewPublication {
                    run_id: &run_id,
                    destination: &url,
                    status,
                    gw_report_id: &gw_id,
                    digest: &digest,
                    error: &error,
                    by: &by,
                })
                .await
        }
    };
    match gw.publish_report(&bytes).await {
        Ok(answer) => {
            let gw_id = answer["summary"]["id"].as_str().unwrap_or("").to_owned();
            record("published", gw_id.clone(), String::new()).await?;
            tracing::info!(run_id = %run.id, gw_report_id = %gw_id, %by, "report published");
            let link = console_url.as_ref().map(|c| format!("{c}/platform/testing/benchmarks/{gw_id}"));
            Ok(json!({ "gw_report_id": gw_id, "created": answer["created"], "link": link, "digest": digest }))
        }
        Err(e) => {
            record("failed", String::new(), e.to_string()).await?;
            Err(ApiError::bad_gateway(format!("the gateway did not take the report: {e}")))
        }
    }
}

/// One cell with its seconds and the gateway's samples.
async fn cell(
    State(st): State<AppState>,
    _p: Principal,
    Path((id, idx)): Path<(String, i32)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!(st.store.cell(&id, idx).await?.ok_or_else(|| ApiError::not_found("cell"))?)))
}

async fn cancel(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    p.require("bench.runs.start")?;
    if st.runs.cancel_active(&id) {
        return Ok(Json(json!({ "cancelling": true })));
    }
    if st.store.cancel_queued(&id).await? {
        st.runs.forget_key(&id);
        return Ok(Json(json!({ "cancelled": true })));
    }
    Err(ApiError::conflict("the run is neither queued nor going"))
}

async fn remove(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<StatusCode> {
    p.require("bench.targets.manage")?;
    let run = st.store.run(&id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    if !RunStatus::parse(&run.status).is_some_and(RunStatus::finished) {
        return Err(ApiError::conflict("a run is deleted once it has ended"));
    }
    if run.leftovers.as_array().is_some_and(|l| !l.is_empty()) {
        return Err(ApiError::conflict("the run left objects on its gateway; clean them up first"));
    }
    st.store.delete_run(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The run as it is (`snapshot`), then what happens while it goes, as server-sent events.
async fn events(
    State(st): State<AppState>,
    _p: Principal,
    Path(id): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Subscribed before the snapshot is read, so that nothing between the two is lost.
    let live = st.runs.subscribe(&id);
    let run = st.store.run(&id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    let cells: Vec<Value> = st.store.cells(&id).await?.iter().map(cell_view_row).collect();
    let snapshot = Event::default().event("snapshot").data(json!({ "run": run, "cells": cells }).to_string());
    let head = stream::iter([Ok(snapshot)]);
    let tail: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> = match live {
        Some(rx) => Box::pin(BroadcastStream::new(rx).filter_map(|m| async move {
            // A follower too slow for the run misses events; the next snapshot catches it up.
            let v = m.ok()?;
            let name = v["event"].as_str().unwrap_or("message").to_owned();
            Some(Ok(Event::default().event(name).data(v.to_string())))
        })),
        None => Box::pin(stream::iter([Ok(Event::default().event("done").data("{}"))])),
    };
    Ok(Sse::new(head.chain(tail)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
