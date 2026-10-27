//! Targets: the gateways the benchmark manages (from its configuration) and external targets
//! (a URL and a model; the key is pasted for each run).

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use pacod_bench_engine::controller::{self, Target};
use pacod_bench_engine::model::MockProfile;
use pacod_bench_engine::requests::Names;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState, Principal};
use crate::config::check_url;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/targets", get(list).post(create))
        .route("/targets/{id}", get(one).put(update).delete(remove))
        .route("/targets/{id}/check", post(check))
}

const PRODUCTS: [&str; 3] = ["pacod_gw", "pacod_gw_lite", "third_party"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalDefinition {
    /// The gateway as the agents reach it.
    pub base_url: String,
    /// The model as that gateway names it.
    pub model: String,
    /// Proxy mode: the provider in `/proxy/<provider>/…` and the model's id there.
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub upstream: String,
    /// Calls may reach a paid provider: each cell needs a number of calls at most, and each run
    /// an acknowledgement of the cost.
    #[serde(default = "default_paid")]
    pub paid: bool,
    /// The mock behind that gateway, as the operator set it up; without it the run's cells are
    /// not compared with other targets' (`mock_undeclared`).
    #[serde(default)]
    pub mock: Option<MockProfile>,
}

fn default_paid() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetInput {
    name: String,
    #[serde(default = "default_product")]
    product: String,
    #[serde(default)]
    definition: Option<Value>,
    #[serde(default)]
    declared: Option<Value>,
}

fn default_product() -> String {
    "third_party".into()
}

fn check_name(name: &str) -> ApiResult<()> {
    let ok = !name.is_empty()
        && name.len() <= 60
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '));
    if ok { Ok(()) } else { Err(ApiError::invalid("the name is 1 to 60 letters, digits, spaces, - _ .")) }
}

/// The declared fingerprint: an object of short values.
fn check_declared(v: &Value) -> ApiResult<()> {
    let Some(o) = v.as_object() else { return Err(ApiError::invalid("declared is an object")) };
    if o.len() > 20 || serde_json::to_string(v).map_or(true, |s| s.len() > 4000) {
        return Err(ApiError::invalid("declared holds at most 20 short fields"));
    }
    Ok(())
}

fn check_definition(v: &Value) -> ApiResult<Value> {
    let d: ExternalDefinition =
        serde_json::from_value(v.clone()).map_err(|e| ApiError::invalid(format!("definition: {e}")))?;
    check_url(&d.base_url).map_err(|e| ApiError::invalid(format!("definition.base_url: {e}")))?;
    if d.model.is_empty() || d.model.len() > 200 {
        return Err(ApiError::invalid("definition.model names the model as that gateway does"));
    }
    if d.provider.len() > 100 || d.upstream.len() > 200 || d.provider.contains('/') {
        return Err(ApiError::invalid("definition.provider is a provider's name, definition.upstream a model id"));
    }
    if let Some(m) = &d.mock {
        m.check().map_err(|e| ApiError::invalid(format!("definition.{e}")))?;
    }
    let mut out = v.clone();
    out["base_url"] = json!(d.base_url.trim_end_matches('/'));
    out["paid"] = json!(d.paid);
    Ok(out)
}

async fn list(State(st): State<AppState>, _p: Principal) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": st.store.targets().await? })))
}

async fn one(State(st): State<AppState>, _p: Principal, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(st.store.target(&id).await?.ok_or_else(|| ApiError::not_found("target"))?)))
}

async fn create(
    State(st): State<AppState>,
    p: Principal,
    Json(t): Json<TargetInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require("bench.targets.manage")?;
    check_name(&t.name)?;
    if !PRODUCTS.contains(&t.product.as_str()) {
        return Err(ApiError::invalid("product is pacod_gw, pacod_gw_lite or third_party"));
    }
    let definition =
        check_definition(t.definition.as_ref().ok_or_else(|| ApiError::invalid("definition is required"))?)?;
    let declared = t.declared.unwrap_or_else(|| json!({}));
    check_declared(&declared)?;
    let row =
        st.store.insert_target(&t.name, &t.product, &definition, &declared, &p.display).await.map_err(
            |e| match &e {
                sqlx::Error::Database(d) if d.is_unique_violation() => {
                    ApiError::conflict("a target of that name exists")
                }
                _ => e.into(),
            },
        )?;
    Ok((StatusCode::CREATED, Json(json!(row))))
}

