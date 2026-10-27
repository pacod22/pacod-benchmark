//! The API the UI's server side calls (`/api/v1`, on `BENCH_LISTEN`). Every route but the
//! health checks needs a caller with a benchmark role; docs/architecture.md lists the routes and
//! the permission each needs.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pacod_bench_engine::gw::GwError;
use serde_json::json;

use crate::agents::Agents;
use crate::config::Config;
use crate::gateways::Gateways;
use crate::runs::Runs;
use crate::settings::Settings;
use crate::store::Store;

mod agents;
pub mod auth;
mod misc;
pub(crate) mod runs;
mod schedules;
mod settings;
mod suites;
mod targets;

pub use auth::{AuthMode, Principal};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Store,
    pub runs: Arc<Runs>,
    pub auth: Arc<AuthMode>,
    pub http: reqwest::Client,
    /// The gateways the benchmark manages, by target id, shared with the run queue.
    pub gateways: Arc<Gateways>,
    /// The load agents, shared with the run queue.
    pub agents: Arc<Agents>,
    pub settings: Arc<Settings>,
}

/// An answer that is not a success: `{"error": {"type", "message"}}`, as pacod-gw answers.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub kind: &'static str,
    pub message: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, kind: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { status, kind, message: message.into() }
    }

    pub fn invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", message)
    }

    pub fn not_found(what: &str) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("no such {what}"))
    }

    pub fn conflict(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn unavailable(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
    }

    pub fn bad_gateway(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": { "type": self.kind, "message": self.message } }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = %e, "a database call failed");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "the benchmark's database failed")
    }
}

impl From<GwError> for ApiError {
    fn from(e: GwError) -> Self {
        ApiError::bad_gateway(e.to_string())
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .merge(misc::routes())
        .merge(agents::routes())
        .merge(settings::routes())
        .merge(targets::routes())
        .merge(suites::routes())
        .merge(runs::routes())
        .merge(schedules::routes());
    Router::new().nest("/api/v1", api).with_state(state)
}
