-- Queued runs start in the order of their position, which the UI changes (POST
-- /runs/{id}/move); a new run goes to the end. Existing runs keep the order they were asked for.
CREATE SEQUENCE runs_position;
ALTER TABLE runs ADD COLUMN position bigint;
UPDATE runs SET position = o.n
  FROM (SELECT id, row_number() OVER (ORDER BY created_at) AS n FROM runs) o
 WHERE runs.id = o.id;
SELECT setval('runs_position', COALESCE((SELECT max(position) FROM runs), 0) + 1, false);
ALTER TABLE runs ALTER COLUMN position SET DEFAULT nextval('runs_position');
ALTER TABLE runs ALTER COLUMN position SET NOT NULL;
ALTER SEQUENCE runs_position OWNED BY runs.position;
CREATE INDEX runs_queued ON runs (position) WHERE status = 'queued';
