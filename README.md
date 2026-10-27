# pacod-benchmark

Benchmarks of [pacod-gw](../pacod-gw): its managed and proxy modes, the minimal gateway
(pacod-gw-lite), its deployment layouts, and other gateway products, in many configurations at
once, each measured against a baseline straight to the mock. Its own UI writes the suites,
starts the runs, follows them second by second and compares them; its reports go to pacod-gw's
console (**Platform → Testing → Benchmarks**), published from here or uploaded as a file.

It builds on what the pacod products already have:

| From | What |
|---|---|
| pacod-gw | The load agents (`pacod-gw-load-agent`) and their contract, which the gateway's own load tests use; the mock provider; the report's contract (`pacod-gw-bench-report`), which the gateway checks and shows; the log format |
| pacod-id | Sign-in for the UI (`@pacod/id-sveltekit`), the API's check of every caller and its roles (`pacod-id-client`), the rules engine and the roles of `authz/bench.yaml`, the design tokens (`@pacod/ui`) |
| pacod-demo | The shared networks, PostgreSQL, Caddy, Prometheus and the conventions of every product |

## Layout

| Path | What |
|---|---|
| `crates/pacod-bench-engine` | The controller, without a database or a server: suites expanded into cells (`plan`), the scene on a managed gateway (`scene`, `gw`), the agents (`agents`, `split`, `requests`), the seconds added up (`collect`), a run cell by cell (`controller`), what the figures can be taken for (`measure`), the report (`report`) |
| `crates/pacod-bench` | `pacod-bench`: `serve` (the API, the run queue, PostgreSQL), and the command line `run`, `plan`, `validate` |
| `ui/` | The UI: SvelteKit 3, a server-side front end of the API, signed in through pacod-id |
| `authz/bench.yaml` | The benchmark's roles and permissions, pushed to pacod-id at every start |
| `examples/suites/` | Suites to start from |
| `vendor/pacod-gw`, `vendor/pacod-id` | The crates and packages this repository builds on, at pinned commits (`scripts/vendor-*.sh`). Never edit them here |
| `deploy/docker/` | The images: `rust.Dockerfile` (pacod-bench), `ui.Dockerfile` (pacod-bench-ui) |
| `docker-compose.yml` | The app file: `bench-api`, `bench-ui` |
| `docker-compose.standalone.yml` | Its copies of the shared components, and a load agent and mock of its own |
| `platform/` | Its fragments of the shared configuration: `caddy/bench.caddy`, `postgres/bench-db.sh`, `prometheus/bench.yml` |
| `docs/` | [architecture](docs/architecture.md), [runbook](docs/runbook.md), [suite format](docs/suite-format.md), [threat model](docs/threat-model.md) |

## Getting started

Against a pacod-gw development stack on the same machine (`make dev-up` in pacod-gw):

```sh
make dev-up                     # the API, the UI, pacod-id, PostgreSQL, Caddy, an agent and a mock
open http://localhost:18380     # admin@pacod-bench.test / pacod-bench-dev
```

Without a gateway token the benchmark manages no gateway: register an external target, or start
the minimal gateway it brings (`make dev-up PROFILE=dev-gateway`). To manage the development
gateway, mint a platform-admin token there and put both in `.env`
([runbook](docs/runbook.md), section 3):

```sh
BENCH_GW_ADMIN_URL=http://host.docker.internal:18081
BENCH_GW_ADMIN_TOKEN=pgwa_…
```

Without Docker, the command line runs a suite and writes its report:

```sh
cargo run -p pacod-bench -- run --suite examples/suites/modes.yaml --target target.toml \
  --agents local=http://127.0.0.1:18096 --out report.json
cargo run -p pacod-bench -- validate report.json
```

## Where it runs

| Platform | What runs |
|---|---|
| Docker Compose, on its own | This repository's stack (`make up`, `make dev-up`), on amd64 or arm64 hosts: macOS, Linux, Windows with Docker Desktop |
| Docker Compose, in the full stack | pacod-demo includes the app file next to the other products, with the gateway's own load agent and mock; the agent shares the gateway's machine, so the figures show behaviour |
| A native binary | The command line (`scripts/build-binaries.sh`): Linux (static), macOS |
| Load agents anywhere | pacod-gw's `deploy/load-agent/` on another machine, or its Helm chart's `loadAgent` |

A Helm chart of its own is planned.

## Shared networks

Every service joins the external network `pacod-net`. The API listens on `pacod-admin-net`
alone, as `bench-api-internal:8190`: only the UI's server calls it. It calls pacod-gw's
management API (`pacod-gw-admin:8081`) and pacod-id's internal listener
(`pacod-id-internal:8071`) on that network too, and the load agents over `bench-agents`. Names
follow [pacod-demo/docs/NETWORK.md](../pacod-demo/docs/NETWORK.md): `bench-*` for the
benchmark's own services, `pacod-*` for shared components.

## Logging

Every service logs one event per line to stdout, as
[pacod-demo/docs/LOGGING.md](../pacod-demo/docs/LOGGING.md) describes: `RUST_LOG` sets the
level, `BENCH_LOG_FORMAT` the format (`json`, `text`), `SERVICE_NAME` the service.
