//! What one cell of a benchmark runs, and what it can vary.
//!
//! A cell's configuration has five parts, and a suite's axes name their fields by path
//! (`request.mode`, `mock.delay_ms`, `workspace.cache`, `key.inspect`, `load.target`):
//!
//! - `request`: the call itself. Interface, managed or proxy mode, stream or whole answer,
//!   prompt and answer size, HTTP version, extra headers.
//! - `mock`: the mock provider behind the gateway and its latency, written into the provider's
//!   URLs (`…/x-mock/delay=200,chunk_delay=20,chunks=20/v1`).
//! - `workspace`: the scratch workspace's settings through the management API: the response
//!   cache's binding, audit mode, stored responses, what a full event spool does.
//! - `key`: the cell's key: whether proxy mode reads the body, the usage policy, a concurrency
//!   cap.
//! - `load`: how much load and for how long, as the agents take it.
//!
//! What only a deployment decides (analytics on or off, Valkey or memory, the scale layout, the
//! minimal gateway, the hardware) is no axis: each such setting is a target of its own, whose
//! fingerprint the run records.

use std::collections::BTreeMap;
use std::fmt;

use pacod_gw_load::contract::{HttpVersion, Mode as LoadMode, Stage};
use serde::{Deserialize, Serialize};

/// The API a call is made in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interface {
    /// OpenAI chat completions.
    Openai,
    /// Anthropic messages.
    Anthropic,
    /// Gemini's own API.
    Gemini,
    /// The gateway's native chat (managed mode only).
    Native,
    /// OpenAI embeddings (never streamed).
    Embeddings,
}

/// How the gateway serves a call: planned and possibly translated, or relayed unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GwMode {
    Managed,
    Proxy,
}

impl GwMode {
    pub fn as_str(self) -> &'static str {
        match self {
            GwMode::Managed => "managed",
            GwMode::Proxy => "proxy",
        }
    }
}

/// The API of the mock provider behind the gateway: the dialect the gateway speaks upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Openai,
    Anthropic,
    Gemini,
}

impl Family {
    pub const ALL: [Family; 3] = [Family::Openai, Family::Anthropic, Family::Gemini];

