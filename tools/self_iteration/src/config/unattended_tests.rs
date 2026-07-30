use super::*;

#[test]
fn parses_unattended_layered_defaults() {
    let config = Config::parse(vec![
        "loop".to_owned(),
        "--strategy".to_owned(),
        "unattended-layered".to_owned(),
    ])
    .expect("config should parse");

    assert_eq!(config.strategy, Strategy::UnattendedLayered);
    assert_eq!(config.max_wall_clock_hours, 36);
    assert_eq!(config.explore_timeout_seconds, 900);
    assert_eq!(config.macro_explore_timeout_seconds, 2700);
    assert_eq!(config.max_explore_attempts_per_cycle, 3);
    assert_eq!(config.macro_after_competitive_failures, 4);
    assert_eq!(config.macro_after_empty_candidates, 6);
    assert_eq!(config.cycle_sleep_seconds, 120);
}

#[test]
fn parses_unattended_layered_overrides() {
    let config = Config::parse(vec![
        "loop".to_owned(),
        "--strategy=layered".to_owned(),
        "--max-wall-clock-hours=48".to_owned(),
        "--explore-timeout-seconds=600".to_owned(),
        "--macro-explore-timeout-seconds=1800".to_owned(),
        "--max-explore-attempts-per-cycle".to_owned(),
        "2".to_owned(),
        "--macro-after-competitive-failures".to_owned(),
        "3".to_owned(),
        "--cycle-sleep-seconds".to_owned(),
        "30".to_owned(),
    ])
    .expect("config should parse");

    assert_eq!(config.strategy, Strategy::UnattendedLayered);
    assert_eq!(config.max_wall_clock_hours, 48);
    assert_eq!(config.explore_timeout_seconds, 600);
    assert_eq!(config.macro_explore_timeout_seconds, 1800);
    assert_eq!(config.max_explore_attempts_per_cycle, 2);
    assert_eq!(config.macro_after_competitive_failures, 3);
    assert_eq!(config.cycle_sleep_seconds, 30);
}
