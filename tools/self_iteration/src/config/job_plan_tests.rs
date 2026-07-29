use super::*;

#[test]
fn auto_jobs_use_available_machine_capacity() {
    let config = Config::parse(vec!["evaluate".to_owned()]).expect("config should parse");

    assert_eq!(config.profile, "fast");
    let plan = JobPlan::from_inputs(&config, 32, None);

    assert_eq!(plan.global, 32);
    assert_eq!(plan.repositories, 16);
    assert_eq!(plan.queries, 32);
}

#[test]
fn job_env_override_only_replaces_global_limit() {
    let config = Config::parse(vec!["evaluate".to_owned()]).expect("config should parse");

    let plan = JobPlan::from_inputs(&config, 32, Some(6));

    assert_eq!(plan.global, 6);
    assert_eq!(plan.repositories, 16);
    assert_eq!(plan.queries, 32);
}
