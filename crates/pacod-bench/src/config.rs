//! `pacod-bench serve`'s settings, from the environment (`BENCH_*`). docs/runbook.md lists them.
//!
//! Secrets come from a variable or from the file a `_FILE` variable names (a mounted secret);
//! both set is refused, so that it is clear which one is used. They are never command-line
//! arguments, where `ps` would show them.

use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context, bail, ensure};
use pacod_bench_engine::agents::{AgentRef, MIN_TOKEN_LEN, apply_placement, parse_agents};
use serde::Deserialize;
use serde_json::Value;

/// A shorter development token is refused.
const MIN_DEV_TOKEN: usize = 32;

pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn set(env: Env, name: &str) -> Option<String> {
    env(name).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// `NAME`, or the content of the file `NAME_FILE` names.
pub fn secret(env: Env, name: &str) -> anyhow::Result<Option<String>> {
    let file = format!("{name}_FILE");
    match (set(env, name), set(env, &file)) {
        (Some(_), Some(_)) => bail!("{name} and {file} are both set; set one of them"),
        (Some(v), None) => Ok(Some(v)),
        (None, Some(path)) => {
            let v = std::fs::read_to_string(&path).with_context(|| format!("{file}: reading {path}"))?;
            let v = v.trim().to_owned();
            ensure!(!v.is_empty(), "{file}: {path} is empty");
            Ok(Some(v))
        }
        (None, None) => Ok(None),
    }
}

fn url(env: Env, name: &str) -> anyhow::Result<Option<String>> {
    let Some(v) = set(env, name) else { return Ok(None) };
    check_url(&v).with_context(|| name.to_owned())?;
    Ok(Some(v.trim_end_matches('/').to_owned()))
}

/// `host:port`, the host an address or a name.
fn check_listen(text: &str) -> anyhow::Result<()> {
    if text.parse::<SocketAddr>().is_ok() {
        return Ok(());
    }
    let (host, port) = text.rsplit_once(':').with_context(|| format!("{text:?} is not host:port"))?;
    ensure!(!host.is_empty() && port.parse::<u16>().is_ok(), "{text:?} is not host:port");
    Ok(())
}

/// http(s), a host, no user name or password (they would end up in logs), no query or fragment.
pub fn check_url(text: &str) -> anyhow::Result<()> {
    let u = url::Url::parse(text).context("not a URL")?;
    ensure!(
        matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty()),
        "{text} is not http(s) with a host"
    );
    ensure!(u.username().is_empty() && u.password().is_none(), "the URL may not hold a user name or password");
    ensure!(u.query().is_none() && u.fragment().is_none(), "the URL may not hold a query or a fragment");
    Ok(())
}

fn number<T: std::str::FromStr>(env: Env, name: &str, default: T) -> anyhow::Result<T> {
    match set(env, name) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| anyhow::anyhow!("{name}: {v:?} is not a number")),
    }
}

/// pacod-id, for the people who sign in to the UI and the service client the API pushes its
/// manifest with.
#[derive(Clone)]
pub struct IdSettings {
    /// The public issuers whose tokens the API takes, exactly as their `iss` claims carry them.
    pub issuers: Vec<String>,
    /// pacod-id's internal listener: JWKS, the feed, the catalogue.
    pub internal_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub audience: String,
    /// Older than this, the copy of the bindings is not trusted and calls answer 503.
    pub max_staleness_s: u64,
}

/// How callers of the API are checked.
#[derive(Clone)]
pub enum Auth {
    /// Tokens of pacod-id, roles from its feed: every installation.
    Oidc(IdSettings),
    /// One static token that holds every benchmark role: development only, without pacod-id.
    Dev { token: String },
}

/// A gateway the benchmark manages: it sets each run's scene up through the management API.
#[derive(Clone)]
pub struct ManagedGw {
    pub name: String,
    pub admin_url: String,
    pub token: String,
    /// The mock as this gateway reaches it.
    pub mock_url: String,
    /// The data plane as the agents reach it; none: each agent's own target.
    pub data_plane: Option<String>,
    /// What the operator says about the deployment: layout, arch, CPUs, notes.
    pub declared: Value,
}

