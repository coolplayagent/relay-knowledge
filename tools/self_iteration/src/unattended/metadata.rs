fn unattended_metadata(
    config: &Config,
    state: &UnattendedState,
    layer: &str,
    category: EvaluationCategory,
    links: MetadataLinks<'_>,
) -> serde_json::Value {
    serde_json::json!({
        "strategy": config.strategy.label(),
        "layer": layer,
        "parent_run_id": links.parent_run_id,
        "promoted_from_run_id": links.promoted_from_run_id,
        "macro_trigger": links.macro_trigger,
        "category_focus": category.label(),
        "promotion_decision": links.promotion_decision,
        "wall_clock_started_at": state.started_at.to_string(),
        "wall_clock_elapsed_seconds": state.elapsed_seconds(unix_timestamp()),
    })
}
