//! What a run's figures can be taken for: the gateway's capacity, or only how it behaves.
//!
//! An agent in the gateway's stack shares its machine: its CPU is taken from the gateway, and
//! Docker's network adds time (pacod-gw docs/testing.md §9.6). Its figures show behaviour, not
//! capacity. So do the figures of an agent short of CPU, and of a mock that other load shares.
//! A run's figures count as capacity only when none of this holds; the reasons say why not.

use serde::{Deserialize, Serialize};

use crate::controller::{CellResult, CellStatus};
use crate::model::Placement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Behaviour,
    Capacity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement {
    pub class: Class,
    pub reasons: Vec<String>,
}

/// The flags that make a cell's figures describe the agent or the mock rather than the target.
const SATURATION: [&str; 3] = ["agent_cpu_high", "agent_lag", "other_traffic"];

/// `mock_shared`: the target's mock serves other load too (pacod-gw's load mock is shared with
/// the tester's load tests).
pub fn classify(cells: &[CellResult], mock_shared: bool) -> Measurement {
    let mut reasons = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for c in cells.iter().filter(|c| c.status == CellStatus::Done) {
        for a in &c.agents {
            let why = match a.placement {
                Placement::Remote => continue,
                Placement::InStack => format!("agent {} ran in the gateway's stack", a.name),
                Placement::SameHost => format!("agent {} ran on the gateway's machine", a.name),
                Placement::Unknown => format!("where agent {} ran is not declared", a.name),
            };
            if seen.insert(why.clone()) {
                reasons.push(why);
            }
        }
        for f in c.flags.iter().filter(|f| SATURATION.contains(&f.code.as_str())) {
            if seen.insert(f.code.clone()) {
                reasons.push(f.message.clone());
            }
        }
    }
    if mock_shared {
        reasons.push("the mock is shared with other load (the tester's load tests)".into());
    }
    if !cells.iter().any(|c| c.status == CellStatus::Done) {
        reasons.push("no cell ran to its end".into());
    }
    let class = if reasons.is_empty() { Class::Capacity } else { Class::Behaviour };
    Measurement { class, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::{AgentPart, Flag};
    use crate::plan::{CellKind, PlannedCell};

    fn cell(placement: Placement, flags: Vec<Flag>) -> CellResult {
        let planned: PlannedCell = serde_json::from_value(serde_json::json!({
            "index": 0, "rep": 0, "kind": "target", "axes": {}, "comparison_key": "k", "baseline_key": "b",
            "config": { "load": { "stages": [{ "duration_s": 1, "target": 1 }] } }
        }))
        .unwrap();
        assert_eq!(planned.kind, CellKind::Target);
        let mut c = CellResult::of(&planned);
        c.status = CellStatus::Done;
        c.agents = vec![AgentPart { name: "a".into(), placement, ..AgentPart::default() }];
        c.flags = flags;
        c
    }

    #[test]
    fn only_remote_agents_with_cpu_to_spare_measure_capacity() {
        assert_eq!(classify(&[cell(Placement::Remote, vec![])], false).class, Class::Capacity);
        let m = classify(&[cell(Placement::InStack, vec![])], false);
        assert_eq!(m.class, Class::Behaviour);
        assert_eq!(m.reasons, ["agent a ran in the gateway's stack"]);
        let busy = Flag::new("agent_cpu_high", "agent a used up to 97% of its 2 CPUs");
        assert_eq!(classify(&[cell(Placement::Remote, vec![busy])], false).class, Class::Behaviour);
        assert_eq!(classify(&[cell(Placement::Remote, vec![])], true).class, Class::Behaviour);
        assert_eq!(classify(&[], false).reasons, ["no cell ran to its end"]);
    }
}
