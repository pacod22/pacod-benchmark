//! Suites, and the plan a suite makes on a target.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use pacod_bench_engine::plan::{Plan, Suite, expand, with_declared_mock};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState, Principal};
use crate::runs::target_kind;
use crate::store::TargetRow;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/suites", get(list).post(create))
        .route("/suites/{id}", get(one).put(update).delete(remove))
        .route("/plan", post(plan))
}

/// A suite as JSON (`document`) or as YAML text (`yaml`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuiteInput {
    #[serde(default)]
    pub document: Option<Value>,
    #[serde(default)]
    pub yaml: Option<String>,
}

/// The suite of a request: checked, and back as the JSON the database keeps.
pub fn read_suite(input: &SuiteInput) -> ApiResult<(Suite, Value)> {
    let suite = match (&input.document, &input.yaml) {
        (Some(doc), None) => serde_json::from_value::<Suite>(doc.clone())
            .map_err(|e| ApiError::invalid(format!("the suite does not read: {e}")))?,
        (None, Some(text)) => {
            if text.len() > 200_000 {
                return Err(ApiError::invalid("the suite is longer than 200 kB"));
            }
            Suite::parse(text).map_err(ApiError::invalid)?
        }
        _ => return Err(ApiError::invalid("send the suite as document or as yaml, one of them")),
    };
    if suite.name.trim().is_empty() || suite.name.len() > 80 {
        return Err(ApiError::invalid("the suite's name is 1 to 80 characters"));
    }
    let doc = serde_json::to_value(&suite).map_err(|e| ApiError::invalid(e.to_string()))?;
    Ok((suite, doc))
}

/// The suite expanded on a target. An external target that declares its mock gets the suite's
/// mock replaced by it, so that its cells line up with other targets' behind the same mock.
pub fn plan_on(st: &AppState, suite: &Suite, target: &TargetRow) -> ApiResult<Plan> {
    let mut suite = suite.clone();
    let mut notes = Vec::new();
    if target.kind == "external" {
        let declared = serde_json::from_value(target.definition["mock"].clone()).ok().flatten();
        notes.extend(with_declared_mock(&mut suite, declared));
    }
    let mut plan = expand(&suite, target_kind(&target.kind), st.config.max_cells).map_err(ApiError::invalid)?;
    plan.notes.extend(notes);
    if plan.estimate.duration_s > st.config.max_duration_s {
        return Err(ApiError::invalid(format!(
            "the run would take about {} s, more than the {} s allowed (BENCH_MAX_DURATION)",
            plan.estimate.duration_s, st.config.max_duration_s
        )));
    }
    Ok(plan)
}

async fn list(State(st): State<AppState>, _p: Principal) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": st.store.suites().await? })))
}

async fn one(State(st): State<AppState>, _p: Principal, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(st.store.suite(&id).await?.ok_or_else(|| ApiError::not_found("suite"))?)))
}

async fn create(
    State(st): State<AppState>,
    p: Principal,
    Json(input): Json<SuiteInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require("bench.suites.manage")?;
    let (suite, doc) = read_suite(&input)?;
    let row = st.store.insert_suite(&suite.name, &doc, &p.display).await?;
    Ok((StatusCode::CREATED, Json(json!(row))))
}

async fn update(
    State(st): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(input): Json<SuiteInput>,
) -> ApiResult<Json<Value>> {
    p.require("bench.suites.manage")?;
    let (suite, doc) = read_suite(&input)?;
    let row = st.store.update_suite(&id, &suite.name, &doc).await?;
    Ok(Json(json!(row.ok_or_else(|| ApiError::not_found("suite"))?)))
}

async fn remove(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<StatusCode> {
    p.require("bench.suites.manage")?;
    if st.store.delete_suite(&id).await? { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError::not_found("suite")) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanInput {
    #[serde(default)]
    suite_id: Option<String>,
    #[serde(default)]
    suite: Option<SuiteInput>,
    target_id: String,
}

/// The cells a suite makes on a target, the ones left out with the reason, and the estimate.
async fn plan(State(st): State<AppState>, _p: Principal, Json(input): Json<PlanInput>) -> ApiResult<Json<Value>> {
    let suite = match (&input.suite_id, &input.suite) {
        (Some(id), None) => {
            let row = st.store.suite(id).await?.ok_or_else(|| ApiError::not_found("suite"))?;
            serde_json::from_value::<Suite>(row.document).map_err(|e| ApiError::invalid(e.to_string()))?
        }
        (None, Some(s)) => read_suite(s)?.0,
        _ => return Err(ApiError::invalid("send suite_id or suite, one of them")),
    };
    let target = st.store.target(&input.target_id).await?.ok_or_else(|| ApiError::not_found("target"))?;
    Ok(Json(json!(plan_on(&st, &suite, &target)?)))
}
