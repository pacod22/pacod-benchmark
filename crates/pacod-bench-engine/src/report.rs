//! A run as a report the gateway takes (`pacod-bench-report/1`, pacod-gw's
//! `crates/pacod-gw-bench-report`): built from the gateway's own types, so that what the benchmark
//! writes is what the gateway reads, and checked with the gateway's own `validate` before it
//! leaves.
//!
//! The same run makes the same bytes: a report downloaded, and the same report published, are one
//! report at the gateway (it keeps a report once per digest). So the document holds nothing that
//! changes between the two: who ran the run, not who published it, and the run's own times.

use chrono::{DateTime, Utc};
use pacod_gw_bench_report as gwr;
use serde_json::{Value, json};

use crate::collect::{Histograms, Point, Summary};
use crate::controller::{CellResult, CellStatus};
use crate::measure;
use crate::model::{Placement, TargetKind};
use crate::plan::CellKind;

/// The producer's name in every report.
pub const PRODUCER: &str = "pacod-benchmark";

/// What a report says about its run besides the cells.
pub struct RunInfo<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub created_at: DateTime<Utc>,
    pub version: &'a str,
    /// The pacod-gw commit this benchmark was built against (vendor/pacod-gw/VERSION).
    pub gw_commit: &'a str,
    pub run_by: Option<(String, String)>,
    pub target_label: &'a str,
    pub target_kind: TargetKind,
    pub product: &'a str,
    /// `{status, cache}` as the gateway described itself.
    pub observed: Option<&'a Value>,
    /// What the operator declared about the deployment: an object.
    pub declared: &'a Value,
    /// An external target's mock, as declared.
    pub declared_mock: Option<&'a Value>,
    pub agents: &'a [(String, Placement)],
    pub mock_shared: bool,
    pub suite: Value,
}

/// Short names of the configuration's fields, for cell labels (the UI's `axisName`).
fn short(path: &str) -> &str {
    match path {
        "request.mode" => "mode",
        "request.interface" => "interface",
        "request.stream" => "stream",
        "request.prompt_words" => "prompt",
        "request.max_tokens" => "answer",
        "request.http" => "http",
        "mock.family" => "mock",
        "mock.delay_ms" => "delay",
        "mock.chunk_delay_ms" => "chunk delay",
        "mock.chunks" => "chunks",
        "workspace.cache" => "cache",
        "workspace.audit" => "audit",
        "key.inspect" => "inspect",
        "key.usage_policy" => "usage",
        "load.target" => "load",
        "load.mode" => "load mode",
        other => other,
    }
}

/// A cell in a few words, as the UI names it.
pub fn label(c: &CellResult) -> String {
    let rep = if c.rep > 0 { format!(" (#{})", c.rep + 1) } else { String::new() };
    if c.kind == CellKind::Baseline {
        let m = &c.config.mock;
        return format!("baseline {} {} ms, load {}", m.family, m.delay_ms, c.config.load.peak());
    }
    let axes: Vec<String> = c
        .axes
        .iter()
        .map(|(k, v)| format!("{} {}", short(k), v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())))
        .collect();
    let text = if axes.is_empty() { "base".to_owned() } else { axes.join(", ") };
    format!("{text}{rep}")
}

fn pct(p: &crate::collect::Pct) -> gwr::Pct {
    gwr::Pct { p50: p.p50, p95: p.p95, p99: p.p99, max: p.max }
}

fn summary(s: &Summary) -> gwr::Summary {
    gwr::Summary {
        duration_s: s.duration_s,
        calls: s.calls,
        ok: s.ok,
        failed: s.failed,
        skipped: s.skipped,
        rps_mean: s.rps_mean,
        rps_peak: s.rps_peak,
        status: s.status.clone(),
        ttfb: pct(&s.ttfb),
        total: pct(&s.total),
        gw_delay: s.gw_delay.as_ref().map(pct),
    }
}

fn histogram(h: &pacod_gw_load::histogram::Histogram) -> gwr::Histogram {
    gwr::Histogram { n: h.n, min: h.min, max: h.max, sum: h.sum, b: h.b.iter().map(|(i, c)| (*i, *c)).collect() }
}

