//! The API end to end: a database of its own on the test PostgreSQL (`make test-services`, or
//! BENCH_TEST_DATABASE_URL), pacod-gw's agent and mock in process, and a fake of the gateway's
//! management API. Callers use the development token.

use std::sync::Arc;
use std::time::Duration;

use pacod_bench::config::{Auth, Config, ManagedGw, Publish};
use pacod_bench_engine::agents::AgentRef;
use pacod_bench_engine::model::Placement;
use pacod_bench_engine::testing::{AGENT_TOKEN, FakeGw, serve, spawn_agent};
use pacod_gw_testkit::mock::MockProvider;
use serde_json::{Value, json};
use tokio::sync::watch;

const DEV: &str = "a-development-token-of-32-characters";

async fn database() -> String {
    let base =
        std::env::var("BENCH_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:test@127.0.0.1:25436".into());
    let admin = sqlx::PgPool::connect(&format!("{base}/postgres"))
        .await
        .unwrap_or_else(|e| panic!("the test database does not answer ({e}): run make test-services"));
    let name = format!("bench_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
    format!("{base}/{name}")
}

struct Env {
    url: String,
    http: reqwest::Client,
    fake: Arc<FakeGw>,
    _mock: MockProvider,
    _stop: watch::Sender<bool>,
}

impl Env {
    async fn start() -> Env {
        let mock = MockProvider::spawn().await;
        let agent = spawn_agent(&mock.base_url()).await;
        let (fake, admin) = FakeGw::start().await;
        let config = Config {
            listen: "127.0.0.1:0".into(),
            metrics_listen: None,
            database_url: database().await,
            auth: Auth::Dev { token: DEV.into() },
            gateways: vec![ManagedGw {
                name: "gateway".into(),
                admin_url: admin.clone(),
                token: "pidt_test".into(),
                mock_url: "http://gw-load-mock:9000".into(),
                data_plane: None,
                declared: json!({ "layout": "single" }),
            }],
            agents: vec![AgentRef { name: "local".into(), url: agent, placement: Placement::InStack }],
            agent_token: Some(AGENT_TOKEN.into()),
            baseline_mock_url: Some(mock.base_url()),
            max_cells: 64,
            max_duration_s: 3600,
            agent_wait_s: 5,
            publish: Some(Publish { url: admin, token: "pidt_test".into() }),
            console_url: Some("http://console.localhost".into()),
        };
        let (stop, stop_rx) = watch::channel(false);
        let app = pacod_bench::app(Arc::new(config), stop_rx).await.unwrap();
        let addr = serve(app.api).await;
        Env { url: format!("http://{addr}/api/v1"), http: reqwest::Client::new(), fake, _mock: mock, _stop: stop }
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut r = self.http.request(method, format!("{}{path}", self.url)).bearer_auth(DEV);
        if let Some(b) = body {
            r = r.json(&b);
        }
        let response = r.send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn get(&self, path: &str) -> (u16, Value) {
        self.call(reqwest::Method::GET, path, None).await
    }

    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }

    /// Waits until the run has ended; its page.
    async fn until_ended(&self, id: &str, within: Duration) -> Value {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let (_, page) = self.get(&format!("/runs/{id}")).await;
            let status = page["run"]["status"].as_str().unwrap_or("").to_owned();
            if ["finished", "cancelled", "error", "interrupted"].contains(&status.as_str()) {
                return page;
            }
            assert!(tokio::time::Instant::now() < deadline, "the run is still {status}");
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }
}

const SUITE: &str = r#"
name: managed, two load levels
base:
  request: { interface: openai, mode: managed, prompt_words: 8, max_tokens: 8, headers: { x-mock: "delay=20" } }
  mock: { family: openai, delay_ms: 20, chunk_delay_ms: 1, chunks: 3 }
  load:
    mode: concurrency
    stages: [{ duration_s: 0, target: 1 }, { duration_s: 2, target: 1 }]
    timeout_s: 10
axes:
  load.target: [1, 2]
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_suite_runs_from_the_api_to_its_end() {
    let env = Env::start().await;
    let (status, session) = env.get("/session").await;
    assert_eq!(status, 200);
    assert_eq!(session["permissions"].as_array().unwrap().len(), 6);
    assert_eq!(session["gateways"], json!(["gateway"]));

    let (_, targets) = env.get("/targets").await;
    assert_eq!(targets["items"][0]["id"], "cfg-gateway");
    assert_eq!(targets["items"][0]["source"], "config");
    assert!(targets["items"][0]["definition"].get("token").is_none(), "no secret in a target");

    let (status, suite) = env.post("/suites", json!({ "yaml": SUITE })).await;
    assert_eq!(status, 201, "{suite}");
    let suite_id = suite["id"].as_str().unwrap();
    let (status, plan) = env.post("/plan", json!({ "suite_id": suite_id, "target_id": "cfg-gateway" })).await;
    assert_eq!(status, 200, "{plan}");
    assert_eq!(plan["estimate"]["cells"], 2);
    assert_eq!(plan["estimate"]["baseline_cells"], 2);

    let (status, run) =
        env.post("/runs", json!({ "suite_id": suite_id, "target_id": "cfg-gateway", "label": "first" })).await;
    assert_eq!(status, 201, "{run}");
    let id = run["id"].as_str().unwrap().to_owned();
    let page = env.until_ended(&id, Duration::from_secs(120)).await;
    assert_eq!(page["run"]["status"], "finished", "{}", page["run"]["error"]);
    assert_eq!(page["run"]["leftovers"], json!([]));
    assert_eq!(page["run"]["observed"]["status"]["software_version"], "0.1.0-test");
    // The stored results read again: the in-stack agent makes the figures behaviour.
    assert_eq!(page["measurement"]["class"], "behaviour");
    let reasons = page["measurement"]["reasons"].to_string();
    assert!(reasons.contains("agent local ran in the gateway's stack"), "{reasons}");
    assert!(!reasons.contains("no cell ran to its end"), "{reasons}");
    let cells = page["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 4);
    for c in cells {
        assert_eq!(c["status"], "done", "{c}");
        assert!(c["result"]["steady"]["ok"].as_u64().unwrap() > 0);
        assert!(c.get("points").is_none() && c["result"].get("points").is_none(), "lists leave the seconds out");
    }
    // One cell in full, with its seconds.
    let (_, cell) = env.get(&format!("/runs/{id}/cells/2")).await;
    assert!(!cell["points"].as_array().unwrap().is_empty());
    // The target now carries what the gateway said about itself.
    let (_, target) = env.get("/targets/cfg-gateway").await;
    assert_eq!(target["observed"]["cache"]["store"], "memory");
    // The scratch organisation was made and taken down.
    let writes = env.fake.writes();
    assert_eq!(writes.iter().filter(|(m, p, _)| m == "POST" && p == "/orgs").count(), 1);
    assert_eq!(writes.iter().filter(|(m, p, _)| m == "DELETE" && p.split('/').count() == 3).count(), 1);

    // Following a run that has ended: its snapshot, then the end.
    let text = env
        .http
        .get(format!("{}/runs/{id}/events", env.url))
        .bearer_auth(DEV)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(text.starts_with("event: snapshot\n"), "{text}");
    assert!(text.contains("event: done"));

    // The report: what the gateway takes, and the same bytes when published.
    let file = env.http.get(format!("{}/runs/{id}/report", env.url)).bearer_auth(DEV).send().await.unwrap();
    assert_eq!(file.status(), 200);
    let bytes = file.bytes().await.unwrap().to_vec();
    let report = pacod_gw_bench_report::parse(&bytes).unwrap_or_else(|p| panic!("{p:?}"));
    assert_eq!(report.cells.len(), 4);
    assert_eq!(report.measurement.class, pacod_gw_bench_report::Class::Behaviour);
    assert_eq!(report.fact("layout").as_deref(), Some("single"));
    assert_eq!(report.environment["gateway_version"].source, pacod_gw_bench_report::Source::Observed);
    let (status, published) = env.post(&format!("/runs/{id}/publish"), json!({})).await;
    assert_eq!(status, 200, "{published}");
    assert!(
        published["link"].as_str().unwrap().starts_with("http://console.localhost/platform/testing/benchmarks/bmr_")
    );
    assert_eq!(env.fake.reports.lock()[0], bytes, "published as downloaded: one report at the gateway");
    let (_, page) = env.get(&format!("/runs/{id}")).await;
    assert_eq!(page["publications"][0]["status"], "published");

    let (_, list) = env.get("/runs").await;
    assert_eq!(list["items"][0]["id"], id);
    assert!(list["items"][0].get("suite").is_none());
    assert_eq!(list["items"][0]["estimate"]["cells"], 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn callers_need_a_valid_token() {
    let env = Env::start().await;
    let r = env.http.get(format!("{}/session", env.url)).send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = env.http.get(format!("{}/session", env.url)).bearer_auth("not-the-token").send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = env.http.get(format!("{}/healthz", env.url)).send().await.unwrap();
    assert_eq!(r.status(), 200, "the health check needs no token");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_external_target_needs_its_key_and_an_acknowledgement_of_the_cost() {
    let env = Env::start().await;
    let target = json!({
        "name": "third party",
        "product": "third_party",
        "definition": { "base_url": "http://127.0.0.1:9/", "model": "gpt-x" },
        "declared": { "notes": "a laptop" },
    });
    let (status, t) = env.post("/targets", target.clone()).await;
    assert_eq!(status, 201, "{t}");
    assert_eq!(t["definition"]["paid"], true, "paid unless said otherwise");
    assert_eq!(t["definition"]["base_url"], "http://127.0.0.1:9");
    let (status, _) = env.post("/targets", target).await;
    assert_eq!(status, 409, "names are unique");
    let tid = t["id"].as_str().unwrap();
    let suite = json!({ "yaml": SUITE });
    let start = |extra: Value| {
        let mut body = json!({ "suite": suite, "target_id": tid });
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        body
    };
    let (status, e) = env.post("/runs", start(json!({}))).await;
    assert_eq!(
        (status, e["error"]["message"].as_str().unwrap()),
        (422, "an external target needs its key for this run")
    );
    let (status, e) = env.post("/runs", start(json!({ "key": "sk-x" }))).await;
    assert_eq!((status, e["error"]["message"].as_str().unwrap()), (422, "confirm that the calls may cost money"));
    let (status, e) = env.post("/runs", start(json!({ "key": "sk-x", "acknowledge_cost": true }))).await;
    assert_eq!(status, 422);
    assert!(e["error"]["message"].as_str().unwrap().contains("load.max_calls"), "{e}");
    let (status, _) = env.post("/runs", json!({ "suite": suite, "target_id": "cfg-gateway", "key": "x" })).await;
    assert_eq!(status, 422, "a managed gateway takes no pasted key");
    // An axis the benchmark cannot set on an external target is refused at plan time.
    let mut s = SUITE.to_owned();
    s.push_str("  workspace.cache: [\"off\", enforce]\n");
    let (status, e) = env.post("/plan", json!({ "suite": { "yaml": s }, "target_id": tid })).await;
    assert_eq!(status, 422, "{e}");
    // The plan notes that the target declares no mock.
    let (_, plan) = env.post("/plan", json!({ "suite": { "yaml": SUITE }, "target_id": tid })).await;
    assert!(plan["notes"].to_string().contains("mock_undeclared"), "{plan}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_run_cancelled_while_it_goes_takes_its_scene_down_and_a_queued_one_never_starts() {
    let env = Env::start().await;
    let long = SUITE
        .replace("{ duration_s: 2, target: 1 }", "{ duration_s: 30, target: 1 }")
        .replace("  load.target: [1, 2]\n", "  load.target: [1]\n");
    let (_, a) = env.post("/runs", json!({ "suite": { "yaml": long }, "target_id": "cfg-gateway" })).await;
    let (_, b) = env.post("/runs", json!({ "suite": { "yaml": long }, "target_id": "cfg-gateway" })).await;
    let (a, b) = (a["id"].as_str().unwrap().to_owned(), b["id"].as_str().unwrap().to_owned());
    // Wait until the first run's target cell goes.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let (_, page) = env.get(&format!("/runs/{a}")).await;
        if page["cells"].as_array().unwrap().iter().any(|c| c["kind"] == "target" && c["status"] == "running") {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "the target cell never ran");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let (status, _) = env.post(&format!("/runs/{b}/cancel"), json!({})).await;
    assert_eq!(status, 200);
    let (status, _) = env.post(&format!("/runs/{a}/cancel"), json!({})).await;
    assert_eq!(status, 200);
    let page = env.until_ended(&a, Duration::from_secs(60)).await;
    assert_eq!(page["run"]["status"], "cancelled");
    assert_eq!(page["run"]["leftovers"], json!([]), "the scene is taken down on a cancel too");
    let (_, pb) = env.get(&format!("/runs/{b}")).await;
    assert_eq!(pb["run"]["status"], "cancelled");
    assert!(pb["run"]["started_at"].is_null(), "the queued run never started");
    // A key was made for the first run's target cell, and deleted.
    let writes = env.fake.writes();
    let keys = writes.iter().filter(|(m, p, _)| m == "POST" && p.ends_with("/keys")).count();
    let deleted = writes.iter().filter(|(m, p, _)| m == "DELETE" && p.contains("/keys/")).count();
    assert_eq!((keys, deleted), (1, 1));
}
