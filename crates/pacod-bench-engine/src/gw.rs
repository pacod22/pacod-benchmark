//! The gateway's management API (`/api/v1` on pacod-gw's admin listener), as the benchmark uses
//! it: exactly the calls a run makes and no others.
//!
//! The token is a platform admin's (`BENCH_GW_ADMIN_TOKEN`, or the one sealed with a gateway
//! added through the UI): a `pidt_` service-account token when the gateway signs in through
//! pacod-id, a `pgwa_` admin token otherwise. A platform admin
//! may do everything, so this client is the boundary: there is no generic pass-through, and
//! [`GwAdmin::delete`] takes only paths below the run's own scratch organisation or the
//! benchmark's own mock objects.
//!
//! Every answer that changed the configuration carries `x-gw-config-version`; the client keeps
//! the highest, and [`GwAdmin::settle`] waits until every gateway instance has applied it.

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, thiserror::Error)]
pub enum GwError {
    #[error("the gateway's management API cannot be reached at {0}")]
    Unreachable(String),
    #[error("the gateway answered {status} to {what}: {message}")]
    Refused { status: u16, what: String, message: String, code: String },
    #[error("the gateway's answer to {0} does not read")]
    Malformed(String),
    #[error("{0}")]
    Refusal(String),
}

impl GwError {
    pub fn status(&self) -> Option<u16> {
        match self {
            GwError::Refused { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// The management API's errors are `{"error": {"type", "message", "details"}}`.
fn refused(status: u16, what: String, body: &Value) -> GwError {
    let error = &body["error"];
    let mut message = error["message"].as_str().unwrap_or("").to_owned();
    if message.is_empty() && matches!(status, 401 | 403) {
        message = "the benchmark's admin token is refused".into();
    }
    GwError::Refused { status, what, message, code: error["type"].as_str().unwrap_or("").to_owned() }
}

/// `GET /system/config-versions/{v}`.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct VersionApplied {
    pub version: i64,
    pub applied: i64,
    pub of: i64,
}

/// A key the gateway made: its id, and its secret, which it shows once.
#[derive(Clone)]
pub struct MadeKey {
    pub id: String,
    pub secret: String,
}

impl fmt::Debug for MadeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MadeKey").field("id", &self.id).field("secret", &"[redacted]").finish()
    }
}

pub struct GwAdmin {
    /// `http://pacod-gw-admin:8081/api/v1`.
    base: String,
    token: String,
    http: reqwest::Client,
    version: AtomicI64,
}

impl fmt::Debug for GwAdmin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GwAdmin").field("base", &self.base).field("token", &"[redacted]").finish()
    }
}

/// Where [`GwAdmin::delete`] may delete: the scratch organisations and the benchmark's mocks.
pub const SCRATCH_ORG_PREFIX: &str = "bench-";

impl GwAdmin {
    /// `admin_url` is the listener's origin, without `/api/v1`.
    pub fn new(admin_url: &str, token: &str, http: reqwest::Client) -> GwAdmin {
        GwAdmin {
            base: format!("{}/api/v1", admin_url.trim_end_matches('/')),
            token: token.to_owned(),
            http,
            version: AtomicI64::new(0),
        }
    }