fn histograms(h: &Histograms) -> gwr::Histograms {
    gwr::Histograms {
        ttfb: histogram(&h.ttfb),
        total: histogram(&h.total),
        gw_delay: (!h.gw_delay.is_empty()).then(|| histogram(&h.gw_delay)),
    }
}

/// The seconds as columns, every `step` seconds: the busiest of each step's seconds, so that a
/// short spike stays visible.
fn series(points: &[Point], step: u32) -> gwr::Series {
    let step = step.max(1) as usize;
    let chunks: Vec<&[Point]> = points.chunks(step).collect();
    let max = |f: &dyn Fn(&Point) -> Option<f64>| -> Vec<Option<f64>> {
        chunks.iter().map(|c| c.iter().filter_map(f).reduce(f64::max)).collect()
    };
    gwr::Series {
        step_s: step as u32,
        t: chunks.iter().map(|c| c[0].t).collect(),
        ok: max(&|p| Some(p.ok as f64)),
        failed: max(&|p| Some(p.failed as f64)),
        in_flight: max(&|p| Some(p.in_flight as f64)),
        total_p50: max(&|p| p.total.map(|x| x.p50)),
        total_p99: max(&|p| p.total.map(|x| x.p99)),
        ttfb_p50: max(&|p| p.ttfb.map(|x| x.p50)),
        gw_delay_p99: max(&|p| p.gw_delay.map(|x| x.p99)),
    }
}

fn cell(c: &CellResult, step: Option<u32>) -> gwr::Cell {
    let status = match c.status {
        CellStatus::Done => "done",
        CellStatus::Skipped => "skipped",
        CellStatus::Cancelled => "cancelled",
        CellStatus::Error => "error",
        // A cell that never ended: the run stopped in it.
        _ => "cancelled",
    };
    let step = step.unwrap_or_else(|| (c.points.len().div_ceil(gwr::MAX_POINTS)).max(1) as u32);
    gwr::Cell {
        index: c.index,
        rep: c.rep,
        kind: if c.kind == CellKind::Baseline { gwr::CellKind::Baseline } else { gwr::CellKind::Target },
        status: status.into(),
        label: label(c),
        axes: c.axes.clone(),
        comparison_key: c.comparison_key.clone(),
        baseline_key: c.baseline_key.clone(),
        config: json!(c.config),
        steady: c.steady.as_ref().map(summary),
        whole: c.summary.as_ref().map(summary),
        histograms: c.histograms.as_ref().map(histograms),
        series: (!c.points.is_empty()).then(|| series(&c.points, step)),
        flags: c.flags.iter().map(|f| gwr::Flag { code: f.code.clone(), message: f.message.clone() }).collect(),
        error: c.error.chars().take(1500).collect(),
    }
}

fn fact(value: Value, source: gwr::Source) -> gwr::Fact {
    gwr::Fact { value, source }
}

fn environment(info: &RunInfo) -> std::collections::BTreeMap<String, gwr::Fact> {
    let mut env = std::collections::BTreeMap::new();
    if let Some(declared) = info.declared.as_object() {
        for (k, v) in declared.iter().take(40) {
            env.insert(k.chars().take(60).collect(), fact(v.clone(), gwr::Source::Declared));
        }
    }
    if let Some(o) = info.observed {
        let status = &o["status"];
        if let Some(v) = status["software_version"].as_str() {
            env.insert("gateway_version".into(), fact(json!(v), gwr::Source::Observed));
        }
        if let Some(i) = status["instances"].as_array() {
            env.insert("instances".into(), fact(json!(i.len()), gwr::Source::Observed));
        }
        if let Some(v) = o["cache"]["store"].as_str() {
            env.insert("cache_store".into(), fact(json!(v), gwr::Source::Observed));
        }
        let analytics = status["components"].as_array().and_then(|c| c.iter().find(|c| c["name"] == "analytics"));
        if let Some(a) = analytics.and_then(|a| a["status"].as_str()) {
            env.insert("analytics".into(), fact(json!(a), gwr::Source::Observed));
        }
    }
    let mock = match (info.target_kind, info.declared_mock) {
        (TargetKind::Managed, _) if info.mock_shared => {
            json!("pacod-gw's load mock, shared with the tester's load tests")
        }
        (TargetKind::Managed, _) => json!("pacod-gw's load mock, no other load"),
        (TargetKind::External, Some(m)) => json!(format!("declared: {m}")),
        (TargetKind::External, None) => json!("not declared"),
    };
    env.entry("mock".into()).or_insert(fact(mock, gwr::Source::Declared));
    env
}

