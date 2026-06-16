#![cfg(feature = "test-support")]
use obelisk_bevy::scenario::{library::feature_matrix, run::run_scenario};
use std::path::PathBuf;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(format!("tests/golden/{name}.trace"))
}

#[test]
fn scenarios_match_golden_traces() {
    let update = std::env::var("UPDATE_GOLDEN").is_ok();
    let mut failures = Vec::new();
    for scenario in feature_matrix() {
        let trace = run_scenario(&scenario).to_text();
        let path = golden_path(&scenario.name);
        if update {
            std::fs::write(&path, format!("{trace}\n")).expect("write golden");
            continue;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing golden {path:?}; run with UPDATE_GOLDEN=1"));
        if expected.trim_end() != trace.trim_end() {
            failures.push(format!(
                "--- {} ---\nEXPECTED:\n{}\nGOT:\n{}\n",
                scenario.name,
                expected.trim_end(),
                trace
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "golden mismatch (run UPDATE_GOLDEN=1 to regenerate if intended):\n{}",
        failures.join("\n")
    );
}
