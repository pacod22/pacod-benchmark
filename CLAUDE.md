# pacod-benchmark: notes for working on the code

Benchmarks of pacod-gw (managed and proxy mode, pacod-gw-lite, deployment layouts) and of other
gateways. Read [README.md](README.md) first, then [docs/architecture.md](docs/architecture.md)
for how the code is laid out.

## Layout

| Path | What |
|---|---|
| `crates/pacod-bench-engine` | The controller: plan, scene, agents, collection, a run, the report. No database, no server. Feature `testing`: a fake of the management API and an agent in process. |
| `crates/pacod-bench` | `pacod-bench serve` (API, queue, PostgreSQL), `run`, `plan`, `validate`. `tests/api.rs` needs `make test-services`. |
| `ui/` | The UI (SvelteKit 3, adapter-node), signed in through pacod-id. `src/lib/types.ts` follows the API's Rust types by hand. |
| `authz/bench.yaml` | The roles; `authz/vectors/bench.yaml` pins them. |
| `vendor/pacod-gw`, `vendor/pacod-id` | Pinned copies. Never edit them here: change the other repository and vendor again. |
| `platform/` | The benchmark's fragments of the shared configuration (Caddy, PostgreSQL, Prometheus). |

## Commands

```sh
make test-services   # PostgreSQL for the API's tests (25436)
make verify          # fmt, clippy -D warnings, every test, cargo deny, the UI's checks, vendor-check, compose
make dev-up          # the stack from the working tree; PROFILE=dev-gateway adds pacod-gw-lite
make plan SUITE=examples/suites/modes.yaml
```

## Rules

- **Shared networks and names** as [pacod-demo/docs/NETWORK.md](../pacod-demo/docs/NETWORK.md):
  `pacod-net` for every service, `bench-*` for the benchmark's own services; the API on
  `pacod-admin-net` alone, bound to `bench-api-internal`; `depends_on` on a shared component
  carries `required: false`.
- **Logging** through `pacod_gw_logging` (`init_log` in `main.rs`); `SERVICE_NAME`,
  `BENCH_LOG_FORMAT`, `RUST_LOG`.
- **Secrets** come from a variable or its `_FILE`, never from a command line, the database, a log
  or a report. The management API is called only through the allow-list in
  `crates/pacod-bench-engine/src/gw.rs`; no generic pass-through.
- **Nothing is written to a gateway while a cell's load runs**; the key goes first in every
  teardown. `tests/run.rs` checks both.
- **Contracts.** The load agent's (`pacod_gw_load::contract`) and the report's
  (`pacod_gw_bench_report`) are pacod-gw's: a change starts there, with their fixtures, and is
  vendored here; the tests in `crates/pacod-bench-engine/tests/` read the vendored fixtures.
  `vendor-gw-worktree` is for a change to both repositories at once; `make vendor-check` refuses
  it until pacod-gw's side is committed and vendored again.
- **Same run, same report.** A report holds nothing that differs between a download and a
  publish (who ran it, not who published it), so that the gateway keeps it once.
- **Migrations** in `crates/pacod-bench/migrations/` are additive and never edited once released.
- **Dependencies.** `deny.toml` allows registry sources only. The UI pins `eslint-plugin-svelte`
  to pacod-gw's console's version.
- **Code style.** `rustfmt.toml` (120 columns); prettier and eslint for the UI. Comments say why,
  in full sentences.
- **Documentation** is British English, with short declarative sentences, tables, and numbered
  sections in the long documents. Keep the runbook and the architecture document true to the
  code.
- **Commits** are titled `<Area>[, stage N]: <what changed>` and say why in the body.
