//! The run settings (`settings.rs`): what runs use now, what an admin set, and what the
//! environment allows.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState, Principal};
use crate::settings::{self, Effective, Overrides};

pub fn routes() -> Router<AppState> {
    Router::new().route("/settings", get(read).put(write))
}

fn view(st: &AppState) -> Value {
    json!({
        "effective": st.settings.effective(),
        "overrides": st.settings.overrides(),
        "environment": Effective::environment(&st.config),
    })
}

async fn read(State(st): State<AppState>, _p: Principal) -> Json<Value> {
    Json(view(&st))
}

/// Replaces what an admin set: a field left out is the environment's again.
async fn write(State(st): State<AppState>, p: Principal, Json(input): Json<Overrides>) -> ApiResult<Json<Value>> {
    p.require("bench.targets.manage")?;
    let o = settings::check(&st.config, input).map_err(ApiError::invalid)?;
    if let Some(names) = &o.default_agents
        && let Some(unknown) = names.iter().find(|n| st.agents.get(n).is_none())
    {
        return Err(ApiError::invalid(format!("default_agents: unknown agent {unknown}")));
    }
    if let Some(id) = &o.publish_target_id {
        let t = st.store.target(id).await?.ok_or_else(|| ApiError::invalid("publish_target_id: no such target"))?;
        if t.kind != "managed" {
            return Err(ApiError::invalid("publish_target_id: reports go to a gateway the benchmark manages"));
        }
    }
    st.store.save_settings(&json!(o), &p.display).await?;
    st.settings.set(o);
    tracing::info!(by = %p.display, "the run settings were changed");
    Ok(Json(view(&st)))
}
