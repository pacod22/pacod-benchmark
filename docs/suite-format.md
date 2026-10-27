# The suite format

A suite is one base configuration and the axes it varies, in YAML or JSON. A run makes every
combination of the axes' values on a target; `examples/suites/` holds suites to start from. The
types are `crates/pacod-bench-engine/src/model.rs` and `plan.rs`; unknown fields are refused.

## 1. The document

```yaml
name: Managed against proxy mode          # required
description: The same calls in both modes.
base:                                      # one cell's configuration (section 2)
  request: { interface: openai, mode: managed }
  mock: { family: openai, delay_ms: 200, chunk_delay_ms: 20, chunks: 20 }
  workspace: { cache: "off", audit: on_request }
  key: {}
  load:
    mode: concurrency
    stages: [{ duration_s: 0, target: 10 }, { duration_s: 60, target: 10 }]
    timeout_s: 120
    warmup_s: 5
axes:                                      # path → values (section 3)
  request.mode: [managed, proxy]
  load.target: [10, 50, 100]
order: { kind: grouped }                   # or { kind: shuffled, seed: 7 }
repetitions: 1                             # 1 to 20
cap: 64                                    # cells at most (BENCH_MAX_CELLS above it)
baseline: true                             # a baseline cell straight to the mock per profile
```

## 2. A cell's configuration

| Field | Values | Default |
|---|---|---|
| `request.interface` | `openai`, `anthropic`, `gemini`, `native` (managed only), `embeddings` (never streamed) | `openai` |
| `request.mode` | `managed`, `proxy` | `managed` |
| `request.stream` | streamed answers, or whole | `true` |
| `request.prompt_words`, `request.max_tokens` | 1 to 20 000, 1 to 100 000 | 60, 64 |
| `request.http` | `"1.1"`, `"2"` | `"1.1"` |
| `request.headers` | further headers (`x-gw-template`, …); not the key's, `content-type` or `x-gw-cache` | none |
| `mock.family` | `openai`, `anthropic`, `gemini`: the dialect the gateway speaks upstream | `openai` |
| `mock.delay_ms`, `mock.chunk_delay_ms`, `mock.chunks` | the mock's latency: before the head, between chunks, chunks of an answer | 200, 20, 20 |
| `workspace.cache` | the response cache's binding: `off`, `shadow`, `enforce` | `off` |
| `workspace.audit` | `off`, `on`, `on_request` | `on_request` |
| `workspace.responses_store`, `workspace.on_spool_full` | `off`/`on`; `keep_serving`/`fail_closed` | unset |
| `key.inspect` | `full`, `none` (proxy mode's blind relay) | unset |
| `key.usage_policy` | `require`, `estimate`, `ignore` | unset |
| `key.max_concurrency` | a cap on the key | unset |
| `load.mode` | `concurrency` (a closed loop of callers), `rate` (an open loop of arrivals; needs `max_in_flight`) | `concurrency` |
| `load.stages` | each moves the target in a straight line from where the previous one ended (0 at the start); a stage of 0 seconds jumps | required |
| `load.timeout_s` | how long one call may take | 120 |
| `load.max_calls` | calls of the cell at most, all agents together | none |
| `load.warmup_s` | seconds left out of the steady figures | 0 |

A steady load is a jump and a stretch: `[{duration_s: 0, target: 10}, {duration_s: 60, target: 10}]`.

## 3. Axes

An axis names a field by its dotted path and lists its values; each combination is a cell.
`load.target` scales every stage, so that a ramp stays a ramp at another peak.

Combinations that cannot run are left out, and the plan says why:

- proxy mode relays the interface's own dialect: it needs the mock family of that dialect;
- native is managed only; embeddings come from the OpenAI mock;
- the response cache never answers proxy mode or embeddings;
- the same configuration twice is kept once.

On an external target, the benchmark sets nothing up: axes below `mock`, `workspace` and `key`
are refused, and the mock the target declares replaces the suite's.

What only a deployment decides (analytics, the cache store, the scale layout, the minimal gateway,
the hardware) is no axis: register each deployment as a target and run the same suite on each.

**The cache.** Every call of a cell sends the same body; with the cache on, nearly every call is
answered from it. A cache cell measures a hit, not a mix of hits and misses.

## 4. Order and repetitions

`grouped` keeps what costs a configuration write (the workspace variant, the mock profile)
together, so that the gateway reloads as rarely as possible. `shuffled` spreads the cells in a
fixed order given by its seed, so that a slow drift of the machine does not line up with one axis.
Repetitions run each configuration again; their cells share a comparison key.
