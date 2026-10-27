//! The load split across agents: equal shares, the remainder to the first agents. The same
//! split as the tester's (`tester/src/pgw_tester/load/split.py`).

use pacod_gw_load::contract::{Mode, Stage};
use serde::{Deserialize, Serialize};

use crate::model::LoadShape;

/// One agent's part of a cell's load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Share {
    pub stages: Vec<Stage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_in_flight: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_calls: Option<u64>,
}

impl Share {
    pub fn peak(&self) -> f64 {
        self.stages.iter().map(|s| s.target).fold(0.0, f64::max)
    }
}

pub fn split_int(total: u64, n: usize) -> Vec<u64> {
    let n64 = n as u64;
    let (base, extra) = (total / n64, total % n64);
    (0..n64).map(|i| base + u64::from(i < extra)).collect()
}

/// Each agent's stages, in-flight cap and calls. Concurrency targets are whole calls; rates are
/// split exactly. `budget` is the calls left for the agents after their probes.
pub fn shares(shape: &LoadShape, n: usize, budget: Option<u64>) -> Vec<Share> {
    assert!(n > 0, "a cell has at least one agent");
    let per_stage: Vec<Vec<f64>> = shape
        .stages
        .iter()
        .map(|s| match shape.mode {
            Mode::Concurrency => split_int(s.target.round() as u64, n).into_iter().map(|c| c as f64).collect(),
            Mode::Rate => vec![s.target / n as f64; n],
        })
        .collect();
    let in_flight: Vec<Option<u32>> = match shape.max_in_flight {
        Some(m) => split_int(u64::from(m), n).into_iter().map(|c| Some(c as u32)).collect(),
        None => vec![None; n],
    };
    let calls: Vec<Option<u64>> = match budget {
        Some(b) => split_int(b, n).into_iter().map(Some).collect(),
        None => vec![None; n],
    };
    (0..n)
        .map(|i| Share {
            stages: shape
                .stages
                .iter()
                .enumerate()
                .map(|(k, s)| Stage { duration_s: s.duration_s, target: per_stage[k][i] })
                .collect(),
            max_in_flight: in_flight[i],
            max_calls: calls[i],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(mode: Mode, target: f64, max_in_flight: Option<u32>) -> LoadShape {
        LoadShape {
            mode,
            stages: vec![Stage { duration_s: 10, target }],
            max_in_flight,
            timeout_s: 60,
            max_calls: None,
            warmup_s: 0,
        }
    }

    #[test]
    fn the_remainder_goes_to_the_first_agents() {
        assert_eq!(split_int(10, 3), vec![4, 3, 3]);
        assert_eq!(split_int(2, 3), vec![1, 1, 0]);
    }

    #[test]
    fn concurrency_is_split_in_whole_calls_and_rates_exactly() {
        let s = shares(&shape(Mode::Concurrency, 5.0, None), 2, Some(101));
        assert_eq!((s[0].stages[0].target, s[1].stages[0].target), (3.0, 2.0));
        assert_eq!((s[0].max_calls, s[1].max_calls), (Some(51), Some(50)));
        let r = shares(&shape(Mode::Rate, 5.0, Some(9)), 2, None);
        assert_eq!(r[0].stages[0].target, 2.5);
        assert_eq!((r[0].max_in_flight, r[1].max_in_flight), (Some(5), Some(4)));
    }
}