/// The run's report, its series at one point a second (or fewer where a cell is long).
pub fn build(info: &RunInfo, cells: &[CellResult]) -> gwr::Report {
    let m = measure::classify(cells, info.mock_shared);
    let mut agents = Vec::new();
    for (name, placement) in info.agents {
        let part = cells.iter().flat_map(|c| &c.agents).find(|a| &a.name == name);
        agents.push(gwr::Agent {
            name: name.clone(),
            version: part.map(|p| p.version.clone()).unwrap_or_default(),
            cpus: part.map_or(0, |p| p.cpus),
            placement: serde_json::to_value(placement)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
        });
    }
    gwr::Report {
        schema: gwr::SCHEMA.into(),
        minor: gwr::MINOR,
        id: info.id.into(),
        name: info.name.chars().take(200).collect(),
        description: info.description.chars().take(2000).collect(),
        created_at: info.created_at,
        producer: gwr::Producer {
            name: PRODUCER.into(),
            version: info.version.into(),
            gw_commit: info.gw_commit.into(),
            load_contract: pacod_gw_load::contract::CONTRACT,
        },
        run_by: info.run_by.as_ref().map(|(name, id)| gwr::Person { name: name.clone(), id: id.clone() }),
        target: gwr::Target {
            label: info.target_label.chars().take(200).collect(),
            kind: match info.target_kind {
                TargetKind::Managed => gwr::TargetKind::Managed,
                TargetKind::External => gwr::TargetKind::External,
            },
            product: info.product.into(),
        },
        environment: environment(info),
        agents,
        measurement: gwr::Measurement {
            class: match m.class {
                measure::Class::Capacity => gwr::Class::Capacity,
                measure::Class::Behaviour => gwr::Class::Behaviour,
            },
            reasons: m.reasons,
        },
        suite: info.suite.clone(),
        cells: cells.iter().map(|c| cell(c, None)).collect(),
        extensions: Value::Null,
    }
}

/// The report as bytes the gateway takes: at most `MAX_BYTES`, its series thinned out first
/// (flagged `series_downsampled`), then left out. An error when even that does not fit, or the
/// gateway's checks refuse it.
pub fn encode(info: &RunInfo, cells: &[CellResult]) -> Result<Vec<u8>, String> {
    let mut report = build(info, cells);
    let fits = |r: &gwr::Report| serde_json::to_vec(r).map(|b| b.len() <= gwr::MAX_BYTES).unwrap_or(false);
    let mut step = 1;
    while !fits(&report) && step < 60 {
        step = match step {
            1 => 2,
            2 => 5,
            5 => 10,
            10 => 30,
            _ => 60,
        };
        report.cells = cells.iter().map(|c| cell(c, Some(step))).collect();
        for c in &mut report.cells {
            if c.series.is_some() {
                c.flags.push(gwr::Flag {
                    code: "series_downsampled".into(),
                    message: format!("the seconds are shown {step} at a time, the busiest of each"),
                });
            }
        }
    }
    if !fits(&report) {
        for c in &mut report.cells {
            if c.series.take().is_some() {
                c.flags.push(gwr::Flag {
                    code: "series_left_out".into(),
                    message: "the seconds did not fit the report".into(),
                });
            }
        }
    }
    gwr::validate(&report)
        .map_err(|p| p.iter().map(|p| format!("{}: {}", p.path, p.message)).collect::<Vec<_>>().join("; "))?;
    let bytes = serde_json::to_vec(&report).map_err(|e| e.to_string())?;
    if bytes.len() > gwr::MAX_BYTES {
        return Err(format!("the report is {} bytes, more than the gateway takes", bytes.len()));
    }
    Ok(bytes)
}