    /// The highest configuration version a write of this client answered with.
    pub fn last_version(&self) -> i64 {
        self.version.load(Ordering::Acquire)
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<(u16, Value), GwError> {
        let what = format!("{method} {path}");
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(30));
        if let Some(b) = body {
            request = request.json(b);
        }
        let response = request.send().await.map_err(|_| GwError::Unreachable(self.base.clone()))?;
        let status = response.status().as_u16();
        if let Some(v) = response
            .headers()
            .get("x-gw-config-version")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok())
        {
            self.version.fetch_max(v, Ordering::AcqRel);
        }
        let bytes = response.bytes().await.map_err(|_| GwError::Malformed(what.clone()))?;
        let value: Value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| GwError::Malformed(what.clone()))?
        };
        if status >= 400 {
            return Err(refused(status, what, &value));
        }
        Ok((status, value))
    }

    async fn get(&self, path: &str) -> Result<Value, GwError> {
        Ok(self.call(reqwest::Method::GET, path, None).await?.1)
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, GwError> {
        Ok(self.call(reqwest::Method::POST, path, Some(body)).await?.1)
    }

    fn id_of(value: &Value, what: &str) -> Result<String, GwError> {
        value["id"].as_str().map(str::to_owned).ok_or_else(|| GwError::Malformed(what.to_owned()))
    }

    /// An object's id, from a listing or an answer.
    pub fn id(value: &Value) -> Result<String, GwError> {
        Self::id_of(value, "an object without an id")
    }

    // --- what the run records about the gateway ----------------------------------------------

    /// `GET /session`: who the gateway takes the token for (`display`, `kind`), and whether it is
    /// a platform admin. Any valid token may ask; a token that is no platform admin's shows here,
    /// not as a bare 403 of the first call a run makes.
    pub async fn session(&self) -> Result<Value, GwError> {
        self.get("/session").await
    }

    /// `GET /system/status`: software version, instances, configuration head, components. A
    /// refusal here stops a run before it sets anything up.
    pub async fn status(&self) -> Result<Value, GwError> {
        self.get("/system/status").await
    }

    /// `GET /response-cache`: where the gateways keep answers, and the plugin's installation.
    pub async fn cache_view(&self) -> Result<Value, GwError> {
        self.get("/response-cache").await
    }

    /// `GET /monitoring/live`: calls in flight across the instances, refusals, the spool.
    pub async fn live(&self) -> Result<Value, GwError> {
        self.get("/monitoring/live").await
    }

    pub async fn applied(&self, version: i64) -> Result<VersionApplied, GwError> {
        let v = self.get(&format!("/system/config-versions/{version}")).await?;
        serde_json::from_value(v).map_err(|_| GwError::Malformed("GET /system/config-versions".into()))
    }

    /// Waits until every running instance has applied this client's last write; false when they
    /// had not within `timeout`. A key that an instance has not loaded yet answers 401, and an
    /// address with too many of those in a minute is blocked: the probe waits for this.
    pub async fn settle(&self, timeout: Duration) -> Result<bool, GwError> {
        let version = self.last_version();
        if version == 0 {
            return Ok(true);
        }
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let a = self.applied(version).await?;
            if a.applied >= a.of {
                return Ok(true);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    // --- the benchmark's mock providers ------------------------------------------------------

    /// Every item of `/providers` or `/models` whose name contains `search`, following the
    /// cursor.
    pub async fn find(&self, collection: &str, search: &str) -> Result<Vec<Value>, GwError> {
        if !matches!(collection, "providers" | "models") {
            return Err(GwError::Refusal(format!("the benchmark does not list {collection}")));
        }
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let mut path = format!("/{collection}?limit=200&q={search}");
            if let Some(c) = &cursor {
                path.push_str(&format!("&cursor={c}"));
            }
            let page = self.get(&path).await?;
            out.extend(page["items"].as_array().cloned().unwrap_or_default());
            cursor = page["next_cursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }

    fn own_mock(name: &str) -> Result<(), GwError> {
        if name.starts_with(crate::scene::MOCK_PREFIX) {
            Ok(())
        } else {
            Err(GwError::Refusal(format!("{name} is not one of the benchmark's mocks")))
        }
    }

    pub async fn create_provider(&self, body: &Value) -> Result<String, GwError> {
        Self::own_mock(body["name"].as_str().unwrap_or(""))?;
        Self::id_of(&self.post("/providers", body).await?, "POST /providers")
    }

    pub async fn add_credential(&self, provider_id: &str, secret: &str) -> Result<(), GwError> {
        self.post(&format!("/providers/{provider_id}/credentials"), &json!({ "label": "benchmark", "secret": secret }))
            .await?;
        Ok(())
    }

    pub async fn provider(&self, provider_id: &str) -> Result<Value, GwError> {
        self.get(&format!("/providers/{provider_id}")).await
    }

    /// Writes a provider document back (its URLs carry the mock's latency profile).
    pub async fn put_provider(&self, provider_id: &str, doc: &Value) -> Result<(), GwError> {
        Self::own_mock(doc["name"].as_str().unwrap_or(""))?;
        self.call(reqwest::Method::PUT, &format!("/providers/{provider_id}"), Some(doc)).await?;
        Ok(())
    }

    pub async fn create_model(&self, body: &Value) -> Result<String, GwError> {
        Self::own_mock(body["name"].as_str().unwrap_or(""))?;
        Self::id_of(&self.post("/models", body).await?, "POST /models")
    }

    pub async fn activate_model(&self, model_id: &str) -> Result<(), GwError> {
        self.post(&format!("/models/{model_id}/transition"), &json!({ "to": "active" })).await?;
        Ok(())
    }

    // --- the run's scratch organisation ------------------------------------------------------

    pub async fn create_org(&self, name: &str) -> Result<String, GwError> {
        if !name.starts_with(SCRATCH_ORG_PREFIX) {
            return Err(GwError::Refusal(format!("{name} is not a scratch organisation's name")));
        }
        Self::id_of(&self.post("/orgs", &json!({ "name": name })).await?, "POST /orgs")
    }

    pub async fn create_workspace(&self, org: &str, body: &Value) -> Result<String, GwError> {
        Self::id_of(&self.post(&format!("/orgs/{org}/workspaces"), body).await?, "POST /orgs/{org}/workspaces")
    }

    pub async fn bind_plugin(&self, org: &str, ws: &str, body: &Value) -> Result<String, GwError> {
        let path = format!("/orgs/{org}/workspaces/{ws}/plugin-bindings");
        Self::id_of(&self.post(&path, body).await?, "POST plugin-bindings")
    }

    pub async fn grant(&self, org: &str, model_id: &str) -> Result<String, GwError> {
        let made = self.post(&format!("/orgs/{org}/grants"), &json!({ "model_id": model_id })).await?;
        Self::id_of(&made, "POST /orgs/{org}/grants")
    }

    pub async fn create_key(&self, org: &str, ws: &str, body: &Value) -> Result<MadeKey, GwError> {
        let made = self.post(&format!("/orgs/{org}/workspaces/{ws}/keys"), body).await?;
        let id = Self::id_of(&made, "POST keys")?;
        let secret = made["secret"].as_str().ok_or_else(|| GwError::Malformed("POST keys".into()))?.to_owned();
        Ok(MadeKey { id, secret })
    }

    /// Deletes one object a run made, below one of `orgs` (the run's scratch organisations, by
    /// id). One that is gone already counts as deleted.
    pub async fn delete(&self, path: &str, orgs: &[String]) -> Result<(), GwError> {
        let owned = orgs.iter().any(|o| path == format!("/orgs/{o}") || path.starts_with(&format!("/orgs/{o}/")));
        if !owned || path.contains("..") {
            return Err(GwError::Refusal(format!("{path} is not below the run's scratch organisation")));
        }
        match self.call(reqwest::Method::DELETE, path, None).await {
            Ok(_) => Ok(()),
            Err(GwError::Refused { status: 404, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    // --- results -----------------------------------------------------------------------------

    /// `POST /benchmark-reports`: a report, as its raw bytes (the gateway keeps them as sent).
    pub async fn publish_report(&self, document: &[u8]) -> Result<Value, GwError> {
        let response = self
            .http
            .post(format!("{}/benchmark-reports", self.base))
            .bearer_auth(&self.token)
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(60))
            .body(document.to_vec())
            .send()
            .await
            .map_err(|_| GwError::Unreachable(self.base.clone()))?;
        let status = response.status().as_u16();
        let value: Value = response.json().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(refused(status, "POST /benchmark-reports".into(), &value));
        }
        Ok(value)
    }
}
