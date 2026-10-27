//! Runs on a timetable: a suite on a managed gateway, daily at a time on some weekdays or every
//! few hours, in UTC.
//!
//! A schedule fires with its creator's authority: at every fire the benchmark asks its copy of
//! pacod-id's roles whether the creator may still start runs, and switches the schedule off when
//! not. While that copy is not current, a fire waits, rather than counting as a lost role. A fire
//! missed by more than [`MISSED`] (the benchmark was stopped) is skipped, not made up for, and so
//! is a fire while the schedule's previous run still waits or goes. A schedule that publishes
//! sends the report when its run has finished.

use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::api::runs::{Enqueue, enqueue, publish_run};
use crate::api::{AppState, AuthMode, Principal};
use crate::store::ScheduleRow;

/// A fire later than this is skipped.
pub const MISSED: TimeDelta = TimeDelta::minutes(15);
/// How often the scheduler looks for due fires.
const TICK: Duration = Duration::from_secs(30);
/// How long a fire waits for a current copy of the roles before it tries again.
const RETRY: TimeDelta = TimeDelta::minutes(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Cadence {
    /// At `at` (`HH:MM`, UTC) on the `weekdays` (1 Monday to 7 Sunday); none: every day.
    Daily {
        at: String,
        #[serde(default)]
        weekdays: Vec<u32>,
    },
    /// Every `hours` hours, counted from when the schedule was saved or last fired.
    Every { hours: u32 },
}

impl Cadence {
    pub fn check(&self) -> Result<(), String> {
        match self {
            Cadence::Daily { at, weekdays } => {
                NaiveTime::parse_from_str(at, "%H:%M").map_err(|_| "cadence.at is a time, HH:MM in UTC".to_owned())?;
                if weekdays.iter().any(|d| !(1..=7).contains(d)) {
                    return Err("cadence.weekdays are 1 (Monday) to 7 (Sunday)".into());
                }
                Ok(())
            }
            Cadence::Every { hours } if (1..=168).contains(hours) => Ok(()),
            Cadence::Every { .. } => Err("cadence.hours is 1 to 168".into()),
        }
    }

    /// The first fire after `now`.
    pub fn next_after(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Cadence::Every { hours } => now + TimeDelta::hours(i64::from(*hours)),
            Cadence::Daily { at, weekdays } => {
                let time = NaiveTime::parse_from_str(at, "%H:%M").unwrap_or(NaiveTime::MIN);
                // Today's, then each following day's, until a weekday of the schedule: eight days
                // reach every weekday once more.
                (0..8)
                    .map(|d| (now.date_naive() + TimeDelta::days(d)).and_time(time).and_utc())
                    .find(|t| *t > now && (weekdays.is_empty() || weekdays.contains(&t.weekday().number_from_monday())))
                    .unwrap_or(now + TimeDelta::days(1))
            }
        }
    }
}

/// What a fire did, in a sentence for the schedule's page.
enum Fired {
    Queued(String),
    Skipped(String),
    /// The schedule is switched off: it cannot fire again as it is.
    Off(String),
    /// Not now; again at the time given.
    Later(DateTime<Utc>, String),
}

/// What the copy of pacod-id's roles says of a schedule's creator and one permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    May,
    MayNot,
    /// The copy is not current: ask again later rather than count it as a lost role.
    Unknown,
}

/// The decision, apart from where its facts come from: whether the copy is current, whether the
/// account is disabled, and what the creator's bindings allow.
fn standing(fresh: bool, disabled: bool, creator: &Principal, permission: &str) -> Standing {
    if !fresh {
        Standing::Unknown
    } else if disabled || !creator.allowed(permission) {
        Standing::MayNot
    } else {
        Standing::May
    }
}

/// Whether the schedule's creator holds `permission` now.
fn creator_standing(st: &AppState, s: &ScheduleRow, permission: &str) -> Standing {
    match st.auth.as_ref() {
        AuthMode::Dev { .. } => Standing::May,
        AuthMode::Oidc(id) => {
            let kind = if s.created_by_kind == "service" { "service" } else { "user" };
            let creator = Principal::new(
                s.created_by.clone(),
                s.created_by_name.clone(),
                kind,
                id.cache.bindings_of(&s.created_by),
            );
            standing(id.fresh(), id.cache.is_disabled(&s.created_by), &creator, permission)
        }
    }
}

