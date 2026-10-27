//! The benchmark's database (PostgreSQL, database `bench`): targets, suites, runs, their cells,
//! and where their reports were published. Documents are kept as JSON; the columns hold what
//! lists sort and filter on.

use chrono::{DateTime, Utc};
use pacod_bench_engine::controller::CellResult;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{FromRow, Row};

pub type Result<T> = std::result::Result<T, sqlx::Error>;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

/// A new id: a prefix and a time-ordered UUID.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TargetRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub product: String,
    pub source: String,
    pub definition: Value,
    pub declared: Value,
    pub observed: Option<Value>,
    pub observed_at: Option<DateTime<Utc>>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SuiteRow {
    pub id: String,
    pub name: String,
    pub version: i32,
    pub document: Value,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct RunRow {
    pub id: String,
    pub status: String,
    pub label: String,
    pub suite_id: Option<String>,
    pub suite: Value,
    pub target_id: Option<String>,
    pub target: Value,
    pub agents: Value,
    pub plan: Value,
    pub started_by: String,
    pub started_by_name: String,
    pub key_hint: String,
    pub observed: Option<Value>,
    pub scene: Value,
    pub leftovers: Value,
    pub warnings: Value,
    pub error: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CellRow {
    pub run_id: String,
    pub idx: i32,
    pub rep: i32,
    pub kind: String,
    pub status: String,
    pub axes: Value,
    pub config: Value,
    pub comparison_key: String,
    pub baseline_key: String,
    pub result: Option<Value>,
    pub points: Value,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Where a run's report went, or why it did not.
pub struct NewPublication<'a> {
    pub run_id: &'a str,
    pub destination: &'a str,
    pub status: &'a str,
    pub gw_report_id: &'a str,
    pub digest: &'a str,
    pub error: &'a str,
    pub by: &'a str,
}

/// A cell as a run is first written: planned, not run.
pub struct NewCell {
    pub idx: i32,
    pub rep: i32,
    pub kind: String,
    pub axes: Value,
    pub config: Value,
    pub comparison_key: String,
    pub baseline_key: String,
}

pub struct NewRun {
    pub id: String,
    pub label: String,
    pub suite_id: Option<String>,
    pub suite: Value,
    pub target_id: String,
    pub target: Value,
    pub agents: Value,
    pub plan: Value,
    pub started_by: String,
    pub started_by_name: String,
    pub key_hint: String,
}

const ACTIVE: &str = "('preparing', 'running', 'cleaning')";

impl Store {
    pub async fn connect(url: &str) -> Result<Store> {
        let pool = PgPoolOptions::new().max_connections(10).connect(url).await?;
        Ok(Store { pool })
    }

    pub async fn migrate(&self) -> std::result::Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }

    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await.map(|_| ())
    }

    // --- targets --------------------------------------------------------------------------

    /// A gateway of the configuration, written at every start; its id follows its name.
    pub async fn upsert_config_target(&self, name: &str, definition: &Value, declared: &Value) -> Result<()> {
        sqlx::query(
            "INSERT INTO targets (id, name, kind, product, source, definition, declared)
             VALUES ($1, $2, 'managed', 'pacod_gw', 'config', $3, $4)
             ON CONFLICT (id) DO UPDATE SET definition = $3, declared = $4, updated_at = now()",
        )
        .bind(format!("cfg-{name}"))
        .bind(name)
        .bind(definition)
        .bind(declared)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Gateways no longer in the configuration are forgotten; runs keep their copy.
    pub async fn forget_config_targets(&self, keep: &[String]) -> Result<()> {
        let ids: Vec<String> = keep.iter().map(|n| format!("cfg-{n}")).collect();
        sqlx::query("DELETE FROM targets WHERE source = 'config' AND NOT (id = ANY($1))")
            .bind(&ids)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn targets(&self) -> Result<Vec<TargetRow>> {
        sqlx::query_as("SELECT * FROM targets ORDER BY source, name").fetch_all(&self.pool).await
    }

    pub async fn target(&self, id: &str) -> Result<Option<TargetRow>> {
        sqlx::query_as("SELECT * FROM targets WHERE id = $1").bind(id).fetch_optional(&self.pool).await
    }

    pub async fn insert_target(
        &self,
        name: &str,
        product: &str,
        definition: &Value,
        declared: &Value,
        by: &str,
    ) -> Result<TargetRow> {
        sqlx::query_as(
            "INSERT INTO targets (id, name, kind, product, source, definition, declared, created_by)
             VALUES ($1, $2, 'external', $3, 'api', $4, $5, $6) RETURNING *",
        )
        .bind(new_id("tgt"))
        .bind(name)
        .bind(product)
        .bind(definition)
        .bind(declared)
        .bind(by)
        .fetch_one(&self.pool)
        .await
    }

    /// External targets only: a managed gateway's definition is the configuration's.
    pub async fn update_target(
        &self,
        id: &str,
        name: &str,
        product: &str,
        definition: &Value,
        declared: &Value,
    ) -> Result<Option<TargetRow>> {
        sqlx::query_as(
            "UPDATE targets SET name = $2, product = $3, definition = $4, declared = $5, updated_at = now()
             WHERE id = $1 AND source = 'api' RETURNING *",
        )
        .bind(id)
        .bind(name)
        .bind(product)
        .bind(definition)
        .bind(declared)
        .fetch_optional(&self.pool)
        .await
    }

    /// A managed gateway's declared fingerprint can be changed; its definition cannot.
    pub async fn set_declared(&self, id: &str, declared: &Value) -> Result<bool> {
        let done = sqlx::query("UPDATE targets SET declared = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(declared)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn delete_target(&self, id: &str) -> Result<bool> {
        let done =
            sqlx::query("DELETE FROM targets WHERE id = $1 AND source = 'api'").bind(id).execute(&self.pool).await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn set_observed(&self, id: &str, observed: &Value) -> Result<()> {
        sqlx::query("UPDATE targets SET observed = $2, observed_at = now() WHERE id = $1")
            .bind(id)
            .bind(observed)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- suites ---------------------------------------------------------------------------

    pub async fn suites(&self) -> Result<Vec<SuiteRow>> {
        sqlx::query_as("SELECT * FROM suites ORDER BY updated_at DESC").fetch_all(&self.pool).await
    }

    pub async fn suite(&self, id: &str) -> Result<Option<SuiteRow>> {
        sqlx::query_as("SELECT * FROM suites WHERE id = $1").bind(id).fetch_optional(&self.pool).await
    }

    pub async fn insert_suite(&self, name: &str, document: &Value, by: &str) -> Result<SuiteRow> {
        sqlx::query_as("INSERT INTO suites (id, name, document, created_by) VALUES ($1, $2, $3, $4) RETURNING *")
            .bind(new_id("ste"))
            .bind(name)
            .bind(document)
            .bind(by)
            .fetch_one(&self.pool)
            .await
    }

    pub async fn update_suite(&self, id: &str, name: &str, document: &Value) -> Result<Option<SuiteRow>> {
        sqlx::query_as(
            "UPDATE suites SET name = $2, document = $3, version = version + 1, updated_at = now()
             WHERE id = $1 RETURNING *",
        )
        .bind(id)
        .bind(name)
        .bind(document)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_suite(&self, id: &str) -> Result<bool> {
        let done = sqlx::query("DELETE FROM suites WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(done.rows_affected() > 0)
    }

    // --- runs -----------------------------------------------------------------------------

    /// A queued run and its planned cells, in one transaction.
    pub async fn insert_run(&self, run: &NewRun, cells: &[NewCell]) -> Result<RunRow> {
        let mut tx = self.pool.begin().await?;
        let row: RunRow = sqlx::query_as(
            "INSERT INTO runs (id, status, label, suite_id, suite, target_id, target, agents, plan,
                               started_by, started_by_name, key_hint)
             VALUES ($1, 'queued', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING *",
        )
        .bind(&run.id)
        .bind(&run.label)
        .bind(&run.suite_id)
        .bind(&run.suite)
        .bind(&run.target_id)
        .bind(&run.target)
        .bind(&run.agents)
        .bind(&run.plan)
        .bind(&run.started_by)
        .bind(&run.started_by_name)
        .bind(&run.key_hint)
        .fetch_one(&mut *tx)
        .await?;
        for c in cells {
            sqlx::query(
                "INSERT INTO cells (run_id, idx, rep, kind, status, axes, config, comparison_key, baseline_key)
                 VALUES ($1, $2, $3, $4, 'pending', $5, $6, $7, $8)",
            )
            .bind(&run.id)
            .bind(c.idx)
            .bind(c.rep)
            .bind(&c.kind)
            .bind(&c.axes)
            .bind(&c.config)
            .bind(&c.comparison_key)
            .bind(&c.baseline_key)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(row)
    }

    /// Newest first, `limit` at most, created before `before` when given.
    pub async fn runs(&self, limit: i64, before: Option<DateTime<Utc>>) -> Result<Vec<RunRow>> {
        sqlx::query_as(
            "SELECT * FROM runs WHERE ($2::timestamptz IS NULL OR created_at < $2)
             ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .bind(before)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn run(&self, id: &str) -> Result<Option<RunRow>> {
        sqlx::query_as("SELECT * FROM runs WHERE id = $1").bind(id).fetch_optional(&self.pool).await
    }

    /// The oldest queued run, if no run is active.
    pub async fn next_queued(&self) -> Result<Option<RunRow>> {
        sqlx::query_as(&format!(
            "SELECT * FROM runs WHERE status = 'queued'
               AND NOT EXISTS (SELECT 1 FROM runs WHERE status IN {ACTIVE})
             ORDER BY created_at LIMIT 1"
        ))
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn set_status(&self, id: &str, status: &str, error: Option<&str>) -> Result<()> {
        sqlx::query(
            "UPDATE runs SET status = $2, error = COALESCE($3, error),
                started_at = CASE WHEN $2 = 'preparing' THEN now() ELSE started_at END,
                finished_at = CASE WHEN $2 IN ('finished', 'cancelled', 'error', 'interrupted') THEN now()
                                   ELSE finished_at END
             WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_run_observed(&self, id: &str, observed: &Value) -> Result<()> {
        sqlx::query("UPDATE runs SET observed = $2 WHERE id = $1").bind(id).bind(observed).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_scene(&self, id: &str, scene: &Value) -> Result<()> {
        sqlx::query("UPDATE runs SET scene = $2 WHERE id = $1").bind(id).bind(scene).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_leftovers(&self, id: &str, leftovers: &[String]) -> Result<()> {
        sqlx::query("UPDATE runs SET leftovers = $2 WHERE id = $1")
            .bind(id)
            .bind(json!(leftovers))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_warning(&self, id: &str, warning: &str) -> Result<()> {
        sqlx::query("UPDATE runs SET warnings = warnings || $2 WHERE id = $1")
            .bind(id)
            .bind(json!([warning]))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// At start: runs that were going when the benchmark stopped are interrupted (the agents
    /// stopped on their own when their lease ran out), and queued runs whose pasted key was held
    /// in memory cannot start any more. Returns the interrupted runs, whose scene is to be taken
    /// down.
    pub async fn recover(&self) -> Result<Vec<RunRow>> {
        let interrupted: Vec<RunRow> = sqlx::query_as(&format!(
            "UPDATE runs SET status = 'interrupted', finished_at = now(),
                error = 'the benchmark stopped during the run; the agents stopped when their lease ran out'
             WHERE status IN {ACTIVE} RETURNING *"
        ))
        .fetch_all(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE runs SET status = 'error', finished_at = now(),
                error = 'the benchmark stopped before the run started, and the pasted key was held in memory only'
             WHERE status = 'queued' AND target->>'kind' = 'external'",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE cells SET status = 'cancelled' WHERE status NOT IN ('done', 'skipped', 'error', 'cancelled')
               AND run_id IN (SELECT id FROM runs WHERE status IN ('interrupted', 'error'))",
        )
        .execute(&self.pool)
        .await?;
        Ok(interrupted)
    }

    pub async fn delete_run(&self, id: &str) -> Result<bool> {
        let done = sqlx::query(&format!("DELETE FROM runs WHERE id = $1 AND status NOT IN {ACTIVE}"))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// A queued run that never started.
    pub async fn cancel_queued(&self, id: &str) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE runs SET status = 'cancelled', finished_at = now() WHERE id = $1 AND status = 'queued'",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        if done.rows_affected() > 0 {
            sqlx::query("UPDATE cells SET status = 'cancelled' WHERE run_id = $1").bind(id).execute(&self.pool).await?;
        }
        Ok(done.rows_affected() > 0)
    }

    // --- cells ----------------------------------------------------------------------------

    /// The run's cells without their seconds.
    pub async fn cells(&self, run_id: &str) -> Result<Vec<CellRow>> {
        sqlx::query_as(
            "SELECT run_id, idx, rep, kind, status, axes, config, comparison_key, baseline_key,
                    result - 'points' - 'gateway' AS result, '[]'::jsonb AS points, started_at, finished_at
             FROM cells WHERE run_id = $1 ORDER BY idx",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
    }

    /// Every cell of the run with its seconds, for a report.
    pub async fn cells_full(&self, run_id: &str) -> Result<Vec<CellRow>> {
        sqlx::query_as("SELECT * FROM cells WHERE run_id = $1 ORDER BY idx").bind(run_id).fetch_all(&self.pool).await
    }

    pub async fn cell(&self, run_id: &str, idx: i32) -> Result<Option<CellRow>> {
        sqlx::query_as("SELECT * FROM cells WHERE run_id = $1 AND idx = $2")
            .bind(run_id)
            .bind(idx)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn set_cell_status(&self, run_id: &str, idx: i32, status: &str) -> Result<()> {
        sqlx::query(
            "UPDATE cells SET status = $3,
                started_at = CASE WHEN started_at IS NULL AND $3 = 'preparing' THEN now() ELSE started_at END
             WHERE run_id = $1 AND idx = $2",
        )
        .bind(run_id)
        .bind(idx)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The seconds so far of a cell that runs, written every few seconds.
    pub async fn set_cell_points(&self, run_id: &str, idx: i32, points: &Value) -> Result<()> {
        sqlx::query("UPDATE cells SET points = $3 WHERE run_id = $1 AND idx = $2")
            .bind(run_id)
            .bind(idx)
            .bind(points)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A cell's end: its result, with its seconds kept apart.
    pub async fn finish_cell(&self, run_id: &str, c: &CellResult) -> Result<()> {
        sqlx::query(
            "UPDATE cells SET status = $3, result = $4, points = $5,
                started_at = COALESCE($6, started_at), finished_at = $7
             WHERE run_id = $1 AND idx = $2",
        )
        .bind(run_id)
        .bind(c.index as i32)
        .bind(c.status.as_str())
        .bind(json!(c))
        .bind(json!(c.points))
        .bind(c.started_at)
        .bind(c.finished_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // --- publications ---------------------------------------------------------------------

    pub async fn insert_publication(&self, p: &NewPublication<'_>) -> Result<Value> {
        let row = sqlx::query(
            "INSERT INTO publications (id, run_id, destination, status, gw_report_id, digest, error, published_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id, published_at",
        )
        .bind(new_id("pub"))
        .bind(p.run_id)
        .bind(p.destination)
        .bind(p.status)
        .bind(p.gw_report_id)
        .bind(p.digest)
        .bind(p.error)
        .bind(p.by)
        .fetch_one(&self.pool)
        .await?;
        let at: DateTime<Utc> = row.get("published_at");
        Ok(json!({
            "id": row.get::<String, _>("id"), "run_id": p.run_id, "destination": p.destination, "status": p.status,
            "gw_report_id": p.gw_report_id, "digest": p.digest, "error": p.error, "published_by": p.by,
            "published_at": at,
        }))
    }

    pub async fn publications(&self, run_id: &str) -> Result<Vec<Value>> {
        let rows = sqlx::query(
            "SELECT id, destination, status, gw_report_id, digest, error, published_by, published_at
             FROM publications WHERE run_id = $1 ORDER BY published_at DESC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.get::<String, _>("id"),
                    "destination": r.get::<String, _>("destination"),
                    "status": r.get::<String, _>("status"),
                    "gw_report_id": r.get::<String, _>("gw_report_id"),
                    "digest": r.get::<String, _>("digest"),
                    "error": r.get::<String, _>("error"),
                    "published_by": r.get::<String, _>("published_by"),
                    "published_at": r.get::<DateTime<Utc>, _>("published_at"),
                })
            })
            .collect())
    }
}
