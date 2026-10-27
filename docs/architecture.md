# pacod-benchmark architecture

## 1. The parts

```text
browser ──► pacod-caddy ──► bench-ui (SvelteKit, signs in through pacod-id)
                                │ server side only, with the person's token
                                ▼
                        bench-api (pacod-bench serve) ──► PostgreSQL "bench"
                          │  admin token      ──► pacod-gw management API (scene, settle, live figures, reports)
                          │  agent token      ──► pacod-gw-load-agent(s) ──► gateway data plane / external URL
                          │                                              └─► the mock, straight (baselines)
                          └  service client   ──► pacod-id (manifest, feed, keys)
pacod-bench run (command line): the same controller, no database, no UI, a report file
```

- **The controller is a library** (`pacod-bench-engine`). The service and the command line run
  the same code; it reports what happens as events on a channel.
- **The agents make the load.** They are pacod-gw's, in the contract pacod-gw's tester speaks
  (`pacod_gw_load::contract`, vendored). The benchmark plans, sets up, starts them together, and
  adds up their seconds; an agent only makes calls and counts them.
- **The report is pacod-gw's contract** (`pacod_gw_bench_report`, vendored): the benchmark builds
  the gateway's own types and checks them with the gateway's own `validate`.

## 2. Crates

| Module | What |
|---|---|
| `model` | One cell's configuration: `request`, `mock`, `workspace`, `key`, `load`, and its checks |
| `plan` | Suites: axes by dotted path, every combination, the ones that cannot run left out with the reason, grouped or shuffled order, repetitions, baselines, the estimate |
| `gw` | The management API, as an allow-list of the calls a run makes; deletes only below the run's own scratch organisation |
| `scene` | The benchmark's mocks (`bench-load-mock-*`), the scratch organisation, a workspace per variant with its cache binding, a key per cell, the teardown |
| `agents`, `split`, `requests` | The agents (`BENCH_AGENTS`), each agent's share, the request each call sends |
| `collect` | The agents' seconds added up: one point a second, the whole cell's summary and the steady window's |
| `controller` | A run, cell by cell; the target probe |
| `measure` | Behaviour or capacity, and why |
| `report` | A run as `pacod-bench-report/1` |
| `testing` (feature) | A fake of the management API and an agent in process, for tests |

`pacod-bench` holds the API (`api/`), the run queue (`runs.rs`), the database (`store.rs`,
`migrations/`), pacod-id (`identity.rs`), the reports (`reports.rs`) and the command line
(`cli.rs`).

## 3. A run

1. **Plan.** The suite is expanded on the target's kind. An external target refuses axes the
   benchmark cannot set there (`mock.*`, `workspace.*`, `key.*`); its declared mock replaces the
   suite's. The plan is kept with the run, with a copy of the suite, the target and the agents.
2. **Check the agents.** Every agent is there, speaks this contract, is free, and allows each
   cell's share (`Info.caps`).
3. **Set the scene up** (managed gateways). The fingerprint (`/system/status`, `/response-cache`),
   the mocks of the families the cells need, a scratch organisation `bench-<run>`, a workspace
   per workspace variant with an explicit `response_cache` binding (the installation may switch
   the cache on for every workspace), the mock models granted. Then it waits until every instance
   has applied the configuration (`/system/config-versions/{v}`).
4. **Each cell**, in order:
   - the mock profile into the providers' URLs, if it changed; the cell's key (its mode and model,
     expiring 15 minutes after the cell's planned end); wait for the instances again;
   - wait for the agents to be free; then about three seconds of the gateway's calls in flight
     (none expected);
   - prepare every agent at once (each probes the target with one call), start them together;
   - once a second, each agent's new seconds; every two seconds, the gateway's own figures;
   - stop the agents, delete the key, summarise, and flag what a reader should know.

   Nothing is written to the gateway while a cell's load runs.
5. **Take the scene down**, keys first, the rest newest first; what could not be deleted is
   listed as left over. A cancel goes the same way.

Baseline cells run first: the same request and load from the same agents to
`BENCH_BASELINE_MOCK_URL/x-mock/<profile>`, without any gateway.

## 4. The API

`/api/v1` on `BENCH_LISTEN`. Every route but `healthz` and `readyz` needs a caller with a
benchmark role.

| Route | Permission |
|---|---|
| `GET /session`, `/agents`, `/targets`, `/targets/{id}`, `/suites`, `/suites/{id}`, `/runs`, `/runs/{id}`, `/runs/{id}/cells/{idx}`, `/runs/{id}/events`, `/runs/{id}/report`, `/compare?runs=` ; `POST /plan` | `bench.results.read` |
| `POST /suites`, `PUT/DELETE /suites/{id}` | `bench.suites.manage` |
| `POST /runs`, `POST /runs/{id}/cancel`, `POST /targets/{id}/check` | `bench.runs.start` |
| `POST /runs/{id}/publish` | `bench.reports.publish` |
| `POST /targets`, `PUT/DELETE /targets/{id}`, `DELETE /runs/{id}`, `POST /leftovers/clean` | `bench.targets.manage` |

`GET /runs/{id}/events` is a stream of server-sent events: `snapshot` (the run and its cells),
then `status`, `observed`, `cell`, `point`, `cell_done`, `warning`, `done`. The UI's server passes
it on to the browser.

The UI reads the API's answers through hand-written types (`ui/src/lib/types.ts`), which follow
the Rust types; a change to those changes them.

## 5. Data

| Table | What |
|---|---|
| `targets` | Managed gateways (from the configuration, `cfg-<name>`) and external targets; never a secret |
| `suites` | Suites, versioned |
| `runs` | A run: copies of its suite, target and agents, its plan, the gateway's fingerprint, its scene (for a teardown after an interruption), leftovers, warnings |
| `cells` | A run's cells: configuration, comparison key, the result, the seconds |
| `publications` | Where a run's report went, or why not |

One run is active at a time, which a partial unique index holds. A pasted key of an external
target is held in memory until its run starts and never written.

## 6. Comparison keys

A cell's comparison key is the SHA-256 of its whole configuration (request, mock, workspace, key,
load) as canonical JSON. Cells with the same key ran the same configuration, in any run, on any
target; only they are put side by side. A baseline's key covers what reaches the mock (family and
profile, the request as the mock sees it, the load), not the mode or the gateway's settings, so
that a managed and a proxy cell share one baseline.

## 7. Vendoring

`scripts/vendor-pacod-gw.sh` copies the crates of a pacod-gw commit this repository builds on
(`pacod-gw-load`, `pacod-gw-logging`, `pacod-gw-testkit`, `pacod-gw-bench-report`) with the root
manifest their `.workspace = true` fields inherit from; `scripts/vendor-pacod-id.sh` copies
pacod-id and packs its npm packages into `ui/vendor/`. `make vendor-check` refuses a working
tree's copy and runs the contract tests: the load agent's fixtures and the report's.
