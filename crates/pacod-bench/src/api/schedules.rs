//! Schedules (`schedules.rs`): a suite on a managed gateway on a timetable. Whoever may start runs
//! makes them; their creator, or an admin, changes and removes them.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState, Principal};
use crate::schedules::{self, Cadence};
use crate::store::{ScheduleInput, ScheduleRow, new_id};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/schedules", get(list).post(create))
        .route("/schedules/{id}", get(one).put(update).delete(remove))
        .route("/schedules/{id}/fire", post(fire))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    name: String,
    suite_id: String,
    target_id: String,
    /// The agents' names; none: the settings' default agents, else every one.
    #[serde(default)]
    agents: Option<Vec<String>>,
    #[serde(default)]
    label: String,
    cadence: Cadence,
    #[serde(default = "yes")]
    enabled: bool,
    /// Publish the report to pacod-gw when the run has finished.
    #[serde(default)]
    publish: bool,
}

fn yes() -> bool {
    true
}

/// The schedule checked against what exists now: its suite, a managed gateway, its agents.
async fn check(st: &AppState, p: &Principal, s: &Input) -> ApiResult<()> {
    if s.name.trim().is_empty() || s.name.len() > 80 {
        return Err(ApiError::invalid("the name is 1 to 80 characters"));
    }
    if s.label.len() > 80 {
        return Err(ApiError::invalid("the label is at most 80 characters"));
    }
    s.cadence.check().map_err(ApiError::invalid)?;
    if st.store.suite(&s.suite_id).await?.is_none() {
        return Err(ApiError::invalid("suite_id: no such suite"));
    }
    let target = st.store.target(&s.target_id).await?.ok_or_else(|| ApiError::invalid("target_id: no such target"))?;
    if target.kind != "managed" {
        return Err(ApiError::invalid(
            "a schedule runs on a gateway the benchmark manages: an external target's key is never kept",
        ));
    }
    if let Some(agents) = &s.agents {
        if agents.is_empty() || agents.len() > 50 {
            return Err(ApiError::invalid("a schedule names 1 to 50 agents, or none for the default ones"));
        }
        if let Some(unknown) = agents.iter().find(|a| st.agents.get(a).is_none()) {
            return Err(ApiError::invalid(format!("unknown agent {unknown}")));
        }
    }
    if s.publish {
        p.require("bench.reports.publish")?;
    }
    Ok(())
}

/// The schedule's creator, or an admin.
fn may_change(p: &Principal, s: &ScheduleRow) -> ApiResult<()> {
    p.require("bench.runs.start")?;
    if s.created_by == p.id || p.allowed("bench.targets.manage") {
        Ok(())
    } else {
        Err(ApiError::new(StatusCode::FORBIDDEN, "forbidden", "only its creator or an admin changes a schedule"))
    }
}

async fn list(State(st): State<AppState>, _p: Principal) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": st.store.schedules().await? })))
}

async fn one(State(st): State<AppState>, _p: Principal, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(st.store.schedule(&id).await?.ok_or_else(|| ApiError::not_found("schedule"))?)))
}

async fn create(
    State(st): State<AppState>,
    p: Principal,
    Json(input): Json<Input>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require("bench.runs.start")?;
    check(&st, &p, &input).await?;
    let agents = input.agents.as_ref().map(|a| json!(a));
    let cadence = json!(input.cadence);
    let row = st
        .store
        .insert_schedule(
            &new_id("sch"),
            &ScheduleInput {
                name: input.name.trim(),
                suite_id: &input.suite_id,
                target_id: &input.target_id,
                agents: agents.as_ref(),
                label: &input.label,
                cadence: &cadence,
                enabled: input.enabled,
                publish: input.publish,
                next_fire_at: input.enabled.then(|| input.cadence.next_after(Utc::now())),
            },
            (&p.id, &p.display, p.kind),
        )
        .await?;
    tracing::info!(schedule = %row.id, name = %row.name, by = %p.display, "a schedule was made");
    Ok((StatusCode::CREATED, Json(json!(row))))
}

/// The next fire is counted again from now when the cadence changes or the schedule is
/// switched on.
async fn update(
    State(st): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(input): Json<Input>,
) -> ApiResult<Json<Value>> {
    let existing = st.store.schedule(&id).await?.ok_or_else(|| ApiError::not_found("schedule"))?;
    may_change(&p, &existing)?;
    check(&st, &p, &input).await?;
    let cadence = json!(input.cadence);
    let next = match (input.enabled, existing.enabled && existing.cadence == cadence) {
        (false, _) => None,
        (true, true) => existing.next_fire_at.or_else(|| Some(input.cadence.next_after(Utc::now()))),
        (true, false) => Some(input.cadence.next_after(Utc::now())),
    };
    let agents = input.agents.as_ref().map(|a| json!(a));
    let row = st
        .store
        .update_schedule(
            &id,
            &ScheduleInput {
                name: input.name.trim(),
                suite_id: &input.suite_id,
                target_id: &input.target_id,
                agents: agents.as_ref(),
                label: &input.label,
                cadence: &cadence,
                enabled: input.enabled,
                publish: input.publish,
                next_fire_at: next,
            },
        )
        .await?
        .ok_or_else(|| ApiError::not_found("schedule"))?;
    Ok(Json(json!(row)))
}

async fn remove(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let existing = st.store.schedule(&id).await?.ok_or_else(|| ApiError::not_found("schedule"))?;
    may_change(&p, &existing)?;
    st.store.delete_schedule(&id).await?;
    tracing::info!(schedule = %id, name = %existing.name, by = %p.display, "a schedule was removed");
    Ok(StatusCode::NO_CONTENT)
}

/// The schedule's run queued now, by the caller, besides its timetable.
async fn fire(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    p.require("bench.runs.start")?;
    let s = st.store.schedule(&id).await?.ok_or_else(|| ApiError::not_found("schedule"))?;
    let run_id = schedules::fire_now(&st, &s, &p).await.map_err(ApiError::conflict)?;
    Ok(Json(json!({ "run_id": run_id })))
}
