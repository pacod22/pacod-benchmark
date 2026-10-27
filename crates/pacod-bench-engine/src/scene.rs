//! What a run sets up on a gateway it manages, and takes down after it.
//!
//! - **The benchmark's mocks.** Three providers and their models of its own
//!   (`bench-load-mock-*`), kept between runs: the gateway deletes only models that were never
//!   active. They are not the tester's `tester-load-mock-*`, so that a benchmark and a load test
//!   from the console never rewrite each other's URLs. Each cell's latency profile goes into the
//!   provider's URLs (`…/x-mock/delay=200,chunk_delay=20,chunks=20/v1`): managed mode forwards
//!   no `x-mock` header, but calls every path below the base URL, and the mock reads its options
//!   from there.
//! - **A scratch organisation per run** (`bench-<run>`), with one workspace per workspace
//!   variant the suite uses, each with an explicit response-cache binding, and the mock models
//!   granted.
//! - **A key per cell**, for the cell's mode and model only, expiring shortly after the cell
//!   should have ended. It is made before the cell and deleted right after it, first.
//!
//! Every write happens before a cell's load, never during it: a write makes every gateway
//! instance reload its configuration.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::gw::{GwAdmin, GwError, MadeKey};
use crate::model::{CellConfig, Family, WorkspaceVariant};

/// The names of the benchmark's own mock objects start with this.
pub const MOCK_PREFIX: &str = "bench-load-mock";
/// The credential the mock providers carry; the mock takes any.
pub const MOCK_SECRET: &str = "bench-load-mock-provider-secret";
/// How long a cell's key outlives the cell's planned end.
pub const KEY_GRACE: chrono::Duration = chrono::Duration::minutes(15);

struct MockDef {
    provider: &'static str,
    /// Below the mock's origin: the base URL (managed mode) and the proxy URL.
    base_suffix: &'static str,
    proxy_suffix: &'static str,
    model: &'static str,
    upstream: &'static str,
    preset: &'static str,
    /// The provider's fields besides its name and URLs.
    more: fn() -> Value,
}

fn def(family: Family) -> MockDef {
    match family {
        Family::Openai => MockDef {
            provider: "bench-load-mock-openai",
            base_suffix: "/v1",
            proxy_suffix: "",
            model: "bench-load-mock/gpt-mock",
            upstream: "gpt-mock",
            preset: "openai-chat",
            more: || json!({ "family": "openai" }),
        },
        Family::Anthropic => MockDef {
            provider: "bench-load-mock-anthropic",
            base_suffix: "",
            proxy_suffix: "",
            model: "bench-load-mock/claude-mock",
            upstream: "claude-mock",
            preset: "anthropic-budget",
            more: || json!({ "family": "anthropic" }),
        },
        Family::Gemini => MockDef {
            provider: "bench-load-mock-gemini",
            base_suffix: "/emulate/gemini/v1beta",
            proxy_suffix: "/emulate/gemini",
            model: "bench-load-mock/gemini-mock",
            upstream: "gemini-mock",
            preset: "gemini-3-flash",
            more: || json!({ "family": "gemini", "managed_api": "native" }),
        },
    }
}

/// The embedding model, at the OpenAI mock: name, upstream id, capability preset.
const EMBED: (&str, &str, &str) = ("bench-load-mock/embed-mock", "text-embedding-3-small", "embedding");

/// The upstream model id a cell's baseline calls the mock with.
pub fn upstream_of(family: Family, embeddings: bool) -> &'static str {
    if embeddings { EMBED.1 } else { def(family).upstream }
}

/// A mock model as the gateway knows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MockModel {
    pub family: Family,
    pub embeddings: bool,
    pub provider_id: String,
    pub provider: String,
    pub model_id: String,
    pub model: String,
    pub upstream: String,
}

/// The benchmark's mocks on one gateway, and the profile each provider's URLs carry now.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Mocks {
    pub models: Vec<MockModel>,
    pub profiles: BTreeMap<Family, String>,
    providers: BTreeMap<Family, String>,
}

impl Mocks {
    pub fn model(&self, family: Family, embeddings: bool) -> Option<&MockModel> {
        self.models.iter().find(|m| m.family == family && m.embeddings == embeddings)
    }
}

fn urls(mock_url: &str, family: Family, profile: &str) -> (String, String) {
    let d = def(family);
    let mock = mock_url.trim_end_matches('/');
    (format!("{mock}/x-mock/{profile}{}", d.base_suffix), format!("{mock}/x-mock/{profile}{}", d.proxy_suffix))
}

