use serde_json::json;

use super::*;

#[test]
fn rank_changes_distinguish_improvements_from_regressions() {
    assert_eq!(optional_rank_better(Some(1), Some(3)), Some(true));
    assert_eq!(optional_rank_better(Some(4), Some(2)), Some(false));
    assert_eq!(optional_rank_better(Some(2), Some(2)), None);
    assert_eq!(optional_rank_better(None, Some(2)), Some(false));
}

#[test]
fn previous_metrics_keep_only_typed_name_value_pairs() {
    let run = json!({
        "metrics": [
            {"name": "latency", "value": 12.0},
            {"name": "missing-value"},
            {"value": 4.0},
        ],
    });

    assert_eq!(previous_metrics(&run).get("latency"), Some(&12.0));
    assert_eq!(previous_metrics(&run).len(), 1);
}
