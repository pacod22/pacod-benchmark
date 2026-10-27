//! The agents' seconds added up: one point a second for all agents together, and a summary of
//! the cell, the way the tester's runner adds them (`tester/src/pgw_tester/load/runner.py`).
//!
//! Percentiles cannot be combined, histograms can: every second's histograms of every agent are
//! merged before a percentile is taken. Besides the whole cell, the summary covers its steady
//! window: the seconds from `load.warmup_s` to the end of the last stage, without the ramp and
//! without the drain, where the last calls finish while no new ones start.

use std::collections::BTreeMap;

use pacod_gw_load::contract::{Ended, Errors, Interval, Probe, Report, RunState};
use pacod_gw_load::histogram::Histogram;
use serde::{Deserialize, Serialize};
use tokio::time::{Duration, Instant};

use crate::model::Placement;
use crate::split::Share;

/// A second is shown without an agent that has not reported it after this long.
pub const STALE_AFTER: Duration = Duration::from_secs(3);
/// Failed reports in a row after which an agent counts as lost (its lease stops it).
pub const LOST_AFTER: u32 = 10;

/// Something a reader of a cell's figures should know: an agent short of CPU, calls refused,
/// other traffic at the gateway. `code` is from an open set; a reader that does not know one
/// shows the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flag {
    pub code: String,
    pub message: String,
}

impl Flag {
    pub fn new(code: &str, message: impl Into<String>) -> Flag {
        Flag { code: code.to_owned(), message: message.into() }
    }
}

/// Milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Pct {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

fn round(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    (v * f).round() / f
}

pub fn pct(h: &Histogram) -> Pct {
    Pct {
        p50: round(h.percentile(0.5) / 1000.0, 3),
        p95: round(h.percentile(0.95) / 1000.0, 3),
        p99: round(h.percentile(0.99) / 1000.0, 3),
        max: round(h.max as f64 / 1000.0, 3),
    }
}

/// One second of a cell, all agents together.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub t: u64,
    /// Calls at a time, or a second, that the agents aimed at.
    pub target: f64,
    pub started: u64,
    pub completed: u64,
    pub ok: u64,
    /// Calls without a complete answer, and answers that were not a success.
    pub failed: u64,
    pub cancelled: u64,
    pub status: BTreeMap<String, u64>,
    pub bytes: u64,
    pub in_flight: u64,
    pub skipped: u64,
    pub ttfb: Option<Pct>,
    pub total: Option<Pct>,
    pub gw_delay: Option<Pct>,
    /// The busiest agent's CPU use, in percent of its CPUs, and the worst scheduler lag.
    pub cpu_max: Option<f64>,
    pub lag_max_ms: f64,
    /// Calls in flight at the gateway (all instances), from its own figures.
    pub gw_in_flight: Option<i64>,
    /// Agents that reported this second.
    pub agents: u32,
}

/// The gateway's own figures during a cell (`GET /monitoring/live`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GatewaySample {
    /// Seconds since the cell's start.
    pub t: f64,
    pub in_flight: i64,
    /// The agents' calls in flight at the same moment, as they last reported.
    pub agents_in_flight: u64,
    pub rejected_per_second: f64,
    pub spool_bytes: i64,
    pub events_dropped: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub duration_s: f64,
    pub started: u64,
    pub calls: u64,
    pub ok: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub skipped: u64,
    pub status: BTreeMap<String, u64>,
    pub errors: Errors,
    pub bytes: u64,
    /// Calls finished a second, on average and in the busiest second.
    pub rps_mean: f64,
    pub rps_peak: f64,
    pub ttfb: Pct,
    pub total: Pct,
    /// None where no answer carried the gateway's own figure (`x-gw-delay-ms`): a product
    /// other than pacod-gw, or a baseline straight to the mock. Never shown as 0.
    pub gw_delay: Option<Pct>,
}

/// The steady window's histograms, which a report keeps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Histograms {
    pub ttfb: Histogram,
    pub total: Histogram,
    pub gw_delay: Histogram,
}

/// One agent in a cell.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentPart {
    pub name: String,
    pub placement: Placement,
    pub version: String,
    /// The agent's own route to the gateway; calls to another URL go there instead.
    pub target: String,
    pub cpus: u32,
    pub share: Option<Share>,
    pub state: String,
    pub ended: Option<Ended>,
    pub error: String,
    pub probe: Option<Probe>,
    pub calls: u64,
    /// The agent's highest CPU use, in percent of all its CPUs.
    pub cpu_max: f64,
    pub lag_max_ms: f64,
}

pub fn state_name(state: RunState) -> &'static str {
    match state {
        RunState::Prepared => "prepared",
        RunState::Running => "running",
        RunState::Draining => "draining",
        RunState::Done => "done",
        RunState::Cancelled => "cancelled",
        RunState::Failed => "failed",
    }
}

