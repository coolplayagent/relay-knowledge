use std::path::PathBuf;

use super::*;

#[test]
fn macro_trigger_uses_competitive_failure_threshold() {
    let config = Config::parse(vec![
        "loop".to_owned(),
        "--strategy".to_owned(),
        "unattended-layered".to_owned(),
        "--macro-after-competitive-failures".to_owned(),
        "2".to_owned(),
    ])
    .expect("config should parse");
    let mut state = UnattendedState::new(100);
    state.competitive_promotion_failures = 2;
    let paths = history::HistoryPaths::new(&temp_workspace("macro-trigger"));

    let reason = macro_trigger(&config, &paths, &state).expect("macro trigger");

    assert_eq!(
        reason.as_deref(),
        Some("competitive promotion failures reached threshold")
    );
}

fn temp_workspace(name: &str) -> PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_owned());
    std::env::temp_dir().join(format!("relay-knowledge-{name}-{suffix}"))
}