/// The mocks of the families a run needs (with or without the embedding model), each provider's
/// URLs carrying `profiles[family]`. `mock_url` is the mock as the gateway reaches it.
pub async fn ensure_mocks(
    gw: &GwAdmin,
    mock_url: &str,
    needs: &[(Family, bool)],
    profiles: &BTreeMap<Family, String>,
) -> Result<Mocks, GwError> {
    let providers: BTreeMap<String, Value> = gw
        .find("providers", MOCK_PREFIX)
        .await?
        .into_iter()
        .filter_map(|p| Some((p["name"].as_str()?.to_owned(), p)))
        .collect();
    let models: BTreeMap<String, Value> = gw
        .find("models", MOCK_PREFIX)
        .await?
        .into_iter()
        .filter_map(|m| Some((m["name"].as_str()?.to_owned(), m)))
        .collect();
    let mut out = Mocks::default();
    for &(family, embeddings) in needs {
        let d = def(family);
        let profile = profiles.get(&family).cloned().unwrap_or_else(|| crate::model::MockProfile::default().spec());
        let (base_url, proxy_url) = urls(mock_url, family, &profile);
        let provider_id = match (out.providers.get(&family), providers.get(d.provider)) {
            (Some(id), _) => id.clone(),
            (None, Some(existing)) => {
                let id = GwAdmin::id(existing)?;
                let mut doc = gw.provider(&id).await?;
                if doc["base_url"] != json!(base_url) || doc["proxy_base_url"] != json!(proxy_url) {
                    doc["base_url"] = json!(base_url);
                    doc["proxy_base_url"] = json!(proxy_url);
                    gw.put_provider(&id, &doc).await?;
                }
                id
            }
            (None, None) => {
                let mut body = (d.more)();
                body["name"] = json!(d.provider);
                body["base_url"] = json!(base_url);
                body["proxy_base_url"] = json!(proxy_url);
                let id = gw.create_provider(&body).await?;
                gw.add_credential(&id, MOCK_SECRET).await?;
                id
            }
        };
        out.providers.insert(family, provider_id.clone());
        out.profiles.insert(family, profile);
        let (name, upstream, preset) = if embeddings { EMBED } else { (d.model, d.upstream, d.preset) };
        let model_id = match models.get(name) {
            Some(existing) => {
                let id = GwAdmin::id(existing)?;
                if existing["state"].as_str() != Some("active") {
                    gw.activate_model(&id).await?;
                }
                id
            }
            None => {
                let mut body = json!({
                    "name": name,
                    "provider_id": provider_id,
                    "upstream_id": upstream,
                    "capability_preset": preset,
                    "free": true,
                });
                if embeddings {
                    body["kind"] = json!("embedding");
                }
                let id = gw.create_model(&body).await?;
                gw.activate_model(&id).await?;
                id
            }
        };
        out.models.push(MockModel {
            family,
            embeddings,
            provider_id,
            provider: d.provider.to_owned(),
            model_id,
            model: name.to_owned(),
            upstream: upstream.to_owned(),
        });
    }
    Ok(out)
}

/// Writes a latency profile into a mock provider's URLs; false when they carry it already.
pub async fn set_profile(
    gw: &GwAdmin,
    mocks: &mut Mocks,
    mock_url: &str,
    family: Family,
    profile: &str,
) -> Result<bool, GwError> {
    if mocks.profiles.get(&family).map(String::as_str) == Some(profile) {
        return Ok(false);
    }
    let id = mocks.providers.get(&family).cloned().ok_or_else(|| GwError::Refusal(format!("no {family} mock")))?;
    let (base_url, proxy_url) = urls(mock_url, family, profile);
    let mut doc = gw.provider(&id).await?;
    doc["base_url"] = json!(base_url);
    doc["proxy_base_url"] = json!(proxy_url);
    gw.put_provider(&id, &doc).await?;
    mocks.profiles.insert(family, profile.to_owned());
    Ok(true)
}

/// What a run made on the gateway: the scratch organisation, its workspaces (one per variant),
/// and the path of every object it made, for the teardown.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scene {
    pub org: String,
    pub workspaces: Vec<(WorkspaceVariant, String)>,
    pub ledger: Vec<String>,
}

