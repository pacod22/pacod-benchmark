//! The benchmark's controller: what a benchmark run does, without a database or an HTTP
//! server of its own. `pacod-bench serve` and `pacod-bench run` share it.
//!
//! - [`plan`]: a suite (one base configuration and the axes it varies) expanded into cells,
//!   checked, ordered and estimated.
//! - [`gw`]: the gateway's management API, the few calls the benchmark makes and no others.
//! - [`scene`]: what a run sets up on a gateway (mock providers, a scratch organisation, its
//!   workspaces, a key per cell) and takes down after it.
//! - [`agents`], [`split`], [`requests`]: the load agents (pacod-gw's `pacod-gw-load-agent`),
//!   each agent's share of the load, and the one request every call of a cell sends.
//! - [`collect`]: the agents' seconds added up into one point a second and a summary.
//! - [`controller`]: one run, cell by cell, reported as [`controller::RunEvent`]s.
//! - [`measure`]: whether a run's figures show capacity, or only behaviour, and why.
//! - [`report`]: a run as the report the gateway takes (`pacod-bench-report/1`).
//!
//! The orchestration follows pacod-gw's tester (`tester/src/pgw_tester/load/`), which drives
//! the same agents for one load test at a time; its timings and checks are kept.

pub mod agents;
pub mod collect;
pub mod controller;
pub mod gw;
pub mod measure;
pub mod model;
pub mod plan;
pub mod report;
pub mod requests;
pub mod scene;
pub mod split;
#[cfg(feature = "testing")]
pub mod testing;
