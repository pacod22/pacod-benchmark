//! The one request every call of a cell sends, by interface and mode: the shapes of the
//! tester's load tests (`tester/src/pgw_tester/load/requests.py`), with the response cache asked
//! for by name where the cell's workspace has it on.
//!
//! The prompt is the word "word" repeated, as `gw-load` sends it: the mock answers with about as
//! many stream chunks.

use std::collections::BTreeMap;

use pacod_gw_load::contract::{AuthHeader, RequestSpec};
use serde_json::{Value, json};

use crate::model::{CacheMode, CellConfig, Family, GwMode, Interface};

/// Where the SDKs put the key: Anthropic's in x-api-key, Google's in x-goog-api-key.
pub fn auth_header(interface: Interface) -> AuthHeader {
    match interface {
        Interface::Anthropic => AuthHeader::XApiKey,
        Interface::Gemini => AuthHeader::XGoogApiKey,
        _ => AuthHeader::Authorization,
    }
}

/// What the gateway calls the cell's model.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// Managed mode: the gateway's name for the model (`bench-load-mock/gpt-mock`).
    pub model: String,
    /// Proxy mode: the provider in `/proxy/<provider>/…`.
    pub provider: String,
    /// Proxy mode: the provider's id for the model.
    pub upstream: String,
}

/// The cell's request to a gateway. `paid` is false behind the benchmark's own mock: a call that
/// may reach a paid provider is made once (`x-gw-retries: 1`), because a retry would make two
/// provider calls of one.
pub fn build(cfg: &CellConfig, names: &Names, paid: bool) -> RequestSpec {
    let r = &cfg.request;
    let stream = r.stream && r.interface != Interface::Embeddings;
    let cached = cfg.workspace.cache != CacheMode::Off;
    let name = if r.mode == GwMode::Managed { &names.model } else { &names.upstream };
    let mut headers = base_headers(r.interface);
    if paid {
        headers.insert("x-gw-retries".into(), "1".into());
    }
    if cached {
        // The agent sends `x-gw-cache: off` unless the request asks for the cache by name.
        headers.insert("x-gw-cache".into(), "on".into());
    }
    for (k, v) in &r.headers {
        headers.insert(k.to_ascii_lowercase(), v.clone());
    }
    let body = body(cfg, r.interface, name, stream, cached);
    let below = path_below(r.interface, name, stream);
    let path = if r.mode == GwMode::Managed { below } else { format!("/proxy/{}{below}", names.provider) };
    RequestSpec { method: "POST".into(), path, headers, body: body.to_string(), stream }
}

/// The cell's request straight to the mock, without any gateway: in the mock's own API, which
/// is the interface proxy mode would relay. Its latency is the floor every gateway's figures are
/// measured against. `upstream` is the mock's model id.
pub fn baseline(cfg: &CellConfig, upstream: &str) -> (RequestSpec, AuthHeader) {
    let interface = match (cfg.request.interface, cfg.family()) {
        (Interface::Embeddings, _) => Interface::Embeddings,
        (_, Family::Openai) => Interface::Openai,
        (_, Family::Anthropic) => Interface::Anthropic,
        (_, Family::Gemini) => Interface::Gemini,
    };
    let stream = cfg.request.stream && interface != Interface::Embeddings;
    let mut headers = base_headers(interface);
    for (k, v) in &cfg.request.headers {
        headers.insert(k.to_ascii_lowercase(), v.clone());
    }
    let body = body(cfg, interface, upstream, stream, false);
    let below = path_below(interface, upstream, stream);
    // The mock serves Gemini's own API below /emulate/gemini, as the load mock provider's URL says.
    let path = if interface == Interface::Gemini { format!("/emulate/gemini{below}") } else { below };
    (RequestSpec { method: "POST".into(), path, headers, body: body.to_string(), stream }, auth_header(interface))
}

fn base_headers(interface: Interface) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::from([("content-type".to_owned(), "application/json".to_owned())]);
    if interface == Interface::Anthropic {
        headers.insert("anthropic-version".into(), "2023-06-01".into());
    }
    headers
}

