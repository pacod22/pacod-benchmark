//! The load agents the benchmark drives: those of the configuration (`BENCH_AGENTS`,
//! `BENCH_AGENT_PLACEMENT`), and those an admin added through the UI. One list, shared by the API
//! and the run queue, so that the next run uses what the list holds then. Every agent takes the
//! same token (`BENCH_AGENT_TOKEN`), which stays in the environment.

use anyhow::bail;
use pacod_bench_engine::agents::AgentRef;
use pacod_bench_engine::model::Placement;
use parking_lot::RwLock;

use crate::config::Config;
use crate::store::{AgentRow, Store};

#[derive(Clone)]
pub struct Agent {
    pub agent: AgentRef,
    /// From the configuration (`config`) or the UI (`api`).
    pub source: &'static str,
}

#[derive(Default)]
pub struct Agents {
    list: RwLock<Vec<Agent>>,
}

pub fn from_row(row: &AgentRow) -> AgentRef {
    AgentRef {
        name: row.name.clone(),
        url: row.url.clone(),
        placement: Placement::parse(&row.placement).unwrap_or_default(),
    }
}

impl Agents {
    /// The configuration's agents first, then those added through the UI. A name in both stops the
    /// start: which of the two is meant cannot be told.
    pub async fn load(config: &Config, store: &Store) -> anyhow::Result<Agents> {
        let mut list: Vec<Agent> = config.agents.iter().map(|a| Agent { agent: a.clone(), source: "config" }).collect();
        for row in store.agents().await? {
            if list.iter().any(|a| a.agent.name == row.name) {
                bail!("the agent {} is in BENCH_AGENTS and was added through the UI; rename one of them", row.name);
            }
            list.push(Agent { agent: from_row(&row), source: "api" });
        }
        Ok(Agents { list: RwLock::new(list) })
    }

    pub fn all(&self) -> Vec<Agent> {
        self.list.read().clone()
    }

    pub fn get(&self, name: &str) -> Option<Agent> {
        self.list.read().iter().find(|a| a.agent.name == name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.list.read().iter().map(|a| a.agent.name.clone()).collect()
    }

    pub fn first(&self) -> Option<AgentRef> {
        self.list.read().first().map(|a| a.agent.clone())
    }

    pub fn len(&self) -> usize {
        self.list.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// An agent added or changed through the UI.
    pub fn put(&self, agent: AgentRef) {
        let mut list = self.list.write();
        match list.iter_mut().find(|a| a.agent.name == agent.name) {
            Some(a) => a.agent = agent,
            None => list.push(Agent { agent, source: "api" }),
        }
    }

    pub fn remove(&self, name: &str) {
        self.list.write().retain(|a| a.agent.name != name || a.source == "config");
    }
}