#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub started: u64,
    pub completed: u64,
    pub ok: u64,
    pub skipped: u64,
    pub bytes: u64,
    pub status: BTreeMap<String, u64>,
    pub errors: Errors,
    pub hist: Histograms,
    pub lag_max: f64,
}

impl Totals {
    fn add(&mut self, iv: &Interval) {
        self.started += iv.started;
        self.completed += iv.completed;
        self.ok += iv.ok;
        self.skipped += iv.skipped;
        self.bytes += iv.bytes;
        for (k, v) in &iv.status {
            *self.status.entry(k.to_string()).or_default() += v;
        }
        let e = &mut self.errors;
        e.connect += iv.errors.connect;
        e.timeout += iv.errors.timeout;
        e.body += iv.errors.body;
        e.other += iv.errors.other;
        e.cancelled += iv.errors.cancelled;
        self.lag_max = self.lag_max.max(iv.lag_ms.max);
        self.hist.ttfb.merge(&iv.ttfb);
        self.hist.total.merge(&iv.total);
        self.hist.gw_delay.merge(&iv.gw_delay);
    }
}

#[derive(Debug, Default)]
struct Leg {
    cpus: u32,
    last_t: Option<u64>,
    pending: BTreeMap<u64, Interval>,
    failures: u32,
    finished: bool,
    lost: bool,
    elapsed_ms: u64,
    in_flight: u64,
}

/// Adds up the reports of a cell's agents.
pub struct Collector {
    legs: Vec<Leg>,
    pub parts: Vec<AgentPart>,
    pub whole: Totals,
    pub steady: Totals,
    /// `[warmup_s, duration_s)`.
    window: (u64, u64),
    next_t: u64,
    first_seen: BTreeMap<u64, Instant>,
    pub points: Vec<Point>,
    pub gw_in_flight: Option<i64>,
    pub flags: Vec<Flag>,
}

impl Collector {
    pub fn new(parts: Vec<AgentPart>, window: (u64, u64)) -> Collector {
        let legs = parts.iter().map(|p| Leg { cpus: p.cpus.max(1), ..Leg::default() }).collect();
        Collector {
            legs,
            parts,
            whole: Totals::default(),
            steady: Totals::default(),
            window,
            next_t: 0,
            first_seen: BTreeMap::new(),
            points: Vec::new(),
            gw_in_flight: None,
            flags: Vec::new(),
        }
    }

    fn in_window(&self, t: u64) -> bool {
        t >= self.window.0 && t < self.window.1
    }

    fn count(&mut self, iv: &Interval) {
        self.whole.add(iv);
        if self.in_window(iv.t) {
            self.steady.add(iv);
        }
    }

    /// The agents still to be asked for their seconds.
    pub fn active(&self) -> Vec<usize> {
        (0..self.legs.len()).filter(|&i| !(self.legs[i].finished || self.legs[i].lost)).collect()
    }

    pub fn done(&self) -> bool {
        self.legs.iter().all(|l| l.finished || l.lost)
    }

    pub fn all_lost(&self) -> bool {
        self.legs.iter().all(|l| l.lost)
    }

    pub fn finished(&self, leg: usize) -> bool {
        self.legs[leg].finished
    }

    pub fn last_t(&self, leg: usize) -> Option<u64> {
        self.legs[leg].last_t
    }

    /// The agents' calls in flight, as they last reported.
    pub fn agents_in_flight(&self) -> u64 {
        self.legs.iter().filter(|l| !(l.finished || l.lost)).map(|l| l.in_flight).sum()
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.legs.iter().map(|l| l.elapsed_ms).max().unwrap_or(0)
    }

    pub fn any_draining(&self) -> bool {
        self.parts.iter().any(|p| p.state == "draining")
    }

    /// One agent's answer to `report?after=`.
    pub fn take(&mut self, leg: usize, rep: Report, now: Instant) {
        let l = &mut self.legs[leg];
        l.failures = 0;
        l.elapsed_ms = rep.elapsed_ms;
        l.in_flight = rep.in_flight;
        let part = &mut self.parts[leg];
        part.state = state_name(rep.state).into();
        part.ended = rep.ended;
        part.error = rep.error.clone();
        part.calls = rep.calls;
        let mut late = Vec::new();
        for iv in rep.intervals {
            let l = &mut self.legs[leg];
            l.last_t = Some(l.last_t.map_or(iv.t, |t| t.max(iv.t)));
            if iv.t < self.next_t {
                // Too late for its second's point: it still counts in the totals.
                late.push(iv);
                continue;
            }
            self.first_seen.entry(iv.t).or_insert(now);
            l.pending.insert(iv.t, iv);
        }
        for iv in late {
            self.count(&iv);
        }
        if rep.state.finished() {
            self.legs[leg].finished = true;
            if rep.ended == Some(Ended::LeaseExpired) {
                let name = &self.parts[leg].name;
                self.flags.push(Flag::new("lease_expired", format!("agent {name} stopped when its lease ran out")));
            }
        }
    }

