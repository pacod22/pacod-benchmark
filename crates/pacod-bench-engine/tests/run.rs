//! Whole runs against pacod-gw's own load agent and mock provider, started in process, and a
//! fake of the gateway's management API that records every call.
//!
//! The agent's default target is the mock itself, so a managed cell's calls reach the mock
//! without a gateway in between: what is under test is the controller (the scene's writes and
//! their order, the agents' plans, the collection), not a gateway.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use pacod_bench_engine::agents::{AgentClient, AgentRef};
use pacod_bench_engine::controller::{self, CellResult, CellStatus, RunEvent, RunInput, RunStatus, Target};
use pacod_bench_engine::gw::GwAdmin;
use pacod_bench_engine::model::{Placement, TargetKind};
use pacod_bench_engine::plan::{CellKind, Suite, expand};
use pacod_bench_engine::requests::Names;
use pacod_bench_engine::testing::{AGENT_TOKEN as TOKEN, FakeGw, spawn_agent as agent};
use pacod_gw_testkit::mock::MockProvider;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};

struct Run {
    status: RunStatus,
    leftovers: Vec<String>,
    cells: Vec<CellResult>,
    events: Vec<RunEvent>,
}

async fn run(target: Target, kind: TargetKind, suite: &str, agent_url: &str, baseline: Option<String>) -> Run {
    let suite = Suite::parse(suite).unwrap();
    let plan = expand(&suite, kind, 256).unwrap();
    let agents = vec![AgentClient::new(
        AgentRef { name: "local".into(), url: agent_url.into(), placement: Placement::InStack },
        TOKEN,
        reqwest::Client::new(),
    )];
    let input = RunInput {
        id: "test-run".into(),
        short: "test01".into(),
        target,
        agents,
        plan,
        baseline_url: baseline,
        agent_wait: Duration::from_secs(2),
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_cancel, cancel_rx) = watch::channel(false);
    let outcome = tokio::time::timeout(Duration::from_secs(120), controller::run(input, tx, cancel_rx)).await.unwrap();
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    let cells = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::CellDone(c) => Some((**c).clone()),
            _ => None,
        })
        .collect();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    Run { status: outcome.status, leftovers: outcome.leftovers, cells, events }
}

const SUITE: &str = r#"
name: two workspace variants
base:
  request: { interface: openai, mode: managed, prompt_words: 8, max_tokens: 8, headers: { x-mock: "delay=20" } }
  mock: { family: openai, delay_ms: 20, chunk_delay_ms: 1, chunks: 3 }
  load:
    mode: concurrency
    stages: [{ duration_s: 0, target: 2 }, { duration_s: 2, target: 2 }]
    timeout_s: 10
