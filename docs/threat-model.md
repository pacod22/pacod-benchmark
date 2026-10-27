# pacod-benchmark threat model

## 1. What it holds

| Asset | Where | Exposure |
|---|---|---|
| A platform-admin token of each managed gateway (`BENCH_GW_ADMIN_TOKEN`) | The API's environment or a mounted file | Full administration of that gateway |
| The agents' token (`BENCH_AGENT_TOKEN`) | The API's environment | Load on request, from every agent that takes it |
| Pasted keys of external targets | The API's memory, until the run starts and for its duration | Calls on that target, maybe paid |
| Each cell's scratch key | The API's memory and the agents', for the cell | Calls in the cell's mode and model only, expiring 15 minutes after the cell's planned end |
| pacod-id's client secrets, the UI's session secret | The API's and the UI's environment | Sign-in and the authorization feed |
| Results | PostgreSQL `bench` | Descriptions of deployments and their figures |

## 2. The admin token, and what it costs

A run on a managed gateway acts there with the benchmark's token, not with the caller's rights.
The token is a platform admin's, broader than the gateway's own load tests use (they act with
the admin's delegated session). So:

- a holder of `bench.runs.start` can, through a run, create and delete scratch organisations,
  workspaces, keys and grants on the gateway, and rewrite the URLs of the benchmark's mock
  providers (`bench-load-mock-*`);
- a holder of `bench.targets.manage` can aim the agents at any URL with a key they paste;
- whoever takes over `bench-api` holds the gateway's platform admin.

What limits it:

- **An allow-list.** The API's client of the management API has a method for each call a run
  makes and no generic one (`crates/pacod-bench-engine/src/gw.rs`). It deletes only below the
  run's own scratch organisation (by id), and writes providers and models only by the
  `bench-load-mock` names.
- **The token in a file**, never on a command line, never logged, never in the database or a
  report.
- **The API on pacod-admin-net alone**, never published by Caddy; only the UI's server calls it.
- **The gateway's audit log** names the token's principal (`pacod-bench`) for every change.
- **Managed gateways come from the configuration** only; nobody adds one from the UI.
- **Roles.** Give `bench.operator` and `bench.admin` only to people trusted with the above. A
  dedicated gateway installation for heavy benchmarking keeps production out of reach.

A narrower role of the gateway's (one that may create organisations and the benchmark's
providers, and nothing else) would shrink this; pacod-gw has none yet.

## 3. Agents

- The token is compared in constant time by the agent, at least 32 characters, never an
  argument. Rotating it affects pacod-gw's tester and the benchmark together when they share
  agents.
- Agents are configured by the operator only (`BENCH_AGENTS`): an agent's URL receives the token
  and every cell's key, so it never comes from a form.
- The API talks to agents in plain HTTP; the token and the keys cross that link. Agents on other
  machines need a private network or a VPN.
- An agent in the stack reaches only the gateway's data plane and the mock, not the management
  API or the databases (pacod-gw's `docker-compose.load.yml`).

## 4. External targets and keys

- A pasted key stays in memory: the run keeps a hint of it (a gateway key's id part, or the start
  of its SHA-256). A run that was queued when the API restarted fails, rather than waiting for a
  key it no longer has.
- A target that may reach a paid provider needs a number of calls at most in every cell and an
  acknowledgement of the cost at every run; each call is made once (`x-gw-retries: 1`).

## 5. Reports

- A report holds no secret: target labels, the observed and declared fingerprint, agent names and
  placements, figures. Declared notes are the operator's words; keep host names out of them when
  the report travels.
- pacod-gw checks every report as untrusted input (size, counts, finite numbers, short texts) and
  shows it as text.

## 6. Sign-in

- The UI is a confidential client of pacod-id with PKCE; its session is sealed in a cookie
  (`__Host-` on https). The API takes pacod-id access tokens of the audience `pacod-bench` only,
  checks them against pacod-id's keys, and refuses them when its copy of the bindings is older
  than a minute.
- `BENCH_AUTH=dev` replaces all of this by one static token that stands for an admin. It is for
  development without pacod-id; the API says so in its log at every start.
