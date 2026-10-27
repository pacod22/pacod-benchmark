//! Targets: the gateways the benchmark manages, from its configuration or added here with their
//! admin tokens (sealed, `secrets.rs`), and external targets (a URL and a model; the key is
//! pasted for each run).

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use chrono::Utc;
use pacod_bench_engine::controller::{self, Target};
use pacod_bench_engine::gw::GwAdmin;
use pacod_bench_engine::model::MockProfile;
use pacod_bench_engine::requests::Names;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState, Principal};
use crate::config::check_url;
use crate::secrets;
use crate::store::{NewTarget, StoredToken, TargetRow, new_id};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/targets", get(list).post(create))
        .route("/targets/presets", get(presets))
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

/// A gateway the benchmark manages, added here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedDefinition {
    /// Its management API, as the benchmark reaches it.
    pub admin_url: String,
    /// The mock as that gateway reaches it: it must be in the gateway's egress allow-list.
    pub mock_url: String,
    /// The data plane as the agents reach it; none: each agent's own target.
    #[serde(default)]
    pub data_plane: Option<String>,
    /// Its console, for links to published reports.
    #[serde(default)]
    pub console_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetInput {
    name: String,
    /// `external` (the default) or `managed`; a target keeps its kind.
    #[serde(default)]
    kind: Option<String>,
    #[serde(default = "default_product")]
    product: String,
    #[serde(default)]
    definition: Option<Value>,
    #[serde(default)]
    declared: Option<Value>,
    /// A managed gateway's platform-admin token: checked at the gateway, then sealed. Never
    /// answered, never logged.
    #[serde(default)]
    token: Option<String>,
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

/// A managed gateway's addresses, each checked, without a trailing slash; an empty optional one
/// is none.
fn check_managed(v: &Value) -> ApiResult<Value> {
    let d: ManagedDefinition =
        serde_json::from_value(v.clone()).map_err(|e| ApiError::invalid(format!("definition: {e}")))?;
    let one = |field: &str, value: &str| -> ApiResult<String> {
        check_url(value).map_err(|e| ApiError::invalid(format!("definition.{field}: {e}")))?;
        Ok(value.trim_end_matches('/').to_owned())
    };
    let optional = |field: &str, value: Option<String>| -> ApiResult<Option<String>> {
        match value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
            Some(v) => one(field, &v).map(Some),
            None => Ok(None),
        }
    };
    Ok(json!({
        "admin_url": one("admin_url", d.admin_url.trim())?,
        "mock_url": one("mock_url", d.mock_url.trim())?,
        "data_plane": optional("data_plane", d.data_plane)?,
        "console_url": optional("console_url", d.console_url)?,
    }))
}

fn check_token(token: Option<&str>) -> ApiResult<Option<String>> {
    let Some(t) = token.map(str::trim).filter(|t| !t.is_empty()) else { return Ok(None) };
    if t.len() > 4096 || t.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(ApiError::invalid("the token is not a valid header value"));
    }
    Ok(Some(t.to_owned()))
}

const REFUSED: &str = "the gateway refuses this token";

/// Asks the gateway who the token is (`GET /session` there): it must be a platform admin's.
/// What the gateway answered, for the target's page.
async fn check_at_gateway(gw: &GwAdmin) -> ApiResult<Value> {
    let who = gw.session().await.map_err(|e| match e.status() {
        Some(401 | 403) => ApiError::invalid(REFUSED),
        _ => ApiError::bad_gateway(e.to_string()),
    })?;
    let display = who["display"].as_str().unwrap_or("").to_owned();
    if who["platform_admin"].as_bool() != Some(true) {
        return Err(ApiError::invalid(format!(
            "the gateway takes this token for {display:?}, which is no platform admin: give it the role platform admin at the platform"
        )));
    }
    Ok(json!({
        "display": display,
        "kind": who["kind"],
        "id": who["id"],
        "platform_admin": true,
        "checked_at": Utc::now(),
    }))
}