/// Where reports are published: the default gateway's management API, unless set otherwise.
#[derive(Clone)]
pub struct Publish {
    pub url: String,
    pub token: String,
}

#[derive(Clone)]
pub struct Config {
    /// `host:port`: a host name binds to its address, so that the API listens on an alias that
    /// exists on pacod-admin-net alone (`bench-api-internal:8190`), never on 0.0.0.0 there.
    pub listen: String,
    pub metrics_listen: Option<String>,
    pub database_url: String,
    pub auth: Auth,
    pub gateways: Vec<ManagedGw>,
    pub agents: Vec<AgentRef>,
    pub agent_token: Option<String>,
    /// The mock as the agents reach it, for baseline cells.
    pub baseline_mock_url: Option<String>,
    pub max_cells: u32,
    pub max_duration_s: u64,
    pub agent_wait_s: u64,
    pub publish: Option<Publish>,
    /// The gateway console's origin, for links to a published report.
    pub console_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetsFile {
    #[serde(default)]
    gateway: Vec<GatewayEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GatewayEntry {
    name: String,
    admin_url: String,
    token_file: String,
    mock_url: String,
    #[serde(default)]
    data_plane: Option<String>,
    #[serde(default)]
    declared: toml::Table,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && name.as_bytes()[0].is_ascii_lowercase()
}

/// `BENCH_GW_TARGETS`: further gateways, each with its own admin token file.
fn read_targets(path: &Path) -> anyhow::Result<Vec<ManagedGw>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("BENCH_GW_TARGETS: reading {}", path.display()))?;
    let file: TargetsFile = toml::from_str(&text).with_context(|| format!("BENCH_GW_TARGETS: {}", path.display()))?;
    let mut out = Vec::new();
    for g in file.gateway {
        ensure!(valid_name(&g.name), "BENCH_GW_TARGETS: {:?} is not a lower-case name of at most 40", g.name);
        check_url(&g.admin_url).with_context(|| format!("BENCH_GW_TARGETS: {}.admin_url", g.name))?;
        check_url(&g.mock_url).with_context(|| format!("BENCH_GW_TARGETS: {}.mock_url", g.name))?;
        if let Some(d) = &g.data_plane {
            check_url(d).with_context(|| format!("BENCH_GW_TARGETS: {}.data_plane", g.name))?;
        }
        let token = std::fs::read_to_string(&g.token_file)
            .with_context(|| format!("BENCH_GW_TARGETS: {}.token_file: reading {}", g.name, g.token_file))?
            .trim()
            .to_owned();
        ensure!(!token.is_empty(), "BENCH_GW_TARGETS: {}.token_file is empty", g.name);
        out.push(ManagedGw {
            name: g.name,
            admin_url: g.admin_url.trim_end_matches('/').to_owned(),
            token,
            mock_url: g.mock_url.trim_end_matches('/').to_owned(),
            data_plane: g.data_plane.map(|d| d.trim_end_matches('/').to_owned()),
            declared: serde_json::to_value(g.declared)?,
        });
    }
    Ok(out)
}

