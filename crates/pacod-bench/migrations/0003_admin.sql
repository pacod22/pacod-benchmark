-- Load agents added through the UI, next to those of the configuration (BENCH_AGENTS, which stay
-- in the environment). An agent's URL receives the agents' token (BENCH_AGENT_TOKEN, never
-- here) and every cell's key: only admins add one.
CREATE TABLE agents (
    name       text PRIMARY KEY,
    url        text NOT NULL,
    -- in_stack, same_host, remote, unknown
    placement  text NOT NULL DEFAULT 'unknown',
    created_by text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- The run settings an admin changes in the UI: one row. Each limit stays at most the
-- environment's (BENCH_MAX_CELLS, BENCH_MAX_DURATION, BENCH_AGENT_WAIT_S); what is not set here
-- is the environment's.
CREATE TABLE settings (
    id         integer PRIMARY KEY CHECK (id = 1),
    value      jsonb NOT NULL DEFAULT '{}',
    updated_by text NOT NULL DEFAULT '',
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- The settings a run started with, so that a change while it waits leaves its meaning alone.
-- Runs made before this column have none, and read the environment's.
ALTER TABLE runs ADD COLUMN settings jsonb;