    pub fn as_str(self) -> &'static str {
        match self {
            Family::Openai => "openai",
            Family::Anthropic => "anthropic",
            Family::Gemini => "gemini",
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The response cache's binding in the scratch workspace (the `response_cache` plugin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    /// Every scratch workspace gets an explicit binding: an installation may switch the cache on
    /// for every workspace by default (`PGW_CACHE_DEFAULT_MODE`).
    #[default]
    Off,
    Shadow,
    Enforce,
}

impl CacheMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CacheMode::Off => "off",
            CacheMode::Shadow => "shadow",
            CacheMode::Enforce => "enforce",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditMode {
    Off,
    On,
    /// Only calls that ask for it: a load test's calls are not recorded in full, as in the
    /// tester's load tests.
    #[default]
    OnRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Inspect {
    /// Proxy mode buffers and scans the body.
    Full,
    /// Blind relay: proxy mode streams the body without reading it.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsagePolicy {
    Require,
    Estimate,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpoolFull {
    KeepServing,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreResponses {
    Off,
    On,
}

fn default_true() -> bool {
    true
}

fn default_prompt_words() -> u32 {
    60
}

fn default_max_tokens() -> u32 {
    64
}

/// The one call every call of a cell makes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestShape {
    #[serde(default = "default_interface")]
    pub interface: Interface,
    #[serde(default = "default_mode")]
    pub mode: GwMode,
    /// Streamed answers by default; embeddings are never streamed.
    #[serde(default = "default_true")]
    pub stream: bool,
    /// The prompt is the word "word" this many times: the mock answers with about as many
    /// stream chunks' worth of text.
    #[serde(default = "default_prompt_words")]
    pub prompt_words: u32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub http: HttpVersion,
    /// Headers every call carries besides the ones the request needs (`x-gw-template`, …).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

fn default_interface() -> Interface {
    Interface::Openai
}

fn default_mode() -> GwMode {
    GwMode::Managed
}

impl Default for RequestShape {
    fn default() -> Self {
        RequestShape {
            interface: Interface::Openai,
            mode: GwMode::Managed,
            stream: true,
            prompt_words: default_prompt_words(),
            max_tokens: default_max_tokens(),
            http: HttpVersion::Http1,
            headers: BTreeMap::new(),
        }
    }
}

fn default_delay() -> u32 {
    200
}

fn default_chunk_delay() -> u32 {
    20
}

fn default_chunks() -> u32 {
    20
}

/// The mock provider behind the gateway, and the latency it answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MockProfile {
    #[serde(default = "default_family")]
    pub family: Family,
    /// Before the response head.
    #[serde(default = "default_delay")]
    pub delay_ms: u32,
    /// Between stream chunks (and a stream's other events).
    #[serde(default = "default_chunk_delay")]
    pub chunk_delay_ms: u32,
    #[serde(default = "default_chunks")]
    pub chunks: u32,
}

fn default_family() -> Family {
    Family::Openai
}

impl Default for MockProfile {
    fn default() -> Self {
        MockProfile { family: Family::Openai, delay_ms: 200, chunk_delay_ms: 20, chunks: 20 }
    }
}

impl MockProfile {
    /// The options as the mock reads them from a URL path (`/x-mock/<spec>/…`).
    pub fn spec(&self) -> String {
        format!("delay={},chunk_delay={},chunks={}", self.delay_ms, self.chunk_delay_ms, self.chunks)
    }

    /// Why the mock cannot take these options, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        if self.delay_ms > 60_000 || self.chunk_delay_ms > 10_000 || !(1..=1000).contains(&self.chunks) {
            return Err("mock: delay_ms at most 60000, chunk_delay_ms at most 10000, chunks 1 to 1000".into());
        }
        Ok(())
    }

    /// How long the mock takes for one call at least, in milliseconds: a streamed call takes
    /// `delay + (chunks − 1) × chunk delay` (pacod-gw docs/testing.md §9.2), a whole answer the
    /// delay.
    pub fn floor_ms(&self, stream: bool) -> f64 {
        if stream {
            f64::from(self.delay_ms) + f64::from(self.chunks.saturating_sub(1)) * f64::from(self.chunk_delay_ms)
        } else {
            f64::from(self.delay_ms)
        }
    }
}

/// The scratch workspace's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceVariant {
    #[serde(default)]
    pub cache: CacheMode,
    #[serde(default)]
    pub audit: AuditMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responses_store: Option<StoreResponses>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_spool_full: Option<SpoolFull>,
}

impl WorkspaceVariant {
    pub fn is_default(&self) -> bool {
        *self == WorkspaceVariant::default()
    }
}

/// The cell's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyVariant {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspect: Option<Inspect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_policy: Option<UsagePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<u32>,
}

impl KeyVariant {
    pub fn is_default(&self) -> bool {
        *self == KeyVariant::default()
    }
}

fn default_timeout() -> u64 {
    120
}

/// How much load, and for how long.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadShape {
    #[serde(default = "default_load_mode")]
    pub mode: LoadMode,
    pub stages: Vec<Stage>,
    /// Rate mode: calls running at once above which an arrival is skipped (and counted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_in_flight: Option<u32>,
    /// How long one call may take.
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    /// Calls the cell's agents make at most, together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_calls: Option<u64>,
    /// Seconds at the start left out of the steady figures: the ramp, connections opening.
    #[serde(default)]
    pub warmup_s: u64,
}

fn default_load_mode() -> LoadMode {
    LoadMode::Concurrency
}

impl LoadShape {
    pub fn duration_s(&self) -> u64 {
        self.stages.iter().map(|s| s.duration_s).sum()
    }

    pub fn peak(&self) -> f64 {
        self.stages.iter().map(|s| s.target).fold(0.0, f64::max)
    }