/// External targets: everything. Managed gateways: their declared fingerprint only.
async fn update(
    State(st): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(t): Json<TargetInput>,
) -> ApiResult<Json<Value>> {
    p.require("bench.targets.manage")?;
    let existing = st.store.target(&id).await?.ok_or_else(|| ApiError::not_found("target"))?;
    let declared = t.declared.unwrap_or_else(|| existing.declared.clone());
    check_declared(&declared)?;
    if existing.source == "config" {
        if t.definition.is_some() || t.name != existing.name {
            return Err(ApiError::invalid(
                "a gateway of the configuration changes in the configuration; only its declared fingerprint changes here",
            ));
        }
        st.store.set_declared(&id, &declared).await?;
        return Ok(Json(json!(st.store.target(&id).await?)));
    }
    check_name(&t.name)?;
    if !PRODUCTS.contains(&t.product.as_str()) {
        return Err(ApiError::invalid("product is pacod_gw, pacod_gw_lite or third_party"));
    }
    let definition = check_definition(t.definition.as_ref().unwrap_or(&existing.definition))?;
    let row = st.store.update_target(&id, &t.name, &t.product, &definition, &declared).await?;
    Ok(Json(json!(row.ok_or_else(|| ApiError::not_found("target"))?)))
}

async fn remove(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<StatusCode> {
    p.require("bench.targets.manage")?;
    if st.store.delete_target(&id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("external target"))
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct CheckInput {
    /// The agent that makes the probe; the first configured one when left out.
    #[serde(default)]
    agent: Option<String>,
    /// An external target's key, for this probe only; never stored.
    #[serde(default)]
    key: Option<String>,
}

/// A target checked now. A managed gateway: its fingerprint (what it says about itself and its
/// cache), then one call through it, with a probe scene of its own taken down after, so that a
/// mock outside its egress allow-list shows before a run. An external target: one call, with a
/// key sent for this check only.
async fn check(
    State(st): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    body: Option<Json<CheckInput>>,
) -> ApiResult<Json<Value>> {
    p.require("bench.runs.start")?;
    let input = body.map(|Json(b)| b).unwrap_or_default();
    let target = st.store.target(&id).await?.ok_or_else(|| ApiError::not_found("target"))?;
    let agent = match &input.agent {
        Some(name) => st.config.agents.iter().find(|a| &a.name == name),
        None => st.config.agents.first(),
    };
    let token = st.config.agent_token.clone().unwrap_or_default();
    let agent = agent.map(|a| pacod_bench_engine::agents::AgentClient::new(a.clone(), &token, st.http.clone()));
    let short = crate::runs::short(&crate::store::new_id("chk"));
    let (observed, run_target) = match st.gateways.get(&target.id) {
        Some(gw) => {
            let status = gw.status().await.map_err(|e| match e.status() {
                Some(401 | 403) => ApiError::bad_gateway("the gateway refuses BENCH_GW_ADMIN_TOKEN"),
                _ => e.into(),
            })?;
            let cache = gw.cache_view().await.unwrap_or(Value::Null);
            let observed = json!({ "status": status, "cache": cache });
            st.store.set_observed(&id, &observed).await?;
            let g = st.config.gateways.iter().find(|g| format!("cfg-{}", g.name) == target.id);
            let run_target = g.map(|g| Target::Managed {
                gw: gw.clone(),
                mock_url: g.mock_url.clone(),
                data_plane: g.data_plane.clone(),
            });
            (Some(observed), run_target)
        }
        None => {
            let key = input.key.as_deref().map(str::trim).filter(|k| !k.is_empty());
            let run_target = key.map(|k| {
                let d = &target.definition;
                Target::External {
                    base_url: d["base_url"].as_str().unwrap_or("").to_owned(),
                    key: k.to_owned(),
                    names: Names {
                        model: d["model"].as_str().unwrap_or("").to_owned(),
                        provider: d["provider"].as_str().unwrap_or("").to_owned(),
                        upstream: d["upstream"].as_str().unwrap_or("").to_owned(),
                    },
                    paid: d["paid"].as_bool().unwrap_or(true),
                }
            });
            (None, run_target)
        }
    };
    let probe = match (run_target, agent) {
        (Some(t), Some(a)) => Some(controller::probe(&t, &a, &short).await.map_err(ApiError::bad_gateway)?),
        // No agent configured, or an external target checked without its key.
        _ => None,
    };
    Ok(Json(json!({ "kind": target.kind, "observed": observed, "probe": probe })))
}
