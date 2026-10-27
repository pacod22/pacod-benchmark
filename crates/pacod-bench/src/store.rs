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
    /// A gateway added through the UI: a label of its admin token, and who the gateway said the
    /// token was at its last check. The sealed token itself is no field: it is read on its own
    /// ([`Store::sealed_token`]) and never leaves the API.
    pub token_hint: String,
    pub token_checked: Option<Value>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A gateway's admin token as it is written: sealed, its label, and its check.
pub struct StoredToken {
    pub sealed: Value,
    pub hint: String,
    pub checked: Value,
}

pub struct NewTarget<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub kind: &'a str,
    pub product: &'a str,
    pub definition: &'a Value,
    pub declared: &'a Value,
    pub by: &'a str,
    pub token: Option<&'a StoredToken>,
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
    /// The settings the run started with (`settings.rs`); none for runs older than them.
    pub settings: Option<Value>,
    /// Where a queued run stands in the queue: the lowest starts first.
    pub position: i64,
    /// The schedule that started the run, if one did.
    pub schedule_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ScheduleRow {
    pub id: String,
    pub name: String,
    pub suite_id: String,
    pub target_id: String,
    pub agents: Option<Value>,
    pub label: String,
    pub cadence: Value,
    pub enabled: bool,
    pub publish: bool,
    pub created_by: String,
    pub created_by_name: String,
    pub created_by_kind: String,
    pub next_fire_at: Option<DateTime<Utc>>,
    pub last_fire_at: Option<DateTime<Utc>>,
    pub last_run_id: Option<String>,
    pub last_outcome: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A schedule as it is written: what an operator sets.
pub struct ScheduleInput<'a> {
    pub name: &'a str,
    pub suite_id: &'a str,
    pub target_id: &'a str,
    pub agents: Option<&'a Value>,
    pub label: &'a str,
    pub cadence: &'a Value,
    pub enabled: bool,
    pub publish: bool,
    pub next_fire_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AgentRow {
    pub name: String,
    pub url: String,
    pub placement: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    pub settings: Value,
    pub schedule_id: Option<String>,
}

/// Where [`Store::move_queued`] moves a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Move {
    Up,
    Down,
    Top,
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

    pub async fn target_by_name(&self, name: &str) -> Result<Option<TargetRow>> {
        sqlx::query_as("SELECT * FROM targets WHERE name = $1").bind(name).fetch_optional(&self.pool).await
    }

    /// A target made through the API: an external target, or a gateway with its sealed token.
    pub async fn insert_target(&self, t: &NewTarget<'_>) -> Result<TargetRow> {
        sqlx::query_as(
            "INSERT INTO targets (id, name, kind, product, source, definition, declared, created_by,
                                  sealed_token, token_hint, token_checked)
             VALUES ($1, $2, $3, $4, 'api', $5, $6, $7, $8, $9, $10) RETURNING *",
        )
        .bind(t.id)
        .bind(t.name)
        .bind(t.kind)
        .bind(t.product)
        .bind(t.definition)
        .bind(t.declared)
        .bind(t.by)
        .bind(t.token.map(|k| &k.sealed))
        .bind(t.token.map_or("", |k| k.hint.as_str()))
        .bind(t.token.map(|k| &k.checked))
        .fetch_one(&self.pool)
        .await
    }

    /// Targets made through the API only: a gateway of the configuration changes there. The
    /// token stays as it is unless a new one is given.
    pub async fn update_target(
        &self,
        id: &str,
        name: &str,
        product: &str,
        definition: &Value,
        declared: &Value,
        token: Option<&StoredToken>,
    ) -> Result<Option<TargetRow>> {
        sqlx::query_as(
            "UPDATE targets SET name = $2, product = $3, definition = $4, declared = $5, updated_at = now(),
                sealed_token = CASE WHEN $6 THEN $7 ELSE sealed_token END,
                token_hint = CASE WHEN $6 THEN $8 ELSE token_hint END,
                token_checked = CASE WHEN $6 THEN $9 ELSE token_checked END
             WHERE id = $1 AND source = 'api' RETURNING *",
        )
        .bind(id)
        .bind(name)
        .bind(product)
        .bind(definition)
        .bind(declared)
        .bind(token.is_some())
        .bind(token.map(|k| &k.sealed))
        .bind(token.map_or("", |k| k.hint.as_str()))
        .bind(token.map(|k| &k.checked))
        .fetch_optional(&self.pool)
        .await
    }

    /// A gateway's sealed admin token, read on its own.
    pub async fn sealed_token(&self, id: &str) -> Result<Option<Value>> {
        let row =
            sqlx::query("SELECT sealed_token FROM targets WHERE id = $1").bind(id).fetch_optional(&self.pool).await?;
        Ok(row.and_then(|r| r.get::<Option<Value>, _>(0)))
    }

    /// Every sealed token with what it is bound to: `(id, admin_url, sealed)`.
    pub async fn sealed_tokens(&self) -> Result<Vec<(String, String, Value)>> {
        let rows = sqlx::query(
            "SELECT id, definition->>'admin_url', sealed_token FROM targets WHERE sealed_token IS NOT NULL ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.get(0), r.get::<Option<String>, _>(1).unwrap_or_default(), r.get(2))).collect())
    }

    /// Who the gateway took the token for at a check.
    pub async fn set_token_checked(&self, id: &str, checked: &Value) -> Result<()> {
        sqlx::query("UPDATE targets SET token_checked = $2 WHERE id = $1")
            .bind(id)
            .bind(checked)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The token wrapped again under another key (`rewrap-secrets`): the same secret.
    pub async fn set_sealed(&self, id: &str, sealed: &Value) -> Result<()> {
        sqlx::query("UPDATE targets SET sealed_token = $2 WHERE id = $1")
            .bind(id)
            .bind(sealed)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Why the target may not be removed or pointed elsewhere now: a run on it waits or goes,
    /// or left objects on it. None when nothing holds it.
    pub async fn target_in_use(&self, id: &str) -> Result<Option<String>> {
        let row = sqlx::query(&format!(
            "SELECT id, status FROM runs WHERE target_id = $1
               AND (status = 'queued' OR status IN {ACTIVE} OR leftovers <> '[]'::jsonb)
             ORDER BY created_at LIMIT 1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(r) = row {
            let (run, status): (String, String) = (r.get(0), r.get(1));
            return Ok(Some(if status == "queued" || ACTIVE.contains(&format!("'{status}'")) {
                format!("the run {run} on this target is {status}; wait for it or cancel it")
            } else {
                format!("the run {run} left objects on this gateway; clean them up first")
            }));
        }
        let schedule = sqlx::query("SELECT name FROM schedules WHERE target_id = $1 AND enabled ORDER BY name LIMIT 1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(schedule.map(|r| format!("the schedule {} runs on this target; switch it off first", r.get::<String, _>(0))))
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

    // --- agents and settings -------------------------------------------------------------

    /// The agents added through the UI.
    pub async fn agents(&self) -> Result<Vec<AgentRow>> {
        sqlx::query_as("SELECT * FROM agents ORDER BY name").fetch_all(&self.pool).await
    }

    pub async fn insert_agent(&self, name: &str, url: &str, placement: &str, by: &str) -> Result<AgentRow> {
        sqlx::query_as("INSERT INTO agents (name, url, placement, created_by) VALUES ($1, $2, $3, $4) RETURNING *")
            .bind(name)
            .bind(url)
            .bind(placement)
            .bind(by)
            .fetch_one(&self.pool)
            .await
    }

    pub async fn update_agent(&self, name: &str, url: &str, placement: &str) -> Result<Option<AgentRow>> {
        sqlx::query_as("UPDATE agents SET url = $2, placement = $3, updated_at = now() WHERE name = $1 RETURNING *")
            .bind(name)
            .bind(url)
            .bind(placement)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn delete_agent(&self, name: &str) -> Result<bool> {
        let done = sqlx::query("DELETE FROM agents WHERE name = $1").bind(name).execute(&self.pool).await?;
        Ok(done.rows_affected() > 0)
    }

    /// Why the agent may not change or go now: a run that waits or goes uses it.
    pub async fn agent_in_use(&self, name: &str) -> Result<Option<String>> {
        let row = sqlx::query(&format!(
            "SELECT id, status FROM runs WHERE (status = 'queued' OR status IN {ACTIVE})
               AND jsonb_exists(agents->'names', $1) ORDER BY created_at LIMIT 1"
        ))
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            let (run, status): (String, String) = (r.get(0), r.get(1));
            format!("the run {run}, which uses this agent, is {status}; wait for it or cancel it")
        }))
    }

    /// The run settings of the UI; none when nobody changed them.
    pub async fn settings(&self) -> Result<Option<Value>> {
        let row = sqlx::query("SELECT value FROM settings WHERE id = 1").fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.get(0)))
    }

    pub async fn save_settings(&self, value: &Value, by: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (id, value, updated_by) VALUES (1, $1, $2)
             ON CONFLICT (id) DO UPDATE SET value = $1, updated_by = $2, updated_at = now()",
        )
        .bind(value)
        .bind(by)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // --- schedules ------------------------------------------------------------------------

    pub async fn schedules(&self) -> Result<Vec<ScheduleRow>> {
        sqlx::query_as("SELECT * FROM schedules ORDER BY name, id").fetch_all(&self.pool).await
    }

    pub async fn schedule(&self, id: &str) -> Result<Option<ScheduleRow>> {
        sqlx::query_as("SELECT * FROM schedules WHERE id = $1").bind(id).fetch_optional(&self.pool).await
    }

    pub async fn insert_schedule(
        &self,
        id: &str,
        s: &ScheduleInput<'_>,
        by: (&str, &str, &str),
    ) -> Result<ScheduleRow> {
        sqlx::query_as(
            "INSERT INTO schedules (id, name, suite_id, target_id, agents, label, cadence, enabled, publish,
                                    next_fire_at, created_by, created_by_name, created_by_kind)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) RETURNING *",
        )
        .bind(id)
        .bind(s.name)
        .bind(s.suite_id)
        .bind(s.target_id)
        .bind(s.agents)
        .bind(s.label)
        .bind(s.cadence)
        .bind(s.enabled)
        .bind(s.publish)
        .bind(s.next_fire_at)
        .bind(by.0)
        .bind(by.1)
        .bind(by.2)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn update_schedule(&self, id: &str, s: &ScheduleInput<'_>) -> Result<Option<ScheduleRow>> {
        sqlx::query_as(
            "UPDATE schedules SET name = $2, suite_id = $3, target_id = $4, agents = $5, label = $6, cadence = $7,
                enabled = $8, publish = $9, next_fire_at = $10, updated_at = now()
             WHERE id = $1 RETURNING *",
        )
        .bind(id)
        .bind(s.name)
        .bind(s.suite_id)
        .bind(s.target_id)
        .bind(s.agents)
        .bind(s.label)
        .bind(s.cadence)
        .bind(s.enabled)
        .bind(s.publish)
        .bind(s.next_fire_at)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_schedule(&self, id: &str) -> Result<bool> {
        let done = sqlx::query("DELETE FROM schedules WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(done.rows_affected() > 0)
    }

    /// The enabled schedules whose fire is due.
    pub async fn due_schedules(&self) -> Result<Vec<ScheduleRow>> {
        sqlx::query_as("SELECT * FROM schedules WHERE enabled AND next_fire_at <= now() ORDER BY next_fire_at")
            .fetch_all(&self.pool)
            .await
    }

    /// Claims a due fire: moves the schedule's next fire on, if nobody did since it was read.
    /// Whether this caller has the fire.
    pub async fn advance_schedule(&self, id: &str, due: DateTime<Utc>, next: Option<DateTime<Utc>>) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE schedules SET next_fire_at = $3, last_fire_at = now() WHERE id = $1 AND next_fire_at = $2",
        )
        .bind(id)
        .bind(due)
        .bind(next)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    /// What a fire did: the run it queued, or why none; `disable` switches the schedule off.
    pub async fn schedule_outcome(&self, id: &str, outcome: &str, run_id: Option<&str>, disable: bool) -> Result<()> {
        sqlx::query(
            "UPDATE schedules SET last_outcome = $2, last_run_id = COALESCE($3, last_run_id),
                enabled = enabled AND NOT $4 WHERE id = $1",
        )
        .bind(id)
        .bind(outcome)
        .bind(run_id)
        .bind(disable)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A fire put off: the schedule tries again at `at`.
    pub async fn defer_schedule(&self, id: &str, at: DateTime<Utc>, outcome: &str) -> Result<()> {
        sqlx::query("UPDATE schedules SET next_fire_at = $2, last_outcome = $3 WHERE id = $1")
            .bind(id)
            .bind(at)
            .bind(outcome)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The name of a schedule that uses this suite, if one does.
    pub async fn schedule_of_suite(&self, suite_id: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT name FROM schedules WHERE suite_id = $1 ORDER BY name LIMIT 1")
            .bind(suite_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get(0)))
    }

    /// The name of a schedule that names this agent, if one does.
    pub async fn schedule_of_agent(&self, name: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT name FROM schedules WHERE jsonb_exists(agents, $1) ORDER BY name LIMIT 1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get(0)))
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
                               started_by, started_by_name, key_hint, settings, schedule_id)
             VALUES ($1, 'queued', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) RETURNING *",
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
        .bind(&run.settings)
        .bind(&run.schedule_id)
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

    /// The run that goes, if one does, then the queued runs in the order they start.
    pub async fn queue(&self) -> Result<Vec<RunRow>> {
        sqlx::query_as(&format!(
            "SELECT * FROM runs WHERE status = 'queued' OR status IN {ACTIVE}
             ORDER BY status = 'queued', position, created_at"
        ))
        .fetch_all(&self.pool)
        .await
    }

    /// Moves a queued run one place up or down, or to the front; false when it is not queued.
    pub async fn move_queued(&self, id: &str, to: Move) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let queued: Vec<(String, i64)> = sqlx::query_as(
            "SELECT id, position FROM runs WHERE status = 'queued' ORDER BY position, created_at FOR UPDATE",
        )
        .fetch_all(&mut *tx)
        .await?;
        let Some(at) = queued.iter().position(|(q, _)| q == id) else { return Ok(false) };
        let set = |run: &str, position: i64| {
            sqlx::query("UPDATE runs SET position = $2 WHERE id = $1").bind(run.to_owned()).bind(position)
        };
        match to {
            Move::Top if at > 0 => {
                set(id, queued[0].1 - 1).execute(&mut *tx).await?;
            }
            Move::Up if at > 0 => {
                let (other, theirs) = &queued[at - 1];
                // Equal positions (never written so, but cheap to survive) swap by one apart.
                let mine = queued[at].1.max(theirs + 1);
                set(id, *theirs).execute(&mut *tx).await?;
                set(other, mine).execute(&mut *tx).await?;
            }
            Move::Down if at + 1 < queued.len() => {
                let (other, theirs) = &queued[at + 1];
                let mine = queued[at].1.min(theirs - 1);
                set(id, *theirs).execute(&mut *tx).await?;
                set(other, mine).execute(&mut *tx).await?;
            }
            _ => {}
        }
        tx.commit().await?;
        Ok(true)
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