    /// The same shape at another peak: every stage's target scaled, so that a ramp stays a ramp.
    /// What a `load.target` axis does.
    pub fn with_peak(&self, peak: f64) -> LoadShape {
        let old = self.peak();
        let mut out = self.clone();
        for s in &mut out.stages {
            s.target = if old > 0.0 { s.target * peak / old } else { peak };
        }
        out
    }
}

/// Everything one cell runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellConfig {
    #[serde(default)]
    pub request: RequestShape,
    #[serde(default)]
    pub mock: MockProfile,
    #[serde(default)]
    pub workspace: WorkspaceVariant,
    #[serde(default)]
    pub key: KeyVariant,
    pub load: LoadShape,
}

/// The kind of target a run calls, as far as the configuration is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// A pacod-gw whose management API the benchmark uses: it sets the scene up.
    Managed,
    /// A URL and a key: pacod-gw-lite, another installation, another product.
    External,
}

impl CellConfig {
    /// The dialect proxy mode relays: the interface's own.
    fn proxy_dialect(&self) -> Option<Family> {
        match self.request.interface {
            Interface::Openai | Interface::Embeddings => Some(Family::Openai),
            Interface::Anthropic => Some(Family::Anthropic),
            Interface::Gemini => Some(Family::Gemini),
            Interface::Native => None,
        }
    }

    /// Normalises what the configuration implies (embeddings are never streamed) and says why
    /// it cannot run, if it cannot.
    pub fn check(&mut self, target: TargetKind) -> Result<(), String> {
        let r = &mut self.request;
        if r.interface == Interface::Embeddings {
            r.stream = false;
        }
        if !(1..=20_000).contains(&r.prompt_words) {
            return Err("request.prompt_words is between 1 and 20000".into());
        }
        if !(1..=100_000).contains(&r.max_tokens) {
            return Err("request.max_tokens is between 1 and 100000".into());
        }
        if r.headers.len() > 20 {
            return Err("request.headers holds at most 20 headers".into());
        }
        for (name, value) in &r.headers {
            let lower = name.to_ascii_lowercase();
            if pacod_gw_load::agent::check::RESERVED_HEADERS.contains(&lower.as_str())
                || lower == "content-type"
                || lower == "x-gw-cache"
            {
                return Err(format!("request.headers may not set {name}"));
            }
            if value.contains(['\r', '\n', '\0']) {
                return Err(format!("the value of request.headers.{name} holds a line break"));
            }
        }
        if r.mode == GwMode::Proxy && r.interface == Interface::Native {
            return Err("the native interface exists in managed mode only".into());
        }
        self.mock.check()?;
        let m = &self.mock;
        if target == TargetKind::Managed {
            if self.request.interface == Interface::Embeddings && m.family != Family::Openai {
                return Err("embeddings come from the OpenAI mock".into());
            }
            if self.request.mode == GwMode::Proxy && self.proxy_dialect() != Some(m.family) {
                return Err("in proxy mode the interface is the mock provider's own API".into());
            }
            if self.workspace.cache != CacheMode::Off {
                if self.request.mode == GwMode::Proxy {
                    return Err("proxy mode is never cached".into());
                }
                if self.request.interface == Interface::Embeddings {
                    return Err("embeddings are never cached".into());
                }
            }
        }
        if self.key.max_concurrency == Some(0) {
            return Err("key.max_concurrency is at least 1".into());
        }
        let l = &self.load;
        if l.stages.is_empty() || l.stages.len() > 50 {
            return Err("load.stages holds 1 to 50 stages".into());
        }
        if l.stages.iter().any(|s| !s.target.is_finite() || s.target < 0.0) {
            return Err("a stage's target is a number of at least 0".into());
        }
        if l.duration_s() == 0 {
            return Err("the stages last no time at all".into());
        }
        if l.peak() <= 0.0 {
            return Err("no stage has a target above 0".into());
        }
        if l.mode == LoadMode::Rate && l.max_in_flight.is_none() {
            return Err("rate mode needs load.max_in_flight".into());
        }
        if !(1..=600).contains(&l.timeout_s) {
            return Err("load.timeout_s is between 1 and 600".into());
        }
        if l.warmup_s >= l.duration_s() {
            return Err("load.warmup_s leaves no steady seconds".into());
        }
        if l.max_calls == Some(0) {
            return Err("load.max_calls is at least 1".into());
        }
        Ok(())
    }

