//! The run settings an admin changes in the UI, within what the environment allows: the limits
//! (`BENCH_MAX_CELLS`, `BENCH_MAX_DURATION`, `BENCH_AGENT_WAIT_S`, which become ceilings), the
//! mock of the baseline cells (`BENCH_BASELINE_MOCK_URL`), the agents a run uses when it names
//! none, and the gateway reports are published to. What is not set here is the environment's.
//!
//! A run keeps a copy of the settings it started with (`runs.settings`): a change while it waits
//! leaves its meaning alone.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Config, check_url};
use crate::store::Store;

/// What an admin set; each field none: the environment's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cells: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_wait_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_mock_url: Option<String>,
    /// The agents a run uses when it names none; none: every agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_agents: Option<Vec<String>>,
    /// A managed gateway whose token publishes the reports; none: `BENCH_PUBLISH_URL`, else the
    /// configuration's gateway, else the only gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_target_id: Option<String>,
}

/// What a run uses: the overrides over the environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effective {
    pub max_cells: u32,
    pub max_duration_s: u64,
    pub agent_wait_s: u64,
    pub baseline_mock_url: Option<String>,
    /// Empty: every agent.
    #[serde(default)]
    pub default_agents: Vec<String>,
    #[serde(default)]
    pub publish_target_id: Option<String>,
}

impl Effective {
    /// The environment's settings alone: the ceilings, and what a run older than the settings
    /// used.
    pub fn environment(config: &Config) -> Effective {
        Effective {
            max_cells: config.max_cells,
            max_duration_s: config.max_duration_s,
            agent_wait_s: config.agent_wait_s,
            baseline_mock_url: config.baseline_mock_url.clone(),
            default_agents: vec![],
            publish_target_id: None,
        }
    }

    /// A run's copy; the environment's for a run older than the settings.
    pub fn of_run(copy: Option<&Value>, config: &Config) -> Effective {
        copy.and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_else(|| Effective::environment(config))
    }
}

pub struct Settings {
    config: Arc<Config>,
    overrides: RwLock<Overrides>,
}

impl Settings {
    pub async fn load(config: Arc<Config>, store: &Store) -> anyhow::Result<Settings> {
        let overrides = match store.settings().await? {
            Some(v) => serde_json::from_value(v).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "the stored settings do not read; the environment's apply");
                Overrides::default()
            }),
            None => Overrides::default(),
        };
        Ok(Settings { config, overrides: RwLock::new(overrides) })
    }

    pub fn overrides(&self) -> Overrides {
        self.overrides.read().clone()
    }

    pub fn set(&self, overrides: Overrides) {
        *self.overrides.write() = overrides;
    }

    /// The settings a run started now gets. A limit set higher than the environment allows now
    /// (it was lowered since) is the environment's.
    pub fn effective(&self) -> Effective {
        let env = Effective::environment(&self.config);
        let o = self.overrides.read();
        Effective {
            max_cells: o.max_cells.map_or(env.max_cells, |v| v.min(env.max_cells)),
            max_duration_s: o.max_duration_s.map_or(env.max_duration_s, |v| v.min(env.max_duration_s)),
            agent_wait_s: o.agent_wait_s.map_or(env.agent_wait_s, |v| v.min(env.agent_wait_s)),
            baseline_mock_url: o.baseline_mock_url.clone().or(env.baseline_mock_url),
            default_agents: o.default_agents.clone().unwrap_or_default(),
            publish_target_id: o.publish_target_id.clone(),
        }
    }
}

/// The limits and the address of a change, against the environment's ceilings; the agents and
/// the gateway are checked by the API, which knows them. The change, cleaned: empty values are
/// none.
pub fn check(config: &Config, mut o: Overrides) -> Result<Overrides, String> {
    let env = Effective::environment(config);
    if let Some(v) = o.max_cells
        && !(1..=env.max_cells).contains(&v)
    {
        return Err(format!("max_cells is 1 to {} (BENCH_MAX_CELLS)", env.max_cells));
    }
    if let Some(v) = o.max_duration_s
        && !(60..=env.max_duration_s).contains(&v)
    {
        return Err(format!("max_duration_s is 60 to {} (BENCH_MAX_DURATION)", env.max_duration_s));
    }
    if let Some(v) = o.agent_wait_s
        && v > env.agent_wait_s
    {
        return Err(format!("agent_wait_s is at most {} (BENCH_AGENT_WAIT_S)", env.agent_wait_s));
    }
    o.baseline_mock_url =
        o.baseline_mock_url.map(|u| u.trim().trim_end_matches('/').to_owned()).filter(|u| !u.is_empty());
    if let Some(u) = &o.baseline_mock_url {
        check_url(u).map_err(|e| format!("baseline_mock_url: {e}"))?;
    }
    o.default_agents = o.default_agents.filter(|a| !a.is_empty());
    if o.default_agents.as_ref().is_some_and(|a| a.len() > 50) {
        return Err("default_agents names at most 50 agents".into());
    }
    o.publish_target_id = o.publish_target_id.filter(|t| !t.is_empty());
    Ok(o)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config() -> Config {
        let vars: HashMap<&str, &str> = [
            ("DATABASE_URL", "postgres://bench@db/bench"),
            ("BENCH_AUTH", "dev"),
            ("BENCH_DEV_TOKEN", "a-development-token-of-32-characters"),
            ("BENCH_MAX_CELLS", "100"),
            ("BENCH_BASELINE_MOCK_URL", "http://bench-load-mock:9000"),
        ]
        .into_iter()
        .collect();
        Config::from_env(&|k: &str| vars.get(k).map(|v| v.to_string())).unwrap()
    }

    #[test]
    fn the_environment_is_the_ceiling() {
        let c = config();
        assert!(check(&c, Overrides { max_cells: Some(100), ..Default::default() }).is_ok());
        let e = check(&c, Overrides { max_cells: Some(101), ..Default::default() }).unwrap_err();
        assert!(e.contains("BENCH_MAX_CELLS"), "{e}");
        assert!(check(&c, Overrides { max_duration_s: Some(10), ..Default::default() }).is_err());
        assert!(check(&c, Overrides { agent_wait_s: Some(301), ..Default::default() }).is_err());
        assert!(check(&c, Overrides { baseline_mock_url: Some("ftp://m".into()), ..Default::default() }).is_err());
    }

    #[test]
    fn what_is_not_set_is_the_environments_and_empty_is_not_set() {
        let c = config();
        let o = check(
            &c,
            Overrides {
                baseline_mock_url: Some(" ".into()),
                default_agents: Some(vec![]),
                publish_target_id: Some(String::new()),
                agent_wait_s: Some(30),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(o, Overrides { agent_wait_s: Some(30), ..Default::default() });
        let s = Settings { config: Arc::new(c), overrides: RwLock::new(o) };
        let e = s.effective();
        assert_eq!((e.max_cells, e.agent_wait_s), (100, 30));
        assert_eq!(e.baseline_mock_url.as_deref(), Some("http://bench-load-mock:9000"));
    }

    #[test]
    fn a_run_older_than_the_settings_reads_the_environment() {
        let c = config();
        assert_eq!(Effective::of_run(None, &c), Effective::environment(&c));
        let mut copy = serde_json::to_value(Effective::environment(&c)).unwrap();
        copy["agent_wait_s"] = 7.into();
        assert_eq!(Effective::of_run(Some(&copy), &c).agent_wait_s, 7);
    }
}
