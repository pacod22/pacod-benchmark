//! pacod-gw's load contract fixtures (vendor/pacod-gw/crates/pacod-gw-load/tests/contract),
//! which the agent's and the tester's tests read too: every client of the contract tests against
//! them (pacod-gw's CLAUDE.md). Each reads into its type and writes back the same JSON, and the
//! histogram vectors hold. A partial or broken vendored copy fails here first.

use std::path::PathBuf;

use pacod_gw_load::contract::{CONTRACT, ErrorBody, Info, Prepare, Prepared, Report};
use pacod_gw_load::histogram::{Histogram, bounds, index};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/pacod-gw/crates/pacod-gw-load/tests/contract")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

fn round_trip<T: Serialize + DeserializeOwned>(name: &str) -> T {
    let original = fixture(name);
    let typed: T = serde_json::from_value(original.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(serde_json::to_value(&typed).unwrap(), original, "{name} does not write back the same");
    typed
}

#[test]
fn every_fixture_reads_and_writes_back_unchanged() {
    let info: Info = round_trip("info.json");
    assert_eq!(info.contract, CONTRACT, "the vendored agent speaks the contract this benchmark was built for");
    let p: Prepare = round_trip("prepare.json");
    assert!(!format!("{p:?}").contains(&p.key), "the key stays out of Debug");
    round_trip::<Prepare>("prepare-external.json");
    round_trip::<Prepared>("prepared.json");
    round_trip::<Report>("report.json");
    round_trip::<ErrorBody>("error.json");
}

#[test]
fn the_histogram_vectors_hold() {
    let v = fixture("histogram.json");
    for pair in v["index"].as_array().unwrap() {
        assert_eq!(u64::from(index(pair[0].as_u64().unwrap())), pair[1].as_u64().unwrap(), "{pair}");
    }
    for row in v["bounds"].as_array().unwrap() {
        let b = bounds(row[0].as_u64().unwrap() as u32);
        assert_eq!(b, (row[1].as_u64().unwrap(), row[2].as_u64().unwrap()), "{row}");
    }
    let h: Histogram = serde_json::from_value(v["histogram"].clone()).unwrap();
    for pair in v["percentiles"].as_array().unwrap() {
        assert_eq!(h.percentile(pair[0].as_f64().unwrap()), pair[1].as_f64().unwrap(), "{pair}");
    }
}
