//! Health, the caller's session, comparisons, and leftovers.

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState, Principal};
use crate::runs::cell_view_row;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/readyz", get(readyz))
        .route("/session", get(session))
        .route("/compare", get(compare))
        .route("/leftovers/clean", post(clean_leftovers))
}

/// Ready when the database answers and, with pacod-id, the copy of the roles is current.
async fn readyz(State(st): State<AppState>) -> ApiResult<Json<Value>> {
    st.store.ping().await?;
    if let super::AuthMode::Oidc(id) = st.auth.as_ref()
        && !id.fresh()
    {
        return Err(ApiError::unavailable("pacod-id's copy of the roles is not current"));
    }
    Ok(Json(json!({ "status": "ready" })))
}

async fn session(State(st): State<AppState>, p: Principal) -> Json<Value> {
    let c = &st.config;
    let limits = st.settings.effective();
    Json(json!({
        "principal": { "id": p.id, "display": p.display, "kind": p.kind },
        "permissions": p.permissions(),
        "gateways": st.gateways.names(),
        "agents": st.agents.len(),
        "agent_names": st.agents.names(),
        "default_agents": limits.default_agents,
        "baseline": limits.baseline_mock_url.is_some(),
        "publish": super::runs::destination(&st).is_some(),
        "console_url": c.console_url,
        "limits": { "max_cells": limits.max_cells, "max_duration_s": limits.max_duration_s, "agent_wait_s": limits.agent_wait_s },
        "active_run": st.runs.active_id(),
    }))
}

#[derive(Deserialize)]
struct CompareQuery {
    /// Run ids, separated by commas: two to six.
    runs: String,
}

/// Runs side by side: each with its target, fingerprint and cells. The UI aligns cells by their
/// comparison key; only cells with the same key ran the same configuration.
async fn compare(State(st): State<AppState>, _p: Principal, Query(q): Query<CompareQuery>) -> ApiResult<Json<Value>> {
    let ids: Vec<&str> = q.runs.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if !(2..=6).contains(&ids.len()) {
        return Err(ApiError::invalid("compare two to six runs"));
    }
    let mut out = Vec::new();
    for id in ids {
        let run = st.store.run(id).await?.ok_or_else(|| ApiError::not_found("run"))?;
        let cells: Vec<Value> = st.store.cells(id).await?.iter().map(cell_view_row).collect();
        out.push(json!({ "run": run, "cells": cells }));
    }
    Ok(Json(json!({ "runs": out })))
}

/// Takes down again what finished runs left on their gateways.
async fn clean_leftovers(State(st): State<AppState>, p: Principal) -> ApiResult<Json<Value>> {
    p.require("bench.targets.manage")?;
    let mut cleaned = 0;
    let mut left = 0;
    for run in st.store.runs(500, None).await? {
        let has = run.leftovers.as_array().is_some_and(|a| !a.is_empty())
            || run.scene["ledger"].as_array().is_some_and(|a| !a.is_empty());
        let finished = pacod_bench_engine::controller::RunStatus::parse(&run.status).is_some_and(|s| s.finished());
        if has && finished {
            let still = st.runs.clean(&run).await;
            left += still.len();
            cleaned += 1;
        }
    }
    Ok(Json(json!({ "runs": cleaned, "left": left })))
}