    /// A report that did not come; true when the agent now counts as lost.
    pub fn failed(&mut self, leg: usize, why: &str) -> bool {
        let l = &mut self.legs[leg];
        l.failures += 1;
        if l.failures >= LOST_AFTER && !l.lost {
            l.lost = true;
            self.parts[leg].state = "lost".into();
            self.parts[leg].error = why.to_owned();
            let name = &self.parts[leg].name;
            self.flags.push(Flag::new(
                "agent_lost",
                format!("agent {name} stopped answering; its lease stopped its part of the load"),
            ));
            return true;
        }
        false
    }

    /// Turns the seconds every agent has reported into points; returns the new ones. With
    /// `flush`, a second goes out without waiting for agents that have not reported it.
    pub fn emit(&mut self, flush: bool, now: Instant) -> Vec<Point> {
        let mut out = Vec::new();
        loop {
            let t = self.next_t;
            let have: Vec<usize> = (0..self.legs.len()).filter(|&i| self.legs[i].pending.contains_key(&t)).collect();
            if have.is_empty() {
                let later = self.legs.iter().any(|l| l.pending.keys().any(|&k| k > t));
                if flush && later {
                    self.next_t += 1;
                    continue;
                }
                return out;
            }
            let waiting = self.legs.iter().any(|l| !(l.finished || l.lost) && l.last_t.is_none_or(|lt| lt < t));
            let seen = self.first_seen.get(&t).copied().unwrap_or(now);
            if waiting && !flush && now.duration_since(seen) < STALE_AFTER {
                return out;
            }
            let ivs: Vec<(usize, Interval)> =
                have.into_iter().map(|i| (i, self.legs[i].pending.remove(&t).expect("pending"))).collect();
            let point = self.point(t, ivs);
            self.points.push(point.clone());
            out.push(point);
            self.first_seen.remove(&t);
            self.next_t += 1;
        }
    }

    fn point(&mut self, t: u64, ivs: Vec<(usize, Interval)>) -> Point {
        let mut h = Histograms::default();
        let mut p = Point { t, agents: ivs.len() as u32, ..Point::default() };
        let mut cpu: Option<f64> = None;
        for (leg, iv) in &ivs {
            self.count(iv);
            h.ttfb.merge(&iv.ttfb);
            h.total.merge(&iv.total);
            h.gw_delay.merge(&iv.gw_delay);
            p.target += iv.target;
            p.started += iv.started;
            p.completed += iv.completed;
            p.ok += iv.ok;
            p.cancelled += iv.errors.cancelled;
            p.bytes += iv.bytes;
            p.in_flight += iv.in_flight;
            p.skipped += iv.skipped;
            p.lag_max_ms = p.lag_max_ms.max(iv.lag_ms.max);
            for (k, v) in &iv.status {
                *p.status.entry(k.to_string()).or_default() += v;
            }
            let part = &mut self.parts[*leg];
            if let Some(c) = iv.cpu_pct {
                let share = c / f64::from(self.legs[*leg].cpus);
                cpu = Some(cpu.map_or(share, |m: f64| m.max(share)));
                part.cpu_max = round(part.cpu_max.max(share), 1);
            }
            part.lag_max_ms = round(part.lag_max_ms.max(iv.lag_ms.max), 2);
        }
        p.failed = p.completed - p.ok;
        p.cpu_max = cpu.map(|c| round(c, 1));
        p.ttfb = (!h.ttfb.is_empty()).then(|| pct(&h.ttfb));
        p.total = (!h.total.is_empty()).then(|| pct(&h.total));
        p.gw_delay = (!h.gw_delay.is_empty()).then(|| pct(&h.gw_delay));
        p.gw_in_flight = self.gw_in_flight;
        p
    }

    /// The whole cell's summary.
    pub fn summary(&self) -> Summary {
        let duration = self.elapsed_ms() as f64 / 1000.0;
        summarise(&self.whole, duration, self.points.iter().map(|p| p.completed).max().unwrap_or(0))
    }

    /// The steady window's summary: the seconds of the window that were measured.
    pub fn steady_summary(&self) -> Summary {
        let steady: Vec<&Point> = self.points.iter().filter(|p| self.in_window(p.t)).collect();
        let peak = steady.iter().map(|p| p.completed).max().unwrap_or(0);
        summarise(&self.steady, steady.len() as f64, peak)
    }
}

