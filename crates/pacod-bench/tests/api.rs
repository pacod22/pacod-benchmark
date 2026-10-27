//! The API end to end: a database of its own on the test PostgreSQL (`make test-services`, or
//! BENCH_TEST_DATABASE_URL), pacod-gw's agent and mock in process, and a fake of the gateway's
//! management API. Callers use the development token.

use std::sync::Arc;
use std::time::Duration;

use pacod_bench::config::{Auth, Config, InternalGw, ManagedGw, Publish};
use pacod_bench::secrets::{Kek, Keys};
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

/// The API of one started service.
struct Api {
    url: String,
    http: reqwest::Client,
    _stop: watch::Sender<bool>,
}

struct Env {
    api: Api,
    fake: Arc<FakeGw>,
    /// The fake gateway's management API.
    admin: String,
    config: Config,
    _mock: MockProvider,
}

impl std::ops::Deref for Env {
    type Target = Api;
    fn deref(&self) -> &Api {
        &self.api
    }
}

fn keys() -> Keys {
    Keys::new(Kek::new("test", [7; 32]), vec![])
}

async fn serve_app(config: Config) -> anyhow::Result<Api> {
    let (stop, stop_rx) = watch::channel(false);
    let app = pacod_bench::app(Arc::new(config), stop_rx).await?;
    let addr = serve(app.api).await;
    Ok(Api { url: format!("http://{addr}/api/v1"), http: reqwest::Client::new(), _stop: stop })
}

impl Env {
    async fn start() -> Env {
        Env::with(|_| {}).await
    }

    /// A service with a gateway of its configuration (`cfg-gateway`, the fake), the keys that
    /// seal tokens, and an agent in process; `adjust` changes the configuration first.
    async fn with(adjust: impl FnOnce(&mut Config)) -> Env {
        let mock = MockProvider::spawn().await;
        let agent = spawn_agent(&mock.base_url()).await;
        let (fake, admin) = FakeGw::start().await;
        let mut config = Config {
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
            publish: Some(Publish { url: admin.clone(), token: "pidt_test".into() }),
            console_url: Some("http://console.localhost".into()),
            keys: Some(keys()),
            internal: InternalGw {
                admin_url: admin.clone(),
                mock_url: "http://gw-load-mock:9000".into(),
                data_plane: None,
            },
        };
        adjust(&mut config);
        let api = serve_app(config.clone()).await.unwrap();
        Env { api, fake, admin, config, _mock: mock }
    }

    /// The same service started again on the same database, its configuration changed by
    /// `adjust`.
    async fn again(&self, adjust: impl FnOnce(&mut Config)) -> anyhow::Result<Api> {
        let mut config = self.config.clone();
        adjust(&mut config);
        serve_app(config).await
    }
}

impl Api {
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

