//! The report's contract, pacod-gw's (vendor/pacod-gw/crates/pacod-gw-bench-report): its
//! fixtures read through the gateway's own checks, and a report this benchmark builds passes
//! them and fits.

use std::path::PathBuf;

use pacod_bench_engine::collect::{AgentPart, Histograms, Pct, Point, Summary};
use pacod_bench_engine::controller::{CellResult, CellStatus};
use pacod_bench_engine::model::{Placement, TargetKind};
use pacod_bench_engine::plan::{Suite, expand};
use pacod_bench_engine::report::{RunInfo, build, encode, label};
use pacod_gw_bench_report as gwr;
use serde_json::json;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/pacod-gw/crates/pacod-gw-bench-report/tests/fixtures/v1")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_gateways_fixtures_read_as_it_reads_them() {
    for name in ["minimal.json", "full.json", "external-third-party.json", "unknown-fields.json"] {
        gwr::parse(&fixture(name)).unwrap_or_else(|p| panic!("{name}: {p:?}"));
    }
    for name in ["bad-histogram.json", "negative-rate.json", "wrong-schema.json", "bad-key.json"] {
        assert!(gwr::parse(&fixture(name)).is_err(), "{name}");
    }
}

fn pct(ms: f64) -> Pct {
    Pct { p50: ms, p95: ms * 1.1, p99: ms * 1.3, max: ms * 1.5 }
}

fn summary(ms: f64, gw: bool) -> Summary {
    Summary {
        duration_s: 60.0,
        started: 6000,
        calls: 6000,
        ok: 5990,
        failed: 10,
        rps_mean: 100.0,
        rps_peak: 120.0,
        ttfb: pct(ms / 2.0),
        total: pct(ms),
        gw_delay: gw.then(|| pct(0.2)),
        ..Summary::default()
    }
}

/// A finished run of `n` cells of `seconds` each, as the controller reports it.
fn run(n: usize, seconds: u64) -> Vec<CellResult> {
    let suite = Suite::parse(
        "name: s\nbase:\n  load: { stages: [{ duration_s: 0, target: 10 }, { duration_s: 60, target: 10 }] }\naxes:\n  request.prompt_words: [1,2,3,4,5,6,7,8]\n  load.target: [1,2,3,4,5,6,7,8]\nbaseline: false\n",
    )
    .unwrap();
    let plan = expand(&suite, TargetKind::Managed, 256).unwrap();
    plan.cells
        .iter()
        .take(n)
        .map(|c| {
            let mut r = CellResult::of(c);
            r.status = CellStatus::Done;
            r.agents = vec![AgentPart {
                name: "eu-1".into(),
                placement: Placement::Remote,
                cpus: 8,
                version: "0.1.0".into(),
                ..AgentPart::default()
            }];
            r.steady = Some(summary(250.0, true));
            r.summary = Some(summary(260.0, true));
            let mut h = Histograms::default();
            for v in [1_000u64, 50_000, 250_000, 900_000] {
                h.ttfb.record(v);
                h.total.record(v * 2);
                h.gw_delay.record(v / 1000);
            }
            r.histograms = Some(h);
            r.points = (0..seconds)
                .map(|t| Point {
                    t,
                    ok: 100,
                    completed: 100,
                    total: Some(pct(250.0)),
                    ttfb: Some(pct(120.0)),
                    gw_delay: Some(pct(0.2)),
                    ..Point::default()
                })
                .collect();
            r
        })
        .collect()
}

fn info<'a>(declared: &'a serde_json::Value, agents: &'a [(String, Placement)]) -> RunInfo<'a> {
    RunInfo {
        id: "run_x",
        name: "a run",
        description: "",
        created_at: "2026-10-27T10:00:00Z".parse().unwrap(),
        version: "0.1.0",
        gw_commit: "c8548ae",
        run_by: Some(("admin@example.com".into(), "usr_1".into())),
        target_label: "eu gateway",
        target_kind: TargetKind::Managed,
        product: "pacod_gw",
        observed: None,
        declared,
        declared_mock: None,
        agents,
        mock_shared: false,
        suite: json!({}),
    }
}

#[test]
fn a_report_of_this_benchmark_passes_the_gateways_checks() {
    let declared = json!({ "layout": "helm", "arch": "amd64" });
    let agents = [("eu-1".to_owned(), Placement::Remote)];
    let cells = run(4, 60);
    let bytes = encode(&info(&declared, &agents), &cells).unwrap();
    let r = gwr::parse(&bytes).unwrap();
    assert_eq!(r.measurement.class, gwr::Class::Capacity, "{:?}", r.measurement.reasons);
    assert_eq!(r.fact("layout").as_deref(), Some("helm"));
    assert_eq!(r.environment["arch"].source, gwr::Source::Declared);
    assert_eq!(r.cells[0].label, "load 1, prompt 1", "the axes in their order");
    assert_eq!(r.cells[0].series.as_ref().unwrap().t.len(), 60);
    assert_eq!(r.agents[0].placement, "remote");
    // The same run makes the same bytes: downloaded and published, it is one report.
    assert_eq!(encode(&info(&declared, &agents), &cells).unwrap(), bytes);
    assert_eq!(label(&cells[1]), "load 2, prompt 1");
}

#[test]
fn a_large_run_is_thinned_out_to_fit() {
    let declared = json!({});
    let agents = [("eu-1".to_owned(), Placement::Remote)];
    let cells = run(64, 3600);
    let full = serde_json::to_vec(&build(&info(&declared, &agents), &cells)).unwrap();
    assert!(full.len() > gwr::MAX_BYTES, "the test needs a run that does not fit as it is: {}", full.len());
    let bytes = encode(&info(&declared, &agents), &cells).unwrap();
    assert!(bytes.len() <= gwr::MAX_BYTES);
    let r = gwr::parse(&bytes).unwrap();
    assert_eq!(r.cells.len(), 64);
    assert!(r.cells.iter().all(|c| c.flags.iter().any(|f| f.code.starts_with("series_"))));
    assert!(r.cells.iter().all(|c| c.histograms.is_some() && c.steady.is_some()), "the figures stay whole");
}