/// A managed gateway's token, checked at the gateway and sealed for this target and address.
async fn seal_token(st: &AppState, target_id: &str, admin_url: &str, token: &str) -> ApiResult<StoredToken> {
    let keys = st.config.keys.as_ref().ok_or_else(|| {
        ApiError::conflict("BENCH_KEK is not set: the benchmark cannot keep a gateway's token; ask the operator")
    })?;
    let checked = check_at_gateway(&GwAdmin::new(admin_url, token, st.http.clone())).await?;
    let sealed = secrets::seal(keys.current(), &secrets::token_context(target_id, admin_url), token.as_bytes());
    Ok(StoredToken { sealed: json!(sealed), hint: secrets::fingerprint(token), checked })
}

fn unique(e: sqlx::Error) -> ApiError {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => ApiError::conflict("a target of that name exists"),
        _ => e.into(),
    }
}

/// A target as the API answers it: with whether its gateway takes runs now.
fn view(st: &AppState, t: &TargetRow) -> Value {
    let mut v = json!(t);
    if t.kind == "managed" {
        let reason = st.gateways.unusable(&t.id);
        v["usable"] = json!(reason.is_none());
        v["unusable_reason"] = json!(reason);
    } else {
        v["usable"] = json!(true);
    }
    v
}

async fn list(State(st): State<AppState>, _p: Principal) -> ApiResult<Json<Value>> {
    let items: Vec<Value> = st.store.targets().await?.iter().map(|t| view(&st, t)).collect();
    Ok(Json(json!({ "items": items })))
}

async fn one(State(st): State<AppState>, _p: Principal, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let t = st.store.target(&id).await?.ok_or_else(|| ApiError::not_found("target"))?;
    Ok(Json(view(&st, &t)))
}

/// What the form for a new gateway starts from: the gateway of this stack, and whether one of
/// the targets already is it.
async fn presets(State(st): State<AppState>, _p: Principal) -> ApiResult<Json<Value>> {
    let internal = &st.config.internal;
    let same = |u: &Value| u.as_str().is_some_and(|u| u.trim_end_matches('/') == internal.admin_url);
    let taken_by = st
        .store
        .targets()
        .await?
        .into_iter()
        .find(|t| t.kind == "managed" && same(&t.definition["admin_url"]))
        .map(|t| json!({ "id": t.id, "name": t.name }));
    Ok(Json(json!({
        "internal": {
            "admin_url": internal.admin_url,
            "mock_url": internal.mock_url,
            "data_plane": internal.data_plane,
            "taken_by": taken_by,
        },
        "can_keep_tokens": st.config.keys.is_some(),
    })))
}

async fn create(
    State(st): State<AppState>,
    p: Principal,
    Json(t): Json<TargetInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require("bench.targets.manage")?;
    check_name(&t.name)?;
    let declared = t.declared.clone().unwrap_or_else(|| json!({}));
    check_declared(&declared)?;
    let definition = t.definition.as_ref().ok_or_else(|| ApiError::invalid("definition is required"))?;
    let id = new_id("tgt");
    let row = match t.kind.as_deref().unwrap_or("external") {
        "external" => {
            if t.token.is_some() {
                return Err(ApiError::invalid("an external target's key is pasted for each run, never stored"));
            }
            if !PRODUCTS.contains(&t.product.as_str()) {
                return Err(ApiError::invalid("product is pacod_gw, pacod_gw_lite or third_party"));
            }
            let definition = check_definition(definition)?;
            let new = NewTarget {
                id: &id,
                name: &t.name,
                kind: "external",
                product: &t.product,
                definition: &definition,
                declared: &declared,
                by: &p.display,
                token: None,
            };
            st.store.insert_target(&new).await.map_err(unique)?
        }
        "managed" => {
            let definition = check_managed(definition)?;
            let token = check_token(t.token.as_deref())?
                .ok_or_else(|| ApiError::invalid("a gateway needs its platform-admin token"))?;
            if st.store.target_by_name(&t.name).await?.is_some() {
                return Err(ApiError::conflict("a target of that name exists"));
            }
            let admin_url = definition["admin_url"].as_str().unwrap_or_default().to_owned();
            let stored = seal_token(&st, &id, &admin_url, &token).await?;
            let new = NewTarget {
                id: &id,
                name: &t.name,
                kind: "managed",
                product: "pacod_gw",
                definition: &definition,
                declared: &declared,
                by: &p.display,
                token: Some(&stored),
            };
            let row = st.store.insert_target(&new).await.map_err(unique)?;
            st.gateways.reload(&row, &st.store, st.config.keys.as_ref(), &st.http).await.map_err(internal)?;
            observe(&st, &row.id).await;
            tracing::info!(target_id = %row.id, name = %row.name, by = %p.display, admin_url = %admin_url, "a gateway was added");
            st.store.target(&row.id).await?.unwrap_or(row)
        }
        _ => return Err(ApiError::invalid("kind is external or managed")),
    };
    Ok((StatusCode::CREATED, Json(view(&st, &row))))
}

