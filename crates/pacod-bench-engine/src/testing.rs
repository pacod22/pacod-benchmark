//! For tests (feature `testing`): a fake of the gateway's management API that records every
//! call, and pacod-gw's own load agent started in process.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use pacod_gw_load::agent::config::{AgentConfig, Settings};
use pacod_gw_load::agent::{Agent, router, spawn_lease_keeper};
use pacod_gw_load::contract::Caps;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::net::TcpListener;

/// The token the agents of [`spawn_agent`] take.
pub const AGENT_TOKEN: &str = "a-test-token-of-at-least-32-characters";

/// Serves `app` on a free local port.
pub async fn serve(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a local port");
    let addr = listener.local_addr().expect("its address");
    tokio::spawn(async move { axum::serve(listener, app).await });
    addr
}

/// A load agent named `local` whose default target is `target`; its URL.
pub async fn spawn_agent(target: &str) -> String {
    let settings = Settings {
        listen: "127.0.0.1:0".parse().expect("an address"),
        name: "local".into(),
        target: target.parse().expect("a URL"),
        caps: Caps { max_concurrency: 100, max_rate: 100.0, max_duration_s: 60, max_body_bytes: 1 << 20 },
        ca_file: None,
    };
    let agent = Agent::new(AgentConfig::new(settings, AGENT_TOKEN).expect("agent settings"));
    spawn_lease_keeper(agent.clone());
    format!("http://{}", serve(router(agent)).await)
}

/// The management API as far as the benchmark uses it. Every call is recorded as
/// `(method, path below /api/v1, body)`; every write answers with the next configuration
/// version, and every version counts as applied at once.
///
/// Tokens: one that contains `refused` is refused (401) everywhere; one that contains `viewer`
/// is valid but no platform admin's (`/session` says so, every other call answers 403); any other
/// is a platform admin's.
#[derive(Default)]
pub struct FakeGw {
    pub calls: Mutex<Vec<(String, String, Value)>>,
    pub providers: Mutex<Vec<Value>>,
    pub models: Mutex<Vec<Value>>,
    /// What `/monitoring/live` says is in flight.
    pub in_flight: AtomicI64,
    /// Reports posted to `/benchmark-reports`, as their raw bytes.
    pub reports: Mutex<Vec<Vec<u8>>>,
    version: AtomicI64,
    ids: AtomicU64,
}

impl FakeGw {
    /// Serves a new fake; it and its origin.
    pub async fn start() -> (Arc<FakeGw>, String) {
        let fake = Arc::new(FakeGw::default());
        let addr = serve(Router::new().fallback(handle).with_state(fake.clone())).await;
        (fake, format!("http://{addr}"))
    }

    pub fn writes(&self) -> Vec<(String, String, Value)> {
        self.calls.lock().iter().filter(|(m, _, _)| m != "GET").cloned().collect()
    }
}

async fn handle(State(f): State<Arc<FakeGw>>, req: Request) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().trim_start_matches("/api/v1").to_owned();
    let bearer = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned();
    let bytes: Bytes = axum::body::to_bytes(req.into_body(), 4 << 20).await.unwrap_or_default();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    f.calls.lock().push((method.to_string(), path.clone(), body.clone()));
    let id = || format!("id{}", f.ids.fetch_add(1, Ordering::Relaxed));
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let denied = |status: StatusCode| {
        (status, axum::Json(json!({ "error": { "type": "forbidden", "message": "" } }))).into_response()
    };
    if bearer.contains("refused") {
        return denied(StatusCode::UNAUTHORIZED);
    }
    if method == Method::GET && segments.as_slice() == ["session"] {
        let admin = !bearer.contains("viewer");
        let who = json!({ "id": "tok_bench", "kind": "token", "display": "pacod-bench", "platform_admin": admin });
        return (StatusCode::OK, axum::Json(who)).into_response();
    }
    if bearer.contains("viewer") {
        return denied(StatusCode::FORBIDDEN);
    }
    let (status, answer) = match (&method, segments.as_slice()) {
        (&Method::GET, ["system", "status"]) => (
            StatusCode::OK,
            json!({
                "software_version": "0.1.0-test",
                "config_head": f.version.load(Ordering::Relaxed),
                "instances": [{ "role": "all", "software_version": "0.1.0-test", "hostname": "gw-1" }],
                "components": [{ "name": "analytics", "status": "not_configured", "detail": "" }],
            }),
        ),
        (&Method::GET, ["system", "config-versions", v]) => {
            (StatusCode::OK, json!({ "version": v.parse::<i64>().unwrap_or(0), "applied": 1, "of": 1 }))
        }
        (&Method::GET, ["response-cache"]) => (StatusCode::OK, json!({ "store": "memory" })),
        (&Method::GET, ["monitoring", "live"]) => {
            (StatusCode::OK, json!({ "in_flight": f.in_flight.load(Ordering::Relaxed), "events_dropped": 0 }))
        }
        (&Method::GET, ["providers"]) => (StatusCode::OK, json!({ "items": *f.providers.lock(), "next_cursor": null })),
        (&Method::GET, ["models"]) => (StatusCode::OK, json!({ "items": *f.models.lock(), "next_cursor": null })),
        (&Method::GET, ["providers", pid]) => match f.providers.lock().iter().find(|p| p["id"] == *pid) {
            Some(p) => (StatusCode::OK, p.clone()),
            None => (StatusCode::NOT_FOUND, json!({ "error": { "type": "not_found", "message": "no provider" } })),
        },
        (&Method::POST, ["providers"]) => {
            let mut p = body.clone();
            p["id"] = json!(id());
            f.providers.lock().push(p.clone());
            (StatusCode::CREATED, p)
        }
        (&Method::PUT, ["providers", pid]) => {
            let mut all = f.providers.lock();
            match all.iter_mut().find(|p| p["id"] == *pid) {
                Some(p) => {
                    *p = body.clone();
                    (StatusCode::OK, p.clone())
                }
                None => (StatusCode::NOT_FOUND, json!({ "error": { "type": "not_found", "message": "no provider" } })),
            }
        }
        (&Method::POST, ["models"]) => {
            let mut m = body.clone();
            m["id"] = json!(id());
            m["state"] = json!("draft");
            f.models.lock().push(m.clone());
            (StatusCode::CREATED, m)
        }
        (&Method::POST, ["orgs", _, "workspaces", _, "keys"]) => {
            (StatusCode::CREATED, json!({ "id": id(), "secret": "pgw_k_secret" }))
        }
        (&Method::POST, ["benchmark-reports"]) => {
            // As the gateway answers (crates/pacod-gw-core/src/mgmt/benchmarks.rs there).
            f.reports.lock().push(bytes.to_vec());
            (StatusCode::CREATED, json!({ "summary": { "id": format!("bmr_{}", id()) }, "created": true }))
        }
        (&Method::POST, _) => (StatusCode::CREATED, json!({ "id": id() })),
        (&Method::DELETE, _) => (StatusCode::NO_CONTENT, Value::Null),
        _ => (StatusCode::NOT_FOUND, json!({ "error": { "type": "not_found", "message": "no such route" } })),
    };
    let mut response =
        if answer.is_null() { status.into_response() } else { (status, axum::Json(answer)).into_response() };
    if method != Method::GET && segments.first() != Some(&"benchmark-reports") {
        let v = f.version.fetch_add(1, Ordering::Relaxed) + 1;
        response.headers_mut().insert("x-gw-config-version", v.into());
    }
    response
}
