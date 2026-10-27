//! A stored run as its report (`pacod-bench-report/1`), and where the report is published.

use pacod_bench_engine::controller::CellResult;
use pacod_bench_engine::model::{Placement, TargetKind};
use pacod_bench_engine::report::{RunInfo, encode};
use serde_json::Value;

use crate::store::{RunRow, Store};

/// The pacod-gw commit this build vendored (vendor/pacod-gw/VERSION): `commit <sha>`, or
/// `worktree of <sha>, …` while a change to both repositories is being made.
const GW_VERSION: &str = include_str!("../../../vendor/pacod-gw/VERSION");

pub fn gw_commit() -> String {
    let first = GW_VERSION.lines().next().unwrap_or("");
    if let Some(sha) = first.strip_prefix("commit ") {
        return sha.trim().to_owned();
    }
    match first.strip_prefix("worktree of ") {
        Some(rest) => format!("{}+worktree", rest.split(',').next().unwrap_or("").trim()),
        None => String::new(),
    }
}

/// A managed gateway's mock is pacod-gw's load mock, which the tester's load tests share; an
/// external target's is the operator's, declared with it.
pub fn mock_shared(run: &RunRow) -> bool {
    run.target["kind"] == "managed" && run.target["declared"]["mock_dedicated"] != true
}

/// The run's agents and where they stood, from the run's copy of them.
fn agents(run: &RunRow) -> Vec<(String, Placement)> {
    run.agents["agents"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|a| {
                    let name = a["name"].as_str().unwrap_or("").to_owned();
                    let placement = a["placement"].as_str().and_then(Placement::parse).unwrap_or_default();
                    (name, placement)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The report of a run, as bytes: the same run gives the same bytes.
pub async fn report(store: &Store, run: &RunRow) -> Result<Vec<u8>, String> {
    let rows = store.cells_full(&run.id).await.map_err(|e| e.to_string())?;
    let cells: Vec<CellResult> = rows
        .into_iter()
        .filter_map(|r| r.result)
        .filter_map(|mut v| {
            // The seconds are kept in their own column; the result has them too.
            if v.get("points").is_none() {
                v["points"] = Value::Array(vec![]);
            }
            serde_json::from_value(v).ok()
        })
        .collect();
    if cells.is_empty() {
        return Err("no cell of this run ended: there is nothing to report".into());
    }
    let suite_name = run.suite["name"].as_str().unwrap_or("");
    let name = if run.label.is_empty() { suite_name } else { run.label.as_str() };
    let agents = agents(run);
    let gw_commit = gw_commit();
    let info = RunInfo {
        id: &run.id,
        name: if name.is_empty() { &run.id } else { name },
        description: run.suite["description"].as_str().unwrap_or(""),
        created_at: run.finished_at.unwrap_or(run.created_at),
        version: env!("CARGO_PKG_VERSION"),
        gw_commit: &gw_commit,
        run_by: Some((run.started_by_name.clone(), run.started_by.clone())),
        target_label: run.target["name"].as_str().unwrap_or(""),
        target_kind: if run.target["kind"] == "managed" { TargetKind::Managed } else { TargetKind::External },
        product: run.target["product"].as_str().unwrap_or("pacod_gw"),
        observed: run.observed.as_ref(),
        declared: &run.target["declared"],
        declared_mock: run.target["definition"].get("mock").filter(|m| !m.is_null()),
        agents: &agents,
        mock_shared: mock_shared(run),
        suite: run.suite.clone(),
    };
    encode(&info, &cells)
}