impl Config {
    pub fn from_env(env: Env) -> anyhow::Result<Config> {
        let listen = set(env, "BENCH_LISTEN").unwrap_or_else(|| "0.0.0.0:8190".into());
        check_listen(&listen).context("BENCH_LISTEN")?;
        let metrics_listen = match set(env, "BENCH_METRICS_LISTEN").as_deref() {
            Some("off") => None,
            Some(v) => {
                check_listen(v).context("BENCH_METRICS_LISTEN")?;
                Some(v.to_owned())
            }
            None => Some("0.0.0.0:9190".into()),
        };
        let database_url = secret(env, "DATABASE_URL")?.context("DATABASE_URL is not set")?;

        let auth = match set(env, "BENCH_AUTH").as_deref().unwrap_or("oidc") {
            "oidc" => {
                let issuers: Vec<String> = set(env, "BENCH_ID_ISSUER")
                    .context("BENCH_ID_ISSUER is not set (or BENCH_AUTH=dev for development)")?
                    .split(',')
                    .map(|s| s.trim().trim_end_matches('/').to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                Auth::Oidc(IdSettings {
                    issuers,
                    internal_url: url(env, "BENCH_ID_INTERNAL_URL")?.context("BENCH_ID_INTERNAL_URL is not set")?,
                    client_id: set(env, "BENCH_ID_CLIENT_ID").unwrap_or_else(|| "pacod-bench-service".into()),
                    client_secret: secret(env, "BENCH_ID_CLIENT_SECRET")?
                        .context("BENCH_ID_CLIENT_SECRET is not set")?,
                    audience: set(env, "BENCH_ID_AUDIENCE").unwrap_or_else(|| "pacod-bench".into()),
                    max_staleness_s: number(env, "BENCH_ID_MAX_STALENESS", 60)?,
                })
            }
            "dev" => {
                let token = secret(env, "BENCH_DEV_TOKEN")?.context("BENCH_AUTH=dev needs BENCH_DEV_TOKEN")?;
                ensure!(
                    token.chars().count() >= MIN_DEV_TOKEN,
                    "BENCH_DEV_TOKEN is at least {MIN_DEV_TOKEN} characters"
                );
                Auth::Dev { token }
            }
            other => bail!("BENCH_AUTH: {other:?} is neither oidc nor dev"),
        };

        let mut gateways = Vec::new();
        if let Some(admin_url) = url(env, "BENCH_GW_ADMIN_URL")? {
            let token =
                secret(env, "BENCH_GW_ADMIN_TOKEN")?.context("BENCH_GW_ADMIN_URL needs BENCH_GW_ADMIN_TOKEN")?;
            let mock_url = url(env, "BENCH_LOAD_MOCK_URL")?
                .context("BENCH_GW_ADMIN_URL needs BENCH_LOAD_MOCK_URL: the mock as the gateway reaches it")?;
            let declared = match set(env, "BENCH_GW_DECLARED") {
                Some(text) => serde_json::from_str(&text).context("BENCH_GW_DECLARED is a JSON object")?,
                None => serde_json::json!({}),
            };
            ensure!(declared.is_object(), "BENCH_GW_DECLARED is a JSON object");
            gateways.push(ManagedGw {
                name: set(env, "BENCH_GW_NAME").unwrap_or_else(|| "gateway".into()),
                admin_url,
                token,
                mock_url,
                data_plane: url(env, "BENCH_GW_DATA_PLANE")?,
                declared,
            });
        }
        if let Some(path) = set(env, "BENCH_GW_TARGETS") {
            gateways.extend(read_targets(Path::new(&path))?);
        }
        for (i, g) in gateways.iter().enumerate() {
            ensure!(valid_name(&g.name), "the gateway name {:?} is not a lower-case name of at most 40", g.name);
            ensure!(!gateways[..i].iter().any(|o| o.name == g.name), "the gateway {} is named twice", g.name);
        }

        let mut agents = parse_agents(&set(env, "BENCH_AGENTS").unwrap_or_default()).map_err(anyhow::Error::msg)?;
        apply_placement(&set(env, "BENCH_AGENT_PLACEMENT").unwrap_or_default(), &mut agents)
            .map_err(anyhow::Error::msg)?;
        let agent_token = secret(env, "BENCH_AGENT_TOKEN")?;
        if !agents.is_empty() {
            let t = agent_token.as_deref().unwrap_or("");
            ensure!(
                t.chars().count() >= MIN_TOKEN_LEN,
                "BENCH_AGENTS needs BENCH_AGENT_TOKEN (or BENCH_AGENT_TOKEN_FILE) of at least {MIN_TOKEN_LEN} characters, the agents' LOAD_AGENT_TOKEN"
            );
        }

        let publish = match (url(env, "BENCH_PUBLISH_URL")?, secret(env, "BENCH_PUBLISH_TOKEN")?) {
            (Some(url), Some(token)) => Some(Publish { url, token }),
            (Some(_), None) => bail!("BENCH_PUBLISH_URL needs BENCH_PUBLISH_TOKEN"),
            (None, Some(_)) => bail!("BENCH_PUBLISH_TOKEN needs BENCH_PUBLISH_URL"),
            (None, None) => gateways.first().map(|g| Publish { url: g.admin_url.clone(), token: g.token.clone() }),
        };

        Ok(Config {
            listen,
            metrics_listen,
            database_url,
            auth,
            gateways,
            agents,
            agent_token,
            baseline_mock_url: url(env, "BENCH_BASELINE_MOCK_URL")?,
            max_cells: number(env, "BENCH_MAX_CELLS", 256)?,
            max_duration_s: number(env, "BENCH_MAX_DURATION", 4 * 3600)?,
            agent_wait_s: number(env, "BENCH_AGENT_WAIT_S", 300)?,
            publish,
            console_url: url(env, "BENCH_GW_CONSOLE_URL")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| map.get(k).cloned()
    }

    const DEV: [(&str, &str); 3] = [
        ("DATABASE_URL", "postgres://bench@db/bench"),
        ("BENCH_AUTH", "dev"),
        ("BENCH_DEV_TOKEN", "a-development-token-of-32-characters"),
    ];

    #[test]
    fn a_development_setting_needs_nothing_but_a_database_and_a_token() {
        let c = Config::from_env(&env(&DEV)).unwrap();
        assert!(matches!(c.auth, Auth::Dev { .. }));
        assert!(c.gateways.is_empty() && c.agents.is_empty() && c.publish.is_none());
        assert_eq!(c.listen, "0.0.0.0:8190");
        assert!(check_listen("bench-api-internal:8190").is_ok() && check_listen("nope").is_err());
    }

    #[test]
    fn the_managed_gateway_needs_its_token_and_its_mock() {
        let mut pairs = DEV.to_vec();
        pairs.push(("BENCH_GW_ADMIN_URL", "http://pacod-gw-admin:8081/"));
        assert!(Config::from_env(&env(&pairs)).err().unwrap().to_string().contains("BENCH_GW_ADMIN_TOKEN"));
        pairs.push(("BENCH_GW_ADMIN_TOKEN", "pidt_x"));
        assert!(Config::from_env(&env(&pairs)).err().unwrap().to_string().contains("BENCH_LOAD_MOCK_URL"));
        pairs.push(("BENCH_LOAD_MOCK_URL", "http://gw-load-mock:9000"));
        let c = Config::from_env(&env(&pairs)).unwrap();
        assert_eq!(c.gateways[0].admin_url, "http://pacod-gw-admin:8081");
        // Reports go to the same gateway unless told otherwise.
        assert_eq!(c.publish.unwrap().url, "http://pacod-gw-admin:8081");
    }

    #[test]
    fn agents_need_a_long_enough_token_and_secrets_come_from_one_place() {
        let mut pairs = DEV.to_vec();
        pairs.push(("BENCH_AGENTS", "local=http://gw-load-agent:8096"));
        pairs.push(("BENCH_AGENT_PLACEMENT", "local=in_stack"));
        assert!(Config::from_env(&env(&pairs)).is_err());
        pairs.push(("BENCH_AGENT_TOKEN", "an-agent-token-that-is-at-least-32-long"));
        let c = Config::from_env(&env(&pairs)).unwrap();
        assert_eq!(c.agents[0].placement, pacod_bench_engine::model::Placement::InStack);
        pairs.push(("BENCH_AGENT_TOKEN_FILE", "/run/secrets/x"));
        assert!(Config::from_env(&env(&pairs)).err().unwrap().to_string().contains("both set"));
    }

    #[test]
    fn urls_with_credentials_are_refused() {
        assert!(check_url("http://u:p@gw:8081").is_err());
        assert!(check_url("http://gw:8081/?x=1").is_err());
        assert!(check_url("ftp://gw").is_err());
        assert!(check_url("https://gw.example.com/base").is_ok());
    }
}