fn internal(e: anyhow::Error) -> ApiError {
    tracing::error!(error = format!("{e:#}"), "reading a gateway again failed");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "the benchmark's database failed")
}

/// The gateway's fingerprint read at once, so that the new target shows what it is. A failure
/// here leaves it for the next check.
async fn observe(st: &AppState, id: &str) {
    let Ok(g) = st.gateways.get(id) else { return };
    if let Ok(status) = g.gw.status().await {
        let cache = g.gw.cache_view().await.unwrap_or(Value::Null);
        let _ = st.store.set_observed(id, &json!({ "status": status, "cache": cache })).await;
    }
}

/// External targets: everything. Gateways added here: the name, the addresses, the declared
/// fingerprint, the token; a new management API needs the token again. Gateways of the
/// configuration: their declared fingerprint only.
async fn update(
    State(st): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(t): Json<TargetInput>,
) -> ApiResult<Json<Value>> {
    p.require("bench.targets.manage")?;
    let existing = st.store.target(&id).await?.ok_or_else(|| ApiError::not_found("target"))?;
    if t.kind.as_deref().is_some_and(|k| k != existing.kind) {
        return Err(ApiError::invalid("a target keeps its kind; add a new target instead"));
    }
    let declared = t.declared.clone().unwrap_or_else(|| existing.declared.clone());
    check_declared(&declared)?;
    if existing.source == "config" {
        if t.definition.is_some() || t.token.is_some() || t.name != existing.name {
            return Err(ApiError::invalid(
                "a gateway of the configuration changes in the configuration; only its declared fingerprint changes here",
            ));
        }
        st.store.set_declared(&id, &declared).await?;
        let row = st.store.target(&id).await?.ok_or_else(|| ApiError::not_found("target"))?;
        return Ok(Json(view(&st, &row)));
    }
    check_name(&t.name)?;
    let row = if existing.kind == "managed" {
        let definition = check_managed(t.definition.as_ref().unwrap_or(&existing.definition))?;
        let token = check_token(t.token.as_deref())?;
        let admin_url = definition["admin_url"].as_str().unwrap_or_default().to_owned();
        let moved = existing.definition["admin_url"].as_str() != Some(admin_url.as_str());
        let addresses_change = moved
            || ["mock_url", "data_plane"].iter().any(|f| existing.definition[*f] != definition[*f])
            || token.is_some();
        if addresses_change && let Some(why) = st.store.target_in_use(&id).await? {
            return Err(ApiError::conflict(why));
        }
        let stored = match token {
            Some(token) => Some(seal_token(&st, &id, &admin_url, &token).await?),
            None if moved => {
                return Err(ApiError::invalid(
                    "a new management API needs the token again: the stored one is bound to the old address",
                ));
            }
            None => None,
        };
        let row = st
            .store
            .update_target(&id, &t.name, "pacod_gw", &definition, &declared, stored.as_ref())
            .await
            .map_err(unique)?
            .ok_or_else(|| ApiError::not_found("target"))?;
        st.gateways.reload(&row, &st.store, st.config.keys.as_ref(), &st.http).await.map_err(internal)?;
        tracing::info!(target_id = %id, by = %p.display, token_replaced = stored.is_some(), "a gateway was changed");
        row
    } else {
        if t.token.is_some() {
            return Err(ApiError::invalid("an external target's key is pasted for each run, never stored"));
        }
        if !PRODUCTS.contains(&t.product.as_str()) {
            return Err(ApiError::invalid("product is pacod_gw, pacod_gw_lite or third_party"));
        }
        let definition = check_definition(t.definition.as_ref().unwrap_or(&existing.definition))?;
        st.store
            .update_target(&id, &t.name, &t.product, &definition, &declared, None)
            .await
            .map_err(unique)?
            .ok_or_else(|| ApiError::not_found("target"))?
    };
    Ok(Json(view(&st, &row)))
}

