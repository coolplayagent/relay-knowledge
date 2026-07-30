use super::*;

#[test]
fn modes_and_strategy_aliases_parse_to_stable_contracts() {
    assert_eq!(Mode::parse("research_plan"), Some(Mode::ResearchPlan));
    assert_eq!(
        Strategy::parse("layered").expect("strategy"),
        Strategy::UnattendedLayered
    );
    assert_eq!(Strategy::UnattendedLayered.label(), "unattended-layered");
    assert!(Strategy::parse("unknown").is_err());
}