axes:
  workspace.audit: [on_request, "off"]
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_managed_run_sets_up_measures_and_takes_down() {
    let mock = MockProvider::spawn().await;
    let agent_url = agent(&mock.base_url()).await;
    let (fake, admin) = FakeGw::start().await;
    let gw = Arc::new(GwAdmin::new(&admin, "pidt_test", reqwest::Client::new()));
    let target = Target::Managed { gw, mock_url: "http://gw-load-mock:9000".into(), data_plane: None };
    let r = run(target, TargetKind::Managed, SUITE, &agent_url, Some(mock.base_url())).await;

    assert_eq!(r.status, RunStatus::Finished);
    assert!(r.leftovers.is_empty(), "{:?}", r.leftovers);
    let kinds: Vec<CellKind> = r.cells.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [CellKind::Baseline, CellKind::Target, CellKind::Target]);
    for c in &r.cells {
        assert_eq!(c.status, CellStatus::Done, "cell {}: {}", c.index, c.error);
        let s = c.steady.as_ref().unwrap();
        assert!(s.ok > 10 && s.failed == 0, "cell {}: {s:?}", c.index);
        assert!(s.gw_delay.is_none(), "no gateway answered: its figure is absent, not 0");
        assert_eq!(c.noise.quiet_before, Some(true));
        assert!(c.flags.iter().any(|f| f.code == "in_stack_agent"));
        assert!(!c.flags.iter().any(|f| f.code == "other_traffic"), "{:?}", c.flags);
        assert!(!c.points.is_empty() && !c.gateway.is_empty());
        assert_eq!(c.agents[0].state, "done");
    }
    assert!(r.cells[1].key_hint.starts_with("id"), "the key's id, never its secret");
    assert!(
        r.events.iter().any(|e| matches!(e, RunEvent::Observed(v) if v["status"]["software_version"] == "0.1.0-test"))
    );

    let writes = fake.writes();
    let made: Vec<String> =
        writes.iter().filter(|(m, p, _)| m == "POST" && p.starts_with("/orgs")).map(|(_, p, _)| p.clone()).collect();
    assert_eq!(made.iter().filter(|p| *p == "/orgs").count(), 1);
    assert_eq!(made.iter().filter(|p| p.ends_with("/workspaces")).count(), 2, "one per workspace variant");
    assert_eq!(made.iter().filter(|p| p.ends_with("/keys")).count(), 2, "one per target cell");
    let bindings: Vec<&Value> =
        writes.iter().filter(|(_, p, _)| p.ends_with("/plugin-bindings")).map(|(_, _, b)| b).collect();
    assert_eq!(bindings.len(), 2);
    assert!(bindings.iter().all(|b| b["plugin"] == "response_cache" && b["mode"] == "off"), "explicit, even off");
    let key_body = &writes.iter().find(|(_, p, _)| p.ends_with("/keys")).unwrap().2;
    assert_eq!(key_body["allowed_modes"], json!(["managed"]));
    assert!(key_body["expires_at"].is_string());

    // Nothing is written while a cell's load runs: between a key's creation and its deletion
    // there are only reads.
    let calls = fake.calls.lock().clone();
    for (i, (m, p, _)) in calls.iter().enumerate() {
        if m == "POST" && p.ends_with("/keys") {
            let deleted =
                calls.iter().skip(i + 1).position(|(m, p2, _)| m == "DELETE" && p2.contains("/keys/")).unwrap();
            assert!(calls[i + 1..i + 1 + deleted].iter().all(|(m, _, _)| m == "GET"), "a write during the cell");
        }
    }
    // Everything made in the scratch organisation is deleted, the organisation last.
    let deleted: Vec<&String> = writes.iter().filter(|(m, _, _)| m == "DELETE").map(|(_, p, _)| p).collect();
    assert_eq!(deleted.len(), 2 + 1 + 2 + 2 + 1, "keys, organisation, workspaces, bindings, grant");
    assert!(deleted.last().unwrap().split('/').count() == 3, "the organisation goes last: {deleted:?}");
    let unique: BTreeSet<&String> = deleted.iter().copied().collect();
    assert_eq!(unique.len(), deleted.len());
    // The mocks' providers carry the cell's profile, below the mock as the gateway reaches it.
    let provider = fake.providers.lock()[0].clone();
    assert_eq!(provider["base_url"], "http://gw-load-mock:9000/x-mock/delay=20,chunk_delay=1,chunks=3/v1");
    assert_eq!(provider["name"], "bench-load-mock-openai");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_external_run_sets_nothing_up_and_keeps_a_hint_of_the_key() {
    let mock = MockProvider::spawn().await;
    let agent_url = agent("http://127.0.0.1:9").await;
    let suite = SUITE.replace("axes:\n  workspace.audit: [on_request, \"off\"]\n", "");
    let target = Target::External {
        base_url: mock.base_url(),
        key: "pgw_ext_secretpart".into(),
        names: Names { model: "gpt-x".into(), ..Names::default() },
        paid: true,
    };
    let r = run(target, TargetKind::External, &suite, &agent_url, None).await;
    assert_eq!(r.status, RunStatus::Finished);
    let kinds: Vec<(CellKind, CellStatus)> = r.cells.iter().map(|c| (c.kind, c.status)).collect();
    assert_eq!(kinds, [(CellKind::Baseline, CellStatus::Skipped), (CellKind::Target, CellStatus::Done)]);
    let cell = &r.cells[1];
    assert_eq!(cell.key_hint, "pgw_ext_…");
    assert!(cell.noise.quiet_before.is_none(), "an external target has no figures of its own");
    assert!(cell.steady.as_ref().unwrap().ok > 10);
    assert!(r.events.iter().any(|e| matches!(e, RunEvent::Warning(w) if w.contains("BENCH_BASELINE_MOCK_URL"))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_busy_agent_stops_the_run_before_anything_starts() {
    let mock = MockProvider::spawn().await;
    let agent_url = agent(&mock.base_url()).await;
    // Another controller (the tester) holds the agent: a prepared run it never starts.
    let other = AgentClient::new(
        AgentRef { name: "local".into(), url: agent_url.clone(), placement: Placement::Unknown },
        TOKEN,
        reqwest::Client::new(),
    );
    let suite = Suite::parse(&SUITE.replace("axes:\n  workspace.audit: [on_request, \"off\"]\n", "")).unwrap();
    let plan = expand(&suite, TargetKind::External, 256).unwrap();
    let names = Names { model: "gpt-x".into(), ..Names::default() };
    let spec = pacod_bench_engine::requests::build(&plan.cells[1].config, &names, false);
    let hold: pacod_gw_load::contract::Prepare = serde_json::from_value(json!({
        "contract": 1, "run_id": "tester-run", "part": 0, "parts": 1, "key": "k",
        "auth_header": "authorization", "base_url": mock.base_url(), "request": spec,
        "profile": { "mode": "concurrency", "stages": [{ "duration_s": 5, "target": 1 }], "timeout_s": 5, "drain_s": 1 },
        "lease_s": 60
    }))
    .unwrap();
    other.prepare(&hold, Duration::from_secs(10)).await.unwrap();

    let target = Target::External { base_url: mock.base_url(), key: "k".into(), names, paid: false };
    let suite_text = SUITE.replace("axes:\n  workspace.audit: [on_request, \"off\"]\n", "");
    let suite = Suite::parse(&suite_text).unwrap();
    let plan = expand(&suite, TargetKind::External, 256).unwrap();
    let agents = vec![other.clone()];
    let input = RunInput {
        id: "busy".into(),
        short: "busy01".into(),
        target,
        agents,
        plan,
        baseline_url: Some(mock.base_url()),
        agent_wait: Duration::from_secs(1),
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_c, cancel) = watch::channel(false);
    let outcome = controller::run(input, tx, cancel).await;
    // The run's own check finds the agent busy before anything starts.
    assert_eq!(outcome.status, RunStatus::Error);
    assert!(outcome.error.unwrap().contains("busy with tester-run"));
    while rx.try_recv().is_ok() {}
    other.cancel("tester-run").await.unwrap();
}