fn body(cfg: &CellConfig, interface: Interface, name: &str, stream: bool, deterministic: bool) -> Value {
    let words = cfg.request.prompt_words as usize;
    let prompt = vec!["word"; words].join(" ");
    let n = cfg.request.max_tokens;
    let mut body = match interface {
        Interface::Gemini => {
            let mut generation = json!({ "maxOutputTokens": n });
            if deterministic {
                generation["temperature"] = json!(0);
            }
            return json!({
                "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
                "generationConfig": generation,
            });
        }
        Interface::Embeddings => return json!({ "model": name, "input": [prompt] }),
        Interface::Anthropic => json!({
            "model": name,
            "max_tokens": n,
            "messages": [{ "role": "user", "content": prompt }],
        }),
        Interface::Native => json!({
            "model": name,
            "messages": [{ "role": "user", "content": [{ "type": "text", "text": prompt }] }],
            "max_output_tokens": n,
        }),
        Interface::Openai => json!({
            "model": name,
            "messages": [{ "role": "user", "content": prompt }],
            "max_tokens": n,
        }),
    };
    // The response cache answers only calls that ask for temperature 0: a caller that does
    // expects the same answer to the same request.
    if deterministic {
        body["temperature"] = json!(0);
    }
    if stream {
        body["stream"] = json!(true);
        if interface == Interface::Openai {
            body["stream_options"] = json!({ "include_usage": true });
        }
    }
    body
}

fn path_below(interface: Interface, name: &str, stream: bool) -> String {
    match interface {
        Interface::Openai => "/v1/chat/completions".into(),
        Interface::Anthropic => "/v1/messages".into(),
        Interface::Gemini => {
            let call = if stream { "streamGenerateContent?alt=sse" } else { "generateContent" };
            format!("/v1beta/models/{name}:{call}")
        }
        Interface::Native => "/gw/v1/chat".into(),
        Interface::Embeddings => "/v1/embeddings".into(),
    }
}

#[cfg(test)]
mod tests {
    use pacod_gw_load::contract::{Mode, Stage};

    use super::*;
    use crate::model::{KeyVariant, LoadShape, MockProfile, RequestShape, WorkspaceVariant};

    fn cfg(interface: Interface, mode: GwMode, family: Family) -> CellConfig {
        CellConfig {
            request: RequestShape { interface, mode, ..RequestShape::default() },
            mock: MockProfile { family, ..MockProfile::default() },
            workspace: WorkspaceVariant::default(),
            key: KeyVariant::default(),
            load: LoadShape {
                mode: Mode::Concurrency,
                stages: vec![Stage { duration_s: 1, target: 1.0 }],
                max_in_flight: None,
                timeout_s: 10,
                max_calls: None,
                warmup_s: 0,
            },
        }
    }

    fn names() -> Names {
        Names {
            model: "bench-load-mock/gpt-mock".into(),
            provider: "bench-load-mock-openai".into(),
            upstream: "gpt-mock".into(),
        }
    }

    #[test]
    fn managed_and_proxy_calls_differ_in_path_and_model_name() {
        let m = build(&cfg(Interface::Openai, GwMode::Managed, Family::Openai), &names(), false);
        assert_eq!(m.path, "/v1/chat/completions");
        let body: Value = serde_json::from_str(&m.body).unwrap();
        assert_eq!(body["model"], "bench-load-mock/gpt-mock");
        assert_eq!(body["stream_options"]["include_usage"], true);
        let p = build(&cfg(Interface::Openai, GwMode::Proxy, Family::Openai), &names(), false);
        assert_eq!(p.path, "/proxy/bench-load-mock-openai/v1/chat/completions");
        assert_eq!(serde_json::from_str::<Value>(&p.body).unwrap()["model"], "gpt-mock");
        assert!(!p.headers.contains_key("x-gw-retries"), "the benchmark's mock costs nothing");
    }

    #[test]
    fn a_cached_cell_asks_for_the_cache_and_for_temperature_0() {
        let mut c = cfg(Interface::Gemini, GwMode::Managed, Family::Gemini);
        c.workspace.cache = CacheMode::Enforce;
        let s = build(&c, &names(), true);
        assert_eq!(s.headers["x-gw-cache"], "on");
        assert_eq!(s.headers["x-gw-retries"], "1");
        let body: Value = serde_json::from_str(&s.body).unwrap();
        assert_eq!(body["generationConfig"]["temperature"], 0);
        assert!(s.path.ends_with(":streamGenerateContent?alt=sse"));
    }

    #[test]
    fn the_baseline_speaks_the_mocks_own_api() {
        let (s, auth) = baseline(&cfg(Interface::Openai, GwMode::Managed, Family::Anthropic), "claude-mock");
        assert_eq!(s.path, "/v1/messages");
        assert_eq!(auth, AuthHeader::XApiKey);
        assert_eq!(s.headers["anthropic-version"], "2023-06-01");
        let (g, _) = baseline(&cfg(Interface::Gemini, GwMode::Proxy, Family::Gemini), "gemini-mock");
        assert_eq!(g.path, "/emulate/gemini/v1beta/models/gemini-mock:streamGenerateContent?alt=sse");
    }
}
