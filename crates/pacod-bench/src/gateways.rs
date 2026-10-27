//! The gateways the benchmark manages, by target id: those of the configuration
//! (`BENCH_GW_ADMIN_URL`, `BENCH_GW_TARGETS`, ids `cfg-<name>`) and those added through the UI,
//! whose admin tokens are sealed in the database (`secrets.rs`).
//!
//! One registry, shared by the API and the run queue, so that a gateway added, changed or removed
//! through the UI is the one the next run uses. A gateway whose token cannot be opened (its key
//! is not configured any more) stays listed with the reason, and takes no run.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::bail;
use pacod_bench_engine::gw::GwAdmin;
use parking_lot::RwLock;
use serde_json::{Value, json};

use crate::config::Config;
use crate::secrets::{self, Keys, Sealed};
use crate::store::{Store, TargetRow};

/// A gateway ready for a run.
pub struct Gateway {
    pub name: String,
    pub gw: Arc<GwAdmin>,
    pub admin_url: String,
    /// The mock as this gateway reaches it.
    pub mock_url: String,
    /// The data plane as the agents reach it; none: each agent's own target.
    pub data_plane: Option<String>,
    /// Its console, for links to published reports; none: `BENCH_GW_CONSOLE_URL`.
    pub console_url: Option<String>,
    /// From the configuration (`config`) or the UI (`api`).
    pub source: &'static str,
}

enum Entry {
    Ready(Arc<Gateway>),
    Unusable { name: String, reason: String },
}

#[derive(Default)]
pub struct Gateways {
    map: RwLock<BTreeMap<String, Entry>>,
}

fn text(v: &Value, field: &str) -> Option<String> {
    v[field].as_str().map(str::to_owned).filter(|s| !s.is_empty())
}

impl Gateways {
    /// The configuration's gateways, written as targets, then those added through the UI with
    /// their tokens opened. Before the run queue starts: taking down what an interrupted run left
    /// needs its gateway.
    pub async fn load(config: &Config, store: &Store, http: &reqwest::Client) -> anyhow::Result<Gateways> {
        let gateways = Gateways::default();
        for g in &config.gateways {
            if let Some(other) = store.target_by_name(&g.name).await?
                && other.source == "api"
            {
                bail!(
                    "the gateway {} of the configuration has the name of a target added through the UI ({}); rename one of them",
                    g.name,
                    other.id
                );
            }
            let definition = json!({ "admin_url": g.admin_url, "mock_url": g.mock_url, "data_plane": g.data_plane });
            store.upsert_config_target(&g.name, &definition, &g.declared).await?;
            gateways.map.write().insert(
                format!("cfg-{}", g.name),
                Entry::Ready(Arc::new(Gateway {
                    name: g.name.clone(),
                    gw: Arc::new(GwAdmin::new(&g.admin_url, &g.token, http.clone())),
                    admin_url: g.admin_url.clone(),
                    mock_url: g.mock_url.clone(),
                    data_plane: g.data_plane.clone(),
                    console_url: None,
                    source: "config",
                })),
            );
        }
        store.forget_config_targets(&config.gateways.iter().map(|g| g.name.clone()).collect::<Vec<_>>()).await?;
        for row in store.targets().await? {
            if row.kind == "managed" && row.source == "api" {
                gateways.reload(&row, store, config.keys.as_ref(), http).await?;
            }
        }
        Ok(gateways)
    }

    /// A gateway added through the UI, read again after it was written: its token opened under
    /// the keys.
    pub async fn reload(
        &self,
        row: &TargetRow,
        store: &Store,
        keys: Option<&Keys>,
        http: &reqwest::Client,
    ) -> anyhow::Result<()> {
        let admin_url = text(&row.definition, "admin_url").unwrap_or_default();
        let sealed = store.sealed_token(&row.id).await?;
        let opened = match (keys, sealed) {
            (_, None) => Err("no token is stored with this gateway; set one".to_owned()),
            (None, Some(_)) => Err("BENCH_KEK is not set: the stored token cannot be opened".to_owned()),
            (Some(keys), Some(sealed)) => serde_json::from_value::<Sealed>(sealed)
                .map_err(|_| "the stored token does not read".to_owned())
                .and_then(|s| {
                    secrets::open(keys.all(), &secrets::token_context(&row.id, &admin_url), &s)
                        .map_err(|e| e.to_string())
                })
                .and_then(|t| String::from_utf8(t.to_vec()).map_err(|_| "the stored token does not read".to_owned())),
        };
        let entry = match opened {
            Ok(token) => Entry::Ready(Arc::new(Gateway {
                name: row.name.clone(),
                gw: Arc::new(GwAdmin::new(&admin_url, &token, http.clone())),
                admin_url,
                mock_url: text(&row.definition, "mock_url").unwrap_or_default(),
                data_plane: text(&row.definition, "data_plane"),
                console_url: text(&row.definition, "console_url"),
                source: "api",
            })),
            Err(reason) => {
                tracing::warn!(target_id = %row.id, name = %row.name, %reason, "a gateway added through the UI cannot be used");
                Entry::Unusable { name: row.name.clone(), reason }
            }
        };
        self.map.write().insert(row.id.clone(), entry);
        Ok(())
    }

    pub fn remove(&self, id: &str) {
        self.map.write().remove(id);
    }

    /// The gateway of this target, or why it takes no run.
    pub fn get(&self, id: &str) -> Result<Arc<Gateway>, String> {
        match self.map.read().get(id) {
            Some(Entry::Ready(g)) => Ok(g.clone()),
            Some(Entry::Unusable { reason, .. }) => Err(format!("the gateway cannot be used: {reason}")),
            None => Err("the gateway is no longer configured".into()),
        }
    }

    /// Why this target's gateway takes no run; none when it does or is no gateway.
    pub fn unusable(&self, id: &str) -> Option<String> {
        match self.map.read().get(id) {
            Some(Entry::Unusable { reason, .. }) => Some(reason.clone()),
            _ => None,
        }
    }

    /// The names of every managed gateway, usable or not.
    pub fn names(&self) -> Vec<String> {
        self.map
            .read()
            .values()
            .map(|e| match e {
                Entry::Ready(g) => g.name.clone(),
                Entry::Unusable { name, .. } => name.clone(),
            })
            .collect()
    }

    /// The one usable gateway, when there is exactly one: where reports go when no destination
    /// is configured.
    pub fn only(&self) -> Option<Arc<Gateway>> {
        let map = self.map.read();
        let mut ready = map.values().filter_map(|e| match e {
            Entry::Ready(g) => Some(g.clone()),
            Entry::Unusable { .. } => None,
        });
        match (ready.next(), ready.next()) {
            (Some(g), None) => Some(g),
            _ => None,
        }
    }
}
