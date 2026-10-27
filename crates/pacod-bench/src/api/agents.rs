//! The load agents: every one asked whether it is there, and those an admin adds, changes and
//! removes here (`agents.rs`). Those of the configuration change there.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use futures_util::future::join_all;
use pacod_bench_engine::agents::{AgentClient, AgentRef, valid_name};
use pacod_bench_engine::model::Placement;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState, Principal};
use crate::config::check_url;

pub fn routes() -> Router<AppState> {
    Router::new().route("/agents", get(list).post(create)).route("/agents/{name}", put(update).delete(remove))
}

/// Every agent, asked whether it is there.
async fn list(State(st): State<AppState>, _p: Principal) -> Json<Value> {
    let token = st.config.agent_token.clone().unwrap_or_default();
    let asks = st.agents.all().into_iter().map(|a| {
        let client = AgentClient::new(a.agent.clone(), &token, st.http.clone());
        async move {
            let base =
                json!({ "name": a.agent.name, "url": a.agent.url, "placement": a.agent.placement, "source": a.source });
            match client.info().await {
                Ok(info) => {
                    let mut v = json!(info);
                    v["reachable"] = json!(true);
                    v["url"] = json!(a.agent.url);
                    v["placement"] = json!(a.agent.placement);
                    v["source"] = json!(a.source);
                    v
                }
                Err(e) => {
                    let mut v = base;
                    v["reachable"] = json!(false);
                    v["error"] = json!(e.to_string());
                    v
                }
            }
        }
    });
    Json(json!({ "items": join_all(asks).await, "can_add": st.config.agent_token.is_some() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentInput {
    #[serde(default)]
    name: Option<String>,
    url: String,
    #[serde(default)]
    placement: Option<String>,
}

/// The agent's address and where it stands, checked.
fn read(name: &str, input: &AgentInput) -> ApiResult<AgentRef> {
    if !valid_name(name) {
        return Err(ApiError::invalid("the name is a lower-case name of at most 32: letters, digits, -"));
    }
    let url = input.url.trim().trim_end_matches('/').to_owned();
    check_url(&url).map_err(|e| ApiError::invalid(format!("url: {e}")))?;
    let placement = match input.placement.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => Placement::parse(p)
            .ok_or_else(|| ApiError::invalid("placement is in_stack, same_host, remote or unknown"))?,
        None => Placement::Unknown,
    };
    Ok(AgentRef { name: name.to_owned(), url, placement })
}

async fn create(
    State(st): State<AppState>,
    p: Principal,
    Json(input): Json<AgentInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.require("bench.targets.manage")?;
    if st.config.agent_token.is_none() {
        return Err(ApiError::conflict(
            "BENCH_AGENT_TOKEN is not set: the agents take the benchmark's token; ask the operator",
        ));
    }
    let name = input.name.clone().unwrap_or_default();
    let agent = read(name.trim(), &input)?;
    if st.agents.get(&agent.name).is_some() {
        return Err(ApiError::conflict("an agent of that name exists"));
    }
    let row = st.store.insert_agent(&agent.name, &agent.url, agent.placement.as_str(), &p.display).await.map_err(
        |e| match &e {
            sqlx::Error::Database(d) if d.is_unique_violation() => ApiError::conflict("an agent of that name exists"),
            _ => e.into(),
        },
    )?;
    st.agents.put(agent);
    tracing::info!(agent = %row.name, url = %row.url, by = %p.display, "an agent was added");
    Ok((
        StatusCode::CREATED,
        Json(json!({ "name": row.name, "url": row.url, "placement": row.placement, "source": "api" })),
    ))
}

/// Not while a run that waits or goes uses the agent: the run finds its agents by name when it
/// starts, and keeps where they stood.
async fn update(
    State(st): State<AppState>,
    p: Principal,
    Path(name): Path<String>,
    Json(input): Json<AgentInput>,
) -> ApiResult<Json<Value>> {
    p.require("bench.targets.manage")?;
    let existing = st.agents.get(&name).ok_or_else(|| ApiError::not_found("agent"))?;
    if existing.source == "config" {
        return Err(ApiError::invalid("an agent of the configuration changes there (BENCH_AGENTS)"));
    }
    if input.name.as_deref().is_some_and(|n| n != name) {
        return Err(ApiError::invalid("an agent keeps its name; add a new one instead"));
    }
    let agent = read(&name, &input)?;
    if agent != existing.agent
        && let Some(why) = st.store.agent_in_use(&name).await?
    {
        return Err(ApiError::conflict(why));
    }
    let row = st
        .store
        .update_agent(&name, &agent.url, agent.placement.as_str())
        .await?
        .ok_or_else(|| ApiError::not_found("agent"))?;
    st.agents.put(agent);
    Ok(Json(json!({ "name": row.name, "url": row.url, "placement": row.placement, "source": "api" })))
}

async fn remove(State(st): State<AppState>, p: Principal, Path(name): Path<String>) -> ApiResult<StatusCode> {
    p.require("bench.targets.manage")?;
    let existing = st.agents.get(&name).ok_or_else(|| ApiError::not_found("agent"))?;
    if existing.source == "config" {
        return Err(ApiError::invalid("an agent of the configuration is removed there (BENCH_AGENTS)"));
    }
    if let Some(why) = st.store.agent_in_use(&name).await? {
        return Err(ApiError::conflict(why));
    }
    if let Some(schedule) = st.store.schedule_of_agent(&name).await? {
        return Err(ApiError::conflict(format!("the schedule {schedule} names this agent; change it first")));
    }
    if st.settings.effective().default_agents.contains(&name) {
        return Err(ApiError::conflict("the settings name this agent as a default one; change them first"));
    }
    st.store.delete_agent(&name).await?;
    st.agents.remove(&name);
    tracing::info!(agent = %name, by = %p.display, "an agent was removed");
    Ok(StatusCode::NO_CONTENT)
}