    /// The mock family this cell's model comes from.
    pub fn family(&self) -> Family {
        if self.request.interface == Interface::Embeddings { Family::Openai } else { self.mock.family }
    }
}

/// Where a run's agents stand, as the operator declares it (`BENCH_AGENT_PLACEMENT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// In the gateway's stack: it shares the gateway's machine and Docker's network.
    InStack,
    /// On the gateway's machine, outside its stack.
    SameHost,
    /// On another machine.
    Remote,
    #[default]
    Unknown,
}

impl Placement {
    pub fn parse(text: &str) -> Option<Placement> {
        match text.trim() {
            "in_stack" => Some(Placement::InStack),
            "same_host" => Some(Placement::SameHost),
            "remote" => Some(Placement::Remote),
            "unknown" => Some(Placement::Unknown),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> CellConfig {
        CellConfig {
            request: RequestShape::default(),
            mock: MockProfile::default(),
            workspace: WorkspaceVariant::default(),
            key: KeyVariant::default(),
            load: LoadShape {
                mode: LoadMode::Concurrency,
                stages: vec![Stage { duration_s: 10, target: 5.0 }],
                max_in_flight: None,
                timeout_s: 60,
                max_calls: None,
                warmup_s: 0,
            },
        }
    }

    #[test]
    fn proxy_mode_needs_the_mock_providers_own_api() {
        let mut c = cell();
        c.request.mode = GwMode::Proxy;
        c.mock.family = Family::Anthropic;
        assert!(c.check(TargetKind::Managed).unwrap_err().contains("proxy mode"));
        c.request.interface = Interface::Anthropic;
        assert!(c.check(TargetKind::Managed).is_ok());
        // An external target's mock is not the benchmark's to know.
        c.request.interface = Interface::Openai;
        assert!(c.check(TargetKind::External).is_ok());
    }

    #[test]
    fn the_cache_is_refused_where_the_gateway_never_caches() {
        let mut c = cell();
        c.workspace.cache = CacheMode::Enforce;
        assert!(c.check(TargetKind::Managed).is_ok());
        c.request.mode = GwMode::Proxy;
        assert_eq!(c.check(TargetKind::Managed).unwrap_err(), "proxy mode is never cached");
        c.request.mode = GwMode::Managed;
        c.request.interface = Interface::Embeddings;
        assert_eq!(c.check(TargetKind::Managed).unwrap_err(), "embeddings are never cached");
    }

    #[test]
    fn embeddings_are_never_streamed() {
        let mut c = cell();
        c.request.interface = Interface::Embeddings;
        c.check(TargetKind::Managed).unwrap();
        assert!(!c.request.stream);
    }

    #[test]
    fn reserved_headers_are_refused() {
        let mut c = cell();
        c.request.headers.insert("Authorization".into(), "x".into());
        assert!(c.check(TargetKind::Managed).is_err());
        let mut c = cell();
        c.request.headers.insert("x-gw-cache".into(), "on".into());
        assert!(c.check(TargetKind::Managed).is_err(), "the cache header follows workspace.cache");
    }

    #[test]
    fn a_new_peak_keeps_the_shape_of_the_stages() {
        let mut c = cell();
        c.load.stages = vec![Stage { duration_s: 5, target: 10.0 }, Stage { duration_s: 5, target: 20.0 }];
        let l = c.load.with_peak(100.0);
        assert_eq!(l.stages[0].target, 50.0);
        assert_eq!(l.stages[1].target, 100.0);
    }

    #[test]
    fn the_mock_floor_follows_the_documented_formula() {
        let m = MockProfile { family: Family::Openai, delay_ms: 200, chunk_delay_ms: 20, chunks: 20 };
        assert_eq!(m.floor_ms(true), 580.0);
        assert_eq!(m.floor_ms(false), 200.0);
        assert_eq!(m.spec(), "delay=200,chunk_delay=20,chunks=20");
    }
}
