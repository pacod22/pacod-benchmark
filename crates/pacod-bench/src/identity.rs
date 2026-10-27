//! pacod-id for the API: the client, the copy of the bindings the feed keeps current, and the
//! check of who calls. The benchmark's manifest (`authz/bench.yaml`) is pushed at start, and
//! again until pacod-id takes it. The same arrangement as pacod-gw's core
//! (`crates/pacod-gw-core/src/identity.rs` there).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use pacod_id_authz::{Catalog, Manifest};
use pacod_id_client::{Authenticator, AuthzCache, IdClient, IdConfig};
use tokio::sync::watch;

use crate::config::IdSettings;

/// The benchmark's manifest, the same text the API's checks use.
pub const MANIFEST: &str = include_str!("../../../authz/bench.yaml");

/// The rules engine with the benchmark's manifest: which role may do what.
pub static CATALOG: LazyLock<Catalog> =
    LazyLock::new(|| Catalog::new().with(Manifest::from_yaml(MANIFEST).expect("authz/bench.yaml is valid")));

pub struct Identity {
    pub settings: IdSettings,
    pub auth: Authenticator,
    pub cache: Arc<AuthzCache>,
}

impl Identity {
    /// Starts following pacod-id's feed and pushes the manifest, in the background: the API
    /// starts whether or not pacod-id answers, and calls wait for the copy.
    pub fn start(settings: IdSettings, stop: watch::Receiver<bool>) -> Arc<Identity> {
        let client = IdClient::new(IdConfig {
            issuers: settings.issuers.clone(),
            internal_url: settings.internal_url.clone(),
            client_id: settings.client_id.clone(),
            client_secret: settings.client_secret.clone(),
            product: "bench".into(),
            audience: settings.audience.clone(),
        });
        let cache = AuthzCache::spawn(client.clone(), stop.clone());
        let auth = Authenticator::new(client.clone(), cache.clone());
        tokio::spawn(push_manifest(client, stop));
        Arc::new(Identity { settings, auth, cache })
    }

    /// Whether the copy of the bindings is current enough to decide with.
    pub fn fresh(&self) -> bool {
        let max = Duration::from_secs(self.settings.max_staleness_s);
        self.cache.is_ready() && self.cache.feed_age().is_some_and(|age| age <= max)
    }
}

async fn push_manifest(client: IdClient, mut stop: watch::Receiver<bool>) {
    let source = Manifest::from_yaml(MANIFEST).expect("authz/bench.yaml is valid").source().clone();
    let mut backoff = Duration::from_secs(1);
    loop {
        match client.push_catalog(&source, &HashMap::new()).await {
            Ok(pushed) => {
                tracing::info!(
                    version = pushed.version,
                    changed = pushed.changed,
                    "the benchmark's manifest is at pacod-id"
                );
                return;
            }
            Err(e) => tracing::warn!(error = %e, "pushing the benchmark's manifest to pacod-id failed; trying again"),
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = stop.wait_for(|s| *s) => return,
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_benchmark_vectors_hold() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../authz");
        let text = std::fs::read_to_string(dir.join("vectors/bench.yaml")).unwrap();
        let load = |name: &str| {
            let text = std::fs::read_to_string(dir.join(format!("{name}.yaml"))).map_err(|e| e.to_string())?;
            Manifest::from_yaml(&text).map_err(|e| e.to_string())
        };
        let checks = pacod_id_authz::vectors::run(&text, load).unwrap_or_else(|f| panic!("{}", f.join("\n")));
        assert!(checks >= 20, "{checks}");
    }
}