async fn remove(State(st): State<AppState>, p: Principal, Path(id): Path<String>) -> ApiResult<StatusCode> {
    p.require("bench.targets.manage")?;
    let existing = st.store.target(&id).await?.ok_or_else(|| ApiError::not_found("target"))?;
    if existing.source == "config" {
        return Err(ApiError::invalid("a gateway of the configuration is removed from the configuration"));
    }
    if let Some(why) = st.store.target_in_use(&id).await? {
        return Err(ApiError::conflict(why));
    }
    if st.settings.effective().publish_target_id.as_deref() == Some(id.as_str()) {
        return Err(ApiError::conflict("reports are published to this gateway (the settings); change them first"));
    }
    if !st.store.delete_target(&id).await? {
        return Err(ApiError::not_found("target"));
    }
    st.gateways.remove(&id);
    tracing::info!(target_id = %id, name = %existing.name, by = %p.display, "a target was removed");
    Ok(StatusCode::NO_CONTENT)
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

/// A target checked now. A managed gateway: who it takes the token for, its fingerprint (what it
/// says about itself and its cache), then one call through it, with a probe scene of its own
/// taken down after, so that a mock outside its egress allow-list shows before a run. An
/// external target: one call, with a key sent for this check only.
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
        Some(name) => {
            Some(st.agents.get(name).ok_or_else(|| ApiError::invalid(format!("unknown agent {name}")))?.agent)
        }
        None => st.agents.first(),
    };
    let token = st.config.agent_token.clone().unwrap_or_default();
    let agent = agent.map(|a| pacod_bench_engine::agents::AgentClient::new(a, &token, st.http.clone()));
    let short = crate::runs::short(&new_id("chk"));
    let mut token = None;
    let (observed, run_target) = if target.kind == "managed" {
        let g = st.gateways.get(&target.id).map_err(ApiError::conflict)?;
        let refused = || match target.source.as_str() {
            "config" => ApiError::bad_gateway("the gateway refuses BENCH_GW_ADMIN_TOKEN"),
            _ => ApiError::bad_gateway("the gateway refuses the token stored with this target; replace it"),
        };
        let who = check_at_gateway(&g.gw).await.map_err(|e| if e.message == REFUSED { refused() } else { e })?;
        st.store.set_token_checked(&id, &who).await?;
        token = Some(who);
        let status = g.gw.status().await.map_err(|e| match e.status() {
            Some(401 | 403) => refused(),
            _ => e.into(),
        })?;
        let cache = g.gw.cache_view().await.unwrap_or(Value::Null);
        let observed = json!({ "status": status, "cache": cache });
        st.store.set_observed(&id, &observed).await?;
        let run_target =
            Target::Managed { gw: g.gw.clone(), mock_url: g.mock_url.clone(), data_plane: g.data_plane.clone() };
        (Some(observed), Some(run_target))
    } else {
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
    };
    let probe = match (run_target, agent) {
        (Some(t), Some(a)) => Some(controller::probe(&t, &a, &short).await.map_err(ApiError::bad_gateway)?),
        // No agent configured, or an external target checked without its key.
        _ => None,
    };
    Ok(Json(json!({ "kind": target.kind, "token": token, "observed": observed, "probe": probe })))
}
