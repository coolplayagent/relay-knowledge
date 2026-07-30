use super::*;

#[test]
fn returns_the_value_after_a_flag_and_reports_missing_values() {
    let tokens = vec!["--limit".to_owned(), "10".to_owned()];

    assert_eq!(
        value_after(&tokens, 0, "--limit").expect("limit value"),
        "10"
    );
    assert_eq!(
        value_after(&tokens, 1, "--limit"),
        Err(CliError::MissingValue("--limit"))
    );
}

#[test]
fn parses_every_freshness_policy_and_rejects_unknown_values() {
    assert_eq!(
        parse_freshness("allow-stale"),
        Ok(FreshnessPolicy::AllowStale)
    );
    assert_eq!(
        parse_freshness("wait-until-fresh"),
        Ok(FreshnessPolicy::WaitUntilFresh)
    );
    assert_eq!(
        parse_freshness("graph-only"),
        Ok(FreshnessPolicy::GraphOnly)
    );
    assert_eq!(
        parse_freshness("cached"),
        Err(CliError::InvalidFreshness("cached".to_owned()))
    );
}