    async fn put(&self, path: &str, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::PUT, path, Some(body)).await
    }

    async fn delete(&self, path: &str) -> (u16, Value) {
        self.call(reqwest::Method::DELETE, path, None).await
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

const TOKEN: &str = "pgwa_a-gateway-token-of-the-ui-0123";

/// A gateway added through the UI, on the fake gateway.
fn gateway(name: &str, admin: &str, token: &str) -> Value {
    json!({
        "name": name,
        "kind": "managed",
        "definition": {
            "admin_url": format!("{admin}/"),
            "mock_url": "http://gw-load-mock:9000",
            "data_plane": "",
            "console_url": "http://eu-console.localhost",
        },
        "declared": { "layout": "helm" },
        "token": token,
    })
}

/// A suite whose one target cell runs for `seconds`.
fn long_suite(seconds: u32) -> Value {
    let yaml = SUITE
        .replace("{ duration_s: 2, target: 1 }", &format!("{{ duration_s: {seconds}, target: 1 }}"))
        .replace("  load.target: [1, 2]\n", "  load.target: [1]\n");
    json!({ "yaml": yaml })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_gateway_added_from_the_ui_takes_runs_and_never_shows_its_token() {
    // No gateway of the configuration and no publish destination: the one added here is both.
    let env = Env::with(|c| {
        c.gateways.clear();
        c.publish = None;
    })
    .await;
    let (_, presets) = env.get("/targets/presets").await;
    assert_eq!(presets["can_keep_tokens"], true);
    assert_eq!(presets["internal"]["admin_url"], env.admin);
    assert!(presets["internal"]["taken_by"].is_null());

    // A token the gateway refuses, and one that is no platform admin's.
    let (status, e) = env.post("/targets", gateway("eu", &env.admin, "pgwa_refused_0123456789")).await;
    assert_eq!((status, e["error"]["message"].as_str().unwrap()), (422, "the gateway refuses this token"));
    let (status, e) = env.post("/targets", gateway("eu", &env.admin, "pgwa_viewer_0123456789")).await;
    assert_eq!(status, 422);
    assert!(e["error"]["message"].as_str().unwrap().contains("no platform admin"), "{e}");
    assert!(!e.to_string().contains("pgwa_viewer"), "a refusal does not echo the token");
    let (status, e) = env.post("/targets", json!({ "name": "eu", "kind": "managed", "definition": { "admin_url": env.admin, "mock_url": "http://m:9000" } })).await;
    assert_eq!((status, e["error"]["message"].as_str().unwrap()), (422, "a gateway needs its platform-admin token"));

    let (status, t) = env.post("/targets", gateway("eu", &env.admin, TOKEN)).await;
    assert_eq!(status, 201, "{t}");
    let id = t["id"].as_str().unwrap().to_owned();
    assert_eq!(
        (t["kind"].as_str(), t["source"].as_str(), t["product"].as_str()),
        (Some("managed"), Some("api"), Some("pacod_gw"))
    );
    assert_eq!(t["definition"]["admin_url"], env.admin, "without its trailing slash");
    assert!(t["definition"]["data_plane"].is_null(), "an empty address is none");
    assert_eq!(t["usable"], true);
    assert_eq!(t["token_checked"]["display"], "pacod-bench");
    assert!(t["token_hint"].as_str().unwrap().starts_with("…0123 ("), "{t}");
    assert_eq!(t["observed"]["status"]["software_version"], "0.1.0-test", "the fingerprint is read at once");
    let (_, presets) = env.get("/targets/presets").await;
    assert_eq!(presets["internal"]["taken_by"]["id"], id.as_str());
    let (_, session) = env.get("/session").await;
    assert_eq!((session["gateways"].clone(), session["publish"].clone()), (json!(["eu"]), json!(true)));

    // A run on it, published to it: the only gateway.
    let (status, run) = env.post("/runs", json!({ "suite": { "yaml": SUITE }, "target_id": id })).await;
    assert_eq!(status, 201, "{run}");
    let run_id = run["id"].as_str().unwrap().to_owned();
    let page = env.until_ended(&run_id, Duration::from_secs(120)).await;
    assert_eq!(page["run"]["status"], "finished", "{}", page["run"]["error"]);
    assert_eq!(page["run"]["leftovers"], json!([]));
    let (status, published) = env.post(&format!("/runs/{run_id}/publish"), json!({})).await;
    assert_eq!(status, 200, "{published}");
    assert!(
        published["link"].as_str().unwrap().starts_with("http://eu-console.localhost/platform/testing/benchmarks/")
    );
    let report = String::from_utf8(env.fake.reports.lock()[0].clone()).unwrap();

    // The token is in no answer, no copy a run keeps, no report.
    let (_, list) = env.get("/targets").await;
    let (_, one) = env.get(&format!("/targets/{id}")).await;
    for text in [t.to_string(), list.to_string(), one.to_string(), page.to_string(), report] {
        assert!(!text.contains(TOKEN) && !text.contains("sealed") && !text.contains("ciphertext"), "{text}");
    }

    // A new management API needs the token again; a new token alone is checked again.
    let mut moved = gateway("eu", "http://elsewhere:8081", TOKEN);
    moved.as_object_mut().unwrap().remove("token");
    let (status, e) = env.put(&format!("/targets/{id}"), moved).await;
    assert_eq!(status, 422);
    assert!(e["error"]["message"].as_str().unwrap().contains("needs the token again"), "{e}");
    let (status, _) = env.put(&format!("/targets/{id}"), gateway("eu", &env.admin, "pgwa_viewer_x0123456789")).await;
    assert_eq!(status, 422, "a replaced token is checked like a new one");
    let mut renamed = gateway("eu-helm", &env.admin, TOKEN);
    renamed.as_object_mut().unwrap().remove("token");
    let (status, t) = env.put(&format!("/targets/{id}"), renamed).await;
    assert_eq!(status, 200, "{t}");
    assert_eq!((t["name"].as_str(), t["usable"].as_bool()), (Some("eu-helm"), Some(true)));

    // A check asks the gateway who the token is again.
    let (status, check) = env.post(&format!("/targets/{id}/check"), json!({})).await;
    assert_eq!(status, 200, "{check}");
    assert_eq!(check["token"]["platform_admin"], true);
    assert_eq!(check["probe"]["leftovers"], json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_gateway_in_use_is_neither_removed_nor_moved() {
    let env = Env::start().await;
    let (_, t) = env.post("/targets", gateway("eu", &env.admin, TOKEN)).await;
    let id = t["id"].as_str().unwrap().to_owned();
    let (_, run) = env.post("/runs", json!({ "suite": long_suite(30), "target_id": id })).await;
    let run_id = run["id"].as_str().unwrap().to_owned();
    let (status, e) = env.delete(&format!("/targets/{id}")).await;
    assert_eq!(status, 409, "{e}");
    assert!(e["error"]["message"].as_str().unwrap().contains(&run_id));
    let mut other_mock = gateway("eu", &env.admin, TOKEN);
    other_mock["definition"]["mock_url"] = json!("http://other-mock:9000");
    other_mock.as_object_mut().unwrap().remove("token");
    let (status, _) = env.put(&format!("/targets/{id}"), other_mock).await;
    assert_eq!(status, 409, "the mock changes under no queued or going run");
    let mut declared = gateway("eu", &env.admin, TOKEN);
    declared["declared"] = json!({ "layout": "scale" });
    declared.as_object_mut().unwrap().remove("token");
    let (status, _) = env.put(&format!("/targets/{id}"), declared).await;
    assert_eq!(status, 200, "what is declared may change at any time");

    env.post(&format!("/runs/{run_id}/cancel"), json!({})).await;
    env.until_ended(&run_id, Duration::from_secs(60)).await;
    let (status, _) = env.delete(&format!("/targets/{id}")).await;
    assert_eq!(status, 204);
    let (status, _) = env.post("/runs", json!({ "suite": { "yaml": SUITE }, "target_id": id })).await;
    assert_eq!(status, 404);
    let (status, _) = env.delete("/targets/cfg-gateway").await;
    assert_eq!(status, 422, "a gateway of the configuration is removed there");
    let (status, _) = env.post("/targets", gateway("gateway", &env.admin, TOKEN)).await;
    assert_eq!(status, 409, "names are unique across the configuration and the UI");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_a_key_no_gateway_is_added() {
    let env = Env::with(|c| c.keys = None).await;
    let (_, presets) = env.get("/targets/presets").await;
    assert_eq!(presets["can_keep_tokens"], false);
    let (status, e) = env.post("/targets", gateway("eu", &env.admin, TOKEN)).await;
    assert_eq!(status, 409);
    assert!(e["error"]["message"].as_str().unwrap().starts_with("BENCH_KEK is not set"), "{e}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_gateway_outlives_a_restart_and_what_an_interrupted_run_left_is_taken_down() {
    let env = Env::start().await;
    let (_, t) = env.post("/targets", gateway("eu", &env.admin, TOKEN)).await;
    let id = t["id"].as_str().unwrap().to_owned();
    // A run that was going on that gateway when the service stopped.
    let db = sqlx::PgPool::connect(&env.config.database_url).await.unwrap();
    sqlx::query(
        "INSERT INTO runs (id, status, suite, target_id, target, agents, plan, started_by, scene)
         VALUES ('run_gone', 'running', '{}', $1, '{\"kind\": \"managed\"}', '{}', '{}', 'dev',
                 '{\"org\": \"org9\", \"workspaces\": [], \"ledger\": [\"/orgs/org9\"]}')",
    )
    .bind(&id)
    .execute(&db)
    .await
    .unwrap();

    // Without its key, the gateway is listed but takes no run, and the leftovers wait.
    let without = env.again(|c| c.keys = None).await.unwrap();
    let (_, one) = without.get(&format!("/targets/{id}")).await;
    assert_eq!(one["usable"], false);
    assert!(one["unusable_reason"].as_str().unwrap().contains("BENCH_KEK"), "{one}");
    let (status, _) = without.post("/runs", json!({ "suite": { "yaml": SUITE }, "target_id": id })).await;
    assert_eq!(status, 409);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (_, page) = without.get("/runs/run_gone").await;
        if page["run"]["leftovers"] == json!(["/orgs/org9"]) {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "the leftovers are not recorded: {page}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let (status, _) = without.delete(&format!("/targets/{id}")).await;
    assert_eq!(status, 409, "a gateway with leftovers stays");

    // With its key again, the next start opens the token and cleans up.
    let with = env.again(|_| {}).await.unwrap();
    let (_, one) = with.get(&format!("/targets/{id}")).await;
    assert_eq!(one["usable"], true, "{one}");
    let (status, cleaned) = with.post("/leftovers/clean", json!({})).await;
    assert_eq!((status, cleaned["left"].as_u64()), (200, Some(0)), "{cleaned}");
    assert!(env.fake.writes().iter().any(|(m, p, _)| m == "DELETE" && p == "/orgs/org9"));
    let (_, page) = with.get("/runs/run_gone").await;
    assert_eq!(page["run"]["leftovers"], json!([]));

    // A gateway of the configuration may not take a name the UI already gave.
    let collision = env
        .again(|c| {
            let mut g = c.gateways[0].clone();
            g.name = "eu".into();
            c.gateways.push(g);
        })
        .await;
    assert!(collision.err().unwrap().to_string().contains("rename one of them"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_settings_stay_within_the_environment_and_go_with_each_run() {
    let env = Env::start().await;
    let (_, s) = env.get("/settings").await;
    assert_eq!(s["environment"]["max_cells"], 64);
    assert_eq!(s["overrides"], json!({}));
    let (status, e) = env.put("/settings", json!({ "max_cells": 65 })).await;
    assert_eq!(status, 422);
    assert!(e["error"]["message"].as_str().unwrap().contains("BENCH_MAX_CELLS"), "{e}");
    let (status, _) = env.put("/settings", json!({ "default_agents": ["nobody"] })).await;
    assert_eq!(status, 422);
    let (status, _) = env.put("/settings", json!({ "publish_target_id": "nothing" })).await;
    assert_eq!(status, 422);

    // A limit of the settings holds for plans.
    let (status, s) = env.put("/settings", json!({ "max_cells": 1, "agent_wait_s": 2 })).await;
    assert_eq!(status, 200, "{s}");
    assert_eq!((s["effective"]["max_cells"].as_u64(), s["effective"]["agent_wait_s"].as_u64()), (Some(1), Some(2)));
    let (status, _) = env.post("/plan", json!({ "suite": { "yaml": SUITE }, "target_id": "cfg-gateway" })).await;
    assert_eq!(status, 422, "two cells, one allowed");
    let (_, session) = env.get("/session").await;
    assert_eq!(session["limits"]["max_cells"], 1);

    // A queued run keeps the settings it was started with.
    let (status, _) = env.put("/settings", json!({ "agent_wait_s": 3 })).await;
    assert_eq!(status, 200);
    let (_, a) = env.post("/runs", json!({ "suite": long_suite(20), "target_id": "cfg-gateway" })).await;
    let (_, b) = env.post("/runs", json!({ "suite": long_suite(20), "target_id": "cfg-gateway" })).await;
    let (a, b) = (a["id"].as_str().unwrap().to_owned(), b["id"].as_str().unwrap().to_owned());
    env.put("/settings", json!({ "agent_wait_s": 4 })).await;
    let (_, pb) = env.get(&format!("/runs/{b}")).await;
    assert_eq!(pb["run"]["status"], "queued");
    assert_eq!(pb["run"]["settings"]["agent_wait_s"], 3);
    for id in [&b, &a] {
        env.post(&format!("/runs/{id}/cancel"), json!({})).await;
    }
    env.until_ended(&a, Duration::from_secs(60)).await;

    // Reports go to the gateway the settings name.
    let (_, t) = env.post("/targets", gateway("eu", &env.admin, TOKEN)).await;
    let tid = t["id"].as_str().unwrap().to_owned();
    let (status, _) = env.put("/settings", json!({ "publish_target_id": tid })).await;
    assert_eq!(status, 200);
    let (_, run) = env.post("/runs", json!({ "suite": { "yaml": SUITE }, "target_id": "cfg-gateway" })).await;
    let run_id = run["id"].as_str().unwrap().to_owned();
    env.until_ended(&run_id, Duration::from_secs(120)).await;
    let (status, published) = env.post(&format!("/runs/{run_id}/publish"), json!({})).await;
    assert_eq!(status, 200, "{published}");
    assert!(published["link"].as_str().unwrap().starts_with("http://eu-console.localhost/"));
    let (status, _) = env.delete(&format!("/targets/{tid}")).await;
    assert_eq!(status, 409, "reports go there");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agents_added_from_the_ui_take_part_in_runs() {
    let env = Env::start().await;
    let second = spawn_agent(&env._mock.base_url()).await;
    let (status, _) = env.post("/agents", json!({ "name": "local", "url": second })).await;
    assert_eq!(status, 409, "the configuration has a local already");
    let (status, _) = env.post("/agents", json!({ "name": "Eu 1", "url": second })).await;
    assert_eq!(status, 422);
    let (status, _) = env.post("/agents", json!({ "name": "eu-1", "url": "ftp://x" })).await;
    assert_eq!(status, 422);
    let (status, a) =
        env.post("/agents", json!({ "name": "eu-1", "url": format!("{second}/"), "placement": "remote" })).await;
    assert_eq!(status, 201, "{a}");
    assert_eq!((a["url"].as_str(), a["placement"].as_str()), (Some(second.as_str()), Some("remote")));
    let (_, list) = env.get("/agents").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!((items[1]["source"].as_str(), items[1]["reachable"].as_bool()), (Some("api"), Some(true)));
    let (status, _) = env.put("/agents/local", json!({ "url": second })).await;
    assert_eq!(status, 422, "an agent of the configuration changes there");

    // A run on the new agent alone.
    let (status, run) =
        env.post("/runs", json!({ "suite": { "yaml": SUITE }, "target_id": "cfg-gateway", "agents": ["eu-1"] })).await;
    assert_eq!(status, 201, "{run}");
    let run_id = run["id"].as_str().unwrap().to_owned();
    let (status, e) = env.delete("/agents/eu-1").await;
    assert_eq!(status, 409, "{e}");
    assert!(e["error"]["message"].as_str().unwrap().contains(&run_id), "{e}");
    let page = env.until_ended(&run_id, Duration::from_secs(120)).await;
    assert_eq!(page["run"]["status"], "finished", "{}", page["run"]["error"]);
    assert_eq!(page["run"]["agents"]["agents"][0]["placement"], "remote");

    // While a run that waits uses it, the agent stays as it is.
    let (_, long) =
        env.post("/runs", json!({ "suite": long_suite(20), "target_id": "cfg-gateway", "agents": ["eu-1"] })).await;
    let long = long["id"].as_str().unwrap().to_owned();
    let (status, _) = env.put("/agents/eu-1", json!({ "url": second, "placement": "same_host" })).await;
    assert_eq!(status, 409);
    let (status, _) = env.delete("/agents/eu-1").await;
    assert_eq!(status, 409);
    env.post(&format!("/runs/{long}/cancel"), json!({})).await;
    env.until_ended(&long, Duration::from_secs(60)).await;
    let (status, _) = env.put("/settings", json!({ "default_agents": ["eu-1"] })).await;
    assert_eq!(status, 200);
    let (status, _) = env.delete("/agents/eu-1").await;
    assert_eq!(status, 409, "a default agent of the settings stays");
    env.put("/settings", json!({})).await;
    let (status, _) = env.delete("/agents/eu-1").await;
    assert_eq!(status, 204);
    let (_, session) = env.get("/session").await;
    assert_eq!(session["agents"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_queue_is_reordered_and_a_run_runs_again_with_the_same_keys() {
    let env = Env::start().await;
    let (_, going) =
        env.post("/runs", json!({ "suite": long_suite(30), "target_id": "cfg-gateway", "label": "going" })).await;
    let going = going["id"].as_str().unwrap().to_owned();
    // The first run goes before the others are queued, so that the queue holds them alone.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while env.get("/queue").await.1["active"]["id"] != going.as_str() {
        assert!(tokio::time::Instant::now() < deadline, "the first run never started");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut queued = Vec::new();
    for label in ["a", "b", "c"] {
        let (_, r) =
            env.post("/runs", json!({ "suite": { "yaml": SUITE }, "target_id": "cfg-gateway", "label": label })).await;
        queued.push(r["id"].as_str().unwrap().to_owned());
    }
    let order = |q: &Value| -> Vec<String> {
        q["queued"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap().to_owned()).collect()
    };
    let (_, q) = env.get("/queue").await;
    assert_eq!(order(&q), ["a", "b", "c"]);
    let (status, q) = env.post(&format!("/runs/{}/move", queued[2]), json!({ "to": "top" })).await;
    assert_eq!(status, 200, "{q}");
    assert_eq!(order(&q), ["c", "a", "b"]);
    let (_, q) = env.post(&format!("/runs/{}/move", queued[0]), json!({ "to": "down" })).await;
    assert_eq!(order(&q), ["c", "b", "a"]);
    let (_, q) = env.post(&format!("/runs/{}/move", queued[0]), json!({ "to": "up" })).await;
    assert_eq!(order(&q), ["c", "a", "b"]);
    let (status, _) = env.post(&format!("/runs/{going}/move"), json!({ "to": "top" })).await;
    assert_eq!(status, 409, "only queued runs move");

    // The first in the queue is the next to start.
    for id in [&queued[0], &queued[1]] {
        env.post(&format!("/runs/{id}/cancel"), json!({})).await;
    }
    env.post(&format!("/runs/{going}/cancel"), json!({})).await;
    let first = env.until_ended(&queued[2], Duration::from_secs(120)).await;
    assert_eq!(first["run"]["status"], "finished", "{}", first["run"]["error"]);

    // Again: the same suite, target and agents; the same comparison keys.
    let (status, again) = env.post(&format!("/runs/{}/rerun", queued[2]), json!({})).await;
    assert_eq!(status, 201, "{again}");
    assert_eq!(again["label"], "c");
    let again = env.until_ended(again["id"].as_str().unwrap(), Duration::from_secs(120)).await;
    let keys = |page: &Value| -> Vec<String> {
        page["cells"].as_array().unwrap().iter().map(|c| c["comparison_key"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(keys(&again), keys(&first));
    assert_eq!(again["run"]["agents"]["names"], first["run"]["agents"]["names"]);

    // A suite copied is a suite of its own.
    let (_, suite) = env.post("/suites", json!({ "yaml": SUITE })).await;
    let (status, copy) = env.post(&format!("/suites/{}/copy", suite["id"].as_str().unwrap()), json!({})).await;
    assert_eq!(status, 201, "{copy}");
    assert_ne!(copy["id"], suite["id"]);
    assert_eq!(copy["name"], "managed, two load levels (copy)");
    assert_eq!(copy["document"]["name"], copy["name"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_schedule_fires_publishes_and_never_makes_up_for_a_missed_fire() {
    let env = Env::start().await;
    let (_, suite) = env.post("/suites", json!({ "yaml": SUITE })).await;
    let suite_id = suite["id"].as_str().unwrap().to_owned();
    let (_, external) = env
        .post(
            "/targets",
            json!({ "name": "elsewhere", "definition": { "base_url": "http://127.0.0.1:9", "model": "m" } }),
        )
        .await;
    let every = json!({ "kind": "every", "hours": 6 });
    let (status, e) = env
        .post("/schedules", json!({ "name": "x", "suite_id": suite_id, "target_id": external["id"], "cadence": every }))
        .await;
    assert_eq!(status, 422, "an external target's key is never kept: {e}");
    let (status, _) = env
        .post("/schedules", json!({ "name": "x", "suite_id": suite_id, "target_id": "cfg-gateway", "cadence": { "kind": "every", "hours": 0 } }))
        .await;
    assert_eq!(status, 422);

    let (status, s) = env
        .post(
            "/schedules",
            json!({ "name": "nightly", "suite_id": suite_id, "target_id": "cfg-gateway", "cadence": every, "publish": true, "agents": ["local"] }),
        )
        .await;
    assert_eq!(status, 201, "{s}");
    let sid = s["id"].as_str().unwrap().to_owned();
    let next: chrono::DateTime<chrono::Utc> = s["next_fire_at"].as_str().unwrap().parse().unwrap();
    assert!(next > chrono::Utc::now() + chrono::TimeDelta::hours(5), "{next}");

    // What it holds on to stays.
    let (status, _) = env.delete(&format!("/suites/{suite_id}")).await;
    assert_eq!(status, 409);
    let (status, _) = env.put("/targets/cfg-gateway", json!({ "name": "gateway", "declared": {} })).await;
    assert_eq!(status, 200, "a gateway's declared fingerprint changes under a schedule");

    // Fired by hand: a run of the schedule, then no second one while it waits or goes.
    let (status, fired) = env.post(&format!("/schedules/{sid}/fire"), json!({})).await;
    assert_eq!(status, 200, "{fired}");
    let run_id = fired["run_id"].as_str().unwrap().to_owned();
    let (status, again) = env.post(&format!("/schedules/{sid}/fire"), json!({})).await;
    assert_eq!(status, 409, "{again}");
    assert!(again["error"]["message"].as_str().unwrap().contains("still"), "{again}");
    let page = env.until_ended(&run_id, Duration::from_secs(120)).await;
    assert_eq!(page["run"]["status"], "finished", "{}", page["run"]["error"]);
    assert_eq!(
        (page["run"]["schedule_id"].as_str(), page["run"]["label"].as_str()),
        (Some(sid.as_str()), Some("schedule: nightly"))
    );
    // Published when it finished.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let (_, page) = env.get(&format!("/runs/{run_id}")).await;
        if let Some(p) = page["publications"].as_array().and_then(|p| p.first()) {
            assert_eq!(p["status"], "published", "{p}");
            assert_eq!(p["published_by"], "development (schedule nightly)");
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "the report was not published");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // A fire missed while the benchmark was stopped is skipped; a due one fires at the next start.
    let db = sqlx::PgPool::connect(&env.config.database_url).await.unwrap();
    let set_next = |ago_min: i64| {
        sqlx::query("UPDATE schedules SET next_fire_at = now() - make_interval(mins => $2) WHERE id = $1")
            .bind(sid.clone())
            .bind(ago_min as i32)
    };
    set_next(60).execute(&db).await.unwrap();
    let restarted = env.again(|_| {}).await.unwrap();
    let path = format!("/schedules/{sid}");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let s = loop {
        let s = restarted.get(&path).await.1;
        if s["last_outcome"].as_str().unwrap().contains("missed") {
            break s;
        }
        assert!(tokio::time::Instant::now() < deadline, "the missed fire was not seen: {s}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert_eq!(s["last_run_id"], run_id.as_str(), "no run for a missed fire");
    let next: chrono::DateTime<chrono::Utc> = s["next_fire_at"].as_str().unwrap().parse().unwrap();
    assert!(next > chrono::Utc::now(), "the next fire is ahead again");

    // A schedule whose suite is gone switches itself off.
    sqlx::query("DELETE FROM suites WHERE id = $1").bind(&suite_id).execute(&db).await.unwrap();
    set_next(1).execute(&db).await.unwrap();
    let restarted = env.again(|_| {}).await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let s = restarted.get(&path).await.1;
        if s["enabled"] == false {
            assert!(s["last_outcome"].as_str().unwrap().contains("the suite is gone"), "{s}");
            assert!(s["next_fire_at"].is_string(), "{s}");
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "the schedule is still on: {s}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let (status, _) = restarted.delete(&format!("/schedules/{sid}")).await;
    assert_eq!(status, 204);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_schedule_holds_its_target_and_agents() {
    let env = Env::start().await;
    let (_, suite) = env.post("/suites", json!({ "yaml": SUITE })).await;
    let (_, t) = env.post("/targets", gateway("eu", &env.admin, TOKEN)).await;
    let tid = t["id"].as_str().unwrap().to_owned();
    let second = spawn_agent(&env._mock.base_url()).await;
    env.post("/agents", json!({ "name": "eu-1", "url": second })).await;
    let body = json!({
        "name": "daily", "suite_id": suite["id"], "target_id": tid, "agents": ["eu-1"],
        "cadence": { "kind": "daily", "at": "02:30", "weekdays": [1, 2, 3, 4, 5] },
    });
    let (status, s) = env.post("/schedules", body.clone()).await;
    assert_eq!(status, 201, "{s}");
    let sid = s["id"].as_str().unwrap().to_owned();
    let (status, e) = env.delete(&format!("/targets/{tid}")).await;
    assert_eq!(status, 409);
    assert!(e["error"]["message"].as_str().unwrap().contains("daily"), "{e}");
    let (status, _) = env.delete("/agents/eu-1").await;
    assert_eq!(status, 409);
    // Switched off, it no longer holds the gateway; it still names the agent.
    let mut off = body.clone();
    off["enabled"] = json!(false);
    let (status, s) = env.put(&format!("/schedules/{sid}"), off).await;
    assert_eq!(status, 200, "{s}");
    assert!(s["next_fire_at"].is_null());
    let (status, _) = env.delete("/agents/eu-1").await;
    assert_eq!(status, 409);
    let (status, _) = env.delete(&format!("/schedules/{sid}")).await;
    assert_eq!(status, 204);
    let (status, _) = env.delete(&format!("/targets/{tid}")).await;
    assert_eq!(status, 204);
}
