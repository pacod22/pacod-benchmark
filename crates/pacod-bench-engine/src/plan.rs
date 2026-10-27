//! A suite expanded into the cells a run makes.
//!
//! A suite is one base configuration and the axes it varies. Each axis names a field of the
//! configuration by its path (`request.mode`, `mock.delay_ms`, `workspace.cache`) and lists its
//! values; the run makes every combination. `load.target` is special: it scales every stage, so
//! that a ramp stays a ramp at another peak.
//!
//! Combinations that cannot run are left out with the reason (proxy mode with another dialect
//! than the mock's, the cache with proxy mode). The cells are ordered so that what costs a
//! configuration write (the workspace variant, the mock's profile) changes least often, and
//! each cell is repeated as often as the suite says. Each distinct mock profile and request gets
//! a baseline cell first: the same load straight to the mock, the floor every gateway's figures
//! are measured against.

use std::collections::{BTreeMap, BTreeSet};

use pacod_gw_load::contract::Mode as LoadMode;
use ring::digest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{CellConfig, KeyVariant, TargetKind, WorkspaceVariant};

/// At most this many cells (repetitions included) in a run, whatever a suite's cap says.
pub const HARD_MAX_CELLS: u32 = 256;
/// How long a cell's calls may take to finish after the last stage, at most.
pub const DRAIN_S: u64 = 30;
/// What a cell costs besides its load, in seconds: the key, the settle, the quiet check, the
/// probes, the teardown of the key.
pub const CELL_OVERHEAD_S: u64 = 8;

fn default_repetitions() -> u32 {
    1
}

fn default_cap() -> u32 {
    64
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Order {
    /// What costs a configuration write changes least often.
    #[default]
    Grouped,
    /// A fixed shuffle, so that a slow drift of the machine does not line up with one axis.
    Shuffled { seed: u64 },
}

/// A suite as the editor and the command line write it, in YAML or JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub base: CellConfig,
    /// Path → values. A `BTreeMap` keeps the axes in one order whatever the document's.
    #[serde(default)]
    pub axes: BTreeMap<String, Vec<Value>>,
    #[serde(default)]
    pub order: Order,
    #[serde(default = "default_repetitions")]
    pub repetitions: u32,
    #[serde(default = "default_cap")]
    pub cap: u32,
    /// A baseline cell straight to the mock for each mock profile and request.
    #[serde(default = "default_true")]
    pub baseline: bool,
}

