-- pacod-benchmark's tables. Migrations only add: a column, a table, an index. They are never
-- edited once released.

-- Where runs go. Gateways the benchmark manages come from its configuration (BENCH_GW_ADMIN_URL,
-- BENCH_GW_TARGETS) and are written here at every start; external targets are made through the
-- API. No secret is ever stored: a managed gateway's token stays in the configuration, an
-- external target's key is pasted for each run and held in memory.
CREATE TABLE targets (
    id          text PRIMARY KEY,
    name        text NOT NULL UNIQUE,
    kind        text NOT NULL CHECK (kind IN ('managed', 'external')),
    -- pacod_gw, pacod_gw_lite, third_party
    product     text NOT NULL,
    source      text NOT NULL CHECK (source IN ('config', 'api')),
    -- managed: {admin_url, mock_url, data_plane}; external: {base_url, model, provider,
    -- upstream, mock, paid}
    definition  jsonb NOT NULL,
    -- What the operator says about the deployment: layout, arch, CPUs, memory, notes.
    declared    jsonb NOT NULL DEFAULT '{}',
    -- What the gateway said about itself at the last check or run.
    observed    jsonb,
    observed_at timestamptz,
    created_by  text NOT NULL DEFAULT '',
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE suites (
    id         text PRIMARY KEY,
    name       text NOT NULL,
    version    integer NOT NULL DEFAULT 1,
    document   jsonb NOT NULL,
    created_by text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- A run keeps a copy of its suite, its target and its agents as they were: a later change to
-- either leaves the run's meaning alone.
CREATE TABLE runs (
    id              text PRIMARY KEY,
    status          text NOT NULL,
    label           text NOT NULL DEFAULT '',
    suite_id        text,
    suite           jsonb NOT NULL,
    target_id       text,
    target          jsonb NOT NULL,
    agents          jsonb NOT NULL,
    -- The plan's estimate, pruned cells and notes.
    plan            jsonb NOT NULL,
    started_by      text NOT NULL,
    started_by_name text NOT NULL DEFAULT '',
    key_hint        text NOT NULL DEFAULT '',
    observed        jsonb,
    -- What the run made on the gateway, for a teardown after an interruption.
    scene           jsonb NOT NULL DEFAULT '{}',
    leftovers       jsonb NOT NULL DEFAULT '[]',
    warnings        jsonb NOT NULL DEFAULT '[]',
    error           text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    started_at      timestamptz,
    finished_at     timestamptz
);

CREATE INDEX runs_created ON runs (created_at DESC);
-- One run at a time: the benchmark's mocks are shared by every run on a gateway, and two runs
-- would measure each other.
CREATE UNIQUE INDEX runs_one_active ON runs ((true)) WHERE status IN ('preparing', 'running', 'cleaning');

CREATE TABLE cells (
    run_id         text NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    idx            integer NOT NULL,
    rep            integer NOT NULL,
    kind           text NOT NULL,
    status         text NOT NULL,
    axes           jsonb NOT NULL,
    config         jsonb NOT NULL,
    comparison_key text NOT NULL,
    baseline_key   text NOT NULL,
    -- The cell's result as the controller reports it, its seconds and the gateway's samples.
    result         jsonb,
    points         jsonb NOT NULL DEFAULT '[]',
    started_at     timestamptz,
    finished_at    timestamptz,
    PRIMARY KEY (run_id, idx)
);

CREATE INDEX cells_comparison ON cells (comparison_key);

CREATE TABLE publications (
    id           text PRIMARY KEY,
    run_id       text NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    destination  text NOT NULL,
    status       text NOT NULL CHECK (status IN ('published', 'failed')),
    gw_report_id text NOT NULL DEFAULT '',
    digest       text NOT NULL DEFAULT '',
    error        text NOT NULL DEFAULT '',
    published_by text NOT NULL,
    published_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX publications_run ON publications (run_id, published_at DESC);