/// One fire of a schedule: the checks, then its run queued. `by` is who pressed "fire now", if
/// someone did; a timed fire acts as the creator.
async fn fire(st: &AppState, s: &ScheduleRow, by: Option<(&str, &str)>) -> Fired {
    if let Some(prev) = &s.last_run_id
        && let Ok(Some(run)) = st.store.run(prev).await
        && (run.status == "queued" || ["preparing", "running", "cleaning"].contains(&run.status.as_str()))
    {
        return Fired::Skipped(format!("skipped: the previous run {prev} is still {}", run.status));
    }
    if by.is_none() {
        match creator_standing(st, s, "bench.runs.start") {
            Standing::Unknown => {
                return Fired::Later(Utc::now() + RETRY, "waiting: pacod-id's copy of the roles is not current".into());
            }
            Standing::MayNot => {
                return Fired::Off(format!("switched off: {} may no longer start runs", s.created_by_name));
            }
            Standing::May => {}
        }
    }
    let suite = match st.store.suite(&s.suite_id).await {
        Ok(Some(row)) => match serde_json::from_value(row.document) {
            Ok(suite) => suite,
            Err(e) => return Fired::Off(format!("switched off: the suite does not read: {e}")),
        },
        Ok(None) => return Fired::Off("switched off: the suite is gone".into()),
        Err(e) => return Fired::Skipped(format!("skipped: the database failed: {e}")),
    };
    let agents: Option<Vec<String>> = s.agents.clone().and_then(|a| serde_json::from_value(a).ok());
    let (by_id, by_name) = by.unwrap_or((&s.created_by, &s.created_by_name));
    let label = if s.label.is_empty() { format!("schedule: {}", s.name) } else { s.label.clone() };
    let queued = enqueue(
        st,
        Enqueue {
            suite,
            suite_id: Some(s.suite_id.clone()),
            target_id: s.target_id.clone(),
            agents,
            label,
            key: None,
            acknowledge_cost: false,
            by_id: by_id.to_owned(),
            by_name: by_name.to_owned(),
            schedule_id: Some(s.id.clone()),
        },
    )
    .await;
    match queued {
        Ok(run) => Fired::Queued(run.id),
        Err(e) if e.status == axum::http::StatusCode::NOT_FOUND => {
            Fired::Off("switched off: the target is gone".into())
        }
        Err(e) => Fired::Skipped(format!("not started: {}", e.message)),
    }
}

/// Records a fire; the run it queued, if it did.
async fn record(st: &AppState, s: &ScheduleRow, fired: Fired) -> Result<Option<String>, String> {
    let r = match &fired {
        Fired::Queued(run) => {
            st.store.schedule_outcome(&s.id, &format!("queued the run {run}"), Some(run), false).await
        }
        Fired::Skipped(why) => st.store.schedule_outcome(&s.id, why, None, false).await,
        Fired::Off(why) => st.store.schedule_outcome(&s.id, why, None, true).await,
        Fired::Later(at, why) => st.store.defer_schedule(&s.id, *at, why).await,
    };
    if let Err(e) = r {
        tracing::warn!(schedule = %s.id, error = %e, "recording a schedule's fire failed");
    }
    match fired {
        Fired::Queued(run) => {
            tracing::info!(schedule = %s.id, name = %s.name, run_id = %run, "a schedule queued a run");
            Ok(Some(run))
        }
        Fired::Skipped(why) | Fired::Off(why) | Fired::Later(_, why) => {
            tracing::info!(schedule = %s.id, name = %s.name, outcome = %why, "a schedule did not queue a run");
            Err(why)
        }
    }
}

/// "Fire now", by someone who may start runs: the run it queued, or why none.
pub async fn fire_now(st: &AppState, s: &ScheduleRow, by: &Principal) -> Result<String, String> {
    let fired = fire(st, s, Some((&by.id, &by.display))).await;
    record(st, s, fired).await?.ok_or_else(|| "no run was queued".to_owned())
}

/// The fires that are due now.
async fn tick(st: &AppState) {
    let due = match st.store.due_schedules().await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "reading the due schedules failed");
            return;
        }
    };
    let now = Utc::now();
    for s in due {
        let (Some(at), Ok(cadence)) = (s.next_fire_at, serde_json::from_value::<Cadence>(s.cadence.clone())) else {
            continue;
        };
        match st.store.advance_schedule(&s.id, at, Some(cadence.next_after(now))).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                tracing::warn!(schedule = %s.id, error = %e, "claiming a schedule's fire failed");
                continue;
            }
        }
        let fired = if now - at > MISSED {
            Fired::Skipped(format!(
                "skipped: the fire of {} was missed while the benchmark was stopped",
                at.format("%F %R UTC")
            ))
        } else {
            fire(st, &s, None).await
        };
        let _ = record(st, &s, fired).await;
    }
}