impl Suite {
    /// A suite in YAML (of which JSON is a part).
    pub fn parse(text: &str) -> Result<Suite, String> {
        serde_saphyr::from_str(text).map_err(|e| format!("the suite does not read: {e}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellKind {
    /// Through the target.
    Target,
    /// Straight to the mock, without any gateway.
    Baseline,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedCell {
    pub index: u32,
    pub rep: u32,
    pub kind: CellKind,
    /// The value of every axis for this cell.
    pub axes: BTreeMap<String, Value>,
    pub config: CellConfig,
    /// Cells with the same key ran the same configuration, in any run: only they are compared
    /// across runs and targets.
    pub comparison_key: String,
    /// The baseline cell this cell is measured against.
    pub baseline_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pruned {
    pub axes: BTreeMap<String, Value>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub cells: u32,
    pub baseline_cells: u32,
    /// The whole run, in seconds.
    pub duration_s: u64,
    /// Calls of all cells, about: the mock's latency decides how many a closed loop makes.
    pub calls: u64,
    /// Configuration writes on the gateway (each makes every instance reload).
    pub config_writes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub cells: Vec<PlannedCell>,
    pub pruned: Vec<Pruned>,
    pub estimate: Estimate,
    /// What the plan changed or left out, besides pruned cells.
    pub notes: Vec<String>,
}

fn sha256_hex(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("a configuration serialises");
    hex::encode(digest::digest(&digest::SHA256, &bytes))
}

/// The comparison key: the configuration as a canonical JSON object. serde_json keeps the
/// struct's field order, and the maps inside are `BTreeMap`s, so equal configurations give equal
/// bytes.
pub fn comparison_key(cfg: &CellConfig) -> String {
    sha256_hex(cfg)
}

/// What a baseline cell depends on: the mock and its profile, the request as the mock sees it,
/// and the load. Not the mode, nor the gateway's settings.
pub fn baseline_key(cfg: &CellConfig) -> String {
    #[derive(Serialize)]
    struct B<'a> {
        family: crate::model::Family,
        embeddings: bool,
        mock: &'a crate::model::MockProfile,
        stream: bool,
        prompt_words: u32,
        max_tokens: u32,
        http: pacod_gw_load::contract::HttpVersion,
        headers: &'a BTreeMap<String, String>,
        load: &'a crate::model::LoadShape,
    }
    sha256_hex(&B {
        family: cfg.family(),
        embeddings: cfg.request.interface == crate::model::Interface::Embeddings,
        mock: &cfg.mock,
        stream: cfg.request.stream,
        prompt_words: cfg.request.prompt_words,
        max_tokens: cfg.request.max_tokens,
        http: cfg.request.http,
        headers: &cfg.request.headers,
        load: &cfg.load,
    })
}

/// Sets `path` (dotted) in `doc` to `value`, creating objects on the way.
fn set_path(doc: &mut Value, path: &str, value: Value) -> Result<(), String> {
    let mut at = doc;
    let parts: Vec<&str> = path.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return Err(format!("the axis {path:?} is not a dotted path"));
        }
        let Value::Object(map) = at else {
            return Err(format!("the axis {path:?} goes below a value that is not an object"));
        };
        if i == parts.len() - 1 {
            map.insert((*part).to_owned(), value);
            return Ok(());
        }
        at = map.entry((*part).to_owned()).or_insert_with(|| Value::Object(Default::default()));
    }
    Ok(())
}

const AXIS_ROOTS: [&str; 5] = ["request", "mock", "workspace", "key", "load"];

/// The suite expanded for a target of `target`'s kind. An error means the suite as a whole
/// cannot run there.
pub fn expand(suite: &Suite, target: TargetKind, max_cells: u32) -> Result<Plan, String> {
    let mut notes = Vec::new();
    for (path, values) in &suite.axes {
        let root = path.split('.').next().unwrap_or("");
        if !AXIS_ROOTS.contains(&root) {
            return Err(format!("the axis {path:?} is not below request, mock, workspace, key or load"));
        }
        if values.is_empty() {
            return Err(format!("the axis {path:?} has no values"));
        }
        if target == TargetKind::External && matches!(root, "mock" | "workspace" | "key") {
            return Err(format!(
                "the axis {path:?} cannot be set on an external target: the benchmark sets nothing up there"
            ));
        }
    }
    if suite.repetitions == 0 || suite.repetitions > 20 {
        return Err("repetitions is between 1 and 20".into());
    }
    let mut base = suite.base.clone();
    if target == TargetKind::External && !(base.workspace.is_default() && base.key.is_default()) {
        base.workspace = WorkspaceVariant::default();
        base.key = KeyVariant::default();
        notes.push("an external target has no workspace or key settings of the benchmark's: they were left out".into());
    }
    let base_doc = serde_json::to_value(&base).map_err(|e| e.to_string())?;

    // Every combination, the first axis outermost.
    let axes: Vec<(&String, &Vec<Value>)> = suite.axes.iter().collect();
    let mut combos: Vec<BTreeMap<String, Value>> = vec![BTreeMap::new()];
    for (path, values) in &axes {
        let mut next = Vec::with_capacity(combos.len() * values.len());
        for c in &combos {
            for v in *values {
                let mut c = c.clone();
                c.insert((*path).clone(), v.clone());
                next.push(c);
            }
        }
        combos = next;
        if combos.len() > (HARD_MAX_CELLS as usize) * 64 {
            return Err("the axes make far too many combinations".into());
        }
    }

    let mut configs: Vec<(BTreeMap<String, Value>, CellConfig)> = Vec::new();
    let mut pruned = Vec::new();
    for combo in combos {
        let mut doc = base_doc.clone();
        let mut peak = None;
        for (path, value) in &combo {
            if path == "load.target" {
                peak = Some(value.as_f64().ok_or_else(|| "load.target takes numbers".to_owned())?);
                continue;
            }
            set_path(&mut doc, path, value.clone())?;
        }
        let mut cfg: CellConfig = match serde_json::from_value(doc) {
            Ok(c) => c,
            Err(e) => {
                pruned.push(Pruned { axes: combo, reason: format!("not a configuration: {e}") });
                continue;
            }
        };
        if let Some(p) = peak {
            cfg.load = cfg.load.with_peak(p);
        }
        match cfg.check(target) {
            Ok(()) => configs.push((combo, cfg)),
            Err(reason) => pruned.push(Pruned { axes: combo, reason }),
        }
    }

    // The same configuration twice (an axis that changes nothing): kept once.
    let mut seen = BTreeSet::new();
    configs.retain(|(combo, cfg)| {
        let fresh = seen.insert(comparison_key(cfg));
        if !fresh {
            pruned.push(Pruned { axes: combo.clone(), reason: "the same configuration as another cell".into() });
        }
        fresh
    });

    match suite.order {
        Order::Grouped => configs.sort_by_cached_key(|(_, c)| group_key(c)),
        Order::Shuffled { seed } => shuffle(&mut configs, seed),
    }

    let mut cells = Vec::new();
    if suite.baseline {
        let mut baselines = BTreeSet::new();
        for (_, cfg) in &configs {
            let key = baseline_key(cfg);
            if baselines.insert(key.clone()) {
                cells.push(PlannedCell {
                    index: 0,
                    rep: 0,
                    kind: CellKind::Baseline,
                    axes: BTreeMap::new(),
                    config: cfg.clone(),
                    comparison_key: key.clone(),
                    baseline_key: key,
                });
            }
        }
    }
    for (combo, cfg) in configs {
        for rep in 0..suite.repetitions {
            cells.push(PlannedCell {
                index: 0,
                rep,
                kind: CellKind::Target,
                axes: combo.clone(),
                comparison_key: comparison_key(&cfg),
                baseline_key: baseline_key(&cfg),
                config: cfg.clone(),
            });
        }
    }
    if let Order::Shuffled { seed } = suite.order {
        // Repetitions are spread too, the baselines stay first.
        let first_target = cells.iter().position(|c| c.kind == CellKind::Target).unwrap_or(cells.len());
        let mut targets = cells.split_off(first_target);
        shuffle(&mut targets, seed.wrapping_add(1));
        cells.extend(targets);
    }
    for (i, c) in cells.iter_mut().enumerate() {
        c.index = i as u32;
    }

    let n_target = cells.iter().filter(|c| c.kind == CellKind::Target).count() as u32;
    let cap = suite.cap.min(max_cells).min(HARD_MAX_CELLS);
    if n_target == 0 {
        return Err("no combination of the axes can run".into());
    }
    if n_target > cap {
        return Err(format!("the suite makes {n_target} cells, more than the {cap} allowed"));
    }
    let estimate = estimate(&cells, target);
    Ok(Plan { cells, pruned, estimate, notes })
}

/// An external target's suite: its mock replaced by the one the target declares (the operator
/// put that gateway in front of the same mock), so that its cells line up with other targets'.
/// Without a declared mock its cells are not compared with any other target's. Returns the
/// note to show.
pub fn with_declared_mock(suite: &mut Suite, declared: Option<crate::model::MockProfile>) -> Option<String> {
    match declared {
        Some(mock) if suite.base.mock != mock => {
            suite.base.mock = mock;
            Some("the suite's mock was replaced by the one the target declares".into())
        }
        Some(_) => None,
        None => {
            Some("the target declares no mock: its cells are not compared with other targets' (mock_undeclared)".into())
        }
    }
}

/// Outermost first: what costs a configuration write.
fn group_key(c: &CellConfig) -> String {
    serde_json::to_string(&(&c.workspace, &c.mock, &c.key, &c.request, &c.load)).unwrap_or_default()
}

/// Fisher-Yates with a small fixed generator (splitmix64): the same seed gives the same order.
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    for i in (1..items.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

/// About how many calls a cell makes: a closed loop of `c` callers makes `c × T / L` calls
/// against a mock that takes `L`; an open loop makes what its stages ask for.
fn calls_of(cfg: &CellConfig) -> u64 {
    let l = &cfg.load;
    let mut prev = 0.0;
    let mut sum = 0.0;
    for s in &l.stages {
        let mean = (prev + s.target) / 2.0;
        let per_s = match l.mode {
            LoadMode::Rate => mean,
            LoadMode::Concurrency => mean * 1000.0 / cfg.mock.floor_ms(cfg.request.stream).max(1.0),
        };
        sum += per_s * s.duration_s as f64;
        prev = s.target;
    }
    let calls = sum.round() as u64;
    l.max_calls.map_or(calls, |m| calls.min(m))
}

fn estimate(cells: &[PlannedCell], target: TargetKind) -> Estimate {
    let mut e = Estimate::default();
    let managed = target == TargetKind::Managed;
    let mut profiles = BTreeMap::new();
    let mut variants = BTreeSet::new();
    for c in cells {
        let drain = DRAIN_S.min(c.config.load.timeout_s);
        e.duration_s += c.config.load.duration_s() + drain + CELL_OVERHEAD_S;
        e.calls += calls_of(&c.config);
        match c.kind {
            CellKind::Baseline => e.baseline_cells += 1,
            CellKind::Target => {
                e.cells += 1;
                if managed {
                    // The cell's key, made and deleted.
                    e.config_writes += 2;
                    let profile = c.config.mock.spec();
                    if profiles.insert(c.config.family(), profile.clone()).is_some_and(|p| p != profile) {
                        e.config_writes += 1;
                    }
                    variants.insert(serde_json::to_string(&c.config.workspace).unwrap_or_default());
                }
            }
        }
    }
    if managed {
        // The organisation, a workspace and a cache binding per variant, and a grant per model;
        // the teardown deletes as many.
        let setup = 1 + 2 * variants.len() as u64 + profiles.len() as u64;
        e.config_writes += 2 * setup;
    }
    e
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const SUITE: &str = r#"
name: managed vs proxy
base:
  request: { interface: openai, mode: managed }
  mock: { family: openai, delay_ms: 100, chunk_delay_ms: 10, chunks: 11 }
  load:
    mode: concurrency
    stages: [{ duration_s: 0, target: 10 }, { duration_s: 30, target: 10 }]
    timeout_s: 60
    warmup_s: 5
axes:
  request.mode: [managed, proxy]
  load.target: [10, 50]
"#;

    #[test]
    fn a_two_by_two_suite_makes_four_cells_after_its_baselines() {
        let suite = Suite::parse(SUITE).unwrap();
        let plan = expand(&suite, TargetKind::Managed, 256).unwrap();
        let kinds: Vec<CellKind> = plan.cells.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [
                CellKind::Baseline,
                CellKind::Baseline,
                CellKind::Target,
                CellKind::Target,
                CellKind::Target,
                CellKind::Target
            ]
        );
        assert_eq!(plan.estimate.cells, 4);
        assert_eq!(plan.estimate.baseline_cells, 2, "one per load level: the mode does not reach the mock");
        let targets: Vec<f64> = plan.cells.iter().map(|c| c.config.load.peak()).collect();
        assert_eq!(targets, [10.0, 50.0, 10.0, 50.0, 10.0, 50.0]);
        // Two managed and two proxy cells, each pointing at its load level's baseline.
        for c in plan.cells.iter().filter(|c| c.kind == CellKind::Target) {
            let b = plan.cells.iter().find(|b| b.kind == CellKind::Baseline && b.comparison_key == c.baseline_key);
            assert!(b.is_some());
        }
        // 30 s at 10 callers against a 200 ms mock: 1500 calls.
        assert_eq!(calls_of(&plan.cells[0].config), 1500);
        assert_eq!(plan.cells.iter().map(|c| c.index).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn combinations_that_cannot_run_are_pruned_with_a_reason() {
        let mut suite = Suite::parse(SUITE).unwrap();
        suite.axes.insert("workspace.cache".into(), vec![json!("off"), json!("enforce")]);
        let plan = expand(&suite, TargetKind::Managed, 256).unwrap();
        assert_eq!(plan.estimate.cells, 6, "proxy with the cache is never cached");
        assert_eq!(plan.pruned.len(), 2);
        assert!(plan.pruned.iter().all(|p| p.reason == "proxy mode is never cached"));
    }

    #[test]
    fn an_external_target_refuses_what_it_cannot_be_given() {
        let mut suite = Suite::parse(SUITE).unwrap();
        assert!(expand(&suite, TargetKind::External, 256).is_ok());
        suite.axes.insert("mock.delay_ms".into(), vec![json!(10), json!(20)]);
        let e = expand(&suite, TargetKind::External, 256).unwrap_err();
        assert!(e.contains("external target"), "{e}");
    }

    #[test]
    fn the_cap_and_the_same_configuration_twice() {
        let mut suite = Suite::parse(SUITE).unwrap();
        suite.cap = 3;
        assert!(expand(&suite, TargetKind::Managed, 256).unwrap_err().contains("more than the 3"));
        let mut suite = Suite::parse(SUITE).unwrap();
        suite.axes.insert("request.prompt_words".into(), vec![json!(60), json!(60)]);
        let plan = expand(&suite, TargetKind::Managed, 256).unwrap();
        assert_eq!(plan.estimate.cells, 4);
        assert_eq!(plan.pruned.len(), 4);
    }

    #[test]
    fn unknown_fields_and_paths_are_refused() {
        let mut suite = Suite::parse(SUITE).unwrap();
        suite.axes.insert("request.modee".into(), vec![json!("proxy")]);
        let plan = expand(&suite, TargetKind::Managed, 256);
        // Every combination carries the unknown field: none can run.
        assert_eq!(plan.unwrap_err(), "no combination of the axes can run");
        suite.axes.clear();
        suite.axes.insert("gateway.x".into(), vec![json!(1)]);
        assert!(expand(&suite, TargetKind::Managed, 256).is_err());
        assert!(Suite::parse("name: x\nbase: { load: { stages: [] } }\nfoo: 1\n").is_err());
    }

    #[test]
    fn a_shuffle_is_fixed_by_its_seed() {
        let mut suite = Suite::parse(SUITE).unwrap();
        suite.order = Order::Shuffled { seed: 7 };
        suite.repetitions = 3;
        let a = expand(&suite, TargetKind::Managed, 256).unwrap();
        let b = expand(&suite, TargetKind::Managed, 256).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.cells.iter().filter(|c| c.kind == CellKind::Target).count(), 12);
        assert_eq!(a.cells[0].kind, CellKind::Baseline);
    }

    #[test]
    fn grouped_order_keeps_one_workspace_variant_together() {
        let mut suite = Suite::parse(SUITE).unwrap();
        suite.axes.insert("workspace.audit".into(), vec![json!("on"), json!("off")]);
        let plan = expand(&suite, TargetKind::Managed, 256).unwrap();
        let audits: Vec<String> = plan
            .cells
            .iter()
            .filter(|c| c.kind == CellKind::Target)
            .map(|c| serde_json::to_string(&c.config.workspace.audit).unwrap())
            .collect();
        let changes = audits.windows(2).filter(|w| w[0] != w[1]).count();
        assert_eq!(changes, 1, "{audits:?}");
    }
}
