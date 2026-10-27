# pacod-benchmark runbook

How to run pacod-benchmark, connect it to gateways and agents, and read what it measures.
[architecture.md](architecture.md) describes how it works; [suite-format.md](suite-format.md)
the suites.

## 1. What runs

| Service | What | Listens |
|---|---|---|
| `bench-api` | `pacod-bench serve`: the API, the run queue, the database `bench` | `bench-api-internal:8190` on pacod-admin-net; metrics on 9190 |
| `bench-ui` | The UI (SvelteKit, adapter-node) behind Caddy | 3000 |
| `bench-load-agent` | Standalone stack only: pacod-gw's load agent | 8096 |
| `bench-load-mock` | Standalone stack only: pacod-gw's mock provider, keeping no requests | 9000 |
| `bench-gw-lite` | Profile `dev-gateway`: pacod-gw-lite in front of `bench-load-mock` | 8080 |

One run goes at a time; runs asked for while one goes wait in a queue, which the Runs page shows
and reorders (to the front, up, down). **Run again** on a finished run queues the same suite (the
run's copy of it) on the same target with the same agents, under today's settings: its cells have
the same comparison keys as the first run's.

**Schedules** (the Schedules page) start a suite on a gateway the benchmark manages on a
timetable, in UTC: daily at a time on chosen weekdays, or every few hours. Whoever may start runs
makes one; its creator or an admin changes it. A schedule fires with its creator's authority: at
every fire the benchmark asks its copy of pacod-id's roles whether the creator may still start
runs, and switches the schedule off when not (and waits while that copy is not current). A fire
is skipped while the schedule's previous run still waits or goes, and a fire missed by more than
15 minutes (the benchmark was stopped) is skipped, not made up for. A schedule that publishes
sends the report to pacod-gw when its run has finished, if its creator may still publish reports. External targets take no schedule: their
key is never kept. A schedule holds its suite, its gateway and its agents: none of them is
removed until it changes. A run's scene on a
gateway is taken down when it ends, keys first, also when it is cancelled. When `bench-api`
stops during a run, the agents stop on their own (their lease runs out) and the next start takes
down what the run had made.

## 2. Configuration of bench-api

Secrets come from the variable, or from the file `<NAME>_FILE` names; both set is refused.

| Variable | Meaning |
|---|---|
| `DATABASE_URL` | PostgreSQL (`postgres://bench:…@pacod-postgres:5432/bench`) |
| `BENCH_LISTEN` | `host:port` of the API; a host name binds to its address (`bench-api-internal:8190`). Default `0.0.0.0:8190` |
| `BENCH_METRICS_LISTEN` | `/metrics`; `off` for none. Default `0.0.0.0:9190` |
| `BENCH_AUTH` | `oidc` (default): callers bring pacod-id tokens. `dev`: one static token (`BENCH_DEV_TOKEN`, at least 32 characters) stands for an admin; development only |
| `BENCH_ID_ISSUER` | pacod-id's public issuer(s), comma-separated, as the tokens' `iss` says |
| `BENCH_ID_INTERNAL_URL` | pacod-id's internal listener (`http://pacod-id-internal:8071`) |
| `BENCH_ID_CLIENT_ID`, `BENCH_ID_CLIENT_SECRET` | The service client (`pacod-bench-service`, product `bench`) |
| `BENCH_ID_AUDIENCE` | The tokens' audience. Default `pacod-bench` |
| `BENCH_GW_ADMIN_URL` | The management API of the gateway the benchmark manages (`http://pacod-gw-admin:8081`); none: no managed gateway |
| `BENCH_GW_ADMIN_TOKEN` | A platform-admin token of that gateway (section 3) |
| `BENCH_GW_NAME` | The managed gateway's name among the targets. Default `gateway` |
| `BENCH_LOAD_MOCK_URL` | The mock as that gateway reaches it (`http://gw-load-mock:9000`); it must be in the gateway's `PGW_EGRESS_ALLOW_PRIVATE` |
| `BENCH_GW_DATA_PLANE` | The data plane as the agents reach it; none: each agent's own target |
| `BENCH_GW_DECLARED` | What only you know about that deployment, as a JSON object: `{"layout": "scale", "arch": "amd64", "cpus": 16}` |
| `BENCH_GW_TARGETS` | A TOML file of further managed gateways (section 4) |
| `BENCH_KEK` | The key the tokens of gateways added through the UI are sealed under: base64 of 32 bytes (`openssl rand -base64 32`). None: no gateway is added there (section 3) |
| `BENCH_KEK_ID`, `BENCH_KEK_OLD` | The key's id (default `default`; the month suits, `2026-10`), and older keys as `id=base64,…`, which still open tokens sealed before a rotation |
| `BENCH_GW_INTERNAL_ADMIN_URL`, `BENCH_GW_INTERNAL_MOCK_URL`, `BENCH_GW_INTERNAL_DATA_PLANE` | The gateway of this stack, as the UI offers it when a gateway is added: default `http://pacod-gw-admin:8081`, `http://gw-load-mock:9000`, none |
| `BENCH_GW_CONSOLE_URL` | The gateway console's address, for links to published reports |
| `BENCH_PUBLISH_URL`, `BENCH_PUBLISH_TOKEN` | Where reports are published; default the configuration's gateway, else the one gateway the benchmark manages when there is one only |
| `BENCH_AGENTS` | The load agents, `name=URL,…` (the syntax of pacod-gw's `TESTER_LOAD_AGENTS`) |
| `BENCH_AGENT_TOKEN` | The agents' token (`LOAD_AGENT_TOKEN`), at least 32 characters |
| `BENCH_AGENT_PLACEMENT` | Where each agent stands, `name=in_stack|same_host|remote,…`; default `unknown` |
| `BENCH_BASELINE_MOCK_URL` | The mock as the agents reach it, for the baseline cells; none: they are skipped |
| `BENCH_MAX_CELLS`, `BENCH_MAX_DURATION`, `BENCH_AGENT_WAIT_S` | Limits: cells of a run (256), seconds of a run (14 400), seconds a cell waits for a busy agent (300) |

**Settings in the UI** (Settings, an admin): the limits of a run (cells, seconds, the wait for a
busy agent), the mock of the baseline cells, the agents a run uses when it names none, and the
gateway reports are published to. Each limit is at most the environment's
(`BENCH_MAX_CELLS`, `BENCH_MAX_DURATION`, `BENCH_AGENT_WAIT_S`), which becomes its ceiling; what
is left empty is the environment's. A run keeps the settings it was started with, so a change
while it waits leaves it alone. Neither the agent a setting names nor the gateway reports go to
is removed until the settings change.

The UI takes `BENCH_API_URL`, `BENCH_AUTH`, `BENCH_DEV_TOKEN`, `BENCH_ID_ISSUER`,
`BENCH_ID_INTERNAL_URL`, `BENCH_UI_CLIENT_ID` (`pacod-bench-ui`), `BENCH_UI_CLIENT_SECRET`,
`BENCH_UI_SESSION_SECRET` (at least 32 characters) and `BENCH_ORIGIN`, and adapter-node's
`PROTOCOL_HEADER=x-forwarded-proto`: without it form posts behind Caddy fail the CSRF check.

## 3. A token for the gateway

The benchmark sets each run's scene up through the gateway's management API: a scratch
organisation, a workspace per variant, keys, its own mock providers. That takes a platform
admin's token.

1. In the gateway's console, **Platform → Access → Admin tokens**: a token named `pacod-bench`.
2. Give it the role **platform admin** at the platform.
3. Either add the gateway in the benchmark's UI with it (below), or put it into
   `BENCH_GW_ADMIN_TOKEN` (or a file named by `BENCH_GW_ADMIN_TOKEN_FILE`) with
   `BENCH_GW_ADMIN_URL`.

With the management API (local sign-in shown; with pacod-id the console's page does the same):

```sh
curl -X POST "$GW/api/v1/admin-tokens" -H "authorization: Bearer $SESSION" -d '{"name":"pacod-bench"}'
curl -X POST "$GW/api/v1/role-bindings" -H "authorization: Bearer $SESSION" \
  -d '{"principal":"<id>","role":"platform_admin","scope":{"type":"platform"}}'
```

In pacod-demo the bootstrap makes this token (`make bootstrap`, its `scripts/bootstrap.py`) and
keeps it while the gateway takes it.

**A gateway added through the UI** (Targets → Add a gateway the benchmark manages, an admin):

- **The gateway of this stack** fills in its management API and mock from
  `BENCH_GW_INTERNAL_*`; it is not offered once a target is that gateway (in pacod-demo, the
  bootstrap's `demo-gateway`). **Another installation** takes any management API the benchmark
  reaches, the mock as that gateway reaches it, and the data plane as the agents reach it.
- The benchmark asks the gateway who the token is (`GET /api/v1/session`) and refuses one that is
  no platform admin's. Then it seals the token under `BENCH_KEK`, bound to the target and its
  management API ([threat-model.md](threat-model.md), section 2.1). Nobody reads it back; the
  target's page shows a label of it and whom the gateway takes it for.
- A new management API needs the token again; a new token alone replaces the old one. Neither,
  nor removing the gateway, while a run on it waits or goes, or left objects on it.
- **Rotating `BENCH_KEK`.** Set the new key in `BENCH_KEK` (a new `BENCH_KEK_ID`) and the old one
  in `BENCH_KEK_OLD`, restart, then run `pacod-bench rewrap-secrets` (the same environment) and
  drop the old key at the next restart. A gateway whose key is missing shows as unusable, with
  the reason.

A refusal of the token stops a run before anything is set up ("the gateway refuses the
benchmark's admin token"). What the token allows is the reason for the roles of section 7
([threat-model.md](threat-model.md)).

## 4. Targets

| Kind | What | How it is registered |
|---|---|---|
| A managed gateway | A pacod-gw whose scene the benchmark sets up: the one of this stack or another installation | The UI's Targets page, by an admin, with its token (section 3); or `BENCH_GW_ADMIN_URL` and `BENCH_GW_TARGETS`, read at every start and shown locked in the UI |
| An external target | pacod-gw-lite, another product: a URL and a model; the key is pasted for each run | The UI's Targets page, by an admin |

What only a deployment decides (analytics on or off, Valkey or memory, the scale layout, the
minimal gateway, the hardware) is no axis of a suite: register each such deployment as a target
of its own and run the same suite on each. Its fingerprint goes with each run: observed from the
gateway (version, instances, cache store, analytics) and declared by you (layout, CPU
architecture, CPUs, notes).

Further managed gateways in the configuration, each with its own token file (a name the UI
already gave a target stops the start, until one of the two is renamed):

```toml
[[gateway]]
name = "eu-helm"
admin_url = "https://gw-admin.eu.example.com"
token_file = "/run/secrets/eu-helm-token"
# The mock as that gateway reaches it: in the Helm chart, loadMock's Service.
mock_url = "http://pacod-gw-load-mock:9000"
data_plane = "https://api.eu.example.com"
[gateway.declared]
layout = "helm"
arch = "amd64"
cpus = 16
```

**An external target behind the same mock.** Its cells line up with a managed gateway's only
when it sits in front of the same mock with the same profile: point its upstream at
`http://<mock>:9000/x-mock/delay=200,chunk_delay=20,chunks=20/v1` and declare that profile with
the target. pacod-gw-lite: `PGW_PROVIDER_OPENAI_BASE_URL` (the standalone stack's
`bench-gw-lite` does this). A target that may reach a paid provider needs `load.max_calls` in
every cell and an acknowledgement of the cost at every run.

**Check a target** (Targets → the target → Check now): whom the gateway takes the token for, the
gateway's fingerprint, and one call
through it with a probe scene of its own, taken down after. A mock outside the gateway's egress
allow-list shows here as a refused call, not later as a run full of errors.

## 5. Agents

The benchmark drives pacod-gw's load agents, the same ones the gateway's own load tests use
(pacod-gw `docs/testing.md`, section 9). An agent runs one test at a time: a cell waits for a busy
agent (`BENCH_AGENT_WAIT_S`, or less in the settings), then is skipped with the flag
`agent_busy_skipped`.

Agents come from `BENCH_AGENTS` (with `BENCH_AGENT_PLACEMENT`), shown locked in the UI, or an
admin adds them on the Agents page: a name, the URL and where the agent stands. Every agent takes
the same token, `BENCH_AGENT_TOKEN`, which stays in the environment: set the agent's
`LOAD_AGENT_TOKEN` to it. An agent's URL receives that token and every cell's key: add only
agents on a private network or a VPN. An agent a waiting or going run uses is neither changed nor
removed; a name in both `BENCH_AGENTS` and the UI stops the start until one is renamed.

| Where | How |
|---|---|
| The standalone stack | `bench-load-agent`; its default target is `BENCH_AGENT_TARGET_URL` |
| A pacod-gw stack | Its `gw-load-agent` (`docker-compose.load.yml`), with the stack's `TESTER_LOAD_TOKEN` |
| Another machine | pacod-gw's `deploy/load-agent/`, aimed at the gateway's public data plane |
| Kubernetes | pacod-gw's Helm chart, `loadAgent.enabled` |

**Behaviour or capacity.** An agent on the gateway's machine takes CPU from the gateway, and
Docker's network adds time: its figures show how the gateway behaves, not what it can take. A run
counts as capacity only when every agent is declared `remote`, none ran short of CPU or late
(`agent_cpu_high`, `agent_lag`), no other traffic was seen at the gateway, and the mock was no
one else's (`"mock_dedicated": true` in the target's declared fingerprint). Every result page and
every report says which, and why.

**Baselines.** Each run starts with one baseline cell per mock profile and request: the same load
from the same agents straight to the mock. A cell's overhead is its latency minus its baseline's:
what the target added, including what the gateway's own figure (`x-gw-delay-ms`, the time to the
response head) leaves out, such as relaying a stream. `BENCH_BASELINE_MOCK_URL` is the mock as the
agents reach it: in a pacod-gw stack its `gw-load-mock`, which its load overlay puts on the
agent's network.

## 6. Results and reports

A run's page shows each cell's steady window (from the end of the warm-up to the end of the last
stage): calls a second, latency percentiles, the gateway's own delay (n/a where the target sent
none), the overhead against the baseline, and flags. Compare puts runs side by side; only cells
with the same comparison key share a row.

A finished run's report (`pacod-bench-report/1`, pacod-gw's `crates/pacod-gw-bench-report`):

- **Publish to pacod-gw** sends it to `BENCH_PUBLISH_URL` with `BENCH_PUBLISH_TOKEN` (by default
  the managed gateway and its token). It shows under Platform → Testing → Benchmarks.
- **Download** gives the same bytes: upload the file in any pacod-gw console. The gateway keeps a
  report once per digest, so a published report uploaded again is the same report.
- `pacod-bench validate report.json` checks a file as the gateway does.

A report is at most 1 MiB: the seconds of long runs are thinned out (`series_downsampled`), the
figures and histograms never.

## 7. Roles

Roles are pacod-id's (`authz/bench.yaml`), at the platform:

| Role | May |
|---|---|
| `bench.viewer` | Read agents, targets, suites, runs, results; download reports |
| `bench.operator` | And write suites, start and cancel runs, publish reports |
| `bench.admin` | And add gateways, external targets and agents, change the settings, delete runs, clean up leftovers, give roles |

A run acts on a managed gateway with the benchmark's admin token, not with the caller's rights:
give `bench.operator` and `bench.admin` only to people who may create scratch objects there and
aim the agents at any URL ([threat-model.md](threat-model.md)).

## 8. The command line

`pacod-bench run` runs a suite on a target from a terminal, without a database, a sign-in or the
UI, and writes the report. The target file names where the secrets are, never the secrets:

```toml
name = "lite on the laptop"
product = "pacod_gw_lite"          # pacod_gw, pacod_gw_lite, third_party
[external]
base_url = "http://127.0.0.1:18381"
model = "openai/gpt-mock"
key_env = "BENCH_TARGET_KEY"
paid = false
mock = { family = "openai", delay_ms = 200, chunk_delay_ms = 20, chunks = 20 }
[declared]
layout = "lite"
arch = "arm64"
```

A managed gateway instead: `[managed]` with `admin_url`, `admin_token_env`, `mock_url` and
optionally `data_plane`. The agents' token is `BENCH_AGENT_TOKEN`. Ctrl-C cancels; the scene is
taken down first. `pacod-bench plan --suite <file>` prints the cells and the estimate.
`scripts/build-binaries.sh` builds the binary for Linux and macOS.

## 9. Troubleshooting

| What | Why, and what to do |
|---|---|
| "the gateway refuses the benchmark's admin token", "the gateway refuses BENCH_GW_ADMIN_TOKEN" | The token is unknown, expired, or not a platform admin's (section 3). A gateway added through the UI: replace the token on its page |
| A gateway shows as unusable | Its token was sealed under a key that is not configured (`BENCH_KEK`, `BENCH_KEK_OLD`), or `BENCH_KEK` is not set (section 3) |
| A cell's probe answers 401 | The instances had not applied the cell's key; the flag `settle_timeout` says so. Check the gateway's configuration reloads (`/system/status`) |
| A cell's probe answers 502 with an egress message | The mock is not in the gateway's `PGW_EGRESS_ALLOW_PRIVATE` |
| Cells skipped, `agent_busy_skipped` | The tester's load test, or another run, held the agent |
| `other_traffic` | Other calls were in flight at the gateway during the cell: its figures are not the cell's alone |
| `config_changed_during_cell` | Someone changed the gateway's configuration during the cell; every instance reloaded |
| Baselines skipped | `BENCH_BASELINE_MOCK_URL` is not set, or the agents cannot reach that mock |
| The UI's forms answer "Cross-site POST form submissions are forbidden" | `PROTOCOL_HEADER` is not set, or the proxy does not send `X-Forwarded-Proto` |
| "pacod-id's copy of the roles is not current" (503) | The API has not reached pacod-id's feed for a minute (`BENCH_ID_MAX_STALENESS`) |
| Leftovers on a run | Objects the run made could not be deleted; an admin cleans them up (the run's page, or the Runs page; `POST /api/v1/leftovers/clean`) |