/// A run of a schedule that publishes has ended: its report goes to pacod-gw when it finished.
async fn ended(st: &AppState, run_id: &str) {
    let Ok(Some(run)) = st.store.run(run_id).await else { return };
    let Some(schedule_id) = &run.schedule_id else { return };
    let Ok(Some(s)) = st.store.schedule(schedule_id).await else { return };
    if !s.publish || run.status != "finished" {
        return;
    }
    // Publishing acts with the creator's authority too, checked again now.
    let why = match creator_standing(st, &s, "bench.reports.publish") {
        Standing::May => None,
        Standing::MayNot => Some(format!("{} may no longer publish reports", s.created_by_name)),
        Standing::Unknown => Some("pacod-id's copy of the roles is not current; publish it from the run's page".into()),
    };
    if let Some(why) = why {
        let _ = st
            .store
            .schedule_outcome(&s.id, &format!("the report of {run_id} was not published: {why}"), None, false)
            .await;
        return;
    }
    let by = format!("{} (schedule {})", s.created_by_name, s.name);
    if let Err(e) = publish_run(st, &run, &by).await {
        tracing::warn!(run_id, schedule = %s.id, error = %e.message, "a scheduled run's report was not published");
        let _ = st
            .store
            .schedule_outcome(&s.id, &format!("the report of {run_id} was not published: {}", e.message), None, false)
            .await;
    }
}

/// The scheduler: due fires every [`TICK`], and the reports of scheduled runs as they end.
pub fn spawn(st: AppState, mut ended_runs: mpsc::UnboundedReceiver<String>, mut stop: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(TICK);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            // What comes first is chosen, then done: the stop's future holds a guard that may not
            // wait across the work.
            let next = tokio::select! {
                _ = every.tick() => None,
                Some(run_id) = ended_runs.recv() => Some(run_id),
                _ = stop.wait_for(|s| *s) => return,
            };
            match next {
                None => tick(&st).await,
                Some(run_id) => ended(&st, &run_id).await,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use pacod_id_authz::{Binding, ScopeRef};

    use super::*;

    fn creator(role: &str) -> Principal {
        Principal::new("u1".into(), "a@b".into(), "user", vec![Binding::new(role, vec![ScopeRef::platform()])])
    }

    #[test]
    fn a_creator_is_checked_again_at_every_fire() {
        let operator = creator("bench.operator");
        assert_eq!(standing(true, false, &operator, "bench.runs.start"), Standing::May);
        assert_eq!(
            standing(true, false, &creator("bench.viewer"), "bench.runs.start"),
            Standing::MayNot,
            "lost the role"
        );
        assert_eq!(standing(true, true, &operator, "bench.runs.start"), Standing::MayNot, "the account is disabled");
        assert_eq!(
            standing(false, false, &operator, "bench.runs.start"),
            Standing::Unknown,
            "an outage is no lost role"
        );
        assert_eq!(standing(false, true, &creator("bench.viewer"), "bench.runs.start"), Standing::Unknown);
        assert_eq!(standing(true, false, &operator, "bench.reports.publish"), Standing::May);
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    #[test]
    fn a_daily_cadence_fires_at_its_time_on_its_weekdays() {
        // 2026-10-27 is a Tuesday.
        let workdays = Cadence::Daily { at: "02:30".into(), weekdays: vec![1, 2, 3, 4, 5] };
        assert_eq!(workdays.next_after(at(2026, 10, 27, 1, 0)), at(2026, 10, 27, 2, 30), "later today");
        assert_eq!(
            workdays.next_after(at(2026, 10, 27, 2, 30)),
            at(2026, 10, 28, 2, 30),
            "never the same minute again"
        );
        assert_eq!(workdays.next_after(at(2026, 10, 30, 23, 59)), at(2026, 11, 2, 2, 30), "over the weekend");
        let sundays = Cadence::Daily { at: "00:00".into(), weekdays: vec![7] };
        assert_eq!(sundays.next_after(at(2026, 10, 27, 12, 0)), at(2026, 11, 1, 0, 0));
        assert_eq!(sundays.next_after(at(2026, 11, 1, 0, 0)), at(2026, 11, 8, 0, 0), "a week on");
        let daily = Cadence::Daily { at: "23:45".into(), weekdays: vec![] };
        assert_eq!(daily.next_after(at(2026, 12, 31, 23, 50)), at(2027, 1, 1, 23, 45), "across the year");
    }

    #[test]
    fn an_every_cadence_counts_from_now() {
        let c = Cadence::Every { hours: 6 };
        assert_eq!(c.next_after(at(2026, 10, 27, 22, 0)), at(2026, 10, 28, 4, 0));
    }

    #[test]
    fn a_cadence_is_checked() {
        assert!(Cadence::Daily { at: "24:00".into(), weekdays: vec![] }.check().is_err());
        assert!(Cadence::Daily { at: "07:00".into(), weekdays: vec![0] }.check().is_err());
        assert!(Cadence::Every { hours: 0 }.check().is_err());
        assert!(Cadence::Every { hours: 169 }.check().is_err());
        let read: Cadence = serde_json::from_value(serde_json::json!({ "kind": "daily", "at": "07:00" })).unwrap();
        assert!(read.check().is_ok());
        assert!(serde_json::from_value::<Cadence>(serde_json::json!({ "kind": "hourly" })).is_err());
    }
}
