//! The load agents the benchmark knows (`BENCH_AGENTS`), and how it talks to one: pacod-gw's
//! `pacod-gw-load-agent`, in the contract of `pacod_gw_load::contract`. The list's syntax is the
//! tester's (`TESTER_LOAD_AGENTS`), so that one list serves both.
//!
//! An agent's URL receives the agent token and the keys of every cell: agents come from the
//! operator's configuration, or from an admin of the benchmark (pacod-bench's `agents.rs`), never
//! from anyone else.

use std::fmt;
use std::time::Duration;

use pacod_gw_load::contract::{ErrorBody, Info, Prepare, Prepared, Report};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::model::Placement;

/// A shorter token is refused by the agents (`LOAD_AGENT_TOKEN`).
pub const MIN_TOKEN_LEN: usize = 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRef {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub placement: Placement,
}

/// A lower-case name of at most 32: letters, digits, `-`.
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// `local=http://gw-load-agent:8096,remote=http://10.0.0.8:8096`.
pub fn parse_agents(text: &str) -> Result<Vec<AgentRef>, String> {
    let mut out: Vec<AgentRef> = Vec::new();
    for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let Some((name, url)) = item.split_once('=') else {
            return Err(format!("BENCH_AGENTS: {item:?} is not name=URL"));
        };
        let (name, url) = (name.trim(), url.trim().trim_end_matches('/'));
        if !valid_name(name) {
            return Err(format!("BENCH_AGENTS: {name:?} is not a lower-case name of at most 32"));
        }
        let parsed = url::Url::parse(url).map_err(|_| format!("BENCH_AGENTS: the URL of {name} does not read"))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none_or(str::is_empty) {
            return Err(format!("BENCH_AGENTS: the URL of {name} is not http(s) with a host"));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(format!("BENCH_AGENTS: the URL of {name} may not hold a user name or password"));
        }
        if out.iter().any(|a| a.name == name) {
            return Err(format!("BENCH_AGENTS: {name} is named twice"));
        }
        out.push(AgentRef { name: name.to_owned(), url: url.to_owned(), placement: Placement::Unknown });
    }
    Ok(out)
}

/// `local=in_stack,eu-1=remote`: where each agent stands. Agents left out stay `unknown`.
pub fn apply_placement(text: &str, agents: &mut [AgentRef]) -> Result<(), String> {
    for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, value) =
            item.split_once('=').ok_or_else(|| format!("BENCH_AGENT_PLACEMENT: {item:?} is not name=placement"))?;
        let placement = Placement::parse(value)
            .ok_or_else(|| format!("BENCH_AGENT_PLACEMENT: {value:?} is not in_stack, same_host, remote or unknown"))?;
        let agent = agents
            .iter_mut()
            .find(|a| a.name == name.trim())
            .ok_or_else(|| format!("BENCH_AGENT_PLACEMENT: {name} is not in BENCH_AGENTS"))?;
        agent.placement = placement;
    }
    Ok(())
}

/// An agent did not answer, or refused: one sentence, without the token.
#[derive(Debug, Clone)]
pub struct AgentError {
    pub agent: String,
    pub message: String,
    pub status: Option<u16>,
    /// The agent's error code: `unauthorized`, `busy`, `invalid`, `not_found`, `failed`.
    pub code: String,
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "agent {}: {}", self.agent, self.message)
    }
}

impl std::error::Error for AgentError {}

#[derive(Clone)]
pub struct AgentClient {
    pub agent: AgentRef,
    token: String,
    http: reqwest::Client,
}

impl fmt::Debug for AgentClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentClient").field("agent", &self.agent).field("token", &"[redacted]").finish()
    }
}

impl AgentClient {
    pub fn new(agent: AgentRef, token: &str, http: reqwest::Client) -> AgentClient {
        AgentClient { agent, token: token.to_owned(), http }
    }

    pub fn name(&self) -> &str {
        &self.agent.name
    }

    fn error(&self, message: impl Into<String>, status: Option<u16>, code: &str) -> AgentError {
        AgentError { agent: self.agent.name.clone(), message: message.into(), status, code: code.into() }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        timeout: Duration,
        body: Option<&Prepare>,
    ) -> Result<T, AgentError> {
        let mut request =
            self.http.request(method, format!("{}{path}", self.agent.url)).bearer_auth(&self.token).timeout(timeout);
        if let Some(b) = body {
            request = request.json(b);
        }
        let response = request.send().await.map_err(|e| {
            let why = if e.is_timeout() { "timed out" } else { "the connection failed" };
            self.error(format!("does not answer at {} ({why})", self.agent.url), None, "")
        })?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|_| self.error("broke off its answer", Some(status), ""))?;
        if status >= 400 {
            let (code, message) = match serde_json::from_slice::<ErrorBody>(&bytes) {
                Ok(e) => (e.error.code, e.error.message),
                Err(_) => (String::new(), format!("answered {status}")),
            };
            let message = if status == 401 {
                "refused the benchmark's token (BENCH_AGENT_TOKEN and the agent's LOAD_AGENT_TOKEN differ)".into()
            } else {
                message
            };
            return Err(self.error(message, Some(status), &code));
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            self.error("answered in a form this benchmark does not read; are both of one version?", Some(status), "")
        })
    }

    pub async fn info(&self) -> Result<Info, AgentError> {
        self.call(reqwest::Method::GET, "/v1/info", Duration::from_secs(3), None).await
    }

    pub async fn prepare(&self, plan: &Prepare, timeout: Duration) -> Result<Prepared, AgentError> {
        self.call(reqwest::Method::POST, "/v1/runs", timeout, Some(plan)).await
    }

    pub async fn start(&self, run_id: &str) -> Result<(), AgentError> {
        let _: serde_json::Value =
            self.call(reqwest::Method::POST, &format!("/v1/runs/{run_id}/start"), Duration::from_secs(5), None).await?;
        Ok(())
    }

    pub async fn report(&self, run_id: &str, after: Option<u64>) -> Result<Report, AgentError> {
        let query = after.map(|t| format!("?after={t}")).unwrap_or_default();
        self.call(reqwest::Method::GET, &format!("/v1/runs/{run_id}/report{query}"), Duration::from_secs(5), None).await
    }

    pub async fn cancel(&self, run_id: &str) -> Result<(), AgentError> {
        let _: serde_json::Value = self
            .call(reqwest::Method::POST, &format!("/v1/runs/{run_id}/cancel"), Duration::from_secs(5), None)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tester_syntax_is_read() {
        let mut a = parse_agents("local=http://gw-load-agent:8096/, eu-1=http://10.0.0.21:8096").unwrap();
        assert_eq!(a[0].url, "http://gw-load-agent:8096");
        apply_placement("local=in_stack,eu-1=remote", &mut a).unwrap();
        assert_eq!((a[0].placement, a[1].placement), (Placement::InStack, Placement::Remote));
        assert!(parse_agents("Local=http://x:1").is_err());
        assert!(parse_agents("a=http://x:1,a=http://y:1").is_err());
        assert!(parse_agents("a=ftp://x").is_err());
        assert!(parse_agents("a=http://u:p@x:1").is_err());
        assert!(apply_placement("b=remote", &mut a).is_err());
        assert!(parse_agents("").unwrap().is_empty());
    }
}