impl Scene {
    pub fn workspace(&self, variant: &WorkspaceVariant) -> Option<&str> {
        self.workspaces.iter().find(|(v, _)| v == variant).map(|(_, id)| id.as_str())
    }

    pub fn orgs(&self) -> Vec<String> {
        if self.org.is_empty() { vec![] } else { vec![self.org.clone()] }
    }
}

fn workspace_body(index: usize, v: &WorkspaceVariant) -> Value {
    let mut settings = json!({ "audit": { "mode": v.audit } });
    if let Some(store) = v.responses_store {
        settings["responses"] = json!({ "store": store });
    }
    if let Some(policy) = v.on_spool_full {
        settings["on_spool_full"] = json!(policy);
    }
    json!({ "name": format!("w{index}"), "settings": settings })
}

/// The scratch organisation `name`, a workspace per variant with its cache binding, and the
/// models granted. The ledger grows as objects are made, so a failure half-way still knows what
/// to take down.
pub async fn scratch(
    gw: &GwAdmin,
    scene: &mut Scene,
    name: &str,
    variants: &[WorkspaceVariant],
    model_ids: &[String],
) -> Result<(), GwError> {
    let org = gw.create_org(name).await?;
    scene.org = org.clone();
    scene.ledger.push(format!("/orgs/{org}"));
    for (i, v) in variants.iter().enumerate() {
        let ws = gw.create_workspace(&org, &workspace_body(i, v)).await?;
        scene.ledger.push(format!("/orgs/{org}/workspaces/{ws}"));
        // Explicit even when off: the installation may bind the cache for every workspace.
        let binding = gw.bind_plugin(&org, &ws, &json!({ "plugin": "response_cache", "mode": v.cache })).await?;
        scene.ledger.push(format!("/orgs/{org}/workspaces/{ws}/plugin-bindings/{binding}"));
        scene.workspaces.push((*v, ws));
    }
    for model in model_ids {
        let grant = gw.grant(&org, model).await?;
        scene.ledger.push(format!("/orgs/{org}/grants/{grant}"));
    }
    Ok(())
}

/// The cell's key: its mode and model only, expiring `KEY_GRACE` after `ends`.
pub async fn cell_key(
    gw: &GwAdmin,
    scene: &mut Scene,
    ws: &str,
    cfg: &CellConfig,
    model_id: &str,
    name: &str,
    ends: DateTime<Utc>,
) -> Result<MadeKey, GwError> {
    let mut body = json!({
        "name": name,
        "allowed_modes": [cfg.request.mode.as_str()],
        "allowed_models": [model_id],
        "expires_at": (ends + KEY_GRACE).to_rfc3339(),
    });
    if let Some(i) = cfg.key.inspect {
        body["inspect"] = json!(i);
    }
    if let Some(p) = cfg.key.usage_policy {
        body["usage_policy"] = json!(p);
    }
    if let Some(c) = cfg.key.max_concurrency {
        body["max_concurrency"] = json!(c);
    }
    let org = scene.org.clone();
    let key = gw.create_key(&org, ws, &body).await?;
    scene.ledger.push(format!("/orgs/{org}/workspaces/{ws}/keys/{}", key.id));
    Ok(key)
}

/// Deletes one key the scene made, and forgets it.
pub async fn delete_key(gw: &GwAdmin, scene: &mut Scene, key_id: &str) -> Result<(), GwError> {
    let Some(pos) = scene.ledger.iter().position(|p| p.ends_with(&format!("/keys/{key_id}"))) else {
        return Ok(());
    };
    gw.delete(&scene.ledger[pos], &scene.orgs()).await?;
    scene.ledger.remove(pos);
    Ok(())
}

/// Deletes what a run made, keys first, the rest newest first; returns what could not be
/// deleted. `orgs` are the run's own scratch organisations: nothing outside them is deleted,
/// whatever the ledger says.
pub async fn teardown(gw: &GwAdmin, ledger: &[String], orgs: &[String]) -> Vec<String> {
    let keys = ledger.iter().filter(|p| p.contains("/keys/"));
    let rest = ledger.iter().rev().filter(|p| !p.contains("/keys/"));
    let mut leftovers = Vec::new();
    for path in keys.chain(rest) {
        if let Err(e) = gw.delete(path, orgs).await {
            tracing::warn!(path = %path, error = %e, "deleting a scratch object failed");
            leftovers.push(path.clone());
        }
    }
    leftovers
}
