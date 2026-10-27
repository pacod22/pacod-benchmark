-- Runs started on a timetable: a suite on a managed gateway (an external target's key is never
-- kept, so it takes no schedule). A schedule fires with its creator's authority, checked again at
-- every fire; missed fires are not made up for.
CREATE TABLE schedules (
    id              text PRIMARY KEY,
    name            text NOT NULL,
    suite_id        text NOT NULL,
    target_id       text NOT NULL,
    -- The agents' names; null: the settings' default agents, else every one.
    agents          jsonb,
    label           text NOT NULL DEFAULT '',
    -- {"kind": "daily", "at": "02:30", "weekdays": [1, 2, 3, 4, 5]} or {"kind": "every", "hours": 6},
    -- in UTC.
    cadence         jsonb NOT NULL,
    enabled         boolean NOT NULL DEFAULT true,
    -- Publish the report to pacod-gw when the run has finished.
    publish         boolean NOT NULL DEFAULT false,
    created_by      text NOT NULL,
    created_by_name text NOT NULL DEFAULT '',
    created_by_kind text NOT NULL DEFAULT 'user',
    next_fire_at    timestamptz,
    last_fire_at    timestamptz,
    last_run_id     text,
    -- What the last fire did, in a sentence.
    last_outcome    text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX schedules_due ON schedules (next_fire_at) WHERE enabled;

-- The schedule that started a run, if one did.
ALTER TABLE runs ADD COLUMN schedule_id text;