fn summarise(tot: &Totals, duration: f64, peak: u64) -> Summary {
    Summary {
        duration_s: round(duration, 1),
        started: tot.started,
        calls: tot.completed,
        ok: tot.ok,
        failed: tot.completed - tot.ok,
        cancelled: tot.errors.cancelled,
        skipped: tot.skipped,
        status: tot.status.clone(),
        errors: tot.errors,
        bytes: tot.bytes,
        rps_mean: if duration > 0.0 { round(tot.completed as f64 / duration, 1) } else { 0.0 },
        rps_peak: peak as f64,
        ttfb: pct(&tot.hist.ttfb),
        total: pct(&tot.hist.total),
        gw_delay: (!tot.hist.gw_delay.is_empty()).then(|| pct(&tot.hist.gw_delay)),
    }
}

#[cfg(test)]
mod tests {
    use pacod_gw_load::contract::Lag;

    use super::*;

    fn iv(t: u64, ok: u64, ms: u64, gw_ms: Option<u64>) -> Interval {
        let mut ttfb = Histogram::default();
        let mut total = Histogram::default();
        let mut gw = Histogram::default();
        for _ in 0..ok {
            ttfb.record(ms * 500);
            total.record(ms * 1000);
            if let Some(g) = gw_ms {
                gw.record(g * 1000);
            }
        }
        Interval {
            t,
            target: 2.0,
            started: ok,
            completed: ok,
            ok,
            status: BTreeMap::from([(200, ok)]),
            lag_ms: Lag::default(),
            cpu_pct: Some(50.0),
            ttfb,
            total,
            gw_delay: gw,
            ..Interval::default()
        }
    }

    fn report(state: RunState, intervals: Vec<Interval>) -> Report {
        Report {
            run_id: "r".into(),
            state,
            ended: state.finished().then_some(Ended::Duration),
            error: String::new(),
            elapsed_ms: intervals.iter().map(|i| (i.t + 1) * 1000).max().unwrap_or(0),
            in_flight: 0,
            calls: intervals.iter().map(|i| i.completed).sum(),
            intervals,
        }
    }

    fn parts(n: usize) -> Vec<AgentPart> {
        (0..n).map(|i| AgentPart { name: format!("a{i}"), cpus: 2, ..AgentPart::default() }).collect()
    }

    #[test]
    fn a_second_waits_for_every_agent_until_it_is_stale() {
        let t0 = Instant::now();
        let mut c = Collector::new(parts(2), (0, 10));
        c.take(0, report(RunState::Running, vec![iv(0, 3, 100, Some(2))]), t0);
        assert!(c.emit(false, t0).is_empty(), "agent 1 has not reported second 0");
        let p = c.emit(false, t0 + STALE_AFTER);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].agents, p[0].ok), (1, 3));
        // Agent 1's second 0 comes too late for the point but still counts.
        c.take(1, report(RunState::Running, vec![iv(0, 2, 100, Some(2))]), t0 + STALE_AFTER);
        assert_eq!(c.whole.ok, 5);
    }

    #[test]
    fn the_steady_window_leaves_out_the_ramp_and_the_drain() {
        let t0 = Instant::now();
        let mut c = Collector::new(parts(1), (1, 3));
        let ivs = vec![iv(0, 10, 900, None), iv(1, 10, 100, None), iv(2, 10, 100, None), iv(3, 1, 900, None)];
        c.take(0, report(RunState::Done, ivs), t0);
        c.emit(true, t0);
        assert!(c.done());
        let whole = c.summary();
        let steady = c.steady_summary();
        assert_eq!((whole.calls, steady.calls), (31, 20));
        assert_eq!(steady.duration_s, 2.0);
        assert_eq!(steady.rps_mean, 10.0);
        assert!((steady.total.p99 - 100.0).abs() < 1.0, "{:?}", steady.total);
        assert!(whole.total.p99 > 800.0);
        assert!(steady.gw_delay.is_none(), "no answer carried x-gw-delay-ms");
    }

    #[test]
    fn cpu_is_in_percent_of_the_agents_cpus() {
        let t0 = Instant::now();
        let mut c = Collector::new(parts(1), (0, 1));
        c.take(0, report(RunState::Done, vec![iv(0, 1, 10, Some(1))]), t0);
        let p = c.emit(true, t0);
        assert_eq!(p[0].cpu_max, Some(25.0));
        assert_eq!(c.parts[0].cpu_max, 25.0);
        assert!(c.summary().gw_delay.is_some());
    }

    #[test]
    fn an_agent_is_lost_after_ten_failed_reports() {
        let mut c = Collector::new(parts(1), (0, 1));
        for _ in 0..9 {
            assert!(!c.failed(0, "no answer"));
        }
        assert!(c.failed(0, "no answer"));
        assert!(c.all_lost() && c.done());
    }
}
